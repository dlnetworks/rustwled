// RustWLED - Multi-mode LED visualization system for WLED devices
// Supports network bandwidth monitoring, MIDI input, live audio, and relay modes
use anyhow::Result;
use clap::Parser;
use crossterm::event::{self, poll, read, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ddp_rs::connection::DDPConnection;
use ddp_rs::protocol::{PixelConfig, ID};
use notify::{Config, Event as NotifyEvent, RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::text::{Line, Span};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;
use std::io::{self, Write};
use std::net::UdpSocket;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::broadcast;

mod midi;
mod audio;
mod types;
mod gradients;
mod renderer;
mod httpd;
mod relay;
mod webcam;
mod tron;
mod geometry;
mod sand;
mod config;
mod multi_device;
mod cert;

// Import shared types
use types::{ModeExitReason, InterpolationMode, Rgb, build_gradient_from_color};
use multi_device::{MultiDeviceConfig, MultiDeviceManager, WLEDDevice};

// Import renderer types
use renderer::{DirectionMode, SharedRenderState, Renderer};

// Import config types
use config::{Args, BandwidthConfig, resolve_tx_rx_colors};

// Detect OS type (Darwin/Linux) via uname
async fn detect_os(ssh_target: Option<&str>) -> Result<String> {
    let output = if let Some(target) = ssh_target {
        Command::new("ssh")
            .arg(target)
            .arg("uname")
            .output()
            .await?
    } else {
        Command::new("uname")
            .output()
            .await?
    };

    let os_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(os_name)
}

// Spawn bandwidth monitoring command based on OS
async fn spawn_bandwidth_monitor(args: &Args, config: &BandwidthConfig) -> Result<tokio::process::Child> {
    // Priority: config.ssh_host > args.host (for backwards compatibility)
    let ssh_host = if !config.ssh_host.is_empty() {
        Some(&config.ssh_host)
    } else {
        args.host.as_ref()
    };

    // Use ssh_user from config (CLI doesn't have --user flag, but users can use user@host format in --host)
    let ssh_user = if !config.ssh_user.is_empty() {
        Some(&config.ssh_user)
    } else {
        None
    };

    if let Some(host) = ssh_host {
        // For remote hosts, use a single SSH connection that auto-detects OS and runs appropriate command
        spawn_remote_monitor(host, ssh_user, &config.interface).await
    } else {
        // Local monitoring - detect OS
        let os = detect_os(None).await?;

        let child = if os == "Darwin" {
            // macOS: use netstat
            spawn_netstat_monitor(None, None, &config.interface).await?
        } else {
            // Linux: use /proc/net/dev
            spawn_procnet_monitor(None, None, &config.interface).await?
        };

        Ok(child)
    }
}

// Remote monitoring with OS auto-detection in a single SSH session
async fn spawn_remote_monitor(host: &String, user: Option<&String>, interface: &str) -> Result<tokio::process::Child> {
    // Construct SSH target: user@host or just host
    let ssh_target = if let Some(u) = user {
        format!("{}@{}", u, host)
    } else {
        host.clone()
    };

    // Parse comma-separated interfaces for egrep pattern (Linux)
    let interfaces: Vec<&str> = interface.split(',').map(|s| s.trim()).collect();
    let egrep_pattern = interfaces.join("|");

    // Create a script that detects OS and runs appropriate monitoring command
    // This all runs in ONE SSH session, so only ONE password prompt
    let script = format!(
        r#"
OS=$(uname)
if [ "$OS" = "Darwin" ]; then
    # macOS
    netstat -w 1 -I {}
else
    # Linux
    while true; do cat /proc/net/dev | egrep '({})'; sleep 1; done
fi
"#,
        interface, egrep_pattern
    );

    let child = Command::new("ssh")
        .arg(&ssh_target)
        .arg(&script)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    Ok(child)
}

// macOS: netstat -w 1 -I <interfaces>
async fn spawn_netstat_monitor(host: Option<&String>, user: Option<&String>, interface: &str) -> Result<tokio::process::Child> {
    let netstat_cmd = format!("netstat -w 1 -I {}", interface);

    let child = if let Some(h) = host {
        // Construct SSH target: user@host or just host
        let ssh_target = if let Some(u) = user {
            format!("{}@{}", u, h)
        } else {
            h.clone()
        };

        // SSH without pseudo-terminal - allows password prompt via stdin/stderr
        Command::new("ssh")
            .arg(&ssh_target)
            .arg(&netstat_cmd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?
    } else {
        Command::new("sh")
            .arg("-c")
            .arg(&netstat_cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?
    };

    Ok(child)
}

// Linux: poll /proc/net/dev and stream raw data
async fn spawn_procnet_monitor(host: Option<&String>, user: Option<&String>, interface: &str) -> Result<tokio::process::Child> {
    // Parse comma-separated interfaces for egrep pattern
    let interfaces: Vec<&str> = interface.split(',').map(|s| s.trim()).collect();
    let egrep_pattern = interfaces.join("|");

    // Simple script: just output raw /proc/net/dev lines every second
    // All calculation will be done in Rust
    let script = format!(
        "while true; do cat /proc/net/dev | egrep '({})'; sleep 1; done",
        egrep_pattern
    );

    let child = if let Some(h) = host {
        // Construct SSH target: user@host or just host
        let ssh_target = if let Some(u) = user {
            format!("{}@{}", u, h)
        } else {
            h.clone()
        };

        // SSH without pseudo-terminal - allows password prompt via stdin/stderr
        Command::new("ssh")
            .arg(&ssh_target)
            .arg(&script)
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?
    } else {
        Command::new("sh")
            .arg("-c")
            .arg(&script)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?
    };

    Ok(child)
}

fn get_timestamp() -> String {
    let now = SystemTime::now();
    let duration = now.duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();

    // Format as HH:MM:SS.mmm
    let hours = (secs / 3600) % 24;
    let minutes = (secs / 60) % 60;
    let seconds = secs % 60;

    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
}

// State for tracking bandwidth calculation per interface
struct InterfaceState {
    prev_rx_bytes: u64,
    prev_tx_bytes: u64,
    prev_time: Instant,
}

struct BandwidthTracker {
    interfaces: std::collections::HashMap<String, InterfaceState>,
}

impl BandwidthTracker {
    fn new() -> Self {
        BandwidthTracker {
            interfaces: std::collections::HashMap::new(),
        }
    }

    // Parse /proc/net/dev line and accumulate bandwidth
    // Returns Some when all interfaces have been processed (after collecting all lines)
    fn update_from_procnet_line(&mut self, line: &str) -> Option<(f64, f64)> {
        // Format: "  eth9: 12345 ... (16 fields total)"
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() != 2 {
            return None;
        }

        let iface = parts[0].trim();
        let fields: Vec<&str> = parts[1].trim().split_whitespace().collect();

        // /proc/net/dev format:
        // RX: bytes packets errs drop fifo frame compressed multicast
        // TX: bytes packets errs drop fifo colls carrier compressed
        if fields.len() < 16 {
            return None;
        }

        let rx_bytes = fields[0].parse::<u64>().ok()?;
        let tx_bytes = fields[8].parse::<u64>().ok()?;

        let now = Instant::now();

        if let Some(state) = self.interfaces.get(iface) {
            let time_delta = now.duration_since(state.prev_time).as_secs_f64();
            if time_delta > 0.0 {
                let rx_delta = rx_bytes.saturating_sub(state.prev_rx_bytes) as f64;
                let tx_delta = tx_bytes.saturating_sub(state.prev_tx_bytes) as f64;

                // Calculate kbps: (bytes * 8) / (time_seconds * 1000)
                let rx_kbps = (rx_delta * 8.0) / (time_delta * 1000.0);
                let tx_kbps = (tx_delta * 8.0) / (time_delta * 1000.0);

                self.interfaces.insert(
                    iface.to_string(),
                    InterfaceState {
                        prev_rx_bytes: rx_bytes,
                        prev_tx_bytes: tx_bytes,
                        prev_time: now,
                    },
                );

                // Return the bandwidth for this interface
                return Some((rx_kbps, tx_kbps));
            }
        }

        // First reading - just store values
        self.interfaces.insert(
            iface.to_string(),
            InterfaceState {
                prev_rx_bytes: rx_bytes,
                prev_tx_bytes: tx_bytes,
                prev_time: now,
            },
        );

        None
    }
}

fn parse_bandwidth_line(line: &str, tracker: &mut Option<BandwidthTracker>) -> Option<(f64, f64)> {
    let parts: Vec<&str> = line.trim().split_whitespace().collect();

    // macOS netstat format: 7 columns (packets errs bytes packets errs bytes colls)
    // Column 2 = input bytes/sec, Column 5 = output bytes/sec
    if parts.len() == 7 {
        let rx_bytes_per_sec = parts[2].parse::<f64>().ok()?;
        let tx_bytes_per_sec = parts[5].parse::<f64>().ok()?;

        // Convert bytes/sec to kbps
        let rx_kbps = (rx_bytes_per_sec * 8.0) / 1000.0;
        let tx_kbps = (tx_bytes_per_sec * 8.0) / 1000.0;

        Some((rx_kbps, tx_kbps))
    }
    // Linux /proc/net/dev format: interface: rx_bytes ... (has colon)
    else if line.contains(':') {
        if let Some(t) = tracker {
            t.update_from_procnet_line(line)
        } else {
            None
        }
    } else {
        None
    }
}

fn parse_led_numbers(test_str: &str) -> Result<Vec<usize>> {
    let mut leds = Vec::new();

    for part in test_str.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let range_parts: Vec<&str> = part.split('-').collect();
            if range_parts.len() == 2 {
                let start = range_parts[0].parse::<usize>()?;
                let end = range_parts[1].parse::<usize>()?;
                for i in start..=end {
                    leds.push(i);
                }
            }
        } else {
            leds.push(part.parse::<usize>()?);
        }
    }

    Ok(leds)
}


async fn test_mode(args: &Args) -> Result<()> {
    use crate::multi_device::{MultiDeviceConfig, MultiDeviceManager, WLEDDevice};

    let test_str = args.test.as_ref().unwrap();
    let led_numbers = parse_led_numbers(test_str)?;

    // Load config to get device configuration
    let config = BandwidthConfig::load().unwrap_or_default();

    // Get FPS from args or config, default to 10 FPS
    let fps = args.fps.unwrap_or(config.fps);
    let frame_time_ms = (1000.0 / fps) as u64;

    println!("Test mode: sequencing through LEDs {:?}", led_numbers);
    println!("Target FPS: {:.1} ({} ms per frame)", fps, frame_time_ms);

    // Setup multi-device or single device based on config
    let mut multi_device_manager: Option<MultiDeviceManager> = None;
    let mut single_ddp_conn: Option<DDPConnection> = None;

    if !config.wled_devices.is_empty() {
        println!("Using device configuration from config file:");
        for (idx, device) in config.wled_devices.iter().enumerate() {
            println!("  Device {}: {} (LEDs {}-{}, {})",
                idx,
                device.ip,
                device.led_offset,
                device.led_offset + device.led_count - 1,
                if device.enabled { "enabled" } else { "disabled" }
            );
        }

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

        match MultiDeviceManager::new(md_config) {
            Ok(manager) => {
                println!("Multi-device manager initialized with {} device(s)", manager.device_count());
                multi_device_manager = Some(manager);
            }
            Err(e) => {
                eprintln!("Failed to initialize multi-device manager: {}", e);
                return Err(e);
            }
        }
    } else {
        // Fall back to legacy single device
        let default_wled = "led.local".to_string();
        let wled_ip = args.wled_ip.as_ref().unwrap_or(&default_wled);
        println!("Connecting to WLED at {}:4048", wled_ip);

        let dest_addr = format!("{}:4048", wled_ip);
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        single_ddp_conn = Some(DDPConnection::try_new(&dest_addr, PixelConfig::default(), ID::Default, socket)?);
    }

    println!("Connected! Starting sequential LED test...");

    let test_color = Rgb::from_hex("FF0000")?;

    // Calculate frame size from device configuration
    let total_leds = if !config.wled_devices.is_empty() {
        config.wled_devices.iter()
            .map(|d| d.led_offset + d.led_count)
            .max()
            .unwrap_or(100)
    } else {
        config.total_leds
    };
    let frame_size = total_leds * 3;

    println!("Frame size: {} LEDs ({} bytes)", total_leds, frame_size);
    println!("Testing {} LEDs total", led_numbers.len());
    println!("Press Ctrl+C or 'q' to quit\n");

    // Enable raw mode for keyboard input
    use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
    use crossterm::event::{poll, read, Event, KeyCode};
    enable_raw_mode()?;

    // Setup Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            r.store(false, Ordering::SeqCst);
        }
    });

    // Continuous loop through each LED in the range
    'outer: while running.load(Ordering::SeqCst) {
        for &led_num in &led_numbers {
            // Check for keyboard input (non-blocking)
            if poll(std::time::Duration::from_millis(0))? {
                if let Event::Key(key_event) = read()? {
                    if matches!(key_event.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
                        running.store(false, Ordering::SeqCst);
                        break 'outer;
                    }
                }
            }

            if !running.load(Ordering::SeqCst) {
                break 'outer;
            }

            // Create frame with only this LED lit
            let mut frame = vec![0u8; frame_size];
            let offset = led_num * 3;
            if offset + 2 < frame_size {
                frame[offset] = test_color.r;
                frame[offset + 1] = test_color.g;
                frame[offset + 2] = test_color.b;
            } else {
                eprintln!("Skipping LED {} - exceeds frame size", led_num);
                continue;
            }

            // Send via multi-device or single device
            if let Some(manager) = multi_device_manager.as_mut() {
                if let Err(e) = manager.send_frame(&frame) {
                    eprintln!("Multi-device send error: {:?}", e);
                }
            } else if let Some(conn) = single_ddp_conn.as_mut() {
                conn.write(&frame)?;
            }

            print!("\rLED {} ON  ", led_num);
            use std::io::Write;
            std::io::stdout().flush()?;
            tokio::time::sleep(tokio::time::Duration::from_millis(frame_time_ms)).await;
        }
    }

    disable_raw_mode()?;
    println!("\nTest mode stopped.");
    Ok(())
}

// Run interactive first-time setup
fn run_first_time_setup(midi_mode: bool) -> Result<BandwidthConfig> {
    if midi_mode {
        println!("\n=== WLED MIDI Mode - First Time Setup ===\n");

        // 1. List available MIDI ports and let user select
        println!("Detecting MIDI input ports...\n");
        let midi_ports = match midi::list_midi_ports() {
            Ok(ports) if !ports.is_empty() => ports,
            Ok(_) => {
                eprintln!("Error: No MIDI input ports found!");
                eprintln!("Please ensure a MIDI device is connected or create a virtual MIDI port (e.g., IAC Bus on macOS).");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("Error detecting MIDI ports: {}", e);
                std::process::exit(1);
            }
        };

        println!("Available MIDI input ports:");
        for (i, port) in midi_ports.iter().enumerate() {
            println!("  {}. {}", i + 1, port);
        }

        // Prompt for MIDI port selection
        let midi_device = loop {
            print!("\nSelect MIDI port (1-{}): ", midi_ports.len());
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if let Ok(choice) = input.trim().parse::<usize>() {
                if choice > 0 && choice <= midi_ports.len() {
                    break midi_ports[choice - 1].clone();
                }
            }
            println!("Invalid selection. Please enter a number between 1 and {}", midi_ports.len());
        };

        println!("Selected: {}\n", midi_device);

        // 2. Prompt for Total LEDs
        let total_leds = loop {
            print!("Enter total number of LEDs in your strip: ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if let Ok(leds) = input.trim().parse::<usize>() {
                if leds > 0 {
                    break leds;
                }
            }
            println!("Invalid input. Please enter a positive number.");
        };

        println!();

        // 3. Prompt for WLED IP
        print!("Enter WLED IP address or hostname (e.g., led.local or 192.168.1.100): ");
        io::stdout().flush()?;
        let mut wled_ip = String::new();
        io::stdin().read_line(&mut wled_ip)?;
        let wled_ip = wled_ip.trim().to_string();

        if wled_ip.is_empty() {
            eprintln!("Error: WLED IP address is required!");
            std::process::exit(1);
        }

        println!("\n=== Configuration Summary ===");
        println!("MIDI Device: {}", midi_device);
        println!("Total LEDs: {}", total_leds);
        println!("WLED IP: {}", wled_ip);
        println!("\nAll other settings will use default values.");
        println!("You can modify these later via the config file or web interface at http://localhost:8080\n");

        // Create config with provided values and defaults
        let mut config = BandwidthConfig::default();
        config.midi_device = midi_device;
        config.total_leds = total_leds;
        config.wled_ip = wled_ip;

        // Save the config
        config.save()?;
        println!("Configuration saved to: {}\n", BandwidthConfig::config_path(None)?.display());
        println!("Starting MIDI mode...\n");

        // Give user a moment to read the summary
        thread::sleep(Duration::from_secs(2));

        Ok(config)
    } else {
        println!("\n=== RustWLED - First Time Setup ===\n");

        // 1. Query and display network interfaces
        println!("Detecting network interfaces...\n");
        let interfaces = httpd::get_network_interfaces()?;

        if interfaces.is_empty() {
            eprintln!("Error: No network interfaces found!");
            std::process::exit(1);
        }

        // Auto-select first interface by default
        let interface = interfaces[0].clone();
        println!("Auto-selected interface: {}\n", interface);

        // Auto-configure with sensible defaults
        let wled_ip = "led.local".to_string();
        let total_leds = 600;
        let max_gbps = 10.0;

        println!("\n=== Configuration Summary ===");
        println!("Interface: {}", interface);
        println!("WLED IP: {}", wled_ip);
        println!("Total LEDs: {}", total_leds);
        println!("Max Speed: {} Gbps", max_gbps);
        println!("\nAll other settings will use default values.");
        println!("You can modify these later via the config file or web interface at http://localhost:8080\n");

        // Create config with provided values and defaults
        let mut config = BandwidthConfig::default();
        config.interface = interface;
        config.wled_ip = wled_ip;
        config.total_leds = total_leds;
        config.max_gbps = max_gbps;

        // Save the config
        config.save()?;
        println!("Configuration saved to: {}\n", BandwidthConfig::config_path(None)?.display());
        println!("Starting RustWLED...\n");

        // Give user a moment to read the summary
        thread::sleep(Duration::from_secs(2));

        Ok(config)
    }
}

/// Generate compact config info display for TUI
fn generate_config_info_display(config: &BandwidthConfig) -> Vec<Line<'static>> {
    vec![
        Line::from(format!("═══ Network ═══════════════════════════════════════════════════════════════")),
        Line::from(format!("interface: {}  |  max_gbps: {}  |  ssh_host: {}  |  ssh_user: {}",
            config.interface, config.max_gbps, config.ssh_host, config.ssh_user)),
        Line::from(""),
        Line::from(format!("═══ Display ═══════════════════════════════════════════════════════════════")),
        Line::from(format!("wled_ip: {}  |  total_leds: {}  |  fps: {:.0}  |  direction: {}  |  swap: {}",
            config.wled_ip, config.total_leds, config.fps, config.direction, config.swap)),
        Line::from(format!("rx_split_percent: {:.0}%  |  use_gradient: {}  |  interpolation: {}  |  log_scale: {}",
            config.rx_split_percent, config.use_gradient, config.interpolation, config.log_scale)),
        Line::from(""),
        Line::from(format!("═══ Colors ════════════════════════════════════════════════════════════════")),
        Line::from(format!("color: {}...", if config.color.len() > 40 { &config.color[..40] } else { &config.color })),
        Line::from(format!("tx_color: {}  |  rx_color: {}",
            if config.tx_color.is_empty() { "(default)" } else { &config.tx_color },
            if config.rx_color.is_empty() { "(default)" } else { &config.rx_color })),
        Line::from(""),
        Line::from(format!("═══ Animation ═════════════════════════════════════════════════════════════")),
        Line::from(format!("animation_speed: {}  |  scale_animation_speed: {}  |  tx_direction: {}  |  rx_direction: {}",
            config.animation_speed, config.scale_animation_speed, config.tx_animation_direction, config.rx_animation_direction)),
        Line::from(format!("interpolation_time_ms: {}ms", config.interpolation_time_ms)),
        Line::from(""),
        Line::from(format!("═══ Strobe ════════════════════════════════════════════════════════════════")),
        Line::from(format!("strobe_on_max: {}  |  rate: {}Hz  |  duration: {}ms  |  color: {}",
            config.strobe_on_max, config.strobe_rate_hz, config.strobe_duration_ms, config.strobe_color)),
        Line::from(""),
        Line::from(format!("═══ Audio/MIDI ════════════════════════════════════════════════════════════")),
        Line::from(format!("midi_device: {}  |  midi_gradient: {}  |  midi_random_colors: {}  |  midi_velocity_colors: {}",
            config.midi_device, config.midi_gradient, config.midi_random_colors, config.midi_velocity_colors)),
        Line::from(format!("midi_one_to_one: {}  |  midi_channel_mode: {}  |  vu: {}  |  audio_device: {}",
            config.midi_one_to_one, config.midi_channel_mode, config.vu, config.audio_device)),
        Line::from(format!("attack_ms: {:.1}  |  decay_ms: {:.1}  |  ddp_delay_ms: {:.1}",
            config.attack_ms, config.decay_ms, config.ddp_delay_ms)),
        Line::from(""),
        Line::from(format!("═══ HTTP Server ═══════════════════════════════════════════════════════════")),
        Line::from(format!("httpd_enabled: {}  |  httpd_ip: {}  |  httpd_port: {}  |  httpd_auth_enabled: {}",
            config.httpd_enabled, config.httpd_ip, config.httpd_port, config.httpd_auth_enabled)),
        Line::from(""),
        Line::from(format!("═══ Test Mode ═════════════════════════════════════════════════════════════")),
        Line::from(format!("test_tx: {}  |  test_rx: {}  |  test_tx_percent: {}%  |  test_rx_percent: {}%",
            config.test_tx, config.test_rx, config.test_tx_percent, config.test_rx_percent)),
    ]
}

