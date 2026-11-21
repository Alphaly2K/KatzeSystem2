use super::{ProjectedVertex, Rect, Surface, TextureAddress, TextureFilter};

#[derive(Clone, Copy, Default)]
pub(crate) struct RasterState {
    pub origin: [u32; 2],
    pub clip: Option<Rect>,
    pub blend_mode: u32,
    pub address: [super::TextureAddress; 2],
}

impl Surface {
    /// 按整数像素中心及左／上边包含规则绘制纯色三角形，使用设备普通 Alpha 混合。
    pub fn blend_triangle(&mut self, points: [[f32; 2]; 3], argb: u32) -> bool {
        self.blend_triangle_at(points, argb, RasterState::default())
    }

    pub(crate) fn blend_triangle_at(
        &mut self,
        points: [[f32; 2]; 3],
        argb: u32,
        state: RasterState,
    ) -> bool {
        if argb >> 24 == 0 {
            return false;
        }
        let color = argb.to_le_bytes().map(f64::from);
        self.rasterize(points, [1.; 3], state, |_| Some(color))
    }

    /// 以 RHW 透视插值顶点颜色，裁掉归一化深度区间外的像素；不使用深度缓冲。
    pub fn blend_projected_triangle(&mut self, vertices: [ProjectedVertex; 3]) -> bool {
        self.blend_projected_triangle_at(vertices, RasterState::default())
    }

    pub(crate) fn blend_projected_triangle_at(
        &mut self,
        vertices: [ProjectedVertex; 3],
        state: RasterState,
    ) -> bool {
        let Some(interpolation) = Interpolation::new(vertices) else {
            return false;
        };
        self.rasterize(
            vertices.map(|v| [v.position[0], v.position[1]]),
            vertices.map(|v| v.rhw),
            state,
            |point| Some(interpolation.color(interpolation.weights(interpolation.areas(point))?)),
        )
    }

    /// 透视插值 UV 与颜色，以镜像寻址取样，颜色相乘后作设备普通 Alpha 混合。
    pub fn blend_textured_triangle(
        &mut self,
        vertices: [ProjectedVertex; 3],
        uv: [[f32; 2]; 3],
        texture: &Self,
        filter: TextureFilter,
    ) -> bool {
        self.blend_textured_triangle_at(vertices, uv, texture, filter, RasterState::default())
    }

    pub(crate) fn blend_textured_triangle_at(
        &mut self,
        vertices: [ProjectedVertex; 3],
        uv: [[f32; 2]; 3],
        texture: &Self,
        filter: TextureFilter,
        state: RasterState,
    ) -> bool {
        if texture.size().contains(&0) || uv.iter().flatten().any(|n| !n.is_finite()) {
            return false;
        }
        let Some(interpolation) = Interpolation::new(vertices) else {
            return false;
        };
        let extent = texture.size().map(f64::from);
        let affine_point = filter == TextureFilter::Point
            && interpolation
                .rhw
                .iter()
                .all(|&rhw| rhw == interpolation.rhw[0]);
        self.rasterize(
            vertices.map(|v| [v.position[0], v.position[1]]),
            vertices.map(|v| v.rhw),
            state,
            |point| {
                let areas = interpolation.areas(point);
                let weights = interpolation.weights(areas)?;
                let coordinates = std::array::from_fn(|axis| {
                    let [a, b, c] = uv.map(|uv| f64::from(uv[axis]));
                    if affine_point {
                        // 点采样时在纹素坐标中先合计再除面积，避免边界落入前一个纹素。
                        let span = extent[axis];
                        a * span
                            + ((b - a) * span * areas[1] + (c - a) * span * areas[2])
                                / interpolation.area
                    } else {
                        (a + (b - a) * weights[1] + (c - a) * weights[2]) * extent[axis]
                    }
                });
                if coordinates.iter().any(|n: &f64| !n.is_finite()) {
                    return None;
                }
                let sample = texture.sample(coordinates, filter, state.address);
                let color = interpolation.color(weights);
                Some(std::array::from_fn(|i| sample[i] * color[i] / 255.))
            },
        )
    }

