use anyhow::{anyhow, Result};
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ddp_rs::connection::DDPConnection;
use ddp_rs::protocol::{PixelConfig, ID};

// WLED DDP timeout is ~1 second, so send keepalive every 500ms to be safe
const KEEPALIVE_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub struct WLEDDevice {
    pub ip: String,
    pub led_offset: usize,
    pub led_count: usize,
    pub enabled: bool,
}

pub struct MultiDeviceConfig {
    pub devices: Vec<WLEDDevice>,
    pub send_parallel: bool,
    pub fail_fast: bool,
}

impl MultiDeviceConfig {
    pub fn validate(&self) -> Result<()> {
        if self.devices.is_empty() {
            return Err(anyhow!("No devices configured"));
        }

        // Check for overlapping LED ranges
        for i in 0..self.devices.len() {
            if !self.devices[i].enabled {
                continue;
            }
            for j in (i + 1)..self.devices.len() {
                if !self.devices[j].enabled {
                    continue;
                }
                let dev1_start = self.devices[i].led_offset;
                let dev1_end = dev1_start + self.devices[i].led_count;
                let dev2_start = self.devices[j].led_offset;
                let dev2_end = dev2_start + self.devices[j].led_count;

                if dev1_start < dev2_end && dev1_end > dev2_start {
                    return Err(anyhow!(
                        "Overlapping LED ranges: Device {} ({}-{}) overlaps with Device {} ({}-{})",
                        self.devices[i].ip,
                        dev1_start,
                        dev1_end - 1,
                        self.devices[j].ip,
                        dev2_start,
                        dev2_end - 1
                    ));
                }
            }
        }

        Ok(())
    }
}

struct DeviceConnection {
    device_config: WLEDDevice,
    ddp_connection: Arc<Mutex<DDPConnection>>,
    last_send_time: Arc<Mutex<Instant>>,
}

impl DeviceConnection {
    fn new(device_config: WLEDDevice) -> Result<Self> {
        let dest_addr = format!("{}:4048", device_config.ip);
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        let ddp_connection = DDPConnection::try_new(&dest_addr, PixelConfig::default(), ID::Default, socket)?;

        Ok(DeviceConnection {
            device_config,
            ddp_connection: Arc::new(Mutex::new(ddp_connection)),
            last_send_time: Arc::new(Mutex::new(Instant::now())),
        })
    }
}

pub struct MultiDeviceManager {
    devices: Vec<DeviceConnection>,
    config: MultiDeviceConfig,
}

impl MultiDeviceManager {
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    pub fn new(config: MultiDeviceConfig) -> Result<Self> {
        config.validate()?;

        let mut devices = Vec::new();
        for device_config in &config.devices {
            if device_config.enabled {
                match DeviceConnection::new(device_config.clone()) {
                    Ok(conn) => devices.push(conn),
                    Err(e) => {
                        eprintln!("Warning: Failed to connect to {}: {}", device_config.ip, e);
                    }
                }
            }
        }

        if devices.is_empty() {
            return Err(anyhow!("No devices connected successfully"));
        }

        Ok(MultiDeviceManager { devices, config })
    }

    pub fn send_frame(&mut self, frame: &[u8]) -> Result<Vec<String>> {
        self.send_frame_with_brightness(frame, None)
    }

    /// Send frame with optional brightness override
    /// brightness: None = use frame as-is, Some(0.0-1.0) = apply brightness multiplier
    pub fn send_frame_with_brightness(&mut self, frame: &[u8], brightness: Option<f64>) -> Result<Vec<String>> {
        // Frame size should be divisible by 3 (RGB)
        if frame.len() % 3 != 0 {
            return Err(anyhow!(
                "Frame size must be divisible by 3 (RGB), got {} bytes",
                frame.len()
            ));
        }

        // Apply brightness if specified
        let frame_to_send: Vec<u8>;
        let frame_ref = if let Some(brightness) = brightness {
            if brightness < 1.0 {
                // Apply brightness multiplier to all RGB values
                frame_to_send = frame.iter().map(|&val| {
                    (val as f64 * brightness).round() as u8
                }).collect();
                &frame_to_send
            } else {
                frame  // No brightness adjustment needed
            }
        } else {
            frame  // No brightness specified
        };

        if self.config.send_parallel {
            self.send_parallel(frame_ref)
        } else {
            self.send_sequential(frame_ref)
        }
    }

