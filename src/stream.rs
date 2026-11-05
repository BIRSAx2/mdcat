// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Split Markdown input into blocks, to render each block as soon as it is read.

use std::collections::VecDeque;
use std::io::{BufRead, Error, Result};

use pulldown_cmark_mdcat::FrontmatterFormat;

/// A construct which blank lines do not end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fence {
    /// A fenced code block, opened with the given number of backticks or tildes.
    Code(char, usize),
    /// Frontmatter of the given format at the start of the input.
    Frontmatter(FrontmatterFormat),
    /// An HTML block of CommonMark types 1 to 5, e.g. `<pre>` or a comment, which only a line
    /// with the given end tag or marker closes.
    Html(&'static str),
}

/// The tags which open an HTML block of CommonMark type 1, and the end tags which close it.
const RAW_TAGS: [(&str, &str); 4] = [
    ("pre", "</pre>"),
    ("script", "</script>"),
    ("style", "</style>"),
    ("textarea", "</textarea>"),
];

/// The end tag or marker of the HTML block of CommonMark types 1 to 5 which `line` opens, if any.
///
/// Like pulldown-cmark, which mdcat parses Markdown with, and unlike CommonMark, only the end tag
/// in lower case which matches the start tag closes a block of type 1, e.g. neither `</PRE>` nor
/// `</script>` closes `<pre>`.
fn html_block_end(line: &str) -> Option<&'static str> {
    let markers = [("<!--", "-->"), ("<?", "?>"), ("<![CDATA[", "]]>")];
    if let Some(&(_, end)) = markers.iter().find(|(start, _)| line.starts_with(start)) {
        return Some(end);
    }
    if line.starts_with("<!") && line.as_bytes().get(2).is_some_and(u8::is_ascii_alphabetic) {
        return Some(">");
    }
    let name = line.strip_prefix('<')?;
    RAW_TAGS.iter().find_map(|&(tag, end)| {
        let rest = name
            .get(..tag.len())
            .filter(|start| start.eq_ignore_ascii_case(tag))
            .map(|_| &name[tag.len()..])?;
        matches!(rest.chars().next(), None | Some(' ' | '\t' | '>')).then_some(end)
    })
}

/// The fence which `line` opens, if it starts an HTML block of CommonMark types 1 to 5 which does
/// not end on the same line.
///
/// Like CommonMark, only accept up to three spaces of indentation, so that an indented code block
/// never opens a fence.  This misses HTML blocks inside list items with more indentation, and
/// after list item and block quote markers; this is safe, because a line which is not indented
/// after a blank line ends these containers, and thus the HTML blocks in them, too.
///
/// A start tag or marker inside an HTML block of type 6 or 7, e.g. `<pre>` inside `<div>`, still
/// opens a fence.  This only delays rendering, unless the end tag is missing or inside a later
/// fenced code block.
fn html_block(line: &str) -> Option<Fence> {
    let html = line.trim_start_matches(' ');
    if line.len() - html.len() > 3 {
        return None;
    }
    let end = html_block_end(html)?;
    (!line.contains(end)).then_some(Fence::Html(end))
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
            .map(|(c, length, _)| Fence::Code(c, length))
            .or_else(|| html_block(line)),
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
        Some(Fence::Html(end)) if line.contains(end) => None,
        Some(Fence::Html(_)) => fence,
    }
}

/// The maximum number of lines of frontmatter, including its delimiters.
///
/// If no closing delimiter follows the opening delimiter within this many lines, the input does
/// not start with frontmatter, but e.g. with a thematic break, and a block can end before the
/// whole input was read.
const FRONTMATTER_MAX_LINES: usize = 80;

/// Whether `line` is blank, i.e. contains only spaces and tabs.
fn is_blank(line: &str) -> bool {
    line.trim_matches([' ', '\t', '\r', '\n']).is_empty()
}

