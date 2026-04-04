//! Subpixel layout detection used to match LCD glyph rasterization to the display.

use std::{env, process::Command};

use log::info;

use crate::font::SubpixelLayout;

/// Detects the platform subpixel layout using fontconfig first and environment fallbacks second.
pub fn detect_subpixel_layout() -> SubpixelLayout {
    if let Some(layout) = fontconfig_layout() {
        info!("subpixel.layout source=fontconfig value={layout:?}");
        return layout;
    }
    if let Some(layout) = env_layout() {
        info!("subpixel.layout source=env value={layout:?}");
        return layout;
    }
    info!("subpixel.layout source=default value=HorizontalRgb");
    SubpixelLayout::HorizontalRgb
}

fn fontconfig_layout() -> Option<SubpixelLayout> {
    let output = Command::new("fc-match")
        .args(["-f", "%{rgba}\n", "sans"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_layout(std::str::from_utf8(&output.stdout).ok()?)
}

fn env_layout() -> Option<SubpixelLayout> {
    [
        "STELE_SUBPIXEL_LAYOUT",
        "FC_RGBA",
        "FONTCONFIG_RGBA",
        "XFT_RGBA",
    ]
    .into_iter()
    .find_map(|key| env::var(key).ok().and_then(|value| parse_layout(&value)))
}

fn parse_layout(value: &str) -> Option<SubpixelLayout> {
    match value.trim().to_ascii_lowercase().as_str() {
        "rgb" | "horizontal_rgb" => Some(SubpixelLayout::HorizontalRgb),
        "bgr" | "horizontal_bgr" => Some(SubpixelLayout::HorizontalBgr),
        "vrgb" | "vertical_rgb" => Some(SubpixelLayout::VerticalRgb),
        "vbgr" | "vertical_bgr" => Some(SubpixelLayout::VerticalBgr),
        "none" | "unknown" => Some(SubpixelLayout::None),
        _ => None,
    }
}
