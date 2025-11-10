// Renderer Module - LED rendering functions and DDP helpers
use anyhow::Result;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use crate::multi_device::{MultiDeviceConfig, MultiDeviceManager, WLEDDevice};
use crate::config::BandwidthConfig;
use std::time::{Duration, Instant, SystemTime};

// Import shared types
use crate::types::{build_gradient_from_color, build_intensity_gradient, InterpolationMode, Rgb};

// Import midi module for MIDI rendering functions
use crate::midi;

// Direction mode for LED rendering
#[derive(Clone, Copy)]
pub enum DirectionMode {
    Mirrored,
    Opposing,
    Left,
    Right,
}

// Shared state between main thread and render thread
#[derive(Clone)]
pub struct SharedRenderState {
    pub current_rx_kbps: f64,
    pub current_tx_kbps: f64,
    pub start_rx_kbps: f64,
    pub start_tx_kbps: f64,
    pub last_bandwidth_update: Option<Instant>,
    pub animation_speed: f64,
    pub scale_animation_speed: bool,
    pub tx_animation_direction: String,
    pub rx_animation_direction: String,
    pub interpolation_time_ms: f64,
    pub enable_interpolation: bool,
    pub max_bandwidth_kbps: f64,

    // Color configuration (as strings, renderer will rebuild gradients when changed)
    pub tx_color: String,
    pub rx_color: String,
    pub use_gradient: bool,
    pub intensity_colors: bool,  // Map utilization to color gradient position (all LEDs same color)
    pub interpolation_mode: InterpolationMode,

    // Rendering configuration
    pub direction: DirectionMode,
    pub swap: bool,
    pub fps: f64,
    pub ddp_delay_ms: f64,
    pub global_brightness: f64,
    pub total_leds: usize,
    pub rx_split_percent: f64,
    pub strobe_on_max: bool,
    pub strobe_rate_hz: f64,
    pub strobe_duration_ms: f64,
    pub strobe_color: String,
    pub test_mode: bool,  // Use exponential smoothing instead of time-based interpolation

    // Generation counter to detect changes
    pub generation: u64,
}

// Dedicated renderer that runs in its own thread at configurable FPS
pub struct Renderer {
    multi_device_manager: Arc<Mutex<MultiDeviceManager>>,
    shared_state: Arc<Mutex<SharedRenderState>>,
    shutdown: Arc<AtomicBool>,

    // Owned by renderer thread
    tx_animation_offset: f64,
    rx_animation_offset: f64,

    // Built from shared state
    tx_gradient: Option<colorgrad::Gradient>,
    rx_gradient: Option<colorgrad::Gradient>,
    tx_intensity_gradient: Option<colorgrad::Gradient>,  // Linear gradient for intensity mode
    rx_intensity_gradient: Option<colorgrad::Gradient>,  // Linear gradient for intensity mode
    tx_colors: Vec<Rgb>,
    rx_colors: Vec<Rgb>,
    tx_solid_color: Rgb,
    rx_solid_color: Rgb,

    // Cache to detect when gradients need rebuilding
    last_generation: u64,
}

impl Renderer {
    pub fn new(
        config: &BandwidthConfig,
        shared_state: Arc<Mutex<SharedRenderState>>,
        shutdown: Arc<AtomicBool>,
    ) -> Result<Self> {
        // Create multi-device manager
        let devices: Vec<WLEDDevice> = config.wled_devices.iter().map(|d| WLEDDevice {
            ip: d.ip.clone(),
            led_offset: d.led_offset,
            led_count: d.led_count,
            enabled: d.enabled,
        }).collect();

        let md_config = MultiDeviceConfig {
            devices,
            send_parallel: config.multi_device_send_parallel,
            fail_fast: config.multi_device_fail_fast,
        };

        let manager = MultiDeviceManager::new(md_config)?;

        // Lock shared state to get initial colors
        let state = shared_state.lock().unwrap();
        let (tx_gradient, tx_colors, tx_solid_color) =
            build_gradient_from_color(&state.tx_color, state.use_gradient, state.interpolation_mode)?;
        let (rx_gradient, rx_colors, rx_solid_color) =
            build_gradient_from_color(&state.rx_color, state.use_gradient, state.interpolation_mode)?;
        let tx_intensity_gradient =
            build_intensity_gradient(&state.tx_color, state.use_gradient, state.interpolation_mode)?;
        let rx_intensity_gradient =
            build_intensity_gradient(&state.rx_color, state.use_gradient, state.interpolation_mode)?;
        let last_generation = state.generation;
        drop(state);

        Ok(Renderer {
            multi_device_manager: Arc::new(Mutex::new(manager)),
            shared_state,
            shutdown,
            tx_animation_offset: 0.0,
            rx_animation_offset: 0.0,
            tx_gradient,
            rx_gradient,
            tx_intensity_gradient,
            rx_intensity_gradient,
            tx_colors,
            rx_colors,
            tx_solid_color,
            rx_solid_color,
            last_generation,
        })
    }

    fn rebuild_gradients_if_needed(&mut self) -> Result<()> {
        let state = self.shared_state.lock().unwrap();

        // Check if generation changed (config updated)
        if state.generation != self.last_generation {
            let (tx_gradient, tx_colors, tx_solid_color) =
                build_gradient_from_color(&state.tx_color, state.use_gradient, state.interpolation_mode)?;
            let (rx_gradient, rx_colors, rx_solid_color) =
                build_gradient_from_color(&state.rx_color, state.use_gradient, state.interpolation_mode)?;
            let tx_intensity_gradient =
                build_intensity_gradient(&state.tx_color, state.use_gradient, state.interpolation_mode)?;
            let rx_intensity_gradient =
                build_intensity_gradient(&state.rx_color, state.use_gradient, state.interpolation_mode)?;

            self.tx_gradient = tx_gradient;
            self.tx_intensity_gradient = tx_intensity_gradient;
            self.tx_colors = tx_colors;
            self.tx_solid_color = tx_solid_color;
            self.rx_gradient = rx_gradient;
            self.rx_intensity_gradient = rx_intensity_gradient;
            self.rx_colors = rx_colors;
            self.rx_solid_color = rx_solid_color;
            self.last_generation = state.generation;
        }

        Ok(())
    }

