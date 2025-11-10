use rand::Rng;
use std::collections::HashMap;

/// Parse hex color string (with or without #) to RGB tuple
fn parse_hex_color(hex: &str) -> (u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
        (r, g, b)
    } else {
        (255, 255, 255) // Default to white on error
    }
}

/// Particle types in the falling sand simulation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Particle {
    Empty,
    Sand,
    Water,
    Stone,
    Fire,
    Smoke,
    Wood,
    Lava,
}

impl Particle {
    /// Returns true if this particle type falls due to gravity
    pub fn falls(&self) -> bool {
        matches!(self, Particle::Sand | Particle::Water | Particle::Lava | Particle::Wood | Particle::Stone | Particle::Fire)
    }

    /// Returns true if this particle type disperses horizontally
    pub fn disperses(&self) -> bool {
        matches!(self, Particle::Water | Particle::Lava)
    }

    /// Returns true if this particle type rises (like smoke)
    pub fn rises(&self) -> bool {
        matches!(self, Particle::Smoke)
    }

    /// Returns density (higher = heavier, used for displacement)
    pub fn density(&self) -> u8 {
        match self {
            Particle::Empty => 0,
            Particle::Smoke => 1,
            Particle::Fire => 2,
            Particle::Water => 10,
            Particle::Sand => 20,
            Particle::Lava => 25,
            Particle::Wood => 15,
            Particle::Stone => 30,
        }
    }

    /// Returns flammability (0 = fireproof, 255 = very flammable)
    pub fn flammability(&self) -> u8 {
        match self {
            Particle::Wood => 200,
            Particle::Sand => 0,
            Particle::Water => 0,
            Particle::Stone => 0,
            Particle::Fire => 0,
            Particle::Smoke => 0,
            Particle::Lava => 0,
            Particle::Empty => 0,
        }
    }
}

pub struct SandSimulation {
    width: usize,
    height: usize,
    grid: Vec<Particle>,
    velocity: Vec<(i8, i8)>, // Optional velocity tracking for particles
    fixed: Vec<bool>, // Track which cells are fixed obstacles (don't move)
    spawn_particle: Particle,
    spawn_rate: f32, // Probability of spawning per frame (0.0-1.0)
    spawn_radius: usize,
    spawn_x: usize, // X position where particles spawn (0 to width-1)
    fire_enabled: bool,
    colors: HashMap<Particle, (u8, u8, u8)>, // Custom colors for each particle type
}

impl SandSimulation {
    pub fn new(
        width: usize,
        height: usize,
        spawn_particle: Particle,
        spawn_rate: f32,
        spawn_radius: usize,
        spawn_x: usize,
        fire_enabled: bool,
        color_sand: &str,
        color_water: &str,
        color_stone: &str,
        color_fire: &str,
        color_smoke: &str,
        color_wood: &str,
        color_lava: &str,
    ) -> Self {
        let size = width * height;

        // Initialize color map
        let mut colors = HashMap::new();
        colors.insert(Particle::Empty, (0, 0, 0));
        colors.insert(Particle::Sand, parse_hex_color(color_sand));
        colors.insert(Particle::Water, parse_hex_color(color_water));
        colors.insert(Particle::Stone, parse_hex_color(color_stone));
        colors.insert(Particle::Fire, parse_hex_color(color_fire));
        colors.insert(Particle::Smoke, parse_hex_color(color_smoke));
        colors.insert(Particle::Wood, parse_hex_color(color_wood));
        colors.insert(Particle::Lava, parse_hex_color(color_lava));

        // Clamp spawn_x to valid range
        let spawn_x = spawn_x.min(if width > 0 { width - 1 } else { 0 });

        Self {
            width,
            height,
            grid: vec![Particle::Empty; size],
            velocity: vec![(0, 0); size],
            fixed: vec![false; size],
            spawn_particle,
            spawn_rate,
            spawn_radius,
            spawn_x,
            fire_enabled,
            colors,
        }
    }

