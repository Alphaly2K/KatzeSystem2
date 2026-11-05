use super::{ImageRegion, Rect, Surface};
use crate::hg3::{DecodedFrame, StandardInfo};
use crate::kcs::ResourceServices;
use crate::resource::ResourceMode;
use crate::{Error, HandleAllocator, Result, hg3, read};
use std::collections::BTreeMap;

/// 普通 111 的单个图像名称；帧号位于冒号后，扩展名按原名称重新拼接。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageReference {
    pub name: Vec<u8>,
    pub frame: i32,
}

impl ImageReference {
    pub fn parse(input: &[u8]) -> Result<Self> {
        let input = &input[..input.iter().position(|&b| b == 0).unwrap_or(input.len())];
        if input.len() >= 260 {
            return Err(Error::new(0, "图像资源名超过基线缓冲区范围"));
        }
        let Some(colon) = input.iter().position(|&b| b == b':') else {
            return Ok(Self {
                name: input.to_vec(),
                frame: 0,
            });
        };
        // 原辅助函数从末尾逐字节找扩展名，不进行 CP932 字符扫描。
        let dot = input
            .iter()
            .rposition(|b| matches!(b, b'.' | b'/' | b'\\'))
            .filter(|&p| input[p] == b'.');
        let extension = dot.map_or(&[][..], |p| &input[p + 1..]);
        let end = dot.filter(|&p| p > colon).unwrap_or(input.len());
        let frame = crate::text::integer_prefix(&input[colon + 1..end])
            .map_or(0, |(_, value)| value as i32);
        // 扩展名位于冒号之前时，截断后冒号已不可见；追加仍按同一规则剥去末段扩展名。
        let name = &input[..colon.min(dot.unwrap_or(input.len()))];
        let dot = name
            .iter()
            .rposition(|b| matches!(b, b'.' | b'/' | b'\\'))
            .filter(|&p| name[p] == b'.');
        let mut name = name[..dot.unwrap_or(name.len())].to_vec();
        name.push(b'.');
        name.extend(extension);
        Ok(Self { name, frame })
    }
}

/// 带有裁剪几何和帧 ID 的已解码图像；像素始终为顶端起始 BGRA8。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    id: u32,
    info: StandardInfo,
    surface: Surface,
    pub(super) filter_revision: u64,
}

impl Image {
    /// 创建普通 112 使用的零初始化图像，画布、偏移、基点及帧 ID 均为零。
    pub fn new(size: [u32; 2]) -> Result<Self> {
        Ok(Self {
            id: 0,
            info: StandardInfo {
                size,
                bit_depth: 32,
                offset: [0; 2],
                canvas_size: [0; 2],
                reserved: 0,
                base: [0; 2],
            },
            surface: Surface::new(size)?,
            filter_revision: 0,
        })
    }

    pub fn from_frame(frame: DecodedFrame) -> Result<Self> {
        if frame.info.bit_depth as i16 != 32 {
            return Err(Error::new(0, "图像对象需要 32 位像素"));
        }
        Ok(Self {
            id: frame.id,
            info: frame.info,
            surface: Surface::from_bgra(frame.info.size, frame.pixels)?,
            filter_revision: 0,
        })
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn info(&self) -> &StandardInfo {
        &self.info
    }

    pub(crate) fn use_pixel_canvas(&mut self) {
        self.info.offset = [0; 2];
        self.info.canvas_size = self.info.size;
    }

    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        self.surface.pixels_mut()
    }

    pub fn fill_rect_from_bottom(&mut self, rect: Rect, argb: u32) -> bool {
        self.surface.fill_rect_from_bottom(rect, argb)
    }

    /// 普通 115 的图像目标分支：目标位置叠加源偏移，两端矩形独立裁剪，不缩放。
    pub fn blend_from(&mut self, source: &Self, source_rect: Rect, at: [i32; 2]) -> bool {
        let Some(region) = self.blend_region(source, source_rect, at) else {
            return false;
        };
        self.surface.blend_region(&source.surface, region, 255);
        true
    }

