use super::{RenderTarget, raster};
use crate::{
    Result,
    host::graphics::{DrawList, FrameCanvas, GraphicsBackend, Surface},
};

impl DrawList {
    /// Draw over an existing software target without clearing it. A source
    /// canvas is required only for commands referencing canvas tiles.
    pub fn render_into(
        &self,
        output: &mut RenderTarget,
        canvas: Option<&RenderTarget>,
    ) -> Result<bool> {
        self.render_at(output, canvas, [0; 2])
    }

    fn render_at(
        &self,
        output: &mut RenderTarget,
        canvas: Option<&RenderTarget>,
        origin: [u32; 2],
    ) -> Result<bool> {
        let mut touched = false;
        for command in &self.commands {
            touched |= raster::draw_command(output, canvas, command, self.filter, origin)?;
        }
        Ok(touched)
    }
}

/// 不访问窗口或设备的软件图形后端。
#[derive(Default)]
pub struct SoftwareBackend;

impl SoftwareBackend {
    /// Draw over an existing software target without clearing it.
    pub fn render_into(
        frame: &DrawList,
        canvas: &FrameCanvas<'_>,
        output: &mut RenderTarget,
    ) -> Result<bool> {
        frame.render_into(output, Some(canvas.target()))
    }
}

impl GraphicsBackend for SoftwareBackend {
    fn render(
        &mut self,
        frame: &DrawList,
        canvas: &mut FrameCanvas<'_>,
        size: [u32; 2],
    ) -> Result<Surface> {
        let mut output = RenderTarget::new(size)?;
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
            render_in_bands(frame, canvas.target(), &mut output, workers)?;
        }
        Ok(output.into_single_surface())
    }
}

fn render_in_bands(
    frame: &DrawList,
    canvas: &RenderTarget,
    output: &mut RenderTarget,
    workers: usize,
) -> Result<()> {
    let [width, height] = output.size();
    let rows = height.div_ceil(workers as u32);
    // 各分块使用同一全局像素坐标；不平移顶点或改变命令顺序。
    let render_band = |top| -> Result<RenderTarget> {
        let mut band = RenderTarget::new([width, rows.min(height - top)])?;
        frame.render_at(&mut band, Some(canvas), [0, top])?;
        Ok(band)
    };
    let mut copy_band = |top, band: RenderTarget| {
        if band.dirty_tiles()[0] {
            let surface = band.tiles()[0].as_ref().unwrap();
            output.copy_surface(surface, surface.bounds(), [0, top as i32]);
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
        let canvas = RenderTarget::from_tiles([2, 2], [1, 1], vec![Some(texture)]).unwrap();
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
        let mut parallel = serial.clone();
        assert!(frame.render_into(&mut serial, Some(&canvas)).unwrap());
        render_in_bands(&frame, &canvas, &mut parallel, 4).unwrap();
        assert_eq!(parallel, serial);
    }
}
