use super::{
    CachedResources, Host, Interpreter, NativeReturn, ResourceServices, TaskClock, strings,
};
use crate::resource::{ResourceMode, XmlDocument};
use crate::{Error, HandleAllocator, Result, read};
use std::collections::BTreeMap;

mod image_metadata;

pub(crate) fn load_sequence_file(
    arguments: &[u8],
    vm: &Interpreter,
    services: Option<&mut dyn ResourceServices>,
) -> Result<Option<(u32, Vec<u8>)>> {
    let handle = read::u32_at(arguments, 0)?;
    if arguments.len() != 8 && !(handle == 0 && arguments.len() == 4) {
        return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
    }
    if handle == 0 {
        return Ok(None);
    }
    let mut name = strings::format(vm, read::u32_at(arguments, 4)?, 0)?;
    name.truncate(
        name.iter()
            .position(|&byte| byte == 0)
            .unwrap_or(name.len()),
    );
    let services = services.ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
    let name = services.resolve(b"config", &name)?;
    Ok(services
        .read(&name, ResourceMode::LooseFirst)?
        .filter(|bytes| !bytes.is_empty())
        .map(|bytes| (handle, bytes)))
}

/// 为已有宿主提供图像原始数据、XML、三整数分组表、名称表、节表及句柄生命周期。
pub struct ResourceHost<'a, H, S> {
    pub inner: &'a mut H,
    pub services: CachedResources<&'a mut S>,
    state: ResourceState,
}

#[derive(Default)]
pub(crate) struct ResourceState {
    xml: BTreeMap<u32, XmlDocument>,
    images: BTreeMap<u32, ImageData>,
    owned_images: BTreeMap<u32, BTreeMap<u32, u32>>,
    triples: BTreeMap<u32, super::triples::TripleTable>,
    pub(super) gallery_tables: BTreeMap<u32, super::gallery_table::GalleryTable>,
    pub(super) name_tables: BTreeMap<u32, super::name_table::NameTable>,
    pub(super) section_tables: BTreeMap<u32, super::section_table::SectionTable>,
    pub(super) database_tables: BTreeMap<u32, super::database::table::Table>,
    pub(super) quiz_banks: BTreeMap<u32, super::quiz::QuizBank>,
    pub(super) quiz_options: BTreeMap<u32, super::quiz::options::OptionTables>,
    handles: HandleAllocator,
}

struct ImageData {
    bytes: std::rc::Rc<Vec<u8>>,
    references: u32,
}

impl<'a, H: Host, S: ResourceServices> ResourceHost<'a, H, S> {
    pub fn new(inner: &'a mut H, services: &'a mut S) -> Self {
        let handles = inner.handle_allocator().unwrap_or_default();
        Self {
            inner,
            services: CachedResources::new(services),
            state: ResourceState::with_handles(handles),
        }
    }

    /// 借用成功装载的图像资源原始字节，存活至最后一份引用释放。
    pub fn image_data(&self, handle: u32) -> Result<&[u8]> {
        self.state.image_data(handle)
    }
}

impl ResourceState {
    pub fn with_handles(handles: HandleAllocator) -> Self {
        Self {
            handles,
            ..Self::default()
        }
    }

    pub fn image_data(&self, handle: u32) -> Result<&[u8]> {
        self.images
            .get(&handle)
            .map(|image| image.bytes.as_slice())
            .ok_or_else(|| Error::new(0, "KCS 图像数据句柄无效"))
    }

    pub(crate) fn retain_image(&mut self, owner: u32, handle: u32) -> Result<std::rc::Rc<Vec<u8>>> {
        let image = self
            .images
            .get_mut(&handle)
            .ok_or_else(|| Error::new(0, "KCS 图像数据句柄无效"))?;
        image.references = image
            .references
            .checked_add(1)
            .ok_or_else(|| Error::new(0, "KCS 图像数据引用超过上限"))?;
        *self
            .owned_images
            .entry(owner)
            .or_default()
            .entry(handle)
            .or_default() += 1;
        Ok(image.bytes.clone())
    }

