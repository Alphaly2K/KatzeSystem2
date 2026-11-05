use super::{Vertex, drawable::arithmetic as f};
use crate::kcs::{multiply_matrix, transform_vector};

const IDENTITY: [f32; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

/// 进入栅格化阶段的顶点；RHW 独立保存，不隐式除以 W。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectedVertex {
    pub position: [f32; 3],
    pub rhw: f32,
    pub color: u32,
}

/// 本对象变换之后的投影、绘图坐标组及最终画布变换。
#[derive(Clone, Debug, PartialEq)]
pub struct Projection {
    pub center: [f32; 2],
    pub camera_depth: f32,
    pub translation: [f32; 2],
    pub matrix: [f32; 16],
    pub coordinate_offset: [f32; 2],
    pub coordinate_scale: [f32; 2],
    pub coordinate_matrix: [f32; 16],
}

impl Default for Projection {
    fn default() -> Self {
        Self {
            center: [0.; 2],
            camera_depth: 0.,
            translation: [0.; 2],
            matrix: IDENTITY,
            coordinate_offset: [0.; 2],
            coordinate_scale: [1.; 2],
            coordinate_matrix: IDENTITY,
        }
    }
}

impl Projection {
    /// 负端点先归零，再以 W=0 应用画布矩阵及独立平移；保留端点顺序。
    pub fn project_clip(&self, clip: super::Rect) -> super::Rect {
        let point = |x: i32, y: i32| {
            let value = transform_vector([x.max(0) as f32, y.max(0) as f32, 0., 0.], self.matrix);
            std::array::from_fn::<_, 2, _>(|i| {
                let v = f(1, value[i], self.translation[i]);
                if (-2147483648.0..2147483648.0).contains(&v) {
                    v as i32
                } else {
                    i32::MIN
                }
            })
        };
        let [left, top] = point(clip.left, clip.top);
        let [right, bottom] = point(clip.right, clip.bottom);
        super::Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// origin 为已选择的投影原点；零 Z 或关闭 perspective 使用平面路径。
    pub fn project_triangle(
        &self,
        vertices: [Vertex; 3],
        base_depth: f32,
        origin: [f32; 2],
        perspective: bool,
    ) -> [ProjectedVertex; 3] {
        let distance = f(2, f(2, 65000., base_depth), self.camera_depth);
        vertices.map(|vertex| {
            let [x, y, z] = vertex.position;
            let mut point;
            if perspective && z != 0. {
                let denominator = if z == distance { 1. } else { f(2, z, distance) };
                let scale = f(4, self.camera_depth, denominator);
                let scale = if scale < 0. { 0. } else { scale };
                let mut matrix = [0.; 16];
                matrix[0] = scale;
                matrix[5] = scale;
                matrix[10] = f32::from_bits(0x3781_0e36);
                matrix[15] = f(4, 1., denominator);
                matrix = multiply_matrix(IDENTITY, matrix);
                point = transform_vector([f(2, x, origin[0]), f(2, y, origin[1]), z, 1.], matrix);
                for (i, origin) in origin.into_iter().enumerate() {
                    point[i] = f(1, point[i], origin);
                    point[i] = f(2, point[i], self.center[i]);
                    point[i] = f(3, self.coordinate_scale[i], point[i]);
                }
                point = transform_vector(point, self.coordinate_matrix);
            } else {
                let depth = f(
                    3,
                    f(2, 65000., f(1, z, base_depth)),
                    f32::from_bits(0x3781_0e36),
                );
                point = transform_vector(
                    [
                        f(3, f(2, x, self.center[0]), self.coordinate_scale[0]),
                        f(3, f(2, y, self.center[1]), self.coordinate_scale[1]),
                        0.,
                        1.,
                    ],
                    self.coordinate_matrix,
                );
                point[2] = depth;
            }
            for (i, component) in point.iter_mut().enumerate().take(2) {
                *component = f(
                    1,
                    f(2, self.center[i], self.coordinate_offset[i]),
                    *component,
                );
            }
            point = transform_vector(point, self.matrix);
            point[0] = f(1, point[0], self.translation[0]);
            point[1] = f(1, point[1], self.translation[1]);
            ProjectedVertex {
                position: [point[0], point[1], point[2]],
                rhw: point[3],
                color: vertex.color,
            }
        })
    }
}
