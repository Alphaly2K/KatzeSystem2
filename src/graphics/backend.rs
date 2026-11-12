use super::{DrawList, RenderTarget};
use crate::{Error, Result};

/// 执行合成器生成的绘制列表，独立持有设备和呈现资源。
pub trait GraphicsBackend {
    /// 返回清零后绘制的完整帧；画布是只读纹理来源，可消费其更新标记。
    fn render(
        &mut self,
        frame: &DrawList,
        canvas: &mut RenderTarget,
        size: [u32; 2],
    ) -> Result<RenderTarget>;

    /// 提交至宿主已附着的窗口；暂时无法呈现时返回 false。
    fn present(
        &mut self,
        _frame: &DrawList,
        _canvas: &mut RenderTarget,
        _size: [u32; 2],
    ) -> Result<bool> {
        Err(Error::new(0, "图形后端未提供窗口呈现"))
    }
}

/// 不访问窗口或设备的软件图形后端。
#[derive(Default)]
pub struct SoftwareBackend;

impl GraphicsBackend for SoftwareBackend {
    fn render(
        &mut self,
        frame: &DrawList,
        canvas: &mut RenderTarget,
        size: [u32; 2],
    ) -> Result<RenderTarget> {
        let mut output = RenderTarget::new(size)?;
        for &command in &frame.commands {
            super::raster::draw_command(&mut output, Some(canvas), command, frame.filter)?;
        }
        Ok(output)
    }
}
