use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn, ParticleTask, format_string};
use crate::task::CommandBuffers;
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn load_particle_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        if arguments.len() != 24 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut record = [0; 0x290];
        record[..4].copy_from_slice(&u32::from(index == 693).to_le_bytes());
        for (argument, at, category) in [(4, 4, &b"ptcl"[..]), (8, 0x84, &b"image"[..])] {
            let name = format_string(vm, read::u32_at(arguments, argument)?, 0)?;
            let name = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(vm.pc, "运行时未配置粒子资源服务"))?
                .resolve(category, &name)?;
            let name = &name[..name
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(name.len())];
            if name.len() >= 128 {
                return Err(Error::new(vm.pc, "粒子资源名称超过缓冲区上限"));
            }
            record[at..at + name.len()].copy_from_slice(name);
        }
        record[0x104..0x10c].copy_from_slice(&arguments[12..20]);
        record[0x10c..0x18c].copy_from_slice(&vm.read(read::u32_at(arguments, 20)?, 128)?);
        self.command_target_with_buffers(
            read::u32_at(arguments, 0)?,
            320,
            [0; 3],
            &mut CommandBuffers {
                data1: Some(&mut record),
                ..CommandBuffers::default()
            },
        )?;
        Ok(NativeReturn::Value(1))
    }

    pub(super) fn update_particle_task(
        &mut self,
        task: &mut ParticleTask,
        now: u32,
        budget: usize,
    ) -> Result<bool> {
        task.host.frame_rate = self
            .host
            .platform_settings
            .as_ref()
            .map_or(60, |settings| settings.window_fps);
        task.host.surface_size = self.host.surface_size;
        if task.host.object_id != -1 {
            return Err(Error::new(0, "粒子外部来源查询尚未接入"));
        }
        std::mem::swap(&mut self.host.random, &mut task.host.random);
        let result = task.update(now, budget);
        std::mem::swap(&mut self.host.random, &mut task.host.random);
        self.host
            .debug_messages
            .append(&mut task.host.debug_messages);
        Ok(!matches!(result?, Some(crate::Step::Yield)))
    }
}
