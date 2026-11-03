/// 设备坐标到逻辑画布的两级换算；画布外坐标仍保留。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoordinateMap {
    pub device_offset: [f32; 2],
    pub device_scale: f32,
    pub configuration_offset: [i32; 2],
    pub logical_scale: [f32; 2],
    /// 反向转换使用独立配置值，不自动取 logical_scale 的倒数。
    pub device_units_per_logical: [f32; 2],
    pub logical_size: [i32; 2],
}

impl Default for CoordinateMap {
    fn default() -> Self {
        Self {
            device_offset: [0.; 2],
            device_scale: 1.,
            configuration_offset: [0; 2],
            logical_scale: [1.; 2],
            device_units_per_logical: [1.; 2],
            logical_size: [0; 2],
        }
    }
}

impl CoordinateMap {
    /// 返回换算后的点及是否位于逻辑范围内；端点排除右下边。
    pub fn to_logical(&self, point: [i32; 2]) -> ([i32; 2], bool) {
        let point = std::array::from_fn(|i| self.logical_axis(i, point[i]));
        let inside = (0..2).all(|i| point[i] >= 0 && point[i] < self.logical_size[i]);
        (point, inside)
    }

    pub(crate) fn logical_axis(&self, axis: usize, point: i32) -> i32 {
        let inverse = 1.0 / self.device_scale;
        let shifted = point as f32 - self.device_offset[axis];
        let device = truncate(shifted * inverse);
        let configured = device.wrapping_sub(self.configuration_offset[axis]);
        truncate(configured as f32 * self.logical_scale[axis])
    }

    pub(crate) fn truncated_device_offset(&self) -> [i32; 2] {
        self.device_offset.map(truncate)
    }

    /// 逻辑点先作范围判断，再转换为设备位置；画布外点仍完成换算。
    pub fn to_device(&self, point: [i32; 2]) -> ([i32; 2], bool) {
        let inside = (0..2).all(|i| point[i] >= 0 && point[i] < self.logical_size[i]);
        let inverse = 1.0 / self.device_scale;
        let point = std::array::from_fn(|i| {
            let scaled = truncate(point[i] as f32 * self.device_units_per_logical[i]);
            let configured = scaled.wrapping_add(self.configuration_offset[i]);
            let device = configured as f32 / inverse;
            truncate(device + self.device_offset[i])
        });
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
