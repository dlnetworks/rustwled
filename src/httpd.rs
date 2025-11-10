// HTTP Server Module - Web UI and API endpoints
use anyhow::{Context, Result};
use async_stream::stream;
use axum::{
    extract::{ConnectInfo, Json, Query, Request, State, ws::WebSocketUpgrade},
    http::{StatusCode, header::{AUTHORIZATION, WWW_AUTHENTICATE}},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response, sse::{Event as SseEvent, Sse}},
    routing::{get, post},
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use base64::{Engine as _, engine::general_purpose};
use futures::stream::Stream;
use rustls_pemfile::{certs, pkcs8_private_keys};
use serde::Deserialize;
use std::collections::HashMap;
use std::convert::Infallible;
use std::io::BufReader;
use std::net::SocketAddr;
use std::process::{Command as StdCommand, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::broadcast;

// Import from other modules
use crate::audio;
use crate::cert;
use crate::gradients;
use crate::webcam;
use crate::config::BandwidthConfig;

const WEB_UI_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>RustWLED Configuration</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, sans-serif;
            background: #1a1a1a;
            color: #e0e0e0;
            padding: 20px;
            padding-top: 30px;
            line-height: 1.6;
            overscroll-behavior: none; /* Prevent pull-to-refresh on mobile */
        }
        .container {
            max-width: 900px;
            margin: 0 auto;
        }
        h1 {
            color: #00aaff;
            margin-bottom: 30px;
            font-size: 2em;
        }
        .config-grid {
            display: grid;
            gap: 15px;
        }
        .config-item {
            margin-bottom: 12px;
        }
        .config-item label {
            display: block;
            color: #b0b0b0;
            margin-bottom: 8px;
            font-size: 0.9em;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }
        .input-group {
            display: flex;
            gap: 10px;
            align-items: center;
        }
        input[type="text"], input[type="number"], select, textarea {
            flex: 1;
            background: #1a1a1a;
            border: 1px solid #505050;
            color: #e0e0e0;
            padding: 10px 12px;
            border-radius: 4px;
            font-size: 1em;
        }
        input.invalid {
            border: 2px solid #ff4444;
            background: #3a1a1a;
        }
        button:disabled {
            background: #555555;
            cursor: not-allowed;
            opacity: 0.5;
        }
        input[type="checkbox"] {
            width: 20px;
            height: 20px;
            cursor: pointer;
        }
        input[type="range"] {
            flex: 1;
            cursor: pointer;
            touch-action: none; /* Prevent pull-to-refresh on mobile */
        }
        .range-value {
            min-width: 80px;
            text-align: center;
            color: #00aaff;
            font-weight: 600;
        }
        button {
            background: #00aaff;
            color: white;
            border: none;
            padding: 10px 20px;
            border-radius: 4px;
            cursor: pointer;
            font-size: 0.9em;
            font-weight: 600;
            transition: background 0.2s;
        }
        button:hover {
            background: #0088cc;
        }
        button:active {
            background: #006699;
        }
        .message {
            position: fixed;
            top: 20px;
            right: 20px;
            padding: 15px 20px;
            border-radius: 4px;
            font-weight: 500;
            opacity: 0;
            transition: opacity 0.3s;
            z-index: 1000;
        }
        .message.show {
            opacity: 1;
        }
        .message.success {
            background: #2d5016;
            border: 1px solid #4a8028;
            color: #a3d977;
        }
        .message.error {
            background: #5a1a1a;
            border: 1px solid #902020;
            color: #ff9090;
        }
        .help-text {
            font-size: 0.85em;
            color: #808080;
            margin-top: 4px;
        }
        .section {
            background: #2a2a2a;
            border: 1px solid #404040;
            border-radius: 8px;
            padding: 20px;
            margin-bottom: 20px;
        }
        .sticky-controls {
            position: sticky;
            top: 0;
            z-index: 100;
            background: #1a1a1a;
            padding: 12px 0;
            margin-bottom: 20px;
            box-shadow: 0 4px 6px rgba(0,0,0,0.3);
        }
        .sticky-controls .section {
            margin-bottom: 0;
        }
        .header-bar {
            display: flex;
            align-items: center;
            gap: 30px;
            flex-wrap: wrap;
        }
        .header-bar .field {
            margin: 0;
            display: flex;
            align-items: center;
            gap: 10px;
        }
        .header-bar .field label {
            margin: 0;
            white-space: nowrap;
        }
        .section-header {
            color: #00aaff;
            font-size: 1.3em;
            font-weight: 600;
            margin-bottom: 20px;
            padding-bottom: 10px;
            border-bottom: 2px solid #404040;
        }
        .testing-grid {
            display: flex;
            justify-content: center;
            gap: 40px;
            align-items: center;
        }
        .testing-item {
            display: flex;
            align-items: center;
            gap: 10px;
        }
        .testing-item label {
            margin: 0;
            cursor: pointer;
        }
    </style>
