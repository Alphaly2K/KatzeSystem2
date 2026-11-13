use super::{CanvasTriangle, ProjectedVertex, Projection, Transform};
use crate::kcs::float_operation;
use crate::{Error, Result, read};
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// 不带纹理的三角形顶点；颜色保留原始 ARGB DWORD。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: u32,
}

/// 登记在绘图管理器中的三角形列表及对象属性。
#[derive(Clone, Debug, PartialEq)]
pub struct Drawable {
    pub(super) transform: Transform,
    pub(super) triangles: Vec<[Vertex; 3]>,
    pub(super) canvas_triangles: Option<Vec<CanvasTriangle>>,
    pub(super) canvas_source: Option<Box<super::canvas::CanvasSource>>,
    pub(crate) particle_mesh: Option<super::ParticleMesh>,
    pub(super) bounds: [[f32; 3]; 2],
    pub(super) color: u32,
    pub(super) color_enabled: u32,
    pub(super) display: u32,
    pub(super) blend_mode: u32,
    pub(super) camera_id: u32,
    pub(super) projection_origin_mode: u32,
    pub(super) clip: Option<super::Rect>,
}

impl Default for Drawable {
    fn default() -> Self {
        Self {
            transform: Transform::default(),
            triangles: Vec::new(),
            canvas_triangles: None,
            canvas_source: None,
            particle_mesh: None,
            bounds: [[0.0; 3]; 2],
            color: u32::MAX,
            color_enabled: 0,
            display: 0,
            blend_mode: 0,
            camera_id: 0,
            projection_origin_mode: 0,
            clip: None,
        }
    }
}

impl Drawable {
    pub fn from_particle_mesh(mesh: super::ParticleMesh) -> Self {
        Self {
            particle_mesh: Some(mesh),
            ..Self::default()
        }
    }

    pub fn particle_mesh(&self) -> Option<&super::ParticleMesh> {
        self.particle_mesh.as_ref()
    }

    /// 读取 KX2 的独立二维边界和 XYZ／ARGB 三角形，不从顶点重新计算边界。
    pub fn from_kx2(data: &[u8]) -> Result<Self> {
        if data.len() > read::MAX_BYTES || data.get(..4) != Some(b"KX2\0") {
            return Err(Error::new(0, "无效 KX2 几何资源"));
        }
        let count = read::u32_at(data, 20)? as usize;
        if count > (read::MAX_BYTES - 24) / 48 {
            return Err(Error::new(20, "KX2 三角形数量超过上限"));
        }
        let vertices = read::range(data, 24, count * 48)?;
        let mut triangles = Vec::with_capacity(count);
        for bytes in vertices.chunks_exact(48) {
            triangles.push(std::array::from_fn(|i| {
                let word = |n| {
                    u32::from_le_bytes(
                        bytes[i * 16 + n * 4..i * 16 + n * 4 + 4]
                            .try_into()
                            .unwrap(),
                    )
                };
                Vertex {
                    position: std::array::from_fn(|n| f32::from_bits(word(n))),
                    color: word(3),
                }
            }));
        }
        let bounds = [
            [
                f32::from_bits(read::u32_at(data, 4)?),
                f32::from_bits(read::u32_at(data, 8)?),
                0.,
            ],
            [
                f32::from_bits(read::u32_at(data, 12)?),
                f32::from_bits(read::u32_at(data, 16)?),
                0.,
            ],
        ];
        Ok(Self {
            triangles,
            bounds,
            ..Self::default()
        })
    }

    /// 创建两个三角形组成的原始矩形，保留反向尺寸与退化边。
    pub fn rectangle(at: [f32; 3], size: [f32; 2], color: u32) -> Self {
        let right = arithmetic(1, at[0], size[0]);
        let bottom = arithmetic(1, at[1], size[1]);
        let vertex = |x, y| Vertex {
            position: [x, y, at[2]],
            color,
        };
        let top_left = vertex(at[0], at[1]);
        let top_right = vertex(right, at[1]);
        let bottom_left = vertex(at[0], bottom);
        let bottom_right = vertex(right, bottom);
        Self {
            triangles: vec![
                [top_left, top_right, bottom_right],
                [top_left, bottom_left, bottom_right],
            ],
            bounds: [[at[0], at[1], 0.0], [right, bottom, 0.0]],
            ..Self::default()
        }
    }

