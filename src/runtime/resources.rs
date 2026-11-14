use crate::graphics::{Compositor, GraphicsBackend, Images, Rect, RenderTarget, SoftwareBackend};
use crate::kcs::{Interpreter, NativeReturn, ResourceServices, ResourceState, format_string};
use crate::{Error, Result};

#[derive(Default)]
pub(super) struct Resources {
    pub services: Option<Box<dyn ResourceServices>>,
    pub fonts: Option<Box<dyn crate::message::FontServices>>,
    pub images: Images,
    pub compositor: Compositor,
    pub graphics_backend: Option<Box<dyn crate::graphics::GraphicsBackend>>,
    pub capture_rectangle: Option<Rect>,
    pub offscreen_frame: Option<crate::graphics::RenderTarget>,
    pub raw: ResourceState,
    pub input_callbacks: std::collections::BTreeMap<u32, crate::input::CommandState>,
    pub button_callbacks: std::collections::BTreeMap<u32, crate::input::ButtonObject>,
    pub graphics_responses: std::collections::BTreeMap<u32, u32>,
    pub input_device: Option<Box<dyn crate::input::InputDevice>>,
    pub input_coordinates: crate::input::CoordinateMap,
    pub input_pointer_control: bool,
    pub motion_host: crate::kcs::MotionHost,
    pub motion_controllers: std::collections::BTreeMap<u32, crate::kcs::MotionQueue>,
    pub motion_snapshots: std::collections::BTreeMap<u32, super::MotionSnapshot>,
    pub motion_programs: std::collections::BTreeMap<String, crate::kcs::Program>,
    pub backlogs: std::collections::BTreeMap<u32, crate::backlog::Backlog>,
    pub cst_contexts: std::collections::BTreeMap<u32, super::CstContext>,
    pub emocon: std::collections::BTreeMap<u32, super::emocon::EmoconState>,
    pub movie: Option<super::movie::Movie>,
}

impl Resources {
    pub fn compose_frame(&mut self, range: Option<[f32; 2]>) -> Result<crate::graphics::DrawList> {
        if range.is_none() {
            let size = self.movie.as_ref().map(|movie| movie.surface_size);
            if let Some(size) = size {
                let filter = self
                    .compositor
                    .frame_view
                    .as_ref()
                    .map_or(crate::graphics::TextureFilter::Linear, |view| view.2);
                if let Some(frame) = self.movie_frame(size, filter)? {
                    return Ok(frame);
                }
            }
        }
        self.compositor.compose(range)
    }

    pub fn present_frame(&mut self, size: [u32; 2]) -> Result<bool> {
        let frame = self.compose_frame(None)?;
        let canvas = self
            .compositor
            .drawing
            .target
            .as_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置画布纹理输入"))?;
        self.graphics_backend
            .as_deref_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置窗口图形后端"))?
            .present(&frame, canvas, size)
    }

    pub fn render_offscreen(&mut self, size: [u32; 2], range: [f32; 2]) -> Result<bool> {
        let Some(frame) = self.render_depth_frame(size, range)? else {
            return Ok(false);
        };
        self.offscreen_frame = Some(frame);
        Ok(true)
    }

    pub fn render_auxiliary(
        &mut self,
        index: u32,
        size: [u32; 2],
        range: [f32; 2],
    ) -> Result<bool> {
        if index as usize >= self.compositor.auxiliary_targets.len() {
            return Ok(false);
        }
        let Some(frame) = self.render_depth_frame(size, range)? else {
            return Ok(false);
        };
        if index == 0 && !self.compositor.shatters.entries.is_empty() {
            let image = frame.tiles()[0].as_ref().cloned().map(std::sync::Arc::new);
            for effect in self.compositor.shatters.entries.values_mut() {
                effect.source = image.clone();
            }
        }
        if index == 0 && !self.compositor.page_flips.is_empty() {
            if let Some(image) = frame.tiles()[0].as_ref() {
                let image = std::sync::Arc::new(image.clone());
                for effect in self.compositor.page_flips.values_mut() {
                    effect.set_source(image.clone());
                }
            }
        }
        self.compositor.auxiliary_targets[index as usize] = frame;
        Ok(true)
    }

