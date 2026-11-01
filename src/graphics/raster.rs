use super::{Drawables, Projection, Surface};
use crate::{Error, Result};

impl Surface {
    /// 按整数像素中心及左／上边包含规则绘制纯色三角形，使用设备普通 Alpha 混合。
    pub fn blend_triangle(&mut self, points: [[f32; 2]; 3], argb: u32) -> bool {
        self.blend_triangle_at(points, argb, [0; 2])
    }

    pub(super) fn blend_triangle_at(
        &mut self,
        points: [[f32; 2]; 3],
        argb: u32,
        origin: [u32; 2],
    ) -> bool {
        if argb >> 24 == 0 || points.iter().flatten().any(|n| !n.is_finite()) {
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
        let edges = [
            (points[0], points[1]),
            (points[1], points[2]),
            (points[2], points[0]),
        ];
        let included = edges.map(|(a, b)| b[1] < a[1] || (b[1] == a[1] && b[0] > a[0]));
        let source = argb.to_le_bytes();
        let alpha = source[3] as u32;
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
                let at = (y - origin[1]) as usize * stride + (x - origin[0]) as usize * 4;
                for (target, source) in self.pixels[at..at + 4].iter_mut().zip(source) {
                    *target = ((source as u32 * alpha + *target as u32 * (255 - alpha) + 127) / 255)
                        as u8;
                }
                touched = true;
            }
        }
        touched
    }
}

impl Drawables {
    /// 按登记顺序与基点 Z 绘制基础纯色三角形；仅处理默认坐标组及普通混合。
    pub fn render_triangles(
        &mut self,
        target: &mut super::RenderTarget,
        projection: &Projection,
        parent: [f32; 16],
    ) -> Result<bool> {
        self.order();
        let mut touched = false;
        for id in &self.order {
            let drawable = &self.entries[id];
            if drawable.display() == 0 || drawable.triangles().is_empty() {
                continue;
            }
            if drawable.blend_mode() != 0 {
                return Err(Error::new(0, "三角形特殊混合模式尚未接入"));
            }
            let position = drawable.transform().position();
            // 原设备入口单次对象提交最多读取 32 个三角形。
            for index in 0..drawable.triangles().len().min(32) {
                let vertices = drawable
                    .projected_triangle(index, parent, projection, [position[0], position[1]], true)
                    .unwrap();
                if vertices.iter().any(|v| v.color != vertices[0].color) {
                    return Err(Error::new(0, "三角形顶点颜色插值尚未接入"));
                }
                if vertices
                    .iter()
                    .any(|v| v.position[2].is_finite() && !(0.0..=1.0).contains(&v.position[2]))
                {
                    return Err(Error::new(0, "三角形深度裁剪尚未接入"));
                }
                touched |= target.blend_triangle(
                    vertices.map(|v| [v.position[0], v.position[1]]),
                    vertices[0].color,
                );
            }
        }
        Ok(touched)
    }
}

fn edge(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}
