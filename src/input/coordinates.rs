/// 设备坐标到逻辑画布的两级换算；画布外坐标仍保留。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoordinateMap {
    pub device_offset: [f32; 2],
    pub device_scale: f32,
    pub configuration_offset: [i32; 2],
    pub logical_scale: [f32; 2],
    pub logical_size: [i32; 2],
}

impl Default for CoordinateMap {
    fn default() -> Self {
        Self {
            device_offset: [0.; 2],
            device_scale: 1.,
            configuration_offset: [0; 2],
            logical_scale: [1.; 2],
            logical_size: [0; 2],
        }
    }
}

impl CoordinateMap {
    /// 返回换算后的点及是否位于逻辑范围内；端点排除右下边。
    pub fn to_logical(&self, point: [i32; 2]) -> ([i32; 2], bool) {
        let inverse = 1.0 / self.device_scale;
        let point = std::array::from_fn(|i| {
            let shifted = point[i] as f32 - self.device_offset[i];
            let device = truncate(shifted * inverse);
            let configured = device.wrapping_sub(self.configuration_offset[i]);
            truncate(configured as f32 * self.logical_scale[i])
        });
        let inside = (0..2).all(|i| point[i] >= 0 && point[i] < self.logical_size[i]);
        (point, inside)
    }
}

fn truncate(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}
