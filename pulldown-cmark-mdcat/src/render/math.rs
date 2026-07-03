use std::io::Cursor;

use crate::terminal::TerminalSize;

const INLINE_BASELINE_RATIO: f32 = 0.8;

fn is_layout_command(command: &str) -> bool {
    matches!(
        command,
        "mathrm"
            | "mathsf"
            | "mathit"
            | "mathbf"
            | "mathcal"
            | "text"
            | "textbf"
            | "textit"
            | "operatorname"
    )
}

fn parse_command(input: &str, start: usize) -> Option<(&str, usize)> {
    if input.as_bytes().get(start) != Some(&b'\\') {
        return None;
    }
    let command_start = start + 1;
    let mut command_end = command_start;
    for (offset, ch) in input[command_start..].char_indices() {
        if ch.is_ascii_alphabetic() {
            command_end = command_start + offset + ch.len_utf8();
        } else {
            break;
        }
    }
    if command_end == command_start {
        None
    } else {
        Some((&input[command_start..command_end], command_end))
    }
}

fn parse_braced_group(input: &str, start: usize) -> Option<(&str, usize)> {
    if input.as_bytes().get(start) != Some(&b'{') {
        return None;
    }
    let mut depth = 1;
    let mut escaped = false;
    let content_start = start + 1;
    for (offset, b) in input.as_bytes()[content_start..].iter().enumerate() {
        let index = content_start + offset;
        if escaped {
            escaped = false;
            continue;
        }
        match *b {
            b'\\' => escaped = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&input[content_start..index], index + 1));
                }
            }
            _ => {}
        }
    }
    None
}

fn rewrite_latex(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        let Some((command, command_end)) = parse_command(input, index) else {
            let ch = input[index..].chars().next().expect("valid char boundary");
            output.push(ch);
            index += ch.len_utf8();
            continue;
        };

        match command {
            "frac" => {
                if let Some((numerator, numerator_end)) = parse_braced_group(input, command_end) {
                    if let Some((denominator, denominator_end)) =
                        parse_braced_group(input, numerator_end)
                    {
                        output.push_str(&rewrite_latex(numerator));
                        output.push('/');
                        output.push_str(&rewrite_latex(denominator));
                        index = denominator_end;
                        continue;
                    }
                }
            }
            "sqrt" => {
                if let Some((radicand, group_end)) = parse_braced_group(input, command_end) {
                    output.push_str("√(");
                    output.push_str(&rewrite_latex(radicand));
                    output.push(')');
                    index = group_end;
                    continue;
                }
            }
            "xrightarrow" => {
                if let Some((label, group_end)) = parse_braced_group(input, command_end) {
                    output.push('—');
                    output.push_str(&rewrite_latex(label));
                    output.push('→');
                    index = group_end;
                    continue;
                }
            }
            command if is_layout_command(command) => {
                if let Some((contents, group_end)) = parse_braced_group(input, command_end) {
                    output.push_str(&rewrite_latex(contents));
                    index = group_end;
                    continue;
                }
            }
            _ => {}
        }

        output.push('\\');
        output.push_str(command);
        index = command_end;
    }
    output
}

/// Convert LaTeX math to Unicode text (fallback for inline math and
/// terminals without image support).
pub(crate) fn render_math_unicode(input: &str) -> String {
    let s = rewrite_latex(input);
    unicodeit::replace(&s)
}

fn ansi_to_ratex_color(color: anstyle::AnsiColor) -> ratex_types::Color {
    use anstyle::AnsiColor::*;
    match color {
        Black => ratex_types::Color::rgb(0.0, 0.0, 0.0),
        Red => ratex_types::Color::rgb(0.8, 0.0, 0.0),
        Green => ratex_types::Color::rgb(0.0, 0.8, 0.0),
        Yellow => ratex_types::Color::rgb(0.8, 0.8, 0.0),
        Blue => ratex_types::Color::rgb(0.2, 0.4, 1.0),
        Magenta => ratex_types::Color::rgb(0.8, 0.0, 0.8),
        Cyan => ratex_types::Color::rgb(0.0, 0.8, 0.8),
        White => ratex_types::Color::rgb(0.9, 0.9, 0.9),
        BrightBlack => ratex_types::Color::rgb(0.5, 0.5, 0.5),
        BrightRed => ratex_types::Color::rgb(1.0, 0.3, 0.3),
        BrightGreen => ratex_types::Color::rgb(0.3, 1.0, 0.3),
        BrightYellow => ratex_types::Color::rgb(1.0, 1.0, 0.3),
        BrightBlue => ratex_types::Color::rgb(0.5, 0.6, 1.0),
        BrightMagenta => ratex_types::Color::rgb(1.0, 0.3, 1.0),
        BrightCyan => ratex_types::Color::rgb(0.3, 1.0, 1.0),
        BrightWhite => ratex_types::Color::rgb(1.0, 1.0, 1.0),
    }
}

