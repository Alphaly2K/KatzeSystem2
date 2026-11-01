use crate::graphics::Rect;
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result, read};

/// 输入区域的原始十五 DWORD 记录；所有字段保持原位模式。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    words: [u32; 15],
}

impl Region {
    pub fn id(&self) -> u32 {
        self.words[0]
    }

    pub fn priority(&self) -> i32 {
        self.words[1] as i32
    }

    pub fn bounds(&self) -> Rect {
        Rect {
            left: self.words[3] as i32,
            top: self.words[4] as i32,
            right: self.words[5] as i32,
            bottom: self.words[6] as i32,
        }
    }

    /// 选择标记的原始值；可保留非布尔 DWORD。
    pub fn selected(&self) -> u32 {
        self.words[7]
    }

    pub fn enabled(&self) -> u32 {
        self.words[8]
    }

    pub fn words(&self) -> &[u32; 15] {
        &self.words
    }
}

/// 按有符号优先级稳定排列的输入区域，允许重复编号。
#[derive(Default)]
pub struct Regions {
    records: Vec<Region>,
    geometry_dirty: bool,
}

impl Regions {
    pub fn records(&self) -> &[Region] {
        &self.records
    }

    /// 重复编号时返回当前排序中的首项。
    pub fn get(&self, id: u32) -> Option<&Region> {
        self.records.iter().find(|r| r.id() == id)
    }

    /// 矩形或启用属性变动要求后续输入更新重新检查命中区域。
    pub fn geometry_dirty(&self) -> bool {
        self.geometry_dirty
    }

    /// 输入宿主完成区域重算后消费标记。
    pub fn clear_geometry_dirty(&mut self) {
        self.geometry_dirty = false;
    }

    /// 处理输入对象命令 127～136；true 表示已消费，未知编号返回 false。
    /// 调用者负责对象删除门控、基类分派及设备更新。
    pub fn dispatch_command(
        &mut self,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        let [a, b, _] = command.data;
        match command.code {
            127 => {
                if self.records.len() == read::MAX_ITEMS {
                    return Err(Error::new(0, "输入区域数量超过上限"));
                }
                let mut words = words::<15>(buffers.data1.as_deref())?;
                words[1] = a;
                words[7..10].copy_from_slice(&[0, 1, 1]);
                words[11..15].copy_from_slice(&[0, 0, 0, 1]);
                self.records.push(Region { words });
                self.sort();
            }
            128 => {
                if let Some(index) = self.index(b) {
                    self.records.remove(index);
                    self.sort();
                }
            }
            129 => {
                if let Some(index) = self.index(a) {
                    let mut region = self.records.remove(index);
                    region.words[1] = b;
                    self.records.push(region);
                    self.sort();
                }
            }
            130 => {
                if let Some(index) = self.index(a) {
                    let rect = if buffers.data1.is_some() || b != 0 {
                        words::<4>(buffers.data1.as_deref())?
                    } else {
                        [0; 4]
                    };
                    self.records[index].words[3..7].copy_from_slice(&rect);
                }
                self.geometry_dirty = true;
            }
            131 => {
                if let Some(index) = self.index(a) {
                    if buffers.data1.is_some() || b != 0 {
                        let buffer = buffers
                            .data1
                            .as_deref_mut()
                            .filter(|bytes| bytes.len() >= 16)
                            .ok_or_else(|| Error::new(0, "输入区域输出缓冲区缺失或过短"))?;
                        for (word, bytes) in self.records[index].words[3..7]
                            .iter()
                            .zip(buffer.chunks_exact_mut(4))
                        {
                            bytes.copy_from_slice(&word.to_le_bytes());
                        }
                    }
                }
            }
            132 => {
                if let Some(index) = self.index(a) {
                    self.records[index].words[2] = b;
                }
            }
            133 => {
                for region in &mut self.records {
                    if region.id() == a {
                        region.words[7] = b;
                    } else if b != 0 {
                        region.words[7] = 0;
                    }
                }
            }
            134 => {
                if let Some(region) = self.get(a) {
                    *output = region.selected();
                }
            }
            135 | 136 => {
                if let Some(index) = self.index(a) {
                    self.records[index].words[if command.code == 135 { 8 } else { 9 }] = b;
                }
                self.geometry_dirty = true;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn index(&self, id: u32) -> Option<usize> {
        self.records.iter().position(|r| r.id() == id)
    }

    fn sort(&mut self) {
        self.records
            .sort_by_key(|r| std::cmp::Reverse(r.priority()));
    }
}

fn words<const N: usize>(buffer: Option<&[u8]>) -> Result<[u32; N]> {
    let bytes = buffer
        .filter(|bytes| bytes.len() >= N * 4)
        .ok_or_else(|| Error::new(0, "输入区域缓冲区缺失或过短"))?;
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
