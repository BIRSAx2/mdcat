// Copyright Mouhieddine Sabir <me@mouhieddine.dev>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Render markdown into Ratatui widgets and text.
//!
//! Markdown is rendered through mdcat's terminal renderer with ANSI styling, image and mark
//! protocols disabled. The resulting SGR sequences are converted to [`ratatui::text::Text`]. OSC
//! hyperlinks become visible reference links because Ratatui spans do not carry link targets.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::io::{Error, ErrorKind, Read, Result};
use std::sync::OnceLock;

use ::ratatui::buffer::Buffer;
use ::ratatui::layout::Rect;
use ::ratatui::style::{Color, Modifier, Style};
use ::ratatui::text::{Line, Span, Text};
use ::ratatui::widgets::{Block, Paragraph, StatefulWidget, Widget};
pub use ::ratatui_image::picker::{
    Capability as ImageCapability, Picker as ImagePicker, ProtocolType as ImageProtocol,
};
use ::ratatui_image::protocol::StatefulProtocol;
use ::ratatui_image::{FontSize, StatefulImage};
use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};
use syntect::parsing::SyntaxSet;
use url::Url;

use crate::references::UrlBase;
use crate::render::math;
use crate::resources::{NoopResourceHandler, ResourceUrlHandler};
use crate::terminal::PixelSize;
use crate::{markdown_options, strip_frontmatter, Environment, Settings, TerminalProgram};
use crate::{TerminalSize, Theme};

static DEFAULT_SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static NOOP_RESOURCE_HANDLER: NoopResourceHandler = NoopResourceHandler;

fn default_syntax_set() -> &'static SyntaxSet {
    DEFAULT_SYNTAX_SET.get_or_init(two_face::syntax::extra_newlines)
}

/// How Ratatui image support should be configured.
///
/// Ratatui's backend trait does not expose terminal graphics capabilities. Use
/// [`detect_image_picker`] after entering the alternate screen if you want mdcat to probe stdio,
/// or pass a preconfigured [`ImagePicker`] explicitly.
#[derive(Debug, Clone, Default)]
pub enum ImageMode {
    /// Render markdown images as text/link fallbacks.
    TextOnly,
    /// No image picker configured; callers may call [`MdcatWidgetState::detect_images`] to probe
    /// stdio and enable image rendering.
    #[default]
    Auto,
    /// Use an already configured image picker.
    Picker(ImagePicker),
}

/// Rendering options for Ratatui output.
#[derive(Clone)]
pub struct RenderOptions<'a> {
    columns: u16,
    syntax_set: &'a SyntaxSet,
    theme: Theme,
    syntax_theme: Option<syntect::highlighting::Theme>,
    environment: Option<Environment>,
    resource_handler: &'a dyn ResourceUrlHandler,
    image_mode: ImageMode,
}

impl Default for RenderOptions<'static> {
    fn default() -> Self {
        Self {
            columns: TerminalSize::default().columns,
            syntax_set: default_syntax_set(),
            theme: Theme::default(),
            syntax_theme: None,
            environment: None,
            resource_handler: &NOOP_RESOURCE_HANDLER,
            image_mode: ImageMode::default(),
        }
    }
}

impl<'a> RenderOptions<'a> {
    /// Set the render width in terminal columns.
    pub fn width(mut self, columns: u16) -> Self {
        self.columns = columns;
        self
    }

    /// Return the configured render width in terminal columns.
    pub fn columns(&self) -> u16 {
        self.columns
    }

    /// Use a custom syntax set for fenced code blocks.
    pub fn syntax_set(mut self, syntax_set: &'a SyntaxSet) -> Self {
        self.syntax_set = syntax_set;
        self
    }

    /// Use a custom mdcat color theme.
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Use a custom syntect theme for fenced code blocks.
    pub fn syntax_theme(mut self, syntax_theme: Option<syntect::highlighting::Theme>) -> Self {
        self.syntax_theme = syntax_theme;
        self
    }

    /// Use a custom markdown environment for resolving relative links and resources.
    pub fn environment(mut self, environment: Environment) -> Self {
        self.environment = Some(environment);
        self
    }

    /// Use a custom resource handler for linked resources.
    pub fn resource_handler(mut self, resource_handler: &'a dyn ResourceUrlHandler) -> Self {
        self.resource_handler = resource_handler;
        self
    }

    /// Configure image handling for Ratatui widgets.
    pub fn images(mut self, image_mode: ImageMode) -> Self {
        self.image_mode = image_mode;
        self
    }
}

/// Markdown renderer for Ratatui output.
#[derive(Clone)]
pub struct Renderer<'a> {
    options: RenderOptions<'a>,
}

impl Default for Renderer<'static> {
    fn default() -> Self {
        Self::new(RenderOptions::default())
    }
}

impl<'a> Renderer<'a> {
    /// Create a renderer from explicit options.
    pub fn new(options: RenderOptions<'a>) -> Self {
        Self { options }
    }

