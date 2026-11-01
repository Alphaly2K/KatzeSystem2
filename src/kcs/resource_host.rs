use super::{Host, Interpreter, NativeReturn, TaskClock, strings};
use crate::resource::{ResourceMode, XmlDocument};
use crate::{Error, HandleAllocator, Result, read};
use std::collections::BTreeMap;

/// 普通脚本的同步资源读取服务；None 表示未找到或无法打开。
pub trait ResourceServices {
    /// 依类别补全名称；可直接委托 ArchiveConfig::resolve。
    fn resolve(&mut self, _category: &[u8], _name: &[u8]) -> Result<Vec<u8>> {
        Err(Error::new(0, "资源服务未提供类别名称补全"))
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>>;
}

/// 为已有宿主提供普通表的图像原始数据、XML 资源及句柄生命周期。
pub struct ResourceHost<'a, H, S> {
    pub inner: &'a mut H,
    pub services: &'a mut S,
    state: ResourceState,
}

#[derive(Default)]
pub(crate) struct ResourceState {
    xml: BTreeMap<u32, XmlDocument>,
    images: BTreeMap<u32, Vec<u8>>,
    handles: HandleAllocator,
}

impl<'a, H: Host, S: ResourceServices> ResourceHost<'a, H, S> {
    pub fn new(inner: &'a mut H, services: &'a mut S) -> Self {
        let handles = inner.handle_allocator().unwrap_or_default();
        Self {
            inner,
            services,
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
            .map(Vec::as_slice)
            .ok_or_else(|| Error::new(0, "KCS 图像数据句柄无效"))
    }

    fn allocate_handle(&mut self, offset: usize) -> Result<u32> {
        if self.xml.len() + self.images.len() >= read::MAX_ITEMS {
            return Err(Error::new(offset, "KCS 资源对象数量超过上限"));
        }
        self.handles.allocate_at(offset)
    }
}

impl<H: Host, S: ResourceServices> Host for ResourceHost<'_, H, S> {
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

    fn images_mut(&mut self) -> Option<&mut crate::graphics::Images> {
        self.inner.images_mut()
    }

    fn update_image_list(
        &mut self,
        list: &mut crate::graphics::ImageList,
        submit: bool,
        force: bool,
    ) -> Result<bool> {
        self.inner.update_image_list(list, submit, force)
    }

    fn create_task_child(&mut self, kind: u32, name: &[u8], context: &[u8]) -> Result<u32> {
        self.inner.create_task_child(kind, name, context)
    }

    fn canvas_layout(&self) -> Result<Option<crate::graphics::CanvasLayout>> {
        self.inner.canvas_layout()
    }

    fn resource_services(&mut self) -> Option<&mut dyn ResourceServices> {
        Some(self.services)
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
        if !matches!(index, 365 | 366 | 368 | 590..=593) {
            return self.inner.call(index, arguments, vm);
        }
        self.state.call(index, arguments, vm, Some(self.services))
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
            365 | 366 | 590 | 591 => 1,
            368 | 592 | 593 => 3,
            _ => unreachable!(),
        };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let args: Vec<_> = arguments
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        if index == 365 {
            let name = strings::format(vm, args[0], 0)?;
            let services = services.ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
            let name = services.resolve(b"image", &name)?;
            let Some(bytes) = services.read(&name, ResourceMode::LooseFirst)? else {
                return Ok(NativeReturn::Value(0));
            };
            if bytes.is_empty() {
                return Ok(NativeReturn::Value(0));
            }
            if bytes.len() > read::MAX_BYTES {
                return Err(Error::new(vm.pc, "KCS 图像资源超过读取上限"));
            }
            let handle = self.allocate_handle(vm.pc)?;
            self.images.insert(handle, bytes);
            return Ok(NativeReturn::Value(handle));
        }
        if index == 366 {
            if args[0] != 0 && self.images.remove(&args[0]).is_none() {
                return Err(Error::new(vm.pc, "KCS 图像数据句柄无效"));
            }
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
