// Geometry Mode - Mathematical and harmonic line-art animations
use std::f64::consts::PI;
use std::time::{Duration, Instant};

const PHI: f64 = 1.618033988749895; // Golden ratio
const GOLDEN_ANGLE: f64 = 137.5; // Golden angle in degrees

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryMode {
    // Original 10 modes
    Lissajous = 0,
    FibonacciSpiral = 1,
    PolarRose = 2,
    NestedPolygons = 3,
    Hypotrochoid = 4,
    Phyllotaxis = 5,
    Kaleidoscope = 6,
    VectorField = 7,
    GoldenStarburst = 8,
    Wireframe3D = 9,
    // New fractal modes
    MandelbrotSet = 10,
    DragonCurve = 11,
    HilbertCurve = 12,
    SierpinskiTriangle = 13,
    FourierEpicycles = 14,
    // New dynamic systems
    StrangeAttractor = 15,
    Boids = 16,
    // New geometric patterns
    PenroseTiling = 17,
    Metaballs = 18,
    // New 3D shapes
    Icosahedron = 19,
}

impl GeometryMode {
    pub fn from_index(index: usize) -> Self {
        match index % 20 {
            0 => GeometryMode::Lissajous,
            1 => GeometryMode::FibonacciSpiral,
            2 => GeometryMode::PolarRose,
            3 => GeometryMode::NestedPolygons,
            4 => GeometryMode::Hypotrochoid,
            5 => GeometryMode::Phyllotaxis,
            6 => GeometryMode::Kaleidoscope,
            7 => GeometryMode::VectorField,
            8 => GeometryMode::GoldenStarburst,
            9 => GeometryMode::Wireframe3D,
            10 => GeometryMode::MandelbrotSet,
            11 => GeometryMode::DragonCurve,
            12 => GeometryMode::HilbertCurve,
            13 => GeometryMode::SierpinskiTriangle,
            14 => GeometryMode::FourierEpicycles,
            15 => GeometryMode::StrangeAttractor,
            16 => GeometryMode::Boids,
            17 => GeometryMode::PenroseTiling,
            18 => GeometryMode::Metaballs,
            19 => GeometryMode::Icosahedron,
            _ => GeometryMode::Lissajous,
        }
    }

    pub fn from_string(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "lissajous" => Some(GeometryMode::Lissajous),
            "fibonacci" | "fibonacci_spiral" => Some(GeometryMode::FibonacciSpiral),
            "polar_rose" | "polarrose" | "rose" => Some(GeometryMode::PolarRose),
            "nested_polygons" | "nestedpolygons" | "polygons" => Some(GeometryMode::NestedPolygons),
            "hypotrochoid" | "spirograph" => Some(GeometryMode::Hypotrochoid),
            "phyllotaxis" | "sunflower" => Some(GeometryMode::Phyllotaxis),
            "kaleidoscope" | "mirror" => Some(GeometryMode::Kaleidoscope),
            "vector_field" | "vectorfield" | "flow" => Some(GeometryMode::VectorField),
            "golden_starburst" | "goldenstarburst" | "starburst" => Some(GeometryMode::GoldenStarburst),
            "wireframe_3d" | "wireframe3d" | "wireframe" | "cube" => Some(GeometryMode::Wireframe3D),
            "mandelbrot" | "mandelbrot_set" | "mandelbrotset" => Some(GeometryMode::MandelbrotSet),
            "dragon" | "dragon_curve" | "dragoncurve" => Some(GeometryMode::DragonCurve),
            "hilbert" | "hilbert_curve" | "hilbertcurve" => Some(GeometryMode::HilbertCurve),
            "sierpinski" | "sierpinski_triangle" | "sierpinskitriangle" => Some(GeometryMode::SierpinskiTriangle),
            "fourier" | "epicycles" | "fourier_epicycles" => Some(GeometryMode::FourierEpicycles),
            "attractor" | "strange_attractor" | "strangeattractor" | "lorenz" => Some(GeometryMode::StrangeAttractor),
            "boids" | "swarm" | "flocking" => Some(GeometryMode::Boids),
            "penrose" | "penrose_tiling" | "penrosetiling" => Some(GeometryMode::PenroseTiling),
            "metaballs" | "blobs" => Some(GeometryMode::Metaballs),
            "icosahedron" | "sphere" | "polyhedron" => Some(GeometryMode::Icosahedron),
            _ => None,
        }
    }

    pub fn next(&self) -> Self {
        Self::from_index(*self as usize + 1)
    }

    pub fn random() -> Self {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        Self::from_index(rng.gen_range(0..20))
    }
}

pub(crate) struct Boid {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    is_predator: bool,
}

pub struct GeometryState {
    pub current_mode: GeometryMode,
    pub mode_start_time: Instant,
    pub animation_start_time: Instant,  // Never reset - used for continuous animation time
    pub mode_duration: Duration,
    pub transition_duration: Duration,
    pub total_leds: usize,
    pub grid_width: usize,
    pub grid_height: usize,
    pub frame_buffer: Vec<(f32, f32, f32)>, // RGB float buffer for blending
    pub fixed_mode: Option<GeometryMode>,  // If Some, stay on this mode; if None, cycle
    pub randomize_order: bool,  // If true, pick random modes when cycling
    pub next_mode: Option<GeometryMode>,  // Pre-selected next mode for smooth transitions
    pub boids: Vec<Boid>,  // Boid positions and velocities
    // Boid configuration parameters
    pub boid_count: usize,
    pub boid_separation_distance: f64,
    pub boid_alignment_distance: f64,
    pub boid_cohesion_distance: f64,
    pub boid_max_speed: f64,
    pub boid_max_force: f64,
    // Predator-prey settings
    pub boid_predator_enabled: bool,
    pub boid_predator_count: usize,
    pub boid_predator_speed: f64,
    pub boid_avoidance_distance: f64,
    pub boid_chase_force: f64,
    // Gradient colors for rendering
    pub gradient_colors: Vec<(f32, f32, f32)>,  // RGB colors from gradient
    // Gradient animation
    pub animation_offset: f64,  // 0.0 to 1.0, for gradient animation
    pub animation_direction: String,  // "left" or "right"
    pub last_geometry_cycle: i64,  // Track geometry cycle to detect when animation repeats
    pub last_config_direction: String,  // Track config direction to detect manual changes
}

impl GeometryState {
    pub fn new(
        total_leds: usize,
        grid_width: usize,
        grid_height: usize,
        mode_select: &str,
        duration_seconds: f64,
        randomize: bool,
        boid_count: usize,
        boid_separation_distance: f64,
        boid_alignment_distance: f64,
        boid_cohesion_distance: f64,
        boid_max_speed: f64,
        boid_max_force: f64,
        boid_predator_enabled: bool,
        boid_predator_count: usize,
        boid_predator_speed: f64,
        boid_avoidance_distance: f64,
        boid_chase_force: f64,
    ) -> Self {
        // Parse mode selection
        let fixed_mode = if mode_select.to_lowercase() == "cycle" {
            None
        } else {
            GeometryMode::from_string(mode_select)
        };

        let current_mode = fixed_mode.unwrap_or(GeometryMode::Lissajous);

        // Initialize boids with random positions and velocities
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let mut boids = Vec::new();

        // Create predators first if enabled
        let num_predators = if boid_predator_enabled { boid_predator_count } else { 0 };
        for _ in 0..num_predators {
            // Random angle for initial velocity direction
            let angle = rng.gen_range(0.0..(2.0 * PI));
            let vx = angle.cos() * boid_predator_speed;
            let vy = angle.sin() * boid_predator_speed;
            boids.push(Boid {
                x: rng.gen_range(-0.8..0.8),
                y: rng.gen_range(-0.8..0.8),
                vx,
                vy,
                is_predator: true,
            });
        }

        // Create regular prey boids
        for _ in 0..boid_count {
            // Random angle for initial velocity direction
            let angle = rng.gen_range(0.0..(2.0 * PI));
            let vx = angle.cos() * boid_max_speed;
            let vy = angle.sin() * boid_max_speed;
            boids.push(Boid {
                x: rng.gen_range(-0.8..0.8),
                y: rng.gen_range(-0.8..0.8),
                vx,
                vy,
                is_predator: false,
            });
        }

        // Initialize with default rainbow gradient
        let default_gradient_colors = vec![
            (1.0, 0.0, 0.0),     // Red
            (1.0, 0.5, 0.0),     // Orange
            (1.0, 1.0, 0.0),     // Yellow
            (0.0, 1.0, 0.0),     // Green
            (0.0, 0.0, 1.0),     // Blue
            (0.3, 0.0, 0.5),     // Indigo
            (0.6, 0.0, 0.8),     // Violet
        ];

        let now = Instant::now();
        Self {
            current_mode,
            mode_start_time: now,
            animation_start_time: now,  // Start continuous animation clock
            mode_duration: Duration::from_secs_f64(duration_seconds.max(1.0)),
            transition_duration: Duration::from_secs(2), // 2 second transitions
            total_leds,
            grid_width,
            grid_height,
            frame_buffer: vec![(0.0, 0.0, 0.0); total_leds],
            fixed_mode,
            randomize_order: randomize,
            next_mode: None,
            boids,
            boid_count,
            boid_separation_distance,
            boid_alignment_distance,
            boid_cohesion_distance,
            boid_max_speed,
            boid_max_force,
            boid_predator_enabled,
            boid_predator_count,
            boid_predator_speed,
            boid_avoidance_distance,
            boid_chase_force,
            gradient_colors: default_gradient_colors,
            animation_offset: 0.0,
            animation_direction: "left".to_string(),
            last_geometry_cycle: -1,
            last_config_direction: "left".to_string(),
        }
    }