    pub fn update_config(
        &mut self,
        spawn_particle: Particle,
        spawn_rate: f32,
        spawn_radius: usize,
        spawn_x: usize,
        fire_enabled: bool,
        color_sand: &str,
        color_water: &str,
        color_stone: &str,
        color_fire: &str,
        color_smoke: &str,
        color_wood: &str,
        color_lava: &str,
    ) {
        self.spawn_particle = spawn_particle;
        self.spawn_rate = spawn_rate.clamp(0.0, 1.0);
        self.spawn_radius = spawn_radius;
        self.spawn_x = spawn_x.min(if self.width > 0 { self.width - 1 } else { 0 });
        self.fire_enabled = fire_enabled;

        // Update colors
        self.colors.insert(Particle::Sand, parse_hex_color(color_sand));
        self.colors.insert(Particle::Water, parse_hex_color(color_water));
        self.colors.insert(Particle::Stone, parse_hex_color(color_stone));
        self.colors.insert(Particle::Fire, parse_hex_color(color_fire));
        self.colors.insert(Particle::Smoke, parse_hex_color(color_smoke));
        self.colors.insert(Particle::Wood, parse_hex_color(color_wood));
        self.colors.insert(Particle::Lava, parse_hex_color(color_lava));
    }

    fn get(&self, x: usize, y: usize) -> Particle {
        if x >= self.width || y >= self.height {
            return Particle::Stone; // Treat out of bounds as solid
        }
        self.grid[y * self.width + x]
    }

    fn set(&mut self, x: usize, y: usize, particle: Particle) {
        if x < self.width && y < self.height {
            self.grid[y * self.width + x] = particle;
        }
    }

    fn swap(&mut self, x1: usize, y1: usize, x2: usize, y2: usize) {
        if x1 >= self.width || y1 >= self.height || x2 >= self.width || y2 >= self.height {
            return;
        }
        // Don't swap if either particle is fixed
        if self.is_fixed(x1, y1) || self.is_fixed(x2, y2) {
            return;
        }
        let idx1 = y1 * self.width + x1;
        let idx2 = y2 * self.width + x2;
        self.grid.swap(idx1, idx2);
        self.velocity.swap(idx1, idx2);
    }

    /// Handle interactions between two adjacent particles
    /// Returns true if an interaction occurred (particles transformed)
    fn handle_particle_interaction(&mut self, x1: usize, y1: usize, x2: usize, y2: usize, rng: &mut impl Rng) -> bool {
        let p1 = self.get(x1, y1);
        let p2 = self.get(x2, y2);

        // Water + Lava -> Smoke (extinguish lava)
        if (p1 == Particle::Water && p2 == Particle::Lava) || (p1 == Particle::Lava && p2 == Particle::Water) {
            if rng.gen::<f32>() < 0.3 { // 30% chance
                // Turn lava into stone, water into smoke
                if p1 == Particle::Lava {
                    self.set(x1, y1, Particle::Stone);
                    self.set(x2, y2, Particle::Smoke);
                } else {
                    self.set(x1, y1, Particle::Smoke);
                    self.set(x2, y2, Particle::Stone);
                }
                return true;
            }
        }

        // Lava + Wood -> Fire (ignite wood)
        if (p1 == Particle::Lava && p2 == Particle::Wood) || (p1 == Particle::Wood && p2 == Particle::Lava) {
            if self.fire_enabled && rng.gen::<f32>() < 0.5 { // 50% chance
                // Turn wood into fire
                if p2 == Particle::Wood {
                    self.set(x2, y2, Particle::Fire);
                } else {
                    self.set(x1, y1, Particle::Fire);
                }
                return true;
            }
        }

        false
    }

