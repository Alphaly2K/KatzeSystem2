use super::*;

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn echo_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let length = if index == 398 { 48 } else { 28 };
        if arguments.len() != length {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let word = |i: usize| read::u32_at(arguments, i * 4).unwrap();
        let target = word(0);
        if index == 398 {
            if target != 0 {
                if word(1) == 0 {
                    self.command_target(target, 79, [0; 3])?;
                } else {
                    let words = [
                        word(1),
                        word(2),
                        word(6),
                        word(7),
                        word(8),
                        word(9),
                        0,
                        word(3),
                        word(4),
                        word(5),
                        0,
                        word(10),
                        word(11),
                    ];
                    let mut config: Vec<u8> =
                        words.into_iter().flat_map(u32::to_le_bytes).collect();
                    self.command_target_with_buffers(
                        target,
                        79,
                        [1, 0, 0],
                        &mut CommandBuffers {
                            data1: Some(&mut config),
                            ..Default::default()
                        },
                    )?;
                }
            }
        } else {
            let mut state = [0; 128];
            for (i, offset, length) in [(1, 12, 12), (2, 28, 12), (3, 44, 12), (4, 60, 64)] {
                state[offset..offset + length].copy_from_slice(&vm.read(word(i), length)?);
            }
            state[124..128].copy_from_slice(&word(5).to_le_bytes());
            state[8..12].copy_from_slice(&word(6).to_le_bytes());
            self.command_target_with_buffers(
                target,
                80,
                [0; 3],
                &mut CommandBuffers {
                    data1: Some(&mut state),
                    ..Default::default()
                },
            )?;
        }
        Ok(NativeReturn::Continue)
    }
}