    pub fn new(triangles: Vec<[Vertex; 3]>) -> Result<Self> {
        if triangles.len() > read::MAX_BYTES / 48 {
            return Err(Error::new(0, "三角形顶点存储超过上限"));
        }
        Ok(Self {
            triangles,
            ..Self::default()
        })
    }

    pub fn transform(&self) -> &Transform {
        &self.transform
    }

    pub fn triangles(&self) -> &[[Vertex; 3]] {
        &self.triangles
    }

    /// 独立于顶点的原始边界；通用三角形列表初值为零，矩形保留其请求端点。
    pub fn bounds(&self) -> [[f32; 3]; 2] {
        self.bounds
    }

    /// 返回覆盖颜色及原始启用值；零值使用每个顶点自己的颜色。
    pub fn color(&self) -> (u32, u32) {
        (self.color, self.color_enabled)
    }

    pub fn display(&self) -> u32 {
        self.display
    }

    pub fn blend_mode(&self) -> u32 {
        self.blend_mode
    }

    pub fn camera_id(&self) -> u32 {
        self.camera_id
    }

    /// 最终画布变换前的裁剪区域；None 使用完整输出。
    pub fn clip(&self) -> Option<super::Rect> {
        self.clip
    }

    /// 透视原点：零取对象平移，一取绘图中心，其他值取坐标零点。
    pub fn projection_origin_mode(&self) -> u32 {
        self.projection_origin_mode
    }

    /// 取一个三角形并应用对象及父级矩阵；颜色覆盖在变换路径上生效。
    pub fn transformed_triangle(&self, index: usize, parent: [f32; 16]) -> Option<[Vertex; 3]> {
        Some(self.triangles.get(index)?.map(|vertex| {
            let point = self
                .transform
                .transform_point_with_parent(vertex.position, parent);
            Vertex {
                position: [point[0], point[1], point[2]],
                color: if self.color_enabled != 0 {
                    self.color
                } else {
                    vertex.color
                },
            }
        }))
    }

    /// 在对象／父级变换和颜色覆盖之后，生成供绘图设备使用的投影顶点。
    pub fn projected_triangle(
        &self,
        index: usize,
        parent: [f32; 16],
        projection: &Projection,
        origin: [f32; 2],
        perspective: bool,
    ) -> Option<[ProjectedVertex; 3]> {
        Some(projection.project_triangle(
            self.transformed_triangle(index, parent)?,
            self.transform.base_point()[2],
            origin,
            perspective,
        ))
    }
}

pub(super) fn arithmetic(op: u8, left: f32, right: f32) -> f32 {
    f32::from_bits(float_operation(op, left.to_bits(), right.to_bits()))
}

/// 按对象编号索引、按登记及优先级维护绘制顺序；任务编号不决定同级顺序。
#[derive(Default)]
pub struct Drawables {
    pub(super) entries: BTreeMap<u32, Drawable>,
    pub(super) order: Vec<u32>,
    order_dirty: bool,
    pub(super) cameras: BTreeMap<u32, super::Camera2D>,
    pub(super) next_camera: u32,
}

impl Drawables {
    /// 已存在的编号返回 false 并保留原对象；重新登记须先 remove。
    pub fn insert(&mut self, id: u32, drawable: Drawable) -> Result<bool> {
        if self.entries.contains_key(&id) {
            return Ok(false);
        }
        if self.entries.len() == read::MAX_ITEMS {
            return Err(Error::new(0, "绘图对象数量超过上限"));
        }
        self.entries.insert(id, drawable);
        self.order.push(id);
        self.order_dirty = true;
        Ok(true)
    }

    pub fn get(&self, id: u32) -> Option<&Drawable> {
        self.entries.get(&id)
    }

    pub fn set_camera(&mut self, id: u32, camera: u32) -> bool {
        if let Some(drawable) = self.entries.get_mut(&id) {
            drawable.camera_id = camera;
            true
        } else {
            false
        }
    }

    pub fn remove(&mut self, id: u32) -> bool {
        if self.entries.remove(&id).is_none() {
            return false;
        }
        self.order.retain(|&item| item != id);
        true
    }

    /// 按基点 Z 从小到大稳定归并，保留相等值及 NaN 的原比较顺序。
    pub fn order(&mut self) -> &[u32] {
        if self.order_dirty {
            let mut scratch = self.order.clone();
            merge_order(&mut self.order, &mut scratch, &self.entries);
            self.order_dirty = false;
        }
        &self.order
    }

