use super::Transform;
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
    transform: Transform,
    triangles: Vec<[Vertex; 3]>,
    color: u32,
    color_enabled: u32,
    display: u32,
    blend_mode: u32,
}

impl Default for Drawable {
    fn default() -> Self {
        Self {
            transform: Transform::default(),
            triangles: Vec::new(),
            color: u32::MAX,
            color_enabled: 0,
            display: 0,
            blend_mode: 0,
        }
    }
}

impl Drawable {
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
}

/// 按对象编号索引、按登记及优先级维护绘制顺序；任务编号不决定同级顺序。
#[derive(Default)]
pub struct Drawables {
    entries: BTreeMap<u32, Drawable>,
    order: Vec<u32>,
    order_dirty: bool,
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
        let Some(drawable) = self.entries.get_mut(&id) else {
            return false;
        };
        drawable.transform.set_base_point(value);
        // 原管理器在基点设置后总登记重排，即使浮点比较未改变属性。
        self.order_dirty = true;
        true
    }

    pub fn set_position(&mut self, id: u32, value: [f32; 3]) -> bool {
        self.update(id, |d| d.transform.set_position(value))
    }

    pub fn set_scale(&mut self, id: u32, value: [f32; 3]) -> bool {
        self.update(id, |d| d.transform.set_scale(value))
    }

    pub fn set_rotation(&mut self, id: u32, value: [f32; 3]) -> bool {
        self.update(id, |d| d.transform.set_rotation(value))
    }

    pub fn set_matrix(&mut self, id: u32, value: [f32; 16]) -> bool {
        self.update(id, |d| d.transform.set_matrix(value))
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

    fn update(&mut self, id: u32, change: impl FnOnce(&mut Drawable)) -> bool {
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
