use super::{ObjectFactory, ObjectHost};
use crate::graphics::ExportFormat;
use crate::kcs::{Interpreter, NativeReturn, format_string};
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn image_export_native(
        &mut self,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        if arguments.len() != 8 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let handle = read::u32_at(arguments, 0)?;
        if handle == 0 {
            return Ok(NativeReturn::Value(0));
        }
        let name = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
        let name = self
            .host
            .folder_paths
            .as_ref()
            .ok_or_else(|| Error::new(vm.pc, "运行时未配置目录展开上下文"))?
            .expand_bytes(&name, 259)?;
        let format = ExportFormat::from_name(&name);
        if self.resources.images.pixel_handle(handle)? == 0 {
            return if format == ExportFormat::Png {
                Ok(NativeReturn::Value(0))
            } else {
                Err(Error::new(vm.pc, "图像导出预处理读取空位图"))
            };
        }
        let Some(bytes) = self
            .resources
            .images
            .get_mut(handle)?
            .encode_export(format)?
        else {
            return Ok(NativeReturn::Value(0));
        };
        let Some(mut writer) = self
            .host
            .file_services
            .as_deref_mut()
            .ok_or_else(|| Error::new(vm.pc, "运行时未配置图像文件输出服务"))?
            .create_utf16(self.host.task_id, &name)?
        else {
            return Ok(NativeReturn::Value(0));
        };
        // 原写入器返回字节数或全一错误码，包装仅将零视为失败。
        Ok(NativeReturn::Value(u32::from(!matches!(
            writer.write(&bytes),
            Ok(0)
        ))))
    }
}
