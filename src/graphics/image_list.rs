use super::image::ResourceGeometry;
use super::{Image, Images, Rect};
use crate::anm::{DisplayState, ImageAnimation};
use crate::{Error, Result, read};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

mod animation;
mod shared;
pub(super) use shared::SharedEntries;
use shared::insert_frames;

/// 图像列表的无变换来源属性；位置和基点分别截断为整数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageListProperties {
    pub position: [f32; 2],
    pub base_point: [f32; 2],
    pub opacity: u32,
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
    shared: Option<u32>,
    // 有帧缓存时只借用其中的包装；生命周期由整个条目及所属任务管理。
    image: u32,
    frames: BTreeMap<u32, u32>,
    source_identity: u32,
    capture_revision: Option<u64>,
    resource: Option<u32>,
    resource_geometry: Option<ResourceGeometry>,
    animation: ImageAnimation,
    priority: i32,
    properties: ImageListProperties,
    history: Option<Rect>,
    clip: Rect,
}

#[derive(Clone)]
pub(super) struct EntryRef(Rc<RefCell<Entry>>);

impl EntryRef {
    fn new(entry: Entry) -> Self {
        Self(Rc::new(RefCell::new(entry)))
    }

    fn read(&self) -> std::cell::Ref<'_, Entry> {
        self.0.borrow()
    }

    fn write(&self) -> Result<std::cell::RefMut<'_, Entry>> {
        self.0
            .try_borrow_mut()
            .map_err(|_| Error::new(0, "图像列表条目仍有活动读取视图"))
    }

    fn release(self, images: &mut Images) -> Result<()> {
        if let Some(entry) = Rc::into_inner(self.0) {
            let entry = entry.into_inner();
            if let Some(handle) = entry.shared {
                images.shared_entries.remove(handle);
            }
            entry.release(images)?;
        }
        Ok(())
    }
}

impl Entry {
    fn register_bounds(&mut self, images: &Images, source_base: [f32; 2]) -> Result<Option<Rect>> {
        // 显示状态先按原基点更新几何，再使用请求基点标记脏区。
        self.history = accumulate(self.history, self.bounds(images, source_base)?, self.clip);
        if self.visible() {
            self.history = accumulate(
                self.history,
                self.bounds(images, self.properties.base_point)?,
                self.clip,
            );
            Ok(self.history)
        } else {
            Ok(None)
        }
    }

    fn bounds(&self, images: &Images, base: [f32; 2]) -> Result<Rect> {
        self.resource_geometry.map_or_else(
            || images.list_bounds(self.image, self.properties.position, base),
            |geometry| Ok(geometry.bounds(self.properties.position, base)),
        )
    }

    fn display(&self) -> DisplayState {
        let state = if self.animation.timeline.is_some() {
            self.animation.display()
        } else {
            DisplayState::default()
        };
        let mut state = state.compose(
            self.properties.position,
            self.properties.opacity as i32,
            i32::from(self.properties.visible),
        );
        if self.animation.timeline.is_none() {
            state.blend = self.properties.opacity as i32;
            state.visible = i32::from(self.properties.visible);
        }
        state
    }

    fn visible(&self) -> bool {
        self.display().visible != 0
    }

    fn release(self, images: &mut Images) -> Result<()> {
        if let Some(resource) = self.resource {
            images.release_resource_reference(resource)?;
        }
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
    owner: Option<u32>,
    image: u32,
    entries: BTreeMap<u32, EntryRef>,
    blocks: Vec<Block>,
    offset: [i32; 2],
    animation_pass: Option<animation::Pass>,
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
        Self::new_tiled_for(Some(owner), size, tile_size, images)
    }

