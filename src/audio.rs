// Audio Module - Real-time audio visualization using CQT
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::Device;
use std::collections::HashSet;

/// List all available audio devices (both input and output)
/// Returns a vector of (device_name, is_output) tuples
pub fn list_audio_devices() -> Result<Vec<(String, bool)>> {
    let host = cpal::default_host();
    let mut device_list = Vec::new();
    let mut seen_devices = HashSet::new();

    // WORKAROUND: Get default devices first to avoid hanging on some macOS systems
    if let Some(device) = host.default_input_device() {
        if let Ok(name) = device.name() {
            device_list.push((format!("{} [INPUT] (default)", name), false));
            seen_devices.insert(name);
        }
    }

    if let Some(device) = host.default_output_device() {
        if let Ok(name) = device.name() {
            device_list.push((format!("{} [OUTPUT] (default)", name), true));
            seen_devices.insert(name);
        }
    }

    // Get input devices (skip if already added as default)
    if let Ok(devices) = host.input_devices() {
        for device in devices {
            if let Ok(name) = device.name() {
                if !seen_devices.contains(&name) {
                    device_list.push((format!("{} [INPUT]", name), false));
                    seen_devices.insert(name);
                }
            }
        }
    }

    // Get output devices (skip if already added as default)
    if let Ok(devices) = host.output_devices() {
        for device in devices {
            if let Ok(name) = device.name() {
                if !seen_devices.contains(&name) {
                    device_list.push((format!("{} [OUTPUT/LOOPBACK]", name), true));
                    seen_devices.insert(name);
                }
            }
        }
    }

    if device_list.is_empty() {
        return Err(anyhow!("No audio devices found"));
    }

    Ok(device_list)
}

/// Find an audio device by name (checks both input and output devices)
/// Case-insensitive substring match
pub fn find_audio_device(device_name: &str) -> Result<Device> {
    let host = cpal::default_host();

    // Clean up the device name (remove [INPUT], [OUTPUT/LOOPBACK], and (default) tags)
    let clean_name = device_name
        .replace(" [INPUT] (default)", "")
        .replace(" [OUTPUT] (default)", "")
        .replace(" [OUTPUT/LOOPBACK] (default)", "")
        .replace(" [INPUT]", "")
        .replace(" [OUTPUT/LOOPBACK]", "")
        .replace(" (default)", "");

    // WORKAROUND: Check default devices first to avoid hanging on some macOS systems
    if let Some(device) = host.default_input_device() {
        if let Ok(name) = device.name() {
            if name.to_lowercase().contains(&clean_name.to_lowercase()) {
                return Ok(device);
            }
        }
    }

    if let Some(device) = host.default_output_device() {
        if let Ok(name) = device.name() {
            if name.to_lowercase().contains(&clean_name.to_lowercase()) {
                return Ok(device);
            }
        }
    }

    // Try to find in input devices
    if let Ok(devices) = host.input_devices() {
        for device in devices {
            if let Ok(name) = device.name() {
                if name.to_lowercase().contains(&clean_name.to_lowercase()) {
                    return Ok(device);
                }
            }
        }
    }

    // Try to find in output devices
    if let Ok(devices) = host.output_devices() {
        for device in devices {
            if let Ok(name) = device.name() {
                if name.to_lowercase().contains(&clean_name.to_lowercase()) {
                    return Ok(device);
                }
            }
        }
    }

    // If not found by name, try default device if that's what was requested
    if device_name.contains("default") || clean_name.to_lowercase().contains("default") {
        if let Some(device) = host.default_input_device() {
            return Ok(device);
        }
        if let Some(device) = host.default_output_device() {
            return Ok(device);
        }
        return Err(anyhow!("No default audio device available"));
    }

    Err(anyhow!("Audio device '{}' not found", device_name))
}

