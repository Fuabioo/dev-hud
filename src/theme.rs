use iced::{Background, Color};

/// How the theme is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
    /// Follow the desktop environment / system theme (updates dynamically).
    Auto,
    /// Sample the screen under the HUD to pick theme automatically.
    Adaptive,
}

/// All colors and font sizes used throughout the HUD, derived from the active theme.
pub struct ThemeColors {
    pub is_dark: bool,
    // Text
    pub marker: Color,
    pub muted: Color,
    pub hover_text: Color,
    pub error: Color,
    pub approval: Color,
    // Backgrounds
    pub modal_bg: Color,
    pub detail_bg: Color,
    pub selected: Color,
    pub hover: Color,
    pub hud_backdrop: Color,
    // Font sizes (logical pixels)
    /// Corner markers, base for modal titles (title = marker_size * 0.7)
    pub marker_size: f32,
    /// Main widget content: sessions, activity entries (overlay)
    pub widget_text: f32,
    /// Modal content: activity log entries, detail text (focused view)
    pub modal_text: f32,
    /// Loader labels, auxiliary UI text
    pub label_text: f32,
    /// Version/info line at the bottom
    pub info_text: f32,
}

impl ThemeColors {
    /// Dark theme — white text on transparent, dark overlays.
    /// Matches the original hardcoded values from main.rs.
    pub fn dark() -> Self {
        Self {
            is_dark: true,
            marker: Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.85,
            },
            muted: Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.4,
            },
            hover_text: Color {
                r: 1.0,
                g: 0.78,
                b: 0.0,
                a: 1.0,
            },
            error: Color {
                r: 0.9,
                g: 0.2,
                b: 0.2,
                a: 1.0,
            },
            approval: Color {
                r: 1.0,
                g: 0.46,
                b: 0.15,
                a: 1.0,
            },
            modal_bg: Color {
                r: 0.05,
                g: 0.05,
                b: 0.08,
                a: 0.92,
            },
            detail_bg: Color {
                r: 0.08,
                g: 0.08,
                b: 0.12,
                a: 0.6,
            },
            selected: Color {
                r: 0.15,
                g: 0.15,
                b: 0.22,
                a: 0.8,
            },
            hover: Color {
                r: 0.12,
                g: 0.12,
                b: 0.18,
                a: 0.6,
            },
            hud_backdrop: Color {
                r: 0.05,
                g: 0.05,
                b: 0.08,
                a: 0.65,
            },
            marker_size: 16.0,
            widget_text: 8.0,
            modal_text: 24.0,
            label_text: 24.0,
            info_text: 8.0,
        }
    }

    /// Light theme — dark text on light overlays.
    pub fn light() -> Self {
        Self {
            is_dark: false,
            marker: Color {
                r: 0.08,
                g: 0.08,
                b: 0.08,
                a: 0.9,
            },
            muted: Color {
                r: 0.35,
                g: 0.35,
                b: 0.35,
                a: 0.8,
            },
            hover_text: Color {
                r: 0.6,
                g: 0.35,
                b: 0.0,
                a: 1.0,
            },
            error: Color {
                r: 0.75,
                g: 0.1,
                b: 0.1,
                a: 1.0,
            },
            approval: Color {
                r: 0.7,
                g: 0.3,
                b: 0.0,
                a: 1.0,
            },
            modal_bg: Color {
                r: 0.92,
                g: 0.92,
                b: 0.95,
                a: 0.93,
            },
            detail_bg: Color {
                r: 0.85,
                g: 0.85,
                b: 0.90,
                a: 0.7,
            },
            selected: Color {
                r: 0.75,
                g: 0.75,
                b: 0.85,
                a: 0.8,
            },
            hover: Color {
                r: 0.80,
                g: 0.80,
                b: 0.88,
                a: 0.6,
            },
            hud_backdrop: Color {
                r: 0.95,
                g: 0.95,
                b: 0.95,
                a: 0.65,
            },
            marker_size: 24.0,
            widget_text: 9.5,
            modal_text: 11.0,
            label_text: 12.0,
            info_text: 7.2,
        }
    }

    pub fn modal_bg_style(&self) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
        let color = self.modal_bg;
        move |_theme: &iced::Theme| iced::widget::container::Style {
            background: Some(Background::Color(color)),
            ..Default::default()
        }
    }

    pub fn detail_bg_style(&self) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
        let color = self.detail_bg;
        move |_theme: &iced::Theme| iced::widget::container::Style {
            background: Some(Background::Color(color)),
            ..Default::default()
        }
    }

    pub fn selected_style(&self) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
        let color = self.selected;
        move |_theme: &iced::Theme| iced::widget::container::Style {
            background: Some(Background::Color(color)),
            ..Default::default()
        }
    }

    pub fn hover_style(&self) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
        let color = self.hover;
        move |_theme: &iced::Theme| iced::widget::container::Style {
            background: Some(Background::Color(color)),
            ..Default::default()
        }
    }

    pub fn hud_backdrop_style(&self) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
        let color = self.hud_backdrop;
        move |_theme: &iced::Theme| iced::widget::container::Style {
            background: Some(Background::Color(color)),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Detect system dark mode using the claude-viz detection cascade.
/// Spawns CLI tools synchronously; avoid calling from the main UI thread
/// in tight loops.
pub fn detect_system_dark() -> bool {
    // 1. COSMIC DE: read the is_dark file directly
    if let Some(home) = dirs::home_dir() {
        let cosmic_path = home.join(".config/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark");
        if let Ok(contents) = std::fs::read_to_string(&cosmic_path) {
            let trimmed = contents.trim();
            if trimmed == "true" {
                return true;
            }
            if trimmed == "false" {
                return false;
            }
        }
    }

    // 2. XDG Desktop Portal (COSMIC, GNOME 42+, KDE 5.24+)
    //    color-scheme: 0=no preference, 1=dark, 2=light
    if let Ok(output) = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply=literal",
            "--dest=org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings.ReadOne",
            "string:org.freedesktop.appearance",
            "string:color-scheme",
        ])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("uint32 1") {
                return true;
            }
            if stdout.contains("uint32 2") {
                return false;
            }
        }
    }

    // 3. gsettings color-scheme (GNOME 42+)
    if let Ok(output) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("prefer-dark") {
            return true;
        }
        if stdout.contains("prefer-light") || stdout.contains("default") {
            return false;
        }
    }

    // 4. gsettings gtk-theme name (older GNOME)
    if let Ok(output) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
        if stdout.contains("dark") {
            return true;
        }
        if output.status.success() && !stdout.trim().is_empty() {
            return false;
        }
    }

    // 5. GTK_THEME env var (e.g. "Adwaita:dark")
    if let Ok(val) = std::env::var("GTK_THEME") {
        return val.to_lowercase().contains("dark");
    }

    // 6. All detection failed, default to dark
    true
}