    /// Spawn particles at the configured spawn position
    pub fn spawn_particles(&mut self) {
        let mut rng = rand::thread_rng();

        if rng.gen::<f32>() > self.spawn_rate {
            return; // Skip this frame
        }

        let spawn_y = 2; // Spawn near top

        // Spawn in a radius around spawn_x
        // Adjust for serpentine mapping so spawn_x represents physical position
        for dx in -(self.spawn_radius as i32)..=(self.spawn_radius as i32) {
            for dy in 0..=(self.spawn_radius as i32) {
                let y = spawn_y + dy as usize;

                if y >= self.height {
                    continue;
                }

                // Calculate x position, adjusting for serpentine on odd rows
                let mut x = (self.spawn_x as i32 + dx) as usize;

                // For odd rows, mirror the x coordinate to account for serpentine LED layout
                // This ensures spawn_x represents the physical/visual position
                if y % 2 == 1 && x < self.width {
                    x = self.width - 1 - x;
                }

                if x < self.width {
                    let dist_sq = (dx * dx + dy * dy) as f32;
                    let radius_sq = (self.spawn_radius * self.spawn_radius) as f32;

                    if dist_sq <= radius_sq && self.get(x, y) == Particle::Empty {
                        if rng.gen::<f32>() < 0.3 { // 30% chance per cell in radius
                            self.set(x, y, self.spawn_particle);
                        }
                    }
                }
            }
        }
    }

    /// Update simulation one step
    pub fn update(&mut self) {
        let mut rng = rand::thread_rng();

        // Process grid from bottom to top, randomizing left/right to avoid bias
        for y in (0..self.height).rev() {
            let x_order: Vec<usize> = if rng.gen::<bool>() {
                (0..self.width).collect()
            } else {
                (0..self.width).rev().collect()
            };

            for &x in &x_order {
                let particle = self.get(x, y);
                if particle == Particle::Empty {
                    continue;
                }

                // Skip fixed obstacles (they don't move)
                if self.is_fixed(x, y) {
                    continue;
                }

                // Handle particle behavior based on type
                if particle.falls() {
                    self.update_falling_particle(x, y, &mut rng);

                    // Fire-specific behavior (spreading and conversion to smoke)
                    if particle == Particle::Fire && self.fire_enabled {
                        self.update_fire(x, y, &mut rng);

                        // Fire converts to smoke over time
                        if self.get(x, y) == Particle::Fire && rng.gen::<f32>() < 0.05 {
                            self.set(x, y, Particle::Smoke);
                        }
                    }
                } else if particle.rises() {
                    self.update_rising_particle(x, y, &mut rng);
                }
            }
        }
    }

    fn update_falling_particle(&mut self, x: usize, y: usize, rng: &mut impl Rng) {
        let particle = self.get(x, y);

        // Try to fall down
        if y + 1 < self.height {
            let below = self.get(x, y + 1);

            // Handle particle interactions
            if self.handle_particle_interaction(x, y, x, y + 1, rng) {
                return; // Interaction occurred, skip normal movement
            }

            // Fall into empty space
            if below == Particle::Empty {
                self.swap(x, y, x, y + 1);
                return;
            }

            // Displace lighter particles (like water sinking through water)
            if particle.density() > below.density() {
                self.swap(x, y, x, y + 1);
                return;
            }

            // If can't fall, try to disperse horizontally (water/lava behavior)
            if particle.disperses() {
                let left_ok = x > 0 && self.get(x - 1, y + 1) == Particle::Empty;
                let right_ok = x + 1 < self.width && self.get(x + 1, y + 1) == Particle::Empty;

                if left_ok && right_ok {
                    // Randomly choose direction
                    if rng.gen::<bool>() {
                        self.swap(x, y, x - 1, y + 1);
                    } else {
                        self.swap(x, y, x + 1, y + 1);
                    }
                } else if left_ok {
                    self.swap(x, y, x - 1, y + 1);
                } else if right_ok {
                    self.swap(x, y, x + 1, y + 1);
                } else {
                    // Try moving sideways on same level
                    let left_same = x > 0 && self.get(x - 1, y) == Particle::Empty;
                    let right_same = x + 1 < self.width && self.get(x + 1, y) == Particle::Empty;

                    if left_same && right_same {
                        if rng.gen::<bool>() {
                            self.swap(x, y, x - 1, y);
                        } else {
                            self.swap(x, y, x + 1, y);
                        }
                    } else if left_same {
                        self.swap(x, y, x - 1, y);
                    } else if right_same {
                        self.swap(x, y, x + 1, y);
                    }
                }
            } else {
                // Sand - try diagonal slide
                let left_ok = x > 0 && self.get(x - 1, y + 1) == Particle::Empty;
                let right_ok = x + 1 < self.width && self.get(x + 1, y + 1) == Particle::Empty;

                if left_ok && right_ok {
                    if rng.gen::<bool>() {
                        self.swap(x, y, x - 1, y + 1);
                    } else {
                        self.swap(x, y, x + 1, y + 1);
                    }
                } else if left_ok {
                    self.swap(x, y, x - 1, y + 1);
                } else if right_ok {
                    self.swap(x, y, x + 1, y + 1);
                }
            }
        }
    }