/// An iterator over the blocks of Markdown text read from a reader.
///
/// A block ends at a blank line which is followed by a line that is not indented, since such a
/// line normally starts a new top-level Markdown block, whereas indented lines continue the
/// block before, e.g. with another paragraph of a list item.  Blank lines inside fenced code
/// blocks, inside HTML blocks which only an end tag or marker closes, e.g. `<pre>` or comments,
/// and inside frontmatter at the start of the input never end a block.
///
/// An opening frontmatter delimiter which a blank line follows, or which no closing delimiter
/// follows within [`FRONTMATTER_MAX_LINES`] lines, does not open frontmatter, but e.g. a thematic
/// break, even if a closing delimiter follows later.
///
/// Parsed on its own, each block renders almost the same as within the whole input: reference
/// links and footnotes only resolve against definitions in the same block, and list items which
/// blank lines separate become separate lists.
pub struct Blocks<R> {
    reader: R,
    /// Lines to process before reading more lines: the first line of the next block, which was
    /// read to find the end of the current block, or the lines of frontmatter which did not close.
    pending: VecDeque<String>,
    /// The error from reading the line after the pending lines.
    error: Option<Error>,
    /// Whether no line was read yet, i.e. whether frontmatter can begin.
    at_start: bool,
}

impl<R: BufRead> Blocks<R> {
    /// Read blocks from `reader`.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            pending: VecDeque::new(),
            error: None,
            at_start: true,
        }
    }

    /// The next pending line, or else the next line read, which is empty at the end of the input.
    fn next_line(&mut self) -> Result<String> {
        if let Some(line) = self.pending.pop_front() {
            return Ok(line);
        }
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        Ok(line)
    }
}

