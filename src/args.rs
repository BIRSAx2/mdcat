// Copyright 2018-2020 Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use clap::{ValueEnum, ValueHint};
use clap_complete::Shell;

/// Which colour theme to use for rendering.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum ThemeChoice {
    /// Detect dark/light mode from the terminal and pick accordingly.
    #[default]
    Auto,
    /// Use the built-in dark theme.
    Dark,
    /// Use the built-in light theme.
    Light,
    /// Catppuccin Mocha (dark).
    #[value(name = "catppuccin-mocha")]
    CatppuccinMocha,
    /// Catppuccin Latte (light).
    #[value(name = "catppuccin-latte")]
    CatppuccinLatte,
    /// Gruvbox dark.
    #[value(name = "gruvbox-dark")]
    GruvboxDark,
    /// Gruvbox light.
    #[value(name = "gruvbox-light")]
    GruvboxLight,
    /// Dracula.
    Dracula,
    /// Nord.
    Nord,
    /// Solarized dark.
    #[value(name = "solarized-dark")]
    SolarizedDark,
    /// Solarized light.
    #[value(name = "solarized-light")]
    SolarizedLight,
}

/// Which inline image protocol to use, overriding auto-detection.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ImageProtocolChoice {
    /// Disable inline images entirely.
    None,
    /// iTerm2's inline image protocol.
    #[value(name = "iterm2")]
    ITerm2,
    /// The kitty terminal graphics protocol.
    Kitty,
    /// The sixel image protocol.
    Sixel,
}

impl ImageProtocolChoice {
    /// The image capability this choice maps to, or `None` to disable inline images.
    pub fn to_image_capability(
        self,
    ) -> Option<pulldown_cmark_mdcat::terminal::capabilities::ImageCapability> {
        use pulldown_cmark_mdcat::terminal::capabilities::{iterm2, kitty, sixel, ImageCapability};
        match self {
            ImageProtocolChoice::None => None,
            ImageProtocolChoice::ITerm2 => Some(ImageCapability::ITerm2(iterm2::ITerm2Protocol)),
            ImageProtocolChoice::Kitty => {
                Some(ImageCapability::Kitty(kitty::KittyGraphicsProtocol))
            }
            ImageProtocolChoice::Sixel => Some(ImageCapability::Sixel(sixel::SixelProtocol)),
        }
    }
}

fn after_help() -> &'static str {
    "See 'man 1 mdcat' for more information.

mdcat can be installed as or linked to mdless,
for automatic pagination.

Report issues to <https://github.com/BIRSAx2/mdcat>."
}

fn long_version() -> &'static str {
    concat!(
        env!("CARGO_PKG_VERSION"),
        "
Copyright (C) Sebastian Wiesner, Mouhieddine Sabir, and contributors

This program is subject to the terms of the Mozilla Public License,
v. 2.0. If a copy of the MPL was not distributed with this file,
You can obtain one at http://mozilla.org/MPL/2.0/."
    )
}

#[derive(Debug, clap::Parser)]
#[command(multicall = true)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    #[command(version, about, after_help = after_help(), long_version = long_version())]
    Mdcat {
        #[command(flatten)]
        args: CommonArgs,
        /// Paginate the output of mdcat with a pager like less (default for mdless).
        #[arg(short, long, overrides_with = "no_pager")]
        paginate: bool,
        /// Do not paginate output (default). Overrides an earlier --paginate.
        #[arg(short = 'P', long)]
        no_pager: bool,
    },
    #[command(version, about, after_help = after_help(), long_version = long_version())]
    Mdless {
        #[command(flatten)]
        args: CommonArgs,
        /// Do not paginate output (default for mdcat).
        #[arg(short = 'P', long, overrides_with = "paginate")]
        no_pager: bool,
        /// Paginate the output of mdcat with a pager like less (default). Overrides an earlier --no-pager.
        #[arg(short, long)]
        paginate: bool,
    },
}

impl Command {
    pub fn paginate(&self) -> bool {
        match *self {
            // In both cases look at the option indicating the non-default
            // behaviour; the overrides above are configured accordingly.
            Command::Mdcat { paginate, .. } => paginate,
            Command::Mdless { no_pager, .. } => !no_pager,
        }
    }
}

