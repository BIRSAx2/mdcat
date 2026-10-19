// Copyright 2020 Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Rendering tests.
//!
//! Test a set of custom samples as well as all samples from the official common
//! mark spec against fixed snapshots to catch rendering regressions.

#![deny(warnings, clippy::all)]

use std::path::Path;
use std::sync::OnceLock;

use insta::{assert_snapshot, glob};
use pulldown_cmark::{Options, Parser};
use syntect::parsing::SyntaxSet;
use url::Url;

use pulldown_cmark_mdcat::resources::*;
use pulldown_cmark_mdcat::terminal::{TerminalProgram, TerminalSize};
use pulldown_cmark_mdcat::Settings;
use pulldown_cmark_mdcat::{Environment, Theme};

static TEST_READ_LIMIT: u64 = 5_242_880;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn resource_handler() -> DispatchingResourceHandler {
    let handlers: Vec<Box<dyn ResourceUrlHandler>> =
        vec![Box::new(FileResourceHandler::new(TEST_READ_LIMIT))];
    DispatchingResourceHandler::new(handlers)
}

fn render_markdown_to_string(markdown: &str, base_dir: &Path, settings: &Settings) -> String {
    let parser = Parser::new_ext(
        markdown,
        Options::ENABLE_TASKLISTS
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TABLES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_MATH,
    );
    let mut sink = Vec::new();
    let env = Environment {
        hostname: "HOSTNAME".to_string(),
        ..Environment::for_local_directory(&base_dir).unwrap()
    };
    pulldown_cmark_mdcat::push_tty(settings, &env, &resource_handler(), &mut sink, parser).unwrap();
    String::from_utf8(sink).unwrap()
}

fn render_to_string<P: AsRef<Path>>(markdown_file: P, settings: &Settings) -> String {
    let markdown = std::fs::read_to_string(&markdown_file).unwrap();
    let abs_path = std::fs::canonicalize(&markdown_file).unwrap();
    let base_dir = abs_path
        .parent()
        .expect("Absolute file name must have a parent!");
    render_markdown_to_string(&markdown, base_dir, settings)
}

#[test]
fn test_render() {
    let cwd = std::env::current_dir().expect("Require working directory");
    let mut cwd_url = Url::from_directory_path(&cwd).expect("Working directory URL");
    let mut root_url = cwd_url.join("/").expect("Join root URL");
    root_url.set_host(Some("HOSTNAME")).unwrap();
    cwd_url.set_host(Some("HOSTNAME")).unwrap();

    let dumb_settings = Settings {
        terminal_capabilities: TerminalProgram::Dumb.capabilities(),
        terminal_size: TerminalSize::default(),
        theme: Theme::default(),
        syntax_set: syntax_set(),
        syntax_theme: None,
    };
    let ansi_settings = Settings {
        terminal_capabilities: TerminalProgram::Ansi.capabilities(),
        terminal_size: TerminalSize::default(),
        theme: Theme::default(),
        syntax_set: syntax_set(),
        syntax_theme: None,
    };
    let iterm2_settings = Settings {
        terminal_capabilities: TerminalProgram::ITerm2.capabilities(),
        terminal_size: TerminalSize::default(),
        theme: Theme::default(),
        syntax_set: syntax_set(),
        syntax_theme: None,
    };

    glob!("markdown/**/*.md", |markdown_file| {
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path("snapshots/render");
        settings.set_prepend_module_to_snapshot(false);
        settings.add_filter(
            regex::escape(cwd_url.as_str()).as_str(),
            "file://HOSTNAME/WORKING_DIRECTORY/",
        );
        settings.add_filter(
            regex::escape(root_url.as_str()).as_str(),
            "file://HOSTNAME/ROOT/",
        );
        let category = markdown_file
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        let name = markdown_file.file_stem().unwrap().to_str().unwrap();
        settings.set_snapshot_suffix(format!("{category}-{name}"));
        let _guard = settings.bind_to_scope();
        assert_snapshot!("dumb", render_to_string(markdown_file, &dumb_settings));
        assert_snapshot!("ansi", render_to_string(markdown_file, &ansi_settings));
        assert_snapshot!("iterm2", render_to_string(markdown_file, &iterm2_settings));
        drop(_guard);
    });
}

#[test]
fn inline_math_image_fallback_preserves_single_pending_space() {
    let settings = Settings {
        terminal_capabilities: TerminalProgram::ITerm2.capabilities(),
        terminal_size: TerminalSize::default(),
        theme: Theme::default(),
        syntax_set: syntax_set(),
        syntax_theme: None,
    };
    let cwd = std::env::current_dir().expect("Require working directory");
    let output = render_markdown_to_string(r"Text $\unknown$ end", &cwd, &settings);

    assert_eq!(output, "Text\u{1b}[33m \\unknown\u{1b}[0m end\n");
}

