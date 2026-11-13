use super::super::{Transform, drawable::arithmetic as f};
use crate::{Error, Result, read};

#[derive(Clone)]
pub(super) struct Snapshot {
    pub base: [f32; 3],
    pub position: [f32; 3],
    pub scale: [f32; 3],
    pub matrix: [f32; 16],
    pub color: u32,
    pub display: u32,
}

impl Snapshot {
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let bytes = read::range(bytes, 0, 128)?;
        let vector = |at| {
            std::array::from_fn(|i| {
                f32::from_bits(u32::from_le_bytes(
                    bytes[at + i * 4..at + i * 4 + 4].try_into().unwrap(),
                ))
            })
        };
        Ok(Self {
            base: vector(12),
            position: vector(28),
            scale: vector(44),
            matrix: std::array::from_fn(|i| {
                f32::from_bits(read::u32_at(bytes, 60 + i * 4).unwrap())
            }),
            color: read::u32_at(bytes, 124)?,
            display: read::u32_at(bytes, 8)?,
        })
    }
}

pub(super) struct History {
    pub mode: u32,
    interval: usize,
    alpha: u32,
    scale: [f32; 3],
    offset: [f32; 3],
    depth: i32,
    pub history: Vec<Snapshot>,
    size: [f32; 2],
}

impl History {
    pub fn new(bytes: &[u8], source: Snapshot, size: [f32; 2]) -> Result<Self> {
        let words: [u32; 13] = std::array::from_fn(|i| read::u32_at(bytes, i * 4).unwrap_or(0));
        read::range(bytes, 0, 52)?;
        let mode = words[11];
        let interval = match mode {
            1 => 2,
            2..=5 => 1,
            _ => words[1],
        } as usize;
        let count = (words[0] as usize)
            .checked_mul(interval)
            .filter(|&n| n < read::MAX_ITEMS)
            .ok_or_else(|| Error::new(0, "历史绘图状态数量超过上限"))?;
        if interval == 0 {
            return Err(Error::new(0, "历史绘图采样间隔不能为零"));
        }
        let offset = [words[7], words[8], words[9]].map(f32::from_bits);
        let mut history = vec![source; count + 1];
        for i in 1..=count {
            history[i].display = 0;
            history[i].position =
                std::array::from_fn(|axis| f(2, history[i - 1].position[axis], offset[axis]));
        }
        Ok(Self {
            mode,
            interval,
            alpha: words[2],
            scale: [words[3], words[4], words[5]].map(f32::from_bits),
            offset,
            depth: words[12] as i32,
            history,
            size,
        })
    }

    pub fn created(&self, index: usize) -> bool {
        self.mode != 0 || self.sampled(index)
    }

    pub fn sampled(&self, index: usize) -> bool {
        index.is_multiple_of(self.interval)
    }

    pub fn advance(&mut self, source: Snapshot) {
        for i in (1..self.history.len()).rev() {
            self.history[i] = self.history[i - 1].clone();
            self.history[i].position =
                std::array::from_fn(|axis| f(1, self.history[i].position[axis], self.offset[axis]));
        }
        self.history[0] = source;
    }