    fn calculate_leds(&self, bandwidth_kbps: f64, max_bandwidth_kbps: f64, leds_per_direction: usize) -> usize {
        let percentage = bandwidth_kbps / max_bandwidth_kbps;
        let leds = (percentage * leds_per_direction as f64) as usize;
        leds.min(leds_per_direction)
    }

    fn calculate_effective_speed(&self, rx_kbps: f64, tx_kbps: f64, state: &SharedRenderState) -> (f64, f64) {
        if state.scale_animation_speed {
            // Use the currently displayed (interpolated) bandwidth values, not the target values
            // This ensures animation continues smoothly during the interpolation period
            let tx_utilization = (tx_kbps / state.max_bandwidth_kbps).clamp(0.0, 1.0);
            let rx_utilization = (rx_kbps / state.max_bandwidth_kbps).clamp(0.0, 1.0);

            // Quantize to nice fractions to avoid aliasing/stuttering
            // Use FPS for quantization to avoid stuttering at different frame rates
            let tx_quantized = (tx_utilization * state.fps).round() / state.fps;
            let rx_quantized = (rx_utilization * state.fps).round() / state.fps;

            let tx_speed = state.animation_speed * tx_quantized;
            let rx_speed = state.animation_speed * rx_quantized;

            (tx_speed, rx_speed)
        } else {
            (state.animation_speed, state.animation_speed)
        }
    }

    fn calculate_led_positions(&self, tx_leds: usize, rx_leds: usize, direction: DirectionMode, swap: bool, total_leds: usize, leds_per_direction: usize) -> (Vec<usize>, Vec<usize>) {
        let half = leds_per_direction;

        let (first_half_leds, second_half_leds) = if swap {
            (tx_leds, rx_leds)
        } else {
            (rx_leds, tx_leds)
        };

        let (first_half_pos, second_half_pos) = match direction {
            DirectionMode::Mirrored => {
                let first: Vec<usize> = (0..first_half_leds).map(|i| half - 1 - i).collect();
                let second: Vec<usize> = (0..second_half_leds).map(|i| half + i).collect();
                (first, second)
            }
            DirectionMode::Opposing => {
                let first: Vec<usize> = (0..first_half_leds).collect();
                let second: Vec<usize> = (0..second_half_leds)
                    .map(|i| total_leds - 1 - i)
                    .collect();
                (first, second)
            }
            DirectionMode::Left => {
                let first: Vec<usize> = (0..first_half_leds).map(|i| half - 1 - i).collect();
                let second: Vec<usize> = (0..second_half_leds)
                    .map(|i| total_leds - 1 - i)
                    .collect();
                (first, second)
            }
            DirectionMode::Right => {
                let first: Vec<usize> = (0..first_half_leds).collect();
                let second: Vec<usize> = (0..second_half_leds).map(|i| half + i).collect();
                (first, second)
            }
        };

        if swap {
            (first_half_pos, second_half_pos)
        } else {
            (second_half_pos, first_half_pos)
        }
    }

