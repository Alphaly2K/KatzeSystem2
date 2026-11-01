use super::{Images, Rect};
use crate::{Error, Result, read};
use std::collections::BTreeMap;

/// 图像列表的无变换来源属性；位置和基点分别截断为整数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageListProperties {
    pub position: [f32; 2],
    pub base_point: [f32; 2],
    pub opacity: u8,
    pub visible: bool,
}

impl Default for ImageListProperties {
    fn default() -> Self {
        Self {
            position: [0.; 2],
            base_point: [0.; 2],
            opacity: 255,
            visible: true,
        }
    }
}

struct Entry {
    image: u32,
    properties: ImageListProperties,
}

/// 持有独立图像并显式整幅重绘的图像列表核心，支持无变换图像来源。
/// 所有包装登记到 owner；调用方应在该任务释放前使用或显式释放本列表。
pub struct ImageList {
    owner: u32,
    image: u32,
    entries: BTreeMap<u32, Entry>,
    offset: [i32; 2],
}

impl ImageList {
    pub fn new(owner: u32, size: [u32; 2], images: &mut Images) -> Result<Self> {
        Ok(Self {
            owner,
            image: images.create(owner, size)?,
            entries: BTreeMap::new(),
            offset: [0; 2],
        })
    }

    /// 登记编号按移位后直接 OR 合并，sender 不先截断至 16 位。
    pub fn key(sender: u32, slot: u32) -> u32 {
        (slot << 16) | sender
    }

    /// 列表自有图像的普通包装，可用于图像查询；寿命与列表所属任务一致。
    pub fn image(&self) -> u32 {
        self.image
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn offset(&self) -> [i32; 2] {
        self.offset
    }

    /// 提交偏移不改变列表自有图像的几何和像素。
    pub fn set_offset(&mut self, offset: [i32; 2]) {
        self.offset = offset;
    }

    /// 新条目创建自己的包装，已有条目保留包装并替换像素来源。
    pub fn set(
        &mut self,
        key: u32,
        source: u32,
        properties: ImageListProperties,
        images: &mut Images,
    ) -> Result<()> {
        if self.image == 0 {
            return Err(Error::new(0, "图像列表已释放"));
        }
        if let Some(entry) = self.entries.get_mut(&key) {
            images.bind(entry.image, source)?;
            entry.properties = properties;
        } else {
            if self.entries.len() == read::MAX_ITEMS {
                return Err(Error::new(0, "图像列表条目数量超过上限"));
            }
            let image = images.share_for(Some(self.owner), source)?;
            self.entries.insert(key, Entry { image, properties });
        }
        Ok(())
    }

    pub fn remove(&mut self, key: u32, images: &mut Images) -> Result<bool> {
        let Some(entry) = self.entries.get(&key) else {
            return Ok(false);
        };
        images.release(entry.image)?;
        self.entries.remove(&key);
        Ok(true)
    }

    /// 释放登记来源，保留列表自有图像，像素在下一次重绘时更新。
    pub fn clear(&mut self, images: &mut Images) -> Result<()> {
        while let Some(&key) = self.entries.keys().next() {
            self.remove(key, images)?;
        }
        Ok(())
    }

    /// 清空自有图像并按无符号 key 顺序重绘所有可见且透明度非零的来源。
    /// 对应全部条目保持默认排序优先级的整幅重绘路径。
    pub fn redraw(&mut self, images: &mut Images) -> Result<()> {
        images.get_mut(self.image)?.pixels_mut().fill(0);
        for entry in self.entries.values() {
            let properties = entry.properties;
            if properties.visible && properties.opacity != 0 {
                images.draw_list_entry(
                    self.image,
                    entry.image,
                    properties.position,
                    properties.base_point,
                    properties.opacity,
                )?;
            }
        }
        Ok(())
    }

    pub fn release(&mut self, images: &mut Images) -> Result<()> {
        self.clear(images)?;
        images.release(self.image)?;
        self.image = 0;
        Ok(())
    }

    pub(crate) fn bounds(&self, images: &Images) -> Result<Rect> {
        Ok(images.get(self.image)?.surface().bounds())
    }
}
