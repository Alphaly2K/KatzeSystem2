use super::{ImageReference, ObjectAttributes, ObjectReference, SubcommandContext};

/// CPLANE 的属性与待更新标记；下层任务由运行时维护。
#[derive(Clone, Debug, PartialEq)]
pub struct CPlaneState {
    pub(crate) position: [f32; 4],
    pub(crate) base_point: [f32; 4],
    rotation: [f32; 4],
    scale: [f32; 4],
    matrix: [f32; 16],
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
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 4],
            matrix: *crate::graphics::Transform::default().matrix(),
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

    pub fn rotation(&self) -> [f32; 4] {
        self.rotation
    }

    pub fn scale(&self) -> [f32; 4] {
        self.scale
    }

    pub fn matrix(&self) -> &[f32; 16] {
        &self.matrix
    }

    pub fn subcommand_context(&self) -> SubcommandContext {
        SubcommandContext {
            base: self.base_point[..3].try_into().unwrap(),
            position: self.position[..3].try_into().unwrap(),
            velocity: [0.0; 3],
            rotation: self.rotation[..3].try_into().unwrap(),
            scale: self.scale[..3].try_into().unwrap(),
            color: self.color,
        }
    }

    pub fn motion_properties(&self) -> [u32; 36] {
        let mut properties = [0; 36];
        for i in 0..3 {
            properties[3 + i] = self.base_point[i].to_bits();
            properties[6 + i] = self.position[i].to_bits();
            properties[25 + i] = self.scale[i].to_bits();
            properties[29 + i] = self.rotation[i].to_bits();
        }
        properties[5] = (self.priority as f32).to_bits();
        properties[9..25].copy_from_slice(&self.matrix.map(f32::to_bits));
        properties[28] = self.color;
        properties[35] = self.display as u32;
        properties
    }

    /// 写回基点 Z 并保留独立优先级；总返回 true，供宿主递归标记后代失效。
    pub fn apply_motion_properties(&mut self, properties: &[u32; 36]) -> bool {
        let value = |i| f32::from_bits(properties[i]);
        for (current, start) in [
            (&mut self.base_point, 3),
            (&mut self.position, 6),
            (&mut self.scale, 25),
        ] {
            let next = [value(start), value(start + 1), value(start + 2), 0.0];
            if *current != next {
                *current = next;
            }
        }
        self.matrix = std::array::from_fn(|i| value(i + 9));
        self.rotation[..3].copy_from_slice(&[value(29), value(30), value(31)]);
        self.color = properties[28];
        self.display = properties[35] as i32;
        self.dirty = true;
        true
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

/// PLANE 复用平面属性，并保存独立的画布取样位置。
#[derive(Clone, Debug, PartialEq)]
pub struct PlaneState {
    pub base: CPlaneState,
    vram: [f32; 4],
}

impl Default for PlaneState {
    fn default() -> Self {
        Self {
            base: CPlaneState::default(),
            vram: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

impl PlaneState {
    pub fn vram(&self) -> [f32; 4] {
        self.vram
    }

    pub fn subcommand_context(&self) -> SubcommandContext {
        SubcommandContext {
            velocity: self.vram[..3].try_into().unwrap(),
            ..self.base.subcommand_context()
        }
    }

    /// 返回本次是否使属性失效；VRAM 独立于屏幕位置和尺寸重建。
    pub fn apply(&mut self, attributes: &ObjectAttributes, bank: u32) -> bool {
        let mut dirty = self.base.apply(attributes, bank);
        if let Some(vram) = attributes.vram {
            let next = [vram[0] as f32, vram[1] as f32, 0.0, 0.0];
            if self.vram != next {
                self.vram = next;
                self.base.dirty = true;
                dirty = true;
            }
        }
        dirty
    }
}
