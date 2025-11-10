// MIDI Module - Real-time MIDI input to LED control
use anyhow::{anyhow, Result};
use midir::{MidiInput, MidiInputConnection};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// RGB color representation
#[derive(Clone, Copy, Debug)]
pub struct RGB {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RGB {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        RGB { r, g, b }
    }
}

/// MIDI note state manager - tracks active notes and their velocities
#[derive(Clone)]
pub struct NoteState {
    active_notes: Arc<Mutex<HashMap<(u8, u8), u8>>>, // (channel, note) -> velocity
}

impl NoteState {
    pub fn new() -> Self {
        NoteState {
            active_notes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add or update a note
    pub fn note_on(&self, channel: u8, note: u8, velocity: u8) {
        let mut notes = self.active_notes.lock().unwrap();
        notes.insert((channel, note), velocity);
    }

    /// Remove a note
    pub fn note_off(&self, channel: u8, note: u8) {
        let mut notes = self.active_notes.lock().unwrap();
        notes.remove(&(channel, note));
    }

    /// Get all active notes with their channels
    pub fn get_active_notes(&self) -> Vec<(u8, u8, u8)> {
        let notes = self.active_notes.lock().unwrap();
        notes.iter().map(|((ch, n), v)| (*ch, *n, *v)).collect()
    }

    /// Get count of active notes
    pub fn count(&self) -> usize {
        let notes = self.active_notes.lock().unwrap();
        notes.len()
    }
}

/// Convert MIDI note number to musical note name (e.g., 60 -> "C4")
pub fn note_number_to_name(note: u8) -> String {
    let note_names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let octave = (note / 12) as i32 - 1;
    let note_index = (note % 12) as usize;
    format!("{}{}", note_names[note_index], octave)
}

/// Color map for storing note-to-color assignments
pub type ColorMap = HashMap<u8, RGB>;

/// Generate random color map for all 128 MIDI notes
/// Randomly shuffles the 12 primary colors and assigns them to the 12 chromatic notes
pub fn generate_random_color_map() -> ColorMap {
    use rand::seq::SliceRandom;

    let mut rng = rand::thread_rng();

    // Define the 12 primary colors
    let mut colors = [
        RGB::new(255, 0, 0),      // Red
        RGB::new(255, 128, 0),    // Orange
        RGB::new(255, 255, 0),    // Yellow
        RGB::new(128, 255, 0),    // Chartreuse
        RGB::new(0, 255, 0),      // Green
        RGB::new(0, 255, 255),    // Cyan
        RGB::new(0, 128, 255),    // Light Blue
        RGB::new(0, 0, 255),      // Blue
        RGB::new(128, 0, 255),    // Purple
        RGB::new(255, 0, 255),    // Magenta
        RGB::new(255, 0, 128),    // Pink
        RGB::new(255, 255, 255),  // White
    ];

    // Shuffle the colors
    colors.shuffle(&mut rng);

    let mut color_map = HashMap::new();

    // Assign shuffled colors to all notes based on note letter (note % 12)
    for note in 0..128 {
        let note_in_octave = note % 12;
        color_map.insert(note, colors[note_in_octave as usize]);
    }

    color_map
}

/// Map MIDI note to a color based on note letter (C, C#, D, etc.)
/// All octaves of the same note get the same color
/// Uses 12 primary colors for the 12 chromatic notes
pub fn note_to_color(note: u8) -> RGB {
    let note_in_octave = note % 12;

    match note_in_octave {
        0  => RGB::new(255, 0, 0),      // C  - Red
        1  => RGB::new(255, 128, 0),    // C# - Orange
        2  => RGB::new(255, 255, 0),    // D  - Yellow
        3  => RGB::new(128, 255, 0),    // D# - Chartreuse
        4  => RGB::new(0, 255, 0),      // E  - Green
        5  => RGB::new(0, 255, 255),    // F  - Cyan
        6  => RGB::new(0, 128, 255),    // F# - Light Blue
        7  => RGB::new(0, 0, 255),      // G  - Blue
        8  => RGB::new(128, 0, 255),    // G# - Purple
        9  => RGB::new(255, 0, 255),    // A  - Magenta
        10 => RGB::new(255, 0, 128),    // A# - Pink
        11 => RGB::new(255, 255, 255),  // B  - White
        _  => RGB::new(255, 255, 255),  // Fallback - White
    }
}

/// Get color for a note, using color map if provided, otherwise using note-based primary colors
pub fn get_note_color(note: u8, color_map: Option<&ColorMap>) -> RGB {
    match color_map {
        Some(map) => *map.get(&note).unwrap_or(&RGB::new(255, 255, 255)),
        None => note_to_color(note),
    }
}

/// Convert MIDI velocity (0-127) to LED brightness (0-255)
/// Minimum velocity is clamped to 10 to ensure LEDs are visible
pub fn velocity_to_brightness(velocity: u8) -> u8 {
    let clamped_velocity = velocity.max(10);
    ((clamped_velocity as f64 / 127.0) * 255.0) as u8
}

/// Map MIDI velocity to a color across the full spectrum
/// Low velocity (0) = Violet/Blue, High velocity (127) = Red
/// Similar to spectrum analyzer gradient
/// Minimum velocity is clamped to 10 for consistency with brightness
pub fn velocity_to_color(velocity: u8) -> RGB {
    let clamped_velocity = velocity.max(10);
    let pos = clamped_velocity as f32 / 127.0;

    if pos < 0.16 {
        // Violet to Blue
        let t = pos / 0.16;
        let r = (138.0 * (1.0 - t)) as u8;
        let g = (43.0 * (1.0 - t)) as u8;
        let b = 226;
        RGB::new(r, g, b)
    } else if pos < 0.33 {
        // Blue to Cyan
        let t = (pos - 0.16) / 0.17;
        let r = 0;
        let g = (255.0 * t) as u8;
        let b = 255;
        RGB::new(r, g, b)
    } else if pos < 0.50 {
        // Cyan to Green
        let t = (pos - 0.33) / 0.17;
        let r = 0;
        let g = 255;
        let b = (255.0 * (1.0 - t)) as u8;
        RGB::new(r, g, b)
    } else if pos < 0.66 {
        // Green to Yellow
        let t = (pos - 0.50) / 0.16;
        let r = (255.0 * t) as u8;
        let g = 255;
        let b = 0;
        RGB::new(r, g, b)
    } else if pos < 0.83 {
        // Yellow to Orange
        let t = (pos - 0.66) / 0.17;
        let r = 255;
        let g = (255.0 * (1.0 - t * 0.5)) as u8;
        let b = 0;
        RGB::new(r, g, b)
    } else {
        // Orange to Red
        let t = (pos - 0.83) / 0.17;
        let r = 255;
        let g = (127.0 * (1.0 - t)) as u8;
        let b = 0;
        RGB::new(r, g, b)
    }
}

/// MIDI Event types we care about
#[derive(Debug, Clone)]
pub enum MidiEvent {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8 },
}

/// Parse MIDI message bytes into our MidiEvent type
pub fn parse_midi_message(message: &[u8]) -> Option<MidiEvent> {
    if message.len() < 3 {
        return None;
    }

    let status = message[0];
    let note = message[1];
    let velocity = message[2];

    // Extract channel from status byte (0-15, which represents MIDI channels 1-16)
    let channel = status & 0x0F;

    // Note On: 0x90-0x9F
    if status >= 0x90 && status <= 0x9F {
        if velocity > 0 {
            return Some(MidiEvent::NoteOn { channel, note, velocity });
        } else {
            // Note On with velocity 0 is treated as Note Off
            return Some(MidiEvent::NoteOff { channel, note });
        }
    }

    // Note Off: 0x80-0x8F
    if status >= 0x80 && status <= 0x8F {
        return Some(MidiEvent::NoteOff { channel, note });
    }

    None
}

/// Calculate LED layout parameters for MIDI mode
/// Returns (leds_per_note, start_offset, end_offset)
pub fn calculate_led_layout(total_leds: usize) -> (usize, usize, usize) {
    let leds_per_note = total_leds / 128;
    let used_leds = 128 * leds_per_note;
    let unused_leds = total_leds.saturating_sub(used_leds);
    let start_offset = unused_leds / 2;
    let end_offset = unused_leds - start_offset;
    (leds_per_note, start_offset, end_offset)
}

/// Get LED range for a specific MIDI note
/// Returns (start_led, end_led) - exclusive end
pub fn note_to_led_range(note: u8, leds_per_note: usize, start_offset: usize) -> (usize, usize) {
    let start_led = start_offset + (note as usize * leds_per_note);
    let end_led = start_led + leds_per_note;
    (start_led, end_led)
}

/// Get all LED indices for a note in 1-to-1 mapping mode
/// Middle C (note 60) is at the center, note pattern repeats every 128 LEDs
/// Returns vector of all LED positions that map to this note
pub fn note_to_leds_one_to_one(note: u8, total_leds: usize) -> Vec<usize> {
    const MIDDLE_C: i32 = 60;
    let middle_led = (total_leds / 2) as i32;
    let base_offset = note as i32 - MIDDLE_C;

    let mut leds = Vec::new();

    // The note pattern repeats every 128 LEDs
    // Find the first occurrence of this note on the strip (in the 0-127 range)
    let mut first_led = middle_led + base_offset;

    // Normalize to 0-127 range
    while first_led < 0 {
        first_led += 128;
    }
    while first_led >= 128 {
        first_led -= 128;
    }

    // Now step through the strip, adding every 128th LED
    let mut current_led = first_led;
    while current_led < total_leds as i32 {
        leds.push(current_led as usize);
        current_led += 128;
    }

    leds
}

/// Get LED index for a note in channel mode
/// Each channel gets 128 LEDs, last channel uses remaining LEDs
/// Channel 0 (MIDI channel 1): LEDs 0-127
/// Channel 1 (MIDI channel 2): LEDs 128-255, etc.
/// Returns None if LED position exceeds total_leds
pub fn channel_and_note_to_led(channel: u8, note: u8, total_leds: usize) -> Option<usize> {
    let led = (channel as usize * 128) + note as usize;
    if led < total_leds {
        Some(led)
    } else {
        None
    }
}

/// List all available MIDI input ports
/// Returns a vector of port names
pub fn list_midi_ports() -> Result<Vec<String>> {
    let midi_in = MidiInput::new("rustwled")?;
    let ports = midi_in.ports();

    let mut port_names = Vec::new();
    for port in ports.iter() {
        if let Ok(name) = midi_in.port_name(port) {
            port_names.push(name);
        }
    }

    Ok(port_names)
}

/// Find a MIDI input port by name (case-insensitive substring match)
pub fn find_midi_port(midi_in: &MidiInput, port_name: &str) -> Result<usize> {
    let ports = midi_in.ports();

    for (i, port) in ports.iter().enumerate() {
        if let Ok(name) = midi_in.port_name(port) {
            if name.to_lowercase().contains(&port_name.to_lowercase()) {
                return Ok(i);
            }
        }
    }

    Err(anyhow!("MIDI port '{}' not found", port_name))
}

/// Connect to a MIDI input device
pub fn connect_midi<F>(device_name: &str, callback: F) -> Result<MidiInputConnection<()>>
where
    F: FnMut(u64, &[u8], &mut ()) + Send + 'static,
{
    let midi_in = MidiInput::new("rustwled")?;

    // Get available ports
    let ports = midi_in.ports();
    if ports.is_empty() {
        return Err(anyhow!("No MIDI input ports available"));
    }

    // Try to find the requested port
    let port_index = match find_midi_port(&midi_in, device_name) {
        Ok(idx) => idx,
        Err(_) => {
            // If not found, use the first port
            0
        }
    };

    let port = &ports[port_index];

    let connection = midi_in
        .connect(port, "rustwled_input", callback, ())
        .map_err(|e| anyhow!("Failed to connect to MIDI port: {}", e))?;

    Ok(connection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_note_to_name() {
        assert_eq!(note_number_to_name(0), "C-1");
        assert_eq!(note_number_to_name(60), "C4");
        assert_eq!(note_number_to_name(69), "A4");
        assert_eq!(note_number_to_name(127), "G9");
    }

    #[test]
    fn test_velocity_to_brightness() {
        assert_eq!(velocity_to_brightness(0), 0);
        assert_eq!(velocity_to_brightness(127), 255);
        assert_eq!(velocity_to_brightness(64), 128);
    }

    #[test]
    fn test_note_to_color() {
        let color = note_to_color(0);
        assert!(color.r > 200); // Should be reddish

        let color = note_to_color(64);
        assert!(color.b > 200 || color.g > 200); // Should be cyan/greenish
    }
}