    /// Return this renderer's options.
    pub fn options(&self) -> &RenderOptions<'a> {
        &self.options
    }

    /// Return the explicitly configured image picker, if any.
    pub fn configured_image_picker(&self) -> Option<ImagePicker> {
        match &self.options.image_mode {
            ImageMode::Picker(picker) => Some(picker.clone()),
            ImageMode::TextOnly | ImageMode::Auto => None,
        }
    }

    fn settings(&self, columns: u16) -> Settings<'_> {
        Settings {
            terminal_capabilities: TerminalProgram::Ansi.capabilities(),
            terminal_size: TerminalSize {
                columns,
                ..TerminalSize::default()
            },
            syntax_set: self.options.syntax_set,
            theme: self.options.theme.clone(),
            syntax_theme: self.options.syntax_theme.clone(),
        }
    }

    fn environment(&self) -> Result<Cow<'_, Environment>> {
        match &self.options.environment {
            Some(environment) => Ok(Cow::Borrowed(environment)),
            None => {
                let current_dir = std::env::current_dir()?;
                Environment::for_local_directory(&current_dir).map(Cow::Owned)
            }
        }
    }

    /// Render a markdown string into Ratatui text.
    pub fn text_from_str(&self, markdown: &str) -> Result<Text<'static>> {
        let settings = self.settings(self.options.columns);
        let environment = self.environment()?;
        push_text_str(
            &settings,
            &environment,
            self.options.resource_handler,
            markdown,
        )
    }

    /// Render markdown read from `reader` into Ratatui text.
    pub fn text_from_read<R: Read>(&self, mut reader: R) -> Result<Text<'static>> {
        let mut markdown = String::new();
        reader.read_to_string(&mut markdown)?;
        self.text_from_str(&markdown)
    }

    fn text_from_str_at_width(&self, markdown: &str, columns: u16) -> Result<Text<'static>> {
        let settings = self.settings(columns);
        let environment = self.environment()?;
        push_text_str(
            &settings,
            &environment,
            self.options.resource_handler,
            markdown,
        )
    }
}

/// Probe stdio for Ratatui image support.
///
/// This delegates to `ratatui-image` and writes terminal query sequences. Call it after the TUI
/// enters the alternate screen and before the event loop starts reading input.
pub fn detect_image_picker() -> std::result::Result<ImagePicker, ::ratatui_image::errors::Errors> {
    ImagePicker::from_query_stdio()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    markdown_hash: u64,
    renderer_hash: u64,
    width: u16,
}

fn renderer_hash(options: &RenderOptions<'_>) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&options.theme, &mut hasher);
    if let Some(theme) = &options.syntax_theme {
        std::hash::Hash::hash(&theme.name, &mut hasher);
    }
    if let Some(environment) = &options.environment {
        std::hash::Hash::hash(environment.base_url.as_str(), &mut hasher);
    }
    match &options.image_mode {
        ImageMode::TextOnly => std::hash::Hash::hash(&0_u8, &mut hasher),
        ImageMode::Auto => std::hash::Hash::hash(&1_u8, &mut hasher),
        ImageMode::Picker(picker) => {
            std::hash::Hash::hash(&2_u8, &mut hasher);
            let FontSize { width, height } = picker.font_size();
            std::hash::Hash::hash(&width, &mut hasher);
            std::hash::Hash::hash(&height, &mut hasher);
            std::hash::Hash::hash(&(picker.protocol_type() as u8), &mut hasher);
        }
    }
    // Identify the syntax set by pointer — same set means same highlighting.
    std::hash::Hash::hash(&(options.syntax_set as *const _), &mut hasher);
    hasher.finish()
}

struct CachedDocument {
    key: CacheKey,
    text: Text<'static>,
    images: Vec<CachedImage>,
}

struct CachedImage {
    line: u16,
    indent: u16,
    width: u16,
    height: u16,
    protocol: StatefulProtocol,
}

/// State for [`MdcatWidget`].
#[derive(Default)]
pub struct MdcatWidgetState {
    scroll: u16,
    cache: Option<CachedDocument>,
    image_picker: Option<ImagePicker>,
}

impl MdcatWidgetState {
    /// Create empty widget state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the current vertical scroll offset.
    pub fn scroll(&self) -> u16 {
        self.scroll
    }

    /// Set the current vertical scroll offset.
    pub fn set_scroll(&mut self, scroll: u16) {
        self.scroll = scroll;
    }

    /// Scroll down by `amount` rows.
    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_add(amount);
    }

    /// Scroll up by `amount` rows.
    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    /// Clear the cached render output.
    pub fn clear_cache(&mut self) {
        self.cache = None;
    }

    /// Store an image picker for future image-aware rendering.
    pub fn set_image_picker(&mut self, picker: ImagePicker) {
        self.image_picker = Some(picker);
        self.clear_cache();
    }

    /// Probe stdio and store the detected image picker.
    pub fn detect_images(&mut self) -> std::result::Result<(), ::ratatui_image::errors::Errors> {
        self.image_picker = Some(detect_image_picker()?);
        self.clear_cache();
        Ok(())
    }

    /// Return the configured image picker.
    pub fn image_picker(&self) -> Option<&ImagePicker> {
        self.image_picker.as_ref()
    }

    /// Return the configured image protocol.
    pub fn image_protocol(&self) -> Option<ImageProtocol> {
        self.image_picker.as_ref().map(ImagePicker::protocol_type)
    }

    /// Return the total number of rendered lines in the cache, or 0 if nothing has been rendered yet.
    pub fn total_lines(&self) -> usize {
        self.cache.as_ref().map(|c| c.text.lines.len()).unwrap_or(0)
    }

    fn rendered_document<'s>(
        &'s mut self,
        renderer: &Renderer<'_>,
        markdown: &str,
        width: u16,
    ) -> Result<&'s mut CachedDocument> {
        let key = CacheKey {
            markdown_hash: markdown_hash(markdown),
            renderer_hash: renderer_hash(renderer.options()),
            width,
        };
        if self.cache.as_ref().is_none_or(|cache| cache.key != key) {
            let mut text = renderer.text_from_str_at_width(markdown, width)?;
            let images = match (
                matches!(renderer.options().image_mode, ImageMode::TextOnly),
                self.image_picker.as_ref(),
            ) {
                (false, Some(picker)) => renderer.image_overlays(markdown, picker, &mut text),
                _ => Vec::new(),
            };
            self.cache = Some(CachedDocument { key, text, images });
        }
        Ok(self
            .cache
            .as_mut()
            .expect("render cache was just populated"))
    }
}