</head>
<body>
    <iframe id="wled-liveview" style="position: fixed; top: 0; left: 0; width: 100%; height: 10px; border: none; overflow: hidden; display: block; margin: 0; padding: 0; z-index: 1000; background: #000; transition: top 0.3s ease;" scrolling="no" frameborder="0"></iframe>
    <div id="liveview-toggle" onclick="toggleLiveview()" style="position: fixed; top: 10px; left: 50%; transform: translateX(-50%); background: #333; color: #fff; padding: 3px 12px; border-radius: 0 0 6px 6px; cursor: pointer; font-size: 11px; z-index: 1001; user-select: none; box-shadow: 0 2px 4px rgba(0,0,0,0.3); transition: top 0.3s ease;">
        <span id="liveview-toggle-icon">▼</span> WLED Preview
    </div>
    <div class="container">
        <h1 id="page-title">LED Visualization Configuration</h1>
        <div class="sticky-controls">
            <div class="section">
                <div class="header-bar">
                    <div class="field">
                        <label for="mode">Mode:</label>
                        <select id="mode" onchange="saveField('mode', 'select')" style="font-weight: bold; font-size: 1.05em;">
                            <option value="bandwidth">bandwidth</option>
                            <option value="midi">midi</option>
                            <option value="live">live audio</option>
                            <option value="relay">relay</option>
                            <option value="webcam">webcam</option>
                            <option value="tron">tron game</option>
                            <option value="geometry">geometry</option>
                            <option value="sand">falling sand</option>
                        </select>
                        <span id="mode-status" style="font-weight: bold; color: #00aaff; margin-left: 8px;"></span>
                    </div>
                    <div class="field">
                        <label for="global-brightness">Brightness:</label>
                        <input type="range" id="global-brightness" min="0" max="100" step="1" value="100"
                               oninput="updateBrightnessDisplay(this.value)"
                               onchange="saveBrightness(this.value)"
                               style="width: 150px;">
                        <span id="global-brightness-value" class="range-value">100%</span>
                    </div>
                </div>
            </div>
        </div>
        <div id="config-container"></div>

        <!-- Danger Zone -->
        <div class="section" style="margin-top: 40px; border: 2px solid #a03030; background: #2a1a1a;">
            <div class="section-header" style="color: #ff6666;">⚠️ Danger Zone</div>
            <div style="text-align: center;">
                <p style="color: #b0b0b0; margin-bottom: 15px;">This will immediately terminate the entire application including all visualization modes.</p>
                <button onclick="shutdownApp()" style="background: #cc3333; padding: 12px 30px; font-size: 1em;">
                    🛑 Shutdown Application
                </button>
            </div>
        </div>
    </div>
    <div id="message" class="message" style="display: none;"></div>

    <script>
        // IMPORTANT: When adding new config fields to BandwidthConfig struct:
        // 1. Add the field to this fieldSections array below
        // 2. Add the field to the API handler match statement in update_config()
        // The validateWebUIFields() function will automatically check on page load
        // and log a console warning if any fields are missing from the Web UI
        // (it fetches /api/config/fields which returns the full config as JSON)

        const fieldSections = [
            // Global settings - appear in all modes
            {
                title: 'WLED Device Configuration',
                modes: ['bandwidth', 'midi', 'live', 'relay', 'webcam', 'tron', 'geometry'],
                isInfo: true,
                info: function() {
                    const devices = config.wled_devices || [];
                    const multiEnabled = config.multi_device_enabled || false;
                    return `
                        <div style="font-size: 14px; line-height: 1.6;">
                            <p style="margin-top: 0; color: #ccc;">
                                ${devices.length === 0 ? 'No devices configured. Click "Add Device" to get started.' :
                                  devices.length === 1 ? 'Single device mode. Add more devices below to enable multi-controller setup.' :
                                  `Multi-device mode: ${devices.filter(d => d.enabled).length} of ${devices.length} devices active`}
                            </p>

                            <div id="device-list" style="margin: 16px 0;">
                                ${devices.map((device, idx) => `
                                    <div class="device-card" style="background: #2a2a2a; padding: 16px; border-radius: 8px; margin-bottom: 12px; border-left: 4px solid ${device.enabled ? '#4caf50' : '#888'};">
                                        <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px;">
                                            <h4 style="margin: 0; color: ${device.enabled ? '#4caf50' : '#888'};">
                                                ${idx === 0 ? '🎯 Primary Device' : `Device ${idx + 1}`}: ${device.ip}
                                            </h4>
                                            <div style="display: flex; gap: 8px;">
                                                <button onclick="toggleDevice(${idx})" style="padding: 6px 12px; background: ${device.enabled ? '#ff9800' : '#4caf50'}; border: none; color: white; border-radius: 4px; cursor: pointer; font-size: 12px;">
                                                    ${device.enabled ? 'Disable' : 'Enable'}
                                                </button>
                                                ${idx > 0 ? `<button onclick="removeDevice(${idx})" style="padding: 6px 12px; background: #f44336; border: none; color: white; border-radius: 4px; cursor: pointer; font-size: 12px;">Remove</button>` : ''}
                                            </div>
                                        </div>
                                        <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 12px;">
                                            <div>
                                                <label style="display: block; font-size: 12px; color: #888; margin-bottom: 4px;">IP Address</label>
                                                <input type="text" value="${device.ip}" onchange="updateDevice(${idx}, 'ip', this.value)" style="width: 100%; padding: 8px; background: #1a1a1a; border: 1px solid #444; color: white; border-radius: 4px; font-size: 13px;">
                                            </div>
                                            <div>
                                                <label style="display: block; font-size: 12px; color: #888; margin-bottom: 4px;">LED Offset</label>
                                                <input type="number" value="${device.led_offset}" onchange="updateDevice(${idx}, 'led_offset', parseInt(this.value))" style="width: 100%; padding: 8px; background: #1a1a1a; border: 1px solid #444; color: white; border-radius: 4px; font-size: 13px;">
                                            </div>
                                            <div>
                                                <label style="display: block; font-size: 12px; color: #888; margin-bottom: 4px;">LED Count</label>
                                                <input type="number" value="${device.led_count}" onchange="updateDevice(${idx}, 'led_count', parseInt(this.value))" style="width: 100%; padding: 8px; background: #1a1a1a; border: 1px solid #444; color: white; border-radius: 4px; font-size: 13px;">
                                            </div>
                                        </div>
                                        <p style="font-size: 11px; color: #666; margin: 8px 0 0 0;">Range: LEDs ${device.led_offset} to ${device.led_offset + device.led_count - 1}</p>
                                    </div>
                                `).join('')}
                            </div>

                            <button onclick="addDevice()" style="width: 100%; padding: 12px; background: #1976d2; border: none; color: white; border-radius: 4px; cursor: pointer; font-size: 14px; font-weight: bold;">
                                + Add Device
                            </button>

                            ${devices.length > 1 ? `
                                <div style="margin-top: 16px; padding: 12px; background: #2d2d2d; border-radius: 4px;">
                                    <h4 style="margin: 0 0 12px 0; font-size: 13px; color: #ccc;">Multi-Device Options</h4>
                                    <div style="display: flex; flex-direction: column; gap: 8px;">
                                        <label style="display: flex; align-items: center; gap: 8px; cursor: pointer;">
                                            <input type="checkbox" id="multi_device_send_parallel" ${multiEnabled ? 'checked' : ''} onchange="updateConfigField('multi_device_enabled', this.checked)" style="cursor: pointer;">
                                            <span style="font-size: 13px;">Enable Multi-Device Mode (send to all devices)</span>
                                        </label>
                                        <label style="display: flex; align-items: center; gap: 8px; cursor: pointer; margin-left: 24px;">
                                            <input type="checkbox" id="multi_device_send_parallel_cb" ${config.multi_device_send_parallel ? 'checked' : ''} onchange="updateConfigField('multi_device_send_parallel', this.checked)" style="cursor: pointer;">
                                            <span style="font-size: 12px; color: #888;">Send in parallel (faster but may cause issues)</span>
                                        </label>
                                        <label style="display: flex; align-items: center; gap: 8px; cursor: pointer; margin-left: 24px;">
                                            <input type="checkbox" id="multi_device_fail_fast_cb" ${config.multi_device_fail_fast ? 'checked' : ''} onchange="updateConfigField('multi_device_fail_fast', this.checked)" style="cursor: pointer;">
                                            <span style="font-size: 12px; color: #888;">Fail fast (stop all if one fails)</span>
                                        </label>
                                    </div>
                                </div>
                            ` : ''}

                            <div style="margin-top: 16px; padding: 12px; background: #2d2d2d; border-radius: 4px; font-size: 12px; color: #888;">
                                <strong>💡 Tips:</strong><br>
                                • <strong>Single Device:</strong> Only the primary device is used<br>
                                • <strong>Multiple Devices:</strong> Enable multi-device mode to send to all<br>
                                • LED Offset: Starting position in the unified frame (0-based)<br>
                                • LED Count: Number of LEDs this controller manages<br>
                                • Virtual Offset: DDP channel offset on the device (usually 0)<br>
                                • Devices should not have overlapping LED ranges<br>
                                • All changes apply immediately without restart
                            </div>
                        </div>
                    `;
                }
            },
            {
                title: 'Performance',
                modes: ['bandwidth', 'midi', 'live', 'geometry'],
                fields: [
                    { name: 'fps', label: 'Frame Rate (FPS)', type: 'number', step: '1', help: 'Rendering frame rate. Try 30, 60, 120, or 144' },
                ]
            },
            {
                title: 'Audio/MIDI Timing',
                modes: ['midi', 'live'],
                fields: [
                    { name: 'ddp_delay_ms', label: 'DDP Packet Delay (ms)', type: 'number', step: '0.1', help: 'Delay in milliseconds before sending each DDP packet. Use to fine-tune audio/LED synchronization.' },
                    { name: 'attack_ms', label: 'Attack Time (ms)', type: 'number', step: '1', help: 'Time in milliseconds for LEDs to fade in' },
                    { name: 'decay_ms', label: 'Decay Time (ms)', type: 'number', step: '1', help: 'Time in milliseconds for LEDs to fade out' },
                ]
            },
            {
                title: 'Visualization Settings',
                modes: ['bandwidth', 'live', 'geometry'],
                fields: [
                    { name: 'color', label: 'Default Color', type: 'gradient', help: 'Select a gradient preset or enter custom hex colors' },
                    { name: 'tx_color', label: 'TX (Upload) / Right Channel Color', type: 'gradient', help: 'Overrides default color for TX/Right. Leave empty to use default.', allowNone: true, visibleWhen: (config) => config.mode !== 'geometry' },
                    { name: 'rx_color', label: 'RX (Download) / Left Channel Color', type: 'gradient', help: 'Overrides default color for RX/Left. Leave empty to use default.', allowNone: true, visibleWhen: (config) => config.mode !== 'geometry' },
                    { name: 'use_gradient', label: 'Use Gradient Blending', type: 'checkbox', help: 'Smooth gradients vs hard color segments' },
                    { name: 'intensity_colors', label: 'Intensity Colors Mode', type: 'checkbox', help: 'All LEDs show the same color that changes based on level/utilization. 0% = first color, 100% = last color in gradient.', visibleWhen: (config) => config.use_gradient && (config.mode === 'bandwidth' || config.vu) },
                    { name: 'interpolation', label: 'Gradient Interpolation', type: 'select', options: ['linear', 'basis', 'catmullrom'], help: 'Gradient interpolation algorithm', visibleWhen: (config) => !config.intensity_colors && config.mode !== 'geometry' },
                    { name: 'animation_speed', label: 'Animation Speed', type: 'number', step: '0.1', help: 'Speed of gradient animation (0 = disabled)', visibleWhen: (config) => !config.intensity_colors && config.mode !== 'geometry' },
                    { name: 'scale_animation_speed', label: 'Scale Speed with Bandwidth/Audio Level', type: 'checkbox', help: 'Animation speed scales with bandwidth utilization or audio level', visibleWhen: (config) => !config.intensity_colors && config.mode !== 'geometry' && (config.mode !== 'live' || config.vu) },
                    { name: 'peak_direction_toggle', label: 'Toggle Direction on New Peak', type: 'checkbox', help: 'Change animation direction each time a new peak is held (VU mode only)', visibleWhen: (config) => config.vu && config.peak_hold && !config.intensity_colors && config.mode !== 'geometry' },
                    { name: 'tx_animation_direction', label: 'TX (Upload) / Right Channel Direction', type: 'radio', options: ['left', 'right'], help: 'Direction TX/Right animation moves', visibleWhen: (config) => !config.intensity_colors && !config.peak_direction_toggle && config.mode !== 'geometry' },
                    { name: 'rx_animation_direction', label: 'RX (Download) / Left Channel Direction', type: 'radio', options: ['left', 'right'], help: 'Direction RX/Left animation moves', visibleWhen: (config) => !config.intensity_colors && !config.peak_direction_toggle && config.mode !== 'geometry' },
                    { name: 'interpolation_time_ms', label: 'Interpolation Time (ms)', type: 'number', step: '10', help: 'Time in milliseconds to smoothly transition between bandwidth readings', visibleWhen: (config) => config.mode === 'bandwidth' },
                    { name: 'enable_interpolation', label: 'Enable Interpolation', type: 'checkbox', help: 'Smooth bandwidth transitions (disable for instant response)', visibleWhen: (config) => config.mode === 'bandwidth' },
                ]
            },
            {
                title: 'HTTP Server',
                modes: ['bandwidth', 'midi', 'live', 'relay', 'geometry'],
                fields: [
                    { name: 'httpd_ip', label: 'HTTP Server IP', type: 'text', help: 'IP address to listen on. Also used for SSL certificate when HTTPS is enabled. Changes require restart.' },
                    { name: 'httpd_port', label: 'HTTP Server Port', type: 'number', step: '1', help: 'Port for HTTP server. Changes require restart.' },
                    { name: 'httpd_https_enabled', label: 'Enable HTTPS', type: 'checkbox', help: 'Enable HTTPS with self-signed certificates. Browser will show security warning (click "Proceed"). Requires restart.' },
                ]
            },
            // Bandwidth mode specific
            {
                title: 'Network Monitoring',
                modes: ['bandwidth'],
                isGroup: true,
                groupFields: [
                    { name: 'interface', label: 'Network Interface', type: 'network_interface', help: 'Select one or more network interfaces to monitor. If SSH host is configured, interfaces will be loaded from the remote host.' },
                    { name: 'ssh_host', label: 'SSH Host (Remote)', type: 'text', help: 'SSH host for remote monitoring (e.g., 192.168.1.100). Leave empty for local monitoring.' },
                    { name: 'ssh_user', label: 'SSH User', type: 'text', help: 'SSH username for remote monitoring. Leave empty to use current user.' },
                ],
                help: 'Changes apply dynamically without restart.'
            },
            {
                title: 'Bandwidth Settings',
                modes: ['bandwidth'],
                fields: [
                    { name: 'max_gbps', label: 'Max Bandwidth (Gbps)', type: 'number', step: '0.1', help: 'Maximum bandwidth in Gbps for visualization scaling' },
                    { name: 'log_scale', label: 'Use Logarithmic Scale', type: 'checkbox', help: 'Use logarithmic scaling for bandwidth visualization' },
                ]
            },
            {
                title: 'LED Layout',
                modes: ['bandwidth', 'live'],
                fields: [
                    { name: 'direction', label: 'Fill Direction', type: 'select', options: ['mirrored', 'opposing', 'left', 'right'], help: 'How LEDs fill across the strip (bandwidth/VU) or spectrum (live)' },
                    { name: 'swap', label: 'Swap TX/RX Halves', type: 'checkbox', help: 'Swap which half shows TX vs RX', visibleWhen: (config) => config.mode === 'bandwidth' },
                    { name: 'rx_split_percent', label: 'RX/TX LED Split', type: 'range', min: '0', max: '100', step: '1', help: 'Percentage of LEDs allocated to RX. TX gets the remainder. (50 = 50/50, 70 = 70/30)', visibleWhen: (config) => config.mode === 'bandwidth' },
                ]
            },
            {
                title: 'Strobe Effects',
                modes: ['bandwidth', 'live'],
                visibleWhen: (config) => config.mode !== 'live' || config.vu,  // Hide in live mode unless VU meter is enabled
                fields: [
                    { name: 'strobe_on_max', label: 'Strobe at Max/Clipping', type: 'checkbox', help: 'Flash when bandwidth exceeds maximum or audio clips (VU mode)' },
                    { name: 'strobe_rate_hz', label: 'Strobe Rate (Hz)', type: 'number', step: '0.1', help: 'Strobe frequency in Hz (flashes per second)' },
                    { name: 'strobe_duration_ms', label: 'Strobe Duration (ms)', type: 'number', step: '1', help: 'Duration of strobe effect in milliseconds' },
                    { name: 'strobe_color', label: 'Strobe Color (Hex)', type: 'text', help: 'Hex color to flash when at 100%+ utilization (default: FFFFFF white)' },
                ]
            },
            {
                title: 'Testing Mode',
                modes: ['bandwidth'],
                isTesting: true,
                help: 'Simulate bandwidth utilization for testing. Values above 100% trigger strobe effect.',
                fields: [
                    { name: 'test_tx', label: 'TX (Upload)', type: 'checkbox' },
                    { name: 'test_tx_percent', label: 'TX Utilization', type: 'range', min: '0', max: '101', step: '1' },
                    { name: 'test_rx', label: 'RX (Download)', type: 'checkbox' },
                    { name: 'test_rx_percent', label: 'RX Utilization', type: 'range', min: '0', max: '101', step: '1' },
                ]
            },
            // MIDI mode specific
            {
                title: 'MIDI Input',
                modes: ['midi'],
                fields: [
                    { name: 'midi_device', label: 'MIDI Device Name', type: 'text', help: 'Name of MIDI input device (default: "IAC Bus 1" on macOS)' },
                ]
            },
            {
                title: 'MIDI Visualization',
                modes: ['midi'],
                fields: [
                    { name: 'midi_gradient', label: 'Enable Gradient Blending', type: 'checkbox', help: 'Multiple notes blend together in a gradient' },
                    { name: 'midi_random_colors', label: 'Randomize Colors', type: 'checkbox', help: 'Shuffle the 12 primary colors randomly at mode start' },
                    { name: 'midi_velocity_colors', label: 'Velocity-Based Colors', type: 'checkbox', help: 'Map velocity to color spectrum instead of note' },
                    { name: 'midi_one_to_one', label: '1-to-1 LED Mapping', type: 'checkbox', help: 'Map 1 LED per note (centered at middle C)' },
                    { name: 'midi_channel_mode', label: 'MIDI Channel Mode', type: 'checkbox', help: 'Use MIDI channels to map notes to LEDs' },
                ]
            },
            // Live audio mode specific
            {
                title: 'Audio Settings',
                modes: ['live'],
                fields: [
                    { name: 'audio_device', label: 'Audio Device', type: 'audio_device', help: 'Select audio input device for live mode' },
                    { name: 'audio_gain', label: 'Audio Input Gain (%)', type: 'range', min: '-200', max: '200', step: '1', help: 'Adjust audio input gain. 0 = no change, +200 = triple amplitude, -200 = muted' },
                    { name: 'vu', label: 'VU Meter Mode', type: 'checkbox', help: 'Enable VU meter mode (splits LEDs for left/right channels)' },
                    { name: 'peak_hold', label: 'Enable Peak Hold', type: 'checkbox', help: 'Show a single LED at the peak level that holds for a duration', visibleWhen: (config) => config.vu },
                    { name: 'peak_hold_duration_ms', label: 'Peak Hold Duration (ms)', type: 'number', step: '100', help: 'How long the peak LED stays lit (in milliseconds)', visibleWhen: (config) => config.vu && config.peak_hold },
                    { name: 'peak_hold_color', label: 'Peak Hold Color', type: 'color', help: 'Hex color for the peak hold LED', visibleWhen: (config) => config.vu && config.peak_hold },
                    { name: 'spectrogram', label: 'Spectrogram Mode', type: 'checkbox', help: 'Enable scrolling spectrogram visualization (like FFmpeg showspec or Winamp voiceprint)' },
                    { name: 'spectrogram_scroll_direction', label: 'Scroll Direction', type: 'radio', options: ['right', 'left', 'up', 'down'], help: 'Direction time flows: right (left-to-right), left (right-to-left), up (bottom-to-top), down (top-to-bottom)', visibleWhen: (config) => config.spectrogram },
                    { name: 'spectrogram_scroll_speed', label: 'Scroll Speed (pixels/sec)', type: 'range', min: '1', max: '120', step: '1', help: 'How fast the spectrogram scrolls', visibleWhen: (config) => config.spectrogram },
                    { name: 'spectrogram_window_size', label: 'FFT Window Size', type: 'radio', options: ['512', '1024', '2048', '4096'], help: 'Larger = better frequency resolution but slower response', visibleWhen: (config) => config.spectrogram },
                    { name: 'spectrogram_color_mode', label: 'Color Mapping', type: 'radio', options: ['intensity', 'frequency', 'volume'], help: 'intensity = magnitude->color, frequency = Y-position->color, volume = overall level shifts hue', visibleWhen: (config) => config.spectrogram },
                    { name: 'matrix_2d_enabled', label: '2D Matrix Output', type: 'checkbox', help: 'Enable 2D matrix visualization for spectrum display' },
                    { name: 'matrix_2d_width', label: 'Matrix Width (LEDs)', type: 'number', step: '1', min: '1', help: 'Width of the 2D matrix in LEDs/pixels', visibleWhen: (config) => config.matrix_2d_enabled },
                    { name: 'matrix_2d_height', label: 'Matrix Height (LEDs)', type: 'number', step: '1', min: '1', help: 'Height of the 2D matrix in LEDs/pixels', visibleWhen: (config) => config.matrix_2d_enabled },
                    { name: 'matrix_2d_gradient_direction', label: 'Gradient Direction', type: 'radio', options: ['horizontal', 'vertical'], help: 'horizontal = gradient across frequencies, vertical = gradient across amplitude', visibleWhen: (config) => config.matrix_2d_enabled },
                ]
            },
            // Relay mode specific
            {
                title: 'Network Configuration',
                modes: ['relay'],
                isGroup: true,
                saveButtonText: 'Save IP & Port',
                groupFields: [
                    { name: 'relay_listen_ip', label: 'UDP Listen IP', type: 'text', help: 'IP address to listen on (0.0.0.0 for all interfaces, 127.0.0.1 for localhost)' },
                    { name: 'relay_listen_port', label: 'UDP Listen Port', type: 'number', step: '1', help: 'Port to listen for raw RGB24 frames (default: 1234)' },
                ],
            },
            {
                title: 'Frame Configuration',
                modes: ['relay'],
                isGroup: true,
                saveButtonText: 'Save Frame Dimensions',
                groupFields: [
                    { name: 'relay_frame_width', label: 'Frame Width (pixels)', type: 'number', step: '1', help: 'Width of incoming frames in pixels' },
                    { name: 'relay_frame_height', label: 'Frame Height (pixels)', type: 'number', step: '1', help: 'Height of incoming frames in pixels' },
                ],
            },
            {
                title: 'DDP Packet Timing',
                modes: ['relay'],
                fields: [
                    { name: 'ddp_delay_ms', label: 'DDP Packet Delay (ms)', type: 'number', step: '0.1', help: 'Delay in milliseconds before sending each DDP packet to adjust latency' },
                ]
            },
            {
                title: 'FFmpeg Setup',
                modes: ['relay'],
                isInfo: true,
                info: function() {
                    const listenIp = config.relay_listen_ip || '127.0.0.1';
                    const listenPort = config.relay_listen_port || 1234;
                    const frameWidth = config.relay_frame_width || 64;
                    const frameHeight = config.relay_frame_height || 32;
                    const cmd = `ffmpeg -re -i &lt;input&gt; -an -vf scale=${frameWidth}:${frameHeight} -f rawvideo -pix_fmt rgb24 -s ${frameWidth}x${frameHeight} udp://${listenIp}:${listenPort}`;
                    const cmdPlain = `ffmpeg -re -i <input> -an -vf scale=${frameWidth}:${frameHeight} -f rawvideo -pix_fmt rgb24 -s ${frameWidth}x${frameHeight} udp://${listenIp}:${listenPort}`;
                    return `<div style="display: flex; align-items: center; gap: 10px;">
                               <div style="flex: 1; background: #2d2d2d; padding: 12px; border-radius: 4px; font-family: monospace; font-size: 13px; overflow-x: auto; white-space: nowrap;">${cmd}</div>
                               <button onclick="copyFFmpegCommand('${cmdPlain.replace(/'/g, "\\'")}')" style="padding: 10px 16px; white-space: nowrap; background: #1976d2; border: none; color: white; border-radius: 4px; cursor: pointer; font-size: 13px;">Copy</button>
                           </div>
                           <div style="margin-top: 8px; font-size: 12px; color: #888;">Copy this command to stream video to relay mode</div>`;
                }
            },
            // Webcam mode configuration
            {
                title: 'Webcam Configuration',
                modes: ['webcam'],
                fields: [
                    { name: 'webcam_frame_width', label: 'Frame Width (pixels)', type: 'number', step: '1', help: 'Width of captured webcam frames in pixels' },
                    { name: 'webcam_frame_height', label: 'Frame Height (pixels)', type: 'number', step: '1', help: 'Height of captured webcam frames in pixels' },
                    { name: 'webcam_target_fps', label: 'Target FPS', type: 'number', step: '1', help: 'Target frames per second for webcam capture' },
                    { name: 'webcam_brightness', label: 'Brightness', type: 'range', step: '0.05', min: '0', max: '2', help: 'Brightness multiplier (0.0-2.0). Default 0.5 prevents washout. Lower = darker, higher = brighter' },
                ]
            },
            // Webcam live preview and controls
            {
                title: 'Webcam Preview',
                modes: ['webcam'],
                isInfo: true,
                info: function() {
                    return `
                        <div id="webcam-container" style="display: flex; flex-direction: column; gap: 16px;">
                            <div style="display: flex; justify-content: center;">
                                <video id="webcam-video" autoplay playsinline style="display: none;"></video>
                                <canvas id="webcam-canvas" style="border-radius: 8px; background: #000; image-rendering: pixelated; image-rendering: crisp-edges;"></canvas>
                            </div>
                            <div style="display: flex; flex-direction: column; gap: 12px; align-items: center;">
                                <div style="display: flex; align-items: center; gap: 8px;">
                                    <label for="webcam-device-select" style="font-size: 14px; color: #ccc;">Camera:</label>
                                    <select id="webcam-device-select" style="padding: 8px; background: #2a2a2a; color: white; border: 1px solid #444; border-radius: 4px; font-size: 14px; min-width: 200px;">
                                        <option value="">Loading cameras...</option>
                                    </select>
                                </div>
                                <div style="display: flex; gap: 12px;">
                                    <button id="webcam-start-btn" onclick="startWebcam()" style="padding: 12px 24px; background: #4caf50; border: none; color: white; border-radius: 4px; cursor: pointer; font-size: 14px; font-weight: bold;">Start Webcam</button>
                                    <button id="webcam-stop-btn" onclick="stopWebcam()" disabled style="padding: 12px 24px; background: #f44336; border: none; color: white; border-radius: 4px; cursor: pointer; font-size: 14px; font-weight: bold; opacity: 0.5;">Stop Webcam</button>
                                </div>
                            </div>
                            <div id="webcam-stats" style="text-align: center; font-size: 13px; color: #888;">
                                Not started
                            </div>
                        </div>
                    `;
                }
            },
            // Tron game mode configuration
            {
                title: 'Tron Game Configuration',
                modes: ['tron'],
                fields: [
                    { name: 'tron_width', label: 'Grid Width (pixels)', type: 'number', step: '1', help: 'Width of the game grid in pixels' },
                    { name: 'tron_height', label: 'Grid Height (pixels)', type: 'number', step: '1', help: 'Height of the game grid in pixels' },
                    { name: 'tron_speed_ms', label: 'Game Speed (ms)', type: 'number', step: '0.01', min: '5', max: '10000', help: 'Update interval in milliseconds (lower = faster, 100ms = 10 FPS, minimum 5ms)' },
                    { name: 'tron_reset_delay_ms', label: 'Reset Delay (ms)', type: 'number', step: '100', help: 'Time to wait before restarting after game over' },
                ]
            },
            {
                title: 'Tron AI Configuration',
                modes: ['tron'],
                fields: [
                    { name: 'tron_num_players', label: 'Number of Players', type: 'number', step: '1', help: 'Number of AI players (1 = Snake mode, 2-8 = Tron)' },
                    { name: 'tron_food_mode', label: 'Food Mode', type: 'checkbox', help: 'Players start with length 1 and compete to eat food pixels to grow. Game never resets.' },
                    { name: 'tron_food_max_count', label: 'Maximum Food Count', type: 'number', step: '1', min: '1', max: '100', help: 'Maximum number of food items that can appear simultaneously (1-100). Players pursue closest/safest food.', visibleWhen: (config) => config.tron_food_mode },
                    { name: 'tron_food_ttl_seconds', label: 'Food TTL (seconds)', type: 'number', step: '1', min: '1', max: '300', help: 'How long food stays in one location before relocating (1-300 seconds). Prevents AI from circling forever.', visibleWhen: (config) => config.tron_food_mode },
                    { name: 'tron_super_food_enabled', label: 'Super Food Enabled', type: 'checkbox', help: 'Enable super food spawning (red color, 10% chance, adds +5 length instead of +1)', visibleWhen: (config) => config.tron_food_mode },
                    { name: 'tron_power_food_enabled', label: 'Power Food Enabled', type: 'checkbox', help: 'Enable power food spawning (yellow color, 1% chance, 10 second power mode with immunity, kills on contact, and 25% speed boost)', visibleWhen: (config) => config.tron_food_mode },
                    { name: 'tron_diagonal_movement', label: 'Diagonal Movement', type: 'checkbox', help: 'Enable diagonal movement (8 directions instead of 4 cardinal directions)' },
                    { name: 'tron_look_ahead', label: 'AI Look-Ahead Distance', type: 'number', step: '1', min: '1', max: '128', help: 'How many steps ahead the AI looks (1-128). Higher = smarter but slower' },
                    { name: 'tron_trail_fade', label: 'Trail Fade Effect', type: 'checkbox', help: 'Enable brightness fading on player trails (tail dimmer, head brighter)' },
                    { name: 'tron_trail_length', label: 'Max Trail Length (0 = infinite)', type: 'number', step: '10', min: '0', max: '500', help: '0 = infinite trail, >0 = trail fades after this many steps', visibleWhen: (config) => !config.tron_food_mode },
                    { name: 'tron_ai_aggression', label: 'AI Aggressiveness', type: 'range', min: '0', max: '1', step: '0.05', help: 'How aggressive the AI plays (0 = cautious, 0.5 = balanced, 1.0 = aggressive)' },
                ]
            },
            {
                title: 'Tron Player Colors',
                modes: ['tron'],
                fields: [
                    { name: 'tron_player_1_color', label: 'Player 1 Color', type: 'gradient', help: 'Gradient or hex color for Player 1', visibleWhen: (config) => config.tron_num_players >= 1 },
                    { name: 'tron_player_2_color', label: 'Player 2 Color', type: 'gradient', help: 'Gradient or hex color for Player 2', visibleWhen: (config) => config.tron_num_players >= 2 },
                    { name: 'tron_player_3_color', label: 'Player 3 Color', type: 'gradient', help: 'Gradient or hex color for Player 3', visibleWhen: (config) => config.tron_num_players >= 3 },
                    { name: 'tron_player_4_color', label: 'Player 4 Color', type: 'gradient', help: 'Gradient or hex color for Player 4', visibleWhen: (config) => config.tron_num_players >= 4 },
                    { name: 'tron_player_5_color', label: 'Player 5 Color', type: 'gradient', help: 'Gradient or hex color for Player 5', visibleWhen: (config) => config.tron_num_players >= 5 },
                    { name: 'tron_player_6_color', label: 'Player 6 Color', type: 'gradient', help: 'Gradient or hex color for Player 6', visibleWhen: (config) => config.tron_num_players >= 6 },
                    { name: 'tron_player_7_color', label: 'Player 7 Color', type: 'gradient', help: 'Gradient or hex color for Player 7', visibleWhen: (config) => config.tron_num_players >= 7 },
                    { name: 'tron_player_8_color', label: 'Player 8 Color', type: 'gradient', help: 'Gradient or hex color for Player 8', visibleWhen: (config) => config.tron_num_players >= 8 },
                ]
            },
            {
                title: 'Tron Gradient Animation',
                modes: ['tron'],
                fields: [
                    { name: 'tron_animation_speed', label: 'Animation Speed', type: 'number', step: '0.1', help: 'Speed of gradient animation on trails (0 = disabled, 1.0 = standard speed)' },
                    { name: 'tron_scale_animation_speed', label: 'Scale Speed with Trail Length', type: 'checkbox', help: 'When enabled, longer trails animate faster' },
                    { name: 'tron_animation_direction', label: 'Animation Direction', type: 'radio', options: ['forward', 'backward'], help: 'Direction of gradient animation: forward (head to tail) or backward (tail to head)' },
                    { name: 'tron_flip_direction_on_food', label: 'Flip Direction on Food Eaten', type: 'checkbox', help: 'When enabled, each player\'s animation direction flips every time they eat food' },
                    { name: 'tron_interpolation', label: 'Gradient Interpolation', type: 'select', options: ['linear', 'basis', 'catmullrom'], help: 'Controls how smoothly colors blend: linear (sharp), basis/catmullrom (smooth)' },
                ]
            },
            // Geometry mode configuration
            {
                title: 'Geometry Mode Selection',
                modes: ['geometry'],
                fields: [
                    { name: 'geometry_mode_select', label: 'Geometry Mode', type: 'select',
                      options: ['cycle', 'lissajous', 'fibonacci', 'polar_rose', 'nested_polygons', 'hypotrochoid', 'phyllotaxis', 'kaleidoscope', 'vector_field', 'golden_starburst', 'wireframe_3d', 'mandelbrot', 'dragon', 'hilbert', 'sierpinski', 'fourier', 'attractor', 'boids', 'penrose', 'metaballs', 'icosahedron'],
                      help: 'Select a specific geometry to display, or "cycle" to rotate through all 20 modes' },
                    { name: 'geometry_mode_duration_seconds', label: 'Mode Duration (seconds)', type: 'number', step: '0.5', min: '1', help: 'How long to display each geometry before transitioning to the next (only applies when cycling)', visibleWhen: (config) => config.geometry_mode_select === 'cycle' },
                    { name: 'geometry_randomize_order', label: 'Randomize Order', type: 'checkbox', help: 'Randomly select next geometry instead of cycling sequentially', visibleWhen: (config) => config.geometry_mode_select === 'cycle' },
                ]
            },
            {
                title: 'Gradient Animation',
                modes: ['geometry'],
                fields: [
                    { name: 'interpolation', label: 'Gradient Interpolation', type: 'select', options: ['linear', 'basis', 'catmullrom'], help: 'Gradient interpolation algorithm' },
                    { name: 'animation_speed', label: 'Animation Speed', type: 'number', step: '0.1', help: 'Speed of gradient animation (0 = disabled)' },
                    { name: 'tx_animation_direction', label: 'Animation Direction', type: 'radio', options: ['left', 'right'], help: 'Direction gradient animation moves' },
                ]
            },
            {
                title: 'Geometry Grid Configuration',
                modes: ['geometry'],
                fields: [
                    { name: 'geometry_grid_width', label: 'Grid Width', type: 'number', step: '1', help: 'Width of the 2D grid for geometry calculations (default 64)' },
                    { name: 'geometry_grid_height', label: 'Grid Height', type: 'number', step: '1', help: 'Height of the 2D grid for geometry calculations (default 32)' },
                ]
            },
            {
                title: 'Boid Simulation Parameters',
                modes: ['geometry'],
                fields: [
                    { name: 'boid_count', label: 'Number of Prey Boids', type: 'range', step: '1', min: '1', max: '200', help: 'Number of prey boids in the flocking simulation (default 50)', visibleWhen: (config) => config.geometry_mode_select === 'boids' || config.geometry_mode_select === 'cycle' },
                    { name: 'boid_separation_distance', label: 'Separation Distance', type: 'range', step: '0.01', min: '0.01', max: '0.5', help: 'How far boids stay apart from each other (default 0.1)', visibleWhen: (config) => config.geometry_mode_select === 'boids' || config.geometry_mode_select === 'cycle' },
                    { name: 'boid_alignment_distance', label: 'Alignment Distance', type: 'range', step: '0.01', min: '0.01', max: '1.0', help: 'Distance for velocity alignment with neighbors (default 0.3)', visibleWhen: (config) => config.geometry_mode_select === 'boids' || config.geometry_mode_select === 'cycle' },
                    { name: 'boid_cohesion_distance', label: 'Cohesion Distance', type: 'range', step: '0.01', min: '0.01', max: '1.0', help: 'Distance for steering toward group center (default 0.3)', visibleWhen: (config) => config.geometry_mode_select === 'boids' || config.geometry_mode_select === 'cycle' },
                    { name: 'boid_max_speed', label: 'Prey Max Speed', type: 'range', step: '0.001', min: '0.001', max: '0.1', help: 'Maximum velocity of prey boids (default 0.03)', visibleWhen: (config) => config.geometry_mode_select === 'boids' || config.geometry_mode_select === 'cycle' },
                    { name: 'boid_max_force', label: 'Max Force', type: 'range', step: '0.0001', min: '0.0001', max: '0.01', help: 'Maximum steering force (default 0.001)', visibleWhen: (config) => config.geometry_mode_select === 'boids' || config.geometry_mode_select === 'cycle' },
                ]
            },
            {
                title: 'Predator-Prey Settings',
                modes: ['geometry'],
                fields: [
                    { name: 'boid_predator_enabled', label: 'Enable Predator-Prey', type: 'checkbox', help: 'Enable predator birds that chase prey boids', visibleWhen: (config) => config.geometry_mode_select === 'boids' || config.geometry_mode_select === 'cycle' },
                    { name: 'boid_predator_count', label: 'Number of Predators', type: 'range', step: '1', min: '1', max: '20', help: 'Number of predator birds (default 3)', visibleWhen: (config) => (config.geometry_mode_select === 'boids' || config.geometry_mode_select === 'cycle') && config.boid_predator_enabled },
                    { name: 'boid_predator_speed', label: 'Predator Speed', type: 'range', step: '0.001', min: '0.001', max: '0.15', help: 'Maximum velocity of predators (default 0.04)', visibleWhen: (config) => (config.geometry_mode_select === 'boids' || config.geometry_mode_select === 'cycle') && config.boid_predator_enabled },
                    { name: 'boid_avoidance_distance', label: 'Avoidance Distance', type: 'range', step: '0.01', min: '0.1', max: '1.0', help: 'Distance at which prey flee from predators (default 0.4)', visibleWhen: (config) => (config.geometry_mode_select === 'boids' || config.geometry_mode_select === 'cycle') && config.boid_predator_enabled },
                    { name: 'boid_chase_force', label: 'Chase Force', type: 'range', step: '0.0001', min: '0.0001', max: '0.01', help: 'Force applied to predator pursuit (default 0.002)', visibleWhen: (config) => (config.geometry_mode_select === 'boids' || config.geometry_mode_select === 'cycle') && config.boid_predator_enabled },
                ]
            },
            // Falling Sand mode
            {
                title: 'Falling Sand Settings',
                modes: ['sand'],
                fields: [
                    { name: 'sand_grid_width', label: 'Grid Width', type: 'number', step: '1', min: '8', max: '128', help: 'Width of simulation grid in cells (default 64)' },
                    { name: 'sand_grid_height', label: 'Grid Height', type: 'number', step: '1', min: '8', max: '64', help: 'Height of simulation grid in cells (default 32)' },
                    { name: 'sand_spawn_enabled', label: 'Particle Spawning', type: 'toggle', help: 'Turn on/off spawning new particles (default true)' },
                    { name: 'sand_particle_type', label: 'Spawn Particle', type: 'select', options: ['sand', 'water', 'stone', 'fire', 'wood', 'lava', 'smoke'], help: 'Type of particle to spawn (default sand)', autoSave: true },
                    { name: 'sand_spawn_rate', label: 'Spawn Rate', type: 'range', step: '0.01', min: '0.0', max: '1.0', help: 'Probability of spawning particles per frame (default 0.3)' },
                    { name: 'sand_spawn_radius', label: 'Spawn Radius', type: 'range', step: '1', min: '1', max: '10', help: 'Radius of spawn area in cells (default 3)' },
                    { name: 'sand_spawn_x', label: 'Spawn Position', type: 'range', step: '1', min: '0', max: '63', help: 'X position where particles spawn (0-63, default 32 = center)' },
                    { name: 'sand_obstacles_enabled', label: 'Add Random Obstacles', type: 'checkbox', help: 'Place random wood/stone obstacles in bottom quarter of grid (default false)' },
                    { name: 'sand_obstacle_density', label: 'Obstacle Density', type: 'range', step: '0.01', min: '0.0', max: '1.0', help: 'How many obstacles to place (0.0-1.0, default 0.15)' },
                    { name: 'sand_fire_enabled', label: 'Enable Fire Spread', type: 'checkbox', help: 'Enable fire spreading to flammable materials (default true)' },
                    { name: 'sand_restart', label: 'Restart Simulation', type: 'button', buttonLabel: 'Clear & Restart', help: 'Clear all particles and restart the simulation' },
                ]
            },
            {
                title: 'Particle Colors',
                modes: ['sand'],
                fields: [
                    { name: 'sand_color_sand', label: 'Sand Color', type: 'color', help: 'Color for sand particles (default C2B280)' },
                    { name: 'sand_color_water', label: 'Water Color', type: 'color', help: 'Color for water particles (default 0077BE)' },
                    { name: 'sand_color_stone', label: 'Stone Color', type: 'color', help: 'Color for stone particles (default 808080)' },
                    { name: 'sand_color_fire', label: 'Fire Color', type: 'color', help: 'Color for fire particles (default FF4500)' },
                    { name: 'sand_color_smoke', label: 'Smoke Color', type: 'color', help: 'Color for smoke particles (default 404040)' },
                    { name: 'sand_color_wood', label: 'Wood Color', type: 'color', help: 'Color for wood particles (default 8B4513)' },
                    { name: 'sand_color_lava', label: 'Lava Color', type: 'color', help: 'Color for lava particles (default FF8C00)' },
                ]
            },
        ];

        let config = {};
        let pollingInterval = null;
        let activeSliders = new Set(); // Track sliders being actively dragged

        // Update mode status indicator
        function updateModeStatus() {
            const statusDiv = document.getElementById('mode-status');
            if (!statusDiv) return;

            if (config.mode === 'live' && config.spectrogram) {
                statusDiv.textContent = '→ Spectrogram Mode Active';
                statusDiv.style.color = '#ff00ff';
            } else if (config.mode === 'live' && config.vu) {
                statusDiv.textContent = '→ VU Meter Mode Active';
                statusDiv.style.color = '#00ff88';
            } else if (config.mode === 'live') {
                statusDiv.textContent = '→ FFT Spectrum Mode Active';
                statusDiv.style.color = '#00aaff';
            } else {
                statusDiv.textContent = '';
            }
        }

        // Update individual field in DOM without full re-render
        function updateFieldInDOM(fieldName, value) {
            const element = document.getElementById(fieldName);
            if (!element) return;

            // Don't update if user is actively dragging this slider
            if (activeSliders.has(fieldName)) {
                return;
            }

            // Don't update if user is actively interacting with this element
            if (document.activeElement === element) {
                return;
            }

            // Update the element based on its type
            if (element.type === 'checkbox') {
                element.checked = value;
            } else if (element.type === 'hidden' && document.getElementById(fieldName + '_button')) {
                // Toggle field (hidden input + button)
                element.value = value;
                const button = document.getElementById(fieldName + '_button');
                const isEnabled = value === true || value === 'true';
                button.textContent = isEnabled ? 'ENABLED' : 'DISABLED';
                button.style.backgroundColor = isEnabled ? '#4caf50' : '#757575';
            } else if (element.type === 'radio') {
                const radio = document.querySelector(`input[name="${fieldName}"][value="${value}"]`);
                if (radio) radio.checked = true;
            } else if (element.type === 'range') {
                element.value = value;
                // Update the display value label
                const valueLabel = document.getElementById(`${fieldName}_value`);
                if (valueLabel) {
                    if (fieldName === 'webcam_brightness') {
                        valueLabel.textContent = value.toFixed(2) + 'x';
                    } else if (fieldName === 'sand_spawn_rate' || fieldName === 'sand_obstacle_density') {
                        valueLabel.textContent = value.toFixed(2);
                    } else {
                        valueLabel.textContent = value.toFixed ? value.toFixed(0) + '%' : value;
                    }
                }
            } else if (element.tagName === 'SELECT') {
                element.value = value;
            } else {
                element.value = value;
            }
        }

        function trackSliderStart(fieldName) {
            activeSliders.add(fieldName);
        }

        function trackSliderEnd(fieldName) {
            // Delay removal to ensure SSE update doesn't interrupt
            setTimeout(() => {
                activeSliders.delete(fieldName);
            }, 100);
        }

        async function loadConfig() {
            try {
                const res = await fetch('/api/config');
                const newConfig = await res.json();

                // Detect which fields changed
                const changedFields = [];
                const oldConfig = config;

                for (const key in newConfig) {
                    if (JSON.stringify(newConfig[key]) !== JSON.stringify(oldConfig[key])) {
                        changedFields.push(key);
                    }
                }

                // Check if structural changes require full re-render
                const needsFullRender = pollingInterval === null ||
                    changedFields.includes('mode') ||
                    changedFields.includes('vu') ||
                    changedFields.includes('use_gradient') ||
                    changedFields.includes('intensity_colors');

                config = newConfig;

                // Update mode selector
                const modeSelect = document.getElementById('mode');
                if (modeSelect && config.mode) {
                    modeSelect.value = config.mode;
                }

                // Update brightness slider
                const brightnessSlider = document.getElementById('global-brightness');
                if (brightnessSlider && config.global_brightness !== undefined) {
                    const brightnessPercent = Math.round(config.global_brightness * 100);
                    brightnessSlider.value = brightnessPercent;
                    updateBrightnessDisplay(brightnessPercent);
                }

                // Update mode status indicator
                updateModeStatus();

                // Update WLED liveview iframe - always active regardless of mode
                const wledIframe = document.getElementById('wled-liveview');
                if (wledIframe && config.wled_ip) {
                    const liveviewUrl = `http://${config.wled_ip}/liveview?ws`;
                    if (wledIframe.src !== liveviewUrl) {
                        wledIframe.src = liveviewUrl;
                        console.log('WLED Liveview loaded:', liveviewUrl);
                    }
                }

                if (needsFullRender) {
                    // Full re-render for structural changes
                    renderConfig();
                    if (changedFields.length > 0 && pollingInterval !== null) {
                        showMessage('Config reloaded from file', 'success');
                    }
                } else if (changedFields.length > 0) {
                    // Selective update for simple value changes
                    changedFields.forEach(field => {
                        updateFieldInDOM(field, newConfig[field]);
                    });
                    console.log('Updated fields:', changedFields.join(', '));
                }
            } catch (e) {
                showMessage('Failed to load configuration', 'error');
            }
        }

        function renderConfig() {
            const container = document.getElementById('config-container');
            const currentMode = config.mode || 'bandwidth';

            // Filter sections to only show those for current mode and custom visibility rules
            const visibleSections = fieldSections.filter(section => {
                // Check if section applies to current mode
                if (!section.modes || !section.modes.includes(currentMode)) {
                    return false;
                }
                // Check custom visibility condition if provided
                if (section.visibleWhen && !section.visibleWhen(config)) {
                    return false;
                }
                return true;
            });

            container.innerHTML = visibleSections.map(section => {
                // Special handling for Testing section
                if (section.isTesting) {
                    const testingHTML = `
                        <div style="display: flex; flex-direction: column; gap: 20px;">
                            <div style="width: 100%;">
                                <div style="margin-bottom: 10px;">
                                    <label style="display: inline-flex; align-items: center; gap: 8px; cursor: pointer; font-size: 1em;">
                                        <input type="checkbox" id="test_tx" onchange="saveField('test_tx', 'checkbox')" ${config.test_tx ? 'checked' : ''}>
                                        <span>TX (Upload)</span>
                                    </label>
                                </div>
                                <div>
                                    <label style="display: block; font-size: 0.85em; color: #808080; margin-bottom: 8px;">Utilization: <span id="test_tx_percent_value">${(config.test_tx_percent || 100).toFixed(0)}%</span></label>
                                    <input type="range" id="test_tx_percent" value="${config.test_tx_percent || 100}"
                                           min="0" max="101" step="1" style="width: 100%;"
                                           oninput="updateTestRangeValue('test_tx_percent'); saveField('test_tx_percent', 'range')"
                                           onmousedown="trackSliderStart('test_tx_percent')" onmouseup="trackSliderEnd('test_tx_percent')"
                                           ontouchstart="trackSliderStart('test_tx_percent')" ontouchend="trackSliderEnd('test_tx_percent')">
                                </div>
                            </div>
                            <div style="width: 100%;">
                                <div style="margin-bottom: 10px;">
                                    <label style="display: inline-flex; align-items: center; gap: 8px; cursor: pointer; font-size: 1em;">
                                        <input type="checkbox" id="test_rx" onchange="saveField('test_rx', 'checkbox')" ${config.test_rx ? 'checked' : ''}>
                                        <span>RX (Download)</span>
                                    </label>
                                </div>
                                <div>
                                    <label style="display: block; font-size: 0.85em; color: #808080; margin-bottom: 8px;">Utilization: <span id="test_rx_percent_value">${(config.test_rx_percent || 100).toFixed(0)}%</span></label>
                                    <input type="range" id="test_rx_percent" value="${config.test_rx_percent || 100}"
                                           min="0" max="101" step="1" style="width: 100%;"
                                           oninput="updateTestRangeValue('test_rx_percent'); saveField('test_rx_percent', 'range')"
                                           onmousedown="trackSliderStart('test_rx_percent')" onmouseup="trackSliderEnd('test_rx_percent')"
                                           ontouchstart="trackSliderStart('test_rx_percent')" ontouchend="trackSliderEnd('test_rx_percent')">
                                </div>
                            </div>
                        </div>
                    `;

                    return `
                        <div class="section">
                            <div class="section-header">${section.title}</div>
                            ${testingHTML}
                            ${section.help ? `<div class="help-text" style="text-align: center; margin-top: 12px;">${section.help}</div>` : ''}
                        </div>
                    `;
                }

                // Info sections - display dynamic information with no inputs
                if (section.isInfo && section.info) {
                    return `
                        <div class="section">
                            <div class="section-header">${section.title}</div>
                            <div style="padding: 12px;">
                                ${section.info()}
                            </div>
                        </div>
                    `;
                }

                // Group sections - multiple fields with single save button
                if (section.isGroup && section.groupFields) {
                    const groupFieldsHTML = section.groupFields.map(field => {
                        const value = config[field.name];
                        let inputHTML;

                        if (field.type === 'network_interface') {
                            // Network interface selector with checkboxes - always shown
                            const currentInterfaces = value ? value.split(',').map(s => s.trim()) : [];
                            const statusMsg = !value || value.trim() === ''
                                ? '<p style="color: #ff9800; margin: 5px 0;">⚠️ No interface configured. Please select one or more network interfaces:</p>'
                                : `<p style="color: #4caf50; margin: 5px 0;">✓ Currently monitoring: <strong>${value}</strong></p>`;

                            inputHTML = `
                                <div id="interface_selector">
                                    ${statusMsg}
                                    <div id="interface_list" style="margin: 10px 0; max-height: 300px; overflow-y: auto; border: 1px solid #444; padding: 10px; background: #1e1e1e; border-radius: 4px;">
                                        <p style="color: #999;">Loading network interfaces...</p>
                                    </div>
                                    <div style="margin-top: 10px;">
                                        <input type="text" id="interface_manual" placeholder="Or enter interface names manually (e.g., en0,en1)" style="width: 100%; padding: 8px; background: #2a2a2a; border: 1px solid #444; color: #fff; border-radius: 4px;" value="${value || ''}">
                                    </div>
                                </div>
                            `;
                        } else {
                            inputHTML = `<input type="${field.type}" id="${field.name}" value="${value || ''}" ${field.step ? `step="${field.step}"` : ''}>`;
                        }

                        const helpText = field.help ? `<div class="help-text">${field.help}</div>` : '';

                        return `
                            <div class="config-item">
                                <label for="${field.name}">${field.label}</label>
                                <div class="input-group">
                                    ${inputHTML}
                                </div>
                                ${helpText}
                            </div>
                        `;
                    }).join('');

                    const groupFieldNames = section.groupFields.map(f => f.name).join(',');
                    const buttonText = section.saveButtonText || 'Save Settings';

                    return `
                        <div class="section">
                            <div class="section-header">${section.title}</div>
                            <div class="config-grid">
                                ${groupFieldsHTML}
                                <div class="config-item" style="grid-column: 1 / -1;">
                                    <button onclick="saveGroup('${groupFieldNames}')" style="width: 100%; padding: 10px; font-size: 1em;">${buttonText}</button>
                                </div>
                            </div>
                            ${section.help ? `<div class="help-text" style="margin-top: 12px;">${section.help}</div>` : ''}
                        </div>
                    `;
                }

                // Regular sections
                // Filter fields based on visibleWhen condition
                const visibleFields = section.fields.filter(field => {
                    if (field.visibleWhen && !field.visibleWhen(config)) {
                        return false;
                    }
                    return true;
                });

                const fieldsHTML = visibleFields.map(field => {
                    const value = config[field.name];
                    let inputHTML = '';
                    let saveButton = '';

                    if (field.type === 'checkbox') {
                        // Checkboxes auto-save on change, no Save button needed
                        inputHTML = `<input type="checkbox" id="${field.name}" onchange="saveField('${field.name}', '${field.type}')" ${value ? 'checked' : ''}>`;
                    } else if (field.type === 'toggle') {
                        // Large toggle button for boolean values
                        const isEnabled = value === true || value === 'true';
                        const buttonText = isEnabled ? 'ENABLED' : 'DISABLED';
                        const buttonColor = isEnabled ? '#4caf50' : '#757575';
                        inputHTML = `
                            <button id="${field.name}_button"
                                    onclick="toggleField('${field.name}')"
                                    style="width: 100%; padding: 16px 24px; font-size: 18px; font-weight: bold;
                                           background-color: ${buttonColor}; color: white; border: none;
                                           border-radius: 8px; cursor: pointer; transition: background-color 0.3s;">
                                ${buttonText}
                            </button>
                            <input type="hidden" id="${field.name}" value="${isEnabled}">
                        `;
                        saveButton = ''; // Auto-saves on toggle
                    } else if (field.type === 'range') {
                        // Range sliders show current value and auto-save on change
                        // Set default value based on field type
                        let currentValue;
                        if (value !== undefined && value !== null) {
                            currentValue = value;
                        } else if (field.name === 'rx_split_percent') {
                            currentValue = 50;
                        } else if (field.name === 'audio_gain') {
                            currentValue = 0;  // Audio gain defaults to 0 (no change)
                        } else {
                            currentValue = 0;
                        }

                        // Format display value based on field type
                        let displayValue;
                        if (field.name === 'rx_split_percent') {
                            const txSplit = (100 - currentValue).toFixed(0);
                            displayValue = `RX ${currentValue.toFixed(0)}% / TX ${txSplit}%`;
                        } else if (field.name === 'audio_gain') {
                            // Audio gain: show with explicit + sign for positive values
                            const sign = currentValue > 0 ? '+' : '';
                            displayValue = `${sign}${currentValue.toFixed(0)}%`;
                        } else if (field.name === 'webcam_brightness') {
                            // Brightness: show as multiplier (0.0x to 2.0x)
                            displayValue = `${currentValue.toFixed(2)}x`;
                        } else if (field.name === 'boid_count') {
                            // Boid count: integer value
                            displayValue = `${currentValue.toFixed(0)}`;
                        } else if (field.name && field.name.startsWith('boid_')) {
                            // Boid parameters: show as decimal values with appropriate precision
                            if (field.name === 'boid_max_force') {
                                displayValue = `${currentValue.toFixed(4)}`;
                            } else if (field.name === 'boid_max_speed') {
                                displayValue = `${currentValue.toFixed(3)}`;
                            } else {
                                // separation, alignment, cohesion distances
                                displayValue = `${currentValue.toFixed(2)}`;
                            }
                        } else if (field.name === 'sand_spawn_rate' || field.name === 'sand_obstacle_density') {
                            // Sand rates: show as decimal 0.0-1.0
                            displayValue = `${currentValue.toFixed(2)}`;
                        } else {
                            // All other sliders just show the value with % sign
                            displayValue = `${currentValue.toFixed(0)}%`;
                        }

                        inputHTML = `
                            <input type="range" id="${field.name}" value="${currentValue}"
                                   min="${field.min || 0}" max="${field.max || 100}" step="${field.step || 1}"
                                   oninput="updateRangeValue('${field.name}')"
                                   onchange="saveField('${field.name}', '${field.type}')"
                                   onmousedown="trackSliderStart('${field.name}')" onmouseup="trackSliderEnd('${field.name}')"
                                   ontouchstart="trackSliderStart('${field.name}')" ontouchend="trackSliderEnd('${field.name}')">
                            <div class="range-value" id="${field.name}_value">${displayValue}</div>
                        `;
                    } else if (field.type === 'radio') {
                        // Radio buttons auto-save on change, no Save button needed
                        inputHTML = field.options.map(opt => `
                            <label style="display: inline-flex; align-items: center; gap: 5px; margin-right: 20px; cursor: pointer;">
                                <input type="radio" name="${field.name}" value="${opt}"
                                       onchange="saveField('${field.name}', '${field.type}')"
                                       ${String(value) === String(opt) ? 'checked' : ''}>
                                <span>${opt}</span>
                            </label>
                        `).join('');
                    } else if (field.type === 'select') {
                        // Select dropdowns can auto-save on change (no Save button needed for immediate fields)
                        const autoSave = field.autoSave || field.name === 'direction';
                        inputHTML = `<select id="${field.name}" ${autoSave ? `onchange="saveField('${field.name}', '${field.type}')"` : ''}>${field.options.map(opt =>
                            `<option value="${opt}" ${value === opt ? 'selected' : ''}>${opt}</option>`
                        ).join('')}</select>`;
                        if (!autoSave) {
                            saveButton = `<button onclick="saveField('${field.name}', '${field.type}')">Save</button>`;
                        }
                    } else if (field.type === 'gradient') {
                        // Gradient field with dropdown + custom option
                        const gradientId = `${field.name}_gradient`;
                        const customId = `${field.name}_custom`;
                        const currentValue = value || '';
                        const allowNone = field.allowNone || false;

                        inputHTML = `
                            <div id="${gradientId}_container" data-allow-none="${allowNone}">
                                <select id="${gradientId}" onchange="handleGradientChange('${field.name}')">
                                    <option value="">Loading gradients...</option>
                                </select>
                                <div id="${gradientId}_expand" style="display: none; margin-top: 10px;">
                                    <button onclick="expandGradientName('${field.name}')" style="width: 100%; background-color: #1976d2; color: white;">Expand Gradient to Hex Colors</button>
                                </div>
                                <div id="${customId}_container" style="display: none; margin-top: 10px;">
                                    <textarea id="${customId}" style="resize: vertical; font-family: monospace; width: 100%; min-height: 40px; height: 40px; overflow-y: hidden; overflow-x: hidden; box-sizing: border-box; white-space: pre-wrap; word-wrap: break-word;" oninput="autoResizeTextarea('${customId}')" placeholder="Enter comma-separated hex colors (e.g., FF0000,00FF00,0000FF)">${currentValue}</textarea>
                                    <div style="margin-top: 8px; display: flex; gap: 10px;">
                                        <button onclick="saveCustomGradient('${field.name}')" style="flex: 1;">Save as Custom Gradient</button>
                                    </div>
                                </div>
                                <div id="${gradientId}_delete" style="display: none; margin-top: 10px;">
                                    <button onclick="deleteSelectedGradient('${field.name}')" style="width: 100%; background-color: #d32f2f; color: white;">Remove Custom Gradient</button>
                                </div>
                            </div>
                        `;
                        saveButton = `<button id="${gradientId}_save" onclick="saveGradientField('${field.name}')">Save</button>`;
                    } else if (field.type === 'audio_device') {
                        // Audio device dropdown with dynamic loading
                        inputHTML = `
                            <select id="${field.name}" onchange="saveField('${field.name}', 'select')">
                                <option value="">Loading audio devices...</option>
                            </select>
                        `;
                        saveButton = ''; // Auto-save on change
                    } else if (field.type === 'textarea') {
                        inputHTML = `<textarea id="${field.name}" rows="2" style="resize: vertical; font-family: monospace; overflow: hidden;" oninput="autoResizeTextarea(this)">${value || ''}</textarea>`;
                        saveButton = `<button onclick="saveField('${field.name}', '${field.type}')">Save</button>`;
                    } else if (field.type === 'network_interface') {
                        // Network interface selector with checkboxes - always shown
                        const currentInterfaces = value ? value.split(',').map(s => s.trim()) : [];
                        const statusMsg = !value || value.trim() === ''
                            ? '<p style="color: #ff9800; margin: 5px 0;">⚠️ No interface configured. Please select one or more network interfaces:</p>'
                            : `<p style="color: #4caf50; margin: 5px 0;">✓ Currently monitoring: <strong>${value}</strong></p>`;

                        inputHTML = `
                            <div id="interface_selector">
                                ${statusMsg}
                                <div id="interface_list" style="margin: 10px 0; max-height: 300px; overflow-y: auto; border: 1px solid #ccc; padding: 10px; background: #f9f9f9;">
                                    <p>Loading network interfaces...</p>
                                </div>
                                <div style="margin-top: 10px;">
                                    <input type="text" id="interface_manual" placeholder="Or enter interface names manually (e.g., en0,en1)" style="width: 100%; padding: 5px;" value="${value || ''}">
                                </div>
                            </div>
                        `;
                        saveButton = `<button onclick="saveSelectedInterfaces()">Apply Changes</button>`;
                    } else if (field.type === 'arrows') {
                        // Number input with left/right arrow buttons
                        const min = field.min || 0;
                        const max = field.max || 100;
                        const step = field.step || 1;
                        inputHTML = `
                            <div style="display: flex; align-items: center; gap: 10px;">
                                <button onclick="adjustValue('${field.name}', -${step}, ${min}, ${max})" style="padding: 8px 16px; font-size: 18px; font-weight: bold;">←</button>
                                <input type="number" id="${field.name}" value="${value || min}" min="${min}" max="${max}" step="${step}" style="width: 80px; text-align: center; font-size: 16px; padding: 5px;" readonly>
                                <button onclick="adjustValue('${field.name}', ${step}, ${min}, ${max})" style="padding: 8px 16px; font-size: 18px; font-weight: bold;">→</button>
                            </div>
                        `;
                        saveButton = ''; // Auto-saves on arrow click
                    } else if (field.type === 'button') {
                        // Action button (no value, just triggers an action)
                        inputHTML = `<button onclick="triggerAction('${field.name}')" style="padding: 8px 16px; font-weight: bold;">${field.buttonLabel || field.label}</button>`;
                        saveButton = ''; // No separate save button needed
                    } else {
                        // Special handling for strobe_duration_ms validation
                        if (field.name === 'strobe_duration_ms') {
                            inputHTML = `<input type="${field.type}" id="${field.name}" value="${value || ''}" ${field.step ? `step="${field.step}"` : ''} oninput="validateStrobeDuration()">`;
                            saveButton = `<button id="save_${field.name}" onclick="saveField('${field.name}', '${field.type}')">Save</button>`;
                        } else {
                            inputHTML = `<input type="${field.type}" id="${field.name}" value="${value || ''}" ${field.step ? `step="${field.step}"` : ''} ${field.min !== undefined ? `min="${field.min}"` : ''} ${field.max !== undefined ? `max="${field.max}"` : ''}>`;
                            saveButton = `<button onclick="saveField('${field.name}', '${field.type}')">Save</button>`;
                        }
                    }

                    // Dynamic help text for strobe_duration_ms
                    let helpText = '';
                    if (field.help) {
                        if (field.name === 'strobe_duration_ms') {
                            const maxDuration = config.strobe_rate_hz > 0 ? (1000.0 / config.strobe_rate_hz).toFixed(1) : '1000.0';
                            helpText = `<div class="help-text" id="help_${field.name}">${field.help} Current max: ${maxDuration}ms</div>`;
                        } else {
                            helpText = `<div class="help-text">${field.help}</div>`;
                        }
                    }

                    return `
                        <div class="config-item">
                            <label for="${field.name}">${field.label}</label>
                            <div class="input-group">
                                ${inputHTML}
                                ${saveButton}
                            </div>
                            ${helpText}
                        </div>
                    `;
                }).join('');

                return `
                    <div class="section">
                        <div class="section-header">${section.title}</div>
                        <div class="config-grid">
                            ${fieldsHTML}
                        </div>
                    </div>
                `;
            }).join('');

            // After rendering, validate strobe duration and auto-size textareas
            setTimeout(() => {
                validateStrobeDuration();
                document.querySelectorAll('textarea').forEach(ta => autoResizeTextarea(ta));

                // Populate gradient dropdowns for ALL gradient-type fields (not just specific ones)
                // Find all select elements with IDs ending in '_gradient'
                document.querySelectorAll('select[id$="_gradient"]').forEach(select => {
                    const fieldName = select.id.replace('_gradient', '');
                    populateGradientDropdown(fieldName);
                });

                // Populate audio device dropdown if present
                if (document.getElementById('audio_device')) {
                    populateAudioDeviceDropdown();
                }

                // Load network interfaces if interface selector is present
                if (document.getElementById('interface_selector')) {
                    loadNetworkInterfaces();
                }

                // Load webcam devices if webcam selector is present
                if (document.getElementById('webcam-device-select')) {
                    loadWebcamDevices();
                }
            }, 0);
        }

        function autoResizeTextarea(textarea) {
            // Reset height to auto to get the correct scrollHeight
            textarea.style.height = 'auto';
            // Set height to scrollHeight to fit content
            textarea.style.height = textarea.scrollHeight + 'px';
        }

        function copyFFmpegCommand(cmd) {
            const button = event.target;
            const originalText = button.textContent;

            // Try modern clipboard API first, fallback to execCommand for iOS
            if (navigator.clipboard && navigator.clipboard.writeText) {
                navigator.clipboard.writeText(cmd).then(() => {
                    // Visual feedback - change button text briefly
                    button.textContent = 'Copied!';
                    button.style.background = '#4caf50';
                    setTimeout(() => {
                        button.textContent = originalText;
                        button.style.background = '#1976d2';
                    }, 2000);
                }).catch(err => {
                    console.error('Clipboard API failed, trying fallback:', err);
                    fallbackCopy(cmd, button, originalText);
                });
            } else {
                // Fallback for iOS and older browsers
                fallbackCopy(cmd, button, originalText);
            }
        }

        function fallbackCopy(text, button, originalText) {
            // Create temporary textarea
            const textarea = document.createElement('textarea');
            textarea.value = text;
            textarea.style.position = 'fixed';
            textarea.style.top = '0';
            textarea.style.left = '0';
            textarea.style.width = '2em';
            textarea.style.height = '2em';
            textarea.style.padding = '0';
            textarea.style.border = 'none';
            textarea.style.outline = 'none';
            textarea.style.boxShadow = 'none';
            textarea.style.background = 'transparent';
            textarea.setAttribute('readonly', '');
            document.body.appendChild(textarea);

            // Select and copy
            textarea.select();
            textarea.setSelectionRange(0, text.length);

            try {
                const successful = document.execCommand('copy');
                if (successful) {
                    button.textContent = 'Copied!';
                    button.style.background = '#4caf50';
                    setTimeout(() => {
                        button.textContent = originalText;
                        button.style.background = '#1976d2';
                    }, 2000);
                } else {
                    alert('Failed to copy to clipboard');
                }
            } catch (err) {
                console.error('Fallback copy failed:', err);
                alert('Failed to copy to clipboard');
            } finally {
                document.body.removeChild(textarea);
            }
        }

        // Webcam functionality
        let webcamStream = null;
        let webcamWs = null;
        let webcamInterval = null;
        let webcamFrameCount = 0;

        // Enumerate available cameras - requires permission first
        async function loadWebcamDevices() {
            const select = document.getElementById('webcam-device-select');

            try {
                // First, request basic camera permission to get device labels
                select.innerHTML = '<option value="">Requesting camera permission...</option>';
                const stream = await navigator.mediaDevices.getUserMedia({ video: true });

                // Now enumerate with labels available
                const devices = await navigator.mediaDevices.enumerateDevices();
                const videoDevices = devices.filter(device => device.kind === 'videoinput');

                // Stop the permission stream immediately
                stream.getTracks().forEach(track => track.stop());

                if (videoDevices.length === 0) {
                    select.innerHTML = '<option value="">No cameras found</option>';
                    return;
                }

                select.innerHTML = videoDevices.map((device, index) => {
                    const label = device.label || `Camera ${index + 1}`;
                    return `<option value="${device.deviceId}">${label}</option>`;
                }).join('');
            } catch (err) {
                console.error('Failed to enumerate cameras:', err);
                select.innerHTML = '<option value="">Camera permission denied or error</option>';
            }
        }

        async function startWebcam() {
            try {
                const video = document.getElementById('webcam-video');
                const canvas = document.getElementById('webcam-canvas');
                const startBtn = document.getElementById('webcam-start-btn');
                const stopBtn = document.getElementById('webcam-stop-btn');
                const stats = document.getElementById('webcam-stats');

                // Fetch fresh config to ensure we have latest values
                stats.textContent = 'Loading configuration...';
                const configResponse = await fetch('/api/config');
                const freshConfig = await configResponse.json();

                // Get config values
                const width = freshConfig.webcam_frame_width || 16;
                const height = freshConfig.webcam_frame_height || 16;
                const targetFps = freshConfig.webcam_target_fps || 30;
                const brightness = freshConfig.webcam_brightness || 0.5;
                const frameInterval = 1000 / targetFps;

                console.log(`Webcam config: ${width}×${height} @ ${targetFps} FPS, brightness: ${brightness}`);

                // Get selected camera device
                const deviceSelect = document.getElementById('webcam-device-select');
                const selectedDeviceId = deviceSelect.value;

                // Request webcam access
                stats.textContent = 'Requesting webcam access...';
                const constraints = {
                    video: {
                        width: { ideal: width * 10 },
                        height: { ideal: height * 10 }
                    }
                };

                // Add deviceId constraint if a specific camera is selected
                if (selectedDeviceId) {
                    constraints.video.deviceId = { exact: selectedDeviceId };
                }

                webcamStream = await navigator.mediaDevices.getUserMedia(constraints);
                video.srcObject = webcamStream;

                // Wait for video to be ready
                await new Promise(resolve => video.onloadedmetadata = resolve);

                // Setup canvas at exact configured size (actual capture resolution)
                canvas.width = width;
                canvas.height = height;

                // Scale canvas display to 4x for visibility (CSS scaling, preserves pixelation)
                canvas.style.width = (width * 4) + 'px';
                canvas.style.height = (height * 4) + 'px';

                const ctx = canvas.getContext('2d', { willReadFrequently: true });

                console.log(`Webcam canvas: ${width}x${height} (${width * height * 4} bytes RGBA)`);

                // Connect WebSocket
                const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
                const wsUrl = `${protocol}//${window.location.host}/ws/webcam`;
                stats.textContent = 'Connecting to server...';

                webcamWs = new WebSocket(wsUrl);
                webcamWs.binaryType = 'arraybuffer';

                webcamWs.onopen = () => {
                    stats.textContent = 'Connected! Streaming...';
                    startBtn.disabled = true;
                    startBtn.style.opacity = '0.5';
                    stopBtn.disabled = false;
                    stopBtn.style.opacity = '1';

                    // Start capturing frames as fast as possible
                    // Server will rate-limit based on FPS config
                    function captureLoop() {
                        if (webcamWs && webcamWs.readyState === WebSocket.OPEN) {
                            captureAndSendFrame(video, canvas, ctx, width, height);
                            requestAnimationFrame(captureLoop);
                        }
                    }
                    requestAnimationFrame(captureLoop);
                };

                webcamWs.onmessage = (event) => {
                    try {
                        const msg = JSON.parse(event.data);
                        if (msg.type === 'stats') {
                            stats.textContent = `Streaming | Frames: ${msg.frameCount} | ${targetFps} FPS`;
                        }
                    } catch (e) {
                        // Binary message, ignore
                    }
                };

                webcamWs.onerror = (error) => {
                    console.error('WebSocket error:', error);
                    stats.textContent = 'Connection error';
                    stopWebcam();
                };

                webcamWs.onclose = () => {
                    if (webcamStream) {
                        stats.textContent = 'Connection closed';
                        stopWebcam();
                    }
                };

            } catch (error) {
                console.error('Error starting webcam:', error);
                document.getElementById('webcam-stats').textContent = `Error: ${error.message}`;
                stopWebcam();
            }
        }

        function captureAndSendFrame(video, canvas, ctx, width, height) {
            // Check WebSocket buffer - drop frame if too much data is queued
            if (!webcamWs || webcamWs.readyState !== WebSocket.OPEN) {
                return;
            }

            // Drop frame if WebSocket buffer has too much pending
            if (webcamWs.bufferedAmount > 5120) {
                return;
            }

            // Draw video frame to canvas
            ctx.drawImage(video, 0, 0, canvas.width, canvas.height);

            // Get raw RGBA image data
            const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);

            // Send raw RGBA data to server (server handles FPS limiting, brightness, etc)
            webcamWs.send(imageData.data.buffer);
            webcamFrameCount++;
        }

        function stopWebcam() {
            const startBtn = document.getElementById('webcam-start-btn');
            const stopBtn = document.getElementById('webcam-stop-btn');
            const stats = document.getElementById('webcam-stats');
            const video = document.getElementById('webcam-video');

            // Close WebSocket (this will stop the requestAnimationFrame loop)
            if (webcamWs) {
                webcamWs.close();
                webcamWs = null;
            }

            // Stop webcam stream
            if (webcamStream) {
                webcamStream.getTracks().forEach(track => track.stop());
                webcamStream = null;
                video.srcObject = null;
            }

            // Reset UI
            if (startBtn && stopBtn && stats) {
                startBtn.disabled = false;
                startBtn.style.opacity = '1';
                stopBtn.disabled = true;
                stopBtn.style.opacity = '0.5';
                stats.textContent = `Stopped (${webcamFrameCount} frames sent)`;
            }

            webcamFrameCount = 0;
        }

        function updateRangeValue(fieldName) {
            const input = document.getElementById(fieldName);
            const display = document.getElementById(fieldName + '_value');
            const value = parseFloat(input.value);

            // Format the display based on field type
            if (fieldName === 'rx_split_percent') {
                // RX/TX split shows both percentages
                const txValue = 100 - value;
                display.textContent = `RX ${value.toFixed(0)}% / TX ${txValue.toFixed(0)}%`;
            } else if (fieldName === 'audio_gain') {
                // Audio gain: -200% to +200%, middle is 0%
                const sign = value > 0 ? '+' : '';
                display.textContent = `${sign}${value.toFixed(0)}%`;
            } else if (fieldName === 'webcam_brightness') {
                // Brightness: show as multiplier (0.0x to 2.0x)
                display.textContent = `${value.toFixed(2)}x`;
            } else if (fieldName === 'boid_count') {
                // Boid count: integer value
                display.textContent = `${value.toFixed(0)}`;
            } else if (fieldName.startsWith('boid_')) {
                // Boid parameters: show as decimal values with appropriate precision
                if (fieldName === 'boid_max_force') {
                    display.textContent = `${value.toFixed(4)}`;
                } else if (fieldName === 'boid_max_speed') {
                    display.textContent = `${value.toFixed(3)}`;
                } else {
                    // separation, alignment, cohesion distances
                    display.textContent = `${value.toFixed(2)}`;
                }
            } else if (fieldName === 'sand_spawn_rate' || fieldName === 'sand_obstacle_density') {
                // Sand rates: show as decimal 0.0-1.0
                display.textContent = `${value.toFixed(2)}`;
            } else {
                // All other range sliders just show the value with % sign
                display.textContent = `${value.toFixed(0)}%`;
            }
        }

        function updateTestRangeValue(fieldName) {
            const input = document.getElementById(fieldName);
            const display = document.getElementById(fieldName + '_value');
            const value = parseFloat(input.value);
            display.textContent = `${value.toFixed(0)}%`;
        }

        function validateStrobeDuration() {
            const durationInput = document.getElementById('strobe_duration_ms');
            const saveButton = document.getElementById('save_strobe_duration_ms');
            const helpText = document.getElementById('help_strobe_duration_ms');

            if (!durationInput || !saveButton) return;

            const duration = parseFloat(durationInput.value);
            const strobeRateHz = config.strobe_rate_hz || 3.0;
            const maxDuration = strobeRateHz > 0 ? (1000.0 / strobeRateHz) : 1000.0;

            if (duration > maxDuration || duration < 0 || isNaN(duration)) {
                // Invalid - highlight red and disable save
                durationInput.classList.add('invalid');
                saveButton.disabled = true;
                if (helpText) {
                    helpText.innerHTML = `Duration of strobe effect in milliseconds. Cannot exceed cycle time (e.g., 3 Hz = 333ms max) <span style="color: #ff4444; font-weight: bold;">Current max: ${maxDuration.toFixed(1)}ms - Value exceeds maximum!</span>`;
                }
            } else {
                // Valid - remove red and enable save
                durationInput.classList.remove('invalid');
                saveButton.disabled = false;
                if (helpText) {
                    helpText.innerHTML = `Duration of strobe effect in milliseconds. Cannot exceed cycle time (e.g., 3 Hz = 333ms max) Current max: ${maxDuration.toFixed(1)}ms`;
                }
            }
        }

        // Global variable to store loaded gradients
        let allGradients = {};
        let audioDevices = [];

        // Load gradients from API
        async function loadGradients() {
            try {
                const res = await fetch('/api/gradients');
                allGradients = await res.json();
                console.log('Loaded gradients:', Object.keys(allGradients).length);
            } catch (e) {
                console.error('Failed to load gradients:', e);
            }
        }

        // Load audio devices from API
        async function loadAudioDevices() {
            try {
                const res = await fetch('/api/audio_devices');
                audioDevices = await res.json();
                console.log('Loaded audio devices:', audioDevices.length);
            } catch (e) {
                console.error('Failed to load audio devices:', e);
            }
        }

        // Load and display network interfaces
        async function loadNetworkInterfaces() {
            const interfaceList = document.getElementById('interface_list');
            if (!interfaceList) return;

            // Get SSH config to determine where to fetch interfaces from
            const sshHost = config.ssh_host || '';
            const sshUser = config.ssh_user || '';
            const currentInterfaces = config.interface ? config.interface.split(',').map(s => s.trim()) : [];

            try {
                // Build query params for SSH if configured
                let url = '/api/network_interfaces';
                const params = new URLSearchParams();
                if (sshHost.trim() !== '') {
                    params.append('ssh_host', sshHost);
                    if (sshUser.trim() !== '') {
                        params.append('ssh_user', sshUser);
                    }
                }
                if (params.toString()) {
                    url += '?' + params.toString();
                }

                const res = await fetch(url);
                const interfaces = await res.json();

                if (interfaces.length === 0) {
                    interfaceList.innerHTML = '<p style="color: #f44336;">No network interfaces found!</p>';
                    return;
                }

                // Build checkboxes for each interface, pre-selecting currently configured ones
                let html = '<div style="display: flex; flex-direction: column; gap: 8px;">';
                interfaces.forEach((iface, index) => {
                    const isChecked = currentInterfaces.includes(iface) ? 'checked' : '';
                    html += `
                        <label style="display: flex; align-items: center; gap: 8px; cursor: pointer; padding: 8px; border-radius: 4px; background: #2a2a2a; border: 1px solid #444; transition: background 0.2s;" onmouseover="this.style.background='#333'" onmouseout="this.style.background='#2a2a2a'">
                            <input type="checkbox" name="interface_check" value="${iface}" id="interface_${index}" ${isChecked} style="cursor: pointer; width: 16px; height: 16px;">
                            <span style="font-family: monospace; font-weight: 500; color: #fff;">${iface}</span>
                        </label>
                    `;
                });
                html += '</div>';

                interfaceList.innerHTML = html;
            } catch (e) {
                console.error('Failed to load network interfaces:', e);
                interfaceList.innerHTML = '<p style="color: #f44336;">Error loading network interfaces!</p>';
            }
        }

        // Save selected network interfaces
        async function saveSelectedInterfaces() {
            // Get checked interfaces
            const checked = Array.from(document.querySelectorAll('input[name="interface_check"]:checked'))
                .map(cb => cb.value);

            // Get manual input if any
            const manualInput = document.getElementById('interface_manual');
            const manualValue = manualInput ? manualInput.value.trim() : '';

            let interfaces = '';
            if (checked.length > 0) {
                interfaces = checked.join(',');
            } else if (manualValue) {
                interfaces = manualValue;
            } else {
                showMessage('Please select at least one interface or enter manually', 'error');
                return;
            }

            // Save to config
            try {
                const res = await fetch('/api/config', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ field: 'interface', value: interfaces })
                });

                if (res.ok) {
                    flashFieldLabel('interface', 'success');
                    // Reload config to update UI
                    await loadConfig();
                    renderConfig();
                } else {
                    flashFieldLabel('interface', 'error');
                }
            } catch (e) {
                console.error('Failed to save interfaces:', e);
                flashFieldLabel('interface', 'error');
            }
        }

        // Populate gradient dropdown for a specific field
        function populateGradientDropdown(fieldName) {
            const selectId = `${fieldName}_gradient`;
            const select = document.getElementById(selectId);
            if (!select) return;

            const currentValue = config[fieldName] || '';
            let selectedValue = '';

            // Check if this field allows "None" option
            const container = document.getElementById(`${fieldName}_gradient_container`);
            const allowNone = container && container.dataset.allowNone === 'true';

            // Build options
            let options = '';

            // Add "None (Use Default)" option for tx_color/rx_color fields
            if (allowNone) {
                const isNone = currentValue === '';
                options += `<option value="none" ${isNone ? 'selected' : ''}>None (Use Default)</option>`;
                if (isNone) selectedValue = 'none';
            } else {
                options += '<option value="">-- Select Gradient --</option>';
            }

            // Add built-in gradients
            const builtinGradients = Object.keys(allGradients).filter(k => k.startsWith('builtin:'));
            if (builtinGradients.length > 0) {
                options += '<optgroup label="Built-in Gradients">';
                builtinGradients.forEach(key => {
                    const name = key.replace('builtin:', '');
                    const hexColors = allGradients[key];
                    const isSelected = currentValue === name || currentValue === hexColors;
                    if (isSelected) selectedValue = key;
                    options += `<option value="${key}" ${isSelected ? 'selected' : ''}>${name}</option>`;
                });
                options += '</optgroup>';
            }

            // Add custom gradients
            const customGradients = Object.keys(allGradients).filter(k => k.startsWith('custom:'));
            if (customGradients.length > 0) {
                options += '<optgroup label="Custom Gradients">';
                customGradients.forEach(key => {
                    const name = key.replace('custom:', '');
                    const hexColors = allGradients[key];
                    const isSelected = currentValue === name || currentValue === hexColors;
                    if (isSelected) selectedValue = key;
                    options += `<option value="${key}" ${isSelected ? 'selected' : ''}>${name}</option>`;
                });
                options += '</optgroup>';
            }

            // Add custom hex input option
            const isCustom = !selectedValue && currentValue && selectedValue !== 'none';
            options += `<option value="custom" ${isCustom ? 'selected' : ''}>Custom (Enter Hex Colors)</option>`;

            select.innerHTML = options;

            // If custom is selected, show the textarea with current value
            if (isCustom) {
                document.getElementById(`${fieldName}_custom_container`).style.display = 'block';
                document.getElementById(`${fieldName}_custom`).value = currentValue;
                // Auto-resize the textarea to fit content
                setTimeout(() => autoResizeTextarea(`${fieldName}_custom`), 10);
            }

            // Update visibility of delete button and expand button
            handleGradientChange(fieldName);
        }

        // Populate audio device dropdown
        function populateAudioDeviceDropdown() {
            const select = document.getElementById('audio_device');
            if (!select) return;

            const currentValue = config.audio_device || '';

            // Build options HTML
            let optionsHTML = '<option value="">-- Select Audio Device --</option>';
            audioDevices.forEach(device => {
                const selected = device === currentValue ? 'selected' : '';
                optionsHTML += `<option value="${device}" ${selected}>${device}</option>`;
            });

            select.innerHTML = optionsHTML;
        }

        // Handle gradient dropdown change
        function handleGradientChange(fieldName) {
            const selectId = `${fieldName}_gradient`;
            const select = document.getElementById(selectId);
            const selectedValue = select.value;

            const customContainer = document.getElementById(`${fieldName}_custom_container`);
            const deleteContainer = document.getElementById(`${selectId}_delete`);
            const expandContainer = document.getElementById(`${selectId}_expand`);

            if (selectedValue === 'custom') {
                customContainer.style.display = 'block';
                deleteContainer.style.display = 'none';
                // Show expand button when custom textarea is shown
                if (expandContainer) expandContainer.style.display = 'block';

                // Auto-resize the textarea after it becomes visible
                // Use requestAnimationFrame to ensure DOM has updated
                requestAnimationFrame(() => {
                    setTimeout(() => autoResizeTextarea(`${fieldName}_custom`), 0);
                });
            } else if (selectedValue.startsWith('custom:')) {
                customContainer.style.display = 'none';
                deleteContainer.style.display = 'block';
                if (expandContainer) expandContainer.style.display = 'none';
            } else {
                customContainer.style.display = 'none';
                deleteContainer.style.display = 'none';
                if (expandContainer) expandContainer.style.display = 'none';
            }
        }

        // Save gradient field value
        async function saveGradientField(fieldName) {
            const selectId = `${fieldName}_gradient`;
            const select = document.getElementById(selectId);
            const selectedValue = select.value;

            let value;

            if (selectedValue === 'none') {
                // "None (Use Default)" - save empty string
                value = '';
            } else if (selectedValue === 'custom') {
                // Use custom hex colors from textarea
                const customTextarea = document.getElementById(`${fieldName}_custom`);
                value = customTextarea.value.trim();
            } else if (selectedValue) {
                // Use gradient name (remove builtin: or custom: prefix)
                value = selectedValue.replace(/^(builtin|custom):/, '');
            } else {
                showMessage('Please select a gradient or enter custom colors', 'error');
                return;
            }

            try {
                const res = await fetch('/api/config', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ field: fieldName, value })
                });

                if (res.ok) {
                    flashFieldLabel(fieldName, 'success');
                    config[fieldName] = value;
                } else {
                    flashFieldLabel(fieldName, 'error');
                }
            } catch (e) {
                flashFieldLabel(fieldName, 'error');
            }
        }

        // Save custom gradient with a name
        async function saveCustomGradient(fieldName) {
            const customTextarea = document.getElementById(`${fieldName}_custom`);
            const hexColors = customTextarea.value.trim();

            if (!hexColors) {
                showMessage('Please enter hex colors', 'error');
                return;
            }

            const name = prompt('Enter a name for this custom gradient (letters, numbers, and underscores only):');
            if (!name) return;

            // Sanitize name on client side too
            const sanitizedName = name.replace(/[^a-zA-Z0-9_]/g, '');
            if (!sanitizedName) {
                showMessage('Invalid gradient name', 'error');
                return;
            }

            try {
                const res = await fetch('/api/gradients/save', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ name: sanitizedName, hex_colors: hexColors })
                });

                if (res.ok) {
                    showMessage(`Custom gradient "${sanitizedName}" saved successfully`, 'success');
                    // Reload gradients and update dropdown
                    await loadGradients();
                    populateGradientDropdown(fieldName);
                    // Auto-select the newly saved gradient
                    const select = document.getElementById(`${fieldName}_gradient`);
                    select.value = `custom:${sanitizedName}`;
                    handleGradientChange(fieldName);
                } else {
                    const errorText = await res.text();
                    showMessage(`Failed to save gradient: ${errorText}`, 'error');
                }
            } catch (e) {
                showMessage('Error saving gradient', 'error');
            }
        }

        // Delete selected custom gradient
        async function deleteSelectedGradient(fieldName) {
            const selectId = `${fieldName}_gradient`;
            const select = document.getElementById(selectId);
            const selectedValue = select.value;

            if (!selectedValue.startsWith('custom:')) {
                showMessage('Can only delete custom gradients', 'error');
                return;
            }

            const gradientName = selectedValue.replace('custom:', '');

            if (!confirm(`Are you sure you want to delete the custom gradient "${gradientName}"?`)) {
                return;
            }

            try {
                const res = await fetch('/api/gradients/delete', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ name: gradientName })
                });

                if (res.ok) {
                    showMessage(`Custom gradient "${gradientName}" deleted successfully`, 'success');
                    // Reload gradients and update dropdown
                    await loadGradients();
                    populateGradientDropdown(fieldName);
                } else {
                    const errorText = await res.text();
                    showMessage(`Failed to delete gradient: ${errorText}`, 'error');
                }
            } catch (e) {
                showMessage('Error deleting gradient', 'error');
            }
        }

        // Auto-resize textarea to fit content
        function autoResizeTextarea(fieldId) {
            const textarea = document.getElementById(fieldId);
            if (!textarea) return;

            // Check if textarea is actually visible before resizing
            if (textarea.offsetParent === null) {
                // Not visible yet, try again shortly
                setTimeout(() => autoResizeTextarea(fieldId), 50);
                return;
            }

            // Reset height to minimum to get accurate scrollHeight
            textarea.style.height = '40px';

            // Force a reflow to ensure accurate measurement
            textarea.scrollHeight;

            // Calculate new height based on content
            // Add buffer (8px) to ensure all content is visible and account for any rounding
            const newHeight = Math.max(40, textarea.scrollHeight + 8);
            textarea.style.height = newHeight + 'px';
        }

        // Expand gradient name to hex colors (client-side only, doesn't save)
        function expandGradientName(fieldName) {
            const customTextarea = document.getElementById(`${fieldName}_custom`);
            const textareaValue = customTextarea.value.trim();

            if (!textareaValue) {
                showMessage('Please enter a gradient name in the custom colors field', 'error');
                return;
            }

            // Check if the value is already hex colors (contains comma)
            if (textareaValue.includes(',')) {
                showMessage('This already appears to be hex colors', 'error');
                return;
            }

            // Try to find this gradient name in our gradients
            let hexColors = null;

            // Check built-in gradients
            for (const key of Object.keys(allGradients)) {
                if (key.startsWith('builtin:')) {
                    const name = key.replace('builtin:', '');
                    if (name.toLowerCase() === textareaValue.toLowerCase()) {
                        hexColors = allGradients[key];
                        break;
                    }
                }
            }

            // If not found in built-in, check custom gradients
            if (!hexColors) {
                for (const key of Object.keys(allGradients)) {
                    if (key.startsWith('custom:')) {
                        const name = key.replace('custom:', '');
                        if (name.toLowerCase() === textareaValue.toLowerCase()) {
                            hexColors = allGradients[key];
                            break;
                        }
                    }
                }
            }

            if (!hexColors) {
                showMessage(`Gradient "${textareaValue}" not found. Please enter a valid gradient name.`, 'error');
                return;
            }

            // Just update the textarea with the hex colors - don't save
            customTextarea.value = hexColors;

            // Auto-resize the textarea (with small delay to ensure DOM updates)
            setTimeout(() => autoResizeTextarea(`${fieldName}_custom`), 10);

            showMessage(`Gradient expanded to hex colors (not saved yet)`, 'success');
        }

        function updateBrightnessDisplay(value) {
            document.getElementById('global-brightness-value').textContent = `${value}%`;
        }

        async function saveBrightness(value) {
            const brightnessValue = parseFloat(value) / 100.0;  // Convert 0-100 to 0.0-1.0
            try {
                const res = await fetch('/api/config', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ field: 'global_brightness', value: brightnessValue })
                });

                if (res.ok) {
                    showMessage(`Brightness set to ${value}%`, 'success');
                } else {
                    showMessage('Failed to update brightness', 'error');
                }
            } catch (e) {
                showMessage('Network error updating brightness', 'error');
            }
        }

        async function saveField(fieldName, fieldType) {
            let value;

            if (fieldType === 'checkbox') {
                const input = document.getElementById(fieldName);
                // Check if it's a hidden input (from toggle button) or actual checkbox
                if (input.type === 'hidden') {
                    value = input.value === 'true';
                } else {
                    value = input.checked;
                }
            } else if (fieldType === 'radio') {
                const selectedRadio = document.querySelector(`input[name="${fieldName}"]:checked`);
                value = selectedRadio ? selectedRadio.value : null;
            } else if (fieldType === 'range') {
                const input = document.getElementById(fieldName);
                value = parseFloat(input.value);
            } else if (fieldType === 'number') {
                const input = document.getElementById(fieldName);
                value = parseFloat(input.value);
            } else {
                const input = document.getElementById(fieldName);
                value = input.value;
            }

            try {
                const res = await fetch('/api/config', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ field: fieldName, value })
                });

                if (res.ok) {
                    // Update local config
                    config[fieldName] = value;

                    // Flash label green for success
                    flashFieldLabel(fieldName, 'success');

                    // If gradient blending is turned off, automatically turn off intensity colors
                    if (fieldName === 'use_gradient' && value === false) {
                        const intensityColorsInput = document.getElementById('intensity_colors');
                        if (intensityColorsInput && intensityColorsInput.checked) {
                            intensityColorsInput.checked = false;
                            // Also save the change to the backend
                            await fetch('/api/config', {
                                method: 'POST',
                                headers: { 'Content-Type': 'application/json' },
                                body: JSON.stringify({ field: 'intensity_colors', value: false })
                            });
                            config.intensity_colors = false;
                        }
                        renderConfig(); // Re-render to hide intensity_colors checkbox
                    }

                    // If mode changed, update URL and re-render
                    if (fieldName === 'mode') {
                        // Update browser URL to match mode
                        const newPath = `/${value}`;
                        if (window.location.pathname !== newPath) {
                            window.history.pushState({}, '', newPath);
                        }
                        updateModeStatus();
                        renderConfig();
                    } else if (fieldName === 'vu') {
                        // VU mode affects visibility of sections (like strobe), re-render
                        updateModeStatus();
                        renderConfig();
                    } else if (fieldName === 'tron_num_players' || fieldName === 'tron_food_mode' || fieldName === 'matrix_2d_enabled' || fieldName === 'geometry_mode_select' || fieldName === 'boid_predator_enabled') {
                        // These fields affect visibility of other fields, re-render
                        renderConfig();
                    }

                    // If strobe_rate_hz changed, revalidate strobe_duration_ms
                    if (fieldName === 'strobe_rate_hz') {
                        validateStrobeDuration();
                    }
                } else {
                    flashFieldLabel(fieldName, 'error');
                }
            } catch (e) {
                flashFieldLabel(fieldName, 'error');
            }
        }

        async function adjustValue(fieldName, delta, min, max) {
            const input = document.getElementById(fieldName);
            let currentValue = parseInt(input.value) || min;
            let newValue = currentValue + delta;

            // Clamp to min/max
            newValue = Math.max(min, Math.min(max, newValue));

            input.value = newValue;

            // Auto-save the new value
            await saveField(fieldName, 'number');
        }

        async function toggleField(fieldName) {
            const input = document.getElementById(fieldName);
            const button = document.getElementById(fieldName + '_button');

            // Toggle the value
            const currentValue = input.value === 'true';
            const newValue = !currentValue;

            input.value = String(newValue);

            // Update button appearance
            button.textContent = newValue ? 'ENABLED' : 'DISABLED';
            button.style.backgroundColor = newValue ? '#4caf50' : '#757575';

            // Auto-save the new value
            await saveField(fieldName, 'checkbox');  // Use checkbox type for boolean handling
        }

        async function triggerAction(actionName) {
            try {
                const res = await fetch('/api/action', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ action: actionName })
                });

                if (res.ok) {
                    showMessage(`Action "${actionName}" triggered successfully`, 'success');
                } else {
                    showMessage(`Failed to trigger action "${actionName}"`, 'error');
                }
            } catch (e) {
                showMessage('Network error triggering action', 'error');
            }
        }

        function flashFieldLabel(fieldName, status) {
            // Find the label for this field
            const label = document.querySelector(`label[for="${fieldName}"]`);
            if (!label) return;

            // Save original color
            const originalColor = label.style.color || '';

            // Set color based on status
            label.style.color = status === 'success' ? '#00ff00' : '#ff0000';
            label.style.transition = 'color 0.2s';

            // Reset after 3 seconds
            setTimeout(() => {
                label.style.color = originalColor;
            }, 3000);
        }

        async function saveGroup(fieldNamesStr) {
            const fieldNames = fieldNamesStr.split(',');

            try {
                // Save all fields in the group
                for (const fieldName of fieldNames) {
                    let value;

                    // Special handling for interface field (uses checkboxes)
                    if (fieldName === 'interface') {
                        // Get checked interfaces
                        const checked = Array.from(document.querySelectorAll('input[name="interface_check"]:checked'))
                            .map(cb => cb.value);

                        // Get manual input if any
                        const manualInput = document.getElementById('interface_manual');
                        const manualValue = manualInput ? manualInput.value.trim() : '';

                        if (checked.length > 0) {
                            value = checked.join(',');
                        } else if (manualValue) {
                            value = manualValue;
                        } else {
                            // If no interface selected, use empty string
                            value = '';
                        }
                    } else {
                        // Regular fields - get value from input
                        const input = document.getElementById(fieldName);
                        if (!input) {
                            value = '';
                        } else if (input.type === 'number') {
                            // Parse number inputs as numbers
                            value = parseFloat(input.value);
                        } else if (input.type === 'checkbox') {
                            value = input.checked;
                        } else if (input.type === 'hidden' && document.getElementById(fieldName + '_button')) {
                            // Hidden input from toggle button
                            value = input.value === 'true';
                        } else {
                            value = input.value;
                        }
                    }

                    const res = await fetch('/api/config', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ field: fieldName, value })
                    });

                    if (!res.ok) {
                        flashFieldLabel(fieldName, 'error');
                        return;
                    }

                    config[fieldName] = value;
                }

                // Flash all fields green on success
                for (const fieldName of fieldNames) {
                    flashFieldLabel(fieldName, 'success');
                }

                // Reload config and re-render to update the UI
                await loadConfig();
                renderConfig();
            } catch (e) {
                // Flash all fields red on error
                for (const fieldName of fieldNames) {
                    flashFieldLabel(fieldName, 'error');
                }
            }
        }

        function showMessage(text, type, duration = 3000) {
            const msg = document.getElementById('message');
            msg.textContent = text;
            msg.className = `message ${type} show`;
            setTimeout(() => msg.className = 'message', duration);
        }

        // Validate that all config fields are present in the Web UI
        async function validateWebUIFields() {
            try {
                const res = await fetch('/api/config/fields');
                const allFields = await res.json();

                // Extract all field names from fieldSections
                const webUIFields = new Set();
                fieldSections.forEach(section => {
                    if (section.fields) {
                        section.fields.forEach(field => {
                            webUIFields.add(field.name);
                        });
                    }
                    if (section.groupFields) {
                        section.groupFields.forEach(field => {
                            webUIFields.add(field.name);
                        });
                    }
                });

                // Check for missing fields
                const missingFields = allFields.filter(field => !webUIFields.has(field));

                if (missingFields.length > 0) {
                    console.warn('⚠️  Web UI is missing %d config field(s):', missingFields.length, missingFields);
                    console.warn('Please add these fields to the fieldSections array in the Web UI');
                } else {
                    console.log('✓ All %d config fields are present in Web UI', allFields.length);
                }
            } catch (e) {
                console.error('Failed to validate Web UI fields:', e);
            }
        }

        // Multi-device management functions
        async function addDevice() {
            try {
                const res = await fetch('/api/devices/add', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({
                        ip: '192.168.1.100',
                        led_offset: 0,
                        led_count: 50,
                        enabled: true
                    })
                });

                if (res.ok) {
                    await loadConfig();
                    showMessage('Device added successfully', 'success');
                } else {
                    showMessage('Failed to add device', 'error');
                }
            } catch (e) {
                console.error('Failed to add device:', e);
                showMessage('Error adding device', 'error');
            }
        }

        async function removeDevice(index) {
            if (!confirm('Remove this device?')) return;

            try {
                const res = await fetch('/api/devices/remove', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ index })
                });

                if (res.ok) {
                    await loadConfig();
                    showMessage('Device removed successfully', 'success');
                } else {
                    showMessage('Failed to remove device', 'error');
                }
            } catch (e) {
                console.error('Failed to remove device:', e);
                showMessage('Error removing device', 'error');
            }
        }

        async function updateDevice(index, field, value) {
            try {
                const res = await fetch('/api/devices/update', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ index, field, value })
                });

                if (res.ok) {
                    // Update local config without full reload
                    config.wled_devices[index][field] = value;
                    showMessage('Device updated', 'success', 1500);
                } else {
                    showMessage('Failed to update device', 'error');
                }
            } catch (e) {
                console.error('Failed to update device:', e);
                showMessage('Error updating device', 'error');
            }
        }

        async function toggleDevice(index) {
            const device = config.wled_devices[index];
            await updateDevice(index, 'enabled', !device.enabled);
            await loadConfig(); // Reload to update UI state
        }

        async function updateConfigField(fieldName, value) {
            try {
                const res = await fetch('/api/config', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ field: fieldName, value })
                });

                if (res.ok) {
                    config[fieldName] = value;
                    showMessage('Settings updated', 'success', 1500);
                } else {
                    showMessage('Failed to update settings', 'error');
                }
            } catch (e) {
                console.error('Failed to update config field:', e);
                showMessage('Error updating settings', 'error');
            }
        }

        // Initial load
        async function initializePage() {
            // Check if URL path indicates a specific mode
            const path = window.location.pathname;
            const modeFromPath = path.substring(1); // Remove leading slash

            // Valid modes
            const validModes = ['bandwidth', 'audio', 'webcam', 'midi', 'relay'];

            // If path matches a mode, set it before loading config
            if (validModes.includes(modeFromPath)) {
                try {
                    await fetch('/api/config', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ field: 'mode', value: modeFromPath })
                    });
                } catch (e) {
                    console.error('Failed to set mode from URL:', e);
                }
            }

            await loadGradients();
            await loadAudioDevices();
            await loadConfig();
            validateWebUIFields();
        }
        initializePage();

        // Setup Server-Sent Events (SSE) for real-time config updates
        let eventSource = null;
        let usePolling = false;

        function setupSSE() {
            try {
                eventSource = new EventSource('/api/config/events');

                eventSource.addEventListener('config-changed', function(e) {
                    // Don't reload config if webcam is actively streaming
                    if (!webcamWs || webcamWs.readyState !== WebSocket.OPEN) {
                        loadConfig();
                    } else {
                        // Just update the local config variable without re-rendering
                        fetch('/api/config')
                            .then(res => res.json())
                            .then(newConfig => { config = newConfig; });
                    }
                });

                eventSource.onerror = function(e) {
                    console.error('SSE error, falling back to polling:', e);
                    eventSource.close();
                    if (!usePolling) {
                        usePolling = true;
                        pollingInterval = setInterval(loadConfig, 5000);
                    }
                };

                console.log('✓ SSE connection established for real-time config updates');
            } catch (e) {
                console.error('Failed to setup SSE, using polling fallback:', e);
                usePolling = true;
                pollingInterval = setInterval(loadConfig, 5000);
            }
        }

        // Toggle WLED liveview iframe visibility
        let liveviewVisible = true; // Will be toggled to false on page load
        function toggleLiveview() {
            const iframe = document.getElementById('wled-liveview');
            const toggle = document.getElementById('liveview-toggle');
            const icon = document.getElementById('liveview-toggle-icon');

            liveviewVisible = !liveviewVisible;

            if (liveviewVisible) {
                // Slide iframe and toggle down to show
                iframe.style.top = '0';
                toggle.style.top = '10px';
                icon.textContent = '▼';
            } else {
                // Slide iframe and toggle up to hide
                iframe.style.top = '-10px';
                toggle.style.top = '0';
                icon.textContent = '▶';
            }
        }

        // Initialize liveview as hidden to save CPU
        document.addEventListener('DOMContentLoaded', function() {
            toggleLiveview(); // Hide by default
        });

        // Shutdown application with confirmation
        async function shutdownApp() {
            const confirmed = confirm(
                '⚠️ WARNING: This will immediately terminate the entire application.\n\n' +
                'All visualization modes will stop and the program will exit.\n\n' +
                'Are you absolutely sure you want to shut down?'
            );

            if (!confirmed) {
                return;
            }

            // Double confirmation for safety
            const doubleConfirmed = confirm(
                '🛑 FINAL CONFIRMATION\n\n' +
                'This action cannot be undone. The application will shut down immediately.\n\n' +
                'Click OK to shut down, or Cancel to abort.'
            );

            if (!doubleConfirmed) {
                return;
            }

            try {
                const res = await fetch('/api/shutdown', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' }
                });

                if (res.ok) {
                    showMessage('Application shutting down...', 'success');
                    // Update page to show shutdown message
                    document.body.innerHTML = '<div style="display: flex; justify-content: center; align-items: center; height: 100vh; flex-direction: column;">' +
                        '<h1 style="color: #ff6666;">🛑 Application Shutting Down</h1>' +
                        '<p style="color: #b0b0b0; margin-top: 20px;">The application has been terminated.</p>' +
                        '<p style="color: #808080; margin-top: 10px;">You can close this window.</p>' +
                        '</div>';
                } else {
                    showMessage('Failed to shutdown application', 'error');
                }
            } catch (e) {
                // Connection lost means shutdown was successful
                showMessage('Application shut down successfully', 'success');
                setTimeout(() => {
                    document.body.innerHTML = '<div style="display: flex; justify-content: center; align-items: center; height: 100vh; flex-direction: column;">' +
                        '<h1 style="color: #ff6666;">🛑 Application Shut Down</h1>' +
                        '<p style="color: #b0b0b0; margin-top: 20px;">The application has been terminated.</p>' +
                        '<p style="color: #808080; margin-top: 10px;">You can close this window.</p>' +
                        '</div>';
                }, 1000);
            }
        }

        setupSSE();
    </script>
