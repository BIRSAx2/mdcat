// Copyright Mouhieddine Sabir <me@mouhieddine.dev>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use anstyle::Style;
use merman::ascii::{AsciiRenderOptions, HeadlessAsciiRenderer};
use merman::render::{
    HeadlessRenderer, HostThemeOutput, HostThemePipelinePreset, HostThemeProfile,
    HostThemeRootBackground,
};

use crate::resources::svg::render_svg_to_png_scaled;
use crate::terminal::TerminalSize;

use super::math::{png_dimensions, style_to_ratex_color};

pub(crate) struct MermaidImage {
    pub png: Vec<u8>,
    pub width_columns: u16,
    pub height_rows: u16,
}

/// Uniform raster upscale applied to every rendered diagram.
///
/// Deliberately the *only* size knob: Mermaid diagram types are wildly inconsistent in how (or
/// whether) they read font-size config, so per-type config/CSS tuning turns into whack-a-mole
/// across diagram types. A flat raster upscale leaves each diagram's own internal proportions
/// alone and just makes the whole image bigger, uniformly, for every diagram type.
const MERMAID_RASTER_SCALE: f32 = 1.3;

/// Convert a style's foreground color to a `#rrggbb` CSS color.
fn style_to_css_hex(style: &Style) -> String {
    let color = style_to_ratex_color(style);
    format!(
        "#{:02x}{:02x}{:02x}",
        (color.r * 255.0).round().clamp(0.0, 255.0) as u8,
        (color.g * 255.0).round().clamp(0.0, 255.0) as u8,
        (color.b * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

/// Host theme profile for rendering a Mermaid diagram to PNG.
///
/// Based on merman's `editor_dark`/`editor_light` presets (contrast-paired fill/text/border
/// colors); only `line` is re-tinted with `mermaid_style`, since overriding every role with one
/// accent color breaks the pairing (e.g. accent-on-accent text). Font size is deliberately left
/// alone here (see [`MERMAID_RASTER_SCALE`]).
fn host_theme_profile(mermaid_style: &Style, is_dark: bool) -> HostThemeProfile {
    let mut profile = if is_dark {
        HostThemeProfile::editor_dark()
    } else {
        HostThemeProfile::editor_light()
    };
    profile.roles.line = Some(style_to_css_hex(mermaid_style));
    profile.output = HostThemeOutput {
        pipeline: HostThemePipelinePreset::ResvgSafe,
        root_background: HostThemeRootBackground::None,
        ..profile.output
    };
    profile
}

/// Render a Mermaid diagram to a PNG image, via SVG.
///
/// Returns `None` if the diagram can't be parsed/laid out, or if PNG rasterization is
/// unavailable (the `svg` feature is disabled).
pub(crate) fn render_mermaid_png(
    input: &str,
    terminal_size: &TerminalSize,
    mermaid_style: &Style,
    is_dark: bool,
) -> Option<MermaidImage> {
    let profile = host_theme_profile(mermaid_style, is_dark);
    let svg = HeadlessRenderer::new()
        .with_host_theme(&profile)
        .render_svg_sync(input)
        .ok()??;
    // Some diagram types (e.g. timeline) hardcode `stroke="black"` regardless of theme, making
    // those lines invisible on a dark terminal; resvg's CSS engine doesn't apply `!important`
    // overrides for this reliably, so patch the attribute directly instead.
    let accent = style_to_css_hex(mermaid_style);
    let svg = svg.replace("stroke=\"black\"", &format!("stroke=\"{accent}\""));
    let png = render_svg_to_png_scaled(svg.as_bytes(), MERMAID_RASTER_SCALE).ok()?;
    let (px_w, px_h) = png_dimensions(&png);
    let (width_columns, height_rows) = match terminal_size.cell {
        Some(cell) => (
            ((px_w as f32 / cell.x as f32).ceil() as u16).max(1),
            ((px_h as f32 / cell.y as f32).ceil() as u16).max(1),
        ),
        None => (1, 1),
    };
    Some(MermaidImage {
        png,
        width_columns,
        height_rows,
    })
}

/// Render a Mermaid diagram as Unicode text, for terminals without image support.
///
/// Returns `None` if the diagram can't be parsed/laid out.
pub(crate) fn render_mermaid_text(input: &str) -> Option<String> {
    HeadlessAsciiRenderer::new()
        .with_ascii_options(AsciiRenderOptions::unicode())
        .render_ascii_sync(input)
        .ok()?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yellow() -> Style {
        Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()))
    }

    #[test]
    #[cfg(feature = "svg")]
    fn render_png_simple_flowchart() {
        let terminal_size = crate::terminal::TerminalSize {
            columns: 80,
            rows: 24,
            pixels: Some(crate::terminal::PixelSize { x: 800, y: 480 }),
            cell: Some(crate::terminal::PixelSize { x: 10, y: 20 }),
        };
        let img = render_mermaid_png(
            "flowchart TD\nA[Start] --> B[Done]",
            &terminal_size,
            &yellow(),
            true,
        );
        assert!(img.is_some());
        let img = img.unwrap();
        assert!(img.png.len() > 100);
        assert_eq!(&img.png[1..4], b"PNG");
        assert!(img.width_columns > 0);
        assert!(img.height_rows > 0);
    }

    #[test]
    fn render_png_invalid_diagram_returns_none() {
        let terminal_size = crate::terminal::TerminalSize::default();
        assert!(render_mermaid_png(
            "not a mermaid diagram at all {{{",
            &terminal_size,
            &yellow(),
            true
        )
        .is_none());
    }

    #[test]
    fn style_to_css_hex_converts_rgb() {
        let style = Style::new().fg_color(Some(anstyle::RgbColor(0x1a, 0x2b, 0x3c).into()));
        assert_eq!(style_to_css_hex(&style), "#1a2b3c");
    }

    #[test]
    fn render_text_simple_flowchart() {
        let text = render_mermaid_text("flowchart TD\nA[Start] --> B[Done]");
        assert!(text.is_some());
        assert!(!text.unwrap().is_empty());
    }

    #[test]
    fn render_text_invalid_diagram_returns_none() {
        assert!(render_mermaid_text("not a mermaid diagram at all {{{").is_none());
    }
}
