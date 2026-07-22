// Copyright 2020 Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::cmp::{max, min};
use std::io::{Result, Write};
use std::iter::zip;

use anstyle::Style;
use pulldown_cmark::{Alignment, CodeBlockKind, CowStr, HeadingLevel};
use syntect::highlighting::HighlightState;
use syntect::parsing::{ParseState, ScopeStack};
use textwrap::core::{display_width, Word};
use textwrap::WordSeparator;

use crate::references::*;
use crate::render::data::{CurrentLine, CurrentTable, LinkReferenceDefinition, TableCell};
use crate::render::highlighting::highlighter;
use crate::render::state::*;
use crate::terminal::capabilities::{MarkCapability, StyleCapability, TerminalCapabilities};
use crate::terminal::osc::{clear_link, set_link_url};
use crate::terminal::TerminalSize;
use crate::theme::CombineStyle;
use crate::Theme;
use crate::{Environment, Settings};

pub fn write_indent<W: Write>(writer: &mut W, level: u16) -> Result<()> {
    write!(writer, "{}", " ".repeat(level as usize))
}

pub fn write_styled<W: Write, S: AsRef<str>>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    style: &Style,
    text: S,
) -> Result<()> {
    match capabilities.style {
        None => write!(writer, "{}", text.as_ref()),
        Some(StyleCapability::Ansi) => write!(
            writer,
            "{}{}{}",
            style.render(),
            text.as_ref(),
            style.render_reset()
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn write_remaining_lines<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    style: &Style,
    indent: u16,
    mut buffer: String,
    next_lines: &[&[Word]],
    last_line: &[Word],
    line_prefix: &str,
) -> Result<CurrentLine> {
    // Finish the previous line
    writeln!(writer)?;
    write_indent(writer, indent)?;
    write!(writer, "{}", line_prefix)?;
    // Now write all lines up to the last
    for line in next_lines {
        match line.split_last() {
            None => {
                // The line was empty, so there's nothing to do anymore.
            }
            Some((last, heads)) => {
                for word in heads {
                    buffer.push_str(word.word);
                    buffer.push_str(word.whitespace);
                }
                buffer.push_str(last.word);
                write_styled(writer, capabilities, style, &buffer)?;
                writeln!(writer)?;
                write_indent(writer, indent)?;
                write!(writer, "{}", line_prefix)?;
                buffer.clear();
            }
        };
    }

    // Now write the last line and keep track of its width
    match last_line.split_last() {
        None => {
            // The line was empty, so there's nothing to do anymore.
            Ok(CurrentLine::empty())
        }
        Some((last, heads)) => {
            for word in heads {
                buffer.push_str(word.word);
                buffer.push_str(word.whitespace);
            }
            buffer.push_str(last.word);
            write_styled(writer, capabilities, style, &buffer)?;
            Ok(CurrentLine {
                length: textwrap::core::display_width(&buffer) as u16,
                trailing_space: Some(last.whitespace.to_owned()),
            })
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn write_styled_and_wrapped<W: Write, S: AsRef<str>>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    style: &Style,
    max_width: u16,
    indent: u16,
    current_line: CurrentLine,
    text: S,
    line_prefix: &str,
    prefix_cols: u16,
) -> Result<CurrentLine> {
    let max_width = max_width.saturating_sub(prefix_cols);
    let words = WordSeparator::UnicodeBreakProperties
        .find_words(text.as_ref())
        .collect::<Vec<_>>();
    match words.first() {
        // There were no words in the text so we just do nothing.
        None => Ok(current_line),
        Some(first_word) => {
            let current_width = current_line.length
                + indent
                + current_line
                    .trailing_space
                    .as_ref()
                    .map_or(0, |s| display_width(s.as_ref()) as u16);

            // If the current line is not empty and we can't even add the first first word of the text to it
            // then lets finish the line and start over.  If the current line is empty the word simply doesn't
            // fit into the terminal size so we must print it anyway.
            if 0 < current_line.length
                && max_width < current_width + display_width(first_word) as u16
            {
                writeln!(writer)?;
                write_indent(writer, indent)?;
                write!(writer, "{}", line_prefix)?;
                return write_styled_and_wrapped(
                    writer,
                    capabilities,
                    style,
                    max_width + prefix_cols,
                    indent,
                    CurrentLine::empty(),
                    text,
                    line_prefix,
                    prefix_cols,
                );
            }

            let widths = [
                // For the first line we need to subtract the length of the current line, and
                // the trailing space we need to add if we add more words to this line
                (max_width - current_width.min(max_width)) as f64,
                // For remaining lines we only need to account for the indent
                max_width.saturating_sub(indent) as f64,
            ];
            let lines = textwrap::wrap_algorithms::wrap_first_fit(&words, &widths);
            match lines.split_first() {
                None => {
                    // there was nothing to wrap so we continue as before
                    Ok(current_line)
                }
                Some((first_line, tails)) => {
                    let mut buffer = String::with_capacity(max_width as usize);

                    // Finish the current line
                    let new_current_line = match first_line.split_last() {
                        None => {
                            // The first line was empty, so there's nothing to do anymore.
                            current_line
                        }
                        Some((last, heads)) => {
                            if let Some(s) = current_line.trailing_space {
                                buffer.push_str(&s);
                            }
                            for word in heads {
                                buffer.push_str(word.word);
                                buffer.push_str(word.whitespace);
                            }
                            buffer.push_str(last.word);
                            let length =
                                current_line.length + textwrap::core::display_width(&buffer) as u16;
                            write_styled(writer, capabilities, style, &buffer)?;
                            buffer.clear();
                            CurrentLine {
                                length,
                                trailing_space: Some(last.whitespace.to_owned()),
                            }
                        }
                    };

                    // Now write the rest of the lines
                    match tails.split_last() {
                        None => {
                            // There are no more lines and we're done here.
                            //
                            // We arrive here when the text fragment we wrapped above was
                            // shorter than the max length of the current line, i.e. we're
                            // still continuing with the current line.
                            Ok(new_current_line)
                        }
                        Some((last_line, next_lines)) => write_remaining_lines(
                            writer,
                            capabilities,
                            style,
                            indent,
                            buffer,
                            next_lines,
                            last_line,
                            line_prefix,
                        ),
                    }
                }
            }
        }
    }
}

pub fn write_mark<W: Write>(writer: &mut W, capabilities: &TerminalCapabilities) -> Result<()> {
    if let Some(mark) = capabilities.marks {
        match mark {
            MarkCapability::ITerm2(marks) => marks.set_mark(writer),
        }
    } else {
        Ok(())
    }
}

pub fn write_rule<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    theme: &Theme,
    length: u16,
) -> std::io::Result<()> {
    let rule = "\u{2550}".repeat(length as usize);
    write_styled(
        writer,
        capabilities,
        &Style::new().fg_color(Some(theme.rule_color)),
        rule,
    )
}

pub fn write_code_block_border<W: Write>(
    writer: &mut W,
    _theme: &Theme,
    _capabilities: &TerminalCapabilities,
    _terminal_size: &TerminalSize,
) -> std::io::Result<()> {
    writeln!(writer)
}

pub fn write_link_refs<W: Write>(
    writer: &mut W,
    environment: &Environment,
    capabilities: &TerminalCapabilities,
    links: Vec<LinkReferenceDefinition>,
) -> Result<()> {
    if !links.is_empty() {
        writeln!(writer)?;
        for link in links {
            write_styled(
                writer,
                capabilities,
                &link.style,
                format!("[{}]: ", link.index),
            )?;

            // If we can resolve the link try to write it as inline link to make the URL
            // clickable.  This mostly helps images inside inline links which we had to write as
            // reference links because we can't nest inline links.
            if let Some(url) = environment.resolve_reference(&link.target) {
                match &capabilities.style {
                    Some(StyleCapability::Ansi) => {
                        set_link_url(writer, url, &environment.hostname)?;
                        write_styled(writer, capabilities, &link.style, link.target)?;
                        clear_link(writer)?;
                    }
                    None => write_styled(writer, capabilities, &link.style, link.target)?,
                };
            } else {
                write_styled(writer, capabilities, &link.style, link.target)?;
            }

            if !link.title.is_empty() {
                write_styled(
                    writer,
                    capabilities,
                    &link.style,
                    format!(" {}", link.title),
                )?;
            }
            writeln!(writer)?;
        }
    };
    Ok(())
}

pub fn write_start_code_block<W: Write>(
    writer: &mut W,
    settings: &Settings,
    indent: u16,
    style: Style,
    block_kind: CodeBlockKind<'_>,
) -> Result<StackedState> {
    writeln!(writer)?;
    // And start the indent for the contents of the block
    write_indent(writer, indent)?;

    match (&settings.terminal_capabilities.style, block_kind) {
        (Some(StyleCapability::Ansi), CodeBlockKind::Fenced(name)) if !name.is_empty() => {
            match settings.syntax_set.find_syntax_by_token(&name) {
                None => Ok(LiteralBlockAttrs {
                    indent,
                    style: settings.theme.code_style.on_top_of(&style),
                }
                .into()),
                Some(syntax) => {
                    let parse_state = ParseState::new(syntax);
                    let highlight_state = HighlightState::new(highlighter(), ScopeStack::new());
                    Ok(HighlightBlockAttrs {
                        indent,
                        highlight_state,
                        parse_state,
                    }
                    .into())
                }
            }
        }
        (_, _) => Ok(LiteralBlockAttrs {
            indent,
            style: settings.theme.code_style.on_top_of(&style),
        }
        .into()),
    }
}

pub fn write_start_heading<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    theme: &Theme,
    context_style: Style,
    level: HeadingLevel,
) -> Result<StackedState> {
    let level_style = match level {
        HeadingLevel::H1 => {
            writeln!(writer)?;
            write_styled(writer, capabilities, &theme.h1_prefix_style, " ")?;
            theme.h1_text_style
        }
        HeadingLevel::H2 => {
            let s = theme.h2_style.on_top_of(&context_style);
            write_styled(writer, capabilities, &s, &theme.h2_marker)?;
            s
        }
        HeadingLevel::H3 => {
            let s = theme.h3_style.on_top_of(&context_style);
            write_styled(writer, capabilities, &s, &theme.h3_marker)?;
            s
        }
        HeadingLevel::H4 => {
            let s = theme.h4_style.on_top_of(&context_style);
            write_styled(writer, capabilities, &s, &theme.h4_marker)?;
            s
        }
        HeadingLevel::H5 => {
            let s = theme.h5_style.on_top_of(&context_style);
            write_styled(writer, capabilities, &s, &theme.h5_marker)?;
            s
        }
        HeadingLevel::H6 => {
            let s = theme.h6_style.on_top_of(&context_style);
            write_styled(writer, capabilities, &s, &theme.h6_marker)?;
            s
        }
    };

    // Headlines never wrap, so indent doesn't matter
    Ok(StackedState::Inline(
        InlineState::InlineBlock,
        InlineAttrs {
            style: level_style,
            indent: 0,
            quote_depth: 0,
            border_style: None,
        },
    ))
}

/// Compute, per column, its natural (unwrapped) width and the width of its
/// single longest word (the narrowest a column can get without forcibly
/// breaking a word apart).
fn column_metrics(table: &CurrentTable) -> Option<(Vec<usize>, Vec<usize>)> {
    let first_row = table.head.as_ref().or(table.rows.first())?;
    let mut natural_widths = vec![0; first_row.cells.len()];
    let mut min_word_widths = vec![0; first_row.cells.len()];
    let rows = table.head.iter().chain(table.rows.as_slice());
    for row in rows {
        let natural = row.cells.iter().map(|cell| cell.text_width());
        natural_widths = zip(&natural_widths, natural)
            .map(|(&a, b)| max(a, b))
            .collect();
        let longest_word = row.cells.iter().map(cell_longest_word_width);
        min_word_widths = zip(&min_word_widths, longest_word)
            .map(|(&a, b)| max(a, b))
            .collect();
    }
    Some((natural_widths, min_word_widths))
}

fn line_longest_word_width(line: &[(Style, CowStr<'_>)]) -> usize {
    let mut full_text = String::new();
    for (_, text) in line {
        full_text.push_str(text.as_ref());
    }
    WordSeparator::UnicodeBreakProperties
        .find_words(&full_text)
        .map(|w| display_width(w.word))
        .max()
        .unwrap_or(0)
}

fn cell_longest_word_width(cell: &TableCell<'_>) -> usize {
    cell.lines
        .iter()
        .map(|line| line_longest_word_width(line))
        .max()
        .unwrap_or(0)
}

/// Distribute `available_width` among columns whose natural (unwrapped)
/// widths are `natural_widths`.
///
/// If the natural widths already fit, they're returned unchanged so that
/// tables which fit on screen keep rendering exactly as before. Otherwise
/// width is distributed proportionally to each column's natural width,
/// shrinking columns as needed so the table fits the terminal and long cell
/// content wraps instead of overflowing. Columns are shrunk no further than
/// their `min_widths` (typically the column's longest word) where possible,
/// so wrapping doesn't needlessly break words apart in a narrow column while
/// a wide neighbour still has wrappable slack.
fn distribute_column_widths(
    natural_widths: &[usize],
    min_widths: &[usize],
    available_width: usize,
) -> Vec<usize> {
    let n = natural_widths.len();
    if n == 0 {
        return Vec::new();
    }
    let natural_total: usize = natural_widths.iter().sum();
    if natural_total <= available_width {
        return natural_widths.to_vec();
    }

    let floors: Vec<usize> = zip(natural_widths, min_widths)
        .map(|(&w, &min_w)| min_w.clamp(0, w).max(if w > 0 { 1 } else { 0 }))
        .collect();

    let mut widths: Vec<usize> = zip(natural_widths, &floors)
        .map(|(&w, &floor)| {
            let share = (available_width as f64 * w as f64 / natural_total as f64).floor() as usize;
            share.clamp(floor, w)
        })
        .collect();

    // Flooring each share may leave the total short of `available_width`;
    // hand out the remainder, one column at a time, to columns that still
    // want more (i.e. haven't reached their natural width yet).
    let mut total: usize = widths.iter().sum();
    let mut idx = 0;
    while total < available_width && widths.iter().zip(natural_widths).any(|(w, n)| w < n) {
        if widths[idx] < natural_widths[idx] {
            widths[idx] += 1;
            total += 1;
        }
        idx = (idx + 1) % n;
    }

    // If even the per-column floors don't fit (very narrow terminal, many
    // columns), shrink the widest columns down towards 1 as a last resort,
    // preferring columns that are still above their own floor.
    while total > available_width {
        let candidate = widths
            .iter()
            .enumerate()
            .filter(|&(i, &w)| w > floors[i])
            .max_by_key(|&(_, &w)| w)
            .or_else(|| widths.iter().enumerate().max_by_key(|&(_, &w)| w));
        let Some((idx, &w)) = candidate else {
            break;
        };
        if w <= 1 {
            break;
        }
        widths[idx] -= 1;
        total -= 1;
    }

    widths
}

fn write_table_prefix<W: Write>(writer: &mut W, indent: u16, line_prefix: &str) -> Result<()> {
    write_indent(writer, indent)?;
    write!(writer, "{line_prefix}")
}

fn write_table_rule<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    indent: u16,
    line_prefix: &str,
    length: u16,
) -> Result<()> {
    write_table_prefix(writer, indent, line_prefix)?;
    let rule = "\u{2500}".repeat(length.into());
    write_styled(writer, capabilities, &Style::new(), rule)?;
    writeln!(writer)
}

/// A word carrying the style it (and its trailing whitespace, which may
/// belong to a differently-styled fragment, e.g. a plain space between two
/// styled runs) should be rendered with.
#[derive(Debug, Clone, Copy)]
struct StyledWord<'a> {
    style: Style,
    whitespace_style: Style,
    word: Word<'a>,
}

impl textwrap::core::Fragment for StyledWord<'_> {
    fn width(&self) -> f64 {
        self.word.width()
    }

    fn whitespace_width(&self) -> f64 {
        self.word.whitespace_width()
    }

    fn penalty_width(&self) -> f64 {
        self.word.penalty_width()
    }
}

/// Break a single (possibly overlong) styled word into pieces no wider than
/// `width`, eagerly, so the pieces don't borrow from the temporary `word`.
///
/// Only the last piece keeps the trailing whitespace (and its style), same
/// as `Word::break_apart` itself.
fn break_apart_styled(word: StyledWord<'_>, width: usize) -> Vec<StyledWord<'_>> {
    word.word
        .break_apart(width)
        .map(|w| StyledWord {
            style: word.style,
            whitespace_style: word.whitespace_style,
            word: w,
        })
        .collect()
}

/// Push `text` onto `runs`, merging into the last run if it has the same style.
fn push_run(runs: &mut Vec<(Style, String)>, style: Style, text: &str) {
    if text.is_empty() {
        return;
    }
    match runs.last_mut() {
        Some((s, buffer)) if *s == style => buffer.push_str(text),
        _ => runs.push((style, text.to_owned())),
    }
}

/// Word-wrap a single explicit line (a `<br>`-delimited segment) of a table
/// cell to `width` columns, returning the resulting output lines as runs of
/// styled text ready to be written out.
///
/// Fragment boundaries don't necessarily align with word boundaries (e.g. a
/// plain space between two differently-styled runs is its own fragment), so
/// all fragments are concatenated into a single string for tokenizing, and
/// each resulting word is then mapped back to the style of the fragment its
/// first byte falls in.
fn wrap_cell_line(line: &[(Style, CowStr<'_>)], width: usize) -> Vec<Vec<(Style, String)>> {
    let mut full_text = String::new();
    let mut style_spans: Vec<(usize, usize, Style)> = Vec::with_capacity(line.len());
    for (style, text) in line {
        let start = full_text.len();
        full_text.push_str(text.as_ref());
        style_spans.push((start, full_text.len(), *style));
    }
    let style_at = |offset: usize| -> Style {
        style_spans
            .iter()
            .find(|(start, end, _)| *start <= offset && offset < *end)
            .map_or_else(Style::new, |(_, _, style)| *style)
    };

    let words: Vec<StyledWord<'_>> = WordSeparator::UnicodeBreakProperties
        .find_words(&full_text)
        .flat_map(|word| {
            let offset = word.word.as_ptr() as usize - full_text.as_ptr() as usize;
            let style = style_at(offset);
            let whitespace_style = if word.whitespace.is_empty() {
                style
            } else {
                style_at(offset + word.word.len())
            };
            break_apart_styled(
                StyledWord {
                    style,
                    whitespace_style,
                    word,
                },
                width.max(1),
            )
        })
        .collect();

    if words.is_empty() {
        return vec![Vec::new()];
    }

    let wrapped = textwrap::wrap_algorithms::wrap_first_fit(&words, &[width as f64]);
    wrapped
        .into_iter()
        .map(|words| {
            let mut runs: Vec<(Style, String)> = Vec::new();
            let Some((last, heads)) = words.split_last() else {
                return runs;
            };
            for word in heads {
                push_run(&mut runs, word.style, word.word.word);
                push_run(&mut runs, word.whitespace_style, word.word.whitespace);
            }
            // Drop the last word's trailing whitespace: it's just padding
            // before the next wrapped line or the cell's closing space.
            push_run(&mut runs, last.style, last.word.word);
            runs
        })
        .collect()
}

/// Word-wrap every explicit line of `cell` to `width` columns.
fn wrap_cell(cell: TableCell<'_>, width: usize) -> Vec<Vec<(Style, String)>> {
    cell.lines
        .iter()
        .flat_map(|line| wrap_cell_line(line, width))
        .collect()
}

fn line_text_width(line: &[(Style, String)]) -> usize {
    line.iter().map(|(_, text)| display_width(text)).sum()
}

fn write_table_cell_line<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    line: &[(Style, String)],
    width: usize,
    alignment: Alignment,
) -> Result<()> {
    let text_width = line_text_width(line);
    let padding = width.saturating_sub(text_width);
    match alignment {
        Alignment::Right => {
            write!(writer, " {:>padding$}", "")?;
            for (style, text) in line {
                write_styled(writer, capabilities, style, text)?;
            }
            write!(writer, " ")?;
        }
        Alignment::Center => {
            let left = padding / 2;
            let right = padding - left;
            write!(writer, " {:>left$}", "")?;
            for (style, text) in line {
                write_styled(writer, capabilities, style, text)?;
            }
            write!(writer, "{:>right$} ", "")?;
        }
        _ => {
            write!(writer, " ")?;
            for (style, text) in line {
                write_styled(writer, capabilities, style, text)?;
            }
            write!(writer, "{:>padding$} ", "")?;
        }
    }
    Ok(())
}

/// Write one output row (which may span several terminal lines, if any
/// cell wrapped) for a table row's cells, already wrapped to their column
/// widths.
fn write_table_row<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    indent: u16,
    line_prefix: &str,
    wrapped_cells: &[Vec<Vec<(Style, String)>>],
    widths: &[usize],
    alignments: &[Alignment],
) -> Result<()> {
    let height = wrapped_cells
        .iter()
        .map(|c| c.len())
        .max()
        .unwrap_or(1)
        .max(1);
    let empty_line: Vec<(Style, String)> = Vec::new();
    for line_idx in 0..height {
        write_table_prefix(writer, indent, line_prefix)?;
        for ((cell, &width), &alignment) in zip(zip(wrapped_cells, widths), alignments) {
            let line = cell.get(line_idx).unwrap_or(&empty_line);
            write_table_cell_line(writer, capabilities, line, width, alignment)?;
        }
        writeln!(writer)?;
    }
    Ok(())
}

pub fn write_table<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    terminal_size: &TerminalSize,
    indent: u16,
    line_prefix: &str,
    prefix_cols: u16,
    table: CurrentTable,
) -> Result<()> {
    if let Some((natural_widths, min_word_widths)) = column_metrics(&table) {
        let n = natural_widths.len();
        let overhead = 2 * n;
        let available_width = (terminal_size.columns as usize)
            .saturating_sub(indent as usize)
            .saturating_sub(prefix_cols as usize)
            .saturating_sub(overhead);
        let widths = distribute_column_widths(&natural_widths, &min_word_widths, available_width);
        let total_width: usize = widths.iter().sum();
        let rule_length = min(
            (total_width + overhead).try_into().unwrap_or(u16::MAX),
            terminal_size
                .columns
                .saturating_sub(indent)
                .saturating_sub(prefix_cols),
        );
        write_table_rule(writer, capabilities, indent, line_prefix, rule_length)?;

        if let Some(head) = table.head {
            let wrapped_cells: Vec<_> = zip(head.cells, &widths)
                .map(|(cell, &width)| wrap_cell(cell, width))
                .collect();
            write_table_row(
                writer,
                capabilities,
                indent,
                line_prefix,
                &wrapped_cells,
                &widths,
                &table.alignments,
            )?;
            write_table_rule(writer, capabilities, indent, line_prefix, rule_length)?;
        }

        for row in table.rows {
            let wrapped_cells: Vec<_> = zip(row.cells, &widths)
                .map(|(cell, &width)| wrap_cell(cell, width))
                .collect();
            write_table_row(
                writer,
                capabilities,
                indent,
                line_prefix,
                &wrapped_cells,
                &widths,
                &table.alignments,
            )?;
        }
        write_table_rule(writer, capabilities, indent, line_prefix, rule_length)?;
    }
    Ok(())
}
