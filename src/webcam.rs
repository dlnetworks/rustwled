// Webcam Mode Module - WebSocket-based webcam frame streaming to WLED
use anyhow::Result;
use axum::extract::ws::{Message, WebSocket};
use futures::StreamExt;
use image::{ImageBuffer, RgbaImage};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

use crate::config::BandwidthConfig;
use crate::multi_device::{MultiDeviceConfig, MultiDeviceManager, WLEDDevice};

use std::sync::atomic::{AtomicU64, Ordering};

use std::time::Instant;

/// Shared state for webcam mode
pub struct WebcamState {
    pub config: Arc<RwLock<BandwidthConfig>>,
    pub multi_device_manager: Arc<Mutex<Option<MultiDeviceManager>>>,
    pub frame_count: Arc<RwLock<u64>>,        // Total frames received
    pub frames_sent: Arc<AtomicU64>,          // Frames actually sent
    pub frames_dropped: Arc<AtomicU64>,       // Frames dropped due to backpressure
    pub last_frame_time: Arc<Mutex<Instant>>, // Last time a frame was sent to DDP
}

impl WebcamState {
    pub fn new(config: Arc<RwLock<BandwidthConfig>>) -> Self {
        Self {
            config,
            multi_device_manager: Arc::new(Mutex::new(None)),
            frame_count: Arc::new(RwLock::new(0)),
            frames_sent: Arc::new(AtomicU64::new(0)),
            frames_dropped: Arc::new(AtomicU64::new(0)),
            last_frame_time: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// Initialize multi-device manager for sending frames to WLED
    pub async fn init_ddp_client(&self) -> Result<()> {
        // Always reload config from disk to get fresh device config
        let config = BandwidthConfig::load()?;

        // Convert config to multi-device format
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
        *self.multi_device_manager.lock().unwrap() = Some(manager);
        Ok(())
    }

    /// Reinitialize multi-device manager (for when device config changes)
    pub async fn reinit_ddp_client_if_needed(&self) -> Result<bool> {
        // Check if device config has changed
        let config = BandwidthConfig::load()?;
        let new_devices = config.wled_devices.clone();

        let old_devices = {
            let cached_config = self.config.read().await;
            cached_config.wled_devices.clone()
        };

        // Simple comparison - reinit if anything changed
        let devices_changed = new_devices.len() != old_devices.len() ||
            new_devices.iter().zip(old_devices.iter()).any(|(new, old)| {
                new.ip != old.ip ||
                new.led_offset != old.led_offset ||
                new.led_count != old.led_count ||
                new.enabled != old.enabled
            });

        if devices_changed {
            // Device config changed, reinitialize
            self.init_ddp_client().await?;

            // Update cached config
            let mut cached_config = self.config.write().await;
            *cached_config = config;

            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Handle WebSocket connection for webcam streaming
pub async fn handle_webcam_ws(
    mut socket: WebSocket,
    state: Arc<WebcamState>,
) {
    // Initialize multi-device manager if not already done
    if state.multi_device_manager.lock().unwrap().is_none() {
        if let Err(e) = state.init_ddp_client().await {
            let _ = socket.send(Message::Text(format!("Error: {}", e))).await;
            let _ = socket.close().await;
            return;
        }
    }

    // Send initial config to client
    let config = state.config.read().await;
    let init_msg = serde_json::json!({
        "type": "config",
        "width": config.webcam_frame_width,
        "height": config.webcam_frame_height,
        "targetFps": config.webcam_target_fps,
    });
    drop(config);

    if socket.send(Message::Text(init_msg.to_string())).await.is_err() {
        return;
    }

    // Process incoming frames
    while let Some(msg) = socket.next().await {
        match msg {
            Ok(Message::Binary(data)) => {
                // Received raw RGBA frame data
                if let Err(_e) = process_frame(&state, data).await {
                    // Silently continue on errors to avoid TUI interference
                }
            }
            Ok(Message::Text(text)) => {
                // Handle text messages (e.g., stats requests, config updates)
                let _ = handle_text_message(&mut socket, &state, text).await;
            }
            Ok(Message::Close(_)) => {
                break;
            }
            Ok(Message::Ping(data)) => {
                let _ = socket.send(Message::Pong(data)).await;
            }
            Ok(_) => {}
            Err(_e) => {
                break;
            }
        }
    }
}

/// Process incoming RGBA frame and send to WLED via DDP
async fn process_frame(state: &WebcamState, data: Vec<u8>) -> Result<()> {
    // Update total frame count first (counts all received frames)
    let current_count = {
        let mut count = state.frame_count.write().await;
        *count += 1;
        *count
    };

    // Reload config from disk to pick up all changes (brightness, FPS, WLED IP, etc)
    let config = BandwidthConfig::load().unwrap_or_else(|_| {
        // Fallback to cached config if load fails
        let cfg = state.config.blocking_read();
        cfg.clone()
    });

    // Check if WLED IP changed and reinitialize DDP client if needed
    // Do this check every 60 frames to avoid excessive overhead
    if current_count % 60 == 0 {
        let _ = state.reinit_ddp_client_if_needed().await;
    }

    let target_width = config.webcam_frame_width;
    let target_height = config.webcam_frame_height;
    let brightness = config.webcam_brightness;
    let global_brightness = config.global_brightness;
    let target_fps = config.webcam_target_fps;
    let ddp_delay_ms = config.ddp_delay_ms;

    // FPS rate limiting - check if enough time has elapsed
    let frame_interval = std::time::Duration::from_secs_f64(1.0 / target_fps);
    let should_process = {
        let mut last_time = state.last_frame_time.lock().unwrap();
        let elapsed = last_time.elapsed();

        if elapsed < frame_interval {
            false // Too soon
        } else {
            *last_time = Instant::now();
            true
        }
    }; // MutexGuard dropped here

    if !should_process {
        // Too soon, drop this frame (FPS limiting)
        state.frames_dropped.fetch_add(1, Ordering::SeqCst);
        return Ok(());
    }

    // Client sends frames at exact configured dimensions
    let input_width = target_width as u32;
    let input_height = target_height as u32;
    let expected_size = (input_width * input_height * 4) as usize;

    if data.len() != expected_size {
        anyhow::bail!(
            "Invalid frame size: got {} bytes, expected {}x{} RGBA ({} bytes)",
            data.len(),
            input_width,
            input_height,
            expected_size
        );
    }

    // Apply DDP delay if configured
    if ddp_delay_ms > 0.0 {
        tokio::time::sleep(std::time::Duration::from_millis(ddp_delay_ms as u64)).await;
    }

    // Parse RGBA image and convert inline (no spawn_blocking for low latency)
    let img: RgbaImage = match ImageBuffer::from_raw(input_width, input_height, data) {
        Some(img) => img,
        None => {
            anyhow::bail!("Failed to parse RGBA image");
        }
    };

    // Convert to RGB with brightness adjustment (inline, no blocking task)
    // Note: Browser canvas typically sends BGRA, so we swap R and B channels
    let mut rgb_data = Vec::with_capacity((input_width * input_height * 3) as usize);
    for pixel in img.pixels() {
        let b = (pixel[0] as f64 * brightness).min(255.0) as u8;  // B from pixel[0]
        let g = (pixel[1] as f64 * brightness).min(255.0) as u8;  // G from pixel[1]
        let r = (pixel[2] as f64 * brightness).min(255.0) as u8;  // R from pixel[2]

        rgb_data.push(r); // R first
        rgb_data.push(g); // G second
        rgb_data.push(b); // B third
    }

    // Send to WLED via multi-device manager with global brightness
    if let Ok(mut manager_guard) = state.multi_device_manager.lock() {
        if let Some(manager) = manager_guard.as_mut() {
            if manager.send_frame_with_brightness(&rgb_data, Some(global_brightness)).is_ok() {
                state.frames_sent.fetch_add(1, Ordering::SeqCst);
            } else {
                state.frames_dropped.fetch_add(1, Ordering::SeqCst);
            }
        } else {
            state.frames_dropped.fetch_add(1, Ordering::SeqCst);
        }
    } else {
        state.frames_dropped.fetch_add(1, Ordering::SeqCst);
    }

    Ok(())
}

/// Handle text messages from client
async fn handle_text_message(
    socket: &mut WebSocket,
    state: &WebcamState,
    text: String,
) -> Result<()> {
    // Parse JSON message
    let msg: serde_json::Value = serde_json::from_str(&text)?;

    match msg["type"].as_str() {
        Some("stats") => {
            // Return current stats
            let frame_count = *state.frame_count.read().await;
            let response = serde_json::json!({
                "type": "stats",
                "frameCount": frame_count,
            });
            socket.send(Message::Text(response.to_string())).await?;
        }
        Some("ping") => {
            // Simple ping/pong
            socket.send(Message::Text(r#"{"type":"pong"}"#.to_string())).await?;
        }
        _ => {
            // Unknown message type
            eprintln!("Unknown message type: {:?}", msg["type"]);
        }
    }

    Ok(())
}