    pub fn update_colors(&mut self, colors: Vec<(f32, f32, f32)>) {
        if !colors.is_empty() {
            self.gradient_colors = colors;
        }
    }

    // Get color from gradient at position t (0.0 to 1.0) with animation
    fn get_gradient_color(&self, t: f64) -> (f32, f32, f32) {
        // Apply animation offset based on direction
        let animated_t = if self.animation_direction == "right" {
            (1.0 + t - self.animation_offset) % 1.0
        } else {
            (t + self.animation_offset) % 1.0
        };
        let t = animated_t.clamp(0.0, 1.0) as f32;
        let num_colors = self.gradient_colors.len();
        if num_colors == 0 {
            return (1.0, 1.0, 1.0); // White fallback
        }
        if num_colors == 1 {
            return self.gradient_colors[0];
        }

        // Map t to the gradient color array
        let scaled = t * (num_colors - 1) as f32;
        let index = scaled.floor() as usize;
        let frac = scaled - index as f32;

        if index >= num_colors - 1 {
            return self.gradient_colors[num_colors - 1];
        }

        // Linear interpolation between two colors
        let c1 = self.gradient_colors[index];
        let c2 = self.gradient_colors[index + 1];

        (
            c1.0 + (c2.0 - c1.0) * frac,
            c1.1 + (c2.1 - c1.1) * frac,
            c1.2 + (c2.2 - c1.2) * frac,
        )
    }

    pub fn update_boid_config(
        &mut self,
        boid_count: usize,
        boid_separation_distance: f64,
        boid_alignment_distance: f64,
        boid_cohesion_distance: f64,
        boid_max_speed: f64,
        boid_max_force: f64,
        boid_predator_enabled: bool,
        boid_predator_count: usize,
        boid_predator_speed: f64,
        boid_avoidance_distance: f64,
        boid_chase_force: f64,
    ) {
        // Check if we need to rebuild boids (count or predator settings changed)
        let num_predators_old = if self.boid_predator_enabled { self.boid_predator_count } else { 0 };
        let num_predators_new = if boid_predator_enabled { boid_predator_count } else { 0 };
        let needs_rebuild = boid_count != self.boid_count || num_predators_new != num_predators_old;

        // Update config parameters
        self.boid_count = boid_count;
        self.boid_separation_distance = boid_separation_distance;
        self.boid_alignment_distance = boid_alignment_distance;
        self.boid_cohesion_distance = boid_cohesion_distance;
        self.boid_max_speed = boid_max_speed;
        self.boid_max_force = boid_max_force;
        self.boid_predator_enabled = boid_predator_enabled;
        self.boid_predator_count = boid_predator_count;
        self.boid_predator_speed = boid_predator_speed;
        self.boid_avoidance_distance = boid_avoidance_distance;
        self.boid_chase_force = boid_chase_force;

        // Only rebuild boids if count or predator count changed
        if needs_rebuild {
            use rand::Rng;
            let mut rng = rand::thread_rng();

            // Rebuild boids
            self.boids.clear();

            // Create predators first
            for _ in 0..num_predators_new {
                // Random angle for initial velocity direction
                let angle = rng.gen_range(0.0..(2.0 * PI));
                let vx = angle.cos() * boid_predator_speed;
                let vy = angle.sin() * boid_predator_speed;
                self.boids.push(Boid {
                    x: rng.gen_range(-0.8..0.8),
                    y: rng.gen_range(-0.8..0.8),
                    vx,
                    vy,
                    is_predator: true,
                });
            }

            // Create prey boids
            for _ in 0..boid_count {
                // Random angle for initial velocity direction
                let angle = rng.gen_range(0.0..(2.0 * PI));
                let vx = angle.cos() * boid_max_speed;
                let vy = angle.sin() * boid_max_speed;
                self.boids.push(Boid {
                    x: rng.gen_range(-0.8..0.8),
                    y: rng.gen_range(-0.8..0.8),
                    vx,
                    vy,
                    is_predator: false,
                });
            }
        }
        // Otherwise just keep existing boids with their positions/velocities
        // The new parameters will affect their behavior on the next update
    }

    pub fn update(&mut self, global_brightness: f64, animation_speed: f64, animation_direction: &str) -> Vec<u8> {
        // Update animation offset for gradient animation
        if animation_speed > 0.0 {
            let half_leds = self.total_leds / 2;
            let offset_delta = animation_speed / half_leds as f64;
            self.animation_offset = (self.animation_offset + offset_delta) % 1.0;
        }

        let mut elapsed = self.mode_start_time.elapsed();
        let total_cycle = self.mode_duration + self.transition_duration;

        // Track if we just switched modes
        let mut just_switched = false;

        // Check if we should transition to next mode (only if not in fixed mode)
        if self.fixed_mode.is_none() && elapsed >= total_cycle {
            // Cycle to next mode - use the pre-selected next_mode if available
            self.current_mode = self.next_mode.unwrap_or_else(|| {
                if self.randomize_order {
                    GeometryMode::random()
                } else {
                    self.current_mode.next()
                }
            });
            self.mode_start_time = Instant::now();  // Reset cycle timer
            self.next_mode = None; // Reset for next cycle
            self.last_geometry_cycle = -1; // Reset cycle tracking so new geometry uses config direction
            just_switched = true;
            elapsed = Duration::ZERO; // Reset elapsed to prevent immediate transition on same frame
            // Note: animation_start_time is NEVER reset - animations continue smoothly
        }

        // Use continuous animation time (never resets)
        let mode_time = self.animation_start_time.elapsed().as_secs_f64();

        // Only transition if we're cycling (not in fixed mode) AND we didn't just switch
        let transition_progress = if self.fixed_mode.is_none() && !just_switched && elapsed >= self.mode_duration {
            // In transition phase - pre-select next mode if not already set
            if self.next_mode.is_none() {
                self.next_mode = Some(if self.randomize_order {
                    GeometryMode::random()
                } else {
                    self.current_mode.next()
                });
            }
            (elapsed - self.mode_duration).as_secs_f64() / self.transition_duration.as_secs_f64()
        } else {
            0.0
        };

        // Clear frame buffer
        for pixel in &mut self.frame_buffer {
            *pixel = (0.0, 0.0, 0.0);
        }

        // Render current mode
        self.render_mode(self.current_mode, mode_time);

        // Handle gradient animation direction
        let current_cycle = self.get_geometry_cycle(self.current_mode, mode_time);

        // Check if user manually changed direction in config
        if animation_direction != self.last_config_direction {
            // User manually changed direction - use it immediately
            self.animation_direction = animation_direction.to_string();
            self.last_config_direction = animation_direction.to_string();
        } else if current_cycle != self.last_geometry_cycle && self.last_geometry_cycle >= 0 {
            // Geometry cycle completed - toggle animation direction
            self.animation_direction = if self.animation_direction == "left" {
                "right".to_string()
            } else {
                "left".to_string()
            };
        } else if self.last_geometry_cycle < 0 {
            // First cycle - use direction from config
            self.animation_direction = animation_direction.to_string();
            self.last_config_direction = animation_direction.to_string();
        }
        self.last_geometry_cycle = current_cycle;

        // If in transition, blend with next mode
        if transition_progress > 0.0 {
            // Use the pre-selected next_mode (guaranteed to be Some at this point)
            let next_mode = self.next_mode.unwrap();
            let mut next_buffer = vec![(0.0, 0.0, 0.0); self.total_leds];
            std::mem::swap(&mut self.frame_buffer, &mut next_buffer);

            self.render_mode(next_mode, mode_time);

            // Crossfade blend
            for i in 0..self.total_leds {
                let (r1, g1, b1) = next_buffer[i];
                let (r2, g2, b2) = self.frame_buffer[i];
                let alpha = transition_progress as f32;
                self.frame_buffer[i] = (
                    r1 * (1.0 - alpha) + r2 * alpha,
                    g1 * (1.0 - alpha) + g2 * alpha,
                    b1 * (1.0 - alpha) + b2 * alpha,
                );
            }
        }

        // Convert float buffer to u8 with brightness
        let mut output = vec![0u8; self.total_leds * 3];
        for (i, &(r, g, b)) in self.frame_buffer.iter().enumerate() {
            output[i * 3] = (r * 255.0 * global_brightness as f32).min(255.0).max(0.0) as u8;
            output[i * 3 + 1] = (g * 255.0 * global_brightness as f32).min(255.0).max(0.0) as u8;
            output[i * 3 + 2] = (b * 255.0 * global_brightness as f32).min(255.0).max(0.0) as u8;
        }

        output
    }

