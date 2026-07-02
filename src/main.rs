// Copyright 2018-2020 Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![deny(warnings, clippy::all)]
#![forbid(unsafe_code)]

//! Show CommonMark documents on TTYs.

use std::time::Duration;

use clap::{CommandFactory, Parser};
use clap_complete::generate;
use mdcat::{create_resource_handler, process_file};
use pulldown_cmark_mdcat::terminal::{TerminalProgram, TerminalSize};
use pulldown_cmark_mdcat::{Settings, Theme};
use syntect::highlighting::Theme as SyntectTheme;
use tracing::{event, Level};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::EnvFilter;
use two_face::syntax::extra_newlines;
use two_face::theme::{EmbeddedThemeName, LazyThemeSet};

use mdcat::args::{Args, ThemeChoice};
use mdcat::output::Output;

struct ResolvedTheme {
    mdcat: Theme,
    is_light: bool,
    default_syntax: Option<EmbeddedThemeName>,
}

fn resolve_theme(choice: ThemeChoice, is_tty: bool) -> ResolvedTheme {
    match choice {
        ThemeChoice::Auto => {
            let is_light = if is_tty {
                let mut opts = terminal_colorsaurus::QueryOptions::default();
                opts.timeout = Duration::from_millis(100);
                matches!(
                    terminal_colorsaurus::theme_mode(opts),
                    Ok(terminal_colorsaurus::ThemeMode::Light)
                )
            } else {
                false
            };
            ResolvedTheme {
                mdcat: if is_light {
                    Theme::light()
                } else {
                    Theme::dark()
                },
                is_light,
                default_syntax: None,
            }
        }
        ThemeChoice::Dark => ResolvedTheme {
            mdcat: Theme::dark(),
            is_light: false,
            default_syntax: None,
        },
        ThemeChoice::Light => ResolvedTheme {
            mdcat: Theme::light(),
            is_light: true,
            default_syntax: None,
        },
        ThemeChoice::CatppuccinMocha => ResolvedTheme {
            mdcat: Theme::catppuccin_mocha(),
            is_light: false,
            default_syntax: Some(EmbeddedThemeName::CatppuccinMocha),
        },
        ThemeChoice::CatppuccinLatte => ResolvedTheme {
            mdcat: Theme::catppuccin_latte(),
            is_light: true,
            default_syntax: Some(EmbeddedThemeName::CatppuccinLatte),
        },
        ThemeChoice::GruvboxDark => ResolvedTheme {
            mdcat: Theme::gruvbox_dark(),
            is_light: false,
            default_syntax: Some(EmbeddedThemeName::GruvboxDark),
        },
        ThemeChoice::GruvboxLight => ResolvedTheme {
            mdcat: Theme::gruvbox_light(),
            is_light: true,
            default_syntax: Some(EmbeddedThemeName::GruvboxLight),
        },
        ThemeChoice::Dracula => ResolvedTheme {
            mdcat: Theme::dracula(),
            is_light: false,
            default_syntax: Some(EmbeddedThemeName::Dracula),
        },
        ThemeChoice::Nord => ResolvedTheme {
            mdcat: Theme::nord(),
            is_light: false,
            default_syntax: Some(EmbeddedThemeName::Nord),
        },
        ThemeChoice::SolarizedDark => ResolvedTheme {
            mdcat: Theme::solarized_dark(),
            is_light: false,
            default_syntax: Some(EmbeddedThemeName::SolarizedDark),
        },
        ThemeChoice::SolarizedLight => ResolvedTheme {
            mdcat: Theme::solarized_light(),
            is_light: true,
            default_syntax: Some(EmbeddedThemeName::SolarizedLight),
        },
    }
}

fn resolve_syntax_theme(
    is_light: bool,
    default_embedded: Option<EmbeddedThemeName>,
) -> Option<SyntectTheme> {
    let bat_name = if is_light {
        std::env::var("BAT_THEME_LIGHT")
            .or_else(|_| std::env::var("BAT_THEME"))
            .ok()
    } else {
        std::env::var("BAT_THEME_DARK")
            .or_else(|_| std::env::var("BAT_THEME"))
            .ok()
    };
    let name = bat_name.or_else(|| default_embedded.map(|n| n.as_name().to_owned()))?;
    let themes = LazyThemeSet::from(two_face::theme::extra());
    themes.get(&name).cloned()
}

fn main() {
    // Initialize curl for remote resources
    curl::init();

    // Setup tracing
    let filter = EnvFilter::builder()
        // Disable all logging by default, to avoid interfering with regular output at all cost.
        // tracing is a debugging tool here so we expect it to be enabled explicitly.
        .with_default_directive(LevelFilter::OFF.into())
        .with_env_var("MDCAT_LOG")
        .from_env_lossy();
    tracing_subscriber::fmt::Subscriber::builder()
        .pretty()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse().command;
    event!(target: "mdcat::main", Level::TRACE, ?args, "mdcat arguments");

    if let Some(shell) = args.completions {
        let binary = match args {
            mdcat::args::Command::Mdcat { .. } => "mdcat",
            mdcat::args::Command::Mdless { .. } => "mdless",
        };
        let mut command = Args::command();
        let subcommand = command.find_subcommand_mut(binary).unwrap();
        generate(shell, subcommand, binary, &mut std::io::stdout());
        std::process::exit(0);
    }

    let terminal = if args.no_colour {
        TerminalProgram::Dumb
    } else if args.paginate() || args.ansi_only {
        // A pager won't support any terminal-specific features
        TerminalProgram::Ansi
    } else if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        // Not a TTY: strip all formatting so ANSI escapes don't pollute pipes
        TerminalProgram::Dumb
    } else {
        TerminalProgram::detect()
    };

    if args.detect_and_exit {
        println!("Terminal: {terminal}");
    } else {
        // Enable Ansi color processing on Windows
        #[cfg(windows)]
        anstyle_query::windows::enable_ansi_colors();

        let terminal_size = TerminalSize::detect().unwrap_or_default();
        let terminal_size = match args.columns {
            None => terminal_size.with_max_columns(terminal_size.columns.min(80)),
            Some(0) => terminal_size.with_max_columns(u16::MAX),
            Some(max_columns) => terminal_size.with_max_columns(max_columns),
        };

        let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdout());
        let resolved = resolve_theme(args.theme, is_tty);
        let syntax_theme = resolve_syntax_theme(resolved.is_light, resolved.default_syntax);
        let theme = resolved.mdcat;
        let exit_code = match Output::new(args.paginate()) {
            Ok(mut output) => {
                let settings = Settings {
                    terminal_capabilities: terminal.capabilities(),
                    terminal_size,
                    syntax_set: &extra_newlines(),
                    theme,
                    syntax_theme,
                };
                event!(
                    target: "mdcat::main",
                    Level::TRACE,
                    ?settings.terminal_size,
                    ?settings.terminal_capabilities,
                    "settings"
                );
                // TODO: Handle this error properly
                let resource_handler = create_resource_handler(args.resource_access()).unwrap();
                args.filenames
                    .iter()
                    .try_fold(0, |code, filename| {
                        process_file(filename, &settings, &resource_handler, &mut output)
                            .map(|_| code)
                            .or_else(|error| {
                                eprintln!("Error: {filename}: {error}");
                                if args.fail_fast {
                                    Err(error)
                                } else {
                                    Ok(1)
                                }
                            })
                    })
                    .unwrap_or(1)
            }
            Err(error) => {
                eprintln!("Error: {error:#}");
                128
            }
        };
        event!(target: "mdcat::main", Level::TRACE, "Exiting with final exit code {}", exit_code);
        std::process::exit(exit_code);
    }
}