    fn render_frame(&mut self, delta_seconds: f64) -> Result<Vec<u8>> {
        // Rebuild gradients if config changed (very quick check)
        self.rebuild_gradients_if_needed()?;

        // Lock shared state only long enough to read current values
        let state = self.shared_state.lock().unwrap();

        // Get bandwidth values (interpolated or instant based on enable_interpolation)
        let (rx_kbps, tx_kbps, test_mode) = if !state.enable_interpolation {
            // Interpolation disabled: instant response
            (state.current_rx_kbps, state.current_tx_kbps, false)
        } else if state.test_mode {
            // Test mode: use exponential smoothing for continuous smooth motion
            // Smoothing factor: move 20% toward target per second (adjusted by delta_seconds)
            let smoothing = (1.0 - (-5.0 * delta_seconds).exp()).min(1.0);

            let rx = state.start_rx_kbps + (state.current_rx_kbps - state.start_rx_kbps) * smoothing;
            let tx = state.start_tx_kbps + (state.current_tx_kbps - state.start_tx_kbps) * smoothing;

            (rx, tx, true)
        } else if let Some(last_update) = state.last_bandwidth_update {
            // Normal mode: time-based linear interpolation
            let elapsed_ms = last_update.elapsed().as_secs_f64() * 1000.0;
            let interpolation_time = state.interpolation_time_ms;
            let t = (elapsed_ms / interpolation_time).min(1.0);

            let interpolated_rx = state.start_rx_kbps + (state.current_rx_kbps - state.start_rx_kbps) * t;
            let interpolated_tx = state.start_tx_kbps + (state.current_tx_kbps - state.start_tx_kbps) * t;

            (interpolated_rx, interpolated_tx, false)
        } else {
            // No update yet, use current values
            (state.current_rx_kbps, state.current_tx_kbps, false)
        };

        let max_bandwidth_kbps = state.max_bandwidth_kbps;
        let direction = state.direction;
        let swap = state.swap;
        let use_gradient = state.use_gradient;
        let intensity_colors = state.intensity_colors;
        let (tx_effective_speed, rx_effective_speed) = self.calculate_effective_speed(rx_kbps, tx_kbps, &state);
        let fps = state.fps;
        let tx_animation_direction = state.tx_animation_direction.clone();
        let rx_animation_direction = state.rx_animation_direction.clone();
        let total_leds = state.total_leds;
        let rx_split_percent = state.rx_split_percent.clamp(0.0, 100.0);
        let strobe_on_max = state.strobe_on_max;
        let strobe_rate_hz = state.strobe_rate_hz;
        let strobe_duration_ms = state.strobe_duration_ms;
        let strobe_color_str = state.strobe_color.clone();
        drop(state); // Release lock immediately

        // Parse strobe color
        let strobe_color = Rgb::from_hex(&strobe_color_str).unwrap_or(Rgb { r: 0, g: 0, b: 0 });

        // Calculate LED split based on rx_split_percent
        let rx_leds_available = ((total_leds as f64 * rx_split_percent) / 100.0) as usize;
        let tx_leds_available = total_leds - rx_leds_available;
        let leds_per_direction = total_leds / 2; // Keep for backward compatibility with position calculations

        // Calculate LED counts using the configurable split
        let rx_leds = self.calculate_leds(rx_kbps, max_bandwidth_kbps, rx_leds_available);
        let tx_leds = self.calculate_leds(tx_kbps, max_bandwidth_kbps, tx_leds_available);

        // Determine if we're in strobe mode for each segment
        let mut rx_strobe_active = false;
        let mut tx_strobe_active = false;

        if strobe_on_max && strobe_rate_hz > 0.0 {
            let now = SystemTime::now();
            let elapsed_millis = now.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis();

            // Calculate full cycle time in milliseconds
            let cycle_ms = (1000.0 / strobe_rate_hz) as u128;
            // Clamp strobe duration to not exceed the full cycle
            let clamped_duration = (strobe_duration_ms as u128).min(cycle_ms);

            // Determine position within the current cycle
            let position_in_cycle = elapsed_millis % cycle_ms;
            // Strobe is active during the last 'duration' milliseconds of each cycle
            let strobe_phase_active = position_in_cycle >= (cycle_ms - clamped_duration);

            // Activate strobe if at max and in strobe phase
            if rx_leds >= rx_leds_available && strobe_phase_active {
                rx_strobe_active = true;
            }

            if tx_leds >= tx_leds_available && strobe_phase_active {
                tx_strobe_active = true;
            }
        }

        // Update animation offsets independently for TX and RX
        if tx_effective_speed > 0.0 {
            let leds_per_second = tx_effective_speed * fps;
            let offset_delta = (leds_per_second * delta_seconds) / leds_per_direction as f64;
            self.tx_animation_offset = (self.tx_animation_offset + offset_delta) % 1.0;
        }

        if rx_effective_speed > 0.0 {
            let leds_per_second = rx_effective_speed * fps;
            let offset_delta = (leds_per_second * delta_seconds) / leds_per_direction as f64;
            self.rx_animation_offset = (self.rx_animation_offset + offset_delta) % 1.0;
        }

        // Prepare frame
        let frame_size = total_leds * 3;
        let mut frame = vec![0u8; frame_size];

        let (tx_positions, rx_positions) = self.calculate_led_positions(tx_leds, rx_leds, direction, swap, total_leds, leds_per_direction);

        // Render TX positions
        if tx_strobe_active {
            // Strobe mode: fill all TX LEDs with strobe color
            for &led_pos in tx_positions.iter() {
                let offset = led_pos * 3;
                frame[offset] = strobe_color.r;
                frame[offset + 1] = strobe_color.g;
                frame[offset + 2] = strobe_color.b;
            }
        } else if intensity_colors && self.tx_intensity_gradient.is_some() {
            // Intensity Colors Mode: Map utilization to gradient position (all LEDs same color)
            // Use the linear intensity gradient (0.0 = first color, 1.0 = last color)
            let tx_utilization = (tx_kbps / max_bandwidth_kbps).clamp(0.0, 1.0);
            let tx_gradient = self.tx_intensity_gradient.as_ref().unwrap();
            let rgba = tx_gradient.at(tx_utilization).to_rgba8();

            for &led_pos in tx_positions.iter() {
                let offset = led_pos * 3;
                frame[offset] = rgba[0];
                frame[offset + 1] = rgba[1];
                frame[offset + 2] = rgba[2];
            }
        } else if !use_gradient && self.tx_colors.len() >= 2 && !tx_positions.is_empty() {
            // Use total available LEDs for pattern, not just lit LEDs (so segments don't scale with level)
            let total_pattern_leds = tx_leds_available as f64;
            let pattern_offset = if tx_animation_direction == "right" {
                -self.tx_animation_offset * total_pattern_leds
            } else {
                self.tx_animation_offset * total_pattern_leds
            };
            let segment_size = total_pattern_leds / self.tx_colors.len() as f64;

            for (i, &led_pos) in tx_positions.iter().enumerate() {
                // Map LED index to pattern position (even if not all LEDs are lit)
                let pattern_pos = ((i as f64 + pattern_offset) % total_pattern_leds + total_pattern_leds) % total_pattern_leds;
                let segment_idx = (pattern_pos / segment_size).floor() as usize % self.tx_colors.len();
                let color = &self.tx_colors[segment_idx];

                let offset = led_pos * 3;
                frame[offset] = color.r;
                frame[offset + 1] = color.g;
                frame[offset + 2] = color.b;
            }
        } else if let Some(ref tx_gradient) = self.tx_gradient {
            for &led_pos in tx_positions.iter() {
                // Map LED position to gradient position (0.0-1.0 across the full TX half)
                let pos_ratio = (led_pos % leds_per_direction) as f64 / leds_per_direction as f64;
                let animated_pos = if tx_animation_direction == "right" {
                    (1.0 + pos_ratio - self.tx_animation_offset) % 1.0
                } else {
                    (pos_ratio + self.tx_animation_offset) % 1.0
                };

                let rgba = tx_gradient.at(animated_pos).to_rgba8();
                let offset = led_pos * 3;
                frame[offset] = rgba[0];
                frame[offset + 1] = rgba[1];
                frame[offset + 2] = rgba[2];
            }
        } else {
            for &led_pos in &tx_positions {
                let offset = led_pos * 3;
                frame[offset] = self.tx_solid_color.r;
                frame[offset + 1] = self.tx_solid_color.g;
                frame[offset + 2] = self.tx_solid_color.b;
            }
        }

        // Render RX positions
        if rx_strobe_active {
            // Strobe mode: fill all RX LEDs with strobe color
            for &led_pos in rx_positions.iter() {
                let offset = led_pos * 3;
                frame[offset] = strobe_color.r;
                frame[offset + 1] = strobe_color.g;
                frame[offset + 2] = strobe_color.b;
            }
        } else if intensity_colors && self.rx_intensity_gradient.is_some() {
            // Intensity Colors Mode: Map utilization to gradient position (all LEDs same color)
            // Use the linear intensity gradient (0.0 = first color, 1.0 = last color)
            let rx_utilization = (rx_kbps / max_bandwidth_kbps).clamp(0.0, 1.0);
            let rx_gradient = self.rx_intensity_gradient.as_ref().unwrap();
            let rgba = rx_gradient.at(rx_utilization).to_rgba8();

            for &led_pos in rx_positions.iter() {
                let offset = led_pos * 3;
                frame[offset] = rgba[0];
                frame[offset + 1] = rgba[1];
                frame[offset + 2] = rgba[2];
            }
        } else if !use_gradient && self.rx_colors.len() >= 2 && !rx_positions.is_empty() {
            // Use total available LEDs for pattern, not just lit LEDs (so segments don't scale with level)
            let total_pattern_leds = rx_leds_available as f64;
            // Invert direction logic for RX so "right" means same visual direction as TX "right"
            let pattern_offset = if rx_animation_direction == "left" {
                -self.rx_animation_offset * total_pattern_leds
            } else {
                self.rx_animation_offset * total_pattern_leds
            };
            let segment_size = total_pattern_leds / self.rx_colors.len() as f64;

            for (i, &led_pos) in rx_positions.iter().enumerate() {
                // Map LED index to pattern position (even if not all LEDs are lit)
                let pattern_pos = ((i as f64 + pattern_offset) % total_pattern_leds + total_pattern_leds) % total_pattern_leds;
                let segment_idx = (pattern_pos / segment_size).floor() as usize % self.rx_colors.len();
                let color = &self.rx_colors[segment_idx];

                let offset = led_pos * 3;
                frame[offset] = color.r;
                frame[offset + 1] = color.g;
                frame[offset + 2] = color.b;
            }
        } else if let Some(ref rx_gradient) = self.rx_gradient {
            for &led_pos in rx_positions.iter() {
                // Map LED position to gradient position (0.0-1.0 across the full RX half)
                let pos_ratio = (led_pos % leds_per_direction) as f64 / leds_per_direction as f64;
                let animated_pos = if rx_animation_direction == "right" {
                    (1.0 + pos_ratio - self.rx_animation_offset) % 1.0
                } else {
                    (pos_ratio + self.rx_animation_offset) % 1.0
                };

                let rgba = rx_gradient.at(animated_pos).to_rgba8();
                let offset = led_pos * 3;
                frame[offset] = rgba[0];
                frame[offset + 1] = rgba[1];
                frame[offset + 2] = rgba[2];
            }
        } else {
            for &led_pos in &rx_positions {
                let offset = led_pos * 3;
                frame[offset] = self.rx_solid_color.r;
                frame[offset + 1] = self.rx_solid_color.g;
                frame[offset + 2] = self.rx_solid_color.b;
            }
        }

        // Update start values for exponential smoothing in test mode
        if test_mode {
            let mut state = self.shared_state.lock().unwrap();
            state.start_rx_kbps = rx_kbps;
            state.start_tx_kbps = tx_kbps;
            drop(state);
        }

        // Return frame buffer for delayed sending
        Ok(frame)
    }

