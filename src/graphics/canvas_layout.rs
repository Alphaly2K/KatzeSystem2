use super::{CanvasTriangle, Rect, Vertex};
use crate::{Error, Result, read};

/// 画布纹理尺寸、面块网格及几何切割步长；与像素存储相互独立。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasLayout {
    tile_size: [u32; 2],
    grid: [u32; 2],
    step: [u32; 2],
    cells: [u32; 2],
}

impl CanvasLayout {
    /// 步长须为正且不超过单块尺寸；每轴的细分数使用整数除法。
    pub fn new(tile_size: [u32; 2], grid: [u32; 2], step: [u32; 2]) -> Result<Self> {
        let mut cells = [0; 2];
        for axis in 0..2 {
            if step[axis] == 0 || step[axis] > tile_size[axis] {
                return Err(Error::new(0, "画布切割步长超出面块范围"));
            }
            cells[axis] = (tile_size[axis] / step[axis])
                .checked_mul(grid[axis])
                .ok_or_else(|| Error::new(0, "画布切割网格溢出"))?;
            if tile_size[axis]
                .checked_mul(grid[axis].max(1))
                .is_none_or(|n| n > i32::MAX as u32)
            {
                return Err(Error::new(0, "画布切割尺寸超出有符号坐标范围"));
            }
        }
        if u64::from(cells[0]) * u64::from(cells[1]) > read::MAX_ITEMS as u64 {
            return Err(Error::new(0, "画布切割网格数量超过上限"));
        }
        Ok(Self {
            tile_size,
            grid,
            step,
            cells,
        })
    }

    pub fn tile_size(&self) -> [u32; 2] {
        self.tile_size
    }

    pub fn grid(&self) -> [u32; 2] {
        self.grid
    }

    pub fn step(&self) -> [u32; 2] {
        self.step
    }

    /// 切割已变换的源三角形，按源、行、列、扇形顺序输出局部 UV。
    /// 输出 Z 为零、颜色取源首顶点；此处不施加包装对象变换或半像素偏移。
    pub fn triangulate(&self, source: &[[Vertex; 3]]) -> Result<Vec<CanvasTriangle>> {
        if source.len() > read::MAX_BYTES / 48
            || source.len() as u64 * u64::from(self.cells[0]) * u64::from(self.cells[1])
                > read::MAX_ITEMS as u64
        {
            return Err(Error::new(0, "画布几何切割工作量超过上限"));
        }
        if source
            .iter()
            .flatten()
            .any(|v| v.position[..2].iter().any(|n| !n.is_finite()))
        {
            return Err(Error::new(0, "画布源几何包含非有限 XY 坐标"));
        }
        let mut output = Vec::new();
        let subdivisions = self.tile_size[0] / self.step[0];
        for (tag, triangle) in source.iter().enumerate() {
            let triangle = triangle.map(|v| [v.position[0], v.position[1]]);
            let bounds = triangle_bounds(triangle);
            let mut bottom = 0.;
            for row in 0..self.cells[1] {
                let top = bottom;
                bottom += self.step[1] as f32;
                let y = row * self.step[1];
                let band = Rect::from_xywh(
                    0,
                    y as i32,
                    (self.cells[0] * self.step[0]) as i32,
                    self.step[1] as i32,
                );
                if bounds.intersect(band).is_none() {
                    continue;
                }
                let mut right = 0.;
                for column in 0..self.cells[0] {
                    let left = right;
                    right += self.step[0] as f32;
                    let x = column * self.step[0];
                    let rect = Rect::from_xywh(
                        x as i32,
                        y as i32,
                        self.step[0] as i32,
                        self.step[1] as i32,
                    );
                    if bounds.intersect(rect).is_none() {
                        continue;
                    }
                    let polygon = clip_triangle(
                        triangle,
                        [[left, top], [right, top], [right, bottom], [left, bottom]],
                    );
                    // 基线的面块行号同样使用横向细分数作除数。
                    let tile = row / subdivisions * self.grid[0] + column / subdivisions;
                    let origin = [
                        (tile % self.grid[0]) as f32 * self.tile_size[0] as f32,
                        (tile / self.grid[0]) as f32 * self.tile_size[1] as f32,
                    ];
                    for i in 1..polygon.len().saturating_sub(1) {
                        if output.len() == read::MAX_BYTES / 80 {
                            return Err(Error::new(0, "画布三角形存储超过上限"));
                        }
                        let points = [polygon[0], polygon[i], polygon[i + 1]];
                        output.push(CanvasTriangle {
                            tag: tag as u32,
                            tile,
                            vertices: points.map(|p| Vertex {
                                position: [p[0], p[1], 0.],
                                color: source[tag][0].color,
                            }),
                            uv: points.map(|p| {
                                std::array::from_fn(|axis| {
                                    (p[axis] - origin[axis]) / self.tile_size[axis] as f32
                                })
                            }),
                        });
                    }
                }
            }
        }
        Ok(output)
    }
}

