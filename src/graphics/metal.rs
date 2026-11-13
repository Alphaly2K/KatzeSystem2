//! macOS Metal 最终帧绘制。画布仍由运行时维护，设备仅上传变更面块。

use super::{FrameTriangle, Rect, RenderTarget, Surface, TextureFilter, TextureSource};
use crate::{Error, Result};
use ::metal::*;
use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    position: [f32; 4],
    color: [f32; 4],
    uv: [f32; 2],
    depth: f32,
    padding: f32,
}

struct Tile {
    texture: Texture,
    source: usize,
}

struct ImageTexture {
    texture: Texture,
    // 保留来源分配，避免后续图像复用同一地址时误命中缓存。
    _source: Arc<Surface>,
}

/// 一个窗口／画布使用一个实例。呈现会清空最终帧，并消费画布的 dirty 标记。
/// 更换画布时须调用 invalidate_canvas；宿主应为每帧提供 Objective-C autorelease pool。
pub struct Renderer {
    device: Device,
    queue: CommandQueue,
    pipelines: Vec<RenderPipelineState>,
    layer: MetalLayer,
    samplers: [SamplerState; 2],
    white: Texture,
    tiles: Vec<Option<Tile>>,
    images: BTreeMap<usize, ImageTexture>,
    vertices: Vec<Vertex>,
    buffer: Option<Buffer>,
    previous: Option<CommandBuffer>,
    passes: Vec<Texture>,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        let device =
            Device::system_default().ok_or_else(|| Error::new(0, "没有可用的 Metal 设备"))?;
        let library = device
            .new_library_with_source(include_str!("frame.metal"), &CompileOptions::new())
            .map_err(|error| Error::new(0, format!("Metal 着色器：{error}")))?;
        let descriptor = RenderPipelineDescriptor::new();
        let vertex = library
            .get_function("frame_vertex", None)
            .map_err(|e| Error::new(0, e))?;
        let fragment = library
            .get_function("frame_fragment", None)
            .map_err(|e| Error::new(0, e))?;
        descriptor.set_vertex_function(Some(&vertex));
        descriptor.set_fragment_function(Some(&fragment));
        let color = descriptor.color_attachments().object_at(0).unwrap();
        color.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        color.set_blending_enabled(true);
        let mut pipelines = Vec::with_capacity(6);
        for (source, destination) in [
            (
                MTLBlendFactor::SourceAlpha,
                MTLBlendFactor::OneMinusSourceAlpha,
            ),
            (MTLBlendFactor::SourceAlpha, MTLBlendFactor::One),
            (MTLBlendFactor::One, MTLBlendFactor::OneMinusSourceAlpha),
            (MTLBlendFactor::Zero, MTLBlendFactor::SourceColor),
            (
                MTLBlendFactor::OneMinusDestinationColor,
                MTLBlendFactor::Zero,
            ),
            (MTLBlendFactor::DestinationColor, MTLBlendFactor::One),
        ] {
            color.set_source_rgb_blend_factor(source);
            color.set_source_alpha_blend_factor(alpha_factor(source));
            color.set_destination_rgb_blend_factor(destination);
            color.set_destination_alpha_blend_factor(alpha_factor(destination));
            pipelines.push(
                device
                    .new_render_pipeline_state(&descriptor)
                    .map_err(|e| Error::new(0, e))?,
            );
        }
        let samplers = [
            MTLSamplerMinMagFilter::Nearest,
            MTLSamplerMinMagFilter::Linear,
        ]
        .map(|filter| {
            let descriptor = SamplerDescriptor::new();
            descriptor.set_min_filter(filter);
            descriptor.set_mag_filter(filter);
            descriptor.set_address_mode_s(MTLSamplerAddressMode::MirrorRepeat);
            descriptor.set_address_mode_t(MTLSamplerAddressMode::MirrorRepeat);
            device.new_sampler(&descriptor)
        });
        let layer = MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        layer.set_display_sync_enabled(true);
        let white = texture(&device, [1, 1], MTLTextureUsage::ShaderRead);
        white.replace_region(region([1, 1]), 0, [255u8; 4].as_ptr().cast(), 4);
        let queue = device.new_command_queue();
        Ok(Self {
            device,
            queue,
            pipelines,
            layer,
            samplers,
            white,
            tiles: Vec::new(),
            images: BTreeMap::new(),
            vertices: Vec::new(),
            buffer: None,
            previous: None,
            passes: Vec::new(),
        })
    }

    pub fn device_name(&self) -> &str {
        self.device.name()
    }

    /// 由窗口宿主将此 CAMetalLayer 附着到原生视图。
    pub fn layer(&self) -> &MetalLayerRef {
        &self.layer
    }

    pub fn invalidate_canvas(&mut self) {
        self.tiles.clear();
    }

    /// 等待上一帧结束，确保下一次上传不会改写 GPU 仍在使用的资源。
    pub fn wait(&mut self) -> Result<()> {
        if let Some(command) = self.previous.take() {
            command.wait_until_completed();
            if command.status() == MTLCommandBufferStatus::Error {
                return Err(Error::new(0, "Metal 帧执行失败"));
            }
        }
        Ok(())
    }

    /// 直接绘制至窗口交换链；窗口不可呈现时返回 false，保留画布更新标记。
    pub fn present(
        &mut self,
        triangles: &[FrameTriangle],
        canvas: &mut RenderTarget,
        size: [u32; 2],
        filter: TextureFilter,
    ) -> Result<bool> {
        self.present_passes(triangles, &[], canvas, size, filter)
    }

    fn present_passes(
        &mut self,
        triangles: &[FrameTriangle],
        passes: &[super::DrawPass],
        canvas: &mut RenderTarget,
        size: [u32; 2],
        filter: TextureFilter,
    ) -> Result<bool> {
        validate_size(size)?;
        let mut drawable_size = self.layer.drawable_size();
        drawable_size.width = f64::from(size[0]);
        drawable_size.height = f64::from(size[1]);
        self.layer.set_drawable_size(drawable_size);
        // Drawable 必须独立持有，后面的资源更新需要借用整个 renderer。
        let Some(drawable) = self.layer.next_drawable().map(ToOwned::to_owned) else {
            return Ok(false);
        };
        let combined = self.prepare_passes(triangles, passes, canvas, size)?;
        let command = self.queue.new_command_buffer();
        let offset = self.encode_passes(command, passes, &combined, canvas, filter);
        self.encode(
            command,
            drawable.texture(),
            triangles,
            canvas,
            filter,
            (offset, 0),
        );
        command.present_drawable(&drawable);
        command.commit();
        self.previous = Some(command.to_owned());
        Ok(true)
    }

    /// 离屏绘制与 BGRA 读回，供画面捕获使用。
    pub fn render_offscreen(
        &mut self,
        triangles: &[FrameTriangle],
        canvas: &mut RenderTarget,
        size: [u32; 2],
        filter: TextureFilter,
    ) -> Result<Surface> {
        self.render_offscreen_passes(triangles, &[], canvas, size, filter)
    }

    fn render_offscreen_passes(
        &mut self,
        triangles: &[FrameTriangle],
        passes: &[super::DrawPass],
        canvas: &mut RenderTarget,
        size: [u32; 2],
        filter: TextureFilter,
    ) -> Result<Surface> {
        validate_size(size)?;
        let mut surface = Surface::new(size)?;
        let combined = self.prepare_passes(triangles, passes, canvas, size)?;
        let target = texture(&self.device, size, MTLTextureUsage::RenderTarget);
        let command = self.queue.new_command_buffer();
        let offset = self.encode_passes(command, passes, &combined, canvas, filter);
        self.encode(command, &target, triangles, canvas, filter, (offset, 0));
        if target.storage_mode() == MTLStorageMode::Managed {
            let blit = command.new_blit_command_encoder();
            blit.synchronize_resource(&target);
            blit.end_encoding();
        }
        command.commit();
        self.previous = Some(command.to_owned());
        self.wait()?;
        target.get_bytes(
            surface.pixels_mut().as_mut_ptr().cast(),
            u64::from(size[0]) * 4,
            region(size),
            0,
        );
        Ok(surface)
    }

    fn prepare_passes<'a>(
        &mut self,
        triangles: &'a [FrameTriangle],
        passes: &[super::DrawPass],
        canvas: &mut RenderTarget,
        size: [u32; 2],
    ) -> Result<Cow<'a, [FrameTriangle]>> {
        for (index, pass) in passes.iter().enumerate() {
            super::frame::validate_sources(&pass.commands, index)?;
        }
        super::frame::validate_sources(triangles, passes.len())?;
        self.wait()?;
        if passes.is_empty() {
            self.passes.clear();
            self.prepare(triangles, canvas, &[(triangles.len(), size)])?;
            return Ok(Cow::Borrowed(triangles));
        }
        for (index, pass) in passes.iter().enumerate() {
            validate_size(pass.size)?;
            if self.passes.get(index).is_none_or(|target| {
                target.width() != u64::from(pass.size[0])
                    || target.height() != u64::from(pass.size[1])
            }) {
                let target = texture(
                    &self.device,
                    pass.size,
                    MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead,
                );
                if index < self.passes.len() {
                    self.passes[index] = target;
                } else {
                    self.passes.push(target);
                }
            }
        }
        self.passes.truncate(passes.len());
        let mut combined = Vec::new();
        let mut sizes = Vec::new();
        for pass in passes {
            combined.extend_from_slice(&pass.commands);
            sizes.push((combined.len(), pass.size));
        }
        combined.extend_from_slice(triangles);
        sizes.push((combined.len(), size));
        self.prepare(&combined, canvas, &sizes)?;
        Ok(Cow::Owned(combined))
    }

    fn encode_passes(
        &self,
        command: &CommandBufferRef,
        passes: &[super::DrawPass],
        combined: &[FrameTriangle],
        canvas: &RenderTarget,
        filter: TextureFilter,
    ) -> usize {
        let mut offset = 0;
        for (index, pass) in passes.iter().enumerate() {
            let end = offset + pass.commands.len();
            self.encode(
                command,
                &self.passes[index],
                &combined[offset..end],
                canvas,
                filter,
                (offset, pass.clear),
            );
            offset = end;
        }
        offset
    }

    fn prepare(
        &mut self,
        triangles: &[FrameTriangle],
        canvas: &mut RenderTarget,
        sizes: &[(usize, [u32; 2])],
    ) -> Result<()> {
        self.wait()?;
        self.vertices.clear();
        let mut group = 0;
        for (triangle_index, triangle) in triangles.iter().enumerate() {
            while triangle_index >= sizes[group].0 {
                group += 1;
            }
            let size = sizes[group].1;
            let textured = triangle.texture.as_ref().is_some_and(|t| {
                matches!(t.source, TextureSource::Pass(_)) || t.source.surface(canvas).is_some()
            });
            let interpolate = !super::raster::triangle_culled(triangle.vertices)
                && (textured
                    || triangle
                        .vertices
                        .iter()
                        .any(|v| v.color != triangle.vertices[0].color));
            if interpolate
                && triangle
                    .vertices
                    .iter()
                    .any(|v| !v.rhw.is_finite() || v.rhw <= 0.)
            {
                return Err(Error::new(0, "设备插值需要有限的正 RHW"));
            }
            for (index, vertex) in triangle.vertices.iter().enumerate() {
                let w = if interpolate { 1. / vertex.rhw } else { 1. };
                let xy = [vertex.position[0], vertex.position[1]];
                self.vertices.push(Vertex {
                    // D3D 的整数像素中心对应 Metal 的半整数中心。
                    position: [
                        ((xy[0] + 0.5) * 2. / size[0] as f32 - 1.) * w,
                        (1. - (xy[1] + 0.5) * 2. / size[1] as f32) * w,
                        0.,
                        w,
                    ],
                    color: [
                        (vertex.color >> 16 & 255) as f32 / 255.,
                        (vertex.color >> 8 & 255) as f32 / 255.,
                        (vertex.color & 255) as f32 / 255.,
                        (vertex.color >> 24) as f32 / 255.,
                    ],
                    uv: triangle
                        .texture
                        .as_ref()
                        .filter(|_| textured)
                        .map_or([0.; 2], |t| t.uv[index]),
                    depth: if vertex.position[2].is_finite() {
                        vertex.position[2]
                    } else {
                        0.
                    },
                    padding: 0.,
                });
            }
        }
        let bytes = std::mem::size_of_val(self.vertices.as_slice()) as u64;
        if bytes > 0 {
            if self
                .buffer
                .as_ref()
                .is_none_or(|buffer| buffer.length() < bytes)
            {
                self.buffer = Some(self.device.new_buffer(
                    bytes.next_power_of_two(),
                    MTLResourceOptions::StorageModeShared,
                ));
            }
            // Vertex 是无指针的 repr(C) 记录；wait 已排除上一帧的并发读取。
            unsafe {
                std::ptr::copy_nonoverlapping(
                    self.vertices.as_ptr().cast::<u8>(),
                    self.buffer.as_ref().unwrap().contents().cast(),
                    bytes as usize,
                );
            }
        }
        self.tiles.resize_with(canvas.tiles().len(), || None);
        for (index, source) in canvas.tiles().iter().enumerate() {
            let Some(source) = source else {
                self.tiles[index] = None;
                continue;
            };
            if source.size().contains(&0) {
                self.tiles[index] = None;
                continue;
            }
            validate_size(source.size())?;
            let pointer = source.pixels().as_ptr() as usize;
            let replace = self.tiles[index].as_ref().is_none_or(|tile| {
                tile.source != pointer
                    || tile.texture.width() != u64::from(source.size()[0])
                    || tile.texture.height() != u64::from(source.size()[1])
            });
            if replace {
                self.tiles[index] = Some(Tile {
                    texture: texture(&self.device, source.size(), MTLTextureUsage::ShaderRead),
                    source: pointer,
                });
            }
            if replace || canvas.dirty_tiles()[index] {
                self.tiles[index].as_ref().unwrap().texture.replace_region(
                    region(source.size()),
                    0,
                    source.pixels().as_ptr().cast(),
                    source.stride() as u64,
                );
            }
        }
        let mut images = BTreeMap::new();
        for triangle in triangles {
            if super::raster::triangle_culled(triangle.vertices) {
                continue;
            }
            let Some(super::FrameTexture {
                source: TextureSource::Image(source),
                ..
            }) = &triangle.texture
            else {
                continue;
            };
            if source.size().contains(&0) {
                continue;
            }
            let pointer = Arc::as_ptr(source) as usize;
            if images.contains_key(&pointer) {
                continue;
            }
            let image = if let Some(image) = self.images.remove(&pointer) {
                image
            } else {
                validate_size(source.size())?;
                let texture = texture(&self.device, source.size(), MTLTextureUsage::ShaderRead);
                texture.replace_region(
                    region(source.size()),
                    0,
                    source.pixels().as_ptr().cast(),
                    source.stride() as u64,
                );
                ImageTexture {
                    texture,
                    _source: source.clone(),
                }
            };
            images.insert(pointer, image);
        }
        self.images = images;
        canvas.clear_dirty();
        Ok(())
    }

    fn encode(
        &self,
        command: &CommandBufferRef,
        target: &TextureRef,
        triangles: &[FrameTriangle],
        canvas: &RenderTarget,
        filter: TextureFilter,
        settings: (usize, u32),
    ) {
        let (offset, clear) = settings;
        let descriptor = RenderPassDescriptor::new();
        let color = descriptor.color_attachments().object_at(0).unwrap();
        color.set_texture(Some(target));
        color.set_load_action(MTLLoadAction::Clear);
        color.set_store_action(MTLStoreAction::Store);
        color.set_clear_color(MTLClearColor::new(
            f64::from(clear >> 16 & 255) / 255.,
            f64::from(clear >> 8 & 255) / 255.,
            f64::from(clear & 255) / 255.,
            f64::from(clear >> 24) / 255.,
        ));
        let encoder = command.new_render_command_encoder(descriptor);
        encoder.set_render_pipeline_state(&self.pipelines[0]);
        let mut blend_mode = 0;
        encoder.set_cull_mode(MTLCullMode::None);
        encoder.set_vertex_buffer(0, self.buffer.as_deref(), 0);
        encoder.set_fragment_sampler_state(
            0,
            Some(&self.samplers[usize::from(filter == TextureFilter::Linear)]),
        );
        let bounds = Rect::from_xywh(0, 0, target.width() as i32, target.height() as i32);
        for (index, triangle) in triangles.iter().enumerate() {
            if super::raster::triangle_culled(triangle.vertices) {
                continue;
            }
            let source = triangle
                .texture
                .as_ref()
                .and_then(|t| t.source.surface(canvas));
            let Some(clip) = triangle.clip.unwrap_or(bounds).intersect(bounds) else {
                continue;
            };
            if triangle
                .vertices
                .iter()
                .any(|v| v.position[..2].iter().any(|v| !v.is_finite()))
                || source.is_some_and(|surface| surface.size().contains(&0))
                || (source.is_some()
                    || triangle
                        .texture
                        .as_ref()
                        .is_some_and(|texture| matches!(texture.source, TextureSource::Pass(_))))
                    && triangle
                        .texture
                        .as_ref()
                        .is_some_and(|t| t.uv.iter().flatten().any(|v| !v.is_finite()))
            {
                continue;
            }
            encoder.set_scissor_rect(MTLScissorRect {
                x: clip.left as u64,
                y: clip.top as u64,
                width: (clip.right - clip.left) as u64,
                height: (clip.bottom - clip.top) as u64,
            });
            let texture = triangle
                .texture
                .as_ref()
                .and_then(|t| match &t.source {
                    TextureSource::Canvas(tile) => self
                        .tiles
                        .get(*tile as usize)
                        .and_then(Option::as_ref)
                        .map(|tile| &*tile.texture),
                    TextureSource::Pass(index) => {
                        self.passes.get(*index as usize).map(|texture| &**texture)
                    }
                    TextureSource::Image(image) => self
                        .images
                        .get(&(Arc::as_ptr(image) as usize))
                        .map(|image| &*image.texture),
                })
                .unwrap_or(&self.white);
            let selected = if triangle.blend_mode <= 5 {
                triangle.blend_mode
            } else {
                0
            };
            if selected != blend_mode {
                encoder.set_render_pipeline_state(&self.pipelines[selected as usize]);
                blend_mode = selected;
            }
            encoder.set_fragment_texture(0, Some(texture));
            encoder.draw_primitives(MTLPrimitiveType::Triangle, (offset + index) as u64 * 3, 3);
        }
        encoder.end_encoding();
    }
}