    pub fn load_image(
        &mut self,
        name: &[u8],
        services: &mut dyn ResourceServices,
        owner: Option<u32>,
        offset: usize,
    ) -> Result<u32> {
        let name = services.resolve(b"image", name)?;
        let Some(bytes) = services.read(&name, ResourceMode::LooseFirst)? else {
            return Ok(0);
        };
        if bytes.is_empty() {
            return Ok(0);
        }
        self.insert_image(bytes, owner, offset)
    }

    pub(crate) fn load_resolved_fes_image(
        &mut self,
        name: &[u8],
        services: &mut dyn ResourceServices,
        owner: u32,
        offset: usize,
    ) -> Result<u32> {
        let Some(bytes) = services.read(name, ResourceMode::LooseFirst)? else {
            return Ok(0);
        };
        self.insert_image(bytes, Some(owner), offset)
    }

    pub(crate) fn cache_image(
        &mut self,
        owner: u32,
        bytes: Vec<u8>,
        offset: usize,
    ) -> Result<(u32, std::rc::Rc<Vec<u8>>)> {
        let handle = self.insert_image(bytes, Some(owner), offset)?;
        Ok((handle, self.images[&handle].bytes.clone()))
    }

    fn insert_image(&mut self, bytes: Vec<u8>, owner: Option<u32>, offset: usize) -> Result<u32> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(offset, "KCS 图像资源超过读取上限"));
        }
        let handle = self.allocate_handle(offset)?;
        self.images.insert(
            handle,
            ImageData {
                bytes: std::rc::Rc::new(bytes),
                references: 1,
            },
        );
        if let Some(owner) = owner {
            self.owned_images
                .entry(owner)
                .or_default()
                .insert(handle, 1);
        }
        Ok(handle)
    }

    pub(crate) fn release_owned_image(&mut self, owner: u32, handle: u32) -> Result<()> {
        if handle == 0 {
            return Ok(());
        }
        let handles = self
            .owned_images
            .get_mut(&owner)
            .ok_or_else(|| Error::new(0, "KCS 图像数据所属任务无效"))?;
        let count = handles
            .get_mut(&handle)
            .ok_or_else(|| Error::new(0, "KCS 图像数据所属任务无效"))?;
        *count -= 1;
        if *count == 0 {
            handles.remove(&handle);
        }
        if handles.is_empty() {
            self.owned_images.remove(&owner);
        }
        self.release_image(handle)
    }

    pub fn release_image(&mut self, handle: u32) -> Result<()> {
        if handle != 0 {
            let image = self
                .images
                .get_mut(&handle)
                .ok_or_else(|| Error::new(0, "KCS 图像数据句柄无效"))?;
            image.references -= 1;
            if image.references == 0 {
                self.images.remove(&handle);
            }
        }
        Ok(())
    }

    pub fn release_owner(&mut self, owner: u32) {
        if let Some(handles) = self.owned_images.remove(&owner) {
            for (handle, count) in handles {
                if let Some(image) = self.images.get_mut(&handle) {
                    image.references = image.references.saturating_sub(count);
                    if image.references == 0 {
                        self.images.remove(&handle);
                    }
                }
            }
        }
    }

    pub(super) fn allocate_handle(&mut self, offset: usize) -> Result<u32> {
        if self.xml.len()
            + self.images.len()
            + self.triples.len()
            + self.gallery_tables.len()
            + self.name_tables.len()
            + self.section_tables.len()
            + self.database_tables.len()
            + self.quiz_banks.len()
            + self.quiz_options.len()
            >= read::MAX_ITEMS
        {
            return Err(Error::new(offset, "KCS 资源对象数量超过上限"));
        }
        self.handles.allocate_at(offset)
    }
}

