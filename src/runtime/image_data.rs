use super::Resources;
use crate::kcs::{Interpreter, NativeReturn};
use crate::{Error, Result};

impl Resources {
    pub(super) fn image_data_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let count = if index == 369 { 2 } else { 3 };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let args: Vec<_> = arguments
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        if matches!(index, 362 | 363) {
            for address in &args[1..] {
                vm.read(*address, 4)?;
            }
            for address in &args[1..] {
                vm.write(*address, &[0; 4])?;
            }
            if args[0] != 0 {
                let info = self.images.get(args[0])?.info();
                for (axis, address) in args[1..].iter().enumerate() {
                    let value = if index == 363 {
                        info.offset[axis] as u32
                    } else if info.canvas_size[axis] == 0 {
                        info.size[axis]
                    } else {
                        info.canvas_size[axis]
                    };
                    vm.write(*address, &value.to_le_bytes())?;
                }
            }
            return Ok(NativeReturn::Continue);
        }
        if args[0] == 0 {
            return Ok(if index == 369 {
                NativeReturn::Continue
            } else {
                NativeReturn::Value(0)
            });
        }
        if index == 369 {
            self.images.get_mut(args[0])?.multiply_color(args[1]);
            return Ok(NativeReturn::Continue);
        }
        if index == 1026 {
            let image = self.images.get(args[0])?;
            let info = image.info();
            let x = args[1].wrapping_sub(info.offset[0] as u32);
            let y = args[2].wrapping_sub(info.offset[1] as u32);
            let pixel = if x < info.size[0] && y < info.size[1] {
                image.surface().pixel(x, info.size[1] - 1 - y).unwrap_or(0)
            } else {
                0
            };
            return Ok(NativeReturn::Value(pixel));
        }
        let info = self.images.get_mut(args[0])?.info_mut();
        match index {
            372 => info.canvas_size = [args[1], args[2]],
            373 => info.offset = [args[1] as i32, args[2] as i32],
            _ => unreachable!(),
        }
        Ok(NativeReturn::Value(1))
    }
}
