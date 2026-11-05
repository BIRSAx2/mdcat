// Copyright 2020 Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Command line application to render markdown to TTYs.
//!
//! Note that as of version 2.0.0 mdcat itself no longer contains the core rendering functions.
//! Use [`pulldown_cmark_mdcat`] instead.

#![deny(warnings, missing_docs, clippy::all)]
#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::fs::File;
use std::io::stdin;
use std::io::{self, prelude::*, BufReader, BufWriter, LineWriter};
use std::path::PathBuf;

use anyhow::{Context, Result};
use pulldown_cmark::{Event, Parser};
use pulldown_cmark_mdcat::resources::{
    DispatchingResourceHandler, FileResourceHandler, ResourceUrlHandler,
};
use pulldown_cmark_mdcat::{
    expand_tabs, markdown_options, substitute_emoji, Environment, Settings,
};
use resources::CurlResourceHandler;
use tracing::{event, instrument, Level};

use args::ResourceAccess;
use output::Output;

/// Argument parsing for mdcat.
#[allow(missing_docs)]
pub mod args;
/// User configuration file.
pub mod config;
/// Output handling for mdcat.
pub mod output;
/// Resource handling for mdca.
pub mod resources;
/// Splitting input into blocks, for streaming.
mod stream;
/// Table of contents generation.
pub mod toc;

/// Default read size limit for resources.
pub static DEFAULT_RESOURCE_READ_LIMIT: u64 = 104_857_600;

/// A writer that prepends two spaces of left margin after every newline.
///
/// Only used when `--margin` is passed; callers must shrink the render width
/// by 2 columns to account for it.
struct MarginWriter<W: Write> {
    inner: W,
    at_line_start: bool,
}

impl<W: Write> MarginWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            at_line_start: true,
        }
    }
}

impl<W: Write> Write for MarginWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut start = 0;
        while start < buf.len() {
            if self.at_line_start {
                self.inner.write_all(b"  ")?;
                self.at_line_start = false;
            }
            match buf[start..].iter().position(|&b| b == b'\n') {
                Some(pos) => {
                    let end = start + pos + 1;
                    self.inner.write_all(&buf[start..end])?;
                    self.at_line_start = true;
                    start = end;
                }
                None => {
                    self.inner.write_all(&buf[start..])?;
                    start = buf.len();
                }
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Read input for `filename`.
///
/// If `filename` is `-` read from standard input, otherwise try to open and
/// read the given file.
pub fn read_input<T: AsRef<str>>(filename: T) -> Result<(PathBuf, String)> {
    let (base_dir, mut source) = open_input(filename.as_ref())?;
    let mut buffer = String::new();
    source.read_to_string(&mut buffer)?;
    Ok((base_dir, buffer))
}

/// Open input for `filename`, along with the base directory for the input.
///
/// If `filename` is `-` open standard input, otherwise try to open the given file.
fn open_input(filename: &str) -> Result<(PathBuf, Box<dyn BufRead>)> {
    let cd = std::env::current_dir()?;

    if filename == "-" {
        Ok((cd, Box::new(stdin().lock())))
    } else {
        let source = BufReader::new(File::open(filename)?);
        let base_dir = cd
            .join(filename)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(cd);
        Ok((base_dir, Box::new(source)))
    }
}

/// Remove frontmatter from the start of `input`.
///
/// If `render_frontmatter` is true, put a table of the frontmatter in its place, unless the
/// frontmatter isn't a flat key-value mapping, or fails to parse (see
/// [`pulldown_cmark_mdcat::frontmatter_table_markdown`]).
fn replace_frontmatter(input: &str, render_frontmatter: bool) -> Cow<'_, str> {
    let (body, frontmatter) = pulldown_cmark_mdcat::split_frontmatter(input);
    match frontmatter
        .filter(|_| render_frontmatter)
        .and_then(|(format, raw)| pulldown_cmark_mdcat::frontmatter_table_markdown(format, raw))
    {
        Some(table) => Cow::Owned(format!("{table}\n{body}")),
        None => Cow::Borrowed(body),
    }
}

/// Render-time options for [`process_file`], bundling the flags that control how the document
/// is transformed before and during rendering.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderOptions {
    /// Prepend a two-space left margin to every rendered line.
    ///
    /// Callers must shrink `settings.terminal_size` by 2 columns beforehand so wrapping still
    /// respects the requested output width.
    pub margin: bool,
    /// Render typographic punctuation (curly quotes, en/em dashes, ellipsis) instead of the
    /// literal input characters.
    pub smart_punctuation: bool,
    /// Render GitHub-style `:emoji:` shortcodes as Unicode emoji.
    pub emoji: bool,
    /// Render YAML/TOML frontmatter as a table instead of stripping it.
    ///
    /// Has no effect if the frontmatter isn't a flat key-value mapping, or fails to parse; it's
    /// stripped as usual in that case (see
    /// [`pulldown_cmark_mdcat::frontmatter_table_markdown`]).
    pub frontmatter: bool,
    /// Prepend a table of contents generated from the document's headings.
    ///
    /// On standard input (`filename` is `-`) its entries are plain text, since there is no file
    /// to link to; otherwise they link to `filename#slug`.
    pub toc: bool,
    /// If `Some`, expand literal tabs in the input to spaces using that tab stop width before
    /// parsing (see [`pulldown_cmark_mdcat::expand_tabs`]).
    pub tabs: Option<u16>,
    /// Render the input block by block as it is read, instead of reading all of it first.
    ///
    /// A block ends at a blank line followed by a line that is not indented, outside of fenced
    /// code blocks and frontmatter.  Reference links and footnotes only resolve against
    /// definitions in the same block, and `toc` has no effect, as a table of contents needs the
    /// whole document.
    pub stream: bool,
}

