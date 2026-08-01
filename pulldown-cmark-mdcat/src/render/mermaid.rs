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
use crate::theme::MermaidPalette;

use super::math::{png_dimensions, style_to_ratex_color};

pub(crate) struct MermaidImage {
    pub png: Vec<u8>,
    pub width_columns: u16,
    pub height_rows: u16,
}

/// Default raster upscale for diagram types not listed in [`raster_scale_for`].
///
/// The *only* size knob, deliberately: Mermaid diagram types are wildly inconsistent in how (or
/// whether) they read font-size config, so per-type config/CSS tuning turns into whack-a-mole. A
/// flat raster upscale leaves a diagram's own internal proportions alone and just makes the
/// whole image bigger, uniformly. Per-type overrides below exist only because a single scale
/// can't satisfy diagram types that need to move in opposite directions (e.g. class wants
/// bigger, mindmap wants smaller) — see the `project-mermaid-diagram-sizing` memory for the
/// visual-feedback history behind these specific numbers.
const MERMAID_RASTER_SCALE_DEFAULT: f32 = 1.3;

/// Mermaid's own implicit default `themeVariables.fontSize`.
///
/// Diagram types we don't otherwise touch (flowchart, sequence, ...) get this size unless
/// something above them in the config chain overrides it. Some diagram types read their font
/// sizes from a different, diagram-specific config namespace instead of `themeVariables.fontSize`
/// (see [`apply_diagram_specific_config`]) and default to a smaller value there — this constant
/// is the target we bring them back up to, so every diagram type reads the same body text size.
const MERMAID_BASE_FONT_SIZE: f64 = 16.0;

/// Sniffs a Mermaid diagram's type from the first non-empty line of its source — the same
/// keyword Mermaid itself uses to detect diagram type.
fn diagram_type(input: &str) -> &str {
    let first_line = input
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    first_line
        .trim()
        .split(|c: char| c.is_whitespace())
        .next()
        .unwrap_or("")
}

/// Picks a raster scale for a diagram type (see [`diagram_type`]).
fn raster_scale_for(diagram_type: &str) -> f32 {
    match diagram_type {
        // Renders visibly smaller than other types even at the default scale.
        "classDiagram" => 1.6,
        "stateDiagram" | "stateDiagram-v2" => 1.55,
        "mindmap" => 1.5,
        "gitGraph" => 1.95,
        _ => MERMAID_RASTER_SCALE_DEFAULT,
    }
}

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

/// Fills in `site_config`/`theme_variables` for diagram types that don't (fully) read the
/// theme's normal `themeVariables.fontSize`/`textColor`, so their output matches every other
/// diagram type instead of using Mermaid's own smaller/mismatched built-in defaults.
///
/// `gantt`/`quadrantChart` read font sizes from their own top-level config namespace instead of
/// `themeVariables.fontSize`, which `HostThemeProfile` never populates — so without this they
/// default to a visibly smaller size than every other diagram type. `journey` sets one flat text
/// color for every task label regardless of that task's own (always light, pastel) background
/// fill, so our theme's dark-mode `textColor` (light, for contrast against a *dark* canvas)
/// renders as light-on-light there; pin it to a fixed dark color that reads on journey's palette
/// regardless of our overall theme.
fn apply_diagram_specific_config(profile: &mut HostThemeProfile, diagram_type: &str) {
    match diagram_type {
        "gantt" => {
            profile.site_config.insert(
                "gantt".to_string(),
                serde_json::json!({
                    "fontSize": MERMAID_BASE_FONT_SIZE,
                    "sectionFontSize": MERMAID_BASE_FONT_SIZE,
                }),
            );
        }
        "quadrantChart" => {
            profile.site_config.insert(
                "quadrantChart".to_string(),
                serde_json::json!({
                    "titleFontSize": MERMAID_BASE_FONT_SIZE * 1.25,
                    "xAxisLabelFontSize": MERMAID_BASE_FONT_SIZE,
                    "yAxisLabelFontSize": MERMAID_BASE_FONT_SIZE,
                    "quadrantLabelFontSize": MERMAID_BASE_FONT_SIZE,
                    "pointLabelFontSize": MERMAID_BASE_FONT_SIZE * 0.85,
                }),
            );
        }
        "journey" => {
            profile
                .theme_variables
                .insert("textColor".to_string(), serde_json::json!("#1f2937"));
        }
        _ => {}
    }
}

