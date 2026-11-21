//! Reference CPU renderer for hosts that need an offscreen backend.

use super::{DrawList, FrameCanvas, GraphicsBackend, SequenceCapture, Surface};
use crate::{
    Result,
    graphics::{RenderTarget, canvas_tile_bytes},
};

mod commands;
use commands::Target;

impl RenderTarget {
    /// Draw over this tiled target without clearing it. A source canvas is
    /// required only for commands referencing canvas tiles.
    pub fn draw_list(&mut self, frame: &DrawList, canvas: Option<&RenderTarget>) -> Result<bool> {
        frame.render_at(Target::Tiled(self), canvas.map(RenderTarget::tiles), [0; 2])
    }
}

impl DrawList {
    fn render_at(
        &self,
        mut output: Target<'_>,
        tiles: Option<&[Option<Surface>]>,
        origin: [u32; 2],
    ) -> Result<bool> {
        let mut touched = false;
        for command in &self.commands {
            touched |= commands::draw_command(&mut output, tiles, command, self.filter, origin)?;
        }
        Ok(touched)
    }
}

/// Software renderer for host presentation and synchronous readback.
#[derive(Default)]
pub struct SoftwareBackend;

impl SoftwareBackend {
    /// Submit the software-rendered frame to a pending sequence capture.
    pub fn submit_capture(capture: SequenceCapture<'_>, frame: &Surface) -> Result<bool> {
        capture.write_bgra(frame.size(), frame.stride(), frame.pixels())
    }

    /// Draw over the host's BGRA surface without clearing it.
    pub fn render_into(
        frame: &DrawList,
        canvas: &FrameCanvas<'_>,
        output: &mut Surface,
    ) -> Result<bool> {
        frame.render_at(Target::Surface(output), Some(canvas.tiles()), [0; 2])
    }
}

impl GraphicsBackend for SoftwareBackend {
    fn render(
        &mut self,
        frame: &DrawList,
        canvas: &mut FrameCanvas<'_>,
        size: [u32; 2],
    ) -> Result<Surface> {
        canvas_tile_bytes(size)?;
        let mut output = Surface::new(size)?;
        let pixels = size[0] as usize * size[1] as usize;
        let jobs = (pixels / (192 * 1024)).clamp(1, 4);
        let workers =
            if jobs > 1 && size[1] > 1 && frame.commands.iter().any(|c| c.texture.is_some()) {
                std::thread::available_parallelism().map_or(1, |count| jobs.min(count.get()))
            } else {
                1
            };
        if workers == 1 {
            Self::render_into(frame, canvas, &mut output)?;
        } else {
            render_in_bands(frame, canvas.tiles(), &mut output, workers)?;
        }
        Ok(output)
    }
}

fn render_in_bands(
    frame: &DrawList,
    tiles: &[Option<Surface>],
    output: &mut Surface,
    workers: usize,
) -> Result<()> {
    let [width, height] = output.size();
    let rows = height.div_ceil(workers as u32);
    // 各分块使用同一全局像素坐标；不平移顶点或改变命令顺序。
    let render_band = |top| -> Result<Option<Surface>> {
        let mut band = Surface::new([width, rows.min(height - top)])?;
        let touched = frame.render_at(Target::Surface(&mut band), Some(tiles), [0, top])?;
        Ok(touched.then_some(band))
    };
    let mut copy_band = |top, band: Option<Surface>| {
        if let Some(band) = band {
            output.copy_from(&band, band.bounds(), [0, top as i32]);
        }
    };
    std::thread::scope(|scope| {
        let bands: Vec<_> = (rows..height)
            .step_by(rows as usize)
            .map(|top| (top, scope.spawn(move || render_band(top))))
            .collect();
        copy_band(0, render_band(0)?);
        for (top, band) in bands {
            copy_band(top, band.join().unwrap()?);
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{
        DrawCommand, FrameTexture, ProjectedVertex, Rect, Surface, TextureAddress, TextureFilter,
        TextureSource,
    };

    #[test]
    fn software_bands_preserve_global_clipping_and_blending_order() {
        let size = [17, 13];
        let colors = [0x4020_4060u32, 0xffff_8020, 0x8090_00ff, 0xc040_a011];
        let texture = Surface::from_bgra(
            [2, 2],
            colors.into_iter().flat_map(u32::to_le_bytes).collect(),
        )
        .unwrap();
        let mut canvas = RenderTarget::from_tiles([2, 2], [1, 1], vec![Some(texture)]).unwrap();
        let vertex = |position, color, rhw| ProjectedVertex {
            position,
            color,
            rhw,
        };
        let frame = DrawList {
            commands: vec![
                DrawCommand {
                    vertices: [
                        vertex([-1.5, 0.75, 0.2], 0xffcc_8011, 1.),
                        vertex([16.5, 6.5, 0.3], 0x8010_cc11, 0.5),
                        vertex([0.5, 12.75, 0.6], 0xffa0_90ff, 1.5),
                    ],
                    texture: Some(FrameTexture {
                        source: TextureSource::Canvas(0),
                        uv: [[-0.5, 0.1], [1.5, 0.2], [0.8, 1.4]],
                        address: [TextureAddress::Wrap, TextureAddress::Mirror],
                        filter: None,
                    }),
                    blend_mode: 0,
                    clip: Some(Rect::from_xywh(1, 2, 14, 9)),
                },
                DrawCommand {
                    vertices: [
                        vertex([12.25, 1., 0.2], 0xff01_0203, 1.),
                        vertex([0.25, 11.75, 0.6], 0xff01_0203, 1.),
                        vertex([13.25, 11.75, 0.3], 0xff01_0203, 1.),
                    ],
                    texture: None,
                    blend_mode: 1,
                    clip: None,
                },
            ],
            filter: TextureFilter::Linear,
        };
        let mut serial = RenderTarget::new(size).unwrap();
        let mut surface = Surface::new(size).unwrap();
        let mut parallel = Surface::new(size).unwrap();
        assert!(serial.draw_list(&frame, Some(&canvas)).unwrap());
        assert!(
            SoftwareBackend::render_into(&frame, &canvas.frame_canvas(), &mut surface).unwrap()
        );
        render_in_bands(&frame, canvas.tiles(), &mut parallel, 4).unwrap();
        assert_eq!(&surface, serial.tiles()[0].as_ref().unwrap());
        assert_eq!(parallel, surface);
    }
}
