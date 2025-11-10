// Tron Game Mode - 2 AI players with gradient trails
use anyhow::Result;
use colorgrad::Gradient;
use ddp_rs::connection::DDPConnection;
use rand::Rng;
use std::collections::{VecDeque, BinaryHeap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::BandwidthConfig;
use crate::multi_device::{MultiDeviceConfig, MultiDeviceManager, WLEDDevice};
use crate::types::{build_gradient_from_color, InterpolationMode};
use crate::gradients;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

impl Direction {
    fn turn_left(&self) -> Direction {
        match self {
            Direction::Up => Direction::UpLeft,
            Direction::UpLeft => Direction::Left,
            Direction::Left => Direction::DownLeft,
            Direction::DownLeft => Direction::Down,
            Direction::Down => Direction::DownRight,
            Direction::DownRight => Direction::Right,
            Direction::Right => Direction::UpRight,
            Direction::UpRight => Direction::Up,
        }
    }

    fn turn_right(&self) -> Direction {
        match self {
            Direction::Up => Direction::UpRight,
            Direction::UpRight => Direction::Right,
            Direction::Right => Direction::DownRight,
            Direction::DownRight => Direction::Down,
            Direction::Down => Direction::DownLeft,
            Direction::DownLeft => Direction::Left,
            Direction::Left => Direction::UpLeft,
            Direction::UpLeft => Direction::Up,
        }
    }

    // Turn 90 degrees left (cardinal directions only)
    fn turn_left_90(&self) -> Direction {
        match self {
            Direction::Up => Direction::Left,
            Direction::Left => Direction::Down,
            Direction::Down => Direction::Right,
            Direction::Right => Direction::Up,
            // Diagonal directions turn to their left cardinal
            Direction::UpLeft => Direction::Left,
            Direction::DownLeft => Direction::Down,
            Direction::DownRight => Direction::Right,
            Direction::UpRight => Direction::Up,
        }
    }

    // Turn 90 degrees right (cardinal directions only)
    fn turn_right_90(&self) -> Direction {
        match self {
            Direction::Up => Direction::Right,
            Direction::Right => Direction::Down,
            Direction::Down => Direction::Left,
            Direction::Left => Direction::Up,
            // Diagonal directions turn to their right cardinal
            Direction::UpRight => Direction::Right,
            Direction::DownRight => Direction::Down,
            Direction::DownLeft => Direction::Left,
            Direction::UpLeft => Direction::Up,
        }
    }

    /// Calculate next position from current position and direction
    fn next_position(&self, pos: Position) -> Position {
        match self {
            Direction::Up => Position { x: pos.x, y: pos.y - 1 },
            Direction::Down => Position { x: pos.x, y: pos.y + 1 },
            Direction::Left => Position { x: pos.x - 1, y: pos.y },
            Direction::Right => Position { x: pos.x + 1, y: pos.y },
            Direction::UpLeft => Position { x: pos.x - 1, y: pos.y - 1 },
            Direction::UpRight => Position { x: pos.x + 1, y: pos.y - 1 },
            Direction::DownLeft => Position { x: pos.x - 1, y: pos.y + 1 },
            Direction::DownRight => Position { x: pos.x + 1, y: pos.y + 1 },
        }
    }

    /// Check if this direction is diagonal
    fn is_diagonal(&self) -> bool {
        matches!(self, Direction::UpLeft | Direction::UpRight | Direction::DownLeft | Direction::DownRight)
    }

    /// Convert diagonal direction to cardinal direction (prefer vertical component)
    fn to_cardinal(&self) -> Direction {
        match self {
            Direction::UpLeft => Direction::Up,
            Direction::UpRight => Direction::Up,
            Direction::DownLeft => Direction::Down,
            Direction::DownRight => Direction::Down,
            // Already cardinal - return as-is
            _ => *self,
        }
    }

    /// Get intermediate positions that must be checked for diagonal moves
    /// Returns the two positions being "crossed through" when moving diagonally
    fn get_intermediate_positions(&self, from: Position) -> Vec<Position> {
        match self {
            Direction::UpLeft => vec![
                Position { x: from.x - 1, y: from.y },  // Left
                Position { x: from.x, y: from.y - 1 },  // Up
            ],
            Direction::UpRight => vec![
                Position { x: from.x + 1, y: from.y },  // Right
                Position { x: from.x, y: from.y - 1 },  // Up
            ],
            Direction::DownLeft => vec![
                Position { x: from.x - 1, y: from.y },  // Left
                Position { x: from.x, y: from.y + 1 },  // Down
            ],
            Direction::DownRight => vec![
                Position { x: from.x + 1, y: from.y },  // Right
                Position { x: from.x, y: from.y + 1 },  // Down
            ],
            _ => vec![],  // Non-diagonal moves have no intermediates
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Position {
    x: i32,
    y: i32,
}

// A* node for pathfinding
#[derive(Clone, Eq, PartialEq)]
struct AStarNode {
    pos: Position,
    g_cost: i32,  // Cost from start
    h_cost: i32,  // Heuristic to goal
    parent: Option<Position>,
}

impl AStarNode {
    fn f_cost(&self) -> i32 {
        self.g_cost + self.h_cost
    }
}

impl Ord for AStarNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering for min-heap behavior
        other.f_cost().cmp(&self.f_cost())
    }
}

impl PartialOrd for AStarNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// Manhattan distance heuristic
fn manhattan_distance(a: Position, b: Position) -> i32 {
    (a.x - b.x).abs() + (a.y - b.y).abs()
}

// Food types in food mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FoodType {
    Normal,  // White, +1 trail length
    Super,   // Red, +5 trail length (10% spawn chance)
    Power,   // Yellow, activates 10 second power mode (1% spawn chance)
}

// Food piece traveling down a player's trail
#[derive(Debug, Clone)]
struct TravelingFood {
    trail_position: usize,  // Index in trail (starts at trail.len()-1 [head], decrements toward 0 [tail])
    color: (u8, u8, u8),    // RGB color of the food piece
}

struct Player {
    id: u8,
    pos: Position,
    direction: Direction,
    trail: VecDeque<Position>,
    alive: bool,
    gradient: Gradient,
    death_time: Option<Instant>,  // When player died, for blink animation
    aggression_modifier: f64,      // Current aggression multiplier (0.9 to 1.1)
    aggression_phase: f64,         // Phase for oscillation (0 to 2π)
    aggression_frequency: f64,     // How fast this player's aggression oscillates
    max_trail_length: usize,       // Max trail length for this player (grows in food mode)

    // Power mode (yellow power food)
    power_active: bool,            // Whether player is in power mode
    power_end_time: Option<Instant>,  // When power mode ends (10 seconds from activation)
    yellow_flash_until: Option<Instant>,  // Flash yellow until this time (when crossing trail or killing)

    // Food targeting
    current_target_food: Option<Position>,  // Which food we're currently pursuing
    frames_since_retarget: u32,             // Frames since last food target re-evaluation

    // Gradient animation
    animation_offset: f64,  // Animation offset for gradient cycling (0.0 to 1.0)
    animation_direction_flipped: bool,  // Whether this player's animation direction is flipped

    // Per-player move timing (for speed differentiation)
    last_move_time: Instant,  // When this player last moved
    food_move_counter: u32,  // Frame counter for food movement (moves every N frames)

    // Traveling food pieces
    traveling_food: Vec<TravelingFood>,  // Food pieces moving down the trail
}

impl Player {
    fn new(id: u8, start_x: i32, start_y: i32, direction: Direction, gradient: Gradient, initial_trail_length: usize) -> Self {
        let pos = Position { x: start_x, y: start_y };
        let mut trail = VecDeque::new();
        trail.push_back(pos);

        // Each player gets unique aggression variance characteristics
        let mut rng = rand::thread_rng();
        let aggression_phase = rng.gen_range(0.0..std::f64::consts::TAU); // Random starting phase
        let aggression_frequency = rng.gen_range(0.005..0.015); // Different oscillation speeds

        Player {
            id,
            pos,
            direction,
            trail,
            alive: true,
            gradient,
            death_time: None,
            aggression_modifier: 1.0, // Will be calculated from phase
            aggression_phase,
            aggression_frequency,
            max_trail_length: initial_trail_length,
            power_active: false,  // Start without power mode
            power_end_time: None,  // No power mode active
            yellow_flash_until: None,  // No yellow flash active
            current_target_food: None,
            frames_since_retarget: 0,
            animation_offset: 0.0, // Start at 0
            animation_direction_flipped: false, // Start with normal direction
            last_move_time: Instant::now(),  // Initialize per-player move timing
            food_move_counter: 0,  // Initialize frame counter for food movement
            traveling_food: Vec::new(),  // No food traveling initially
        }
    }

    fn limit_trail(&mut self, max_length: usize, grid: &mut Vec<Vec<Option<u8>>>) {
        if max_length > 0 && self.trail.len() > max_length {
            if let Some(old_pos) = self.trail.pop_front() {
                // Clear this position from the grid if it still belongs to this player
                let x = old_pos.x as usize;
                let y = old_pos.y as usize;
                if grid[y][x] == Some(self.id) {
                    grid[y][x] = None;
                }
            }
        }
    }

}

pub struct TronGame {
    width: usize,
    height: usize,
    players: Vec<Player>,
    grid: Vec<Vec<Option<u8>>>, // None = empty, Some(player_id) = occupied - tracks ALL visited positions
    game_over: bool,
    last_update: Instant,
    update_interval: Duration,
    look_ahead: i32,
    trail_length: usize,  // Max trail length for rendering, 0 = infinite
    ai_aggression: f64,
    food_mode: bool,  // Food mode - players compete to eat food and grow
    food_positions: Vec<(Position, Instant, FoodType)>,  // Current food positions with spawn timestamps and food type (can have multiple foods)
    food_max_count: usize,  // Maximum number of foods that can exist simultaneously
    food_ttl_seconds: u64,  // Food time-to-live in seconds before relocating
    trail_fade: bool,  // Enable trail brightness fading effect
    super_food_enabled: bool,  // Enable super food spawning (red, 10% chance, +5 length)
    diagonal_movement: bool,  // Enable diagonal movement (8 directions instead of 4)
}

impl TronGame {
    pub fn new(width: usize, height: usize, speed_ms: f64, look_ahead: i32, trail_length: usize, ai_aggression: f64, num_players: usize, player_colors: &[String], food_mode: bool, food_max_count: usize, food_ttl_seconds: u64, trail_fade: bool, super_food_enabled: bool, diagonal_movement: bool, interpolation: &str) -> Self {
        // Create players distributed around the perimeter
        let mut players = Vec::new();
        let mut rng = rand::thread_rng();

        // Parse interpolation mode
        let interp_mode = match interpolation {
            "basis" => InterpolationMode::Basis,
            "catmullrom" => InterpolationMode::CatmullRom,
            _ => InterpolationMode::Linear,
        };

        // Track already used positions
        let mut used_positions: Vec<Position> = Vec::new();

        for i in 0..num_players {
            let player_id = (i + 1) as u8;

            // Randomize starting position - find empty spot not too close to others
            let (start_x, start_y) = {
                let mut attempts = 0;
                const MAX_ATTEMPTS: usize = 1000;
                const MIN_DISTANCE: i32 = 5; // Minimum distance between players

                loop {
                    // Random position avoiding exact edges (leave 1 cell margin)
                    let x = rng.gen_range(1..(width as i32 - 1));
                    let y = rng.gen_range(1..(height as i32 - 1));

                    // Check if too close to any existing player
                    let too_close = used_positions.iter().any(|pos| {
                        let dx = (pos.x - x).abs();
                        let dy = (pos.y - y).abs();
                        dx + dy < MIN_DISTANCE
                    });

                    if !too_close {
                        used_positions.push(Position { x, y });
                        break (x, y);
                    }

                    attempts += 1;
                    if attempts >= MAX_ATTEMPTS {
                        // Fallback to any random position if can't find good spot
                        let fallback_x = rng.gen_range(1..(width as i32 - 1));
                        let fallback_y = rng.gen_range(1..(height as i32 - 1));
                        used_positions.push(Position { x: fallback_x, y: fallback_y });
                        break (fallback_x, fallback_y);
                    }
                }
            };

            // Completely random starting direction
            let direction = match rng.gen_range(0..4) {
                0 => Direction::Up,
                1 => Direction::Down,
                2 => Direction::Left,
                _ => Direction::Right,
            };

            // Get gradient for this player
            let color_name = player_colors.get(i).map(|s| s.as_str()).unwrap_or("Rainbow");

            // Resolve gradient name to hex colors
            let hex_colors = gradients::resolve_color_string(color_name);

            // If it's a single color, duplicate it to make a solid "gradient"
            let hex_for_gradient = if !hex_colors.contains(',') {
                format!("{},{}", hex_colors, hex_colors)
            } else {
                hex_colors.clone()
            };

            let (gradient_opt, _, _) = build_gradient_from_color(&hex_for_gradient, true, interp_mode).unwrap_or_else(|_e| {
                // Fallback to rainbow if parsing fails
                let fallback_hex = gradients::resolve_color_string("Rainbow");
                build_gradient_from_color(&fallback_hex, true, interp_mode).unwrap()
            });
            let gradient = gradient_opt.unwrap_or_else(|| {
                // Fallback gradient if None (should not happen now)
                colorgrad::CustomGradient::new()
                    .html_colors(&["#ff0000", "#00ff00", "#0000ff"])
                    .build()
                    .unwrap()
            });

            // In food mode, players start with trail length 1, otherwise use global trail_length (0 = infinite)
            let initial_trail_length = if food_mode { 1 } else { trail_length };
            players.push(Player::new(player_id, start_x, start_y, direction, gradient, initial_trail_length));
        }

        // Initialize grid and mark starting positions
        let mut grid = vec![vec![None; width]; height];
        for player in &players {
            let x = player.pos.x as usize;
            let y = player.pos.y as usize;
            if x < width && y < height {
                grid[y][x] = Some(player.id);
            }
        }

        TronGame {
            width,
            height,
            players,
            grid,
            game_over: false,
            last_update: Instant::now(),
            update_interval: Duration::from_secs_f64(speed_ms / 1000.0),
            look_ahead,
            trail_length,
            ai_aggression,
            food_mode,
            food_positions: Vec::new(),  // Will spawn on first update
            food_max_count,
            food_ttl_seconds,
            trail_fade,
            super_food_enabled,
            diagonal_movement,
        }
    }

    /// Spawn food at a random empty position (avoiding visible trails and existing foods)
    fn spawn_food(&mut self) {
        // Check if we've already reached max food count
        if self.food_positions.len() >= self.food_max_count {
            return;
        }

        let mut rng = rand::thread_rng();
        let mut attempts = 0;
        const MAX_ATTEMPTS: usize = 1000;

        // Try to find empty position (avoid boundary edges to prevent players from dying when eating food)
        while attempts < MAX_ATTEMPTS {
            let x = rng.gen_range(1..(self.width - 1)) as i32;
            let y = rng.gen_range(1..(self.height - 1)) as i32;
            let pos = Position { x, y };

            // Check if position is empty (not on any visible trail or existing food)
            let occupied_by_trail = self.players.iter().any(|p| {
                p.trail.iter().any(|trail_pos| trail_pos.x == x && trail_pos.y == y)
            });

            let occupied_by_food = self.food_positions.iter().any(|(food_pos, _, _)| {
                food_pos.x == x && food_pos.y == y
            });

            if !occupied_by_trail && !occupied_by_food {
                // Load config to check if power food is enabled
                let power_food_enabled = BandwidthConfig::load()
                    .map(|cfg| cfg.tron_power_food_enabled)
                    .unwrap_or(false);

                // Determine food type with priority: Power (1%) > Super (10%) > Normal
                let food_type = if power_food_enabled && rng.gen_bool(0.01) {
                    FoodType::Power  // 1% chance for power food
                } else if self.super_food_enabled && rng.gen_bool(0.1) {
                    FoodType::Super  // 10% chance for super food (if power food didn't spawn)
                } else {
                    FoodType::Normal  // Normal food by default
                };

                self.food_positions.push((pos, Instant::now(), food_type));
                return;
            }

            attempts += 1;
        }

        // If we couldn't find an empty spot after MAX_ATTEMPTS, just don't spawn (keep existing foods)
    }

    /// Respawn a dead player at a random empty position in food mode
    fn respawn_player(&mut self, player_idx: usize) {
        let mut rng = rand::thread_rng();
        let mut attempts = 0;
        const MAX_ATTEMPTS: usize = 100;

        // Try to find empty position
        while attempts < MAX_ATTEMPTS {
            let x = rng.gen_range(0..self.width) as i32;
            let y = rng.gen_range(0..self.height) as i32;
            let pos = Position { x, y };

            // Check if position is empty (not on any visible trail or food)
            let occupied_by_trail = self.players.iter().any(|p| {
                p.trail.iter().any(|trail_pos| trail_pos.x == x && trail_pos.y == y)
            });
            let occupied_by_food = self.food_positions.iter().any(|(food_pos, _, _)| {
                food_pos.x == x && food_pos.y == y
            });
            let occupied = occupied_by_trail || occupied_by_food;

            if !occupied {
                // Respawn player here
                let player = &mut self.players[player_idx];
                player.pos = pos;
                player.trail.clear();
                player.trail.push_back(pos);
                player.alive = true;
                player.death_time = None;
                player.max_trail_length = 1; // Reset to starting length
                player.power_active = false;  // Reset power mode
                player.power_end_time = None;  // Clear power mode timer
                player.yellow_flash_until = None;  // Clear yellow flash

                // Random starting direction
                player.direction = match rng.gen_range(0..4) {
                    0 => Direction::Up,
                    1 => Direction::Down,
                    2 => Direction::Left,
                    _ => Direction::Right,
                };

                return;
            }

            attempts += 1;
        }

        // If we couldn't find a spot, just respawn at center
        let player = &mut self.players[player_idx];
        player.pos = Position { x: (self.width / 2) as i32, y: (self.height / 2) as i32 };
        player.trail.clear();
        player.trail.push_back(player.pos);
        player.alive = true;
        player.death_time = None;
        player.max_trail_length = 1;
        player.direction = Direction::Right;
    }

    pub fn reset(&mut self, num_players: usize, player_colors: &[String]) {
        // Load interpolation from config
        let interpolation = BandwidthConfig::load()
            .map(|cfg| cfg.tron_interpolation)
            .unwrap_or_else(|_| "catmullrom".to_string());

        *self = TronGame::new(
            self.width,
            self.height,
            self.update_interval.as_secs_f64() * 1000.0,
            self.look_ahead,
            self.trail_length,
            self.ai_aggression,
            num_players,
            player_colors,
            self.food_mode,
            self.food_max_count,
            self.food_ttl_seconds,
            self.trail_fade,
            self.super_food_enabled,
            self.diagonal_movement,
            &interpolation,
        );
    }

    // Check if a position is occupied (considering game mode)
    fn is_occupied(&self, pos: Position, _player_id: u8) -> bool {
        if pos.x < 0 || pos.x >= self.width as i32 || pos.y < 0 || pos.y >= self.height as i32 {
            return true; // Out of bounds = occupied
        }

        if self.food_mode {
            // Food mode: only check visible trails
            self.players.iter().any(|p| {
                p.alive && p.trail.iter().any(|trail_pos| trail_pos.x == pos.x && trail_pos.y == pos.y)
            })
        } else {
            // Tron mode: check persistent grid - any trail blocks
            self.grid[pos.y as usize][pos.x as usize].is_some()
        }
    }

    // Check if position is blocked for a powered player (walls and own trail only, can cross other trails)
    fn is_occupied_powered(&self, pos: Position, player_id: u8) -> bool {
        if pos.x < 0 || pos.x >= self.width as i32 || pos.y < 0 || pos.y >= self.height as i32 {
            return true; // Out of bounds = occupied
        }

        if self.food_mode {
            // Food mode: powered players can cross other players' trails, but not their own
            self.players.iter().any(|p| {
                p.alive && p.id == player_id && p.trail.iter().any(|trail_pos| trail_pos.x == pos.x && trail_pos.y == pos.y)
            })
        } else {
            // Tron mode: check persistent grid for own trail only
            if let Some(occupying_id) = self.grid[pos.y as usize][pos.x as usize] {
                occupying_id == player_id
            } else {
                false
            }
        }
    }

    // A* pathfinding: find shortest path from start to goal
    fn find_path_astar(&self, start: Position, goal: Position, player_id: u8) -> Option<Vec<Position>> {
        let mut open_set = BinaryHeap::new();
        let mut closed_set = HashSet::new();
        let mut came_from: std::collections::HashMap<(i32, i32), Position> = std::collections::HashMap::new();

        let start_node = AStarNode {
            pos: start,
            g_cost: 0,
            h_cost: manhattan_distance(start, goal),
            parent: None,
        };

        open_set.push(start_node);

        while let Some(current) = open_set.pop() {
            let current_key = (current.pos.x, current.pos.y);

            if current.pos.x == goal.x && current.pos.y == goal.y {
                // Reconstruct path
                let mut path = vec![current.pos];
                let mut pos = current.pos;
                while let Some(parent) = came_from.get(&(pos.x, pos.y)) {
                    path.push(*parent);
                    pos = *parent;
                }
                path.reverse();
                return Some(path);
            }

            if closed_set.contains(&current_key) {
                continue;
            }
            closed_set.insert(current_key);

            // Check all 4 directions
            let neighbors = vec![
                Position { x: current.pos.x, y: current.pos.y - 1 }, // Up
                Position { x: current.pos.x, y: current.pos.y + 1 }, // Down
                Position { x: current.pos.x - 1, y: current.pos.y }, // Left
                Position { x: current.pos.x + 1, y: current.pos.y }, // Right
            ];

            for neighbor_pos in neighbors {
                let neighbor_key = (neighbor_pos.x, neighbor_pos.y);

                if closed_set.contains(&neighbor_key) {
                    continue;
                }

                // Skip if occupied (but allow the goal position even if it's food)
                if self.is_occupied(neighbor_pos, player_id) {
                    // Check if this is the goal (food position)
                    if !(neighbor_pos.x == goal.x && neighbor_pos.y == goal.y) {
                        continue;
                    }
                }

                let g_cost = current.g_cost + 1;
                let h_cost = manhattan_distance(neighbor_pos, goal);

                let neighbor_node = AStarNode {
                    pos: neighbor_pos,
                    g_cost,
                    h_cost,
                    parent: Some(current.pos),
                };

                came_from.insert(neighbor_key, current.pos);
                open_set.push(neighbor_node);
            }
        }

        None // No path found
    }

    // Get the direction to move from current position toward target position
    fn direction_to_target(&self, from: Position, to: Position) -> Option<Direction> {
        let dx = to.x - from.x;
        let dy = to.y - from.y;

        // Choose direction based on which axis has greater difference
        if dx.abs() > dy.abs() {
            if dx > 0 {
                Some(Direction::Right)
            } else {
                Some(Direction::Left)
            }
        } else if dy.abs() > 0 {
            if dy > 0 {
                Some(Direction::Down)
            } else {
                Some(Direction::Up)
            }
        } else {
            None // Already at target
        }
    }

    // Check if a position has sufficient escape routes (prevents dead-end traps)
    // Returns true if the position is safe to go to (has multiple exits or enough space)
    fn has_sufficient_escape_routes(&self, pos: Position, player_id: u8, min_safe_directions: usize, is_powered: bool) -> bool {
        // Check all 4 directions from this position
        let directions = vec![Direction::Up, Direction::Down, Direction::Left, Direction::Right];

        let mut safe_directions = 0;
        let mut total_safe_steps = 0;

        for dir in directions {
            let (safe_steps, _) = self.count_safe_steps_detailed(player_id, pos, dir, 10, is_powered);
            if safe_steps > 0 {
                safe_directions += 1;
                total_safe_steps += safe_steps;
            }
        }

        // Position is safe if:
        // 1. It has at least min_safe_directions open (typically 2 = not a dead end)
        // 2. OR it has significant total reachable space (>= 15 steps total across all directions)
        safe_directions >= min_safe_directions || total_safe_steps >= 15
    }

    // Calculate edge penalty - positions near edges are less desirable
    // Returns a penalty score (higher = worse, 0 = center, 3 = corner)
    fn calculate_edge_penalty(&self, pos: Position) -> i32 {
        let mut penalty = 0;

        // Check each edge
        if pos.x == 0 || pos.x == (self.width - 1) as i32 {
            penalty += 1;
        }
        if pos.y == 0 || pos.y == (self.height - 1) as i32 {
            penalty += 1;
        }

        // Additional penalty for being very close to edges (within 1 cell)
        if pos.x == 1 || pos.x == (self.width - 2) as i32 {
            penalty += 1;
        }
        if pos.y == 1 || pos.y == (self.height - 2) as i32 {
            penalty += 1;
        }

        penalty
    }

    // Calculate spacing score - prefer positions with more free space around them
    // Returns count of free adjacent cells (higher = better, 0-4)
    fn calculate_spacing_score(&self, pos: Position, player_id: u8) -> i32 {
        let directions = vec![
            Position { x: pos.x, y: pos.y - 1 }, // Up
            Position { x: pos.x, y: pos.y + 1 }, // Down
            Position { x: pos.x - 1, y: pos.y }, // Left
            Position { x: pos.x + 1, y: pos.y }, // Right
        ];

        let mut free_count = 0;

        for check_pos in directions {
            if !self.is_occupied(check_pos, player_id) {
                free_count += 1;
            }
        }

        free_count
    }

    // Calculate reachable cells using flood fill (for territory analysis)
    // Returns count of cells player can reach from their position
    // Limited to max_depth to avoid excessive computation
    fn calculate_reachable_cells(&self, start_pos: Position, player_id: u8, max_depth: i32) -> i32 {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back((start_pos, 0));
        visited.insert((start_pos.x, start_pos.y));

        let mut reachable_count = 0;

        while let Some((pos, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            reachable_count += 1;

            // Check all 4 directions
            let neighbors = vec![
                Position { x: pos.x, y: pos.y - 1 }, // Up
                Position { x: pos.x, y: pos.y + 1 }, // Down
                Position { x: pos.x - 1, y: pos.y }, // Left
                Position { x: pos.x + 1, y: pos.y }, // Right
            ];

            for next_pos in neighbors {
                let key = (next_pos.x, next_pos.y);

                if visited.contains(&key) {
                    continue;
                }

                if !self.is_occupied(next_pos, player_id) {
                    visited.insert(key);
                    queue.push_back((next_pos, depth + 1));
                }
            }
        }

        reachable_count
    }

    // Calculate territory control score - how much does this move reduce opponent's space?
    // Returns score (higher = better for cutting off opponent)
    fn calculate_territory_control_score(&self, my_pos: Position, my_dir: Direction, my_id: u8, opp_idx: usize) -> f64 {
        let opponent = &self.players[opp_idx];
        if !opponent.alive {
            return 0.0;
        }

        // Calculate where we'd be after this move
        let my_next_pos = my_dir.next_position(my_pos);

        // Check if next position is blocked
        if self.is_occupied(my_next_pos, my_id) {
            return 0.0;
        }

        let opp_pos = opponent.pos;
        let opp_id = opponent.id;

        // Calculate opponent's reachable territory BEFORE our move
        let opp_territory_before = self.calculate_reachable_cells(opp_pos, opp_id, 15);

        // Simulate our move: temporarily mark next position as occupied
        // NOTE: We can't actually modify grid here, so we estimate the impact
        // by checking if our next position is in opponent's reachable area

        // Distance from our next position to opponent
        let dist_to_opp = manhattan_distance(my_next_pos, opp_pos);

        // If we're close to opponent and between them and open space, high control score
        // Increased range from 5 to 10 to make players more aggressive from further away
        let control_score = if dist_to_opp <= 10 {
            // Close to opponent - check if we're cutting them off
            // Higher score if we're between opponent and center of grid
            let grid_center = Position {
                x: (self.width / 2) as i32,
                y: (self.height / 2) as i32,
            };

            let opp_to_center = manhattan_distance(opp_pos, grid_center);
            let us_to_center = manhattan_distance(my_next_pos, grid_center);

            // If we're closer to center than opponent, we're potentially cutting them off
            if us_to_center < opp_to_center {
                let cutoff_score = (opp_to_center - us_to_center) as f64 * 5.0;
                // Bonus for opponent having less space (more restricted = better for us)
                let space_restriction_bonus = if opp_territory_before < 30 {
                    (30 - opp_territory_before) as f64 * 0.5
                } else {
                    0.0
                };
                cutoff_score + space_restriction_bonus
            } else if dist_to_opp <= 5 {
                // Even if not cutting to center, being close to opponent is good
                5.0 - dist_to_opp as f64
            } else {
                0.0
            }
        } else {
            0.0
        };

        control_score.max(0.0)
    }

    // Calculate how many escape routes a position has (for caging opponents)
    fn calculate_escape_route_count(&self, pos: Position, player_id: u8) -> i32 {
        let directions = vec![Direction::Up, Direction::Down, Direction::Left, Direction::Right];
        let mut escape_count = 0;

        for dir in directions {
            let next_pos = dir.next_position(pos);
            if !self.is_occupied(next_pos, player_id) {
                // This direction is open - check if it has reasonable space
                // Evaluating opponent escape routes - opponent is not powered
                let (safe_steps, _) = self.count_safe_steps_detailed(player_id, pos, dir, 5, false);
                if safe_steps >= 2 {
                    escape_count += 1;
                }
            }
        }

        escape_count
    }

    // Calculate escape routes if a specific position is blocked (simulating our movement)
    fn calculate_escape_route_count_if_blocked(&self, pos: Position, player_id: u8, blocked_pos: Position) -> i32 {
        let directions = vec![Direction::Up, Direction::Down, Direction::Left, Direction::Right];
        let mut escape_count = 0;

        for dir in directions {
            let next_pos = dir.next_position(pos);

            // If this direction leads to the blocked position, skip it
            if next_pos.x == blocked_pos.x && next_pos.y == blocked_pos.y {
                continue;
            }

            if !self.is_occupied(next_pos, player_id) {
                // This direction is open - check if it has reasonable space
                // Evaluating opponent escape routes - opponent is not powered
                let (safe_steps, _) = self.count_safe_steps_detailed(player_id, pos, dir, 5, false);
                if safe_steps >= 2 {
                    escape_count += 1;
                }
            }
        }

        escape_count
    }

    // Calculate minimum distance to nearest edge
    fn distance_to_nearest_edge(&self, pos: Position) -> i32 {
        let dist_left = pos.x;
        let dist_right = (self.width as i32 - 1) - pos.x;
        let dist_top = pos.y;
        let dist_bottom = (self.height as i32 - 1) - pos.y;

        dist_left.min(dist_right).min(dist_top).min(dist_bottom)
    }

    // Find the direction that gives the most escape space
    fn find_best_escape_direction(&self, pos: Position, player_id: u8) -> Option<Direction> {
        let directions = vec![Direction::Up, Direction::Down, Direction::Left, Direction::Right];
        let mut best_dir: Option<Direction> = None;
        let mut best_steps = 0;

        for dir in directions {
            let next_pos = dir.next_position(pos);
            if !self.is_occupied(next_pos, player_id) {
                // Evaluating opponent escape routes - opponent is not powered
                let (safe_steps, _) = self.count_safe_steps_detailed(player_id, pos, dir, 10, false);
                if safe_steps > best_steps {
                    best_steps = safe_steps;
                    best_dir = Some(dir);
                }
            }
        }

        best_dir
    }

    // Calculate cutoff score - how well does this move block opponent's escape routes?
    fn ai_decide(&mut self, player_idx: usize) {
        let player = &self.players[player_idx];
        if !player.alive {
            return;
        }

        let current_dir = player.direction;
        let player_id = player.id;
        let player_pos = player.pos;

        // FOOD MODE: Use A* pathfinding with periodic re-targeting
        if self.food_mode {
            // POWER MODE: When powered, use strategic caging to trap opponents
            if self.players[player_idx].power_active {
                // Find opponent with longest trail (most valuable target)
                if let Some(opp_idx) = self.find_longest_opponent(player_idx) {
                    let opponent = &self.players[opp_idx];
                    let opp_pos = opponent.pos;
                    let opp_id = opponent.id;

                    // Calculate opponent's current escape routes
                    let opp_current_escapes = self.calculate_escape_route_count(opp_pos, opp_id);

                    // Evaluate each possible direction for caging effectiveness
                    struct CagingScore {
                        dir: Direction,
                        is_blocked: bool,
                        has_escape: bool,
                        reduced_escapes: i32,  // How many escape routes we'd cut off
                        distance_to_opp: i32,  // Closer is better for pressure
                        pushes_to_edge: bool,  // Pushing toward wall/corner
                        cuts_best_route: bool, // Blocks their best escape direction
                    }

                    // Powered players always move diagonally (ignore global diagonal_movement setting)
                    let directions = vec![
                        current_dir,
                        current_dir.turn_left(),
                        current_dir.turn_right(),
                    ];

                    let mut caging_scores: Vec<CagingScore> = Vec::new();

                    for dir in directions {
                        let next_pos = dir.next_position(player_pos);
                        // Powered players can cross other trails, only blocked by walls and own trail
                        let is_blocked = self.is_occupied_powered(next_pos, player_id);
                        // Powered players have different movement rules - can cross trails
                        let has_escape = self.has_sufficient_escape_routes(next_pos, player_id, 1, true);

                        if is_blocked {
                            caging_scores.push(CagingScore {
                                dir,
                                is_blocked: true,
                                has_escape: false,
                                reduced_escapes: 0,
                                distance_to_opp: 9999,
                                pushes_to_edge: false,
                                cuts_best_route: false,
                            });
                            continue;
                        }

                        // Simulate: how many escapes would opponent have if we move here?
                        let distance_to_opp = manhattan_distance(next_pos, opp_pos);
                        let opp_escapes_after = self.calculate_escape_route_count_if_blocked(opp_pos, opp_id, next_pos);
                        let reduced_escapes = opp_current_escapes - opp_escapes_after;

                        // Check if we're pushing opponent toward edges
                        let opp_edge_dist = self.distance_to_nearest_edge(opp_pos);
                        let our_edge_dist = self.distance_to_nearest_edge(next_pos);
                        let pushes_to_edge = our_edge_dist > opp_edge_dist && distance_to_opp <= 5;

                        // Check if this direction cuts off opponent's best escape route
                        let opp_best_escape_dir = self.find_best_escape_direction(opp_pos, opp_id);
                        let cuts_best_route = if let Some(best_dir) = opp_best_escape_dir {
                            // Check if our next position blocks their best escape
                            let opp_escape_pos = best_dir.next_position(opp_pos);
                            manhattan_distance(next_pos, opp_escape_pos) <= 1
                        } else {
                            false
                        };

                        caging_scores.push(CagingScore {
                            dir,
                            is_blocked: false,
                            has_escape,
                            reduced_escapes,
                            distance_to_opp,
                            pushes_to_edge,
                            cuts_best_route,
                        });
                    }

                    // Sort by caging effectiveness
                    caging_scores.sort_by(|a, b| {
                        // Never choose blocked
                        if a.is_blocked != b.is_blocked {
                            return a.is_blocked.cmp(&b.is_blocked);
                        }

                        if a.is_blocked {
                            return std::cmp::Ordering::Equal;
                        }

                        // Prefer moves that don't trap ourselves
                        if a.has_escape != b.has_escape {
                            return b.has_escape.cmp(&a.has_escape);
                        }

                        // Primary goal: reduce opponent's escape routes
                        if a.reduced_escapes != b.reduced_escapes {
                            return b.reduced_escapes.cmp(&a.reduced_escapes);
                        }

                        // Secondary: cut off their best escape
                        if a.cuts_best_route != b.cuts_best_route {
                            return b.cuts_best_route.cmp(&a.cuts_best_route);
                        }

                        // Tertiary: push toward edges/corners
                        if a.pushes_to_edge != b.pushes_to_edge {
                            return b.pushes_to_edge.cmp(&a.pushes_to_edge);
                        }

                        // Finally: get closer for pressure
                        a.distance_to_opp.cmp(&b.distance_to_opp)
                    });

                    // Use best caging move
                    if let Some(best) = caging_scores.first() {
                        if !best.is_blocked && best.has_escape {
                            self.players[player_idx].direction = best.dir;
                            return;
                        }
                    }
                }

                // If no opponents or caging failed, fall through to defensive mode below
            }

            // NOT POWERED or no valid opponent target: Seek food normally
            if !self.food_positions.is_empty() {
                // Increment frames since last retarget
                self.players[player_idx].frames_since_retarget += 1;

            // Re-evaluate food target every frame (instant reaction) OR if we don't have a target OR target no longer exists
            let should_retarget = self.players[player_idx].frames_since_retarget >= 1
                || self.players[player_idx].current_target_food.is_none()
                || !self.food_positions.iter().any(|(pos, _, _)|
                    Some(*pos) == self.players[player_idx].current_target_food
                );

            if should_retarget {
                // Special case: if there's only 1 food on the grid, always pursue it regardless of other players
                let only_one_food = self.food_positions.len() == 1;

                // Find nearest food by type (power, super, normal)
                // Skip foods that another player is closer to (unless only 1 food exists)
                let mut nearest_normal: Option<(Position, i32)> = None;
                let mut nearest_super: Option<(Position, i32)> = None;
                let mut nearest_power: Option<(Position, i32)> = None;

                for (food_pos, _spawn_time, food_type) in &self.food_positions {
                    let my_dist = manhattan_distance(player_pos, *food_pos);

                    // Check if any other alive player is closer to this food
                    let another_player_closer = !only_one_food && self.players.iter().any(|other_player| {
                        if !other_player.alive || other_player.id == player_id {
                            return false; // Skip dead players and self
                        }
                        let other_dist = manhattan_distance(other_player.pos, *food_pos);
                        other_dist < my_dist
                    });

                    // Skip this food if another player is closer (unless it's the only food)
                    if another_player_closer {
                        continue;
                    }

                    // CRITICAL: Check if food position has sufficient escape routes
                    // Skip foods in dead ends to prevent player from trapping itself
                    let player_powered = self.players[player_idx].power_active;
                    if !self.has_sufficient_escape_routes(*food_pos, player_id, 2, player_powered) {
                        continue; // Skip this food - it's in a dead end or has no escape
                    }

                    match food_type {
                        FoodType::Power => {
                            // Track nearest power food
                            if let Some((_, min_dist)) = nearest_power {
                                if my_dist < min_dist {
                                    nearest_power = Some((*food_pos, my_dist));
                                }
                            } else {
                                nearest_power = Some((*food_pos, my_dist));
                            }
                        }
                        FoodType::Super => {
                            // Track nearest super food
                            if let Some((_, min_dist)) = nearest_super {
                                if my_dist < min_dist {
                                    nearest_super = Some((*food_pos, my_dist));
                                }
                            } else {
                                nearest_super = Some((*food_pos, my_dist));
                            }
                        }
                        FoodType::Normal => {
                            // Track nearest normal food
                            if let Some((_, min_dist)) = nearest_normal {
                                if my_dist < min_dist {
                                    nearest_normal = Some((*food_pos, my_dist));
                                }
                            } else {
                                nearest_normal = Some((*food_pos, my_dist));
                            }
                        }
                    }
                }

                // Choose target: prioritize power food > super food > normal food
                let chosen_target = match (nearest_power, nearest_super, nearest_normal) {
                    (Some((power_pos, power_dist)), _, _) => {
                        // Power food always takes priority if its distance < 4x normal food distance
                        if let Some((_, normal_dist)) = nearest_normal {
                            if power_dist < normal_dist * 4 {
                                Some(power_pos)
                            } else {
                                Some(power_pos) // Still prioritize power food
                            }
                        } else {
                            Some(power_pos)
                        }
                    },
                    (None, Some((super_pos, super_dist)), Some((normal_pos, normal_dist))) => {
                        // Choose super food if its distance < 2x normal food distance
                        if super_dist < normal_dist * 2 {
                            Some(super_pos)
                        } else {
                            Some(normal_pos)
                        }
                    },
                    (None, Some((super_pos, _)), None) => Some(super_pos),  // Only super food exists
                    (None, None, Some((normal_pos, _))) => Some(normal_pos),  // Only normal food exists
                    (None, None, None) => None,  // No food exists (or all contested)
                };

                // Update target and reset counter
                if let Some(new_target) = chosen_target {
                    self.players[player_idx].current_target_food = Some(new_target);
                    self.players[player_idx].frames_since_retarget = 0;
                }
            }

            // Use current target if we have one
            if let Some(target_food) = self.players[player_idx].current_target_food {
                // Try A* pathfinding to the target food
                if let Some(path) = self.find_path_astar(player_pos, target_food, player_id) {
                    // Path found! Follow it
                    if path.len() >= 2 {
                        let next_step = path[1]; // path[0] is current position

                        // SAFETY CHECK: Verify next step has escape routes
                        // Don't follow path if next step would trap us
                        let player_powered = self.players[player_idx].power_active;
                        if self.has_sufficient_escape_routes(next_step, player_id, 1, player_powered) {
                            // Determine direction to next step
                            if let Some(dir) = self.direction_to_target(player_pos, next_step) {
                                self.players[player_idx].direction = dir;
                                return;
                            }
                        } else {
                            // Next step is unsafe (leads to dead end), clear target
                            self.players[player_idx].current_target_food = None;
                        }
                    }
                }

                // If A* failed to find path to target, clear target and try again next frame
                self.players[player_idx].current_target_food = None;
            }
            } // End of !self.food_positions.is_empty() check

            // Fallback: No path to food found - use defensive positioning heuristics
            struct DirectionEval {
                dir: Direction,
                is_blocked: bool,
                edge_penalty: i32,
                spacing_score: i32,
                is_current: bool,
            }

            // Powered players always move diagonally, non-powered players respect setting
            let directions = if self.players[player_idx].power_active || self.diagonal_movement {
                vec![
                    current_dir,
                    current_dir.turn_right(),
                    current_dir.turn_left(),
                ]
            } else {
                vec![
                    current_dir,
                    current_dir.turn_right_90(),
                    current_dir.turn_left_90(),
                ]
            };

            let mut evals: Vec<DirectionEval> = Vec::new();

            for dir in directions {
                let next_pos = dir.next_position(player_pos);

                let is_blocked = self.is_occupied(next_pos, player_id);
                let edge_penalty = self.calculate_edge_penalty(next_pos);
                let spacing_score = self.calculate_spacing_score(next_pos, player_id);
                let is_current = dir == current_dir;

                evals.push(DirectionEval {
                    dir,
                    is_blocked,
                    edge_penalty,
                    spacing_score,
                    is_current,
                });
            }

            // Sort by: unblocked > spacing (high) > edge (low penalty) > current direction
            evals.sort_by(|a, b| {
                // First: prefer unblocked
                if a.is_blocked != b.is_blocked {
                    return a.is_blocked.cmp(&b.is_blocked);
                }

                // If both blocked, doesn't matter
                if a.is_blocked {
                    return std::cmp::Ordering::Equal;
                }

                // Prefer better spacing (higher = better)
                if a.spacing_score != b.spacing_score {
                    return b.spacing_score.cmp(&a.spacing_score);
                }

                // Prefer less edge penalty (lower = better)
                if a.edge_penalty != b.edge_penalty {
                    return a.edge_penalty.cmp(&b.edge_penalty);
                }

                // Prefer continuing straight
                b.is_current.cmp(&a.is_current)
            });

            // Pick best option
            if let Some(best) = evals.first() {
                if !best.is_blocked {
                    self.players[player_idx].direction = best.dir;
                    return;
                }
            }

            // All directions blocked - keep current direction (will die next frame)
            return;
        }

        // TRON MODE: Defensive survival with opportunistic aggression
        let player_aggression = (self.ai_aggression * self.players[player_idx].aggression_modifier).clamp(0.0, 1.0);
        let effective_look_ahead = self.look_ahead;

        let directions = if self.diagonal_movement {
            vec![
                current_dir,
                current_dir.turn_left(),
                current_dir.turn_right(),
            ]
        } else {
            vec![
                current_dir,
                current_dir.turn_left_90(),
                current_dir.turn_right_90(),
            ]
        };

        struct DirectionScore {
            dir: Direction,
            safe_steps: i32,
            is_blocked: bool,
            edge_penalty: i32,
            spacing_score: i32,
            intercept_score: f64,
            territory_control_score: f64,
            randomness: f64,
            is_current: bool,
        }

        let mut scores: Vec<DirectionScore> = Vec::new();
        let mut rng = rand::thread_rng();
        let nearest_opponent = self.find_nearest_opponent(player_idx);

        for dir in directions {
            let next_pos = dir.next_position(player_pos);

            let is_blocked = self.is_occupied(next_pos, player_id);
            let player_powered = self.players[player_idx].power_active;
            let (safe_steps, _hits_own) = self.count_safe_steps_detailed(player_id, player_pos, dir, effective_look_ahead, player_powered);
            let edge_penalty = self.calculate_edge_penalty(next_pos);
            let spacing_score = self.calculate_spacing_score(next_pos, player_id);
            let is_current = dir == current_dir;

            // Only calculate offensive scores for unblocked moves with reasonable safety
            let (intercept_score, territory_control_score) = if !is_blocked && safe_steps >= 3 {
                if let Some((opp_idx, opp_dist)) = nearest_opponent {
                    let intercept = self.calculate_intercept_score(player_pos, dir, opp_idx, opp_dist);
                    let territory = self.calculate_territory_control_score(player_pos, dir, player_id, opp_idx);
                    (intercept, territory)
                } else {
                    (0.0, 0.0)
                }
            } else {
                (0.0, 0.0)
            };

            let randomness = rng.gen_range(0.0..3.0);

            scores.push(DirectionScore {
                dir,
                safe_steps,
                is_blocked,
                edge_penalty,
                spacing_score,
                intercept_score,
                territory_control_score,
                randomness,
                is_current,
            });
        }

        let max_safe_steps = scores.iter().map(|s| s.safe_steps).max().unwrap_or(0);

        scores.sort_by(|a, b| {
            // CRITICAL RULE 1: Never choose blocked positions
            if a.is_blocked != b.is_blocked {
                return a.is_blocked.cmp(&b.is_blocked);
            }

            // If both blocked, doesn't matter (both bad)
            if a.is_blocked {
                return std::cmp::Ordering::Equal;
            }

            // CRITICAL RULE 2: Avoid immediate death (0 safe steps) if any alternative exists
            if max_safe_steps > 0 {
                if a.safe_steps == 0 && b.safe_steps > 0 {
                    return std::cmp::Ordering::Greater;
                }
                if b.safe_steps == 0 && a.safe_steps > 0 {
                    return std::cmp::Ordering::Less;
                }
            }

            // CRITICAL RULE 3: Strongly prefer moves with more safe steps (SURVIVAL FIRST)
            // If one move has significantly fewer safe steps, heavily penalize it
            if a.safe_steps < 5 || b.safe_steps < 5 {
                // In danger zone - prioritize safe steps heavily
                let safe_diff = (b.safe_steps - a.safe_steps).abs();
                if safe_diff > 2 {
                    return b.safe_steps.cmp(&a.safe_steps);
                }
            }

            // Now among reasonably safe moves, apply defensive + offensive heuristics

            // Defensive heuristics (ALWAYS IMPORTANT)
            let defensive_score_a = (a.safe_steps as f64 * 2.0)        // Safe steps is critical
                + (a.spacing_score as f64 * 3.0)      // Open space is very important
                - (a.edge_penalty as f64 * 4.0);      // Edges are dangerous

            let defensive_score_b = (b.safe_steps as f64 * 2.0)
                + (b.spacing_score as f64 * 3.0)
                - (b.edge_penalty as f64 * 4.0);

            // Offensive heuristics (scaled by aggression)
            // In Tron mode, make offense much more important to encourage boxing in opponents
            let offensive_score_a = (a.intercept_score * player_aggression * 2.5)
                + (a.territory_control_score * player_aggression * 3.0);

            let offensive_score_b = (b.intercept_score * player_aggression * 2.5)
                + (b.territory_control_score * player_aggression * 3.0);

            // Prefer continuing straight (slight bonus for stability)
            let continuity_bonus_a = if a.is_current { 0.5 } else { 0.0 };
            let continuity_bonus_b = if b.is_current { 0.5 } else { 0.0 };

            // Total score: Balance defense and offense (offense now weighted much higher)
            let a_total = defensive_score_a + offensive_score_a + continuity_bonus_a + (a.randomness * 0.1);
            let b_total = defensive_score_b + offensive_score_b + continuity_bonus_b + (b.randomness * 0.1);

            b_total.partial_cmp(&a_total).unwrap_or(std::cmp::Ordering::Equal)
        });

        self.players[player_idx].direction = scores[0].dir;
    }

    // Find the nearest living opponent
    fn find_nearest_opponent(&self, player_idx: usize) -> Option<(usize, f64)> {
        let player = &self.players[player_idx];
        let mut nearest: Option<(usize, f64)> = None;

        for (i, other) in self.players.iter().enumerate() {
            if i == player_idx || !other.alive {
                continue;
            }

            let dx = (other.pos.x - player.pos.x) as f64;
            let dy = (other.pos.y - player.pos.y) as f64;
            let dist = (dx * dx + dy * dy).sqrt();

            if let Some((_, min_dist)) = nearest {
                if dist < min_dist {
                    nearest = Some((i, dist));
                }
            } else {
                nearest = Some((i, dist));
            }
        }

        nearest
    }

    // Find the opponent with the longest trail (highest max_trail_length)
    fn find_longest_opponent(&self, player_idx: usize) -> Option<usize> {
        let mut longest: Option<(usize, usize)> = None;

        for (i, other) in self.players.iter().enumerate() {
            if i == player_idx || !other.alive {
                continue;
            }

            let trail_length = other.max_trail_length;

            if let Some((_, max_length)) = longest {
                if trail_length > max_length {
                    longest = Some((i, trail_length));
                }
            } else {
                longest = Some((i, trail_length));
            }
        }

        longest.map(|(idx, _)| idx)
    }

    // Calculate how good a direction is for intercepting an opponent
    // Returns a score from 0.0 (bad) to 10.0 (excellent)
    fn calculate_intercept_score(&self, pos: Position, dir: Direction, opp_idx: usize, _opp_dist: f64) -> f64 {
        let opponent = &self.players[opp_idx];

        // Predict opponent's future position (project forward along their direction)
        let prediction_steps = (self.look_ahead / 2).max(3);
        let mut predicted_pos = opponent.pos;
        for _ in 0..prediction_steps {
            predicted_pos = opponent.direction.next_position(predicted_pos);

            // Stop if predicted position goes out of bounds
            if predicted_pos.x < 0 || predicted_pos.x >= self.width as i32 ||
               predicted_pos.y < 0 || predicted_pos.y >= self.height as i32 {
                break;
            }
        }

        // Calculate where we'd be if we go in this direction
        let mut test_pos = pos;
        for _ in 0..prediction_steps {
            test_pos = dir.next_position(test_pos);

            if test_pos.x < 0 || test_pos.x >= self.width as i32 ||
               test_pos.y < 0 || test_pos.y >= self.height as i32 {
                break;
            }
        }

        // Calculate distance from our projected position to opponent's predicted position
        let dx = (predicted_pos.x - test_pos.x) as f64;
        let dy = (predicted_pos.y - test_pos.y) as f64;
        let intercept_dist = (dx * dx + dy * dy).sqrt();

        // Also consider if this direction is generally toward the opponent's current position
        let to_opp_x = opponent.pos.x - pos.x;
        let to_opp_y = opponent.pos.y - pos.y;

        let dir_vec = match dir {
            Direction::Up => (0, -1),
            Direction::Down => (0, 1),
            Direction::Left => (-1, 0),
            Direction::Right => (1, 0),
            Direction::UpLeft => (-1, -1),
            Direction::UpRight => (1, -1),
            Direction::DownLeft => (-1, 1),
            Direction::DownRight => (1, 1),
        };

        // Dot product to see if we're heading toward opponent
        let toward_score = (dir_vec.0 * to_opp_x + dir_vec.1 * to_opp_y) as f64;

        // Combine scores: closer intercept is better, heading toward is better
        // Inverse distance (closer = higher score) + direction alignment
        let intercept_score = if intercept_dist > 0.0 {
            10.0 / (1.0 + intercept_dist)
        } else {
            10.0
        };

        let direction_bonus = if toward_score > 0.0 { 2.0 } else { 0.0 };

        intercept_score + direction_bonus
    }

    // Returns (safe_steps, hits_own_trail)
    fn count_safe_steps_detailed(&self, player_id: u8, start_pos: Position, direction: Direction, max_steps: i32, is_powered: bool) -> (i32, bool) {
        let mut pos = start_pos;
        let mut steps = 0;
        let mut hits_own_trail = false;

        for _ in 0..max_steps {
            // Calculate next position in this direction
            pos = direction.next_position(pos);

            // Check boundaries
            if pos.x < 0 || pos.x >= self.width as i32 ||
               pos.y < 0 || pos.y >= self.height as i32 {
                break;
            }

            // Check if occupied
            let occupied = if self.food_mode {
                // Food mode: check visible trails only
                self.players.iter().find_map(|p| {
                    if !p.alive {
                        return None;
                    }
                    if p.trail.iter().any(|trail_pos| trail_pos.x == pos.x && trail_pos.y == pos.y) {
                        Some(p.id)
                    } else {
                        None
                    }
                })
            } else {
                // Tron mode: check persistent grid
                self.grid[pos.y as usize][pos.x as usize]
            };

            if let Some(occupant) = occupied {
                // Hit something
                if occupant == player_id {
                    // Hit own trail - always stop
                    hits_own_trail = true;
                    break;
                } else if is_powered && self.food_mode {
                    // Powered players in food mode can pass through other players' trails
                    steps += 1;
                    continue;
                } else {
                    // All other cases: stop at any trail
                    // - Tron mode: everyone stops at trails
                    // - Food mode, not powered: stop at any trail
                    break;
                }
            }

            steps += 1;
        }

        (steps, hits_own_trail)
    }

    pub fn update(&mut self) -> bool {
        if self.game_over {
            return false;
        }

        // Move traveling food pieces BEFORE timing check for smooth, consistent movement
        // Food moves from head to tail at a slower, controlled rate
        let mut any_food_moved = false;
        if self.food_mode {
            for player in &mut self.players {
                if !player.alive {
                    continue;
                }

                // Increment food movement counter every update call
                player.food_move_counter += 1;

                // Move food every 3 updates for visible but smooth movement
                // This makes food travel at roughly 1/3 the frame rate
                if player.food_move_counter >= 3 {
                    player.food_move_counter = 0;

                    // Move all food pieces by one step
                    let had_food = !player.traveling_food.is_empty();
                    player.traveling_food.retain_mut(|food| {
                        if food.trail_position == 0 {
                            // Food reached the tail - add to max trail length and remove
                            player.max_trail_length += 1;
                            false
                        } else {
                            // Move one step toward tail
                            food.trail_position -= 1;
                            true
                        }
                    });
                    if had_food {
                        any_food_moved = true;
                    }
                }
            }
        }

        // NEW SPEED BOOST SYSTEM:
        // Global game always ticks at 0.99x configured interval (1% faster)
        // Non-powered players only move when their last_move_time >= update_interval
        // Powered players move when their last_move_time >= 0.99 * update_interval
        // This creates a 1% speed difference between powered and non-powered players
        let global_tick_interval = Duration::from_secs_f64(self.update_interval.as_secs_f64() * 0.99);

        if self.last_update.elapsed() < global_tick_interval {
            // Return true if food moved, so it gets rendered
            return any_food_moved;
        }
        self.last_update = Instant::now();

        // Food mode: maintain minimum food count equal to alive player count
        if self.food_mode {
            let alive_player_count = self.players.iter().filter(|p| p.alive).count();
            let min_food_count = alive_player_count;

            // Spawn food until we reach minimum count
            while self.food_positions.len() < min_food_count && self.food_positions.len() < self.food_max_count {
                self.spawn_food();
            }

            // Random spawning: 3% chance per update cycle to spawn additional food beyond minimum
            if self.food_positions.len() < self.food_max_count {
                let mut rng = rand::thread_rng();
                if rng.gen_bool(0.03) {
                    self.spawn_food();
                }
            }
        }

        // Food mode: check for expired food and relocate based on TTL
        if self.food_mode {
            let now = Instant::now();
            let ttl_duration = Duration::from_secs(self.food_ttl_seconds);
            let mut rng = rand::thread_rng();

            // Find indices of expired foods
            let expired_food_indices: Vec<usize> = self.food_positions.iter()
                .enumerate()
                .filter(|(_, (_, spawn_time, _))| spawn_time.elapsed() >= ttl_duration)
                .map(|(idx, _)| idx)
                .collect();

            // Relocate each expired food
            for food_idx in expired_food_indices {
                let mut attempts = 0;
                const MAX_ATTEMPTS: usize = 1000;

                while attempts < MAX_ATTEMPTS {
                    let x = rng.gen_range(1..(self.width - 1)) as i32;
                    let y = rng.gen_range(1..(self.height - 1)) as i32;
                    let new_pos = Position { x, y };

                    // Check if position is empty (not on any visible trail or other food)
                    let occupied_by_trail = self.players.iter().any(|p| {
                        p.trail.iter().any(|trail_pos| trail_pos.x == x && trail_pos.y == y)
                    });

                    let occupied_by_other_food = self.food_positions.iter()
                        .enumerate()
                        .any(|(idx, (other_pos, _, _))| {
                            idx != food_idx && other_pos.x == x && other_pos.y == y
                        });

                    if !occupied_by_trail && !occupied_by_other_food {
                        // Found empty spot, relocate food here
                        if let Some((food_pos, spawn_time, _)) = self.food_positions.get_mut(food_idx) {
                            *food_pos = new_pos;
                            *spawn_time = now;
                        }
                        break;
                    }

                    attempts += 1;
                }

                // If we couldn't find an empty spot, just reset the timer without moving
                // This prevents infinite relocation attempts in crowded grids
                if attempts >= MAX_ATTEMPTS {
                    if let Some((_, spawn_time, _)) = self.food_positions.get_mut(food_idx) {
                        *spawn_time = now;
                    }
                }
            }
        }

        // Food mode: respawn dead players after a short delay
        if self.food_mode {
            let dead_players: Vec<usize> = self.players.iter().enumerate()
                .filter(|(_, p)| !p.alive && p.death_time.is_some())
                .filter(|(_, p)| p.death_time.unwrap().elapsed() >= Duration::from_millis(500))
                .map(|(idx, _)| idx)
                .collect();

            for player_idx in dead_players {
                self.respawn_player(player_idx);
            }
        }

        // Check for expired power modes
        let now = Instant::now();
        for player in &mut self.players {
            if player.power_active {
                if let Some(end_time) = player.power_end_time {
                    if now >= end_time {
                        // Power mode expired
                        player.power_active = false;
                        player.power_end_time = None;

                        // If diagonal movement is disabled and player is moving diagonally,
                        // convert to cardinal direction
                        if !self.diagonal_movement && player.direction.is_diagonal() {
                            player.direction = player.direction.to_cardinal();
                        }
                    }
                }
            }
        }

        // Update each player's aggression modifier (continuous oscillation for individuality)
        for player in &mut self.players {
            // Advance phase
            player.aggression_phase += player.aggression_frequency;
            if player.aggression_phase > std::f64::consts::TAU {
                player.aggression_phase -= std::f64::consts::TAU;
            }

            // Calculate modifier: oscillates between 0.9 and 1.1 (±10%)
            // sin ranges from -1 to 1, so we map it to 0.9 to 1.1
            player.aggression_modifier = 1.0 + (player.aggression_phase.sin() * 0.1);
        }

        // Update gradient animation offsets (reload config each frame to pick up changes)
        let (animation_speed, scale_animation_speed, animation_direction) = {
            if let Ok(cfg) = BandwidthConfig::load() {
                (cfg.tron_animation_speed, cfg.tron_scale_animation_speed, cfg.tron_animation_direction)
            } else {
                // Fallback to no animation if config load fails
                (0.0, false, "forward".to_string())
            }
        };

        if animation_speed > 0.0 {
            let delta_seconds = self.update_interval.as_secs_f64();
            let fps = 1.0 / delta_seconds;

            for player in &mut self.players {
                if !player.alive {
                    continue;
                }

                let trail_len = player.trail.len().max(1);

                // Calculate effective speed (optionally scaled by trail length)
                let effective_speed = if scale_animation_speed {
                    // Scale speed based on how long the trail is relative to max length
                    let length_ratio = if player.max_trail_length > 0 {
                        trail_len as f64 / player.max_trail_length as f64
                    } else {
                        1.0
                    };
                    animation_speed * length_ratio
                } else {
                    animation_speed
                };

                // Advance animation offset
                let leds_per_second = effective_speed * fps;
                let offset_delta = (leds_per_second * delta_seconds) / trail_len as f64;

                // Determine effective direction (accounting for per-player flip state)
                let effective_direction = if player.animation_direction_flipped {
                    // Flip the direction for this player
                    if animation_direction == "forward" {
                        "backward"
                    } else {
                        "forward"
                    }
                } else {
                    animation_direction.as_str()
                };

                // Apply direction (forward = head to tail, backward = tail to head)
                // User reported directions were reversed, so swapped the logic:
                // Also: backward needs speed multiplier to match visual perception of forward
                if effective_direction == "forward" {
                    // Forward: subtract (was backward before)
                    player.animation_offset = (player.animation_offset - offset_delta) % 1.0;
                    if player.animation_offset < 0.0 {
                        player.animation_offset += 1.0;
                    }
                } else {
                    // Backward: add with 1.7x speed multiplier to match visual perception
                    // The multiplier compensates for how our eyes perceive tail-to-head movement
                    player.animation_offset = (player.animation_offset + (offset_delta * 1.7)) % 1.0;
                }
            }
        }

        // Determine which players should move this tick based on per-player timing
        // Powered players: move if last_move_time >= 0.99 * update_interval
        // Non-powered players: move if last_move_time >= update_interval
        let powered_interval = self.update_interval.mul_f64(0.99);
        let normal_interval = self.update_interval;

        let players_should_move: Vec<bool> = self.players.iter().map(|p| {
            if !p.alive {
                false  // Dead players don't move
            } else {
                let required_interval = if p.power_active {
                    powered_interval
                } else {
                    normal_interval
                };
                p.last_move_time.elapsed() >= required_interval
            }
        }).collect();

        // AI makes decisions (only for players who should move this tick)
        for i in 0..self.players.len() {
            if players_should_move[i] {
                self.ai_decide(i);
            }
        }

        // Note: Grid is persistent and already contains all visited positions
        // We don't rebuild it from trails because trail_length may be limited for rendering
        // The grid tracks ALL positions ever visited to prevent players from crossing their own path

        // Track which food was eaten (can't modify food_positions inside player loop due to borrow checker)
        let mut eaten_food_indices: Vec<usize> = Vec::new();

        // Pre-compute collision data for food mode (can't borrow players inside mutable loop)
        // Store (player_id, player_idx, trail_positions, power_active, head_pos, max_trail_length) for each player
        let visible_trail_data: Vec<(u8, usize, Vec<Position>, bool, Position, usize)> = if self.food_mode {
            self.players.iter().enumerate().map(|(idx, p)| {
                if p.alive {
                    (p.id, idx, p.trail.iter().copied().collect(), p.power_active, p.pos, p.max_trail_length)
                } else {
                    (p.id, idx, Vec::new(), false, Position { x: 0, y: 0 }, 0)
                }
            }).collect()
        } else {
            Vec::new()
        };

        // Track positions players are moving to in this update (to prevent two players moving to same spot)
        let mut positions_claimed_this_frame: Vec<Position> = Vec::new();

        // Track players to sever trails due to power mode collisions (can't modify during loop due to borrow checker)
        // Format: (victim_player_idx, collision_position)
        // Track trail cuts: (attacker_idx, victim_idx, collision_pos)
        let mut players_to_sever: Vec<(usize, usize, Position)> = Vec::new();

        // Track head-on kills: (attacker_idx, victim_idx, victim_length)
        // When powered player hits another player's head, kill victim and transfer their length
        let mut head_on_kills: Vec<(usize, usize, usize)> = Vec::new();

        // Pre-compute player ID to index mapping (for Tron mode collision detection)
        let player_id_to_idx: std::collections::HashMap<u8, usize> = self.players.iter()
            .enumerate()
            .map(|(idx, p)| (p.id, idx))
            .collect();

        // Update player positions and check collisions (only for players who should move this tick)
        for (player_idx, player) in self.players.iter_mut().enumerate() {
            if !player.alive {
                continue;
            }

            // Skip players who shouldn't move this tick (based on per-player timing)
            if !players_should_move[player_idx] {
                continue;
            }

            // Calculate next position
            let next_pos = player.direction.next_position(player.pos);

            // DIAGONAL COLLISION FIX: Check intermediate positions for diagonal moves
            // When moving diagonally, the player crosses through two intermediate positions
            // that must also be checked for collisions to prevent "phasing through" trails
            if player.direction.is_diagonal() {
                let intermediate_positions = player.direction.get_intermediate_positions(player.pos);

                let mut diagonal_blocked = false;

                // Check if any intermediate position is occupied or out of bounds
                for intermediate_pos in intermediate_positions {
                    // Check boundaries for intermediate positions
                    if intermediate_pos.x < 0 || intermediate_pos.x >= self.width as i32 ||
                       intermediate_pos.y < 0 || intermediate_pos.y >= self.height as i32 {
                        diagonal_blocked = true;
                        break;
                    }

                    // Check if intermediate position is occupied and which player owns it
                    let (is_occupied, hit_player_id, hit_player_idx) = if self.food_mode {
                        // Food mode: check visible trails and determine owner
                        let mut occupant_id: Option<u8> = None;
                        let mut occupant_idx: Option<usize> = None;

                        for (other_id, other_idx, trail, _, _, _) in &visible_trail_data {
                            if trail.iter().any(|trail_pos| trail_pos.x == intermediate_pos.x && trail_pos.y == intermediate_pos.y) {
                                occupant_id = Some(*other_id);
                                occupant_idx = Some(*other_idx);
                                break;
                            }
                        }

                        let is_occ = occupant_id.is_some() || positions_claimed_this_frame.iter().any(|claimed_pos| {
                            claimed_pos.x == intermediate_pos.x && claimed_pos.y == intermediate_pos.y
                        });

                        (is_occ, occupant_id, occupant_idx)
                    } else {
                        // Tron mode: check persistent grid
                        let ix = intermediate_pos.x as usize;
                        let iy = intermediate_pos.y as usize;
                        let grid_value = self.grid[iy][ix];
                        (grid_value.is_some(), grid_value, None)
                    };

                    if is_occupied {
                        // Power mode handling: powered players are INVINCIBLE
                        if self.food_mode && player.power_active {
                            // Powered players can pass through ANY trail (own or others)
                            if let Some(hit_id) = hit_player_id {
                                if hit_id != player.id {
                                    // Hit other player's trail - sever their trail at this point
                                    if let Some(victim_idx) = hit_player_idx {
                                        players_to_sever.push((player_idx, victim_idx, intermediate_pos));
                                    }
                                }
                                // If hit_id == player.id, just continue (don't die, don't sever)
                            }
                            // Powered player continues moving through any trail
                        } else {
                            // Not powered or not in food mode - collision is fatal
                            diagonal_blocked = true;
                            break;
                        }
                    }
                }

                if diagonal_blocked {
                    // Can't move diagonally through occupied space
                    player.alive = false;
                    player.death_time = Some(Instant::now());
                    continue;
                }
            }

            // Check boundaries
            if next_pos.x < 0 || next_pos.x >= self.width as i32 ||
               next_pos.y < 0 || next_pos.y >= self.height as i32 {
                player.alive = false;
                player.death_time = Some(Instant::now());
                continue;
            }

            // Check for head-on collision with other players (food mode with power active)
            if self.food_mode && player.power_active {
                for (other_id, other_idx, _trail, _powered, other_head_pos, other_max_length) in &visible_trail_data {
                    if *other_id != player.id {
                        // Check if next position matches the other player's head position
                        if next_pos.x == other_head_pos.x && next_pos.y == other_head_pos.y {
                            // Head-on collision! Kill the victim and steal their length
                            head_on_kills.push((player_idx, *other_idx, *other_max_length));
                            break;
                        }
                    }
                }
            }

            // Check collision with trails
            let x = next_pos.x as usize;
            let y = next_pos.y as usize;

            // Check for collision and determine which player was hit
            let (collision, hit_player_idx) = if self.food_mode {
                // Food mode: check visible trails AND positions claimed by other players this frame
                let mut hit_player: Option<usize> = None;
                let mut has_collision = false;
                let mut hit_own_trail = false;

                // Check trails - find which player's trail was hit
                for (other_id, other_idx, trail, _other_powered, _, _) in &visible_trail_data {
                    if trail.iter().any(|trail_pos| trail_pos.x == next_pos.x && trail_pos.y == next_pos.y) {
                        has_collision = true;
                        if *other_id == player.id {
                            // Hit own trail - always fatal
                            hit_own_trail = true;
                        } else {
                            // Hit other player's trail
                            hit_player = Some(*other_idx);
                        }
                        break;
                    }
                }

                // If no trail hit, check claimed positions (head-on collisions)
                if !has_collision {
                    has_collision = positions_claimed_this_frame.iter().any(|claimed_pos| {
                        claimed_pos.x == next_pos.x && claimed_pos.y == next_pos.y
                    });
                }

                // Return collision status and victim (None if hit own trail)
                (has_collision, if hit_own_trail { None } else { hit_player })
            } else {
                // Tron mode: use persistent grid (ALL visited positions)
                // Determine which player's trail was hit
                if let Some(occupant_id) = self.grid[y][x] {
                    let hit_player = if occupant_id == player.id {
                        None // Hit own trail - always fatal
                    } else {
                        // Hit another player's trail - look up their index
                        player_id_to_idx.get(&occupant_id).copied()
                    };
                    (true, hit_player)
                } else {
                    (false, None)
                }
            };

            if collision {
                // Food mode + powered: Can cross trails and sever them
                if self.food_mode && player.power_active {
                    // Powered players are INVINCIBLE - can pass through ANY trail
                    // If hitting another player's trail, sever it
                    if let Some(victim_idx) = hit_player_idx {
                        players_to_sever.push((player_idx, victim_idx, next_pos));
                    }
                    // Powered player continues moving (don't kill them)
                    // This includes crossing own trail and other players' trails
                } else {
                    // Normal collision: current player dies
                    // This includes:
                    // - Tron mode: hitting any trail
                    // - Food mode (not powered): hitting any trail or own trail
                    player.alive = false;
                    player.death_time = Some(Instant::now());
                    continue;
                }
            }

            // Claim this position for this frame (food mode only)
            if self.food_mode {
                positions_claimed_this_frame.push(next_pos);
            }

            // Move is safe, update position and mark grid
            player.pos = next_pos;
            player.trail.push_back(next_pos);

            // Update last move time for per-player speed timing
            player.last_move_time = Instant::now();

            // Mark this position as occupied in the grid (only in Tron mode)
            // Food mode doesn't use persistent grid marking
            if !self.food_mode {
                self.grid[y][x] = Some(player.id);
            }

            // Food mode: check if player ate any food
            if self.food_mode {
                // Check if direction flipping is enabled
                let flip_direction_on_food = BandwidthConfig::load()
                    .map(|cfg| cfg.tron_flip_direction_on_food)
                    .unwrap_or(false);

                for (food_idx, (food_pos, _spawn_time, food_type)) in self.food_positions.iter().enumerate() {
                    if next_pos.x == food_pos.x && next_pos.y == food_pos.y {
                        // Player ate this food!
                        match food_type {
                            FoodType::Power => {
                                // Activate power mode: 10 second duration
                                player.power_active = true;
                                player.power_end_time = Some(Instant::now() + Duration::from_secs(10));
                                // Spawn 10 yellow food pieces to travel down the trail from head to tail
                                // Stagger them so they appear as 10 consecutive LEDs at the head
                                let trail_len = player.trail.len();
                                for i in 0..10 {
                                    if i < trail_len {
                                        player.traveling_food.push(TravelingFood {
                                            trail_position: trail_len - 1 - i,  // Start at head, staggered downward
                                            color: (255, 255, 0),  // Yellow
                                        });
                                    }
                                }
                            }
                            FoodType::Super => {
                                // Spawn 5 red food pieces to travel down the trail from head to tail
                                // Stagger them so they appear as 5 consecutive LEDs at the head
                                let trail_len = player.trail.len();
                                for i in 0..5 {
                                    if i < trail_len {
                                        player.traveling_food.push(TravelingFood {
                                            trail_position: trail_len - 1 - i,  // Start at head, staggered downward
                                            color: (255, 0, 0),  // Red
                                        });
                                    }
                                }
                            }
                            FoodType::Normal => {
                                // Spawn 1 white food piece at head to travel down the trail
                                let trail_len = player.trail.len();
                                if trail_len > 0 {
                                    player.traveling_food.push(TravelingFood {
                                        trail_position: trail_len - 1,  // Start at head
                                        color: (255, 255, 255),  // White
                                    });
                                }
                            }
                        }

                        // Flip animation direction if enabled
                        if flip_direction_on_food {
                            player.animation_direction_flipped = !player.animation_direction_flipped;
                        }

                        // Mark this food as eaten (will remove after loop)
                        eaten_food_indices.push(food_idx);
                        break; // Only eat one food per move
                    }
                }

                // Limit trail using player's individual max_trail_length
                if player.max_trail_length > 0 {
                    player.limit_trail(player.max_trail_length, &mut self.grid);
                }

                // Note: Traveling food pieces are moved outside the player loop
                // based on powered player speed timing
            } else {
                // Non-food mode: use global trail_length for rendering and clear grid
                if self.trail_length > 0 {
                    player.limit_trail(self.trail_length, &mut self.grid);
                }
            }
        }

        // Sever trails for players hit by powered players (after player loop to avoid borrow issues)
        // Track length to transfer: (attacker_idx, segments_removed)
        let mut length_transfers: Vec<(usize, usize)> = Vec::new();

        // Process in reverse player index order to avoid issues with trail modifications
        for (attacker_idx, victim_idx, collision_pos) in players_to_sever {
            if victim_idx < self.players.len() {
                let victim = &mut self.players[victim_idx];
                if victim.alive {
                    // Find the collision position in the victim's trail
                    // We need to find where the collision happened and remove everything from there backward
                    if let Some(collision_trail_idx) = victim.trail.iter().position(|pos| {
                        pos.x == collision_pos.x && pos.y == collision_pos.y
                    }) {
                        // Remove all trail segments from the collision point backward (toward tail)
                        // Keep segments from collision point forward (toward head)
                        // This means keeping indices [collision_trail_idx..trail.len()]
                        let segments_to_keep: Vec<Position> = victim.trail.iter()
                            .skip(collision_trail_idx)
                            .copied()
                            .collect();

                        victim.trail.clear();
                        for segment in segments_to_keep {
                            victim.trail.push_back(segment);
                        }

                        // Reduce max_trail_length by the number of segments removed
                        // to prevent the trail from growing back immediately
                        let segments_removed = collision_trail_idx;
                        if segments_removed > 0 {
                            victim.max_trail_length = victim.max_trail_length.saturating_sub(segments_removed);
                            // Ensure minimum trail length of 1
                            if victim.max_trail_length < 1 {
                                victim.max_trail_length = 1;
                            }

                            // Track length to transfer to attacker
                            length_transfers.push((attacker_idx, segments_removed));
                        }
                    }
                }
            }
        }

        // Transfer severed lengths to attackers (after victim loop to avoid borrow issues)
        for (attacker_idx, segments_to_add) in length_transfers {
            if attacker_idx < self.players.len() {
                let attacker = &mut self.players[attacker_idx];
                if attacker.alive {
                    attacker.max_trail_length += segments_to_add;
                    // Flash yellow for 100ms when cutting a trail
                    attacker.yellow_flash_until = Some(Instant::now() + Duration::from_millis(100));
                }
            }
        }

        // Process head-on kills: powered player hits another player's head
        // Kill victim and transfer their trail length to the attacker
        for (attacker_idx, victim_idx, victim_length) in head_on_kills {
            // Kill the victim
            if victim_idx < self.players.len() {
                let victim = &mut self.players[victim_idx];
                if victim.alive {
                    victim.alive = false;
                    victim.death_time = Some(Instant::now());
                }
            }

            // Transfer victim's length to attacker
            if attacker_idx < self.players.len() {
                let attacker = &mut self.players[attacker_idx];
                if attacker.alive {
                    attacker.max_trail_length += victim_length;
                    // Flash yellow for 200ms when killing a player (longer than trail cut)
                    attacker.yellow_flash_until = Some(Instant::now() + Duration::from_millis(200));
                }
            }
        }

        // Remove eaten foods (after player loop to avoid borrow issues)
        // Remove in reverse order to avoid index shifting issues
        eaten_food_indices.sort_unstable();
        eaten_food_indices.dedup(); // Remove duplicates if two players ate same food
        for &idx in eaten_food_indices.iter().rev() {
            if idx < self.food_positions.len() {
                self.food_positions.remove(idx);
            }
        }

        // Check if game is over (skip in food mode - game never ends)
        if !self.food_mode {
            // Single player mode (snake): game over when player dies
            // Multi-player mode: game over when only one or zero players remain
            let alive_count = self.players.iter().filter(|p| p.alive).count();

            if self.players.len() == 1 {
                // Single player mode - game over when the player dies
                if alive_count == 0 {
                    self.game_over = true;
                }
            } else if self.players.len() > 1 {
                // Multi-player mode - game over when only one or zero alive
                if alive_count <= 1 {
                    // Wait for death animation to finish before declaring game over
                    // Use the LAST (most recent) death time, not first, so all dead players get their full animation
                    if let Some(last_death) = self.players.iter().filter_map(|p| p.death_time).max() {
                        if last_death.elapsed() >= Duration::from_millis(1500) {
                            self.game_over = true;
                        }
                    } else {
                        self.game_over = true;
                    }
                } else if alive_count == 0 {
                    self.game_over = true;
                }
            }
        }

        true // Updated
    }

    pub fn render(&self, total_leds: usize) -> Vec<u8> {
        let mut frame = vec![0u8; total_leds * 3];

        // Render all foods (white for regular, red for super, yellow for power)
        for (food_pos, _spawn_time, food_type) in &self.food_positions {
            let x = food_pos.x as usize;
            let y = food_pos.y as usize;
            if x < self.width && y < self.height {
                let led_idx = y * self.width + x;
                if led_idx < total_leds {
                    let offset = led_idx * 3;
                    match food_type {
                        FoodType::Power => {
                            // Power food is yellow
                            frame[offset] = 255;     // R
                            frame[offset + 1] = 255; // G
                            frame[offset + 2] = 0;   // B
                        }
                        FoodType::Super => {
                            // Super food is red
                            frame[offset] = 255;     // R
                            frame[offset + 1] = 0;   // G
                            frame[offset + 2] = 0;   // B
                        }
                        FoodType::Normal => {
                            // Regular food is white
                            frame[offset] = 255;     // R
                            frame[offset + 1] = 255; // G
                            frame[offset + 2] = 255; // B
                        }
                    }
                }
            }
        }

        // Render each player's trail with gradient
        for player in &self.players {
            let trail_len = player.trail.len();
            if trail_len == 0 {
                continue;
            }

            // Check if player is dead and should blink
            let mut render = true;
            if let Some(death_time) = player.death_time {
                let elapsed = death_time.elapsed().as_millis();
                if elapsed < 1000 {
                    // Blink for 1 second: 100ms on, 100ms off
                    render = (elapsed / 100) % 2 == 0;
                } else {
                    // After 1 second, don't render dead players
                    continue;
                }
            }

            if !render {
                continue;
            }

            // Check if player is in power mode and should flash yellow
            // Only flash when crossing trails or killing players (checked via yellow_flash_until)
            let should_flash_yellow = if player.power_active {
                if let Some(flash_end) = player.yellow_flash_until {
                    let now = Instant::now();
                    now < flash_end
                } else {
                    false
                }
            } else {
                false
            };

            for (idx, pos) in player.trail.iter().enumerate() {
                let x = pos.x as usize;
                let y = pos.y as usize;

                if x >= self.width || y >= self.height {
                    continue;
                }

                // Calculate LED index (depends on matrix layout)
                let led_idx = y * self.width + x;
                if led_idx >= total_leds {
                    continue;
                }

                // Check if there's a traveling food piece at this trail position
                let food_at_position = player.traveling_food.iter()
                    .find(|food| food.trail_position == idx);

                // Check if this position is adjacent to a traveling food piece
                let is_adjacent_to_food = player.traveling_food.iter()
                    .any(|food| {
                        let food_pos = food.trail_position as i32;
                        let current_pos = idx as i32;
                        (food_pos - current_pos).abs() == 1
                    });

                // Apply brightness fade when enabled and trail length > 1
                let mut brightness = if self.trail_fade && player.max_trail_length > 1 {
                    // Fade from 0.2 (oldest/tail) to 1.0 (newest/head) with minimum 20% brightness
                    let fade = idx as f64 / (player.max_trail_length - 1).max(1) as f64;
                    0.2 + (fade * 0.8)  // Range: 0.2 to 1.0
                } else {
                    1.0 // Full brightness if fading disabled or length is 1
                };

                // Dim adjacent LEDs for better visual contrast with traveling food
                if is_adjacent_to_food && food_at_position.is_none() {
                    brightness *= 0.3;  // Reduce brightness to 30% for adjacent LEDs
                }

                let offset = led_idx * 3;

                // Render priority: traveling food > power flash > normal gradient
                if let Some(food) = food_at_position {
                    // Render traveling food piece with its color
                    frame[offset] = (food.color.0 as f64 * brightness) as u8;     // R
                    frame[offset + 1] = (food.color.1 as f64 * brightness) as u8; // G
                    frame[offset + 2] = (food.color.2 as f64 * brightness) as u8; // B
                } else if should_flash_yellow {
                    // Power mode yellow flash
                    frame[offset] = (255.0 * brightness) as u8;     // R
                    frame[offset + 1] = (255.0 * brightness) as u8; // G
                    frame[offset + 2] = 0;                          // B
                } else {
                    // Normal gradient color
                    // Gradient position: 0.0 (oldest) to 1.0 (newest/head)
                    let gradient_pos = idx as f64 / trail_len.max(1) as f64;
                    // Apply animation offset to gradient position
                    let animated_pos = (gradient_pos + player.animation_offset) % 1.0;
                    let color = player.gradient.at(animated_pos);

                    frame[offset] = (color.r * 255.0 * brightness) as u8;
                    frame[offset + 1] = (color.g * 255.0 * brightness) as u8;
                    frame[offset + 2] = (color.b * 255.0 * brightness) as u8;
                }
            }
        }

        frame
    }

    pub fn is_game_over(&self) -> bool {
        self.game_over
    }
}


pub async fn run_tron_mode(
    config: Arc<Mutex<BandwidthConfig>>,
    ddp_client: Arc<Mutex<Option<DDPConnection>>>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
) -> Result<()> {
    // Check if multi-device mode is enabled and create manager
    let mut multi_device_manager: Option<MultiDeviceManager> = None;
    let multi_device_enabled = {
        let cfg = config.lock().unwrap();
        if cfg.multi_device_enabled && !cfg.wled_devices.is_empty() {
            // Convert config to multi-device format
            let devices: Vec<WLEDDevice> = cfg.wled_devices.iter().map(|d| WLEDDevice {
                ip: d.ip.clone(),
                led_offset: d.led_offset,
                led_count: d.led_count,
                enabled: d.enabled,
            }).collect();

            let md_config = MultiDeviceConfig {
                devices,
                send_parallel: cfg.multi_device_send_parallel,
                fail_fast: cfg.multi_device_fail_fast,
            };

            match MultiDeviceManager::new(md_config) {
                Ok(manager) => {
                    println!("Multi-device mode enabled with {} devices", manager.device_count());
                    multi_device_manager = Some(manager);
                    true
                }
                Err(e) => {
                    eprintln!("Failed to initialize multi-device manager: {}", e);
                    false
                }
            }
        } else {
            false
        }
    };

    // Initial config
    let (mut width, mut height, mut speed_ms, mut reset_delay_ms, mut look_ahead, mut trail_length, mut ai_aggression, mut num_players, mut player_colors, mut food_mode, mut food_max_count, mut food_ttl_seconds, mut trail_fade, mut super_food_enabled, mut diagonal_movement, mut interpolation, mut global_brightness) = {
        let cfg = config.lock().unwrap();
        let colors = vec![
            cfg.tron_player_1_color.clone(),
            cfg.tron_player_2_color.clone(),
            cfg.tron_player_3_color.clone(),
            cfg.tron_player_4_color.clone(),
            cfg.tron_player_5_color.clone(),
            cfg.tron_player_6_color.clone(),
            cfg.tron_player_7_color.clone(),
            cfg.tron_player_8_color.clone(),
        ];
        (
            cfg.tron_width,
            cfg.tron_height,
            cfg.tron_speed_ms,
            cfg.tron_reset_delay_ms,
            cfg.tron_look_ahead,
            cfg.tron_trail_length,
            cfg.tron_ai_aggression,
            cfg.tron_num_players,
            colors,
            cfg.tron_food_mode,
            cfg.tron_food_max_count,
            cfg.tron_food_ttl_seconds,
            cfg.tron_trail_fade,
            cfg.tron_super_food_enabled,
            cfg.tron_diagonal_movement,
            cfg.tron_interpolation.clone(),
            cfg.global_brightness,
        )
    };

    let mut total_leds = width * height;
    let mut game = TronGame::new(width, height, speed_ms, look_ahead, trail_length, ai_aggression, num_players, &player_colors, food_mode, food_max_count, food_ttl_seconds, trail_fade, super_food_enabled, diagonal_movement, &interpolation);

    let mut last_config_check = Instant::now();

    loop {
        // Check shutdown signal
        if shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }

        // Check for config changes every 500ms
        if last_config_check.elapsed() > Duration::from_millis(500) {
            last_config_check = Instant::now();

            // Reload config from disk to pick up web UI changes
            let cfg = match BandwidthConfig::load() {
                Ok(c) => c,
                Err(_) => {
                    // If reload fails, use cached config
                    config.lock().unwrap().clone()
                }
            };

            // Build new player colors vector
            let new_player_colors = vec![
                cfg.tron_player_1_color.clone(),
                cfg.tron_player_2_color.clone(),
                cfg.tron_player_3_color.clone(),
                cfg.tron_player_4_color.clone(),
                cfg.tron_player_5_color.clone(),
                cfg.tron_player_6_color.clone(),
                cfg.tron_player_7_color.clone(),
                cfg.tron_player_8_color.clone(),
            ];

            // Check if any individual player color changed
            let colors_changed = new_player_colors != player_colors;

            // Update global brightness immediately (even if other config hasn't changed)
            global_brightness = cfg.global_brightness;

            let config_changed = cfg.tron_width != width
                || cfg.tron_height != height
                || cfg.tron_speed_ms != speed_ms
                || cfg.tron_look_ahead != look_ahead
                || cfg.tron_trail_length != trail_length
                || cfg.tron_ai_aggression != ai_aggression
                || cfg.tron_num_players != num_players
                || cfg.tron_food_mode != food_mode
                || cfg.tron_food_max_count != food_max_count
                || cfg.tron_food_ttl_seconds != food_ttl_seconds
                || cfg.tron_trail_fade != trail_fade
                || cfg.tron_super_food_enabled != super_food_enabled
                || cfg.tron_diagonal_movement != diagonal_movement
                || cfg.tron_interpolation != interpolation
                || colors_changed;

            if config_changed {
                // Check if we need to reset the game (grid size, player count, food mode, max count, diagonal movement, or interpolation changed)
                let needs_reset = cfg.tron_width != width
                    || cfg.tron_height != height
                    || cfg.tron_num_players != num_players
                    || cfg.tron_food_mode != food_mode
                    || cfg.tron_food_max_count != food_max_count
                    || cfg.tron_diagonal_movement != diagonal_movement
                    || cfg.tron_interpolation != interpolation;

                // Update local vars
                width = cfg.tron_width;
                height = cfg.tron_height;
                speed_ms = cfg.tron_speed_ms;
                reset_delay_ms = cfg.tron_reset_delay_ms;
                look_ahead = cfg.tron_look_ahead;
                trail_length = cfg.tron_trail_length;
                ai_aggression = cfg.tron_ai_aggression;
                num_players = cfg.tron_num_players;
                food_mode = cfg.tron_food_mode;
                food_max_count = cfg.tron_food_max_count;
                food_ttl_seconds = cfg.tron_food_ttl_seconds;
                trail_fade = cfg.tron_trail_fade;
                super_food_enabled = cfg.tron_super_food_enabled;
                diagonal_movement = cfg.tron_diagonal_movement;
                interpolation = cfg.tron_interpolation.clone();
                player_colors = new_player_colors;

                // Update cached config
                if let Ok(mut cached) = config.lock() {
                    *cached = cfg;
                }

                if needs_reset {
                    // Reset game with new config
                    total_leds = width * height;
                    game = TronGame::new(width, height, speed_ms, look_ahead, trail_length, ai_aggression, num_players, &player_colors, food_mode, food_max_count, food_ttl_seconds, trail_fade, super_food_enabled, diagonal_movement, &interpolation);
                } else {
                    // Update game parameters without resetting
                    game.update_interval = Duration::from_secs_f64(speed_ms / 1000.0);
                    game.look_ahead = look_ahead;
                    game.trail_length = trail_length;
                    game.ai_aggression = ai_aggression;
                    game.food_ttl_seconds = food_ttl_seconds;
                    game.trail_fade = trail_fade;
                    game.super_food_enabled = super_food_enabled;

                    // Update player colors if they changed
                    if colors_changed {
                        // Parse interpolation mode
                        let interp_mode = match interpolation.as_str() {
                            "basis" => InterpolationMode::Basis,
                            "catmullrom" => InterpolationMode::CatmullRom,
                            _ => InterpolationMode::Linear,
                        };

                        for (i, player) in game.players.iter_mut().enumerate() {
                            let color_name = player_colors.get(i).map(|s| s.as_str()).unwrap_or("Rainbow");

                            // Resolve gradient name to hex colors
                            let hex_colors = gradients::resolve_color_string(color_name);

                            // If it's a single color, duplicate it to make a solid "gradient"
                            let hex_for_gradient = if !hex_colors.contains(',') {
                                format!("{},{}", hex_colors, hex_colors)
                            } else {
                                hex_colors.clone()
                            };

                            let (gradient_opt, _, _) = build_gradient_from_color(&hex_for_gradient, true, interp_mode).unwrap_or_else(|_e| {
                                // Fallback to rainbow if parsing fails
                                let fallback_hex = gradients::resolve_color_string("Rainbow");
                                build_gradient_from_color(&fallback_hex, true, interp_mode).unwrap()
                            });
                            player.gradient = gradient_opt.unwrap_or_else(|| {
                                // Fallback gradient if None (should not happen now)
                                colorgrad::CustomGradient::new()
                                    .html_colors(&["#ff0000", "#00ff00", "#0000ff"])
                                    .build()
                                    .unwrap()
                            });
                        }
                    }
                }
            }
        }

        // Update game state
        let updated = game.update();

        if updated {
            // Only render and send when game actually updated
            let frame = game.render(total_leds);

            // Send to WLED (multi-device or single device)
            if multi_device_enabled {
                if let Some(manager) = multi_device_manager.as_mut() {
                    if let Err(e) = manager.send_frame_with_brightness(&frame, Some(global_brightness)) {
                        eprintln!("Multi-device send error: {:?}", e);
                    }
                }
            } else {
                // Apply brightness to frame for single-device mode
                let frame_to_send: Vec<u8> = if global_brightness < 1.0 {
                    frame.iter().map(|&val| (val as f64 * global_brightness).round() as u8).collect()
                } else {
                    frame
                };

                if let Ok(mut client_guard) = ddp_client.lock() {
                    if let Some(conn) = client_guard.as_mut() {
                        let _ = conn.write(&frame_to_send);
                    }
                }
            }

            // If game over, wait and reset
            if game.is_game_over() {
                tokio::time::sleep(Duration::from_millis(reset_delay_ms)).await;
                game.reset(num_players, &player_colors);
            }
        } else {
            // Game didn't update yet - sleep briefly to avoid busy-waiting
            tokio::time::sleep(Duration::from_micros(100)).await;
        }
    }
}
