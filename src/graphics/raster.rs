use super::{Drawables, ProjectedVertex, Projection, Rect, Surface};
use crate::{Error, Result};

/// 对归一化 UV 使用镜像寻址的单级纹理过滤。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureFilter {
    Point,
    Linear,
}

impl Surface {
    /// 按整数像素中心及左／上边包含规则绘制纯色三角形，使用设备普通 Alpha 混合。
    pub fn blend_triangle(&mut self, points: [[f32; 2]; 3], argb: u32) -> bool {
        self.blend_triangle_at(points, argb, [0; 2], None)
    }

    pub(super) fn blend_triangle_at(
        &mut self,
        points: [[f32; 2]; 3],
        argb: u32,
        origin: [u32; 2],
        clip: Option<Rect>,
    ) -> bool {
        if argb >> 24 == 0 {
            return false;
        }
        let color = argb.to_le_bytes().map(f64::from);
        self.rasterize(points, origin, clip, |_| Some(color))
    }

    /// 以 RHW 透视插值顶点颜色；不执行深度裁剪或深度缓冲。
    pub fn blend_projected_triangle(&mut self, vertices: [ProjectedVertex; 3]) -> bool {
        self.blend_projected_triangle_at(vertices, [0; 2], None)
    }

    pub(super) fn blend_projected_triangle_at(
        &mut self,
        vertices: [ProjectedVertex; 3],
        origin: [u32; 2],
        clip: Option<Rect>,
    ) -> bool {
        let Some(interpolation) = Interpolation::new(vertices) else {
            return false;
        };
        self.rasterize(
            vertices.map(|v| [v.position[0], v.position[1]]),
            origin,
            clip,
            |point| Some(interpolation.color(interpolation.weights(point)?)),
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
        self.blend_textured_triangle_at(vertices, uv, texture, filter, [0; 2], None)
    }

    pub(super) fn blend_textured_triangle_at(
        &mut self,
        vertices: [ProjectedVertex; 3],
        uv: [[f32; 2]; 3],
        texture: &Self,
        filter: TextureFilter,
        origin: [u32; 2],
        clip: Option<Rect>,
    ) -> bool {
        if texture.size.contains(&0) || uv.iter().flatten().any(|n| !n.is_finite()) {
            return false;
        }
        let Some(interpolation) = Interpolation::new(vertices) else {
            return false;
        };
        self.rasterize(
            vertices.map(|v| [v.position[0], v.position[1]]),
            origin,
            clip,
            |point| {
                let weights = interpolation.weights(point)?;
                let coordinates = std::array::from_fn(|axis| {
                    if uv.iter().all(|p| p[axis] == uv[0][axis]) {
                        f64::from(uv[0][axis])
                    } else {
                        (0..3).map(|i| f64::from(uv[i][axis]) * weights[i]).sum()
                    }
                });
                if coordinates.iter().any(|n: &f64| !n.is_finite()) {
                    return None;
                }
                let sample = texture.sample(coordinates, filter);
                let color = interpolation.color(weights);
                Some(std::array::from_fn(|i| sample[i] * color[i] / 255.))
            },
        )
    }

    fn rasterize(
        &mut self,
        points: [[f32; 2]; 3],
        origin: [u32; 2],
        clip: Option<Rect>,
        mut shade: impl FnMut([f64; 2]) -> Option<[f64; 4]>,
    ) -> bool {
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
        }
        let lower: [u32; 2] = std::array::from_fn(|i| {
            points
                .iter()
                .map(|p| p[i])
                .fold(f64::INFINITY, f64::min)
                .ceil()
                .clamp(origin[i] as f64, (origin[i] + self.size[i]) as f64) as u32
        });
        let upper: [u32; 2] = std::array::from_fn(|i| {
            points
                .iter()
                .map(|p| p[i])
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .clamp(origin[i] as f64, (origin[i] + self.size[i]) as f64) as u32
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
        let edges = [
            (points[0], points[1]),
            (points[1], points[2]),
            (points[2], points[0]),
        ];
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
                for (target, value) in self.pixels[at..at + 4].iter_mut().zip(source) {
                    *target = ((value * source[3] + f64::from(*target) * (255. - source[3])) / 255.)
                        .round() as u8;
                }
                touched = true;
            }
        }
        touched
    }

    fn sample(&self, uv: [f64; 2], filter: TextureFilter) -> [f64; 4] {
        // 先缩到镜像的两个周期内，避免大 UV 乘以尺寸后超出索引范围。
        let position: [f64; 2] =
            std::array::from_fn(|i| uv[i].rem_euclid(2.) * self.size[i] as f64);
        if filter == TextureFilter::Point {
            return self.texel(position.map(|v| v.floor() as i64));
        }
        let position = position.map(|v| v - 0.5);
        let lower = position.map(|v| v.floor() as i64);
        let fraction: [f64; 2] = std::array::from_fn(|i| position[i] - lower[i] as f64);
        let mut sample = [0.; 4];
        for y in 0..2 {
            for x in 0..2 {
                let weight = if x == 0 {
                    1. - fraction[0]
                } else {
                    fraction[0]
                } * if y == 0 {
                    1. - fraction[1]
                } else {
                    fraction[1]
                };
                let texel = self.texel([lower[0] + x, lower[1] + y]);
                for (value, texel) in sample.iter_mut().zip(texel) {
                    *value += texel * weight;
                }
            }
        }
        sample
    }

