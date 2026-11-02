use super::{ImageState, ObjectAttributes, ObjectReference};
use crate::{Error, Result};

/// BUTTON 继承图像状态，另保存输入启用、事件及导航配置。
#[derive(Clone, Debug, PartialEq)]
pub struct ButtonState {
    pub image: ImageState,
    pub enabled: i32,
    pub default_events: Vec<u8>,
    /// 顺序为上、下、左、右；空名称解除该方向的显式目标。
    pub keys: [ObjectReference; 4],
    /// TYPE 原始值；非零值在区域登记时请求图像 Alpha 掩码。
    pub mask_type: i32,
    /// 网格编号、横坐标、纵坐标；编号 −1 表示没有网格。
    pub keyblock: [i32; 3],
    pub system_key: i32,
    /// 关联控件的名称／银行；独立于 KEY_* 和父子关系。
    pub control: Option<ObjectReference>,
}

impl Default for ButtonState {
    fn default() -> Self {
        Self {
            image: ImageState::default(),
            enabled: 1,
            default_events: Vec::new(),
            keys: std::array::from_fn(|_| ObjectReference {
                name: String::new(),
                bank: 0,
            }),
            mask_type: 0,
            keyblock: [-1, 0, 0],
            system_key: -1,
            control: None,
        }
    }
}

impl ButtonState {
    pub fn set_control(&mut self, control: Option<&ObjectReference>) -> Result<()> {
        if control
            .is_some_and(|target| target.name.len() >= 32 || target.name.as_bytes().contains(&0))
        {
            return Err(Error::new(0, "FES 关联控件名称超过存储上限或包含零字节"));
        }
        self.control = control.cloned();
        Ok(())
    }

    /// 按 IMAGE、SIZE、ENABLE、事件与导航配置的顺序应用属性。
    /// 返回本次是否需要使后代失效；ENABLE 只更新本对象的属性标记。
    pub fn apply(
        &mut self,
        attributes: &ObjectAttributes,
        bank: u32,
        transform_name: impl FnMut(&[u8]) -> Result<Vec<u8>>,
    ) -> Result<bool> {
        let dirty = self.image.apply(attributes, bank, transform_name)?;
        if let Some(size) = attributes.size {
            self.image.base.set_size(size);
        }
        if let Some(enabled) = attributes.enabled {
            self.image.base.updates.dirty |= self.enabled != enabled;
            self.enabled = enabled;
        }
        if let Some(events) = &attributes.default_events {
            let end = events.iter().position(|&b| b == 0).unwrap_or(events.len());
            if end >= 32 {
                return Err(Error::new(0, "FES 默认事件组名称超过存储上限"));
            }
            self.default_events = events[..end].to_vec();
        }
        for (key, reference) in self.keys.iter_mut().zip(&attributes.keys) {
            if let Some(reference) = reference {
                if reference.name.len() >= 32 || reference.name.as_bytes().contains(&0) {
                    return Err(Error::new(0, "FES 方向目标名称超过存储上限或包含零字节"));
                }
                key.clone_from(reference);
            }
        }
        if let Some(kind) = attributes.object_type {
            self.mask_type = kind;
        }
        if let Some(keyblock) = attributes.keyblock {
            self.keyblock = keyblock;
        }
        if let Some(system_key) = attributes.system_key {
            self.system_key = system_key;
        }
        Ok(dirty)
    }
}