    fn rasterize(
        &mut self,
        points: [[f32; 2]; 3],
        mut rhw: [f32; 3],
        state: RasterState,
        mut shade: impl FnMut([f64; 2]) -> Option<[f64; 4]>,
    ) -> bool {
        let RasterState {
            origin,
            clip,
            blend_mode,
            ..
        } = state;
        if points.iter().flatten().any(|n| !n.is_finite()) {
            return false;
        }
        let mut points = points.map(|p| p.map(|v| (f64::from(v) * 256. + 0.5).floor() / 256.));
        let area = edge(points[0], points[1], points[2]);
        if area == 0. {
            return false;
        }
        if area < 0. {
            points.swap(1, 2);
            rhw.swap(1, 2);
        }
        let crosses_camera = rhw.iter().any(|&value| value < 0.);
        let lower: [u32; 2] = std::array::from_fn(|i| {
            if crosses_camera {
                return origin[i];
            }
            points
                .iter()
                .map(|p| p[i])
                .fold(f64::INFINITY, f64::min)
                .ceil()
                .clamp(origin[i] as f64, (origin[i] + self.size()[i]) as f64) as u32
        });
        let upper: [u32; 2] = std::array::from_fn(|i| {
            if crosses_camera {
                return origin[i] + self.size()[i];
            }
            points
                .iter()
                .map(|p| p[i])
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .clamp(origin[i] as f64, (origin[i] + self.size()[i]) as f64) as u32
        });
        let (lower, upper) = if let Some(clip) = clip {
            let lower = [
                lower[0].max(clip.left.max(0) as u32),
                lower[1].max(clip.top.max(0) as u32),
            ];
            let upper = [
                upper[0].min(clip.right.max(0) as u32),
                upper[1].min(clip.bottom.max(0) as u32),
            ];
            (lower, upper)
        } else {
            (lower, upper)
        };
        let mut edges = [
            (points[0], points[1]),
            (points[1], points[2]),
            (points[2], points[0]),
        ];
        // 负 W 顶点的对边反向，投影后的可见区域可能在原三角形外。
        for (edge, opposite) in edges.iter_mut().zip([2, 0, 1]) {
            if rhw[opposite] < 0. {
                *edge = (edge.1, edge.0);
            }
        }
        let included = edges.map(|(a, b)| b[1] < a[1] || (b[1] == a[1] && b[0] > a[0]));
        let stride = self.stride();
        let mut touched = false;
        for y in lower[1]..upper[1] {
            for x in lower[0]..upper[0] {
                let point = [x as f64, y as f64];
                if edges.iter().zip(included).any(|(&(a, b), included)| {
                    let value = edge(a, b, point);
                    value < 0. || (value == 0. && !included)
                }) {
                    continue;
                }
                let Some(source) = shade(point) else {
                    continue;
                };
                if source.iter().any(|n| !n.is_finite()) {
                    continue;
                }
                let source = source.map(|v| v.clamp(0., 255.));
                if source[3] <= 0. {
                    continue;
                }
                let at = (y - origin[1]) as usize * stride + (x - origin[0]) as usize * 4;
                for (target, value) in self.pixels_mut()[at..at + 4].iter_mut().zip(source) {
                    let destination = f64::from(*target);
                    let value = match blend_mode {
                        1 => value * source[3] / 255. + destination,
                        2 => value + destination * (255. - source[3]) / 255.,
                        3 => value * destination / 255.,
                        4 => value * (255. - destination) / 255.,
                        5 => value * destination / 255. + destination,
                        _ => (value * source[3] + destination * (255. - source[3])) / 255.,
                    };
                    *target = value.clamp(0., 255.).round() as u8;
                }
                touched = true;
            }
        }
        touched
    }

    fn sample(
        &self,
        texel: [f64; 2],
        filter: TextureFilter,
        address: [TextureAddress; 2],
    ) -> [f64; 4] {
        // 先缩到镜像的两个周期内，避免大 UV 乘以尺寸后超出索引范围。
        let position: [f64; 2] = std::array::from_fn(|i| {
            let period = f64::from(self.size()[i]) * 2.;
            if (0. ..period).contains(&texel[i]) {
                texel[i]
            } else {
                texel[i].rem_euclid(period)
            }
        });
        let resolve =
            |axis: usize, texel| texture_coordinate(texel, self.size()[axis], address[axis]);
        if filter == TextureFilter::Point {
            return self.texel(
                resolve(0, position[0].floor() as i64),
                resolve(1, position[1].floor() as i64),
            );
        }
        let position = position.map(|v| v - 0.5);
        let lower = position.map(|v| v.floor() as i64);
        let fraction: [f64; 2] = std::array::from_fn(|i| position[i] - lower[i] as f64);
        let x: [usize; 2] = std::array::from_fn(|i| resolve(0, lower[0] + i as i64));
        let y: [usize; 2] = std::array::from_fn(|i| resolve(1, lower[1] + i as i64));
        let mut sample = [0.; 4];
        for (y, wy) in y.into_iter().zip([1. - fraction[1], fraction[1]]) {
            for (x, wx) in x.into_iter().zip([1. - fraction[0], fraction[0]]) {
                let weight = wx * wy;
                let texel = self.texel(x, y);
                for (value, texel) in sample.iter_mut().zip(texel) {
                    *value += texel * weight;
                }
            }
        }
        sample
    }

