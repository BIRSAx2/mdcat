// Copyright Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Inline image handling

use std::io::Write;

use url::Url;

use crate::{ResourceUrlHandler, TerminalSize};

/// An implementation of an inline image protocol.
pub trait InlineImageProtocol {
    /// Write an inline image to `writer`.
    ///
    /// `url` is the URL pointing to the image was obtained from.  If the underlying terminal does
    /// not support URLs directly the protocol should use `resource_handler` to load the image data
    /// from `url`.
    ///
    /// `size` denotes the dimensions of the current terminal, to be used as indication for the
    /// size the image should be rendered at.
    ///
    /// Implementations are encouraged to return an IO error with [`std::io::ErrorKind::Unsupported`]
    /// if either the underlying terminal does not support images currently or if it does not
    /// support the given image format.
    fn write_inline_image(
        &self,
        writer: &mut dyn Write,
        resource_handler: &dyn ResourceUrlHandler,
        url: &Url,
        terminal_size: TerminalSize,
    ) -> std::io::Result<()>;
}

/// Fit an image to the given terminal size.
///
/// Terminal image protocols place an image by its pixel size alone: the terminal divides that
/// size by its cell size and rounds up to get the number of rows/columns to occupy, then either
/// stretches the image to fill that whole area, or leaves the rest of the last row/column blank,
/// depending on the terminal and protocol. mdcat cannot rely on the terminal to always do the
/// former, so images that don't already land on a whole number of cell rows are explicitly scaled
/// to do so; this is particularly common for small inline images like shields.io-style badges,
/// which have historically ended up rendered too small, at native resolution, occupying only
/// part of their allotted row.
///
/// Also downsizes the image to fit into the terminal's column limit, if `image` is wider than
/// that.
///
/// Returns `None` if `size` gives neither a cell size nor a pixel size, or if `image` already
/// exactly fits (row-aligned, and not wider than the column limit).
#[cfg(feature = "image-processing")]
pub fn fit_image_to_terminal(
    image: &image::DynamicImage,
    size: TerminalSize,
) -> Option<image::DynamicImage> {
    use image::{imageops::FilterType, GenericImageView};
    use tracing::{event, Level};
    let (image_width, image_height) = image.dimensions();
    event!(
        Level::DEBUG,
        "Terminal size {:?}; image is {:?}",
        size,
        (image_width, image_height)
    );

    if let Some(cell) = size.cell.filter(|cell| cell.x > 0 && cell.y > 0) {
        let rows = (image_height as f64 / cell.y as f64).ceil().max(1.0);
        let mut target_height = (rows * cell.y as f64).round() as u32;
        let mut target_width = (image_width as f64 * target_height as f64 / image_height as f64)
            .round()
            .max(1.0) as u32;

        // The row-aligned width may overflow the terminal; cap it, even if that gives up the
        // row alignment we just computed.
        if let Some(win_size) = size.pixels {
            if target_width > win_size.x {
                target_width = win_size.x.max(1);
                target_height = (image_height as f64 * target_width as f64 / image_width as f64)
                    .round()
                    .max(1.0) as u32;
            }
        }

        if target_width == image_width && target_height == image_height {
            None
        } else {
            Some(image.resize_exact(target_width, target_height, FilterType::Lanczos3))
        }
    } else if let Some(win_size) = size.pixels {
        if win_size.x < image_width {
            Some(image.resize(win_size.x, win_size.y, FilterType::Nearest))
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(all(test, feature = "image-processing"))]
mod tests {
    use super::*;
    use crate::terminal::PixelSize;
    use image::{DynamicImage, GenericImageView, RgbaImage};

    fn image_sized(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::new(width, height))
    }

    #[test]
    fn no_size_info_leaves_image_untouched() {
        let image = image_sized(108, 20);
        assert!(fit_image_to_terminal(&image, TerminalSize::default()).is_none());
    }

    #[test]
    fn short_badge_is_scaled_up_to_a_whole_cell_row() {
        // A 108x20px badge on a terminal with 34px-tall cells doesn't fill a whole row at its
        // native size; it should get scaled up to fill exactly one row, keeping aspect ratio.
        let image = image_sized(108, 20);
        let size = TerminalSize {
            pixels: Some(PixelSize { x: 1600, y: 816 }),
            cell: Some(PixelSize { x: 10, y: 34 }),
            ..TerminalSize::default()
        };
        let resized = fit_image_to_terminal(&image, size).expect("badge should be resized");
        let (w, h) = resized.dimensions();
        assert_eq!(h, 34);
        assert_eq!(w, (108.0_f64 * 34.0 / 20.0).round() as u32);
    }

    #[test]
    fn image_already_row_aligned_is_left_untouched() {
        let image = image_sized(100, 34);
        let size = TerminalSize {
            pixels: Some(PixelSize { x: 1600, y: 816 }),
            cell: Some(PixelSize { x: 10, y: 34 }),
            ..TerminalSize::default()
        };
        assert!(fit_image_to_terminal(&image, size).is_none());
    }

    #[test]
    fn row_alignment_yields_to_column_limit() {
        // Row-aligning this image to 3*34=102px height would need ~551px of width, well past the
        // terminal's pixel width; the column limit must win, even though the result then isn't
        // row-aligned anymore.
        let image = image_sized(1000, 60);
        let size = TerminalSize {
            pixels: Some(PixelSize { x: 200, y: 816 }),
            cell: Some(PixelSize { x: 10, y: 34 }),
            ..TerminalSize::default()
        };
        let resized = fit_image_to_terminal(&image, size).expect("image should be resized");
        let (w, _) = resized.dimensions();
        assert_eq!(w, 200);
    }

    #[test]
    fn without_cell_size_falls_back_to_column_downsize() {
        let image = image_sized(1000, 60);
        let size = TerminalSize {
            pixels: Some(PixelSize { x: 200, y: 816 }),
            cell: None,
            ..TerminalSize::default()
        };
        let resized = fit_image_to_terminal(&image, size).expect("image should be resized");
        let (w, _) = resized.dimensions();
        assert_eq!(w, 200);
    }
}