impl CachedDocument {
    fn render_images(&mut self, content_area: Rect, scroll: u16, buf: &mut Buffer) {
        for image in &mut self.images {
            if image.line.saturating_add(image.height) <= scroll {
                continue;
            }
            if image.line >= scroll.saturating_add(content_area.height) {
                continue;
            }

            let y = content_area
                .y
                .saturating_add(image.line.saturating_sub(scroll));
            let x = content_area.x.saturating_add(image.indent);
            if y >= content_area.y.saturating_add(content_area.height)
                || x >= content_area.x.saturating_add(content_area.width)
            {
                continue;
            }

            let width = image
                .width
                .min(content_area.width.saturating_sub(image.indent));
            let height = image
                .height
                .min(content_area.y.saturating_add(content_area.height) - y);
            if width == 0 || height == 0 {
                continue;
            }

            StatefulImage::default().render(
                Rect {
                    x,
                    y,
                    width,
                    height,
                },
                buf,
                &mut image.protocol,
            );
        }
    }
}

impl Renderer<'_> {
    fn image_overlays(
        &self,
        markdown: &str,
        picker: &ImagePicker,
        text: &mut Text<'static>,
    ) -> Vec<CachedImage> {
        let assets = collect_markdown_assets(markdown);
        let mut overlays = Vec::new();

        if let Ok(environment) = self.environment() {
            overlays.extend(assets.images.into_iter().filter_map(|image| {
                let url = environment.resolve_reference(&image.target)?;
                let resource = self.options.resource_handler.read_resource(&url).ok()?;
                let decoded = image::load_from_memory(&resource.data).ok()?;
                let (line, indent, reference_index) = find_image_marker(text, &image.alt)?;
                let (width, height) = image_size_in_cells(&decoded, picker, self.options.columns);
                let line_u16: u16 = line.try_into().ok()?;
                replace_line_with_image_space(text, line, height);
                remove_reference_line(text, reference_index);
                Some(CachedImage {
                    line: line_u16,
                    indent,
                    width,
                    height,
                    protocol: picker.new_resize_protocol(decoded),
                })
            }));
        }

        let terminal_size = terminal_size_for_picker(picker, self.options.columns);
        overlays.extend(assets.math.into_iter().filter_map(|math| {
            let rendered = math::render_math_png(
                &math.latex,
                math.display_mode,
                &terminal_size,
                &self.options.theme.math_style,
            )?;
            let decoded = image::load_from_memory(&rendered.png).ok()?;
            let (line, start, indent) = find_text_marker(text, &math.fallback)?;
            let line_u16: u16 = line.try_into().ok()?;
            if math.display_mode {
                replace_line_with_image_space(text, line, rendered.height_rows);
            } else {
                replace_text_with_image_space(
                    text,
                    line,
                    start,
                    &math.fallback,
                    rendered.width_columns,
                );
            }
            Some(CachedImage {
                line: line_u16,
                indent,
                width: rendered.width_columns,
                height: rendered.height_rows,
                protocol: picker.new_resize_protocol(decoded),
            })
        }));

        overlays
    }
}

#[derive(Debug)]
struct MarkdownImage {
    target: String,
    alt: String,
}

#[derive(Debug)]
struct MarkdownMath {
    latex: String,
    fallback: String,
    display_mode: bool,
}

struct MarkdownAssets {
    images: Vec<MarkdownImage>,
    math: Vec<MarkdownMath>,
}

