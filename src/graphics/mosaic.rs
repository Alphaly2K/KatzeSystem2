use super::{DrawCommand, FrameTexture, ProjectedVertex, Rect, Surface, TextureSource};
use crate::{Error, Result};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MosaicKind {
    #[default]
    Dual,
    Fade,
}

#[derive(Clone)]
struct Level {
    image: Arc<Surface>,
    cell: u32,
    time: u32,
    source: usize,
    opacity: u8,
}

#[derive(Default)]
pub(crate) struct Mosaics {
    pub entries: BTreeMap<u32, Mosaic>,
}

impl Mosaics {
    pub fn remove(&mut self, id: u32) {
        self.entries.remove(&id);
    }
}

#[derive(Default)]
pub(crate) struct Mosaic {
    pub kind: MosaicKind,
    opacity: u8,
    pub far: f32,
    pub duration: u32,
    pub block: i32,
    pub algorithm: u32,
    pub state: u32,
    pub now: u32,
    pub start: u32,
    pub deadline: u32,
    levels: Vec<Level>,
    frames: [Option<Arc<Surface>>; 2],
    last_time: Option<u32>,
    last_source: usize,
}

impl Mosaic {
    pub fn capture(&mut self, index: usize, surface: Surface) -> Result<()> {
        let image = Arc::new(surface);
        self.frames[index] = Some(image.clone());
        let mut levels = self.levels(image, index, self.duration >> 1)?;
        if index == 0 {
            self.levels = levels;
        } else {
            let times: Vec<_> = levels.iter().map(|level| level.time).collect();
            levels.reverse();
            // 时间随成对交换移位，奇数级别表的中项保留原值。
            let count = levels.len();
            for (index, (level, time)) in levels.iter_mut().zip(times).enumerate() {
                level.time = if count % 2 != 0 && index == count / 2 {
                    time
                } else {
                    time.wrapping_add(self.duration >> 1)
                };
            }
            self.levels.extend(levels);
            self.sample(0)?;
        }
        Ok(())
    }

    pub fn fading(surface: Surface) -> Self {
        Self {
            kind: MosaicKind::Fade,
            far: 1_000_000.,
            frames: [Some(Arc::new(surface)), None],
            ..Default::default()
        }
    }

    pub fn prepare_fade(&mut self) -> Result<()> {
        let image = self.frames[0].as_ref().unwrap().clone();
        self.levels = self.levels(image, 0, self.duration)?;
        self.sample(0)?;
        Ok(())
    }

    fn levels(&self, image: Arc<Surface>, source: usize, steps: u32) -> Result<Vec<Level>> {
        let size = image.size();
        let mut levels = vec![Level {
            image,
            cell: 1,
            time: 0,
            source,
            opacity: 255,
        }];
        let delta = self.block as f32 - 1.;
        let mut previous = 1;
        for time in 1..steps {
            let fraction = time as f32 / steps as f32;
            let cell = (fraction * delta + 1.) as i32;
            if cell == previous {
                continue;
            }
            if cell <= 0 {
                return Err(Error::new(0, "马赛克块尺寸必须为正数"));
            }
            let cell = cell as u32;
            let small = size.map(|value| value.div_ceil(cell) + 1);
            let image = Arc::new(scale(&levels.last().unwrap().image, small)?);
            levels.push(Level {
                image,
                cell,
                time,
                source,
                opacity: if self.kind == MosaicKind::Fade {
                    !((fraction * 255.) as u8)
                } else {
                    255
                },
            });
            previous = cell as i32;
        }
        Ok(levels)
    }

    pub fn set_state(&mut self, state: u32, now: u32) {
        self.state = state;
        if state != 0 {
            self.restart(now);
        }
        self.last_time = None;
    }

    pub fn restart(&mut self, now: u32) {
        self.start = now;
        self.deadline = now.wrapping_add(self.duration);
    }

    fn sample(&mut self, elapsed: u32) -> Result<usize> {
        // 首次提交固定使用首级；越过整个级别表时保留上次结果。
        let selected = if self.last_time.is_none() {
            self.levels.first()
        } else {
            self.levels
                .iter()
                .position(|level| level.time > elapsed)
                .map(|index| &self.levels[index.saturating_sub(1)])
        };
        let Some(level) = selected else {
            if self.kind == MosaicKind::Fade {
                self.opacity = 0;
            }
            return Ok(self.last_source);
        };
        self.opacity = level.opacity;
        if self.last_time == Some(level.time) {
            return Ok(self.last_source);
        }
        let size = self.frames[level.source].as_ref().unwrap().size();
        let image = if self.algorithm == 0 {
            let mut image = Surface::new(size)?;
            let small = level.image.size();
            let half = level.cell / 2;
            // 纵向截断按来源行的整块起点计算，尾部再用最后一行填满。
            let mut rows = vec![0; half.min(size[1]) as usize];
            for sy in 1..small[1].saturating_sub(1) {
                let count = size[1]
                    .saturating_sub(sy.saturating_mul(level.cell))
                    .min(level.cell);
                rows.extend(std::iter::repeat_n(sy, count as usize));
            }
            rows.resize(size[1] as usize, small[1] - 1);
            for (y, sy) in rows.into_iter().enumerate() {
                for x in 0..size[0] {
                    let sx = if x < half {
                        0
                    } else {
                        (x - half) / level.cell + 1
                    }
                    .min(small[0] - 1);
                    let pixel = level.image.pixel(sx, sy).unwrap() | 0xff00_0000;
                    let at = y * image.stride() + x as usize * 4;
                    image.pixels_mut()[at..at + 4].copy_from_slice(&pixel.to_le_bytes());
                }
            }
            image
        } else {
            scale(&level.image, size)?
        };
        self.frames[level.source] = Some(Arc::new(image));
        self.last_time = Some(level.time);
        self.last_source = level.source;
        Ok(level.source)
    }

