// Copyright 2020 Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::cmp::{max, min};
use std::io::{Result, Write};
use std::iter::zip;

use anstyle::{AnsiColor, Style};
use pulldown_cmark::{Alignment, CodeBlockKind, HeadingLevel};
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
                (max_width - indent) as f64,
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
    style: Style,
    level: HeadingLevel,
) -> Result<StackedState> {
    let level_style = match level {
        HeadingLevel::H1 => {
            writeln!(writer)?;
            let text_style = Style::new()
                .bg_color(Some(AnsiColor::BrightBlue.into()))
                .fg_color(Some(AnsiColor::BrightWhite.into()))
                .bold();
            let pad_style = Style::new()
                .bg_color(Some(AnsiColor::BrightBlue.into()))
                .fg_color(Some(AnsiColor::BrightBlue.into()));
            write_styled(writer, capabilities, &pad_style, " ")?;
            text_style
        }
        HeadingLevel::H2 => {
            write_styled(writer, capabilities, &style, "━━ ")?;
            style
        }
        HeadingLevel::H3 => {
            write_styled(writer, capabilities, &style, "  ── ")?;
            style
        }
        HeadingLevel::H4 => {
            let s = style.dimmed();
            write_styled(writer, capabilities, &s, "    ┄ ")?;
            s
        }
        HeadingLevel::H5 => {
            let s = style.dimmed().italic();
            write_styled(writer, capabilities, &s, "      ╌ ")?;
            s
        }
        HeadingLevel::H6 => {
            let s = style.dimmed().italic();
            write_styled(writer, capabilities, &s, "        · ")?;
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

fn calculate_column_widths(table: &CurrentTable) -> Option<Vec<usize>> {
    let first_row = table.head.as_ref().or(table.rows.first())?;
    let mut widths = vec![0; first_row.cells.len()];
    let rows = table.head.iter().chain(table.rows.as_slice());
    for row in rows {
        let current = row.cells.as_slice().iter().map(|cell| {
            cell.fragments
                .as_slice()
                .iter()
                .fold(0, |acc, x| acc + x.len())
        });
        widths = zip(widths, current).map(|(a, b)| max(a, b)).collect();
    }
    Some(widths)
}

// TODO: Support themes for table rule.
fn write_table_rule<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    length: u16,
) -> Result<()> {
    let rule = "\u{2500}".repeat(length.into());
    write_styled(writer, capabilities, &Style::new(), rule)?;
    writeln!(writer)
}

fn format_table_cell(cell: TableCell, width: usize, alignment: Alignment) -> String {
    use Alignment::*;
    let content = cell.fragments.join("");
    match alignment {
        Left | None => format!(" {:<width$} ", content),
        Center => format!(" {:^width$} ", content),
        Right => format!(" {:>width$} ", content),
    }
}

pub fn write_table<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    terminal_size: &TerminalSize,
    table: CurrentTable,
) -> Result<()> {
    if let Some(widths) = calculate_column_widths(&table) {
        // Calculate length of the table rule.
        let total_width: usize = widths.iter().sum();
        let rule_length = min(
            // We use two spaces for padding for each cell in format_table_cell.
            (total_width + 2 * widths.len())
                .try_into()
                .unwrap_or(u16::MAX),
            terminal_size.columns,
        );
        write_table_rule(writer, capabilities, rule_length)?;

        // Write the table head in bold if any.
        if let Some(head) = table.head {
            for ((cell, &width), &alignment) in zip(zip(head.cells, &widths), &table.alignments) {
                write_styled(
                    writer,
                    capabilities,
                    &Style::new().bold(),
                    format_table_cell(cell, width, alignment),
                )?;
            }
            writeln!(writer)?;
            write_table_rule(writer, capabilities, rule_length)?;
        }

        // Write table body.
        for row in table.rows {
            for ((cell, &width), &alignment) in zip(zip(row.cells, &widths), &table.alignments) {
                write_styled(
                    writer,
                    capabilities,
                    &Style::new(),
                    format_table_cell(cell, width, alignment),
                )?;
            }
            writeln!(writer)?;
        }
        write_table_rule(writer, capabilities, rule_length)?;
    }
    // Do nothing when there are no rows in the table, which should be impossible.
    Ok(())
}
