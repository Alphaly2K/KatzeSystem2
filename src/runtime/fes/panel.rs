use super::{ObjectState, Objects};
use crate::{
    Error, Result,
    fes::PanelContent,
    graphics::{Image, Rect, Surface},
    hg3::Metadata,
    kcs::ResourceServices,
    message::{FontRequest, GlyphEffects, GlyphPaint, TextEncoding},
    resource::ResourceMode,
    runtime::{ObjectFactory, Store},
};
use std::collections::BTreeMap;

struct PanelFile {
    bytes: Vec<u8>,
    metadata: Metadata,
    frames: BTreeMap<i32, Option<Image>>,
}

impl PanelFile {
    fn frame(&mut self, id: i32) -> Result<Option<&Image>> {
        if !self.frames.contains_key(&id) {
            let image = crate::hg3::decode_frame(&self.bytes, id)?
                .map(Image::from_frame)
                .transpose()?;
            self.frames.insert(id, image);
        }
        Ok(self.frames[&id].as_ref())
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    fn panel_file<'a>(
        &mut self,
        objects: &Objects,
        files: &'a mut BTreeMap<Vec<u8>, Option<PanelFile>>,
        name: &[u8],
    ) -> Result<Option<&'a mut PanelFile>> {
        let name = name.split(|&b| b == 0).next().unwrap();
        if name.is_empty() {
            return Ok(None);
        }
        if !files.contains_key(name) {
            let services = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(0, "运行时未配置面板图像资源服务"))?;
            let mut services =
                super::preload::ImageServices::new(&objects.preloaded_images, services);
            let resolved = services.resolve(b"image", name)?;
            let file = services
                .read(&resolved, ResourceMode::LooseFirst)?
                .filter(|bytes| !bytes.is_empty())
                .map(|bytes| {
                    let metadata = Metadata::parse(&bytes)?;
                    Ok(PanelFile {
                        bytes,
                        metadata,
                        frames: BTreeMap::new(),
                    })
                })
                .transpose()?;
            files.insert(name.to_vec(), file);
        }
        Ok(files.get_mut(name).unwrap().as_mut())
    }

    pub(super) fn update_fes_panel(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
        image: Option<u32>,
    ) -> Result<()> {
        let list = self.check_fes_image_list(owner, objects)?;
        let wrapper = match image {
            Some(image) => image,
            None => self.fes_image_wrapper(owner, objects, handle)?,
        };
        let ObjectState::SavePanel(panel) = &objects.get(handle).unwrap().state else {
            unreachable!()
        };
        let prepared = panel.image.base.updates.needs_rebuild();
        if prepared {
            let name = panel.image.file.clone();
            let mut files = BTreeMap::new();
            let geometry = self
                .panel_file(objects, &mut files, &name)?
                .and_then(|file| file.metadata.frames.first())
                .and_then(|frame| frame.info)
                .map(|info| info.canvas_size);
            if let Some(size) = geometry {
                objects
                    .get_mut(handle)
                    .unwrap()
                    .state
                    .image_mut()
                    .unwrap()
                    .set_size(size.map(|v| v as i32));
            }
            let ObjectState::SavePanel(panel) = &objects.get(handle).unwrap().state else {
                unreachable!()
            };
            let size = panel.image.base.size;
            if size.into_iter().all(|axis| axis > 0) {
                let mut surface = Surface::new(size.map(|v| v as u32))?;
                for id in &panel.enabled_ids {
                    if *id == -1 {
                        break;
                    }
                    let Some(content) = panel.contents.get(id) else {
                        continue;
                    };
                    let position = self
                        .panel_file(objects, &mut files, &name)?
                        .and_then(|file| file.metadata.frame(*id))
                        .and_then(|frame| frame.info)
                        .map_or([0; 2], |info| info.offset);
                    match content.kind() {
                        0 => {
                            if let Some(part) = self
                                .panel_file(objects, &mut files, &name)?
                                .map(|file| file.frame(*id))
                                .transpose()?
                                .flatten()
                            {
                                blend(&mut surface, part, [0; 2]);
                            }
                        }
                        1 | 6 => {
                            if let Some(part) = content.image() {
                                blend(&mut surface, part, position);
                            }
                        }
                        2 | 5 => {
                            let reference =
                                content.record()[4..0x24].split(|&b| b == 0).next().unwrap();
                            let Some(source) = std::str::from_utf8(reference)
                                .ok()
                                .and_then(|name| objects.lookup(name, content.word(0x24) as i32))
                                .and_then(|id| objects.get(id))
                            else {
                                continue;
                            };
                            let source = match &source.state {
                                ObjectState::Image(state) => state,
                                ObjectState::Button(state) => &state.image,
                                _ => continue,
                            };
                            let mut frame = content.number();
                            let selector = content.word(0x44) as i32;
                            if selector != 0 {
                                let divisor = selector.wrapping_mul(10);
                                if divisor == 0 || (frame == i32::MIN && divisor == -1) {
                                    return Err(Error::new(0, "面板数位选择发生整数除法异常"));
                                }
                                frame = (frame % divisor)
                                    .checked_div(selector)
                                    .ok_or_else(|| Error::new(0, "面板数位选择发生整数除法异常"))?;
                            }
                            if content.word(0x48) == 0 && source.frame_table_enabled {
                                frame = source.frames.get(frame as usize).copied().unwrap_or(0);
                            }
                            if let Some(part) = self
                                .panel_file(objects, &mut files, &source.file)?
                                .map(|file| file.frame(frame))
                                .transpose()?
                                .flatten()
                            {
                                blend(&mut surface, part, position);
                            }
                        }
                        3 | 4 => self.panel_text(&mut surface, content, position)?,
                        7 => {
                            if let Some(part) = content.image() {
                                let mut part = part.clone();
                                part.resize_canvas([content.word(0xa4), content.word(0xa8)])?;
                                let crop = [0xac, 0xb0, 0xb4, 0xb8]
                                    .map(|offset| content.word(offset) as i32);
                                if crop.into_iter().any(|value| value < 0) {
                                    return Err(Error::new(0, "面板裁剪图像不支持负尺寸或起点"));
                                }
                                let size = part.info().size.map(|v| v as i32);
                                if crop[0] <= size[0] && crop[1] <= size[1] {
                                    let size = [
                                        crop[2].min(size[0] - crop[0]),
                                        crop[3].min(size[1] - crop[1]),
                                    ];
                                    let mut clipped = Surface::new(size.map(|v| v as u32))?;
                                    clipped.copy_from(
                                        part.surface(),
                                        Rect::from_xywh(crop[0], crop[1], size[0], size[1]),
                                        [0; 2],
                                    );
                                    blend_surface(&mut surface, &clipped, position);
                                }
                            }
                        }
                        _ => (),
                    }
                }
                self.resources.images.recreate(wrapper, surface.size())?;
                let image = self.resources.images.get_mut(wrapper)?;
                image.pixels_mut().copy_from_slice(surface.pixels());
                image.use_pixel_canvas();
            }
        }
        self.register_fes_image(owner, objects, handle, list, wrapper, prepared)?;
        self.update_fes_image_animation(owner, objects, handle, list)?;
        self.update_fes_image_priority(owner, objects, handle, list)
    }

    fn panel_text(
        &mut self,
        surface: &mut Surface,
        content: &PanelContent,
        mut position: [i32; 2],
    ) -> Result<()> {
        if content.text().is_empty() {
            return Ok(());
        }
        let font_size = content.word(0x34);
        if font_size > i32::MAX as u32 - 8 {
            return Err(Error::new(0, "面板字号超过字形服务支持范围"));
        }
        let name = content.record()[0x64..].split(|&b| b == 0).next().unwrap();
        let name = if name.is_empty() {
            self.host
                .message_defaults
                .as_ref()
                .map_or_else(Vec::new, |defaults| defaults.font_name.clone())
        } else {
            crate::text::cp932(name)?.encode_utf16().collect()
        };
        let request = FontRequest {
            encoding: TextEncoding::Unicode,
            name,
            weight: 400,
            italic: false,
            backend: 0,
            glyph_parameter: 1,
            measure_parameter: 0,
        };
        let fonts = self
            .resources
            .fonts
            .as_deref_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置面板字体服务"))?;
        fonts.validate(&request)?;
        let paint = GlyphPaint {
            color: crate::message::message_color_argb(content.word(0x38)),
            shadow_color: crate::message::message_color_argb(content.word(0x54)),
            font_size,
            double_coverage: false,
            effects: GlyphEffects {
                outline: content.word(0x4c) != 0,
                shadow: content.word(0x50) != 0,
                ..Default::default()
            },
        };
        let start = position[0];
        let size = font_size as i32;
        let limit = (content.word(0x3c) as i32)
            .wrapping_mul(size)
            .wrapping_sub(size / 2);
        let (mut width, mut line) = (0i32, 0i32);
        for character in crate::text::cp932(content.text())?.chars() {
            if width > limit {
                line = line.wrapping_add(1);
                if line >= content.word(0x5c) as i32 {
                    break;
                }
                position[0] = start;
                position[1] = position[1]
                    .wrapping_add(size)
                    .wrapping_add(content.word(0x60) as i32);
                width = 0;
            }
            let glyph = fonts.rasterize(&request, character, font_size)?;
            let mut image = Surface::new([font_size + 8; 2])?;
            glyph.paint(&mut image, &paint)?;
            blend_surface(surface, &image, position);
            let advance = glyph.advance();
            position[0] = position[0].wrapping_add(if advance == 0 { size / 2 } else { advance });
            width = width.wrapping_add(if advance == 0 { size } else { advance });
            if paint.effects.outline || paint.effects.shadow {
                position[0] = position[0].wrapping_sub(2);
                width = width.wrapping_sub(2);
            }
        }
        Ok(())
    }
}

fn blend(target: &mut Surface, source: &Image, position: [i32; 2]) {
    let at = std::array::from_fn(|i| position[i].wrapping_add(source.info().offset[i]));
    blend_surface(target, source.surface(), at);
}

fn blend_surface(target: &mut Surface, source: &Surface, at: [i32; 2]) {
    // 原面板以目标尺寸构造裁剪窗口，负起点裁剪后仍从来源首像素开始。
    let size = target.size().map(|v| v as i32);
    let Some(rect) = Rect::from_xywh(at[0], at[1], size[0], size[1]).intersect(target.bounds())
    else {
        return;
    };
    let width = (rect.right - rect.left).min(source.size()[0] as i32);
    let height = (rect.bottom - rect.top).min(source.size()[1] as i32);
    target.blend_from(
        source,
        Rect::from_xywh(0, 0, width, height),
        [rect.left, rect.top],
        255,
    );
}
