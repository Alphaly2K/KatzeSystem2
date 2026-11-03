use super::KeySample;
use crate::Result;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerSample {
    pub position: [i32; 2],
    pub active: u32,
}

/// 同步设备服务；每次查询均可观察前一次调用或脚本回调的修改。
pub trait InputDevice {
    fn poll(&mut self) -> Result<()>;
    fn pointer(&mut self) -> Result<PointerSample>;
    /// group 为绑定组 0～3；repeat 表示是否需要首次重复开关，掩码查询仍会执行。
    fn key(&mut self, key: u32, group: u32, repeat: bool) -> Result<KeySample>;
    fn take_wheel(&mut self) -> Result<u32>;
    fn is_foreground(&mut self) -> Result<bool>;
    fn finish(&mut self) -> Result<()>;
    fn commit(&mut self) -> Result<()>;
    fn history(&mut self) -> Result<[u8; 32]>;
    fn show_cursor(&mut self, visible: u32) -> Result<()>;
    fn cursor_visible(&mut self) -> Result<u32>;
    fn set_pointer(&mut self, position: [i32; 2]) -> Result<()>;
    fn clear_keys(&mut self) -> Result<()>;
    /// 清空触摸接触点映射和活动计数，保留鼠标及按键状态。
    fn clear_touches(&mut self) -> Result<()> {
        Err(crate::Error::new(0, "输入设备未提供触摸重置"))
    }
}

/// 输入任务的原始设备位置、拖动起点和光标状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointerState {
    pub position: [i32; 2],
    pub origin: [i32; 2],
    pub active: u32,
    pub move_started: u32,
    pub move_remaining: i32,
    pub move_target: [i32; 2],
    pub cursor_requested: u32,
    pub cursor_shown: u32,
    pub auto_hide: u32,
    pub idle_limit: u32,
    pub idle_count: u32,
    pub forced: u32,
    pub drag_threshold: u32,
    pub history: [u8; 32],
}

impl Default for PointerState {
    fn default() -> Self {
        Self {
            position: [0; 2],
            origin: [0; 2],
            active: 0,
            move_started: 0,
            move_remaining: 0,
            move_target: [0; 2],
            cursor_requested: 1,
            cursor_shown: 1,
            auto_hide: 0,
            idle_limit: 0,
            idle_count: 0,
            forced: 0,
            drag_threshold: 2,
            history: [0; 32],
        }
    }
}
