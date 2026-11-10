use crate::graphics::Rect;
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result, read};

mod events;
pub use events::{Events, HitMask, KeyEvent, KeyEventKind};
mod motion;
mod selection;
pub use motion::PointerMotion;
mod keys;
pub use keys::{KeySample, KeyServices, KeyState, KeyTransition, Keys};
mod state;
pub use state::{CommandState, InputDelivery, InputState};
mod object;
pub use object::{InputFactory, InputObject};
mod button;
pub use button::ButtonObject;
mod slider;
pub use slider::SliderControl;
mod scroll2;
pub use scroll2::Scroll2Control;
mod scroll2_images;
mod scroll2_task;
pub use scroll2_task::Scroll2Object;
mod slider_task;
pub use slider_task::SliderObject;
mod coordinates;
pub use coordinates::CoordinateMap;
mod device;
pub use device::{InputDevice, PointerSample, PointerState};
mod bindings;
pub use bindings::{ButtonSource, DeviceBinding, DeviceKeys};
mod wheel;
pub use wheel::WheelButtons;
mod update;

/// 输入区域的十五 DWORD 记录；可移植位掩码以存在标记代替指针槽。
#[derive(Clone, Debug)]
pub struct Region {
    words: [u32; 15],
    instance: u64,
    mask: Option<HitMask>,
}

impl PartialEq for Region {
    fn eq(&self, other: &Self) -> bool {
        self.words == other.words && self.mask == other.mask
    }
}

impl Eq for Region {}

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

    pub fn event_mask(&self) -> u32 {
        self.words[2]
    }

    pub fn selection_enabled(&self) -> u32 {
        self.words[9]
    }

    pub fn motion_enabled(&self) -> u32 {
        self.words[14]
    }

    pub fn hit_mask(&self) -> Option<&HitMask> {
        self.mask.as_ref()
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
    next_instance: u64,
}

impl Regions {
    pub fn records(&self) -> &[Region] {
        &self.records
    }

    /// 重复编号时返回当前排序中的首项。
    pub fn get(&self, id: u32) -> Option<&Region> {
        self.records.iter().find(|r| r.id() == id)
    }

    /// 给首个匹配项设置显式位掩码；原指针槽仅保存存在标记一。
    pub fn set_hit_mask(&mut self, id: u32, mask: Option<HitMask>) -> bool {
        let Some(index) = self.index(id) else {
            return false;
        };
        let region = &mut self.records[index];
        region.words[11..14].copy_from_slice(&mask.as_ref().map_or([0; 3], |mask| {
            let [width, height] = mask.size();
            [1, width, height]
        }));
        region.mask = mask;
        true
    }

    fn set_image_mask(&mut self, id: u32, handle: u32, host: &dyn crate::kcs::Host) -> Result<()> {
        let Some(index) = self.index(id) else {
            return Ok(());
        };
        let region = &mut self.records[index];
        if handle == 0 {
            if region.mask.take().is_some() {
                region.words[11..14].fill(0);
            }
        } else {
            region.mask = None;
            region.words[11] = 0;
            let image = host.image(handle)?;
            region.words[12..14].copy_from_slice(&HitMask::image_size(image));
            region.mask = Some(HitMask::from_image(image)?);
            region.words[11] = 1;
        }
        Ok(())
    }

    /// 矩形或启用属性变动要求后续输入更新重新检查命中区域。
    pub fn geometry_dirty(&self) -> bool {
        self.geometry_dirty
    }

    /// 输入宿主完成区域重算后消费标记。
    pub fn clear_geometry_dirty(&mut self) {
        self.geometry_dirty = false;
    }

    /// 处理输入对象命令 127～136 及 154；true 表示已消费，未知编号返回 false。
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
                let instance = self.allocate_instance()?;
                self.records.push(Region {
                    words,
                    instance,
                    mask: None,
                });
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
                    let instance = self.allocate_instance()?;
                    let mut region = self.records.remove(index);
                    region.words[1] = b;
                    region.instance = instance;
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
            135 | 136 | 154 => {
                if let Some(index) = self.index(a) {
                    let word = match command.code {
                        135 => 8,
                        136 => 9,
                        _ => 14,
                    };
                    self.records[index].words[word] = b;
                }
                if command.code != 154 {
                    self.geometry_dirty = true;
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn index(&self, id: u32) -> Option<usize> {
        self.records.iter().position(|r| r.id() == id)
    }

    fn current(&self, index: usize, instance: u64) -> Result<(usize, &Region)> {
        if let Some(region) = self
            .records
            .get(index)
            .filter(|region| region.instance == instance)
        {
            return Ok((index, region));
        }
        self.records
            .iter()
            .enumerate()
            .find(|(_, region)| region.instance == instance)
            .ok_or_else(|| Error::new(0, "输入回调移除或重新登记了当前区域，无法继续原游标"))
    }

    fn allocate_instance(&mut self) -> Result<u64> {
        self.next_instance = self
            .next_instance
            .checked_add(1)
            .ok_or_else(|| Error::new(0, "输入区域登记序号耗尽"))?;
        Ok(self.next_instance)
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
