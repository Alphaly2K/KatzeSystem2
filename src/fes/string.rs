use super::PlaneState;
use crate::message::{MessageDefaults, TextEncoding};
use crate::{Error, Result};

/// STRING／STRING8 的二维属性和消息配置；播放状态由运行时持有。
#[derive(Clone, Debug, PartialEq)]
pub struct StringState {
    pub base: PlaneState,
    encoding: TextEncoding,
    configuration: Vec<u8>,
    configuration_dirty: bool,
    restore_colors: [u32; 3],
    defaults: MessageDefaults,
    font_inheritance: [bool; 4],
}

impl StringState {
    /// measurement_parameter 保留全局原始 DWORD，独立于字形服务参数。
    pub fn new(
        encoding: TextEncoding,
        defaults: &MessageDefaults,
        measurement_parameter: u32,
    ) -> Result<Self> {
        let mut state = Self {
            base: PlaneState::default(),
            encoding,
            configuration: defaults.configuration(encoding)?,
            configuration_dirty: false,
            restore_colors: [u32::MAX, 0xff00_0000, u32::MAX],
            defaults: defaults.clone(),
            font_inheritance: [encoding == TextEncoding::Cp932, true, true, true],
        };
        for offset in [0xc, 0x10, 0x14] {
            state.set(offset, 0);
        }
        state.set(0x7c, measurement_parameter);
        if encoding == TextEncoding::Unicode {
            state.set(0x10c, 0);
        }
        Ok(state)
    }

    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    /// 含长度标签及提交尾部，供消息任务的配置命令使用。
    pub fn configuration_bytes(&self) -> &[u8] {
        &self.configuration
    }

    pub fn configuration_dirty(&self) -> bool {
        self.configuration_dirty
    }

    pub fn clear_configuration_dirty(&mut self) {
        self.configuration_dirty = false;
    }

    pub fn restore_colors(&self) -> [u32; 3] {
        self.restore_colors
    }

    /// 顺序为字形参数、字体名、字重及阴影设置。
    pub fn font_inheritance(&self) -> [bool; 4] {
        self.font_inheritance
    }

    pub fn refresh_font_defaults(&mut self, defaults: &MessageDefaults) -> Result<()> {
        if self.font_inheritance[1] {
            self.copy_font_name(&defaults.font_name)?;
        }
        if self.font_inheritance[0] {
            self.set(self.font_offset() + 0xc, 0);
        }
        if self.font_inheritance[2] {
            self.set(self.font_offset(), if defaults.bold { 700 } else { 400 });
        }
        if self.font_inheritance[3] {
            self.set(self.font_offset() + 4, defaults.shadow);
            self.set(self.font_offset() + 8, defaults.outline);
        }
        self.defaults = defaults.clone();
        self.configuration_dirty = true;
        Ok(())
    }

    fn set(&mut self, offset: usize, value: u32) {
        self.configuration[offset + 4..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    /// 数值参数缺省时由回调返回 default；未知子命令不调用回调并返回 false。
    /// 同一次数组广播应先求值，再向各银行应用相同结果。
    pub fn apply_layout(
        &mut self,
        command: &[u8],
        mut value: impl FnMut(usize, i32) -> Result<i32>,
        frame_delay: i32,
    ) -> Result<bool> {
        let (offset, values) = match command {
            b"view" => (4, vec![value(0, 800)?, value(1, 600)?]),
            b"color" => {
                let mut colors = [0xff00_0000; 3];
                for (color, start) in colors.iter_mut().zip([0, 3, 6]) {
                    for component in 0..3 {
                        *color |= (value(start + component, 0)? as u8 as u32) << (component * 8);
                    }
                }
                self.restore_colors = colors;
                (0xc, colors.map(|color| color as i32).to_vec())
            }
            b"margin" => (0x40, (0..4).map(|i| value(i, 0)).collect::<Result<_>>()?),
            b"move" => (0x50, vec![value(0, 0)?, value(1, 0)?]),
            b"ruby" => (0x58, vec![value(0, 0)?]),
            b"frame" => {
                let delay = value(0, -1)?;
                let delay = if delay == -1 { frame_delay } else { delay };
                (0x70, vec![delay; 2])
            }
            b"shadow" => (self.font_offset() + 4, vec![value(0, 0)?, value(1, 0)?]),
            b"size" | b"rsize" => {
                let first = value(0, if command == b"size" { 20 } else { 10 })?;
                let mut sizes = vec![first];
                for index in 1..5 {
                    sizes.push(value(index, first)?);
                }
                (if command == b"size" { 0x18 } else { 0x2c }, sizes)
            }
            b"indent" => (0x6c, vec![value(0, 0)?]),
            b"weight" => {
                let weight = value(0, -1)?;
                self.font_inheritance[2] = weight == -1;
                (
                    self.font_offset(),
                    vec![if weight == -1 {
                        if self.defaults.bold { 700 } else { 400 }
                    } else {
                        weight
                    }],
                )
            }
            b"escape" => (0x60, vec![value(0, 0)?]),
            _ => return Ok(false),
        };
        for (index, &number) in values.iter().enumerate() {
            self.set(offset + index * 4, number as u32);
        }
        if command == b"view" {
            self.base.base.size = [values[0], values[1]];
            if self.encoding == TextEncoding::Unicode {
                // 此入口共用旧布局的尺寸写入方法，更新标记落在 Unicode 字体区内。
                self.set(0xe8, 1);
            } else {
                self.configuration_dirty = true;
            }
        } else if command != b"weight" {
            self.configuration_dirty = true;
        }
        if command == b"shadow" {
            self.font_inheritance[3] = false;
        }
        Ok(true)
    }

    pub fn set_font(&mut self, name: &[u16], backend: u32) -> Result<()> {
        self.copy_font_name(name)?;
        self.set(self.font_offset() + 0x14, backend);
        self.font_inheritance[1] = false;
        self.configuration_dirty = true;
        Ok(())
    }

    fn copy_font_name(&mut self, name: &[u16]) -> Result<()> {
        let name = &name[..name
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(name.len())];
        let capacity = (self.font_offset() - 0x80) / 2;
        if name.len() >= capacity {
            return Err(Error::new(0, "FES 字体名超过配置容量"));
        }
        for (index, &unit) in name.iter().chain(std::iter::once(&0)).enumerate() {
            let offset = 0x84 + index * 2;
            self.configuration[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
        Ok(())
    }

    fn font_offset(&self) -> usize {
        match self.encoding {
            TextEncoding::Cp932 => 0xc0,
            TextEncoding::Unicode => 0x100,
        }
    }
}
