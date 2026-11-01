use super::{ImageReference, ObjectAttributes, ObjectReference, SubcommandContext};
use crate::{Result, hg3::StandardInfo};

/// 图像的属性、来源和排序更新分别保留，初始均不请求更新。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageUpdates {
    pub(crate) dirty: bool,
    pub(crate) rebuild: bool,
    pub(crate) priority_changed: bool,
}

impl ImageUpdates {
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn needs_rebuild(&self) -> bool {
        self.rebuild
    }

    pub fn priority_changed(&self) -> bool {
        self.priority_changed
    }

    /// 直接修改显示属性后请求重新登记；后代失效由对象存储处理。
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// 直接修改尺寸或颜色后请求重新准备来源。
    pub fn request_rebuild(&mut self) {
        self.rebuild = true;
    }
}

/// CIMAGE 的脚本可设置状态；父子对象关系和绘制由宿主维护。
#[derive(Clone, Debug, PartialEq)]
pub struct CImageState {
    pub position: [f32; 2],
    pub base_point: [f32; 2],
    pub scale: [f32; 2],
    pub rotation: f32,
    pub size: [i32; 2],
    pub plane: ObjectReference,
    pub color: i32,
    pub blend: i32,
    pub priority: i32,
    /// PRI 原始输入，用作图像列表槽号；运动修改 priority 时保持此值。
    pub parent_priority: i32,
    pub display: i32,
    pub mask: i32,
    pub button_type: i32,
    pub position_resource: Option<ImageReference>,
    pub size_resource: Option<ImageReference>,
    pub updates: ImageUpdates,
}

impl Default for CImageState {
    fn default() -> Self {
        Self {
            position: [0.0; 2],
            base_point: [0.0; 2],
            scale: [1.0; 2],
            rotation: 0.0,
            size: [-1; 2],
            plane: ObjectReference {
                name: String::new(),
                bank: 0,
            },
            color: -1,
            blend: 255,
            priority: -1,
            parent_priority: -1,
            display: 0,
            mask: 0,
            button_type: 0,
            position_resource: None,
            size_resource: None,
            updates: ImageUpdates::default(),
        }
    }
}

impl CImageState {
    pub fn subcommand_context(&self) -> SubcommandContext {
        SubcommandContext {
            base: [self.base_point[0], self.base_point[1], self.priority as f32],
            position: [self.position[0], self.position[1], 0.0],
            velocity: [0.0; 3],
            rotation: [0.0, 0.0, self.rotation],
            scale: [self.scale[0], self.scale[1], 1.0],
            color: self.color as u32,
        }
    }

    pub fn motion_properties(&self) -> [u32; 36] {
        let mut properties = [0; 36];
        for (index, value) in [
            (3, self.base_point[0]),
            (4, self.base_point[1]),
            (5, self.priority.wrapping_sub(20) as f32),
            (6, self.position[0]),
            (7, self.position[1]),
            (25, self.scale[0]),
            (26, self.scale[1]),
            (27, 1.0),
            (31, self.rotation),
        ] {
            properties[index] = value.to_bits();
        }
        properties[28] = (self.blend as u32) << 24;
        properties[35] = self.display as u32;
        properties
    }

    /// 返回本次是否使显示属性失效；排序变化单独登记。
    pub fn apply_motion_properties(&mut self, properties: &[u32; 36]) -> bool {
        let mut dirty = false;
        let value = |i| f32::from_bits(properties[i]);
        for (current, indices) in [
            (&mut self.position, [6, 7]),
            (&mut self.scale, [25, 26]),
            (&mut self.base_point, [3, 4]),
        ] {
            let next = indices.map(value);
            if *current != next {
                *current = next;
                dirty = true;
            }
        }
        if self.rotation != value(31) {
            self.rotation = value(31);
            dirty = true;
        }
        dirty |= replace(&mut self.blend, (properties[28] >> 24) as i32);
        self.set_priority(integer_position(value(5)));
        dirty |= replace(&mut self.display, properties[35] as i32);
        self.updates.dirty |= dirty;
        dirty
    }

    /// 仅应用 CIMAGE 对应的列。bank 是本次应用的具体银行号。
    pub fn apply(&mut self, attributes: &ObjectAttributes, bank: u32) -> bool {
        let dirty = self.apply_visual(attributes, bank);
        if let Some(size) = attributes.size {
            self.updates.rebuild |= replace(&mut self.size, size);
        }
        if let Some(color) = attributes.color {
            self.color = color;
            self.updates.rebuild = true;
        }
        dirty
    }

