// Copyright 2018-2020 Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! SVG rendering for mdcat.

use std::io::Result;

use crate::TerminalSize;

/// Render an SVG image to a PNG pixel graphic, scaled up by `scale` (e.g. `2.0` renders at
/// twice the SVG's native pixel size, uniformly enlarging every element).
pub fn render_svg_to_png_scaled(svg: &[u8], scale: f32) -> Result<Vec<u8>> {
    implementation::render_svg_to_png(svg, scale)
}

/// Render an SVG image to a PNG pixel graphic, scaled to fill whatever cell area
/// `terminal_size` says the image will occupy.
///
/// An SVG's declared `width`/`height` is often small (e.g. a 24x24 icon), so rendering it at
/// its native size (`scale` `1.0`) produces a tiny raster. The usual post-render fit step (see
/// [`crate::resources::image::fit_image_to_terminal`]) only enlarges that raster by blurry pixel
/// interpolation, and does not enlarge it at all unless the terminal reports a cell pixel size.
/// Rendering at the target resolution up front instead keeps the image crisp: unlike a raster
/// image, a vector image loses no quality when rendered larger, only when rendered small and
/// then stretched.
///
/// Falls back to the SVG's native size (`scale` `1.0`) if that size can't be determined, or if
/// `terminal_size` doesn't report a cell size.
pub fn render_svg_to_png_for_terminal(svg: &[u8], terminal_size: TerminalSize) -> Result<Vec<u8>> {
    let scale = implementation::native_size(svg)
        .and_then(|(width, height)| scale_to_fill_terminal(width, height, terminal_size))
        .unwrap_or(1.0);
    implementation::render_svg_to_png(svg, scale)
}

/// Compute the scale factor that renders a `(width, height)` native-pixel image to fill whatever
/// cell area it's placed in, mirroring the row-alignment
/// [`crate::resources::image::fit_image_to_terminal`] uses for already-rasterized images: round
/// the height up to a whole number of cell rows, then cap the resulting width to the terminal's
/// pixel width if it would overflow.
///
/// Returns `None` if `width`/`height` is zero, or if `terminal_size` has no cell size.
fn scale_to_fill_terminal(width: u32, height: u32, size: TerminalSize) -> Option<f32> {
    if width == 0 || height == 0 {
        return None;
    }
    let cell = size.cell.filter(|cell| cell.x > 0 && cell.y > 0)?;
    let rows = (height as f64 / cell.y as f64).ceil().max(1.0);
    let mut target_height = rows * cell.y as f64;
    let mut target_width = width as f64 * target_height / height as f64;
    if let Some(pixels) = size.pixels {
        if target_width > pixels.x as f64 {
            target_width = (pixels.x as f64).max(1.0);
            target_height = height as f64 * target_width / width as f64;
        }
    }
    Some((target_height / height as f64) as f32)
}

#[cfg(test)]
mod tests {
    use super::scale_to_fill_terminal;
    use crate::terminal::PixelSize;
    use crate::TerminalSize;

    #[test]
    fn scale_to_fill_terminal_rounds_up_to_whole_cell_rows() {
        // A 24x24 icon in a terminal with 20x30px cells: one row tall (24 rounds up to one 30px
        // row), so it should scale up by 30/24.
        let size = TerminalSize {
            cell: Some(PixelSize::from_xy((20, 30))),
            ..TerminalSize::default()
        };
        let scale = scale_to_fill_terminal(24, 24, size).unwrap();
        assert!((scale - 30.0 / 24.0).abs() < 1e-6, "scale was {scale}");
    }

    #[test]
    fn scale_to_fill_terminal_caps_width_to_window_pixels() {
        // A wide 1000x10 banner in a narrow 100px-wide window: width must not exceed the
        // window, even though that breaks exact row alignment.
        let size = TerminalSize {
            cell: Some(PixelSize::from_xy((10, 20))),
            pixels: Some(PixelSize::from_xy((100, 200))),
            ..TerminalSize::default()
        };
        let scale = scale_to_fill_terminal(1000, 10, size).unwrap();
        assert!((scale - 0.1).abs() < 1e-6, "scale was {scale}");
    }

    #[test]
    fn scale_to_fill_terminal_none_without_cell_size() {
        assert_eq!(
            scale_to_fill_terminal(24, 24, TerminalSize::default()),
            None
        );
    }

    #[test]
    fn scale_to_fill_terminal_none_for_zero_sized_image() {
        let size = TerminalSize {
            cell: Some(PixelSize::from_xy((20, 30))),
            ..TerminalSize::default()
        };
        assert_eq!(scale_to_fill_terminal(0, 24, size), None);
        assert_eq!(scale_to_fill_terminal(24, 0, size), None);
    }

