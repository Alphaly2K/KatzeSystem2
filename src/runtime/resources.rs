use crate::graphics::{Drawables, DrawingContext, Images, Rect};
use crate::kcs::{Interpreter, NativeReturn, ResourceServices, ResourceState, format_string};
use crate::{Error, Result};

#[derive(Default)]
pub(super) struct Resources {
    pub services: Option<Box<dyn ResourceServices>>,
    pub fonts: Option<Box<dyn crate::message::FontServices>>,
    pub images: Images,
    pub drawing: DrawingContext,
    pub drawables: Drawables,
    pub frame_view: Option<(
        crate::graphics::Projection,
        [f32; 16],
        crate::graphics::TextureFilter,
    )>,
    pub capture_rectangle: Option<Rect>,
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
}

impl Resources {
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
    ) -> Result<bool> {
        let regions = list.update(&mut self.images)?;
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
            copied |= self.drawing.copy_image(
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
        if matches!(index, 365 | 366 | 368 | 590..=597) {
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
            115 => {
                length == 40
                    || ((24..40).contains(&length)
                        && length.is_multiple_of(4)
                        && arguments[20..24] == [0; 4])
            }
            117 => length == 24 || (length == 20 && arguments.starts_with(&[0; 4])),
            _ => unreachable!(),
        };
        if !valid {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut args = [0; 10];
        for (arg, bytes) in args.iter_mut().zip(arguments.chunks_exact(4)) {
            *arg = u32::from_le_bytes(bytes.try_into().unwrap());
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
                self.drawing
                    .copy_image(self.images.get(args[5])?, source_rect, at);
            } else {
                self.images.blend(args[0], at, args[5], source_rect)?;
            }
            return Ok(NativeReturn::Continue);
        }
        if args[0] == 0 {
            self.drawing.clear_rect(Rect::from_xywh(
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
