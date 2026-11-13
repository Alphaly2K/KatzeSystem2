use super::{DrawCommand, FrameTexture, ProjectedVertex, Projection, Surface, TextureSource};
use crate::kcs::{ParticleHost, ParticleVertex, transform_vector};
use crate::{Error, Result, read};
use std::cmp::Ordering;
use std::sync::Arc;

/// 旧式粒子绘制快照；解释器更新与设备提交分别持有自己的网格。
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleMesh {
    quads: Vec<[ParticleVertex; 4]>,
    offsets: Vec<[f32; 4]>,
    flags: Vec<u32>,
    order: Vec<usize>,
    texture: Option<Arc<Surface>>,
    viewport: [u32; 2],
    depth: f32,
    reference_depth: f32,
    pub(crate) opacity: u32,
}

impl ParticleMesh {
    pub fn new(
        particles: &ParticleHost,
        texture: Option<Arc<Surface>>,
        reference_depth: f32,
    ) -> Result<Self> {
        let mut mesh = Self {
            quads: Vec::new(),
            offsets: Vec::new(),
            flags: Vec::new(),
            order: Vec::new(),
            texture,
            viewport: particles.surface_size,
            depth: particles.depth_offset,
            reference_depth,
            opacity: 256,
        };
        mesh.update(particles)?;
        Ok(mesh)
    }

    /// 更新复用已有容量；数组不一致时保留上一份快照。
    pub fn update(&mut self, particles: &ParticleHost) -> Result<()> {
        let count = particles.quads.len();
        if count > read::MAX_BYTES / 244
            || particles.offsets.len() != count
            || particles.quad_flags.len() != count
        {
            return Err(Error::new(0, "粒子绘制网格数组数量不匹配"));
        }
        self.quads.clone_from(&particles.quads);
        self.offsets.clone_from(&particles.offsets);
        self.flags.clone_from(&particles.quad_flags);
        self.depth = particles.depth_offset;
        self.viewport = particles.surface_size;
        self.order.clear();
        self.order.extend(0..count);
        self.order.sort_by(|&left, &right| {
            let depth = |i: usize| self.quads[i][0].position[2] + self.offsets[i][2];
            let delta = depth(left) - depth(right);
            if delta < 0. {
                Ordering::Less
            } else if delta > 0. {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        });
        Ok(())
    }

    /// 256 表示全不透明；保留 DWORD 乘法的回绕行为。
    pub fn set_opacity(&mut self, opacity: u32) {
        self.opacity = opacity;
    }

    /// 保留逐顶点 RHW、原始 UV 和独立装载深度，不应用普通平面投影。
    pub fn projected_quad(&self, index: usize, view: &Projection) -> Option<[ParticleVertex; 4]> {
        let quad = self.quads.get(index)?;
        let offset = self.offsets[index];
        let center = self.viewport.map(|size| (size >> 1) as f32);
        let identity = std::array::from_fn(|i| if i % 5 == 0 { 1. } else { 0. });
        Some(quad.map(|vertex| {
            let mut point = transform_vector(
                [
                    vertex.position[0] - offset[0],
                    vertex.position[1] - offset[1],
                    vertex.position[2] - offset[2],
                    vertex.rhw - offset[3],
                ],
                identity,
            );
            for axis in 0..2 {
                point[axis] = (point[axis] - center[axis]) * view.coordinate_scale[axis];
            }
            point = transform_vector(point, view.coordinate_matrix);
            point[2] = 65000. - (point[2] + self.depth);
            let distance = if point[2] == self.reference_depth {
                1.
            } else {
                point[2] - self.reference_depth
            };
            let factor = 1000. / distance;
            let mut projection = [0.; 16];
            projection[0] = if factor < 0. { 0. } else { factor };
            projection[5] = projection[0];
            projection[10] = f32::from_bits(0x3781_0e36);
            projection[15] = 1. / distance;
            point = transform_vector(point, projection);
            point[0] += center[0];
            point[1] += center[1];
            point = transform_vector(point, view.matrix);
            for (axis, component) in point.iter_mut().enumerate().take(2) {
                *component += view.translation[axis];
                *component -= view.coordinate_offset[axis];
            }
            let left = ((vertex.color >> 8) & 0xffff_0000).wrapping_mul(self.opacity);
            let right = ((vertex.color >> 8) & 0xff_0000).wrapping_mul(self.opacity);
            ParticleVertex {
                position: [point[0], point[1], point[2]],
                rhw: point[3],
                color: ((left ^ vertex.color) & 0xff_ffff) ^ right,
                uv: vertex.uv,
            }
        }))
    }

    pub(super) fn visit(
        &self,
        view: &Projection,
        blend_mode: u32,
        mut draw: impl FnMut(DrawCommand) -> Result<bool>,
    ) -> Result<bool> {
        if self.opacity == 0 {
            return Ok(false);
        }
        let mut touched = false;
        for &index in &self.order {
            if self.flags[index] == 0 {
                continue;
            }
            let quad = self.projected_quad(index, view).unwrap();
            for indices in [[0, 1, 2], [2, 1, 3]] {
                let vertices = indices.map(|i| ProjectedVertex {
                    position: quad[i].position,
                    rhw: quad[i].rhw,
                    color: quad[i].color,
                });
                if super::raster::triangle_culled(vertices) {
                    continue;
                }
                touched |= draw(DrawCommand {
                    vertices,
                    texture: self.texture.as_ref().map(|image| FrameTexture {
                        source: TextureSource::Image(image.clone()),
                        uv: indices.map(|i| quad[i].uv),
                    }),
                    clip: None,
                    blend_mode,
                })?;
            }
        }
        Ok(touched)
    }
}
