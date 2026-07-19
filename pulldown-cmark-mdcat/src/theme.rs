// Copyright 2018-2020 Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Provide a colour theme for mdcat.

use anstyle::{AnsiColor, Color, RgbColor, Style};

/// A colour theme for mdcat.
///
/// All fields are public so themes can be fully customised, e.g. from a user config file.
/// `h1_prefix_style` is a derived convenience (background-only padding matching `h1_text_style`'s
/// background); use [`Theme::with_h1`] to keep the two in sync when changing the H1 background.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Theme {
    /// Style for HTML blocks.
    pub html_block_style: Style,
    /// Style for inline HTML.
    pub inline_html_style: Style,
    /// Style for code, unless the code is syntax-highlighted.
    pub code_style: Style,
    /// Style for links.
    pub link_style: Style,
    /// Color for image links (unless the image is rendered inline)
    pub image_link_style: Style,
    /// Color for rulers.
    pub rule_color: Color,
    /// Style for block quote borders (`│`).
    pub quote_border_style: Style,
    /// Style for H2 headings.
    pub h2_style: Style,
    /// Marker written before H2 heading text (default `"━━ "`).
    pub h2_marker: String,
    /// Style for H3 headings.
    pub h3_style: Style,
    /// Marker written before H3 heading text (default `"── "`).
    pub h3_marker: String,
    /// Style for H4 headings.
    pub h4_style: Style,
    /// Marker written before H4 heading text (default `"┄ "`).
    pub h4_marker: String,
    /// Style for H5 headings.
    pub h5_style: Style,
    /// Marker written before H5 heading text (default `"╌ "`).
    pub h5_marker: String,
    /// Style for H6 headings.
    pub h6_style: Style,
    /// Marker written before H6 heading text (default `"· "`).
    pub h6_marker: String,
    /// Style for footnote references and definitions.
    pub footnote_style: Style,
    /// Style for math expressions.
    pub math_style: Style,
    /// Style for `[!NOTE]` alerts.
    pub alert_note_style: Style,
    /// Icon and label written for `[!NOTE]` alerts (default `"ℹ NOTE"`).
    pub alert_note_label: String,
    /// Style for `[!TIP]` alerts.
    pub alert_tip_style: Style,
    /// Icon and label written for `[!TIP]` alerts (default `"◆ TIP"`).
    pub alert_tip_label: String,
    /// Style for `[!IMPORTANT]` alerts.
    pub alert_important_style: Style,
    /// Icon and label written for `[!IMPORTANT]` alerts (default `"★ IMPORTANT"`).
    pub alert_important_label: String,
    /// Style for `[!WARNING]` alerts.
    pub alert_warning_style: Style,
    /// Icon and label written for `[!WARNING]` alerts (default `"⚠ WARNING"`).
    pub alert_warning_label: String,
    /// Style for `[!CAUTION]` alerts.
    pub alert_caution_style: Style,
    /// Icon and label written for `[!CAUTION]` alerts (default `"✖ CAUTION"`).
    pub alert_caution_label: String,
    /// Background-colored padding space written before H1 text.
    pub h1_prefix_style: Style,
    /// Style for H1 heading text (fg + bg color).
    pub h1_text_style: Style,
}

impl Theme {
    /// Set the H1 style, keeping `h1_prefix_style`'s background in sync with `text_style`'s.
    pub fn with_h1(mut self, text_style: Style) -> Self {
        let bg = text_style.get_bg_color();
        self.h1_prefix_style = Style::new().bg_color(bg).fg_color(bg);
        self.h1_text_style = text_style;
        self
    }
}

/// Default heading markers and alert labels, shared by every built-in theme; only colors vary
/// between themes.
#[allow(clippy::type_complexity)]
fn default_markers_and_labels() -> (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
) {
    (
        "━━ ".to_string(),
        "── ".to_string(),
        "┄ ".to_string(),
        "╌ ".to_string(),
        "· ".to_string(),
        "ℹ NOTE".to_string(),
        "◆ TIP".to_string(),
        "★ IMPORTANT".to_string(),
        "⚠ WARNING".to_string(),
        "✖ CAUTION".to_string(),
    )
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    RgbColor(r, g, b).into()
}