    fn update_rising_particle(&mut self, x: usize, y: usize, rng: &mut impl Rng) {
        // Smoke rises
        if y > 0 {
            let above = self.get(x, y - 1);

            if above == Particle::Empty {
                self.swap(x, y, x, y - 1);
                return;
            }

            // Try diagonal rise
            let left_ok = x > 0 && self.get(x - 1, y - 1) == Particle::Empty;
            let right_ok = x + 1 < self.width && self.get(x + 1, y - 1) == Particle::Empty;

            if left_ok && right_ok {
                if rng.gen::<bool>() {
                    self.swap(x, y, x - 1, y - 1);
                } else {
                    self.swap(x, y, x + 1, y - 1);
                }
            } else if left_ok {
                self.swap(x, y, x - 1, y - 1);
            } else if right_ok {
                self.swap(x, y, x + 1, y - 1);
            }
        }

        // Smoke dissipates
        if self.get(x, y) == Particle::Smoke && rng.gen::<f32>() < 0.02 {
            self.set(x, y, Particle::Empty);
        }
    }

    fn update_fire(&mut self, x: usize, y: usize, rng: &mut impl Rng) {
        // Fire spreads to adjacent flammable materials
        let neighbors = [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ];

        for &(nx, ny) in &neighbors {
            if nx < self.width && ny < self.height {
                let neighbor = self.get(nx, ny);
                let flammability = neighbor.flammability();

                if flammability > 0 && rng.gen::<u8>() < flammability / 10 {
                    self.set(nx, ny, Particle::Fire);
                }
            }
        }
    }

    /// Render grid to RGB frame for LEDs
    pub fn render(&self, total_leds: usize) -> Vec<u8> {
        let mut frame = vec![0u8; total_leds * 3];

        // Map 2D grid to 1D LED strip (serpentine pattern)
        for y in 0..self.height {
            for x in 0..self.width {
                let particle = self.get(x, y);
                let (r, g, b) = self.colors.get(&particle).copied().unwrap_or((0, 0, 0));

                // Calculate LED index with serpentine mapping
                let led_idx = if y % 2 == 0 {
                    // Even rows go left to right
                    y * self.width + x
                } else {
                    // Odd rows go right to left
                    y * self.width + (self.width - 1 - x)
                };

                if led_idx < total_leds {
                    let pixel_idx = led_idx * 3;
                    frame[pixel_idx] = r;
                    frame[pixel_idx + 1] = g;
                    frame[pixel_idx + 2] = b;
                }
            }
        }

        frame
    }

    /// Clear the grid
    pub fn clear(&mut self) {
        self.grid.fill(Particle::Empty);
        self.velocity.fill((0, 0));
        self.fixed.fill(false);
    }

    /// Check if a cell is a fixed obstacle
    fn is_fixed(&self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        self.fixed[y * self.width + x]
    }

    /// Mark a cell as a fixed obstacle
    fn set_fixed(&mut self, x: usize, y: usize, fixed: bool) {
        if x < self.width && y < self.height {
            self.fixed[y * self.width + x] = fixed;
        }
    }

    /// Place random wood/stone obstacles in the bottom quarter of the grid
    pub fn place_obstacles(&mut self, enabled: bool, density: f32) {
        if !enabled {
            return;
        }

        let mut rng = rand::thread_rng();

        // Bottom 25% of grid
        let start_y = (self.height * 3) / 4;

        // Place obstacles randomly based on density
        for y in start_y..self.height {
            for x in 0..self.width {
                if rng.gen::<f32>() < density {
                    // Randomly choose between wood and stone
                    let particle = if rng.gen::<bool>() {
                        Particle::Wood
                    } else {
                        Particle::Stone
                    };

                    self.set(x, y, particle);
                    self.set_fixed(x, y, true);
                }
            }
        }
    }
}