    /// 对自身执行相同混合，保留底端起始、从左向右的重叠反馈。
    pub fn blend_within(&mut self, source_rect: Rect, at: [i32; 2]) -> bool {
        let Some(region) = self.blend_region(self, source_rect, at) else {
            return false;
        };
        self.surface.blend_region_within(region, 255);
        true
    }

    fn blend_region(&self, source: &Self, source_rect: Rect, at: [i32; 2]) -> Option<ImageRegion> {
        let at = [
            at[0].wrapping_add(source.info.offset[0]),
            at[1].wrapping_add(source.info.offset[1]),
        ];
        let mut region = self.surface.clip_blend(&source.surface, source_rect, at)?;
        region.y = self.info.size[1] as usize - region.y - region.height;
        region.source_y = source.info.size[1] as usize - region.source_y - region.height;
        Some(region)
    }

    fn list_bounds(&self, position: [f32; 2], base: [f32; 2]) -> Rect {
        let at = std::array::from_fn::<_, 2, _>(|i| {
            list_coordinate(position[i])
                .wrapping_sub(list_coordinate(base[i]))
                .wrapping_add(self.info.offset[i])
        });
        Rect::from_xywh(
            at[0],
            at[1],
            self.info.size[0] as i32,
            self.info.size[1] as i32,
        )
    }

    fn list_region(
        &self,
        source: &Self,
        position: [f32; 2],
        base: [f32; 2],
        clip: Rect,
    ) -> Option<ImageRegion> {
        let bounds = source.list_bounds(position, base);
        let rect = bounds.intersect(self.surface.bounds())?.intersect(clip)?;
        let width = (rect.right - rect.left) as usize;
        let height = (rect.bottom - rect.top) as usize;
        Some(ImageRegion {
            x: rect.left as usize,
            y: self.info.size[1] as usize - rect.bottom as usize,
            source_x: rect.left.wrapping_sub(bounds.left) as usize,
            source_y: source.info.size[1] as usize
                - rect.top.wrapping_sub(bounds.top) as usize
                - height,
            width,
            height,
        })
    }

    fn draw_list_from(
        &mut self,
        source: &Self,
        position: [f32; 2],
        base: [f32; 2],
        opacity: u8,
        clip: Rect,
        has_alpha: &mut Option<bool>,
    ) {
        let Some(region) = self.list_region(source, position, base, clip) else {
            return;
        };
        let has_alpha = *has_alpha.get_or_insert_with(|| {
            source
                .surface
                .pixels()
                .chunks_exact(4)
                .any(|pixel| pixel[3] != 255)
        });
        if opacity == 255 && !has_alpha {
            self.surface.copy_from(
                &source.surface,
                Rect::from_xywh(
                    region.source_x as i32,
                    region.source_y as i32,
                    region.width as i32,
                    region.height as i32,
                ),
                [region.x as i32, region.y as i32],
            );
        } else {
            self.surface.blend_region(&source.surface, region, opacity);
        }
    }

    /// 普通 111 的后续图像合成，保留首图几何、零透明度和独立裁剪规则。
    pub fn composite(&mut self, source: &Self) -> bool {
        let bounds = Rect::from_xywh(
            self.info.offset[0],
            self.info.offset[1],
            self.info.size[0] as i32,
            self.info.size[1] as i32,
        );
        let Some(target) = Rect::from_xywh(
            source.info.offset[0],
            source.info.offset[1],
            self.info.size[0] as i32,
            self.info.size[1] as i32,
        )
        .intersect(bounds) else {
            return false;
        };
        // 即使源边界回绕，原入口也先与自己的全局矩形求交。
        let source_bounds = Rect::from_xywh(
            source.info.offset[0],
            source.info.offset[1],
            source.info.size[0] as i32,
            source.info.size[1] as i32,
        );
        if source_bounds.left >= source_bounds.right || source_bounds.top >= source_bounds.bottom {
            return false;
        }
        let at = [
            target.left.wrapping_sub(bounds.left),
            target.top.wrapping_sub(bounds.top),
        ];
        let Some(mut region) =
            self.surface
                .clip_blend(&source.surface, source.surface.bounds(), at)
        else {
            return false;
        };
        // 低层绘制坐标直接作用于底端起始的内存，两端裁剪后才转换行方向。
        region.y = self.info.size[1] as usize - region.y - region.height;
        region.source_y = source.info.size[1] as usize - region.source_y - region.height;
        self.surface.blend_region(&source.surface, region, 0);
        true
    }
}

