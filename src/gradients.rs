// Gradients Module - Spectrum gradient functions and custom gradient management
use anyhow::Result;
use std::path::PathBuf;

/// Get list of all available spectrum gradient names
pub fn get_spectrum_gradient_names() -> Vec<&'static str> {
    vec![
        "Rainbow",
        "Fire",
        "Ice",
        "Purple Haze",
        "Ocean",
        "Sunset",
        "Forest",
        "Neon",
        "Heat Map",
        "Cool Blues",
        "Warm Reds",
        "Grayscale",
        "Retro",
        "Plasma",
        "Viridis",
        "Inferno",
        "Magma",
        "Turbo",
        "Spectral",
        "Cividis",
    ]
}

/// Get spectrum gradient function by name
/// Returns a function that maps position (0.0-1.0) to RGB color (r, g, b)
pub fn get_spectrum_gradient(name: &str) -> Box<dyn Fn(f32) -> (u8, u8, u8) + Send + Sync> {
    match name {
        "Rainbow" => Box::new(gradient_rainbow),
        "Fire" => Box::new(gradient_fire),
        "Ice" => Box::new(gradient_ice),
        "Purple Haze" => Box::new(gradient_purple_haze),
        "Ocean" => Box::new(gradient_ocean),
        "Sunset" => Box::new(gradient_sunset),
        "Forest" => Box::new(gradient_forest),
        "Neon" => Box::new(gradient_neon),
        "Heat Map" => Box::new(gradient_heat_map),
        "Cool Blues" => Box::new(gradient_cool_blues),
        "Warm Reds" => Box::new(gradient_warm_reds),
        "Grayscale" => Box::new(gradient_grayscale),
        "Retro" => Box::new(gradient_retro),
        "Plasma" => Box::new(gradient_plasma),
        "Viridis" => Box::new(gradient_viridis),
        "Inferno" => Box::new(gradient_inferno),
        "Magma" => Box::new(gradient_magma),
        "Turbo" => Box::new(gradient_turbo),
        "Spectral" => Box::new(gradient_spectral),
        "Cividis" => Box::new(gradient_cividis),
        _ => Box::new(gradient_rainbow), // Default fallback
    }
}

// Gradient 1: Rainbow (Classic ROYGBIV - violet -> blue -> cyan -> green -> yellow -> orange -> red)
fn gradient_rainbow(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.16 {
        let t = pos / 0.16;
        ((138.0 * (1.0 - t)) as u8, (43.0 * (1.0 - t)) as u8, 226)
    } else if pos < 0.33 {
        let t = (pos - 0.16) / 0.17;
        (0, (255.0 * t) as u8, 255)
    } else if pos < 0.50 {
        let t = (pos - 0.33) / 0.17;
        (0, 255, (255.0 * (1.0 - t)) as u8)
    } else if pos < 0.66 {
        let t = (pos - 0.50) / 0.16;
        ((255.0 * t) as u8, 255, 0)
    } else if pos < 0.83 {
        let t = (pos - 0.66) / 0.17;
        (255, (255.0 * (1.0 - t * 0.5)) as u8, 0)
    } else {
        let t = (pos - 0.83) / 0.17;
        (255, (127.0 * (1.0 - t)) as u8, 0)
    }
}

// Gradient 2: Fire (black -> red -> orange -> yellow -> white)
fn gradient_fire(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.25 {
        let t = pos / 0.25;
        ((255.0 * t) as u8, 0, 0)
    } else if pos < 0.50 {
        let t = (pos - 0.25) / 0.25;
        (255, (165.0 * t) as u8, 0)
    } else if pos < 0.75 {
        let t = (pos - 0.50) / 0.25;
        (255, (165.0 + 90.0 * t) as u8, (255.0 * t) as u8)
    } else {
        (255, 255, 255)
    }
}

// Gradient 3: Ice (dark blue -> cyan -> light blue -> white)
fn gradient_ice(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.33 {
        let t = pos / 0.33;
        (0, (128.0 * t) as u8, (128.0 + 127.0 * t) as u8)
    } else if pos < 0.66 {
        let t = (pos - 0.33) / 0.33;
        ((128.0 * t) as u8, (128.0 + 127.0 * t) as u8, 255)
    } else {
        let t = (pos - 0.66) / 0.34;
        ((128.0 + 127.0 * t) as u8, 255, 255)
    }
}

