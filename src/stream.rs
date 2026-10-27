// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Split Markdown input into blocks, to render each block as soon as it is read.

use std::io::{BufRead, Result};

use pulldown_cmark_mdcat::FrontmatterFormat;

/// A construct which blank lines do not end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fence {
    /// A fenced code block, opened with the given number of backticks or tildes.
    Code(char, usize),
    /// Frontmatter of the given format at the start of the input.
    Frontmatter(FrontmatterFormat),
}

/// The fence character, the fence length, and the info string, if `line` is a code fence.
fn code_fence(line: &str) -> Option<(char, usize, &str)> {
    let line = line.trim_start_matches([' ', '\t']);
    let c = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let info = line.trim_start_matches(c);
    let length = line.len() - info.len();
    (length >= 3).then_some((c, length, info.trim()))
}

/// The fence open after `line`, given the `fence` open before it.
///
/// Frontmatter can only open on the first line of the input, i.e. if `at_start` is true.  Like
/// [`pulldown_cmark_mdcat::split_frontmatter`], ignore a byte order mark at the start of the input
/// and trailing whitespace on frontmatter delimiters, so that the first block holds all of the
/// frontmatter which `split_frontmatter` finds.
fn next_fence(fence: Option<Fence>, line: &str, at_start: bool) -> Option<Fence> {
    let line = if at_start {
        line.strip_prefix('\u{feff}').unwrap_or(line)
    } else {
        line
    };
    let line = line.trim_end_matches(['\r', '\n']);
    let delimiter = line.trim_end();
    match fence {
        None if at_start && delimiter == "---" => Some(Fence::Frontmatter(FrontmatterFormat::Yaml)),
        None if at_start && delimiter == "+++" => Some(Fence::Frontmatter(FrontmatterFormat::Toml)),
        // The info string of a backtick fence cannot contain backticks.
        None => code_fence(line)
            .filter(|&(c, _, info)| c == '~' || !info.contains('`'))
            .map(|(c, length, _)| Fence::Code(c, length)),
        Some(Fence::Code(c, length)) => match code_fence(line) {
            Some((closing, closing_length, "")) if closing == c && closing_length >= length => None,
            _ => fence,
        },
        Some(Fence::Frontmatter(FrontmatterFormat::Yaml))
            if delimiter == "---" || delimiter == "..." =>
        {
            None
        }
        Some(Fence::Frontmatter(FrontmatterFormat::Toml)) if delimiter == "+++" => None,
        Some(Fence::Frontmatter(_)) => fence,
    }
}

/// Whether `line` is blank, i.e. contains only spaces and tabs.
fn is_blank(line: &str) -> bool {
    line.trim_matches([' ', '\t', '\r', '\n']).is_empty()
}

/// An iterator over the blocks of Markdown text read from a reader.
///
/// A block ends at a blank line which is followed by a line that is not indented, since such a
/// line normally starts a new top-level Markdown block, whereas indented lines continue the
/// block before, e.g. with another paragraph of a list item.  Blank lines inside fenced code
/// blocks and inside frontmatter at the start of the input never end a block.
///
/// Parsed on its own, each block renders almost the same as within the whole input: reference
/// links and footnotes only resolve against definitions in the same block, and list items which
/// blank lines separate become separate lists.
pub struct Blocks<R> {
    reader: R,
    /// The first line of the next block, or the error from reading it, which was read to find
    /// the end of the current block.
    next_line: Option<Result<String>>,
    /// Whether no line was read yet, i.e. whether frontmatter can begin.
    at_start: bool,
}

impl<R: BufRead> Blocks<R> {
    /// Read blocks from `reader`.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            next_line: None,
            at_start: true,
        }
    }

    /// Read the next line, which is empty at the end of the input.
    fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        Ok(line)
    }
}

