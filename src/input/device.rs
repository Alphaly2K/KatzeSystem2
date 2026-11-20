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