// Gradient 4: Purple Haze (dark purple -> purple -> pink -> white)
fn gradient_purple_haze(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.33 {
        let t = pos / 0.33;
        ((75.0 + 105.0 * t) as u8, 0, (130.0 + 86.0 * t) as u8)
    } else if pos < 0.66 {
        let t = (pos - 0.33) / 0.33;
        ((180.0 + 75.0 * t) as u8, (105.0 * t) as u8, (216.0 + 24.0 * t) as u8)
    } else {
        let t = (pos - 0.66) / 0.34;
        (255, (105.0 + 150.0 * t) as u8, (240.0 + 15.0 * t) as u8)
    }
}

// Gradient 5: Ocean (deep blue -> blue -> cyan -> turquoise)
fn gradient_ocean(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.33 {
        let t = pos / 0.33;
        (0, 0, (105.0 + 150.0 * t) as u8)
    } else if pos < 0.66 {
        let t = (pos - 0.33) / 0.33;
        (0, (206.0 * t) as u8, 255)
    } else {
        let t = (pos - 0.66) / 0.34;
        ((64.0 * t) as u8, (206.0 + 18.0 * t) as u8, (255.0 - 47.0 * t) as u8)
    }
}

// Gradient 6: Sunset (purple -> orange -> yellow -> pink)
fn gradient_sunset(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.33 {
        let t = pos / 0.33;
        ((128.0 + 127.0 * t) as u8, 0, (128.0 - 128.0 * t) as u8)
    } else if pos < 0.66 {
        let t = (pos - 0.33) / 0.33;
        (255, (165.0 * t) as u8, 0)
    } else {
        let t = (pos - 0.66) / 0.34;
        (255, (165.0 + 90.0 * t) as u8, (192.0 * t) as u8)
    }
}

// Gradient 7: Forest (dark green -> green -> lime -> yellow-green)
fn gradient_forest(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.33 {
        let t = pos / 0.33;
        (0, (100.0 + 28.0 * t) as u8, 0)
    } else if pos < 0.66 {
        let t = (pos - 0.33) / 0.33;
        ((50.0 * t) as u8, (128.0 + 127.0 * t) as u8, 0)
    } else {
        let t = (pos - 0.66) / 0.34;
        ((50.0 + 124.0 * t) as u8, 255, (154.0 * t) as u8)
    }
}

// Gradient 8: Neon (hot pink -> purple -> blue -> cyan)
fn gradient_neon(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.33 {
        let t = pos / 0.33;
        ((255.0 - 127.0 * t) as u8, (20.0 - 20.0 * t) as u8, (147.0 + 108.0 * t) as u8)
    } else if pos < 0.66 {
        let t = (pos - 0.33) / 0.33;
        ((128.0 - 128.0 * t) as u8, 0, 255)
    } else {
        let t = (pos - 0.66) / 0.34;
        (0, (255.0 * t) as u8, 255)
    }
}

// Gradient 9: Heat Map (black -> purple -> red -> orange -> yellow -> white)
fn gradient_heat_map(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.20 {
        let t = pos / 0.20;
        ((128.0 * t) as u8, 0, (128.0 * t) as u8)
    } else if pos < 0.40 {
        let t = (pos - 0.20) / 0.20;
        ((128.0 + 127.0 * t) as u8, 0, (128.0 - 128.0 * t) as u8)
    } else if pos < 0.60 {
        let t = (pos - 0.40) / 0.20;
        (255, (165.0 * t) as u8, 0)
    } else if pos < 0.80 {
        let t = (pos - 0.60) / 0.20;
        (255, (165.0 + 90.0 * t) as u8, (255.0 * t) as u8)
    } else {
        (255, 255, 255)
    }
}

// Gradient 10: Cool Blues (navy -> blue -> sky blue -> white)
fn gradient_cool_blues(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.33 {
        let t = pos / 0.33;
        (0, 0, (128.0 + 127.0 * t) as u8)
    } else if pos < 0.66 {
        let t = (pos - 0.33) / 0.33;
        ((135.0 * t) as u8, (206.0 * t) as u8, 255)
    } else {
        let t = (pos - 0.66) / 0.34;
        ((135.0 + 120.0 * t) as u8, (206.0 + 49.0 * t) as u8, 255)
    }
}

