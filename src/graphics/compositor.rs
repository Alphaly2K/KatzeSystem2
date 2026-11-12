use super::{DrawList, Drawables, DrawingContext, Projection, RenderTarget, TextureFilter};
use crate::{Error, Result};

/// 持有画布、绘图登记和帧视图，生成设备无关的绘制列表。
#[derive(Default)]
pub struct Compositor {
    pub(crate) drawing: DrawingContext,
    pub(crate) drawables: Drawables,
    pub(crate) frame_view: Option<(Projection, [f32; 16], TextureFilter)>,
}

impl Compositor {
    pub fn drawing(&self) -> &DrawingContext {
        &self.drawing
    }

    pub fn drawing_mut(&mut self) -> &mut DrawingContext {
        &mut self.drawing
    }

    pub fn drawables(&self) -> &Drawables {
        &self.drawables
    }

    pub fn drawables_mut(&mut self) -> &mut Drawables {
        &mut self.drawables
    }

    /// 后续合成使用同一视图；调用本身不绘制或更改画布像素。
    pub fn set_frame_view(
        &mut self,
        projection: Projection,
        parent: [f32; 16],
        filter: TextureFilter,
    ) {
        self.frame_view = Some((projection, parent, filter));
    }

    /// 合成全帧或指定深度区间；调用前须配置帧视图。
    pub fn compose(&mut self, range: Option<[f32; 2]>) -> Result<DrawList> {
        let (projection, parent, filter) = self
            .frame_view
            .as_ref()
            .ok_or_else(|| Error::new(0, "绘制列表需要配置运行时帧视图"))?;
        self.drawables.compose(projection, *parent, *filter, range)
    }

    /// 使用显式视图向现有目标混合软件帧，不清空目标。
    pub fn render_frame(
        &mut self,
        output: &mut RenderTarget,
        projection: &Projection,
        parent: [f32; 16],
        filter: TextureFilter,
    ) -> Result<bool> {
        let canvas = self
            .drawing
            .target
            .as_ref()
            .ok_or_else(|| Error::new(0, "运行时未配置画布纹理输入"))?;
        self.drawables
            .render_frame(output, canvas, projection, parent, filter)
    }

    /// 使用当前绘图上下文提交非纹理三角形，保留上下文显示门控。
    pub fn render_triangles(&mut self, projection: &Projection, parent: [f32; 16]) -> Result<bool> {
        self.drawing
            .render_triangles(&mut self.drawables, projection, parent)
    }
}
