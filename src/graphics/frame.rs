use super::{Drawables, ProjectedVertex, Projection, Rect, RenderTarget, Surface, TextureFilter};
use crate::Result;
use std::sync::Arc;

/// 画布面块或共享的只读图像；绘制列表持有图像直到后端提交完成。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureSource {
    Canvas(u32),
    Image(Arc<Surface>),
}

impl TextureSource {
    pub fn surface<'a>(&'a self, canvas: &'a RenderTarget) -> Option<&'a Surface> {
        match self {
            Self::Canvas(tile) => canvas.tiles().get(*tile as usize).and_then(Option::as_ref),
            Self::Image(image) => Some(image),
        }
    }
}

/// 归一化纹理坐标越界时的取样方式；U、V 分别指定。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextureAddress {
    #[default]
    Mirror,
    Wrap,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameTexture {
    pub source: TextureSource,
    pub uv: [[f32; 2]; 3],
    pub address: [TextureAddress; 2],
    /// None 沿用绘制列表的取样方式。
    pub filter: Option<TextureFilter>,
}

/// 已按图元顺序、相机和投影准备的设备输入。
#[derive(Clone, Debug, PartialEq)]
pub struct DrawCommand {
    pub vertices: [ProjectedVertex; 3],
    pub texture: Option<FrameTexture>,
    pub clip: Option<Rect>,
    /// 0～5 对应普通、加算、预乘、乘算、反向目标及乘算加算；其他值使用普通混合。
    pub blend_mode: u32,
}

impl DrawCommand {
    /// Returns whether the triangle is culled by the engine's screen-space winding rules.
    pub fn is_culled(&self) -> bool {
        super::raster::triangle_culled(self.vertices)
    }
}

pub type FrameTriangle = DrawCommand;

/// 已完成排序、裁剪和投影的绘制命令；不包含设备资源或脚本对象。
#[derive(Clone, Debug, PartialEq)]
pub struct DrawList {
    pub commands: Vec<DrawCommand>,
    pub filter: TextureFilter,
}

impl DrawList {
    pub(crate) fn push_screen_quad(
        &mut self,
        rect: Rect,
        image: Option<&Arc<Surface>>,
        color: u32,
        filter: Option<TextureFilter>,
    ) {
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return;
        }
        let positions = [
            [rect.left as f32, rect.top as f32, 0.],
            [rect.right as f32, rect.top as f32, 0.],
            [rect.left as f32, rect.bottom as f32, 0.],
            [rect.right as f32, rect.bottom as f32, 0.],
        ];
        let uv = [[0., 0.], [1., 0.], [0., 1.], [1., 1.]];
        for indices in [[0, 1, 2], [2, 1, 3]] {
            self.commands.push(DrawCommand {
                vertices: indices.map(|i| ProjectedVertex {
                    position: positions[i],
                    rhw: 1.,
                    color,
                }),
                texture: image.map(|image| FrameTexture {
                    filter,
                    address: Default::default(),
                    source: TextureSource::Image(image.clone()),
                    uv: indices.map(|i| uv[i]),
                }),
                clip: None,
                blend_mode: 0,
            });
        }
    }

    /// 向已有目标混合软件帧；画布仅在命令使用其面块时需要，不清空目标。
    pub fn render_into(
        &self,
        output: &mut RenderTarget,
        canvas: Option<&RenderTarget>,
    ) -> Result<bool> {
        self.render_at(output, canvas, [0; 2])
    }

    pub(super) fn render_at(
        &self,
        output: &mut RenderTarget,
        canvas: Option<&RenderTarget>,
        origin: [u32; 2],
    ) -> Result<bool> {
        let mut touched = false;
        for command in &self.commands {
            touched |= super::raster::draw_command(output, canvas, command, self.filter, origin)?;
        }
        Ok(touched)
    }
}