impl super::GraphicsBackend for Renderer {
    fn render(
        &mut self,
        frame: &super::DrawList,
        canvas: &mut RenderTarget,
        size: [u32; 2],
    ) -> Result<RenderTarget> {
        frame.validate_passes()?;
        let surface = ::metal::objc::rc::autoreleasepool(|| {
            self.render_offscreen_passes(&frame.commands, &frame.passes, canvas, size, frame.filter)
        })?;
        RenderTarget::from_tiles(size, [1, 1], vec![Some(surface)])
    }

    fn present(
        &mut self,
        frame: &super::DrawList,
        canvas: &mut RenderTarget,
        size: [u32; 2],
    ) -> Result<bool> {
        frame.validate_passes()?;
        ::metal::objc::rc::autoreleasepool(|| {
            self.present_passes(&frame.commands, &frame.passes, canvas, size, frame.filter)
        })
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let _ = self.wait();
    }
}

fn validate_size(size: [u32; 2]) -> Result<()> {
    if size.iter().any(|&axis| axis == 0 || axis > 16384) {
        return Err(Error::new(0, "Metal 目标尺寸超出范围"));
    }
    Ok(())
}

fn texture(device: &DeviceRef, size: [u32; 2], usage: MTLTextureUsage) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_width(u64::from(size[0]));
    descriptor.set_height(u64::from(size[1]));
    descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    descriptor.set_storage_mode(if device.has_unified_memory() {
        MTLStorageMode::Shared
    } else {
        MTLStorageMode::Managed
    });
    descriptor.set_usage(usage);
    device.new_texture(&descriptor)
}

fn region(size: [u32; 2]) -> MTLRegion {
    MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: u64::from(size[0]),
            height: u64::from(size[1]),
            depth: 1,
        },
    }
}

fn alpha_factor(factor: MTLBlendFactor) -> MTLBlendFactor {
    match factor {
        MTLBlendFactor::SourceColor => MTLBlendFactor::SourceAlpha,
        MTLBlendFactor::DestinationColor => MTLBlendFactor::DestinationAlpha,
        MTLBlendFactor::OneMinusDestinationColor => MTLBlendFactor::OneMinusDestinationAlpha,
        _ => factor,
    }
}