    fn render_depth_frame(
        &mut self,
        size: [u32; 2],
        range: [f32; 2],
    ) -> Result<Option<RenderTarget>> {
        if self.compositor.drawing.mode != 0 || !self.compositor.drawing.enabled {
            return Ok(None);
        }
        if self.compositor.drawing.target.is_none() {
            return Ok(None);
        }
        let frame = self.compose_frame(Some(range))?;
        let canvas = self.compositor.drawing.target.as_mut().unwrap();
        if let Some(backend) = self.graphics_backend.as_deref_mut() {
            backend.render(&frame, canvas, size).map(Some)
        } else {
            SoftwareBackend.render(&frame, canvas, size).map(Some)
        }
    }

    pub fn capture_image(
        &mut self,
        size: [u32; 2],
        range: [f32; 2],
    ) -> Result<Option<crate::graphics::Image>> {
        if !self.render_offscreen(size, range)? {
            return Ok(None);
        }
        let frame = self.offscreen_frame.as_ref().unwrap();
        let rectangle = self
            .capture_rectangle
            .unwrap_or_else(|| Rect::from_xywh(0, 0, size[0] as i32, size[1] as i32));
        let extent = [
            rectangle.right.wrapping_sub(rectangle.left),
            rectangle.bottom.wrapping_sub(rectangle.top),
        ];
        if extent.iter().any(|&value| value < 0) {
            return Err(Error::new(0, "画面捕获区域尺寸不能为负数"));
        }
        let mut snapshot = crate::graphics::Surface::new(extent.map(|value| value as u32))?;
        snapshot.copy_from(frame.tiles()[0].as_ref().unwrap(), rectangle, [0; 2]);
        let mut image = crate::graphics::Image::new(snapshot.size())?;
        image.pixels_mut().copy_from_slice(snapshot.pixels());
        Ok(Some(image))
    }

    pub fn load_motion(&mut self, name: &[u8]) -> Result<Option<crate::kcs::Program>> {
        let name = &name[..name
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(name.len())];
        let key = crate::text::cp932(name)?.to_uppercase();
        if let Some(program) = self.motion_programs.get(&key) {
            return Ok(Some(program.clone()));
        }
        let services = self
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置 FES 运动资源服务"))?;
        let Some(bytes) = services.read(name, crate::resource::ResourceMode::LooseFirst)? else {
            return Ok(None);
        };
        let program = crate::kcs::Program::parse(&bytes)?;
        self.motion_programs.insert(key, program.clone());
        Ok(Some(program))
    }

    pub fn with_handles(handles: crate::HandleAllocator) -> Self {
        Self {
            images: Images::with_handles(handles.clone()),
            raw: ResourceState::with_handles(handles),
            ..Self::default()
        }
    }

    pub fn update_image_list(
        &mut self,
        list: &mut crate::graphics::ImageList,
        submit: bool,
        force: bool,
        clip: Option<crate::graphics::Rect>,
    ) -> Result<bool> {
        let regions = list.update_region(&mut self.images, clip)?;
        if !submit {
            return Ok(false);
        }
        let regions = if force {
            vec![list.bounds(&self.images)?]
        } else {
            regions
        };
        let image = self.images.get(list.image())?;
        let offset = list.offset();
        let mut copied = false;
        for region in regions {
            copied |= self.compositor.drawing.copy_image(
                image,
                region,
                [
                    offset[0].wrapping_add(region.left),
                    offset[1].wrapping_add(region.top),
                ],
            );
        }
        Ok(copied)
    }

