use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn};
use crate::memory::SharedMemory;
use crate::save::StateTree;
use crate::task::CommandBuffers;
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn state_record_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let container = read::u32_at(arguments, 0)?;
        if container == 0 {
            return Ok(NativeReturn::Value(0));
        }
        if arguments.len() != 12 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let record = read::u32_at(arguments, 4)?;
        let target = read::u32_at(arguments, 8)?;
        if index == 688 {
            let mut buffers = CommandBuffers::default();
            let state = self.command_target_with_buffers(target, 445, [0; 3], &mut buffers)?;
            if state == 0 {
                return Ok(NativeReturn::Value(0));
            }
            let encoded = buffers
                .external
                .as_ref()
                .ok_or_else(|| Error::new(vm.pc, "状态树查询缺少编码视图"))?;
            let bytes = encoded.read(0, encoded.len())?.to_vec();
            if !bytes.is_empty() {
                StateTree::from_record(&bytes)?;
                self.host
                    .data_container_mut(container)?
                    .append(record, &bytes)?;
            }
            self.command_target_with_buffers(target, 447, [0, state, 0], &mut buffers)?;
            return Ok(NativeReturn::Value(u32::from(!bytes.is_empty())));
        }
        let Some(bytes) = self.host.data_container(container)?.record(record)? else {
            return Ok(NativeReturn::Value(0));
        };
        let mut bytes = bytes.to_vec();
        if index == 689 {
            StateTree::from_record(&bytes)?;
            self.command_target_with_buffers(
                target,
                446,
                [0; 3],
                &mut CommandBuffers {
                    external: Some(SharedMemory::new(bytes)?),
                    ..Default::default()
                },
            )?;
            return Ok(NativeReturn::Value(1));
        }
        let result = self.command_target_with_buffers(
            target,
            437,
            [0; 3],
            &mut CommandBuffers {
                data1: Some(&mut bytes),
                ..Default::default()
            },
        );
        // 437 使用容器正文的原地址，接收者对记录的修改须写回首个同编号记录。
        let destination = self
            .host
            .data_container_mut(container)?
            .record_mut(record)?
            .filter(|record| record.len() == bytes.len())
            .ok_or_else(|| Error::new(vm.pc, "恢复期间状态记录已改变"))?;
        destination.copy_from_slice(&bytes);
        Ok(NativeReturn::Value(u32::from(result? != 0)))
    }
}
