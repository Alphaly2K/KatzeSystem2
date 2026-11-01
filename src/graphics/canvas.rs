use super::{Drawable, Vertex, drawable::arithmetic as f};
use crate::kcs::transform_vector;
use crate::{Error, Result, read};

/// 画布面块的三角形；tag 保留原前置 DWORD，tile 为按行排列的面块索引。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasTriangle {
    pub tag: u32,
    pub tile: u32,
    pub vertices: [Vertex; 3],
    pub uv: [[f32; 2]; 3],
}

impl Drawable {
    /// 读取 count 条八十字节的画布三角形；初始覆盖颜色为白色，保留原顶点颜色。
    pub fn from_canvas(data: &[u8], count: u32) -> Result<Self> {
        let count = count as usize;
        if count > read::MAX_BYTES / 80 {
            return Err(Error::new(0, "画布三角形数量超过上限"));
        }
        let data = read::range(data, 0, count * 80)?;
        let mut triangles = Vec::with_capacity(count);
        let mut bounds = [[100_000_000.; 3], [-100_000_000.; 3]];
        for bytes in data.chunks_exact(80) {
            let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            let vertices = std::array::from_fn(|i| Vertex {
                position: std::array::from_fn(|axis| f32::from_bits(word(8 + i * 24 + axis * 4))),
                color: word(20 + i * 24),
            });
            for vertex in vertices {
                for (axis, value) in vertex.position.into_iter().enumerate() {
                    if value < bounds[0][axis] {
                        bounds[0][axis] = value;
                    }
                    if value > bounds[1][axis] {
                        bounds[1][axis] = value;
                    }
                }
            }
            triangles.push(CanvasTriangle {
                tag: word(0),
                tile: word(4),
                vertices,
                uv: std::array::from_fn(|i| {
                    std::array::from_fn(|axis| f32::from_bits(word(24 + i * 24 + axis * 4)))
                }),
            });
        }
        Ok(Self {
            canvas_triangles: Some(triangles),
            bounds,
            color_enabled: 1,
            ..Self::default()
        })
    }

    /// None 为非纹理图元，Some 的空列表为零三角形画布图元。
    pub fn canvas_triangles(&self) -> Option<&[CanvasTriangle]> {
        self.canvas_triangles.as_deref()
    }

    /// Z 平移先于对象矩阵，XY 平移及半像素偏移随后生效，父矩阵最后相乘。
    pub fn transformed_canvas_triangle(
        &self,
        index: usize,
        parent: [f32; 16],
    ) -> Option<CanvasTriangle> {
        let mut triangle = *self.canvas_triangles.as_ref()?.get(index)?;
        let base = self.transform.base_point();
        let position = self.transform.position();
        triangle.vertices = triangle.vertices.map(|vertex| {
            let point = [
                f(2, vertex.position[0], base[0]),
                f(2, vertex.position[1], base[1]),
                f(1, vertex.position[2], position[2]),
                1.,
            ];
            let mut point = transform_vector(point, *self.transform.matrix());
            for axis in 0..2 {
                point[axis] = f(2, f(1, position[axis], point[axis]), 0.5);
            }
            let point = transform_vector(point, parent);
            Vertex {
                position: [point[0], point[1], point[2]],
                color: if self.color_enabled != 0 {
                    self.color
                } else {
                    vertex.color
                },
            }
        });
        Some(triangle)
    }
}