</body>
</html>
"#;


#[derive(Deserialize)]
struct UpdateField {
    field: String,
    value: serde_json::Value,
}

async fn serve_index() -> impl IntoResponse {
    Html(WEB_UI_HTML)
}

async fn get_config() -> impl IntoResponse {
    match BandwidthConfig::load() {
        Ok(config) => (StatusCode::OK, Json(config)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_all_fields() -> impl IntoResponse {
    match BandwidthConfig::load() {
        Ok(config) => (StatusCode::OK, Json(config)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn update_config(
    State(config_tx): State<broadcast::Sender<()>>,
    Json(payload): Json<UpdateField>,
) -> impl IntoResponse {
    let mut config = match BandwidthConfig::load() {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let result = match payload.field.as_str() {
        "max_gbps" => payload.value.as_f64().map(|v| { config.max_gbps = v; }).ok_or("Invalid value"),
        "color" => payload.value.as_str().map(|v| { config.color = v.to_string(); }).ok_or("Invalid value"),
        "tx_color" => payload.value.as_str().map(|v| { config.tx_color = v.to_string(); }).ok_or("Invalid value"),
        "rx_color" => payload.value.as_str().map(|v| { config.rx_color = v.to_string(); }).ok_or("Invalid value"),
        "direction" => payload.value.as_str().map(|v| { config.direction = v.to_string(); }).ok_or("Invalid value"),
        "swap" => payload.value.as_bool().map(|v| { config.swap = v; }).ok_or("Invalid value"),
        "rx_split_percent" => payload.value.as_f64().map(|v| { config.rx_split_percent = v.clamp(0.0, 100.0); }).ok_or("Invalid value"),
        "strobe_on_max" => payload.value.as_bool().map(|v| { config.strobe_on_max = v; }).ok_or("Invalid value"),
        "strobe_rate_hz" => payload.value.as_f64().map(|v| {
            config.strobe_rate_hz = v;
            if config.strobe_rate_hz > 0.0 {
                let max_duration = 1000.0 / config.strobe_rate_hz;
                config.strobe_duration_ms = config.strobe_duration_ms.min(max_duration);
            }
        }).ok_or("Invalid value"),
        "strobe_duration_ms" => payload.value.as_f64().map(|v| {
            let max_duration = if config.strobe_rate_hz > 0.0 {
                1000.0 / config.strobe_rate_hz
            } else {
                1000.0
            };
            config.strobe_duration_ms = v.max(0.0).min(max_duration);
        }).ok_or("Invalid value"),
        "strobe_color" => payload.value.as_str().map(|v| { config.strobe_color = v.to_string(); }).ok_or("Invalid value"),
        "animation_speed" => payload.value.as_f64().map(|v| { config.animation_speed = v; }).ok_or("Invalid value"),
        "scale_animation_speed" => payload.value.as_bool().map(|v| { config.scale_animation_speed = v; }).ok_or("Invalid value"),
        "tx_animation_direction" => payload.value.as_str().map(|v| { config.tx_animation_direction = v.to_string(); }).ok_or("Invalid value"),
        "rx_animation_direction" => payload.value.as_str().map(|v| { config.rx_animation_direction = v.to_string(); }).ok_or("Invalid value"),
        "interpolation_time_ms" => payload.value.as_f64().map(|v| { config.interpolation_time_ms = v; }).ok_or("Invalid value"),
        "enable_interpolation" => payload.value.as_bool().map(|v| { config.enable_interpolation = v; }).ok_or("Invalid value"),
        "wled_ip" => payload.value.as_str().map(|v| { config.wled_ip = v.to_string(); }).ok_or("Invalid value"),
        "interface" => payload.value.as_str().map(|v| { config.interface = v.to_string(); }).ok_or("Invalid value"),
        "ssh_host" => payload.value.as_str().map(|v| { config.ssh_host = v.to_string(); }).ok_or("Invalid value"),
        "ssh_user" => payload.value.as_str().map(|v| { config.ssh_user = v.to_string(); }).ok_or("Invalid value"),
        "total_leds" => payload.value.as_u64().map(|v| { config.total_leds = v as usize; }).ok_or("Invalid value"),
        "use_gradient" => payload.value.as_bool().map(|v| { config.use_gradient = v; }).ok_or("Invalid value"),
        "intensity_colors" => payload.value.as_bool().map(|v| { config.intensity_colors = v; }).ok_or("Invalid value"),
        "interpolation" => payload.value.as_str().map(|v| { config.interpolation = v.to_string(); }).ok_or("Invalid value"),
        "fps" => payload.value.as_f64().map(|v| {
            config.fps = v;
            println!("✓ FPS updated to {} (will save to config file)", v);
        }).ok_or("Invalid value"),
        "ddp_delay_ms" => payload.value.as_f64().map(|v| { config.ddp_delay_ms = v.max(0.0); }).ok_or("Invalid value"),
        "global_brightness" => payload.value.as_f64().map(|v| { config.global_brightness = v.max(0.0).min(1.0); }).ok_or("Invalid value"),
        "mode" => payload.value.as_str().map(|v| { config.mode = v.to_string(); }).ok_or("Invalid value"),
        "httpd_enabled" => payload.value.as_bool().map(|v| { config.httpd_enabled = v; }).ok_or("Invalid value"),
        "httpd_https_enabled" => payload.value.as_bool().map(|v| { config.httpd_https_enabled = v; }).ok_or("Invalid value"),
        "httpd_ip" => payload.value.as_str().map(|v| { config.httpd_ip = v.to_string(); }).ok_or("Invalid value"),
        "httpd_port" => payload.value.as_u64().map(|v| { config.httpd_port = v as u16; }).ok_or("Invalid value"),
        "midi_device" => payload.value.as_str().map(|v| { config.midi_device = v.to_string(); }).ok_or("Invalid value"),
        "midi_gradient" => payload.value.as_bool().map(|v| { config.midi_gradient = v; }).ok_or("Invalid value"),
        "midi_random_colors" => payload.value.as_bool().map(|v| { config.midi_random_colors = v; }).ok_or("Invalid value"),
        "midi_velocity_colors" => payload.value.as_bool().map(|v| { config.midi_velocity_colors = v; }).ok_or("Invalid value"),
        "midi_one_to_one" => payload.value.as_bool().map(|v| { config.midi_one_to_one = v; }).ok_or("Invalid value"),
        "midi_channel_mode" => payload.value.as_bool().map(|v| { config.midi_channel_mode = v; }).ok_or("Invalid value"),
        "audio_device" => payload.value.as_str().map(|v| { config.audio_device = v.to_string(); }).ok_or("Invalid value"),
        "audio_gain" => payload.value.as_f64().map(|v| { config.audio_gain = v.clamp(-200.0, 200.0); }).ok_or("Invalid value"),
        "attack_ms" => payload.value.as_f64().map(|v| { config.attack_ms = v as f32; }).ok_or("Invalid value"),
        "decay_ms" => payload.value.as_f64().map(|v| { config.decay_ms = v as f32; }).ok_or("Invalid value"),
        "log_scale" => payload.value.as_bool().map(|v| { config.log_scale = v; }).ok_or("Invalid value"),
        "vu" => payload.value.as_bool().map(|v| { config.vu = v; }).ok_or("Invalid value"),
        "peak_hold" => payload.value.as_bool().map(|v| { config.peak_hold = v; }).ok_or("Invalid value"),
        "peak_hold_duration_ms" => payload.value.as_f64().map(|v| { config.peak_hold_duration_ms = v; }).ok_or("Invalid value"),
        "peak_hold_color" => payload.value.as_str().map(|v| { config.peak_hold_color = v.to_string(); }).ok_or("Invalid value"),
        "peak_direction_toggle" => payload.value.as_bool().map(|v| { config.peak_direction_toggle = v; }).ok_or("Invalid value"),
        "spectrogram" => payload.value.as_bool().map(|v| {
            config.spectrogram = v;
            // Spectrogram requires 2D matrix mode
            if v {
                config.matrix_2d_enabled = true;
                // Auto-calculate good matrix dimensions if not already set
                // Try to make it roughly square, favoring wider (more time history)
                if config.matrix_2d_width * config.matrix_2d_height != config.total_leds {
                    let sqrt = (config.total_leds as f64).sqrt() as usize;
                    config.matrix_2d_width = sqrt;
                    config.matrix_2d_height = config.total_leds / sqrt;
                }
            }
        }).ok_or("Invalid value"),
        "spectrogram_scroll_direction" => payload.value.as_str().map(|v| { config.spectrogram_scroll_direction = v.to_string(); }).ok_or("Invalid value"),
        "spectrogram_scroll_speed" => payload.value.as_f64().map(|v| { config.spectrogram_scroll_speed = v.max(1.0); }).ok_or("Invalid value"),
        "spectrogram_window_size" => {
            // Radio buttons send string values, parse to number
            if let Some(s) = payload.value.as_str() {
                if let Ok(v) = s.parse::<usize>() {
                    config.spectrogram_window_size = v;
                    Ok(())
                } else {
                    Err("Invalid value")
                }
            } else if let Some(v) = payload.value.as_u64() {
                config.spectrogram_window_size = v as usize;
                Ok(())
            } else {
                Err("Invalid value")
            }
        },
        "spectrogram_color_mode" => payload.value.as_str().map(|v| { config.spectrogram_color_mode = v.to_string(); }).ok_or("Invalid value"),
        "matrix_2d_enabled" => payload.value.as_bool().map(|v| { config.matrix_2d_enabled = v; }).ok_or("Invalid value"),
        "matrix_2d_width" => payload.value.as_u64().map(|v| { config.matrix_2d_width = v as usize; }).ok_or("Invalid value"),
        "matrix_2d_height" => payload.value.as_u64().map(|v| { config.matrix_2d_height = v as usize; }).ok_or("Invalid value"),
        "matrix_2d_gradient_direction" => payload.value.as_str().map(|v| { config.matrix_2d_gradient_direction = v.to_string(); }).ok_or("Invalid value"),
        "test_tx" => payload.value.as_bool().map(|v| { config.test_tx = v; }).ok_or("Invalid value"),
        "test_rx" => payload.value.as_bool().map(|v| { config.test_rx = v; }).ok_or("Invalid value"),
        "test_tx_percent" => payload.value.as_f64().map(|v| { config.test_tx_percent = v.clamp(0.0, 101.0); }).ok_or("Invalid value"),
        "test_rx_percent" => payload.value.as_f64().map(|v| { config.test_rx_percent = v.clamp(0.0, 101.0); }).ok_or("Invalid value"),
        "relay_listen_ip" => payload.value.as_str().map(|v| { config.relay_listen_ip = v.to_string(); }).ok_or("Invalid value"),
        "relay_listen_port" => payload.value.as_u64().map(|v| { config.relay_listen_port = v as u16; }).ok_or("Invalid value"),
        "relay_frame_width" => payload.value.as_u64().map(|v| { config.relay_frame_width = v as usize; }).ok_or("Invalid value"),
        "relay_frame_height" => payload.value.as_u64().map(|v| { config.relay_frame_height = v as usize; }).ok_or("Invalid value"),
        "webcam_frame_width" => payload.value.as_u64().map(|v| { config.webcam_frame_width = v as usize; }).ok_or("Invalid value"),
        "webcam_frame_height" => payload.value.as_u64().map(|v| { config.webcam_frame_height = v as usize; }).ok_or("Invalid value"),
        "webcam_target_fps" => payload.value.as_f64().map(|v| { config.webcam_target_fps = v; }).ok_or("Invalid value"),
        "webcam_brightness" => payload.value.as_f64().map(|v| { config.webcam_brightness = v.clamp(0.0, 2.0); }).ok_or("Invalid value"),
        "tron_width" => payload.value.as_u64().map(|v| { config.tron_width = v as usize; }).ok_or("Invalid value"),
        "tron_height" => payload.value.as_u64().map(|v| { config.tron_height = v as usize; }).ok_or("Invalid value"),
        "tron_speed_ms" => payload.value.as_f64().map(|v| { config.tron_speed_ms = v; }).ok_or("Invalid value"),
        "tron_reset_delay_ms" => payload.value.as_u64().map(|v| { config.tron_reset_delay_ms = v; }).ok_or("Invalid value"),
        "tron_look_ahead" => payload.value.as_i64().map(|v| { config.tron_look_ahead = v as i32; }).ok_or("Invalid value"),
        "tron_trail_length" => payload.value.as_u64().map(|v| { config.tron_trail_length = v as usize; }).ok_or("Invalid value"),
        "tron_ai_aggression" => payload.value.as_f64().map(|v| { config.tron_ai_aggression = v.clamp(0.0, 1.0); }).ok_or("Invalid value"),
        "tron_num_players" => payload.value.as_u64().map(|v| { config.tron_num_players = v as usize; }).ok_or("Invalid value"),
        "tron_food_mode" => payload.value.as_bool().map(|v| { config.tron_food_mode = v; }).ok_or("Invalid value"),
        "tron_food_max_count" => payload.value.as_u64().map(|v| { config.tron_food_max_count = v as usize; }).ok_or("Invalid value"),
        "tron_food_ttl_seconds" => payload.value.as_u64().map(|v| { config.tron_food_ttl_seconds = v; }).ok_or("Invalid value"),
        "tron_super_food_enabled" => payload.value.as_bool().map(|v| { config.tron_super_food_enabled = v; }).ok_or("Invalid value"),
        "tron_power_food_enabled" => payload.value.as_bool().map(|v| { config.tron_power_food_enabled = v; }).ok_or("Invalid value"),
        "tron_diagonal_movement" => payload.value.as_bool().map(|v| { config.tron_diagonal_movement = v; }).ok_or("Invalid value"),
        "tron_trail_fade" => payload.value.as_bool().map(|v| { config.tron_trail_fade = v; }).ok_or("Invalid value"),
        "tron_player_colors" => payload.value.as_str().map(|v| { config.tron_player_colors = v.to_string(); }).ok_or("Invalid value"),
        "tron_player_1_color" => payload.value.as_str().map(|v| { config.tron_player_1_color = v.to_string(); }).ok_or("Invalid value"),
        "tron_player_2_color" => payload.value.as_str().map(|v| { config.tron_player_2_color = v.to_string(); }).ok_or("Invalid value"),
        "tron_player_3_color" => payload.value.as_str().map(|v| { config.tron_player_3_color = v.to_string(); }).ok_or("Invalid value"),
        "tron_player_4_color" => payload.value.as_str().map(|v| { config.tron_player_4_color = v.to_string(); }).ok_or("Invalid value"),
        "tron_player_5_color" => payload.value.as_str().map(|v| { config.tron_player_5_color = v.to_string(); }).ok_or("Invalid value"),
        "tron_player_6_color" => payload.value.as_str().map(|v| { config.tron_player_6_color = v.to_string(); }).ok_or("Invalid value"),
        "tron_player_7_color" => payload.value.as_str().map(|v| { config.tron_player_7_color = v.to_string(); }).ok_or("Invalid value"),
        "tron_player_8_color" => payload.value.as_str().map(|v| { config.tron_player_8_color = v.to_string(); }).ok_or("Invalid value"),
        "tron_animation_speed" => payload.value.as_f64().map(|v| { config.tron_animation_speed = v.max(0.0); }).ok_or("Invalid value"),
        "tron_scale_animation_speed" => payload.value.as_bool().map(|v| { config.tron_scale_animation_speed = v; }).ok_or("Invalid value"),
        "tron_animation_direction" => payload.value.as_str().map(|v| { config.tron_animation_direction = v.to_string(); }).ok_or("Invalid value"),
        "tron_flip_direction_on_food" => payload.value.as_bool().map(|v| { config.tron_flip_direction_on_food = v; }).ok_or("Invalid value"),
        "tron_interpolation" => payload.value.as_str().map(|v| { config.tron_interpolation = v.to_string(); }).ok_or("Invalid value"),
        "geometry_grid_width" => payload.value.as_u64().map(|v| { config.geometry_grid_width = v as usize; }).ok_or("Invalid value"),
        "geometry_grid_height" => payload.value.as_u64().map(|v| { config.geometry_grid_height = v as usize; }).ok_or("Invalid value"),
        "geometry_mode_select" => payload.value.as_str().map(|v| { config.geometry_mode_select = v.to_string(); }).ok_or("Invalid value"),
        "geometry_mode_duration_seconds" => payload.value.as_f64().map(|v| { config.geometry_mode_duration_seconds = v.max(1.0); }).ok_or("Invalid value"),
        "geometry_randomize_order" => payload.value.as_bool().map(|v| { config.geometry_randomize_order = v; }).ok_or("Invalid value"),
        "boid_count" => payload.value.as_u64().map(|v| { config.boid_count = (v as usize).clamp(1, 200); }).ok_or("Invalid value"),
        "boid_separation_distance" => payload.value.as_f64().map(|v| { config.boid_separation_distance = v.clamp(0.01, 0.5); }).ok_or("Invalid value"),
        "boid_alignment_distance" => payload.value.as_f64().map(|v| { config.boid_alignment_distance = v.clamp(0.01, 1.0); }).ok_or("Invalid value"),
        "boid_cohesion_distance" => payload.value.as_f64().map(|v| { config.boid_cohesion_distance = v.clamp(0.01, 1.0); }).ok_or("Invalid value"),
        "boid_max_speed" => payload.value.as_f64().map(|v| { config.boid_max_speed = v.clamp(0.001, 0.1); }).ok_or("Invalid value"),
        "boid_max_force" => payload.value.as_f64().map(|v| { config.boid_max_force = v.clamp(0.0001, 0.01); }).ok_or("Invalid value"),
        "boid_predator_enabled" => payload.value.as_bool().map(|v| { config.boid_predator_enabled = v; }).ok_or("Invalid value"),
        "boid_predator_count" => payload.value.as_u64().map(|v| { config.boid_predator_count = (v as usize).clamp(1, 20); }).ok_or("Invalid value"),
        "boid_predator_speed" => payload.value.as_f64().map(|v| { config.boid_predator_speed = v.clamp(0.001, 0.15); }).ok_or("Invalid value"),
        "boid_avoidance_distance" => payload.value.as_f64().map(|v| { config.boid_avoidance_distance = v.clamp(0.1, 1.0); }).ok_or("Invalid value"),
        "boid_chase_force" => payload.value.as_f64().map(|v| { config.boid_chase_force = v.clamp(0.0001, 0.01); }).ok_or("Invalid value"),
        "sand_grid_width" => payload.value.as_u64().map(|v| { config.sand_grid_width = (v as usize).clamp(8, 128); }).ok_or("Invalid value"),
        "sand_grid_height" => payload.value.as_u64().map(|v| { config.sand_grid_height = (v as usize).clamp(8, 64); }).ok_or("Invalid value"),
        "sand_spawn_enabled" => payload.value.as_bool().map(|v| { config.sand_spawn_enabled = v; }).ok_or("Invalid value"),
        "sand_particle_type" => payload.value.as_str().map(|v| { config.sand_particle_type = v.to_string(); }).ok_or("Invalid value"),
        "sand_spawn_rate" => payload.value.as_f64().map(|v| { config.sand_spawn_rate = v.clamp(0.0, 1.0); }).ok_or("Invalid value"),
        "sand_spawn_radius" => payload.value.as_u64().map(|v| { config.sand_spawn_radius = (v as usize).clamp(1, 10); }).ok_or("Invalid value"),
        "sand_spawn_x" => payload.value.as_u64().map(|v| { config.sand_spawn_x = (v as usize).clamp(0, config.sand_grid_width.saturating_sub(1)); }).ok_or("Invalid value"),
        "sand_obstacles_enabled" => payload.value.as_bool().map(|v| { config.sand_obstacles_enabled = v; }).ok_or("Invalid value"),
        "sand_obstacle_density" => payload.value.as_f64().map(|v| { config.sand_obstacle_density = v.clamp(0.0, 1.0); }).ok_or("Invalid value"),
        "sand_fire_enabled" => payload.value.as_bool().map(|v| { config.sand_fire_enabled = v; }).ok_or("Invalid value"),
        "sand_color_sand" => payload.value.as_str().map(|v| { config.sand_color_sand = v.to_string(); }).ok_or("Invalid value"),
        "sand_color_water" => payload.value.as_str().map(|v| { config.sand_color_water = v.to_string(); }).ok_or("Invalid value"),
        "sand_color_stone" => payload.value.as_str().map(|v| { config.sand_color_stone = v.to_string(); }).ok_or("Invalid value"),
        "sand_color_fire" => payload.value.as_str().map(|v| { config.sand_color_fire = v.to_string(); }).ok_or("Invalid value"),
        "sand_color_smoke" => payload.value.as_str().map(|v| { config.sand_color_smoke = v.to_string(); }).ok_or("Invalid value"),
        "sand_color_wood" => payload.value.as_str().map(|v| { config.sand_color_wood = v.to_string(); }).ok_or("Invalid value"),
        "sand_color_lava" => payload.value.as_str().map(|v| { config.sand_color_lava = v.to_string(); }).ok_or("Invalid value"),
        "multi_device_enabled" => payload.value.as_bool().map(|v| { config.multi_device_enabled = v; }).ok_or("Invalid value"),
        "multi_device_send_parallel" => payload.value.as_bool().map(|v| { config.multi_device_send_parallel = v; }).ok_or("Invalid value"),
        "multi_device_fail_fast" => payload.value.as_bool().map(|v| { config.multi_device_fail_fast = v; }).ok_or("Invalid value"),
        _ => Err("Unknown field"),
    };

    if let Err(e) = result {
        return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
    }

    match config.save() {
        Ok(_) => {
            println!("✓ Config saved successfully (field: {}, value: {:?})", payload.field, payload.value);
            // Broadcast config change event via SSE
            let _ = config_tx.send(());
            (StatusCode::OK, "Configuration updated").into_response()
        },
        Err(e) => {
            eprintln!("✗ Failed to save config: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        },
    }
}

// SSE handler - streams config change events to connected clients
async fn config_events(
    State(tx): State<broadcast::Sender<()>>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let mut rx = tx.subscribe();

    let event_stream = stream! {
        loop {
            // Wait for a config change notification
            match rx.recv().await {
                Ok(_) => {
                    // Send a simple "reload" event to the client
                    yield Ok(SseEvent::default().event("config-changed").data("reload"));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // If we missed some events, still send a reload event
                    yield Ok(SseEvent::default().event("config-changed").data("reload"));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    // Channel closed, exit the stream
                    break;
                }
            }
        }
    };

    Sse::new(event_stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive")
    )
}

// Device management endpoints
#[derive(Deserialize)]
struct AddDeviceRequest {
    ip: String,
    led_offset: usize,
    led_count: usize,
    enabled: bool,
}

#[derive(Deserialize)]
struct RemoveDeviceRequest {
    index: usize,
}

#[derive(Deserialize)]
struct UpdateDeviceRequest {
    index: usize,
    field: String,
    value: serde_json::Value,
}

async fn add_device(
    State(config_tx): State<broadcast::Sender<()>>,
    Json(payload): Json<AddDeviceRequest>,
) -> impl IntoResponse {
    let mut config = match BandwidthConfig::load() {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let device = crate::config::WLEDDeviceConfig {
        ip: payload.ip,
        led_offset: payload.led_offset,
        led_count: payload.led_count,
        enabled: payload.enabled,
    };

    config.wled_devices.push(device);

    match config.save() {
        Ok(_) => {
            let _ = config_tx.send(());
            (StatusCode::OK, "Device added").into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn remove_device(
    State(config_tx): State<broadcast::Sender<()>>,
    Json(payload): Json<RemoveDeviceRequest>,
) -> impl IntoResponse {
    let mut config = match BandwidthConfig::load() {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    if payload.index >= config.wled_devices.len() {
        return (StatusCode::BAD_REQUEST, "Invalid device index").into_response();
    }

    config.wled_devices.remove(payload.index);

    match config.save() {
        Ok(_) => {
            let _ = config_tx.send(());
            (StatusCode::OK, "Device removed").into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn update_device_field(
    State(config_tx): State<broadcast::Sender<()>>,
    Json(payload): Json<UpdateDeviceRequest>,
) -> impl IntoResponse {
    let mut config = match BandwidthConfig::load() {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    if payload.index >= config.wled_devices.len() {
        return (StatusCode::BAD_REQUEST, "Invalid device index").into_response();
    }

    let device = &mut config.wled_devices[payload.index];

    let result = match payload.field.as_str() {
        "ip" => payload.value.as_str().map(|v| { device.ip = v.to_string(); }).ok_or("Invalid value"),
        "led_offset" => payload.value.as_u64().map(|v| { device.led_offset = v as usize; }).ok_or("Invalid value"),
        "led_count" => payload.value.as_u64().map(|v| { device.led_count = v as usize; }).ok_or("Invalid value"),
        "enabled" => payload.value.as_bool().map(|v| { device.enabled = v; }).ok_or("Invalid value"),
        _ => Err("Unknown field"),
    };

    if let Err(e) = result {
        return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
    }

    match config.save() {
        Ok(_) => {
            let _ = config_tx.send(());
            (StatusCode::OK, "Device updated").into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_gradients() -> impl IntoResponse {
    let mut gradients_map = HashMap::new();

    // Add built-in gradients
    for name in gradients::get_spectrum_gradient_names() {
        let hex_colors = gradients::gradient_to_hex_string(name);
        gradients_map.insert(format!("builtin:{}", name), hex_colors);
    }

    // Add custom gradients
    if let Ok(custom_gradients) = gradients::load_custom_gradients() {
        for (name, hex_colors) in custom_gradients {
            gradients_map.insert(format!("custom:{}", name), hex_colors);
        }
    }

    (StatusCode::OK, Json(gradients_map)).into_response()
}

#[derive(Deserialize)]
struct SaveGradientRequest {
    name: String,
    hex_colors: String,
}

async fn save_gradient(Json(payload): Json<SaveGradientRequest>) -> impl IntoResponse {
    match gradients::save_custom_gradient(&payload.name, &payload.hex_colors) {
        Ok(_) => (StatusCode::OK, "Gradient saved successfully").into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct DeleteGradientRequest {
    name: String,
}

async fn delete_gradient(Json(payload): Json<DeleteGradientRequest>) -> impl IntoResponse {
    match gradients::delete_custom_gradient(&payload.name) {
        Ok(_) => (StatusCode::OK, "Gradient deleted successfully").into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct TriggerActionRequest {
    action: String,
}

async fn trigger_action(Json(payload): Json<TriggerActionRequest>) -> impl IntoResponse {
    match payload.action.as_str() {
        "sand_restart" => {
            // Write a flag file to signal the sand mode to restart
            match std::fs::write("/tmp/rustwled_sand_restart", "1") {
                Ok(_) => (StatusCode::OK, "Sand simulation restart triggered").into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to trigger restart: {}", e)).into_response(),
            }
        }
        _ => (StatusCode::BAD_REQUEST, format!("Unknown action: {}", payload.action)).into_response(),
    }
}

async fn get_audio_devices() -> impl IntoResponse {
    match audio::list_audio_devices() {
        Ok(devices) => {
            let device_names: Vec<String> = devices.iter().map(|(name, _)| name.clone()).collect();
            (StatusCode::OK, Json(device_names)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_network_interfaces_api(
    Query(params): Query<HashMap<String, String>>
) -> impl IntoResponse {
    let ssh_host = params.get("ssh_host").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let ssh_user = params.get("ssh_user").map(|s| s.as_str()).filter(|s| !s.is_empty());

    if let Some(host) = ssh_host {
        // Fetch interfaces from remote SSH host
        match get_remote_network_interfaces(host, ssh_user).await {
            Ok(interfaces) => (StatusCode::OK, Json(interfaces)).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    } else {
        // Fetch interfaces from local system
        match get_network_interfaces() {
            Ok(interfaces) => (StatusCode::OK, Json(interfaces)).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    }
}

// Get network interfaces from a remote SSH host
pub async fn get_remote_network_interfaces(host: &str, user: Option<&str>) -> Result<Vec<String>> {
    // Construct SSH target: user@host or just host
    let ssh_target = if let Some(u) = user {
        format!("{}@{}", u, host)
    } else {
        host.to_string()
    };

    // Script that detects OS and lists interfaces
    let script = r#"
OS=$(uname)
if [ "$OS" = "Darwin" ]; then
    # macOS - use ifconfig -l
    ifconfig -l | tr ' ' '\n' | grep -v '^lo' | grep -v '^gif' | grep -v '^stf'
else
    # Linux - list from /sys/class/net
    ls /sys/class/net | grep -v '^lo'
fi
"#;

    let output = Command::new("ssh")
        .arg(&ssh_target)
        .arg(script)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .await?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("Failed to fetch interfaces from remote host"));
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut interfaces: Vec<String> = output_str
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    interfaces.sort();
    Ok(interfaces)
}

// HTTP access logging middleware
async fn logging_middleware(
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    // Access logging disabled to avoid cluttering TUI output
    // Logs can be added to a file here if needed in the future
    let response = next.run(req).await;
    response
}

async fn basic_auth_middleware(
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Load config to check if auth is enabled
    let config = match BandwidthConfig::load() {
        Ok(c) => c,
        Err(_) => BandwidthConfig::default(),
    };

    // If auth is disabled, pass through
    if !config.httpd_auth_enabled || config.httpd_auth_user.is_empty() || config.httpd_auth_pass.is_empty() {
        return Ok(next.run(req).await);
    }

    // Check Authorization header
    let auth_header = req.headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    if let Some(auth) = auth_header {
        // Parse "Basic base64(user:pass)"
        if let Some(encoded) = auth.strip_prefix("Basic ") {
            if let Ok(decoded) = general_purpose::STANDARD.decode(encoded) {
                if let Ok(credentials) = String::from_utf8(decoded) {
                    let parts: Vec<&str> = credentials.splitn(2, ':').collect();
                    if parts.len() == 2 && parts[0] == config.httpd_auth_user && parts[1] == config.httpd_auth_pass {
                        // Auth successful
                        return Ok(next.run(req).await);
                    }
                }
            }
        }
    }

    // Auth failed - return 401 with WWW-Authenticate header
    let mut response = Response::new(String::from("Unauthorized").into());
    *response.status_mut() = StatusCode::UNAUTHORIZED;
    response.headers_mut().insert(
        WWW_AUTHENTICATE,
        "Basic realm=\"RustWLED\"".parse().unwrap(),
    );
    Ok(response)
}

// Shutdown endpoint handler - terminates the entire application
async fn shutdown_app() -> Result<axum::Json<serde_json::Value>, StatusCode> {
    eprintln!("\n🛑 Shutdown requested via web UI");

    // Spawn a thread to kill the process after a short delay
    // This allows the HTTP response to be sent first
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(500));
        eprintln!("🛑 Shutting down application...");
        std::process::exit(0);
    });

    Ok(axum::Json(serde_json::json!({
        "success": true,
        "message": "Application shutting down..."
    })))
}

/// WebSocket handler for webcam mode
async fn webcam_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<webcam::WebcamState>>,
) -> Response {
    ws.on_upgrade(move |socket| webcam::handle_webcam_ws(socket, state))
}

pub async fn run_http_server(
    ip: String,
    port: u16,
    https_enabled: bool,
    config_change_tx: broadcast::Sender<()>,
    webcam_state: Arc<webcam::WebcamState>,
) -> Result<()> {
    // Create webcam WebSocket router with its own state
    let webcam_router = Router::new()
        .route("/ws/webcam", get(webcam_ws_handler))
        .with_state(webcam_state);

    // Create main router with config state
    let app = Router::new()
        .route("/", get(serve_index))
        .route("/bandwidth", get(serve_index))
        .route("/audio", get(serve_index))
        .route("/webcam", get(serve_index))
        .route("/midi", get(serve_index))
        .route("/relay", get(serve_index))
        .route("/tron", get(serve_index))
        .route("/api/config", get(get_config))
        .route("/api/config", post(update_config))
        .route("/api/config/fields", get(get_all_fields))
        .route("/api/config/events", get(config_events))
        .route("/api/gradients", get(get_gradients))
        .route("/api/gradients/save", post(save_gradient))
        .route("/api/gradients/delete", post(delete_gradient))
        .route("/api/audio_devices", get(get_audio_devices))
        .route("/api/network_interfaces", get(get_network_interfaces_api))
        .route("/api/devices/add", post(add_device))
        .route("/api/devices/remove", post(remove_device))
        .route("/api/devices/update", post(update_device_field))
        .route("/api/action", post(trigger_action))
        .route("/api/shutdown", post(shutdown_app))
        .layer(middleware::from_fn(basic_auth_middleware))
        .layer(middleware::from_fn(logging_middleware))
        .with_state(config_change_tx)
        .merge(webcam_router);

    let addr = format!("{}:{}", ip, port);

    if https_enabled {
        // Ensure certificates exist
        cert::ensure_certificates(&ip)?;

        // Load certificates
        let (cert_pem, key_pem) = cert::load_certificates()?;

        // Parse certificate and key (rustls-pemfile 1.0 API)
        let cert_chain = certs(&mut BufReader::new(&cert_pem[..]))
            .context("Failed to parse certificate")?
            .into_iter()
            .map(rustls::Certificate)
            .collect::<Vec<_>>();

        let mut keys = pkcs8_private_keys(&mut BufReader::new(&key_pem[..]))
            .context("Failed to parse private key")?;

        if keys.is_empty() {
            anyhow::bail!("No private key found in key file");
        }

        let key = rustls::PrivateKey(keys.remove(0));

        // Create rustls config (rustls 0.21 API)
        let mut server_config = rustls::ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(cert_chain, key)
            .context("Failed to create TLS configuration")?;

        server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        let tls_config = RustlsConfig::from_config(Arc::new(server_config));

        println!("🔒 HTTPS server listening on https://{}:{}", ip, port);

        // Start HTTPS server
        axum_server::bind_rustls(addr.parse()?, tls_config)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await?;
    } else {
        // Start regular HTTP server
        println!("🌐 HTTP server listening on http://{}:{}", ip, port);

        let listener = tokio::net::TcpListener::bind(&addr).await?;

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;
    }

    Ok(())
}

// Get available network interfaces from the system
pub fn get_network_interfaces() -> Result<Vec<String>> {
    #[cfg(target_os = "macos")]
    {
        // On macOS, use ifconfig to list interfaces
        let output = StdCommand::new("ifconfig")
            .arg("-l")
            .output()?;

        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut interfaces: Vec<String> = output_str
            .split_whitespace()
            .map(|s| s.to_string())
            .filter(|s| !s.starts_with("lo") && !s.starts_with("gif") && !s.starts_with("stf"))
            .collect();

        interfaces.sort();
        return Ok(interfaces);
    }

    #[cfg(target_os = "linux")]
    {
        // On Linux, read from /sys/class/net
        let mut interfaces = Vec::new();
        if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with("lo") {
                    interfaces.push(name);
                }
            }
        }

        interfaces.sort();
        return Ok(interfaces);
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Ok(Vec::new())
    }
}
