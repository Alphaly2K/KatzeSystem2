use super::{ImageRegion, Rect, Surface};
use crate::hg3::{DecodedFrame, StandardInfo};
use crate::kcs::ResourceServices;
use crate::resource::ResourceMode;
use crate::{Error, Result, hg3, read};
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
}

impl Image {
    pub fn from_frame(frame: DecodedFrame) -> Result<Self> {
        if frame.info.bit_depth as i16 != 32 {
            return Err(Error::new(0, "图像对象需要 32 位像素"));
        }
        Ok(Self {
            id: frame.id,
            info: frame.info,
            surface: Surface::from_bgra(frame.info.size, frame.pixels)?,
        })
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn info(&self) -> &StandardInfo {
        &self.info
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

/// 普通任务持有的图像句柄；所属任务释放时一起失效。
#[derive(Default)]
pub struct Images {
    entries: BTreeMap<u32, (u32, Image)>,
    next_handle: u32,
}

impl Images {
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
        if self.entries.len() == read::MAX_ITEMS {
            return Err(Error::new(0, "图像对象数量超过上限"));
        }
        let handle = self.next_handle.max(1);
        self.next_handle = handle
            .checked_add(1)
            .ok_or_else(|| Error::new(0, "图像句柄耗尽"))?;
        self.entries.insert(handle, (owner, image));
        Ok(handle)
    }

    pub fn get(&self, handle: u32) -> Result<&Image> {
        self.entries
            .get(&handle)
            .map(|(_, image)| image)
            .ok_or_else(|| Error::new(0, "图像对象句柄无效"))
    }

    pub fn get_mut(&mut self, handle: u32) -> Result<&mut Image> {
        self.entries
            .get_mut(&handle)
            .map(|(_, image)| image)
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
        if source == target {
            return Ok(self.get_mut(target)?.blend_within(source_rect, at));
        }
        let source_image = self
            .entries
            .remove(&source)
            .ok_or_else(|| Error::new(0, "图像对象句柄无效"))?;
        let result = self
            .get_mut(target)
            .map(|target| target.blend_from(&source_image.1, source_rect, at));
        self.entries.insert(source, source_image);
        result
    }

    pub fn release_owner(&mut self, owner: u32) {
        self.entries.retain(|_, (id, _)| *id != owner);
    }
}