    // Calculate the current cycle number for each geometry mode
    fn get_geometry_cycle(&self, mode: GeometryMode, time: f64) -> i64 {
        match mode {
            GeometryMode::SierpinskiTriangle => (time * 0.1 / 2.0) as i64, // 20 second cycles
            GeometryMode::HilbertCurve => (time * 0.2 / 8.0) as i64, // 8 second cycles
            GeometryMode::MandelbrotSet => (time * 0.05) as i64, // 20 second cycles
            GeometryMode::Phyllotaxis => (time * 0.05) as i64, // 20 second cycles
            GeometryMode::DragonCurve => (time * 0.05) as i64, // 20 second cycles
            GeometryMode::Lissajous => (time * 0.1) as i64, // 10 second cycles
            GeometryMode::FibonacciSpiral => (time * 0.1) as i64, // 10 second cycles
            GeometryMode::PolarRose => (time * 0.1) as i64, // 10 second cycles
            GeometryMode::NestedPolygons => (time * 0.1) as i64, // 10 second cycles
            GeometryMode::Hypotrochoid => (time * 0.1) as i64, // 10 second cycles
            GeometryMode::Kaleidoscope => (time * 0.1) as i64, // 10 second cycles
            GeometryMode::VectorField => (time * 0.1) as i64, // 10 second cycles
            GeometryMode::GoldenStarburst => (time * 0.1) as i64, // 10 second cycles
            GeometryMode::Wireframe3D => (time * 0.1) as i64, // 10 second cycles
            GeometryMode::FourierEpicycles => (time * 0.1) as i64, // 10 second cycles
            GeometryMode::StrangeAttractor => (time * 0.05) as i64, // 20 second cycles
            GeometryMode::Boids => (time * 0.05) as i64, // 20 second cycles
            GeometryMode::PenroseTiling => (time * 0.1) as i64, // 10 second cycles
            GeometryMode::Metaballs => (time * 0.1) as i64, // 10 second cycles
            GeometryMode::Icosahedron => (time * 0.1) as i64, // 10 second cycles
        }
    }

    fn render_mode(&mut self, mode: GeometryMode, time: f64) {
        match mode {
            GeometryMode::Lissajous => self.render_lissajous(time),
            GeometryMode::FibonacciSpiral => self.render_fibonacci_spiral(time),
            GeometryMode::PolarRose => self.render_polar_rose(time),
            GeometryMode::NestedPolygons => self.render_nested_polygons(time),
            GeometryMode::Hypotrochoid => self.render_hypotrochoid(time),
            GeometryMode::Phyllotaxis => self.render_phyllotaxis(time),
            GeometryMode::Kaleidoscope => self.render_kaleidoscope(time),
            GeometryMode::VectorField => self.render_vector_field(time),
            GeometryMode::GoldenStarburst => self.render_golden_starburst(time),
            GeometryMode::Wireframe3D => self.render_wireframe_3d(time),
            GeometryMode::MandelbrotSet => self.render_mandelbrot(time),
            GeometryMode::DragonCurve => self.render_dragon_curve(time),
            GeometryMode::HilbertCurve => self.render_hilbert_curve(time),
            GeometryMode::SierpinskiTriangle => self.render_sierpinski(time),
            GeometryMode::FourierEpicycles => self.render_fourier_epicycles(time),
            GeometryMode::StrangeAttractor => self.render_strange_attractor(time),
            GeometryMode::Boids => self.render_boids(time),
            GeometryMode::PenroseTiling => self.render_penrose_tiling(time),
            GeometryMode::Metaballs => self.render_metaballs(time),
            GeometryMode::Icosahedron => self.render_icosahedron(time),
        }
    }

    // Map normalized coordinates (-1 to 1) to LED strip position
    fn coord_to_led(&self, x: f64, y: f64) -> Option<usize> {
        // Convert normalized coordinates (-1 to 1) to grid coordinates (0 to grid_width/height)
        let grid_x = ((x + 1.0) / 2.0 * self.grid_width as f64) as i32;
        let grid_y = ((y + 1.0) / 2.0 * self.grid_height as f64) as i32;

        // Check bounds
        if grid_x < 0 || grid_x >= self.grid_width as i32 || grid_y < 0 || grid_y >= self.grid_height as i32 {
            return None;
        }

        // Map 2D grid position to 1D LED strip index (row-major order)
        let grid_index = (grid_y as usize * self.grid_width) + grid_x as usize;
        let total_grid_pixels = self.grid_width * self.grid_height;

        // Map grid index to LED strip (scale to fit total_leds)
        let led_idx = (grid_index * self.total_leds) / total_grid_pixels;

        if led_idx < self.total_leds {
            Some(led_idx)
        } else {
            None
        }
    }

    // Mode 1: Lissajous Curves
    fn render_lissajous(&mut self, time: f64) {
        let a = 3.0 + (time * 0.2).sin() * 2.0;
        let b = 2.0 + (time * 0.15).cos() * 2.0;
        let delta = time * 0.5;

        let steps = 1000;
        for i in 0..steps {
            let t = (i as f64 / steps as f64) * 2.0 * PI;
            let x = (a * t + delta).sin();
            let y = (b * t).sin();

            if let Some(led) = self.coord_to_led(x, y) {
                let gradient_pos = (t / (2.0 * PI)) % 1.0;
                let (r, g, b) = self.get_gradient_color(gradient_pos);
                self.frame_buffer[led] = (
                    self.frame_buffer[led].0.max(r * 0.3),
                    self.frame_buffer[led].1.max(g * 0.3),
                    self.frame_buffer[led].2.max(b * 0.3),
                );
            }
        }
    }

    // Mode 2: Fibonacci Spiral
    fn render_fibonacci_spiral(&mut self, time: f64) {
        let rotation = time * 0.3;
        let pulse = 1.0 + (time * 2.0).sin() * 0.1;

        let steps = 500;
        for i in 0..steps {
            let theta = (i as f64 * 0.1) + rotation;
            let r = (0.1 * (0.2 * theta).exp()) * pulse;

            let x = r * theta.cos();
            let y = r * theta.sin();

            if let Some(led) = self.coord_to_led(x, y) {
                let gradient_pos = (theta / (2.0 * PI)) % 1.0;
                let (r, g, b) = self.get_gradient_color(gradient_pos);
                self.frame_buffer[led] = (
                    self.frame_buffer[led].0.max(r * 0.4),
                    self.frame_buffer[led].1.max(g * 0.4),
                    self.frame_buffer[led].2.max(b * 0.4),
                );
            }
        }
    }