impl<H: Host, S: ResourceServices> Host for ResourceHost<'_, H, S> {
    fn diagnostic_trace(&self) -> Option<&crate::host::DiagnosticTrace> {
        self.inner.diagnostic_trace()
    }

    fn register_file_source(&mut self, source: crate::host::storage::SharedSource) -> Result<u32> {
        self.inner.register_file_source(source)
    }
    fn language_selection(&self) -> [u32; 3] {
        self.inner.language_selection()
    }

    fn load_sequence_data(&mut self, handle: u32, bytes: Vec<u8>) -> Result<()> {
        self.inner.load_sequence_data(handle, bytes)
    }
    fn create_database(&mut self, database: crate::kcs::Database) -> Result<u32> {
        self.inner.create_database(database)
    }
    fn create_quest_table(&mut self, bytes: &[u8]) -> Result<u32> {
        self.inner.create_quest_table(bytes)
    }
    fn set_task_shared_numbers(&mut self, available: bool) {
        self.inner.set_task_shared_numbers(available);
    }

    fn load_kcs_program(&mut self, name: &[u8]) -> Result<Option<super::Program>> {
        self.inner.load_kcs_program(name)
    }

    fn update_particle(
        &mut self,
        task: &mut crate::kcs::ParticleTask,
        now: u32,
        budget: usize,
    ) -> Result<bool> {
        self.inner.update_particle(task, now, budget)
    }

    fn fes_services(&mut self) -> Option<&mut dyn crate::runtime::fes::Services> {
        self.inner.fes_services()
    }

    fn surface_size(&self) -> [u32; 2] {
        self.inner.surface_size()
    }

    fn parent_task_id(&self) -> Result<u32> {
        self.inner.parent_task_id()
    }

    fn root_index_contains(&self, id: u32) -> Result<bool> {
        self.inner.root_index_contains(id)
    }

    fn handle_allocator(&self) -> Option<HandleAllocator> {
        Some(self.state.handles.clone())
    }

    fn drawables(&mut self) -> Option<&mut crate::graphics::Drawables> {
        self.inner.drawables()
    }

    fn compositor(&mut self) -> Option<&mut crate::graphics::Compositor> {
        self.inner.compositor()
    }

    fn reserve_auxiliary_targets(&mut self, count: u32) -> Result<bool> {
        self.inner.reserve_auxiliary_targets(count)
    }

    fn image(&self, handle: u32) -> Result<&crate::graphics::Image> {
        self.inner.image(handle)
    }

    fn pixel_image(&self, pixels: u32) -> Result<&crate::graphics::Image> {
        self.inner.pixel_image(pixels)
    }

    fn image_data(&self, handle: u32) -> Result<&[u8]> {
        self.state.image_data(handle)
    }

    fn images_mut(&mut self) -> Option<&mut crate::graphics::Images> {
        self.inner.images_mut()
    }

    fn effect_speed(&self) -> u32 {
        self.inner.effect_speed()
    }

    fn unicode_text(&self) -> bool {
        self.inner.unicode_text()
    }

    fn sound_stereo(&self) -> u32 {
        self.inner.sound_stereo()
    }

    fn sound_config_enabled(&self) -> u32 {
        self.inner.sound_config_enabled()
    }

    fn jtalk_enabled(&self) -> u32 {
        self.inner.jtalk_enabled()
    }

    fn speech_synthesizer(&mut self) -> Option<&mut dyn crate::host::SpeechSynthesizer> {
        self.inner.speech_synthesizer()
    }

    fn sound_group_id(&self, name: &[u8]) -> Result<u32> {
        self.inner.sound_group_id(name)
    }

    fn system_time_ms(&self) -> u32 {
        self.inner.system_time_ms()
    }

    fn fonts(&mut self) -> Option<&mut dyn crate::host::FontServices> {
        self.inner.fonts()
    }

    fn message_defaults(&self) -> Result<&crate::message::MessageDefaults> {
        self.inner.message_defaults()
    }

    fn frame_rate(&self) -> u32 {
        self.inner.frame_rate()
    }

    fn message_time_scale(&self) -> Option<u32> {
        self.inner.message_time_scale()
    }

    fn message_plane_display(&mut self, id: u32) -> Result<Option<u32>> {
        self.inner.message_plane_display(id)
    }

    fn submit_message(
        &mut self,
        surface: &crate::graphics::Surface,
        rectangle: crate::graphics::Rect,
        at: [i32; 2],
    ) -> Result<bool> {
        self.inner.submit_message(surface, rectangle, at)
    }

    fn submit_message_pixels(
        &mut self,
        pixels: u32,
        rectangle: crate::graphics::Rect,
        at: [i32; 2],
    ) -> Result<bool> {
        self.inner.submit_message_pixels(pixels, rectangle, at)
    }

    fn update_image_list(
        &mut self,
        list: &mut crate::graphics::ImageList,
        submit: bool,
        force: bool,
        clip: Option<crate::graphics::Rect>,
    ) -> Result<bool> {
        self.inner.update_image_list(list, submit, force, clip)
    }

    fn update_image_list_animations(
        &mut self,
        list: &mut crate::graphics::ImageList,
        now: u32,
        budget: usize,
    ) -> Result<bool> {
        self.inner.update_image_list_animations(list, now, budget)
    }

    fn restart_image_list_animation(
        &mut self,
        list: &mut crate::graphics::ImageList,
        key: u32,
        budget: usize,
    ) -> Result<bool> {
        self.inner.restart_image_list_animation(list, key, budget)
    }

    fn create_task_child(&mut self, kind: u32, name: &[u8], context: &[u8]) -> Result<u32> {
        self.inner.create_task_child(kind, name, context)
    }

    fn canvas_layout(&self) -> Result<Option<crate::graphics::CanvasLayout>> {
        self.inner.canvas_layout()
    }

    fn resource_services(&mut self) -> Option<&mut dyn ResourceServices> {
        Some(&mut self.services)
    }

    fn quiz_debug_packs(&self, handle: u32) -> Result<Vec<Vec<u8>>> {
        self.state.quiz_debug_packs(handle)
    }

    fn export_services(&mut self) -> Option<&mut dyn crate::host::ExportServices> {
        self.inner.export_services()
    }

    fn movie_enabled(&self) -> bool {
        self.inner.movie_enabled()
    }

    fn load_plane_movie(
        &mut self,
        owner: u32,
        source: crate::host::storage::SharedSource,
        effect: bool,
    ) -> Result<[u32; 2]> {
        self.inner.load_plane_movie(owner, source, effect)
    }

    fn plane_movie_handle(&self, owner: u32, effect: bool) -> u32 {
        self.inner.plane_movie_handle(owner, effect)
    }

    fn release_plane_movies(&mut self, owner: u32) -> Result<()> {
        self.inner.release_plane_movies(owner)
    }

    fn set_task_clock(&mut self, clock: &TaskClock) {
        self.inner.set_task_clock(clock);
    }

    fn set_task_id(&mut self, id: u32) {
        self.inner.set_task_id(id);
    }

    fn set_task_local_numbers(&mut self, values: crate::expression::LocalNumbers) {
        self.inner.set_task_local_numbers(values);
    }
    fn set_task_named_values(&mut self, values: crate::expression::NamedValues) {
        self.inner.set_task_named_values(values);
    }

    fn set_task_context(&mut self, context: &[u8]) {
        self.inner.set_task_context(context);
    }

    fn take_task_effects(&mut self, queued_messages: usize) -> super::TaskEffects {
        self.inner.take_task_effects(queued_messages)
    }

    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        if index == 29 {
            let request = super::files::request(arguments, vm)?;
            if let Some(mode) = request.mode {
                let handle = match self.services.open(&request.name, mode)? {
                    Some(source) => self.inner.register_file_source(source)?,
                    None => 0,
                };
                return Ok(NativeReturn::Value(handle));
            }
        }
        if matches!(index, 434..=438) {
            return super::prefetch_call(index, arguments, vm, Some(&mut self.services));
        }
        if matches!(index, 683..=685) {
            return self
                .state
                .section_table_call(index, arguments, vm, Some(&mut self.services));
        }
        if matches!(index, 791..=794) {
            return self
                .state
                .database_table_call(index, arguments, vm, Some(&mut self.services));
        }
        if index == 795 {
            let handle = match super::load_quest_table(arguments, vm, Some(&mut self.services))? {
                Some(bytes) => self.inner.create_quest_table(&bytes)?,
                None => 0,
            };
            return Ok(NativeReturn::Value(handle));
        }
        if matches!(
            super::native::runtime_native_handler(index),
            super::native::RuntimeHandler::QuizOptions
        ) {
            let random = if matches!(index, 831 | 832) {
                self.inner.random()
            } else {
                None
            };
            return self.state.quiz_option_call(
                index,
                arguments,
                vm,
                Some(&mut self.services),
                random,
            );
        }
        if matches!(
            super::native::runtime_native_handler(index),
            super::native::RuntimeHandler::Quiz
        ) {
            let random = if index == 765 {
                self.inner.random()
            } else {
                None
            };
            return self
                .state
                .quiz_call(index, arguments, vm, Some(&mut self.services), random);
        }
        if matches!(index, 633..=640) {
            return self.state.name_table_call(
                index,
                arguments,
                vm,
                Some(&mut self.services),
                self.inner.unicode_text(),
            );
        }
        if index == 869 {
            let database = super::load_database(arguments, vm, Some(&mut self.services))?;
            return Ok(NativeReturn::Value(self.inner.create_database(database)?));
        }
        if index == 458 {
            if let Some((handle, bytes)) =
                load_sequence_file(arguments, vm, Some(&mut self.services))?
            {
                self.inner.load_sequence_data(handle, bytes)?;
            }
            return Ok(NativeReturn::Continue);
        }
        if !matches!(index, 284 | 322 | 365 | 366 | 368 | 375 | 405..=416 | 431 | 590..=597) {
            return self.inner.call(index, arguments, vm);
        }
        self.state
            .call(index, arguments, vm, Some(&mut self.services))
    }
}

