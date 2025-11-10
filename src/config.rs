// Config Module - Configuration management and command-line argument parsing
use anyhow::Result;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::gradients;

// Global storage for custom config path
static CUSTOM_CONFIG_PATH: OnceLock<Option<String>> = OnceLock::new();

/// Unified color resolution system for bandwidth and live modes
/// Returns (tx_color_resolved, rx_color_resolved) as comma-separated hex strings
/// Handles the logic: if tx_color is empty, use color; if rx_color is empty, use color
pub fn resolve_tx_rx_colors(config: &BandwidthConfig) -> (String, String) {
    // Resolve TX color (falls back to default color if empty)
    let tx_color_str = if !config.tx_color.is_empty() {
        gradients::resolve_color_string(&config.tx_color)
    } else {
        gradients::resolve_color_string(&config.color)
    };

    // Resolve RX color (falls back to default color if empty)
    let rx_color_str = if !config.rx_color.is_empty() {
        gradients::resolve_color_string(&config.rx_color)
    } else {
        gradients::resolve_color_string(&config.color)
    };

    (tx_color_str, rx_color_str)
}

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Real-time bandwidth visualization on WLED LED strips via DDP protocol",
    long_about = "Monitors network interface bandwidth and visualizes it in real-time on WLED LED strips.\n\
                  Upload traffic is displayed on LEDs 0-599, download traffic on LEDs 600-1199.\n\
                  Supports both linear and logarithmic scaling, custom color gradients, and remote gateway monitoring."
)]
pub struct Args {
    /// Maximum bandwidth in Gbps
    #[arg(short, long)]
    pub max: Option<f64>,

    /// LED colors (for both TX and RX unless overridden)
    #[arg(short, long)]
    pub color: Option<String>,

    /// TX LED colors
    #[arg(long)]
    pub tx_color: Option<String>,

    /// RX LED colors
    #[arg(long)]
    pub rx_color: Option<String>,

    /// Remote SSH host
    #[arg(short = 'H', long)]
    pub host: Option<String>,

    /// WLED device address
    #[arg(short, long)]
    pub wled_ip: Option<String>,

    /// Network interface to monitor
    #[arg(short = 'i', long = "int")]
    pub interface: Option<String>,

    /// Total number of LEDs
    #[arg(short = 'L', long)]
    pub leds: Option<usize>,

    /// LED fill direction mode
    #[arg(short = 'd', long)]
    pub direction: Option<String>,

    /// Swap TX and RX half assignments
    #[arg(short = 's', long)]
    pub swap: Option<bool>,

    /// Test mode
    #[arg(short = 't', long)]
    pub test: Option<String>,

    /// Quiet mode
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Visualization mode (bandwidth, midi, live, relay) - overrides --midi and --live flags
    #[arg(long)]
    pub mode: Option<String>,

    /// Enable MIDI mode (kept for backwards compatibility, use --mode=midi instead)
    #[arg(short = 'M', long)]
    pub midi: bool,

    /// MIDI device name (default: "IAC Bus 1" on macOS)
    #[arg(long)]
    pub midi_device: Option<String>,

    /// Shuffle the 12 primary colors randomly at launch
    #[arg(long)]
    pub midi_random_colors: bool,

    /// Live audio spectrum visualization mode (kept for backwards compatibility, use --mode=live instead)
    #[arg(long)]
    pub live: bool,

    /// Delay in milliseconds before sending to WLED (for audio/video sync)
    #[arg(long)]
    pub delay: Option<u64>,

    /// Audio test mode - test audio capture and show peak/RMS levels
    #[arg(long)]
    pub audio_test: bool,

    /// Target framerate (frames per second) for test mode and other modes
    #[arg(long)]
    pub fps: Option<f64>,

