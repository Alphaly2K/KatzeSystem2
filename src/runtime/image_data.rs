use super::Resources;
use crate::graphics::Image;
use crate::kcs::{Interpreter, NativeReturn};
use crate::{Error, Result};

impl Resources {
    pub(super) fn image_data_native(
        &mut self,
        owner: u32,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let count = match index {
            118 | 377 => 1,
            364 | 369 | 378 => 2,
            121 => 4,
            _ => 3,
        };
        let short_zero = matches!(index, 370 | 371 | 374 | 378)
            && (4..count * 4).contains(&arguments.len())
            && arguments.len().is_multiple_of(4)
            && arguments.starts_with(&[0; 4]);
        if arguments.len() != count * 4 && !short_zero {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let args: Vec<_> = arguments
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        if index == 118 {
            return Ok(NativeReturn::Value(self.images.duplicate(owner, args[0])?));
        }
        if index == 364 {
            if args[0] == 0 {
                return Ok(NativeReturn::Value(0));
            }
            let bytes = self.raw.image_data(args[0])?;
            let frame = if bytes.starts_with(b"HG-3") {
                crate::hg3::decode_frame(bytes, args[1] as i32)?
            } else if bytes.starts_with(b"HG-2") {
                Some(crate::hg2::decode_frame(bytes)?).filter(|frame| frame.id == args[1])
            } else if bytes.starts_with(&[0xff, 0xd8]) {
                return Err(Error::new(vm.pc, "未支持的 JPEG 图像解码"));
            } else {
                None
            };
            let Some(frame) = frame else {
                return Ok(NativeReturn::Value(0));
            };
            let has_alpha = Some(frame.info.reserved != 0);
            return Ok(NativeReturn::Value(self.images.insert(
                owner,
                Image::from_frame(frame)?,
                has_alpha,
            )?));
        }
        if matches!(index, 119 | 362 | 363) {
            for address in &args[1..] {
                vm.read(*address, 4)?;
            }
            for address in &args[1..] {
                vm.write(*address, &[0; 4])?;
            }
            if args[0] != 0 {
                if index == 119 && self.images.pixel_handle(args[0])? == 0 {
                    return Ok(NativeReturn::Continue);
                }
                let info = self.images.get(args[0])?.info();
                for (axis, address) in args[1..].iter().enumerate() {
                    let value = if index == 119 {
                        info.size[axis]
                    } else if index == 363 {
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
            if index == 377 {
                return Err(Error::new(vm.pc, "空图像的透明度查询返回未初始化结果"));
            }
            return Ok(if matches!(index, 121 | 122 | 369 | 378 | 538) {
                NativeReturn::Continue
            } else {
                NativeReturn::Value(0)
            });
        }
        if index == 370 {
            let image = self.images.get(args[0])?;
            return Ok(NativeReturn::Value(if image.is_rgb() {
                255
            } else {
                image.pixel_from_bottom(args[1], args[2]) >> 24
            }));
        }
        if index == 371 {
            let base = self.images.get(args[0])?.info().base;
            for (value, address) in base.into_iter().zip(&args[1..]) {
                vm.write(*address, &value.to_le_bytes())?;
            }
            return Ok(NativeReturn::Value(1));
        }
        if index == 377 {
            return Ok(NativeReturn::Value(u32::from(
                self.images.uses_alpha(args[0])?,
            )));
        }
        if index == 378 {
            self.images.set_uses_alpha(args[0], args[1] != 0)?;
            return Ok(NativeReturn::Continue);
        }
        if index == 121 {
            let rgb = [args[1], args[2], args[3]];
            if rgb != [100; 3] {
                let image = self.images.get_mut(args[0])?;
                if !image.is_rgb() {
                    image.adjust_rgb_percent(rgb);
                }
            }
            return Ok(NativeReturn::Continue);
        }
        if index == 122 {
            self.images
                .get_mut(args[0])?
                .resize_canvas([args[1], args[2]])?;
            return Ok(NativeReturn::Continue);
        }
        if index == 369 {
            self.images.get_mut(args[0])?.multiply_color(args[1]);
            return Ok(NativeReturn::Continue);
        }
        if index == 538 {
            self.images
                .get_mut(args[0])?
                .blur([args[1] as i32, args[2] as i32])?;
            return Ok(NativeReturn::Continue);
        }
        if index == 1026 {
            let image = self.images.get(args[0])?;
            return Ok(NativeReturn::Value(
                image.pixel_from_bottom(args[1], args[2]),
            ));
        }
        let info = self.images.get_mut(args[0])?.info_mut();
        match index {
            372 => info.canvas_size = [args[1], args[2]],
            373 => info.offset = [args[1] as i32, args[2] as i32],
            374 => info.base = [args[1] as i32, args[2] as i32],
            _ => unreachable!(),
        }
        Ok(NativeReturn::Value(1))
    }
}