impl ResourceState {
    pub fn call(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
        services: Option<&mut dyn ResourceServices>,
    ) -> Result<NativeReturn> {
        if matches!(index, 405..=416) {
            return self.gallery_table_call(index, arguments, vm, services);
        }
        let count = match index {
            284 | 375 | 431 => 2,
            322 | 365 | 366 | 590 | 591 | 594 | 595 => 1,
            368 | 592 | 593 => 3,
            596 | 597 => 5,
            _ => unreachable!(),
        };
        let null_triple_query = matches!(index, 596 | 597) && arguments == [0; 4];
        if arguments.len() != count * 4 && !null_triple_query {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let args: Vec<_> = arguments
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        if index == 322 {
            let mut name = strings::format(vm, args[0], 0)?;
            name.truncate(
                name.iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(name.len()),
            );
            if name.len() >= 260 {
                return Err(Error::new(vm.pc, "CST 文件名超过缓冲区上限"));
            }
            let services =
                services.ok_or_else(|| Error::new(vm.pc, "运行时未配置 CST 资源服务"))?;
            let resolved = services.resolve(b"scene", &name)?;
            let resolved = crate::text::cp932_replacing(
                resolved.split(|&byte| byte == 0).next().unwrap_or_default(),
            );
            let wide: Vec<_> = resolved.encode_utf16().collect();
            return Ok(NativeReturn::Value(u32::from(
                services
                    .open_utf16(&wide, ResourceMode::LooseFirst)?
                    .is_some(),
            )));
        }
        if index == 284 {
            let mut name = strings::format(vm, args[1], 0)?;
            name.truncate(name.iter().position(|&b| b == 0).unwrap_or(name.len()));
            if name.is_empty() {
                return Ok(NativeReturn::Value(0));
            }
            let services = services.ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
            let name = services.resolve(b"sound", &name)?;
            return Ok(NativeReturn::Value(u32::from(
                services.open(&name, ResourceMode::LooseFirst)?.is_some(),
            )));
        }
        if index == 594 {
            let mut name = strings::format(vm, args[0], 0)?;
            name.truncate(
                name.iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(name.len()),
            );
            let services = services.ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
            let Some(bytes) = services.read(&name, ResourceMode::LooseFirst)? else {
                return Ok(NativeReturn::Value(0));
            };
            let table = super::triples::TripleTable::parse(&bytes)?;
            let handle = self.allocate_handle(vm.pc)?;
            self.triples.insert(handle, table);
            return Ok(NativeReturn::Value(handle));
        }
        if index == 595 {
            if args[0] != 0 && self.triples.remove(&args[0]).is_none() {
                return Err(Error::new(vm.pc, "KCS 三整数分组表句柄无效"));
            }
            return Ok(NativeReturn::Continue);
        }
        if matches!(index, 596 | 597) {
            if args[0] == 0 {
                return Ok(NativeReturn::Value(0));
            }
            let table = self
                .triples
                .get(&args[0])
                .ok_or_else(|| Error::new(vm.pc, "KCS 三整数分组表句柄无效"))?;
            let name = strings::format(vm, args[1], 0)?;
            let Some(values) = table.query(&name, index == 597)? else {
                return Ok(NativeReturn::Value(0));
            };
            for (address, value) in args[2..].iter().zip(values) {
                vm.write(*address, &value.to_le_bytes())?;
            }
            return Ok(NativeReturn::Value(1));
        }
        if index == 365 {
            let name = strings::format(vm, args[0], 0)?;
            let services = services.ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
            let handle = self.load_image(&name, services, None, vm.pc)?;
            return Ok(NativeReturn::Value(handle));
        }
        if index == 366 {
            self.release_image(args[0])
                .map_err(|error| Error::new(vm.pc, error.message))?;
            return Ok(NativeReturn::Continue);
        }
        if index == 368 {
            if args[0] == 0 || vm.read(args[2], 1).is_err() {
                return Ok(NativeReturn::Value(0));
            }
            vm.write(args[2], &[0; 36])?;
            let info = image_metadata::query(self.image_data(args[0])?, args[1])?;
            return image_metadata::write(info, vm, args[2]);
        }
        if index == 431 {
            if args[0] == 0 || vm.read(args[1], 1).is_err() {
                return Ok(NativeReturn::Value(0));
            }
            vm.write(args[1], &[0; 36])?;
            let info = image_metadata::bounds(self.image_data(args[0])?)?;
            return image_metadata::write(info, vm, args[1]);
        }
        if index == 375 {
            if vm.read(args[1], 1).is_err() {
                return Ok(NativeReturn::Value(0));
            }
            let name = strings::format(vm, args[0], 0)?;
            let name = name.split(|&byte| byte == 0).next().unwrap_or_default();
            let services = services.ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
            let name = services.resolve(b"image", name)?;
            let Some(source) = services.open(&name, ResourceMode::LooseFirst)? else {
                return Ok(NativeReturn::Value(0));
            };
            let mut header = [0; 1024];
            let size = source.len()?.min(header.len() as u64) as usize;
            if source.read_exact_at(0, &mut header[..size]).is_err() {
                return Ok(NativeReturn::Value(0));
            }
            let info = image_metadata::query(&header[..size], 0).ok().flatten();
            return image_metadata::write(info, vm, args[1]);
        }
        if index == 590 {
            let name = strings::format(vm, args[0], 0)?;
            let services = services.ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
            let Some(bytes) = services.read(&name, ResourceMode::LooseFirst)? else {
                return Ok(NativeReturn::Value(0));
            };
            let Ok(xml) = XmlDocument::parse(&bytes) else {
                return Ok(NativeReturn::Value(0));
            };
            let handle = self.allocate_handle(vm.pc)?;
            self.xml.insert(handle, xml);
            return Ok(NativeReturn::Value(handle));
        }
        if index == 591 {
            if args[0] != 0 && self.xml.remove(&args[0]).is_none() {
                return Err(Error::new(vm.pc, "KCS XML 句柄无效"));
            }
            return Ok(NativeReturn::Continue);
        }
        if args[0] == 0 {
            return Ok(if index == 592 {
                NativeReturn::Value(args[1])
            } else {
                NativeReturn::Continue
            });
        }
        let xml = self
            .xml
            .get(&args[0])
            .ok_or_else(|| Error::new(vm.pc, "KCS XML 句柄无效"))?;
        let path = strings::format(vm, args[2], 0)?;
        if index == 592 {
            return Ok(NativeReturn::Value(xml.integer(&path, args[1])?));
        }
        // 原入口以目标自身作为缺失默认值，复制前需要一个完整的零结尾串。
        vm.read_string(args[1])?;
        if let Some(value) = xml.text(&path)? {
            let mut value = value.to_vec();
            value.push(0);
            vm.write(args[1], &value)?;
        }
        Ok(NativeReturn::Continue)
    }
}
