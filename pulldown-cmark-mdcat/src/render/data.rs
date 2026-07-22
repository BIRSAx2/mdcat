// Copyright 2020 Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use anstyle::Style;
use pulldown_cmark::{Alignment, CowStr, LinkType};

/// A pending link.
#[derive(Debug, PartialEq)]
pub struct PendingLink<'a> {
    /// The type of this link.
    pub(crate) link_type: LinkType,
    /// The destination URL of this link.
    pub(crate) dest_url: CowStr<'a>,
    /// The link title as it appeared in Markdown.
    pub(crate) title: CowStr<'a>,
}

/// The definition of a reference link, i.e. a numeric index for a link.
#[derive(Debug, PartialEq)]
pub struct LinkReferenceDefinition<'a> {
    /// The reference index of this link.
    pub(crate) index: u16,
    /// The link target as it appeared in Markdown.
    pub(crate) target: CowStr<'a>,
    /// The link title as it appeared in Markdown.
    pub(crate) title: CowStr<'a>,
    /// The style to use for the link.
    pub(crate) style: Style,
}

/// The state of the current line for render.md.wrapping.
#[derive(Debug)]
pub struct CurrentLine {
    /// The line length
    pub(super) length: u16,
    /// Trailing space to add before continuing this line.
    pub(super) trailing_space: Option<String>,
}

impl CurrentLine {
    /// An empty current line
    pub(super) fn empty() -> Self {
        Self {
            length: 0,
            trailing_space: None,
        }
    }
}

/// A cell in the table.
///
/// A cell consists of one or more explicit lines, split apart by `<br>`
/// tags; each line holds a sequence of styled text fragments which are
/// later word-wrapped to fit the column width.
#[derive(Debug)]
pub struct TableCell<'a> {
    /// Explicit lines of the cell, as introduced by `<br>`.
    pub(super) lines: Vec<Vec<(Style, CowStr<'a>)>>,
}

impl TableCell<'_> {
    /// A new empty table cell, with a single empty line.
    pub(super) fn empty() -> Self {
        Self {
            lines: vec![Vec::new()],
        }
    }

    /// Display width of the cell's text content (no ANSI sequences), i.e.
    /// the width of its widest explicit line.
    pub(super) fn text_width(&self) -> usize {
        self.lines
            .iter()
            .map(|line| {
                line.iter()
                    .map(|(_, t)| textwrap::core::display_width(t.as_ref()))
                    .sum()
            })
            .max()
            .unwrap_or(0)
    }
}

/// A row in the table.
#[derive(Debug)]
pub struct TableRow<'a> {
    /// Completed cells of the table row.
    pub(super) cells: Vec<TableCell<'a>>,
    /// Current incomplete cell of the table row.
    pub(super) current_cell: TableCell<'a>,
}

impl TableRow<'_> {
    /// A new empty table row.
    pub(super) fn empty() -> Self {
        Self {
            cells: Vec::new(),
            current_cell: TableCell::empty(),
        }
    }
}

/// The state of the current table.
#[derive(Debug)]
pub struct CurrentTable<'a> {
    /// Head row of the table.
    pub(super) head: Option<TableRow<'a>>,
    /// Complete rows of the table.
    pub(super) rows: Vec<TableRow<'a>>,
    /// Current incomplete row of the table.
    pub(super) current_row: TableRow<'a>,
    /// Alignments of columns.
    pub(super) alignments: Vec<Alignment>,
    /// Whether we are currently collecting the head row.
    pub(super) is_head: bool,
    /// Nesting depth of `Strong` spans.
    pub(super) strong_depth: u32,
    /// Nesting depth of `Emphasis` spans.
    pub(super) emphasis_depth: u32,
    /// Nesting depth of `Strikethrough` spans.
    pub(super) strikethrough_depth: u32,
    /// Override style for link/image/code spans; replaces the normal text style.
    pub(super) span_style: Option<Style>,
}

impl<'a> CurrentTable<'a> {
    /// A new empty table.
    pub(super) fn empty() -> Self {
        Self {
            head: None,
            rows: Vec::new(),
            current_row: TableRow::empty(),
            alignments: Vec::new(),
            is_head: true,
            strong_depth: 0,
            emphasis_depth: 0,
            strikethrough_depth: 0,
            span_style: None,
        }
    }

    /// Compute the effective style for a text fragment given a base style.
    fn effective_style(&self, base: Style) -> Style {
        let mut style = self.span_style.unwrap_or(base);
        if self.strong_depth > 0 || self.is_head {
            style = style.bold();
        }
        if self.emphasis_depth > 0 {
            style = style.italic();
        }
        if self.strikethrough_depth > 0 {
            style = style.strikethrough();
        }
        style
    }

    /// Push a plain text fragment using the current inline markup state.
    pub(super) fn push_text(mut self, text: CowStr<'a>) -> Self {
        let style = self.effective_style(Style::new());
        self.current_line_mut().push((style, text));
        self
    }

    /// Push a text fragment with a specific base style (e.g. code or inline HTML).
    pub(super) fn push_styled_text(mut self, text: CowStr<'a>, base: Style) -> Self {
        let style = self.effective_style(base);
        self.current_line_mut().push((style, text));
        self
    }

    /// Start a new explicit line within the current cell, as introduced by `<br>`.
    pub(super) fn push_break(mut self) -> Self {
        self.current_row.current_cell.lines.push(Vec::new());
        self
    }

    fn current_line_mut(&mut self) -> &mut Vec<(Style, CowStr<'a>)> {
        self.current_row
            .current_cell
            .lines
            .last_mut()
            .expect("a table cell always has at least one line")
    }

    pub(super) fn enter_strong(mut self) -> Self {
        self.strong_depth += 1;
        self
    }

