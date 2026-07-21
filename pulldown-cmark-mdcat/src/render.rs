// Copyright Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Rendering algorithm.

use std::io::prelude::*;
use std::io::Result;

use anstyle::{Effects, Style};
use pulldown_cmark::Event::*;
use pulldown_cmark::Tag;
use pulldown_cmark::Tag::*;
use pulldown_cmark::TagEnd;
use pulldown_cmark::{BlockQuoteKind, Event, HeadingLevel, LinkType};
use syntect::highlighting::HighlightIterator;
use syntect::util::LinesWithEndings;
use textwrap::core::display_width;
use tracing::{event, instrument, Level};
use url::Url;

use crate::render::highlighting::{highlighter, highlighter_for};
use crate::resources::ResourceUrlHandler;
use crate::terminal::capabilities::TerminalCapabilities;
use crate::theme::CombineStyle;
use crate::{Environment, Settings, Theme};

mod data;
mod highlighting;
pub(crate) mod math;
mod state;
mod write;

use crate::references::*;
use state::*;
use write::*;

use crate::render::data::{CurrentLine, CurrentTable};
use crate::render::state::MarginControl::NoMargin;
use crate::terminal::capabilities::{ImageCapability, StyleCapability};
use crate::terminal::osc::{clear_link, set_link_url};
pub use data::StateData;

fn alert_style_and_label(kind: BlockQuoteKind, theme: &Theme) -> (Style, &str) {
    match kind {
        BlockQuoteKind::Note => (theme.alert_note_style, theme.alert_note_label.as_str()),
        BlockQuoteKind::Tip => (theme.alert_tip_style, theme.alert_tip_label.as_str()),
        BlockQuoteKind::Important => (
            theme.alert_important_style,
            theme.alert_important_label.as_str(),
        ),
        BlockQuoteKind::Warning => (
            theme.alert_warning_style,
            theme.alert_warning_label.as_str(),
        ),
        BlockQuoteKind::Caution => (
            theme.alert_caution_style,
            theme.alert_caution_label.as_str(),
        ),
    }
}

fn write_alert_label<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    style: Style,
    label: &str,
) -> Result<()> {
    if let Some(StyleCapability::Ansi) = capabilities.style {
        write!(
            writer,
            "{}\u{2502}{} ",
            style.render(),
            style.render_reset()
        )?;
    } else {
        write!(writer, "  ")?;
    }
    write_styled(writer, capabilities, &style, label)?;
    writeln!(writer)
}

fn quote_line_prefix(
    capabilities: &TerminalCapabilities,
    theme: &Theme,
    depth: u16,
    border_style: Option<Style>,
) -> (String, u16) {
    if depth == 0 {
        return (String::new(), 0);
    }
    let prefix = match capabilities.style {
        Some(StyleCapability::Ansi) => {
            let s = border_style.unwrap_or(theme.quote_border_style);
            format!("{}\u{2502}{} ", s.render(), s.render_reset()).repeat(depth as usize)
        }
        None => "  ".repeat(depth as usize),
    };
    (prefix, depth * 2)
}
pub use state::State;
pub use state::StateAndData;

fn render_math_image(
    settings: &Settings,
    math: &str,
    display_mode: bool,
) -> Option<math::MathImage> {
    match settings.terminal_capabilities.image.as_ref()? {
        ImageCapability::Kitty(_) | ImageCapability::ITerm2(_) => {}
        #[cfg(feature = "sixel")]
        ImageCapability::Sixel(_) => {}
    }
    math::render_math_png(
        math,
        display_mode,
        &settings.terminal_size,
        &settings.theme.math_style,
    )
}

/// Write a prepared math PNG image. Returns (width_cols, height_rows) on success.
fn write_math_image<W: Write>(
    writer: &mut W,
    settings: &Settings,
    img: math::MathImage,
    move_cursor: bool,
) -> Result<(u16, u16, bool)> {
    let dims = (img.width_columns, img.height_rows);
    let cursor_moved = match settings.terminal_capabilities.image.as_ref() {
        Some(ImageCapability::Kitty(k)) => {
            k.write_png_data(writer, img.png, move_cursor)?;
            move_cursor
        }
        Some(ImageCapability::ITerm2(i)) => {
            i.write_png_data(writer, &img.png)?;
            false
        }
        #[cfg(feature = "sixel")]
        Some(ImageCapability::Sixel(s)) => {
            s.write_png_data(writer, &img.png)?;
            false
        }
        None => return Ok((0, 0, false)),
    };
    Ok((dims.0, dims.1, cursor_moved))
}

