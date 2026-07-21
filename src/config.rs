// Copyright Mouhieddine Sabir <me@mouhieddine.dev>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! User configuration, loaded from `~/.config/mdcat/config.toml`.
//!
//! The file has two parts: a `[defaults]` table setting default values for a handful of CLI
//! flags, and a `[theme]` table customising a built-in theme (starting from `base`, then
//! overriding individual styles on top of it), in the same spirit as editor theme files (e.g.
//! Helix's `theme.toml`): a `[theme.palette]` of named colors, and a `[theme.styles]` table of
//! `{ fg, bg, modifiers }` per element.

use std::collections::HashMap;
use std::path::PathBuf;

use anstyle::{AnsiColor, Color, RgbColor, Style};
use anyhow::{bail, Context, Result};
use pulldown_cmark_mdcat::Theme;
use serde::Deserialize;

/// The user's configuration file, deserialized from TOML.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Default values for CLI flags.
    #[serde(default)]
    pub defaults: Defaults,
    /// Theme customisation.
    pub theme: Option<ThemeConfig>,
}

/// Default values for a handful of CLI flags, used when the flag isn't passed explicitly.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    /// Default for `--margin`.
    pub margin: Option<bool>,
    /// Default for `--smart-punctuation`.
    pub smart_punctuation: Option<bool>,
    /// Default for `--columns`.
    pub columns: Option<u16>,
    /// Default for `--local`.
    pub local_only: Option<bool>,
    /// Default for `--fail`.
    pub fail_fast: Option<bool>,
    /// Default for `--image-protocol`: `"none"`, `"iterm2"`, `"kitty"`, or `"sixel"`.
    pub image_protocol: Option<String>,
    /// Default for `--tabs`.
    pub tabs: Option<u16>,
}

/// Theme customisation: a built-in theme as a starting point, plus style overrides.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ThemeConfig {
    /// The built-in theme to start from, by name (e.g. `"dark"`, `"catppuccin-mocha"`).
    pub base: Option<String>,
    /// Override for the ruler color; a plain color, since rules have no other style.
    pub rule: Option<String>,
    /// Named colors that `fg`/`bg` below may reference instead of a literal color.
    #[serde(default)]
    pub palette: HashMap<String, String>,
    /// Per-element style overrides. Keys match the names below in [`apply_theme`].
    #[serde(default)]
    pub styles: HashMap<String, StyleConfig>,
}

/// A single style override: foreground/background color, text effects, and, for headings and
/// alerts, the marker/label text itself.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct StyleConfig {
    /// Foreground color: a `#rrggbb` hex value, an ANSI color name, or a `[theme.palette]` name.
    pub fg: Option<String>,
    /// Background color: a `#rrggbb` hex value, an ANSI color name, or a `[theme.palette]` name.
    pub bg: Option<String>,
    /// Text effects: `bold`, `dimmed`, `italic`, `underline`, `blink`, `invert`, `hidden`,
    /// `strikethrough`.
    #[serde(default)]
    pub modifiers: Vec<String>,
    /// The marker written before heading text (`h2`-`h6`), or the icon and label written for an
    /// alert (`alert_note`-`alert_caution`). Ignored for other style names.
    pub text: Option<String>,
}

/// Return the path to the user's config file, if the platform has a config directory.
///
/// Respects `$XDG_CONFIG_HOME` if set; otherwise falls back to `~/.config` on Unix and `%APPDATA%`
/// on Windows.
pub fn config_path() -> Option<PathBuf> {
    let config_dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            if cfg!(windows) {
                std::env::var_os("APPDATA").map(PathBuf::from)
            } else {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
            }
        })?;
    Some(config_dir.join("mdcat").join("config.toml"))
}

/// Load and parse the config file at `path`, if it exists.
pub fn load(path: &std::path::Path) -> Result<Option<Config>> {
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            let config = toml::from_str(&contents)
                .with_context(|| format!("Failed to parse config at {}", path.display()))?;
            Ok(Some(config))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("Failed to read config at {}", path.display()))
        }
    }
}

