use super::{CachedResources, Host, Interpreter, NativeReturn, ResourceCache, TaskClock, strings};
use crate::resource::{ResourceMode, XmlDocument};
use crate::{Error, HandleAllocator, Result, read};
use std::collections::BTreeMap;

/// 普通脚本的同步资源读取服务；None 表示未找到或无法打开。
pub trait ResourceServices {
    /// 依类别补全名称；可直接委托 ArchiveConfig::resolve。
    fn resolve(&mut self, _category: &[u8], _name: &[u8]) -> Result<Vec<u8>> {
        Err(Error::new(0, "资源服务未提供类别名称补全"))
    }

    /// 按启动配置的类别槽位补全名称。
    ///
    /// 声音装载携带 INT 槽位而非类别名；只提供名称服务的宿主沿用 ``sound``
    /// 类别，保持既有实现可用。
    fn resolve_slot(&mut self, _slot: u32, name: &[u8]) -> Result<Vec<u8>> {
        self.resolve(b"sound", name)
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>>;

    /// CST 与名称表原始文字的编码；默认沿用基线 CP932。
    fn script_encoding(&self) -> &'static encoding_rs::Encoding {
        encoding_rs::SHIFT_JIS
    }

    /// 宽字符资源读取；默认转为 CP932，无法无损表示时由宿主覆盖此方法。
    fn read_utf16(&mut self, name: &[u16], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        let name = &name[..name
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(name.len())];
        let name =
            String::from_utf16(name).map_err(|_| Error::new(0, "资源名称包含无效 UTF-16"))?;
        let (bytes, _, errors) = encoding_rs::SHIFT_JIS.encode(&name);
        if errors {
            return Err(Error::new(0, "资源服务需要宽字符名称读取能力"));
        }
        self.read(&bytes, mode)
    }

    /// 返回与普通读取共用的预读缓存；默认由 CachedResources 提供。
    fn resource_cache(&self) -> Option<ResourceCache> {
        None
    }
}

impl<S: ResourceServices + ?Sized> ResourceServices for &mut S {
    fn script_encoding(&self) -> &'static encoding_rs::Encoding {
        (**self).script_encoding()
    }
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        (**self).resolve(category, name)
    }
    fn resolve_slot(&mut self, slot: u32, name: &[u8]) -> Result<Vec<u8>> {
        (**self).resolve_slot(slot, name)
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        (**self).read(name, mode)
    }
    fn read_utf16(&mut self, name: &[u16], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        (**self).read_utf16(name, mode)
    }
    fn resource_cache(&self) -> Option<ResourceCache> {
        (**self).resource_cache()
    }
}

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

/// 为已有宿主提供图像原始数据、XML、三整数分组表、名称表及句柄生命周期。
pub struct ResourceHost<'a, H, S> {
    pub inner: &'a mut H,
    pub services: CachedResources<&'a mut S>,
    state: ResourceState,
}

#[derive(Default)]
pub(crate) struct ResourceState {
    xml: BTreeMap<u32, XmlDocument>,
    images: BTreeMap<u32, ImageData>,
    triples: BTreeMap<u32, super::triples::TripleTable>,
    pub(super) name_tables: BTreeMap<u32, super::name_table::NameTable>,
    handles: HandleAllocator,
}

struct ImageData {
    bytes: Vec<u8>,
    owner: Option<u32>,
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

    /// 借用成功装载的图像资源原始字节，存活至 366 释放或宿主销毁。
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

    pub(crate) fn load_fes_image(
        &mut self,
        name: &[u8],
        services: &mut dyn ResourceServices,
        owner: u32,
        offset: usize,
    ) -> Result<u32> {
        let name = services.resolve(b"image", name)?;
        let Some(bytes) = services.read(&name, ResourceMode::LooseFirst)? else {
            return Ok(0);
        };
        self.insert_image(bytes, Some(owner), offset)
    }

    fn insert_image(&mut self, bytes: Vec<u8>, owner: Option<u32>, offset: usize) -> Result<u32> {
        if bytes.len() > read::MAX_BYTES {
            return Err(Error::new(offset, "KCS 图像资源超过读取上限"));
        }
        let handle = self.allocate_handle(offset)?;
        self.images.insert(handle, ImageData { bytes, owner });
        Ok(handle)
    }

    pub fn release_image(&mut self, handle: u32) -> Result<()> {
        if handle != 0 && self.images.remove(&handle).is_none() {
            return Err(Error::new(0, "KCS 图像数据句柄无效"));
        }
        Ok(())
    }