/// MIDI mode main loop with TUI
fn run_midi_mode(config: &BandwidthConfig, midi_device: Option<String>, random_colors: bool, config_change_tx: broadcast::Sender<()>) -> Result<ModeExitReason> {
    let device_name = midi_device.unwrap_or_else(|| config.midi_device.clone());

    // Create color map if random colors enabled
    let use_random = random_colors || config.midi_random_colors;
    let color_map = if use_random {
        Some(midi::generate_random_color_map())
    } else {
        None
    };

    // Create shared MIDI note state
    let note_state = midi::NoteState::new();
    let note_state_callback = note_state.clone();
    let note_state_render = note_state.clone();

    // Event log for TUI (store last 100 events)
    let event_log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let event_log_callback = event_log.clone();
    let color_map_callback = color_map.clone();
    let velocity_colors_callback = config.midi_velocity_colors;

    // Debug info for TUI (decay tracking)
    let debug_info: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // Connect to MIDI device
    println!("\n🎵 MIDI Mode");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let _midi_connection = midi::connect_midi(&device_name, move |_timestamp, message, _| {
        if let Some(event) = midi::parse_midi_message(message) {
            match event {
                midi::MidiEvent::NoteOn { channel, note, velocity } => {
                    note_state_callback.note_on(channel, note, velocity);

                    // Get actual brightness being used for rendering
                    let (display_color, actual_brightness) = if velocity_colors_callback {
                        // Velocity colors mode: color from velocity, full brightness
                        let color = midi::velocity_to_color(velocity);
                        (color, 255)
                    } else {
                        // Note colors mode: color from note, brightness from velocity
                        let color = midi::get_note_color(note, color_map_callback.as_ref());
                        let brightness = midi::velocity_to_brightness(velocity);
                        (color, brightness)
                    };

                    // Add to event log for TUI
                    let mut log = event_log_callback.lock().unwrap();
                    log.push(format!(
                        "[NOTE ON ] Ch:{:2} Note:{:3} ({:4})   RGB({:3},{:3},{:3})   Bright: {:3}",
                        channel + 1,  // Display as 1-16 instead of 0-15
                        note,
                        midi::note_number_to_name(note),
                        display_color.r,
                        display_color.g,
                        display_color.b,
                        actual_brightness
                    ));
                    if log.len() > 100 {
                        log.remove(0);
                    }
                }
                midi::MidiEvent::NoteOff { channel, note } => {
                    note_state_callback.note_off(channel, note);

                    // Add to event log for TUI
                    let mut log = event_log_callback.lock().unwrap();
                    log.push(format!(
                        "[NOTE OFF] Ch:{:2} Note:{:3} ({:4})",
                        channel + 1,  // Display as 1-16 instead of 0-15
                        note,
                        midi::note_number_to_name(note)
                    ));
                    if log.len() > 100 {
                        log.remove(0);
                    }
                }
            }
        }
    })?;

    // Setup multi-device manager for WLED
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

    let mut multi_device_manager = MultiDeviceManager::new(md_config)?;

    // Create smoothing state for attack/decay
    let mut smoothed_frame = vec![0.0_f32; config.total_leds];  // Current brightness per LED (smoothed)
    let mut target_brightness = vec![0.0_f32; config.total_leds];  // Target brightness per LED (independent of velocity functions)
    let mut last_colors = vec![(0_u8, 0_u8, 0_u8); config.total_leds];  // Base RGB color (0-255) per LED, brightness applied separately

    // Track current config values for real-time updates
    let mut current_config = config.clone();
    let mut current_fps = current_config.fps;
    let mut frame_time_ms = 1000.0 / current_fps;

    let mut attack_factor = (frame_time_ms / current_config.attack_ms as f64).min(1.0) as f32;
    let mut decay_factor = (frame_time_ms / current_config.decay_ms as f64).min(1.0) as f32;

    println!("\n✓ Connected to WLED at {}", config.wled_ip);
    println!("✓ LED Count: {}", config.total_leds);
    println!("✓ Running at {:.1} FPS ({:.2}ms per frame)", current_fps, frame_time_ms);
    println!("✓ Attack: {:.1}ms (factor: {:.6}, ~{} frames to complete)",
             current_config.attack_ms, attack_factor, (current_config.attack_ms as f64 / frame_time_ms).ceil() as u32);
    println!("✓ Decay: {:.1}ms (factor: {:.6}, ~{} frames to complete)",
             current_config.decay_ms, decay_factor, (current_config.decay_ms as f64 / frame_time_ms).ceil() as u32);
    println!("✓ Velocity colors: {}", if current_config.midi_velocity_colors { "enabled" } else { "disabled" });
    println!("✓ Debug log: /tmp/midi_decay_debug.log");
    println!("\n🎹 Play some notes! Press 'q' to quit.\n");

    // Subscribe to SSE broadcast channel for config changes (no file watching needed)
    let mut config_change_rx = config_change_tx.subscribe();

    // Setup terminal for TUI
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Frame buffer for delay - stores (send_time, frame_data)
    let mut frame_buffer: std::collections::VecDeque<(Instant, Vec<u8>)> = std::collections::VecDeque::new();

    // Config info toggle
    let mut show_config_info = false;

    // Main loop - use global fps from config
    let mut frame_duration = Duration::from_secs_f64(1.0 / current_fps);

    loop {
        let loop_start = Instant::now();

        // Check for keyboard input with brief timeout for better responsiveness
        if poll(Duration::from_millis(10))? {
            if let Event::Key(key) = read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        terminal.show_cursor()?;
                        disable_raw_mode()?;
                        terminal.backend_mut().execute(LeaveAlternateScreen)?;
                        println!("\n👋 MIDI mode stopped.\n");
                        return Ok(ModeExitReason::UserQuit);
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        terminal.show_cursor()?;
                        disable_raw_mode()?;
                        terminal.backend_mut().execute(LeaveAlternateScreen)?;
                        println!("\n👋 MIDI mode stopped.\n");
                        return Ok(ModeExitReason::UserQuit);
                    }
                    KeyCode::Char('i') | KeyCode::Char('I') => {
                        show_config_info = !show_config_info;
                        terminal.clear()?;
                    },
                    _ => {}
                }
            }
        }

        // Check for config updates via SSE broadcast
        if let Ok(()) = config_change_rx.try_recv() {
            let new_config = match BandwidthConfig::load() {
                Ok(c) => c,
                Err(_) => continue, // Skip if load fails
            };

            // Update FPS if changed
            if new_config.fps != current_config.fps {
                current_fps = new_config.fps;
                frame_time_ms = 1000.0 / current_fps;
                frame_duration = Duration::from_secs_f64(1.0 / current_fps);
            }

            // Update attack/decay if changed
            if new_config.attack_ms != current_config.attack_ms || new_config.fps != current_config.fps {
                attack_factor = (frame_time_ms / new_config.attack_ms as f64).min(1.0) as f32;
            }
            if new_config.decay_ms != current_config.decay_ms || new_config.fps != current_config.fps {
                decay_factor = (frame_time_ms / new_config.decay_ms as f64).min(1.0) as f32;
            }

            // Resize smoothed frame if total_leds changed
            if new_config.total_leds != current_config.total_leds {
                smoothed_frame.resize(new_config.total_leds, 0.0);
                target_brightness.resize(new_config.total_leds, 0.0);
                last_colors.resize(new_config.total_leds, (0, 0, 0));
            }

            // Reinitialize multi-device manager if device config changed
            let devices_changed = new_config.wled_devices.len() != current_config.wled_devices.len() ||
                new_config.wled_devices.iter().zip(current_config.wled_devices.iter()).any(|(new, old)| {
                    new.ip != old.ip ||
                    new.led_offset != old.led_offset ||
                    new.led_count != old.led_count ||
                    new.enabled != old.enabled
                }) ||
                new_config.multi_device_send_parallel != current_config.multi_device_send_parallel ||
                new_config.multi_device_fail_fast != current_config.multi_device_fail_fast;

            if devices_changed {
                let devices: Vec<WLEDDevice> = new_config.wled_devices.iter().map(|d| WLEDDevice {
                    ip: d.ip.clone(),
                    led_offset: d.led_offset,
                    led_count: d.led_count,
                            enabled: d.enabled,
                }).collect();

                let md_config = MultiDeviceConfig {
                    devices,
                    send_parallel: new_config.multi_device_send_parallel,
                    fail_fast: new_config.multi_device_fail_fast,
                };

                match MultiDeviceManager::new(md_config) {
                    Ok(new_manager) => {
                        multi_device_manager = new_manager;
                        println!("\n✓ Reinitialized multi-device manager");
                    }
                    Err(e) => {
                        eprintln!("\n⚠️  Failed to reinitialize multi-device manager: {}", e);
                        eprintln!("   Continuing with previous configuration");
                    }
                }
            }

            // Check if mode changed - if so, exit MIDI mode to allow mode switch
            if new_config.mode != "midi" {
                println!("\n🔄 Mode changed to '{}', exiting MIDI mode...", new_config.mode);
                terminal.show_cursor()?;
                disable_raw_mode()?;
                terminal.backend_mut().execute(LeaveAlternateScreen)?;
                return Ok(ModeExitReason::ModeChanged);
            }

            // Check if MIDI device changed - if so, exit and restart with new device
            if new_config.midi_device != current_config.midi_device {
                println!("\n🔄 MIDI device changed to '{}', restarting MIDI mode...", new_config.midi_device);
                terminal.show_cursor()?;
                disable_raw_mode()?;
                terminal.backend_mut().execute(LeaveAlternateScreen)?;
                return Ok(ModeExitReason::ModeChanged);
            }

            current_config = new_config;
        }

        // Render MIDI state to LEDs with attack/decay smoothing
        let frame = renderer::render_midi_to_leds(
            &note_state_render,
            current_config.total_leds,
            current_config.midi_gradient,
            color_map.as_ref(),
            current_config.midi_velocity_colors,
            current_config.midi_one_to_one,
            current_config.midi_channel_mode,
            &mut smoothed_frame,
            &mut target_brightness,
            &mut last_colors,
            attack_factor,
            decay_factor,
            Some(&debug_info),
        )?;

        // Add frame to buffer with scheduled send time
        let delay_duration = Duration::from_micros((current_config.ddp_delay_ms * 1000.0) as u64);
        let send_time = loop_start + delay_duration;
        frame_buffer.push_back((send_time, frame));

        // Send all frames that are ready (send_time <= now)
        let now = Instant::now();
        while let Some((send_time, _)) = frame_buffer.front() {
            if *send_time <= now {
                if let Some((_, frame_to_send)) = frame_buffer.pop_front() {
                    let _ = multi_device_manager.send_frame_with_brightness(&frame_to_send, Some(current_config.global_brightness));
                }
            } else {
                break;
            }
        }

        // Update TUI
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),  // Header
                    Constraint::Min(10),    // Main content
                    Constraint::Length(3),  // Footer
                ])
                .split(f.size());

            // Header - Mode and sub-mode
            let active_count = note_state_render.count();
            let sub_mode = if current_config.midi_channel_mode {
                "Channel Mode"
            } else if current_config.midi_one_to_one {
                "1-to-1 Mode"
            } else {
                "Spread Mode"
            };
            let header_text = format!("🎹 MIDI Mode | Sub-mode: {} | Active Notes: {}", sub_mode, active_count);
            let header = Paragraph::new(header_text)
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, chunks[0]);

            // Main content - either config info or event log/debug
            if show_config_info {
                let config_lines = generate_config_info_display(&current_config);
                let config_widget = Paragraph::new(config_lines)
                    .block(Block::default().borders(Borders::ALL).title("Configuration (Press 'i' to hide)"));
                f.render_widget(config_widget, chunks[1]);
            } else {
                // Split main area for event log and debug info
                let main_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Percentage(50),
                        Constraint::Percentage(50),
                    ])
                    .split(chunks[1]);

                // Event log
                let log = event_log.lock().unwrap();
                let log_text: Vec<Line> = log.iter().map(|s| Line::from(s.as_str())).collect();
                let log_widget = Paragraph::new(log_text)
                    .block(Block::default().borders(Borders::ALL).title("MIDI Events"));
                f.render_widget(log_widget, main_chunks[0]);

                // Debug info
                let debug = debug_info.lock().unwrap();
                let debug_text: Vec<Line> = debug.iter().map(|s| Line::from(s.as_str())).collect();
                let debug_widget = Paragraph::new(debug_text)
                    .block(Block::default().borders(Borders::ALL).title("Attack/Decay Debug"));
                f.render_widget(debug_widget, main_chunks[1]);
            }

            // Footer - Monitoring source and controls
            let footer_text = format!(
                "Source: MIDI [{}] | WLED: {} | LEDs: {} | FPS: {:.0} | Delay: {:.1}ms | Press 'i' for config, 'q' or Ctrl+C to quit",
                current_config.midi_device, current_config.wled_ip, current_config.total_leds, current_fps, current_config.ddp_delay_ms
            );
            let footer = Paragraph::new(footer_text)
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(footer, chunks[2]);
        })?;

        // Frame rate limiting
        let elapsed = loop_start.elapsed();
        if elapsed < frame_duration {
            thread::sleep(frame_duration - elapsed);
        }
    }
}