fn h1(bg: Color, fg: Color) -> (Style, Style) {
    (
        Style::new().bg_color(Some(bg)).fg_color(Some(bg)),
        Style::new().bg_color(Some(bg)).fg_color(Some(fg)).bold(),
    )
}

impl Theme {
    /// A theme for dark terminal backgrounds (the default).
    pub fn dark() -> Self {
        let (h1_prefix_style, h1_text_style) = (
            Style::new()
                .bg_color(Some(AnsiColor::BrightBlue.into()))
                .fg_color(Some(AnsiColor::BrightBlue.into())),
            Style::new()
                .bg_color(Some(AnsiColor::BrightBlue.into()))
                .fg_color(Some(AnsiColor::BrightWhite.into()))
                .bold(),
        );
        let (
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
        ) = default_markers_and_labels();
        Self {
            html_block_style: Style::new().fg_color(Some(AnsiColor::Green.into())),
            inline_html_style: Style::new().fg_color(Some(AnsiColor::Green.into())),
            code_style: Style::new().fg_color(Some(AnsiColor::Yellow.into())),
            link_style: Style::new().fg_color(Some(AnsiColor::Blue.into())),
            image_link_style: Style::new().fg_color(Some(AnsiColor::Magenta.into())),
            rule_color: AnsiColor::Green.into(),
            quote_border_style: Style::new().fg_color(Some(AnsiColor::Cyan.into())),
            h2_style: Style::new().fg_color(Some(AnsiColor::Blue.into())).bold(),
            h3_style: Style::new().fg_color(Some(AnsiColor::Cyan.into())).bold(),
            h4_style: Style::new().fg_color(Some(AnsiColor::Green.into())).bold(),
            h5_style: Style::new().fg_color(Some(AnsiColor::Yellow.into())).bold(),
            h6_style: Style::new()
                .fg_color(Some(AnsiColor::Magenta.into()))
                .bold(),
            footnote_style: Style::new().fg_color(Some(AnsiColor::Cyan.into())),
            math_style: Style::new().fg_color(Some(AnsiColor::Yellow.into())),
            alert_note_style: Style::new()
                .fg_color(Some(AnsiColor::BrightBlue.into()))
                .bold(),
            alert_tip_style: Style::new()
                .fg_color(Some(AnsiColor::BrightGreen.into()))
                .bold(),
            alert_important_style: Style::new()
                .fg_color(Some(AnsiColor::BrightMagenta.into()))
                .bold(),
            alert_warning_style: Style::new()
                .fg_color(Some(AnsiColor::BrightYellow.into()))
                .bold(),
            alert_caution_style: Style::new()
                .fg_color(Some(AnsiColor::BrightRed.into()))
                .bold(),
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
            h1_prefix_style,
            h1_text_style,
        }
    }

    /// A theme for light terminal backgrounds.
    pub fn light() -> Self {
        let (h1_prefix_style, h1_text_style) = (
            Style::new()
                .bg_color(Some(AnsiColor::BrightCyan.into()))
                .fg_color(Some(AnsiColor::BrightCyan.into())),
            Style::new()
                .bg_color(Some(AnsiColor::BrightCyan.into()))
                .fg_color(Some(AnsiColor::Black.into()))
                .bold(),
        );
        let (
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
        ) = default_markers_and_labels();
        Self {
            html_block_style: Style::new().fg_color(Some(AnsiColor::Green.into())),
            inline_html_style: Style::new().fg_color(Some(AnsiColor::Green.into())),
            code_style: Style::new().fg_color(Some(AnsiColor::Blue.into())),
            link_style: Style::new().fg_color(Some(AnsiColor::Blue.into())),
            image_link_style: Style::new().fg_color(Some(AnsiColor::Magenta.into())),
            rule_color: AnsiColor::Green.into(),
            quote_border_style: Style::new().fg_color(Some(AnsiColor::Cyan.into())),
            h2_style: Style::new()
                .fg_color(Some(AnsiColor::Magenta.into()))
                .bold(),
            h3_style: Style::new().fg_color(Some(AnsiColor::Blue.into())).bold(),
            h4_style: Style::new().fg_color(Some(AnsiColor::Cyan.into())).bold(),
            h5_style: Style::new().fg_color(Some(AnsiColor::Green.into())).bold(),
            h6_style: Style::new().fg_color(Some(AnsiColor::Red.into())).bold(),
            footnote_style: Style::new().fg_color(Some(AnsiColor::Cyan.into())),
            math_style: Style::new().fg_color(Some(AnsiColor::Blue.into())),
            alert_note_style: Style::new().fg_color(Some(AnsiColor::Blue.into())).bold(),
            alert_tip_style: Style::new().fg_color(Some(AnsiColor::Green.into())).bold(),
            alert_important_style: Style::new()
                .fg_color(Some(AnsiColor::Magenta.into()))
                .bold(),
            alert_warning_style: Style::new().fg_color(Some(AnsiColor::Red.into())).bold(),
            alert_caution_style: Style::new()
                .fg_color(Some(AnsiColor::BrightRed.into()))
                .bold(),
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
            h1_prefix_style,
            h1_text_style,
        }
    }