/// Picks merman's `HostThemeProfile` preset constructor matching a [`MermaidPalette`] choice.
///
/// `is_dark` disambiguates `Generic`, the only variant without a dedicated dark/light preset
/// pair of its own; every other variant already implies an appearance.
fn base_profile_for(palette: MermaidPalette, is_dark: bool) -> HostThemeProfile {
    match palette {
        MermaidPalette::Generic if is_dark => HostThemeProfile::editor_dark(),
        MermaidPalette::Generic => HostThemeProfile::editor_light(),
        MermaidPalette::OneDark => HostThemeProfile::one_dark(),
        MermaidPalette::GruvboxDark => HostThemeProfile::gruvbox_dark(),
        MermaidPalette::GruvboxLight => HostThemeProfile::gruvbox_light(),
        MermaidPalette::AyuDark => HostThemeProfile::ayu_dark(),
        MermaidPalette::AyuLight => HostThemeProfile::ayu_light(),
    }
}

/// Host theme profile for rendering a Mermaid diagram to PNG, plus the theme's own canvas text
/// color (the preset's `roles.text`, before any diagram-specific override) for use by
/// post-processing fixes that need to restore it (see [`fix_journey_title_and_legend_fill`]).
///
/// Based on the merman preset [`base_profile_for`] picks for the theme's [`MermaidPalette`]
/// (contrast-paired fill/text/border colors); only `line` is re-tinted with `mermaid_style`,
/// since overriding every role with one accent color breaks the pairing (e.g. accent-on-accent
/// text). Font size is deliberately left alone here (see [`raster_scale_for`]) except for the
/// per-type fixups in [`apply_diagram_specific_config`].
fn host_theme_profile(
    mermaid_style: &Style,
    is_dark: bool,
    mermaid_palette: MermaidPalette,
    diagram_type: &str,
) -> (HostThemeProfile, String) {
    let mut profile = base_profile_for(mermaid_palette, is_dark);
    let canvas_text_color = profile
        .roles
        .text
        .clone()
        .unwrap_or_else(|| if is_dark { "#e5e7eb" } else { "#0f172a" }.to_string());
    profile.roles.line = Some(style_to_css_hex(mermaid_style));
    profile.output = HostThemeOutput {
        pipeline: HostThemePipelinePreset::ResvgSafe,
        root_background: HostThemeRootBackground::None,
        ..profile.output
    };
    extend_series_palette(&mut profile);
    apply_diagram_specific_config(&mut profile, diagram_type);
    (profile, canvas_text_color)
}

/// Extends a profile's `series_palette` to `SERIES_PALETTE_MIN_LEN` entries by cycling its own
/// colors, so mindmap/timeline diagrams with more top-level sections than the preset's original
/// (8-color) palette don't fall back to a hardcoded, uncontrasted black/white for the extra
/// sections. merman computes each section's node/text contrast per palette entry
/// (`readable_text_color`) up to a 12-section limit; only sections *beyond* the palette length
/// hit that fallback, so repeating already-contrasted colors (rather than needing new hand-picked
/// ones) is enough to avoid it — colors repeat past the 9th section, which is preferable to
/// unreadable ones.
const SERIES_PALETTE_MIN_LEN: usize = 12;

fn extend_series_palette(profile: &mut HostThemeProfile) {
    if profile.series_palette.is_empty() {
        return;
    }
    let original_len = profile.series_palette.len();
    while profile.series_palette.len() < SERIES_PALETTE_MIN_LEN {
        let next = profile.series_palette[profile.series_palette.len() % original_len].clone();
        profile.series_palette.push(next);
    }
}

