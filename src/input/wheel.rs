/// 将垂直滚轮消息转换为鼠标 6／7 按键及逐帧重复。
#[derive(Default)]
pub struct WheelButtons {
    live: [bool; 2],
    direction: u32,
    pending: u32,
}

impl WheelButtons {
    /// 每条平台滚轮消息立即按下；同向连续消息累加延迟重复。
    pub fn scroll(&mut self, up: bool, mut emit: impl FnMut(u32, u32)) {
        let index = usize::from(!up);
        let code = 6 + index as u32;
        self.live[index] = true;
        emit(code, 1);
        if self.direction == code {
            self.pending = self.pending.wrapping_add(1);
        } else {
            self.pending = 0;
            self.direction = code;
        }
    }

    /// 在逻辑键扫描后、设备 commit 前调用，释放脉冲并推进延迟重复。
    pub fn finish(&mut self, mut emit: impl FnMut(u32, u32)) {
        for index in 0..2 {
            let code = 6 + index as u32;
            if self.live[index] {
                self.live[index] = false;
                emit(code, 0);
            } else if self.direction == code {
                if self.pending as i32 > 0 {
                    self.pending -= 1;
                }
                self.live[index] = self.pending as i32 > 0;
                emit(code, u32::from(self.live[index]));
            }
        }
    }

    /// 保留已经按下的脉冲及其后续释放，只清除重复方向和计数。
    pub fn cancel_repeat(&mut self) {
        self.direction = 0;
        self.pending = 0;
    }
}
