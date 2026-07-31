// Copyright Mouhieddine Sabir <me@mouhieddine.dev>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Interactive fuzzy picker for Markdown files, backing the `mdpick` entry point.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use ignore::WalkBuilder;

/// Recursively collect Markdown files below `root`, honouring `.gitignore` and friends.
fn find_markdown_files(root: &Path) -> Vec<PathBuf> {
    WalkBuilder::new(root)
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
                && entry
                    .path()
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        })
        .map(|entry| entry.into_path())
        .collect()
}

/// Let the user fuzzy-pick a Markdown file below `root` with `fzf`.
///
/// Returns `Ok(None)` if the user cancelled the picker, e.g. with Esc or Ctrl-C.
pub fn pick_markdown_file(root: &Path) -> Result<Option<PathBuf>> {
    let mut files = find_markdown_files(root);
    if files.is_empty() {
        bail!("No Markdown files found below {}", root.display());
    }
    files.sort();

    let mut fzf = Command::new("fzf")
        .arg("--prompt=mdpick> ")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context(
            "Failed to run fzf; mdpick requires fzf to be installed, \
             see https://github.com/junegunn/fzf",
        )?;

    {
        let stdin = fzf
            .stdin
            .as_mut()
            .expect("fzf stdin is piped and available");
        for file in &files {
            writeln!(stdin, "{}", file.display())?;
        }
    }

    let output = fzf
        .wait_with_output()
        .context("Failed to read the file selected in fzf")?;
    // fzf exits non-zero both when the user cancels (Esc/Ctrl-C, status 130) and when it can't
    // find a match; either way there's nothing to render.
    if !output.status.success() {
        return Ok(None);
    }

    let selection = String::from_utf8(output.stdout)
        .context("fzf produced non-UTF-8 output")?
        .trim()
        .to_string();
    if selection.is_empty() {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(selection)))
}
