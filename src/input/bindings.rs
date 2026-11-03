use super::KeySample;
use crate::{Error, Result, resource::XmlDocument};

/// 配置数组的设备次序；四组分别对应每种设备的第一至第四个绑定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum ButtonSource {
    Keyboard,
    Mouse,
    Joypad,
    Touch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceBinding {
    pub enabled: u32,
    /// 第一维按 ButtonSource，第二维为绑定组。
    pub codes: [[u8; 4]; 4],
    pub repeat: [[u32; 4]; 4],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BindingState {
    masks: [u32; 4],
    repeat: u32,
    forced: u32,
}

/// 将物理按钮事件映射到八十个逻辑键槽，供平台 InputDevice 使用。
pub struct DeviceKeys {
    bindings: [DeviceBinding; 80],
    current: [BindingState; 80],
    held: [BindingState; 80],
    /// 零时只更新前 32 槽；其他槽保留旧状态并仍可查询。
    pub debug_mode: u32,
}

impl Default for DeviceKeys {
    fn default() -> Self {
        Self::new([DeviceBinding::default(); 80])
    }
}

impl DeviceKeys {
    pub fn new(bindings: [DeviceBinding; 80]) -> Self {
        Self {
            bindings,
            current: [BindingState::default(); 80],
            held: [BindingState::default(); 80],
            debug_mode: 0,
        }
    }

    /// 从 KEYCUSTOMIZE/key00～key79 读取完整绑定；非空 name 启用对应槽。
    /// 缺失字段取零，代码及重复字段均截取低八位。
    pub fn from_xml(bytes: &[u8]) -> Result<Self> {
        let xml = XmlDocument::parse(bytes)?;
        let mut bindings = [DeviceBinding::default(); 80];
        for (key, binding) in bindings.iter_mut().enumerate() {
            let path = format!("KEYCUSTOMIZE/key{key:02}");
            binding.enabled = u32::from(
                xml.text(format!("{path}/name").as_bytes())?
                    .is_some_and(|name| name.first().is_some_and(|&byte| byte != 0)),
            );
            for (source, name) in ["keyboard", "mouse", "joypad", "touch"].iter().enumerate() {
                for group in 0..4 {
                    let number = group + 1;
                    binding.codes[source][group] =
                        xml.integer(format!("{path}/{name}{number}").as_bytes(), 0)? as u8;
                    binding.repeat[source][group] = xml
                        .integer(format!("{path}/rep_{name}{number}").as_bytes(), 0)?
                        as u8 as u32;
                }
            }
        }
        Ok(Self::new(bindings))
    }

    pub fn bindings(&self) -> &[DeviceBinding; 80] {
        &self.bindings
    }

    /// 替换单槽配置并清空该槽的按键、重复元数据和强制查询计数。
    pub fn set_binding(&mut self, key: u32, binding: DeviceBinding) -> Result<()> {
        let index = key_index(key)?;
        self.bindings[index] = binding;
        self.current[index] = BindingState::default();
        self.held[index] = BindingState::default();
        Ok(())
    }

    /// 按下保留到 commit，释放只清持有态；同一轮内按下再释放仍可观察到按下。
    /// Mouse 与 Touch 共用两类绑定，重复开关优先取匹配的 Mouse 项。
    pub fn button(&mut self, source: ButtonSource, code: u32, pressed: u32) {
        let code = code as u8;
        let mask = match source {
            ButtonSource::Keyboard => 1,
            ButtonSource::Mouse => 2,
            ButtonSource::Joypad => 4,
            ButtonSource::Touch => 16,
        };
        let count = if self.debug_mode == 0 { 32 } else { 80 };
        for group in 0..4 {
            for key in 0..count {
                let binding = &self.bindings[key];
                if binding.enabled == 0 {
                    continue;
                }
                let matched = match source {
                    ButtonSource::Mouse | ButtonSource::Touch => {
                        if binding.codes[ButtonSource::Mouse as usize][group] == code {
                            Some(ButtonSource::Mouse)
                        } else if binding.codes[ButtonSource::Touch as usize][group] == code {
                            Some(ButtonSource::Touch)
                        } else {
                            None
                        }
                    }
                    _ if binding.codes[source as usize][group] == code => Some(source),
                    _ => None,
                };
                let Some(matched) = matched else {
                    continue;
                };
                if pressed != 0 {
                    self.current[key].masks[group] |= mask;
                    self.held[key].masks[group] |= mask;
                } else {
                    self.held[key].masks[group] &= !mask;
                }
                let repeat = binding.repeat[matched as usize][group];
                self.current[key].repeat = repeat;
                self.held[key].repeat = repeat;
            }
        }
    }

    /// 强制返回按下的查询次数，四个组和额外设备查询共用同一计数。
    pub fn force(&mut self, key: u32, queries: u32) -> Result<()> {
        let index = key_index(key)?;
        self.current[index].forced = queries;
        self.held[index].forced = queries;
        Ok(())
    }

    pub fn sample(&mut self, key: u32, group: u32, repeat: bool) -> Result<KeySample> {
        let index = key_index(key)?;
        if group >= 4 {
            return Err(Error::new(0, "输入设备绑定组超出 0～3"));
        }
        let state = &mut self.current[index];
        let mask = if state.forced != 0 {
            state.forced = state.forced.wrapping_sub(1);
            self.held[index].forced = self.held[index].forced.wrapping_sub(1);
            1
        } else {
            state.masks[group as usize]
        };
        Ok(KeySample {
            mask,
            repeat: if repeat { state.repeat } else { 0 },
        })
    }

    /// 设备收尾后提交持有态，消费本轮按下脉冲。
    pub fn commit(&mut self) {
        self.current = self.held;
    }

    /// 清空两份按键掩码，保留配置、重复元数据及强制查询计数。
    pub fn clear_pressed(&mut self) {
        for state in self.current.iter_mut().chain(&mut self.held) {
            state.masks = [0; 4];
        }
    }
}

fn key_index(key: u32) -> Result<usize> {
    if key >= 80 {
        Err(Error::new(0, "输入键槽位超出 0～79"))
    } else {
        Ok(key as usize)
    }
}