struct ImageEntry {
    owner: Option<u32>,
    references: usize,
    // 零表示包装尚未绑定像素，与已绑定的零尺寸图像分别保存。
    data: u32,
}

struct ImageData {
    references: usize,
    // 矩形填充不使像素对象的 Alpha 分类失效。
    has_alpha: Option<bool>,
    image: Image,
}

/// 普通任务与绘制使用者共享的图像；最后一个引用释放后句柄失效。
#[derive(Default)]
pub struct Images {
    entries: BTreeMap<u32, ImageEntry>,
    data: BTreeMap<u32, ImageData>,
    handles: HandleAllocator,
}

impl Images {
    /// 与其他类型的存储共用编号源，避免跨类型编号碰撞。
    pub fn with_handles(handles: HandleAllocator) -> Self {
        Self {
            entries: BTreeMap::new(),
            data: BTreeMap::new(),
            handles,
        }
    }

    pub fn create(&mut self, owner: u32, size: [u32; 2]) -> Result<u32> {
        self.insert(owner, Image::new(size)?, None)
    }

    /// 创建尚未绑定像素的包装；pixel_handle 返回零，recreate 首次分配像素。
    pub fn create_empty(&mut self, owner: u32) -> Result<u32> {
        self.empty_wrapper(Some(owner))
    }

    fn empty_wrapper(&mut self, owner: Option<u32>) -> Result<u32> {
        if self.entries.len() == read::MAX_ITEMS {
            return Err(Error::new(0, "图像对象数量超过上限"));
        }
        let handle = self.handles.allocate()?;
        self.entries.insert(
            handle,
            ImageEntry {
                owner,
                references: 1,
                data: 0,
            },
        );
        Ok(handle)
    }

    /// 重建包装绑定的空白图像；基线在任一轴尺寸相同时复用原像素。
    /// 两轴均不同时替换像素，其他包装继续持有旧像素；失败保留原绑定。
    pub fn recreate(&mut self, handle: u32, size: [u32; 2]) -> Result<()> {
        let previous = self.entry_mut(handle)?.data;
        if let Some(data) = self.data.get(&previous) {
            let old_size = data.image.info().size;
            if old_size.into_iter().zip(size).any(|(old, new)| old == new) {
                return Ok(());
            }
        }
        let image = Image::new(size)?;
        let pixels = self.handles.allocate()?;
        self.data.insert(
            pixels,
            ImageData {
                references: 1,
                has_alpha: None,
                image,
            },
        );
        self.entries.get_mut(&handle).unwrap().data = pixels;
        release_data(&mut self.data, previous);
        Ok(())
    }

