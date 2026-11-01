use super::{Image, Images, Rect};
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
    // 有帧缓存时只借用其中的包装；生命周期由整个条目及所属任务管理。
    image: u32,
    frames: BTreeMap<u32, u32>,
    priority: i32,
    properties: ImageListProperties,
    history: Option<Rect>,
    clip: Rect,
}

impl Entry {
    fn release(self, images: &mut Images) -> Result<()> {
        if self.frames.is_empty() {
            images.release(self.image)?;
        }
        for &image in self.frames.values() {
            images.release(image)?;
        }
        Ok(())
    }
}

struct Block {
    bounds: Rect,
    dirty: Option<Rect>,
}

/// 持有自有图像及独立来源包装的无变换图像列表。
/// 所有包装登记到 owner；调用方应在该任务释放前使用或显式释放本列表。
pub struct ImageList {
    owner: u32,
    image: u32,
    entries: BTreeMap<u32, Entry>,
    blocks: Vec<Block>,
    offset: [i32; 2],
}

impl ImageList {
    /// 使用一个覆盖自有图像的面块。
    pub fn new(owner: u32, size: [u32; 2], images: &mut Images) -> Result<Self> {
        Self::new_tiled(owner, size, [size[0].max(size[1]).max(1); 2], images)
    }

    /// tile_size 指定面块横纵步长；面块高度沿用基线读取横向配置的行为。
    pub fn new_tiled(
        owner: u32,
        size: [u32; 2],
        tile_size: [u32; 2],
        images: &mut Images,
    ) -> Result<Self> {
        if tile_size.contains(&0) {
            return Err(Error::new(0, "图像列表面块步长不能为零"));
        }
        let grid = std::array::from_fn::<_, 2, _>(|i| size[i].div_ceil(tile_size[i]));
        let count = u64::from(grid[0]) * u64::from(grid[1]);
        if count > read::MAX_ITEMS as u64 {
            return Err(Error::new(0, "图像列表面块数量超过上限"));
        }
        let image = images.create(owner, size)?;
        let blocks = (0..count as u32)
            .map(|index| {
                let x = (index % grid[0]) * tile_size[0];
                let y = (index / grid[0]) * tile_size[1];
                Block {
                    bounds: Rect::from_xywh(
                        x as i32,
                        y as i32,
                        (size[0] - x).min(tile_size[0]) as i32,
                        (size[1] - y).min(tile_size[0]) as i32,
                    ),
                    dirty: None,
                }
            })
            .collect();
        Ok(Self {
            owner,
            image,
            entries: BTreeMap::new(),
            blocks,
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
    pub(crate) fn contains_key(&self, key: u32) -> bool {
        self.entries.contains_key(&key)
    }
    pub fn offset(&self) -> [i32; 2] {
        self.offset
    }

    /// 提交偏移不改变列表自有图像的几何和像素，也不产生脏区。
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
        self.set_pixels(key, images.pixel_handle(source)?, properties, images)
    }

    /// 按底层像素句柄登记，持有独立包装；不接受普通图像包装或其他资源句柄。
    pub fn set_pixels(
        &mut self,
        key: u32,
        pixels: u32,
        properties: ImageListProperties,
        images: &mut Images,
    ) -> Result<()> {
        let source_base = images.pixel_image(pixels)?.info().base.map(|v| v as f32);
        let (history, clip, priority) = self.source_history(key, images)?;
        let image = if let Some(entry) = self.entries.get(&key) {
            let image = entry.image;
            images.bind_pixels(image, pixels)?;
            image
        } else {
            images.share_pixels_for(Some(self.owner), pixels)?
        };
        let frames = self
            .entries
            .get_mut(&key)
            .map(|entry| std::mem::take(&mut entry.frames))
            .unwrap_or_default();
        self.finish_source(
            key,
            Entry {
                image,
                frames,
                priority,
                properties,
                history,
                clip,
            },
            source_base,
            images,
        )
    }

    /// 登记完整 HG-3 原始数据，独立持有全部帧；初始显示最小无符号 ID。
    pub fn set_hg3(
        &mut self,
        key: u32,
        data: &[u8],
        properties: ImageListProperties,
        images: &mut Images,
    ) -> Result<()> {
        self.set_frames(key, Self::decode_hg3(data)?, properties, images)
    }

    pub(crate) fn decode_hg3(data: &[u8]) -> Result<BTreeMap<u32, Image>> {
        let metadata = crate::hg3::Metadata::parse(data)?;
        let mut frames = BTreeMap::new();
        let mut length = 0usize;
        for frame in metadata.frames {
            if frames.contains_key(&frame.id) {
                continue;
            }
            let image = crate::hg3::decode_frame(data, frame.id as i32)?
                .ok_or_else(|| Error::new(frame.offset, "HG-3 帧目录与解码结果不匹配"))?;
            length = length
                .checked_add(image.pixels.len())
                .filter(|&length| length <= read::MAX_BYTES)
                .ok_or_else(|| Error::new(frame.offset, "图像列表帧集合超过像素存储上限"))?;
            frames.insert(frame.id, Image::from_frame(image)?);
        }
        Ok(frames)
    }

    pub(crate) fn set_frames(
        &mut self,
        key: u32,
        decoded: BTreeMap<u32, Image>,
        properties: ImageListProperties,
        images: &mut Images,
    ) -> Result<()> {
        let (history, clip, priority) = self.source_history(key, images)?;
        let mut frames = BTreeMap::new();
        for (id, image) in decoded {
            let alpha = Some(image.info().reserved != 0);
            match images.insert(self.owner, image, alpha) {
                Ok(image) => {
                    frames.insert(id, image);
                }
                Err(error) => {
                    for &image in frames.values() {
                        images.release(image)?;
                    }
                    return Err(error);
                }
            }
        }
        let image = *frames
            .first_key_value()
            .ok_or_else(|| Error::new(0, "图像列表原始来源没有可用帧"))?
            .1;
        let source_base = images.get(image)?.info().base.map(|v| v as f32);
        if let Some(previous) = self.entries.remove(&key) {
            previous.release(images)?;
        }
        self.finish_source(
            key,
            Entry {
                image,
                frames,
                priority,
                properties,
                history,
                clip,
            },
            source_base,
            images,
        )
    }

    /// 选择缓存帧并恢复该帧基点；缺失 key 或 ID 保持当前来源并返回 false。
    /// 选帧本身不产生脏区；重新登记或显式失效后才重绘。
    pub fn select_frame(&mut self, key: u32, id: u32, images: &mut Images) -> Result<bool> {
        let Some(entry) = self.entries.get_mut(&key) else {
            return Ok(false);
        };
        let Some(&image) = entry.frames.get(&id) else {
            return Ok(false);
        };
        entry.image = image;
        entry.properties.base_point = images.get(image)?.info().base.map(|v| v as f32);
        Ok(true)
    }

    fn source_history(&mut self, key: u32, images: &Images) -> Result<(Option<Rect>, Rect, i32)> {
        let viewport = self.bounds(images)?;
        let mut history = None;
        let mut clip = viewport;
        let mut priority = (key >> 16) as i32;
        if let Some(entry) = self.entries.get(&key) {
            history = entry.history;
            clip = entry.clip;
            priority = entry.priority;
            if entry.properties.visible {
                history = accumulate(
                    history,
                    images.list_bounds(
                        entry.image,
                        entry.properties.position,
                        entry.properties.base_point,
                    )?,
                    clip,
                );
                self.mark(history);
            }
        } else if self.entries.len() == read::MAX_ITEMS {
            return Err(Error::new(0, "图像列表条目数量超过上限"));
        }
        Ok((history, clip, priority))
    }

    fn finish_source(
        &mut self,
        key: u32,
        mut entry: Entry,
        source_base: [f32; 2],
        images: &Images,
    ) -> Result<()> {
        let properties = entry.properties;
        // 设置显示状态时先使用来源基点更新几何，再写入请求基点。
        entry.history = accumulate(
            entry.history,
            images.list_bounds(entry.image, properties.position, source_base)?,
            entry.clip,
        );
        if properties.visible {
            entry.history = accumulate(
                entry.history,
                images.list_bounds(entry.image, properties.position, properties.base_point)?,
                entry.clip,
            );
            self.mark(entry.history);
        }
        self.entries.insert(key, entry);
        Ok(())
    }

    pub fn remove(&mut self, key: u32, images: &mut Images) -> Result<bool> {
        let Some(entry) = self.entries.get(&key) else {
            return Ok(false);
        };
        let image = entry.image;
        if entry.properties.visible {
            let history = accumulate(
                entry.history,
                images.list_bounds(
                    image,
                    entry.properties.position,
                    entry.properties.base_point,
                )?,
                entry.clip,
            );
            self.mark(history);
        }
        self.entries.remove(&key).unwrap().release(images)?;
        Ok(true)
    }

    /// 初始优先级取 key 的高 16 位；显式优先级按有符号整数排序。
    pub fn priority(&self, key: u32) -> Option<i32> {
        self.entries.get(&key).map(|entry| entry.priority)
    }

    /// 相同优先级也将可见条目的历史范围标脏；来源重登记保留优先级。
    pub fn set_priority(&mut self, key: u32, priority: i32, images: &Images) -> Result<bool> {
        self.bounds(images)?;
        let Some(entry) = self.entries.get_mut(&key) else {
            return Ok(false);
        };
        entry.priority = priority;
        let history = entry.properties.visible.then_some(entry.history).flatten();
        self.mark(history);
        Ok(true)
    }

    /// 仅在显示状态变化时更新几何和脏区，保留来源与请求基点。
    pub fn set_visible(&mut self, key: u32, visible: bool, images: &Images) -> Result<bool> {
        self.bounds(images)?;
        let Some(entry) = self.entries.get_mut(&key) else {
            return Ok(false);
        };
        if entry.properties.visible != visible {
            entry.properties.visible = visible;
            entry.history = accumulate(
                entry.history,
                images.list_bounds(
                    entry.image,
                    entry.properties.position,
                    entry.properties.base_point,
                )?,
                entry.clip,
            );
            let history = entry.history;
            self.mark(history);
        }
        Ok(true)
    }

    /// 修改条目裁剪；None 恢复完整视口且不标脏，空矩形恢复视口并按变化标脏。
    pub fn set_clip(&mut self, key: u32, clip: Option<Rect>, images: &Images) -> Result<bool> {
        let viewport = self.bounds(images)?;
        let Some(entry) = self.entries.get_mut(&key) else {
            return Ok(false);
        };
        let Some(clip) = clip else {
            entry.clip = viewport;
            entry.history = entry
                .history
                .and_then(|history| history.intersect(viewport));
            return Ok(true);
        };
        if clip != entry.clip {
            let previous = entry.history;
            entry.clip = if clip.left >= clip.right || clip.top >= clip.bottom {
                viewport
            } else {
                clip
            };
            entry.history = entry
                .history
                .and_then(|history| history.intersect(entry.clip));
            let next = entry.properties.visible.then_some(entry.history).flatten();
            self.mark(previous);
            self.mark(next);
        }
        Ok(true)
    }

    pub(crate) fn has_blocks(&self) -> bool {
        !self.blocks.is_empty()
    }

    /// 释放全部来源，立即清空自有图像并将全部面块标脏。
    pub fn clear(&mut self, images: &mut Images) -> Result<()> {
        self.release_entries(images)?;
        if self.image != 0 {
            images.get_mut(self.image)?.pixels_mut().fill(0);
            self.invalidate_all();
        }
        Ok(())
    }

    fn release_entries(&mut self, images: &mut Images) -> Result<()> {
        while let Some(&key) = self.entries.keys().next() {
            self.entries.remove(&key).unwrap().release(images)?;
        }
        Ok(())
    }

    pub fn dirty_rectangles(&self) -> impl Iterator<Item = Rect> + '_ {
        self.blocks.iter().filter_map(|block| block.dirty)
    }

    pub fn invalidate_all(&mut self) {
        for block in &mut self.blocks {
            block.dirty = Some(block.bounds);
        }
    }

    fn mark(&mut self, rect: Option<Rect>) {
        let Some(rect) = rect else {
            return;
        };
        for block in &mut self.blocks {
            if let Some(clipped) = rect.intersect(block.bounds) {
                block.dirty = Some(union(block.dirty, clipped));
            }
        }
    }

    /// 清除全部脏区后按条目、面块顺序绘制；成功后消费脏区并返回提交矩形。
    /// 来源像素修改不单独产生脏区，重登记或 invalidate_all 后才重绘。
    pub fn update(&mut self, images: &mut Images) -> Result<Vec<Rect>> {
        images.get(self.image)?;
        let regions: Vec<_> = self.dirty_rectangles().collect();
        for &region in &regions {
            images.get_mut(self.image)?.fill_rect_from_bottom(region, 0);
        }
        let mut entries: Vec<_> = self.entries.iter().collect();
        entries.sort_unstable_by_key(|&(&key, entry)| (entry.priority, key));
        for (_, entry) in entries {
            let p = entry.properties;
            if p.visible && p.opacity != 0 {
                for &clip in &regions {
                    let Some(clip) = clip.intersect(entry.clip) else {
                        continue;
                    };
                    images.draw_list_entry(
                        self.image,
                        entry.image,
                        p.position,
                        p.base_point,
                        p.opacity,
                        clip,
                    )?;
                }
            }
        }
        for block in &mut self.blocks {
            block.dirty = None;
        }
        Ok(regions)
    }

    /// 将全部面块标脏后更新，保留重叠面块的重复混合。
    pub fn redraw(&mut self, images: &mut Images) -> Result<()> {
        self.invalidate_all();
        self.update(images)?;
        Ok(())
    }

    pub fn release(&mut self, images: &mut Images) -> Result<()> {
        self.release_entries(images)?;
        images.release(self.image)?;
        self.image = 0;
        self.blocks = Vec::new();
        Ok(())
    }

    pub(crate) fn bounds(&self, images: &Images) -> Result<Rect> {
        Ok(images.get(self.image)?.surface().bounds())
    }
}

fn union(previous: Option<Rect>, next: Rect) -> Rect {
    previous.map_or(next, |old| Rect {
        left: old.left.min(next.left),
        top: old.top.min(next.top),
        right: old.right.max(next.right),
        bottom: old.bottom.max(next.bottom),
    })
}

fn accumulate(previous: Option<Rect>, next: Rect, viewport: Rect) -> Option<Rect> {
    if next.left >= next.right || next.top >= next.bottom {
        return previous;
    }
    union(previous, next).intersect(viewport)
}