fn truncate(value: f32) -> i32 {
    if !(-2147483648.0..2147483648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}

fn triangle_bounds(triangle: [[f32; 2]; 3]) -> Rect {
    let min_max = |axis| {
        let [a, b, c] = triangle.map(|p| p[axis]);
        // MINSS/MAXSS 在相等或无序时选第二操作数。
        let mut min = if a < b { a } else { b };
        if c < min {
            min = c;
        }
        let mut max = if a > b { a } else { b };
        if c > max {
            max = c;
        }
        [truncate(min), truncate(max)]
    };
    let [left, right] = min_max(0);
    let [top, bottom] = min_max(1);
    Rect {
        left,
        top,
        right,
        bottom,
    }
}

fn same_side(a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]) -> bool {
    let x = a[0] - b[0];
    let y = a[1] - b[1];
    let cross = |p: [f32; 2]| (p[1] - a[1]) * x + (a[0] - p[0]) * y;
    cross(c) * cross(d) >= 0.
}

fn inside(triangle: [[f32; 2]; 3], p: [f32; 2]) -> bool {
    let [a, b, c] = triangle;
    if (a[1] - b[1]) * (a[0] - c[0]) == (a[1] - c[1]) * (a[0] - b[0]) {
        return false;
    }
    same_side(a, b, p, c) && same_side(a, c, p, b) && same_side(b, c, p, a)
}

fn sign(value: f32) -> i32 {
    if value > 0. {
        1
    } else if value < 0. {
        -1
    } else {
        0
    }
}

fn side(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> i32 {
    sign((p[1] - a[1]) * (b[0] - a[0]) - (b[1] - a[1]) * (p[0] - a[0]))
}

fn intersection(a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]) -> Option<[f32; 2]> {
    let x1 = b[0] - a[0];
    let y1 = a[1] - b[1];
    let x2 = d[0] - c[0];
    let y2 = c[1] - d[1];
    let determinant = x2 * y1 - x1 * y2;
    if determinant == 0. || side(a, b, c) == side(a, b, d) || side(c, d, a) == side(c, d, b) {
        return None;
    }
    let first = a[0] * b[1] - b[0] * a[1];
    let second = c[0] * d[1] - d[0] * c[1];
    Some([
        (second * x1 - first * x2) / determinant,
        (second * y1 - first * y2) / (x1 * y2 - x2 * y1),
    ])
}

fn append_unique(points: &mut Vec<[f32; 2]>, p: [f32; 2]) {
    if !points.contains(&p) {
        points.push(p);
    }
}

fn clip_triangle(triangle: [[f32; 2]; 3], corners: [[f32; 2]; 4]) -> Vec<[f32; 2]> {
    let mut points = Vec::with_capacity(7);
    for p in triangle {
        if inside([corners[0], corners[1], corners[2]], p)
            || inside([corners[0], corners[2], corners[3]], p)
        {
            points.push(p);
        }
    }
    for p in corners {
        if inside(triangle, p) {
            append_unique(&mut points, p);
        }
    }
    for i in 0..3 {
        for j in 0..4 {
            if let Some(p) = intersection(
                triangle[i],
                triangle[(i + 1) % 3],
                corners[j],
                corners[(j + 1) % 4],
            ) {
                append_unique(&mut points, p);
            }
        }
    }
    if !points.is_empty() {
        let mut first = 0;
        for i in 1..points.len() {
            if points[i][0] < points[first][0]
                || (points[i][0] == points[first][0] && points[i][1] < points[first][1])
            {
                first = i;
            }
        }
        points.swap(0, first);
        let origin = points[0];
        let angle = |p: [f32; 2]| {
            let x = f64::from(p[0] - origin[0]);
            let y = f64::from(p[1] - origin[1]);
            let mut angle = x.atan2(y) as f32 + std::f32::consts::PI;
            if angle < 0. {
                angle += std::f32::consts::TAU;
            }
            angle
        };
        points[1..].sort_by(|&a, &b| {
            angle(b)
                .partial_cmp(&angle(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    points
}
