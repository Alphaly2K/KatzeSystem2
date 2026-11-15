fn length(vector: [f32; 3]) -> f32 {
    let squares = vector.map(|n| n * n);
    (((squares[0] + squares[1]) + squares[2]) as f64).sqrt() as f32
}

pub(super) struct TwoPointPath {
    pub(super) points: [[f32; 3]; 2],
    pub(super) tangent: [f32; 3],
    pub(super) length: f32,
    pub(super) duration: f32,
}

impl TwoPointPath {
    pub(super) fn new(points: [[f32; 3]; 2], duration: f32, strength: f32) -> Self {
        let delta = std::array::from_fn(|i| points[1][i] - points[0][i]);
        let length = length(delta);
        let tangent = if duration == 0. {
            [0.; 3]
        } else {
            delta.map(|n| (n / length) * strength)
        };
        Self {
            points,
            tangent,
            length,
            duration,
        }
    }

    pub(super) fn at(&self, mut time: f32) -> [f32; 3] {
        if time <= 0. {
            return self.points[0];
        }
        if time >= self.duration {
            time = self.duration;
        }
        let distance = (time / self.duration) * self.length;
        if self.length <= distance || distance.is_nan() {
            return self.points[1];
        }
        let t = (distance - 0.) / self.length;
        let square = t * t;
        let cube = square * t;
        let basis = [
            [2., -2., 1., 1.],
            [-3., 3., -2., -1.],
            [0., 0., 1., 0.],
            [1., 0., 0., 0.],
        ];
        let coefficients = basis.map(|row| {
            std::array::from_fn::<_, 3, _>(|i| {
                ((self.points[0][i] * row[0] + self.points[1][i] * row[1])
                    + self.tangent[i] * row[2])
                    + self.tangent[i] * row[3]
            })
        });
        std::array::from_fn(|i| {
            ((coefficients[0][i] * cube + coefficients[1][i] * square) + coefficients[2][i] * t)
                + coefficients[3][i]
        })
    }
}