    fn send_parallel(&mut self, frame: &[u8]) -> Result<Vec<String>> {
        use std::thread;

        let errors = Arc::new(Mutex::new(Vec::new()));
        let frame_arc = Arc::new(frame.to_vec());

        thread::scope(|s| {
            for device in &self.devices {
                let device_ip = device.device_config.ip.clone();
                let byte_offset = device.device_config.led_offset * 3;
                let byte_count = device.device_config.led_count * 3;
                let frame_clone = Arc::clone(&frame_arc);
                let errors_clone = Arc::clone(&errors);
                let conn_clone = Arc::clone(&device.ddp_connection);

                let last_send_clone = Arc::clone(&device.last_send_time);

                s.spawn(move || {
                    // Validate range
                    if byte_offset + byte_count > frame_clone.len() {
                        let err = format!(
                            "Device {} range exceeds frame size: offset={} count={} (device wants LEDs {}-{}, frame has {} LEDs)",
                            device_ip,
                            byte_offset / 3,
                            byte_count / 3,
                            byte_offset / 3,
                            (byte_offset + byte_count) / 3 - 1,
                            frame_clone.len() / 3
                        );
                        eprintln!("{}", err);
                        errors_clone.lock().unwrap().push(err);
                        return;
                    }

                    // Extract device frame slice
                    let device_frame = &frame_clone[byte_offset..byte_offset + byte_count];

                    // Check if we need to send a keepalive (time since last send)
                    let needs_keepalive = {
                        if let Ok(last_send) = last_send_clone.lock() {
                            last_send.elapsed() >= KEEPALIVE_INTERVAL
                        } else {
                            false
                        }
                    };

                    // Skip sending if all zeros AND we don't need a keepalive
                    let all_zeros = device_frame.iter().all(|&b| b == 0);
                    if all_zeros && !needs_keepalive {
                        return;
                    }

                    // Send using DDPConnection - SAME AS SEQUENTIAL MODE
                    if let Ok(mut conn) = conn_clone.lock() {
                        if let Err(e) = conn.write(device_frame) {
                            let err = format!("Failed to send to {}: {}", device_ip, e);
                            eprintln!("{}", err);
                            errors_clone.lock().unwrap().push(err);
                        } else {
                            // Update last send time on successful send
                            if let Ok(mut last_send) = last_send_clone.lock() {
                                *last_send = Instant::now();
                            }
                        }
                    } else {
                        let err = format!("Failed to acquire lock for device {}", device_ip);
                        eprintln!("{}", err);
                        errors_clone.lock().unwrap().push(err);
                    }
                });
            }
        });

        let errors = Arc::try_unwrap(errors).unwrap().into_inner().unwrap();
        if errors.is_empty() {
            Ok(vec![])
        } else {
            Ok(errors)
        }
    }

    fn send_sequential(&mut self, frame: &[u8]) -> Result<Vec<String>> {
        let mut errors = Vec::new();

        for device in &mut self.devices {
            let device_ip = device.device_config.ip.clone();
            let byte_offset = device.device_config.led_offset * 3;
            let byte_count = device.device_config.led_count * 3;

            if byte_offset + byte_count > frame.len() {
                let err = format!(
                    "Device {} range exceeds frame size: offset={} count={} total_needed={} frame_size={} (device wants LEDs {}-{}, frame has {} LEDs)",
                    device_ip,
                    device.device_config.led_offset,
                    device.device_config.led_count,
                    byte_offset + byte_count,
                    frame.len(),
                    device.device_config.led_offset,
                    device.device_config.led_offset + device.device_config.led_count - 1,
                    frame.len() / 3
                );
                eprintln!("{}", err);
                errors.push(err);
                if self.config.fail_fast {
                    return Err(anyhow!("Frame range error"));
                }
                continue;
            }

            // Extract slice for this device
            let device_frame = &frame[byte_offset..byte_offset + byte_count];

            // Check if we need to send a keepalive (time since last send)
            let needs_keepalive = {
                if let Ok(last_send) = device.last_send_time.lock() {
                    last_send.elapsed() >= KEEPALIVE_INTERVAL
                } else {
                    false
                }
            };

            // Skip sending if all zeros AND we don't need a keepalive
            let all_zeros = device_frame.iter().all(|&b| b == 0);
            if all_zeros && !needs_keepalive {
                continue;
            }

            // Send using DDPConnection - SAME AS SINGLE DEVICE MODE
            if let Ok(mut conn) = device.ddp_connection.lock() {
                if let Err(e) = conn.write(device_frame) {
                    let err = format!("Failed to send to {}: {}", device_ip, e);
                    eprintln!("{}", err);
                    errors.push(err);
                    if self.config.fail_fast {
                        return Err(anyhow!("Failed to send to device"));
                    }
                } else {
                    // Update last send time on successful send
                    if let Ok(mut last_send) = device.last_send_time.lock() {
                        *last_send = Instant::now();
                    }
                }
            } else {
                let err = format!("Failed to acquire lock for device {}", device_ip);
                eprintln!("{}", err);
                errors.push(err);
                if self.config.fail_fast {
                    return Err(anyhow!("Failed to acquire device lock"));
                }
            }
        }

        if errors.is_empty() {
            Ok(vec![])
        } else {
            Ok(errors)
        }
    }
}
