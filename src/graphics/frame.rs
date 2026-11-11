use super::{Drawables, ProjectedVertex, Projection, Rect};
use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameTexture {
    pub tile: u32,
    pub uv: [[f32; 2]; 3],
}

/// 已按图元顺序、相机和投影准备的设备输入。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameTriangle {
    pub vertices: [ProjectedVertex; 3],
    pub texture: Option<FrameTexture>,
    pub clip: Option<Rect>,
}

impl Drawables {
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
            drawable.prepare_canvas()?;
            let count = drawable
                .canvas_triangles()
                .map_or(drawable.triangles().len(), |t| t.len());
            if count == 0 {
                continue;
            }
            if drawable.blend_mode() != 0 {
                return Err(Error::new(0, "三角形特殊混合模式尚未接入"));
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
                    let triangle = drawable.transformed_canvas_triangle(index, parent).unwrap();
                    (
                        selected.project_triangle(
                            triangle.vertices,
                            drawable.transform().base_point()[2],
                            origin,
                            true,
                        ),
                        Some(FrameTexture {
                            tile: triangle.tile,
                            uv: triangle.uv,
                        }),
                    )
                } else {
                    (
                        drawable
                            .projected_triangle(index, parent, &selected, origin, true)
                            .unwrap(),
                        None,
                    )
                };
                super::raster::validate_depth(vertices)?;
                touched |= draw(FrameTriangle {
                    vertices,
                    texture,
                    clip,
                })?;
            }
        }
        Ok(touched)
    }
}
