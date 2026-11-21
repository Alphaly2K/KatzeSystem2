use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn};
use crate::{Error, Result};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn save_configuration_native(
        &mut self,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
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

    pub(super) fn open_file_native(
        &mut self,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        crate::kcs::open_file_native(
            self.host,
            arguments,
            vm,
            self.resources
                .services
                .as_mut()
                .map(|services| services.as_mut() as _),
        )
    }
}