fn parse_ansi_color(name: &str) -> Option<AnsiColor> {
    Some(match name {
        "black" => AnsiColor::Black,
        "red" => AnsiColor::Red,
        "green" => AnsiColor::Green,
        "yellow" => AnsiColor::Yellow,
        "blue" => AnsiColor::Blue,
        "magenta" => AnsiColor::Magenta,
        "cyan" => AnsiColor::Cyan,
        "white" => AnsiColor::White,
        "bright-black" => AnsiColor::BrightBlack,
        "bright-red" => AnsiColor::BrightRed,
        "bright-green" => AnsiColor::BrightGreen,
        "bright-yellow" => AnsiColor::BrightYellow,
        "bright-blue" => AnsiColor::BrightBlue,
        "bright-magenta" => AnsiColor::BrightMagenta,
        "bright-cyan" => AnsiColor::BrightCyan,
        "bright-white" => AnsiColor::BrightWhite,
        _ => return None,
    })
}

fn parse_hex_color(value: &str) -> Result<Color> {
    let hex = value.strip_prefix('#').with_context(|| {
        format!("Invalid color {value:?}: expected '#rrggbb' or an ANSI/palette name")
    })?;
    if hex.len() != 6 {
        bail!("Invalid color {value:?}: expected 6 hex digits after '#'");
    }
    let channel = |range| {
        u8::from_str_radix(&hex[range], 16)
            .with_context(|| format!("Invalid color {value:?}: not valid hex"))
    };
    Ok(RgbColor(channel(0..2)?, channel(2..4)?, channel(4..6)?).into())
}

/// Resolve a color value, following one level of palette indirection.
fn resolve_color(value: &str, palette: &HashMap<String, String>) -> Result<Color> {
    let value = palette.get(value).map(String::as_str).unwrap_or(value);
    if let Some(ansi) = parse_ansi_color(value) {
        Ok(ansi.into())
    } else {
        parse_hex_color(value)
    }
}

fn apply_modifier(style: Style, modifier: &str) -> Result<Style> {
    Ok(match modifier {
        "bold" => style.bold(),
        "dimmed" => style.dimmed(),
        "italic" => style.italic(),
        "underline" => style.underline(),
        "blink" => style.blink(),
        "invert" => style.invert(),
        "hidden" => style.hidden(),
        "strikethrough" => style.strikethrough(),
        other => bail!(
            "Unknown modifier {other:?}: expected one of bold, dimmed, italic, underline, \
             blink, invert, hidden, strikethrough"
        ),
    })
}

/// Resolve a [`StyleConfig`] override into a concrete [`Style`], layered onto `base`.
fn resolve_style(
    config: &StyleConfig,
    base: Style,
    palette: &HashMap<String, String>,
) -> Result<Style> {
    let mut style = base;
    if let Some(fg) = &config.fg {
        style = style.fg_color(Some(resolve_color(fg, palette)?));
    }
    if let Some(bg) = &config.bg {
        style = style.bg_color(Some(resolve_color(bg, palette)?));
    }
    for modifier in &config.modifiers {
        style = apply_modifier(style, modifier)?;
    }
    Ok(style)
}