#[test]
fn table_as_first_list_item_block_does_not_panic() {
    let settings = Settings {
        terminal_capabilities: TerminalProgram::Dumb.capabilities(),
        terminal_size: TerminalSize::default(),
        theme: Theme::default(),
        syntax_set: syntax_set(),
        syntax_theme: None,
    };
    let cwd = std::env::current_dir().expect("Require working directory");
    let output =
        render_markdown_to_string("- | A | B |\n  | - | - |\n  | 1 | 2 |", &cwd, &settings);

    assert_eq!(
        output,
        "• \n  ──────\n   A  B \n  ──────\n   1  2 \n  ──────\n"
    );
}

#[test]
fn math_in_table_cells_does_not_panic() {
    let settings = Settings {
        terminal_capabilities: TerminalProgram::Dumb.capabilities(),
        terminal_size: TerminalSize::default(),
        theme: Theme::default(),
        syntax_set: syntax_set(),
        syntax_theme: None,
    };
    let cwd = std::env::current_dir().expect("Require working directory");
    let output = render_markdown_to_string(
        "| Math |\n| - |\n| $\\alpha$ |\n| $$\\frac{1}{2}$$ |",
        &cwd,
        &settings,
    );

    assert_eq!(output, "──────\n Math \n──────\n α    \n 1/2  \n──────\n");
}

#[test]
fn footnote_reference_in_table_cell_does_not_panic() {
    let settings = Settings {
        terminal_capabilities: TerminalProgram::Dumb.capabilities(),
        terminal_size: TerminalSize::default(),
        theme: Theme::default(),
        syntax_set: syntax_set(),
        syntax_theme: None,
    };
    let cwd = std::env::current_dir().expect("Require working directory");
    let output = render_markdown_to_string(
        "| Berry | Rs |\n| - | - |\n| Strawberry | 2[^1] |\n\n[^1]: citation needed",
        &cwd,
        &settings,
    );

    assert_eq!(
        output,
        "──────────────────\n Berry       Rs   \n──────────────────\n Strawberry  2[1] \n──────────────────\n\n[1]: citation needed\n"
    );
}

fn render_markdown_with_emoji_to_string(
    markdown: &str,
    base_dir: &Path,
    settings: &Settings,
) -> String {
    let parser = Parser::new_ext(
        markdown,
        Options::ENABLE_TASKLISTS
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TABLES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_MATH,
    );
    let parser = pulldown_cmark_mdcat::substitute_emoji(parser);
    let mut sink = Vec::new();
    let env = Environment {
        hostname: "HOSTNAME".to_string(),
        ..Environment::for_local_directory(&base_dir).unwrap()
    };
    pulldown_cmark_mdcat::push_tty(settings, &env, &resource_handler(), &mut sink, parser).unwrap();
    String::from_utf8(sink).unwrap()
}

#[test]
fn emoji_shortcode_renders_when_enabled_but_not_in_code() {
    let settings = Settings {
        terminal_capabilities: TerminalProgram::Dumb.capabilities(),
        terminal_size: TerminalSize::default(),
        theme: Theme::default(),
        syntax_set: syntax_set(),
        syntax_theme: None,
    };
    let cwd = std::env::current_dir().expect("Require working directory");
    let output = render_markdown_with_emoji_to_string(
        "Thumbs up :+1:\n\n```\ncode block :+1:\n```\n\n`inline code :+1:`\n",
        &cwd,
        &settings,
    );

    assert!(output.contains('\u{1F44D}'));
    assert!(output.contains("code block :+1:"));
    assert!(output.contains("inline code :+1:"));
}

#[test]
fn inline_math_kitty_placement_does_not_move_cursor() {
    let settings = Settings {
        terminal_capabilities: TerminalProgram::Kitty.capabilities(),
        terminal_size: TerminalSize::default(),
        theme: Theme::default(),
        syntax_set: syntax_set(),
        syntax_theme: None,
    };
    let cwd = std::env::current_dir().expect("Require working directory");
    let output = render_markdown_to_string(r"Text $\alpha$, $\beta$.", &cwd, &settings);

    assert!(output.contains("\u{1b}_Ga=T,t=d,I=1,f=100,C=1,m=0,q=2;"));
    assert!(!output.contains(",s="));
    assert!(!output.contains(",v="));
}

#[test]
fn display_math_kitty_placement_does_not_add_extra_newline() {
    let settings = Settings {
        terminal_capabilities: TerminalProgram::Kitty.capabilities(),
        terminal_size: TerminalSize::default(),
        theme: Theme::default(),
        syntax_set: syntax_set(),
        syntax_theme: None,
    };
    let cwd = std::env::current_dir().expect("Require working directory");
    let output = render_markdown_to_string("$$x$$\n$$y$$", &cwd, &settings);

    assert!(output.contains("\u{1b}\\\n\u{1b}_Ga=T,t=d,I=1,f=100,m=0,q=2;"));
    assert!(!output.contains("\u{1b}\\\n\n\u{1b}_Ga=T,t=d,I=1,f=100,m=0,q=2;"));
}