// Gradient 11: Warm Reds (maroon -> red -> orange -> yellow)
fn gradient_warm_reds(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.33 {
        let t = pos / 0.33;
        ((128.0 + 127.0 * t) as u8, 0, 0)
    } else if pos < 0.66 {
        let t = (pos - 0.33) / 0.33;
        (255, (165.0 * t) as u8, 0)
    } else {
        let t = (pos - 0.66) / 0.34;
        (255, (165.0 + 90.0 * t) as u8, (255.0 * t) as u8)
    }
}

// Gradient 12: Grayscale (black -> gray -> white)
fn gradient_grayscale(pos: f32) -> (u8, u8, u8) {
    let val = (pos.clamp(0.0, 1.0) * 255.0) as u8;
    (val, val, val)
}

// Gradient 13: Retro (magenta -> cyan -> yellow)
fn gradient_retro(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.50 {
        let t = pos / 0.50;
        ((255.0 - 255.0 * t) as u8, (255.0 * t) as u8, 255)
    } else {
        let t = (pos - 0.50) / 0.50;
        ((255.0 * t) as u8, 255, (255.0 - 255.0 * t) as u8)
    }
}

// Gradient 14: Plasma (deep purple -> red -> orange -> yellow)
fn gradient_plasma(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.25 {
        let t = pos / 0.25;
        ((13.0 + 115.0 * t) as u8, (8.0 - 8.0 * t) as u8, (135.0 - 135.0 * t) as u8)
    } else if pos < 0.50 {
        let t = (pos - 0.25) / 0.25;
        ((128.0 + 127.0 * t) as u8, 0, 0)
    } else if pos < 0.75 {
        let t = (pos - 0.50) / 0.25;
        (255, (165.0 * t) as u8, 0)
    } else {
        let t = (pos - 0.75) / 0.25;
        (255, (165.0 + 90.0 * t) as u8, (255.0 * t) as u8)
    }
}

// Gradient 15: Viridis (purple -> blue -> teal -> green -> yellow)
fn gradient_viridis(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.25 {
        let t = pos / 0.25;
        ((68.0 - 24.0 * t) as u8, (1.0 + 55.0 * t) as u8, (84.0 + 32.0 * t) as u8)
    } else if pos < 0.50 {
        let t = (pos - 0.25) / 0.25;
        ((44.0 - 11.0 * t) as u8, (56.0 + 67.0 * t) as u8, (116.0 - 5.0 * t) as u8)
    } else if pos < 0.75 {
        let t = (pos - 0.50) / 0.25;
        ((33.0 + 89.0 * t) as u8, (123.0 + 70.0 * t) as u8, (111.0 - 61.0 * t) as u8)
    } else {
        let t = (pos - 0.75) / 0.25;
        ((122.0 + 131.0 * t) as u8, (193.0 + 36.0 * t) as u8, (50.0 - 18.0 * t) as u8)
    }
}

// Gradient 16: Inferno (black -> purple -> red -> orange -> yellow -> white)
fn gradient_inferno(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.20 {
        let t = pos / 0.20;
        ((66.0 * t) as u8, (10.0 * t) as u8, (104.0 * t) as u8)
    } else if pos < 0.40 {
        let t = (pos - 0.20) / 0.20;
        ((66.0 + 66.0 * t) as u8, (10.0 - 10.0 * t) as u8, (104.0 - 54.0 * t) as u8)
    } else if pos < 0.60 {
        let t = (pos - 0.40) / 0.20;
        ((132.0 + 123.0 * t) as u8, (60.0 * t) as u8, (50.0 - 50.0 * t) as u8)
    } else if pos < 0.80 {
        let t = (pos - 0.60) / 0.20;
        (255, (60.0 + 105.0 * t) as u8, (89.0 * t) as u8)
    } else {
        let t = (pos - 0.80) / 0.20;
        (255, (165.0 + 90.0 * t) as u8, (89.0 + 166.0 * t) as u8)
    }
}