/// Process a single file.
///
/// Read from `filename` and render the contents to `output` according to `render_options`.
#[instrument(skip(output, settings, resource_handler), level = "debug")]
pub fn process_file(
    filename: &str,
    settings: &Settings,
    resource_handler: &dyn ResourceUrlHandler,
    output: &mut Output,
    render_options: RenderOptions,
) -> Result<()> {
    if render_options.stream {
        return stream_file(filename, settings, resource_handler, output, render_options);
    }
    let (base_dir, input) = read_input(filename)?;
    let body = replace_frontmatter(&input, render_options.frontmatter);
    let input = match render_options.tabs {
        Some(tab_width) => expand_tabs(&body, tab_width),
        None => Cow::Borrowed(body.as_ref()),
    };
    let input = input.as_ref();
    event!(
        Level::TRACE,
        "Read input, using {} as base directory",
        base_dir.display()
    );
    let options = markdown_options(render_options.smart_punctuation);
    let toc_events = if render_options.toc {
        let file_ref = (filename != "-")
            .then(|| {
                std::path::Path::new(filename)
                    .file_name()
                    .and_then(|s| s.to_str())
            })
            .flatten();
        toc::build_toc_events(input, options, file_ref)
    } else {
        Vec::new()
    };
    let parser = toc_events
        .into_iter()
        .chain(Parser::new_ext(input, options));
    let parser: Box<dyn Iterator<Item = pulldown_cmark::Event>> = if render_options.emoji {
        Box::new(substitute_emoji(parser))
    } else {
        Box::new(parser)
    };
    let env = Environment::for_local_directory(&base_dir)?;
    let sink = BufWriter::new(output.writer());
    render_events(
        parser,
        settings,
        &env,
        resource_handler,
        sink,
        render_options.margin,
    )
}