    pub(super) fn exit_strong(mut self) -> Self {
        self.strong_depth = self.strong_depth.saturating_sub(1);
        self
    }

    pub(super) fn enter_emphasis(mut self) -> Self {
        self.emphasis_depth += 1;
        self
    }

    pub(super) fn exit_emphasis(mut self) -> Self {
        self.emphasis_depth = self.emphasis_depth.saturating_sub(1);
        self
    }

    pub(super) fn enter_strikethrough(mut self) -> Self {
        self.strikethrough_depth += 1;
        self
    }

    pub(super) fn exit_strikethrough(mut self) -> Self {
        self.strikethrough_depth = self.strikethrough_depth.saturating_sub(1);
        self
    }

    /// Enter a link or image span with a given style.
    pub(super) fn enter_span(mut self, style: Style) -> Self {
        self.span_style = Some(style);
        self
    }

    /// Exit a link or image span.
    pub(super) fn exit_span(mut self) -> Self {
        self.span_style = None;
        self
    }

    /// Complete the current cell and start a new cell in the current row.
    pub(super) fn end_cell(mut self) -> Self {
        self.current_row.cells.push(self.current_row.current_cell);
        self.current_row.current_cell = TableCell::empty();
        self
    }

    /// Complete the head row and start a new row.
    pub(super) fn end_head(mut self) -> Self {
        self.head = Some(self.current_row);
        self.current_row = TableRow::empty();
        self.is_head = false;
        self
    }

    /// Complete the current row and start a new row.
    pub(super) fn end_row(mut self) -> Self {
        self.rows.push(self.current_row);
        self.current_row = TableRow::empty();
        self
    }
}

/// Data associated with rendering state.
///
/// Unlike state attributes state data represents cross-cutting
/// concerns which are manipulated across all states.
#[derive(Debug)]
pub struct StateData<'a> {
    /// A list of pending links.
    ///
    /// These are links which we still need to create a reference number for.
    pub(super) pending_links: Vec<PendingLink<'a>>,
    /// A list of pending reference link definitions.
    ///
    /// These are links which mdcat already created a reference number for
    /// but didn't yet write out.
    pub(super) pending_link_definitions: Vec<LinkReferenceDefinition<'a>>,
    /// The reference number for the next link.
    pub(super) next_link: u16,
    /// The state of the current line for render.md.wrapping.
    pub(super) current_line: CurrentLine,
    /// The state of the current table.
    pub(super) current_table: CurrentTable<'a>,
    /// Map from footnote label to numeric index (order of first reference).
    pub(super) footnote_indices: HashMap<String, u16>,
    /// Counter for footnote numbering.
    pub(super) next_footnote: u16,
    /// Collected footnote definitions: (index, body text).
    pub(super) footnote_definitions: Vec<(u16, String)>,
    /// Buffer for the footnote definition currently being collected.
    pub(super) current_footnote: Option<(u16, String)>,
}

impl<'a> StateData<'a> {
    pub(crate) fn current_line(self, current_line: CurrentLine) -> Self {
        Self {
            current_line,
            ..self
        }
    }

    /// Push a pending link.
    pub(crate) fn push_pending_link(
        mut self,
        link_type: LinkType,
        dest_url: CowStr<'a>,
        title: CowStr<'a>,
    ) -> Self {
        self.pending_links.push(PendingLink {
            link_type,
            dest_url,
            title,
        });
        self
    }

    /// Pop a pending link.
    ///
    /// Panics if there is no pending link.
    pub(crate) fn pop_pending_link(mut self) -> (Self, PendingLink<'a>) {
        let link = self.pending_links.pop().unwrap();
        (self, link)
    }

    /// Add a pending link to the state data.
    ///
    /// `target` is the link target, and `title` the link title to show after the URL.
    /// `colour` is the colour to use for foreground text to differentiate between
    /// different types of links.
    pub(crate) fn add_link_reference(
        mut self,
        target: CowStr<'a>,
        title: CowStr<'a>,
        style: Style,
    ) -> (Self, u16) {
        let index = self.next_link;
        self.next_link += 1;
        self.pending_link_definitions.push(LinkReferenceDefinition {
            index,
            target,
            title,
            style,
        });
        (self, index)
    }

    pub(crate) fn take_link_references(self) -> (Self, Vec<LinkReferenceDefinition<'a>>) {
        let links = self.pending_link_definitions;
        (
            StateData {
                pending_link_definitions: Vec::new(),
                ..self
            },
            links,
        )
    }
}

impl<'a> StateData<'a> {
    pub(super) fn footnote_index(&mut self, label: &str) -> u16 {
        if let Some(&idx) = self.footnote_indices.get(label) {
            return idx;
        }
        let idx = self.next_footnote;
        self.next_footnote += 1;
        self.footnote_indices.insert(label.to_owned(), idx);
        idx
    }

    pub(super) fn start_footnote_definition(&mut self, label: &str) {
        let idx = self.footnote_index(label);
        self.current_footnote = Some((idx, String::new()));
    }

    pub(super) fn append_footnote_text(&mut self, text: &str) {
        if let Some((_, ref mut body)) = self.current_footnote {
            body.push_str(text);
        }
    }

    pub(super) fn end_footnote_definition(&mut self) {
        if let Some((idx, body)) = self.current_footnote.take() {
            self.footnote_definitions.push((idx, body));
        }
    }
}

impl Default for StateData<'_> {
    fn default() -> Self {
        StateData {
            pending_links: Vec::new(),
            pending_link_definitions: Vec::new(),
            next_link: 1,
            current_line: CurrentLine::empty(),
            current_table: CurrentTable::empty(),
            footnote_indices: HashMap::new(),
            next_footnote: 1,
            footnote_definitions: Vec::new(),
            current_footnote: None,
        }
    }
}