fn collect_markdown_assets(markdown: &str) -> MarkdownAssets {
    let parser = Parser::new_ext(strip_frontmatter(markdown), markdown_options(false));
    let mut images = Vec::new();
    let mut math = Vec::new();
    let mut current_image: Option<MarkdownImage> = None;
    for event in parser {
        match event {
            Event::Start(Tag::Image { dest_url, .. }) => {
                current_image = Some(MarkdownImage {
                    target: dest_url.to_string(),
                    alt: String::new(),
                });
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(image) = &mut current_image {
                    image.alt.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(image) = &mut current_image {
                    image.alt.push(' ');
                }
            }
            Event::End(TagEnd::Image) => {
                if let Some(image) = current_image.take() {
                    images.push(image);
                }
            }
            Event::InlineMath(math_text) => {
                let latex = math_text.to_string();
                math.push(MarkdownMath {
                    fallback: math::render_math_unicode(&latex),
                    latex,
                    display_mode: false,
                });
            }
            Event::DisplayMath(math_text) => {
                let latex = math_text.to_string();
                math.push(MarkdownMath {
                    fallback: math::render_math_unicode(&latex),
                    latex,
                    display_mode: true,
                });
            }
            _ => {}
        }
    }
    MarkdownAssets { images, math }
}

fn terminal_size_for_picker(picker: &ImagePicker, columns: u16) -> TerminalSize {
    let FontSize {
        width: font_width,
        height: font_height,
    } = picker.font_size();
    TerminalSize {
        columns,
        rows: TerminalSize::default().rows,
        pixels: None,
        cell: Some(PixelSize::from_xy((
            font_width.max(1).into(),
            font_height.max(1).into(),
        ))),
    }
}

fn image_size_in_cells(
    image: &image::DynamicImage,
    picker: &ImagePicker,
    max_width: u16,
) -> (u16, u16) {
    let FontSize {
        width: font_width,
        height: font_height,
    } = picker.font_size();
    let width = image.width().div_ceil(font_width.max(1).into());
    let height = image.height().div_ceil(font_height.max(1).into());
    (
        width.max(1).min(max_width.max(1).into()) as u16,
        height.max(1).min(u16::MAX.into()) as u16,
    )
}

fn find_image_marker(text: &Text<'_>, alt: &str) -> Option<(usize, u16, usize)> {
    text.lines
        .iter()
        .enumerate()
        .find_map(|(line_index, line)| {
            let line_text = line_text(line);
            let trimmed = line_text.trim_start();
            let prefix_len = line_text.len() - trimmed.len();
            let indent = textwrap::core::display_width(&line_text[..prefix_len])
                .try_into()
                .ok()?;
            let marker = trimmed.strip_prefix(alt)?;
            let marker = marker.strip_prefix('[')?;
            let (index, _) = marker.split_once(']')?;
            index
                .parse()
                .ok()
                .map(|reference_index| (line_index, indent, reference_index))
        })
}

fn find_text_marker(text: &Text<'_>, marker: &str) -> Option<(usize, usize, u16)> {
    text.lines
        .iter()
        .enumerate()
        .find_map(|(line_index, line)| {
            let line_text = line_text(line);
            let start = line_text.find(marker)?;
            let indent = textwrap::core::display_width(&line_text[..start])
                .try_into()
                .ok()?;
            Some((line_index, start, indent))
        })
}

fn replace_line_with_image_space(text: &mut Text<'static>, line: usize, height: u16) {
    let blanks = std::iter::repeat_with(|| Line::from(String::new())).take(height.max(1).into());
    text.lines.splice(line..=line, blanks);
}

fn replace_text_with_image_space(
    text: &mut Text<'static>,
    line: usize,
    start: usize,
    marker: &str,
    width: u16,
) {
    let end = start + marker.len();
    let replacement = " ".repeat(width.max(1).into());
    let mut cursor = 0;
    let mut inserted_replacement = false;
    let mut spans = Vec::with_capacity(text.lines[line].spans.len() + 1);

    for span in &text.lines[line].spans {
        let content = span.content.as_ref();
        let span_start = cursor;
        let span_end = span_start + content.len();
        cursor = span_end;

        if span_end <= start || span_start >= end {
            spans.push(span.clone());
            continue;
        }

        if start > span_start {
            spans.push(Span::styled(
                content[..start - span_start].to_owned(),
                span.style,
            ));
        }
        if !inserted_replacement {
            spans.push(Span::raw(replacement.clone()));
            inserted_replacement = true;
        }
        if end < span_end {
            spans.push(Span::styled(
                content[end - span_start..].to_owned(),
                span.style,
            ));
        }
    }

    if inserted_replacement {
        text.lines[line] = Line::from(spans);
    }
}

fn remove_reference_line(text: &mut Text<'static>, reference_index: usize) {
    let prefix = format!("[{reference_index}]: ");
    text.lines
        .retain(|line| !line_text(line).starts_with(&prefix));
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn markdown_hash(markdown: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    markdown.hash(&mut hasher);
    hasher.finish()
}

/// A stateful Ratatui widget that renders Markdown with mdcat.
pub struct MdcatWidget<'a, 'renderer> {
    markdown: Cow<'a, str>,
    renderer: Renderer<'renderer>,
    block: Option<Block<'a>>,
}

impl<'a> MdcatWidget<'a, 'static> {
    /// Create a widget with default renderer options.
    pub fn new(markdown: impl Into<Cow<'a, str>>) -> Self {
        Self::with_renderer(markdown, Renderer::default())
    }
}

impl<'a, 'renderer> MdcatWidget<'a, 'renderer> {
    /// Create a widget with an explicit renderer.
    pub fn with_renderer(markdown: impl Into<Cow<'a, str>>, renderer: Renderer<'renderer>) -> Self {
        Self {
            markdown: markdown.into(),
            renderer,
            block: None,
        }
    }

    /// Set the surrounding block.
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }
}

impl StatefulWidget for MdcatWidget<'_, '_> {
    type State = MdcatWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        if state.image_picker.is_none() {
            state.image_picker = self.renderer.configured_image_picker();
        }

        let content_area = self.block.as_ref().map_or(area, |block| block.inner(area));
        let width = content_area.width;
        let scroll = state.scroll;
        let document = state.rendered_document(&self.renderer, &self.markdown, width);
        let text = match document.as_ref() {
            Ok(document) => document.text.clone(),
            Err(error) => Text::from(format!("Failed to render markdown: {error}")),
        };
        let mut paragraph = Paragraph::new(text).scroll((scroll, 0));
        if let Some(block) = self.block {
            paragraph = paragraph.block(block);
        }
        paragraph.render(area, buf);
        if let Ok(document) = document {
            document.render_images(content_area, scroll, buf);
        }
    }
}

