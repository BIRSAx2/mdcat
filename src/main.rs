// Copyright 2018-2020 Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![deny(warnings, clippy::all)]
#![forbid(unsafe_code)]

//! Show CommonMark documents on TTYs.

use std::io::Write;
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::Duration;

use anyhow::Context;
use clap::{CommandFactory, Parser, ValueEnum};
use clap_complete::generate;
use mdcat::{create_resource_handler, process_file};
use notify::{RecursiveMode, Watcher};
use pulldown_cmark::Parser as MarkdownParser;
use pulldown_cmark_mdcat::resources::{NoopResourceHandler, ResourceUrlHandler};
use pulldown_cmark_mdcat::terminal::{TerminalProgram, TerminalSize};
use pulldown_cmark_mdcat::{markdown_options, push_tty, Environment, Settings, Theme};
use syntect::highlighting::Theme as SyntectTheme;
use tracing::{event, Level};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::EnvFilter;
use two_face::syntax::extra_newlines;
use two_face::theme::{EmbeddedThemeName, LazyThemeSet};

use mdcat::args::{Args, ThemeChoice};
use mdcat::output::Output;

/// Clear the terminal screen and scrollback, then move the cursor home.
fn clear_screen() {
    print!("\x1B[3J\x1B[H\x1B[2J");
    let _ = std::io::stdout().flush();
}

/// Watch `filename` for changes, re-rendering to `output` on every save.
///
/// Watches the parent directory rather than the file itself, because
/// editors commonly save by writing a temporary file and renaming it over
/// the original, which a direct file watch can miss.
fn watch_file(
    filename: &str,
    settings: &Settings,
    resource_handler: &dyn ResourceUrlHandler,
    output: &mut Output,
    margin: bool,
    smart_punctuation: bool,
) -> anyhow::Result<()> {
    let path = Path::new(filename)
        .canonicalize()
        .with_context(|| format!("Failed to resolve path of {filename}"))?;
    let parent = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| Path::new(".").to_path_buf());

    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |result| {
        let _ = tx.send(result);
    })
    .with_context(|| "Failed to set up file watcher".to_string())?;
    watcher
        .watch(&parent, RecursiveMode::NonRecursive)
        .with_context(|| format!("Failed to watch {}", parent.display()))?;

    let render = |output: &mut Output| {
        clear_screen();
        if let Err(error) = process_file(
            filename,
            settings,
            resource_handler,
            output,
            margin,
            smart_punctuation,
        ) {
            eprintln!("Error: {filename}: {error:#}");
        }
    };

    render(output);
    eprintln!("Watching {filename} for changes, press Ctrl+C to stop…");

    loop {
        match rx.recv() {
            Ok(Ok(event))
                if !matches!(event.kind, notify::EventKind::Access(_))
                    && event.paths.contains(&path) =>
            {
                // Debounce: editors often fire several events per save.
                while rx.recv_timeout(Duration::from_millis(150)).is_ok() {}
                render(output);
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                event!(Level::WARN, %error, "Watch error");
            }
            Err(_) => break,
        }
    }
    Ok(())
}

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

/// A short sample exercising headings, inline styles, an alert, and a code block, to give a
/// representative preview of a theme's colours.
const THEME_SAMPLE: &str = "\
## Sample heading

Some **bold**, _italic_, `inline code`, and a [link](https://example.com).

> [!NOTE]
> A quick note in a callout.

```rust
fn greet(name: &str) -> String {
    format!(\"Hello, {name}!\")
}
```";

/// Print a short sample rendered with every built-in theme, to help pick one.
fn list_themes() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let env = Environment::for_local_directory(&cwd)?;
    let syntax_set = extra_newlines();
    let mut writer = std::io::BufWriter::new(std::io::stdout());

    let ignore_broken_pipe = |error: std::io::Error| {
        if error.kind() == std::io::ErrorKind::BrokenPipe {
            Ok(())
        } else {
            Err(error)
        }
    };

    for choice in ThemeChoice::value_variants() {
        // `auto` isn't a distinct visual theme; it just picks dark or light for us.
        if matches!(choice, ThemeChoice::Auto) {
            continue;
        }
        let name = choice
            .to_possible_value()
            .expect("all ThemeChoice variants have a value name")
            .get_name()
            .to_string();
        let resolved = resolve_theme(*choice, false);
        let syntax_theme = resolve_syntax_theme(resolved.is_light, resolved.default_syntax);
        let settings = Settings {
            terminal_capabilities: TerminalProgram::Ansi.capabilities(),
            terminal_size: TerminalSize::default(),
            syntax_set: &syntax_set,
            theme: resolved.mdcat,
            syntax_theme,
        };
        writeln!(writer, "=== {name} ===\n").or_else(&ignore_broken_pipe)?;
        let parser = MarkdownParser::new_ext(THEME_SAMPLE, markdown_options(false));
        push_tty(&settings, &env, &NoopResourceHandler, &mut writer, parser)
            .or_else(&ignore_broken_pipe)?;
        writeln!(writer).or_else(&ignore_broken_pipe)?;
    }
    writer.flush().or_else(&ignore_broken_pipe)?;
    Ok(())
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

    if args.list_themes {
        if let Err(error) = list_themes() {
            eprintln!("Error: {error:#}");
            std::process::exit(1);
        }
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
        // Reserve the margin's two columns so wrapped output plus margin never
        // exceeds the requested width.
        let terminal_size = if args.margin {
            terminal_size.with_max_columns(terminal_size.columns.saturating_sub(2))
        } else {
            terminal_size
        };

        if args.watch && args.paginate() {
            eprintln!("Error: --watch cannot be combined with --paginate");
            std::process::exit(1);
        }
        if args.watch && (args.filenames.len() != 1 || args.filenames[0] == "-") {
            eprintln!("Error: --watch requires exactly one file argument (not stdin)");
            std::process::exit(1);
        }
        let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdout());
        if args.watch && !is_tty {
            eprintln!("Error: --watch requires standard output to be a terminal");
            std::process::exit(1);
        }

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
                if args.watch {
                    match watch_file(
                        &args.filenames[0],
                        &settings,
                        &resource_handler,
                        &mut output,
                        args.margin,
                        args.smart_punctuation,
                    ) {
                        Ok(()) => 0,
                        Err(error) => {
                            eprintln!("Error: {error:#}");
                            1
                        }
                    }
                } else {
                    args.filenames
                        .iter()
                        .try_fold(0, |code, filename| {
                            process_file(
                                filename,
                                &settings,
                                &resource_handler,
                                &mut output,
                                args.margin,
                                args.smart_punctuation,
                            )
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
