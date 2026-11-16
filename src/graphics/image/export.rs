use super::Image;
use crate::{Error, Result};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportFormat {
    Bmp,
    Png,
    Jpeg,
}

impl ExportFormat {
    pub(crate) fn from_name(name: &[u16]) -> Self {
        let extension = name
            .iter()
            .rposition(|unit| matches!(*unit, 46 | 47 | 92))
            .filter(|&at| name[at] == 46)
            .map_or(&[][..], |at| &name[at + 1..]);
        let matches = |text: &[u8]| {
            extension
                .iter()
                .copied()
                .map(|unit| {
                    if (65..=90).contains(&unit) {
                        unit + 32
                    } else {
                        unit
                    }
                })
                .eq(text.iter().map(|&byte| u16::from(byte)))
        };
        if matches(b"bmp") {
            Self::Bmp
        } else if matches(b"png") {
            Self::Png
        } else {
            Self::Jpeg
        }
    }
}

impl Image {
    pub(crate) fn encode_export(&mut self, format: ExportFormat) -> Result<Option<Vec<u8>>> {
        let changed = (format == ExportFormat::Jpeg && !self.channels_swapped)
            || (format != ExportFormat::Png && !self.is_rgb());
        if format == ExportFormat::Jpeg && !self.channels_swapped {
            for pixel in self.surface.pixels_mut().chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            self.channels_swapped = true;
        }
        if format != ExportFormat::Png && !self.is_rgb() {
            for pixel in self.surface.pixels_mut().chunks_exact_mut(4) {
                for channel in 0..3 {
                    pixel[channel] = ((u32::from(pixel[channel]) * u32::from(pixel[3])) >> 8) as u8;
                }
                pixel[3] = 255;
            }
            self.info.bit_depth = 24;
        }
        if changed {
            self.filter_revision = self.filter_revision.wrapping_add(1);
        }
        if self.info.size.contains(&0) {
            return Ok(None);
        }
        if format == ExportFormat::Bmp {
            return self.encode_bmp().map(Some);
        }
        let mut pixels = Vec::with_capacity(self.surface.pixels().len());
        // 这两个编码器按位图的物理行序提交，保留底端起始的输出方向。
        for row in self
            .surface
            .pixels()
            .chunks_exact(self.surface.stride())
            .rev()
        {
            for pixel in row.chunks_exact(4) {
                if format == ExportFormat::Jpeg {
                    pixels.extend_from_slice(&pixel[..3]);
                } else {
                    pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
                    if !self.is_rgb() {
                        pixels.push(pixel[3]);
                    }
                }
            }
        }
        let [width, height] = self.info.size;
        let mut bytes = Vec::new();
        if format == ExportFormat::Jpeg {
            let (Ok(width), Ok(height)) = (u16::try_from(width), u16::try_from(height)) else {
                return Ok(None);
            };
            let mut encoder = jpeg_encoder::Encoder::new(&mut bytes, 90);
            encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::F_2_2);
            if encoder
                .encode(&pixels, width, height, jpeg_encoder::ColorType::Rgb)
                .is_err()
            {
                return Ok(None);
            }
        } else {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_color(if self.is_rgb() {
                png::ColorType::Rgb
            } else {
                png::ColorType::Rgba
            });
            let Ok(mut writer) = encoder.write_header() else {
                return Ok(None);
            };
            if writer.write_image_data(&pixels).is_err() || writer.finish().is_err() {
                return Ok(None);
            }
            bytes.resize((bytes.len() + 7) & !7, 0);
        }
        Ok(Some(bytes))
    }

    fn encode_bmp(&self) -> Result<Vec<u8>> {
        let [width, height] = self.info.size;
        let stride = (width as usize * 3 + 3) & !3;
        let length = stride * height as usize;
        let file_length =
            u32::try_from(length + 54).map_err(|_| Error::new(0, "BMP 输出长度超过 DWORD 范围"))?;
        let mut bytes = vec![0; (file_length as usize + 7) & !7];
        bytes[..2].copy_from_slice(b"BM");
        for (at, value) in [
            (2, file_length),
            (10, 54),
            (14, 40),
            (18, width),
            (22, height),
            (34, length as u32),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
        for (y, row) in self
            .surface
            .pixels()
            .chunks_exact(self.surface.stride())
            .rev()
            .enumerate()
        {
            for (x, pixel) in row.chunks_exact(4).enumerate() {
                let at = 54 + y * stride + x * 3;
                bytes[at..at + 3].copy_from_slice(&pixel[..3]);
            }
        }
        Ok(bytes)
    }
}
