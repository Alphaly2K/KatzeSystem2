use super::{DrawCommand, FrameTexture, ProjectedVertex, Projection, Surface, TextureSource};
use crate::host::graphics::triangle_culled;
use crate::kcs::{ParticleHost, ParticleVertex, transform_vector};
use crate::{Error, Result, read};
use std::sync::Arc;

/// 旧式粒子绘制快照；解释器更新与设备提交分别持有自己的网格。
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleMesh {
    quads: Vec<[ParticleVertex; 4]>,
    offsets: Vec<[f32; 4]>,
    depth_shifts: Vec<f32>,
    flags: Vec<u32>,
    order: Vec<usize>,
    texture: Option<Arc<Surface>>,
    address: [super::TextureAddress; 2],
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
            depth_shifts: Vec::new(),
            flags: Vec::new(),
            order: Vec::new(),
            texture,
            address: particles.texture_address,
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
        self.update_with_depth_shifts(particles, &[])
    }

    pub(super) fn update_with_depth_shifts(
        &mut self,
        particles: &ParticleHost,
        depth_shifts: &[f32],
    ) -> Result<()> {
        let count = particles.quads.len();
        if count > read::MAX_BYTES / 244
            || particles.offsets.len() != count
            || particles.quad_flags.len() != count
        {
            return Err(Error::new(0, "粒子绘制网格数组数量不匹配"));
        }
        self.quads.clone_from(&particles.quads);
        self.offsets.clone_from(&particles.offsets);
        self.depth_shifts.clear();
        self.depth_shifts.extend_from_slice(depth_shifts);
        self.flags.clone_from(&particles.quad_flags);
        self.address = particles.texture_address;
        self.depth = particles.depth_offset;
        self.viewport = particles.surface_size;
        self.order.clear();
        self.order.extend(0..count);
        sort_depth(&mut self.order, &|i| {
            let depth = self.quads[i][0].position[2] + self.offsets[i][2];
            // The sort key uses scene depth units; the submitted vertex uses normalized Z.
            self.depth_shifts
                .get(i)
                .map_or(depth, |shift| depth - shift * 65000.)
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
            if let Some(shift) = self.depth_shifts.get(index) {
                point[2] -= shift;
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
                if triangle_culled(vertices) {
                    continue;
                }
                touched |= draw(DrawCommand {
                    vertices,
                    texture: self.texture.as_ref().map(|image| FrameTexture {
                        filter: None,
                        address: self.address,
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

fn sort_depth(mut order: &mut [usize], depth: &impl Fn(usize) -> f32) {
    while order.len() > 1 {
        // 原排序将中点移到分区末位；相等和 NaN 均不进入左分区。
        // 该比较不满足 Rust 标准排序要求的全序，必须保留原分区过程。
        let middle = (order.len() - 1) / 2;
        let pivot = order[middle];
        let pivot_depth = depth(pivot);
        order[middle] = order[0];
        let mut boundary = 0;
        for index in 1..order.len() {
            if depth(order[index]) - pivot_depth < 0. {
                boundary += 1;
                order.swap(boundary, index);
            }
        }
        order[0] = order[boundary];
        order[boundary] = pivot;
        let (left, right) = order.split_at_mut(boundary);
        let right = &mut right[1..];
        // 两分区互不影响，先递归较短者以限制栈深度。
        if left.len() < right.len() {
            sort_depth(left, depth);
            order = right;
        } else {
            sort_depth(right, depth);
            order = left;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_shifts_change_sort_and_submitted_z_without_changing_projection() -> Result<()> {
        let mut host = ParticleHost::default();
        host.reset_quads(3, [16.; 2])?;
        for (quad, depth) in host.quads.iter_mut().zip([0., 100., 200.]) {
            for v in quad {
                v.position[2] = depth;
            }
        }
        let mut mesh = ParticleMesh::new(&host, None, 59000.)?;
        let view = Projection::default();
        let original = mesh.projected_quad(1, &view).unwrap();
        assert_eq!(mesh.order, [0, 1, 2]);
        mesh.update_with_depth_shifts(&host, &[0., 0.003, 0.001])?;
        assert_eq!(mesh.order, [1, 0, 2]);
        for (before, after) in original.iter().zip(mesh.projected_quad(1, &view).unwrap()) {
            assert_eq!(after.position[..2], before.position[..2]);
            assert_eq!(after.position[2], before.position[2] - 0.003);
            assert_eq!(after.rhw, before.rhw);
        }
        mesh.update(&host)?;
        assert_eq!(mesh.order, [0, 1, 2]);
        assert_eq!(mesh.projected_quad(1, &view).unwrap(), original);
        Ok(())
    }

    #[test]
    fn depth_sort_preserves_equal_and_unordered_partition_order() -> Result<()> {
        for (depths, expected) in [
            (vec![0.; 4], vec![1, 2, 0, 3]),
            (vec![2., -1., 1., 0., 2.], vec![1, 3, 2, 0, 4]),
            (
                vec![
                    f32::NAN,
                    2.,
                    -1.,
                    -f32::NAN,
                    1.,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                ],
                vec![3, 0, 6, 2, 4, 1, 5],
            ),
        ] {
            let mut host = ParticleHost::default();
            host.reset_quads(depths.len(), [16.; 2])?;
            for (quad, depth) in host.quads.iter_mut().zip(depths) {
                quad[0].position[2] = depth;
            }
            let mut mesh = ParticleMesh::new(&host, None, 59000.)?;
            assert_eq!(mesh.order, expected);
            mesh.update(&host)?;
            assert_eq!(mesh.order, expected);
        }
        Ok(())
    }
}