    pub(crate) fn copy_snapshot(
        &mut self,
        target: u32,
        source: u32,
        rectangle: Option<Rect>,
    ) -> Result<bool> {
        let pixels = self.pixel_handle(source)?;
        let data = self
            .data
            .get(&pixels)
            .ok_or_else(|| Error::new(0, "截图来源没有像素"))?;
        let alpha = data.has_alpha;
        let mut snapshot = if let Some(rect) = rectangle {
            let source = &data.image;
            let [width, height] = source.info.size.map(|value| value as i32);
            if rect.left > width || rect.top > height {
                return Ok(false);
            }
            if rect.left < 0 || rect.top < 0 || rect.right < rect.left || rect.bottom < rect.top {
                return Err(Error::new(0, "截图裁剪范围无效"));
            }
            let size = [
                rect.right.min(width) - rect.left,
                rect.bottom.min(height) - rect.top,
            ];
            let mut snapshot = Image::new(size.map(|value| value as u32))?;
            snapshot.id = source.id;
            snapshot.info.reserved = source.info.reserved;
            snapshot.use_pixel_canvas();
            snapshot.surface.copy_from(
                &source.surface,
                Rect::from_xywh(rect.left, height - rect.top - size[1], size[0], size[1]),
                [0; 2],
            );
            snapshot
        } else {
            data.image.clone()
        };
        self.recreate(target, snapshot.info.size)?;
        if rectangle.is_some() {
            snapshot.info.base = self.get(target)?.info.base;
        }
        let pixels = self.pixel_handle(target)?;
        let data = self.data.get_mut(&pixels).unwrap();
        data.image = snapshot;
        data.has_alpha = alpha;
        Ok(true)
    }

    /// 接收已格式展开的名称列表，仅 ASCII 空格分隔，不解释引号。
    pub fn load<S: ResourceServices + ?Sized>(
        &mut self,
        owner: u32,
        names: &[u8],
        services: &mut S,
    ) -> Result<u32> {
        let mut image: Option<Image> = None;
        let names = &names[..names.iter().position(|&b| b == 0).unwrap_or(names.len())];
        for token in names.split(|&b| b == b' ').filter(|s| !s.is_empty()) {
            let reference = ImageReference::parse(token)?;
            let name = services.resolve(b"image", &reference.name)?;
            let Some(bytes) = services.read(&name, ResourceMode::LooseFirst)? else {
                continue;
            };
            if bytes.is_empty() {
                continue;
            }
            let Some(frame) = hg3::decode_frame(&bytes, reference.frame)? else {
                continue;
            };
            let next = Image::from_frame(frame)?;
            if let Some(image) = &mut image {
                image.composite(&next);
            } else {
                image = Some(next);
            }
        }
        let Some(image) = image else {
            return Ok(0);
        };
        let has_alpha = Some(image.info().reserved != 0);
        self.insert(owner, image, has_alpha)
    }

    pub(crate) fn load_fes<S: ResourceServices + ?Sized>(
        &mut self,
        handle: u32,
        names: &[u8],
        frame: i32,
        direct: bool,
        services: &mut S,
    ) -> Result<bool> {
        let names = &names[..names.iter().position(|&b| b == 0).unwrap_or(names.len())];
        if direct && names.is_empty() {
            return Ok(false);
        }
        self.bind_pixels(handle, 0)?;
        for token in names
            .split(|&b| !direct && b == b' ')
            .filter(|s| !s.is_empty())
        {
            if token.len() >= 260 {
                return Err(Error::new(0, "图像资源名超过基线缓冲区范围"));
            }
            let name = if direct {
                token.to_vec()
            } else {
                services.resolve(b"image", token)?
            };
            let Some(bytes) = services.read(&name, ResourceMode::LooseFirst)? else {
                continue;
            };
            if bytes.is_empty() {
                continue;
            }
            let Some(frame) = hg3::decode_frame(&bytes, frame)? else {
                continue;
            };
            let next = Image::from_frame(frame)?;
            if self.pixel_handle(handle)? != 0 {
                self.get_mut(handle)?.composite(&next);
            } else {
                let pixels = self.handles.allocate()?;
                self.data.insert(
                    pixels,
                    ImageData {
                        references: 1,
                        has_alpha: Some(next.info().reserved != 0),
                        image: next,
                    },
                );
                self.entries.get_mut(&handle).unwrap().data = pixels;
            }
        }
        Ok(self.pixel_handle(handle)? != 0)
    }

