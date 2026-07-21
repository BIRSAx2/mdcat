// SPDX-License-Identifier: Apache-2.0

//! Sixel image support as thin wrapper on top of icy_sixel.

use std::io::{self, Write};

use crate::resources::image::*;
use icy_sixel::SixelImage;
use image::GenericImageView;

use crate::resources::MimeData;
use crate::terminal::size::TerminalSize;

/// Sixel graphics protocol implementation.
#[derive(Debug, Copy, Clone)]
pub struct SixelProtocol;

impl SixelProtocol {
    fn load_image(mime: MimeData) -> io::Result<image::DynamicImage> {
        match mime.mime_type_essence() {
            Some("image/svg+xml") => {
                let png = crate::resources::svg::render_svg_to_png(&mime.data)?;

                image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                    .map_err(io::Error::other)
            }

            Some("image/png") => {
                image::load_from_memory_with_format(&mime.data, image::ImageFormat::Png)
                    .map_err(io::Error::other)
            }

            _ => image::load_from_memory(&mime.data).map_err(io::Error::other),
        }
    }

    /// Inject a sixel "set raster attributes" command (`"Pan;Pad;Ph;Pv`) declaring the image's
    /// true pixel size, right after the DCS introducer and before the colour/band data.
    ///
    /// icy_sixel never emits this command itself; it only writes data in bands of 6 rows
    /// (rounding the row count up), so a terminal has no way to know the exact height short of
    /// counting bands, and ends up treating the image as up to 5px taller than it is. On a
    /// terminal whose cell height isn't itself a multiple of 6, that's enough to push the
    /// post-image cursor row (see DECSET 8452 in [`Self::render_sixel`]) one row further down
    /// than intended, and that compounds with every consecutive image into a descending
    /// staircase.
    fn inject_raster_attributes(sixel: &str, width: u32, height: u32) -> String {
        let intro_end = sixel.find('q').map_or(0, |i| i + 1);
        format!(
            "{}\"1;1;{width};{height}{}",
            &sixel[..intro_end],
            &sixel[intro_end..]
        )
    }

    fn render_sixel(writer: &mut dyn Write, img: image::DynamicImage) -> io::Result<()> {
        let (w, h) = img.dimensions();
        let rgba = img.to_rgba8();

        let sixel = SixelImage::try_from_rgba(rgba.into_raw(), w as usize, h as usize)
            .map_err(io::Error::other)?
            .encode()
            .map_err(io::Error::other)?;
        let sixel = Self::inject_raster_attributes(&sixel, w, h);
        // By default a terminal moves the cursor to the left margin of the line below a sixel
        // image, regardless of the image's height; DECSET 8452 instead leaves the cursor to the
        // right of the image, on the row it started on. Without this, images meant to flow
        // inline with surrounding text (e.g. several badges in a row) stack vertically instead,
        // one below the other, and can scroll the earlier ones out of view. Scope the mode to
        // just this image so we don't leave the terminal's sixel behaviour altered afterwards.
        write!(writer, "\x1b[?8452h{sixel}\x1b[?8452l")
    }

    /// Write raw PNG bytes inline to the terminal.
    pub(crate) fn write_png_data(&self, writer: &mut dyn Write, png_data: &[u8]) -> io::Result<()> {
        let img = image::load_from_memory_with_format(png_data, image::ImageFormat::Png)
            .map_err(io::Error::other)?;

        Self::render_sixel(writer, img)
    }
}

impl crate::resources::InlineImageProtocol for SixelProtocol {
    fn write_inline_image(
        &self,
        writer: &mut dyn Write,
        resource_handler: &dyn crate::resources::ResourceUrlHandler,
        url: &url::Url,
        terminal_size: TerminalSize,
    ) -> io::Result<()> {
        let mime = resource_handler.read_resource(url)?;
        let image = SixelProtocol::load_image(mime)?;

        let image = if let Some(downsized) = fit_image_to_terminal(&image, terminal_size) {
            downsized
        } else {
            image
        };

        SixelProtocol::render_sixel(writer, image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raster_attributes_are_inserted_right_after_the_dcs_introducer() {
        let sixel = "\x1bP9;1;0q#0;2;0;0;0$-";
        let with_raster = SixelProtocol::inject_raster_attributes(sixel, 108, 20);
        assert_eq!(with_raster, "\x1bP9;1;0q\"1;1;108;20#0;2;0;0;0$-");
    }

    #[test]
    fn raster_attributes_reflect_the_true_dimensions() {
        let sixel = "\x1bP0;0;0q#0;2;0;0;0$-";
        let with_raster = SixelProtocol::inject_raster_attributes(sixel, 1, 1);
        assert!(with_raster.contains("\"1;1;1;1"));
    }
}
