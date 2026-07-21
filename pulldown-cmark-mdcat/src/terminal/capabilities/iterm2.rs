// Copyright 2018-2020 Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Support for specific iTerm2 features.
//!
//! This module provides the iTerm2 marks and the iTerm2 image protocol.

use std::borrow::Cow;
use std::io::{self, Result, Write};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use tracing::{event, instrument, Level};

use crate::resources::{svg, InlineImageProtocol};
use crate::terminal::osc::write_osc;
use crate::ResourceUrlHandler;

/// Iterm2 terminal protocols.
#[derive(Debug, Copy, Clone)]
pub struct ITerm2Protocol;

impl ITerm2Protocol {
    /// Write an iterm2 mark command to the given `writer`.
    pub fn set_mark<W: Write>(self, writer: &mut W) -> io::Result<()> {
        write_osc(writer, "1337;SetMark")
    }
}

/// The iterm2 inline image protocol.
///
/// See <https://iterm2.com/documentation-images.html> for details; effectively we write a base64
/// encoded dump of the pixel data.
///
/// This implementation does **not** validate whether iterm2 actually supports the image type;
/// it writes data opportunistically and hopes iTerm2 copes.  For rare formats which are not
/// supported by macOS, this may yield false positives, i.e. this implementation might not return
/// an error even though iTerm2 cannot actually display the image.
impl ITerm2Protocol {
    /// Write raw PNG bytes inline to the terminal.
    pub(crate) fn write_png_data(
        &self,
        writer: &mut dyn Write,
        png_data: &[u8],
    ) -> std::io::Result<()> {
        let data = STANDARD.encode(png_data);
        write_osc(
            writer,
            &format!("1337;File=size={};inline=1:{}", png_data.len(), data),
        )
    }
}

impl InlineImageProtocol for ITerm2Protocol {
    #[instrument(skip(self, writer, terminal_size, resource_handler), fields(url = %url))]
    fn write_inline_image(
        &self,
        writer: &mut dyn Write,
        resource_handler: &dyn ResourceUrlHandler,
        url: &url::Url,
        #[cfg_attr(not(feature = "image-processing"), allow(unused_variables))]
        terminal_size: crate::TerminalSize,
    ) -> Result<()> {
        let mime_data = resource_handler.read_resource(url)?;
        event!(
            Level::DEBUG,
            "Received data of mime type {:?}",
            mime_data.mime_type
        );

        // Determine the local file name to use, by taking the last segment of the URL.
        // If the URL has no last segment do not tell iterm about a file name.
        let name = url
            .path_segments()
            .and_then(|mut s| s.next_back())
            .map(Cow::Borrowed);
        let (name, contents) = if let Some("image/svg+xml") = mime_data.mime_type_essence() {
            event!(Level::DEBUG, "Rendering SVG from {}", url);
            (
                name.map(|n| {
                    let mut name = String::new();
                    name.push_str(&n);
                    name.push_str(".png");
                    Cow::Owned(name)
                }),
                Cow::Owned(svg::render_svg_to_png(&mime_data.data)?),
            )
        } else {
            event!(Level::DEBUG, "Rendering mime data literally");
            (name, Cow::Borrowed(&mime_data.data))
        };

        // Without explicit `width=`/`height=`, iTerm2's "auto" sizing decides how many cells
        // the image takes, and has historically left small inline images (e.g. shields.io-style
        // badges) rendered at native resolution rather than filling a whole cell row. iTerm2
        // does not reliably stretch the transmitted pixel data to fill a declared display size,
        // so when the image doesn't already land on a whole number of terminal rows (see
        // `fit_image_to_terminal`), actually resize and re-encode the pixel data, and tell
        // iTerm2 its exact new pixel size explicitly.
        #[cfg(feature = "image-processing")]
        let (contents, pixel_size) = {
            use crate::resources::image::fit_image_to_terminal;
            use image::GenericImageView;
            match image::load_from_memory(&contents)
                .ok()
                .and_then(|image| fit_image_to_terminal(&image, terminal_size))
            {
                Some(resized) => {
                    let mut png = Vec::new();
                    let mut cursor = std::io::Cursor::new(&mut png);
                    match resized.write_to(&mut cursor, image::ImageFormat::Png) {
                        Ok(()) => (Cow::Owned(png), Some(resized.dimensions())),
                        Err(_) => (contents, None),
                    }
                }
                None => (contents, None),
            }
        };
        #[cfg(not(feature = "image-processing"))]
        let pixel_size: Option<(u32, u32)> = None;

        let data = STANDARD.encode(contents.as_ref());
        let size_params = pixel_size.map_or_else(String::new, |(width, height)| {
            format!(",width={width}px,height={height}px")
        });
        write_osc(
            writer,
            &name.map_or_else(
                || {
                    format!(
                        "1337;File=size={}{size_params};inline=1:{}",
                        contents.len(),
                        data
                    )
                },
                |name| {
                    format!(
                        "1337;File=name={};size={}{size_params};inline=1:{}",
                        STANDARD.encode(name.as_bytes()),
                        contents.len(),
                        data
                    )
                },
            ),
        )
    }
}

#[cfg(all(test, feature = "image-processing"))]
mod tests {
    use super::*;
    use crate::resources::MimeData;
    use crate::terminal::PixelSize;
    use crate::TerminalSize;
    use image::{DynamicImage, RgbaImage};

    struct FixedImage(Vec<u8>);

    impl ResourceUrlHandler for FixedImage {
        fn read_resource(&self, _url: &url::Url) -> io::Result<MimeData> {
            Ok(MimeData {
                mime_type: Some("image/png".parse().unwrap()),
                data: self.0.clone(),
            })
        }
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(RgbaImage::new(width, height));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    fn write_image(width: u32, height: u32, terminal_size: TerminalSize) -> String {
        let handler = FixedImage(png_bytes(width, height));
        let url = url::Url::parse("file:///badge.png").unwrap();
        let mut out = Vec::new();
        ITerm2Protocol
            .write_inline_image(&mut out, &handler, &url, terminal_size)
            .unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn resizes_and_reports_explicit_size_when_not_row_aligned() {
        // A 108x20px badge on a terminal with 34px-tall cells doesn't fill a whole row at its
        // native size.
        let size = TerminalSize {
            pixels: Some(PixelSize { x: 1600, y: 816 }),
            cell: Some(PixelSize { x: 10, y: 34 }),
            ..TerminalSize::default()
        };
        let output = write_image(108, 20, size);
        assert!(
            output.contains(",width=184px,height=34px;"),
            "output should declare the resized dimensions, got: {output:?}"
        );
    }

    #[test]
    fn leaves_row_aligned_images_untouched() {
        let size = TerminalSize {
            pixels: Some(PixelSize { x: 1600, y: 816 }),
            cell: Some(PixelSize { x: 10, y: 34 }),
            ..TerminalSize::default()
        };
        let output = write_image(100, 34, size);
        assert!(
            !output.contains("width="),
            "already row-aligned image shouldn't get an explicit size, got: {output:?}"
        );
    }

    #[test]
    fn no_cell_size_means_no_resize() {
        let output = write_image(108, 20, TerminalSize::default());
        assert!(
            !output.contains("width="),
            "without cell size info no resize should happen, got: {output:?}"
        );
    }
}