    pub(crate) fn insert(
        &mut self,
        owner: u32,
        image: Image,
        has_alpha: Option<bool>,
    ) -> Result<u32> {
        if self.entries.len() == read::MAX_ITEMS {
            return Err(Error::new(0, "图像对象数量超过上限"));
        }
        let handle = self.handles.allocate()?;
        let pixels = self.handles.allocate()?;
        self.data.insert(
            pixels,
            ImageData {
                references: 1,
                has_alpha,
                image,
            },
        );
        self.entries.insert(
            handle,
            ImageEntry {
                owner: Some(owner),
                references: 1,
                data: pixels,
            },
        );
        Ok(handle)
    }

    /// 为同一像素对象创建独立包装；没有任务登记，使用者须显式释放。
    /// 源包装释放后新句柄继续有效，两个包装的像素修改互相可见。
    pub fn share(&mut self, handle: u32) -> Result<u32> {
        let pixels = self.pixel_handle(handle)?;
        if pixels == 0 {
            self.empty_wrapper(None)
        } else {
            self.share_pixels_for(None, pixels)
        }
    }

    /// 查询包装当前绑定的像素句柄，不增加引用；空包装返回零。
    /// 非零像素句柄在至少一个包装持有时有效。
    pub fn pixel_handle(&self, handle: u32) -> Result<u32> {
        Ok(self
            .entries
            .get(&handle)
            .ok_or_else(|| Error::new(0, "图像对象句柄无效"))?
            .data)
    }

    pub(crate) fn pixel_image(&self, pixels: u32) -> Result<&Image> {
        self.data
            .get(&pixels)
            .map(|data| &data.image)
            .ok_or_else(|| Error::new(0, "图像像素句柄无效"))
    }

    pub(crate) fn share_pixels_for(&mut self, owner: Option<u32>, pixels: u32) -> Result<u32> {
        if self.entries.len() == read::MAX_ITEMS {
            return Err(Error::new(0, "图像对象数量超过上限"));
        }
        let references = self
            .data
            .get(&pixels)
            .ok_or_else(|| Error::new(0, "图像像素句柄无效"))?
            .references
            .checked_add(1)
            .ok_or_else(|| Error::new(0, "图像引用数量超过上限"))?;
        let shared = self.handles.allocate()?;
        self.data.get_mut(&pixels).unwrap().references = references;
        self.entries.insert(
            shared,
            ImageEntry {
                owner,
                references: 1,
                data: pixels,
            },
        );
        Ok(shared)
    }

    pub(crate) fn bind_pixels(&mut self, handle: u32, pixels: u32) -> Result<()> {
        let previous = self.entry_mut(handle)?.data;
        if pixels == 0 {
            self.entries.get_mut(&handle).unwrap().data = 0;
            release_data(&mut self.data, previous);
            return Ok(());
        }
        let data = self
            .data
            .get_mut(&pixels)
            .ok_or_else(|| Error::new(0, "图像像素句柄无效"))?;
        if previous != pixels {
            data.references = data
                .references
                .checked_add(1)
                .ok_or_else(|| Error::new(0, "图像引用数量超过上限"))?;
            self.entries.get_mut(&handle).unwrap().data = pixels;
            release_data(&mut self.data, previous);
        }
        Ok(())
    }

    /// 增加共享持有，使用者通过 `release_reference` 配对释放；像素修改对所有使用者可见。
    pub fn retain(&mut self, handle: u32) -> Result<()> {
        let entry = self.entry_mut(handle)?;
        entry.references = entry
            .references
            .checked_add(1)
            .ok_or_else(|| Error::new(0, "图像引用数量超过上限"))?;
        Ok(())
    }

    /// 解除所属任务登记并释放一个引用；零句柄跳过。
    pub fn release(&mut self, handle: u32) -> Result<()> {
        if handle == 0 {
            return Ok(());
        }
        self.entry_mut(handle)?.owner = None;
        self.release_reference(handle)
    }

    /// 普通 114：只解除调用任务的匹配登记，并始终释放一个引用。
    pub fn release_for(&mut self, owner: u32, handle: u32) -> Result<()> {
        if handle == 0 {
            return Ok(());
        }
        let entry = self.entry_mut(handle)?;
        if entry.owner == Some(owner) {
            entry.owner = None;
        }
        self.release_reference(handle)
    }

