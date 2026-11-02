use crate::runtime::{ObjectFactory, Store};
use crate::{Error, Result};

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn fes_font_command(&mut self, args: &[Vec<u8>], offset: usize) -> Result<bool> {
        if !args.first().is_some_and(|name| name == b"fontcom") {
            return Ok(false);
        }
        match args.get(1).map(Vec::as_slice) {
            Some(b"add") => {
                if let Some(name) = args.get(2).filter(|name| !name.is_empty()) {
                    self.resources
                        .fonts
                        .as_mut()
                        .ok_or_else(|| Error::new(offset, "运行时未配置字体服务"))?
                        .add_resource(name)?;
                }
            }
            Some(b"init" | b"count" | b"get" | b"search" | b"default") => {
                return Err(Error::new(offset, "FES 字体枚举及默认名称命令尚未接入"));
            }
            _ => (),
        }
        Ok(true)
    }
}
