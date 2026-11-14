use super::{DrawList, Drawables, DrawingContext, Projection, RenderTarget, TextureFilter};
use crate::{Error, Result};

#[derive(Clone, Copy)]
pub(super) enum ScreenEffect {
    Mosaic(u32),
    Shatter(u32),
    PageFlip(u32),
    RWipe(u32),
}

/// 持有画布、绘图登记和帧视图，生成设备无关的绘制列表。
#[derive(Default)]
pub struct Compositor {
    pub(crate) drawing: DrawingContext,
    pub(crate) drawables: Drawables,
    pub(crate) frame_view: Option<(Projection, [f32; 16], TextureFilter)>,
    pub(crate) auxiliary_targets: Vec<RenderTarget>,
    pub(crate) mosaics: super::mosaic::Mosaics,
    pub(crate) shatters: super::shatter::Shatters,
    pub(crate) page_flips: std::collections::BTreeMap<u32, super::page_flip::PageFlip>,
    pub(crate) rwipes: std::collections::BTreeMap<u32, super::rwipe::RWipe>,
    pub(crate) drawing_rect: Option<super::Rect>,
    pub(super) screen_effect: Option<ScreenEffect>,
}

impl Compositor {
    /// 当前输出目标的绘图矩形；None 使用逻辑屏幕全幅，不影响画布面块布局。
    pub fn set_drawing_rect(&mut self, rect: Option<super::Rect>) {
        self.drawing_rect = rect;
    }

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

    /// 按零起始编号保存的辅助绘制目标；独立于画布和单次画面捕获。
    pub fn auxiliary_targets(&self) -> &[RenderTarget] {
        &self.auxiliary_targets
    }

    pub(crate) fn reserve_auxiliary_targets(
        &mut self,
        size: [u32; 2],
        count: u32,
        maximum_size: u32,
    ) -> Result<()> {
        if count > 2 || count as usize <= self.auxiliary_targets.len() {
            return Ok(());
        }
        self.auxiliary_targets.clear();
        if maximum_size as i32 > 0 && size.iter().any(|&dimension| dimension > maximum_size) {
            return Ok(());
        }
        for _ in 0..count {
            self.auxiliary_targets.push(RenderTarget::new(size)?);
        }
        Ok(())
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
        self.compose_view(projection.clone(), *parent, *filter, range)
    }

    fn compose_view(
        &mut self,
        projection: Projection,
        parent: [f32; 16],
        filter: TextureFilter,
        range: Option<[f32; 2]>,
    ) -> Result<DrawList> {
        if range.is_some() {
            return self.drawables.compose(&projection, parent, filter, range);
        }
        if let Some(ScreenEffect::RWipe(id)) = self.screen_effect {
            let effect = self.rwipes.get_mut(&id).unwrap();
            let far = effect.far;
            let mut frame = self
                .drawables
                .compose(&projection, parent, filter, Some([0., far]))?;
            frame.commands.splice(0..0, effect.background());
            frame.commands.extend(effect.frame()?);
            frame.commands.extend(
                self.drawables
                    .compose(&projection, parent, filter, Some([far, 1_000_000.]))?
                    .commands,
            );
            return Ok(frame);
        }
        if let Some(ScreenEffect::Shatter(id)) = self.screen_effect {
            let effect = self.shatters.entries.get_mut(&id).unwrap();
            let far = effect.far;
            let mut frame = self
                .drawables
                .compose(&projection, parent, filter, Some([0., far]))?;
            frame
                .commands
                .splice(0..0, effect.background().into_iter().flatten());
            frame.commands.extend(effect.frame());
            frame.commands.extend(
                self.drawables
                    .compose(&projection, parent, filter, Some([far, 1_000_000.]))?
                    .commands,
            );
            return Ok(frame);
        }
        if let Some(ScreenEffect::PageFlip(id)) = self.screen_effect {
            let effect = self.page_flips.get_mut(&id).unwrap();
            let depth = effect.depth;
            let mut frame =
                self.drawables
                    .compose(&projection, parent, filter, Some([0., depth]))?;
            frame.commands.splice(0..0, effect.background());
            frame.commands.extend(effect.frame());
            frame.commands.extend(
                self.drawables
                    .compose(&projection, parent, filter, Some([depth, 1_000_000.]))?
                    .commands,
            );
            return Ok(frame);
        }
        let Some(ScreenEffect::Mosaic(id)) = self.screen_effect else {
            return self.drawables.compose(&projection, parent, filter, range);
        };
        let effect = self.mosaics.entries.get_mut(&id).unwrap();
        let far = effect.far;
        let image = effect.frame()?;
        let mut frame = self
            .drawables
            .compose(&projection, parent, filter, Some([0., far]))?;
        if let Some((image, opacity)) = image {
            frame
                .commands
                .extend(super::mosaic::screen_commands(image, opacity));
        }
        frame.commands.extend(
            self.drawables
                .compose(&projection, parent, filter, Some([far, 1_000_000.]))?
                .commands,
        );
        Ok(frame)
    }

    /// 使用显式视图向现有目标混合软件帧，不清空目标。
    pub fn render_frame(
        &mut self,
        output: &mut RenderTarget,
        projection: &Projection,
        parent: [f32; 16],
        filter: TextureFilter,
    ) -> Result<bool> {
        let frame = self.compose_view(projection.clone(), parent, filter, None)?;
        let canvas = self
            .drawing
            .target
            .as_ref()
            .ok_or_else(|| Error::new(0, "运行时未配置画布纹理输入"))?;
        frame.render_into(output, Some(canvas))
    }

    /// 使用当前绘图上下文提交非纹理三角形，保留上下文显示门控。
    pub fn render_triangles(&mut self, projection: &Projection, parent: [f32; 16]) -> Result<bool> {
        self.drawing
            .render_triangles(&mut self.drawables, projection, parent)
    }
}
