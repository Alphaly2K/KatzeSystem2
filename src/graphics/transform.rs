use crate::kcs::{float_operation, multiply_matrix, transform_vector};

const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// 绘图对象的基点、平移、缩放和旋转；基点 Z 独立于顶点变换。
#[derive(Clone, Debug, PartialEq)]
pub struct Transform {
    base: [f32; 3],
    position: [f32; 3],
    scale: [f32; 3],
    rotation: [f32; 3],
    matrix_override: Option<[f32; 16]>,
    matrix: [f32; 16],
    dirty: bool,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            base: [0.0, 0.0, 5000.0],
            position: [0.0; 3],
            scale: [1.0; 3],
            rotation: [0.0; 3],
            matrix_override: None,
            matrix: IDENTITY,
            dirty: false,
        }
    }
}

impl Transform {
    pub fn base_point(&self) -> [f32; 3] {
        self.base
    }

    pub fn position(&self) -> [f32; 3] {
        self.position
    }

    pub fn scale(&self) -> [f32; 3] {
        self.scale
    }

    pub fn rotation(&self) -> [f32; 3] {
        self.rotation
    }

    pub fn matrix(&self) -> &[f32; 16] {
        &self.matrix
    }

    pub fn matrix_override(&self) -> Option<&[f32; 16]> {
        self.matrix_override.as_ref()
    }

    pub(crate) fn rotation_matrix(&self) -> [f32; 16] {
        if let Some(matrix) = self.matrix_override {
            return matrix;
        }
        let mut rotation = self.clone();
        rotation.scale = [1.; 3];
        rotation.rebuild_matrix();
        rotation.matrix
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// 浮点比较相等时保留整组原值，包括正负零位模式。
    pub fn set_base_point(&mut self, value: [f32; 3]) {
        if self.base != value {
            self.base = value;
            self.dirty = true;
        }
    }

    pub fn set_position(&mut self, value: [f32; 3]) {
        if self.position != value {
            self.position = value;
            self.dirty = true;
        }
    }

    pub fn set_scale(&mut self, value: [f32; 3]) {
        if self.scale != value {
            self.scale = value;
            self.dirty = true;
            self.rebuild_matrix();
        }
    }

    /// 设置弧度旋转；即使角度相等，也退出显式矩阵模式。
    pub fn set_rotation(&mut self, value: [f32; 3]) {
        if self.rotation != value || self.matrix_override.is_some() {
            self.rotation = value;
            self.matrix_override = None;
            self.dirty = true;
            self.rebuild_matrix();
        }
    }

    /// 矩阵按位比较，替换旋转部分；缩放仍先于此矩阵执行。
    pub fn set_matrix(&mut self, value: [f32; 16]) {
        if self.matrix_override.as_ref().map(|m| m.map(f32::to_bits))
            != Some(value.map(f32::to_bits))
        {
            self.matrix_override = Some(value);
            self.dirty = true;
            self.rebuild_matrix();
        }
    }

    /// 基点只减 X／Y，变换后加四维平移，W 也加一；不作 W 除法。
    pub fn transform_point(&self, point: [f32; 3]) -> [f32; 4] {
        self.transform_point_inner(point, None)
    }

    fn transform_point_inner(
        &self,
        point: [f32; 3],
        wave: Option<super::vertex_wave::VertexWave>,
    ) -> [f32; 4] {
        let mut position = [
            arithmetic(2, point[0], self.base[0]),
            arithmetic(2, point[1], self.base[1]),
            point[2],
        ];
        if let Some(wave) = wave {
            position = wave.apply(position);
        }
        let point = [position[0], position[1], position[2], 1.];
        let point = transform_vector(point, self.matrix);
        [
            arithmetic(1, point[0], self.position[0]),
            arithmetic(1, self.position[1], point[1]),
            arithmetic(1, self.position[2], point[2]),
            arithmetic(1, 1.0, point[3]),
        ]
    }

    /// 在本对象变换之后应用父级／绘图上下文矩阵，保留前一步产生的 W。
    pub fn transform_point_with_parent(&self, point: [f32; 3], parent: [f32; 16]) -> [f32; 4] {
        transform_vector(self.transform_point(point), parent)
    }

    pub(super) fn transform_point_with_wave(
        &self,
        point: [f32; 3],
        parent: [f32; 16],
        wave: Option<super::vertex_wave::VertexWave>,
    ) -> [f32; 4] {
        transform_vector(self.transform_point_inner(point, wave), parent)
    }

    fn rebuild_matrix(&mut self) {
        let mut scaling = IDENTITY;
        for i in 0..3 {
            scaling[i * 5] = self.scale[i];
        }
        let mut matrix = multiply_matrix(IDENTITY, scaling);
        if let Some(value) = self.matrix_override {
            self.matrix = multiply_matrix(matrix, value);
            return;
        }
        for (axis, angle) in self.rotation.into_iter().enumerate() {
            let (sine, cosine) = sin_cos(angle);
            let mut rotation = IDENTITY;
            match axis {
                0 => {
                    rotation[5] = cosine;
                    rotation[6] = sine;
                    rotation[9] = -sine;
                    rotation[10] = cosine;
                }
                1 => {
                    rotation[0] = cosine;
                    rotation[2] = -sine;
                    rotation[8] = sine;
                    rotation[10] = cosine;
                }
                _ => {
                    rotation[0] = cosine;
                    rotation[1] = sine;
                    rotation[4] = -sine;
                    rotation[5] = cosine;
                }
            }
            matrix = multiply_matrix(matrix, rotation);
        }
        self.matrix = matrix;
    }
}

fn arithmetic(op: u8, left: f32, right: f32) -> f32 {
    f32::from_bits(float_operation(op, left.to_bits(), right.to_bits()))
}

pub(super) use crate::kcs::matrix_sin_cos as sin_cos;