    // Mode 3: Polar Rose
    fn render_polar_rose(&mut self, time: f64) {
        let k = 2.0 + ((time * 0.3).sin() * 3.0);

        let steps = 1000;
        for i in 0..steps {
            let theta = (i as f64 / steps as f64) * 2.0 * PI * k;
            let r = (k * theta).sin().abs();

            let x = r * theta.cos();
            let y = r * theta.sin();

            if let Some(led) = self.coord_to_led(x, y) {
                let gradient_pos = (theta / (2.0 * PI * k)) % 1.0;
                let brightness = r as f32;
                let (r, g, b) = self.get_gradient_color(gradient_pos);
                let (r, g, b) = (r * brightness, g * brightness, b * brightness);
                self.frame_buffer[led] = (
                    self.frame_buffer[led].0.max(r * 0.5),
                    self.frame_buffer[led].1.max(g * 0.5),
                    self.frame_buffer[led].2.max(b * 0.5),
                );
            }
        }
    }

    // Mode 4: Nested Polygons
    fn render_nested_polygons(&mut self, time: f64) {
        let layers = 8;
        for layer in 0..layers {
            let sides = 3 + (layer % 4);
            let scale = (1.0 / PHI).powi(layer) * (1.0 + (time * 2.0).sin() * 0.1);
            let rotation = time * 0.5 + layer as f64 * 0.3;

            for i in 0..=sides {
                let theta1 = (i as f64 / sides as f64) * 2.0 * PI + rotation;
                let theta2 = ((i + 1) as f64 / sides as f64) * 2.0 * PI + rotation;

                for step in 0..20 {
                    let alpha = step as f64 / 20.0;
                    let theta = theta1 * (1.0 - alpha) + theta2 * alpha;
                    let x = scale * theta.cos();
                    let y = scale * theta.sin();

                    if let Some(led) = self.coord_to_led(x, y) {
                        let gradient_pos = layer as f64 / layers as f64;
                        let brightness = 0.3 + 0.7 * ((time * 3.0 + layer as f64).sin() * 0.5 + 0.5);
                        let (r, g, b) = self.get_gradient_color(gradient_pos);
                        let (r, g, b) = (r * brightness as f32, g * brightness as f32, b * brightness as f32);
                        self.frame_buffer[led] = (
                            self.frame_buffer[led].0.max(r * 0.4),
                            self.frame_buffer[led].1.max(g * 0.4),
                            self.frame_buffer[led].2.max(b * 0.4),
                        );
                    }
                }
            }
        }
    }

    // Mode 5: Hypotrochoid
    fn render_hypotrochoid(&mut self, time: f64) {
        let big_r = 1.0;
        let r = 0.3 + (time * 0.2).sin() * 0.1;
        let d = 0.5 + (time * 0.15).cos() * 0.2;

        let steps = 1000;
        for i in 0..steps {
            let theta = (i as f64 / steps as f64) * 4.0 * PI;
            let x = (big_r - r) * theta.cos() + d * ((big_r - r) / r * theta).cos();
            let y = (big_r - r) * theta.sin() - d * ((big_r - r) / r * theta).sin();

            if let Some(led) = self.coord_to_led(x * 0.7, y * 0.7) {
                let gradient_pos = (theta / (4.0 * PI)) % 1.0;
                let (r, g, b) = self.get_gradient_color(gradient_pos);
                self.frame_buffer[led] = (
                    self.frame_buffer[led].0.max(r * 0.3),
                    self.frame_buffer[led].1.max(g * 0.3),
                    self.frame_buffer[led].2.max(b * 0.3),
                );
            }
        }
    }

    // Mode 6: Phyllotaxis - smooth 0→full→0 with rotation and zoom
    fn render_phyllotaxis(&mut self, time: f64) {
        // Smooth point count animation: 0 → 500 → 0 (triangle wave)
        let cycle = (time * 0.15) % 2.0;
        let max_n = if cycle < 1.0 {
            // Growing phase: 0 → 500
            (cycle * 500.0) as i32
        } else {
            // Shrinking phase: 500 → 0
            ((2.0 - cycle) * 500.0) as i32
        };

        // Rotation in place
        let rotation = time * 0.2;

        // Zoom in and out smoothly
        let zoom = 0.8 + (time * 0.3).sin() * 0.4; // 0.4 to 1.2 range

        let c = 0.05 * zoom;

        for n in 0..max_n {
            let theta = (n as f64 * GOLDEN_ANGLE).to_radians() + rotation;
            let r = c * (n as f64).sqrt();

            let x = r * theta.cos();
            let y = r * theta.sin();

            if let Some(led) = self.coord_to_led(x, y) {
                // Brightness based on position in spiral (newer = brighter)
                let age = (max_n - n) as f32 / 50.0;
                let brightness = (1.0 - age * 0.7).max(0.3);
                let gradient_pos = (n as f64 / 500_f64.max(max_n as f64)) % 1.0;
                let (r, g, b) = self.get_gradient_color(gradient_pos);
                let (r, g, b) = (r * brightness, g * brightness, b * brightness);
                self.frame_buffer[led] = (
                    self.frame_buffer[led].0.max(r * 0.5),
                    self.frame_buffer[led].1.max(g * 0.5),
                    self.frame_buffer[led].2.max(b * 0.5),
                );
            }
        }
    }

    // Mode 7: Kaleidoscope
    fn render_kaleidoscope(&mut self, time: f64) {
        let segments = 6;
        let rotation = time * 0.4;

        // Draw a simple rotating pattern
        for segment in 0..segments {
            let base_angle = (segment as f64 / segments as f64) * 2.0 * PI + rotation;

            for step in 0..100 {
                let r = (step as f64 / 100.0) * 1.0;
                let angle_offset = (time * 2.0 + step as f64 * 0.1).sin() * 0.5;
                let angle = base_angle + angle_offset;

                let x = r * angle.cos();
                let y = r * angle.sin();

                if let Some(led) = self.coord_to_led(x, y) {
                    let gradient_pos = segment as f64 / segments as f64;
                    let brightness = 1.0 - r as f32 * 0.3;
                    let (r, g, b) = self.get_gradient_color(gradient_pos);
                    let (r, g, b) = (r * brightness, g * brightness, b * brightness);
                    self.frame_buffer[led] = (
                        self.frame_buffer[led].0.max(r * 0.4),
                        self.frame_buffer[led].1.max(g * 0.4),
                        self.frame_buffer[led].2.max(b * 0.4),
                    );
                }
            }
        }
    }

    // Mode 8: Vector Field Flow
    fn render_vector_field(&mut self, time: f64) {
        let particles = 200;

        for p in 0..particles {
            let mut x = ((p as f64 / particles as f64) * 2.0 - 1.0) * 0.9;
            let mut y = ((time * 0.5 + p as f64 * 0.1).sin()) * 0.9;

            for _step in 0..30 {
                // Simple curl field
                let dx = (y * 3.0 + time).sin() * 0.04;
                let dy = (x * 3.0 + time).cos() * 0.04;

                x += dx;
                y += dy;

                if x.abs() > 1.2 || y.abs() > 1.2 {
                    break;
                }

                if let Some(led) = self.coord_to_led(x, y) {
                    let gradient_pos = ((x + 1.0) / 2.0 + time * 0.1) % 1.0;
                    let brightness = 0.6 + ((y + 1.0) / 2.0 * 0.4) as f32;
                    let (r, g, b) = self.get_gradient_color(gradient_pos);
                    let (r, g, b) = (r * brightness, g * brightness, b * brightness);
                    self.frame_buffer[led] = (
                        self.frame_buffer[led].0.max(r * 0.7),
                        self.frame_buffer[led].1.max(g * 0.7),
                        self.frame_buffer[led].2.max(b * 0.7),
                    );
                }
            }
        }
    }

    // Mode 9: Golden Starburst
    fn render_golden_starburst(&mut self, time: f64) {
        let rays = 50;

        for ray in 0..rays {
            let angle = (ray as f64 * GOLDEN_ANGLE).to_radians() + time * 0.3;
            let length_mod = (time * 2.0 + ray as f64 * 0.1).sin() * 0.5 + 0.5;
            let max_length = 0.3 + length_mod * 0.7;

            for step in 0..50 {
                let r = (step as f64 / 50.0) * max_length;
                let x = r * angle.cos();
                let y = r * angle.sin();

                if let Some(led) = self.coord_to_led(x, y) {
                    let gradient_pos = ray as f64 / rays as f64;
                    let brightness = (1.0 - r / max_length) as f32;
                    let (r, g, b) = self.get_gradient_color(gradient_pos);
                    let (r, g, b) = (r * brightness, g * brightness, b * brightness);
                    self.frame_buffer[led] = (
                        self.frame_buffer[led].0.max(r * 0.4),
                        self.frame_buffer[led].1.max(g * 0.4),
                        self.frame_buffer[led].2.max(b * 0.4),
                    );
                }
            }
        }
    }