    pub fn frame(&mut self) -> Result<Option<(Arc<Surface>, u8)>> {
        let selected = if self.state != 0 {
            let elapsed = self.now.wrapping_sub(self.start).min(self.deadline);
            self.sample(elapsed)?
        } else {
            0
        };
        let opacity = if self.state == 0 { 255 } else { self.opacity };
        let frame = self.frames[selected]
            .as_ref()
            .map(|frame| (frame.clone(), opacity));
        if self.now >= self.deadline {
            self.state = 0;
        }
        Ok(frame)
    }
}

fn scale(source: &Surface, size: [u32; 2]) -> Result<Surface> {
    let mut source = source.clone();
    super::resample::flip_rows(&mut source);
    let mut image = Surface::new(size)?;
    let bounds = image.bounds();
    super::resample::copy_scaled(&mut image, Some(&source), source.bounds(), bounds)?;
    super::resample::flip_rows(&mut image);
    Ok(image)
}

pub(crate) fn screen_commands(image: Arc<Surface>, opacity: u8) -> [DrawCommand; 2] {
    let [width, height] = image.size().map(|value| value as f32);
    let vertices =
        [[0., 0.], [width, 0.], [0., height], [width, height]].map(|position| ProjectedVertex {
            position: [position[0] - 0.5, position[1] - 0.5, 0.],
            rhw: 1.,
            color: 0x00ff_ffff | (u32::from(opacity) << 24),
        });
    let uv = [[0., 0.], [1., 0.], [0., 1.], [1., 1.]];
    [[0, 1, 2], [2, 1, 3]].map(|indices| DrawCommand {
        vertices: indices.map(|i| vertices[i]),
        texture: Some(FrameTexture {
            source: TextureSource::Image(image.clone()),
            uv: indices.map(|i| uv[i]),
        }),
        clip: Some(Rect::from_xywh(0, 0, width as i32, height as i32)),
        blend_mode: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(index: u32) -> Surface {
        let mut image = Surface::new([8, 6]).unwrap();
        for y in 0..6 {
            for x in 0..8 {
                let pixel = 0xff00_0000
                    | ((x * 17 + y * 31 + index * 71) & 255)
                    | (((x * 37 + y * 7 + index * 109) & 255) << 8)
                    | (((x * 11 + y * 53 + index * 29) & 255) << 16);
                let offset = (y * 8 + x) as usize * 4;
                image.pixels_mut()[offset..offset + 4].copy_from_slice(&pixel.to_le_bytes());
            }
        }
        image
    }

    #[test]
    fn both_algorithms_keep_verified_level_pixels_and_the_last_cached_frame() {
        for (algorithm, expected) in [
            (
                0,
                [
                    0xff40_2c30,
                    0xff11_3b1b,
                    0xff7e_1b7b,
                    0xff1d_6d47,
                    0xff51_793f,
                ],
            ),
            (
                1,
                [
                    0xff00_0000,
                    0xff0a_2410,
                    0xff7e_1b7b,
                    0xff1d_6d47,
                    0xff50_783e,
                ],
            ),
        ] {
            let mut effect = Mosaic {
                duration: 8,
                block: 5,
                algorithm,
                ..Default::default()
            };
            effect.capture(0, source(0)).unwrap();
            effect.capture(1, source(1)).unwrap();
            effect.set_state(1, 0);
            for (frame, at, pixel) in [
                (0, [0, 0], expected[0]),
                (1, [1, 0], expected[1]),
                (3, [0, 4], expected[2]),
                (4, [0, 0], expected[3]),
                (6, [7, 5], expected[4]),
            ] {
                let selected = effect.sample(frame).unwrap();
                assert_eq!(
                    effect.frames[selected]
                        .as_ref()
                        .unwrap()
                        .pixel(at[0], at[1]),
                    Some(pixel)
                );
            }
            let last = effect.frames[1].clone();
            assert_eq!(effect.sample(100).unwrap(), 1);
            assert_eq!(effect.frames[1], last);
            effect.set_state(1, 100);
            assert_eq!(effect.sample(100).unwrap(), 0);
        }
    }

    #[test]
    fn an_odd_reverse_level_count_keeps_the_middle_timestamp() {
        let mut effect = Mosaic {
            duration: 11,
            block: 12,
            ..Default::default()
        };
        effect.capture(0, source(0)).unwrap();
        effect.capture(1, source(1)).unwrap();
        assert_eq!(
            effect
                .levels
                .iter()
                .map(|level| level.time)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5, 6, 2, 8, 9]
        );
        effect.set_state(1, 0);
        effect.sample(0).unwrap();
        assert_eq!(effect.sample(6).unwrap(), 1);
        assert_eq!(
            effect.frames[1].as_ref().unwrap().pixel(2, 0),
            Some(0xff3a_7a74)
        );
    }

    #[test]
    fn fading_mosaic_uses_level_opacity_and_becomes_transparent_past_the_table() {
        for algorithm in [0, 1] {
            let mut effect = Mosaic::fading(source(0));
            effect.duration = 8;
            effect.block = 5;
            effect.algorithm = algorithm;
            effect.prepare_fade().unwrap();
            effect.set_state(1, 0);
            for (elapsed, alpha) in [255, 255, 192, 192, 128, 128, 0, 0, 0]
                .into_iter()
                .enumerate()
            {
                effect.sample(elapsed as u32).unwrap();
                assert_eq!(effect.opacity, alpha);
            }
            assert_eq!(effect.last_time, Some(4));
        }
    }
}