/// Render markdown events into Ratatui text.
///
/// This uses the theme, syntax set, syntax theme, and terminal width from `settings`.
/// `settings.terminal_capabilities` is always overridden with plain ANSI — terminal-specific
/// image protocols and jump-mark sequences are disabled because they would corrupt the
/// ANSI-to-ratatui conversion. Pass any value for that field; it is ignored.
pub fn push_text<'e, I>(
    settings: &Settings,
    environment: &Environment,
    resource_handler: &dyn ResourceUrlHandler,
    events: I,
) -> Result<Text<'static>>
where
    I: Iterator<Item = Event<'e>>,
{
    let tty_settings = Settings {
        terminal_capabilities: TerminalProgram::Ansi.capabilities(),
        terminal_size: settings.terminal_size,
        syntax_set: settings.syntax_set,
        theme: settings.theme.clone(),
        syntax_theme: settings.syntax_theme.clone(),
    };
    let events = events.collect::<Vec<_>>();
    let reference_links = collect_reference_links(&events);
    let mut output = Vec::new();
    crate::push_tty(
        &tty_settings,
        environment,
        resource_handler,
        &mut output,
        events.into_iter(),
    )?;
    let output = String::from_utf8(output).map_err(|error| {
        Error::new(
            ErrorKind::InvalidData,
            format!("mdcat rendered invalid UTF-8: {error}"),
        )
    })?;
    Ok(ansi_to_text_with_reference_links(&output, reference_links))
}

/// Render a markdown string into Ratatui text.
///
/// This strips frontmatter and parses `markdown` with the same pulldown-cmark extensions as the
/// mdcat CLI, then delegates to [`push_text`].
pub fn push_text_str(
    settings: &Settings,
    environment: &Environment,
    resource_handler: &dyn ResourceUrlHandler,
    markdown: &str,
) -> Result<Text<'static>> {
    let parser = Parser::new_ext(strip_frontmatter(markdown), markdown_options(false));
    push_text(settings, environment, resource_handler, parser)
}

/// Render a markdown string into Ratatui text with conservative defaults.
///
/// The output uses the default mdcat theme and bundled syntax definitions, wraps to `columns`, and
/// does not read linked resources. Use [`push_text_str`] if you need a custom theme, syntax set,
/// syntax theme, or local resource access.
pub fn text_from_str(markdown: &str, columns: u16) -> Result<Text<'static>> {
    Renderer::new(RenderOptions::default().width(columns)).text_from_str(markdown)
}

/// Render markdown read from `reader` into Ratatui text with conservative defaults.
pub fn text_from_read<R: Read>(reader: R, columns: u16) -> Result<Text<'static>> {
    Renderer::new(RenderOptions::default().width(columns)).text_from_read(reader)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CurrentStyle {
    fg: Option<Color>,
    bg: Option<Color>,
    modifiers: Modifier,
}

impl Default for CurrentStyle {
    fn default() -> Self {
        Self {
            fg: None,
            bg: None,
            modifiers: Modifier::empty(),
        }
    }
}

impl From<CurrentStyle> for Style {
    fn from(current: CurrentStyle) -> Self {
        let mut style = Style::default();
        if let Some(fg) = current.fg {
            style = style.fg(fg);
        }
        if let Some(bg) = current.bg {
            style = style.bg(bg);
        }
        style.add_modifier(current.modifiers)
    }
}

#[derive(Debug, Default)]
struct TextBuilder {
    lines: Vec<Line<'static>>,
    spans: Vec<Span<'static>>,
    buffer: String,
}

impl TextBuilder {
    fn push_char(&mut self, c: char) {
        self.buffer.push(c);
    }

    fn flush_span(&mut self, style: CurrentStyle) {
        if !self.buffer.is_empty() {
            self.spans.push(Span::styled(
                std::mem::take(&mut self.buffer),
                Style::from(style),
            ));
        }
    }

    fn push_styled_str(&mut self, text: impl AsRef<str>, style: CurrentStyle) {
        self.buffer.push_str(text.as_ref());
        self.flush_span(style);
    }

    fn push_newline(&mut self, style: CurrentStyle) {
        self.flush_span(style);
        self.lines.push(Line::from(std::mem::take(&mut self.spans)));
    }

    fn current_line_is_empty(&self) -> bool {
        self.buffer.is_empty() && self.spans.is_empty()
    }

    fn append_references(&mut self, style: CurrentStyle, references: Vec<LinkReference>) {
        if references.is_empty() {
            return;
        }

        self.flush_span(style);
        if !self.current_line_is_empty() {
            self.lines.push(Line::from(std::mem::take(&mut self.spans)));
        }
        self.lines.push(Line::from(Vec::new()));

        for reference in references {
            let style = reference.style;
            self.push_styled_str(format!("[{}]: ", reference.index), style);
            self.push_styled_str(reference.target, style);
            if !reference.title.is_empty() {
                self.push_styled_str(format!(" {}", reference.title), style);
            }
            self.lines.push(Line::from(std::mem::take(&mut self.spans)));
        }
    }

    fn finish(mut self, style: CurrentStyle) -> Text<'static> {
        self.flush_span(style);
        self.lines.push(Line::from(self.spans));
        Text::from(self.lines)
    }
}

#[derive(Debug)]
struct ActiveLink {
    target: String,
    text: String,
    style: CurrentStyle,
}

impl ActiveLink {
    fn new(target: String) -> Self {
        Self {
            target,
            text: String::new(),
            style: CurrentStyle::default(),
        }
    }

    fn push_char(&mut self, c: char, style: CurrentStyle) {
        self.text.push(c);
        if style != CurrentStyle::default() {
            self.style = style;
        }
    }

    fn needs_reference(&self) -> bool {
        let text = self.text.trim();
        if text == self.target || self.target.strip_prefix("mailto:") == Some(text) {
            return false;
        }
        if let (Ok(target), Ok(text)) = (Url::parse(&self.target), Url::parse(text)) {
            return target != text;
        }
        true
    }
}