    // Main render loop that runs at configurable FPS
    pub fn run(mut self) {
        let mut last_frame = Instant::now();

        // Frame buffer for delay - stores (send_time, frame_data)
        let mut frame_buffer: VecDeque<(Instant, Vec<u8>)> = VecDeque::new();

        loop {
            let loop_start = Instant::now();

            // Check for shutdown signal
            if self.shutdown.load(Ordering::Relaxed) {
                break;
            }

            // Read FPS, delay, and brightness from shared state
            let (fps, delay_ms, global_brightness) = {
                let state = self.shared_state.lock().unwrap();
                (state.fps, state.ddp_delay_ms, state.global_brightness)
            };

            let delay_duration = Duration::from_micros((delay_ms * 1000.0) as u64);

            // Calculate frame duration based on FPS
            let frame_duration_micros = (1_000_000.0 / fps) as u64;
            let frame_duration = Duration::from_micros(frame_duration_micros);

            let elapsed = loop_start.duration_since(last_frame);

            // Render new frame if it's time
            if elapsed >= frame_duration {
                let delta_seconds = elapsed.as_secs_f64();
                last_frame = loop_start;

                // Render frame and add to buffer with scheduled send time
                if let Ok(frame) = self.render_frame(delta_seconds) {
                    let send_time = loop_start + delay_duration;
                    frame_buffer.push_back((send_time, frame));
                }
            }

            // Send all frames that are ready (send_time <= now)
            let now = Instant::now();
            while let Some((send_time, _)) = frame_buffer.front() {
                if *send_time <= now {
                    if let Some((_, frame_to_send)) = frame_buffer.pop_front() {
                        if let Ok(mut manager) = self.multi_device_manager.lock() {
                            // Apply global brightness
                            let _ = manager.send_frame_with_brightness(&frame_to_send, Some(global_brightness));
                        }
                    }
                } else {
                    break;
                }
            }

            // Tiny sleep to avoid spinning CPU at 100%
            thread::sleep(Duration::from_micros(100));
        }
    }
}

// NOTE: These functions are no longer used since we switched to MultiDeviceManager
// Keeping them commented out in case they're needed for reference
//
// /// Send a DDP frame with reconnection on failure
// /// Returns true if a new connection was created
// pub fn send_ddp_with_reconnect(
//     conn: &mut DDPConnection,
//     wled_ip: &str,
//     frame: &[u8],
// ) -> bool {
//     let mut reconnected = false;
//
//     loop {
//         if conn.write(frame).is_ok() {
//             return reconnected;
//         }
//
//         // Send failed, try to reconnect
//         reconnected = reconnect_ddp(conn, wled_ip);
//     }
// }
//
// /// Helper function to reconnect to WLED controller
// /// Returns true after successful reconnection
// fn reconnect_ddp(conn: &mut DDPConnection, wled_ip: &str) -> bool {
//     eprintln!("⚠️  WLED connection lost. Attempting to reconnect to {}...", wled_ip);
//
//     loop {
//         thread::sleep(Duration::from_secs(5));
//
//         let dest_addr = format!("{}:4048", wled_ip);
//         match UdpSocket::bind("0.0.0.0:0") {
//             Ok(socket) => {
//                 match DDPConnection::try_new(&dest_addr, PixelConfig::default(), ID::Default, socket) {
//                     Ok(new_conn) => {
//                         *conn = new_conn;
//                         eprintln!("✓ Reconnected to WLED at {}", wled_ip);
//                         return true;
//                     }
//                     Err(e) => {
//                         eprintln!("✗ Failed to reconnect: {}. Retrying in 5 seconds...", e);
//                     }
//                 }
//             }
//             Err(e) => {
//                 eprintln!("✗ Failed to bind socket: {}. Retrying in 5 seconds...", e);
//             }
//         }
//     }
// }

