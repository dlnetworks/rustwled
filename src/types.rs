// Shared types module - Common types used across multiple modules

use anyhow::Result;
use colorgrad::Color;

// Mode exit reason - used to determine if we should quit or switch modes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModeExitReason {
    UserQuit,      // User pressed 'q' or Ctrl+C - should exit app
    ModeChanged,   // Mode changed in config - should switch modes
}

// Gradient interpolation mode
#[derive(Debug, Clone, Copy)]
pub enum InterpolationMode {
    Linear,
    Basis,
    CatmullRom,
}

// RGB color representation
#[derive(Clone, Copy, Debug)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub fn from_hex(hex: &str) -> Result<Self> {
        let hex = hex.trim_start_matches('#');
        if hex.len() != 6 {
            anyhow::bail!("Invalid hex color: {}", hex);
        }
        Ok(Rgb {
            r: u8::from_str_radix(&hex[0..2], 16)?,
            g: u8::from_str_radix(&hex[2..4], 16)?,
            b: u8::from_str_radix(&hex[4..6], 16)?,
        })
    }
}

// Helper function to build gradient from color string (cyclic for animation)
pub fn build_gradient_from_color(
    color_str: &str,
    use_gradient: bool,
    interpolation_mode: InterpolationMode,
) -> Result<(Option<colorgrad::Gradient>, Vec<Rgb>, Rgb)> {
    let hex_colors: Vec<&str> = color_str.split(',').map(|s| s.trim()).collect();

    // Parse all colors into RGB
    let mut rgb_colors = Vec::new();
    for hex in hex_colors.iter() {
        rgb_colors.push(Rgb::from_hex(hex)?);
    }

    // Build gradient only if we have multiple colors and use_gradient is enabled
    let gradient = if rgb_colors.len() >= 2 && use_gradient {
        // Create smooth gradient through all colors (no plateaus)
        let mut colorgrad_colors = Vec::new();

        // Convert RGB colors to colorgrad colors
        for rgb in rgb_colors.iter() {
            let color = Color::from_rgba8(rgb.r, rgb.g, rgb.b, 255);
            colorgrad_colors.push(color);
        }

        // Add first color at end to make it cyclic/repeating
        if let Some(first_rgb) = rgb_colors.first() {
            let first_color = Color::from_rgba8(first_rgb.r, first_rgb.g, first_rgb.b, 255);
            colorgrad_colors.push(first_color);
        }

        let cg_interpolation = match interpolation_mode {
            InterpolationMode::Basis => colorgrad::Interpolation::Basis,
            InterpolationMode::CatmullRom => colorgrad::Interpolation::CatmullRom,
            _ => colorgrad::Interpolation::Linear,
        };

        let gradient = colorgrad::CustomGradient::new()
            .colors(&colorgrad_colors)
            .interpolation(cg_interpolation)
            .build()?;

        Some(gradient)
    } else {
        None
    };

    // First color is used as solid color or fallback
    let solid_color = rgb_colors.first().copied().unwrap_or(Rgb { r: 255, g: 255, b: 255 });

    Ok((gradient, rgb_colors, solid_color))
}

// Helper function to build gradient from color string (linear for intensity mode)
pub fn build_intensity_gradient(
    color_str: &str,
    use_gradient: bool,
    interpolation_mode: InterpolationMode,
) -> Result<Option<colorgrad::Gradient>> {
    if !use_gradient {
        return Ok(None);
    }

    let hex_colors: Vec<&str> = color_str.split(',').map(|s| s.trim()).collect();
    let mut rgb_colors = Vec::new();
    for hex in hex_colors.iter() {
        rgb_colors.push(Rgb::from_hex(hex)?);
    }

    let gradient = if rgb_colors.len() >= 2 {
        let mut colorgrad_colors = Vec::new();
        for rgb in rgb_colors.iter() {
            let color = Color::from_rgba8(rgb.r, rgb.g, rgb.b, 255);
            colorgrad_colors.push(color);
        }
        // NO cyclic behavior - do NOT add first color at end

        let cg_interpolation = match interpolation_mode {
            InterpolationMode::Basis => colorgrad::Interpolation::Basis,
            InterpolationMode::CatmullRom => colorgrad::Interpolation::CatmullRom,
            _ => colorgrad::Interpolation::Linear,
        };

        let gradient = colorgrad::CustomGradient::new()
            .colors(&colorgrad_colors)
            .interpolation(cg_interpolation)
            .build()?;

        Some(gradient)
    } else {
        None
    };

    Ok(gradient)
}
