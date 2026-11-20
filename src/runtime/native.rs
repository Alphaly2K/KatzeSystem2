use super::{NativeInvocation, NativeTraceEvent, ObjectFactory, ObjectHost};
use crate::kcs::{
    Interpreter, NativeReturn, RuntimeHandler, call_native, canonical_native,
    runtime_native_handler,
};
use crate::{Error, Result, read};

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
            RuntimeHandler::FileOpen => crate::kcs::open_file_native(
                self.host,
                arguments,
                vm,
                self.resources
                    .services
                    .as_mut()
                    .map(|services| services.as_mut() as _),
            ),
            RuntimeHandler::Property => {
                if arguments.len() != 4 {
                    return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
                }
                let task = self
                    .running
                    .get_mut(&self.host.task_id)
                    .ok_or_else(|| Error::new(vm.pc, "执行中任务属性上下文不可用"))?;
                task.property = read::u32_at(arguments, 0)?;
                task.property_changed = true;
                Ok(NativeReturn::Continue)
            }
            RuntimeHandler::Configuration => {
                if !arguments.is_empty() {
                    return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
                }
                let keys = self
                    .resources
                    .input_device
                    .as_ref()
                    .and_then(|device| device.key_configuration());
                crate::kcs::save_configuration(self.host, keys, vm.pc)?;
                Ok(NativeReturn::Continue)
            }
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
            RuntimeHandler::NameTable => self.resources.raw.name_table_call(
                index,
                arguments,
                vm,
                self.resources
                    .services
                    .as_mut()
                    .map(|services| services.as_mut() as _),
                self.host.unicode_text != 0,
            ),
            RuntimeHandler::SectionTable => self.resources.raw.section_table_call(
                index,
                arguments,
                vm,
                self.resources
                    .services
                    .as_mut()
                    .map(|services| services.as_mut() as _),
            ),
            RuntimeHandler::DatabaseTable => self.resources.raw.database_table_call(
                index,
                arguments,
                vm,
                self.resources
                    .services
                    .as_mut()
                    .map(|services| services.as_mut() as _),
            ),
            RuntimeHandler::QuestTable => {
                let bytes = crate::kcs::load_quest_table(
                    arguments,
                    vm,
                    self.resources
                        .services
                        .as_mut()
                        .map(|services| services.as_mut() as _),
                )?;
                let handle = match bytes {
                    Some(bytes) => self.host.create_quest_table(&bytes)?,
                    None => 0,
                };
                Ok(NativeReturn::Value(handle))
            }
            RuntimeHandler::Quiz => self.resources.raw.quiz_call(
                index,
                arguments,
                vm,
                self.resources
                    .services
                    .as_mut()
                    .map(|services| services.as_mut() as _),
                Some(&mut self.host.random),
            ),
            RuntimeHandler::QuizOptions => self.resources.raw.quiz_option_call(
                index,
                arguments,
                vm,
                self.resources
                    .services
                    .as_mut()
                    .map(|services| services.as_mut() as _),
                Some(&mut self.host.random),
            ),
            RuntimeHandler::Cst => self.cst_native(index, arguments, vm),
            RuntimeHandler::Motion => self.motion_native(index, arguments, vm),
            RuntimeHandler::SequenceFile => {
                if let Some((handle, bytes)) = crate::kcs::load_sequence_file(
                    arguments,
                    vm,
                    self.resources
                        .services
                        .as_mut()
                        .map(|services| services.as_mut() as _),
                )? {
                    self.host.load_sequence_data(handle, bytes)?;
                }
                Ok(NativeReturn::Continue)
            }
            RuntimeHandler::DatabaseLoad => {
                let database = crate::kcs::load_database(
                    arguments,
                    vm,
                    self.resources
                        .services
                        .as_mut()
                        .map(|services| services.as_mut() as _),
                )?;
                Ok(NativeReturn::Value(self.host.create_database(database)?))
            }
            RuntimeHandler::SharedRecord => self.shared_record(index, arguments, vm),
            RuntimeHandler::SharedNumbers => {
                let length = match index {
                    658 => 4,
                    659 => 8,
                    _ => 0,
                };
                if arguments.len() != length {
                    return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
                }
                if !self
                    .running
                    .get(&self.host.task_id)
                    .is_some_and(|task| task.shared_numbers == Some(true))
                    || (matches!(index, 658 | 659) && self.numbers.active_local().is_none())
                {
                    return Ok(if index == 658 {
                        NativeReturn::Value(0)
                    } else {
                        NativeReturn::Continue
                    });
                }
                call_native(self.numbers, self.host, None, index, arguments, vm)
            }
            RuntimeHandler::SoundLoop => self.load_sound_loop_table(arguments, vm),
            RuntimeHandler::SoundControl => self.control_sound_native(index, arguments, vm),
            RuntimeHandler::SoundLoad => self.load_sound_native(index, arguments, vm),
            RuntimeHandler::Input => self.input_native(index, arguments, vm),
            RuntimeHandler::Message => self.message_native(index, arguments, vm),
            RuntimeHandler::TextWindow => self.text_window_native(index, arguments, vm),
            RuntimeHandler::Fes => self.fes_native(index, arguments, vm),
            RuntimeHandler::PointerPosition => {
                if arguments.len() != 8 {
                    return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
                }
                if let Some(device) = self.resources.input_device.as_mut() {
                    self.host.pointer_position = self
                        .resources
                        .input_coordinates
                        .to_logical(device.pointer()?.position)
                        .0;
                }

                call_native(
                    &mut *self.numbers,
                    &mut *self.host,
                    None,
                    index,
                    arguments,
                    vm,
                )
            }
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