    pub fn sample(&self) -> Vec<(usize, Snapshot)> {
        let count = self.history.len() - 1;
        let mut output = Vec::new();
        if self.mode > 5 {
            return output;
        }
        let strip = if self.mode >= 4 {
            self.strip().2 as f32
        } else {
            self.size[0]
        };
        if self.mode >= 4 && count != 0 {
            let mut state = self.history[0].clone();
            state.matrix = rotate(
                state.matrix,
                angle(&self.history[0], &self.history[1], true),
            );
            output.push((0, state));
        }
        for i in 1..=count {
            if !self.created(i) || (matches!(self.mode, 2 | 3) && i == count) {
                continue;
            }
            let mut state = self.history[i].clone();
            let z = self.depth.wrapping_mul(i as i32) as f32;
            state.base[2] = f(1, state.base[2], z);
            let target = if self.mode == 0 {
                std::array::from_fn(|axis| f(3, state.scale[axis], self.scale[axis]))
            } else {
                self.scale
            };
            state.scale =
                std::array::from_fn(|axis| lerp(state.scale[axis], target[axis], count, i));
            let connect = match self.mode {
                1 => i % 2 != 0,
                2 | 3 => true,
                4 | 5 => i < count,
                _ => false,
            };
            if connect {
                let previous = &self.history[i - 1];
                let next = &self.history[i + 1];
                let vertical = self.mode >= 4;
                state.matrix = rotate(
                    state.matrix,
                    angle(
                        previous,
                        if vertical { &self.history[i] } else { next },
                        vertical,
                    ),
                );
                let distance = distance(previous.position, next.position);
                let distance = if self.mode == 2 {
                    f(3, distance, 0.5)
                } else {
                    distance
                };
                state.scale[usize::from(vertical)] = f(4, f(1, strip, distance), strip);
            } else if self.mode >= 4 {
                state.matrix = rotate(
                    state.matrix,
                    angle(&self.history[i - 1], &self.history[i], true),
                );
            }
            if self.mode >= 4 {
                state.base[1] = if i == count { 0. } else { f(3, strip, 0.5) };
            }
            let alpha = lerp((state.color >> 24) as f32, self.alpha as f32, count, i);
            state.color = (state.color & 0x00ff_ffff) | (truncate(alpha) as u32).wrapping_shl(24);
            output.push((i, state));
        }
        output
    }

    fn strip(&self) -> (usize, i32, i32) {
        let axis = usize::from(self.mode == 4);
        let size = truncate(self.size[axis]);
        let odd = size & 1;
        (axis, size / 2 - i32::from(odd == 0), (odd ^ 1) + 1)
    }

    pub fn geometry_states(&self, source: &Transform) -> Vec<(usize, Transform)> {
        if self.mode < 4 || self.mode > 5 {
            return Vec::new();
        }
        let (axis, half, strip) = self.strip();
        let count = self.history.len() - 1;
        let mut result = Vec::new();
        let mut scale = [1.; 3];
        for i in 1..count {
            let mut transform = source.clone();
            let mut position = source.position();
            position[axis] = f(1, position[axis], half as f32);
            transform.set_position(position);
            scale[axis] = f(4, strip as f32, self.size[axis]);
            transform.set_scale(scale);
            result.push((i, transform));
        }
        let mut last = source.clone();
        let mut position = source.position();
        position[axis] = f(1, position[axis], half.wrapping_add(strip) as f32);
        last.set_position(position);
        result.push((count, last));
        let mut first = source.clone();
        scale[axis] = f(
            4,
            f(2, self.size[axis], half.wrapping_sub(strip) as f32),
            self.size[axis],
        );
        first.set_scale(scale);
        result.push((0, first));
        result
    }
}

fn lerp(from: f32, to: f32, count: usize, index: usize) -> f32 {
    if index == 0 {
        from
    } else if index >= count {
        to
    } else {
        f(
            1,
            f(3, f(4, index as f32, count as f32), f(2, to, from)),
            from,
        )
    }
}
fn distance(from: [f32; 3], to: [f32; 3]) -> f32 {
    let d: [f32; 3] = std::array::from_fn(|axis| f(2, from[axis], to[axis]));
    let sum = f(
        1,
        f(1, f(3, d[1], d[1]), f(3, d[0], d[0])),
        f(3, d[2], d[2]),
    );
    (sum as f64).sqrt() as f32
}
fn angle(from: &Snapshot, to: &Snapshot, vertical: bool) -> f32 {
    let dx = f(2, to.position[0], from.position[0]);
    let dy = f(2, to.position[1], from.position[1]);
    let angle = (dy as f64).atan2(dx as f64) as f32;
    if vertical {
        f(2, angle, std::f32::consts::FRAC_PI_2)
    } else {
        angle
    }
}
fn rotate(matrix: [f32; 16], angle: f32) -> [f32; 16] {
    let (sin, cos) = super::super::transform::sin_cos(angle);
    let mut rotation = *Transform::default().matrix();
    rotation[0] = cos;
    rotation[1] = sin;
    rotation[4] = -sin;
    rotation[5] = cos;
    crate::kcs::multiply_matrix(matrix, rotation)
}
fn truncate(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}
