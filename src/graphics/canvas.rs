use super::{CanvasLayout, Drawable, Drawables, Transform, Vertex, drawable::arithmetic as f};
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

impl Drawables {
    /// 在直接检查记录或变换后顶点前，应用指定对象的源几何变动。
    pub fn prepare_canvas(&mut self, id: u32) -> Result<bool> {
        let Some(drawable) = self.entries.get_mut(&id) else {
            return Ok(false);
        };
        drawable.prepare_canvas()?;
        Ok(true)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CanvasSource {
    pub geometry: Drawable,
    layout: CanvasLayout,
}

impl Drawable {
    /// 保留非纹理源几何及独立的包装变换；源变化后在 prepare_canvas 中重新切割。
    pub fn from_canvas_source(source: Self, layout: CanvasLayout) -> Result<Self> {
        if source.canvas_triangles.is_some() {
            return Err(Error::new(0, "画布包装需要非纹理源几何"));
        }
        let mut drawable = Self {
            bounds: source.bounds,
            canvas_triangles: Some(Vec::new()),
            canvas_source: Some(Box::new(CanvasSource {
                geometry: source,
                layout,
            })),
            ..Self::default()
        };
        drawable.rebuild_canvas()?;
        Ok(drawable)
    }

    /// source 为真时选择包装的非纹理源；显式画布记录没有源对象。
    pub fn geometry_transform(&self, source: bool) -> Option<&Transform> {
        if source && self.canvas_triangles.is_some() {
            self.canvas_source.as_ref().map(|s| &s.geometry.transform)
        } else {
            Some(&self.transform)
        }
    }

    /// 源属性变动后更新原始纹理记录；帧提交会自动为可见对象执行。
    pub fn prepare_canvas(&mut self) -> Result<()> {
        if self
            .canvas_source
            .as_ref()
            .is_some_and(|source| source.geometry.transform.is_dirty())
        {
            self.rebuild_canvas()?;
        }
        Ok(())
    }

    fn rebuild_canvas(&mut self) -> Result<()> {
        const IDENTITY: [f32; 16] = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        let source = self.canvas_source.as_mut().unwrap();
        let transformed: Vec<_> = (0..source.geometry.triangles.len())
            .map(|i| source.geometry.transformed_triangle(i, IDENTITY).unwrap())
            .collect();
        let mut triangles = source.layout.triangulate(&transformed)?;
        let position = source.geometry.transform.position();
        // UV 使用画布坐标，几何再移回源平移前的位置。
        for triangle in &mut triangles {
            for vertex in &mut triangle.vertices {
                for (axis, value) in position[..2].iter().enumerate() {
                    vertex.position[axis] = f(2, vertex.position[axis], *value);
                }
            }
        }
        self.canvas_triangles = Some(triangles);
        source.geometry.transform.clear_dirty();
        Ok(())
    }

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

    /// None 为非纹理图元；有源的包装在读取前应调用 prepare_canvas。
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
