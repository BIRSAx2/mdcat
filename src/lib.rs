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

use std::fs::File;
use std::io::stdin;
use std::io::{self, prelude::*, BufWriter};
use std::path::PathBuf;

use anyhow::{Context, Result};
use pulldown_cmark::Parser;
use pulldown_cmark_mdcat::resources::{
    DispatchingResourceHandler, FileResourceHandler, ResourceUrlHandler,
};
use pulldown_cmark_mdcat::{
    expand_tabs, markdown_options, strip_frontmatter, Environment, Settings,
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
    let cd = std::env::current_dir()?;
    let mut buffer = String::new();

    if filename.as_ref() == "-" {
        stdin().read_to_string(&mut buffer)?;
        Ok((cd, buffer))
    } else {
        let mut source = File::open(filename.as_ref())?;
        source.read_to_string(&mut buffer)?;
        let base_dir = cd
            .join(filename.as_ref())
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(cd);
        Ok((base_dir, buffer))
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
    /// Prepend a table of contents generated from the document's headings.
    ///
    /// On standard input (`filename` is `-`) its entries are plain text, since there is no file
    /// to link to; otherwise they link to `filename#slug`.
    pub toc: bool,
    /// If `Some`, expand literal tabs in the input to spaces using that tab stop width before
    /// parsing (see [`pulldown_cmark_mdcat::expand_tabs`]).
    pub tabs: Option<u16>,
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
    let (base_dir, input) = read_input(filename)?;
    let input = strip_frontmatter(&input);
    let input = match render_options.tabs {
        Some(tab_width) => expand_tabs(input, tab_width),
        None => std::borrow::Cow::Borrowed(input),
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
    let env = Environment::for_local_directory(&base_dir)?;

    let ignore_broken_pipe = |error: io::Error| {
        if error.kind() == io::ErrorKind::BrokenPipe {
            event!(Level::TRACE, "Ignoring broken pipe");
            Ok(())
        } else {
            event!(Level::ERROR, ?error, "Failed to process file: {:#}", error);
            Err(error)
        }
    };

    let mut sink = BufWriter::new(output.writer());
    writeln!(sink).or_else(&ignore_broken_pipe)?;
    if render_options.margin {
        let mut padded = MarginWriter::new(&mut sink);
        pulldown_cmark_mdcat::push_tty(settings, &env, resource_handler, &mut padded, parser)
            .and_then(|_| {
                event!(Level::TRACE, "Finished rendering, flushing output");
                padded.flush()
            })
            .or_else(&ignore_broken_pipe)?;
    } else {
        pulldown_cmark_mdcat::push_tty(settings, &env, resource_handler, &mut sink, parser)
            .or_else(&ignore_broken_pipe)?;
    }
    writeln!(sink).or_else(&ignore_broken_pipe)?;
    sink.flush().or_else(&ignore_broken_pipe)?;
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