impl<R: BufRead> Iterator for Blocks<R> {
    type Item = Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut block = String::new();
        let mut fence = None;
        let mut after_blank_line = false;
        loop {
            let line = match self.next_line.take().unwrap_or_else(|| self.read_line()) {
                Ok(line) => line,
                // Return what was read before the error first.
                Err(error) if !block.is_empty() => {
                    self.next_line = Some(Err(error));
                    return Some(Ok(block));
                }
                Err(error) => return Some(Err(error)),
            };
            if line.is_empty() {
                return (!block.is_empty()).then_some(Ok(block));
            }
            if after_blank_line
                && fence.is_none()
                && !is_blank(&line)
                && !line.starts_with([' ', '\t'])
            {
                self.next_line = Some(Ok(line));
                return Some(Ok(block));
            }
            fence = next_fence(fence, &line, self.at_start);
            self.at_start = false;
            after_blank_line = is_blank(&line);
            block.push_str(&line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Blocks;

    fn blocks(input: &str) -> Vec<String> {
        Blocks::new(input.as_bytes())
            .collect::<std::io::Result<_>>()
            .unwrap()
    }

    #[test]
    fn empty_input_has_no_blocks() {
        assert!(blocks("").is_empty());
    }

    #[test]
    fn blank_line_ends_block() {
        assert_eq!(
            blocks("# Title\n\nSome text\nmore text\n\n\n- item\n"),
            ["# Title\n\n", "Some text\nmore text\n\n\n", "- item\n"]
        );
    }

    #[test]
    fn end_of_input_ends_last_block() {
        assert_eq!(blocks("a\n\nb"), ["a\n\n", "b"]);
    }

    #[test]
    fn read_error_follows_block_before_it() {
        let mut blocks = Blocks::new(&b"Text\n\nInvalid \xff\n"[..]);
        assert_eq!(blocks.next().unwrap().unwrap(), "Text\n\n");
        assert!(blocks.next().unwrap().is_err());
    }

    #[test]
    fn indented_line_continues_block() {
        assert_eq!(
            blocks("- item\n\n  more of the item\n\n      code\n\nText\n"),
            ["- item\n\n  more of the item\n\n      code\n\n", "Text\n"]
        );
    }

    #[test]
    fn fenced_code_block_continues_after_blank_line() {
        assert_eq!(
            blocks("```rust\na\n\nb\n```\n\n~~~\nc\n\nd\n~~~\n\nText\n"),
            ["```rust\na\n\nb\n```\n\n", "~~~\nc\n\nd\n~~~\n\n", "Text\n"]
        );
    }

    #[test]
    fn only_matching_fence_closes_code_block() {
        // A shorter fence, a fence with other characters, and a fence with an info string are
        // all part of the code.
        let code = "````markdown\n```\n\n~~~~\n\n```rust\n\n````\n\n";
        assert_eq!(blocks(&format!("{code}Text\n")), [code, "Text\n"]);
    }

    #[test]
    fn backticks_in_info_string_do_not_open_code_block() {
        assert_eq!(
            blocks("```inline``` code\n\nText\n"),
            ["```inline``` code\n\n", "Text\n"]
        );
    }

    #[test]
    fn frontmatter_continues_after_blank_line() {
        assert_eq!(
            blocks("---\ntitle: x\n\ntags: y\n...\n\nText\n"),
            ["---\ntitle: x\n\ntags: y\n...\n\n", "Text\n"]
        );
        assert_eq!(
            blocks("+++\ntitle = 'x'\n\ntags = 'y'\n+++\n\nText\n"),
            ["+++\ntitle = 'x'\n\ntags = 'y'\n+++\n\n", "Text\n"]
        );
    }

    #[test]
    fn only_matching_delimiter_closes_frontmatter() {
        let toml = "+++\na = 1\n---\n\n...\n\nb = 2\n+++\n\n";
        assert_eq!(blocks(&format!("{toml}Text\n")), [toml, "Text\n"]);
        let yaml = "---\na: 1\n+++\n\nb: 2\n...\n\n";
        assert_eq!(blocks(&format!("{yaml}Text\n")), [yaml, "Text\n"]);
    }

    #[test]
    fn frontmatter_ignores_bom_and_trailing_whitespace() {
        let frontmatter = "\u{feff}--- \r\na: 1\r\n\r\nb: 2\r\n---\t\r\n\r\n";
        assert_eq!(
            blocks(&format!("{frontmatter}Text\r\n")),
            [frontmatter, "Text\r\n"]
        );
    }

    #[test]
    fn frontmatter_only_opens_on_first_line() {
        assert_eq!(
            blocks("Text\n\n---\na\n\nb\n---\n"),
            ["Text\n\n", "---\na\n\n", "b\n---\n"]
        );
    }
}
