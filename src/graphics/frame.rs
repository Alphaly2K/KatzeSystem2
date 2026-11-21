use super::{Drawables, Projection};
use crate::Result;
use crate::host::graphics::{
    DrawCommand, DrawList, FrameTexture, ProjectedVertex, Rect, Surface, TextureFilter,
    TextureSource, triangle_culled,
};
use std::sync::Arc;

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
    ) -> Result<Vec<DrawCommand>> {
        let mut triangles = Vec::new();
        self.visit_frame(projection, parent, None, |triangle| {
            triangles.push(triangle);
            Ok(true)
        })?;
        Ok(triangles)
    }

    pub(crate) fn visit_frame(
        &mut self,
        projection: &Projection,
        parent: [f32; 16],
        range: Option<[f32; 2]>,
        mut draw: impl FnMut(DrawCommand) -> Result<bool>,
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
                if triangle_culled(vertices) {
                    continue;
                }
                touched |= draw(DrawCommand {
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