    /// Catppuccin Mocha (dark).
    pub fn catppuccin_mocha() -> Self {
        let (h1_prefix_style, h1_text_style) = h1(rgb(203, 166, 247), rgb(17, 17, 27));
        let (
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
        ) = default_markers_and_labels();
        Self {
            html_block_style: Style::new().fg_color(Some(rgb(250, 179, 135))),
            inline_html_style: Style::new().fg_color(Some(rgb(250, 179, 135))),
            code_style: Style::new().fg_color(Some(rgb(137, 220, 235))),
            link_style: Style::new().fg_color(Some(rgb(137, 180, 250))),
            image_link_style: Style::new().fg_color(Some(rgb(245, 194, 231))),
            rule_color: rgb(148, 226, 213),
            quote_border_style: Style::new().fg_color(Some(rgb(108, 112, 134))),
            h2_style: Style::new().fg_color(Some(rgb(203, 166, 247))).bold(),
            h3_style: Style::new().fg_color(Some(rgb(137, 180, 250))).bold(),
            h4_style: Style::new().fg_color(Some(rgb(137, 220, 235))).bold(),
            h5_style: Style::new().fg_color(Some(rgb(166, 227, 161))).bold(),
            h6_style: Style::new().fg_color(Some(rgb(250, 179, 135))).bold(),
            footnote_style: Style::new().fg_color(Some(rgb(108, 112, 134))),
            math_style: Style::new().fg_color(Some(rgb(137, 220, 235))),
            alert_note_style: Style::new().fg_color(Some(rgb(116, 199, 236))).bold(),
            alert_tip_style: Style::new().fg_color(Some(rgb(166, 227, 161))).bold(),
            alert_important_style: Style::new().fg_color(Some(rgb(203, 166, 247))).bold(),
            alert_warning_style: Style::new().fg_color(Some(rgb(249, 226, 175))).bold(),
            alert_caution_style: Style::new().fg_color(Some(rgb(243, 139, 168))).bold(),
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
            h1_prefix_style,
            h1_text_style,
        }
    }