/// Sample background luminance by capturing a screenshot and computing the
/// average perceptual luminance of the bottom-left quadrant (where HUD sessions
/// render). Tries `grim` first, falls back to `cosmic-screenshot`.
/// Returns None if no screenshot tool is available.
pub fn sample_bg_luminance() -> Option<f32> {
    // Try grim first (wlroots compositors: sway, wayfire, etc.)
    if let Some(img) = capture_via_grim() {
        return Some(luminance_bottom_left(&img));
    }
    // Fall back to cosmic-screenshot (COSMIC DE)
    if let Some(img) = capture_via_cosmic() {
        return Some(luminance_bottom_left(&img));
    }
    eprintln!("[dev-hud] adaptive: no screenshot tool found (tried grim, cosmic-screenshot)");
    None
}

fn capture_via_grim() -> Option<image::DynamicImage> {
    let output = std::process::Command::new("grim")
        .args(["-s", "0.1", "-t", "png", "-"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }
    image::load_from_memory(&output.stdout).ok()
}

fn capture_via_cosmic() -> Option<image::DynamicImage> {
    let output = std::process::Command::new("cosmic-screenshot")
        .args([
            "--interactive=false",
            "--modal=false",
            "--notify=false",
            "-s",
            "/tmp",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    let img = image::open(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    Some(img)
}

/// Compute average perceptual luminance of the bottom-left quadrant.
/// This is where HUD session rows typically render (above the bottom markers,
/// left-aligned). Uses stride-4 sampling for efficiency on large images.
fn luminance_bottom_left(img: &image::DynamicImage) -> f32 {
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let pixels = rgba.as_raw();

    // Bottom-left quadrant
    let x_end = w / 2;
    let y_start = h / 2;
    let stride = 4;

    let mut total_lum: f64 = 0.0;
    let mut count: usize = 0;

    for y in (y_start..h).step_by(stride) {
        for x in (0..x_end).step_by(stride) {
            let idx = (y * w + x) * 4;
            if idx + 2 >= pixels.len() {
                continue;
            }
            let r = pixels[idx] as f64 / 255.0;
            let g = pixels[idx + 1] as f64 / 255.0;
            let b = pixels[idx + 2] as f64 / 255.0;
            total_lum += 0.2126 * r + 0.7152 * g + 0.0722 * b;
            count += 1;
        }
    }

    if count == 0 {
        return 0.5;
    }

    let lum = (total_lum / count as f64) as f32;
    eprintln!("[dev-hud] adaptive: luminance = {lum:.3} ({count} samples from {w}x{h})");
    lum
}

/// Resolve the initial ThemeColors for a given mode.
pub fn resolve(mode: ThemeMode) -> ThemeColors {
    match mode {
        ThemeMode::Dark => ThemeColors::dark(),
        ThemeMode::Light => ThemeColors::light(),
        ThemeMode::Auto | ThemeMode::Adaptive => {
            if detect_system_dark() {
                ThemeColors::dark()
            } else {
                ThemeColors::light()
            }
        }
    }
}