// Mermaid PNGs go through resvg's system font database, so exact bytes aren't reproducible
// across machines; check structurally instead of snapshotting, unlike the Unicode fallback below.

#[test]
fn mermaid_flowchart_renders_as_unicode_when_no_image_support() {
    let settings = Settings {
        terminal_capabilities: TerminalProgram::Dumb.capabilities(),
        terminal_size: TerminalSize::default(),
        theme: Theme::default(),
        syntax_set: syntax_set(),
        syntax_theme: None,
    };
    let cwd = std::env::current_dir().expect("Require working directory");
    let output = render_markdown_to_string(
        "```mermaid\nflowchart TD\n    A[Start] --> B[Done]\n```\n",
        &cwd,
        &settings,
    );

    assert_snapshot!(output);
}

#[test]
#[cfg(feature = "svg")]
fn mermaid_flowchart_renders_as_kitty_image() {
    let settings = Settings {
        terminal_capabilities: TerminalProgram::Kitty.capabilities(),
        terminal_size: TerminalSize::default(),
        theme: Theme::default(),
        syntax_set: syntax_set(),
        syntax_theme: None,
    };
    let cwd = std::env::current_dir().expect("Require working directory");
    let output = render_markdown_to_string(
        "```mermaid\nflowchart TD\n    A[Start] --> B[Done]\n```\n",
        &cwd,
        &settings,
    );

    assert!(output.contains("\u{1b}_Ga=T,t=d,I=1,f=100"));
}

#[test]
fn mermaid_unparsable_diagram_falls_back_to_literal_source() {
    let settings = Settings {
        terminal_capabilities: TerminalProgram::Dumb.capabilities(),
        terminal_size: TerminalSize::default(),
        theme: Theme::default(),
        syntax_set: syntax_set(),
        syntax_theme: None,
    };
    let cwd = std::env::current_dir().expect("Require working directory");
    let output = render_markdown_to_string(
        "```mermaid\nnot a mermaid diagram at all {{{\n```\n",
        &cwd,
        &settings,
    );

    assert!(output.contains("not a mermaid diagram at all {{{"));
}

/// Regression test for a local SVG image rendering as an actual inline image (not falling back
/// to a text link) across every protocol that renders SVG, confirming the full pipeline from
/// markdown through resource loading, SVG rasterization, and protocol-specific encoding stays
/// intact.
#[test]
#[cfg(feature = "svg")]
fn svg_image_renders_as_inline_image_on_every_svg_capable_protocol() {
    let cwd = std::env::current_dir().expect("Require working directory");
    // `cwd` for a package's integration tests is the package root (`pulldown-cmark-mdcat/`),
    // whose parent is the workspace root; go through the parent rather than joining a literal
    // `..` segment, since `Path::canonicalize` would resolve that but on Windows also adds a
    // `\\?\`-prefixed verbatim path, which `Url::from_file_path` below doesn't need and
    // shouldn't have to handle.
    let svg_path = cwd
        .parent()
        .expect("pulldown-cmark-mdcat must have a parent directory")
        .join("sample/rust-logo.svg");
    assert!(svg_path.exists(), "{svg_path:?} must exist");
    // Image destinations are resolved as URLs, not native OS paths (see `resolve_reference`),
    // so this must be a `file://` URL rather than `svg_path.display()`: a native Windows path
    // uses backslashes, which aren't valid URL path separators.
    let svg_url = url::Url::from_file_path(&svg_path)
        .unwrap_or_else(|()| panic!("{svg_path:?} must be an absolute path"));
    let markdown = format!("![Rust logo]({svg_url})\n");

    // One marker per protocol identifying its inline-image escape sequence; each only appears
    // if `write_inline_image` actually ran, rather than the image falling back to a link.
    for (name, capabilities, marker) in [
        (
            "kitty",
            TerminalProgram::Kitty.capabilities(),
            "\u{1b}_Ga=T,t=d,I=1,f=32",
        ),
        (
            "iterm2",
            TerminalProgram::ITerm2.capabilities(),
            "\u{1b}]1337;File=",
        ),
        (
            "sixel-via-foot",
            TerminalProgram::Foot.capabilities(),
            "\u{1b}[?8452h",
        ),
    ] {
        let settings = Settings {
            terminal_capabilities: capabilities,
            terminal_size: TerminalSize::default(),
            theme: Theme::default(),
            syntax_set: syntax_set(),
            syntax_theme: None,
        };
        let output = render_markdown_to_string(&markdown, &cwd, &settings);
        assert!(
            output.contains(marker),
            "{name}: expected inline-image marker {marker:?} in output, got {output:?}"
        );
        assert!(
            !output.contains("Rust logo]("),
            "{name}: image fell back to a text link instead of rendering inline"
        );
    }
}
