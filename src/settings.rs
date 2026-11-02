/// 普通 359／360 使用的全局配置块，按基线字节布局保存。
///
/// 由配置宿主提供初值；写回只修改配置，不触发设备或消息对象更新。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemSettings {
    pub sound: [u8; 0x15fc],
    pub message: [u8; 0x48],
    pub effects: [u8; 0x20],
    pub window: [u8; 0x0c],
}

impl SystemSettings {
    pub(crate) fn block_mut(&mut self, index: usize) -> &mut [u8] {
        match index {
            0 => &mut self.sound,
            1 => &mut self.message,
            2 => &mut self.effects,
            3 => &mut self.window,
            _ => unreachable!(),
        }
    }
}