impl std::ops::Deref for Command {
    type Target = CommonArgs;

    fn deref(&self) -> &Self::Target {
        match self {
            Command::Mdcat { args, .. } => args,
            Command::Mdless { args, .. } => args,
        }
    }
}

#[derive(Debug, clap::Args)]
// #[command(author, version, about, after_help = after_help(), long_version = long_version())]
pub struct CommonArgs {
    /// Files to read.  If - read from standard input instead.
    #[arg(default_value="-", value_hint = ValueHint::FilePath)]
    pub filenames: Vec<String>,
    /// Disable all colours and other styles.
    #[arg(short = 'c', long, aliases=["nocolour", "no-color", "nocolor"])]
    pub no_colour: bool,
    /// Maximum number of columns to use for output. Defaults to 80, the terminal width (whichever
    /// is smaller), or `defaults.columns` in `~/.config/mdcat/config.toml`. Pass 0 to disable
    /// line wrapping.
    #[arg(long)]
    pub columns: Option<u16>,
    /// Do not load remote resources like images. Also settable via `defaults.local_only` in
    /// `~/.config/mdcat/config.toml`.
    #[arg(short, long = "local")]
    pub local_only: bool,
    /// Exit immediately if any error occurs processing an input file. Also settable via
    /// `defaults.fail_fast` in `~/.config/mdcat/config.toml`.
    #[arg(long = "fail")]
    pub fail_fast: bool,
    /// Print detected terminal name and exit.
    #[arg(long = "detect-terminal")]
    pub detect_and_exit: bool,
    /// Skip terminal detection and only use ANSI formatting.
    #[arg(long = "ansi", conflicts_with = "no_colour")]
    pub ansi_only: bool,
    /// Generate completions for a shell to standard output and exit.
    #[arg(long)]
    pub completions: Option<Shell>,
    /// Colour theme to use. Defaults to auto-detecting dark or light from the terminal, or to
    /// the `[theme]` section's `base` in `~/.config/mdcat/config.toml` if that file exists.
    #[arg(long, env = "MDCAT_THEME", value_name = "THEME")]
    pub theme: Option<ThemeChoice>,
    /// Watch the input file and re-render on change. Requires a single file argument.
    #[arg(short, long)]
    pub watch: bool,
    /// Add a two-space left margin to all output. Reduces the effective render width accordingly.
    /// Also settable via `defaults.margin` in `~/.config/mdcat/config.toml`.
    #[arg(long)]
    pub margin: bool,
    /// Render typographic punctuation: straight quotes become curly, `--`/`---` become en/em
    /// dashes, and `...` becomes an ellipsis. Also settable via `defaults.smart_punctuation` in
    /// `~/.config/mdcat/config.toml`.
    #[arg(long)]
    pub smart_punctuation: bool,
    /// Print a sample rendered with every built-in theme, to help pick one, and exit.
    #[arg(long)]
    pub list_themes: bool,
    /// Print a table of contents generated from the document's headings before its content.
    /// Entries link to the source file, for terminals and later viewers that support OSC 8
    /// links and resolve GitHub-style heading anchors; plain text on standard input, since
    /// there's no file to link to.
    #[arg(long)]
    pub toc: bool,
    /// Force a specific inline image protocol instead of auto-detecting one from the terminal.
    /// Useful inside tmux/screen, where the outer terminal's capabilities usually aren't visible
    /// to auto-detection. `none` disables inline images entirely. Also settable via
    /// `$MDCAT_IMAGE_PROTOCOL`.
    #[arg(long, env = "MDCAT_IMAGE_PROTOCOL", value_name = "PROTOCOL")]
    pub image_protocol: Option<ImageProtocolChoice>,
}

/// What resources mdcat may access.
#[derive(Debug, Copy, Clone)]
pub enum ResourceAccess {
    /// Only allow local resources.
    LocalOnly,
    /// Allow remote resources
    Remote,
}

impl CommonArgs {
    /// Whether remote resource access is permitted.
    pub fn resource_access(&self) -> ResourceAccess {
        if self.local_only {
            ResourceAccess::LocalOnly
        } else {
            ResourceAccess::Remote
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Args;
    use clap::CommandFactory;

    #[test]
    fn verify_app() {
        Args::command().debug_assert();
    }
}
