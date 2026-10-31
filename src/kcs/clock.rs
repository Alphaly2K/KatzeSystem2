/// 普通任务的整数时钟；原始时刻由外层调度器提供。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskClock {
    pub origin: u32,
    pub previous_time: u32,
    pub time: u32,
    pub paused: i32,
    pub one_tick_pending: bool,
}

impl TaskClock {
    pub fn elapsed(&self) -> u32 {
        self.time.wrapping_sub(self.origin)
    }

    pub fn update(&mut self, now: u32) {
        let delta = now.wrapping_sub(self.previous_time);
        if self.paused != 0 {
            self.one_tick_pending = false;
            self.origin = self.origin.wrapping_add(delta);
        } else {
            if self.one_tick_pending {
                self.one_tick_pending = false;
                self.origin = self.origin.wrapping_add(delta.wrapping_sub(1));
            }
            self.time = now;
        }
        self.previous_time = now;
    }
}