/// Clamps negative `stroke-width` CSS values to a small positive minimum.
///
/// Merman's mindmap depth-based edge-thickness formula produces negative widths from depth 5
/// onward (e.g. `.edge-depth-5{stroke-width:-1;}`), which is invalid CSS; resvg silently drops
/// the stroke rather than clamping it, making those edges invisible instead of just thin.
fn fix_negative_stroke_widths(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(idx) = rest.find("stroke-width:-") {
        out.push_str(&rest[..idx]);
        out.push_str("stroke-width:1");
        let after = &rest[idx + "stroke-width:-".len()..];
        let end = after
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// Replaces the value of every CSS declaration matching `prefix` (which must include the
/// property name and colon, e.g. `.lineWrapper line{stroke:`) with `new_value`, regardless of
/// what the existing value is. Scans forward from the end of `prefix` to the next `;` or `}`.
///
/// More robust than matching specific literal values: some diagram types compute a CSS value via
/// internal logic (a contrast formula, or — as with `.lineWrapper line{stroke:...}` — a merman
/// bug where the *last* of several same-selector rules silently wins the cascade) that can
/// produce different literal values across renders. Matching only a couple of literals observed
/// in testing (e.g. `#000000`/`#ffffff`) missed a third (`#333`) that showed up in practice; this
/// replaces whatever is actually there.
fn replace_css_value(svg: &str, prefix: &str, new_value: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(idx) = rest.find(prefix) {
        out.push_str(&rest[..idx]);
        out.push_str(prefix);
        out.push_str(new_value);
        let after = &rest[idx + prefix.len()..];
        let end = after.find([';', '}']).unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// Retints hardcoded black `stroke="..."` presentation attributes to the theme accent color.
///
/// Some diagram types (timeline's axis, journey's axis and actor/mouth outlines) hardcode a
/// literal black stroke attribute regardless of theme, invisible on a dark terminal; resvg's CSS
/// engine doesn't apply `!important` overrides reliably enough to fix this via injected CSS, so
/// patch the attributes directly instead.
fn fix_hardcoded_strokes(svg: &str, accent: &str) -> String {
    let svg = svg.replace("stroke=\"black\"", &format!("stroke=\"{accent}\""));
    svg.replace("stroke=\"#000\"", &format!("stroke=\"{accent}\""))
}

/// Fills in the theme accent color for diagram types (e.g. timeline, journey) whose arrowhead
/// marker (`<marker id="merman-arrowhead">`) has no CSS rule targeting it — unlike per-edge
/// markers (`id="<edge-id>-arrowhead"`, matched by a `[id$="-arrowhead"]` selector), a bare
/// `merman-arrowhead` id doesn't match that suffix selector, so its `<path>` renders with SVG's
/// default black fill, appearing as a barely-visible dark grey smudge on a dark terminal.
fn fix_unstyled_arrowhead_marker(svg: &str, accent: &str) -> String {
    const MARKER: &str = "<marker id=\"merman-arrowhead\"";
    let Some(marker_idx) = svg.find(MARKER) else {
        return svg.to_string();
    };
    let Some(path_rel_idx) = svg[marker_idx..].find("<path ") else {
        return svg.to_string();
    };
    let insert_at = marker_idx + path_rel_idx + "<path ".len();
    let mut out = String::with_capacity(svg.len() + accent.len() + 10);
    out.push_str(&svg[..insert_at]);
    out.push_str(&format!("fill=\"{accent}\" "));
    out.push_str(&svg[insert_at..]);
    out
}

/// Retints timeline's axis/connector lines to the theme accent color.
///
/// Timeline wraps its axis and connector `<line>`s in `<g class="lineWrapper">` and gives each a
/// `stroke="..."` presentation attribute matching the theme accent (see [`fix_hardcoded_strokes`])
/// — but also emits a `.lineWrapper line{stroke:...;}` CSS rule targeting the same elements, once
/// per internal Mermaid color-palette section, unscoped, so only the last one (whatever value
/// that happens to resolve to — a merman bug, not something we control) wins the CSS cascade over
/// the presentation attribute. Retinting every occurrence guarantees the winning one is ours too.
fn fix_line_wrapper_stroke(svg: &str, accent: &str) -> String {
    replace_css_value(svg, ".lineWrapper line{stroke:", accent)
}

/// Retints class diagrams' hardcoded small text back to the normal body size.
///
/// `class`'s method/attribute text (`g.classGroup text{...font-size:10px;}`) and class-name
/// labels (`.classLabel .label{...font-size:10px;}`) are pinned to a literal `10px`, edge
/// terminals to `11px`, and the title to `18px`, all by CSS rules more specific than the themed
/// root `font-size`, so they win by CSS specificity regardless of `themeVariables.fontSize` —
/// unlike `gantt`/`quadrantChart` (see [`apply_diagram_specific_config`]), there's no config key
/// for these, only the literal CSS. Unlike [`fix_line_wrapper_stroke`], the exact current value
/// here is fixed and known (verified against real rendered output), so a plain literal replace is
/// simpler and more precise than scanning for an arbitrary value after a prefix.
fn fix_class_diagram_font_sizes(svg: &str, diagram_type: &str) -> String {
    if diagram_type != "classDiagram" {
        return svg.to_string();
    }
    let body = format!("font-size:{MERMAID_BASE_FONT_SIZE}px;");
    let title = format!("font-size:{}px;", MERMAID_BASE_FONT_SIZE * 1.25);
    let svg = svg.replace("font-size:10px;", &body);
    let svg = svg.replace("font-size:11px;", &body);
    svg.replace("font-size:18px;", &title)
}

/// Retints journey's axis/connector lines to the theme accent color.
///
/// `journey_css` emits `#merman line{stroke:{textColor}}` — an unscoped rule matching every
/// `<line>` in the diagram, including the axis, not just text-adjacent elements. Since
/// [`apply_diagram_specific_config`] pins journey's `textColor` to a fixed dark value for label
/// readability against journey's light task boxes, that same dark value leaks onto the axis line
/// via this rule, making it hard to see against a dark canvas. Scoped to `journey` specifically —
/// unlike `.lineWrapper line` (timeline), a genuinely bare `line{stroke:...}` rule could mean
/// something diagram-specific elsewhere, so this isn't applied unconditionally.
fn fix_journey_line_stroke(svg: &str, accent: &str, diagram_type: &str) -> String {
    if diagram_type != "journey" {
        return svg.to_string();
    }
    replace_css_value(svg, "line{stroke:", accent)
}

/// Restores the theme's own text color on journey's title and actor legend, which
/// [`apply_diagram_specific_config`]'s `textColor` pin (needed for task-label readability on
/// journey's light task boxes) also darkens.
///
/// `journey_css` uses that same flat `textColor` as both the diagram's *inherited default* fill
/// (`#merman{...fill:{textColor}}`, which the unstyled task-label `<text class="task">` elements
/// rely on for their own correct dark-on-light-box color — so that root rule must NOT be touched)
/// and the `.legend{fill:{textColor}}` rule for actor names. The title `<text>` has no class or
/// fill of its own and also inherits the root default. Title and legend sit directly on the
/// canvas rather than on a task box, so — unlike the task labels — they need the theme's real
/// text color (contrasted against the canvas): patch them individually rather than the shared
/// root rule they'd otherwise inherit from.
fn fix_journey_title_and_legend_fill(
    svg: &str,
    canvas_text_color: &str,
    diagram_type: &str,
) -> String {
    if diagram_type != "journey" {
        return svg.to_string();
    }
    let svg = replace_css_value(svg, ".legend{fill:", canvas_text_color);
    fix_journey_title_fill(&svg, canvas_text_color)
}

/// Injects an inline `style="fill:..."` onto journey's title `<text>`, identified by its unique
/// `font-size="4ex"` attribute (no other element in a journey diagram uses that unit), so it no
/// longer relies on inheriting the diagram's shared root fill.
///
/// Must be an inline `style`, not a bare `fill="..."` presentation attribute: presentation
/// attributes have the *lowest* CSS priority, so the existing `#merman{fill:...}` rule (an ID
/// selector) would still win over a bare attribute. An inline `style` attribute beats any
/// stylesheet rule regardless of selector specificity.
fn fix_journey_title_fill(svg: &str, canvas_text_color: &str) -> String {
    const MARKER: &str = "font-size=\"4ex\"";
    let Some(marker_idx) = svg.find(MARKER) else {
        return svg.to_string();
    };
    let insert_at = marker_idx + MARKER.len();
    let mut out = String::with_capacity(svg.len() + canvas_text_color.len() + 20);
    out.push_str(&svg[..insert_at]);
    out.push_str(&format!(" style=\"fill:{canvas_text_color}\""));
    out.push_str(&svg[insert_at..]);
    out
}

/// Renders `input` to SVG and applies all the post-processing fixes above, returning the fixed
/// SVG text and the diagram type it was rendered as. Shared by [`render_mermaid_png`] and by
/// tests that assert specific bad patterns are absent from the final SVG (see the
/// `project-mermaid-diagram-sizing` memory for why literal-value regression tests matter here).
fn render_mermaid_svg_fixed<'a>(
    input: &'a str,
    mermaid_style: &Style,
    is_dark: bool,
    mermaid_palette: MermaidPalette,
) -> Option<(String, &'a str)> {
    let diagram_type = diagram_type(input);
    let (profile, canvas_text_color) =
        host_theme_profile(mermaid_style, is_dark, mermaid_palette, diagram_type);
    let svg = HeadlessRenderer::new()
        .with_host_theme(&profile)
        .render_svg_sync(input)
        .ok()??;
    let accent = style_to_css_hex(mermaid_style);
    let svg = fix_hardcoded_strokes(&svg, &accent);
    let svg = fix_negative_stroke_widths(&svg);
    let svg = fix_unstyled_arrowhead_marker(&svg, &accent);
    let svg = fix_line_wrapper_stroke(&svg, &accent);
    let svg = fix_journey_line_stroke(&svg, &accent, diagram_type);
    let svg = fix_journey_title_and_legend_fill(&svg, &canvas_text_color, diagram_type);
    let svg = fix_class_diagram_font_sizes(&svg, diagram_type);
    Some((svg, diagram_type))
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
    mermaid_palette: MermaidPalette,
) -> Option<MermaidImage> {
    let (svg, diagram_type) =
        render_mermaid_svg_fixed(input, mermaid_style, is_dark, mermaid_palette)?;
    let png = render_svg_to_png_scaled(svg.as_bytes(), raster_scale_for(diagram_type)).ok()?;
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
            MermaidPalette::Generic,
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
            true,
            MermaidPalette::Generic,
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

    #[test]
    fn fix_negative_stroke_widths_clamps_to_one() {
        let svg = ".edge-depth-5{stroke-width:-1;}.edge-depth-8{stroke-width:-10.5;}";
        assert_eq!(
            fix_negative_stroke_widths(svg),
            ".edge-depth-5{stroke-width:1;}.edge-depth-8{stroke-width:1;}"
        );
    }

    #[test]
    fn fix_negative_stroke_widths_leaves_positive_values_alone() {
        let svg = ".edge-depth-1{stroke-width:11;}";
        assert_eq!(fix_negative_stroke_widths(svg), svg);
    }

    #[test]
    fn fix_unstyled_arrowhead_marker_adds_fill() {
        let svg =
            "<marker id=\"merman-arrowhead\" refX=\"5\"><path d=\"M 0,0 V 4 L6,2 Z\"/></marker>";
        assert_eq!(
            fix_unstyled_arrowhead_marker(svg, "#cccc00"),
            "<marker id=\"merman-arrowhead\" refX=\"5\"><path fill=\"#cccc00\" d=\"M 0,0 V 4 L6,2 Z\"/></marker>"
        );
    }

    #[test]
    fn fix_unstyled_arrowhead_marker_leaves_other_svgs_alone() {
        let svg = "<marker id=\"other-arrowhead\"><path d=\"M 0,0\"/></marker>";
        assert_eq!(fix_unstyled_arrowhead_marker(svg, "#cccc00"), svg);
    }

    #[test]
    fn fix_line_wrapper_stroke_retints_black_and_white() {
        let svg = ".lineWrapper line{stroke:#000000;}.other{color:red;}.lineWrapper line{stroke:#ffffff;}";
        assert_eq!(
            fix_line_wrapper_stroke(svg, "#cccc00"),
            ".lineWrapper line{stroke:#cccc00;}.other{color:red;}.lineWrapper line{stroke:#cccc00;}"
        );
    }

    #[test]
    fn fix_journey_line_stroke_retints_unscoped_rule_for_journey_only() {
        let svg = "#merman .mouth{stroke:#666;}#merman line{stroke:#1f2937;}";
        assert_eq!(
            fix_journey_line_stroke(svg, "#cccc00", "journey"),
            "#merman .mouth{stroke:#666;}#merman line{stroke:#cccc00;}"
        );
        assert_eq!(fix_journey_line_stroke(svg, "#cccc00", "timeline"), svg);
    }

    #[test]
    fn extend_series_palette_cycles_to_minimum_length() {
        let mut profile = HostThemeProfile::editor_dark();
        let original = profile.series_palette.clone();
        extend_series_palette(&mut profile);
        assert_eq!(profile.series_palette.len(), SERIES_PALETTE_MIN_LEN);
        assert_eq!(&profile.series_palette[..original.len()], &original[..]);
        // Cycles back to the start once the original colors are exhausted.
        assert_eq!(profile.series_palette[original.len()], original[0]);
    }

    #[test]
    fn extend_series_palette_leaves_empty_palette_alone() {
        let mut profile = HostThemeProfile::editor_dark();
        profile.series_palette.clear();
        extend_series_palette(&mut profile);
        assert!(profile.series_palette.is_empty());
    }

    #[test]
    fn fix_class_diagram_font_sizes_retints_all_four_literals() {
        let svg = "g.classGroup text{fill:#475569;font-size:10px;}.classLabel .label{font-size:10px;}.edgeTerminals{font-size:11px;}.classTitleText,.classDiagramTitleText{font-size:18px;}";
        let fixed = fix_class_diagram_font_sizes(svg, "classDiagram");
        assert!(!fixed.contains("font-size:10px"));
        assert!(!fixed.contains("font-size:11px"));
        assert!(!fixed.contains("font-size:18px"));
        assert!(fixed.contains("font-size:16px"));
    }

    #[test]
    fn fix_class_diagram_font_sizes_ignores_other_diagram_types() {
        let svg = "g.classGroup text{font-size:10px;}";
        assert_eq!(fix_class_diagram_font_sizes(svg, "flowchart"), svg);
    }

    // Regression tests: each of these fixes was discovered by rendering a real diagram and
    // spotting a bad pattern in the output (a literal color, a missing fill, a mispositioned CSS
    // rule) that a future merman version bump could silently reintroduce or add a new instance
    // of. These assert the bad pattern's absence from real rendered SVGs, not just from the
    // synthetic strings the unit tests above use.

    fn rendered_svg(input: &str) -> String {
        render_mermaid_svg_fixed(input, &yellow(), true, MermaidPalette::Generic)
            .expect("diagram should render")
            .0
    }

    #[test]
    fn class_diagram_never_has_hardcoded_small_font_sizes() {
        let svg = rendered_svg(
            "classDiagram\n    Animal <|-- Dog\n    Animal : +String name\n    Dog : +bark()",
        );
        assert!(!svg.contains("font-size:10px"));
        assert!(!svg.contains("font-size:11px"));
        assert!(!svg.contains("font-size:18px"));
    }

    #[test]
    fn timeline_axis_never_has_hardcoded_black_stroke() {
        let svg = rendered_svg("timeline\n    title t\n    2020 : a\n    2023 : b\n    2026 : c");
        assert!(!svg.contains("stroke=\"black\""));
        assert!(!svg.contains(".lineWrapper line{stroke:#000000"));
        assert!(!svg.contains(".lineWrapper line{stroke:#ffffff"));
    }

    #[test]
    fn journey_never_has_hardcoded_black_stroke() {
        let svg = rendered_svg("journey\n    title t\n    section S\n      Task: 5: Me");
        assert!(!svg.contains("stroke=\"black\""));
        assert!(!svg.contains("stroke=\"#000\""));
    }

    #[test]
    fn journey_axis_line_is_never_the_dark_label_text_color() {
        // journey_css's unscoped `line{stroke:...}` rule reads the same `textColor` variable
        // apply_diagram_specific_config pins to a fixed dark value for label readability; without
        // fix_journey_line_stroke, that dark value also becomes the axis line's stroke — via a
        // CSS type-selector rule, which silently overrides the line's own presentation attribute.
        let svg = rendered_svg("journey\n    title t\n    section S\n      Task: 5: Me");
        assert!(!svg.contains("line{stroke:#1f2937"));
    }

    #[test]
    fn mindmap_never_has_negative_stroke_width() {
        let svg = rendered_svg(
            "mindmap\n    root((r))\n        a\n            b\n                c\n                    d",
        );
        assert!(!svg.contains("stroke-width:-"));
    }

    #[test]
    fn journey_title_and_legend_never_use_the_dark_task_label_text_color() {
        // journey's task-label-on-box readability fix (apply_diagram_specific_config, finding D)
        // pins the diagram's shared `textColor` to a fixed dark value; the title and `.legend`
        // (actor name) text inherit/read that same variable but sit directly on the canvas, not a
        // task box, so without fix_journey_title_and_legend_fill they'd render dark-on-dark.
        let svg =
            rendered_svg("journey\n    title Buying coffee\n    section S\n      Task: 5: Me");
        assert!(!svg.contains(".legend{fill:#1f2937"));
        assert!(!svg.contains("font-size=\"4ex\" style=\"fill:#1f2937\""));
    }
}