    /// Catppuccin Latte (light).
    pub fn catppuccin_latte() -> Self {
        let (h1_prefix_style, h1_text_style) = h1(rgb(136, 57, 239), rgb(239, 241, 245));
        let (
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
        ) = default_markers_and_labels();
        Self {
            html_block_style: Style::new().fg_color(Some(rgb(254, 100, 11))),
            inline_html_style: Style::new().fg_color(Some(rgb(254, 100, 11))),
            code_style: Style::new().fg_color(Some(rgb(4, 165, 229))),
            link_style: Style::new().fg_color(Some(rgb(30, 102, 245))),
            image_link_style: Style::new().fg_color(Some(rgb(234, 118, 203))),
            rule_color: rgb(23, 146, 153),
            quote_border_style: Style::new().fg_color(Some(rgb(124, 127, 147))),
            h2_style: Style::new().fg_color(Some(rgb(136, 57, 239))).bold(),
            h3_style: Style::new().fg_color(Some(rgb(30, 102, 245))).bold(),
            h4_style: Style::new().fg_color(Some(rgb(23, 146, 153))).bold(),
            h5_style: Style::new().fg_color(Some(rgb(64, 160, 43))).bold(),
            h6_style: Style::new().fg_color(Some(rgb(254, 100, 11))).bold(),
            footnote_style: Style::new().fg_color(Some(rgb(124, 127, 147))),
            math_style: Style::new().fg_color(Some(rgb(4, 165, 229))),
            alert_note_style: Style::new().fg_color(Some(rgb(32, 159, 181))).bold(),
            alert_tip_style: Style::new().fg_color(Some(rgb(64, 160, 43))).bold(),
            alert_important_style: Style::new().fg_color(Some(rgb(136, 57, 239))).bold(),
            alert_warning_style: Style::new().fg_color(Some(rgb(223, 142, 29))).bold(),
            alert_caution_style: Style::new().fg_color(Some(rgb(210, 15, 57))).bold(),
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
            h1_prefix_style,
            h1_text_style,
        }
    }

    /// Gruvbox Dark.
    pub fn gruvbox_dark() -> Self {
        let (h1_prefix_style, h1_text_style) = h1(rgb(250, 189, 47), rgb(40, 40, 40));
        let (
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
        ) = default_markers_and_labels();
        Self {
            html_block_style: Style::new().fg_color(Some(rgb(214, 93, 14))),
            inline_html_style: Style::new().fg_color(Some(rgb(214, 93, 14))),
            code_style: Style::new().fg_color(Some(rgb(131, 165, 152))),
            link_style: Style::new().fg_color(Some(rgb(131, 165, 152))),
            image_link_style: Style::new().fg_color(Some(rgb(211, 134, 155))),
            rule_color: rgb(184, 187, 38),
            quote_border_style: Style::new().fg_color(Some(rgb(142, 192, 124))),
            h2_style: Style::new().fg_color(Some(rgb(250, 189, 47))).bold(),
            h3_style: Style::new().fg_color(Some(rgb(131, 165, 152))).bold(),
            h4_style: Style::new().fg_color(Some(rgb(142, 192, 124))).bold(),
            h5_style: Style::new().fg_color(Some(rgb(211, 134, 155))).bold(),
            h6_style: Style::new().fg_color(Some(rgb(254, 128, 25))).bold(),
            footnote_style: Style::new().fg_color(Some(rgb(146, 131, 116))),
            math_style: Style::new().fg_color(Some(rgb(131, 165, 152))),
            alert_note_style: Style::new().fg_color(Some(rgb(131, 165, 152))).bold(),
            alert_tip_style: Style::new().fg_color(Some(rgb(184, 187, 38))).bold(),
            alert_important_style: Style::new().fg_color(Some(rgb(211, 134, 155))).bold(),
            alert_warning_style: Style::new().fg_color(Some(rgb(254, 128, 25))).bold(),
            alert_caution_style: Style::new().fg_color(Some(rgb(251, 73, 52))).bold(),
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
            h1_prefix_style,
            h1_text_style,
        }
    }

