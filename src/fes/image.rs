use super::{ImageReference, ObjectAttributes, ObjectReference};
use crate::{Result, hg3::StandardInfo};

/// CIMAGE 的脚本可设置状态；父子对象关系和绘制由宿主维护。
#[derive(Clone, Debug, PartialEq)]
pub struct CImageState {
    pub position: [f32; 2],
    pub size: [i32; 2],
    pub plane: ObjectReference,
    pub color: i32,
    pub blend: i32,
    pub priority: i32,
    pub parent_priority: i32,
    pub display: i32,
    pub mask: i32,
    pub button_type: i32,
    pub position_resource: Option<ImageReference>,
    pub size_resource: Option<ImageReference>,
}

impl Default for CImageState {
    fn default() -> Self {
        Self {
            position: [0.0; 2],
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
        }
    }
}

impl CImageState {
    /// 仅应用 CIMAGE 对应的列。bank 是本次应用的具体银行号。
    pub fn apply(&mut self, attributes: &ObjectAttributes, bank: u32) {
        self.apply_visual(attributes, bank);
        if let Some(size) = attributes.size {
            self.size = size;
        }
        if let Some(color) = attributes.color {
            self.color = color;
        }
    }

    fn apply_visual(&mut self, attributes: &ObjectAttributes, bank: u32) {
        if attributes.position[..2].iter().any(Option::is_some) {
            for (position, value) in self.position.iter_mut().zip(attributes.position) {
                *position = value.unwrap_or_else(|| integer_position(*position)) as f32;
            }
        }
        if let Some(resource) = &attributes.position_resource {
            self.position_resource = Some(ImageReference::parse(resource));
        }
        if let Some(resource) = &attributes.size_resource {
            self.size_resource = Some(ImageReference::parse(resource));
        }
        if let Some(plane) = &attributes.plane {
            self.plane = plane.clone();
        }
        if let Some(blend) = attributes.blend {
            self.blend = blend;
        }
        if let Some(priority) = attributes.priority {
            let priority = if attributes.sequential_priority {
                priority.wrapping_add(bank as i32)
            } else {
                priority
            };
            if self.priority != priority {
                self.priority = priority.wrapping_add(20);
            }
            self.parent_priority = priority;
        }
        if let Some(display) = attributes.display {
            self.display = display;
        }
        if let Some(mask) = attributes.mask {
            self.mask = mask;
        }
        if let Some(button_type) = attributes.button_type {
            self.button_type = button_type;
        }
    }

    /// 对象更新时先处理 SIZE2，再处理一次性 POS2；加载失败返回诊断。
    pub fn resolve_geometry(
        &mut self,
        mut metadata: impl FnMut(&ImageReference) -> Result<StandardInfo>,
    ) -> Result<()> {
        if let Some(resource) = &self.size_resource {
            self.size = metadata(resource)?.size.map(|value| value as i32);
        }
        if let Some(resource) = self.position_resource.take() {
            self.position = metadata(&resource)?.offset.map(|value| value as f32);
        }
        Ok(())
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
    ) -> Result<()> {
        if let Some(file) = &attributes.file {
            self.requested_file = file.clone();
            if !file.is_empty() {
                self.file = transform_name(file)?;
            }
        }
        self.base.apply_visual(attributes, bank);
        match attributes.image_number {
            Some(number) if !self.frame_table_enabled || (0..32).contains(&number) => {
                self.image_number = number;
            }
            _ => (),
        }
        for (id, number) in attributes.frames.iter().enumerate() {
            if let Some(number) = number {
                self.frames[id] = *number;
                self.frame_table_enabled = true;
            }
        }
        if let Some(path_mode) = attributes.path_mode {
            self.path_mode = path_mode;
        }
        Ok(())
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