impl Drawables {
    /// 合成全帧或指定深度区间；区间上界非正时返回空列表。
    pub fn compose(
        &mut self,
        projection: &Projection,
        parent: [f32; 16],
        filter: TextureFilter,
        range: Option<[f32; 2]>,
    ) -> Result<DrawList> {
        let mut frame = DrawList {
            commands: Vec::new(),
            filter,
        };
        if range.is_none_or(|range| range[1] > 0. || range[1].is_nan()) {
            self.visit_frame(projection, parent, range, |command| {
                frame.commands.push(command);
                Ok(true)
            })?;
        }
        Ok(frame)
    }

    pub fn frame_triangles(
        &mut self,
        projection: &Projection,
        parent: [f32; 16],
    ) -> Result<Vec<FrameTriangle>> {
        let mut triangles = Vec::new();
        self.visit_frame(projection, parent, None, |triangle| {
            triangles.push(triangle);
            Ok(true)
        })?;
        Ok(triangles)
    }

    pub(super) fn visit_frame(
        &mut self,
        projection: &Projection,
        parent: [f32; 16],
        range: Option<[f32; 2]>,
        mut draw: impl FnMut(FrameTriangle) -> Result<bool>,
    ) -> Result<bool> {
        self.order();
        let mut touched = false;
        for id in &self.order {
            let wave = self.vertex_waves.get(id).map(|(_, wave)| *wave);
            let drawable = self.entries.get_mut(id).unwrap();
            if let Some([lower, upper]) = range {
                let depth = drawable.transform().base_point()[2];
                if depth < lower {
                    continue;
                }
                if depth >= upper {
                    break;
                }
            }
            if drawable.display() == 0 {
                continue;
            }
            if drawable
                .image_texture
                .as_ref()
                .is_some_and(|texture| texture.surface.is_none())
            {
                continue;
            }
            if let Some(mesh) = &drawable.particle_mesh {
                let mut selected = projection.clone();
                if let Some(camera) = self.cameras.get(&drawable.camera_id()) {
                    camera.apply_projection(&mut selected);
                }
                touched |= mesh.visit(&selected, drawable.blend_mode(), &mut draw)?;
                continue;
            }
            drawable.prepare_canvas()?;
            let count = drawable
                .canvas_triangles()
                .map_or(drawable.triangles().len(), |t| t.len());
            if count == 0 {
                continue;
            }
            let clip = drawable.clip().map(|clip| projection.project_clip(clip));
            let position = drawable.transform().position();
            let origin = match drawable.projection_origin_mode() {
                0 => [position[0], position[1]],
                1 => projection.center,
                _ => [0.; 2],
            };
            let mut selected = projection.clone();
            if let Some(camera) = self.cameras.get(&drawable.camera_id()) {
                camera.apply_projection(&mut selected);
            }
            let count = if drawable.canvas_triangles().is_some() {
                count
            } else {
                count.min(32)
            };
            for index in 0..count {
                let (vertices, texture) = if drawable.canvas_triangles().is_some() {
                    let triangle = drawable
                        .transformed_canvas_triangle_with_wave(index, parent, wave)
                        .unwrap();
                    (
                        selected.project_triangle(
                            triangle.vertices,
                            drawable.transform().base_point()[2],
                            origin,
                            true,
                        ),
                        Some(FrameTexture {
                            filter: None,
                            address: Default::default(),
                            source: TextureSource::Canvas(triangle.tile),
                            uv: triangle.uv,
                        }),
                    )
                } else {
                    (
                        selected.project_triangle(
                            drawable
                                .transformed_triangle_with_wave(index, parent, wave)
                                .unwrap(),
                            drawable.transform().base_point()[2],
                            origin,
                            true,
                        ),
                        drawable.image_texture.as_ref().map(|texture| FrameTexture {
                            filter: None,
                            address: Default::default(),
                            source: TextureSource::Image(texture.surface.as_ref().unwrap().clone()),
                            uv: texture.uv[index],
                        }),
                    )
                };
                if super::raster::triangle_culled(vertices) {
                    continue;
                }
                touched |= draw(FrameTriangle {
                    vertices,
                    texture,
                    clip,
                    blend_mode: drawable.blend_mode(),
                })?;
            }
        }
        Ok(touched)
    }
}