fn style_to_ratex_color(style: &anstyle::Style) -> ratex_types::Color {
    match style.get_fg_color() {
        Some(anstyle::Color::Ansi(c)) => ansi_to_ratex_color(c),
        Some(anstyle::Color::Ansi256(c)) => {
            ansi_to_ratex_color(c.into_ansi().unwrap_or(anstyle::AnsiColor::White))
        }
        Some(anstyle::Color::Rgb(rgb)) => ratex_types::Color::rgb(
            rgb.0 as f32 / 255.0,
            rgb.1 as f32 / 255.0,
            rgb.2 as f32 / 255.0,
        ),
        None => ratex_types::Color::rgb(0.9, 0.9, 0.9),
    }
}

fn font_size_from_terminal(terminal_size: &TerminalSize) -> f32 {
    if let Some(cell) = terminal_size.cell {
        (cell.y as f32 * 0.85).max(8.0)
    } else {
        14.0
    }
}

fn png_dimensions(data: &[u8]) -> (u32, u32) {
    if data.len() < 28 {
        return (0, 0);
    }
    let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    (w, h)
}

fn pad_png_vertically(data: &[u8], top: u32, bottom: u32) -> Option<Vec<u8>> {
    if top == 0 && bottom == 0 {
        return Some(data.to_vec());
    }

    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    if info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let bytes = &buffer[..info.buffer_size()];
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(bytes),
        png::ColorType::Rgb => {
            for pixel in bytes.chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for gray in bytes {
                rgba.extend_from_slice(&[*gray, *gray, *gray, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in bytes.chunks_exact(2) {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        png::ColorType::Indexed => return None,
    }

    let width = info.width;
    let height = info.height + top + bottom;
    let row_len = width as usize * 4;
    let mut padded = vec![0; row_len * height as usize];
    let top_bytes = row_len * top as usize;
    padded[top_bytes..top_bytes + rgba.len()].copy_from_slice(&rgba);

    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&padded).ok()?;
    }
    Some(output)
}

pub(crate) struct MathImage {
    pub png: Vec<u8>,
    pub width_columns: u16,
    pub height_rows: u16,
    #[cfg(test)]
    pub height_pixels: u32,
    #[cfg(test)]
    pub baseline_pixels: u32,
}

/// Render math to a PNG image using RaTeX.
pub(crate) fn render_math_png(
    input: &str,
    display_mode: bool,
    terminal_size: &TerminalSize,
    math_style: &anstyle::Style,
) -> Option<MathImage> {
    use ratex_layout::{layout, to_display_list, LayoutOptions};
    use ratex_parser::parse;
    use ratex_render::{render_to_png, RenderOptions};
    use ratex_types::{Color, MathStyle};

    let ast = parse(input).ok()?;
    let style = if display_mode {
        MathStyle::Display
    } else {
        MathStyle::Text
    };
    let layout_opts = LayoutOptions {
        style,
        color: style_to_ratex_color(math_style),
        ..LayoutOptions::default()
    };
    let lbox = layout(&ast, &layout_opts);
    let display_list = to_display_list(&lbox);
    let font_size = font_size_from_terminal(terminal_size);
    let render_opts = RenderOptions {
        font_size,
        padding: 0.0,
        background_color: Color::new(0.0, 0.0, 0.0, 0.0),
        device_pixel_ratio: 1.0,
        ..RenderOptions::default()
    };
    let mut png = render_to_png(&display_list, &render_opts).ok()?;
    let (px_w, mut px_h) = png_dimensions(&png);
    let baseline_px = (display_list.height as f32 * font_size).round().max(0.0) as u32;
    #[cfg(test)]
    let mut padded_baseline_px = baseline_px;

    if !display_mode {
        if let Some(cell) = terminal_size.cell {
            let target_baseline = (cell.y as f32 * INLINE_BASELINE_RATIO).round() as u32;
            let top_pad = target_baseline.saturating_sub(baseline_px);
            let unrounded_height = top_pad + px_h;
            let bottom_pad = if unrounded_height < cell.y {
                cell.y - unrounded_height
            } else {
                (cell.y - (unrounded_height % cell.y)) % cell.y
            };
            png = pad_png_vertically(&png, top_pad, bottom_pad)?;
            px_h += top_pad + bottom_pad;
            #[cfg(test)]
            {
                padded_baseline_px += top_pad;
            }
        }
    }

    let (width_columns, height_rows) = if let Some(cell) = terminal_size.cell {
        (
            ((px_w as f32 / cell.x as f32).ceil() as u16).max(1),
            ((px_h as f32 / cell.y as f32).ceil() as u16).max(1),
        )
    } else {
        ((input.len() as u16).max(1), 1)
    };
    Some(MathImage {
        png,
        width_columns,
        height_rows,
        #[cfg(test)]
        height_pixels: px_h,
        #[cfg(test)]
        baseline_pixels: padded_baseline_px,
    })
}

#[cfg(test)]
mod tests {
    use super::render_math_unicode;

    #[test]
    fn strip_mathrm() {
        assert_eq!(render_math_unicode(r"\mathrm{id}"), "id");
    }

    #[test]
    fn strip_mathsf() {
        assert_eq!(render_math_unicode(r"\mathsf{Set}"), "Set");
    }

    #[test]
    fn strip_text() {
        assert_eq!(render_math_unicode(r"\text{for all}"), "for all");
    }

    #[test]
    fn frac_renders() {
        assert_eq!(render_math_unicode(r"\frac{a}{b}"), "a/b");
    }

    #[test]
    fn nested_frac_renders() {
        assert_eq!(render_math_unicode(r"\frac{\sqrt{\pi}}{2}"), "√(π)/2");
    }

    #[test]
    fn sqrt_renders() {
        assert_eq!(render_math_unicode(r"\sqrt{x}"), "√(x)");
    }

    #[test]
    fn xrightarrow_renders() {
        assert_eq!(render_math_unicode(r"\xrightarrow{\sim}"), "—∼→");
    }

    #[test]
    fn mathbb_preserved() {
        assert_eq!(render_math_unicode(r"\mathbb{R}"), "ℝ");
    }

    #[test]
    fn combined() {
        assert_eq!(render_math_unicode(r"\mathrm{id}_{\mathsf{C}}"), "id_{C}");
    }

    #[test]
    fn render_png_simple() {
        let math_style = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
        let terminal_size = crate::terminal::TerminalSize::default();
        let img = super::render_math_png(r"x^2 + y^2", false, &terminal_size, &math_style);
        assert!(img.is_some());
        let img = img.unwrap();
        assert!(img.png.len() > 100);
        assert_eq!(&img.png[1..4], b"PNG");
        assert!(img.width_columns > 0);
    }

    #[test]
    fn render_png_aligns_inline_baselines_to_terminal_cell() {
        use crate::terminal::PixelSize;

        let math_style = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
        let terminal_size = crate::terminal::TerminalSize {
            columns: 80,
            rows: 24,
            pixels: Some(PixelSize { x: 800, y: 480 }),
            cell: Some(PixelSize { x: 10, y: 20 }),
        };

        let alpha = super::render_math_png(r"\alpha", false, &terminal_size, &math_style).unwrap();
        let beta = super::render_math_png(r"\beta", false, &terminal_size, &math_style).unwrap();
        let field =
            super::render_math_png(r"\mathbb{F}", false, &terminal_size, &math_style).unwrap();

        assert_eq!(alpha.baseline_pixels, 16);
        assert_eq!(beta.baseline_pixels, 16);
        assert_eq!(field.baseline_pixels, 16);
        assert_eq!(alpha.height_pixels, 20);
        assert_eq!(beta.height_pixels, 20);
        assert_eq!(field.height_pixels, 20);
        assert_eq!(alpha.height_rows, 1);
        assert_eq!(beta.height_rows, 1);
        assert_eq!(field.height_rows, 1);
    }
}