    pub fn release_owner(&mut self, owner: u32) {
        self.images.retain(|_, image| image.owner != Some(owner));
    }

    pub(super) fn allocate_handle(&mut self, offset: usize) -> Result<u32> {
        if self.xml.len() + self.images.len() + self.triples.len() + self.name_tables.len()
            >= read::MAX_ITEMS
        {
            return Err(Error::new(offset, "KCS 资源对象数量超过上限"));
        }
        self.handles.allocate_at(offset)
    }
}

impl<H: Host, S: ResourceServices> Host for ResourceHost<'_, H, S> {
    fn language_selection(&self) -> [u32; 3] {
        self.inner.language_selection()
    }

    fn load_sequence_data(&mut self, handle: u32, bytes: Vec<u8>) -> Result<()> {
        self.inner.load_sequence_data(handle, bytes)
    }
    fn set_task_shared_numbers(&mut self, available: bool) {
        self.inner.set_task_shared_numbers(available);
    }

    fn load_kcs_program(&mut self, name: &[u8]) -> Result<Option<super::Program>> {
        self.inner.load_kcs_program(name)
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

    fn handle_allocator(&self) -> Option<HandleAllocator> {
        Some(self.state.handles.clone())
    }

    fn drawables(&mut self) -> Option<&mut crate::graphics::Drawables> {
        self.inner.drawables()
    }

    fn image(&self, handle: u32) -> Result<&crate::graphics::Image> {
        self.inner.image(handle)
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

    fn sound_group_id(&self, name: &[u8]) -> Result<u32> {
        self.inner.sound_group_id(name)
    }

    fn system_time_ms(&self) -> u32 {
        self.inner.system_time_ms()
    }

    fn fonts(&mut self) -> Option<&mut dyn crate::message::FontServices> {
        self.inner.fonts()
    }

    fn message_defaults(&self) -> Result<&crate::message::MessageDefaults> {
        self.inner.message_defaults()
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

    fn update_image_list(
        &mut self,
        list: &mut crate::graphics::ImageList,
        submit: bool,
        force: bool,
    ) -> Result<bool> {
        self.inner.update_image_list(list, submit, force)
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

    fn set_task_clock(&mut self, clock: &TaskClock) {
        self.inner.set_task_clock(clock);
    }

    fn set_task_id(&mut self, id: u32) {
        self.inner.set_task_id(id);
    }

    fn set_task_context(&mut self, context: &[u8]) {
        self.inner.set_task_context(context);
    }

    fn take_task_effects(&mut self, queued_messages: usize) -> super::TaskEffects {
        self.inner.take_task_effects(queued_messages)
    }

    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        if matches!(index, 434..=438) {
            return super::prefetch_call(index, arguments, vm, Some(&mut self.services));
        }
        if matches!(index, 633..=638) {
            return self.state.name_table_call(
                index,
                arguments,
                vm,
                Some(&mut self.services),
                self.inner.unicode_text(),
            );
        }
        if index == 458 {
            if let Some((handle, bytes)) =
                load_sequence_file(arguments, vm, Some(&mut self.services))?
            {
                self.inner.load_sequence_data(handle, bytes)?;
            }
            return Ok(NativeReturn::Continue);
        }
        if !matches!(index, 365 | 366 | 368 | 590..=597) {
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
        let count = match index {
            365 | 366 | 590 | 591 | 594 | 595 => 1,
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
            let bytes = self.image_data(args[0])?;
            if bytes.starts_with(b"HG-2") || bytes.starts_with(&[0xff, 0xd8]) {
                return Err(Error::new(vm.pc, "未支持的 HG-2／JPEG 元数据查询"));
            }
            if !bytes.starts_with(b"HG-3") {
                return Ok(NativeReturn::Value(0));
            }
            let metadata = crate::hg3::Metadata::parse(bytes)?;
            let Some(info) = metadata.frame(args[1] as i32).and_then(|frame| frame.info) else {
                return Ok(NativeReturn::Value(0));
            };
            let fields = [
                info.offset[0] as u32,
                info.offset[1] as u32,
                info.size[0],
                info.size[1],
                info.canvas_size[0],
                info.canvas_size[1],
                ((info.bit_depth as i16 as i32) / 8) as u32,
                info.base[0] as u32,
                info.base[1] as u32,
            ];
            let fields: Vec<_> = fields.into_iter().flat_map(u32::to_le_bytes).collect();
            vm.write(args[2], &fields)?;
            return Ok(NativeReturn::Value(1));
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
