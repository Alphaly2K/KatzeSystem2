use super::{DrawCommand, Rect, Surface};
use crate::{Error, Result};
use std::sync::Arc;

pub(crate) struct RWipe {
    pub far: f32,
    pub duration: u32,
    pub mode: u32,
    pub state: u32,
    pub now: u32,
    start: u32,
    deadline: u32,
    source: Arc<Surface>,
    mask: Option<Surface>,
}

impl RWipe {
    pub fn captured(source: Surface) -> Self {
        Self {
            far: 1_000_000.,
            duration: 0,
            mode: 0,
            state: 0,
            now: 0,
            start: 0,
            deadline: 0,
            source: Arc::new(source),
            mask: None,
        }
    }

    pub fn prepare_mask(&mut self, mask: &Surface, rect: Rect) -> Result<()> {
        let mut image = Surface::new(self.source.size())?;
        super::resample::copy_scaled(&mut image, Some(mask), mask.bounds(), rect)?;
        // 原绘制器顺序读取底端起始的遮罩行，直接写入顶端起始的纹理行。
        super::resample::flip_rows(&mut image);
        self.mask = Some(image);
        Ok(())
    }

    pub fn restart(&mut self, now: u32) {
        self.start = now;
        self.deadline = now.wrapping_add(self.duration);
    }

    pub fn set_state(&mut self, value: u32, now: u32) {
        self.state = value;
        if value != 0 {
            self.restart(now);
        }
    }

    pub fn sample(&mut self, elapsed: u32, hard: bool) -> Result<()> {
        let Some(mask) = &self.mask else {
            return Ok(());
        };
        if self.duration == 0 {
            return Err(Error::new(0, "遮罩划扫时长不能为零"));
        }
        let progress = elapsed
            .min(self.duration)
            .wrapping_shl(if hard { 8 } else { 9 })
            / self.duration;
        let table: [u8; 256] = std::array::from_fn(|value| {
            (255 - (value as i32 + (progress as i32).clamp(0, 512) - 256).clamp(0, 255)) as u8
        });
        for (pixel, mask) in Arc::make_mut(&mut self.source)
            .pixels_mut()
            .chunks_exact_mut(4)
            .zip(mask.pixels().chunks_exact(4))
        {
            if !hard {
                pixel[3] = table[mask[0] as usize];
            } else if i32::from(mask[0]) >= 256i32.wrapping_sub(progress as i32) {
                // 硬切模式不会恢复之前已经清除的像素。
                pixel[3] = 0;
            }
        }
        Ok(())
    }

    pub fn frame(&mut self) -> Result<[DrawCommand; 2]> {
        if self.state != 0 {
            let elapsed = self.now.wrapping_sub(self.start).min(self.deadline);
            self.sample(elapsed, self.mode != 0)?;
        }
        let mut commands = super::mosaic::screen_commands(self.source.clone(), 255);
        for command in &mut commands {
            for vertex in &mut command.vertices {
                vertex.position[2] = 0.5;
            }
        }
        if self.now >= self.deadline {
            self.state = 0;
        }
        Ok(commands)
    }

    pub fn background(&self) -> [DrawCommand; 2] {
        let mut commands = super::mosaic::screen_commands(self.source.clone(), 255);
        for command in &mut commands {
            command.texture = None;
            for vertex in &mut command.vertices {
                vertex.color = 0xff00_0000;
            }
        }
        commands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_rows_and_time_reversal_preserve_smooth_and_cumulative_cutout() {
        let mut mask = Surface::new([4, 2]).unwrap();
        for (pixel, blue) in mask
            .pixels_mut()
            .chunks_exact_mut(4)
            .zip([255, 128, 64, 0, 0, 64, 128, 255])
        {
            pixel.copy_from_slice(&[blue, 29, 75, 255]);
        }
        for hard in [false, true] {
            let mut source = Surface::new([4, 2]).unwrap();
            source.fill_rect(source.bounds(), 0x1734_5678);
            let mut effect = RWipe::captured(source);
            effect.duration = 8;
            effect.prepare_mask(&mask, mask.bounds()).unwrap();
            effect.sample(0, false).unwrap();
            for (elapsed, smooth, cutout) in [
                (0, [255u8; 4], [255; 4]),
                (2, [255, 255, 255, 128], [255, 255, 255, 0]),
                (4, [255, 191, 127, 0], [255, 255, 0, 0]),
                (7, [63, 0, 0, 0], [255, 0, 0, 0]),
                (8, [0; 4], [0; 4]),
                (0, [255; 4], [0; 4]),
            ] {
                effect.sample(elapsed, hard).unwrap();
                let alpha = if hard { cutout } else { smooth };
                for (x, alpha) in alpha.into_iter().enumerate() {
                    assert_eq!(
                        effect.source.pixel(x as u32, 0),
                        Some((u32::from(alpha) << 24) | 0x0034_5678)
                    );
                }
            }
            effect.duration = 0;
            assert!(effect.sample(0, hard).is_err());
        }
    }
}
