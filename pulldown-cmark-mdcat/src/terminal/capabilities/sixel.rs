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

    fn render_sixel(writer: &mut dyn Write, img: image::DynamicImage) -> io::Result<()> {
        let (w, h) = img.dimensions();
        let rgba = img.to_rgba8();

        let sixel = SixelImage::try_from_rgba(rgba.into_raw(), w as usize, h as usize)
            .map_err(io::Error::other)?
            .encode()
            .map_err(io::Error::other)?;
        write!(writer, "{sixel}")
    }

    /// Write raw PNG bytes inline to the terminal.
    pub fn write_png_data(&self, writer: &mut dyn Write, png_data: &[u8]) -> io::Result<()> {
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

        let image = if let Some(downsized) = downsize_to_columns(&image, terminal_size) {
            downsized
        } else {
            image
        };

        SixelProtocol::render_sixel(writer, image)
    }
}