// Gradient 17: Magma (black -> purple -> magenta -> orange -> yellow -> white)
fn gradient_magma(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.20 {
        let t = pos / 0.20;
        ((13.0 + 38.0 * t) as u8, (8.0 + 16.0 * t) as u8, (68.0 + 49.0 * t) as u8)
    } else if pos < 0.40 {
        let t = (pos - 0.20) / 0.20;
        ((51.0 + 87.0 * t) as u8, (24.0 + 1.0 * t) as u8, (117.0 + 16.0 * t) as u8)
    } else if pos < 0.60 {
        let t = (pos - 0.40) / 0.20;
        ((138.0 + 63.0 * t) as u8, (25.0 + 39.0 * t) as u8, (133.0 - 62.0 * t) as u8)
    } else if pos < 0.80 {
        let t = (pos - 0.60) / 0.20;
        ((201.0 + 54.0 * t) as u8, (64.0 + 108.0 * t) as u8, (71.0 - 49.0 * t) as u8)
    } else {
        let t = (pos - 0.80) / 0.20;
        (255, (172.0 + 83.0 * t) as u8, (22.0 + 233.0 * t) as u8)
    }
}

// Gradient 18: Turbo (blue -> cyan -> green -> yellow -> orange -> red)
fn gradient_turbo(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.16 {
        let t = pos / 0.16;
        ((48.0 - 48.0 * t) as u8, (18.0 + 86.0 * t) as u8, (59.0 + 112.0 * t) as u8)
    } else if pos < 0.33 {
        let t = (pos - 0.16) / 0.17;
        (0, (104.0 + 151.0 * t) as u8, (171.0 + 84.0 * t) as u8)
    } else if pos < 0.50 {
        let t = (pos - 0.33) / 0.17;
        ((122.0 * t) as u8, 255, (255.0 - 255.0 * t) as u8)
    } else if pos < 0.66 {
        let t = (pos - 0.50) / 0.16;
        ((122.0 + 133.0 * t) as u8, 255, 0)
    } else if pos < 0.83 {
        let t = (pos - 0.66) / 0.17;
        (255, (255.0 - 100.0 * t) as u8, 0)
    } else {
        let t = (pos - 0.83) / 0.17;
        (255, (155.0 - 155.0 * t) as u8, 0)
    }
}

// Gradient 19: Spectral (red -> orange -> yellow -> green -> cyan -> blue -> purple)
fn gradient_spectral(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.14 {
        let t = pos / 0.14;
        (255, ((255.0 - 90.0) * t) as u8, 0)
    } else if pos < 0.28 {
        let t = (pos - 0.14) / 0.14;
        (255, (165.0 + 90.0 * t) as u8, (255.0 * t) as u8)
    } else if pos < 0.42 {
        let t = (pos - 0.28) / 0.14;
        ((255.0 - 255.0 * t) as u8, 255, 255)
    } else if pos < 0.57 {
        let t = (pos - 0.42) / 0.15;
        (0, 255, (255.0 - 255.0 * t) as u8)
    } else if pos < 0.71 {
        let t = (pos - 0.57) / 0.14;
        (0, (255.0 - 255.0 * t) as u8, 0)
    } else if pos < 0.85 {
        let t = (pos - 0.71) / 0.14;
        ((255.0 * t) as u8, 0, (255.0 * t) as u8)
    } else {
        (255, 0, 255)
    }
}

// Gradient 20: Cividis (blue -> teal -> yellow - colorblind friendly)
fn gradient_cividis(pos: f32) -> (u8, u8, u8) {
    let pos = pos.clamp(0.0, 1.0);
    if pos < 0.25 {
        let t = pos / 0.25;
        ((0.0 + 32.0 * t) as u8, (32.0 + 74.0 * t) as u8, (77.0 + 44.0 * t) as u8)
    } else if pos < 0.50 {
        let t = (pos - 0.25) / 0.25;
        ((32.0 + 62.0 * t) as u8, (106.0 + 52.0 * t) as u8, (121.0 - 6.0 * t) as u8)
    } else if pos < 0.75 {
        let t = (pos - 0.50) / 0.25;
        ((94.0 + 72.0 * t) as u8, (158.0 + 61.0 * t) as u8, (115.0 - 27.0 * t) as u8)
    } else {
        let t = (pos - 0.75) / 0.25;
        ((166.0 + 89.0 * t) as u8, (219.0 + 36.0 * t) as u8, (88.0 - 22.0 * t) as u8)
    }
}