#[derive(Debug)]
struct LinkReference {
    index: usize,
    target: String,
    title: String,
    style: CurrentStyle,
}

#[derive(Debug)]
struct QueuedLink {
    target: String,
    title: String,
}

fn collect_reference_links<'e>(events: &[Event<'e>]) -> VecDeque<QueuedLink> {
    let mut result = VecDeque::new();
    let mut link_depth: usize = 0;
    for event in events {
        match event {
            Event::Start(Tag::Link {
                link_type: LinkType::Autolink | LinkType::Email,
                ..
            }) => {
                link_depth += 1;
            }
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => {
                link_depth += 1;
                result.push_back(QueuedLink {
                    target: dest_url.to_string(),
                    title: title.to_string(),
                });
            }
            Event::End(TagEnd::Link) => {
                link_depth = link_depth.saturating_sub(1);
            }
            // Images outside links emit an OSC 8 link in ANSI mode. Without a queue entry,
            // the next link's entry would be consumed for this image instead, desynchronising
            // subsequent reference link lookups.
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) if link_depth == 0 && !dest_url.is_empty() => {
                result.push_back(QueuedLink {
                    target: dest_url.to_string(),
                    title: title.to_string(),
                });
            }
            _ => {}
        }
    }
    result
}

#[cfg(test)]
fn ansi_to_text(input: &str) -> Text<'static> {
    ansi_to_text_with_reference_links(input, VecDeque::new())
}

fn ansi_to_text_with_reference_links(
    input: &str,
    mut queued_links: VecDeque<QueuedLink>,
) -> Text<'static> {
    let mut builder = TextBuilder::default();
    let mut style = CurrentStyle::default();
    let mut active_link = None;
    let mut references = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                builder.flush_span(style);
                if let Some(osc) = parse_escape(&mut chars, &mut style) {
                    handle_osc(
                        &mut builder,
                        &mut active_link,
                        &mut references,
                        &mut queued_links,
                        &osc,
                    );
                }
            }
            '\n' => builder.push_newline(style),
            '\r' => {}
            _ => {
                if let Some(link) = &mut active_link {
                    link.push_char(c, style);
                }
                builder.push_char(c);
            }
        }
    }
    builder.append_references(style, references);
    builder.finish(style)
}

fn parse_escape<I>(chars: &mut std::iter::Peekable<I>, style: &mut CurrentStyle) -> Option<String>
where
    I: Iterator<Item = char>,
{
    match chars.next() {
        Some('[') => {
            parse_csi(chars, style);
            None
        }
        Some(']') => parse_osc(chars),
        Some(_) | None => None,
    }
}

fn parse_csi<I>(chars: &mut std::iter::Peekable<I>, style: &mut CurrentStyle)
where
    I: Iterator<Item = char>,
{
    let mut sequence = String::new();
    for c in chars.by_ref() {
        if ('@'..='~').contains(&c) {
            if c == 'm' {
                apply_sgr(style, &sequence);
            }
            break;
        }
        sequence.push(c);
    }
}

fn parse_osc<I>(chars: &mut std::iter::Peekable<I>) -> Option<String>
where
    I: Iterator<Item = char>,
{
    let mut command = String::new();
    while let Some(c) = chars.next() {
        match c {
            '\x07' => return Some(command),
            '\x1b' if chars.next_if_eq(&'\\').is_some() => return Some(command),
            _ => command.push(c),
        }
    }
    None
}

fn handle_osc(
    builder: &mut TextBuilder,
    active_link: &mut Option<ActiveLink>,
    references: &mut Vec<LinkReference>,
    queued_links: &mut VecDeque<QueuedLink>,
    command: &str,
) {
    let Some(target) = command.strip_prefix("8;;") else {
        return;
    };

    if target.is_empty() {
        if let Some(link) = active_link.take().filter(ActiveLink::needs_reference) {
            let index = references.len() + 1;
            let queued = queued_links.pop_front();
            builder.push_styled_str(format!("[{index}]"), link.style);
            references.push(LinkReference {
                index,
                target: queued
                    .as_ref()
                    .map_or_else(|| link.target.clone(), |link| link.target.clone()),
                title: queued.map_or_else(String::new, |link| link.title),
                style: link.style,
            });
        }
    } else {
        *active_link = Some(ActiveLink::new(target.to_owned()));
    }
}

fn apply_sgr(style: &mut CurrentStyle, sequence: &str) {
    let params = parse_sgr_params(sequence);
    let params = if params.is_empty() {
        vec![Some(0)]
    } else {
        params
    };
    let mut i = 0;
    while i < params.len() {
        let code = params[i].unwrap_or(0);
        match code {
            0 => *style = CurrentStyle::default(),
            1 => style.modifiers.insert(Modifier::BOLD),
            2 => style.modifiers.insert(Modifier::DIM),
            3 => style.modifiers.insert(Modifier::ITALIC),
            4 => style.modifiers.insert(Modifier::UNDERLINED),
            7 => style.modifiers.insert(Modifier::REVERSED),
            8 => style.modifiers.insert(Modifier::HIDDEN),
            9 => style.modifiers.insert(Modifier::CROSSED_OUT),
            22 => style.modifiers.remove(Modifier::BOLD | Modifier::DIM),
            23 => style.modifiers.remove(Modifier::ITALIC),
            24 => style.modifiers.remove(Modifier::UNDERLINED),
            27 => style.modifiers.remove(Modifier::REVERSED),
            28 => style.modifiers.remove(Modifier::HIDDEN),
            29 => style.modifiers.remove(Modifier::CROSSED_OUT),
            30..=37 => style.fg = Some(ansi_color(code, false)),
            39 => style.fg = None,
            40..=47 => style.bg = Some(ansi_color(code - 10, false)),
            49 => style.bg = None,
            90..=97 => style.fg = Some(ansi_color(code - 60, true)),
            100..=107 => style.bg = Some(ansi_color(code - 70, true)),
            38 | 48 => {
                if let Some((color, consumed)) = parse_extended_color(&params[i + 1..]) {
                    if code == 38 {
                        style.fg = Some(color);
                    } else {
                        style.bg = Some(color);
                    }
                    i += consumed;
                }
            }
            _ => {}
        }
        i += 1;
    }
}