/// Live audio spectrum visualization mode
fn run_live_mode(config: &BandwidthConfig, delay_ms: Option<u64>, config_change_tx: broadcast::Sender<()>) -> Result<ModeExitReason> {
    use cpal::traits::{DeviceTrait, StreamTrait};
    use cpal::SampleFormat;
    use rustfft::{FftPlanner, num_complex::Complex};
    use std::sync::{Arc, Mutex};
    use std::collections::VecDeque;
    use std::io::Write;

    println!("\n=== Live Audio Spectrum Visualization ===\n");

    // Use audio device from config if available, otherwise prompt user
    let selected_device_name = if !config.audio_device.is_empty() {
        println!("Using audio device from config: {}", config.audio_device);
        config.audio_device.clone()
    } else {
        // List available audio devices using the working audio module
        let device_list = audio::list_audio_devices()?;

        if device_list.is_empty() {
            return Err(anyhow::anyhow!("No audio devices found"));
        }

        println!("Available audio devices:");
        for (i, (name, _is_output)) in device_list.iter().enumerate() {
            println!("  {}. {}", i + 1, name);
        }

        // Use first device as default for this session only
        let selected = device_list[0].0.clone();
        println!("\nAuto-selected first device: {}", selected);
        println!("(Set this in the web UI or config file to persist)");

        selected
    };

    // Find the actual device
    let device = audio::find_audio_device(&selected_device_name)?;

    // Get device config
    let device_config = device.default_input_config()?;
    let sample_rate = device_config.sample_rate().0 as f32;
    let sample_format = device_config.sample_format();

    println!("Sample rate: {} Hz", sample_rate);
    println!("Channels: {}", device_config.channels());
    println!("Format: {:?}", sample_format);

    println!("\nStarting in 2 seconds...");
    thread::sleep(Duration::from_millis(2000));

    // FFT setup - balanced window for responsive transients with good frequency resolution
    let fft_size = 1024;  // ~23ms latency at 44.1kHz - great balance!
    // Gives 43 Hz per bin - still very detailed frequency separation
    let min_freq = 1.0_f32;
    let max_freq = 22050.0_f32;

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    let freq_bin_width = sample_rate / fft_size as f32;
    let min_bin = (min_freq / freq_bin_width).round() as usize;
    let max_bin = ((max_freq / freq_bin_width).round() as usize).min(fft_size / 2 - 1);

    println!("FFT size: {}", fft_size);
    println!("Frequency range: {} Hz - {} Hz", min_freq, max_freq);
    println!("Frequency per bin: {:.2} Hz", freq_bin_width);
    println!("Hz per LED: {:.2}", (max_freq - min_freq) / config.total_leds as f32);

    // Audio buffer - shared between audio thread and processing thread
    let audio_buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
    let audio_buffer_clone = audio_buffer.clone();

    let channels = device_config.channels() as usize;
    println!("Audio has {} channel(s)", channels);

    // Build audio stream
    println!("\nStarting audio capture...");

    let stream = match sample_format {
        SampleFormat::F32 => {
            let channels = channels;
            device.build_input_stream(
                &device_config.into(),
                move |data: &[f32], _| {
                    let mut buffer = audio_buffer_clone.lock().unwrap();

                    // For stereo, store interleaved samples - we'll analyze separately later
                    // For mono, just store as-is
                    buffer.extend_from_slice(data);

                    // Keep last 2 seconds
                    let max_size = (sample_rate * 2.0) as usize * channels;
                    if buffer.len() > max_size {
                        let drain = buffer.len() - max_size;
                        buffer.drain(0..drain);
                    }
                },
                |err| eprintln!("Audio error: {}", err),
                None,
            )?
        },
        SampleFormat::I16 => {
            let channels = channels;
            device.build_input_stream(
                &device_config.into(),
                move |data: &[i16], _| {
                    let mut buffer = audio_buffer_clone.lock().unwrap();

                    // Store interleaved samples - we'll analyze separately later
                    buffer.extend(data.iter().map(|&s| s as f32 / 32768.0));

                    // Keep last 2 seconds
                    let max_size = (sample_rate * 2.0) as usize * channels;
                    if buffer.len() > max_size {
                        let drain = buffer.len() - max_size;
                        buffer.drain(0..drain);
                    }
                },
                |err| eprintln!("Audio error: {}", err),
                None,
            )?
        },
        SampleFormat::U16 => {
            let channels = channels;
            device.build_input_stream(
                &device_config.into(),
                move |data: &[u16], _| {
                    let mut buffer = audio_buffer_clone.lock().unwrap();

                    // Store interleaved samples - we'll analyze separately later
                    buffer.extend(data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0));

                    // Keep last 2 seconds
                    let max_size = (sample_rate * 2.0) as usize * channels;
                    if buffer.len() > max_size {
                        let drain = buffer.len() - max_size;
                        buffer.drain(0..drain);
                    }
                },
                |err| eprintln!("Audio error: {}", err),
                None,
            )?
        },
        _ => {
            eprintln!("Unsupported sample format: {:?}", sample_format);
            std::process::exit(1);
        }
    };

    stream.play()?;
    println!("Audio stream started");

    // Setup multi-device manager
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

    let mut multi_device_manager = MultiDeviceManager::new(md_config)?;

    println!("Connected to WLED at {}", config.wled_ip);

    if let Some(delay) = delay_ms {
        println!("Audio/Video sync delay: {} ms", delay);
    }

    println!("\nStarting visualization... Press Ctrl+C to stop\n");

    thread::sleep(Duration::from_millis(500));

    // Build spectrum gradients from color config (same system as bandwidth mode)
    let spectrum_color_str = if !config.color.is_empty() {
        gradients::resolve_color_string(&config.color)
    } else {
        "FF0000,FF7F00,FFFF00,00FF00,0000FF,4B0082,9400D3".to_string() // Default rainbow
    };

    let interpolation_mode = match config.interpolation.to_lowercase().as_str() {
        "basis" => InterpolationMode::Basis,
        "catmullrom" => InterpolationMode::CatmullRom,
        _ => InterpolationMode::Linear,
    };

    let (mut spectrum_gradient, mut spectrum_colors, mut spectrum_solid) =
        build_gradient_from_color(&spectrum_color_str, config.use_gradient, interpolation_mode)?;

    // Track current config values for real-time updates
    let mut current_config = config.clone();
    let mut smoothed_magnitudes = vec![0.0_f32; current_config.total_leds];
    let threshold = 0.12; // Balanced threshold - sensitive but not too noisy
    let mut frame_count = 0u64;

    // VU meter animation offset tracking
    let mut left_animation_offset = 0.0_f64;
    let mut right_animation_offset = 0.0_f64;

    // VU meter peak hold tracking
    let mut left_peak_led: Option<usize> = None;
    let mut left_peak_time: Option<Instant> = None;
    let mut right_peak_led: Option<usize> = None;
    let mut right_peak_time: Option<Instant> = None;

    // Peak direction toggle - track current animation directions for each channel
    let mut left_animation_direction = current_config.rx_animation_direction.clone();
    let mut right_animation_direction = current_config.tx_animation_direction.clone();

    // Track display levels for TUI
    let mut display_left_level = 0.0_f32;
    let mut display_right_level = 0.0_f32;

    // Spectrogram buffer: stores frequency data over time for scrolling visualization
    // Spectrogram REQUIRES 2D matrix mode (frequency vs time)
    let (spec_width, spec_height) = (current_config.matrix_2d_width, current_config.matrix_2d_height);
    // Store as 2D buffer: spectrogram_buffer[time_column][freq_row] = magnitude
    let mut spectrogram_buffer: Vec<Vec<f32>> = vec![vec![0.0; spec_height]; spec_width];
    let mut spec_scroll_accumulator = 0.0_f64;  // Accumulates fractional scroll pixels

    // Store color strings for TUI rendering (gradients will be rebuilt)
    // Initialize with config values, using unified color resolution system
    // Channel mapping: TX=Right, RX=Left
    let (tx_color_resolved, rx_color_resolved) = resolve_tx_rx_colors(&current_config);
    let mut tui_left_color_str = rx_color_resolved;  // Left channel = RX
    let mut tui_right_color_str = tx_color_resolved;  // Right channel = TX
    let mut tui_use_gradient = current_config.use_gradient;
    let mut tui_interpolation_mode = match current_config.interpolation.as_str() {
        "basis" => InterpolationMode::Basis,
        "catmullrom" => InterpolationMode::CatmullRom,
        _ => InterpolationMode::Linear,
    };
    let mut tui_left_animation_offset = 0.0_f64;
    let mut tui_right_animation_offset = 0.0_f64;

    // Calculate attack/decay factors from config (ms to per-frame multiplier)
    let mut current_fps = current_config.fps;
    let mut frame_time_ms = 1000.0 / current_fps;
    let mut attack_factor = (frame_time_ms / current_config.attack_ms as f64).min(1.0);
    let mut decay_factor = (frame_time_ms / current_config.decay_ms as f64).min(1.0);

    println!("Running at {} FPS ({:.2}ms per frame)", current_fps, frame_time_ms);
    println!("Attack: {}ms ({:.3} per frame, ~{} frames), Decay: {}ms ({:.3} per frame, ~{} frames)",
             current_config.attack_ms, attack_factor, (current_config.attack_ms as f64 / frame_time_ms) as u32,
             current_config.decay_ms, decay_factor, (current_config.decay_ms as f64 / frame_time_ms) as u32);

    if current_config.spectrogram {
        println!("\n📈 SPECTROGRAM MODE ENABLED");
        println!("   Scroll direction: {}", current_config.spectrogram_scroll_direction);
        println!("   Scroll speed: {} pixels/sec", current_config.spectrogram_scroll_speed);
        println!("   Color mode: {}", current_config.spectrogram_color_mode);
        println!("   Window size: {} samples", current_config.spectrogram_window_size);
    } else if current_config.vu {
        println!("\n🎚️  VU METER MODE ENABLED");
        println!("   Left channel:  LEDs 0-{}", current_config.total_leds / 2 - 1);
        println!("   Right channel: LEDs {}-{}", current_config.total_leds / 2, current_config.total_leds - 1);
    } else {
        println!("\n📊 FFT SPECTRUM MODE");
    }

    // Give user a moment to read startup messages
    thread::sleep(Duration::from_millis(1000));

    // Clear screen and setup TUI
    print!("\x1B[2J\x1B[1;1H");
    io::stdout().flush()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    terminal.hide_cursor()?;

    // Subscribe to SSE broadcast channel for config changes (no file watching needed)
    let mut config_change_rx = config_change_tx.subscribe();

    // Frame buffer for delay - stores (send_time, frame_data)
    let mut frame_buffer: VecDeque<(Instant, Vec<u8>)> = VecDeque::new();

    // Config info toggle
    let mut show_config_info = false;

    // Main loop - use global fps from config
    let mut frame_duration = Duration::from_secs_f64(1.0 / current_fps);

    loop {
        let loop_start = Instant::now();
        frame_count += 1;

        // Check for keyboard input
        if poll(Duration::from_millis(0))? {
            if let Event::Key(key) = read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('i') | KeyCode::Char('I') => {
                        show_config_info = !show_config_info;
                        terminal.clear()?;
                    },
                    _ => {}
                }
            }
        }

        // Check for config updates via SSE broadcast
        if let Ok(()) = config_change_rx.try_recv() {
            let new_config = match BandwidthConfig::load() {
                Ok(c) => c,
                Err(_) => continue, // Skip if load fails
            };

            // Update FPS if changed
            if new_config.fps != current_config.fps {
                current_fps = new_config.fps;
                frame_time_ms = 1000.0 / current_fps;
                frame_duration = Duration::from_secs_f64(1.0 / current_fps);
            }

            // Update attack/decay if changed
            if new_config.attack_ms != current_config.attack_ms || new_config.fps != current_config.fps {
                attack_factor = (frame_time_ms / new_config.attack_ms as f64).min(1.0);
            }
            if new_config.decay_ms != current_config.decay_ms || new_config.fps != current_config.fps {
                decay_factor = (frame_time_ms / new_config.decay_ms as f64).min(1.0);
            }

            // Resize smoothed magnitudes if total_leds changed
            if new_config.total_leds != current_config.total_leds {
                smoothed_magnitudes.resize(new_config.total_leds, 0.0);
            }

            // Update spectrum gradient if color or interpolation settings changed (for FFT mode)
            if new_config.color != current_config.color ||
               new_config.use_gradient != current_config.use_gradient ||
               new_config.interpolation != current_config.interpolation {
                let new_spectrum_color_str = if !new_config.color.is_empty() {
                    gradients::resolve_color_string(&new_config.color)
                } else {
                    "FF0000,FF7F00,FFFF00,00FF00,0000FF,4B0082,9400D3".to_string()
                };

                let new_interpolation_mode = match new_config.interpolation.to_lowercase().as_str() {
                    "basis" => InterpolationMode::Basis,
                    "catmullrom" => InterpolationMode::CatmullRom,
                    _ => InterpolationMode::Linear,
                };

                if let Ok((grad, colors, solid)) = build_gradient_from_color(&new_spectrum_color_str, new_config.use_gradient, new_interpolation_mode) {
                    spectrum_gradient = grad;
                    spectrum_colors = colors;
                    spectrum_solid = solid;
                }
            }

            // Update VU meter TUI colors if color settings changed (for VU mode)
            if new_config.color != current_config.color ||
               new_config.tx_color != current_config.tx_color ||
               new_config.rx_color != current_config.rx_color ||
               new_config.use_gradient != current_config.use_gradient ||
               new_config.interpolation != current_config.interpolation {
                // Use unified color resolution system
                // Channel mapping: TX=Right, RX=Left
                let (tx_color_resolved, rx_color_resolved) = resolve_tx_rx_colors(&new_config);
                tui_left_color_str = rx_color_resolved;  // Left channel = RX
                tui_right_color_str = tx_color_resolved;  // Right channel = TX
                tui_use_gradient = new_config.use_gradient;
                tui_interpolation_mode = match new_config.interpolation.as_str() {
                    "basis" => InterpolationMode::Basis,
                    "catmullrom" => InterpolationMode::CatmullRom,
                    _ => InterpolationMode::Linear,
                };
            }

            // Reinitialize multi-device manager if device config changed
            let devices_changed = new_config.wled_devices.len() != current_config.wled_devices.len() ||
                new_config.wled_devices.iter().zip(current_config.wled_devices.iter()).any(|(new, old)| {
                    new.ip != old.ip ||
                    new.led_offset != old.led_offset ||
                    new.led_count != old.led_count ||
                    new.enabled != old.enabled
                }) ||
                new_config.multi_device_send_parallel != current_config.multi_device_send_parallel ||
                new_config.multi_device_fail_fast != current_config.multi_device_fail_fast;

            if devices_changed {
                let devices: Vec<WLEDDevice> = new_config.wled_devices.iter().map(|d| WLEDDevice {
                    ip: d.ip.clone(),
                    led_offset: d.led_offset,
                    led_count: d.led_count,
                            enabled: d.enabled,
                }).collect();

                let md_config = MultiDeviceConfig {
                    devices,
                    send_parallel: new_config.multi_device_send_parallel,
                    fail_fast: new_config.multi_device_fail_fast,
                };

                match MultiDeviceManager::new(md_config) {
                    Ok(new_manager) => {
                        multi_device_manager = new_manager;
                        println!("\n✓ Reinitialized multi-device manager");
                    }
                    Err(e) => {
                        eprintln!("\n⚠️  Failed to reinitialize multi-device manager: {}", e);
                        eprintln!("   Continuing with previous configuration");
                    }
                }
            }

            // Check if mode changed - if so, exit live mode to allow mode switch
            if new_config.mode != "live" {
                println!("\n🔄 Mode changed to '{}', exiting Live Audio mode...", new_config.mode);
                terminal.show_cursor()?;
                disable_raw_mode()?;
                terminal.backend_mut().execute(LeaveAlternateScreen)?;
                return Ok(ModeExitReason::ModeChanged);
            }

            // Check if audio device changed - if so, exit and restart with new device
            if new_config.audio_device != current_config.audio_device && !new_config.audio_device.is_empty() {
                println!("\n🔄 Audio device changed to '{}', restarting Live Audio mode...", new_config.audio_device);
                terminal.show_cursor()?;
                disable_raw_mode()?;
                terminal.backend_mut().execute(LeaveAlternateScreen)?;
                return Ok(ModeExitReason::ModeChanged);
            }

            current_config = new_config;

            // Update animation directions if peak toggle is disabled
            if !current_config.peak_direction_toggle {
                left_animation_direction = current_config.rx_animation_direction.clone();
                right_animation_direction = current_config.tx_animation_direction.clone();
            }
        }

        // Get audio samples (interleaved if stereo)
        // For VU mode, use smaller sample window (512) for faster response
        // For FFT mode, use larger window (1024) for better frequency resolution
        let sample_window = if current_config.vu { 512 } else { fft_size };
        let mut samples = {
            let buffer = audio_buffer.lock().unwrap();
            let needed_samples = sample_window * channels;
            if buffer.len() >= needed_samples {
                buffer[buffer.len() - needed_samples..].to_vec()
            } else {
                vec![0.0; needed_samples]
            }
        };

        // Apply audio gain adjustment
        // Gain formula: multiplier = 1.0 + (audio_gain / 100.0)
        // audio_gain = 0 → multiplier = 1.0 (no change)
        // audio_gain = 100 → multiplier = 2.0 (double amplitude)
        // audio_gain = -100 → multiplier = 0.0 (muted)
        if current_config.audio_gain != 0.0 {
            let gain_multiplier = 1.0 + (current_config.audio_gain / 100.0);
            for sample in samples.iter_mut() {
                *sample *= gain_multiplier as f32;
            }
        }

        // Create frame buffer
        let mut frame = vec![0u8; current_config.total_leds * 3];

        // VU METER MODE or SPECTROGRAM MODE or FFT SPECTRUM MODE
        if current_config.spectrogram {
            // === SPECTROGRAM MODE ===
            // Scrolling frequency visualization (like FFmpeg showspec or Winamp voiceprint)

            // 1. Perform FFT on audio samples
            let window_size = current_config.spectrogram_window_size.min(samples.len() / channels);
            let mut fft_input = vec![Complex::new(0.0, 0.0); window_size];

            // Apply audio gain
            let gain_multiplier = 1.0 + (current_config.audio_gain / 100.0);

            // Mix down to mono for FFT analysis
            for i in 0..window_size {
                let sample_idx = i * channels;
                let mono_sample = if channels >= 2 {
                    (samples[sample_idx] + samples[sample_idx + 1]) / 2.0  // Average L+R
                } else {
                    samples[sample_idx]
                };
                fft_input[i] = Complex::new(mono_sample * gain_multiplier as f32, 0.0);
            }

            // Perform FFT
            let mut planner = FftPlanner::new();
            let fft = planner.plan_fft_forward(window_size);
            fft.process(&mut fft_input);

            // 2. Extract frequency magnitudes (only positive frequencies)
            let freq_bins = window_size / 2;
            let mut freq_magnitudes = Vec::with_capacity(spec_height);

            // Map frequency bins to LED rows (log scale for better visual)
            for row in 0..spec_height {
                let freq_ratio = (row as f64 / spec_height as f64).powf(2.0);  // Exponential mapping
                let bin_idx = (freq_ratio * freq_bins as f64).min((freq_bins - 1) as f64) as usize;
                let magnitude = (fft_input[bin_idx].re * fft_input[bin_idx].re +
                                fft_input[bin_idx].im * fft_input[bin_idx].im).sqrt();
                freq_magnitudes.push(magnitude * 4.0);  // Scale for visibility
            }

            // 3. Scroll the spectrogram buffer
            spec_scroll_accumulator += current_config.spectrogram_scroll_speed * (frame_time_ms / 1000.0);
            let pixels_to_scroll = spec_scroll_accumulator.floor() as usize;
            spec_scroll_accumulator -= pixels_to_scroll as f64;

            if pixels_to_scroll > 0 {
                match current_config.spectrogram_scroll_direction.as_str() {
                    "right" => {
                        // Shift all columns to the right, insert new data at left
                        for _ in 0..pixels_to_scroll {
                            spectrogram_buffer.rotate_right(1);
                            spectrogram_buffer[0] = freq_magnitudes.clone();
                        }
                    }
                    "left" => {
                        // Shift all columns to the left, insert new data at right
                        for _ in 0..pixels_to_scroll {
                            spectrogram_buffer.rotate_left(1);
                            spectrogram_buffer[spec_width - 1] = freq_magnitudes.clone();
                        }
                    }
                    "down" => {
                        // Transpose: time is vertical, frequency is horizontal
                        // Shift rows down, insert new data at top
                        for _ in 0..pixels_to_scroll {
                            for col in 0..spec_width {
                                spectrogram_buffer[col].rotate_right(1);
                                let freq_idx = (col * spec_height) / spec_width;
                                spectrogram_buffer[col][0] = freq_magnitudes[freq_idx.min(spec_height - 1)];
                            }
                        }
                    }
                    "up" => {
                        // Transpose: time is vertical, frequency is horizontal
                        // Shift rows up, insert new data at bottom
                        for _ in 0..pixels_to_scroll {
                            for col in 0..spec_width {
                                spectrogram_buffer[col].rotate_left(1);
                                let freq_idx = (col * spec_height) / spec_width;
                                spectrogram_buffer[col][spec_height - 1] = freq_magnitudes[freq_idx.min(spec_height - 1)];
                            }
                        }
                    }
                    _ => {}  // Unknown direction, do nothing
                }
            }

            // 4. Map 2D spectrogram buffer to LED frame with color mapping
            // For spectrogram, always use a gradient (default to rainbow if none specified)
            let spec_gradient_str = if spectrum_color_str.contains(',') || spectrum_color_str.contains("rainbow") {
                spectrum_color_str.clone()
            } else {
                "rainbow".to_string()
            };
            let (gradient, _, _) = build_gradient_from_color(
                &spec_gradient_str,
                true,  // Always use gradient for spectrogram
                interpolation_mode,
            )?;

            // Find max magnitude in entire buffer for normalization
            let mut buffer_max = 0.0_f32;
            for col in &spectrogram_buffer {
                for &mag in col {
                    buffer_max = buffer_max.max(mag);
                }
            }
            let normalization = if buffer_max > 0.0 { 1.0 / buffer_max } else { 1.0 };

            for x in 0..spec_width {
                for y in 0..spec_height {
                    let magnitude = (spectrogram_buffer[x][y] * normalization).min(1.0);

                    // Calculate color based on color mode
                    let color = match current_config.spectrogram_color_mode.as_str() {
                        "intensity" => {
                            // Map magnitude to gradient position
                            if let Some(ref grad) = gradient {
                                grad.at(magnitude as f64).to_rgba8()
                            } else {
                                [0, 0, 0, 255]
                            }
                        }
                        "frequency" => {
                            // Map frequency (y position) to gradient
                            if let Some(ref grad) = gradient {
                                let freq_pos = y as f64 / spec_height as f64;
                                let rgba = grad.at(freq_pos).to_rgba8();
                                // Modulate brightness by magnitude
                                let mag_f64 = magnitude as f64;
                                [(rgba[0] as f64 * mag_f64) as u8,
                                 (rgba[1] as f64 * mag_f64) as u8,
                                 (rgba[2] as f64 * mag_f64) as u8,
                                 255]
                            } else {
                                [0, 0, 0, 255]
                            }
                        }
                        "volume" => {
                            // Use overall volume level to shift hue
                            let vol_level = freq_magnitudes.iter().sum::<f32>() / freq_magnitudes.len() as f32;
                            if let Some(ref grad) = gradient {
                                let hue_shift = (vol_level * 0.5) as f64;
                                let rgba = grad.at((hue_shift + magnitude as f64 * 0.5).min(1.0)).to_rgba8();
                                let mag_f64 = magnitude as f64;
                                [(rgba[0] as f64 * mag_f64) as u8,
                                 (rgba[1] as f64 * mag_f64) as u8,
                                 (rgba[2] as f64 * mag_f64) as u8,
                                 255]
                            } else {
                                [0, 0, 0, 255]
                            }
                        }
                        _ => {
                            // Default to intensity mode
                            if let Some(ref grad) = gradient {
                                grad.at(magnitude as f64).to_rgba8()
                            } else {
                                [0, 0, 0, 255]
                            }
                        }
                    };

                    // Map 2D spectrogram position to 1D LED strip (flip Y so low freq is at bottom)
                    let led_idx = (spec_height - 1 - y) * spec_width + x;

                    if led_idx < current_config.total_leds {
                        let offset = led_idx * 3;
                        frame[offset] = color[0];
                        frame[offset + 1] = color[1];
                        frame[offset + 2] = color[2];
                    }
                }
            }
        } else if current_config.vu {
            // === VU METER MODE ===
            // Classic stereo VU meter: left channel = first half, right channel = second half

            // Calculate peak levels for each channel (more responsive than RMS for VU meters)
            let left_peak;
            let right_peak;

            if channels >= 2 {
                // Stereo or multi-channel - extract only left (ch 0) and right (ch 1) channels
                let mut left_max = 0.0_f32;
                let mut right_max = 0.0_f32;
                let sample_count = samples.len() / channels;

                for i in 0..sample_count {
                    let left = samples[i * channels].abs();      // Channel 0 (left)
                    let right = samples[i * channels + 1].abs(); // Channel 1 (right)
                    left_max = left_max.max(left);
                    right_max = right_max.max(right);
                }

                left_peak = left_max;
                right_peak = right_max;
            } else {
                // Mono - use same signal for both channels
                let peak = samples.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
                left_peak = peak;
                right_peak = peak;
            }

            // Apply attack/decay smoothing
            if smoothed_magnitudes.len() != 2 {
                smoothed_magnitudes = vec![0.0; 2];
            }

            for (i, peak) in [left_peak, right_peak].iter().enumerate() {
                let target = *peak;
                let current = smoothed_magnitudes[i];
                smoothed_magnitudes[i] = if target > current {
                    current + (target - current) * attack_factor as f32
                } else {
                    current + (target - current) * decay_factor as f32
                };
            }

            let mut smoothed_left = smoothed_magnitudes[0];
            let mut smoothed_right = smoothed_magnitudes[1];

            // Apply VU meter scaling - boost levels for better visibility
            // Higher sensitivity for more responsive meters
            let vu_gain = 4.0;
            let raw_left = smoothed_left * vu_gain;
            let raw_right = smoothed_right * vu_gain;

            // Detect clipping (signal over 1.0 = overdriven)
            let left_clipping = raw_left > 1.0;
            let right_clipping = raw_right > 1.0;

            smoothed_left = raw_left.min(1.0);
            smoothed_right = raw_right.min(1.0);

            // Update display levels for TUI
            display_left_level = smoothed_left;
            display_right_level = smoothed_right;

            // Split LEDs in half
            let half = current_config.total_leds / 2;

            // Build gradients for left and right channels using cached TUI color strings
            // (TUI color strings are already resolved via unified system at init and when config changes)
            let interpolation_mode = match current_config.interpolation.as_str() {
                "basis" => InterpolationMode::Basis,
                "catmullrom" => InterpolationMode::CatmullRom,
                _ => InterpolationMode::Linear,
            };

            let (left_gradient, left_colors, left_solid) = build_gradient_from_color(
                &tui_left_color_str,
                current_config.use_gradient,
                interpolation_mode,
            )?;

            let (right_gradient, right_colors, right_solid) = build_gradient_from_color(
                &tui_right_color_str,
                current_config.use_gradient,
                interpolation_mode,
            )?;

            // Update animation offsets (scaled by level if configured)
            // Channel mapping: TX=Right, RX=Left
            // Offset is kept in 0-1 range like the bandwidth meter
            if current_config.animation_speed > 0.0 {
                let half_leds = current_config.total_leds / 2;

                // Left channel = RX, uses rx_animation_direction
                let left_speed = if current_config.scale_animation_speed {
                    // Scale animation speed based on audio level (0 when silent, max when loud)
                    // Use display level for scaling (0.0 to 1.0 range)
                    current_config.animation_speed * (display_left_level as f64)
                } else {
                    current_config.animation_speed
                };

                // Convert speed to 0-1 range (LEDs per frame / LEDs per channel)
                let left_offset_delta = left_speed / half_leds as f64;
                left_animation_offset = (left_animation_offset + left_offset_delta) % 1.0;

                // Right channel = TX, uses tx_animation_direction
                let right_speed = if current_config.scale_animation_speed {
                    // Scale animation speed based on audio level (0 when silent, max when loud)
                    // Use display level for scaling (0.0 to 1.0 range)
                    current_config.animation_speed * (display_right_level as f64)
                } else {
                    current_config.animation_speed
                };

                // Convert speed to 0-1 range (LEDs per frame / LEDs per channel)
                let right_offset_delta = right_speed / half_leds as f64;
                right_animation_offset = (right_animation_offset + right_offset_delta) % 1.0;
            }

            // Store animation offsets for TUI rendering
            tui_left_animation_offset = left_animation_offset;
            tui_right_animation_offset = right_animation_offset;

            // Check for strobe condition (clipping)
            let strobe_active = current_config.strobe_on_max && (left_clipping || right_clipping);
            let show_strobe = if strobe_active {
                let cycle_ms = 1000.0 / current_config.strobe_rate_hz;
                let phase = (frame_count as f64 * frame_time_ms) % cycle_ms;
                phase < current_config.strobe_duration_ms
            } else {
                false
            };

            // Update peak hold tracking for VU mode
            let peak_hold_color = Rgb::from_hex(&current_config.peak_hold_color).unwrap_or(Rgb { r: 255, g: 255, b: 255 });

            // Left channel peak tracking
            let half_leds = half;
            if current_config.peak_hold {
                let left_lit_count = (smoothed_left * half_leds as f32).round() as usize;
                let left_current_peak = if left_lit_count > 0 {
                    // Convert from lit count to LED index based on direction
                    match current_config.direction.as_str() {
                        "mirrored" => half_leds - left_lit_count,  // fills from right edge going left
                        "opposing" => left_lit_count - 1,  // fills rightward
                        "right" => half_leds - left_lit_count,  // fills from right edge
                        _ => left_lit_count - 1,  // fills from left (default)
                    }
                } else {
                    0
                };

                // Update peak if current level is higher or peak has expired
                let should_update_left_peak = if let (Some(peak_led), Some(peak_time)) = (left_peak_led, left_peak_time) {
                    // Check if expired
                    let expired = peak_time.elapsed().as_secs_f64() * 1000.0 > current_config.peak_hold_duration_ms;
                    // Update if current is higher than stored peak or expired
                    expired || left_lit_count > 0 && match current_config.direction.as_str() {
                        "mirrored" | "right" => left_current_peak < peak_led,  // lower index = higher level
                        _ => left_current_peak > peak_led,  // higher index = higher level
                    }
                } else {
                    left_lit_count > 0
                };

                if should_update_left_peak && left_lit_count > 0 {
                    // Check if this is a NEW peak at a different position
                    let is_new_peak_position = left_peak_led.map_or(true, |old_led| old_led != left_current_peak);

                    // Toggle animation direction if enabled and this is a new peak position
                    if current_config.peak_direction_toggle && is_new_peak_position {
                        left_animation_direction = if left_animation_direction == "left" {
                            "right".to_string()
                        } else {
                            "left".to_string()
                        };
                    }

                    left_peak_led = Some(left_current_peak);
                    left_peak_time = Some(Instant::now());
                } else if let Some(peak_time) = left_peak_time {
                    // Clear peak if expired
                    if peak_time.elapsed().as_secs_f64() * 1000.0 > current_config.peak_hold_duration_ms {
                        left_peak_led = None;
                        left_peak_time = None;
                    }
                }
            } else {
                // Peak hold disabled - clear tracking
                left_peak_led = None;
                left_peak_time = None;
            }

            // Right channel peak tracking
            if current_config.peak_hold {
                let right_lit_count = (smoothed_right * half_leds as f32).round() as usize;
                let right_current_peak = if right_lit_count > 0 {
                    match current_config.direction.as_str() {
                        "mirrored" => right_lit_count - 1,  // fills from left edge going right
                        "opposing" => half_leds - right_lit_count,  // fills leftward
                        "left" => right_lit_count - 1,  // fills from left edge
                        _ => half_leds - right_lit_count,  // fills from right (default)
                    }
                } else {
                    0
                };

                let should_update_right_peak = if let (Some(peak_led), Some(peak_time)) = (right_peak_led, right_peak_time) {
                    let expired = peak_time.elapsed().as_secs_f64() * 1000.0 > current_config.peak_hold_duration_ms;
                    expired || right_lit_count > 0 && match current_config.direction.as_str() {
                        "mirrored" | "left" => right_current_peak > peak_led,
                        _ => right_current_peak < peak_led,
                    }
                } else {
                    right_lit_count > 0
                };

                if should_update_right_peak && right_lit_count > 0 {
                    // Check if this is a NEW peak at a different position
                    let is_new_peak_position = right_peak_led.map_or(true, |old_led| old_led != right_current_peak);

                    // Toggle animation direction if enabled and this is a new peak position
                    if current_config.peak_direction_toggle && is_new_peak_position {
                        right_animation_direction = if right_animation_direction == "left" {
                            "right".to_string()
                        } else {
                            "left".to_string()
                        };
                    }

                    right_peak_led = Some(right_current_peak);
                    right_peak_time = Some(Instant::now());
                } else if let Some(peak_time) = right_peak_time {
                    if peak_time.elapsed().as_secs_f64() * 1000.0 > current_config.peak_hold_duration_ms {
                        right_peak_led = None;
                        right_peak_time = None;
                    }
                }
            } else {
                right_peak_led = None;
                right_peak_time = None;
            }

            // Render left channel (first half) - Left = RX, uses rx_animation_direction (or toggled direction)
            renderer::render_vu_channel(
                &mut frame,
                0,
                half,
                smoothed_left,
                &current_config.direction,  // Use direction for VU meter
                &left_animation_direction,  // Left = RX (may be toggled)
                left_animation_offset,
                left_gradient.as_ref(),
                &left_colors,
                left_solid,
                true,  // is_left_channel
                current_config.intensity_colors,  // intensity colors mode
                current_config.peak_hold,
                left_peak_led,
                peak_hold_color,
            );

            // Render right channel (second half) - Right = TX, uses tx_animation_direction (or toggled direction)
            renderer::render_vu_channel(
                &mut frame,
                half,
                current_config.total_leds,
                smoothed_right,
                &current_config.direction,  // Use direction for VU meter
                &right_animation_direction,  // Right = TX (may be toggled)
                right_animation_offset,
                right_gradient.as_ref(),
                &right_colors,
                right_solid,
                false,  // is_left_channel
                current_config.intensity_colors,  // intensity colors mode
                current_config.peak_hold,
                right_peak_led,
                peak_hold_color,
            );

            // Apply strobe effect if clipping
            if show_strobe {
                let strobe_rgb = Rgb::from_hex(&current_config.strobe_color).unwrap_or(Rgb { r: 255, g: 0, b: 0 });

                if left_clipping {
                    // Strobe left channel
                    for i in 0..half {
                        frame[i * 3] = strobe_rgb.r;
                        frame[i * 3 + 1] = strobe_rgb.g;
                        frame[i * 3 + 2] = strobe_rgb.b;
                    }
                }

                if right_clipping {
                    // Strobe right channel
                    for i in half..current_config.total_leds {
                        frame[i * 3] = strobe_rgb.r;
                        frame[i * 3 + 1] = strobe_rgb.g;
                        frame[i * 3 + 2] = strobe_rgb.b;
                    }
                }
            }

        } else if current_config.matrix_2d_enabled {
            // === 2D MATRIX SPECTRUM MODE ===
            // Display spectrum on a 2D matrix with frequency on X-axis and amplitude on Y-axis
            let width = current_config.matrix_2d_width;
            let height = current_config.matrix_2d_height;

            // Ensure frame buffer matches matrix size
            if frame.len() != width * height * 3 {
                frame = vec![0u8; width * height * 3];
            }

            // Ensure smoothed_magnitudes matches number of columns (frequency bins)
            if smoothed_magnitudes.len() != width {
                smoothed_magnitudes = vec![0.0; width];
            }

            let num_bins = fft_size / 2;
            let display_bins = max_bin - min_bin + 1;

            // Process FFT - combine all channels into mono for 2D display
            let channels_to_process = channels.min(2);
            let mut bin_magnitudes = vec![0.0_f32; num_bins];
            let mut max_magnitude = 0.0_f32;

            for ch in 0..channels_to_process {
                let channel_samples: Vec<f32> = samples.iter().skip(ch).step_by(channels).copied().take(fft_size).collect();
                let mut fft_buffer: Vec<Complex<f32>> = channel_samples
                    .iter()
                    .enumerate()
                    .map(|(i, &s)| {
                        let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos());
                        Complex { re: s * window, im: 0.0 }
                    })
                    .collect();

                fft.process(&mut fft_buffer);

                for (i, complex) in fft_buffer.iter().take(num_bins).enumerate() {
                    let mag = (complex.re * complex.re + complex.im * complex.im).sqrt();
                    bin_magnitudes[i] += mag;
                    max_magnitude = max_magnitude.max(mag);
                }
            }

            // Average magnitudes if combining multiple channels
            if channels_to_process > 1 {
                for mag in &mut bin_magnitudes {
                    *mag /= channels_to_process as f32;
                }
                max_magnitude /= channels_to_process as f32;
            }

            let normalization = if max_magnitude > 0.0 { 1.0 / max_magnitude } else { 1.0 };

            // Map frequency bins to matrix columns with smoothing
            for i in 0..width {
                // Apply direction mode to map physical columns to frequency positions
                let (physical_col, freq_col) = match current_config.direction.as_str() {
                    "right" => {
                        // Right: high freq on left, low freq on right
                        let physical_col = i;
                        let freq_col = width - 1 - i;
                        (physical_col, freq_col)
                    },
                    "mirrored" => {
                        // Mirrored: low freq at center, high freq at edges
                        let half = width / 2;
                        if i < half {
                            // Left half: high freq at edge (col 0), low freq at center
                            let physical_col = i;
                            let freq_col = half - 1 - i;
                            (physical_col, freq_col)
                        } else {
                            // Right half: low freq at center, high freq at edge
                            let physical_col = i;
                            let freq_col = i - half;
                            (physical_col, freq_col)
                        }
                    },
                    "opposing" => {
                        // Opposing: high freq at center, low freq at edges
                        // Both halves show the same frequency range, mirrored
                        let half = width / 2;
                        let physical_col = i;
                        let freq_col = if i < half {
                            // Left half: low freq at edge (col 0), high freq at center (col half-1)
                            i
                        } else {
                            // Right half: high freq at center (col half), low freq at edge (col width-1)
                            // Mirror the left half: col half -> half-1, col half+1 -> half-2, ..., col width-1 -> 0
                            width - 1 - i
                        };
                        (physical_col, freq_col)
                    },
                    _ => {
                        // "left" or default: low freq on left, high freq on right
                        (i, i)
                    }
                };

                // Map frequency column to frequency bin
                let bin_offset = (freq_col * display_bins) / width;
                let bin_index = (min_bin + bin_offset).min(max_bin);
                let magnitude = (bin_magnitudes[bin_index] * normalization).min(1.0);

                // Apply threshold and smoothing (use freq_col for smoothing array index)
                let target = if magnitude > threshold { magnitude } else { 0.0 };
                let current = smoothed_magnitudes[freq_col];
                let smoothed = if target > current {
                    current + (target - current) * attack_factor as f32
                } else {
                    current + (target - current) * decay_factor as f32
                };
                smoothed_magnitudes[freq_col] = smoothed;

                // Calculate how many LEDs to light up in this column (from bottom to top)
                let lit_height = (smoothed * height as f32) as usize;

                // Gradient position based on configuration
                let gradient_pos = if current_config.matrix_2d_gradient_direction == "vertical" {
                    // Vertical: gradient based on amplitude (0.0 = silent, 1.0 = max)
                    smoothed as f64
                } else {
                    // Horizontal (default): gradient based on frequency (0.0 = low freq, 1.0 = high freq)
                    freq_col as f64 / (width - 1).max(1) as f64
                };

                // Get color using gradient system
                let (r, g, b) = if let Some(ref grad) = spectrum_gradient {
                    let color = grad.at(gradient_pos);
                    let rgba = color.to_rgba8();
                    (rgba[0], rgba[1], rgba[2])
                } else if spectrum_colors.len() > 1 {
                    let n = spectrum_colors.len();
                    let segment_size = 1.0 / n as f64;
                    let color_index = ((gradient_pos / segment_size).floor() as usize).min(n - 1);
                    let rgb = &spectrum_colors[color_index];
                    (rgb.r, rgb.g, rgb.b)
                } else if !spectrum_colors.is_empty() {
                    let rgb = &spectrum_colors[0];
                    (rgb.r, rgb.g, rgb.b)
                } else {
                    (spectrum_solid.r, spectrum_solid.g, spectrum_solid.b)
                };

                // Fill column from bottom to top (serpentine pattern)
                for row in 0..height {
                    // Serpentine/zigzag pattern: even rows go left-to-right, odd rows go right-to-left
                    let led_index = if row % 2 == 0 {
                        row * width + physical_col
                    } else {
                        row * width + (width - 1 - physical_col)
                    };

                    // Light LED if it's below the amplitude threshold (bottom-up visualization)
                    // Physical row 0 is at TOP of matrix, so invert: we light rows from (height - lit_height) to (height - 1)
                    if row >= (height - lit_height) {
                        frame[led_index * 3] = r;
                        frame[led_index * 3 + 1] = g;
                        frame[led_index * 3 + 2] = b;
                    } else {
                        // Turn off LEDs above the amplitude
                        frame[led_index * 3] = 0;
                        frame[led_index * 3 + 1] = 0;
                        frame[led_index * 3 + 2] = 0;
                    }
                }
            }
        } else {
            // === FFT SPECTRUM MODE ===
            // Ensure smoothed_magnitudes is the right size for FFT mode
            // (it gets resized to 2 in VU mode, so resize back if needed)
            if smoothed_magnitudes.len() != current_config.total_leds {
                smoothed_magnitudes = vec![0.0; current_config.total_leds];
            }

            let num_bins = fft_size / 2;
            let display_bins = max_bin - min_bin + 1;
            // Process stereo channels separately for all direction modes when stereo audio is available
            let is_stereo_mode = channels >= 2;

            if is_stereo_mode {
                // === STEREO SPECTRUM MODE (mirrored/opposing) ===
                // Process left and right channels separately, each using half the LEDs
                // For multi-channel devices, extract only left (ch 0) and right (ch 1) channels
                let half = current_config.total_leds / 2;

                // Process left channel (first half of LEDs) - extract channel 0
                let left_samples: Vec<f32> = samples.iter().step_by(channels).copied().take(fft_size).collect();
                let mut left_fft: Vec<Complex<f32>> = left_samples
                    .iter()
                    .enumerate()
                    .map(|(i, &s)| {
                        let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos());
                        Complex { re: s * window, im: 0.0 }
                    })
                    .collect();
                fft.process(&mut left_fft);

                let mut left_bins = vec![0.0_f32; num_bins];
                let mut left_max = 0.0_f32;
                for (i, complex) in left_fft.iter().take(num_bins).enumerate() {
                    let mag = (complex.re * complex.re + complex.im * complex.im).sqrt();
                    left_bins[i] = mag;
                    left_max = left_max.max(mag);
                }
                let left_norm = if left_max > 0.0 { 1.0 / left_max } else { 1.0 };

                // Process right channel (second half of LEDs) - extract channel 1
                let right_samples: Vec<f32> = samples.iter().skip(1).step_by(channels).copied().take(fft_size).collect();
                let mut right_fft: Vec<Complex<f32>> = right_samples
                    .iter()
                    .enumerate()
                    .map(|(i, &s)| {
                        let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos());
                        Complex { re: s * window, im: 0.0 }
                    })
                    .collect();
                fft.process(&mut right_fft);

                let mut right_bins = vec![0.0_f32; num_bins];
                let mut right_max = 0.0_f32;
                for (i, complex) in right_fft.iter().take(num_bins).enumerate() {
                    let mag = (complex.re * complex.re + complex.im * complex.im).sqrt();
                    right_bins[i] = mag;
                    right_max = right_max.max(mag);
                }
                let right_norm = if right_max > 0.0 { 1.0 / right_max } else { 1.0 };

                // Map left channel to LEDs
                for i in 0..half {
                    let (led, freq_pos) = match current_config.direction.as_str() {
                        "mirrored" => {
                            // Mirrored: low freq at center (LED 599), high freq at edge (LED 0)
                            let led = half - 1 - i;
                            let freq_pos = i;
                            (led, freq_pos)
                        },
                        "opposing" => {
                            // Opposing: low freq at edge (LED 0), high freq at center (LED 599)
                            let led = i;
                            let freq_pos = i;
                            (led, freq_pos)
                        },
                        "right" => {
                            // Right: high freq on left, low freq on right
                            let led = i;
                            let freq_pos = half - 1 - i;
                            (led, freq_pos)
                        },
                        _ => {
                            // Left (default): low freq on left, high freq on right
                            let led = i;
                            let freq_pos = i;
                            (led, freq_pos)
                        }
                    };

                    let bin_offset = (freq_pos * display_bins) / half;
                    let bin_index = (min_bin + bin_offset).min(max_bin);
                    let magnitude = (left_bins[bin_index] * left_norm).min(1.0);

                    // Apply threshold to target BEFORE smoothing (attack/decay)
                    let target = if magnitude > threshold { magnitude } else { 0.0 };
                    let current = smoothed_magnitudes[led];
                    let smoothed = if target > current {
                        // Attack: fade in to target over attack_ms
                        current + (target - current) * attack_factor as f32
                    } else {
                        // Decay: fade out to target over decay_ms
                        current + (target - current) * decay_factor as f32
                    };
                    smoothed_magnitudes[led] = smoothed;

                    // Use smoothed value directly as brightness
                    let brightness = smoothed;

                    // Gradient position based on frequency (low=0.0, high=1.0)
                    // The LED mapping itself handles visual reversal, so we always map freq directly to gradient
                    let gradient_pos = freq_pos as f64 / (half - 1) as f64;

                    // Get color using gradient system (same as bandwidth meter)
                    let (r, g, b) = if let Some(ref grad) = spectrum_gradient {
                        // Use gradient
                        let color = grad.at(gradient_pos);
                        let rgba = color.to_rgba8();
                        (rgba[0], rgba[1], rgba[2])
                    } else if spectrum_colors.len() > 1 {
                        // Multiple solid colors - pick one based on position
                        let n = spectrum_colors.len();
                        let segment_size = 1.0 / n as f64;
                        let color_index = ((gradient_pos / segment_size).floor() as usize).min(n - 1);
                        let rgb = &spectrum_colors[color_index];
                        (rgb.r, rgb.g, rgb.b)
                    } else if !spectrum_colors.is_empty() {
                        // Single color from array
                        let rgb = &spectrum_colors[0];
                        (rgb.r, rgb.g, rgb.b)
                    } else {
                        // Fallback to solid color
                        (spectrum_solid.r, spectrum_solid.g, spectrum_solid.b)
                    };

                    frame[led * 3] = (r as f32 * brightness) as u8;
                    frame[led * 3 + 1] = (g as f32 * brightness) as u8;
                    frame[led * 3 + 2] = (b as f32 * brightness) as u8;
                }

                // Map right channel to LEDs
                for i in 0..half {
                    let (led, freq_pos) = match current_config.direction.as_str() {
                        "mirrored" => {
                            // Mirrored: low freq at center (LED 600), high freq at edge (LED 1199)
                            let led = half + i;
                            let freq_pos = i;
                            (led, freq_pos)
                        },
                        "opposing" => {
                            // Opposing: low freq at edge (LED 1199), high freq at center (LED 600)
                            let led = current_config.total_leds - 1 - i;
                            let freq_pos = i;
                            (led, freq_pos)
                        },
                        "right" => {
                            // Right: high freq on left, low freq on right
                            let led = half + i;
                            let freq_pos = half - 1 - i;
                            (led, freq_pos)
                        },
                        _ => {
                            // Left (default): low freq on left, high freq on right
                            let led = half + i;
                            let freq_pos = i;
                            (led, freq_pos)
                        }
                    };

                    let bin_offset = (freq_pos * display_bins) / half;
                    let bin_index = (min_bin + bin_offset).min(max_bin);
                    let magnitude = (right_bins[bin_index] * right_norm).min(1.0);

                    // Apply threshold to target BEFORE smoothing (attack/decay)
                    let target = if magnitude > threshold { magnitude } else { 0.0 };
                    let current = smoothed_magnitudes[led];
                    let smoothed = if target > current {
                        // Attack: fade in to target over attack_ms
                        current + (target - current) * attack_factor as f32
                    } else {
                        // Decay: fade out to target over decay_ms
                        current + (target - current) * decay_factor as f32
                    };
                    smoothed_magnitudes[led] = smoothed;

                    // Use smoothed value directly as brightness
                    let brightness = smoothed;

                    // Gradient position based on frequency (low=0.0, high=1.0)
                    // The LED mapping itself handles visual reversal, so we always map freq directly to gradient
                    let gradient_pos = freq_pos as f64 / (half - 1) as f64;

                    // Get color using gradient system (same as bandwidth meter)
                    let (r, g, b) = if let Some(ref grad) = spectrum_gradient {
                        // Use gradient
                        let color = grad.at(gradient_pos);
                        let rgba = color.to_rgba8();
                        (rgba[0], rgba[1], rgba[2])
                    } else if spectrum_colors.len() > 1 {
                        // Multiple solid colors - pick one based on position
                        let n = spectrum_colors.len();
                        let segment_size = 1.0 / n as f64;
                        let color_index = ((gradient_pos / segment_size).floor() as usize).min(n - 1);
                        let rgb = &spectrum_colors[color_index];
                        (rgb.r, rgb.g, rgb.b)
                    } else if !spectrum_colors.is_empty() {
                        // Single color from array
                        let rgb = &spectrum_colors[0];
                        (rgb.r, rgb.g, rgb.b)
                    } else {
                        // Fallback to solid color
                        (spectrum_solid.r, spectrum_solid.g, spectrum_solid.b)
                    };

                    frame[led * 3] = (r as f32 * brightness) as u8;
                    frame[led * 3 + 1] = (g as f32 * brightness) as u8;
                    frame[led * 3 + 2] = (b as f32 * brightness) as u8;
                }

            } else {
                // === MONO SPECTRUM MODE (left/right) ===
                // Use full LED range for frequency spectrum, average both channels
                let mut bin_magnitudes = vec![0.0_f32; num_bins];
                let mut max_magnitude = 0.0_f32;

                // For multi-channel devices, only process first 2 channels (left and right)
                let channels_to_process = if channels >= 2 { 2 } else { channels };

                for ch in 0..channels_to_process {
                    let channel_samples: Vec<f32> = samples
                        .iter()
                        .skip(ch)
                        .step_by(channels)
                        .copied()
                        .take(fft_size)
                        .collect();

                    let mut fft_buffer: Vec<Complex<f32>> = channel_samples
                        .iter()
                        .enumerate()
                        .map(|(i, &s)| {
                            let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos());
                            Complex { re: s * window, im: 0.0 }
                        })
                        .collect();

                    fft.process(&mut fft_buffer);

                    for (i, complex) in fft_buffer.iter().take(num_bins).enumerate() {
                        let mag = (complex.re * complex.re + complex.im * complex.im).sqrt();
                        bin_magnitudes[i] += mag;
                        max_magnitude = max_magnitude.max(mag);
                    }
                }

                if channels_to_process > 1 {
                    for mag in &mut bin_magnitudes {
                        *mag /= channels_to_process as f32;
                    }
                    max_magnitude /= channels_to_process as f32;
                }

                let normalization = if max_magnitude > 0.0 { 1.0 / max_magnitude } else { 1.0 };

                for i in 0..current_config.total_leds {
                    // Map LED to frequency bin based on direction
                    let (led, freq_pos) = if current_config.direction == "right" {
                        // Right: low freq at far end, high freq at LED 0
                        let led = current_config.total_leds - 1 - i;
                        let freq_pos = i;
                        (led, freq_pos)
                    } else {
                        // Left (default): low freq at LED 0, high freq at far end
                        (i, i)
                    };

                    let bin_offset = (freq_pos * display_bins) / current_config.total_leds;
                    let bin_index = (min_bin + bin_offset).min(max_bin);
                    let magnitude = (bin_magnitudes[bin_index] * normalization).min(1.0);

                    // Apply threshold to target BEFORE smoothing (attack/decay)
                    let target = if magnitude > threshold { magnitude } else { 0.0 };
                    let current = smoothed_magnitudes[led];
                    let smoothed = if target > current {
                        // Attack: fade in to target over attack_ms
                        current + (target - current) * attack_factor as f32
                    } else {
                        // Decay: fade out to target over decay_ms
                        current + (target - current) * decay_factor as f32
                    };
                    smoothed_magnitudes[led] = smoothed;

                    // Use smoothed value directly as brightness
                    let brightness = smoothed;

                    // Gradient position based on frequency (low=0.0, high=1.0)
                    // The LED mapping itself handles visual reversal, so we always map freq directly to gradient
                    let gradient_pos = freq_pos as f64 / (current_config.total_leds - 1) as f64;

                    // Get color using gradient system (same as bandwidth meter)
                    let (r, g, b) = if let Some(ref grad) = spectrum_gradient {
                        // Use gradient
                        let color = grad.at(gradient_pos);
                        let rgba = color.to_rgba8();
                        (rgba[0], rgba[1], rgba[2])
                    } else if spectrum_colors.len() > 1 {
                        // Multiple solid colors - pick one based on position
                        let n = spectrum_colors.len();
                        let segment_size = 1.0 / n as f64;
                        let color_index = ((gradient_pos / segment_size).floor() as usize).min(n - 1);
                        let rgb = &spectrum_colors[color_index];
                        (rgb.r, rgb.g, rgb.b)
                    } else if !spectrum_colors.is_empty() {
                        // Single color from array
                        let rgb = &spectrum_colors[0];
                        (rgb.r, rgb.g, rgb.b)
                    } else {
                        // Fallback to solid color
                        (spectrum_solid.r, spectrum_solid.g, spectrum_solid.b)
                    };

                    frame[led * 3] = (r as f32 * brightness) as u8;
                    frame[led * 3 + 1] = (g as f32 * brightness) as u8;
                    frame[led * 3 + 2] = (b as f32 * brightness) as u8;
                }
            }
        } // End FFT spectrum mode

        // Add frame to buffer with timestamp
        let delay_duration = Duration::from_micros((current_config.ddp_delay_ms * 1000.0) as u64);
        let send_time = loop_start + delay_duration;
        frame_buffer.push_back((send_time, frame));

        // Send all frames that are ready (timestamp <= now)
        let now = Instant::now();
        while let Some((send_time, _)) = frame_buffer.front() {
            if *send_time <= now {
                if let Some((_, frame_to_send)) = frame_buffer.pop_front() {
                    let _ = multi_device_manager.send_frame_with_brightness(&frame_to_send, Some(current_config.global_brightness));
                }
            } else {
                break;
            }
        }

        // Update TUI
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),     // Header
                    Constraint::Min(10),       // Main content
                    Constraint::Length(3),     // Footer
                ])
                .split(f.size());

            // Header - Mode and sub-mode
            let sub_mode = if current_config.spectrogram {
                "Spectrogram"
            } else if current_config.vu {
                "VU Meter"
            } else {
                "FFT Spectrum"
            };
            let stereo_mode = if channels >= 2 { "Stereo" } else { "Mono" };
            let header_text = format!("🎚️ Live Audio Mode | Sub-mode: {} ({}) ", sub_mode, stereo_mode);
            let header = Paragraph::new(header_text)
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, chunks[0]);

            // Main content - either config info or VU meters
            if show_config_info {
                let config_lines = generate_config_info_display(&current_config);
                let config_widget = Paragraph::new(config_lines)
                    .block(Block::default().borders(Borders::ALL).title("Configuration (Press 'i' to hide)"));
                f.render_widget(config_widget, chunks[1]);
            } else {

            // Single continuous VU meter bar representing the entire LED strip with gradient colors
            // Left half = left channel (LEDs 0-599), Right half = right channel (LEDs 600-1199)
            let meter_width = chunks[1].width.saturating_sub(4) as usize;
            let half_width = meter_width / 2;

            let left_filled = (display_left_level * half_width as f32) as usize;
            let right_filled = (display_right_level * half_width as f32) as usize;

            // Build gradient bar with colored spans
            let mut bar_spans = vec![Span::raw("[")];

            // Rebuild gradients for TUI from stored color strings
            let (tui_left_gradient, tui_left_colors, tui_left_solid) = if !tui_left_color_str.is_empty() {
                build_gradient_from_color(
                    &tui_left_color_str,
                    tui_use_gradient,
                    tui_interpolation_mode,
                ).unwrap_or_else(|e| {
                    eprintln!("Error building left gradient: {}", e);
                    (None, Vec::new(), Rgb { r: 255, g: 255, b: 255 })
                })
            } else {
                (None, Vec::new(), Rgb { r: 255, g: 255, b: 255 })
            };

            let (tui_right_gradient, tui_right_colors, tui_right_solid) = if !tui_right_color_str.is_empty() {
                build_gradient_from_color(
                    &tui_right_color_str,
                    tui_use_gradient,
                    tui_interpolation_mode,
                ).unwrap_or_else(|e| {
                    eprintln!("Error building right gradient: {}", e);
                    (None, Vec::new(), Rgb { r: 255, g: 255, b: 255 })
                })
            } else {
                (None, Vec::new(), Rgb { r: 255, g: 255, b: 255 })
            };

            // Helper function to get gradient color with animation
            let get_gradient_color = |pos: f64, gradient: &Option<colorgrad::Gradient>, colors: &Vec<Rgb>, solid: &Rgb, animation_offset: f64, animation_dir: &str| -> (u8, u8, u8) {
                if let Some(grad) = gradient {
                    // Apply animation offset (match LED strip logic)
                    let animated_pos = if animation_dir == "right" {
                        (1.0 + pos - animation_offset) % 1.0
                    } else {
                        (pos + animation_offset) % 1.0
                    };
                    let color = grad.at(animated_pos);
                    let rgba = color.to_rgba8();
                    (rgba[0], rgba[1], rgba[2])
                } else if colors.len() > 1 {
                    // Multiple solid colors - pick one based on position
                    let n = colors.len();
                    let segment_size = 1.0 / n as f64;
                    let color_index = ((pos / segment_size).floor() as usize).min(n - 1);
                    let rgb = &colors[color_index];
                    (rgb.r, rgb.g, rgb.b)
                } else if !colors.is_empty() {
                    // Single color
                    let rgb = &colors[0];
                    (rgb.r, rgb.g, rgb.b)
                } else {
                    // Fallback to solid color
                    (solid.r, solid.g, solid.b)
                }
            };

            // Channel mapping: TX=Right, RX=Left
            let left_anim_dir = &current_config.rx_animation_direction;  // Left = RX
            let right_anim_dir = &current_config.tx_animation_direction;  // Right = TX

            match current_config.direction.as_str() {
                "mirrored" => {
                    // Mirrored: left fills from center going left, right fills from center going right
                    let left_empty = half_width.saturating_sub(left_filled);
                    let right_empty = half_width.saturating_sub(right_filled);

                    // Left channel empty space
                    for _ in 0..left_empty {
                        bar_spans.push(Span::raw(" "));
                    }
                    // Left channel filled (from right to left, so reverse positions)
                    for i in 0..left_filled {
                        let pos = (left_filled - 1 - i) as f64 / half_width as f64;
                        let (r, g, b) = get_gradient_color(pos, &tui_left_gradient, &tui_left_colors, &tui_left_solid, tui_left_animation_offset, left_anim_dir);
                        bar_spans.push(Span::styled("█", Style::default().fg(Color::Rgb(r, g, b))));
                    }

                    bar_spans.push(Span::raw("|"));

                    // Right channel filled (from left to right)
                    for i in 0..right_filled {
                        let pos = i as f64 / half_width as f64;
                        let (r, g, b) = get_gradient_color(pos, &tui_right_gradient, &tui_right_colors, &tui_right_solid, tui_right_animation_offset, right_anim_dir);
                        bar_spans.push(Span::styled("█", Style::default().fg(Color::Rgb(r, g, b))));
                    }
                    // Right channel empty space
                    for _ in 0..right_empty {
                        bar_spans.push(Span::raw(" "));
                    }
                }
                "opposing" => {
                    // Opposing: left fills left to right, right fills right to left
                    let left_empty = half_width.saturating_sub(left_filled);
                    let right_empty = half_width.saturating_sub(right_filled);

                    // Left channel filled (left to right)
                    for i in 0..left_filled {
                        let pos = i as f64 / half_width as f64;
                        let (r, g, b) = get_gradient_color(pos, &tui_left_gradient, &tui_left_colors, &tui_left_solid, tui_left_animation_offset, left_anim_dir);
                        bar_spans.push(Span::styled("█", Style::default().fg(Color::Rgb(r, g, b))));
                    }
                    // Left channel empty space
                    for _ in 0..left_empty {
                        bar_spans.push(Span::raw(" "));
                    }

                    bar_spans.push(Span::raw("|"));

                    // Right channel empty space
                    for _ in 0..right_empty {
                        bar_spans.push(Span::raw(" "));
                    }
                    // Right channel filled (right to left, so reverse positions)
                    for i in 0..right_filled {
                        let pos = (right_filled - 1 - i) as f64 / half_width as f64;
                        let (r, g, b) = get_gradient_color(pos, &tui_right_gradient, &tui_right_colors, &tui_right_solid, tui_right_animation_offset, right_anim_dir);
                        bar_spans.push(Span::styled("█", Style::default().fg(Color::Rgb(r, g, b))));
                    }
                }
                "left" => {
                    // Both channels fill left to right
                    let left_empty = half_width.saturating_sub(left_filled);
                    let right_empty = half_width.saturating_sub(right_filled);

                    // Left channel filled (left to right)
                    for i in 0..left_filled {
                        let pos = i as f64 / half_width as f64;
                        let (r, g, b) = get_gradient_color(pos, &tui_left_gradient, &tui_left_colors, &tui_left_solid, tui_left_animation_offset, left_anim_dir);
                        bar_spans.push(Span::styled("█", Style::default().fg(Color::Rgb(r, g, b))));
                    }
                    // Left channel empty space
                    for _ in 0..left_empty {
                        bar_spans.push(Span::raw(" "));
                    }

                    bar_spans.push(Span::raw("|"));

                    // Right channel filled (left to right)
                    for i in 0..right_filled {
                        let pos = i as f64 / half_width as f64;
                        let (r, g, b) = get_gradient_color(pos, &tui_right_gradient, &tui_right_colors, &tui_right_solid, tui_right_animation_offset, right_anim_dir);
                        bar_spans.push(Span::styled("█", Style::default().fg(Color::Rgb(r, g, b))));
                    }
                    // Right channel empty space
                    for _ in 0..right_empty {
                        bar_spans.push(Span::raw(" "));
                    }
                }
                "right" => {
                    // Both channels fill right to left
                    let left_empty = half_width.saturating_sub(left_filled);
                    let right_empty = half_width.saturating_sub(right_filled);

                    // Left channel empty space
                    for _ in 0..left_empty {
                        bar_spans.push(Span::raw(" "));
                    }
                    // Left channel filled (right to left, so reverse positions)
                    for i in 0..left_filled {
                        let pos = (left_filled - 1 - i) as f64 / half_width as f64;
                        let (r, g, b) = get_gradient_color(pos, &tui_left_gradient, &tui_left_colors, &tui_left_solid, tui_left_animation_offset, left_anim_dir);
                        bar_spans.push(Span::styled("█", Style::default().fg(Color::Rgb(r, g, b))));
                    }

                    bar_spans.push(Span::raw("|"));

                    // Right channel empty space
                    for _ in 0..right_empty {
                        bar_spans.push(Span::raw(" "));
                    }
                    // Right channel filled (right to left, so reverse positions)
                    for i in 0..right_filled {
                        let pos = (right_filled - 1 - i) as f64 / half_width as f64;
                        let (r, g, b) = get_gradient_color(pos, &tui_right_gradient, &tui_right_colors, &tui_right_solid, tui_right_animation_offset, right_anim_dir);
                        bar_spans.push(Span::styled("█", Style::default().fg(Color::Rgb(r, g, b))));
                    }
                }
                _ => {
                    // Default: left to right
                    let left_empty = half_width.saturating_sub(left_filled);
                    let right_empty = half_width.saturating_sub(right_filled);

                    // Left channel filled (left to right)
                    for i in 0..left_filled {
                        let pos = i as f64 / half_width as f64;
                        let (r, g, b) = get_gradient_color(pos, &tui_left_gradient, &tui_left_colors, &tui_left_solid, tui_left_animation_offset, left_anim_dir);
                        bar_spans.push(Span::styled("█", Style::default().fg(Color::Rgb(r, g, b))));
                    }
                    // Left channel empty space
                    for _ in 0..left_empty {
                        bar_spans.push(Span::raw(" "));
                    }

                    bar_spans.push(Span::raw("|"));

                    // Right channel filled (left to right)
                    for i in 0..right_filled {
                        let pos = i as f64 / half_width as f64;
                        let (r, g, b) = get_gradient_color(pos, &tui_right_gradient, &tui_right_colors, &tui_right_solid, tui_right_animation_offset, right_anim_dir);
                        bar_spans.push(Span::styled("█", Style::default().fg(Color::Rgb(r, g, b))));
                    }
                    // Right channel empty space
                    for _ in 0..right_empty {
                        bar_spans.push(Span::raw(" "));
                    }
                }
            };

            // Close bracket and add level indicators
            bar_spans.push(Span::raw(format!("]  L: {:.1}%{}  R: {:.1}%{}",
                display_left_level * 100.0,
                if display_left_level >= 0.99 { " 🔴" } else { "" },
                display_right_level * 100.0,
                if display_right_level >= 0.99 { " 🔴" } else { "" }
            )));

            let vu_paragraph = Paragraph::new(Line::from(bar_spans))
                .block(Block::default().borders(Borders::ALL).title("VU Meter - LED Strip Visualization (LED 0 ← Left | Right → LED 1200)"));
            f.render_widget(vu_paragraph, chunks[1]);
            }

            // Footer - Monitoring source and controls
            let footer_text = format!(
                "Source: Audio [{}] | {} Hz | {} ch | WLED: {} | LEDs: {} | FPS: {:.0} | Delay: {:.1}ms | Press 'i' for config, 'q' or Ctrl+C to quit",
                selected_device_name, sample_rate, channels, current_config.wled_ip, current_config.total_leds, current_fps, current_config.ddp_delay_ms
            );
            let footer = Paragraph::new(footer_text)
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(footer, chunks[2]);
        })?;

        // Frame rate limiting
        let elapsed = loop_start.elapsed();
        if elapsed < frame_duration {
            thread::sleep(frame_duration - elapsed);
        }
    }

    // Cleanup
    terminal.show_cursor()?;
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;

    println!("\n👋 Live audio mode stopped.\n");

    Ok(ModeExitReason::UserQuit)
}