    /// Gruvbox Light.
    pub fn gruvbox_light() -> Self {
        let (h1_prefix_style, h1_text_style) = h1(rgb(181, 118, 20), rgb(251, 241, 199));
        let (
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
        ) = default_markers_and_labels();
        Self {
            html_block_style: Style::new().fg_color(Some(rgb(175, 58, 3))),
            inline_html_style: Style::new().fg_color(Some(rgb(175, 58, 3))),
            code_style: Style::new().fg_color(Some(rgb(7, 102, 120))),
            link_style: Style::new().fg_color(Some(rgb(7, 102, 120))),
            image_link_style: Style::new().fg_color(Some(rgb(143, 63, 113))),
            rule_color: rgb(66, 123, 88),
            quote_border_style: Style::new().fg_color(Some(rgb(66, 123, 88))),
            h2_style: Style::new().fg_color(Some(rgb(181, 118, 20))).bold(),
            h3_style: Style::new().fg_color(Some(rgb(7, 102, 120))).bold(),
            h4_style: Style::new().fg_color(Some(rgb(66, 123, 88))).bold(),
            h5_style: Style::new().fg_color(Some(rgb(143, 63, 113))).bold(),
            h6_style: Style::new().fg_color(Some(rgb(175, 58, 3))).bold(),
            footnote_style: Style::new().fg_color(Some(rgb(124, 111, 100))),
            math_style: Style::new().fg_color(Some(rgb(7, 102, 120))),
            alert_note_style: Style::new().fg_color(Some(rgb(7, 102, 120))).bold(),
            alert_tip_style: Style::new().fg_color(Some(rgb(121, 116, 14))).bold(),
            alert_important_style: Style::new().fg_color(Some(rgb(143, 63, 113))).bold(),
            alert_warning_style: Style::new().fg_color(Some(rgb(181, 118, 20))).bold(),
            alert_caution_style: Style::new().fg_color(Some(rgb(157, 0, 6))).bold(),
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
            h1_prefix_style,
            h1_text_style,
        }
    }

    /// Dracula.
    pub fn dracula() -> Self {
        let (h1_prefix_style, h1_text_style) = h1(rgb(189, 147, 249), rgb(40, 42, 54));
        let (
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
        ) = default_markers_and_labels();
        Self {
            html_block_style: Style::new().fg_color(Some(rgb(255, 184, 108))),
            inline_html_style: Style::new().fg_color(Some(rgb(255, 184, 108))),
            code_style: Style::new().fg_color(Some(rgb(241, 250, 140))),
            link_style: Style::new().fg_color(Some(rgb(139, 233, 253))),
            image_link_style: Style::new().fg_color(Some(rgb(255, 121, 198))),
            rule_color: rgb(80, 250, 123),
            quote_border_style: Style::new().fg_color(Some(rgb(98, 114, 164))),
            h2_style: Style::new().fg_color(Some(rgb(189, 147, 249))).bold(),
            h3_style: Style::new().fg_color(Some(rgb(139, 233, 253))).bold(),
            h4_style: Style::new().fg_color(Some(rgb(80, 250, 123))).bold(),
            h5_style: Style::new().fg_color(Some(rgb(255, 121, 198))).bold(),
            h6_style: Style::new().fg_color(Some(rgb(255, 184, 108))).bold(),
            footnote_style: Style::new().fg_color(Some(rgb(98, 114, 164))),
            math_style: Style::new().fg_color(Some(rgb(241, 250, 140))),
            alert_note_style: Style::new().fg_color(Some(rgb(139, 233, 253))).bold(),
            alert_tip_style: Style::new().fg_color(Some(rgb(80, 250, 123))).bold(),
            alert_important_style: Style::new().fg_color(Some(rgb(255, 121, 198))).bold(),
            alert_warning_style: Style::new().fg_color(Some(rgb(255, 184, 108))).bold(),
            alert_caution_style: Style::new().fg_color(Some(rgb(255, 85, 85))).bold(),
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
            h1_prefix_style,
            h1_text_style,
        }
    }

    /// Nord.
    pub fn nord() -> Self {
        let (h1_prefix_style, h1_text_style) = h1(rgb(94, 129, 172), rgb(236, 239, 244));
        let (
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
        ) = default_markers_and_labels();
        Self {
            html_block_style: Style::new().fg_color(Some(rgb(208, 135, 112))),
            inline_html_style: Style::new().fg_color(Some(rgb(208, 135, 112))),
            code_style: Style::new().fg_color(Some(rgb(143, 188, 187))),
            link_style: Style::new().fg_color(Some(rgb(136, 192, 208))),
            image_link_style: Style::new().fg_color(Some(rgb(180, 142, 173))),
            rule_color: rgb(136, 192, 208),
            quote_border_style: Style::new().fg_color(Some(rgb(76, 86, 106))),
            h2_style: Style::new().fg_color(Some(rgb(94, 129, 172))).bold(),
            h3_style: Style::new().fg_color(Some(rgb(129, 161, 193))).bold(),
            h4_style: Style::new().fg_color(Some(rgb(136, 192, 208))).bold(),
            h5_style: Style::new().fg_color(Some(rgb(163, 190, 140))).bold(),
            h6_style: Style::new().fg_color(Some(rgb(235, 203, 139))).bold(),
            footnote_style: Style::new().fg_color(Some(rgb(76, 86, 106))),
            math_style: Style::new().fg_color(Some(rgb(143, 188, 187))),
            alert_note_style: Style::new().fg_color(Some(rgb(143, 188, 187))).bold(),
            alert_tip_style: Style::new().fg_color(Some(rgb(163, 190, 140))).bold(),
            alert_important_style: Style::new().fg_color(Some(rgb(180, 142, 173))).bold(),
            alert_warning_style: Style::new().fg_color(Some(rgb(235, 203, 139))).bold(),
            alert_caution_style: Style::new().fg_color(Some(rgb(191, 97, 106))).bold(),
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
            h1_prefix_style,
            h1_text_style,
        }
    }

