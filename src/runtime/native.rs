use super::{NativeInvocation, NativeTraceEvent, ObjectFactory, ObjectHost};
use crate::Result;
use crate::kcs::{
    Interpreter, NativeReturn, RuntimeHandler, call_native, canonical_native,
    runtime_native_handler,
};

mod storage;
mod tables;
mod task;

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn call_registered_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let Some(trace) = self.resources.native_trace.clone() else {
            return self.dispatch_native(index, arguments, vm);
        };
        let invocation = NativeInvocation {
            task_id: self.host.task_id,
            id: index,
            offset: vm.pc,
        };
        trace(NativeTraceEvent::Enter(invocation));
        let result = self.dispatch_native(index, arguments, vm);
        trace(NativeTraceEvent::Exit {
            invocation,
            failed: result.is_err(),
        });
        result
    }

    fn dispatch_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let index = canonical_native(index);
        self.resources.images.set_cache_limit(
            (self
                .host
                .platform_settings
                .as_ref()
                .map_or(32, |s| s.etc_imgcache) as usize)
                .saturating_mul(1 << 20),
        );
        match runtime_native_handler(index) {
            RuntimeHandler::FileOpen => self.open_file_native(arguments, vm),
            RuntimeHandler::Property => self.set_property_native(arguments, vm),
            RuntimeHandler::Configuration => self.save_configuration_native(arguments, vm),
            RuntimeHandler::DebugWindow => self.debug_window_native(index, arguments, vm),
            RuntimeHandler::Export => self.export_native(index, arguments, vm),
            RuntimeHandler::Bmb => self.bmb_native(index, arguments, vm),
            RuntimeHandler::SystemTask => self.system_task_native(index, arguments, vm),
            RuntimeHandler::PlatformJob => self.platform_job_native(index, arguments, vm),
            RuntimeHandler::Effect => self.effect_native(index, arguments, vm),
            RuntimeHandler::ImageExport => self.image_export_native(arguments, vm),
            RuntimeHandler::Emocon => self.emocon_native(index, arguments, vm),
            RuntimeHandler::Backlog => self.backlog_native(index, arguments, vm),
            RuntimeHandler::LoadProgram => self.ordinary_load_native(arguments, vm),
            RuntimeHandler::LoadParticle => self.load_particle_native(index, arguments, vm),
            RuntimeHandler::Movie => self.movie_native(index, arguments, vm),
            RuntimeHandler::Transition => self.transition_native(index, arguments, vm),
            RuntimeHandler::Table(handler) => self.table_native(handler, index, arguments, vm),
            RuntimeHandler::Cst => self.cst_native(index, arguments, vm),
            RuntimeHandler::Motion => self.motion_native(index, arguments, vm),
            RuntimeHandler::SharedRecord => self.shared_record(index, arguments, vm),
            RuntimeHandler::SharedNumbers => self.shared_numbers_native(index, arguments, vm),
            RuntimeHandler::SoundLoop => self.load_sound_loop_table(arguments, vm),
            RuntimeHandler::SoundControl => self.control_sound_native(index, arguments, vm),
            RuntimeHandler::SoundLoad => self.load_sound_native(index, arguments, vm),
            RuntimeHandler::Input => self.input_native(index, arguments, vm),
            RuntimeHandler::Message => self.message_native(index, arguments, vm),
            RuntimeHandler::TextWindow => self.text_window_native(index, arguments, vm),
            RuntimeHandler::Fes => self.fes_native(index, arguments, vm),
            RuntimeHandler::PointerPosition => self.pointer_position_native(index, arguments, vm),
            RuntimeHandler::Graphics => self.graphics_native(index, arguments, vm),
            RuntimeHandler::Task => self.task_native(index, arguments, vm),
            RuntimeHandler::MediaRecord => self.media_record_native(index, arguments, vm),
            RuntimeHandler::StateRecord => self.state_record_native(index, arguments, vm),
            RuntimeHandler::Resource => {
                self.resources.call(self.host.task_id, index, arguments, vm)
            }
            RuntimeHandler::ImageResource => self.image_resource_native(index, arguments, vm),
            RuntimeHandler::ImageAnimation => self.image_animation_native(index, arguments, vm),
            RuntimeHandler::ImagePack => self.image_pack_native(index, arguments, vm),
            RuntimeHandler::Core => call_native(
                &mut *self.numbers,
                &mut *self.host,
                None,
                index,
                arguments,
                vm,
            ),
        }
    }
}
