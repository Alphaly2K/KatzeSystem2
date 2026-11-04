use super::{Host, Interpreter, NativeReturn, TaskClock, strings};
use crate::{Error, Result};

/// 普通脚本使用的同步平台服务。
pub trait SystemServices {
    /// 按系统 open 操作提交请求；平台返回的启动结果不参与 VM 返回值。
    fn open(
        &mut self,
        task_id: u32,
        target: &[u8],
        parameters: &[u8],
        directory: &[u8],
    ) -> Result<()>;
}

/// 为已有宿主接入普通表的系统打开请求。
pub struct SystemHost<'a, H, S> {
    pub inner: &'a mut H,
    pub services: &'a mut S,
    task_id: u32,
}

impl<'a, H: Host, S: SystemServices> SystemHost<'a, H, S> {
    pub fn new(inner: &'a mut H, services: &'a mut S) -> Self {
        inner.set_task_id(0);
        Self {
            inner,
            services,
            task_id: 0,
        }
    }
}

impl<H: Host, S: SystemServices> Host for SystemHost<'_, H, S> {
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

    fn handle_allocator(&self) -> Option<crate::HandleAllocator> {
        self.inner.handle_allocator()
    }

    fn drawables(&mut self) -> Option<&mut crate::graphics::Drawables> {
        self.inner.drawables()
    }

    fn image(&self, handle: u32) -> Result<&crate::graphics::Image> {
        self.inner.image(handle)
    }

    fn image_data(&self, handle: u32) -> Result<&[u8]> {
        self.inner.image_data(handle)
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

    fn resource_services(&mut self) -> Option<&mut dyn super::ResourceServices> {
        self.inner.resource_services()
    }

    fn set_task_clock(&mut self, clock: &TaskClock) {
        self.inner.set_task_clock(clock);
    }

    fn set_task_id(&mut self, id: u32) {
        self.task_id = id;
        self.inner.set_task_id(id);
    }

    fn set_task_context(&mut self, context: &[u8]) {
        self.inner.set_task_context(context);
    }

    fn take_task_effects(&mut self, queued_messages: usize) -> super::TaskEffects {
        self.inner.take_task_effects(queued_messages)
    }

    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        if index != 629 {
            return self.inner.call(index, arguments, vm);
        }
        if arguments.len() != 12 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut text = Vec::with_capacity(3);
        for bytes in arguments.chunks_exact(4) {
            text.push(strings::format(
                vm,
                u32::from_le_bytes(bytes.try_into().unwrap()),
                0,
            )?);
        }
        self.services
            .open(self.task_id, &text[0], &text[1], &text[2])?;
        Ok(NativeReturn::Continue)
    }
}
