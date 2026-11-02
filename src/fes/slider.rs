use super::{ImageReference, ObjectAttributes, ObjectReference};
use crate::{Error, Result};

/// 各请求独立消费；范围查询与位置通知不等同于显示属性失效。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SliderUpdates {
    pub file: bool,
    pub metadata: bool,
    pub range: bool,
    pub position: bool,
    pub alpha: bool,
    pub dirty: bool,
}

/// SLIDER 的属性及数值控制状态；配置装载、按钮关联和绘制由运行时处理。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SliderState {
    pub file: Vec<u8>,
    pub plane: ObjectReference,
    pub position_resource: Option<ImageReference>,
    pub rectangle: [i32; 4],
    pub range: [i32; 2],
    pub position: i32,
    pub priority: i32,
    pub display: i32,
    pub enabled: i32,
    pub vertical: i32,
    pub alpha: u32,
    pub notification: i32,
    pub updates: SliderUpdates,
}

impl Default for SliderState {
    fn default() -> Self {
        Self {
            file: Vec::new(),
            plane: ObjectReference {
                name: String::new(),
                bank: 0,
            },
            position_resource: None,
            rectangle: [0; 4],
            range: [0; 2],
            position: 0,
            priority: 10,
            display: 0,
            enabled: 1,
            vertical: 0,
            alpha: 255,
            notification: 0,
            updates: SliderUpdates::default(),
        }
    }
}

impl SliderState {
    pub fn set_file(&mut self, input: &[u8]) -> Result<()> {
        let input = prefix(input);
        if input.len() >= 256 {
            return Err(Error::new(0, "FES 滑动控件文件名称超过存储上限"));
        }
        self.file = input.to_vec();
        self.updates.file = true;
        Ok(())
    }

    pub fn set_plane(&mut self, plane: &ObjectReference) -> Result<bool> {
        if plane.name.len() >= 32 || plane.name.as_bytes().contains(&0) {
            return Err(Error::new(
                0,
                "FES 滑动控件平面名称超过存储上限或包含零字节",
            ));
        }
        let changed = self.plane != *plane;
        if changed {
            self.plane.clone_from(plane);
            self.updates.dirty = true;
        }
        Ok(changed)
    }

    pub fn request_position_resource(&mut self, input: &[u8]) -> Result<()> {
        let reference = ImageReference::parse(prefix(input));
        if reference.name.len() >= 128 {
            return Err(Error::new(0, "FES 滑动控件位置资源名称超过存储上限"));
        }
        self.position_resource = Some(reference);
        self.updates.metadata = true;
        Ok(())
    }

    pub fn set_rectangle(&mut self, rectangle: [i32; 4]) {
        self.rectangle = rectangle;
        self.updates.dirty = true;
    }

    pub fn set_range(&mut self, range: [i32; 2]) {
        self.range = range;
        self.updates.range = true;
    }

    /// 保存原值及通知参数，范围约束留给具体输入路径。
    pub fn set_position(&mut self, position: i32, notification: i32) {
        self.position = position;
        self.notification = notification;
        self.updates.position = true;
    }

    pub fn set_priority(&mut self, priority: i32) {
        self.priority = priority.wrapping_add(20);
    }

    pub fn set_display(&mut self, display: i32) {
        self.display = display;
        self.updates.dirty = true;
    }

    pub fn set_enabled(&mut self, enabled: i32) -> bool {
        let changed = self.enabled != enabled;
        if changed {
            self.enabled = enabled;
            self.updates.dirty = true;
        }
        changed
    }

    pub fn set_alpha(&mut self, alpha: u32) {
        self.alpha = alpha;
        self.updates.alpha = true;
    }

    /// 返回本次是否需使后代失效；ENABLE 只标记自身，资源请求独立保留。
    pub fn apply(&mut self, attributes: &ObjectAttributes, bank: u32) -> Result<bool> {
        let mut dirty = false;
        if let Some(file) = &attributes.file {
            self.set_file(file)?;
        }
        if let Some(plane) = &attributes.plane {
            dirty |= self.set_plane(plane)?;
        }
        if let Some(resource) = &attributes.position_resource {
            self.request_position_resource(resource)?;
        }
        if let Some(priority) = attributes.priority {
            self.set_priority(if attributes.sequential_priority {
                priority
            } else {
                priority.wrapping_add(bank as i32)
            });
        }
        if let Some(display) = attributes.display {
            self.set_display(display);
            dirty = true;
        }
        if let Some(enabled) = attributes.enabled {
            self.set_enabled(enabled);
        }
        Ok(dirty)
    }

    pub fn motion_properties(&self) -> [u32; 36] {
        let mut properties = [0; 36];
        properties[28] = (self.alpha & 255) << 24;
        properties
    }

    pub fn apply_motion_properties(&mut self, properties: &[u32; 36]) {
        self.set_alpha(properties[28] >> 24);
    }
}

fn prefix(input: &[u8]) -> &[u8] {
    &input[..input
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(input.len())]
}