    /// Solarized Dark.
    pub fn solarized_dark() -> Self {
        let (h1_prefix_style, h1_text_style) = h1(rgb(38, 139, 210), rgb(0, 43, 54));
        let (
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
        ) = default_markers_and_labels();
        Self {
            html_block_style: Style::new().fg_color(Some(rgb(203, 75, 22))),
            inline_html_style: Style::new().fg_color(Some(rgb(203, 75, 22))),
            code_style: Style::new().fg_color(Some(rgb(42, 161, 152))),
            link_style: Style::new().fg_color(Some(rgb(38, 139, 210))),
            image_link_style: Style::new().fg_color(Some(rgb(211, 54, 130))),
            rule_color: rgb(42, 161, 152),
            quote_border_style: Style::new().fg_color(Some(rgb(88, 110, 117))),
            h2_style: Style::new().fg_color(Some(rgb(38, 139, 210))).bold(),
            h3_style: Style::new().fg_color(Some(rgb(42, 161, 152))).bold(),
            h4_style: Style::new().fg_color(Some(rgb(133, 153, 0))).bold(),
            h5_style: Style::new().fg_color(Some(rgb(211, 54, 130))).bold(),
            h6_style: Style::new().fg_color(Some(rgb(203, 75, 22))).bold(),
            footnote_style: Style::new().fg_color(Some(rgb(88, 110, 117))),
            math_style: Style::new().fg_color(Some(rgb(42, 161, 152))),
            alert_note_style: Style::new().fg_color(Some(rgb(108, 113, 196))).bold(),
            alert_tip_style: Style::new().fg_color(Some(rgb(133, 153, 0))).bold(),
            alert_important_style: Style::new().fg_color(Some(rgb(211, 54, 130))).bold(),
            alert_warning_style: Style::new().fg_color(Some(rgb(181, 137, 0))).bold(),
            alert_caution_style: Style::new().fg_color(Some(rgb(220, 50, 47))).bold(),
            h2_marker,
            h3_marker,
            h4_marker,
            h5_marker,
            h6_marker,
            alert_note_label,
            alert_tip_label,
            alert_important_label,
            alert_warning_label,
            alert_caution_label,
            h1_prefix_style,
            h1_text_style,
        }
    }

    /// Solarized Light.
    pub fn solarized_light() -> Self {
        let (h1_prefix_style, h1_text_style) = h1(rgb(38, 139, 210), rgb(253, 246, 227));
        Self {
            h1_prefix_style,
            h1_text_style,
            ..Self::solarized_dark()
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// Combine styles.
pub trait CombineStyle {
    /// Put this style on top of the other style.
    ///
    /// Return a new style which falls back to the `other` style for all style attributes not
    /// specified in this style.
    fn on_top_of(self, other: &Self) -> Self;
}

impl CombineStyle for Style {
    /// Put this style on top of the `other` style.
    fn on_top_of(self, other: &Style) -> Style {
        Style::new()
            .fg_color(self.get_fg_color().or(other.get_fg_color()))
            .bg_color(self.get_bg_color().or(other.get_bg_color()))
            .effects(other.get_effects() | self.get_effects())
            .underline_color(self.get_underline_color().or(other.get_underline_color()))
    }
}
