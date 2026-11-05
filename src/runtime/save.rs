use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn};
use crate::task::CommandBuffers;
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn shared_record(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let handle = read::u32_at(arguments, 0)?;
        let size = if index == 240 { 20 } else { 16 };
        if arguments.len() != size && !(handle == 0 && arguments.len() == 4) {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        if handle == 0 {
            return Ok(NativeReturn::Value(0));
        }
        let mut available = [0; 4];
        self.command_target_with_buffers(
            read::u32_at(arguments, 8)?,
            177,
            [0, 1, 0],
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut available),
                external: None,
            },
        )?;
        if available == [0; 4] {
            return Ok(NativeReturn::Value(0));
        }
        let record = read::u32_at(arguments, 4)?;
        let start = read::u32_at(arguments, 12)? as usize;
        if index == 240 {
            let count = read::u32_at(arguments, 16)? as usize;
            if count > read::MAX_BYTES / 4 {
                return Err(Error::new(vm.pc, "CSS 数值记录超过读取上限"));
            }
            let data: Vec<_> = self
                .numbers
                .shared
                .get(start..)
                .unwrap_or_default()
                .iter()
                .take(count)
                .flat_map(|value| value.to_le_bytes())
                .collect();
            self.host
                .data_container_mut(handle)?
                .append(record, &data)?;
            return Ok(NativeReturn::Value(1));
        }
        let Some(data) = self.host.data_container(handle)?.record(record)? else {
            return Ok(NativeReturn::Value(0));
        };
        if let Some(target) = self.numbers.shared.get_mut(start..) {
            for (target, source) in target.iter_mut().zip(data.chunks_exact(4)) {
                *target = i32::from_le_bytes(source.try_into().unwrap());
            }
        }
        Ok(NativeReturn::Value(1))
    }
}

pub(super) fn query_shared_numbers_available(
    available: bool,
    deleted: bool,
    buffers: &mut CommandBuffers<'_>,
) -> Result<()> {
    if !deleted {
        let target = buffers
            .data1
            .as_deref_mut()
            .and_then(|b| b.get_mut(..4))
            .ok_or_else(|| Error::new(0, "数值区查询缺少可用标记缓冲区"))?;
        target.copy_from_slice(&u32::from(available).to_le_bytes());
    }
    Ok(())
}