/// Render MIDI notes to LED frame with attack/decay smoothing
pub fn render_midi_to_leds(
    note_state: &midi::NoteState,
    total_leds: usize,
    gradient_enabled: bool,
    color_map: Option<&midi::ColorMap>,
    velocity_colors: bool,
    one_to_one: bool,  // 1-to-1 note mapping (centered at middle C) vs spread across all LEDs
    channel_mode: bool,  // Use MIDI channels to address different LED sections
    smoothed_frame: &mut Vec<f32>,  // Current brightness per LED (smoothed)
    target_brightness: &mut Vec<f32>,  // Target brightness per LED (NOT from velocity, independently controlled)
    last_colors: &mut Vec<(u8, u8, u8)>,  // Store base RGB color (0-255) per LED, brightness applied separately
    attack_factor: f32,
    decay_factor: f32,
    debug_info: Option<&Arc<Mutex<Vec<String>>>>,  // Optional debug output
) -> Result<Vec<u8>> {
    let active_notes = note_state.get_active_notes();

    // Calculate LED layout (only used in spread mode)
    let (leds_per_note, start_offset, _end_offset) = midi::calculate_led_layout(total_leds);

    // Create target frame (what we want to display before smoothing)
    let frame_size = total_leds * 3;
    let mut target_frame = vec![0u8; frame_size];

    if active_notes.is_empty() {
        // No notes active - all LEDs off (already zeroed)
    } else if channel_mode {
        // Channel mode: Use MIDI channels to address different LED sections
        // Channel 0 (MIDI channel 1) = LEDs 0-127, Channel 1 (MIDI channel 2) = LEDs 128-255, etc.
        for (channel, note, velocity) in &active_notes {
            if let Some(led) = midi::channel_and_note_to_led(*channel, *note, total_leds) {
                let (r, g, b) = if velocity_colors {
                    // Velocity mode: color determined by velocity, full brightness
                    let color = midi::velocity_to_color(*velocity);
                    (color.r, color.g, color.b)
                } else {
                    // Note mode: color determined by note, brightness by velocity
                    let color = midi::get_note_color(*note, color_map);
                    let brightness = midi::velocity_to_brightness(*velocity);
                    (
                        ((color.r as f64 * brightness as f64) / 255.0) as u8,
                        ((color.g as f64 * brightness as f64) / 255.0) as u8,
                        ((color.b as f64 * brightness as f64) / 255.0) as u8,
                    )
                };

                let offset = led * 3;
                target_frame[offset] = r;
                target_frame[offset + 1] = g;
                target_frame[offset + 2] = b;
            }
        }
    } else if one_to_one {
        // 1-to-1 mode: Each note lights up multiple LEDs (pattern repeats every 128 LEDs)
        for (_channel, note, velocity) in &active_notes {
            let (r, g, b) = if velocity_colors {
                // Velocity mode: color determined by velocity, full brightness
                let color = midi::velocity_to_color(*velocity);
                (color.r, color.g, color.b)
            } else {
                // Note mode: color determined by note, brightness by velocity
                let color = midi::get_note_color(*note, color_map);
                let brightness = midi::velocity_to_brightness(*velocity);
                (
                    ((color.r as f64 * brightness as f64) / 255.0) as u8,
                    ((color.g as f64 * brightness as f64) / 255.0) as u8,
                    ((color.b as f64 * brightness as f64) / 255.0) as u8,
                )
            };

            // Get all LED positions for this note (pattern repeats every 128 LEDs)
            let leds = midi::note_to_leds_one_to_one(*note, total_leds);
            for led in leds {
                let offset = led * 3;
                target_frame[offset] = r;
                target_frame[offset + 1] = g;
                target_frame[offset + 2] = b;
            }
        }
    } else if active_notes.len() == 1 {
        // Single note: light only its segment
        let (_channel, note, velocity) = active_notes[0];

        let (r, g, b) = if velocity_colors {
            // Velocity mode: color determined by velocity, full brightness
            let color = midi::velocity_to_color(velocity);
            (color.r, color.g, color.b)
        } else {
            // Note mode: color determined by note, brightness by velocity
            let color = midi::get_note_color(note, color_map);
            let brightness = midi::velocity_to_brightness(velocity);
            (
                ((color.r as f64 * brightness as f64) / 255.0) as u8,
                ((color.g as f64 * brightness as f64) / 255.0) as u8,
                ((color.b as f64 * brightness as f64) / 255.0) as u8,
            )
        };

        let (start_led, end_led) = midi::note_to_led_range(note, leds_per_note, start_offset);

        for led in start_led..end_led {
            let offset = led * 3;
            target_frame[offset] = r;
            target_frame[offset + 1] = g;
            target_frame[offset + 2] = b;
        }
    } else if gradient_enabled {
        // Multiple notes with gradient: create gradient spanning from lowest to highest note
        let mut sorted_notes = active_notes.clone();
        sorted_notes.sort_by_key(|(_channel, note, _velocity)| *note);

        let min_note = sorted_notes[0].1;  // .1 = note
        let max_note = sorted_notes[sorted_notes.len() - 1].1;  // .1 = note

        // Get LED span
        let (span_start, _) = midi::note_to_led_range(min_note, leds_per_note, start_offset);
        let (_, span_end) = midi::note_to_led_range(max_note, leds_per_note, start_offset);

        // Build gradient with color stops at each note position
        use colorgrad::{CustomGradient, Color};

        let note_range = (max_note - min_note) as f64;
        let mut colors = Vec::new();
        let mut positions = Vec::new();

        for (_channel, note, velocity) in &sorted_notes {
            let (r, g, b) = if velocity_colors {
                // Velocity mode: color determined by velocity, full brightness
                let rgb = midi::velocity_to_color(*velocity);
                (rgb.r, rgb.g, rgb.b)
            } else {
                // Note mode: color determined by note, brightness by velocity
                let rgb = midi::get_note_color(*note, color_map);
                let brightness = midi::velocity_to_brightness(*velocity);
                (
                    ((rgb.r as f64 * brightness as f64) / 255.0) as u8,
                    ((rgb.g as f64 * brightness as f64) / 255.0) as u8,
                    ((rgb.b as f64 * brightness as f64) / 255.0) as u8,
                )
            };

            colors.push(Color::from_rgba8(r, g, b, 255));

            // Calculate position (0.0 to 1.0) within the note span
            let position = if note_range > 0.0 {
                (*note - min_note) as f64 / note_range
            } else {
                0.5 // Single note position (shouldn't happen but safety)
            };
            positions.push(position);
        }

        // Build the gradient
        let gradient = CustomGradient::new()
            .colors(&colors)
            .domain(&positions)
            .build()
            .unwrap();

        // Apply gradient across the span
        let span_length = span_end - span_start;
        for i in 0..span_length {
            let t = i as f64 / span_length as f64;
            let color = gradient.at(t).to_rgba8();

            let led = span_start + i;
            let offset = led * 3;
            target_frame[offset] = color[0];
            target_frame[offset + 1] = color[1];
            target_frame[offset + 2] = color[2];
        }
    } else {
        // Multiple notes without gradient: light each note's segment independently
        for (_channel, note, velocity) in &active_notes {
            let (r, g, b) = if velocity_colors {
                // Velocity mode: color determined by velocity, full brightness
                let color = midi::velocity_to_color(*velocity);
                (color.r, color.g, color.b)
            } else {
                // Note mode: color determined by note, brightness by velocity
                let color = midi::get_note_color(*note, color_map);
                let brightness = midi::velocity_to_brightness(*velocity);
                (
                    ((color.r as f64 * brightness as f64) / 255.0) as u8,
                    ((color.g as f64 * brightness as f64) / 255.0) as u8,
                    ((color.b as f64 * brightness as f64) / 255.0) as u8,
                )
            };

            let (start_led, end_led) = midi::note_to_led_range(*note, leds_per_note, start_offset);

            for led in start_led..end_led {
                let offset = led * 3;
                target_frame[offset] = r;
                target_frame[offset + 1] = g;
                target_frame[offset + 2] = b;
            }
        }
    }

    // Step 1: Update target brightness based on active notes
    // We DON'T clear the buffer - we selectively update based on note state

    // Build a set of which LEDs should be lit by active notes
    let mut active_leds = vec![false; total_leds];

    if channel_mode {
        // Channel mode: mark single LED per (channel, note) pair
        for (channel, note, _velocity) in &active_notes {
            if let Some(led) = midi::channel_and_note_to_led(*channel, *note, total_leds) {
                active_leds[led] = true;
            }
        }
    } else if one_to_one {
        // 1-to-1 mode: mark all LEDs for each note (pattern repeats every 128 LEDs)
        for (_channel, note, _velocity) in &active_notes {
            let leds = midi::note_to_leds_one_to_one(*note, total_leds);
            for led in leds {
                active_leds[led] = true;
            }
        }
    } else if gradient_enabled && active_notes.len() > 1 {
        // Gradient mode: mark all LEDs in span as active
        if let Some(&min_note) = active_notes.iter().map(|(_ch, n, _vel)| n).min() {
            if let Some(&max_note) = active_notes.iter().map(|(_ch, n, _vel)| n).max() {
                let (span_start, _) = midi::note_to_led_range(min_note, leds_per_note, start_offset);
                let (_, span_end) = midi::note_to_led_range(max_note, leds_per_note, start_offset);
                for led in span_start..span_end {
                    active_leds[led] = true;
                }
            }
        }
    } else {
        // Spread mode: mark each note's segment
        for (_channel, note, _velocity) in &active_notes {
            let (start_led, end_led) = midi::note_to_led_range(*note, leds_per_note, start_offset);
            for led in start_led..end_led {
                active_leds[led] = true;
            }
        }
    }

    // Now update targets based on whether LEDs are active or not
    if channel_mode {
        // Channel mode: direct (channel, note) to LED mapping
        for (channel, note, velocity) in &active_notes {
            if let Some(led) = midi::channel_and_note_to_led(*channel, *note, total_leds) {
                // Get color DIRECTLY from the color function
                let color = if velocity_colors {
                    midi::velocity_to_color(*velocity)
                } else {
                    midi::get_note_color(*note, color_map)
                };

                // Get brightness: full brightness when velocity controls color, velocity-based otherwise
                let brightness = if velocity_colors {
                    255.0 // Full brightness - let color show the velocity
                } else {
                    midi::velocity_to_brightness(*velocity) as f32
                };

                let old_target = target_brightness[led];

                // Note is active - ALWAYS update target brightness (overrides decay)
                target_brightness[led] = brightness;

                // Only update color when target was OFF (new note trigger or retrigger during decay)
                if old_target < 1.0 {
                    // Note turning ON (either new or retriggering during decay)
                    // Store the base RGB color - brightness will be applied during rendering
                    last_colors[led] = (color.r, color.g, color.b);
                }
            }
        }

        // Set targets to 0.0 for LEDs that should be off
        for led in 0..total_leds {
            if !active_leds[led] {
                target_brightness[led] = 0.0;
            }
        }
    } else if one_to_one {
        // 1-to-1 mode: direct note-to-LED mapping (repeating every 128 LEDs)
        for (_channel, note, velocity) in &active_notes {
            let leds = midi::note_to_leds_one_to_one(*note, total_leds);

            // Get color DIRECTLY from the color function
            let color = if velocity_colors {
                midi::velocity_to_color(*velocity)
            } else {
                midi::get_note_color(*note, color_map)
            };

            // Get brightness: full brightness when velocity controls color, velocity-based otherwise
            let brightness = if velocity_colors {
                255.0 // Full brightness - let color show the velocity
            } else {
                midi::velocity_to_brightness(*velocity) as f32
            };

            // Update all LED positions for this note
            for led in leds {
                let old_target = target_brightness[led];

                // Note is active - ALWAYS update target brightness (overrides decay)
                target_brightness[led] = brightness;

                // Only update color when target was OFF (new note trigger or retrigger during decay)
                if old_target < 1.0 {
                    // Note turning ON (either new or retriggering during decay)
                    // Store the base RGB color - brightness will be applied during rendering
                    last_colors[led] = (color.r, color.g, color.b);
                }
            }
        }

        // Set targets to 0.0 for LEDs that should be off
        for led in 0..total_leds {
            if !active_leds[led] {
                target_brightness[led] = 0.0;
            }
        }
    } else if gradient_enabled && active_notes.len() > 1 {
        // Gradient mode (spread)
        for led in 0..total_leds {
            let offset = led * 3;
            let target_r = target_frame[offset] as f32;
            let target_g = target_frame[offset + 1] as f32;
            let target_b = target_frame[offset + 2] as f32;
            let brightness = target_r.max(target_g).max(target_b);

            let old_target = target_brightness[led];

            if active_leds[led] {
                // LED should be lit - ALWAYS update target brightness (overrides decay)
                target_brightness[led] = brightness;

                // Only update color when target was OFF (new note trigger or retrigger during decay)
                if old_target < 1.0 {
                    // Note turning ON (either new or retriggering during decay)
                    // Store the base RGB color - brightness will be applied during rendering
                    last_colors[led] = (target_r as u8, target_g as u8, target_b as u8);
                }
            } else {
                // LED should be OFF - set target to 0 to trigger decay
                target_brightness[led] = 0.0;
            }
        }
    } else {
        // Non-gradient mode
        for (_channel, note, velocity) in &active_notes {
            let (start_led, end_led) = midi::note_to_led_range(*note, leds_per_note, start_offset);

            // Get color DIRECTLY from the color function
            let color = if velocity_colors {
                midi::velocity_to_color(*velocity)
            } else {
                midi::get_note_color(*note, color_map)
            };

            // Get brightness: full brightness when velocity controls color, velocity-based otherwise
            let brightness = if velocity_colors {
                255.0 // Full brightness - let color show the velocity
            } else {
                midi::velocity_to_brightness(*velocity) as f32
            };

            for led in start_led..end_led {
                let old_target = target_brightness[led];

                // Note is active - ALWAYS update target brightness (overrides decay)
                target_brightness[led] = brightness;

                // Only update color when target was OFF (new note trigger or retrigger during decay)
                // Keep existing color when note is sustained (old_target already > 1.0)
                if old_target < 1.0 {
                    // Note turning ON (either new or retriggering during decay)
                    // Store the base RGB color - brightness will be applied during rendering
                    last_colors[led] = (color.r, color.g, color.b);
                }
            }
        }

        // Set targets to 0.0 for LEDs that should be off
        for led in 0..total_leds {
            if !active_leds[led] {
                target_brightness[led] = 0.0;
            }
        }
    }

    // Step 2: Apply attack/decay smoothing - completely independent of velocity functions
    let mut final_frame = vec![0u8; frame_size];

    // Debug: track decaying LED (using thread_local to avoid unsafe static mut)
    use std::cell::Cell;
    thread_local! {
        static DEBUG_FRAME_COUNT: Cell<u32> = Cell::new(0);
        static DEBUG_LED: Cell<Option<usize>> = Cell::new(None);
    }
    let mut found_decaying_led = None;

    for led in 0..total_leds {
        let offset = led * 3;

        // Get target brightness (already set above, independent of velocity functions)
        let target_bright = target_brightness[led];

        // Get current smoothed brightness
        let current_brightness = smoothed_frame[led];

        // Smooth the brightness with f32 precision (NOT limited to 127 velocity steps!)
        let is_attack = target_bright > current_brightness;
        let smoothed_brightness = if is_attack {
            // Attack - fade in
            current_brightness + (target_bright - current_brightness) * attack_factor
        } else {
            // Decay - fade out
            current_brightness + (target_bright - current_brightness) * decay_factor
        };

        smoothed_frame[led] = smoothed_brightness;

        // Apply brightness to the base RGB color
        // Color is stored as RGB (0-255), we just multiply by brightness factor
        let (base_r, base_g, base_b) = last_colors[led];

        // Calculate brightness factor (0.0 to 1.0)
        let brightness_factor = smoothed_brightness / 255.0;

        // Multiply each RGB component by brightness factor
        final_frame[offset] = (base_r as f32 * brightness_factor).round() as u8;
        final_frame[offset + 1] = (base_g as f32 * brightness_factor).round() as u8;
        final_frame[offset + 2] = (base_b as f32 * brightness_factor).round() as u8;

        // Track decaying LEDs for debug
        if !is_attack && smoothed_brightness > 1.0 && target_bright < 1.0 {
            found_decaying_led = Some((led, smoothed_brightness, current_brightness, target_bright,
                                       base_r, base_g, base_b,  // Store RGB for debug
                                       final_frame[offset], final_frame[offset+1], final_frame[offset+2]));
        }

        // Track new attacks (lowered threshold from 50 to 1 to catch all notes)
        if is_attack && target_bright > 1.0 && current_brightness < 1.0 {
            DEBUG_LED.with(|d| d.set(Some(led)));
            DEBUG_FRAME_COUNT.with(|c| c.set(0));
            if let Some(debug) = debug_info {
                let mut dbg = debug.lock().unwrap();
                dbg.clear();
                dbg.push(format!("=== Note ON at LED {}: target={:.2} brightness, decay_factor={:.6} ===",
                                 led, target_bright, decay_factor));
            }
        }
    }

    // Log decay info for any decaying LED we found
    if let Some((led, smoothed, current, _target, cr, cg, cb, r, g, b)) = found_decaying_led {
        // Check if this is the LED we're tracking
        let is_tracked = DEBUG_LED.with(|d| d.get() == Some(led));
        if is_tracked {
            let frame_count = DEBUG_FRAME_COUNT.with(|c| {
                let count = c.get() + 1;
                c.set(count);
                count
            });

            let brightness_drop = current - smoothed;
            let percent_dropped = (brightness_drop / current) * 100.0;

            let msg = format!("F{:3}: brightness {:.2}→{:.2} (drop={:.2}, {:.1}%) | factor={:.6} | Base RGB ({},{},{}) | Final RGB ({},{},{})",
                frame_count, current, smoothed, brightness_drop, percent_dropped,
                decay_factor, cr, cg, cb, r, g, b);

            if let Some(debug) = debug_info {
                let mut dbg = debug.lock().unwrap();
                dbg.push(msg);
                // Keep only last 25 lines
                if dbg.len() > 25 {
                    dbg.remove(0);
                }
            }

            // When decay completes, add final message
            if smoothed < 1.0 && current >= 1.0 {
                if let Some(debug) = debug_info {
                    let mut dbg = debug.lock().unwrap();
                    dbg.push(format!(">>> DECAY COMPLETE in {} frames <<<", frame_count));
                }
                DEBUG_LED.with(|d| d.set(None));  // Stop tracking this LED
            }
        }
    }

    // Return frame buffer for delayed sending
    Ok(final_frame)
}

