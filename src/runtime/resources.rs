use crate::graphics::{DrawingContext, Images, Rect};
use crate::kcs::{Interpreter, NativeReturn, ResourceServices, format_string};
use crate::{Error, Result};

#[derive(Default)]
pub(super) struct Resources {
    pub services: Option<Box<dyn ResourceServices>>,
    pub images: Images,
    pub drawing: DrawingContext,
}

impl Resources {
    pub fn call(
        &mut self,
        owner: u32,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let count = if index == 111 {
            1
        } else if arguments.len() == 20 && arguments.starts_with(&[0; 4]) {
            5
        } else {
            6
        };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mut args = [0; 6];
        for (arg, bytes) in args.iter_mut().zip(arguments.chunks_exact(4)) {
            *arg = u32::from_le_bytes(bytes.try_into().unwrap());
        }
        if index == 111 {
            let names = format_string(vm, args[0], 0)?;
            let names = &names[..names.iter().position(|&b| b == 0).unwrap_or(names.len())];
            if names.iter().all(|&b| b == b' ') {
                return Ok(NativeReturn::Value(0));
            }
            let services = self
                .services
                .as_mut()
                .ok_or_else(|| Error::new(vm.pc, "运行时未配置资源服务"))?;
            let handle = self.images.load(owner, names, services.as_mut())?;
            return Ok(NativeReturn::Value(handle));
        }
        if args[0] == 0 {
            self.drawing.clear_rect(Rect::from_xywh(
                args[1] as i32,
                args[2] as i32,
                args[3] as i32,
                args[4] as i32,
            ));
            return Ok(NativeReturn::Continue);
        }
        self.images.get_mut(args[0])?.fill_rect_from_bottom(
            Rect::from_xywh(
                args[1] as i32,
                args[2] as i32,
                args[3] as i32,
                args[4] as i32,
            ),
            args[5],
        );
        Ok(NativeReturn::Continue)
    }
}