    // Mode 10: 3D Wireframe
    fn render_wireframe_3d(&mut self, time: f64) {
        // Simple rotating cube - scaled 2x larger
        let scale = 1.2;
        let vertices = vec![
            (-scale, -scale, -scale), (scale, -scale, -scale),
            (scale, scale, -scale), (-scale, scale, -scale),
            (-scale, -scale, scale), (scale, -scale, scale),
            (scale, scale, scale), (-scale, scale, scale),
        ];

        let edges = vec![
            (0, 1), (1, 2), (2, 3), (3, 0), // Back face
            (4, 5), (5, 6), (6, 7), (7, 4), // Front face
            (0, 4), (1, 5), (2, 6), (3, 7), // Connecting edges
        ];

        // Rotation matrices
        let rot_x = time * 0.5;
        let rot_y = time * 0.7;
        let rot_z = time * 0.3;

        // Helper function to rotate and project a vertex
        let rotate_and_project = |x: f64, y: f64, z: f64| -> (f64, f64, f64) {
            // Rotate around X
            let y_rot = y * rot_x.cos() - z * rot_x.sin();
            let z_rot = y * rot_x.sin() + z * rot_x.cos();

            // Rotate around Y
            let x_rot = x * rot_y.cos() + z_rot * rot_y.sin();
            let z_rot2 = -x * rot_y.sin() + z_rot * rot_y.cos();

            // Rotate around Z
            let x_final = x_rot * rot_z.cos() - y_rot * rot_z.sin();
            let y_final = x_rot * rot_z.sin() + y_rot * rot_z.cos();

            // Project to 2D
            let k = 4.0;
            let x_proj = x_final / (z_rot2 + k);
            let y_proj = y_final / (z_rot2 + k);

            (x_proj, y_proj, z_rot2)
        };

        // Draw each edge as a line
        for &(v1_idx, v2_idx) in &edges {
            let (x1, y1, z1) = vertices[v1_idx];
            let (x2, y2, z2) = vertices[v2_idx];

            // Rotate and project both vertices
            let (x1_proj, y1_proj, z1_depth) = rotate_and_project(x1, y1, z1);
            let (x2_proj, y2_proj, z2_depth) = rotate_and_project(x2, y2, z2);

            // Average depth for color
            let avg_depth = ((z1_depth + z2_depth) / 2.0 + 1.5) / 3.0;
            let gradient_pos = avg_depth.clamp(0.0, 1.0);
            let (r, g, b) = self.get_gradient_color(gradient_pos);

            // Draw line between the two projected points
            self.draw_line(x1_proj, y1_proj, x2_proj, y2_proj, r, g, b);
        }
    }