/// Falling Sand simulation mode
fn run_sand_mode(config: &BandwidthConfig, config_change_tx: broadcast::Sender<()>) -> Result<ModeExitReason> {
    use std::time::{Duration, Instant};

    // Parse particle type from config
    let particle_type = match config.sand_particle_type.to_lowercase().as_str() {
        "water" => sand::Particle::Water,
        "stone" => sand::Particle::Stone,
        "fire" => sand::Particle::Fire,
        "smoke" => sand::Particle::Smoke,
        "wood" => sand::Particle::Wood,
        "lava" => sand::Particle::Lava,
        _ => sand::Particle::Sand,
    };

    // Initialize sand simulation
    let mut sim = sand::SandSimulation::new(
        config.sand_grid_width,
        config.sand_grid_height,
        particle_type,
        config.sand_spawn_rate as f32,
        config.sand_spawn_radius,
        config.sand_spawn_x,
        config.sand_fire_enabled,
        &config.sand_color_sand,
        &config.sand_color_water,
        &config.sand_color_stone,
        &config.sand_color_fire,
        &config.sand_color_smoke,
        &config.sand_color_wood,
        &config.sand_color_lava,
    );

    // Place obstacles if enabled
    sim.place_obstacles(config.sand_obstacles_enabled, config.sand_obstacle_density as f32);

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

    let mut md_manager = match MultiDeviceManager::new(md_config) {
        Ok(mgr) => mgr,
        Err(e) => {
            eprintln!("Failed to initialize multi-device manager: {}", e);
            return Err(e);
        }
    };

    // Frame timing
    let frame_duration = Duration::from_secs_f64(1.0 / config.fps);
    let mut last_frame = Instant::now();

    let mut config_change_rx = config_change_tx.subscribe();
    let mut current_config = config.clone();

    // Setup terminal for TUI
    use crossterm::terminal::{enable_raw_mode, disable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
    use crossterm::execute;
    use std::io;
    use ratatui::{
        backend::CrosstermBackend,
        widgets::{Block, Borders, Paragraph},
        layout::{Layout, Constraint, Direction},
        Terminal,
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    terminal.hide_cursor()?;

    loop {
        let loop_start = Instant::now();

        // Check for config changes
        if let Ok(()) = config_change_rx.try_recv() {
            if let Ok(new_config) = BandwidthConfig::load() {
                // Check if mode changed
                if new_config.mode != "sand" {
                    // Cleanup terminal
                    terminal.show_cursor().ok();
                    disable_raw_mode().ok();
                    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
                    return Ok(ModeExitReason::ModeChanged);
                }

                // Reinitialize if grid size changed
                if new_config.sand_grid_width != current_config.sand_grid_width ||
                   new_config.sand_grid_height != current_config.sand_grid_height {
                    let new_particle = match new_config.sand_particle_type.to_lowercase().as_str() {
                        "water" => sand::Particle::Water,
                        "stone" => sand::Particle::Stone,
                        "fire" => sand::Particle::Fire,
                        "smoke" => sand::Particle::Smoke,
                        "wood" => sand::Particle::Wood,
                        "lava" => sand::Particle::Lava,
                        _ => sand::Particle::Sand,
                    };

                    sim = sand::SandSimulation::new(
                        new_config.sand_grid_width,
                        new_config.sand_grid_height,
                        new_particle,
                        new_config.sand_spawn_rate as f32,
                        new_config.sand_spawn_radius,
                        new_config.sand_spawn_x,
                        new_config.sand_fire_enabled,
                        &new_config.sand_color_sand,
                        &new_config.sand_color_water,
                        &new_config.sand_color_stone,
                        &new_config.sand_color_fire,
                        &new_config.sand_color_smoke,
                        &new_config.sand_color_wood,
                        &new_config.sand_color_lava,
                    );

                    // Place obstacles if enabled
                    sim.place_obstacles(new_config.sand_obstacles_enabled, new_config.sand_obstacle_density as f32);
                } else {
                    // Update config without rebuilding
                    let new_particle = match new_config.sand_particle_type.to_lowercase().as_str() {
                        "water" => sand::Particle::Water,
                        "stone" => sand::Particle::Stone,
                        "fire" => sand::Particle::Fire,
                        "smoke" => sand::Particle::Smoke,
                        "wood" => sand::Particle::Wood,
                        "lava" => sand::Particle::Lava,
                        _ => sand::Particle::Sand,
                    };

                    sim.update_config(
                        new_particle,
                        new_config.sand_spawn_rate as f32,
                        new_config.sand_spawn_radius,
                        new_config.sand_spawn_x,
                        new_config.sand_fire_enabled,
                        &new_config.sand_color_sand,
                        &new_config.sand_color_water,
                        &new_config.sand_color_stone,
                        &new_config.sand_color_fire,
                        &new_config.sand_color_smoke,
                        &new_config.sand_color_wood,
                        &new_config.sand_color_lava,
                    );
                }

                current_config = new_config;
            }
        }

        // Check for restart flag from web UI
        if std::path::Path::new("/tmp/rustwled_sand_restart").exists() {
            // Clear the simulation
            sim.clear();
            // Place obstacles if enabled
            sim.place_obstacles(current_config.sand_obstacles_enabled, current_config.sand_obstacle_density as f32);
            // Remove the flag file
            let _ = std::fs::remove_file("/tmp/rustwled_sand_restart");
        }

        // Check for keyboard input (q to quit)
        if poll(Duration::from_millis(0))? {
            if let Event::Key(key) = read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        // Cleanup terminal
                        terminal.show_cursor().ok();
                        disable_raw_mode().ok();
                        execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
                        return Ok(ModeExitReason::UserQuit);
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Cleanup terminal
                        terminal.show_cursor().ok();
                        disable_raw_mode().ok();
                        execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
                        return Ok(ModeExitReason::UserQuit);
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        // Clear the simulation
                        sim.clear();
                        // Place obstacles if enabled
                        sim.place_obstacles(current_config.sand_obstacles_enabled, current_config.sand_obstacle_density as f32);
                    }
                    _ => {}
                }
            }
        }

        // Render frame if it's time
        let elapsed = loop_start.duration_since(last_frame);
        if elapsed >= frame_duration {
            last_frame = loop_start;

            // Spawn particles (if enabled)
            if current_config.sand_spawn_enabled {
                sim.spawn_particles();
            }

            // Update physics
            sim.update();

            // Render to LED frame
            let frame = sim.render(current_config.total_leds);

            // Send to WLED devices with brightness applied
            let _ = md_manager.send_frame_with_brightness(&frame, Some(current_config.global_brightness));
        }

        // Update TUI
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),  // Header
                    Constraint::Min(10),    // Main content (simulation visualization placeholder)
                    Constraint::Length(3),  // Footer
                ])
                .split(f.size());

            // Header - Mode and particle type (with quit instructions trailing)
            let particle_name = match current_config.sand_particle_type.as_str() {
                "water" => "Water",
                "stone" => "Stone",
                "fire" => "Fire",
                "smoke" => "Smoke",
                "wood" => "Wood",
                "lava" => "Lava",
                _ => "Sand",
            };

            use ratatui::text::{Line, Span};
            use ratatui::style::{Style, Color};

            let header_left = format!("⏳ Falling Sand Mode | Particle: {} | {}x{} Grid",
                particle_name, current_config.sand_grid_width, current_config.sand_grid_height);
            let header_right = "Press 'r' to restart, 'q' or Ctrl+C to quit";

            // Calculate padding to right-align the quit instructions
            let available_width = chunks[0].width.saturating_sub(4); // Account for borders
            let left_len = header_left.len();
            let right_len = header_right.len();
            let padding = available_width.saturating_sub(left_len as u16).saturating_sub(right_len as u16);

            let header_line = Line::from(vec![
                Span::raw(header_left),
                Span::raw(" ".repeat(padding as usize)),
                Span::styled(header_right, Style::default().fg(Color::Gray)),
            ]);

            let header = Paragraph::new(header_line)
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, chunks[0]);

            // Main content - Simulation info
            let spawn_status = if current_config.sand_spawn_enabled { "✓ Enabled" } else { "✗ Disabled" };
            let fire_status = if current_config.sand_fire_enabled { "✓ Enabled" } else { "✗ Disabled" };
            let obstacles_status = if current_config.sand_obstacles_enabled {
                format!("✓ Enabled ({}% density)", (current_config.sand_obstacle_density * 100.0) as u8)
            } else {
                "✗ Disabled".to_string()
            };

            let main_text = format!(
                "Simulation Running\n\n\
                Spawn: {} (Rate: {:.0}%, Radius: {}, Position: {})\n\
                Fire Spread: {}\n\
                Obstacles: {}\n\n\
                LEDs are displaying the particle simulation in real-time.\n\
                Use the web interface to adjust colors and settings.",
                spawn_status,
                current_config.sand_spawn_rate * 100.0,
                current_config.sand_spawn_radius,
                current_config.sand_spawn_x,
                fire_status,
                obstacles_status
            );
            let main_widget = Paragraph::new(main_text)
                .block(Block::default().borders(Borders::ALL).title("Status"));
            f.render_widget(main_widget, chunks[1]);

            // Footer - Stats and controls
            let total_devices = md_manager.device_count();
            let device_info = if total_devices > 1 {
                format!("{} devices", total_devices)
            } else {
                "single device".to_string()
            };

            let footer_text = format!(
                "WLED: {} | LEDs: {} | FPS: {:.0} | Brightness: {}% | Devices: {}",
                current_config.wled_ip,
                current_config.total_leds,
                current_config.fps,
                (current_config.global_brightness * 100.0) as u8,
                device_info
            );
            let footer = Paragraph::new(footer_text)
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(footer, chunks[2]);
        }).ok();

        // Sleep to maintain target FPS
        let elapsed = loop_start.elapsed();
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
    }

    // Cleanup (this is unreachable but required for consistency)
    #[allow(unreachable_code)]
    {
        terminal.show_cursor().ok();
        disable_raw_mode().ok();
        execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
        Ok(ModeExitReason::UserQuit)
    }
}