    pub fn call(
        &mut self,
        owner: u32,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if matches!(index, 434..=438) {
            return crate::kcs::prefetch_call(
                index,
                arguments,
                vm,
                self.services
                    .as_mut()
                    .map(|services| services.as_mut() as _),
            );
        }
        if matches!(index, 284 | 365 | 366 | 368 | 590..=597) {
            return self.raw.call(
                index,
                arguments,
                vm,
                self.services
                    .as_mut()
                    .map(|services| services.as_mut() as _),
            );
        }
        let length = arguments.len();
        let valid = match index {
            111 | 114 => length == 4,
            112 => length == 8,
            362 => length == 12,
            116 => length == 40,
            115 => {
                length == 40
                    || ((24..40).contains(&length)
                        && length.is_multiple_of(4)
                        && arguments[20..24] == [0; 4])
            }
            117 => length == 24 || (length == 20 && arguments.starts_with(&[0; 4])),
            536 => length == 28 || (length == 4 && arguments == [0; 4]),
            _ => unreachable!(),
        };
        if !valid {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut args = [0; 10];
        for (arg, bytes) in args.iter_mut().zip(arguments.chunks_exact(4)) {
            *arg = u32::from_le_bytes(bytes.try_into().unwrap());
        }
        if index == 362 {
            vm.read(args[1], 4)?;
            vm.read(args[2], 4)?;
            vm.write(args[1], &[0; 4])?;
            vm.write(args[2], &[0; 4])?;
            if args[0] != 0 {
                let info = self.images.get(args[0])?.info();
                for (axis, address) in args[1..3].iter().enumerate() {
                    let extent = if info.canvas_size[axis] == 0 {
                        info.size[axis]
                    } else {
                        info.canvas_size[axis]
                    };
                    vm.write(*address, &extent.to_le_bytes())?;
                }
            }
            return Ok(NativeReturn::Continue);
        }
        if index == 536 {
            if args[0] == 0 {
                return Ok(NativeReturn::Value(0));
            }
            let Some(values) = self.images.get(args[0])?.attribute(args[1]) else {
                return Ok(NativeReturn::Value(0));
            };
            for &address in &args[2..7] {
                vm.read(address, 4)?;
            }
            for (&address, value) in args[2..7].iter().zip(values) {
                vm.write(address, &value.to_le_bytes())?;
            }
            return Ok(NativeReturn::Value(1));
        }
        if index == 112 {
            return Ok(NativeReturn::Value(
                self.images.create(owner, [args[0], args[1]])?,
            ));
        }
        if index == 114 {
            self.images.release_for(owner, args[0])?;
            return Ok(NativeReturn::Continue);
        }
        if index == 111 {
            let names = format_string(vm, args[0], 0)?;
            let names = &names[..names.iter().position(|&b| b == 0).unwrap_or(names.len())];
            if names.iter().all(|&b| b == b' ') {
                return Ok(NativeReturn::Value(0));
            }
            let services = self
                .services
                .as_mut()
                .ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
            let handle = self.images.load(owner, names, services.as_mut())?;
            return Ok(NativeReturn::Value(handle));
        }
        if index == 116 {
            if args[0] != 0 && args[5] != 0 {
                self.images.copy_scaled(
                    args[0],
                    args[5],
                    Rect::from_xywh(
                        args[6] as i32,
                        args[7] as i32,
                        args[8] as i32,
                        args[9] as i32,
                    ),
                    Rect::from_xywh(
                        args[1] as i32,
                        args[2] as i32,
                        args[3] as i32,
                        args[4] as i32,
                    ),
                )?;
            }
            return Ok(NativeReturn::Continue);
        }
        if index == 115 {
            if args[5] == 0 {
                return Ok(NativeReturn::Continue);
            }
            let source_rect = Rect::from_xywh(
                args[6] as i32,
                args[7] as i32,
                args[8] as i32,
                args[9] as i32,
            );
            let at = [args[1] as i32, args[2] as i32];
            if args[0] == 0 {
                self.compositor
                    .drawing
                    .copy_image(self.images.get(args[5])?, source_rect, at);
            } else {
                self.images.blend(args[0], at, args[5], source_rect)?;
            }
            return Ok(NativeReturn::Continue);
        }
        if args[0] == 0 {
            self.compositor.drawing.clear_rect(Rect::from_xywh(
                args[1] as i32,
                args[2] as i32,
                args[3] as i32,
                args[4] as i32,
            ));
            return Ok(NativeReturn::Continue);
        }
        self.images.get_mut(args[0])?.fill_rect_from_bottom(
            Rect::from_xywh(
                args[1] as i32,
                args[2] as i32,
                args[3] as i32,
                args[4] as i32,
            ),
            args[5],
        );
        Ok(NativeReturn::Continue)
    }
}
