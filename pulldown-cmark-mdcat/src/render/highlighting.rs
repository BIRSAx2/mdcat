// Copyright 2018-2020 Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Tools for syntax highlighting.

use anstyle::{AnsiColor, Color, Effects};
use std::{
    io::{Result, Write},
    sync::OnceLock,
};
use syntect::highlighting::{FontStyle, Highlighter, Style, Theme};

static SOLARIZED_DARK_DUMP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/theme.dump"));
static THEME: OnceLock<Theme> = OnceLock::new();
static HIGHLIGHTER: OnceLock<Highlighter> = OnceLock::new();

fn theme() -> &'static Theme {
    THEME.get_or_init(|| syntect::dumps::from_binary(SOLARIZED_DARK_DUMP))
}

pub fn highlighter() -> &'static Highlighter<'static> {
    HIGHLIGHTER.get_or_init(|| Highlighter::new(theme()))
}

/// Write regions as ANSI 8-bit coloured text.
///
/// We use this function to simplify syntax highlighting to 8-bit ANSI values
/// which every theme provides.  Contrary to 24 bit colours this gives us a good
/// guarantee that highlighting works with any terminal colour theme, whether
/// light or dark, and saves us all the hassle of mismatching colours.
///
/// We assume Solarized colours here: Solarized cleanly maps to 8-bit ANSI
/// colours so we can safely map its RGB colour values back to ANSI colours.  We
/// do so for all accent colours, but leave "base*" colours alone: Base colours
/// change depending on light or dark Solarized; to address both light and dark
/// backgrounds we must map all base colours to the default terminal colours.
///
/// Furthermore we completely ignore any background colour settings, to avoid
/// conflicts with the terminal colour themes.
fn solarized_to_ansi(style: Style) -> anstyle::Style {
    let rgb = {
        let fg = style.foreground;
        (fg.r, fg.g, fg.b)
    };
    let color: Option<Color> = match rgb {
        // base03, base02, base01, base00, base0, base1, base2, and base3 — map to terminal default
        (0x00, 0x2b, 0x36)
        | (0x07, 0x36, 0x42)
        | (0x58, 0x6e, 0x75)
        | (0x65, 0x7b, 0x83)
        | (0x83, 0x94, 0x96)
        | (0x93, 0xa1, 0xa1)
        | (0xee, 0xe8, 0xd5)
        | (0xfd, 0xf6, 0xe3) => None,
        (0xb5, 0x89, 0x00) => Some(AnsiColor::Yellow.into()),
        (0xcb, 0x4b, 0x16) => Some(AnsiColor::BrightRed.into()),
        (0xdc, 0x32, 0x2f) => Some(AnsiColor::Red.into()),
        (0xd3, 0x36, 0x82) => Some(AnsiColor::Magenta.into()),
        (0x6c, 0x71, 0xc4) => Some(AnsiColor::BrightMagenta.into()),
        (0x26, 0x8b, 0xd2) => Some(AnsiColor::Blue.into()),
        (0x2a, 0xa1, 0x98) => Some(AnsiColor::Cyan.into()),
        (0x85, 0x99, 0x00) => Some(AnsiColor::Green.into()),
        (r, g, b) => panic!("Unexpected RGB colour: #{r:2>0x}{g:2>0x}{b:2>0x}"),
    };
    let font = style.font_style;
    let effects = Effects::new()
        .set(Effects::BOLD, font.contains(FontStyle::BOLD))
        .set(Effects::ITALIC, font.contains(FontStyle::ITALIC))
        .set(Effects::UNDERLINE, font.contains(FontStyle::UNDERLINE));
    anstyle::Style::new().fg_color(color).effects(effects)
}

pub fn write_as_ansi<'a, W: Write, I: Iterator<Item = (Style, &'a str)>>(
    writer: &mut W,
    regions: I,
) -> Result<()> {
    for (style, text) in regions {
        let style = solarized_to_ansi(style);
        write!(writer, "{}{}{}", style.render(), text, style.render_reset())?;
    }
    Ok(())
}

/// Write syntax-highlighted regions for one line, filling the rest of the line
/// with `bg` using EL (`\x1b[K`), then resetting and writing a newline.
pub fn write_as_ansi_with_bg<'a, W: Write, I: Iterator<Item = (Style, &'a str)>>(
    writer: &mut W,
    regions: I,
    bg: Color,
) -> Result<()> {
    let bg_style = anstyle::Style::new().bg_color(Some(bg));
    write!(writer, "{}", bg_style.render())?;
    for (style, text) in regions {
        let token_style = solarized_to_ansi(style);
        let content = text.trim_end_matches('\n').trim_end_matches('\r');
        write!(
            writer,
            "{}{}{}",
            token_style.render(),
            content,
            token_style.render_reset()
        )?;
        // Re-apply bg after each token reset
        write!(writer, "{}", bg_style.render())?;
    }
    // Fill rest of line with bg, then reset and newline
    writeln!(writer, "\x1b[K\x1b[0m")?;
    Ok(())
}
