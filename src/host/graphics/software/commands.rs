use super::{RenderTarget, Surface, TextureSource};
use crate::{
    Error, Result,
    graphics::{
        Drawables, Projection,
        raster::{RasterState, needs_depth_clip},
    },
    host::graphics::{DrawCommand, ProjectedVertex, TextureFilter},
};

impl Drawables {
    /// 按登记顺序与基点 Z 绘制非纹理三角形；纹理图元需要 render_frame 的独立画布。
    pub fn render_triangles(
        &mut self,
        target: &mut RenderTarget,
        projection: &Projection,
        parent: [f32; 16],
    ) -> Result<bool> {
        self.render_to(target, None, projection, parent, TextureFilter::Point, None)
    }

    /// 读取画布面块并写入独立的最终帧；过滤显式选择，连续调用继续混合输出。
    pub fn render_frame(
        &mut self,
        output: &mut RenderTarget,
        canvas: &RenderTarget,
        projection: &Projection,
        parent: [f32; 16],
        filter: TextureFilter,
    ) -> Result<bool> {
        self.render_to(output, Some(canvas), projection, parent, filter, None)
    }

    /// 按图元基点 Z 选择 [lower, upper)；非正上界不绘制。
    pub fn render_depth_range(
        &mut self,
        output: &mut RenderTarget,
        canvas: &RenderTarget,
        projection: &Projection,
        parent: [f32; 16],
        filter: TextureFilter,
        range: [f32; 2],
    ) -> Result<bool> {
        if range[1] <= 0. {
            return Ok(false);
        }
        self.render_to(
            output,
            Some(canvas),
            projection,
            parent,
            filter,
            Some(range),
        )
    }

    fn render_to(
        &mut self,
        output: &mut RenderTarget,
        canvas: Option<&RenderTarget>,
        projection: &Projection,
        parent: [f32; 16],
        filter: TextureFilter,
        range: Option<[f32; 2]>,
    ) -> Result<bool> {
        let tiles = canvas.map(RenderTarget::tiles);
        self.visit_frame(projection, parent, range, |command| {
            draw_command(output, tiles, &command, filter, [0; 2])
        })
    }
}

pub(super) fn draw_command(
    output: &mut RenderTarget,
    tiles: Option<&[Option<Surface>]>,
    command: &DrawCommand,
    filter: TextureFilter,
    origin: [u32; 2],
) -> Result<bool> {
    let vertices = command.vertices;
    let state = RasterState {
        origin,
        clip: command.clip,
        blend_mode: command.blend_mode,
        ..Default::default()
    };
    if command.is_culled() {
        return Ok(false);
    }
    if let Some(source) = &command.texture {
        let filter = source.filter.unwrap_or(filter);
        let state = RasterState {
            address: source.address,
            ..state
        };
        if tiles.is_none() && matches!(source.source, TextureSource::Canvas(_)) {
            return Err(Error::new(0, "纹理图元需要独立画布输入与帧输出"));
        }
        let texture = source.source.resolve(tiles.unwrap_or_default());
        if let Some(texture) = texture {
            if vertices.iter().any(|v| !v.rhw.is_finite()) {
                return Err(Error::new(0, "纹理插值需要有限的 RHW"));
            }
            return Ok(
                output.blend_textured_triangle_clipped(vertices, source.uv, texture, filter, state)
            );
        }
    }
    draw_colors(output, vertices, state)
}

fn draw_colors(
    target: &mut RenderTarget,
    vertices: [ProjectedVertex; 3],
    state: RasterState,
) -> Result<bool> {
    if vertices.iter().all(|v| v.color == vertices[0].color) {
        if vertices.iter().any(|v| v.rhw < 0.) {
            return Ok(target.blend_projected_triangle_clipped(vertices, state));
        }
        if needs_depth_clip(vertices) {
            return Ok(target.blend_projected_triangle_clipped(
                vertices.map(|v| ProjectedVertex { rhw: 1., ..v }),
                state,
            ));
        }
        Ok(target.blend_triangle_clipped(
            vertices.map(|v| [v.position[0], v.position[1]]),
            vertices[0].color,
            state,
        ))
    } else {
        if vertices.iter().any(|v| !v.rhw.is_finite()) {
            return Err(Error::new(0, "颜色插值需要有限的 RHW"));
        }
        Ok(target.blend_projected_triangle_clipped(vertices, state))
    }
}
