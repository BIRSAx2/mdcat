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
use pulldown_cmark::{Options, Parser};
use pulldown_cmark_mdcat::resources::{
    DispatchingResourceHandler, FileResourceHandler, ResourceUrlHandler,
};
use pulldown_cmark_mdcat::{Environment, Settings};
use resources::CurlResourceHandler;
use tracing::{event, instrument, Level};

use args::ResourceAccess;
use output::Output;

/// Argument parsing for mdcat.
#[allow(missing_docs)]
pub mod args;
/// Output handling for mdcat.
pub mod output;
/// Resource handling for mdca.
pub mod resources;

/// Default read size limit for resources.
pub static DEFAULT_RESOURCE_READ_LIMIT: u64 = 104_857_600;

/// A writer that prepends two spaces of left margin after every newline.
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

/// Strip YAML/TOML frontmatter from the beginning of a markdown document.
///
/// Frontmatter is a `---` block at the very start of the file, closed by
/// another `---` or `...` line.  If no valid frontmatter block is found the
/// input is returned unchanged.
fn strip_frontmatter(input: &str) -> &str {
    let after_open = match input
        .strip_prefix("---\n")
        .or_else(|| input.strip_prefix("---\r\n"))
    {
        Some(s) => s,
        None => return input,
    };

    let mut start = 0;
    while start < after_open.len() {
        let end = after_open[start..]
            .find('\n')
            .map_or(after_open.len(), |i| start + i);
        let line = after_open[start..end].trim_end_matches('\r');
        let next = (end + 1).min(after_open.len());
        if line == "---" || line == "..." {
            return &after_open[next..];
        }
        start = end + 1;
    }

    input
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

/// Process a single file.
///
/// Read from `filename` and render the contents to `output`.
#[instrument(skip(output, settings, resource_handler), level = "debug")]
pub fn process_file(
    filename: &str,
    settings: &Settings,
    resource_handler: &dyn ResourceUrlHandler,
    output: &mut Output,
) -> Result<()> {
    let (base_dir, input) = read_input(filename)?;
    let input = strip_frontmatter(&input);
    event!(
        Level::TRACE,
        "Read input, using {} as base directory",
        base_dir.display()
    );
    let parser = Parser::new_ext(
        input,
        Options::ENABLE_TASKLISTS
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TABLES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_MATH
            | Options::ENABLE_GFM,
    );
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
    {
        let mut padded = MarginWriter::new(&mut sink);
        pulldown_cmark_mdcat::push_tty(settings, &env, resource_handler, &mut padded, parser)
            .and_then(|_| {
                event!(Level::TRACE, "Finished rendering, flushing output");
                padded.flush()
            })
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
