use super::{ImageReference, ObjectAttributes};

/// 二维平面使用的裁剪区域；自身不持有绘图任务。
#[derive(Clone, Debug, PartialEq)]
pub struct ViewportState {
    pub(crate) position: [f32; 2],
    pub(crate) size: [i32; 2],
    pub(crate) position_resource: Option<ImageReference>,
    pub(crate) size_resource: Option<ImageReference>,
    pub(crate) dirty: bool,
    pub(crate) rebuild: bool,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            position: [0.; 2],
            size: [100_000; 2],
            position_resource: None,
            size_resource: None,
            dirty: false,
            rebuild: false,
        }
    }
}

impl ViewportState {
    pub fn position(&self) -> [f32; 2] {
        self.position
    }

    pub fn size(&self) -> [i32; 2] {
        self.size
    }

    pub fn position_resource(&self) -> Option<&ImageReference> {
        self.position_resource.as_ref()
    }

    pub fn size_resource(&self) -> Option<&ImageReference> {
        self.size_resource.as_ref()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn needs_rebuild(&self) -> bool {
        self.rebuild
    }

    /// SIZE2 请求重读元数据；POS2 仅登记一次待消费的偏移。
    pub fn apply(&mut self, attributes: &ObjectAttributes) -> bool {
        let mut dirty = false;
        if attributes.position[..2].iter().any(Option::is_some) {
            let next = std::array::from_fn(|i| {
                attributes.position[i].unwrap_or_else(|| {
                    if (-2147483648.0..2147483648.0).contains(&self.position[i]) {
                        self.position[i] as i32
                    } else {
                        i32::MIN
                    }
                }) as f32
            });
            dirty |= self.position != next;
            self.position = next;
        }
        if let Some(reference) = &attributes.position_resource {
            self.position_resource = Some(ImageReference::parse(reference));
            dirty = true;
        }
        if let Some(size) = attributes.size {
            dirty |= self.size != size;
            self.size = size;
        }
        if let Some(reference) = &attributes.size_resource {
            self.size_resource = Some(ImageReference::parse(reference));
            self.rebuild = true;
        }
        self.dirty |= dirty;
        dirty
    }
}