    fn texel(&self, at: [i64; 2]) -> [f64; 4] {
        let at: [usize; 2] = std::array::from_fn(|i| {
            let size = i64::from(self.size[i]);
            let value = at[i].rem_euclid(size * 2);
            if value < size {
                value as usize
            } else {
                (size * 2 - value - 1) as usize
            }
        });
        let offset = at[1] * self.stride() + at[0] * 4;
        std::array::from_fn(|i| f64::from(self.pixels[offset + i]))
    }
}

struct Interpolation {
    points: [[f64; 2]; 3],
    rhw: [f64; 3],
    colors: [[f64; 4]; 3],
    area: f64,
}

impl Interpolation {
    fn new(vertices: [ProjectedVertex; 3]) -> Option<Self> {
        if vertices.iter().any(|v| !v.rhw.is_finite() || v.rhw <= 0.) {
            return None;
        }
        let points = vertices.map(|v| [f64::from(v.position[0]), f64::from(v.position[1])]);
        let area = edge(points[0], points[1], points[2]);
        if area == 0. || !area.is_finite() {
            return None;
        }
        Some(Self {
            points,
            rhw: vertices.map(|v| f64::from(v.rhw)),
            colors: vertices.map(|v| v.color.to_le_bytes().map(f64::from)),
            area,
        })
    }

    fn weights(&self, point: [f64; 2]) -> Option<[f64; 3]> {
        // 插值使用未对齐的顶点；八位亚像素仅决定像素覆盖。
        let mut weights = std::array::from_fn(|i| {
            edge(self.points[(i + 1) % 3], self.points[(i + 2) % 3], point) / self.area
                * self.rhw[i]
        });
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

impl Drawables {
    /// 按登记顺序与基点 Z 绘制非纹理三角形；纹理图元需要 render_frame 的独立画布。
    pub fn render_triangles(
        &mut self,
        target: &mut super::RenderTarget,
        projection: &Projection,
        parent: [f32; 16],
    ) -> Result<bool> {
        self.render_to(target, None, projection, parent, TextureFilter::Point, None)
    }

    /// 读取画布面块并写入独立的最终帧；过滤显式选择，连续调用继续混合输出。
    pub fn render_frame(
        &mut self,
        output: &mut super::RenderTarget,
        canvas: &super::RenderTarget,
        projection: &Projection,
        parent: [f32; 16],
        filter: TextureFilter,
    ) -> Result<bool> {
        self.render_to(output, Some(canvas), projection, parent, filter, None)
    }

    /// 按图元基点 Z 选择 [lower, upper)；非正上界不绘制。
    pub fn render_depth_range(
        &mut self,
        output: &mut super::RenderTarget,
        canvas: &super::RenderTarget,
        projection: &Projection,
        parent: [f32; 16],
        filter: TextureFilter,
        range: [f32; 2],
    ) -> Result<bool> {
        if range[1] <= 0. {
            return Ok(false);
        }
        self.render_to(
            output,
            Some(canvas),
            projection,
            parent,
            filter,
            Some(range),
        )
    }

    fn render_to(
        &mut self,
        output: &mut super::RenderTarget,
        canvas: Option<&super::RenderTarget>,
        projection: &Projection,
        parent: [f32; 16],
        filter: TextureFilter,
        range: Option<[f32; 2]>,
    ) -> Result<bool> {
        self.visit_frame(projection, parent, range, |command| {
            draw_command(output, canvas, &command, filter)
        })
    }
}

pub(super) fn draw_command(
    output: &mut super::RenderTarget,
    canvas: Option<&super::RenderTarget>,
    command: &super::DrawCommand,
    filter: TextureFilter,
) -> Result<bool> {
    let vertices = command.vertices;
    validate_depth(vertices)?;
    if let Some(source) = &command.texture {
        let texture = match &source.source {
            super::TextureSource::Canvas(tile) => canvas
                .ok_or_else(|| Error::new(0, "纹理图元需要独立画布输入与帧输出"))?
                .tiles()
                .get(*tile as usize)
                .and_then(Option::as_ref),
            super::TextureSource::Image(image) => Some(image.as_ref()),
        };
        if let Some(texture) = texture {
            if vertices.iter().any(|v| !v.rhw.is_finite() || v.rhw <= 0.) {
                return Err(Error::new(0, "纹理插值需要有限的正 RHW"));
            }
            return Ok(output.blend_textured_triangle_clipped(
                vertices,
                source.uv,
                texture,
                filter,
                command.clip,
            ));
        }
    }
    draw_colors(output, vertices, command.clip)
}

pub(super) fn validate_depth(vertices: [ProjectedVertex; 3]) -> Result<()> {
    if vertices
        .iter()
        .any(|v| v.position[2].is_finite() && !(0.0..=1.0).contains(&v.position[2]))
    {
        return Err(Error::new(0, "三角形深度裁剪尚未接入"));
    }
    Ok(())
}

fn draw_colors(
    target: &mut super::RenderTarget,
    vertices: [ProjectedVertex; 3],
    clip: Option<Rect>,
) -> Result<bool> {
    if vertices.iter().all(|v| v.color == vertices[0].color) {
        Ok(target.blend_triangle_clipped(
            vertices.map(|v| [v.position[0], v.position[1]]),
            vertices[0].color,
            clip,
        ))
    } else {
        if vertices.iter().any(|v| !v.rhw.is_finite() || v.rhw <= 0.) {
            return Err(Error::new(0, "颜色插值需要有限的正 RHW"));
        }
        Ok(target.blend_projected_triangle_clipped(vertices, clip))
    }
}

fn edge(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}
