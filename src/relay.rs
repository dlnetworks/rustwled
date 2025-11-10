// Relay Module - UDP frame relay for WLED via DDP protocol
use anyhow::Result;
use crossterm::event::{poll, read, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use notify::{Config, Event as NotifyEvent, RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;
use std::collections::VecDeque;
use std::io;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use crate::config::BandwidthConfig;
use crate::types::ModeExitReason;
use crate::multi_device::{MultiDeviceConfig, MultiDeviceManager, WLEDDevice};

/// Generate config info display for relay mode
fn generate_relay_config_info(config: &BandwidthConfig) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled("UDP Listen IP: ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("{}", config.relay_listen_ip)),
        ]),
        Line::from(vec![
            Span::styled("UDP Listen Port: ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("{}", config.relay_listen_port)),
        ]),
        Line::from(vec![
            Span::styled("Frame Width: ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("{} pixels", config.relay_frame_width)),
        ]),
        Line::from(vec![
            Span::styled("Frame Height: ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("{} pixels", config.relay_frame_height)),
        ]),
        Line::from(vec![
            Span::styled("Frame Size: ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("{} bytes", config.relay_frame_width * config.relay_frame_height * 3)),
        ]),
        Line::from(vec![
            Span::styled("WLED IP: ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("{}", config.wled_ip)),
        ]),
        Line::from(vec![
            Span::styled("DDP Delay: ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("{:.1} ms", config.ddp_delay_ms)),
        ]),
    ]
}

/// Run relay mode - listen for raw RGB24 frames on UDP and forward via DDP
pub fn run_relay_mode(
    config: BandwidthConfig,
    shutdown: Arc<AtomicBool>,
) -> Result<ModeExitReason> {
    // Set up config file watcher for dynamic reloading
    let (config_tx, config_rx) = mpsc::channel::<BandwidthConfig>();
    let config_path = BandwidthConfig::config_path(None)?;

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<NotifyEvent, _>| {
            if res.is_ok() {
                if let Ok(new_config) = BandwidthConfig::load() {
                    let _ = config_tx.send(new_config);
                }
            }
        },
        Config::default(),
    )?;

    if watcher.watch(&config_path, RecursiveMode::NonRecursive).is_err() {
        eprintln!("⚠️  Could not watch config file for changes");
    }

    // Track current config values
    let mut current_config = config.clone();
    let mut current_ddp_delay = current_config.ddp_delay_ms;
    let frame_size = current_config.relay_frame_width * current_config.relay_frame_height * 3;

    // Create UDP socket for receiving with timeout for non-blocking operation
    let socket = UdpSocket::bind(format!("{}:{}", current_config.relay_listen_ip, current_config.relay_listen_port))?;
    socket.set_read_timeout(Some(Duration::from_millis(10)))?;  // 10ms timeout for responsive UI

    // Create multi-device manager for forwarding
    let devices: Vec<WLEDDevice> = current_config.wled_devices.iter().map(|d| WLEDDevice {
        ip: d.ip.clone(),
        led_offset: d.led_offset,
        led_count: d.led_count,
        enabled: d.enabled,
    }).collect();

    let md_config = MultiDeviceConfig {
        devices,
        send_parallel: current_config.multi_device_send_parallel,
        fail_fast: current_config.multi_device_fail_fast,
    };

    let mut multi_device_manager = MultiDeviceManager::new(md_config)?;

    let mut frame_buffer = Vec::with_capacity(frame_size);
    let mut frame_count = 0u64;
    let mut last_frame_time = Instant::now();
    let mut current_fps = 0.0;
    let mut last_receive_time = Instant::now();
    let mut first_frame_received = false;

    // DDP delay ring buffer - stores (send_time, frame_data)
    let mut ddp_buffer: VecDeque<(Instant, Vec<u8>)> = VecDeque::new();

    // Event log for TUI (store last 100 events)
    let event_log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let event_log_render = event_log.clone();

    // Setup terminal for TUI
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Config info toggle
    let mut show_config_info = false;

    // Add ffmpeg example command to event log
    {
        let mut log = event_log.lock().unwrap();
        log.push(format!("🔄 Relay mode started"));
        log.push(format!(""));
        log.push(format!("Example ffmpeg command:"));
        log.push(format!("  ffmpeg -re -i <input> -an -vf scale={}:{} -f rawvideo -pix_fmt rgb24 -s {}x{} udp://{}:{}",
            current_config.relay_frame_width,
            current_config.relay_frame_height,
            current_config.relay_frame_width,
            current_config.relay_frame_height,
            current_config.relay_listen_ip,
            current_config.relay_listen_port));
        log.push(format!(""));
        log.push(format!("Waiting for frames..."));
    }

    loop {
        let loop_start = Instant::now();

        // Check for keyboard input (non-blocking)
        if poll(Duration::from_millis(0))? {
            if let Event::Key(key) = read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        // Cleanup terminal
                        terminal.show_cursor()?;
                        disable_raw_mode()?;
                        terminal.backend_mut().execute(LeaveAlternateScreen)?;
                        println!("\n👋 Relay mode stopped.\n");
                        return Ok(ModeExitReason::UserQuit);
                    },
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Cleanup terminal
                        terminal.show_cursor()?;
                        disable_raw_mode()?;
                        terminal.backend_mut().execute(LeaveAlternateScreen)?;
                        println!("\n👋 Relay mode stopped.\n");
                        return Ok(ModeExitReason::UserQuit);
                    },
                    KeyCode::Char('i') | KeyCode::Char('I') => {
                        show_config_info = !show_config_info;
                        terminal.clear()?;
                    },
                    _ => {}
                }
            }
        }

        // Check for shutdown signal
        if shutdown.load(Ordering::Relaxed) {
            // Cleanup terminal
            terminal.show_cursor()?;
            disable_raw_mode()?;
            terminal.backend_mut().execute(LeaveAlternateScreen)?;
            println!("\n👋 Relay mode stopped.\n");
            return Ok(ModeExitReason::UserQuit);
        }

        // Check for config changes
        if let Ok(new_config) = config_rx.try_recv() {
            // Check if we need to restart (IP, port, or frame dimensions changed)
            if new_config.relay_listen_ip != current_config.relay_listen_ip ||
               new_config.relay_listen_port != current_config.relay_listen_port ||
               new_config.relay_frame_width != current_config.relay_frame_width ||
               new_config.relay_frame_height != current_config.relay_frame_height ||
               new_config.mode != "relay" {
                // Cleanup terminal before restart
                terminal.show_cursor()?;
                disable_raw_mode()?;
                terminal.backend_mut().execute(LeaveAlternateScreen)?;

                let mut log = event_log.lock().unwrap();
                log.push(format!("🔄 Configuration changed, restarting..."));
                drop(log);

                return Ok(ModeExitReason::ModeChanged);
            }

            // Handle in-place updates
            if new_config.ddp_delay_ms != current_ddp_delay {
                current_ddp_delay = new_config.ddp_delay_ms;
                let mut log = event_log.lock().unwrap();
                log.push(format!("⏱️  DDP packet delay updated: {:.1} ms", current_ddp_delay));
                if log.len() > 100 {
                    log.remove(0);
                }
            }

            current_config = new_config;
        }

        // Receive packets (non-blocking) - accumulate data into buffer
        let mut packet_buf = [0u8; 65535];  // Max UDP packet size
        match socket.recv_from(&mut packet_buf) {
            Ok((size, _src)) => {
                frame_buffer.extend_from_slice(&packet_buf[..size]);
                last_receive_time = Instant::now();
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No data available - check if we've been waiting too long
                if last_receive_time.elapsed() > Duration::from_secs(5) {
                    // Reset buffer if no data for 5 seconds - forces resync on stream restart
                    if !frame_buffer.is_empty() {
                        let mut log = event_log.lock().unwrap();
                        log.push(format!("⚠️  Stream timeout - clearing {} bytes to resync", frame_buffer.len()));
                        if log.len() > 100 {
                            log.remove(0);
                        }
                        frame_buffer.clear();
                    }
                    last_receive_time = Instant::now();
                }
            },
            Err(e) => {
                let mut log = event_log.lock().unwrap();
                log.push(format!("❌ UDP recv error: {}", e));
                if log.len() > 100 {
                    log.remove(0);
                }
            }
        }

        // Process ALL complete frames available in the buffer
        while frame_buffer.len() >= frame_size {
            // Extract exactly frame_size bytes for this frame
            let frame_data: Vec<u8> = frame_buffer.drain(0..frame_size).collect();

            // Add frame to delay buffer with timestamp
            let delay_duration = Duration::from_micros((current_ddp_delay * 1000.0) as u64);
            let send_time = loop_start + delay_duration;
            ddp_buffer.push_back((send_time, frame_data));

            // Update stats
            frame_count += 1;
            let frame_elapsed = last_frame_time.elapsed();
            if frame_elapsed.as_secs_f64() > 0.0 {
                current_fps = 1.0 / frame_elapsed.as_secs_f64();
            }
            last_frame_time = Instant::now();

            // Log when first frame is received
            if !first_frame_received {
                first_frame_received = true;
                let mut log = event_log.lock().unwrap();
                log.push(format!("✅ First frame received! Relay active."));
                log.push(format!("Expected frame size: {} bytes ({}x{} @ RGB24)",
                    frame_size,
                    current_config.relay_frame_width,
                    current_config.relay_frame_height));
            }

            // Don't log routine frames - stats are in footer
        }

        // Safety check: if buffer is growing unbounded, log warning (but don't clear!)
        if frame_buffer.len() > frame_size * 10 {
            let mut log = event_log.lock().unwrap();
            log.push(format!("⚠️  Buffer very large: {} bytes ({} frames behind)",
                frame_buffer.len(), frame_buffer.len() / frame_size));
            if log.len() > 100 {
                log.remove(0);
            }
        }

        // Send all frames that are ready (send_time <= now) with global brightness
        let now = Instant::now();
        while let Some((send_time, _)) = ddp_buffer.front() {
            if *send_time <= now {
                if let Some((_, frame_to_send)) = ddp_buffer.pop_front() {
                    let _ = multi_device_manager.send_frame_with_brightness(&frame_to_send, Some(current_config.global_brightness));
                }
            } else {
                break;
            }
        }

        // Draw TUI
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),  // Header
                    Constraint::Min(10),    // Main content
                    Constraint::Length(3),  // Footer
                ])
                .split(f.size());

            // Header - Mode and frame info with controls on right
            let header_width = chunks[0].width.saturating_sub(2) as usize; // Subtract borders
            let left_text = format!("🔄 Relay Mode | Frame: {}x{} ({} bytes)",
                current_config.relay_frame_width,
                current_config.relay_frame_height,
                frame_size);
            let right_text = "Press 'i' for config, 'q' or Ctrl+C to quit";
            let spacing = header_width.saturating_sub(left_text.len() + right_text.len());
            let header_line = Line::from(vec![
                Span::raw(left_text),
                Span::raw(" ".repeat(spacing)),
                Span::raw(right_text),
            ]);
            let header = Paragraph::new(header_line)
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, chunks[0]);

            // Main content - either config info or event log
            if show_config_info {
                let config_lines = generate_relay_config_info(&current_config);
                let config_widget = Paragraph::new(config_lines)
                    .block(Block::default().borders(Borders::ALL).title("Configuration (Press 'i' to hide)"));
                f.render_widget(config_widget, chunks[1]);
            } else {
                // Event log
                let log = event_log_render.lock().unwrap();
                let log_text: Vec<Line> = log.iter().map(|s| Line::from(s.as_str())).collect();
                let log_widget = Paragraph::new(log_text)
                    .block(Block::default().borders(Borders::ALL).title("Relay Events"));
                f.render_widget(log_widget, chunks[1]);
            }

            // Footer - Status info only
            let footer_text = format!(
                "Frames: {} | FPS: {:.1} | Delay: {:.1}ms | UDP: {}:{} -> WLED: {} | LEDs: {}",
                frame_count,
                current_fps,
                current_ddp_delay,
                current_config.relay_listen_ip,
                current_config.relay_listen_port,
                current_config.wled_ip,
                current_config.total_leds
            );
            let footer = Paragraph::new(footer_text)
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(footer, chunks[2]);
        })?;

        // No sleep - we want minimal latency for real-time relay
        // The non-blocking socket with 10ms timeout prevents CPU spinning
    }
}