/// Process a single file like [`process_file`], but render each block of the input as soon as
/// it is read.
fn stream_file(
    filename: &str,
    settings: &Settings,
    resource_handler: &dyn ResourceUrlHandler,
    output: &mut Output,
    render_options: RenderOptions,
) -> Result<()> {
    let (base_dir, input) = open_input(filename)?;
    event!(
        Level::TRACE,
        "Streaming input, using {} as base directory",
        base_dir.display()
    );
    let options = markdown_options(render_options.smart_punctuation);
    let mut read_error = None;
    let events = stream::Blocks::new(input)
        // Stop at the first read error, but render all blocks before it.
        .map_while(|block| block.map_err(|error| read_error = Some(error)).ok())
        .enumerate()
        .flat_map(|(index, block)| {
            // Only the start of the input can hold frontmatter, and the first block always holds
            // all of it.
            let block = if index == 0 {
                replace_frontmatter(&block, render_options.frontmatter)
            } else {
                Cow::Borrowed(block.as_str())
            };
            let block = expand_tabs(&block, render_options.tabs.unwrap_or(0));
            let parser = Parser::new_ext(&block, options);
            let events: Box<dyn Iterator<Item = Event>> = if render_options.emoji {
                Box::new(substitute_emoji(parser))
            } else {
                Box::new(parser)
            };
            // Take ownership of the events, since the block goes away after this closure.
            events.map(Event::into_static).collect::<Vec<_>>()
        });
    let env = Environment::for_local_directory(&base_dir)?;
    // Write each line as soon as it is rendered.
    let sink = LineWriter::new(output.writer());
    render_events(
        events,
        settings,
        &env,
        resource_handler,
        sink,
        render_options.margin,
    )?;
    match read_error {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}

/// Render Markdown `events` to `sink`, with a two-space left margin if `margin` is true.
fn render_events<'e, W: Write>(
    events: impl Iterator<Item = Event<'e>>,
    settings: &Settings,
    env: &Environment,
    resource_handler: &dyn ResourceUrlHandler,
    mut sink: W,
    margin: bool,
) -> Result<()> {
    let ignore_broken_pipe = |error: io::Error| {
        if error.kind() == io::ErrorKind::BrokenPipe {
            event!(Level::TRACE, "Ignoring broken pipe");
            Ok(())
        } else {
            event!(Level::ERROR, ?error, "Failed to process file: {:#}", error);
            Err(error)
        }
    };

    writeln!(sink).or_else(ignore_broken_pipe)?;
    if margin {
        let mut padded = MarginWriter::new(&mut sink);
        pulldown_cmark_mdcat::push_tty(settings, env, resource_handler, &mut padded, events)
            .and_then(|_| {
                event!(Level::TRACE, "Finished rendering, flushing output");
                padded.flush()
            })
            .or_else(ignore_broken_pipe)?;
    } else {
        pulldown_cmark_mdcat::push_tty(settings, env, resource_handler, &mut sink, events)
            .or_else(ignore_broken_pipe)?;
    }
    writeln!(sink).or_else(ignore_broken_pipe)?;
    sink.flush().or_else(ignore_broken_pipe)?;
    Ok(())
}

/// Create the resource handler for mdcat.
pub fn create_resource_handler(access: ResourceAccess) -> Result<DispatchingResourceHandler> {
    let mut resource_handlers: Vec<Box<dyn ResourceUrlHandler>> = vec![Box::new(
        FileResourceHandler::new(DEFAULT_RESOURCE_READ_LIMIT),
    )];
    if let ResourceAccess::Remote = access {
        let user_agent = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
        event!(
            target: "mdcat::main",
            Level::DEBUG,
            "Remote resource access permitted, creating HTTP client with user agent {}",
            user_agent
        );
        let client = CurlResourceHandler::create(DEFAULT_RESOURCE_READ_LIMIT, user_agent)
            .with_context(|| "Failed to build HTTP client".to_string())?;
        resource_handlers.push(Box::new(client));
    }
    Ok(DispatchingResourceHandler::new(resource_handlers))
}