    pub fn set_base_point(&mut self, id: u32, value: [f32; 3]) -> bool {
        self.set_geometry_base_point(id, value, false)
    }

    pub(crate) fn set_geometry_base_point(
        &mut self,
        id: u32,
        value: [f32; 3],
        source: bool,
    ) -> bool {
        if !self.edit_geometry(id, source, |t| t.set_base_point(value)) {
            return false;
        }
        // 原管理器在基点设置后总登记重排，即使浮点比较未改变属性。
        self.order_dirty = true;
        true
    }

    pub fn set_position(&mut self, id: u32, value: [f32; 3]) -> bool {
        self.set_geometry_position(id, value, false)
    }

    pub(crate) fn set_geometry_position(&mut self, id: u32, value: [f32; 3], source: bool) -> bool {
        self.edit_geometry(id, source, |t| t.set_position(value))
    }

    pub fn set_scale(&mut self, id: u32, value: [f32; 3]) -> bool {
        self.update(id, |d| d.transform.set_scale(value))
    }

    pub fn set_rotation(&mut self, id: u32, value: [f32; 3]) -> bool {
        self.set_geometry_rotation(id, value, false)
    }

    pub(crate) fn set_geometry_rotation(&mut self, id: u32, value: [f32; 3], source: bool) -> bool {
        self.edit_geometry(id, source, |t| t.set_rotation(value))
    }

    pub fn set_matrix(&mut self, id: u32, value: [f32; 16]) -> bool {
        self.set_geometry_matrix(id, value, false)
    }

    pub(crate) fn set_geometry_matrix(&mut self, id: u32, value: [f32; 16], source: bool) -> bool {
        self.edit_geometry(id, source, |t| t.set_matrix(value))
    }

    fn edit_geometry(
        &mut self,
        id: u32,
        source: bool,
        change: impl FnOnce(&mut Transform),
    ) -> bool {
        let Some(drawable) = self.entries.get_mut(&id) else {
            return false;
        };
        let selected = if source && drawable.canvas_triangles.is_some() {
            let Some(source) = &mut drawable.canvas_source else {
                return false;
            };
            &mut source.geometry
        } else {
            drawable
        };
        change(&mut selected.transform);
        true
    }

    pub fn set_color(&mut self, id: u32, color: u32, enabled: u32) -> bool {
        self.update(id, |d| {
            d.color = color;
            d.color_enabled = enabled;
        })
    }

    pub fn set_alpha(&mut self, id: u32, alpha: u32) -> bool {
        self.update(id, |d| {
            d.color = (d.color & 0x00ff_ffff) | alpha.wrapping_shl(24);
            d.color_enabled = 1;
        })
    }

    pub fn set_display(&mut self, id: u32, value: u32) -> bool {
        self.update(id, |d| d.display = value)
    }

    pub fn set_blend_mode(&mut self, id: u32, value: u32) -> bool {
        self.update(id, |d| {
            d.blend_mode = if value <= 5 { value } else { 0 };
        })
    }

    pub fn set_clip(&mut self, id: u32, clip: Option<super::Rect>) -> bool {
        self.update(id, |d| d.clip = clip)
    }

    pub fn set_projection_origin_mode(&mut self, id: u32, value: u32) -> bool {
        self.update(id, |d| d.projection_origin_mode = value)
    }

    pub(super) fn update(&mut self, id: u32, change: impl FnOnce(&mut Drawable)) -> bool {
        let Some(drawable) = self.entries.get_mut(&id) else {
            return false;
        };
        change(drawable);
        true
    }
}

fn merge_order(order: &mut [u32], scratch: &mut [u32], entries: &BTreeMap<u32, Drawable>) {
    if order.len() < 2 {
        return;
    }
    let middle = order.len() / 2;
    let (left, right) = order.split_at_mut(middle);
    merge_order(left, &mut scratch[..middle], entries);
    merge_order(right, &mut scratch[middle..], entries);
    let (mut l, mut r) = (0, 0);
    for item in scratch.iter_mut() {
        // NaN 不构成全序；沿原归并规则只在 left > right 时选择右项。
        if r == right.len()
            || (l < left.len()
                && entries[&left[l]].transform.base_point()[2]
                    .partial_cmp(&entries[&right[r]].transform.base_point()[2])
                    != Some(Ordering::Greater))
        {
            *item = left[l];
            l += 1;
        } else {
            *item = right[r];
            r += 1;
        }
    }
    order.copy_from_slice(scratch);
}
