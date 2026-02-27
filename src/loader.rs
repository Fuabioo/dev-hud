use iced::widget::{image as iced_image, svg};
use iced::Font;
use image::AnimationDecoder;

use crate::events::ToolCategory;

pub(crate) const LOADER_IMAGE_SIZE: f32 = 20.0;
pub(crate) const SVG_FRAME_COUNT: usize = 12;

const LOADER_GIF_BYTES: &[u8] = include_bytes!("../assets/loader.gif");

// --- Embedded Fonts ---

pub(crate) const FONT_JETBRAINSMONO_BYTES: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");
pub(crate) const FONT_SPACEMONO_BYTES: &[u8] =
    include_bytes!("../assets/fonts/SpaceMonoNerdFont-Regular.ttf");

pub(crate) const fn nerd_font(name: &'static str) -> Font {
    Font {
        family: iced::font::Family::Name(name),
        weight: iced::font::Weight::Normal,
        stretch: iced::font::Stretch::Normal,
        style: iced::font::Style::Normal,
    }
}

pub(crate) const FONT_OPTIONS: &[(&str, Font)] = &[
    ("jetbrainsmono", nerd_font("JetBrainsMono Nerd Font")),
    ("spacemono", nerd_font("SpaceMono Nerd Font")),
    ("system mono", Font::MONOSPACE),
];

// --- Loader Widget ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoaderStyle {
    Braille,
    Bounce,
    Pipe,
    Gif,
    Svg,
}

impl LoaderStyle {
    pub(crate) const ALL: [LoaderStyle; 5] = [
        LoaderStyle::Braille,
        LoaderStyle::Bounce,
        LoaderStyle::Pipe,
        LoaderStyle::Gif,
        LoaderStyle::Svg,
    ];

    pub(crate) fn text_frames(self) -> &'static [&'static str] {
        match self {
            LoaderStyle::Braille => &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            LoaderStyle::Bounce => &[
                "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█", "▇", "▆", "▅", "▄", "▃", "▂",
            ],
            LoaderStyle::Pipe => &["|", "/", "-", "\\"],
            LoaderStyle::Gif | LoaderStyle::Svg => &[],
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            LoaderStyle::Braille => "braille",
            LoaderStyle::Bounce => "bounce",
            LoaderStyle::Pipe => "pipe",
            LoaderStyle::Gif => "gif",
            LoaderStyle::Svg => "svg",
        }
    }

    fn next(self) -> LoaderStyle {
        let idx = LoaderStyle::ALL
            .iter()
            .position(|&s| s == self)
            .unwrap_or(0);
        LoaderStyle::ALL[(idx + 1) % LoaderStyle::ALL.len()]
    }
}

pub(crate) struct DemoLoader {
    pub(crate) style: LoaderStyle,
    pub(crate) frame: usize,
    pub(crate) gif_frames: Vec<iced_image::Handle>,
    pub(crate) svg_frames: Vec<svg::Handle>,
}

impl DemoLoader {
    pub(crate) fn new() -> Self {
        let gif_frames = decode_gif_frames();
        let svg_frames = generate_svg_frames(SVG_FRAME_COUNT);
        eprintln!(
            "[dev-hud] loader assets: {} gif frames, {} svg frames",
            gif_frames.len(),
            svg_frames.len()
        );
        Self {
            style: LoaderStyle::Braille,
            frame: 0,
            gif_frames,
            svg_frames,
        }
    }

    fn frame_count(&self) -> usize {
        match self.style {
            LoaderStyle::Braille | LoaderStyle::Bounce | LoaderStyle::Pipe => {
                self.style.text_frames().len()
            }
            LoaderStyle::Gif => self.gif_frames.len().max(1),
            LoaderStyle::Svg => self.svg_frames.len().max(1),
        }
    }

    pub(crate) fn tick(&mut self) {
        self.frame = (self.frame + 1) % self.frame_count();
    }

    pub(crate) fn cycle_style(&mut self) {
        self.style = self.style.next();
        self.frame = 0;
    }
}

fn decode_gif_frames() -> Vec<iced_image::Handle> {
    let cursor = std::io::Cursor::new(LOADER_GIF_BYTES);
    let decoder = match image::codecs::gif::GifDecoder::new(cursor) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[dev-hud] failed to decode loader.gif: {e}");
            return Vec::new();
        }
    };
    match decoder.into_frames().collect_frames() {
        Ok(frames) => frames
            .iter()
            .map(|f| {
                let buf = f.buffer();
                let (w, h) = (buf.width(), buf.height());
                iced_image::Handle::from_rgba(w, h, buf.as_raw().clone())
            })
            .collect(),
        Err(e) => {
            eprintln!("[dev-hud] failed to collect gif frames: {e}");
            Vec::new()
        }
    }
}

fn generate_svg_frames(n: usize) -> Vec<svg::Handle> {
    (0..n)
        .map(|i| {
            let angle = (i as f64 / n as f64) * 360.0;
            let content = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128">
  <line x1="64" y1="16" x2="64" y2="56"
    stroke="white" stroke-width="6" stroke-linecap="round"
    transform="rotate({angle} 64 64)"/>
</svg>"#
            );
            svg::Handle::from_memory(content.into_bytes())
        })
        .collect()
}

// --- Tool-State Loader Frame Arrays ---

pub(crate) fn tool_state_frames(category: ToolCategory) -> &'static [&'static str] {
    match category {
        ToolCategory::Reading => &["◉", "◎", "○", "◎"],
        ToolCategory::Writing => &["✎", "✏", "✐", "✎"],
        ToolCategory::Running => &["⚙", "⚙︎", "⚙", "⚙︎"],
        ToolCategory::Thinking => &["◐", "◓", "◑", "◒"],
        ToolCategory::Spawning => &["◇", "◆", "◇", "◆"],
        ToolCategory::Web => &["◌", "◍", "●", "◍"],
        ToolCategory::Mcp => &["⚡", "✦", "⚡", "✦"],
        ToolCategory::Awaiting => &["❯", "❯❯", "❯❯❯", "❯❯"],
        ToolCategory::Unknown => &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
    }
}