/// Convert a gradient name to comma-separated hex colors by sampling at 12 points
pub fn gradient_to_hex_string(gradient_name: &str) -> String {
    let gradient_fn = get_spectrum_gradient(gradient_name);
    let mut hex_colors = Vec::new();

    // Sample the gradient at 12 evenly spaced points
    for i in 0..12 {
        let pos = i as f32 / 11.0; // 0.0 to 1.0
        let (r, g, b) = gradient_fn(pos);
        hex_colors.push(format!("{:02X}{:02X}{:02X}", r, g, b));
    }

    hex_colors.join(",")
}

/// Get path to custom gradients file
pub fn gradients_file_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")?;
    let config_dir = PathBuf::from(home).join(".config").join("rustwled");
    std::fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join("gradients.conf"))
}

/// Load custom gradients from gradients.conf
/// Returns HashMap of gradient_name -> hex_color_string
pub fn load_custom_gradients() -> Result<std::collections::HashMap<String, String>> {
    let path = gradients_file_path()?;
    let mut gradients = std::collections::HashMap::new();

    if !path.exists() {
        return Ok(gradients);
    }

    let contents = std::fs::read_to_string(&path)?;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((name, value)) = line.split_once('=') {
            let name = name.trim().to_string();
            let value = value.trim().trim_matches('"').to_string();
            gradients.insert(name, value);
        }
    }

    Ok(gradients)
}

/// Save a custom gradient to gradients.conf
/// Overwrites if gradient name already exists
pub fn save_custom_gradient(name: &str, hex_colors: &str) -> Result<()> {
    // Sanitize the name: remove spaces and special chars, allow only alphanumeric and underscore
    let sanitized_name: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();

    if sanitized_name.is_empty() {
        anyhow::bail!("Invalid gradient name");
    }

    let mut gradients = load_custom_gradients().unwrap_or_default();
    gradients.insert(sanitized_name.clone(), hex_colors.to_string());

    // Write all gradients back to file
    let path = gradients_file_path()?;
    let mut contents = String::new();
    for (name, colors) in gradients.iter() {
        contents.push_str(&format!("{} = \"{}\"\n", name, colors));
    }
    std::fs::write(&path, contents)?;

    Ok(())
}

/// Delete a custom gradient from gradients.conf
pub fn delete_custom_gradient(name: &str) -> Result<()> {
    let mut gradients = load_custom_gradients().unwrap_or_default();

    if gradients.remove(name).is_none() {
        anyhow::bail!("Gradient '{}' not found", name);
    }

    // Write remaining gradients back to file
    let path = gradients_file_path()?;
    let mut contents = String::new();
    for (name, colors) in gradients.iter() {
        contents.push_str(&format!("{} = \"{}\"\n", name, colors));
    }
    std::fs::write(&path, contents)?;

    Ok(())
}

/// Resolve a color string which can be:
/// 1. A built-in gradient name (e.g. "Rainbow")
/// 2. A custom gradient name (from gradients.conf)
/// 3. Comma-separated hex colors (e.g. "FF0000,00FF00,0000FF")
/// Returns the comma-separated hex color string
pub fn resolve_color_string(color_str: &str) -> String {
    let trimmed = color_str.trim();

    // Check if it's a built-in gradient name (case-insensitive)
    let gradient_names = get_spectrum_gradient_names();
    for name in gradient_names {
        if name.eq_ignore_ascii_case(trimmed) {
            return gradient_to_hex_string(name);
        }
    }

    // Check if it's a custom gradient name (case-insensitive)
    if let Ok(custom_gradients) = load_custom_gradients() {
        for (name, hex_colors) in custom_gradients.iter() {
            if name.eq_ignore_ascii_case(trimmed) {
                return hex_colors.clone();
            }
        }
    }

    // Assume it's already comma-separated hex colors
    color_str.to_string()
}