impl<R: BufRead> Iterator for Blocks<R> {
    type Item = Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut block = String::new();
        let mut lines = 0;
        let mut fence = None;
        let mut after_blank_line = false;
        loop {
            let line = match self.next_line() {
                Ok(line) => line,
                // Return what was read before the error first.
                Err(error) if !block.is_empty() => {
                    self.error = Some(error);
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
                self.pending.push_front(line);
                return Some(Ok(block));
            }
            fence = next_fence(fence, &line, self.at_start);
            self.at_start = false;
            after_blank_line = is_blank(&line);
            block.push_str(&line);
            lines += 1;
            if matches!(fence, Some(Fence::Frontmatter(_)))
                && ((lines == 2 && after_blank_line) || lines == FRONTMATTER_MAX_LINES)
            {
                // This is no frontmatter, so split its lines into blocks like any other lines.
                // Since `at_start` is false now, the opening delimiter does not open frontmatter
                // again.
                for line in block.split_inclusive('\n').rev() {
                    self.pending.push_front(line.to_owned());
                }
                block.clear();
                lines = 0;
                fence = None;
                after_blank_line = false;
            }
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
    fn html_block_continues_after_blank_line() {
        assert_eq!(
            blocks("<pre>x\n\ny</pre>\n\nText\n"),
            ["<pre>x\n\ny</pre>\n\n", "Text\n"]
        );
        let html = [
            "<!--\nhidden\n\nstill hidden\n-->\n\n",
            "<?php\n\n?>\n\n",
            "<!DOCTYPE\n\nhtml>\n\n",
            "<![CDATA[\n\n]]>\n\n",
            "<script type=\"x\">\n\n</script>\n\n",
            "<style>\n\n</style>\n\n",
            "<textarea\n\n</textarea>\n\n",
        ];
        for html in html {
            assert_eq!(blocks(&format!("{html}Text\n")), [html, "Text\n"]);
        }
    }

    #[test]
    fn only_matching_lower_case_end_tag_closes_html_block() {
        // pulldown-cmark ignores the case of the start tag, but not of the end tag, and only
        // accepts the end tag which matches the start tag.
        let html = "<PRE>\na\n\n</PRE>\n\n</Pre>\n\n</script>\n\nb\n</pre>\n\n";
        assert_eq!(blocks(&format!("{html}Text\n")), [html, "Text\n"]);
    }

    #[test]
    fn html_block_can_end_on_its_first_line() {
        assert_eq!(
            blocks("<pre>x</pre>\n\n<!-- x -->\n\n<!-->\n\nText\n"),
            ["<pre>x</pre>\n\n", "<!-- x -->\n\n", "<!-->\n\n", "Text\n"]
        );
    }

    #[test]
    fn html_block_interrupts_paragraph() {
        let html = "Text\n<!--\na\n\nb\n-->\n\n";
        assert_eq!(blocks(&format!("{html}Text\n")), [html, "Text\n"]);
    }

    #[test]
    fn only_raw_tags_open_html_block() {
        // A tag which only starts with a raw tag name, and HTML blocks of types 6 and 7, end at a
        // blank line.
        assert_eq!(
            blocks("<prelude>\n\n<div>\na\n\nb\n</div>\n"),
            ["<prelude>\n\n", "<div>\na\n\n", "b\n</div>\n"]
        );
    }

    #[test]
    fn indented_code_does_not_open_html_block() {
        assert_eq!(
            blocks("    <pre>\n\nText\n\n\t<!--\n\nText\n"),
            ["    <pre>\n\n", "Text\n\n\t<!--\n\n", "Text\n"]
        );
        let html = "   <pre>\n\n</pre>\n\n";
        assert_eq!(blocks(&format!("{html}Text\n")), [html, "Text\n"]);
    }

    #[test]
    fn code_block_does_not_open_html_block() {
        assert_eq!(
            blocks("```\n<pre>\n```\n\nText\n"),
            ["```\n<pre>\n```\n\n", "Text\n"]
        );
    }

    #[test]
    fn html_block_does_not_open_code_block() {
        assert_eq!(
            blocks("<!--\n```\n-->\n\nText\n"),
            ["<!--\n```\n-->\n\n", "Text\n"]
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
    fn blank_line_after_opening_delimiter_ends_frontmatter() {
        assert_eq!(
            blocks("---\n\nText\n\nMore\n"),
            ["---\n\n", "Text\n\n", "More\n"]
        );
    }

    #[test]
    fn frontmatter_has_at_most_80_lines() {
        let body = "a: 1\n\n".repeat(39);
        // The closing delimiter on line 80 closes frontmatter.
        let frontmatter = format!("---\n{body}---\n\n");
        assert_eq!(
            blocks(&format!("{frontmatter}Text\n")),
            [frontmatter.as_str(), "Text\n"]
        );
        // The closing delimiter on line 81 does not, so the lines split into blocks.
        let mut expected = vec!["---\na: 1\n\n"];
        expected.extend(["a: 1\n\n"; 38]);
        expected.push("b: 2\n---\n");
        assert_eq!(blocks(&format!("---\n{body}b: 2\n---\n")), expected);
    }

    #[test]
    fn unclosed_frontmatter_renders_before_input_ends() {
        let mut input = format!("---\n{}", "x\n\n".repeat(40)).into_bytes();
        input.extend(b"Invalid \xff\n");
        let mut blocks = Blocks::new(&input[..]);
        assert_eq!(blocks.next().unwrap().unwrap(), "---\nx\n\n");
    }

    #[test]
    fn fences_continue_after_frontmatter_ends() {
        assert_eq!(
            blocks("---\n\n```\na\n\nb\n```\n\nText\n"),
            ["---\n\n", "```\na\n\nb\n```\n\n", "Text\n"]
        );
        // A code block from line 2 to beyond line 80.
        let code = format!("---\n```\n{}d\n```\n\n", "c\n\n".repeat(39));
        assert_eq!(blocks(&format!("{code}Text\n")), [code.as_str(), "Text\n"]);
    }

    #[test]
    fn frontmatter_only_opens_on_first_line() {
        assert_eq!(
            blocks("Text\n\n---\na\n\nb\n---\n"),
            ["Text\n\n", "---\na\n\n", "b\n---\n"]
        );
    }
}