/// Geometry mode - mathematical and harmonic line-art animations
fn run_geometry_mode(config: &BandwidthConfig, config_change_tx: broadcast::Sender<()>) -> Result<ModeExitReason> {
    use std::time::{Duration, Instant};
    use std::io;

    // Setup terminal for TUI
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    terminal.hide_cursor()?;

    // Setup multi-device manager for WLED
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

    let mut multi_device_manager = MultiDeviceManager::new(md_config)?;

    // Create geometry state
    let mut geometry_state = geometry::GeometryState::new(
        config.total_leds,
        config.geometry_grid_width,
        config.geometry_grid_height,
        &config.geometry_mode_select,
        config.geometry_mode_duration_seconds,
        config.geometry_randomize_order,
        config.boid_count,
        config.boid_separation_distance,
        config.boid_alignment_distance,
        config.boid_cohesion_distance,
        config.boid_max_speed,
        config.boid_max_force,
        config.boid_predator_enabled,
        config.boid_predator_count,
        config.boid_predator_speed,
        config.boid_avoidance_distance,
        config.boid_chase_force
    );

    // Build geometry gradient colors from config
    let geometry_color_str = if !config.color.is_empty() {
        gradients::resolve_color_string(&config.color)
    } else {
        "FF0000,FF7F00,FFFF00,00FF00,0000FF,4B0082,9400D3".to_string() // Default rainbow
    };

    let interpolation_mode = match config.interpolation.to_lowercase().as_str() {
        "basis" => InterpolationMode::Basis,
        "catmullrom" => InterpolationMode::CatmullRom,
        _ => InterpolationMode::Linear,
    };

    if let Ok((_grad, colors, _solid)) = build_gradient_from_color(&geometry_color_str, config.use_gradient, interpolation_mode) {
        let float_colors: Vec<(f32, f32, f32)> = colors.iter().map(|c| (c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0)).collect();
        geometry_state.update_colors(float_colors);
    }

    // Subscribe to config changes
    let mut config_change_rx = config_change_tx.subscribe();
    let mut current_config = config.clone();

    // Frame timing
    let mut frame_duration = Duration::from_secs_f64(1.0 / config.fps);
    let mut last_frame = Instant::now();
    let mut frame_count = 0u64;
    let mut fps_timer = Instant::now();

    // Frame buffer for scheduled sends (non-blocking delay implementation)
    let mut frame_buffer: std::collections::VecDeque<(Instant, Vec<u8>)> = std::collections::VecDeque::new();

    loop {
        let loop_start = Instant::now();

        // Check for keyboard input
        if crossterm::event::poll(Duration::from_millis(0))? {
            if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
                use crossterm::event::{KeyCode, KeyModifiers};
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        terminal.show_cursor()?;
                        disable_raw_mode()?;
                        terminal.backend_mut().execute(LeaveAlternateScreen)?;
                        return Ok(ModeExitReason::UserQuit);
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        terminal.show_cursor()?;
                        disable_raw_mode()?;
                        terminal.backend_mut().execute(LeaveAlternateScreen)?;
                        return Ok(ModeExitReason::UserQuit);
                    }
                    _ => {}
                }
            }
        }

        // Check for config changes
        if let Ok(()) = config_change_rx.try_recv() {
            let new_config = match BandwidthConfig::load() {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Check if mode changed
            if new_config.mode != "geometry" {
                terminal.show_cursor()?;
                disable_raw_mode()?;
                terminal.backend_mut().execute(LeaveAlternateScreen)?;
                return Ok(ModeExitReason::ModeChanged);
            }

            // Reinitialize multi-device manager if device config changed
            let devices_changed = new_config.wled_devices.len() != current_config.wled_devices.len() ||
                new_config.wled_devices.iter().zip(current_config.wled_devices.iter()).any(|(new, old)| {
                    new.ip != old.ip ||
                    new.led_offset != old.led_offset ||
                    new.led_count != old.led_count ||
                    new.enabled != old.enabled
                });

            if devices_changed {
                let devices: Vec<WLEDDevice> = new_config.wled_devices.iter().map(|d| WLEDDevice {
                    ip: d.ip.clone(),
                    led_offset: d.led_offset,
                    led_count: d.led_count,
                            enabled: d.enabled,
                }).collect();

                let md_config = MultiDeviceConfig {
                    devices,
                    send_parallel: new_config.multi_device_send_parallel,
                    fail_fast: new_config.multi_device_fail_fast,
                };

                match MultiDeviceManager::new(md_config) {
                    Ok(new_manager) => {
                        multi_device_manager = new_manager;
                    }
                    Err(_e) => {
                        // Continue with existing manager
                    }
                }
            }

            // Reinitialize geometry state if any geometry settings changed
            if new_config.geometry_grid_width != current_config.geometry_grid_width ||
               new_config.geometry_grid_height != current_config.geometry_grid_height ||
               new_config.total_leds != current_config.total_leds ||
               new_config.geometry_mode_select != current_config.geometry_mode_select ||
               new_config.geometry_mode_duration_seconds != current_config.geometry_mode_duration_seconds ||
               new_config.geometry_randomize_order != current_config.geometry_randomize_order {
                geometry_state = geometry::GeometryState::new(
                    new_config.total_leds,
                    new_config.geometry_grid_width,
                    new_config.geometry_grid_height,
                    &new_config.geometry_mode_select,
                    new_config.geometry_mode_duration_seconds,
                    new_config.geometry_randomize_order,
                    new_config.boid_count,
                    new_config.boid_separation_distance,
                    new_config.boid_alignment_distance,
                    new_config.boid_cohesion_distance,
                    new_config.boid_max_speed,
                    new_config.boid_max_force,
                    new_config.boid_predator_enabled,
                    new_config.boid_predator_count,
                    new_config.boid_predator_speed,
                    new_config.boid_avoidance_distance,
                    new_config.boid_chase_force
                );

                // Reapply gradient colors after recreating geometry state
                let geometry_color_str = if !new_config.color.is_empty() {
                    gradients::resolve_color_string(&new_config.color)
                } else {
                    "FF0000,FF7F00,FFFF00,00FF00,0000FF,4B0082,9400D3".to_string()
                };
                let interpolation_mode = match new_config.interpolation.to_lowercase().as_str() {
                    "basis" => InterpolationMode::Basis,
                    "catmullrom" => InterpolationMode::CatmullRom,
                    _ => InterpolationMode::Linear,
                };
                if let Ok((_grad, colors, _solid)) = build_gradient_from_color(&geometry_color_str, new_config.use_gradient, interpolation_mode) {
                    let float_colors: Vec<(f32, f32, f32)> = colors.iter().map(|c| (c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0)).collect();
                    geometry_state.update_colors(float_colors);
                }
            }

            // Update frame duration if FPS changed
            if new_config.fps != current_config.fps {
                frame_duration = Duration::from_secs_f64(1.0 / new_config.fps);
            }

            // Update boid config if any boid parameters changed
            if new_config.boid_count != current_config.boid_count ||
               new_config.boid_separation_distance != current_config.boid_separation_distance ||
               new_config.boid_alignment_distance != current_config.boid_alignment_distance ||
               new_config.boid_cohesion_distance != current_config.boid_cohesion_distance ||
               new_config.boid_max_speed != current_config.boid_max_speed ||
               new_config.boid_max_force != current_config.boid_max_force ||
               new_config.boid_predator_enabled != current_config.boid_predator_enabled ||
               new_config.boid_predator_count != current_config.boid_predator_count ||
               new_config.boid_predator_speed != current_config.boid_predator_speed ||
               new_config.boid_avoidance_distance != current_config.boid_avoidance_distance ||
               new_config.boid_chase_force != current_config.boid_chase_force {
                geometry_state.update_boid_config(
                    new_config.boid_count,
                    new_config.boid_separation_distance,
                    new_config.boid_alignment_distance,
                    new_config.boid_cohesion_distance,
                    new_config.boid_max_speed,
                    new_config.boid_max_force,
                    new_config.boid_predator_enabled,
                    new_config.boid_predator_count,
                    new_config.boid_predator_speed,
                    new_config.boid_avoidance_distance,
                    new_config.boid_chase_force
                );
            }

            // Update geometry colors if color or gradient settings changed
            if new_config.color != current_config.color ||
               new_config.use_gradient != current_config.use_gradient ||
               new_config.interpolation != current_config.interpolation {
                let new_geometry_color_str = if !new_config.color.is_empty() {
                    gradients::resolve_color_string(&new_config.color)
                } else {
                    "FF0000,FF7F00,FFFF00,00FF00,0000FF,4B0082,9400D3".to_string()
                };

                let new_interpolation_mode = match new_config.interpolation.to_lowercase().as_str() {
                    "basis" => InterpolationMode::Basis,
                    "catmullrom" => InterpolationMode::CatmullRom,
                    _ => InterpolationMode::Linear,
                };

                if let Ok((_grad, colors, _solid)) = build_gradient_from_color(&new_geometry_color_str, new_config.use_gradient, new_interpolation_mode) {
                    let float_colors: Vec<(f32, f32, f32)> = colors.iter().map(|c| (c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0)).collect();
                    geometry_state.update_colors(float_colors);
                }
            }

            current_config = new_config;
        }

        // Render frame if it's time
        let elapsed = loop_start.duration_since(last_frame);
        if elapsed >= frame_duration {
            last_frame = loop_start;

            // Update geometry and get frame
            let render_start = Instant::now();
            let frame = geometry_state.update(
                current_config.global_brightness,
                current_config.animation_speed,
                &current_config.tx_animation_direction
            );
            let render_time = render_start.elapsed();

            // Add frame to buffer with scheduled send time (non-blocking delay)
            let delay_duration = Duration::from_micros((current_config.ddp_delay_ms * 1000.0) as u64);
            let send_time = loop_start + delay_duration;
            frame_buffer.push_back((send_time, frame));

            frame_count += 1;

            // Render TUI
            let actual_fps = if fps_timer.elapsed().as_secs_f64() > 0.0 {
                frame_count as f64 / fps_timer.elapsed().as_secs_f64()
            } else {
                0.0
            };

            // Reset FPS counter every 2 seconds
            if fps_timer.elapsed() >= Duration::from_secs(2) {
                frame_count = 0;
                fps_timer = Instant::now();
            }

            terminal.draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),  // Header
                        Constraint::Min(5),     // Main content
                        Constraint::Length(3),  // Footer
                    ])
                    .split(f.size());

                // Header - Mode and current geometry
                let mode_select = &current_config.geometry_mode_select;
                let current_mode_name = format!("{:?}", geometry_state.current_mode);
                let header_spans = vec![
                    Span::styled(
                        "🔷 Geometry Mode",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    ),
                    Span::raw(" | "),
                    Span::styled(
                        format!("Current: {}", current_mode_name),
                        Style::default().fg(Color::Yellow)
                    ),
                    Span::raw(" | "),
                    Span::styled(
                        if mode_select == "cycle" { "Cycling" } else { "Fixed" },
                        Style::default().fg(Color::Green)
                    ),
                    Span::raw("                                        "), // Spacer
                    Span::styled(
                        "Press 'q' or Ctrl+C to quit",
                        Style::default().fg(Color::DarkGray)
                    ),
                ];
                let header = Paragraph::new(Line::from(header_spans))
                    .block(Block::default().borders(Borders::ALL));
                f.render_widget(header, chunks[0]);

                // Main content - show geometry info
                let elapsed_in_mode = geometry_state.mode_start_time.elapsed().as_secs_f64();
                let time_remaining = (geometry_state.mode_duration.as_secs_f64() - elapsed_in_mode).max(0.0);
                let grid_info = format!("Grid: {}x{}", current_config.geometry_grid_width, current_config.geometry_grid_height);
                let timing_info = if mode_select == "cycle" {
                    format!("Time in mode: {:.1}s / {:.1}s remaining until transition",
                        elapsed_in_mode, time_remaining)
                } else {
                    format!("Running in fixed mode: {}", mode_select)
                };

                let content_lines = vec![
                    Line::from(""),
                    Line::from(format!("  Mode Selection: {}", if mode_select == "cycle" { "Cycle (all 20 modes)" } else { mode_select })),
                    Line::from(format!("  {}", timing_info)),
                    Line::from(format!("  {}", grid_info)),
                    Line::from(format!("  Animation: {} (speed: {:.1}, dir: {})",
                        if current_config.animation_speed > 0.0 { "Enabled" } else { "Disabled" },
                        current_config.animation_speed,
                        current_config.tx_animation_direction
                    )),
                ];

                let content = Paragraph::new(content_lines)
                    .block(Block::default().borders(Borders::ALL).title("Geometry Animation"));
                f.render_widget(content, chunks[1]);

                // Footer - Status
                let footer_text = format!(
                    "LEDs: {} | FPS: {:.1} / {:.1} | Render: {:.2}ms | Buffer: {} | Devices: {}",
                    current_config.total_leds,
                    actual_fps,
                    current_config.fps,
                    render_time.as_secs_f64() * 1000.0,
                    frame_buffer.len(),
                    current_config.wled_devices.len()
                );
                let footer = Paragraph::new(footer_text)
                    .block(Block::default().borders(Borders::ALL));
                f.render_widget(footer, chunks[2]);
            })?;
        }

        // Send all frames that are ready (send_time <= now) - non-blocking
        let now = Instant::now();
        while let Some((send_time, _)) = frame_buffer.front() {
            if *send_time <= now {
                if let Some((_, frame_to_send)) = frame_buffer.pop_front() {
                    let _ = multi_device_manager.send_frame(&frame_to_send);
                }
            } else {
                break;
            }
        }

        // Small sleep to avoid spinning
        std::thread::sleep(Duration::from_micros(100));
    }
}