    /// 释放共享使用者的一个引用，保留所属任务登记；零句柄跳过。
    pub fn release_reference(&mut self, handle: u32) -> Result<()> {
        if handle == 0 {
            return Ok(());
        }
        let entry = self.entry_mut(handle)?;
        entry.references -= 1;
        if entry.references == 0 {
            let data = self.entries.remove(&handle).unwrap().data;
            release_data(&mut self.data, data);
        }
        Ok(())
    }

    pub fn get(&self, handle: u32) -> Result<&Image> {
        self.entries
            .get(&handle)
            .and_then(|entry| self.data.get(&entry.data))
            .map(|data| &data.image)
            .ok_or_else(|| Error::new(0, "图像对象句柄无效或尚未绑定像素"))
    }

    pub fn get_mut(&mut self, handle: u32) -> Result<&mut Image> {
        let data = self.entry_mut(handle)?.data;
        self.data
            .get_mut(&data)
            .map(|data| &mut data.image)
            .ok_or_else(|| Error::new(0, "图像对象尚未绑定像素"))
    }

    fn entry_mut(&mut self, handle: u32) -> Result<&mut ImageEntry> {
        self.entries
            .get_mut(&handle)
            .ok_or_else(|| Error::new(0, "图像对象句柄无效"))
    }

    pub fn blend(
        &mut self,
        target: u32,
        at: [i32; 2],
        source: u32,
        source_rect: Rect,
    ) -> Result<bool> {
        if source == 0 {
            return Ok(false);
        }
        let source_data = self
            .entries
            .get(&source)
            .ok_or_else(|| Error::new(0, "图像对象句柄无效"))?
            .data;
        let target_data = self.entry_mut(target)?.data;
        self.pixel_image(source_data)?;
        self.pixel_image(target_data)?;
        if source_data == target_data {
            return Ok(self.get_mut(target)?.blend_within(source_rect, at));
        }
        let source_image = self.data.remove(&source_data).unwrap();
        let result = self
            .get_mut(target)
            .map(|target| target.blend_from(&source_image.image, source_rect, at));
        self.data.insert(source_data, source_image);
        result
    }

    pub fn release_owner(&mut self, owner: u32) {
        let data = &mut self.data;
        self.entries.retain(|_, entry| {
            if entry.owner == Some(owner) {
                entry.owner = None;
                entry.references -= 1;
            }
            if entry.references == 0 {
                release_data(data, entry.data);
                false
            } else {
                true
            }
        });
    }

    pub(crate) fn draw_list_entry(
        &mut self,
        target: u32,
        source: u32,
        position: [f32; 2],
        base: [f32; 2],
        opacity: u8,
        clip: Rect,
    ) -> Result<()> {
        let source_data = self.entry_mut(source)?.data;
        let target_data = self.entry_mut(target)?.data;
        self.pixel_image(source_data)?;
        self.pixel_image(target_data)?;
        if source_data == target_data {
            return Err(Error::new(0, "图像列表自有图像不能同时用作绘制来源"));
        }
        let mut source_image = self.data.remove(&source_data).unwrap();
        self.data
            .get_mut(&target_data)
            .unwrap()
            .image
            .draw_list_from(
                &source_image.image,
                position,
                base,
                opacity,
                clip,
                &mut source_image.has_alpha,
            );
        self.data.insert(source_data, source_image);
        Ok(())
    }

    pub(crate) fn list_bounds(
        &self,
        source: u32,
        position: [f32; 2],
        base: [f32; 2],
    ) -> Result<Rect> {
        Ok(self.get(source)?.list_bounds(position, base))
    }
}

fn list_coordinate(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn release_data(data: &mut BTreeMap<u32, ImageData>, handle: u32) {
    if handle == 0 {
        return;
    }
    let image = data.get_mut(&handle).unwrap();
    image.references -= 1;
    if image.references == 0 {
        data.remove(&handle);
    }
}