#[allow(clippy::cognitive_complexity)]
#[instrument(level = "trace", skip(writer, settings, environment, resource_handler))]
pub fn write_event<'a, W: Write>(
    writer: &mut W,
    settings: &Settings,
    environment: &Environment,
    resource_handler: &dyn ResourceUrlHandler,
    state: State,
    data: StateData<'a>,
    event: Event<'a>,
) -> Result<StateAndData<StateData<'a>>> {
    use self::DefinitionPart::*;
    use self::InlineState::*;
    use self::ListItemState::*;
    use self::StackedState::*;
    use State::*;

    event!(Level::TRACE, event = ?event, "rendering");
    match (state, event) {
        // Top level items
        (TopLevel(attrs), Start(Paragraph)) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            State::stack_onto(TopLevelAttrs::margin_before())
                .current(Inline(InlineText, InlineAttrs::default()))
                .and_data(data)
                .ok()
        }
        (TopLevel(attrs), Start(Tag::HtmlBlock)) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            // We render HTML literally
            State::stack_onto(TopLevelAttrs::margin_before())
                .current(
                    HtmlBlockAttrs {
                        indent: 0,
                        initial_indent: 0,
                        style: settings.theme.html_block_style,
                    }
                    .into(),
                )
                .and_data(data)
                .ok()
        }
        (TopLevel(attrs), Start(Heading { level, .. })) => {
            let (data, links) = data.take_link_references();
            write_link_refs(writer, environment, &settings.terminal_capabilities, links)?;
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            write_mark(writer, &settings.terminal_capabilities)?;

            State::stack_onto(TopLevelAttrs::margin_before())
                .current(write_start_heading(
                    writer,
                    &settings.terminal_capabilities,
                    &settings.theme,
                    Style::new(),
                    level,
                )?)
                .and_data(data)
                .ok()
        }
        (TopLevel(attrs), Start(BlockQuote(kind))) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            let block_attrs = if let Some(kind) = kind {
                let (style, label) = alert_style_and_label(kind, &settings.theme);
                write_alert_label(writer, &settings.terminal_capabilities, style, label)?;
                StyledBlockAttrs::default()
                    .alert(style)
                    .without_margin_before()
            } else {
                // We've written a block-level margin already, so the first
                // block inside the styled block should add another margin.
                StyledBlockAttrs::default()
                    .block_quote()
                    .without_margin_before()
            };
            State::stack_onto(TopLevelAttrs::margin_before())
                .current(block_attrs.into())
                .and_data(data)
                .ok()
        }
        (TopLevel(attrs), Rule) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            write_rule(
                writer,
                &settings.terminal_capabilities,
                &settings.theme,
                settings.terminal_size.columns,
            )?;
            writeln!(writer)?;
            TopLevel(TopLevelAttrs::margin_before()).and_data(data).ok()
        }
        (TopLevel(attrs), Start(CodeBlock(kind))) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }

            State::stack_onto(TopLevelAttrs::margin_before())
                .current(write_start_code_block(
                    writer,
                    settings,
                    0,
                    Style::new(),
                    kind,
                )?)
                .and_data(data)
                .ok()
        }
        (TopLevel(attrs), Start(List(start))) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            let kind = start.map_or(ListItemKind::Unordered, |start| {
                ListItemKind::Ordered(start)
            });

            State::stack_onto(TopLevelAttrs::margin_before())
                .current(Inline(ListItem(kind, StartItem), InlineAttrs::default()))
                .and_data(data)
                .ok()
        }
        (TopLevel(attrs), Start(Tag::DefinitionList)) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            State::stack_onto(TopLevelAttrs::margin_before())
                .current(Inline(Definition(Term, StartItem), InlineAttrs::default()))
                .and_data(data)
                .ok()
        }
        (TopLevel(attrs), Start(Table(alignments))) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            let current_table = CurrentTable {
                alignments,
                ..data.current_table
            };
            let data = StateData {
                current_table,
                ..data
            };
            State::stack_onto(TopLevelAttrs::margin_before())
                .current(TableBlock(TableBlockAttrs::default()))
                .and_data(data)
                .ok()
        }

        // Nested blocks with style, e.g. paragraphs in quotes, etc.
        (Stacked(stack, StyledBlock(attrs)), Start(Paragraph)) => {
            let (prefix, _) = quote_line_prefix(
                &settings.terminal_capabilities,
                &settings.theme,
                attrs.quote_depth,
                attrs.border_style,
            );
            if attrs.margin_before != NoMargin {
                write_indent(writer, attrs.indent)?;
                write!(writer, "{}", prefix)?;
                writeln!(writer)?;
            }
            write_indent(writer, attrs.indent)?;
            write!(writer, "{}", prefix)?;
            let inline = InlineAttrs::from(&attrs);
            stack
                .push(attrs.with_margin_before().into())
                .current(Inline(InlineText, inline))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, StyledBlock(attrs)), Start(Tag::HtmlBlock)) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            let state = HtmlBlockAttrs {
                indent: attrs.indent,
                initial_indent: attrs.indent,
                style: settings.theme.html_block_style.on_top_of(&attrs.style),
            }
            .into();
            stack
                .push(attrs.with_margin_before().into())
                .current(state)
                .and_data(data)
                .ok()
        }
        (Stacked(stack, StyledBlock(attrs)), Start(BlockQuote(kind))) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            let block_attrs = if let Some(kind) = kind {
                let (style, label) = alert_style_and_label(kind, &settings.theme);
                write_alert_label(writer, &settings.terminal_capabilities, style, label)?;
                attrs.clone().without_margin_before().alert(style)
            } else {
                attrs.clone().without_margin_before().block_quote()
            };
            stack
                .push(attrs.with_margin_before().into())
                .current(block_attrs.into())
                .and_data(data)
                .ok()
        }
        (Stacked(stack, StyledBlock(attrs)), Rule) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            write_indent(writer, attrs.indent)?;
            write_rule(
                writer,
                &settings.terminal_capabilities,
                &settings.theme,
                settings.terminal_size.columns.saturating_sub(attrs.indent),
            )?;
            writeln!(writer)?;
            stack
                .current(attrs.with_margin_before().into())
                .and_data(data)
                .ok()
        }
        (Stacked(stack, StyledBlock(attrs)), Start(Heading { level, .. })) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            write_indent(writer, attrs.indent)?;

            // We deliberately don't mark headings which aren't top-level.
            let style = attrs.style;
            stack
                .push(attrs.with_margin_before().into())
                .current(write_start_heading(
                    writer,
                    &settings.terminal_capabilities,
                    &settings.theme,
                    style,
                    level,
                )?)
                .and_data(data)
                .ok()
        }
        (Stacked(stack, StyledBlock(attrs)), Start(List(start))) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            let kind = start.map_or(ListItemKind::Unordered, |start| {
                ListItemKind::Ordered(start)
            });
            let inline = InlineAttrs::from(&attrs);
            stack
                .push(attrs.with_margin_before().into())
                .current(Inline(ListItem(kind, StartItem), inline))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, StyledBlock(attrs)), Start(Tag::DefinitionList)) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            let inline = InlineAttrs::from(&attrs);
            stack
                .push(attrs.with_margin_before().into())
                .current(Inline(Definition(Term, StartItem), inline))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, StyledBlock(attrs)), Start(Table(alignments))) => {
            if attrs.margin_before != NoMargin {
                let (prefix, _) = quote_line_prefix(
                    &settings.terminal_capabilities,
                    &settings.theme,
                    attrs.quote_depth,
                    attrs.border_style,
                );
                write_indent(writer, attrs.indent)?;
                write!(writer, "{}", prefix)?;
                writeln!(writer)?;
            }
            let current_table = CurrentTable {
                alignments,
                ..data.current_table
            };
            let data = StateData {
                current_table,
                ..data
            };
            let table_attrs = TableBlockAttrs::from_styled_block(&attrs);
            stack
                .push(attrs.with_margin_before().into())
                .current(TableBlock(table_attrs))
                .and_data(data.current_line(CurrentLine::empty()))
                .ok()
        }
        (Stacked(stack, StyledBlock(attrs)), Start(CodeBlock(kind))) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            let StyledBlockAttrs { indent, style, .. } = attrs;
            stack
                .push(attrs.into())
                .current(write_start_code_block(
                    writer, settings, indent, style, kind,
                )?)
                .and_data(data)
                .ok()
        }

        // Lists
        (Stacked(stack, Inline(ListItem(kind, state), attrs)), Start(Item)) => {
            let InlineAttrs {
                indent,
                style,
                quote_depth,
                ..
            } = attrs;
            if state == ItemBlock {
                // Add margin
                writeln!(writer)?;
            }
            write_indent(writer, indent)?;
            let indent = match kind {
                ListItemKind::Unordered => {
                    write!(writer, "\u{2022} ")?;
                    indent + 2
                }
                ListItemKind::Ordered(no) => {
                    write!(writer, "{no:>2}. ")?;
                    indent + 4
                }
            };
            stack
                .current(Inline(
                    ListItem(kind, StartItem),
                    InlineAttrs {
                        style,
                        indent,
                        quote_depth,
                        border_style: attrs.border_style,
                    },
                ))
                .and_data(data.current_line(CurrentLine {
                    length: indent,
                    trailing_space: None,
                }))
                .ok()
        }
        (Stacked(stack, Inline(ListItem(kind, state), attrs)), Start(Paragraph)) => {
            if state != StartItem {
                // Write margin, unless we're at the start of the list item in which case the first line of the
                // paragraph should go right beside the item bullet.
                writeln!(writer)?;
                write_indent(writer, attrs.indent)?;
            }
            stack
                .push(Inline(ListItem(kind, ItemBlock), attrs.clone()))
                .current(Inline(InlineText, attrs))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(ListItem(kind, state), attrs)), Start(Tag::HtmlBlock)) => {
            let InlineAttrs { indent, style, .. } = attrs;
            let initial_indent = if state == StartItem {
                0
            } else {
                writeln!(writer)?;
                indent
            };
            stack
                .push(Inline(ListItem(kind, ItemBlock), attrs))
                .current(
                    HtmlBlockAttrs {
                        style: settings.theme.html_block_style.on_top_of(&style),
                        indent,
                        initial_indent,
                    }
                    .into(),
                )
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(ListItem(kind, _), attrs)), Start(CodeBlock(ck))) => {
            writeln!(writer)?;
            let InlineAttrs { indent, style, .. } = attrs;
            stack
                .push(Inline(ListItem(kind, ItemBlock), attrs))
                .current(write_start_code_block(writer, settings, indent, style, ck)?)
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(ListItem(kind, _), attrs)), Rule) => {
            writeln!(writer)?;
            write_indent(writer, attrs.indent)?;
            write_rule(
                writer,
                &settings.terminal_capabilities,
                &settings.theme,
                settings.terminal_size.columns.saturating_sub(attrs.indent),
            )?;
            writeln!(writer)?;
            stack
                .current(Inline(ListItem(kind, ItemBlock), attrs))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(ListItem(kind, state), attrs)), Start(Heading { level, .. })) => {
            if state != StartItem {
                writeln!(writer)?;
                write_indent(writer, attrs.indent)?;
            }
            // We deliberately don't mark headings which aren't top-level.
            let style = attrs.style;
            stack
                .push(Inline(ListItem(kind, ItemBlock), attrs))
                .current(write_start_heading(
                    writer,
                    &settings.terminal_capabilities,
                    &settings.theme,
                    style,
                    level,
                )?)
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(ListItem(kind, _), attrs)), Start(List(start))) => {
            writeln!(writer)?;
            let nested_kind = start.map_or(ListItemKind::Unordered, |start| {
                ListItemKind::Ordered(start)
            });
            stack
                .push(Inline(ListItem(kind, ItemBlock), attrs.clone()))
                .current(Inline(ListItem(nested_kind, StartItem), attrs))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(ListItem(kind, _), attrs)), Start(Table(alignments))) => {
            writeln!(writer)?;
            let current_table = CurrentTable {
                alignments,
                ..data.current_table
            };
            let data = StateData {
                current_table,
                ..data
            };
            stack
                .push(Inline(ListItem(kind, ItemBlock), attrs.clone()))
                .current(TableBlock(TableBlockAttrs::from_inline(&attrs)))
                .and_data(data.current_line(CurrentLine::empty()))
                .ok()
        }
        (Stacked(stack, Inline(ListItem(kind, _), attrs)), Start(BlockQuote(bq_kind))) => {
            writeln!(writer)?;
            let block_attrs = if let Some(bq_kind) = bq_kind {
                let (style, label) = alert_style_and_label(bq_kind, &settings.theme);
                write_alert_label(writer, &settings.terminal_capabilities, style, label)?;
                StyledBlockAttrs::from(&attrs)
                    .without_margin_before()
                    .alert(style)
            } else {
                StyledBlockAttrs::from(&attrs)
                    .without_margin_before()
                    .block_quote()
            };
            stack
                .push(Inline(ListItem(kind, ItemBlock), attrs))
                .current(block_attrs.into())
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(ListItem(kind, state), attrs)), End(TagEnd::Item)) => {
            let InlineAttrs { indent, style, .. } = attrs;
            let data = if state != ItemBlock {
                // End the inline text of this item
                writeln!(writer)?;
                data.current_line(CurrentLine::empty())
            } else {
                data
            };
            // Decrease indent back to the level where we can write the next item bullet, and increment the list item number.
            let (indent, kind) = match kind {
                ListItemKind::Unordered => (indent - 2, ListItemKind::Unordered),
                ListItemKind::Ordered(no) => (indent - 4, ListItemKind::Ordered(no + 1)),
            };
            stack
                .current(Inline(
                    ListItem(kind, state),
                    InlineAttrs {
                        style,
                        indent,
                        quote_depth: attrs.quote_depth,
                        border_style: attrs.border_style,
                    },
                ))
                .and_data(data)
                .ok()
        }

        // Definition lists.
        //
        // Structurally mirrors list items above: a term/description's content may be direct
        // inline text (tight) or nested blocks wrapped in e.g. `Paragraph` (loose).
        (Stacked(stack, Inline(Definition(_, state), attrs)), Start(Tag::DefinitionListTitle)) => {
            if state == ItemBlock {
                // Add margin before a new term following a previous term's nested block.
                writeln!(writer)?;
            }
            write_indent(writer, attrs.indent)?;
            stack
                .current(Inline(Definition(Term, StartItem), attrs.clone()))
                .and_data(data.current_line(CurrentLine {
                    length: attrs.indent,
                    trailing_space: None,
                }))
                .ok()
        }
        // Term text is bolded ad-hoc here, rather than folded into `attrs.style`, so the base
        // style carries over unchanged to the description that follows.
        (Stacked(stack, Inline(Definition(Term, state), attrs)), Text(text)) => {
            let (prefix, prefix_cols) = quote_line_prefix(
                &settings.terminal_capabilities,
                &settings.theme,
                attrs.quote_depth,
                attrs.border_style,
            );
            let current_line = write_styled_and_wrapped(
                writer,
                &settings.terminal_capabilities,
                &attrs.style.bold(),
                settings.terminal_size.columns,
                attrs.indent,
                data.current_line,
                text,
                &prefix,
                prefix_cols,
            )?;
            Ok(stack
                .current(Inline(Definition(Term, state), attrs))
                .and_data(StateData {
                    current_line,
                    ..data
                }))
        }
        (
            Stacked(stack, Inline(Definition(Term, state), attrs)),
            End(TagEnd::DefinitionListTitle),
        ) => {
            if state != ItemBlock {
                writeln!(writer)?;
            }
            // Carry `state` through unchanged (mirroring `End(Item)`): the term's content is
            // always direct inline text, so `state` is `StartItem` here, meaning the very next
            // description should *not* get a blank line before it.
            stack
                .current(Inline(Definition(Term, state), attrs))
                .and_data(data.current_line(CurrentLine::empty()))
                .ok()
        }
        (
            Stacked(stack, Inline(Definition(_, state), attrs)),
            Start(Tag::DefinitionListDefinition),
        ) => {
            if state == ItemBlock {
                writeln!(writer)?;
            }
            let indent = attrs.indent + 4;
            write_indent(writer, indent)?;
            stack
                .current(Inline(
                    Definition(Description, StartItem),
                    InlineAttrs { indent, ..attrs },
                ))
                .and_data(data.current_line(CurrentLine {
                    length: indent,
                    trailing_space: None,
                }))
                .ok()
        }
        (
            Stacked(stack, Inline(Definition(Description, state), attrs)),
            End(TagEnd::DefinitionListDefinition),
        ) => {
            let data = if state != ItemBlock {
                writeln!(writer)?;
                data.current_line(CurrentLine::empty())
            } else {
                data
            };
            let indent = attrs.indent.saturating_sub(4);
            stack
                .current(Inline(
                    Definition(Description, state),
                    InlineAttrs { indent, ..attrs },
                ))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(Definition(part, state), attrs)), Start(Paragraph)) => {
            if state != StartItem {
                writeln!(writer)?;
                write_indent(writer, attrs.indent)?;
            }
            stack
                .push(Inline(Definition(part, ItemBlock), attrs.clone()))
                .current(Inline(InlineText, attrs))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(Definition(part, state), attrs)), Start(Tag::HtmlBlock)) => {
            let InlineAttrs { indent, style, .. } = attrs;
            let initial_indent = if state == StartItem {
                0
            } else {
                writeln!(writer)?;
                indent
            };
            stack
                .push(Inline(Definition(part, ItemBlock), attrs))
                .current(
                    HtmlBlockAttrs {
                        style: settings.theme.html_block_style.on_top_of(&style),
                        indent,
                        initial_indent,
                    }
                    .into(),
                )
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(Definition(part, _), attrs)), Start(CodeBlock(ck))) => {
            writeln!(writer)?;
            let InlineAttrs { indent, style, .. } = attrs;
            stack
                .push(Inline(Definition(part, ItemBlock), attrs))
                .current(write_start_code_block(writer, settings, indent, style, ck)?)
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(Definition(part, _), attrs)), Rule) => {
            writeln!(writer)?;
            write_indent(writer, attrs.indent)?;
            write_rule(
                writer,
                &settings.terminal_capabilities,
                &settings.theme,
                settings.terminal_size.columns.saturating_sub(attrs.indent),
            )?;
            writeln!(writer)?;
            stack
                .current(Inline(Definition(part, ItemBlock), attrs))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(Definition(part, state), attrs)), Start(Heading { level, .. })) => {
            if state != StartItem {
                writeln!(writer)?;
                write_indent(writer, attrs.indent)?;
            }
            let style = attrs.style;
            stack
                .push(Inline(Definition(part, ItemBlock), attrs))
                .current(write_start_heading(
                    writer,
                    &settings.terminal_capabilities,
                    &settings.theme,
                    style,
                    level,
                )?)
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(Definition(part, _), attrs)), Start(List(start))) => {
            writeln!(writer)?;
            let nested_kind = start.map_or(ListItemKind::Unordered, |start| {
                ListItemKind::Ordered(start)
            });
            stack
                .push(Inline(Definition(part, ItemBlock), attrs.clone()))
                .current(Inline(ListItem(nested_kind, StartItem), attrs))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(Definition(part, _), attrs)), Start(Tag::DefinitionList)) => {
            writeln!(writer)?;
            stack
                .push(Inline(Definition(part, ItemBlock), attrs.clone()))
                .current(Inline(Definition(Term, StartItem), attrs))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(Definition(part, _), attrs)), Start(Table(alignments))) => {
            writeln!(writer)?;
            let current_table = CurrentTable {
                alignments,
                ..data.current_table
            };
            let data = StateData {
                current_table,
                ..data
            };
            stack
                .push(Inline(Definition(part, ItemBlock), attrs.clone()))
                .current(TableBlock(TableBlockAttrs::from_inline(&attrs)))
                .and_data(data.current_line(CurrentLine::empty()))
                .ok()
        }
        (Stacked(stack, Inline(Definition(part, _), attrs)), Start(BlockQuote(bq_kind))) => {
            writeln!(writer)?;
            let block_attrs = if let Some(bq_kind) = bq_kind {
                let (style, label) = alert_style_and_label(bq_kind, &settings.theme);
                write_alert_label(writer, &settings.terminal_capabilities, style, label)?;
                StyledBlockAttrs::from(&attrs)
                    .without_margin_before()
                    .alert(style)
            } else {
                StyledBlockAttrs::from(&attrs)
                    .without_margin_before()
                    .block_quote()
            };
            stack
                .push(Inline(Definition(part, ItemBlock), attrs))
                .current(block_attrs.into())
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(Definition(part, ItemBlock), attrs)), Text(text)) => {
            let (prefix, prefix_cols) = quote_line_prefix(
                &settings.terminal_capabilities,
                &settings.theme,
                attrs.quote_depth,
                attrs.border_style,
            );
            write!(writer, "{}", prefix)?;
            write_indent(writer, attrs.indent)?;
            let current_line = write_styled_and_wrapped(
                writer,
                &settings.terminal_capabilities,
                &attrs.style,
                settings.terminal_size.columns,
                attrs.indent,
                data.current_line,
                text,
                &prefix,
                prefix_cols,
            )?;
            Ok(stack
                .current(Inline(Definition(part, ItemText), attrs))
                .and_data(StateData {
                    current_line,
                    ..data
                }))
        }

        // Literal blocks without highlighting
        (Stacked(stack, LiteralBlock(attrs)), Text(text)) => {
            let LiteralBlockAttrs { indent, style, .. } = attrs;
            for line in LinesWithEndings::from(&text) {
                write_indent(writer, indent)?;
                write_styled(writer, &settings.terminal_capabilities, &style, line)?;
                if !line.ends_with('\n') {
                    writeln!(writer)?;
                }
                write_indent(writer, indent)?;
            }
            stack.current(attrs.into()).and_data(data).ok()
        }
        (Stacked(stack, LiteralBlock(_)), End(TagEnd::CodeBlock)) => {
            write_code_block_border(
                writer,
                &settings.theme,
                &settings.terminal_capabilities,
                &settings.terminal_size,
            )?;
            stack.pop().and_data(data).ok()
        }
        // HTML and extra text in a literal block, i.e HTML in an HTML block
        (Stacked(stack, HtmlBlock(attrs)), Text(text)) => {
            let HtmlBlockAttrs {
                indent,
                initial_indent,
                style,
            } = attrs;
            for (n, line) in LinesWithEndings::from(&text).enumerate() {
                let line_indent = if n == 0 { initial_indent } else { indent };
                write_indent(writer, line_indent)?;
                write_styled(writer, &settings.terminal_capabilities, &style, line)?;
            }
            stack
                .current(
                    HtmlBlockAttrs {
                        initial_indent: attrs.indent,
                        indent: attrs.indent,
                        style: attrs.style,
                    }
                    .into(),
                )
                .and_data(data)
                .ok()
        }
        (Stacked(stack, HtmlBlock(attrs)), Html(html)) => {
            write_indent(writer, attrs.initial_indent)?;
            // TODO: Split html into lines and properly account for initial indent
            write_styled(writer, &settings.terminal_capabilities, &attrs.style, html)?;
            stack
                .current(
                    HtmlBlockAttrs {
                        initial_indent: attrs.indent,
                        indent: attrs.indent,
                        style: attrs.style,
                    }
                    .into(),
                )
                .and_data(data)
                .ok()
        }

        // Highlighted code blocks
        (Stacked(stack, HighlightBlock(mut attrs)), Text(text)) => {
            for line in LinesWithEndings::from(&text) {
                let ops = attrs
                    .parse_state
                    .parse_line(line, settings.syntax_set)
                    .expect("syntect parsing shouldn't fail in mdcat");
                write_indent(writer, attrs.indent)?;
                match &settings.syntax_theme {
                    Some(theme) => {
                        let h = highlighter_for(theme);
                        let regions =
                            HighlightIterator::new(&mut attrs.highlight_state, &ops, line, &h);
                        highlighting::write_as_rgb(writer, regions)?;
                    }
                    None => {
                        let regions = HighlightIterator::new(
                            &mut attrs.highlight_state,
                            &ops,
                            line,
                            highlighter(),
                        );
                        highlighting::write_as_ansi(writer, regions)?;
                    }
                }
                write_indent(writer, attrs.indent)?;
            }
            stack.current(attrs.into()).and_data(data).ok()
        }
        (Stacked(stack, HighlightBlock(_)), End(TagEnd::CodeBlock)) => {
            write_code_block_border(
                writer,
                &settings.theme,
                &settings.terminal_capabilities,
                &settings.terminal_size,
            )?;
            stack.pop().and_data(data).ok()
        }

        // Inline markup
        (Stacked(stack, Inline(state, attrs)), Start(Emphasis)) => {
            let InlineAttrs {
                style,
                indent,
                quote_depth,
                border_style,
            } = attrs;
            let effects = style.get_effects();
            let style =
                style.effects(effects.set(Effects::ITALIC, !effects.contains(Effects::ITALIC)));
            stack
                .push(Inline(state, attrs))
                .current(Inline(
                    state,
                    InlineAttrs {
                        style,
                        indent,
                        quote_depth,
                        border_style,
                    },
                ))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(state, attrs)), Start(Strong)) => {
            let InlineAttrs {
                indent,
                quote_depth,
                border_style,
                ..
            } = attrs;
            let style = attrs.style.bold();
            stack
                .push(Inline(state, attrs))
                .current(Inline(
                    state,
                    InlineAttrs {
                        style,
                        indent,
                        quote_depth,
                        border_style,
                    },
                ))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(state, attrs)), Start(Strikethrough)) => {
            let InlineAttrs {
                indent,
                quote_depth,
                border_style,
                ..
            } = attrs;
            let style = attrs.style.strikethrough();
            stack
                .push(Inline(state, attrs))
                .current(Inline(
                    state,
                    InlineAttrs {
                        style,
                        indent,
                        quote_depth,
                        border_style,
                    },
                ))
                .and_data(data)
                .ok()
        }
        (
            Stacked(stack, Inline(_, _)),
            End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough),
        ) => stack.pop().and_data(data).ok(),
        (Stacked(stack, Inline(state, attrs)), Code(code)) => {
            let (prefix, prefix_cols) = quote_line_prefix(
                &settings.terminal_capabilities,
                &settings.theme,
                attrs.quote_depth,
                attrs.border_style,
            );
            let current_line = write_styled_and_wrapped(
                writer,
                &settings.terminal_capabilities,
                &settings.theme.code_style.on_top_of(&attrs.style),
                settings.terminal_size.columns,
                attrs.indent,
                data.current_line,
                code,
                &prefix,
                prefix_cols,
            )?;
            let data = StateData {
                current_line,
                ..data
            };
            Ok(stack.current(Inline(state, attrs)).and_data(data))
        }

        (Stacked(stack, Inline(state, attrs)), InlineHtml(html)) => {
            let (prefix, prefix_cols) = quote_line_prefix(
                &settings.terminal_capabilities,
                &settings.theme,
                attrs.quote_depth,
                attrs.border_style,
            );
            let current_line = write_styled_and_wrapped(
                writer,
                &settings.terminal_capabilities,
                &settings.theme.inline_html_style.on_top_of(&attrs.style),
                settings.terminal_size.columns,
                attrs.indent,
                data.current_line,
                html,
                &prefix,
                prefix_cols,
            )?;
            let data = StateData {
                current_line,
                ..data
            };
            Ok(stack.current(Inline(state, attrs)).and_data(data))
        }
        (Stacked(stack, Inline(inline, attrs)), TaskListMarker(checked)) => {
            let marker = if checked { "[✓]" } else { "[ ]" };
            write_styled(
                writer,
                &settings.terminal_capabilities,
                &attrs.style,
                marker,
            )?;
            let length = data.current_line.length + display_width(marker) as u16;
            Ok(stack
                .current(Inline(inline, attrs))
                .and_data(data.current_line(CurrentLine {
                    length,
                    trailing_space: Some(" ".to_owned()),
                })))
        }
        // Inline line breaks
        (Stacked(stack, Inline(state, attrs)), SoftBreak) => {
            let length = data.current_line.length;

            Ok(stack
                .current(Inline(state, attrs))
                .and_data(data.current_line(CurrentLine {
                    length,
                    trailing_space: Some(" ".to_owned()),
                })))
        }
        (Stacked(stack, Inline(state, attrs)), HardBreak) => {
            writeln!(writer)?;
            let (prefix, _) = quote_line_prefix(
                &settings.terminal_capabilities,
                &settings.theme,
                attrs.quote_depth,
                attrs.border_style,
            );
            write_indent(writer, attrs.indent)?;
            write!(writer, "{}", prefix)?;

            Ok(stack
                .current(Inline(state, attrs))
                .and_data(data.current_line(CurrentLine::empty())))
        }
        // Inline text
        (Stacked(stack, Inline(ListItem(kind, ItemBlock), attrs)), Text(text)) => {
            // Fresh text after a new block, so indent again.
            let (prefix, prefix_cols) = quote_line_prefix(
                &settings.terminal_capabilities,
                &settings.theme,
                attrs.quote_depth,
                attrs.border_style,
            );
            write!(writer, "{}", prefix)?;
            write_indent(writer, attrs.indent)?;
            let current_line = write_styled_and_wrapped(
                writer,
                &settings.terminal_capabilities,
                &attrs.style,
                settings.terminal_size.columns,
                attrs.indent,
                data.current_line,
                text,
                &prefix,
                prefix_cols,
            )?;
            Ok(stack
                .current(Inline(ListItem(kind, ItemText), attrs))
                .and_data(StateData {
                    current_line,
                    ..data
                }))
        }
        // Inline blocks don't wrap
        (Stacked(stack, Inline(InlineBlock, attrs)), Text(text)) => {
            write_styled(writer, &settings.terminal_capabilities, &attrs.style, text)?;
            Ok(stack.current(Inline(InlineBlock, attrs)).and_data(data))
        }
        (Stacked(stack, Inline(state, attrs)), Text(text)) => {
            let (prefix, prefix_cols) = quote_line_prefix(
                &settings.terminal_capabilities,
                &settings.theme,
                attrs.quote_depth,
                attrs.border_style,
            );
            let current_line = write_styled_and_wrapped(
                writer,
                &settings.terminal_capabilities,
                &attrs.style,
                settings.terminal_size.columns,
                attrs.indent,
                data.current_line,
                text,
                &prefix,
                prefix_cols,
            )?;
            Ok(stack.current(Inline(state, attrs)).and_data(StateData {
                current_line,
                ..data
            }))
        }
        // Ending inline text
        (Stacked(stack, Inline(_, _)), End(TagEnd::Paragraph)) => {
            writeln!(writer)?;
            Ok(stack
                .pop()
                .and_data(data.current_line(CurrentLine::empty())))
        }
        (Stacked(stack, Inline(_, _)), End(TagEnd::Heading(HeadingLevel::H1))) => {
            write_styled(
                writer,
                &settings.terminal_capabilities,
                &settings.theme.h1_prefix_style,
                " ",
            )?;
            writeln!(writer)?;
            writeln!(writer)?;
            Ok(stack
                .pop()
                .and_data(data.current_line(CurrentLine::empty())))
        }
        (Stacked(stack, Inline(_, _)), End(TagEnd::Heading(_))) => {
            writeln!(writer)?;
            Ok(stack
                .pop()
                .and_data(data.current_line(CurrentLine::empty())))
        }

        // Links.
        //
        // Links need a bit more work than standard inline markup because we
        // need to keep track of link references if we can't write inline links.
        (
            Stacked(stack, Inline(state, attrs)),
            Start(Link {
                link_type,
                dest_url,
                title,
                ..
            }),
        ) => {
            let maybe_link = settings
                .terminal_capabilities
                .style
                .filter(|s| *s == StyleCapability::Ansi)
                .and_then(|_| {
                    if let LinkType::Email = link_type {
                        // Turn email autolinks (i.e. <foo@example.com>) into mailto inline links
                        Url::parse(&format!("mailto:{dest_url}")).ok()
                    } else {
                        environment.resolve_reference(&dest_url)
                    }
                });

            let (link_state, data) = match maybe_link {
                None => (
                    InlineText,
                    data.push_pending_link(link_type, dest_url, title),
                ),
                Some(url) => {
                    let data = match data.current_line.trailing_space.as_ref() {
                        Some(space) => {
                            // Flush trailing space before starting a link
                            write!(writer, "{}", space)?;
                            let length = data.current_line.length + 1;
                            data.current_line(CurrentLine {
                                length,
                                trailing_space: None,
                            })
                        }
                        None => data,
                    };
                    set_link_url(writer, url, &environment.hostname)?;
                    (InlineLink, data)
                }
            };

            let InlineAttrs {
                style,
                indent,
                quote_depth,
                border_style,
            } = attrs;
            stack
                .push(Inline(state, attrs))
                .current(Inline(
                    link_state,
                    InlineAttrs {
                        indent,
                        style: settings.theme.link_style.on_top_of(&style),
                        quote_depth,
                        border_style,
                    },
                ))
                .and_data(data)
                .ok()
        }
        (Stacked(stack, Inline(InlineText, attrs)), End(TagEnd::Link)) => {
            let (data, link) = data.pop_pending_link();
            match link.link_type {
                LinkType::Autolink | LinkType::Email => {
                    // When closing email or autolinks in inline text just return because link, being identical
                    // to the link text, was already written.
                    stack.pop().and_data(data).ok()
                }
                _ => {
                    let (data, index) = data.add_link_reference(
                        link.dest_url,
                        link.title,
                        settings.theme.link_style,
                    );
                    write_styled(
                        writer,
                        &settings.terminal_capabilities,
                        &settings.theme.link_style.on_top_of(&attrs.style),
                        format!("[{index}]"),
                    )?;
                    stack.pop().and_data(data).ok()
                }
            }
        }

        // Images
        (
            Stacked(stack, Inline(state, attrs)),
            Start(Image {
                dest_url,
                title,
                link_type,
                ..
            }),
        ) => {
            let InlineAttrs {
                style,
                indent,
                quote_depth,
                border_style,
            } = attrs;
            let resolved_link = environment.resolve_reference(&dest_url);
            let image_state = match (settings.terminal_capabilities.image, resolved_link) {
                (Some(capability), Some(ref url)) => capability
                    .image_protocol()
                    .write_inline_image(writer, &resource_handler, url, settings.terminal_size)
                    .map_err(|error| {
                        event!(Level::ERROR, ?error, %url, "failed to render image with capability {:?}: {:#}", capability, error);
                        error
                    })
                    .map(|_| RenderedImage)
                    .ok(),
                (None, Some(url)) =>
                    if let InlineLink = state {
                        event!(Level::WARN, url = %url, "Terminal does not support images, want to render image as link but cannot: Already inside a link");
                        None
                    } else {
                        event!(Level::INFO, url = %url, "Terminal does not support images, rendering image as link");
                        match settings.terminal_capabilities.style {
                            Some(StyleCapability::Ansi) => {
                                set_link_url(writer, url, &environment.hostname)?;
                                Some(Inline(
                                    InlineLink,
                                    InlineAttrs {
                                        indent,
                                        style: settings.theme.image_link_style.on_top_of(&style),
                                        quote_depth,
                                        border_style,
                                    },
                                ))
                            },
                            None => None,
                        }
                    },
                (_, None) => None,
            };

            let (image_state, data) = match image_state {
                Some(state) => (state, data),
                None => {
                    event!(
                        Level::WARN,
                        "Rendering image {} as inline text, without link",
                        dest_url
                    );
                    // Inside an inline link keep the link style; we cannot nest links so we
                    // should clarify that clicking the link follows the link target and not the image.
                    let style = if let InlineLink = state {
                        style
                    } else {
                        settings.theme.image_link_style.on_top_of(&style)
                    };
                    let state = Inline(
                        InlineText,
                        InlineAttrs {
                            style,
                            indent,
                            quote_depth,
                            border_style,
                        },
                    );
                    (state, data.push_pending_link(link_type, dest_url, title))
                }
            };
            stack
                .push(Inline(state, attrs))
                .current(image_state)
                .and_data(data)
                .ok()
        }
        // To correctly handle nested images in the image description, we push a dummy rendered
        // image state so to maintain a correct state stack at the end of image event, where the
        // tail of the stack gets popped.
        (Stacked(stack, RenderedImage), Start(Image { .. })) => stack
            .push(RenderedImage)
            .current(RenderedImage)
            .and_data(data)
            .ok(),
        (Stacked(stack, RenderedImage), End(TagEnd::Image)) => stack.pop().and_data(data).ok(),
        // Immediately after the start of image event comes the alt text, which we do not support
        // for rendered images. So we just ignore all events other than image events, which are
        // handled above.
        //
        // See also https://docs.rs/pulldown-cmark/0.9.6/src/pulldown_cmark/html.rs.html#280-290 for
        // how the upstream handles images.
        (Stacked(stack, RenderedImage), _) => Stacked(stack, RenderedImage).and_data(data).ok(),
        (Stacked(stack, Inline(InlineText, attrs)), End(TagEnd::Image)) => {
            let (data, link) = data.pop_pending_link();
            let (data, index) =
                data.add_link_reference(link.dest_url, link.title, settings.theme.image_link_style);
            write_styled(
                writer,
                &settings.terminal_capabilities,
                // Regardless of text style always colour the reference to make clear it points to
                // an image
                &settings.theme.image_link_style.on_top_of(&attrs.style),
                format!("[{index}]"),
            )?;
            stack.pop().and_data(data).ok()
        }

        // End any kind of inline link, either a proper link, or an image written out as inline link
        (Stacked(stack, Inline(InlineLink, _)), End(TagEnd::Link | TagEnd::Image)) => {
            clear_link(writer)?;
            stack.pop().and_data(data).ok()
        }

        // Tables
        (Stacked(stack, TableBlock(attrs)), Start(TableHead))
        | (Stacked(stack, TableBlock(attrs)), Start(TableRow))
        | (Stacked(stack, TableBlock(attrs)), Start(TableCell)) => {
            Stacked(stack, TableBlock(attrs)).and_data(data).ok()
        }
        (Stacked(stack, TableBlock(attrs)), End(TagEnd::TableHead)) => {
            let current_table = data.current_table.end_head();
            let data = StateData {
                current_table,
                ..data
            };
            Stacked(stack, TableBlock(attrs)).and_data(data).ok()
        }
        (Stacked(stack, TableBlock(attrs)), End(TagEnd::TableRow)) => {
            let current_table = data.current_table.end_row();
            let data = StateData {
                current_table,
                ..data
            };
            Stacked(stack, TableBlock(attrs)).and_data(data).ok()
        }
        (Stacked(stack, TableBlock(attrs)), End(TagEnd::TableCell)) => {
            let current_table = data.current_table.end_cell();
            let data = StateData {
                current_table,
                ..data
            };
            Stacked(stack, TableBlock(attrs)).and_data(data).ok()
        }
        (Stacked(stack, TableBlock(attrs)), Text(text)) => {
            let current_table = data.current_table.push_text(text);
            let data = StateData {
                current_table,
                ..data
            };
            Stacked(stack, TableBlock(attrs)).and_data(data).ok()
        }
        (Stacked(stack, TableBlock(attrs)), Code(text)) => {
            let current_table = data
                .current_table
                .push_styled_text(text, settings.theme.code_style);
            let data = StateData {
                current_table,
                ..data
            };
            Stacked(stack, TableBlock(attrs)).and_data(data).ok()
        }
        (Stacked(stack, TableBlock(attrs)), End(TagEnd::Table)) => {
            let (prefix, prefix_cols) = quote_line_prefix(
                &settings.terminal_capabilities,
                &settings.theme,
                attrs.quote_depth,
                attrs.border_style,
            );
            write_table(
                writer,
                &settings.terminal_capabilities,
                &settings.terminal_size,
                attrs.indent,
                &prefix,
                prefix_cols,
                data.current_table,
            )?;
            let current_table = data::CurrentTable::empty();
            let data = StateData {
                current_table,
                ..data
            };
            stack.pop().and_data(data).ok()
        }
        (Stacked(stack, TableBlock(attrs)), Start(Strong)) => {
            let current_table = data.current_table.enter_strong();
            Stacked(stack, TableBlock(attrs))
                .and_data(StateData {
                    current_table,
                    ..data
                })
                .ok()
        }
        (Stacked(stack, TableBlock(attrs)), End(TagEnd::Strong)) => {
            let current_table = data.current_table.exit_strong();
            Stacked(stack, TableBlock(attrs))
                .and_data(StateData {
                    current_table,
                    ..data
                })
                .ok()
        }
        (Stacked(stack, TableBlock(attrs)), Start(Emphasis)) => {
            let current_table = data.current_table.enter_emphasis();
            Stacked(stack, TableBlock(attrs))
                .and_data(StateData {
                    current_table,
                    ..data
                })
                .ok()
        }
        (Stacked(stack, TableBlock(attrs)), End(TagEnd::Emphasis)) => {
            let current_table = data.current_table.exit_emphasis();
            Stacked(stack, TableBlock(attrs))
                .and_data(StateData {
                    current_table,
                    ..data
                })
                .ok()
        }
        (Stacked(stack, TableBlock(attrs)), Start(Strikethrough)) => {
            let current_table = data.current_table.enter_strikethrough();
            Stacked(stack, TableBlock(attrs))
                .and_data(StateData {
                    current_table,
                    ..data
                })
                .ok()
        }
        (Stacked(stack, TableBlock(attrs)), End(TagEnd::Strikethrough)) => {
            let current_table = data.current_table.exit_strikethrough();
            Stacked(stack, TableBlock(attrs))
                .and_data(StateData {
                    current_table,
                    ..data
                })
                .ok()
        }
        (Stacked(stack, TableBlock(attrs)), Start(Link { .. })) => {
            let current_table = data.current_table.enter_span(settings.theme.link_style);
            Stacked(stack, TableBlock(attrs))
                .and_data(StateData {
                    current_table,
                    ..data
                })
                .ok()
        }
        (Stacked(stack, TableBlock(attrs)), Start(Image { .. })) => {
            let current_table = data
                .current_table
                .enter_span(settings.theme.image_link_style);
            Stacked(stack, TableBlock(attrs))
                .and_data(StateData {
                    current_table,
                    ..data
                })
                .ok()
        }
        (Stacked(stack, TableBlock(attrs)), End(TagEnd::Link | TagEnd::Image)) => {
            let current_table = data.current_table.exit_span();
            Stacked(stack, TableBlock(attrs))
                .and_data(StateData {
                    current_table,
                    ..data
                })
                .ok()
        }
        (Stacked(stack, TableBlock(attrs)), InlineHtml(text)) => {
            let current_table = data
                .current_table
                .push_styled_text(text, settings.theme.inline_html_style);
            Stacked(stack, TableBlock(attrs))
                .and_data(StateData {
                    current_table,
                    ..data
                })
                .ok()
        }

        // Inline math
        (Stacked(stack, Inline(state, attrs)), InlineMath(math)) => {
            if let Some(img) = render_math_image(settings, &math, false) {
                let mut length = data.current_line.length;
                if let Some(space) = data.current_line.trailing_space.as_ref() {
                    write!(writer, "{}", space)?;
                    length += display_width(space.as_ref()) as u16;
                }

                // Add paragraph indent when starting at column 0, so inline math
                // images align with display math indentation.
                if length == 0 && attrs.indent > 0 {
                    write_indent(writer, attrs.indent)?;
                    length = attrs.indent;
                }

                let img_cols = match settings.terminal_capabilities.image.as_ref() {
                    Some(ImageCapability::Kitty(_)) => {
                        // Kitty with move_cursor=false places the image at the cursor
                        // without affecting cursor position or causing line displacement,
                        // so no pre-allocation is needed. Pre-allocating newlines causes
                        // terminal scrolling which shifts the image onto the wrong line.
                        write!(writer, "\x1b7")?;
                        let (cols, _, _) = write_math_image(writer, settings, img, false)?;
                        write!(writer, "\x1b8")?;
                        if cols > 0 {
                            write!(writer, "\x1b[{}C", cols)?;
                        }
                        cols
                    }
                    _ => {
                        // For iTerm2 and Sixel, pre-allocate vertical space below the
                        // current line so the image doesn't overwrite subsequent content.
                        for _ in 0..img.height_rows {
                            writeln!(writer)?;
                        }
                        write!(writer, "\x1b[{}A", img.height_rows)?;
                        write!(writer, "\x1b[{}G", u32::from(length) + 1)?;
                        write!(writer, "\x1b7")?;
                        let (cols, _, _) = write_math_image(writer, settings, img, false)?;
                        write!(writer, "\x1b8")?;
                        if cols > 0 {
                            write!(writer, "\x1b[{}C", cols)?;
                        }
                        cols
                    }
                };
                return Ok(stack
                    .current(Inline(state, attrs))
                    .and_data(data.current_line(CurrentLine {
                        length: length + img_cols,
                        trailing_space: None,
                    })));
            }
            let rendered = math::render_math_unicode(&math);
            let (prefix, prefix_cols) = quote_line_prefix(
                &settings.terminal_capabilities,
                &settings.theme,
                attrs.quote_depth,
                attrs.border_style,
            );
            let current_line = write_styled_and_wrapped(
                writer,
                &settings.terminal_capabilities,
                &settings.theme.math_style.on_top_of(&attrs.style),
                settings.terminal_size.columns,
                attrs.indent,
                data.current_line,
                &rendered,
                &prefix,
                prefix_cols,
            )?;
            Ok(stack.current(Inline(state, attrs)).and_data(StateData {
                current_line,
                ..data
            }))
        }
        // Display math as an indented block
        (TopLevel(attrs), DisplayMath(math)) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            if let Some(img) = render_math_image(settings, &math, true) {
                let (_, _, cursor_moved) = write_math_image(writer, settings, img, true)?;
                if !cursor_moved {
                    writeln!(writer)?;
                }
                return TopLevel(TopLevelAttrs::margin_before()).and_data(data).ok();
            }
            let rendered = math::render_math_unicode(&math);
            write_indent(writer, 4)?;
            write_styled(
                writer,
                &settings.terminal_capabilities,
                &settings.theme.math_style,
                rendered,
            )?;
            writeln!(writer)?;
            TopLevel(TopLevelAttrs::margin_before()).and_data(data).ok()
        }
        (Stacked(stack, StyledBlock(attrs)), DisplayMath(math)) => {
            if attrs.margin_before != NoMargin {
                writeln!(writer)?;
            }
            if let Some(img) = render_math_image(settings, &math, true) {
                let (_, _, cursor_moved) = write_math_image(writer, settings, img, true)?;
                if !cursor_moved {
                    writeln!(writer)?;
                }
                return stack
                    .current(attrs.with_margin_before().into())
                    .and_data(data)
                    .ok();
            }
            let rendered = math::render_math_unicode(&math);
            write_indent(writer, attrs.indent + 4)?;
            write_styled(
                writer,
                &settings.terminal_capabilities,
                &settings.theme.math_style.on_top_of(&attrs.style),
                rendered,
            )?;
            writeln!(writer)?;
            stack
                .current(attrs.with_margin_before().into())
                .and_data(data)
                .ok()
        }

        // Display math inside inline context (e.g. paragraph)
        (Stacked(stack, Inline(state, attrs)), DisplayMath(math)) => {
            writeln!(writer)?;
            if let Some(img) = render_math_image(settings, &math, true) {
                let (_, _, cursor_moved) = write_math_image(writer, settings, img, true)?;
                if !cursor_moved {
                    writeln!(writer)?;
                }
                return Ok(stack
                    .current(Inline(state, attrs))
                    .and_data(data.current_line(CurrentLine::empty())));
            }
            let rendered = math::render_math_unicode(&math);
            write_indent(writer, attrs.indent + 4)?;
            write_styled(
                writer,
                &settings.terminal_capabilities,
                &settings.theme.math_style.on_top_of(&attrs.style),
                rendered,
            )?;
            writeln!(writer)?;
            Ok(stack
                .current(Inline(state, attrs))
                .and_data(data.current_line(CurrentLine::empty())))
        }

        // Unconditional returns to previous states
        (
            Stacked(stack, _),
            End(
                TagEnd::BlockQuote(_)
                | TagEnd::List(_)
                | TagEnd::HtmlBlock
                | TagEnd::DefinitionList,
            ),
        ) => stack.pop().and_data(data).ok(),

        // Footnotes
        (state, Start(Tag::FootnoteDefinition(label))) => {
            let mut data = data;
            data.start_footnote_definition(&label);
            match state {
                TopLevel(attrs) => State::stack_onto(attrs)
                    .current(FootnoteDefinition)
                    .and_data(data)
                    .ok(),
                Stacked(stack, current) => stack
                    .push(current)
                    .current(FootnoteDefinition)
                    .and_data(data)
                    .ok(),
            }
        }
        (Stacked(stack, FootnoteDefinition), End(TagEnd::FootnoteDefinition)) => {
            let mut data = data;
            data.end_footnote_definition();
            stack.pop().and_data(data).ok()
        }
        (Stacked(stack, FootnoteDefinition), Text(text)) => {
            let mut data = data;
            data.append_footnote_text(&text);
            Stacked(stack, FootnoteDefinition).and_data(data).ok()
        }
        (Stacked(stack, FootnoteDefinition), SoftBreak) => {
            let mut data = data;
            data.append_footnote_text(" ");
            Stacked(stack, FootnoteDefinition).and_data(data).ok()
        }
        (Stacked(stack, FootnoteDefinition), HardBreak) => {
            let mut data = data;
            data.append_footnote_text("\n");
            Stacked(stack, FootnoteDefinition).and_data(data).ok()
        }
        (Stacked(stack, FootnoteDefinition), Code(code)) => {
            let mut data = data;
            data.append_footnote_text(&code);
            Stacked(stack, FootnoteDefinition).and_data(data).ok()
        }
        // Ignore structural events inside footnote definitions (paragraphs, etc.)
        (Stacked(stack, FootnoteDefinition), Start(_) | End(_)) => {
            Stacked(stack, FootnoteDefinition).and_data(data).ok()
        }
        (Stacked(stack, FootnoteDefinition), _) => {
            Stacked(stack, FootnoteDefinition).and_data(data).ok()
        }
        (Stacked(stack, Inline(state, attrs)), FootnoteReference(label)) => {
            let mut data = data;
            let idx = data.footnote_index(&label);
            write_styled(
                writer,
                &settings.terminal_capabilities,
                &settings.theme.footnote_style,
                format!("[{idx}]"),
            )?;
            Stacked(stack, Inline(state, attrs)).and_data(data).ok()
        }

        // Impossible events
        (s, e) => panic!("Event {e:?} impossible in state {s:?}"),
    }
}

#[instrument(level = "trace", skip(writer, settings, environment))]
pub fn finish<'a, W: Write>(
    writer: &mut W,
    settings: &Settings,
    environment: &Environment,
    state: State,
    data: StateData<'a>,
) -> Result<()> {
    match state {
        State::TopLevel(_) => {
            event!(
                Level::TRACE,
                "Writing {} pending link definitions",
                data.pending_link_definitions.len()
            );
            write_link_refs(
                writer,
                environment,
                &settings.terminal_capabilities,
                data.pending_link_definitions,
            )?;
            if !data.footnote_definitions.is_empty() {
                let mut defs = data.footnote_definitions;
                defs.sort_by_key(|(idx, _)| *idx);
                writeln!(writer)?;
                for (idx, body) in &defs {
                    let body = body.trim();
                    write_styled(
                        writer,
                        &settings.terminal_capabilities,
                        &settings.theme.footnote_style,
                        format!("[{idx}]: {body}"),
                    )?;
                    writeln!(writer)?;
                }
            }
            Ok(())
        }
        _ => {
            panic!("Must finish in state TopLevel but got: {state:?}");
        }
    }
}