    /// Config file path or name (e.g., --cfg /full/path or --cfg myconf for ~/.config/rustwled/myconf.conf)
    #[arg(long)]
    pub cfg: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WLEDDeviceConfig {
    pub ip: String,
    pub led_offset: usize,
    pub led_count: usize,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BandwidthConfig {
    #[serde(skip)]
    pub config_path: Option<PathBuf>,  // Stores the config file path (not serialized)

    pub max_gbps: f64,
    pub color: String,
    pub tx_color: String,
    pub rx_color: String,
    pub direction: String,
    pub swap: bool,
    pub rx_split_percent: f64,
    pub strobe_on_max: bool,
    pub strobe_rate_hz: f64,
    pub strobe_duration_ms: f64,
    pub strobe_color: String,
    pub animation_speed: f64,
    pub scale_animation_speed: bool,
    pub tx_animation_direction: String,
    pub rx_animation_direction: String,
    pub interpolation_time_ms: f64,
    pub enable_interpolation: bool,  // Enable/disable bandwidth interpolation smoothing
    pub wled_ip: String,
    pub multi_device_enabled: bool,
    pub multi_device_send_parallel: bool,
    pub multi_device_fail_fast: bool,
    pub wled_devices: Vec<WLEDDeviceConfig>,
    pub interface: String,
    pub ssh_host: String,  // SSH host for remote bandwidth monitoring (empty = local)
    pub ssh_user: String,  // SSH user for remote bandwidth monitoring (empty = current user)
    pub total_leds: usize,
    pub use_gradient: bool,
    pub intensity_colors: bool,  // Map utilization/level to color position (all LEDs same color, changes with level)
    pub interpolation: String,
    pub fps: f64,
    pub ddp_delay_ms: f64,  // Delay in milliseconds before sending each DDP packet (for audio/LED sync)
    pub global_brightness: f64,  // Global brightness multiplier (0.0 to 1.0, default 1.0 = 100%)
    pub mode: String,  // Current mode: bandwidth, midi, live
    pub httpd_enabled: bool,
    pub httpd_https_enabled: bool,  // Enable HTTPS (uses same ip/port as HTTP)
    pub httpd_ip: String,
    pub httpd_port: u16,
    pub httpd_auth_enabled: bool,
    pub httpd_auth_user: String,
    pub httpd_auth_pass: String,
    pub test_tx: bool,
    pub test_rx: bool,
    pub test_tx_percent: f64,
    pub test_rx_percent: f64,
    pub midi_device: String,
    pub midi_gradient: bool,
    pub midi_random_colors: bool,
    pub midi_velocity_colors: bool,  // Map velocity to color spectrum (instead of note)
    pub midi_one_to_one: bool,  // Map 1 LED per note (centered at middle C) instead of spreading across all LEDs
    pub midi_channel_mode: bool,  // Use MIDI channels to map notes to LEDs (channel 1 = LEDs 0-127, channel 2 = LEDs 128-255, etc.)
    pub audio_device: String,  // Audio device name for live mode (empty = prompt user)
    pub audio_gain: f64,  // Audio input gain adjustment in percent (-200 to +200)
    pub log_scale: bool,
    pub attack_ms: f32,  // Time in ms for LEDs to fade in
    pub decay_ms: f32,   // Time in ms for LEDs to fade out
    pub vu: bool,  // VU meter mode for live audio (left/right channels)
    pub peak_hold: bool,  // Enable peak hold LED in VU meter mode
    pub peak_hold_duration_ms: f64,  // How long to hold the peak LED (in milliseconds)
    pub peak_hold_color: String,  // Hex color for peak hold LED
    pub peak_direction_toggle: bool,  // Toggle animation direction on new peak (VU mode with peak hold)
    pub spectrogram: bool,  // Spectrogram mode for live audio (scrolling frequency visualization)
    pub spectrogram_scroll_direction: String,  // Scroll direction: "right", "left", "up", "down" (default "right")
    pub spectrogram_scroll_speed: f64,  // Scroll speed in pixels per second (default 30.0)
    pub spectrogram_window_size: usize,  // FFT window size for spectrogram (default 1024)
    pub spectrogram_color_mode: String,  // Color mapping: "intensity", "frequency", "channel", "volume" (default "intensity")
    pub matrix_2d_enabled: bool,  // Enable 2D matrix output for spectrum visualization
    pub matrix_2d_width: usize,  // Width of 2D matrix in LEDs/pixels
    pub matrix_2d_height: usize,  // Height of 2D matrix in LEDs/pixels
    pub matrix_2d_gradient_direction: String,  // Gradient direction: "horizontal" (across frequencies) or "vertical" (across amplitude)
    pub relay_listen_ip: String,  // IP address to listen on for relay mode (default "127.0.0.1")
    pub relay_listen_port: u16,  // UDP listen port for relay mode (default 1234)
    pub relay_frame_width: usize,  // Frame width in pixels for relay mode (default 16)
    pub relay_frame_height: usize,  // Frame height in pixels for relay mode (default 16)
    pub webcam_frame_width: usize,  // Frame width in pixels for webcam mode (default 16)
    pub webcam_frame_height: usize,  // Frame height in pixels for webcam mode (default 16)
    pub webcam_target_fps: f64,  // Target FPS for webcam capture (default 30)
    pub webcam_brightness: f64,  // Brightness multiplier for webcam (0.0 to 2.0, default 0.5 for 50%)
    pub tron_width: usize,  // Tron game grid width (default 64)
    pub tron_height: usize,  // Tron game grid height (default 32)
    pub tron_speed_ms: f64,  // Tron game update speed in milliseconds (default 100ms, supports 0.01ms precision)
    pub tron_reset_delay_ms: u64,  // Delay before resetting game after game over (default 2000ms)
    pub tron_look_ahead: i32,  // How many steps AI looks ahead (default 8)
    pub tron_trail_length: usize,  // Max trail length, 0 = infinite (default 0)
    pub tron_ai_aggression: f64,  // AI aggressiveness 0.0-1.0 (default 0.3 = cautious)
    pub tron_num_players: usize,  // Number of AI players (1 = Snake, 2-8 = Tron, default 2)
    pub tron_food_mode: bool,  // Food mode: players compete to eat food and grow (default false)
    pub tron_food_max_count: usize,  // Maximum number of food items that can appear simultaneously (default 1)
    pub tron_food_ttl_seconds: u64,  // Food time-to-live in seconds before relocating (default 10)
    pub tron_trail_fade: bool,  // Enable trail brightness fading effect (default true)
    pub tron_super_food_enabled: bool,  // Enable super food spawning (red, 10% chance, +5 length)
    pub tron_power_food_enabled: bool,  // Enable power food spawning (yellow, 1% chance, 10 second power mode with immunity and 25% speed boost)
    pub tron_diagonal_movement: bool,  // Enable diagonal movement (8 directions instead of 4)
    pub tron_player_colors: String,  // Comma-separated list of gradients for players (e.g., "rainbow,fire,ocean") - DEPRECATED, use individual fields
    pub tron_player_1_color: String,  // Player 1 gradient/color
    pub tron_player_2_color: String,  // Player 2 gradient/color
    pub tron_player_3_color: String,  // Player 3 gradient/color
    pub tron_player_4_color: String,  // Player 4 gradient/color
    pub tron_player_5_color: String,  // Player 5 gradient/color
    pub tron_player_6_color: String,  // Player 6 gradient/color
    pub tron_player_7_color: String,  // Player 7 gradient/color
    pub tron_player_8_color: String,  // Player 8 gradient/color
    pub tron_animation_speed: f64,  // Speed of gradient animation on trails (0 = disabled)
    pub tron_scale_animation_speed: bool,  // Scale animation speed based on trail length
    pub tron_animation_direction: String,  // Animation direction: "forward" (head to tail) or "backward" (tail to head)
    pub tron_interpolation: String,  // Gradient interpolation mode: "linear", "basis", "catmullrom"
    pub tron_flip_direction_on_food: bool,  // Flip animation direction each time a player eats food
    pub geometry_grid_width: usize,  // Geometry mode grid width (default 64)
    pub geometry_grid_height: usize,  // Geometry mode grid height (default 32)
    pub geometry_mode_select: String,  // Which geometry to show: "cycle", "lissajous", "fibonacci", etc. (default "cycle")
    pub geometry_mode_duration_seconds: f64,  // How long to show each geometry in seconds (default 12.0)
    pub geometry_randomize_order: bool,  // Randomize the order geometries are shown (default false)

    // Boid simulation parameters
    pub boid_count: usize,  // Number of boids (default 50)
    pub boid_separation_distance: f64,  // Separation distance (default 0.1)
    pub boid_alignment_distance: f64,  // Alignment distance (default 0.3)
    pub boid_cohesion_distance: f64,  // Cohesion distance (default 0.3)
    pub boid_max_speed: f64,  // Maximum speed (default 0.03)
    pub boid_max_force: f64,  // Maximum steering force (default 0.001)
    // Predator-prey settings
    pub boid_predator_enabled: bool,  // Enable predator-prey behavior (default false)
    pub boid_predator_count: usize,  // Number of predators (default 3)
    pub boid_predator_speed: f64,  // Predator maximum speed (default 0.04)
    pub boid_avoidance_distance: f64,  // Distance at which prey avoid predators (default 0.4)
    pub boid_chase_force: f64,  // Force applied to predator chase (default 0.002)

    // Falling sand simulation parameters
    pub sand_grid_width: usize,  // Sand grid width (default 64)
    pub sand_grid_height: usize,  // Sand grid height (default 32)
    pub sand_spawn_enabled: bool,  // Enable spawning particles (default true)
    pub sand_particle_type: String,  // Particle type to spawn: sand, water, stone, fire, wood, lava (default "sand")
    pub sand_spawn_rate: f64,  // Spawn rate 0.0-1.0 (default 0.3)
    pub sand_spawn_radius: usize,  // Spawn radius in cells (default 3)
    pub sand_spawn_x: usize,  // Spawn X position in cells (default width/2)
    pub sand_obstacles_enabled: bool,  // Place random obstacles in bottom quarter (default false)
    pub sand_obstacle_density: f64,  // Obstacle density 0.0-1.0 (default 0.15)
    pub sand_fire_enabled: bool,  // Enable fire spread (default true)
    pub sand_color_sand: String,  // Color for sand particles (default "C2B280" - tan)
    pub sand_color_water: String,  // Color for water particles (default "0077BE" - blue)
    pub sand_color_stone: String,  // Color for stone particles (default "808080" - gray)
    pub sand_color_fire: String,  // Color for fire particles (default "FF4500" - orange-red)
    pub sand_color_smoke: String,  // Color for smoke particles (default "404040" - dark gray)
    pub sand_color_wood: String,  // Color for wood particles (default "8B4513" - saddle brown)
    pub sand_color_lava: String,  // Color for lava particles (default "FF8C00" - dark orange)
}

impl Default for BandwidthConfig {
    fn default() -> Self {
        BandwidthConfig {
            config_path: None,
            max_gbps: 10.0,
            color: "0099FF".to_string(),
            tx_color: "".to_string(),
            rx_color: "".to_string(),
            direction: "mirrored".to_string(),
            swap: false,
            rx_split_percent: 50.0,
            strobe_on_max: false,
            strobe_rate_hz: 3.0,
            strobe_duration_ms: 166.0,
            strobe_color: "FFFFFF".to_string(),  // White flash for strobe effect
            animation_speed: 1.0,
            scale_animation_speed: false,
            tx_animation_direction: "right".to_string(),
            rx_animation_direction: "left".to_string(),
            interpolation_time_ms: 1000.0,
            enable_interpolation: true,
            wled_ip: "led.local".to_string(),
            multi_device_enabled: false,
            multi_device_send_parallel: true,
            multi_device_fail_fast: false,
            wled_devices: vec![
                WLEDDeviceConfig {
                    ip: "led.local".to_string(),
                    led_offset: 0,
                    led_count: 100,
                    enabled: true,
                }
            ],
            interface: "en0".to_string(),
            ssh_host: "".to_string(),  // Empty = local monitoring
            ssh_user: "".to_string(),  // Empty = current user
            total_leds: 1200,
            use_gradient: true,
            intensity_colors: false,  // Default to spatial gradient mode
            interpolation: "linear".to_string(),
            fps: 60.0,
            ddp_delay_ms: 0.0,  // No delay by default
            global_brightness: 1.0,  // Default to 100% brightness
            mode: "bandwidth".to_string(),  // Default to bandwidth meter mode
            httpd_enabled: true,
            httpd_https_enabled: false,  // Disabled by default
            httpd_ip: "localhost".to_string(),
            httpd_port: 8080,
            httpd_auth_enabled: false,
            httpd_auth_user: "".to_string(),
            httpd_auth_pass: "".to_string(),
            test_tx: false,
            test_rx: false,
            test_tx_percent: 100.0,
            test_rx_percent: 100.0,
            midi_device: "IAC Bus 1".to_string(),
            midi_gradient: false,
            midi_random_colors: false,
            midi_velocity_colors: false,
            midi_one_to_one: false,
            midi_channel_mode: false,
            audio_device: "".to_string(),  // Empty = prompt user on first run
            audio_gain: 0.0,  // No gain adjustment by default
            log_scale: false,
            attack_ms: 10.0,   // 10ms fast attack for responsive feel
            decay_ms: 150.0,   // 150ms decay so you can see the notes/hits
            vu: false,
            peak_hold: false,
            peak_hold_duration_ms: 1000.0,  // 1 second hold by default
            peak_hold_color: "FFFFFF".to_string(),  // White peak hold LED
            peak_direction_toggle: false,  // Disabled by default
            spectrogram: false,  // Spectrogram mode disabled by default
            spectrogram_scroll_direction: "right".to_string(),  // Default scroll right (time flows left to right)
            spectrogram_scroll_speed: 30.0,  // Default 30 pixels per second
            spectrogram_window_size: 1024,  // Default 1024 sample window for good frequency resolution
            spectrogram_color_mode: "intensity".to_string(),  // Default to intensity-based coloring
            matrix_2d_enabled: false,  // Disabled by default - use 1D strip mode
            matrix_2d_width: 16,  // Default 16x16 matrix
            matrix_2d_height: 16,
            matrix_2d_gradient_direction: "horizontal".to_string(),  // Default to horizontal gradient (across frequencies)
            relay_listen_ip: "127.0.0.1".to_string(),  // Default to localhost
            relay_listen_port: 1234,  // Default UDP listen port for relay mode
            relay_frame_width: 16,  // Default 16x16 frame
            relay_frame_height: 16,
            webcam_frame_width: 16,  // Default 16x16 webcam capture
            webcam_frame_height: 16,
            webcam_target_fps: 30.0,  // Default 30 FPS for webcam
            webcam_brightness: 0.5,  // Default 50% brightness to avoid washout
            tron_width: 64,  // Default 64x32 grid for Tron game
            tron_height: 32,
            tron_speed_ms: 100.0,  // Default 100ms update interval (10 FPS game speed)
            tron_reset_delay_ms: 2000,  // Default 2 second delay before resetting after game over
            tron_look_ahead: 8,  // Look 8 steps ahead
            tron_trail_length: 0,  // Infinite trail by default
            tron_ai_aggression: 0.3,  // 30% aggression (cautious play)
            tron_num_players: 2,  // Default 2 players
            tron_food_mode: false,  // Food mode disabled by default
            tron_food_max_count: 1,  // Default 1 food at a time
            tron_food_ttl_seconds: 10,  // Default 10 seconds before food relocates
            tron_trail_fade: true,  // Trail fading enabled by default
            tron_super_food_enabled: true,  // Super food enabled by default
            tron_power_food_enabled: true,  // Power food enabled by default
            tron_diagonal_movement: false,  // Diagonal movement disabled by default
            tron_player_colors: "rainbow,fire".to_string(),  // Default colors (deprecated)
            tron_player_1_color: "rainbow".to_string(),
            tron_player_2_color: "fire".to_string(),
            tron_player_3_color: "ocean".to_string(),
            tron_player_4_color: "forest".to_string(),
            tron_player_5_color: "sunset".to_string(),
            tron_player_6_color: "purple".to_string(),
            tron_player_7_color: "cool".to_string(),
            tron_player_8_color: "warm".to_string(),
            tron_animation_speed: 1.0,  // Default animation speed
            tron_scale_animation_speed: false,  // Don't scale by default
            tron_animation_direction: "forward".to_string(),  // Head to tail direction
            tron_interpolation: "catmullrom".to_string(),  // Smooth interpolation by default
            tron_flip_direction_on_food: false,  // Disabled by default
            geometry_grid_width: 64,  // Default 64x32 grid
            geometry_grid_height: 32,
            geometry_mode_select: "cycle".to_string(),  // Cycle through all modes by default
            geometry_mode_duration_seconds: 12.0,  // 12 seconds per mode
            geometry_randomize_order: false,  // Sequential order by default

            // Boid simulation defaults
            boid_count: 50,
            boid_separation_distance: 0.1,
            boid_alignment_distance: 0.3,
            boid_cohesion_distance: 0.3,
            boid_max_speed: 0.03,
            boid_max_force: 0.001,
            boid_predator_enabled: false,
            boid_predator_count: 3,
            boid_predator_speed: 0.04,
            boid_avoidance_distance: 0.4,
            boid_chase_force: 0.002,

            // Falling sand defaults
            sand_grid_width: 64,
            sand_grid_height: 32,
            sand_spawn_enabled: true,
            sand_particle_type: "sand".to_string(),
            sand_spawn_rate: 0.3,
            sand_spawn_radius: 3,
            sand_spawn_x: 32,  // Default to center (width/2)
            sand_obstacles_enabled: false,
            sand_obstacle_density: 0.15,
            sand_fire_enabled: true,
            sand_color_sand: "C2B280".to_string(),
            sand_color_water: "0077BE".to_string(),
            sand_color_stone: "808080".to_string(),
            sand_color_fire: "FF4500".to_string(),
            sand_color_smoke: "404040".to_string(),
            sand_color_wood: "8B4513".to_string(),
            sand_color_lava: "FF8C00".to_string(),
        }
    }
}

impl BandwidthConfig {
    pub fn merge_with_args(&mut self, args: &Args) -> bool {
        // Track if any args were actually provided
        let mut args_provided = false;

        // Handle mode setting - explicit --mode flag takes precedence, then --midi/--live flags
        if let Some(ref mode) = args.mode {
            self.mode = mode.clone();
            args_provided = true;
        } else if args.midi {
            self.mode = "midi".to_string();
            args_provided = true;
        } else if args.live {
            self.mode = "live".to_string();
            args_provided = true;
        }

        // Only override config values if explicitly specified on command line
        if let Some(ref color) = args.color {
            self.color = color.clone();
            args_provided = true;
            // If -c is specified but --tx_color and --rx_color are not, clear them
            if args.tx_color.is_none() {
                self.tx_color = "".to_string();
            }
            if args.rx_color.is_none() {
                self.rx_color = "".to_string();
            }
        }

        // Individual TX/RX colors only set if explicitly specified
        if let Some(ref tx_color) = args.tx_color {
            self.tx_color = tx_color.clone();
            args_provided = true;
        }

        if let Some(ref rx_color) = args.rx_color {
            self.rx_color = rx_color.clone();
            args_provided = true;
        }

        if let Some(max) = args.max {
            self.max_gbps = max;
            args_provided = true;
        }

        if let Some(ref direction) = args.direction {
            self.direction = direction.clone();
            args_provided = true;
        }

        if let Some(ref wled_ip) = args.wled_ip {
            self.wled_ip = wled_ip.clone();
            args_provided = true;
        }

        if let Some(ref interface) = args.interface {
            self.interface = interface.clone();
            args_provided = true;
        }

        if let Some(leds) = args.leds {
            self.total_leds = leds;
            args_provided = true;
        }

        if let Some(swap) = args.swap {
            self.swap = swap;
            args_provided = true;
        }

        if let Some(ref midi_device) = args.midi_device {
            self.midi_device = midi_device.clone();
            args_provided = true;
        }

        if let Some(fps) = args.fps {
            self.fps = fps;
            args_provided = true;
        }

        args_provided
    }

    /// Set the global config path (called once at startup)
    pub fn set_config_path(cfg: Option<String>) {
        let _ = CUSTOM_CONFIG_PATH.set(cfg);
    }

    /// Get the global config path (if set)
    fn get_config_path_arg() -> Option<&'static str> {
        CUSTOM_CONFIG_PATH.get()
            .and_then(|opt| opt.as_deref())
    }

    pub fn config_path(cfg_arg: Option<&str>) -> Result<PathBuf> {
        // Priority: explicit arg > global > None
        let cfg = cfg_arg.or_else(|| Self::get_config_path_arg());

        if let Some(cfg) = cfg {
            // Check if it's an absolute path
            let path = PathBuf::from(cfg);
            if path.is_absolute() {
                return Ok(path);
            }

            // Check if it contains path separators (relative path)
            if cfg.contains('/') || cfg.contains('\\') {
                return Ok(path);
            }

            // Otherwise treat as config name in config directory
            let home = std::env::var("HOME")?;
            let config_dir = PathBuf::from(home).join(".config").join("rustwled");
            std::fs::create_dir_all(&config_dir)?;

            // Add .conf extension if not present
            let filename = if cfg.ends_with(".conf") {
                cfg.to_string()
            } else {
                format!("{}.conf", cfg)
            };

            Ok(config_dir.join(filename))
        } else {
            // Default config path
            let home = std::env::var("HOME")?;
            let config_dir = PathBuf::from(home).join(".config").join("rustwled");
            std::fs::create_dir_all(&config_dir)?;
            Ok(config_dir.join("config.conf"))
        }
    }

    pub fn load_with_path(cfg_arg: Option<&str>) -> Result<Self> {
        let path = Self::config_path(cfg_arg)?;
        let contents = std::fs::read_to_string(&path)?;
        let mut parsed: Self = toml::from_str(&contents)?;
        parsed.config_path = Some(path);
        parsed.sanitize();

        // Auto-migrate: If wled_devices is empty but wled_ip exists, create device[0]
        if parsed.wled_devices.is_empty() && !parsed.wled_ip.is_empty() {
            eprintln!("Migrating wled_ip to multi-device config (device 0)");
            parsed.wled_devices.push(WLEDDeviceConfig {
                ip: parsed.wled_ip.clone(),
                led_offset: 0,
                led_count: parsed.total_leds,
                enabled: true,
            });
            // Save the migrated config
            let _ = parsed.save();
        }

        // Auto-calculate total_leds from multi-device config if devices exist
        if !parsed.wled_devices.is_empty() {
            let calculated_total = parsed.wled_devices.iter()
                .filter(|d| d.enabled)
                .map(|d| d.led_offset + d.led_count)
                .max()
                .unwrap_or(parsed.total_leds);

            // Always use calculated value, update silently in memory only
            parsed.total_leds = calculated_total;
        }

        Ok(parsed)
    }

    /// Sanitize config values to handle common formatting issues
    pub fn sanitize(&mut self) {
        // Sanitize color values (remove trailing commas, extra whitespace)
        self.color = Self::sanitize_color_string(&self.color);
        self.tx_color = Self::sanitize_color_string(&self.tx_color);
        self.rx_color = Self::sanitize_color_string(&self.rx_color);
        self.strobe_color = Self::sanitize_color_string(&self.strobe_color);
        self.peak_hold_color = Self::sanitize_color_string(&self.peak_hold_color);

        // Sanitize string values (trim whitespace)
        self.wled_ip = self.wled_ip.trim().to_string();
        self.interface = self.interface.trim().to_string();
        self.ssh_host = self.ssh_host.trim().to_string();
        self.ssh_user = self.ssh_user.trim().to_string();
        self.direction = self.direction.trim().to_lowercase();
        self.tx_animation_direction = self.tx_animation_direction.trim().to_lowercase();
        self.rx_animation_direction = self.rx_animation_direction.trim().to_lowercase();
        self.interpolation = self.interpolation.trim().to_lowercase();
        self.mode = self.mode.trim().to_lowercase();
        self.httpd_ip = self.httpd_ip.trim().to_string();
        self.httpd_auth_user = self.httpd_auth_user.trim().to_string();
        self.midi_device = self.midi_device.trim().to_string();
        self.audio_device = self.audio_device.trim().to_string();
        self.relay_listen_ip = self.relay_listen_ip.trim().to_string();

        // Clamp numeric values to reasonable ranges
        self.max_gbps = self.max_gbps.max(0.1).min(400.0);
        self.total_leds = self.total_leds.max(1).min(100000);
        self.fps = self.fps.max(1.0).min(500.0);
        self.ddp_delay_ms = self.ddp_delay_ms.max(0.0).min(10000.0);
        self.global_brightness = self.global_brightness.max(0.0).min(1.0);
        self.rx_split_percent = self.rx_split_percent.max(0.0).min(100.0);
        self.strobe_rate_hz = self.strobe_rate_hz.max(0.0).min(100.0);
        self.strobe_duration_ms = self.strobe_duration_ms.max(0.0).min(10000.0);
        self.animation_speed = self.animation_speed.max(0.0).min(100.0);
        self.interpolation_time_ms = self.interpolation_time_ms.max(0.0).min(10000.0);
        self.httpd_port = self.httpd_port.max(1).min(65535);
        self.test_tx_percent = self.test_tx_percent.max(0.0).min(101.0);
        self.test_rx_percent = self.test_rx_percent.max(0.0).min(101.0);
        self.attack_ms = self.attack_ms.max(0.0).min(10000.0);
        self.decay_ms = self.decay_ms.max(0.0).min(10000.0);
        self.peak_hold_duration_ms = self.peak_hold_duration_ms.max(0.0).min(10000.0);
        self.audio_gain = self.audio_gain.max(-200.0).min(200.0);
        self.relay_listen_port = self.relay_listen_port.max(1).min(65535);
        self.relay_frame_width = self.relay_frame_width.max(1).min(10000);
        self.relay_frame_height = self.relay_frame_height.max(1).min(10000);
        self.webcam_frame_width = self.webcam_frame_width.max(1).min(10000);
        self.webcam_frame_height = self.webcam_frame_height.max(1).min(10000);
        self.webcam_target_fps = self.webcam_target_fps.max(1.0).min(120.0);
        self.webcam_brightness = self.webcam_brightness.max(0.0).min(2.0);
        self.tron_width = self.tron_width.max(8).min(256);
        self.tron_height = self.tron_height.max(8).min(256);
        self.tron_speed_ms = self.tron_speed_ms.max(5.0).min(10000.0);
        self.tron_reset_delay_ms = self.tron_reset_delay_ms.max(0).min(10000);
        self.tron_look_ahead = self.tron_look_ahead.max(1).min(128);
        self.tron_trail_length = self.tron_trail_length.min(10000);  // 0 is valid (infinite)
        self.tron_ai_aggression = self.tron_ai_aggression.max(0.0).min(1.0);
        self.tron_num_players = self.tron_num_players.max(1).min(8);  // 1 = Snake mode
        self.tron_food_max_count = self.tron_food_max_count.max(1).min(100);  // 1-100 food items
        self.tron_food_ttl_seconds = self.tron_food_ttl_seconds.max(1).min(300);  // 1-300 seconds
        self.tron_player_colors = Self::sanitize_color_string(&self.tron_player_colors);
        self.tron_player_1_color = Self::sanitize_color_string(&self.tron_player_1_color);
        self.tron_player_2_color = Self::sanitize_color_string(&self.tron_player_2_color);
        self.tron_player_3_color = Self::sanitize_color_string(&self.tron_player_3_color);
        self.tron_player_4_color = Self::sanitize_color_string(&self.tron_player_4_color);
        self.tron_player_5_color = Self::sanitize_color_string(&self.tron_player_5_color);
        self.tron_player_6_color = Self::sanitize_color_string(&self.tron_player_6_color);
        self.tron_player_7_color = Self::sanitize_color_string(&self.tron_player_7_color);
        self.tron_player_8_color = Self::sanitize_color_string(&self.tron_player_8_color);
        self.tron_animation_speed = self.tron_animation_speed.max(0.0).min(100.0);
        self.tron_animation_direction = self.tron_animation_direction.trim().to_lowercase();
        self.tron_interpolation = self.tron_interpolation.trim().to_lowercase();
    }

    /// Sanitize a color string (hex colors or comma-separated list)
    /// Removes trailing commas, leading commas, extra whitespace, and invalid characters
    fn sanitize_color_string(color: &str) -> String {
        // Trim and convert to uppercase for consistency
        let trimmed = color.trim().to_uppercase();

        if trimmed.is_empty() {
            return trimmed;
        }

        // Check if this is a gradient preset name (contains letters beyond hex)
        let has_non_hex = trimmed.chars().any(|c| {
            !c.is_ascii_hexdigit() && c != ',' && !c.is_whitespace()
        });

        if has_non_hex {
            // This is likely a gradient preset name, just trim and return
            return color.trim().to_string();
        }

        // This is hex colors - sanitize the comma-separated list
        let colors: Vec<String> = trimmed
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .filter(|s| s.chars().all(|c| c.is_ascii_hexdigit()))
            .map(|s| s.to_string())
            .collect();

        colors.join(",")
    }

    pub fn load() -> Result<Self> {
        Self::load_with_path(None)
    }

    pub fn save(&self) -> Result<()> {
        let path = self.config_path.clone()
            .unwrap_or_else(|| Self::config_path(None).unwrap());

        // Sanitize values before saving
        let mut sanitized = self.clone();
        sanitized.sanitize();

        // Sync: Keep device[0] in sync with wled_ip/total_leds for backwards compat
        if !sanitized.wled_devices.is_empty() {
            sanitized.wled_ip = sanitized.wled_devices[0].ip.clone();
            sanitized.total_leds = sanitized.wled_devices[0].led_count;
        }

        // Build TOML with comments manually for better documentation
        let mut contents = format!(
            r#"# RustWLED Configuration File
# Edit this file while the program is running to change settings in real-time
# Note: All changes apply automatically without restart

# Maximum bandwidth in Gbps for visualization scaling
max_gbps = {}

# Default LED color (hex, applies to both TX and RX if not overridden)
# Can be single color: "FF0000" or gradient: "FF0000,00FF00,0000FF"
color = "{}"

# TX (upload) LED colors (hex, overrides 'color' setting)
# Can be single color: "FF0000" or gradient: "FF0000,00FF00,0000FF"
tx_color = "{}"

# RX (download) LED colors (hex, overrides 'color' setting)
# Can be single color: "0000FF" or gradient: "0000FF,00FFFF,00FF00"
rx_color = "{}"

# LED fill direction mode
# Options: "mirrored", "opposing", "left", "right"
direction = "{}"

# Swap TX and RX half assignments
# Options: true, false
swap = {}

# RX/TX LED split percentage
# Percentage of total LEDs allocated to RX (0-100), TX gets the remainder
# Example: 50.0 = 50/50 split, 70.0 = 70/30 split (RX/TX)
rx_split_percent = {}

# Strobe entire RX or TX segment when bandwidth exceeds max
# When enabled, the entire segment will flash on/off when at max utilization
strobe_on_max = {}

# Strobe rate in Hz (flashes per second)
# Controls how fast the strobe flashes when at max bandwidth
strobe_rate_hz = {}

# Strobe duration in milliseconds
# How long the strobe color is displayed (cannot exceed half the strobe cycle time)
# Example: 3 Hz = 333ms cycle, so max duration is 166ms
strobe_duration_ms = {}

# Strobe color in hex (color to display during strobe "off" phase)
# Default is "000000" (black/off). Can be any hex color like "FF0000" for red
strobe_color = "{}"

# Animation speed in LEDs per frame (0.0 = disabled, 1.0 = 60 LEDs/sec)
# Controls how fast gradients travel along the strip
animation_speed = {}

# Scale animation speed based on bandwidth utilization
# When enabled, speed scales from 0.0 (no traffic) to animation_speed (max bandwidth)
# Options: true, false
scale_animation_speed = {}

# TX (upload) animation direction
# Options: "left", "right"
tx_animation_direction = "{}"

# RX (download) animation direction
# Options: "left", "right"
rx_animation_direction = "{}"

# Bandwidth interpolation time in milliseconds
# Smoothly transitions between bandwidth readings over this time period
# Higher values = smoother but more laggy, lower values = more responsive but jittery
interpolation_time_ms = {}

# Enable bandwidth interpolation smoothing
# Options: true (smooth transitions), false (instant response)
enable_interpolation = {}

# WLED device IP address or hostname
wled_ip = "{}"

# Multi-Device Support - Send frames directly to multiple WLED controllers
# Eliminates relay lag/tearing by avoiding the need for one controller to relay to another
multi_device_enabled = {}

# Send to devices in parallel (faster) vs sequential (more reliable)
multi_device_send_parallel = {}

# Stop all devices if one fails (true) or continue with working devices (false)
multi_device_fail_fast = {}

# Network interface to monitor
# Can be single interface "eth0" or combined with comma "eth0,eth1"
interface = "{}"

# SSH host for remote bandwidth monitoring (empty = local monitoring)
# Example: "192.168.1.100" or "server.local"
ssh_host = "{}"

# SSH user for remote bandwidth monitoring (empty = current user)
# Example: "myuser"
ssh_user = "{}"

# Total number of LEDs in the strip (can be changed while running)
# TX uses first half (0-N/2), RX uses second half (N/2-N)
total_leds = {}

# Use gradient blending between colors
# Options: true (smooth gradients), false (hard color segments)
use_gradient = {}

# Intensity Colors Mode - Map utilization/level to gradient color (bandwidth & VU modes)
# When enabled, all LEDs show the same color that changes based on level/utilization
# 0% = first color, 50% = middle color, 100% = last color in gradient
# Animation speed/direction are disabled in this mode
# Options: true (intensity mode), false (spatial gradient mode)
intensity_colors = {}

# Gradient interpolation mode (only applies when use_gradient = true)
# Options: "linear" (sharp), "basis" (smooth B-spline), "catmullrom" (smooth Catmull-Rom)
interpolation = "{}"

# Rendering frame rate (can be changed while running)
# Try different values like 30, 60, 120, 144 to reduce stuttering
fps = {}

# DDP packet delay in milliseconds (for audio/LED synchronization)
# Add delay before sending each DDP packet if LEDs are ahead of audio
# Example: 10.0 = 10ms delay, 0.0 = no delay (default)
ddp_delay_ms = {}

# Global brightness multiplier (0.0 to 1.0)
# Applies to all RGB values before sending to WLED
# 1.0 = 100% (full brightness), 0.5 = 50%, 0.0 = off
# Set WLED's brightness to 255 (100%) and control brightness from here
global_brightness = {}

# Mode - Current visualization mode (changes apply immediately without restart)
# Options: "bandwidth" (network traffic), "midi" (MIDI input), "live" (audio visualization)
mode = "{}"

# HTTP server configuration
# Enable or disable the built-in web configuration interface
httpd_enabled = {}

# Enable HTTPS (runs on same IP/port as HTTP, but with TLS)
# SSL certificate is auto-generated using httpd_ip as the hostname
# Browser will show security warning for self-signed cert - click "Proceed Anyway"
# Options: true, false
httpd_https_enabled = {}

# IP address for the HTTP/HTTPS server to listen on
# Also used as the hostname for SSL certificate generation when HTTPS is enabled
# Use "0.0.0.0" to listen on all interfaces, or "127.0.0.1" for localhost only
httpd_ip = "{}"

# Port for the HTTP/HTTPS server to listen on
httpd_port = {}

# Enable HTTP Basic Authentication
# Options: true, false
httpd_auth_enabled = {}

# HTTP Basic Auth username (only used when httpd_auth_enabled = true)
httpd_auth_user = "{}"

# HTTP Basic Auth password (only used when httpd_auth_enabled = true)
httpd_auth_pass = "{}"

# Test Mode - Simulate TX (upload) bandwidth at maximum utilization
# Options: true, false
test_tx = {}

# Test Mode - Simulate RX (download) bandwidth at maximum utilization
# Options: true, false
test_rx = {}

# Test Mode - TX bandwidth utilization percentage (0-100)
# Controls how much of max bandwidth to simulate for TX when test_tx is enabled
test_tx_percent = {}

# Test Mode - RX bandwidth utilization percentage (0-100)
# Controls how much of max bandwidth to simulate for RX when test_rx is enabled
test_rx_percent = {}

# MIDI Mode - MIDI input device name
# Default: "IAC Bus 1" on macOS
# Use --midi flag to enable MIDI mode
midi_device = "{}"

# MIDI Mode - Enable gradient blending for multiple notes
# Options: true (gradient spanning note range), false (each note lights its own segment)
midi_gradient = {}

# MIDI Mode - Randomize note color assignments at launch
# Options: true (shuffles 12 primary colors), false (uses default color-to-note mapping)
midi_random_colors = {}

# MIDI Mode - Map velocity to color spectrum (instead of note number)
# When enabled, velocity controls both color and brightness across full spectrum
# Options: true (velocity → color), false (note → color)
midi_velocity_colors = {}

# MIDI Mode - Use 1-to-1 note-to-LED mapping centered at middle C
# When enabled, each note gets 1 LED, middle C (note 60) is at center, wraps at edges
# When disabled, all 128 notes are spread across total_leds
# Options: true (1 LED per note), false (spread across all LEDs)
midi_one_to_one = {}

# MIDI Channel Mode - Use MIDI channels to map notes to different LED sections
# Channel 1 controls LEDs 0-127, Channel 2 controls LEDs 128-255, etc.
# Allows addressing up to 16 * 128 = 2048 individual LEDs with unique notes
# Options: true (use channels), false (ignore channels)
midi_channel_mode = {}

# Audio Device - Audio input device name for live mode
# Leave empty to be prompted on first run, or set to a device name to use it automatically
# Example: "BlackHole 2ch" or "MacBook Pro Microphone"
audio_device = "{}"

# Audio Gain - Audio input gain adjustment in percent (-100 to +100)
# Positive values boost the signal, negative values reduce it
# 0 = no change, +100 = double amplitude, -100 = muted
# Example: 50 (50% boost), -20 (20% reduction)
audio_gain = {}

# Log Scale - Use logarithmic scaling for bandwidth visualization
# Options: true, false
log_scale = {}

# Attack (ms) - Time in milliseconds for LEDs to fade IN (applies to MIDI and Live modes)
# Lower = faster/snappier response, Higher = smoother/slower fade-in
attack_ms = {}

# Decay (ms) - Time in milliseconds for LEDs to fade OUT (applies to MIDI and Live modes)
# Lower = faster fade-out, Higher = LEDs stay visible longer
decay_ms = {}

# VU Meter Mode - Classic digital VU meter for live audio (left/right channels)
# When enabled in --live mode, LEDs are split in half: first half = left channel, second half = right channel
# Reuses color gradients, direction, and animation settings from bandwidth mode
# Options: true, false
vu = {}

# Peak Hold - Enable peak hold LED in VU meter mode
# When enabled, a single LED will remain lit at the peak position for the specified duration
# Options: true, false
peak_hold = {}

# Peak Hold Duration (ms) - How long the peak LED stays lit before fading
# Higher values = longer hold time
peak_hold_duration_ms = {}

# Peak Hold Color - Hex color for the peak hold LED (e.g., FF0000 for red, FFFFFF for white)
peak_hold_color = "{}"

# Peak Direction Toggle - Toggle animation direction on new peak (VU mode with peak hold)
# When enabled, animation direction changes each time a new peak is held
# Options: true, false
peak_direction_toggle = {}

# Spectrogram Mode - Enable scrolling spectrogram visualization (live mode only)
# Displays a scrolling frequency waterfall like FFmpeg showspec or Winamp voiceprint
# Options: true, false
spectrogram = {}

# Spectrogram Scroll Direction - Direction the spectrogram scrolls
# Options: "right" (time flows left-to-right), "left" (right-to-left), "up" (bottom-to-top), "down" (top-to-bottom)
spectrogram_scroll_direction = "{}"

# Spectrogram Scroll Speed - How fast the spectrogram scrolls (pixels per second)
# Higher values = faster scrolling
spectrogram_scroll_speed = {}

# Spectrogram Window Size - FFT window size for frequency analysis
# Larger values = better frequency resolution but slower response
# Options: 512, 1024, 2048, 4096
spectrogram_window_size = {}

# Spectrogram Color Mode - How colors are mapped in the spectrogram
# Options: "intensity" (magnitude->color), "frequency" (Y-position->color), "volume" (overall level shifts hue)
spectrogram_color_mode = "{}"

# 2D Matrix Mode - Enable 2D matrix output for spectrum visualization (live mode only)
# When enabled, spectrum is rendered on a 2D matrix instead of a 1D strip
# Options: true, false
matrix_2d_enabled = {}

# 2D Matrix Width - Width of the 2D matrix in LEDs/pixels
# Only used when matrix_2d_enabled = true
matrix_2d_width = {}

# 2D Matrix Height - Height of the 2D matrix in LEDs/pixels
# Only used when matrix_2d_enabled = true
matrix_2d_height = {}

# 2D Matrix Gradient Direction - Direction the color gradient spans
# Options: "horizontal" (gradient across frequencies), "vertical" (gradient across amplitude)
matrix_2d_gradient_direction = "{}"

# Relay Mode - IP address to listen on for receiving raw RGB24 frames
# Use "0.0.0.0" to listen on all interfaces, or "127.0.0.1" for localhost only
# Only used when mode = "relay"
relay_listen_ip = "{}"

# Relay Mode - UDP listen port for receiving raw RGB24 frames
# Only used when mode = "relay"
relay_listen_port = {}

# Relay Frame Width - Width of incoming frame in pixels (relay mode only)
relay_frame_width = {}

# Relay Frame Height - Height of incoming frame in pixels (relay mode only)
relay_frame_height = {}

# Webcam Mode - Frame width in pixels for webcam capture
# Only used when mode = "webcam"
webcam_frame_width = {}

# Webcam Mode - Frame height in pixels for webcam capture
# Only used when mode = "webcam"
webcam_frame_height = {}

# Webcam Mode - Target FPS for webcam capture (client-side)
# Only used when mode = "webcam"
webcam_target_fps = {}

# Webcam Mode - Brightness multiplier (0.0 to 2.0)
# Values below 1.0 darken the image, above 1.0 brighten it
# Default is 0.5 (50%) to prevent washout on bright displays
webcam_brightness = {}

# Tron Game Mode - Grid width in pixels
# Only used when mode = "tron"
tron_width = {}

# Tron Game Mode - Grid height in pixels
# Only used when mode = "tron"
tron_height = {}

# Tron Game Mode - Update speed in milliseconds (lower = faster)
# Controls how fast the game runs (100ms = 10 FPS game speed)
tron_speed_ms = {}

# Tron Game Mode - Delay before resetting after game over (milliseconds)
# Time to display final state before restarting the game
tron_reset_delay_ms = {}

# Tron Game Mode - AI look-ahead distance (steps)
# How many steps ahead the AI looks when choosing direction (1-128)
# Higher values = smarter AI but slower performance
tron_look_ahead = {}

# Tron Game Mode - Maximum trail length (0 = infinite)
# Limits how long each player's trail can be (0 = infinite trail)
tron_trail_length = {}

# Tron Game Mode - AI aggressiveness (0.0 to 1.0)
# Higher = more aggressive/risky turns, Lower = more cautious play
# 0.0 = very cautious, 0.5 = balanced, 1.0 = very aggressive
tron_ai_aggression = {}

# Tron Game Mode - Number of AI players (1-8)
# 1 = Snake mode (single player), 2-8 = Tron mode (multiplayer)
tron_num_players = {}

# Tron Game Mode - Food Mode (players compete to eat food and grow)
# When enabled, players start with length 1 and grow by eating food pixels
# Game never resets in food mode
# Options: true, false
tron_food_mode = {}

# Tron Game Mode - Maximum Food Count (1-20)
# Maximum number of food items that can appear simultaneously in food mode
# With random spawning, multiple foods may appear based on this limit
# Players will pursue the closest/safest food target
tron_food_max_count = {}

# Tron Game Mode - Food TTL (Time-To-Live) in seconds (1-300)
# How long food stays in one location before relocating to prevent circling
# Default is 10 seconds
tron_food_ttl_seconds = {}

# Tron Game Mode - Trail Fade Effect
# Enable brightness fading effect on player trails (tail dimmer, head brighter)
# Options: true (fading enabled), false (uniform brightness)
tron_trail_fade = {}

# Tron Game Mode - Super Food Enabled
# Enable super food spawning (red color, 10% spawn chance, adds +5 to length instead of +1)
# Only applies when tron_food_mode is enabled
# Options: true (super food enabled), false (regular food only)
tron_super_food_enabled = {}

# Tron Game Mode - Power Food Enabled
# Enable power food spawning (yellow color, 1% spawn chance, 10 second power mode)
# Power mode grants: immunity to death, kills other players on contact, 25% speed boost
# Player flashes between normal color and yellow during power mode
# Only applies when tron_food_mode is enabled
# Options: true (power food enabled), false (power food disabled)
tron_power_food_enabled = {}

# Tron Game Mode - Diagonal Movement
# Enable diagonal movement (8 directions instead of 4)
# When enabled, players can move diagonally across the grid (NW, NE, SW, SE)
# Options: true (8 directions), false (4 cardinal directions only)
tron_diagonal_movement = {}

# Tron Game Mode - Player colors (comma-separated gradients) - DEPRECATED
# Use gradient names like "rainbow,fire,ocean" or hex colors like "00ffff,ff00ff"
tron_player_colors = "{}"

# Tron Game Mode - Individual player colors (gradient names or hex colors)
tron_player_1_color = "{}"
tron_player_2_color = "{}"
tron_player_3_color = "{}"
tron_player_4_color = "{}"
tron_player_5_color = "{}"
tron_player_6_color = "{}"
tron_player_7_color = "{}"
tron_player_8_color = "{}"

# Tron Game Mode - Gradient Animation Speed (0.0 = disabled, 1.0 = similar to bandwidth mode)
# Controls how fast gradients cycle along player trails
tron_animation_speed = {}

# Tron Game Mode - Scale Animation Speed with Trail Length
# When enabled, longer trails animate faster
# Options: true, false
tron_scale_animation_speed = {}

# Tron Game Mode - Gradient Animation Direction
# Options: "forward" (head to tail), "backward" (tail to head)
tron_animation_direction = "{}"

# Tron Game Mode - Gradient Interpolation Mode
# Controls how smoothly colors blend in the gradient
# Options: "linear" (sharp), "basis" (smooth B-spline), "catmullrom" (smooth Catmull-Rom)
tron_interpolation = "{}"

# Tron Game Mode - Flip Animation Direction on Food Eaten
# When enabled, each player's animation direction flips every time they eat food
# Options: true, false
tron_flip_direction_on_food = {}

# Geometry Mode - Grid Width
# Width of the 2D grid for geometry calculations (default 64)
geometry_grid_width = {}

# Geometry Mode - Grid Height
# Height of the 2D grid for geometry calculations (default 32)
geometry_grid_height = {}

# Geometry Mode - Mode Selection
# Which geometry to show: "cycle" (all modes), "lissajous", "fibonacci", "polar_rose", etc.
geometry_mode_select = "{}"

# Geometry Mode - Mode Duration
# How long to display each geometry in seconds before transitioning (default 12.0)
geometry_mode_duration_seconds = {}

# Geometry Mode - Randomize Order
# Randomly select next geometry instead of cycling sequentially (default false)
geometry_randomize_order = {}

# Boid Simulation - Number of Boids
boid_count = {}

# Boid Simulation - Separation Distance
boid_separation_distance = {}

# Boid Simulation - Alignment Distance
boid_alignment_distance = {}

# Boid Simulation - Cohesion Distance
boid_cohesion_distance = {}

# Boid Simulation - Max Speed
boid_max_speed = {}

# Boid Simulation - Max Force
boid_max_force = {}

# Predator-Prey Settings
boid_predator_enabled = {}
boid_predator_count = {}
boid_predator_speed = {}
boid_avoidance_distance = {}
boid_chase_force = {}

# Falling Sand Simulation
sand_grid_width = {}
sand_grid_height = {}
sand_spawn_enabled = {}
sand_particle_type = "{}"
sand_spawn_rate = {}
sand_spawn_radius = {}
sand_spawn_x = {}
sand_obstacles_enabled = {}
sand_obstacle_density = {}
sand_fire_enabled = {}
sand_color_sand = "{}"
sand_color_water = "{}"
sand_color_stone = "{}"
sand_color_fire = "{}"
sand_color_smoke = "{}"
sand_color_wood = "{}"
sand_color_lava = "{}"
"#,
            sanitized.max_gbps,
            sanitized.color,
            sanitized.tx_color,
            sanitized.rx_color,
            sanitized.direction,
            sanitized.swap,
            sanitized.rx_split_percent,
            sanitized.strobe_on_max,
            sanitized.strobe_rate_hz,
            sanitized.strobe_duration_ms,
            sanitized.strobe_color,
            sanitized.animation_speed,
            sanitized.scale_animation_speed,
            sanitized.tx_animation_direction,
            sanitized.rx_animation_direction,
            sanitized.interpolation_time_ms,
            sanitized.enable_interpolation,
            sanitized.wled_ip,
            sanitized.multi_device_enabled,
            sanitized.multi_device_send_parallel,
            sanitized.multi_device_fail_fast,
            sanitized.interface,
            sanitized.ssh_host,
            sanitized.ssh_user,
            sanitized.total_leds,
            sanitized.use_gradient,
            sanitized.intensity_colors,
            sanitized.interpolation,
            sanitized.fps,
            sanitized.ddp_delay_ms,
            sanitized.global_brightness,
            sanitized.mode,
            sanitized.httpd_enabled,
            sanitized.httpd_https_enabled,
            sanitized.httpd_ip,
            sanitized.httpd_port,
            sanitized.httpd_auth_enabled,
            sanitized.httpd_auth_user,
            sanitized.httpd_auth_pass,
            sanitized.test_tx,
            sanitized.test_rx,
            sanitized.test_tx_percent,
            sanitized.test_rx_percent,
            sanitized.midi_device,
            sanitized.midi_gradient,
            sanitized.midi_random_colors,
            sanitized.midi_velocity_colors,
            sanitized.midi_one_to_one,
            sanitized.midi_channel_mode,
            sanitized.audio_device,
            sanitized.audio_gain,
            sanitized.log_scale,
            sanitized.attack_ms,
            sanitized.decay_ms,
            sanitized.vu,
            sanitized.peak_hold,
            sanitized.peak_hold_duration_ms,
            sanitized.peak_hold_color,
            sanitized.peak_direction_toggle,
            sanitized.spectrogram,
            sanitized.spectrogram_scroll_direction,
            sanitized.spectrogram_scroll_speed,
            sanitized.spectrogram_window_size,
            sanitized.spectrogram_color_mode,
            sanitized.matrix_2d_enabled,
            sanitized.matrix_2d_width,
            sanitized.matrix_2d_height,
            sanitized.matrix_2d_gradient_direction,
            sanitized.relay_listen_ip,
            sanitized.relay_listen_port,
            sanitized.relay_frame_width,
            sanitized.relay_frame_height,
            sanitized.webcam_frame_width,
            sanitized.webcam_frame_height,
            sanitized.webcam_target_fps,
            sanitized.webcam_brightness,
            sanitized.tron_width,
            sanitized.tron_height,
            sanitized.tron_speed_ms,
            sanitized.tron_reset_delay_ms,
            sanitized.tron_look_ahead,
            sanitized.tron_trail_length,
            sanitized.tron_ai_aggression,
            sanitized.tron_num_players,
            sanitized.tron_food_mode,
            sanitized.tron_food_max_count,
            sanitized.tron_food_ttl_seconds,
            sanitized.tron_trail_fade,
            sanitized.tron_super_food_enabled,
            sanitized.tron_power_food_enabled,
            sanitized.tron_diagonal_movement,
            sanitized.tron_player_colors,
            sanitized.tron_player_1_color,
            sanitized.tron_player_2_color,
            sanitized.tron_player_3_color,
            sanitized.tron_player_4_color,
            sanitized.tron_player_5_color,
            sanitized.tron_player_6_color,
            sanitized.tron_player_7_color,
            sanitized.tron_player_8_color,
            sanitized.tron_animation_speed,
            sanitized.tron_scale_animation_speed,
            sanitized.tron_animation_direction,
            sanitized.tron_interpolation,
            sanitized.tron_flip_direction_on_food,
            sanitized.geometry_grid_width,
            sanitized.geometry_grid_height,
            sanitized.geometry_mode_select,
            sanitized.geometry_mode_duration_seconds,
            sanitized.geometry_randomize_order,
            sanitized.boid_count,
            sanitized.boid_separation_distance,
            sanitized.boid_alignment_distance,
            sanitized.boid_cohesion_distance,
            sanitized.boid_max_speed,
            sanitized.boid_max_force,
            sanitized.boid_predator_enabled,
            sanitized.boid_predator_count,
            sanitized.boid_predator_speed,
            sanitized.boid_avoidance_distance,
            sanitized.boid_chase_force,
            sanitized.sand_grid_width,
            sanitized.sand_grid_height,
            sanitized.sand_spawn_enabled,
            sanitized.sand_particle_type,
            sanitized.sand_spawn_rate,
            sanitized.sand_spawn_radius,
            sanitized.sand_spawn_x,
            sanitized.sand_obstacles_enabled,
            sanitized.sand_obstacle_density,
            sanitized.sand_fire_enabled,
            sanitized.sand_color_sand,
            sanitized.sand_color_water,
            sanitized.sand_color_stone,
            sanitized.sand_color_fire,
            sanitized.sand_color_smoke,
            sanitized.sand_color_wood,
            sanitized.sand_color_lava,
        );

        // Append wled_devices array if multi-device mode is enabled and devices are configured
        if !sanitized.wled_devices.is_empty() {
            contents.push_str("\n# Multi-Device Configuration\n");
            contents.push_str("# Configure multiple WLED controllers - each gets a portion of the LED frame\n");
            contents.push_str("# led_offset: Starting LED position in unified frame\n");
            contents.push_str("# led_count: Number of LEDs this device controls\n\n");

            for device in &sanitized.wled_devices {
                contents.push_str("[[wled_devices]]\n");
                contents.push_str(&format!("ip = \"{}\"\n", device.ip));
                contents.push_str(&format!("led_offset = {}\n", device.led_offset));
                contents.push_str(&format!("led_count = {}\n", device.led_count));
                contents.push_str(&format!("enabled = {}\n\n", device.enabled));
            }
        }

        std::fs::write(path, contents)?;
        Ok(())
    }
}