/// Audio test mode - simple diagnostic tool to test audio capture using cpal+dasp
fn run_audio_test_mode() -> Result<()> {
    use cpal::traits::{DeviceTrait, StreamTrait};
    use cpal::SampleFormat;

    println!("\n=== Audio Test Mode (using dasp) ===");
    println!("This mode will test basic audio capture and display live audio levels.\n");

    let host = cpal::default_host();
    println!("Audio host: {:?}\n", host.id());

    // List available audio devices using the working audio module
    let device_list = audio::list_audio_devices()?;

    println!("=== Enumerating ALL Audio Devices ===\n");
    for (i, (name, _is_output)) in device_list.iter().enumerate() {
        println!("  {}. {}", i + 1, name);
    }

    // Prompt for device selection
    let selected_device_name = loop {
        print!("\nSelect audio device to test (1-{}): ", device_list.len());
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if let Ok(choice) = input.trim().parse::<usize>() {
            if choice > 0 && choice <= device_list.len() {
                break &device_list[choice - 1].0;
            }
        }
        println!("Invalid selection. Please enter a number between 1 and {}", device_list.len());
    };

    println!("\nSelected: {}", selected_device_name);

    // Find the actual device
    let device = audio::find_audio_device(selected_device_name)?;
    let device_name = device.name()?;

    // Check if device supports input
    let config = match device.default_input_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("\n✗ ERROR: This device does not support input capture!");
            eprintln!("   Error: {}", e);
            std::process::exit(1);
        }
    };

    println!("\n✓ Device supports input capture!");
    println!("  Configuration: {} Hz, {} channels, {:?}",
            config.sample_rate().0, config.channels(), config.sample_format());

    // Create a shared buffer for audio samples
    let audio_samples = Arc::new(Mutex::new(Vec::<f32>::new()));
    let audio_samples_clone = audio_samples.clone();

    // Add diagnostics for callback data
    let callback_data_info = Arc::new(Mutex::new(String::new()));
    let callback_data_info_clone = callback_data_info.clone();

    // Build the input stream
    println!("\nStarting audio stream...");

    // Add a callback counter to verify the audio callback is actually being called
    let callback_count = Arc::new(Mutex::new(0u64));
    let callback_count_clone = callback_count.clone();

    let stream = match config.sample_format() {
        SampleFormat::F32 => {
            let cb_count = callback_count_clone.clone();
            let data_info = callback_data_info_clone.clone();
            device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let count = *cb_count.lock().unwrap();
                    *cb_count.lock().unwrap() += 1;

                    // On first few callbacks, inspect the raw data
                    if count < 5 {
                        let peak = data.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
                        let non_zero = data.iter().filter(|&&s| s.abs() > 0.0001).count();
                        let info = format!(
                            "Callback {}: len={}, peak={:.6}, non_zero={}, first_10={:?}",
                            count, data.len(), peak, non_zero,
                            &data.iter().take(10).copied().collect::<Vec<f32>>()
                        );
                        *data_info.lock().unwrap() = info;
                    }

                    let mut samples = audio_samples_clone.lock().unwrap();
                    samples.extend_from_slice(data);
                    // Keep only last 2 seconds
                    let len = samples.len();
                    if len > 88200 {
                        let drain_count = len - 88200;
                        samples.drain(0..drain_count);
                    }
                },
                |err| eprintln!("Stream error: {}", err),
                None,
            )?
        }
        SampleFormat::I16 => {
            let cb_count = callback_count_clone.clone();
            device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    *cb_count.lock().unwrap() += 1;
                    let mut samples = audio_samples_clone.lock().unwrap();
                    samples.extend(data.iter().map(|&s| s as f32 / 32768.0));
                    let len = samples.len();
                    if len > 88200 {
                        let drain_count = len - 88200;
                        samples.drain(0..drain_count);
                    }
                },
                |err| eprintln!("Stream error: {}", err),
                None,
            )?
        }
        SampleFormat::U16 => {
            let cb_count = callback_count_clone.clone();
            device.build_input_stream(
                &config.into(),
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    *cb_count.lock().unwrap() += 1;
                    let mut samples = audio_samples_clone.lock().unwrap();
                    samples.extend(data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0));
                    let len = samples.len();
                    if len > 88200 {
                        let drain_count = len - 88200;
                        samples.drain(0..drain_count);
                    }
                },
                |err| eprintln!("Stream error: {}", err),
                None,
            )?
        }
        _ => {
            eprintln!("Unsupported sample format: {:?}", config.sample_format());
            std::process::exit(1);
        }
    };

    stream.play()?;
    println!("✓ Stream playing!\n");

    // Wait for data and check if we're actually receiving anything
    println!("Checking for audio data...");
    thread::sleep(Duration::from_millis(1000));

    // Show the raw callback diagnostics immediately
    {
        let data_info = callback_data_info.lock().unwrap().clone();
        if !data_info.is_empty() {
            println!("\n✓ Raw callback data received:");
            println!("  {}\n", data_info);
        }
    }

    // Quick check to see if we have permission
    let initial_samples = {
        let samples = audio_samples.lock().unwrap();
        samples.clone()
    };

    if !initial_samples.is_empty() {
        let has_non_zero = initial_samples.iter().any(|&s| s.abs() > 0.0001);
        if !has_non_zero {
            println!("\n⚠️  WARNING: Receiving samples but ALL ARE ZERO");
            println!("   This usually means MICROPHONE PERMISSION IS DENIED on macOS.\n");
            println!("=== HOW TO FIX (macOS) ===");
            println!("1. Open: System Settings > Privacy & Security > Microphone");
            println!("2. Look for 'Terminal' (or your terminal app) in the list");
            println!("3. If it's there, make sure it's ENABLED (checkbox checked)");
            println!("4. If it's NOT there:");
            println!("   - Click the (+) button");
            println!("   - Navigate to /Applications/Utilities/Terminal.app");
            println!("   - Add it and enable it");
            println!("5. RESTART this program after granting permission\n");
            println!("Alternative: Try running from a different terminal that has permission");
            println!("(e.g., iTerm2, VS Code terminal, etc.)\n");
            println!("Continuing anyway in case audio starts playing...\n");
        }
    }

    println!("=== Live Audio Monitor (dasp) ===");
    println!("Press Ctrl+C to exit\n");

    let mut last_diagnostic = std::time::Instant::now();

    loop {
        thread::sleep(Duration::from_millis(100));

        let samples = {
            let samples = audio_samples.lock().unwrap();
            samples.clone()
        };

        if samples.is_empty() {
            print!("\r⚠️  No audio data yet...                                              ");
            std::io::stdout().flush()?;
            continue;
        }

        // Use dasp for signal processing
        let peak = samples.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
        let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
        let rms = (sum_squares / samples.len() as f32).sqrt();

        let peak_db = if peak > 0.0 { 20.0 * peak.log10() } else { -100.0 };
        let rms_db = if rms > 0.0 { 20.0 * rms.log10() } else { -100.0 };

        let bar_length = (peak * 50.0).min(50.0) as usize;
        let bar = "█".repeat(bar_length);

        // Count non-zero samples for diagnostics
        let non_zero_count = samples.iter().filter(|&&s| s.abs() > 0.0001).count();
        let non_zero_percent = (non_zero_count as f32 / samples.len() as f32) * 100.0;

        let cb_count = *callback_count.lock().unwrap();

        print!("\rPeak: {:.4} ({:6.1} dB) | RMS: {:.4} ({:6.1} dB) | Buf: {} | Callbacks: {} | Non-zero: {:.1}% | {}",
               peak, peak_db, rms, rms_db, samples.len(), cb_count, non_zero_percent, bar);
        std::io::stdout().flush()?;

        // Every 5 seconds, show detailed diagnostics
        if last_diagnostic.elapsed() > Duration::from_secs(5) {
            println!("\n");
            println!("  Audio callbacks received: {}", cb_count);

            // Show raw callback data inspection
            let data_info = callback_data_info.lock().unwrap().clone();
            if !data_info.is_empty() {
                println!("  Raw callback data: {}", data_info);
            }

            println!("  Buffer size: {} samples", samples.len());
            println!("  Non-zero samples: {} ({:.2}%)", non_zero_count, non_zero_percent);
            println!("  Peak value: {:.6}", peak);
            println!("  RMS value: {:.6}", rms);

            if cb_count == 0 {
                println!("\n  ✗ CRITICAL: Audio callback is NOT being called!");
                println!("     The audio stream says it's playing but no data is coming through.");
                println!("     This usually means:");
                println!("     1. You're connected via SSH (SSH can't access audio hardware)");
                println!("     2. The session doesn't have GUI access");
                println!("     3. CoreAudio isn't running properly");
                println!("\n     Solution: Run this directly on the machine (not over SSH)");
            } else if non_zero_percent < 0.1 {
                println!("\n  ⚠️  Callbacks working BUT all samples are ZERO!");
                println!("     The audio device is connected but not receiving audio.");
                println!("     Possible causes:");
                println!("     1. No audio is currently playing on your system");
                println!("     2. The '{}' device is not the active audio route", device_name);
                println!("     3. Loopback needs to be configured in macOS Audio MIDI Setup");
                println!("     4. The device might need to be selected as an output in Sound Settings");
                println!("\n     Try:");
                println!("     - Open Audio MIDI Setup (in /Applications/Utilities/)");
                println!("     - Check if {} is part of an aggregate or multi-output device", device_name);
                println!("     - Play some audio and check if it's routing through {}", device_name);
            } else if peak > 0.01 {
                println!("\n  ✓ Audio is being captured successfully!");
            }
            println!();
            last_diagnostic = std::time::Instant::now();
        }
    }
}

/// Spawn HTTP server in a separate thread that can be restarted
fn spawn_http_server(config: &BandwidthConfig, config_change_tx: broadcast::Sender<()>, webcam_state: Arc<webcam::WebcamState>) -> Result<Option<thread::JoinHandle<()>>> {
    if !config.httpd_enabled {
        return Ok(None);
    }

    let ip = config.httpd_ip.clone();
    let port = config.httpd_port;
    let https_enabled = config.httpd_https_enabled;

    let handle = thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Err(e) = httpd::run_http_server(ip.clone(), port, https_enabled, config_change_tx, webcam_state).await {
                eprintln!("HTTP server error: {}", e);
            }
        });
    });

    if config.httpd_https_enabled {
        println!("HTTPS server started at https://{}:{}", config.httpd_ip, config.httpd_port);
    } else {
        println!("HTTP server started at http://{}:{}", config.httpd_ip, config.httpd_port);
    }
    Ok(Some(handle))
}