/// Render one channel of VU meter
pub fn render_vu_channel(
    frame: &mut [u8],
    start_led: usize,
    end_led: usize,
    level: f32,  // 0.0 to 1.0
    direction: &str,
    animation_direction: &str,
    animation_offset: f64,
    gradient: Option<&colorgrad::Gradient>,
    colors: &[Rgb],
    solid_color: Rgb,
    is_left_channel: bool,
    intensity_colors: bool,  // Map level to gradient color (all LEDs same color)
    peak_hold_enabled: bool,
    peak_hold_led: Option<usize>,  // LED index (relative to start_led) for peak hold
    peak_hold_color: Rgb,
) {
    let num_leds = end_led - start_led;
    if num_leds == 0 {
        return;
    }

    // Apply threshold - don't light any LEDs if level is too low
    const SILENCE_THRESHOLD: f32 = 0.01;
    if level < SILENCE_THRESHOLD {
        // Turn off all LEDs in this channel
        for i in 0..num_leds {
            let led = start_led + i;
            let led_offset = led * 3;
            frame[led_offset] = 0;
            frame[led_offset + 1] = 0;
            frame[led_offset + 2] = 0;
        }
        return;
    }

    // Calculate how many LEDs to light based on level
    let lit_count = (level * num_leds as f32).round() as usize;

    for i in 0..num_leds {
        let led = start_led + i;
        let led_offset = led * 3;

        // Determine if this LED should be lit based on direction mode
        // For VU meters, "mirrored" means left channel fills LEFT, right channel fills RIGHT
        let should_light = match direction {
            "mirrored" => {
                // Left channel: fills from right edge going left (LED 599 down to 0)
                // Right channel: fills from left edge going right (LED 600 up to 1199)
                if is_left_channel {
                    // Position from the right edge of left channel
                    (num_leds - 1 - i) < lit_count
                } else {
                    // Position from the left edge of right channel
                    i < lit_count
                }
            }
            "opposing" => {
                // Left channel fills rightward to center, right fills leftward to center
                if is_left_channel {
                    i < lit_count
                } else {
                    (num_leds - 1 - i) < lit_count
                }
            }
            "left" => {
                // Both channels fill from left edge (start of their section)
                i < lit_count
            }
            "right" => {
                // Both channels fill from right edge (end of their section)
                (num_leds - 1 - i) < lit_count
            }
            _ => i < lit_count, // Default to left-to-right
        };

        if should_light {
            // Get color based on mode
            let (r, g, b) = if intensity_colors && gradient.is_some() {
                // Intensity Colors Mode: All LEDs same color based on level
                // NOTE: For VU mode, we're using the regular gradient which is cyclic
                // This is a limitation - ideally we'd pass intensity gradients here too
                // For now, use the gradient directly and accept slight color shift at 100%
                let grad = gradient.unwrap();
                let color = grad.at(level as f64);
                let rgba = color.to_rgba8();
                (rgba[0], rgba[1], rgba[2])
            } else {
                // Normal Mode: Spatial gradient with animation
                // Calculate gradient position with animation
                // Match bandwidth meter logic: offset is already in 0-1 range
                let base_pos = i as f64 / num_leds as f64;

                // Apply animation offset (match bandwidth meter direction logic)
                // "right" = subtract (moves right), "left" = add (moves left)
                let animated_pos = if animation_direction == "right" {
                    (1.0 + base_pos - animation_offset) % 1.0
                } else {
                    (base_pos + animation_offset) % 1.0
                };

                // Get color using gradient system (same as bandwidth meter)
                if let Some(grad) = gradient {
                    // Use gradient
                    let color = grad.at(animated_pos);
                    let rgba = color.to_rgba8();
                    (rgba[0], rgba[1], rgba[2])
                } else if colors.len() > 1 {
                    // Multiple solid colors - pick one based on position
                    let n = colors.len();
                    let segment_size = 1.0 / n as f64;
                    let color_index = ((animated_pos / segment_size).floor() as usize).min(n - 1);
                    let rgb = &colors[color_index];
                    (rgb.r, rgb.g, rgb.b)
                } else if !colors.is_empty() {
                    // Single color from array
                    let rgb = &colors[0];
                    (rgb.r, rgb.g, rgb.b)
                } else {
                    // Fallback to solid_color
                    (solid_color.r, solid_color.g, solid_color.b)
                }
            };

            frame[led_offset] = r;
            frame[led_offset + 1] = g;
            frame[led_offset + 2] = b;
        } else {
            // LED is off
            frame[led_offset] = 0;
            frame[led_offset + 1] = 0;
            frame[led_offset + 2] = 0;
        }
    }

    // Render peak hold LED if enabled
    if peak_hold_enabled {
        if let Some(peak_led_index) = peak_hold_led {
            if peak_led_index < num_leds {
                let led = start_led + peak_led_index;
                let led_offset = led * 3;
                frame[led_offset] = peak_hold_color.r;
                frame[led_offset + 1] = peak_hold_color.g;
                frame[led_offset + 2] = peak_hold_color.b;
            }
        }
    }
}