    fn texel(&self, x: usize, y: usize) -> [f64; 4] {
        let offset = y * self.stride() + x * 4;
        std::array::from_fn(|i| f64::from(self.pixels()[offset + i]))
    }
}

fn texture_coordinate(value: i64, size: u32, address: TextureAddress) -> usize {
    let size = i64::from(size);
    let period = if address == TextureAddress::Wrap {
        size
    } else {
        size * 2
    };
    let value = if (0..period).contains(&value) {
        value
    } else {
        value.rem_euclid(period)
    };
    if value < size {
        value as usize
    } else {
        (period - value - 1) as usize
    }
}

struct Interpolation {
    points: [[f64; 2]; 3],
    rhw: [f64; 3],
    colors: [[f64; 4]; 3],
    area: f64,
    depth: Option<[f64; 3]>,
}

impl Interpolation {
    fn new(vertices: [ProjectedVertex; 3]) -> Option<Self> {
        if vertices.iter().any(|v| !v.rhw.is_finite()) || vertices.iter().all(|v| v.rhw < 0.) {
            return None;
        }
        let points = vertices.map(|v| {
            [v.position[0], v.position[1]].map(|n| (f64::from(n) * 256. + 0.5).floor() / 256.)
        });
        let area = edge(points[0], points[1], points[2]);
        if area == 0. || !area.is_finite() {
            return None;
        }
        Some(Self {
            points,
            // 设备入口把零 RHW 解释为一，负值仍参与裁剪和透视插值。
            rhw: vertices.map(|v| if v.rhw == 0. { 1. } else { f64::from(v.rhw) }),
            colors: vertices.map(|v| v.color.to_le_bytes().map(f64::from)),
            area,
            depth: needs_depth_clip(vertices).then(|| {
                vertices.map(|v| {
                    if v.position[2].is_finite() {
                        f64::from(v.position[2])
                    } else {
                        0.
                    }
                })
            }),
        })
    }

    fn areas(&self, point: [f64; 2]) -> [f64; 3] {
        // 插值与覆盖测试使用相同的八位亚像素顶点。
        std::array::from_fn(|i| edge(self.points[(i + 1) % 3], self.points[(i + 2) % 3], point))
    }

    fn weights(&self, areas: [f64; 3]) -> Option<[f64; 3]> {
        let mut weights = areas.map(|area| area / self.area);
        if let Some(depth) = self.depth {
            let value: f64 = (0..3).map(|i| depth[i] * weights[i]).sum();
            if !(0.0..=1.0).contains(&value) {
                return None;
            }
        }
        for (weight, rhw) in weights.iter_mut().zip(self.rhw) {
            *weight *= rhw;
        }
        let denominator: f64 = weights.iter().sum();
        if !denominator.is_finite() || denominator <= 0. {
            return None;
        }
        for value in &mut weights {
            *value /= denominator;
        }
        Some(weights)
    }

    fn color(&self, weights: [f64; 3]) -> [f64; 4] {
        std::array::from_fn(|channel| {
            if self
                .colors
                .iter()
                .all(|c| c[channel] == self.colors[0][channel])
            {
                self.colors[0][channel]
            } else {
                (0..3).map(|i| self.colors[i][channel] * weights[i]).sum()
            }
        })
    }
}

pub(crate) fn needs_depth_clip(vertices: [ProjectedVertex; 3]) -> bool {
    vertices
        .iter()
        .any(|v| v.rhw < 0. || v.position[2].is_finite() && !(0.0..=1.0).contains(&v.position[2]))
}

fn edge(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}
