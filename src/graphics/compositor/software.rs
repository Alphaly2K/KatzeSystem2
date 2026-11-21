use super::Compositor;
use crate::{
    Error, Result,
    graphics::{Projection, RenderTarget},
    host::graphics::{DrawList, TextureFilter, software::SoftwareBackend},
};

impl Compositor {
    /// Compose with the software renderer for any intermediate filter passes.
    pub fn compose(&mut self, range: Option<[f32; 2]>) -> Result<DrawList> {
        self.compose_with_backend(range, Some(&mut SoftwareBackend))
    }

    /// 使用显式视图向现有目标混合软件帧，不清空目标。
    pub fn render_frame(
        &mut self,
        output: &mut RenderTarget,
        projection: &Projection,
        parent: [f32; 16],
        filter: TextureFilter,
    ) -> Result<bool> {
        let frame = self.compose_view(
            projection.clone(),
            parent,
            filter,
            None,
            Some(&mut SoftwareBackend),
        )?;
        let canvas = self
            .drawing
            .target
            .as_ref()
            .ok_or_else(|| Error::new(0, "运行时未配置画布纹理输入"))?;
        frame.render_into(output, Some(canvas))
    }
}