    pub(crate) fn new_tiled_for(
        owner: Option<u32>,
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
        let image = match owner {
            Some(owner) => images.create(owner, size)?,
            None => images.insert_unowned(Image::new(size)?, None)?,
        };
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
            animation_pass: None,
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

    /// 重建输出及面块，保留条目、动画、裁剪、优先级和提交偏移；新面块全部标脏。
    pub fn resize(
        &mut self,
        size: [u32; 2],
        tile_size: [u32; 2],
        images: &mut Images,
    ) -> Result<()> {
        self.check_animation_pass()?;
        let replacement = Self::new_tiled_for(self.owner, size, tile_size, images)?;
        images.release(self.image)?;
        self.image = replacement.image;
        self.blocks = replacement.blocks;
        self.invalidate_all();
        Ok(())
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

    pub(crate) fn set_layer(
        &mut self,
        key: u32,
        source: u32,
        properties: ImageListProperties,
        images: &mut Images,
    ) -> Result<()> {
        // 来源节点保留登记时的几何，像素扩边后仍按原范围生成脏区。
        let geometry = ResourceGeometry::from_image(images.get(source)?);
        self.set_pixels_inner(
            key,
            images.pixel_handle(source)?,
            properties,
            Some(geometry),
            images,
        )
    }

    /// 登记独立捕获图像；后续原地过滤使引用此像素的条目自动重绘。
    pub fn set_capture(
        &mut self,
        key: u32,
        source: u32,
        properties: ImageListProperties,
        images: &mut Images,
    ) -> Result<()> {
        self.set(key, source, properties, images)?;
        self.entries.get(&key).unwrap().write()?.capture_revision =
            Some(images.get(source)?.filter_revision);
        Ok(())
    }

    /// 登记独立图像资源，额外持有资源引用；来源更新在动画推进阶段消费。
    pub fn set_resource(
        &mut self,
        key: u32,
        resource: u32,
        properties: ImageListProperties,
        images: &mut Images,
    ) -> Result<()> {
        let source = images.resource_entry(resource)?;
        let image = source.image;
        let geometry = source.geometry;
        images.retain_resource(resource)?;
        let pixels = images.pixel_handle(image)?;
        if let Err(error) = self.set_pixels_inner(key, pixels, properties, Some(geometry), images) {
            images.release_resource_reference(resource)?;
            return Err(error);
        }
        let mut entry = self.entries.get(&key).unwrap().write()?;
        entry.resource = Some(resource);
        // 可见条目在写入显示属性时先刷新资源的共享更新状态。
        if entry.visible() {
            images.resource_updated(resource, true)?;
        }
        Ok(())
    }

    fn refresh_resource(&mut self, key: u32, images: &mut Images) -> Result<()> {
        let mut entry = self.entries.get(&key).unwrap().write()?;
        let Some(resource) = entry.resource else {
            return Ok(());
        };
        images.refresh_resource(resource)?;
        if !images.resource_updated(resource, true)? {
            return Ok(());
        }
        let source = images.resource_entry(resource)?;
        let image = source.image;
        let geometry = source.geometry;
        images.bind_pixels(entry.image, images.pixel_handle(image)?)?;
        entry.resource_geometry = Some(geometry);
        entry.properties.base_point = geometry.base.map(|value| value as f32);
        Ok(())
    }

    /// 按底层像素句柄登记，持有独立包装；不接受普通图像包装或其他资源句柄。
    pub fn set_pixels(
        &mut self,
        key: u32,
        pixels: u32,
        properties: ImageListProperties,
        images: &mut Images,
    ) -> Result<()> {
        self.set_pixels_inner(key, pixels, properties, None, images)
    }

    fn set_pixels_inner(
        &mut self,
        key: u32,
        pixels: u32,
        properties: ImageListProperties,
        geometry: Option<ResourceGeometry>,
        images: &mut Images,
    ) -> Result<()> {
        let source_base = geometry
            .map_or(images.pixel_image(pixels)?.info().base, |geometry| {
                geometry.base
            })
            .map(|v| v as f32);
        let (history, clip, priority) = self.source_history(key, images)?;
        let source_identity = self
            .entries
            .get(&key)
            .map_or(0, |entry| entry.read().source_identity);
        let entry = if let Some(existing) = self.entries.get(&key) {
            let mut entry = existing.write()?;
            images.bind_pixels(entry.image, pixels)?;
            if let Some(resource) = entry.resource.take() {
                images.release_resource_reference(resource)?;
            }
            entry.capture_revision = None;
            entry.resource_geometry = geometry;
            entry.properties = properties;
            entry.history = history;
            existing.clone()
        } else {
            EntryRef::new(Entry {
                shared: None,
                image: images.share_pixels_for(self.owner, pixels)?,
                frames: BTreeMap::new(),
                animation: ImageAnimation::default(),
                source_identity,
                capture_revision: None,
                resource: None,
                resource_geometry: geometry,
                priority,
                properties,
                history,
                clip,
            })
        };
        self.finish_source(key, entry, source_base, images)
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
        let directory = crate::hg3::FrameDirectory::parse(data)?;
        let mut frames = BTreeMap::new();
        let mut length = 0usize;
        for frame in directory.frames {
            if frames.contains_key(&frame.metadata.id) {
                continue;
            }
            let image = frame.decode(data)?;
            length = length
                .checked_add(image.pixels.len())
                .filter(|&length| length <= read::MAX_BYTES)
                .ok_or_else(|| {
                    Error::new(frame.metadata.offset, "图像列表帧集合超过像素存储上限")
                })?;
            frames.insert(frame.metadata.id, Image::from_frame(image)?);
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
        let source_identity = self
            .entries
            .get(&key)
            .map_or(0, |entry| entry.read().source_identity);
        let previous = self.entries.get(&key).cloned();
        let mut previous_entry = previous.as_ref().map(EntryRef::write).transpose()?;
        let frame_owner = self.owner.filter(|_| {
            previous_entry
                .as_ref()
                .is_none_or(|entry| entry.shared.is_none())
        });
        let (frames, image) = insert_frames(images, frame_owner, decoded)?;
        let source_base = images.get(image)?.info().base.map(|v| v as f32);
        let mut animation = previous_entry
            .as_mut()
            .map_or_else(ImageAnimation::default, |entry| {
                std::mem::take(&mut entry.animation)
            });
        animation.reset_time_on_update = true;
        if let Some(timeline) = &mut animation.timeline {
            timeline.interpreter.max_image_id = *frames.last_key_value().unwrap().0 as i32;
        }
        let fresh = Entry {
            shared: previous_entry.as_ref().and_then(|entry| entry.shared),
            image,
            frames,
            animation,
            source_identity,
            capture_revision: None,
            resource: None,
            resource_geometry: None,
            priority,
            properties,
            history,
            clip,
        };
        let entry = if let Some(mut entry) = previous_entry.take() {
            std::mem::replace(&mut *entry, fresh).release(images)?;
            drop(entry);
            previous.as_ref().unwrap().clone()
        } else {
            EntryRef::new(fresh)
        };
        self.finish_source(key, entry, source_base, images)
    }

    /// 选择缓存帧并恢复该帧基点；缺失 key 或 ID 保持当前来源并返回 false。
    /// 选帧本身不产生脏区；重新登记或显式失效后才重绘。
    pub fn select_frame(&mut self, key: u32, id: u32, images: &mut Images) -> Result<bool> {
        self.check_animation_pass()?;
        let Some(entry) = self.entries.get(&key) else {
            return Ok(false);
        };
        let mut entry = entry.write()?;
        let Some(&image) = entry.frames.get(&id) else {
            return Ok(false);
        };
        entry.image = image;
        entry.properties.base_point = images.get(image)?.info().base.map(|v| v as f32);
        Ok(true)
    }

    /// 保存不增加引用的来源身份；没有条目时返回 false，不产生脏区。
    pub fn set_source_identity(&mut self, key: u32, identity: u32) -> bool {
        let Some(entry) = self.entries.get_mut(&key) else {
            return false;
        };
        let Ok(mut entry) = entry.write() else {
            return false;
        };
        entry.source_identity = identity;
        true
    }

    /// 返回同一来源身份中有符号 key 最小的条目；零也可作为查询身份。
    pub fn find_source_identity(&self, identity: u32) -> Option<u32> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.read().source_identity == identity)
            .map(|(&key, _)| key)
            .min_by_key(|&key| key as i32)
    }

    /// 查询条目的当前像素来源；缺失时返回零，不增加引用或推进动画。
    pub fn source_pixels(&self, key: u32, images: &Images) -> Result<u32> {
        self.entries
            .get(&key)
            .map_or(Ok(0), |entry| images.pixel_handle(entry.read().image))
    }

    /// 按有符号 key 查找当前像素身份；不增加引用或推进动画。
    pub fn find_pixels(&self, pixels: u32, images: &Images) -> Result<Option<u32>> {
        let mut found = None;
        for (&key, entry) in &self.entries {
            if images.pixel_handle(entry.read().image)? == pixels
                && found.is_none_or(|current| (key as i32) < current as i32)
            {
                found = Some(key);
            }
        }
        Ok(found)
    }

    fn source_history(&mut self, key: u32, images: &Images) -> Result<(Option<Rect>, Rect, i32)> {
        self.check_animation_pass()?;
        let viewport = self.bounds(images)?;
        let mut history = None;
        let mut clip = viewport;
        let mut priority = (key >> 16) as i32;
        if let Some(entry) = self.entries.get(&key).cloned() {
            let entry = entry.read();
            history = entry.history;
            clip = entry.clip;
            priority = entry.priority;
            if entry.visible() {
                history = accumulate(
                    history,
                    entry.bounds(images, entry.properties.base_point)?,
                    clip,
                );
                self.mark(history);
            }
        } else if self.entries.len() == read::MAX_ITEMS {
            return Err(Error::new(0, "图像列表条目数量超过上限"));
        }
        Ok((history, clip, priority))
    }

    pub(crate) fn update_properties(
        &mut self,
        key: u32,
        properties: ImageListProperties,
        images: &Images,
    ) -> Result<bool> {
        if !self.contains_key(key) {
            return Ok(false);
        }
        let (history, clip, _) = self.source_history(key, images)?;
        let mut entry = self.entries.get(&key).unwrap().write()?;
        let previous_base = entry.properties.base_point;
        entry.properties = properties;
        entry.history = accumulate(history, entry.bounds(images, previous_base)?, clip);
        if entry.visible() {
            entry.history = accumulate(
                entry.history,
                entry.bounds(images, entry.properties.base_point)?,
                clip,
            );
            let history = entry.history;
            drop(entry);
            self.mark(history);
        }
        Ok(true)
    }

    fn finish_source(
        &mut self,
        key: u32,
        entry: EntryRef,
        source_base: [f32; 2],
        images: &Images,
    ) -> Result<()> {
        self.mark(entry.write()?.register_bounds(images, source_base)?);
        self.entries.insert(key, entry);
        Ok(())
    }

    pub fn remove(&mut self, key: u32, images: &mut Images) -> Result<bool> {
        self.check_animation_pass()?;
        let Some(entry) = self.entries.get(&key) else {
            return Ok(false);
        };
        let entry = entry.read();
        let history = if entry.visible() {
            accumulate(
                entry.history,
                entry.bounds(images, entry.properties.base_point)?,
                entry.clip,
            )
        } else {
            None
        };
        drop(entry);
        self.mark(history);
        self.entries.remove(&key).unwrap().release(images)?;
        Ok(true)
    }

    /// 初始优先级取 key 的高 16 位；显式优先级按有符号整数排序。
    pub fn priority(&self, key: u32) -> Option<i32> {
        self.entries.get(&key).map(|entry| entry.read().priority)
    }

    /// 相同优先级也将可见条目的历史范围标脏；来源重登记保留优先级。
    pub fn set_priority(&mut self, key: u32, priority: i32, images: &Images) -> Result<bool> {
        self.check_animation_pass()?;
        self.bounds(images)?;
        let Some(entry) = self.entries.get_mut(&key) else {
            return Ok(false);
        };
        let mut entry = entry.write()?;
        entry.priority = priority;
        let history = entry.visible().then_some(entry.history).flatten();
        drop(entry);
        self.mark(history);
        Ok(true)
    }

    /// 仅在显示状态变化时更新几何和脏区，保留来源与请求基点。
    pub fn set_visible(&mut self, key: u32, visible: bool, images: &Images) -> Result<bool> {
        self.check_animation_pass()?;
        self.bounds(images)?;
        let Some(entry) = self.entries.get_mut(&key) else {
            return Ok(false);
        };
        let mut entry = entry.write()?;
        if entry.visible() != visible {
            entry.properties.visible = visible;
            entry.history = accumulate(
                entry.history,
                entry.bounds(images, entry.properties.base_point)?,
                entry.clip,
            );
            let history = entry.history;
            drop(entry);
            self.mark(history);
        }
        Ok(true)
    }

    /// 修改条目裁剪；None 恢复完整视口且不标脏，空矩形恢复视口并按变化标脏。
    pub fn set_clip(&mut self, key: u32, clip: Option<Rect>, images: &Images) -> Result<bool> {
        self.check_animation_pass()?;
        let viewport = self.bounds(images)?;
        let Some(entry) = self.entries.get_mut(&key) else {
            return Ok(false);
        };
        let mut entry = entry.write()?;
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
            let next = entry.visible().then_some(entry.history).flatten();
            drop(entry);
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
        self.check_animation_pass()?;
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

    pub(crate) fn invalidate_source(&mut self, key: u32, images: &Images) -> Result<()> {
        let mut entry = self.entries.get(&key).unwrap().write()?;
        if entry.visible() {
            entry.history = accumulate(
                entry.history,
                entry.bounds(images, entry.properties.base_point)?,
                entry.clip,
            );
            let history = entry.history;
            drop(entry);
            self.mark(history);
        }
        Ok(())
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
    /// 捕获来源的过滤自动产生脏区；其他像素修改需重登记或 invalidate_all。
    pub fn update(&mut self, images: &mut Images) -> Result<Vec<Rect>> {
        self.update_region(images, None)
    }

    /// 局部更新只修改裁剪内的像素，保留全部脏区并返回原始提交矩形。
    pub fn update_region(&mut self, images: &mut Images, clip: Option<Rect>) -> Result<Vec<Rect>> {
        self.check_animation_pass()?;
        images.get(self.image)?;
        let mut changed = Vec::new();
        for entry in self.entries.values() {
            let current = entry.read();
            if let Some(previous) = current.capture_revision {
                let revision = images.get(current.image)?.filter_revision;
                if previous != revision {
                    changed.push(current.history);
                    drop(current);
                    entry.write()?.capture_revision = Some(revision);
                }
            }
        }
        for bounds in changed {
            self.mark(bounds);
        }
        let regions: Vec<_> = self.dirty_rectangles().collect();
        let updates = regions
            .iter()
            .filter_map(|&region| clip.map_or(Some(region), |clip| region.intersect(clip)));
        for region in updates.clone() {
            images.get_mut(self.image)?.fill_rect_from_bottom(region, 0);
        }
        let mut entries: Vec<_> = self.entries.iter().collect();
        entries.sort_unstable_by_key(|&(&key, entry)| (entry.read().priority, key));
        for (_, entry) in entries {
            let entry = entry.read();
            let display = entry.display();
            let p = entry.properties;
            if display.visible != 0 && display.blend != 0 {
                for clip in updates.clone() {
                    let Some(clip) = clip.intersect(entry.clip) else {
                        continue;
                    };
                    images.draw_list_entry(
                        self.image,
                        (entry.image, entry.resource_geometry),
                        entry.properties.position,
                        p.base_point,
                        display.blend as u32,
                        clip,
                    )?;
                }
            }
        }
        if clip.is_none() {
            for block in &mut self.blocks {
                block.dirty = None;
            }
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
        self.animation_pass = None;
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