    fn apply_visual(&mut self, attributes: &ObjectAttributes, bank: u32) -> bool {
        let mut dirty = false;
        if attributes.position[..2].iter().any(Option::is_some) {
            let mut next = self.position;
            for (position, value) in next.iter_mut().zip(attributes.position) {
                *position = value.unwrap_or_else(|| integer_position(*position)) as f32;
            }
            dirty |= replace(&mut self.position, next);
        }
        if let Some(resource) = &attributes.position_resource {
            self.position_resource = Some(ImageReference::parse(resource));
            dirty = true;
        }
        if let Some(resource) = &attributes.size_resource {
            self.size_resource = Some(ImageReference::parse(resource));
        }
        if let Some(plane) = &attributes.plane {
            dirty |= replace(&mut self.plane, plane.clone());
        }
        if let Some(blend) = attributes.blend {
            dirty |= replace(&mut self.blend, blend);
        }
        if let Some(priority) = attributes.priority {
            let priority = if attributes.sequential_priority {
                priority.wrapping_add(bank as i32)
            } else {
                priority
            };
            self.set_priority(priority);
            self.parent_priority = priority;
        }
        if let Some(display) = attributes.display {
            dirty |= replace(&mut self.display, display);
        }
        if let Some(mask) = attributes.mask {
            self.mask = mask;
        }
        if let Some(button_type) = attributes.button_type {
            self.button_type = button_type;
        }
        self.updates.dirty |= dirty;
        dirty
    }

    fn set_priority(&mut self, priority: i32) {
        if self.priority != priority {
            self.priority = priority.wrapping_add(20);
            self.updates.priority_changed = true;
        }
    }

    /// 对象更新时先处理 SIZE2，再处理一次性 POS2；加载失败返回诊断。
    pub fn resolve_geometry(
        &mut self,
        mut metadata: impl FnMut(&ImageReference) -> Result<StandardInfo>,
    ) -> Result<()> {
        if let Some(resource) = &self.size_resource {
            self.updates.rebuild |= replace(
                &mut self.size,
                metadata(resource)?.size.map(|value| value as i32),
            );
        }
        if let Some(resource) = self.position_resource.take() {
            self.updates.dirty |= replace(
                &mut self.position,
                metadata(&resource)?.offset.map(|value| value as f32),
            );
        }
        Ok(())
    }
}

fn replace<T: PartialEq>(current: &mut T, next: T) -> bool {
    if *current == next {
        false
    } else {
        *current = next;
        true
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageState {
    pub base: CImageState,
    pub requested_file: Vec<u8>,
    pub file: Vec<u8>,
    pub image_number: i32,
    pub frames: [i32; 32],
    pub frame_table_enabled: bool,
    /// 帧表写入后，下次有效 IDN 即使编号相同也请求重新装载。
    pub frame_mapping_changed: bool,
    pub path_mode: i32,
}

impl Default for ImageState {
    fn default() -> Self {
        Self {
            base: CImageState::default(),
            requested_file: Vec::new(),
            file: Vec::new(),
            image_number: 0,
            frames: std::array::from_fn(|i| i as i32),
            frame_table_enabled: false,
            frame_mapping_changed: true,
            path_mode: 0,
        }
    }
}

impl ImageState {
    /// 名称转换由宿主按配置提供，实际图像加载在对象更新阶段完成。
    pub fn apply(
        &mut self,
        attributes: &ObjectAttributes,
        bank: u32,
        mut transform_name: impl FnMut(&[u8]) -> Result<Vec<u8>>,
    ) -> Result<bool> {
        if let Some(file) = &attributes.file {
            self.requested_file = file.clone();
            if !file.is_empty() {
                let next = transform_name(file)?;
                self.base.updates.rebuild |= replace(&mut self.file, next);
            }
        }
        let dirty = self.base.apply_visual(attributes, bank);
        match attributes.image_number {
            Some(number)
                if (!self.frame_table_enabled || (0..32).contains(&number))
                    && (self.image_number != number || self.frame_mapping_changed) =>
            {
                self.image_number = number;
                self.frame_mapping_changed = false;
                self.base.updates.rebuild = true;
            }
            _ => (),
        }
        for (id, number) in attributes.frames.iter().enumerate() {
            if let Some(number) = number {
                self.frames[id] = *number;
                self.frame_table_enabled = true;
                self.frame_mapping_changed = true;
            }
        }
        if let Some(path_mode) = attributes.path_mode {
            self.path_mode = path_mode;
        }
        Ok(dirty)
    }

    pub fn selected_frame(&self) -> i32 {
        if self.frame_table_enabled {
            self.frames
                .get(self.image_number as usize)
                .copied()
                .unwrap_or(0)
        } else {
            self.image_number
        }
    }
}

fn integer_position(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}