    // Bresenham's line algorithm for drawing lines between two points
    fn draw_line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, r: f32, g: f32, b: f32) {
        // Convert normalized coords to grid coords
        let x0_grid = ((x0 + 1.0) * 0.5 * self.grid_width as f64) as i32;
        let y0_grid = ((y0 + 1.0) * 0.5 * self.grid_height as f64) as i32;
        let x1_grid = ((x1 + 1.0) * 0.5 * self.grid_width as f64) as i32;
        let y1_grid = ((y1 + 1.0) * 0.5 * self.grid_height as f64) as i32;

        let dx = (x1_grid - x0_grid).abs();
        let dy = (y1_grid - y0_grid).abs();
        let sx = if x0_grid < x1_grid { 1 } else { -1 };
        let sy = if y0_grid < y1_grid { 1 } else { -1 };
        let mut err = dx - dy;

        let mut x = x0_grid;
        let mut y = y0_grid;

        loop {
            // Plot this point
            if x >= 0 && x < self.grid_width as i32 && y >= 0 && y < self.grid_height as i32 {
                let led = (y as usize) * self.grid_width + (x as usize);
                if led < self.total_leds {
                    self.frame_buffer[led] = (
                        self.frame_buffer[led].0.max(r),
                        self.frame_buffer[led].1.max(g),
                        self.frame_buffer[led].2.max(b),
                    );
                }
            }

            if x == x1_grid && y == y1_grid {
                break;
            }

            let e2 = 2 * err;
            if e2 > -dy {
                err -= dy;
                x += sx;
            }
            if e2 < dx {
                err += dx;
                y += sy;
            }
        }
    }

    // Mode 11: Mandelbrot Set with animated zoom
    fn render_mandelbrot(&mut self, time: f64) {
        // Smooth zoom animation using known good coordinates
        let zoom_cycle = (time * 0.2).sin() * 0.5 + 0.5; // 0 to 1
        // Zoom from 2.5 (shows full set) to 0.0005 (deep detail) - exponential for smooth feel
        let zoom = 0.0005 + (1.0 - zoom_cycle).powf(2.5) * 2.5;

        // Cycle through different interesting locations
        let location_index = ((time / 15.0) % 4.0) as usize; // Switch location every 15 seconds
        let (center_x, center_y) = match location_index {
            0 => (-0.7435669, 0.1314023),  // Double Spiral Valley
            1 => (-0.743, 0.126),          // Seahorse Valley
            2 => (0.282, -0.01),           // Elephant Valley
            _ => (-0.1592, 1.0317),        // Mini Mandelbrot
        };

        for y in 0..self.grid_height {
            for x in 0..self.grid_width {
                let px = ((x as f64 / self.grid_width as f64) - 0.5) * 4.0 * zoom + center_x;
                let py = ((y as f64 / self.grid_height as f64) - 0.5) * 4.0 * zoom + center_y;

                let mut zx = 0.0;
                let mut zy = 0.0;
                let mut iteration = 0;
                let max_iter = 100;

                while zx * zx + zy * zy < 4.0 && iteration < max_iter {
                    let temp = zx * zx - zy * zy + px;
                    zy = 2.0 * zx * zy + py;
                    zx = temp;
                    iteration += 1;
                }

                if iteration < max_iter {
                    let gradient_pos = (iteration as f64 / max_iter as f64) % 1.0;
                    let (r, g, b) = self.get_gradient_color(gradient_pos);
                    let led = y * self.grid_width + x;
                    if led < self.total_leds {
                        self.frame_buffer[led] = (r, g, b);
                    }
                }
            }
        }
    }

    // Mode 12: Dragon Curve fractal - zooms forever (true fractal behavior)
    fn render_dragon_curve(&mut self, time: f64) {
        // Vary order over time for fractal zoom effect (6 to 12)
        // Slowly increase detail as we zoom in
        let order_float = 6.0 + ((time * 0.1) % 6.0);
        let order = order_float as usize;

        let mut turns = vec![1]; // Start with right turn (1)

        // Build turn sequence: at each iteration, append reverse complement
        for _ in 0..order {
            let len = turns.len();
            turns.push(1); // Middle turn is always right
            for i in (0..len).rev() {
                turns.push(-turns[i]); // Reverse and negate
            }
        }

        // Generate path from turn sequence
        let mut x = 0.0;
        let mut y = 0.0;
        let mut angle = 0.0;
        let mut path = vec![(x, y)];

        for &turn in &turns {
            angle += turn as f64 * PI / 2.0;
            x += angle.cos();
            y += angle.sin();
            path.push((x, y));
        }

        // Normalize to fit screen
        let (min_x, max_x, min_y, max_y) = path.iter().fold(
            (f64::MAX, f64::MIN, f64::MAX, f64::MIN),
            |(min_x, max_x, min_y, max_y), &(x, y)| {
                (min_x.min(x), max_x.max(x), min_y.min(y), max_y.max(y))
            }
        );

        // Continuous zoom forever - fractal behavior
        // Exponential zoom that resets smoothly every 30 seconds
        let zoom_cycle = (time * 0.15) % 1.0;
        let zoom_factor = 0.1 + zoom_cycle * zoom_cycle * 15.0; // Quadratic zoom 0.1 to 15
        let base_scale = 1.8 / (max_x - min_x).max(max_y - min_y);
        let scale = base_scale * zoom_factor;
        let rotation = time * 0.1;

        // Zoom focus point moves slightly for variety
        let focus_x = (time * 0.05).sin() * 0.2;
        let focus_y = (time * 0.07).cos() * 0.2;
        let center_x = (max_x + min_x) / 2.0 + focus_x;
        let center_y = (max_y + min_y) / 2.0 + focus_y;

        // Draw the curve
        for i in 0..path.len() - 1 {
            let x0 = (path[i].0 - center_x) * scale;
            let y0 = (path[i].1 - center_y) * scale;
            let x1 = (path[i + 1].0 - center_x) * scale;
            let y1 = (path[i + 1].1 - center_y) * scale;

            // Apply rotation
            let rx0 = x0 * rotation.cos() - y0 * rotation.sin();
            let ry0 = x0 * rotation.sin() + y0 * rotation.cos();
            let rx1 = x1 * rotation.cos() - y1 * rotation.sin();
            let ry1 = x1 * rotation.sin() + y1 * rotation.cos();

            let gradient_pos = (i as f64 / path.len() as f64 + time * 0.05) % 1.0;
            let (r, g, b) = self.get_gradient_color(gradient_pos);
            self.draw_line(rx0, ry0, rx1, ry1, r, g, b);
        }
    }

    // Mode 13: Hilbert Curve - progressive zoom
    fn render_hilbert_curve(&mut self, time: f64) {
        let phase_duration = 4.0; // 3s draw + 1s pause
        let cycle_time = (time * 0.2) % (2.0 * phase_duration); // 2 phases

        let current_phase = (cycle_time / phase_duration).floor() as usize;
        let time_in_phase = cycle_time % phase_duration;

        let is_drawing = time_in_phase < 3.0;
        let phase_progress = if is_drawing { time_in_phase / 3.0 } else { 1.0 };

        // Normalize to [-0.99, 0.99] to avoid clipping
        let normalize = |coord: i32, size: i32| -> f64 {
            (2.0 * coord as f64 / (size - 1) as f64 - 1.0) * 0.99
        };

        let base_order = 1;
        let current_order = base_order + current_phase;

        // Draw previous phase if exists (zoomed to corner)
        if current_phase > 0 {
            let prev_order = base_order;
            let prev_n = 1 << prev_order;
            let prev_segments = prev_n * prev_n - 1;

            // Calculate where the first quadrant naturally sits in the next order
            // For order N+1 with grid size n, first quadrant spans [0, (n-1)/(current_order)]
            // This maps to range [-0.99, -0.33] for n=4, which is 1/3 scale
            let scale = 1.0 / (1 << current_phase) as f64; // 1/2, 1/4, 1/8, etc.
            let offset = -0.99 * (1.0 - scale); // Position to align with natural quadrant

            for i in 0..prev_segments {
                let (x0, y0) = self.hilbert_d2xy(prev_n, i);
                let (x1, y1) = self.hilbert_d2xy(prev_n, i + 1);

                // Normalize, scale to match quadrant size, and offset to corner
                let nx0 = normalize(x0, prev_n) * scale + offset;
                let ny0 = normalize(y0, prev_n) * scale + offset;
                let nx1 = normalize(x1, prev_n) * scale + offset;
                let ny1 = normalize(y1, prev_n) * scale + offset;

                let (r, g, b) = self.get_gradient_color(i as f64 / prev_segments as f64);
                let (r, g, b) = (r * 0.4, g * 0.4, b * 0.4); // Dimmed
                self.draw_line(nx0, ny0, nx1, ny1, r, g, b);
            }
        }

        // Draw current order (skip segments already shown in previous order)
        let n = 1 << current_order;
        let total_segments = n * n - 1;
        let visible_segments = (phase_progress * total_segments as f64) as i32;

        let start_segment = if current_phase > 0 {
            let prev_n = 1 << (current_order - 1);
            prev_n * prev_n - 1 // Skip the segments from previous order
        } else {
            0
        };

        for i in start_segment..visible_segments.min(total_segments) {
            let (x0, y0) = self.hilbert_d2xy(n, i);
            let (x1, y1) = self.hilbert_d2xy(n, i + 1);

            let nx0 = normalize(x0, n);
            let ny0 = normalize(y0, n);
            let nx1 = normalize(x1, n);
            let ny1 = normalize(y1, n);

            let progress = (current_phase as f64 + i as f64 / total_segments as f64) / 2.0;
            let distance_from_edge = (i as f64 / total_segments as f64 - phase_progress).abs();
            let brightness = if is_drawing && distance_from_edge < 0.05 { 1.0 } else { 0.75 };

            let (r, g, b) = self.get_gradient_color(progress);
            let (r, g, b) = (r * brightness as f32, g * brightness as f32, b * brightness as f32);
            self.draw_line(nx0, ny0, nx1, ny1, r, g, b);
        }
    }

    // Helper for Hilbert curve
    fn hilbert_d2xy(&self, n: i32, d: i32) -> (i32, i32) {
        let mut x = 0;
        let mut y = 0;
        let mut s = 1;
        let mut d = d;

        while s < n {
            let rx = 1 & (d / 2);
            let ry = 1 & (d ^ rx);
            let (nx, ny) = self.hilbert_rot(s, x, y, rx, ry);
            x = nx + s * rx;
            y = ny + s * ry;
            d /= 4;
            s *= 2;
        }
        (x, y)
    }

    fn hilbert_rot(&self, n: i32, x: i32, y: i32, rx: i32, ry: i32) -> (i32, i32) {
        if ry == 0 {
            if rx == 1 {
                return (n - 1 - y, n - 1 - x);
            }
            return (y, x);
        }
        (x, y)
    }

    // Mode 14: Sierpinski Triangle with zoom into surface
    fn render_sierpinski(&mut self, time: f64) {
        let depth = 8;

        // Full cycle: zoom in (10s) → zoom out (10s) = 20 seconds per location
        let cycle_time = (time * 0.1) % 2.0;
        let location_index = ((time * 0.1 / 2.0) as usize) % 5; // Change location every 20 seconds

        // Zoom: 1.2 (out) → 0.001 (surface) → 1.2 (out)
        let zoom = if cycle_time < 1.0 {
            // Zoom in: exponential for smooth approach to surface
            1.2 - (cycle_time.powf(2.0) * 1.199)
        } else {
            // Zoom out: reverse
            0.001 + ((cycle_time - 1.0).powf(2.0) * 1.199)
        };

        // Pick different focus points on the fractal surface
        let (focus_x, focus_y) = match location_index {
            0 => (0.0, 0.5),       // Top vertex area
            1 => (-0.4, -0.3),     // Bottom left area
            2 => (0.4, -0.3),      // Bottom right area
            3 => (-0.2, 0.1),      // Left edge area
            _ => (0.2, 0.1),       // Right edge area
        };

        // Slow rotation
        let rotation = time * 0.05;

        // Base triangle vertices
        let v0 = (rotation.cos(), rotation.sin());
        let v1 = ((rotation + 2.0 * PI / 3.0).cos(), (rotation + 2.0 * PI / 3.0).sin());
        let v2 = ((rotation + 4.0 * PI / 3.0).cos(), (rotation + 4.0 * PI / 3.0).sin());

        // Apply zoom focused on the chosen point
        let v0_zoomed = ((v0.0 - focus_x) * zoom + focus_x, (v0.1 - focus_y) * zoom + focus_y);
        let v1_zoomed = ((v1.0 - focus_x) * zoom + focus_x, (v1.1 - focus_y) * zoom + focus_y);
        let v2_zoomed = ((v2.0 - focus_x) * zoom + focus_x, (v2.1 - focus_y) * zoom + focus_y);

        self.sierpinski_recursive(v0_zoomed, v1_zoomed, v2_zoomed, depth, time);
    }

    fn sierpinski_recursive(&mut self, v0: (f64, f64), v1: (f64, f64), v2: (f64, f64), depth: usize, time: f64) {
        if depth == 0 {
            let gradient_pos = ((v0.0 + v0.1 + time) * 0.5) % 1.0;
            let (r, g, b) = self.get_gradient_color(gradient_pos);
            self.draw_line(v0.0, v0.1, v1.0, v1.1, r, g, b);
            self.draw_line(v1.0, v1.1, v2.0, v2.1, r, g, b);
            self.draw_line(v2.0, v2.1, v0.0, v0.1, r, g, b);
        } else {
            let m0 = ((v0.0 + v1.0) / 2.0, (v0.1 + v1.1) / 2.0);
            let m1 = ((v1.0 + v2.0) / 2.0, (v1.1 + v2.1) / 2.0);
            let m2 = ((v2.0 + v0.0) / 2.0, (v2.1 + v0.1) / 2.0);

            self.sierpinski_recursive(v0, m0, m2, depth - 1, time);
            self.sierpinski_recursive(m0, v1, m1, depth - 1, time);
            self.sierpinski_recursive(m2, m1, v2, depth - 1, time);
        }
    }

    // Mode 15: Fourier Epicycles
    fn render_fourier_epicycles(&mut self, time: f64) {
        let num_circles = 7;
        let mut x = 0.0;
        let mut y = 0.0;
        let scale = 0.5; // Zoom out

        // Draw epicycles
        for i in 0..num_circles {
            let freq = (i + 1) as f64;
            let radius = 0.8 / freq * scale;
            let phase = time * freq * 0.5;

            let new_x = x + radius * phase.cos();
            let new_y = y + radius * phase.sin();

            // Draw circle
            let circle_steps = 30;
            for j in 0..circle_steps {
                let angle1 = 2.0 * PI * j as f64 / circle_steps as f64;
                let angle2 = 2.0 * PI * (j + 1) as f64 / circle_steps as f64;

                let cx1 = x + radius * angle1.cos();
                let cy1 = y + radius * angle1.sin();
                let cx2 = x + radius * angle2.cos();
                let cy2 = y + radius * angle2.sin();

                let gradient_pos = i as f64 / num_circles as f64;
                let (r, g, b) = self.get_gradient_color(gradient_pos);
                let (r, g, b) = (r * 0.5, g * 0.5, b * 0.5);
                self.draw_line(cx1, cy1, cx2, cy2, r, g, b);
            }

            // Draw radius line
            let gradient_pos = i as f64 / num_circles as f64;
            let (r, g, b) = self.get_gradient_color(gradient_pos);
            self.draw_line(x, y, new_x, new_y, r, g, b);

            x = new_x;
            y = new_y;
        }

        // Draw traced point
        if let Some(led) = self.coord_to_led(x, y) {
            self.frame_buffer[led] = (1.0, 1.0, 1.0);
        }
    }

    // Mode 16: Strange Attractor (Lorenz system)
    fn render_strange_attractor(&mut self, time: f64) {
        // Lorenz attractor parameters
        let sigma = 10.0;
        let rho = 28.0;
        let beta = 8.0 / 3.0;
        let dt = 0.01;

        let mut x = 0.1;
        let mut y = 0.0;
        let mut z = 0.0;

        let rotation = time * 0.3;

        for i in 0..3000 {
            let dx = sigma * (y - x);
            let dy = x * (rho - z) - y;
            let dz = x * y - beta * z;

            x += dx * dt;
            y += dy * dt;
            z += dz * dt;

            // Rotate and project to 2D with proper centering
            let rx = x * rotation.cos() - y * rotation.sin();
            let nx = rx / 25.0;
            let ny = (z - 25.0) / 25.0; // Center z around 25 (typical Lorenz z center)

            if let Some(led) = self.coord_to_led(nx, ny) {
                let gradient_pos = ((i as f64 / 3000.0) + time * 0.05) % 1.0;
                let brightness = 0.2 + (i as f32 / 3000.0) * 0.8;
                let (r, g, b) = self.get_gradient_color(gradient_pos);
                let (r, g, b) = (r * brightness, g * brightness, b * brightness);
                self.frame_buffer[led] = (
                    self.frame_buffer[led].0.max(r * 0.6),
                    self.frame_buffer[led].1.max(g * 0.6),
                    self.frame_buffer[led].2.max(b * 0.6),
                );
            }
        }
    }

    // Mode 17: Boids (flocking simulation with separation, alignment, cohesion)
    fn render_boids(&mut self, _time: f64) {
        let num_boids = self.boids.len();

        // Flocking parameters from config
        let separation_dist = self.boid_separation_distance;
        let alignment_dist = self.boid_alignment_distance;
        let cohesion_dist = self.boid_cohesion_distance;
        let max_speed = self.boid_max_speed;
        let max_force = self.boid_max_force;

        // Predator-prey parameters
        let avoidance_dist = self.boid_avoidance_distance;
        let chase_force = self.boid_chase_force;
        let predator_speed = self.boid_predator_speed;

        // Calculate steering forces for each boid
        let mut forces: Vec<(f64, f64)> = Vec::with_capacity(num_boids);

        for i in 0..num_boids {
            let is_predator = self.boids[i].is_predator;

            let mut sep_x = 0.0;
            let mut sep_y = 0.0;
            let mut sep_count = 0;

            let mut align_x = 0.0;
            let mut align_y = 0.0;
            let mut align_count = 0;

            let mut coh_x = 0.0;
            let mut coh_y = 0.0;
            let mut coh_count = 0;

            let mut avoid_x = 0.0;
            let mut avoid_y = 0.0;
            let mut avoid_count = 0;

            let mut chase_x = 0.0;
            let mut chase_y = 0.0;
            let mut chase_count = 0;

            for j in 0..num_boids {
                if i == j { continue; }

                // Simple distance calculation (no wrapping)
                let dx = self.boids[i].x - self.boids[j].x;
                let dy = self.boids[i].y - self.boids[j].y;
                let dist = (dx * dx + dy * dy).sqrt();

                // Predator-prey behavior
                if self.boid_predator_enabled {
                    if is_predator {
                        // Predators chase prey
                        if !self.boids[j].is_predator && dist < avoidance_dist * 2.0 {
                            chase_x += self.boids[j].x;
                            chase_y += self.boids[j].y;
                            chase_count += 1;
                        }
                    } else {
                        // Prey avoid predators
                        if self.boids[j].is_predator && dist < avoidance_dist {
                            avoid_x += dx / dist;
                            avoid_y += dy / dist;
                            avoid_count += 1;
                        }
                    }
                }

                // Regular flocking behavior (only with same type)
                if is_predator == self.boids[j].is_predator {
                    // Separation: steer away from nearby boids
                    if dist < separation_dist && dist > 0.0 {
                        sep_x += dx / dist;
                        sep_y += dy / dist;
                        sep_count += 1;
                    }

                    // Alignment: steer towards average heading
                    if dist < alignment_dist {
                        align_x += self.boids[j].vx;
                        align_y += self.boids[j].vy;
                        align_count += 1;
                    }

                    // Cohesion: steer towards center of mass
                    if dist < cohesion_dist {
                        coh_x += self.boids[j].x;
                        coh_y += self.boids[j].y;
                        coh_count += 1;
                    }
                }
            }

            let mut fx = 0.0;
            let mut fy = 0.0;

            // Avoidance (prey fleeing predators) - highest priority
            if avoid_count > 0 {
                avoid_x /= avoid_count as f64;
                avoid_y /= avoid_count as f64;
                let avoid_mag = (avoid_x * avoid_x + avoid_y * avoid_y).sqrt();
                if avoid_mag > 0.0 {
                    fx += (avoid_x / avoid_mag) * max_force * 3.0;  // Strong avoidance
                    fy += (avoid_y / avoid_mag) * max_force * 3.0;
                }
            }

            // Chase (predators pursuing prey)
            if chase_count > 0 {
                chase_x /= chase_count as f64;
                chase_y /= chase_count as f64;
                let steer_x = chase_x - self.boids[i].x;
                let steer_y = chase_y - self.boids[i].y;
                fx += steer_x * chase_force;
                fy += steer_y * chase_force;
            }

            if sep_count > 0 {
                sep_x /= sep_count as f64;
                sep_y /= sep_count as f64;
                let sep_mag = (sep_x * sep_x + sep_y * sep_y).sqrt();
                if sep_mag > 0.0 {
                    fx += (sep_x / sep_mag) * max_force * 1.5;
                    fy += (sep_y / sep_mag) * max_force * 1.5;
                }
            }

            if align_count > 0 {
                align_x /= align_count as f64;
                align_y /= align_count as f64;
                fx += align_x * max_force * 1.0;
                fy += align_y * max_force * 1.0;
            }

            if coh_count > 0 {
                coh_x /= coh_count as f64;
                coh_y /= coh_count as f64;
                let steer_x = coh_x - self.boids[i].x;
                let steer_y = coh_y - self.boids[i].y;
                fx += steer_x * max_force * 1.0;
                fy += steer_y * max_force * 1.0;
            }

            forces.push((fx, fy));
        }

        // Update boid positions and velocities
        for i in 0..num_boids {
            self.boids[i].vx += forces[i].0;
            self.boids[i].vy += forces[i].1;

            // Normalize to constant speed (forces only change direction, not magnitude)
            let constant_speed = if self.boids[i].is_predator { predator_speed } else { max_speed };
            let speed = (self.boids[i].vx * self.boids[i].vx + self.boids[i].vy * self.boids[i].vy).sqrt();
            if speed > 0.0 {
                self.boids[i].vx = (self.boids[i].vx / speed) * constant_speed;
                self.boids[i].vy = (self.boids[i].vy / speed) * constant_speed;
            }

            self.boids[i].x += self.boids[i].vx;
            self.boids[i].y += self.boids[i].vy;

            // Wrap around edges - seamlessly teleport to opposite side
            while self.boids[i].x > 1.0 { self.boids[i].x -= 2.0; }
            while self.boids[i].x < -1.0 { self.boids[i].x += 2.0; }
            while self.boids[i].y > 1.0 { self.boids[i].y -= 2.0; }
            while self.boids[i].y < -1.0 { self.boids[i].y += 2.0; }
        }

        // Draw boids as single pixels
        for (i, boid) in self.boids.iter().enumerate() {
            if let Some(led) = self.coord_to_led(boid.x, boid.y) {
                let (r, g, b) = if boid.is_predator {
                    // Predators are bright red
                    (1.0, 0.0, 0.0)
                } else {
                    // Prey use gradient coloring
                    let gradient_pos = (i as f64 / num_boids as f64) % 1.0;
                    self.get_gradient_color(gradient_pos)
                };
                self.frame_buffer[led] = (
                    self.frame_buffer[led].0.max(r),
                    self.frame_buffer[led].1.max(g),
                    self.frame_buffer[led].2.max(b),
                );
            }
        }
    }

    // Mode 18: Penrose Tiling
    fn render_penrose_tiling(&mut self, time: f64) {
        let rotation = time * 0.1;
        let num_triangles = 40;

        for i in 0..num_triangles {
            let angle = (i as f64 * GOLDEN_ANGLE * PI / 180.0) + rotation;
            let radius = 0.3 + (i as f64 / num_triangles as f64) * 0.6;

            let x = radius * angle.cos();
            let y = radius * angle.sin();

            // Draw rhombus tiles
            let size = 0.15;
            let a1 = angle + PI / 5.0;
            let a2 = angle - PI / 5.0;

            let p1x = x + size * a1.cos();
            let p1y = y + size * a1.sin();
            let p2x = x + size * a2.cos();
            let p2y = y + size * a2.sin();
            let p3x = x - size * a1.cos();
            let p3y = y - size * a1.sin();
            let p4x = x - size * a2.cos();
            let p4y = y - size * a2.sin();

            let t = (i as f64 / num_triangles as f64) % 1.0;
            let (r, g, b) = self.get_gradient_color(t);

            self.draw_line(p1x, p1y, p2x, p2y, r, g, b);
            self.draw_line(p2x, p2y, p3x, p3y, r, g, b);
            self.draw_line(p3x, p3y, p4x, p4y, r, g, b);
            self.draw_line(p4x, p4y, p1x, p1y, r, g, b);
        }
    }

    // Mode 19: Metaballs
    fn render_metaballs(&mut self, time: f64) {
        let num_balls = 5;
        let mut balls = Vec::new();

        // Generate metaball positions
        for i in 0..num_balls {
            let offset = i as f64 * 2.1;
            let x = (time * 0.5 + offset).cos() * 0.5;
            let y = (time * 0.7 + offset * 1.3).sin() * 0.5;
            let radius = 0.3 + ((time + offset).sin() * 0.1);
            balls.push((x, y, radius));
        }

        // Render metaball field
        for y in 0..self.grid_height {
            for x in 0..self.grid_width {
                let px = (x as f64 / self.grid_width as f64) * 2.0 - 1.0;
                let py = (y as f64 / self.grid_height as f64) * 2.0 - 1.0;

                let mut field = 0.0;
                for &(bx, by, r) in &balls {
                    let dx = px - bx;
                    let dy = py - by;
                    let dist_sq = dx * dx + dy * dy;
                    field += r * r / dist_sq.max(0.01);
                }

                if field > 1.0 {
                    let t = (field * 0.2 + time * 0.1) % 1.0;
                    let brightness = (field.min(3.0) / 3.0) as f32;
                    let (mut r, mut g, mut b) = self.get_gradient_color(t);
                    // Apply brightness scaling
                    r = (r * brightness).min(1.0);
                    g = (g * brightness).min(1.0);
                    b = (b * brightness).min(1.0);

                    let led = y * self.grid_width + x;
                    if led < self.total_leds {
                        self.frame_buffer[led] = (r, g, b);
                    }
                }
            }
        }
    }

    // Mode 20: Icosahedron (20-sided polyhedron) with zoom
    fn render_icosahedron(&mut self, time: f64) {
        let phi = PHI;

        // Dramatic zoom going deep to the center/core of the shape
        // Range: 0.4 (outside view) to 30.0 (deep into center/core)
        let zoom = 15.2 + (time * 0.4).sin() * 14.8; // Range: 0.4 to 30.0
        let scale = zoom;

        // Icosahedron vertices
        let vertices = vec![
            (0.0, 1.0, phi), (0.0, -1.0, phi), (0.0, 1.0, -phi), (0.0, -1.0, -phi),
            (1.0, phi, 0.0), (-1.0, phi, 0.0), (1.0, -phi, 0.0), (-1.0, -phi, 0.0),
            (phi, 0.0, 1.0), (-phi, 0.0, 1.0), (phi, 0.0, -1.0), (-phi, 0.0, -1.0),
        ];

        // Icosahedron edges
        let edges = vec![
            (0, 1), (0, 4), (0, 5), (0, 8), (0, 9),
            (1, 6), (1, 7), (1, 8), (1, 9),
            (2, 3), (2, 4), (2, 5), (2, 10), (2, 11),
            (3, 6), (3, 7), (3, 10), (3, 11),
            (4, 5), (4, 8), (4, 10),
            (5, 9), (5, 11),
            (6, 7), (6, 8), (6, 10),
            (7, 9), (7, 11),
            (8, 10), (9, 11),
        ];

        let rot_x = time * 0.4;
        let rot_y = time * 0.6;
        let rot_z = time * 0.3;

        for &(v1_idx, v2_idx) in &edges {
            let (x1, y1, z1) = vertices[v1_idx];
            let (x2, y2, z2) = vertices[v2_idx];

            let project = |x: f64, y: f64, z: f64| -> (f64, f64, f64) {
                // Rotate X
                let y_rot = y * rot_x.cos() - z * rot_x.sin();
                let z_rot = y * rot_x.sin() + z * rot_x.cos();
                // Rotate Y
                let x_rot = x * rot_y.cos() + z_rot * rot_y.sin();
                let z_rot2 = -x * rot_y.sin() + z_rot * rot_y.cos();
                // Rotate Z
                let x_final = x_rot * rot_z.cos() - y_rot * rot_z.sin();
                let y_final = x_rot * rot_z.sin() + y_rot * rot_z.cos();
                // Project
                let k = 5.0;
                (x_final * scale / (z_rot2 + k), y_final * scale / (z_rot2 + k), z_rot2)
            };

            let (x1_proj, y1_proj, z1_depth) = project(x1, y1, z1);
            let (x2_proj, y2_proj, z2_depth) = project(x2, y2, z2);

            let avg_depth = ((z1_depth + z2_depth) / 2.0 + 2.0) / 4.0;
            let t = avg_depth.clamp(0.0, 1.0);
            let (r, g, b) = self.get_gradient_color(t);

            self.draw_line(x1_proj, y1_proj, x2_proj, y2_proj, r, g, b);
        }
    }
}