fn parse_sgr_params(sequence: &str) -> Vec<Option<u16>> {
    sequence
        .split([';', ':'])
        .map(|part| {
            if part.is_empty() {
                None
            } else {
                part.parse().ok()
            }
        })
        .collect()
}

fn parse_extended_color(params: &[Option<u16>]) -> Option<(Color, usize)> {
    match params.first().copied().flatten()? {
        2 => {
            let mut start = 1;
            if params.get(start).is_some_and(Option::is_none) {
                start += 1;
            }
            let r = params.get(start).copied().flatten()?.try_into().ok()?;
            let g = params.get(start + 1).copied().flatten()?.try_into().ok()?;
            let b = params.get(start + 2).copied().flatten()?.try_into().ok()?;
            Some((Color::Rgb(r, g, b), start + 3))
        }
        5 => {
            let index = params.get(1).copied().flatten()?.try_into().ok()?;
            Some((Color::Indexed(index), 2))
        }
        _ => None,
    }
}

fn ansi_color(code: u16, bright: bool) -> Color {
    match (code, bright) {
        (30, false) => Color::Black,
        (31, false) => Color::Red,
        (32, false) => Color::Green,
        (33, false) => Color::Yellow,
        (34, false) => Color::Blue,
        (35, false) => Color::Magenta,
        (36, false) => Color::Cyan,
        (37, false) => Color::Gray,
        (30, true) => Color::DarkGray,
        (31, true) => Color::LightRed,
        (32, true) => Color::LightGreen,
        (33, true) => Color::LightYellow,
        (34, true) => Color::LightBlue,
        (35, true) => Color::LightMagenta,
        (36, true) => Color::LightCyan,
        (37, true) => Color::White,
        _ => Color::Reset,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use ::ratatui::backend::TestBackend;
    use ::ratatui::style::{Color, Modifier};
    use ::ratatui::widgets::Paragraph;
    use ::ratatui::Terminal;

    use crate::resources::FileResourceHandler;

    use super::*;

    fn text_content(text: &Text<'_>) -> String {
        text.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn buffer_line(buffer: &::ratatui::buffer::Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer.cell((x, y)).expect("cell in bounds").symbol())
            .collect::<String>()
    }

    #[test]
    fn converts_sgr_to_spans() {
        let text = ansi_to_text("hello \x1b[1;34mworld\x1b[0m\n");

        assert_eq!(text.lines.len(), 2);
        assert_eq!(text.lines[0].spans[0].content, "hello ");
        assert_eq!(text.lines[0].spans[0].style, Style::default());
        assert_eq!(text.lines[0].spans[1].content, "world");
        assert_eq!(text.lines[0].spans[1].style.fg, Some(Color::Blue));
        assert!(text.lines[0].spans[1]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn converts_osc_hyperlinks_to_reference_links() {
        let text = ansi_to_text("\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\");

        assert_eq!(text_content(&text), "link[1]\n\n[1]: https://example.com\n");
    }

    #[test]
    fn leaves_autolinks_without_redundant_reference() {
        let text =
            ansi_to_text("\x1b]8;;https://example.com\x1b\\https://example.com\x1b]8;;\x1b\\");

        assert_eq!(text_content(&text), "https://example.com");

        let text = text_from_str("<https://example.com>", 80).unwrap();

        assert_eq!(text_content(&text), "https://example.com\n");
    }

    #[test]
    fn renders_markdown_to_ratatui_text() {
        let text = text_from_str("Hello **world**", 80).unwrap();

        assert_eq!(text.lines[0].spans[0].content, "Hello");
        assert_eq!(text.lines[0].spans[1].content, " world");
        assert!(text.lines[0].spans[1]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn default_renderer_syntax_highlights_fenced_code() {
        let text = Renderer::default()
            .text_from_str("```rust\nfn main() -> u64 { 1 }\n```")
            .unwrap();
        let code_spans = text
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .filter(|span| {
                let content = span.content.as_ref();
                content.contains("fn") || content.contains("main") || content.contains("u64")
            })
            .collect::<Vec<_>>();

        assert!(!code_spans.is_empty());
        assert!(code_spans.iter().any(|span| span.style.fg.is_some()));
    }

    #[test]
    fn renders_markdown_from_reader_to_ratatui_text() {
        let text = text_from_read(Cursor::new("Hello **world**"), 80).unwrap();

        assert_eq!(text_content(&text), "Hello world\n");
        assert!(text.lines[0].spans[1]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn strips_frontmatter_before_rendering_markdown() {
        let text = text_from_str("---\ntitle: Example\n---\nHello", 80).unwrap();

        assert_eq!(text_content(&text), "Hello\n");
    }

    #[test]
    fn preserves_markdown_link_targets_as_references() {
        let text = text_from_str("[Rust](https://www.rust-lang.org/)", 80).unwrap();
        let rendered = text_content(&text);

        assert!(rendered.contains("Rust[1]"));
        assert!(rendered.contains("[1]: https://www.rust-lang.org/"));
    }

    #[test]
    fn uses_original_markdown_link_targets_in_references() {
        let text = text_from_str(r#"[local](docs/readme.md "Title")"#, 80).unwrap();
        let rendered = text_content(&text);

        assert!(rendered.contains("local[1]"));
        assert!(rendered.contains("[1]: docs/readme.md Title"));
    }

    #[test]
    fn accepts_zero_and_narrow_widths() {
        text_from_str("Hello **world**", 0).unwrap();
        text_from_str("- ---", 1).unwrap();
        text_from_str("- ```\n  code\n  ```", 1).unwrap();
    }

    #[test]
    fn renders_inside_ratatui_paragraph_widget() {
        let text = text_from_str("Hello **world**", 20).unwrap();
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new(text.clone()), frame.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer_line(buffer, 0).trim_end(), "Hello world");
        assert!(buffer
            .cell((6, 0))
            .expect("cell in bounds")
            .modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn renders_inside_mdcat_widget() {
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = MdcatWidgetState::new();

        terminal
            .draw(|frame| {
                frame.render_stateful_widget(
                    MdcatWidget::new("Hello **world**"),
                    frame.area(),
                    &mut state,
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer_line(buffer, 0).trim_end(), "Hello world");
        assert!(buffer
            .cell((6, 0))
            .expect("cell in bounds")
            .modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn mdcat_widget_cache_invalidates_when_markdown_changes() {
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = MdcatWidgetState::new();

        terminal
            .draw(|frame| {
                frame.render_stateful_widget(MdcatWidget::new("First"), frame.area(), &mut state);
            })
            .unwrap();
        terminal
            .draw(|frame| {
                frame.render_stateful_widget(MdcatWidget::new("Second"), frame.area(), &mut state);
            })
            .unwrap();

        assert_eq!(
            buffer_line(terminal.backend().buffer(), 0).trim_end(),
            "Second"
        );
    }

    #[test]
    fn mdcat_widget_renders_local_markdown_images() {
        let sample_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../sample")
            .canonicalize()
            .unwrap();
        let file_handler = FileResourceHandler::new(20_000_000);
        let environment = Environment::for_local_directory(&sample_dir).unwrap();
        let mut picker = ImagePicker::from_fontsize((10, 20));
        picker.set_protocol_type(ImageProtocol::Halfblocks);
        let renderer = Renderer::new(
            RenderOptions::default()
                .environment(environment)
                .resource_handler(&file_handler)
                .images(ImageMode::Picker(picker)),
        );
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = MdcatWidgetState::new();

        terminal
            .draw(|frame| {
                frame.render_stateful_widget(
                    MdcatWidget::with_renderer(
                        "![Rust](./rust-logo-128x128.png)\n\nAfter",
                        renderer,
                    ),
                    frame.area(),
                    &mut state,
                );
            })
            .unwrap();

        let rendered = (0..12)
            .map(|y| buffer_line(terminal.backend().buffer(), y))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(!rendered.contains("Rust[1]"));
        assert!(!rendered.contains("[1]:"));
        assert!(rendered.lines().take(8).any(|line| !line.trim().is_empty()));
        assert!(rendered.contains("After"));
    }

    #[test]
    fn mdcat_widget_renders_inline_math_images() {
        let mut picker = ImagePicker::from_fontsize((10, 20));
        picker.set_protocol_type(ImageProtocol::Halfblocks);
        let renderer = Renderer::new(RenderOptions::default().images(ImageMode::Picker(picker)));
        let backend = TestBackend::new(80, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = MdcatWidgetState::new();

        terminal
            .draw(|frame| {
                frame.render_stateful_widget(
                    MdcatWidget::with_renderer("Inline: $E = mc^2$ **bold**", renderer),
                    frame.area(),
                    &mut state,
                );
            })
            .unwrap();

        let first_line = buffer_line(terminal.backend().buffer(), 0);

        assert!(first_line.contains("Inline:"));
        assert!(first_line.contains("bold"));
        assert!(!first_line.contains("E = mc²"));
        let bold_start = first_line.find("bold").expect("bold text rendered");
        let bold_x = textwrap::core::display_width(&first_line[..bold_start]) as u16;
        assert!(terminal
            .backend()
            .buffer()
            .cell((bold_x, 0))
            .expect("cell in bounds")
            .modifier
            .contains(Modifier::BOLD));
        let document = state.cache.as_ref().expect("render cache populated");
        assert_eq!(document.images.len(), 1);
        assert_eq!(document.images[0].line, 0);
        assert_eq!(document.images[0].indent, 8);
        assert_eq!(document.images[0].height, 1);
    }

    #[test]
    fn mdcat_widget_state_tracks_configured_image_picker() {
        let mut picker = ImagePicker::from_fontsize((10, 20));
        picker.set_protocol_type(ImageProtocol::Halfblocks);
        let renderer = Renderer::new(RenderOptions::default().images(ImageMode::Picker(picker)));
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = MdcatWidgetState::new();

        terminal
            .draw(|frame| {
                frame.render_stateful_widget(
                    MdcatWidget::with_renderer("![alt](image.png)", renderer),
                    frame.area(),
                    &mut state,
                );
            })
            .unwrap();

        assert_eq!(state.image_protocol(), Some(ImageProtocol::Halfblocks));
    }
}