/// Watch config file and send control messages when critical settings change
fn spawn_config_watcher(config_change_tx: broadcast::Sender<()>) -> Result<()> {
    let config_path = BandwidthConfig::config_path(None)?;

    std::thread::spawn(move || -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
            Ok(w) => w,
            Err(_) => return Ok(()),
        };

        if watcher
            .watch(&config_path, RecursiveMode::NonRecursive)
            .is_err()
        {
            return Ok(());
        }

        loop {
            match rx.recv() {
                Ok(Ok(NotifyEvent { kind, .. })) => {
                    if matches!(kind, notify::EventKind::Modify(_)) {
                        // Notify all SSE clients that config changed
                        let _ = config_change_tx.send(());
                    }
                }
                Err(_) => break,
                _ => {}
            }
        }
        Ok(())
    });

    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Set global config path immediately (before any config loads)
    BandwidthConfig::set_config_path(args.cfg.clone());

    // Check if we're in audio test mode
    if args.audio_test {
        return run_audio_test_mode();
    }

    if args.test.is_some() {
        // Test mode needs tokio runtime
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(test_mode(&args));
    }

    // Get config file path (custom or default)
    let cfg_arg = args.cfg.as_deref();
    let config_path = BandwidthConfig::config_path(cfg_arg)?;
    let config_file_exists = config_path.exists();

    // Check for first-run scenario BEFORE setting up terminal
    // First-run: no config file exists - always run setup to get WLED IP and total LEDs
    if !config_file_exists && cfg_arg.is_none() {
        // First run - run interactive setup to get essential configuration
        // This is required for both bandwidth mode AND MIDI mode
        // Only auto-run setup for default config, not custom configs
        let _config = run_first_time_setup(args.midi)?;
        // Config has been saved by run_first_time_setup, continue to normal startup
    }

    // Create tokio runtime for bandwidth reading task only - keep it alive for entire session
    let _rt = tokio::runtime::Runtime::new()?;

    // Load existing config or create default, then merge with command line args
    // Note: config_file_exists was already checked above for first-run detection
    let mut config = if config_file_exists {
        // Config file exists - load it or fail with error message
        match BandwidthConfig::load_with_path(cfg_arg) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("\n❌ Failed to load config file: {}", e);
                eprintln!("Config file: {}", config_path.display());
                eprintln!("\nPlease fix the config file or delete it to regenerate with defaults.");
                return Err(e);
            }
        }
    } else {
        // No config file - use defaults (will be saved below)
        let mut default_config = BandwidthConfig::default();
        default_config.config_path = Some(config_path.clone());
        default_config
    };

    let args_provided = config.merge_with_args(&args);

    // Save config ONLY if:
    // - Config file doesn't exist (first run setup - need to create it)
    // - Command-line args were provided (need to persist user's CLI choices)
    if !config_file_exists || args_provided {
        config.save()?;
    }

    println!("Using config file: {}", config.config_path.as_ref().unwrap().display());

    // Create broadcast channel for SSE config change notifications
    // Buffer size of 100 should be enough for config change events
    let (config_change_tx, _config_change_rx) = broadcast::channel(100);

    // Create shared webcam state for HTTP server and webcam mode
    let config_arc = Arc::new(tokio::sync::RwLock::new(config.clone()));
    let webcam_state = Arc::new(webcam::WebcamState::new(config_arc));

    // Start HTTP server if enabled
    let _http_server_handle = spawn_http_server(&config, config_change_tx.clone(), webcam_state.clone())?;

    // Start config watcher for dynamic changes
    spawn_config_watcher(config_change_tx.clone())?;

    // Print mode switching info
    println!("\n=== Dynamic Configuration ===");
    println!("Current mode: {}", config.mode);
    println!("Config changes apply automatically:");
    println!("  - Mode changes: Switches dynamically (no restart needed!)");
    println!("  - Network interface changes: Restarts monitoring automatically");
    println!("  - WLED IP changes: Reconnects automatically");
    println!("  - HTTP server changes: Restarts automatically");
    println!("  - Other settings: Apply in real-time");
    println!();

    // Main mode switching loop - allows dynamic mode changes without restart
    'mode_loop: loop {
        // Reload config to get latest mode setting
        let mut current_config = BandwidthConfig::load().unwrap_or(config.clone());

        match current_config.mode.as_str() {
            "midi" => {
                println!("\n🎵 Starting MIDI mode...");
                match run_midi_mode(&current_config, args.midi_device.clone(), args.midi_random_colors, config_change_tx.clone()) {
                    Ok(ModeExitReason::UserQuit) => {
                        println!("\n👋 Application exiting.");
                        return Ok(());
                    }
                    Ok(ModeExitReason::ModeChanged) => {
                        println!("\n🔄 MIDI mode exited, switching modes...");
                    }
                    Err(e) => {
                        eprintln!("\n❌ MIDI mode error: {}", e);
                        return Err(e);
                    }
                }
            }
            "live" => {
                println!("\n🎧 Starting Live Audio mode...");
                match run_live_mode(&current_config, args.delay, config_change_tx.clone()) {
                    Ok(ModeExitReason::UserQuit) => {
                        println!("\n👋 Application exiting.");
                        return Ok(());
                    }
                    Ok(ModeExitReason::ModeChanged) => {
                        println!("\n🔄 Live Audio mode exited, switching modes...");
                    }
                    Err(e) => {
                        eprintln!("\n❌ Live Audio mode error: {}", e);
                        return Err(e);
                    }
                }
            }
            "relay" => {
                println!("\n🔄 Starting Relay mode...");
                let shutdown = Arc::new(AtomicBool::new(false));
                match relay::run_relay_mode(current_config.clone(), shutdown) {
                    Ok(ModeExitReason::UserQuit) => {
                        println!("\n👋 Application exiting.");
                        return Ok(());
                    }
                    Ok(ModeExitReason::ModeChanged) => {
                        println!("\n🔄 Relay mode exited, restarting...");
                    }
                    Err(e) => {
                        eprintln!("\n❌ Relay mode error: {}", e);
                        return Err(e);
                    }
                }
            }
            "webcam" => {
                println!("\n📹 Webcam mode active - stream via web interface");
                println!("   Web UI: http{}://{}:{}", if current_config.httpd_https_enabled { "s" } else { "" }, current_config.httpd_ip, current_config.httpd_port);

                // Get webcam state for stats (already created above)
                let webcam_state_ref = webcam_state.clone();
                let current_config_clone = current_config.clone();

                // Run async TUI in a new tokio runtime
                _rt.block_on(async {
                    // Setup terminal for TUI
                    enable_raw_mode().unwrap();
                    let mut stdout = io::stdout();
                    stdout.execute(EnterAlternateScreen).unwrap();
                    let backend = CrosstermBackend::new(stdout);
                    let mut terminal = Terminal::new(backend).unwrap();
                    terminal.clear().unwrap();
                    terminal.hide_cursor().unwrap();

                    // Subscribe to SSE broadcast channel for config changes (no file watching needed)
                    let mut config_change_rx = config_change_tx.subscribe();

                    let mut last_frame_count = 0u64;
                    let mut fps = 0.0f64;
                    let mut last_fps_update = Instant::now();
                    let mut config = current_config_clone;

                    // Main TUI loop
                    loop {
                        // Check for config changes via SSE broadcast
                        if let Ok(()) = config_change_rx.try_recv() {
                            if let Ok(new_config) = BandwidthConfig::load() {
                                if new_config.mode != "webcam" {
                                    // Cleanup terminal
                                    terminal.show_cursor().unwrap();
                                    disable_raw_mode().unwrap();
                                    terminal.backend_mut().execute(LeaveAlternateScreen).unwrap();
                                    println!("\n🔄 Mode changed, restarting...");
                                    break;
                                }
                                config = new_config;
                            }
                        }

                        // Calculate FPS and get stats
                        let current_frame_count = *webcam_state_ref.frame_count.read().await;
                        let frames_sent = webcam_state_ref.frames_sent.load(Ordering::SeqCst);
                        let frames_dropped = webcam_state_ref.frames_dropped.load(Ordering::SeqCst);
                        let elapsed = last_fps_update.elapsed();

                        if elapsed.as_secs() >= 1 {
                            let frame_delta = current_frame_count.saturating_sub(last_frame_count);
                            fps = frame_delta as f64 / elapsed.as_secs_f64();
                            last_frame_count = current_frame_count;
                            last_fps_update = Instant::now();
                        }

                        // Calculate drop rate
                        let drop_rate = if current_frame_count > 0 {
                            (frames_dropped as f64 / current_frame_count as f64) * 100.0
                        } else {
                            0.0
                        };

                        // Render TUI
                        terminal.draw(|f| {
                            let chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints([
                                    Constraint::Length(3),  // Header
                                    Constraint::Min(10),    // Main content
                                    Constraint::Length(3),  // Footer
                                ])
                                .split(f.size());

                            // Header
                            let header = Paragraph::new("Webcam Mode")
                                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                                .alignment(Alignment::Center)
                                .block(Block::default().borders(Borders::ALL));
                            f.render_widget(header, chunks[0]);

                            // Main content - webcam stats
                            let frame_size = config.webcam_frame_width * config.webcam_frame_height * 3;
                            let stats_text = format!(
                                "Frame Size: {}x{} ({} bytes)\n\
Target FPS: {:.1}\n\
Actual FPS: {:.1}\n\
\n\
Frames Received: {}\n\
Frames Sent: {} ({:.1}% success)\n\
Frames Dropped: {} ({:.1}%)\n\
\n\
WLED IP: {}\n\
Stream from: http{}://{}:{}",
                                config.webcam_frame_width,
                                config.webcam_frame_height,
                                frame_size,
                                config.webcam_target_fps,
                                fps,
                                current_frame_count,
                                frames_sent,
                                100.0 - drop_rate,
                                frames_dropped,
                                drop_rate,
                                config.wled_ip,
                                if config.httpd_https_enabled { "s" } else { "" },
                                config.httpd_ip,
                                config.httpd_port
                            );

                            let stats = Paragraph::new(stats_text)
                                .style(Style::default().fg(Color::White))
                                .block(Block::default().borders(Borders::ALL).title("Stats"));
                            f.render_widget(stats, chunks[1]);

                            // Footer
                            let footer_text = "Press 'q' to quit | Change mode in config file to switch modes";
                            let footer = Paragraph::new(footer_text)
                                .style(Style::default().fg(Color::Gray))
                                .alignment(Alignment::Center)
                                .block(Block::default().borders(Borders::ALL));
                            f.render_widget(footer, chunks[2]);
                        }).unwrap();

                        // Handle keyboard input
                        if event::poll(Duration::from_millis(100)).unwrap() {
                            if let Event::Key(key) = event::read().unwrap() {
                                if key.code == KeyCode::Char('q') || key.code == KeyCode::Char('Q') {
                                    // Cleanup terminal
                                    terminal.show_cursor().unwrap();
                                    disable_raw_mode().unwrap();
                                    terminal.backend_mut().execute(LeaveAlternateScreen).unwrap();
                                    println!("\nExiting...");
                                    std::process::exit(0);
                                }
                            }
                        }

                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                });
            }
            "tron" => {
                if current_config.tron_num_players == 1 {
                    println!("\n🐍 Starting Snake game mode...");
                } else {
                    println!("\n🎮 Starting Tron game mode...");
                }
                println!("   Grid: {}x{}", current_config.tron_width, current_config.tron_height);
                println!("   Players: {}", current_config.tron_num_players);
                println!("   Press 'q' to exit");

                // Setup terminal for TUI
                enable_raw_mode().unwrap();
                let mut stdout = io::stdout();
                stdout.execute(EnterAlternateScreen).unwrap();
                let backend = CrosstermBackend::new(stdout);
                let mut terminal = Terminal::new(backend).unwrap();
                terminal.clear().unwrap();
                terminal.hide_cursor().unwrap();

                // Create DDP connection
                let ddp_socket = UdpSocket::bind("0.0.0.0:0")?;
                let dest_addr = format!("{}:4048", current_config.wled_ip);
                let pixel_config = PixelConfig::default();
                let ddp_client = DDPConnection::try_new(&dest_addr, pixel_config, ID::Default, ddp_socket)?;
                let ddp_client_arc = Arc::new(Mutex::new(Some(ddp_client)));
                let config_arc = Arc::new(Mutex::new(current_config.clone()));

                // Subscribe to SSE broadcast channel for config changes (no file watching needed)
                let mut config_change_rx = config_change_tx.subscribe();

                // Create shutdown signal for tron game thread
                let shutdown = Arc::new(AtomicBool::new(false));

                // Spawn tron game in background thread
                let tron_config_arc = config_arc.clone();
                let tron_ddp_arc = ddp_client_arc.clone();
                let tron_shutdown = shutdown.clone();
                let tron_handle = thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        let _ = tron::run_tron_mode(tron_config_arc, tron_ddp_arc, tron_shutdown).await;
                    });
                });

                // TUI loop
                let mut last_update = Instant::now();
                let mut config = current_config.clone();
                loop {
                    // Check for config changes via SSE broadcast
                    if let Ok(()) = config_change_rx.try_recv() {
                        if let Ok(new_config) = BandwidthConfig::load() {
                            if new_config.mode != "tron" {
                                // Mode changed, signal shutdown and wait for thread to finish
                                shutdown.store(true, Ordering::Relaxed);
                                terminal.show_cursor().unwrap();
                                disable_raw_mode().unwrap();
                                terminal.backend_mut().execute(LeaveAlternateScreen).unwrap();
                                println!("\n🔄 Mode changed, stopping tron mode...");
                                let _ = tron_handle.join();
                                break;
                            }
                            config = new_config;
                        }
                    }

                    // Render TUI
                    if last_update.elapsed() >= Duration::from_millis(100) {
                        last_update = Instant::now();

                        terminal.draw(|f| {
                            let chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints([
                                    Constraint::Length(3),  // Header
                                    Constraint::Min(10),    // Main content
                                    Constraint::Length(3),  // Footer
                                ])
                                .split(f.size());

                            // Header - Mode name on left, quit instructions on right
                            let mode_name = if config.tron_num_players == 1 {
                                "🐍 Snake Mode"
                            } else {
                                "🎮 Tron Mode"
                            };
                            let header_spans = vec![
                                Span::styled(
                                    mode_name,
                                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                                ),
                                Span::raw("                                                 "),
                                Span::styled(
                                    "q and Ctrl+C",
                                    Style::default().fg(Color::Gray)
                                ),
                            ];
                            let header = Paragraph::new(Line::from(header_spans))
                                .block(Block::default().borders(Borders::ALL));
                            f.render_widget(header, chunks[0]);

                            // Main content - Game stats
                            let stats_text = if config.tron_num_players == 1 {
                                // Single player Snake mode
                                format!(
                                    "Grid: {}x{}\n\
Game Speed: {}ms per update\n\
Trail Length: {}\n\
\n\
Snake Color: {}",
                                    config.tron_width,
                                    config.tron_height,
                                    config.tron_speed_ms,
                                    if config.tron_trail_length == 0 { "Infinite".to_string() } else { config.tron_trail_length.to_string() },
                                    config.tron_player_1_color,
                                )
                            } else {
                                // Multi-player Tron mode
                                let player_colors = vec![
                                    &config.tron_player_1_color,
                                    &config.tron_player_2_color,
                                    &config.tron_player_3_color,
                                    &config.tron_player_4_color,
                                    &config.tron_player_5_color,
                                    &config.tron_player_6_color,
                                    &config.tron_player_7_color,
                                    &config.tron_player_8_color,
                                ];
                                let active_colors: Vec<String> = player_colors.iter()
                                    .take(config.tron_num_players)
                                    .map(|c| c.to_string())
                                    .collect();
                                let colors_display = active_colors.join(", ");

                                format!(
                                    "Grid: {}x{}\n\
Players: {}\n\
Game Speed: {}ms per update\n\
AI Look-Ahead: {} steps\n\
Trail Length: {}\n\
AI Aggression: {:.0}%\n\
\n\
Player Colors:\n  {}",
                                    config.tron_width,
                                    config.tron_height,
                                    config.tron_num_players,
                                    config.tron_speed_ms,
                                    config.tron_look_ahead,
                                    if config.tron_trail_length == 0 { "Infinite".to_string() } else { config.tron_trail_length.to_string() },
                                    config.tron_ai_aggression * 100.0,
                                    colors_display,
                                )
                            };

                            let content = Paragraph::new(stats_text)
                                .style(Style::default().fg(Color::White))
                                .block(Block::default().borders(Borders::ALL).title("Game Info"));
                            f.render_widget(content, chunks[1]);

                            // Footer - Status information
                            let footer_text = format!(
                                "WLED: {} | Config changes apply automatically",
                                config.wled_ip
                            );
                            let footer = Paragraph::new(footer_text)
                                .style(Style::default().fg(Color::Gray))
                                .block(Block::default().borders(Borders::ALL));
                            f.render_widget(footer, chunks[2]);
                        }).unwrap();
                    }

                    // Check for quit
                    if poll(Duration::from_millis(50)).unwrap() {
                        if let Event::Key(key) = event::read().unwrap() {
                            if key.code == KeyCode::Char('q') || key.code == KeyCode::Char('Q') ||
                               (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)) {
                                // Signal shutdown and wait for thread to finish
                                shutdown.store(true, Ordering::Relaxed);
                                // Cleanup terminal
                                terminal.show_cursor().unwrap();
                                disable_raw_mode().unwrap();
                                terminal.backend_mut().execute(LeaveAlternateScreen).unwrap();
                                println!("\nStopping tron mode...");
                                let _ = tron_handle.join();
                                println!("Exiting...");
                                std::process::exit(0);
                            }
                        }
                    }
                }
            }
            "geometry" => {
                match run_geometry_mode(&current_config, config_change_tx.clone()) {
                    Ok(ModeExitReason::UserQuit) => {
                        println!("\n👋 Application exiting.");
                        return Ok(());
                    }
                    Ok(ModeExitReason::ModeChanged) => {
                        println!("   Geometry mode exited, checking for mode change...");
                        continue; // Loop back to reload config and check new mode
                    }
                    Err(e) => {
                        eprintln!("Geometry mode error: {}", e);
                        return Err(e);
                    }
                }
            }
            "sand" => {
                println!("\n🏖️  Starting Falling Sand simulation mode...");
                match run_sand_mode(&current_config, config_change_tx.clone()) {
                    Ok(ModeExitReason::UserQuit) => {
                        println!("\n👋 Application exiting.");
                        return Ok(());
                    }
                    Ok(ModeExitReason::ModeChanged) => {
                        println!("   Sand mode exited, checking for mode change...");
                        continue; // Loop back to reload config and check new mode
                    }
                    Err(e) => {
                        eprintln!("Sand mode error: {}", e);
                        return Err(e);
                    }
                }
            }
            _ => {
                println!("\n📊 Starting network monitoring mode...");

                // Check if interface is configured - if not, auto-select first available
                if current_config.interface.trim().is_empty() {
                    // Get available interfaces
                    let available_interfaces = if !current_config.ssh_host.is_empty() {
                        let ssh_user = if current_config.ssh_user.is_empty() {
                            None
                        } else {
                            Some(current_config.ssh_user.as_str())
                        };
                        _rt.block_on(httpd::get_remote_network_interfaces(&current_config.ssh_host, ssh_user))?
                    } else {
                        httpd::get_network_interfaces()?
                    };

                    if available_interfaces.is_empty() {
                        return Err(anyhow::anyhow!("No network interfaces found"));
                    }

                    println!("\n⚠️  No network interface configured");
                    println!("Available interfaces: {}", available_interfaces.join(", "));
                    println!("\nAuto-selecting first interface: {}", available_interfaces[0]);
                    println!("(Set this in the web UI or config file to persist)");

                    // Auto-select first interface for this session only - DO NOT SAVE to avoid overwriting config
                    current_config.interface = available_interfaces[0].clone();
                }

                // Validate that configured interface(s) actually exist on the host
                let configured_interfaces: Vec<&str> = current_config.interface.split(',').map(|s| s.trim()).collect();

                // Get available interfaces based on whether we're using SSH or local
                let available_interfaces = if !current_config.ssh_host.is_empty() {
                    // Remote SSH host
                    let ssh_user = if current_config.ssh_user.is_empty() {
                        None
                    } else {
                        Some(current_config.ssh_user.as_str())
                    };

                    match _rt.block_on(httpd::get_remote_network_interfaces(&current_config.ssh_host, ssh_user)) {
                        Ok(interfaces) => interfaces,
                        Err(e) => {
                            eprintln!("\n❌ Error: Failed to get network interfaces from remote host: {}", e);
                            return Err(e);
                        }
                    }
                } else {
                    // Local host
                    match httpd::get_network_interfaces() {
                        Ok(interfaces) => interfaces,
                        Err(e) => {
                            eprintln!("\n❌ Error: Failed to get network interfaces: {}", e);
                            return Err(e);
                        }
                    }
                };

                // Check if all configured interfaces exist
                let mut invalid_interfaces = Vec::new();
                for iface in &configured_interfaces {
                    if !available_interfaces.contains(&iface.to_string()) {
                        invalid_interfaces.push(*iface);
                    }
                }

                // If any interfaces are invalid, auto-select first available
                if !invalid_interfaces.is_empty() {
                    eprintln!("\n⚠️  Configured interface(s) not found on host!");
                    eprintln!("Invalid: {}", invalid_interfaces.join(", "));
                    eprintln!("Available: {}", available_interfaces.join(", "));

                    if available_interfaces.is_empty() {
                        return Err(anyhow::anyhow!("No network interfaces found"));
                    }

                    println!("\nAuto-selecting first interface: {}", available_interfaces[0]);
                    println!("(Set this in the web UI or config file to persist)");

                    // Auto-select first interface for this session only - DO NOT SAVE to avoid overwriting config
                    current_config.interface = available_interfaces[0].clone();
                }

                // Run bandwidth mode inline (break to mode_loop when mode changes)
                let quiet = args.quiet;
                // Use current_config for this bandwidth mode session
                let mut config = current_config.clone();

    println!("Connecting to bandwidth monitor...");
    println!("Interface(s): {}", config.interface);
    if args.host.is_some() {
        println!("Please enter your SSH password when prompted...\n");
    }

    let child_result = _rt.block_on(spawn_bandwidth_monitor(&args, &config));
    let mut child = match child_result {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: Failed to start bandwidth monitor: {}", e);
            return Err(e);
        }
    };

    // For remote connections, wait for first line of output to ensure connection succeeded
    if args.host.is_some() {
        println!("Waiting for connection to establish...");

        let wait_result = _rt.block_on(async {
            if let Some(stdout) = child.stdout.take() {
                let mut reader = BufReader::new(stdout);
                let mut first_line = String::new();

                match reader.read_line(&mut first_line).await {
                    Ok(0) => {
                        Err(anyhow::anyhow!("SSH connection failed or closed immediately"))
                    }
                    Ok(_) => {
                        println!("Connection established!");
                        // Put stdout back for later use
                        child.stdout = Some(reader.into_inner());
                        Ok(())
                    }
                    Err(e) => {
                        Err(anyhow::anyhow!("Error reading from SSH: {}", e))
                    }
                }
            } else {
                Err(anyhow::anyhow!("No stdout available"))
            }
        });

        if let Err(e) = wait_result {
            eprintln!("Error: {}", e);
            eprintln!("Please check your SSH credentials and try again");
            return Err(e);
        }
    }

    println!("Connected successfully!\n");

    // Clear the terminal to remove password prompt residue
    print!("\x1B[2J\x1B[1;1H");
    io::stdout().flush()?;

    // NOW setup terminal - after SSH connection is established
    enable_raw_mode()?;
    let mut stdout_handle = io::stdout();
    stdout_handle.execute(EnterAlternateScreen)?;
    stdout_handle.flush()?;
    let backend = CrosstermBackend::new(stdout_handle);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    terminal.hide_cursor()?;

    // Create shared state for renderer
    // Resolve color strings (could be gradient names or hex colors)
    let tx_color = if config.tx_color.is_empty() {
        gradients::resolve_color_string(&config.color)
    } else {
        gradients::resolve_color_string(&config.tx_color)
    };
    let rx_color = if config.rx_color.is_empty() {
        gradients::resolve_color_string(&config.color)
    } else {
        gradients::resolve_color_string(&config.rx_color)
    };

    let interpolation_mode = match config.interpolation.to_lowercase().as_str() {
        "basis" => InterpolationMode::Basis,
        "catmullrom" | "catmull-rom" => InterpolationMode::CatmullRom,
        _ => InterpolationMode::Linear,
    };

    let direction = match config.direction.to_lowercase().as_str() {
        "mirrored" => DirectionMode::Mirrored,
        "opposing" => DirectionMode::Opposing,
        "left" => DirectionMode::Left,
        "right" => DirectionMode::Right,
        _ => DirectionMode::Mirrored,
    };

    // Create shutdown flag for clean termination
    let shutdown = Arc::new(AtomicBool::new(false));

    let shared_state = Arc::new(Mutex::new(SharedRenderState {
        current_rx_kbps: 0.0,
        current_tx_kbps: 0.0,
        start_rx_kbps: 0.0,
        start_tx_kbps: 0.0,
        last_bandwidth_update: None,
        animation_speed: config.animation_speed,
        scale_animation_speed: config.scale_animation_speed,
        tx_animation_direction: config.tx_animation_direction.clone(),
        rx_animation_direction: config.rx_animation_direction.clone(),
        interpolation_time_ms: config.interpolation_time_ms,
        enable_interpolation: config.enable_interpolation,
        max_bandwidth_kbps: config.max_gbps * 1000.0 * 1000.0,
        tx_color,
        rx_color,
        use_gradient: config.use_gradient,
        intensity_colors: config.intensity_colors,
        interpolation_mode,
        direction,
        swap: config.swap,
        fps: config.fps,
        ddp_delay_ms: config.ddp_delay_ms,
        global_brightness: config.global_brightness,
        total_leds: config.total_leds,
        rx_split_percent: config.rx_split_percent,
        strobe_on_max: config.strobe_on_max,
        strobe_rate_hz: config.strobe_rate_hz,
        strobe_duration_ms: config.strobe_duration_ms,
        strobe_color: config.strobe_color.clone(),
        test_mode: config.test_tx || config.test_rx,
        generation: 0,
    }));

    // Create renderer with multi-device support
    let renderer = match Renderer::new(&config, shared_state.clone(), shutdown.clone()) {
        Ok(r) => r,
        Err(e) => {
            terminal.show_cursor()?;
            disable_raw_mode()?;
            terminal.backend_mut().execute(LeaveAlternateScreen)?;
            return Err(e);
        }
    };

    // Spawn dedicated render thread - runs at 60 FPS independently
    thread::spawn(move || {
        renderer.run();
    });

    let (bandwidth_tx, bandwidth_rx) = mpsc::channel::<String>();

    // Message log stored locally
    let mut messages: Vec<String> = Vec::new();

    let leds_per_direction = config.total_leds / 2;

    // Helper function to calculate LEDs (same logic as renderer)
    let calculate_leds = |bandwidth_kbps: f64, max_bandwidth_kbps: f64| -> usize {
        let percentage = bandwidth_kbps / max_bandwidth_kbps;
        let leds = (percentage * leds_per_direction as f64) as usize;
        leds.min(leds_per_direction)
    };

    // Add initial message
    if !quiet {
        messages.push(format!(
            "[{}] Bandwidth meter started. Max: {} Gbps",
            get_timestamp(),
            config.max_gbps
        ));
        messages.push(format!(
            "[{}] Interface: {}, LEDs: {}, WLED: {}",
            get_timestamp(),
            config.interface, config.total_leds, config.wled_ip
        ));
        messages.push(format!("[{}] Config file: {}", get_timestamp(), config_path.display()));
        messages.push(format!("[{}] Edit config file to change settings while running", get_timestamp()));
        messages.push(format!("[{}] Debug log: /tmp/bandwidth_debug.log", get_timestamp()));
    }

    // Spawn bandwidth reader in separate tokio task
    let stdout = child.stdout.take().expect("Failed to capture stdout");
    _rt.spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        // Always create debug log file
        let mut debug_log = std::fs::File::create("/tmp/bandwidth_debug.log").ok();

        while let Ok(Some(line)) = lines.next_line().await {
            // Debug: write raw line with timestamp to file when received from SSH
            if let Some(ref mut log) = debug_log {
                use std::io::Write;
                let _ = writeln!(log, "[{}] SSH OUTPUT: {}", get_timestamp(), line);
                let _ = log.flush(); // Flush immediately so tail -f works
            }

            if bandwidth_tx.send(line).is_err() {
                break; // Main thread dropped receiver, time to exit
            }
        }
    });

    // Subscribe to SSE broadcast channel for config changes (no file watching needed)
    let mut config_change_rx = config_change_tx.subscribe();

    // Force initial render
    {
        terminal.draw(|f| {
            // Three-section layout: Header, Main Content, Footer
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(3)].as_ref())
                .split(f.size());

            // Header - show mode, sub-mode, and interface
            let sub_mode = if config.test_tx || config.test_rx {
                "Test"
            } else {
                "Normal"
            };
            let header_text = format!("📊 Bandwidth Mode | Sub-mode: {} | Interface: {}", sub_mode, config.interface);
            let header = Paragraph::new(header_text)
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, chunks[0]);

            // Main content - messages
            let messages_text: Vec<Line> = messages
                .iter()
                .rev()
                .take(chunks[1].height as usize)
                .rev()
                .map(|m| Line::from(m.as_str()))
                .collect();

            let messages_widget = Paragraph::new(messages_text).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Bandwidth Monitor"),
            );
            f.render_widget(messages_widget, chunks[1]);

            // Footer - show monitoring source and controls
            let footer_text = format!(
                "Source: Network [{}] | WLED: {} | LEDs: {} | FPS: {:.0} | Delay: {:.1}ms | Press 'i' for config, 'q' or Ctrl+C to quit",
                config.interface, config.wled_ip, config.total_leds, config.fps, config.ddp_delay_ms
            );
            let footer = Paragraph::new(footer_text)
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(footer, chunks[2]);
        })?;
    }

    let mut needs_render = true;

    // Initialize bandwidth tracker for Linux /proc/net/dev parsing
    let mut bandwidth_tracker: Option<BandwidthTracker> = Some(BandwidthTracker::new());

    // Initialize test mode bandwidth values if enabled
    if config.test_tx || config.test_rx {
        let mut state = shared_state.lock().unwrap();
        if config.test_rx {
            let test_rx_kbps = config.max_gbps * 1000.0 * 1000.0 * (config.test_rx_percent / 100.0);
            state.current_rx_kbps = test_rx_kbps;
            state.start_rx_kbps = test_rx_kbps;
            state.last_bandwidth_update = Some(Instant::now());
        }
        if config.test_tx {
            let test_tx_kbps = config.max_gbps * 1000.0 * 1000.0 * (config.test_tx_percent / 100.0);
            state.current_tx_kbps = test_tx_kbps;
            state.start_tx_kbps = test_tx_kbps;
            state.last_bandwidth_update = Some(Instant::now());
        }
    }

    // Config info toggle
    let show_config_info = Arc::new(Mutex::new(false));
    let show_config_info_clone = show_config_info.clone();

    // Simple main loop - just handle bandwidth and config updates
    // Rendering happens in dedicated thread at configurable FPS
    loop {
        // Check for keyboard input
        if poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        // Signal render thread to shut down
                        shutdown.store(true, Ordering::Relaxed);
                        thread::sleep(Duration::from_millis(100));
                        terminal.show_cursor()?;
                        disable_raw_mode()?;
                        terminal.backend_mut().execute(LeaveAlternateScreen)?;
                        break;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Signal render thread to shut down
                        shutdown.store(true, Ordering::Relaxed);
                        thread::sleep(Duration::from_millis(100));
                        terminal.show_cursor()?;
                        disable_raw_mode()?;
                        terminal.backend_mut().execute(LeaveAlternateScreen)?;
                        break;
                    }
                    KeyCode::Char('i') | KeyCode::Char('I') => {
                        let mut show = show_config_info.lock().unwrap();
                        *show = !*show;
                        drop(show);
                        terminal.clear()?;
                        needs_render = true;
                    }
                    _ => {}
                }
            }
        }

        // Check bandwidth updates - update shared state
        match bandwidth_rx.try_recv() {
            Ok(line) => {
                if let Some((rx_kbps, tx_kbps)) = parse_bandwidth_line(&line, &mut bandwidth_tracker) {
                    // Override with test values if test mode is enabled for each direction
                    let rx_kbps = if config.test_rx {
                        config.max_gbps * 1000.0 * 1000.0 * (config.test_rx_percent / 100.0)
                    } else {
                        rx_kbps
                    };

                    let tx_kbps = if config.test_tx {
                        config.max_gbps * 1000.0 * 1000.0 * (config.test_tx_percent / 100.0)
                    } else {
                        tx_kbps
                    };

                    // Update shared state (non-blocking for renderer)
                    {
                        let mut state = shared_state.lock().unwrap();
                        // Store current values as the starting point for interpolation
                        state.start_rx_kbps = state.current_rx_kbps;
                        state.start_tx_kbps = state.current_tx_kbps;
                        // Update to new target values
                        state.current_rx_kbps = rx_kbps;
                        state.current_tx_kbps = tx_kbps;
                        // Record the time when this update happened
                        state.last_bandwidth_update = Some(Instant::now());
                    }

                    // Generate messages for UI
                    let rx_leds = calculate_leds(rx_kbps, config.max_gbps * 1000.0 * 1000.0);
                    let tx_leds = calculate_leds(tx_kbps, config.max_gbps * 1000.0 * 1000.0);

                    // Always show both RX and TX on every update
                    if !quiet {
                        messages.push(format!(
                            "[{}] RX: {} LEDs ({:.1} Mbps) | TX: {} LEDs ({:.1} Mbps)",
                            get_timestamp(),
                            rx_leds,
                            rx_kbps / 1000.0,
                            tx_leds,
                            tx_kbps / 1000.0
                        ));
                        needs_render = true;
                    }

                    // Keep message buffer reasonable
                    if messages.len() > 1000 {
                        messages.remove(0);
                    }
                }
            }
            Err(_) => {
                // No new bandwidth data
            }
        }

        // Check config file updates via SSE broadcast
        if let Ok(()) = config_change_rx.try_recv() {
            if let Ok(new_config) = BandwidthConfig::load() {
                // Update shared state with new config
                {
                    let mut state = shared_state.lock().unwrap();

                    // Handle color updates using unified resolution system
                    let color_changed = new_config.color != config.color;
                    let tx_color_changed = new_config.tx_color != config.tx_color;
                    let rx_color_changed = new_config.rx_color != config.rx_color;

                    if tx_color_changed || rx_color_changed || color_changed {
                        // Use unified color resolution system
                        let (resolved_tx_color, resolved_rx_color) = resolve_tx_rx_colors(&new_config);

                        if tx_color_changed || (color_changed && new_config.tx_color.is_empty()) {
                            state.tx_color = resolved_tx_color.clone();
                            state.generation += 1;
                            if !quiet {
                                if new_config.tx_color.is_empty() {
                                    messages.push(format!(
                                        "[{}] TX color updated to: {} (from main color)",
                                        get_timestamp(),
                                        new_config.color
                                    ));
                                } else {
                                    messages.push(format!("[{}] TX color updated to: {}", get_timestamp(), new_config.tx_color));
                                }
                            }
                        }

                        if rx_color_changed || (color_changed && new_config.rx_color.is_empty()) {
                            state.rx_color = resolved_rx_color.clone();
                            state.generation += 1;
                            if !quiet {
                                if new_config.rx_color.is_empty() {
                                    messages.push(format!(
                                        "[{}] RX color updated to: {} (from main color)",
                                        get_timestamp(),
                                        new_config.color
                                    ));
                                } else {
                                    messages.push(format!("[{}] RX color updated to: {}", get_timestamp(), new_config.rx_color));
                                }
                            }
                        }
                    }

                    // Update max bandwidth
                    if new_config.max_gbps != config.max_gbps {
                        state.max_bandwidth_kbps = new_config.max_gbps * 1000.0 * 1000.0;
                        if !quiet {
                            messages.push(format!(
                                "[{}] Max bandwidth updated to: {} Gbps",
                                get_timestamp(),
                                new_config.max_gbps
                            ));
                        }
                    }

                    // Update direction
                    if new_config.direction != config.direction {
                        let direction = match new_config.direction.to_lowercase().as_str() {
                            "mirrored" => DirectionMode::Mirrored,
                            "opposing" => DirectionMode::Opposing,
                            "left" => DirectionMode::Left,
                            "right" => DirectionMode::Right,
                            _ => DirectionMode::Mirrored,
                        };
                        state.direction = direction;
                        state.generation += 1;
                        if !quiet {
                            messages.push(format!("[{}] Direction updated to: {}", get_timestamp(), new_config.direction));
                        }
                    }

                    // Update swap
                    if new_config.swap != config.swap {
                        state.swap = new_config.swap;
                        state.generation += 1;
                        if !quiet {
                            messages.push(format!(
                                "[{}] Swap: {}",
                                get_timestamp(),
                                if new_config.swap { "enabled" } else { "disabled" }
                            ));
                        }
                    }

                    // Update RX/TX split percentage
                    if new_config.rx_split_percent != config.rx_split_percent {
                        state.rx_split_percent = new_config.rx_split_percent;
                        if !quiet {
                            let tx_split = 100.0 - new_config.rx_split_percent;
                            messages.push(format!(
                                "[{}] LED split updated to: RX {:.0}% / TX {:.0}%",
                                get_timestamp(),
                                new_config.rx_split_percent,
                                tx_split
                            ));
                        }
                    }

                    // Update strobe on max
                    if new_config.strobe_on_max != config.strobe_on_max {
                        state.strobe_on_max = new_config.strobe_on_max;
                        if !quiet {
                            messages.push(format!(
                                "[{}] Strobe on max: {}",
                                get_timestamp(),
                                if new_config.strobe_on_max { "enabled" } else { "disabled" }
                            ));
                        }
                    }

                    // Update strobe rate
                    if new_config.strobe_rate_hz != config.strobe_rate_hz {
                        state.strobe_rate_hz = new_config.strobe_rate_hz;
                        // Also validate strobe_duration_ms doesn't exceed new cycle time
                        if new_config.strobe_rate_hz > 0.0 {
                            let max_duration = 1000.0 / new_config.strobe_rate_hz;
                            if state.strobe_duration_ms > max_duration {
                                state.strobe_duration_ms = max_duration;
                            }
                        }
                        if !quiet {
                            messages.push(format!(
                                "[{}] Strobe rate updated to: {:.1} Hz",
                                get_timestamp(),
                                new_config.strobe_rate_hz
                            ));
                        }
                    }

                    // Update strobe duration
                    if new_config.strobe_duration_ms != config.strobe_duration_ms {
                        state.strobe_duration_ms = new_config.strobe_duration_ms;
                        if !quiet {
                            messages.push(format!(
                                "[{}] Strobe duration updated to: {:.0} ms",
                                get_timestamp(),
                                new_config.strobe_duration_ms
                            ));
                        }
                    }

                    // Update strobe color
                    if new_config.strobe_color != config.strobe_color {
                        state.strobe_color = new_config.strobe_color.clone();
                        if !quiet {
                            messages.push(format!(
                                "[{}] Strobe color updated to: {}",
                                get_timestamp(),
                                new_config.strobe_color
                            ));
                        }
                    }

                    // Update animation speed
                    if new_config.animation_speed != config.animation_speed {
                        state.animation_speed = new_config.animation_speed;
                        if !quiet && new_config.animation_speed > 0.0 {
                            messages.push(format!(
                                "[{}] Animation speed: {:.3}",
                                get_timestamp(),
                                new_config.animation_speed
                            ));
                        }
                    }

                    // Update animation speed scaling
                    if new_config.scale_animation_speed != config.scale_animation_speed {
                        state.scale_animation_speed = new_config.scale_animation_speed;
                        if !quiet {
                            messages.push(format!(
                                "[{}] Animation speed scaling: {}",
                                get_timestamp(),
                                if new_config.scale_animation_speed {
                                    "enabled (scales with bandwidth)"
                                } else {
                                    "disabled (constant speed)"
                                }
                            ));
                        }
                    }

                    // Update TX animation direction
                    if new_config.tx_animation_direction != config.tx_animation_direction {
                        state.tx_animation_direction = new_config.tx_animation_direction.clone();
                        if !quiet {
                            messages.push(format!(
                                "[{}] TX animation direction: {}",
                                get_timestamp(),
                                new_config.tx_animation_direction
                            ));
                        }
                    }

                    // Update RX animation direction
                    if new_config.rx_animation_direction != config.rx_animation_direction {
                        state.rx_animation_direction = new_config.rx_animation_direction.clone();
                        if !quiet {
                            messages.push(format!(
                                "[{}] RX animation direction: {}",
                                get_timestamp(),
                                new_config.rx_animation_direction
                            ));
                        }
                    }

                    // Update interpolation time
                    if new_config.interpolation_time_ms != config.interpolation_time_ms {
                        state.interpolation_time_ms = new_config.interpolation_time_ms;
                        if !quiet {
                            messages.push(format!(
                                "[{}] Interpolation time: {} ms",
                                get_timestamp(),
                                new_config.interpolation_time_ms
                            ));
                        }
                    }

                    // Update enable interpolation
                    if new_config.enable_interpolation != config.enable_interpolation {
                        state.enable_interpolation = new_config.enable_interpolation;
                        if !quiet {
                            messages.push(format!(
                                "[{}] Interpolation: {}",
                                get_timestamp(),
                                if new_config.enable_interpolation { "enabled" } else { "disabled" }
                            ));
                        }
                    }

                    // Update interpolation
                    if new_config.interpolation != config.interpolation {
                        let interpolation_mode = match new_config.interpolation.to_lowercase().as_str() {
                            "basis" => InterpolationMode::Basis,
                            "catmullrom" | "catmull-rom" => InterpolationMode::CatmullRom,
                            _ => InterpolationMode::Linear,
                        };
                        state.interpolation_mode = interpolation_mode;
                        state.generation += 1;
                        if !quiet {
                            messages.push(format!(
                                "[{}] Interpolation updated to: {}",
                                get_timestamp(),
                                new_config.interpolation
                            ));
                        }
                    }

                    // Update gradient mode
                    if new_config.use_gradient != config.use_gradient {
                        state.use_gradient = new_config.use_gradient;
                        state.generation += 1;
                        if !quiet {
                            messages.push(format!(
                                "[{}] Gradient mode: {}",
                                get_timestamp(),
                                if new_config.use_gradient {
                                    "enabled (smooth gradients)"
                                } else {
                                    "disabled (hard segments)"
                                }
                            ));
                        }
                    }

                    // Update intensity colors mode
                    if new_config.intensity_colors != config.intensity_colors {
                        state.intensity_colors = new_config.intensity_colors;
                        state.generation += 1;
                        if !quiet {
                            messages.push(format!(
                                "[{}] Intensity colors: {}",
                                get_timestamp(),
                                if new_config.intensity_colors {
                                    "enabled (level-based color)"
                                } else {
                                    "disabled (spatial gradient)"
                                }
                            ));
                        }
                    }

                    // Update FPS
                    if new_config.fps != config.fps {
                        state.fps = new_config.fps;
                        if !quiet {
                            messages.push(format!("[{}] FPS updated to: {}", get_timestamp(), new_config.fps));
                        }
                    }

                    // Update global brightness
                    if new_config.global_brightness != config.global_brightness {
                        state.global_brightness = new_config.global_brightness;
                        if !quiet {
                            messages.push(format!("[{}] Global brightness updated to: {:.0}%", get_timestamp(), new_config.global_brightness * 100.0));
                        }
                    }
                }

                // Check if mode changed - if so, exit bandwidth mode to allow mode switch
                if new_config.mode != "bandwidth" {
                    println!("\n🔄 Mode changed to '{}', exiting Bandwidth mode...", new_config.mode);
                    // Signal render thread to shut down
                    shutdown.store(true, Ordering::Relaxed);
                    // Give render thread a moment to exit cleanly
                    thread::sleep(Duration::from_millis(100));
                    // Clean up terminal
                    terminal.show_cursor()?;
                    disable_raw_mode()?;
                    terminal.backend_mut().execute(LeaveAlternateScreen)?;
                    // Exit bandwidth mode and continue mode loop
                    println!("\n🔄 Bandwidth mode exited, checking for mode change...");
                    continue 'mode_loop;
                }

                // Check if network interface changed - restart to apply
                if new_config.interface != config.interface
                    || new_config.ssh_host != config.ssh_host
                    || new_config.ssh_user != config.ssh_user
                {
                    println!("\n🔄 Network interface settings changed, restarting bandwidth monitoring...");
                    // Signal render thread to shut down
                    shutdown.store(true, Ordering::Relaxed);
                    // Give render thread a moment to exit cleanly
                    thread::sleep(Duration::from_millis(100));
                    // Clean up terminal
                    terminal.show_cursor()?;
                    disable_raw_mode()?;
                    terminal.backend_mut().execute(LeaveAlternateScreen)?;
                    // Exit and restart bandwidth mode with new interface settings
                    continue 'mode_loop;
                }

                // Check if total_leds or device config changed - restart to apply
                let devices_changed = new_config.wled_devices.len() != config.wled_devices.len() ||
                    new_config.wled_devices.iter().zip(config.wled_devices.iter()).any(|(new, old)| {
                        new.ip != old.ip ||
                        new.led_offset != old.led_offset ||
                        new.led_count != old.led_count ||
                        new.enabled != old.enabled
                    }) ||
                    new_config.multi_device_send_parallel != config.multi_device_send_parallel ||
                    new_config.multi_device_fail_fast != config.multi_device_fail_fast;

                if new_config.total_leds != config.total_leds || devices_changed {
                    println!("\n🔄 LED count or device config changed, restarting bandwidth mode...");
                    // Signal render thread to shut down
                    shutdown.store(true, Ordering::Relaxed);
                    // Give render thread a moment to exit cleanly
                    thread::sleep(Duration::from_millis(100));
                    // Clean up terminal
                    terminal.show_cursor()?;
                    disable_raw_mode()?;
                    terminal.backend_mut().execute(LeaveAlternateScreen)?;
                    // Exit and restart bandwidth mode with new settings
                    continue 'mode_loop;
                }

                // Check if WLED IP changed - just show message (DDP reconnects automatically)
                if new_config.wled_ip != config.wled_ip {
                    if !quiet {
                        messages.push(format!("[{}] WLED IP changed to {}", get_timestamp(), new_config.wled_ip));
                    }
                }

                // Update test mode - immediately update bandwidth values and tracking vars
                if new_config.test_tx != config.test_tx
                    || new_config.test_rx != config.test_rx
                    || new_config.test_tx_percent != config.test_tx_percent
                    || new_config.test_rx_percent != config.test_rx_percent {

                    // Calculate test bandwidth values
                    let test_rx_kbps = if new_config.test_rx {
                        new_config.max_gbps * 1000.0 * 1000.0 * (new_config.test_rx_percent / 100.0)
                    } else {
                        0.0
                    };

                    let test_tx_kbps = if new_config.test_tx {
                        new_config.max_gbps * 1000.0 * 1000.0 * (new_config.test_tx_percent / 100.0)
                    } else {
                        0.0
                    };

                    // Update shared state only if test mode is enabled
                    let mut state = shared_state.lock().unwrap();

                    // Update test mode flag and target values
                    state.test_mode = new_config.test_tx || new_config.test_rx;

                    if new_config.test_rx {
                        state.current_rx_kbps = test_rx_kbps;
                    }

                    if new_config.test_tx {
                        state.current_tx_kbps = test_tx_kbps;
                    }

                    drop(state);

                    if !quiet {
                        if new_config.test_tx != config.test_tx {
                            messages.push(format!(
                                "[{}] Test TX: {}",
                                get_timestamp(),
                                if new_config.test_tx { "enabled" } else { "disabled" }
                            ));
                        }
                        if new_config.test_rx != config.test_rx {
                            messages.push(format!(
                                "[{}] Test RX: {}",
                                get_timestamp(),
                                if new_config.test_rx { "enabled" } else { "disabled" }
                            ));
                        }
                        if new_config.test_tx_percent != config.test_tx_percent && new_config.test_tx {
                            messages.push(format!(
                                "[{}] Test TX utilization: {:.0}%",
                                get_timestamp(),
                                new_config.test_tx_percent
                            ));
                        }
                        if new_config.test_rx_percent != config.test_rx_percent && new_config.test_rx {
                            messages.push(format!(
                                "[{}] Test RX utilization: {:.0}%",
                                get_timestamp(),
                                new_config.test_rx_percent
                            ));
                        }
                    }
                }

                // Update config for future comparisons
                config = new_config;

                needs_render = true;
            }
        }

        // Render only when something changed
        if needs_render {
            // Build interface display string
            let interface_display = if !config.ssh_host.is_empty() {
                format!("{} (SSH: {}@{})", config.interface,
                    if config.ssh_user.is_empty() { "user" } else { &config.ssh_user },
                    config.ssh_host)
            } else {
                config.interface.clone()
            };

            terminal.draw(|f| {
                // Three-section layout: Header, Main Content, Footer
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(3)].as_ref())
                    .split(f.size());

                // Header - show mode, sub-mode, and interface
                let sub_mode = if config.test_tx || config.test_rx {
                    "Test"
                } else {
                    "Normal"
                };
                let header_text = format!("📊 Bandwidth Mode | Sub-mode: {} | Interface: {}", sub_mode, interface_display);
                let header = Paragraph::new(header_text)
                    .block(Block::default().borders(Borders::ALL));
                f.render_widget(header, chunks[0]);

                // Main content - toggle between messages and config viewer
                let show_config = show_config_info_clone.lock().unwrap();
                if *show_config {
                    let config_lines = generate_config_info_display(&config);
                    let config_widget = Paragraph::new(config_lines)
                        .block(Block::default().borders(Borders::ALL).title("Configuration (Press 'i' to hide)"));
                    f.render_widget(config_widget, chunks[1]);
                } else {
                    // Messages area
                    let messages_text: Vec<Line> = messages
                        .iter()
                        .rev()
                        .take(chunks[1].height as usize)
                        .rev()
                        .map(|m| Line::from(m.as_str()))
                        .collect();

                    let messages_widget = Paragraph::new(messages_text).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Bandwidth Monitor"),
                    );
                    f.render_widget(messages_widget, chunks[1]);
                }
                drop(show_config);

                // Footer - show monitoring source and controls
                let footer_text = format!(
                    "Source: Network [{}] | WLED: {} | LEDs: {} | FPS: {:.0} | Delay: {:.1}ms | Press 'i' for config, 'q' or Ctrl+C to quit",
                    interface_display, config.wled_ip, config.total_leds, config.fps, config.ddp_delay_ms
                );
                let footer = Paragraph::new(footer_text)
                    .block(Block::default().borders(Borders::ALL));
                f.render_widget(footer, chunks[2]);
            })?;

            needs_render = false;
        }

        // Small sleep to avoid busy-waiting CPU at 100%
        // Renderer runs in separate thread, so main loop can sleep longer
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

                // Bandwidth mode ended normally (Ctrl+C)
                println!("\n🔄 Bandwidth mode exited normally");
                return Ok(());
            }
        }

        // Small delay before checking mode again
        thread::sleep(Duration::from_millis(100));
    }
}
