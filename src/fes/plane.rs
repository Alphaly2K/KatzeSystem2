use super::{ImageReference, ObjectAttributes, ObjectReference};

/// CPLANE 的属性与待更新标记；下层任务由运行时维护。
#[derive(Clone, Debug, PartialEq)]
pub struct CPlaneState {
    pub(crate) position: [f32; 4],
    pub(crate) base_point: [f32; 4],
    pub(crate) size: [i32; 2],
    pub(crate) priority: i32,
    pub(crate) display: i32,
    pub(crate) color: u32,
    pub(crate) creation_type: u32,
    pub(crate) extended_position: i32,
    pub(crate) viewport: ObjectReference,
    pub(crate) position_resource: Option<ImageReference>,
    pub(crate) size_resource: Option<ImageReference>,
    pub(crate) dirty: bool,
    pub(crate) rebuild: bool,
    pub(crate) viewport_dirty: bool,
}

impl Default for CPlaneState {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0, 1.0],
            base_point: [0.0, 0.0, 0.0, 1.0],
            size: [800, 600],
            priority: 5000,
            display: 0,
            color: u32::MAX,
            creation_type: 1,
            extended_position: 0,
            viewport: ObjectReference {
                name: String::new(),
                bank: 0,
            },
            position_resource: None,
            size_resource: None,
            dirty: false,
            rebuild: false,
            viewport_dirty: false,
        }
    }
}

impl CPlaneState {
    pub fn position(&self) -> [f32; 4] {
        self.position
    }

    pub fn base_point(&self) -> [f32; 4] {
        self.base_point
    }

    pub fn size(&self) -> [i32; 2] {
        self.size
    }

    pub fn priority(&self) -> i32 {
        self.priority
    }

    pub fn display(&self) -> i32 {
        self.display
    }

    pub fn color(&self) -> u32 {
        self.color
    }

    pub fn creation_type(&self) -> u32 {
        self.creation_type
    }

    pub fn extended_position(&self) -> i32 {
        self.extended_position
    }

    pub fn viewport(&self) -> &ObjectReference {
        &self.viewport
    }

    pub fn position_resource(&self) -> Option<&ImageReference> {
        self.position_resource.as_ref()
    }

    pub fn size_resource(&self) -> Option<&ImageReference> {
        self.size_resource.as_ref()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn needs_rebuild(&self) -> bool {
        self.rebuild
    }

    pub fn viewport_changed(&self) -> bool {
        self.viewport_dirty
    }

    /// 返回本次是否使属性失效；宿主据此标记 FES 后代，重建标记独立保留。
    pub fn apply(&mut self, attributes: &ObjectAttributes, bank: u32) -> bool {
        let mut dirty = false;
        if attributes.position[..2].iter().any(Option::is_some) {
            let mut position = self.position;
            for (next, value) in position.iter_mut().zip(attributes.position) {
                if let Some(value) = value {
                    *next = value as f32;
                }
            }
            if position != self.position {
                self.position = position;
                dirty = true;
            }
        }
        if let Some(base) = attributes.base {
            let next = [base[0] as f32, base[1] as f32, base[2] as f32, 0.0];
            if next != self.base_point {
                self.base_point = next;
                dirty = true;
            }
        }
        if let Some(size) = attributes.size {
            self.size = size;
            self.rebuild = true;
        }
        if let Some(resource) = &attributes.position_resource {
            self.position_resource = Some(ImageReference::parse(resource));
            dirty = true;
        }
        if let Some(resource) = &attributes.size_resource {
            self.size_resource = Some(ImageReference::parse(resource));
            self.rebuild = true;
        }
        if let Some(priority) = attributes.priority {
            self.priority = if attributes.sequential_priority {
                priority.wrapping_add(bank as i32)
            } else {
                priority
            };
            dirty = true;
        }
        if let Some(display) = attributes.display {
            if self.display != display {
                self.display = display;
                dirty = true;
            }
        }
        if let Some(color) = attributes.color {
            self.color = color as u32;
            dirty = true;
        }
        if let Some(blend) = attributes.blend {
            self.color = (self.color & 0xffffff) | ((blend as u32) << 24);
            dirty = true;
        }
        if let Some(kind) = attributes.object_type {
            self.creation_type = if kind == 2 { 2 } else { 1 };
        }
        if let Some(extended) = attributes.extended_position {
            self.extended_position = extended;
        }
        if let Some(viewport) = &attributes.viewport {
            if self.viewport != *viewport {
                self.viewport = viewport.clone();
                self.viewport_dirty = true;
                dirty = true;
            }
        }
        self.dirty |= dirty;
        dirty
    }
}