    #[test]
    #[cfg(all(feature = "svg", feature = "image-processing"))]
    fn render_svg_to_png_for_terminal_rasterizes_at_target_resolution() {
        use image::GenericImageView;

        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"
            viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" fill="red"/></svg>"#;
        let size = TerminalSize {
            cell: Some(PixelSize::from_xy((10, 20))),
            pixels: Some(PixelSize::from_xy((800, 480))),
            ..TerminalSize::default()
        };

        let native = super::render_svg_to_png_scaled(svg, 1.0).unwrap();
        let native_dims = image::load_from_memory(&native).unwrap().dimensions();
        assert_eq!(native_dims, (24, 24));

        let fitted = super::render_svg_to_png_for_terminal(svg, size).unwrap();
        let fitted_dims = image::load_from_memory(&fitted).unwrap().dimensions();
        // 24px rounds up to two 20px cell rows (40px), instead of staying at the native size.
        assert_eq!(fitted_dims, (40, 40));
    }
}

#[cfg(feature = "svg")]
mod implementation {
    use std::error::Error;
    use std::fmt::Display;
    use std::sync::{Arc, OnceLock};

    use resvg::tiny_skia::{IntSize, Pixmap, Transform};
    use resvg::usvg::{self, Tree};
    use usvg::fontdb;

    #[derive(Debug)]
    pub enum RenderSvgError {
        ParseError(usvg::Error),
        FailedToCreatePixmap(IntSize),
        EncodePngError(Box<dyn Error + Send + Sync>),
    }

    impl Display for RenderSvgError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                RenderSvgError::ParseError(error) => write!(f, "Failed to parse SVG: {error}"),
                RenderSvgError::FailedToCreatePixmap(int_size) => {
                    write!(f, "Failed to create pixmap of size {int_size:?}")
                }
                RenderSvgError::EncodePngError(error) => {
                    write!(f, "Failed to encode pixmap to PNG image: {error}")
                }
            }
        }
    }

    impl std::error::Error for RenderSvgError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            match self {
                RenderSvgError::ParseError(error) => Some(error),
                RenderSvgError::FailedToCreatePixmap(_) => None,
                RenderSvgError::EncodePngError(error) => Some(error.as_ref()),
            }
        }
    }

    impl From<RenderSvgError> for std::io::Error {
        fn from(value: RenderSvgError) -> Self {
            std::io::Error::other(value)
        }
    }

    impl From<usvg::Error> for RenderSvgError {
        fn from(value: usvg::Error) -> Self {
            Self::ParseError(value)
        }
    }

    static FONTS: OnceLock<Arc<fontdb::Database>> = OnceLock::new();

    fn parse_svg(svg: &[u8]) -> Result<Tree, RenderSvgError> {
        let fonts = FONTS.get_or_init(|| {
            let mut fontdb = fontdb::Database::new();
            fontdb.load_system_fonts();
            Arc::new(fontdb)
        });
        let options = usvg::Options {
            fontdb: fonts.clone(),
            ..Default::default()
        };
        Ok(usvg::Tree::from_data(svg, &options)?)
    }

    fn render_svg_to_png_with_resvg(svg: &[u8], scale: f32) -> Result<Vec<u8>, RenderSvgError> {
        let tree = parse_svg(svg)?;
        let native_size = tree.size().to_int_size();
        let width = ((native_size.width() as f32) * scale).round().max(1.0) as u32;
        let height = ((native_size.height() as f32) * scale).round().max(1.0) as u32;
        let size = IntSize::from_wh(width, height).unwrap_or(native_size);
        let mut pixmap = Pixmap::new(size.width(), size.height())
            .ok_or(RenderSvgError::FailedToCreatePixmap(size))?;
        // We create a pixmap of the appropriate size so the size transform in render cannot fail, so
        // if it fails it's a bug in our code or in resvg which we should fix and not hide.  Hence we
        // unwrap the result.
        resvg::render(
            &tree,
            Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        pixmap
            .encode_png()
            .map_err(|err| RenderSvgError::EncodePngError(Box::new(err)))
    }

    pub fn render_svg_to_png(svg: &[u8], scale: f32) -> std::io::Result<Vec<u8>> {
        render_svg_to_png_with_resvg(svg, scale).map_err(Into::into)
    }

    /// Return the native (unscaled) pixel size of an SVG image, or `None` if it fails to parse.
    pub fn native_size(svg: &[u8]) -> Option<(u32, u32)> {
        let size = parse_svg(svg).ok()?.size().to_int_size();
        Some((size.width(), size.height()))
    }
}

#[cfg(not(feature = "svg"))]
mod implementation {
    use std::io::{Error, ErrorKind, Result};
    pub fn render_svg_to_png(_svg: &[u8], _scale: f32) -> Result<Vec<u8>> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "SVG rendering not enabled in this build",
        ))
    }

    /// SVG support is compiled out, so the native size can never be determined.
    pub fn native_size(_svg: &[u8]) -> Option<(u32, u32)> {
        None
    }
}
