use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings};

use crate::util;

fn make_output_option(output: Option<&str>) -> iced_layershell::reexport::OutputOption {
    match output {
        Some(name) => iced_layershell::reexport::OutputOption::OutputName(name.to_string()),
        None => iced_layershell::reexport::OutputOption::None,
    }
}

pub(crate) fn visible_settings(output: Option<&str>) -> NewLayerShellSettings {
    NewLayerShellSettings {
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        keyboard_interactivity: KeyboardInteractivity::None,
        exclusive_zone: Some(-1),
        size: Some((0, 0)),
        events_transparent: true,
        output_option: make_output_option(output),
        ..Default::default()
    }
}

pub(crate) fn focused_settings(output: Option<&str>) -> NewLayerShellSettings {
    NewLayerShellSettings {
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        exclusive_zone: Some(-1),
        size: Some((0, 0)),
        events_transparent: false,
        output_option: make_output_option(output),
        ..Default::default()
    }
}

pub(crate) fn modal_settings(output: Option<&str>) -> NewLayerShellSettings {
    NewLayerShellSettings {
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        exclusive_zone: Some(-1),
        size: Some((0, 0)),
        margin: Some((50, 50, 50, 50)),
        events_transparent: false,
        output_option: make_output_option(output),
        ..Default::default()
    }
}

/// Query available Wayland outputs. Tries cosmic-randr first, then wlr-randr.
pub(crate) fn enumerate_outputs() -> Vec<String> {
    let result = std::process::Command::new("cosmic-randr")
        .arg("list")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .or_else(|| {
            std::process::Command::new("wlr-randr")
                .output()
                .ok()
                .filter(|o| o.status.success())
        });
    let result = match result {
        Some(o) => o,
        None => return Vec::new(),
    };
    let stdout = String::from_utf8_lossy(&result.stdout);
    stdout
        .lines()
        .map(|line| util::strip_ansi(line))
        .filter(|line| !line.starts_with(' ') && !line.starts_with('\t') && !line.is_empty())
        .filter_map(|line| line.split_whitespace().next().map(String::from))
        .collect()
}