/// Apply `config`'s style overrides onto `base`, returning the customised theme.
pub fn apply_theme(config: &ThemeConfig, base: Theme) -> Result<Theme> {
    let mut theme = base;
    if let Some(rule) = &config.rule {
        theme.rule_color = resolve_color(rule, &config.palette)?;
    }
    for (name, style_config) in &config.styles {
        let resolve = |current: Style| resolve_style(style_config, current, &config.palette);
        match name.as_str() {
            "html_block" => theme.html_block_style = resolve(theme.html_block_style)?,
            "inline_html" => theme.inline_html_style = resolve(theme.inline_html_style)?,
            "code" => theme.code_style = resolve(theme.code_style)?,
            "link" => theme.link_style = resolve(theme.link_style)?,
            "image_link" => theme.image_link_style = resolve(theme.image_link_style)?,
            "quote_border" => theme.quote_border_style = resolve(theme.quote_border_style)?,
            "h2" => {
                theme.h2_style = resolve(theme.h2_style)?;
                if let Some(text) = &style_config.text {
                    theme.h2_marker = text.clone();
                }
            }
            "h3" => {
                theme.h3_style = resolve(theme.h3_style)?;
                if let Some(text) = &style_config.text {
                    theme.h3_marker = text.clone();
                }
            }
            "h4" => {
                theme.h4_style = resolve(theme.h4_style)?;
                if let Some(text) = &style_config.text {
                    theme.h4_marker = text.clone();
                }
            }
            "h5" => {
                theme.h5_style = resolve(theme.h5_style)?;
                if let Some(text) = &style_config.text {
                    theme.h5_marker = text.clone();
                }
            }
            "h6" => {
                theme.h6_style = resolve(theme.h6_style)?;
                if let Some(text) = &style_config.text {
                    theme.h6_marker = text.clone();
                }
            }
            "footnote" => theme.footnote_style = resolve(theme.footnote_style)?,
            "math" => theme.math_style = resolve(theme.math_style)?,
            "alert_note" => {
                theme.alert_note_style = resolve(theme.alert_note_style)?;
                if let Some(text) = &style_config.text {
                    theme.alert_note_label = text.clone();
                }
            }
            "alert_tip" => {
                theme.alert_tip_style = resolve(theme.alert_tip_style)?;
                if let Some(text) = &style_config.text {
                    theme.alert_tip_label = text.clone();
                }
            }
            "alert_important" => {
                theme.alert_important_style = resolve(theme.alert_important_style)?;
                if let Some(text) = &style_config.text {
                    theme.alert_important_label = text.clone();
                }
            }
            "alert_warning" => {
                theme.alert_warning_style = resolve(theme.alert_warning_style)?;
                if let Some(text) = &style_config.text {
                    theme.alert_warning_label = text.clone();
                }
            }
            "alert_caution" => {
                theme.alert_caution_style = resolve(theme.alert_caution_style)?;
                if let Some(text) = &style_config.text {
                    theme.alert_caution_label = text.clone();
                }
            }
            "h1_text" => {
                let style = resolve(theme.h1_text_style)?;
                theme = theme.with_h1(style);
            }
            other => bail!(
                "Unknown style {other:?} in config file; see the mdcat manpage for valid names"
            ),
        }
    }
    Ok(theme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_ansi_colors() {
        let palette = HashMap::new();
        assert_eq!(
            resolve_color("#ff0000", &palette).unwrap(),
            Color::Rgb(RgbColor(0xff, 0, 0))
        );
        assert_eq!(
            resolve_color("bright-blue", &palette).unwrap(),
            Color::Ansi(AnsiColor::BrightBlue)
        );
    }

    #[test]
    fn resolves_palette_indirection() {
        let mut palette = HashMap::new();
        palette.insert("mauve".to_string(), "#cba6f7".to_string());
        assert_eq!(
            resolve_color("mauve", &palette).unwrap(),
            Color::Rgb(RgbColor(0xcb, 0xa6, 0xf7))
        );
    }

    #[test]
    fn rejects_invalid_color() {
        let palette = HashMap::new();
        assert!(resolve_color("not-a-color", &palette).is_err());
        assert!(resolve_color("#zzzzzz", &palette).is_err());
        assert!(resolve_color("#fff", &palette).is_err());
    }

    #[test]
    fn rejects_unknown_modifier() {
        assert!(apply_modifier(Style::new(), "sparkly").is_err());
    }

    #[test]
    fn applies_modifiers_and_colors() {
        let config = StyleConfig {
            fg: Some("bright-red".to_string()),
            bg: None,
            modifiers: vec!["bold".to_string(), "italic".to_string()],
            text: None,
        };
        let style = resolve_style(&config, Style::new(), &HashMap::new()).unwrap();
        assert_eq!(
            style.get_fg_color(),
            Some(Color::Ansi(AnsiColor::BrightRed))
        );
        assert_eq!(
            style.get_effects(),
            anstyle::Effects::BOLD | anstyle::Effects::ITALIC
        );
    }

    #[test]
    fn parses_full_config_and_applies_overrides() {
        let toml = r##"
            [defaults]
            margin = true
            columns = 100

            [theme]
            base = "dark"
            rule = "bright-blue"

            [theme.palette]
            mauve = "#cba6f7"

            [theme.styles]
            h2 = { fg = "mauve", modifiers = ["bold"] }
            code = { fg = "bright-yellow" }
        "##;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.defaults.margin, Some(true));
        assert_eq!(config.defaults.columns, Some(100));
        let theme_config = config.theme.unwrap();
        assert_eq!(theme_config.base.as_deref(), Some("dark"));

        let theme = apply_theme(&theme_config, Theme::dark()).unwrap();
        assert_eq!(theme.rule_color, Color::Ansi(AnsiColor::BrightBlue));
        assert_eq!(
            theme.h2_style.get_fg_color(),
            Some(Color::Rgb(RgbColor(0xcb, 0xa6, 0xf7)))
        );
        assert!(theme
            .h2_style
            .get_effects()
            .contains(anstyle::Effects::BOLD));
        assert_eq!(
            theme.code_style.get_fg_color(),
            Some(Color::Ansi(AnsiColor::BrightYellow))
        );
    }

    #[test]
    fn rejects_unknown_style_name() {
        let toml = r##"
            [styles]
            nonexistent = { fg = "red" }
        "##;
        let config: ThemeConfig = toml::from_str(toml).unwrap();
        assert!(apply_theme(&config, Theme::dark()).is_err());
    }

    #[test]
    fn h1_override_keeps_prefix_background_in_sync() {
        let toml = r##"
            [styles]
            h1_text = { fg = "bright-white", bg = "bright-blue" }
        "##;
        let config: ThemeConfig = toml::from_str(toml).unwrap();
        let theme = apply_theme(&config, Theme::dark()).unwrap();
        assert_eq!(
            theme.h1_text_style.get_bg_color(),
            theme.h1_prefix_style.get_bg_color()
        );
        assert_eq!(
            theme.h1_prefix_style.get_fg_color(),
            theme.h1_prefix_style.get_bg_color()
        );
    }

    #[test]
    fn unknown_toml_field_is_rejected() {
        let toml = "typo_field = true";
        let result: Result<Config, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn overrides_heading_marker_text() {
        let toml = r##"
            [styles]
            h2 = { text = "» " }
        "##;
        let config: ThemeConfig = toml::from_str(toml).unwrap();
        let theme = apply_theme(&config, Theme::dark()).unwrap();
        assert_eq!(theme.h2_marker, "» ");
        // Other markers are untouched.
        assert_eq!(theme.h3_marker, "── ");
    }

    #[test]
    fn overrides_alert_label_text() {
        let toml = r##"
            [styles]
            alert_note = { text = " NOTE", fg = "bright-blue" }
        "##;
        let config: ThemeConfig = toml::from_str(toml).unwrap();
        let theme = apply_theme(&config, Theme::dark()).unwrap();
        assert_eq!(theme.alert_note_label, " NOTE");
        assert_eq!(
            theme.alert_note_style.get_fg_color(),
            Some(Color::Ansi(AnsiColor::BrightBlue))
        );
    }

    #[test]
    fn style_without_text_leaves_marker_at_default() {
        let toml = r##"
            [styles]
            h4 = { fg = "bright-red" }
        "##;
        let config: ThemeConfig = toml::from_str(toml).unwrap();
        let theme = apply_theme(&config, Theme::dark()).unwrap();
        assert_eq!(theme.h4_marker, "┄ ");
    }

    #[test]
    fn defaults_default_to_none() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.defaults.margin, None);
        assert_eq!(config.defaults.smart_punctuation, None);
        assert_eq!(config.defaults.columns, None);
        assert_eq!(config.defaults.local_only, None);
        assert_eq!(config.defaults.fail_fast, None);
        assert_eq!(config.defaults.image_protocol, None);
        assert_eq!(config.defaults.tabs, None);
        assert!(config.theme.is_none());
    }
}
