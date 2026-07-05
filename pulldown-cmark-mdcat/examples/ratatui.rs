// Copyright Mouhieddine Sabir <me@mouhieddine.dev>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use pulldown_cmark_mdcat::ratatui::{MdcatWidget, MdcatWidgetState, RenderOptions, Renderer};
use pulldown_cmark_mdcat::resources::FileResourceHandler;
use pulldown_cmark_mdcat::Environment;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::widgets::Block;
use ratatui::DefaultTerminal;

const SAMPLE_MARKDOWN: &str = r#"# mdcat in Ratatui

This example renders Markdown with **mdcat** inside a native Ratatui widget.

## What to look for

- **Bold**, _italic_, `inline code`, and ~~strikethrough~~ are native Ratatui spans.
- Markdown links keep visible references: [mdcat](https://github.com/BIRSAx2/mdcat).
- The widget uses the current width, so resizing the terminal reflows this text.

| Feature | Output |
| --- | --- |
| Styling | Ratatui spans |
| Links | Visible references |
| Rendering | `MdcatWidget` |

> Tip: pass a Markdown file path to this example to render your own document.
"#;

fn main() -> io::Result<()> {
    let (title, markdown, base_dir) = read_markdown()?;
    let file_handler = FileResourceHandler::new(20_000_000);
    let renderer = match base_dir {
        Some(base_dir) => Renderer::new(
            RenderOptions::default()
                .environment(Environment::for_local_directory(&base_dir)?)
                .resource_handler(&file_handler),
        ),
        None => Renderer::default(),
    };
    let mut terminal = ratatui::try_init()?;
    let mut state = MdcatWidgetState::new();
    let _ = state.detect_images();
    let result = run(&mut terminal, &title, &markdown, &renderer, &mut state);
    ratatui::try_restore()?;
    result
}

fn read_markdown() -> io::Result<(String, String, Option<PathBuf>)> {
    match env::args().nth(1) {
        Some(path) if path == "-h" || path == "--help" => {
            println!("Usage: cargo run -p pulldown-cmark-mdcat --example ratatui --features ratatui-demo -- [FILE]");
            std::process::exit(0);
        }
        Some(path) => {
            let path_buf = PathBuf::from(&path);
            let markdown = fs::read_to_string(&path_buf)?;
            let base_dir = fs::canonicalize(&path_buf)?.parent().map(PathBuf::from);
            Ok((path, markdown, base_dir))
        }
        None => Ok((
            "embedded sample".to_owned(),
            SAMPLE_MARKDOWN.to_owned(),
            None,
        )),
    }
}

fn run(
    terminal: &mut DefaultTerminal,
    title: &str,
    markdown: &str,
    renderer: &Renderer<'_>,
    state: &mut MdcatWidgetState,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let block = Block::bordered()
                .title(format!(" mdcat + Ratatui: {title} "))
                .title_bottom(" q/Esc/Ctrl-C quit | arrows scroll | PgUp/PgDn jump ");
            frame.render_stateful_widget(
                MdcatWidget::with_renderer(markdown, renderer.clone()).block(block),
                area,
                state,
            );
        })?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Down | KeyCode::Char('j') => state.scroll_down(1),
                    KeyCode::Up | KeyCode::Char('k') => state.scroll_up(1),
                    KeyCode::PageDown => state.scroll_down(10),
                    KeyCode::PageUp => state.scroll_up(10),
                    KeyCode::Home => state.set_scroll(0),
                    KeyCode::End => {
                        if let Ok(size) = terminal.size() {
                            let lines = state.total_lines() as u16;
                            state.set_scroll(lines.saturating_sub(size.height));
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
    Ok(())
}
