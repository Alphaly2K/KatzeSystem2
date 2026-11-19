use super::super::{Rect, Surface, blend_pixel};

const ONE: i32 = 1 << 15;
const FRACTION: i32 = ONE - 1;

/// Rotation in radians and scale relative to the entry's base point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageListTransform {
    pub rotation: f32,
    pub scale: [f32; 2],
}

impl ImageListTransform {
    pub(in crate::graphics) fn is_identity(self) -> bool {
        self.rotation == 0. && self.scale == [1.; 2]
    }
}

pub(in crate::graphics) struct Affine {
    forward: [i32; 4],
    origin: [f32; 2],
    pivot: [f32; 2],
    size: [u32; 2],
}

impl Affine {
    pub(in crate::graphics) fn new(
        transform: ImageListTransform,
        origin: [f32; 2],
        base: [f32; 2],
        offset: [i32; 2],
        size: [u32; 2],
    ) -> Self {
        let phase = trunc(transform.rotation / std::f32::consts::TAU * ONE as f32);
        let sin = sine(phase);
        let cos = sine(8192i32.wrapping_sub(phase));
        let [x, y] = transform.scale.map(fixed);
        Self {
            forward: [mul(cos, x), mul(-sin, y), mul(sin, x), mul(cos, y)],
            origin,
            pivot: std::array::from_fn(|axis| base[axis] - offset[axis] as u32 as f32),
            size,
        }
    }

    fn corners(&self) -> [[i32; 2]; 4] {
        let [left, bottom] = self.pivot.map(|x| fixed(-x));
        let right = left.wrapping_add(self.size[0].wrapping_shl(15) as i32);
        let top = bottom.wrapping_add(self.size[1].wrapping_shl(15) as i32);
        [[left, bottom], [right, bottom], [left, top], [right, top]]
            .map(|point| map(self.forward, point))
    }

    pub(in crate::graphics) fn dirty_bounds(&self) -> Rect {
        let [x, y] = self.origin.map(trunc);
        let [left, top, right, bottom] = bounds(self.corners().map(|p| p.map(|n| n >> 15)));
        Rect {
            left: left.wrapping_add(x).wrapping_sub(1).max(0),
            top: top.wrapping_add(y).wrapping_sub(1).max(0),
            right: right.wrapping_add(x).wrapping_add(1),
            bottom: bottom.wrapping_add(y).wrapping_add(1),
        }
    }

    fn draw_bounds(&self) -> Rect {
        let [x, y] = self.origin.map(trunc);
        let corners = self.corners();
        let [l, t, r, b] = bounds(corners.map(|p| p.map(|n| n >> 15)));
        let [lo_x, lo_y, hi_x, hi_y] = bounds(corners).map(|n| i32::from(n & FRACTION != 0));
        Rect {
            left: l.wrapping_add(x).wrapping_sub(lo_x),
            top: t.wrapping_add(y).wrapping_sub(lo_y),
            right: r.wrapping_add(x).wrapping_add(hi_x),
            bottom: b.wrapping_add(y).wrapping_add(hi_y),
        }
    }

    pub(in crate::graphics) fn draw(
        &self,
        target: &mut Surface,
        source: &Surface,
        clip: Rect,
        opacity: u32,
    ) {
        let Some(region) = self
            .draw_bounds()
            .intersect(target.bounds())
            .and_then(|r| r.intersect(clip))
        else {
            return;
        };
        let [a, b, c, d] = self.forward;
        let determinant = (i64::from(a) * i64::from(d) - i64::from(b) * i64::from(c)) >> 15;
        let inverse = if determinant != 0 {
            let reciprocal = ((1i64 << 30) / determinant) as i32;
            [d, b.wrapping_neg(), c.wrapping_neg(), a].map(|n| mul(n, reciprocal))
        } else {
            self.forward
        };
        // The integer center correction is added directly to the fixed-point delta.
        let center = map(self.forward, self.pivot.map(fixed)).map(|n| n >> 15);
        let origin = self.origin.map(fixed);
        let pivot = self.pivot.map(trunc);
        let [width, height] = target.size().map(|n| n as usize);
        for y in region.top..region.bottom {
            for x in region.left..region.right {
                let position = [x, y];
                let delta = std::array::from_fn(|axis| {
                    position[axis]
                        .wrapping_shl(15)
                        .wrapping_sub(origin[axis])
                        .wrapping_add(center[axis])
                });
                let sample = map(inverse, delta);
                let at = std::array::from_fn(|axis| (sample[axis] >> 15).wrapping_add(pivot[axis]));
                let Some(pixel) =
                    interpolated_pixel(source, at, sample.map(|n| (n & FRACTION) as u32))
                else {
                    continue;
                };
                let start = ((height - 1 - y as usize) * width + x as usize) * 4;
                let target = &mut target.pixels_mut()[start..start + 4];
                let alpha = ((opacity as u16 as i16 as i32).wrapping_mul(i32::from(pixel[3]) + 1)
                    >> 8) as u8;
                if alpha == 0 && target[3] == 0 {
                    target[..3].copy_from_slice(&pixel[..3]);
                } else if alpha != 0 {
                    blend_pixel(target, &[pixel[0], pixel[1], pixel[2], alpha], 255);
                }
            }
        }
    }
}

fn bounds(points: [[i32; 2]; 4]) -> [i32; 4] {
    let [x, y] = points[0];
    points.into_iter().skip(1).fold(
        [x, y, x.wrapping_add(1), y.wrapping_add(1)],
        |[l, t, r, b], [x, y]| [l.min(x), t.min(y), r.max(x), b.max(y)],
    )
}

fn trunc(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn fixed(value: f32) -> i32 {
    let floor = value.floor();
    trunc(floor).wrapping_shl(15) | trunc((value - floor) * ONE as f32)
}

fn sine(phase: i32) -> i32 {
    let phase = phase & FRACTION;
    ((f64::from(phase) * (std::f64::consts::TAU / f64::from(ONE))).sin() * f64::from(ONE)) as i32
}

fn mul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 15) as i32
}

fn map([a, b, c, d]: [i32; 4], [x, y]: [i32; 2]) -> [i32; 2] {
    [
        mul(x, a).wrapping_add(mul(y, c)),
        mul(x, b).wrapping_add(mul(y, d)),
    ]
}

fn interpolated_pixel(source: &Surface, [x, y]: [i32; 2], [fx, fy]: [u32; 2]) -> Option<[u8; 4]> {
    let [width, height] = source.size();
    let (x, y) = (x as u32, y as u32);
    if x >= width || y >= height {
        return None;
    }
    let pixel = |x, y| source.pixel(x, height - 1 - y).unwrap().to_le_bytes();
    let first = pixel(x, y);
    let pixels = [
        first,
        if x + 1 < width {
            pixel(x + 1, y)
        } else {
            first
        },
        if y + 1 < height {
            pixel(x, y + 1)
        } else {
            first
        },
        if x + 1 < width && y + 1 < height {
            pixel(x + 1, y + 1)
        } else {
            first
        },
    ];
    if pixels.iter().all(|p| p[3] == 0) {
        return None;
    }
    let one = ONE as u32;
    let weights = [
        (one - fx) * (one - fy),
        fx * (one - fy),
        (one - fx) * fy,
        fx * fy,
    ]
    .map(|w| w >> 15);
    Some(std::array::from_fn(|channel| {
        (pixels
            .iter()
            .zip(weights)
            .map(|(p, w)| u32::from(p[channel]) * w)
            .sum::<u32>()
            >> 15) as u8
    }))
}
