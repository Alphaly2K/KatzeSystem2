use super::{ObjectFactory, Runtime};
use crate::graphics::{Image, RenderTarget};
use crate::{Error, Result};

impl<F: ObjectFactory> Runtime<F> {
    /// 为当前呈现帧执行连续 BMP 输出；未启用或绘图设备关闭时返回 false。
    /// 软件呈现宿主在每次提交帧后调用一次；present_frame 已自动执行。
    pub fn capture_sequence_frame(&mut self, frame: &RenderTarget) -> Result<bool> {
        let drawing = &self.resources.compositor.drawing;
        if self.host.frame_capture_enabled == 0 || drawing.mode != 0 || !drawing.enabled {
            return Ok(false);
        }
        let Some(bytes) = Image::encode_frame_bmp(frame, self.resources.capture_rectangle)? else {
            return Ok(false);
        };
        let index = self.host.frame_capture_index;
        self.host.frame_capture_index = index.wrapping_add(1);
        let folder = &self.host.frame_capture_folder;
        let mut name = Vec::with_capacity(folder.len() * 2 + 20);
        name.extend_from_slice(folder);
        name.push(92);
        name.extend_from_slice(folder);
        name.extend(format!("_{:010}.bmp", index as i32).encode_utf16());
        if let Some(mut writer) = self
            .host
            .file_services
            .as_deref_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置连续截图文件输出服务"))?
            .create_utf16(self.host.frame_capture_task_id, &name)?
        {
            let _ = writer.write(&bytes);
        }
        Ok(true)
    }
}
