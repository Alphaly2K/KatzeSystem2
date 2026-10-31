use super::{DisplayState, Program, Timeline};
use crate::Result;

/// 图像对象持有的 ANM 状态；帧资源与实际选帧交给图像宿主。
#[derive(Default)]
pub struct ImageAnimation {
    pub timeline: Option<Timeline>,
    pub playing: i32,
    pub reset_time_on_update: bool,
    pub time: u32,
    display: DisplayState,
    frame_change: Option<i32>,
    invocation: Option<bool>,
}

impl ImageAnimation {
    /// 先移除旧解释器，失败后不继续旧动画；保留对象的播放及时间控制。
    pub fn load(&mut self, bytes: &[u8], max_image_id: i32) -> Result<()> {
        self.timeline = None;
        self.invocation = None;
        self.frame_change = None;
        let mut timeline = Timeline::new(Program::parse(bytes)?);
        timeline.interpreter.max_image_id = max_image_id;
        self.display = timeline.interpreter.display;
        self.timeline = Some(timeline);
        Ok(())
    }

    /// 缓存仅在装载成功或整次推进产生变化时更新。
    pub fn display(&self) -> DisplayState {
        self.display
    }

    /// 本次完成的更新需要图像宿主选择的帧 ID；未变化或预算让出时为空。
    pub fn frame_change(&self) -> Option<i32> {
        self.frame_change
    }

    /// true 为本轮完成。has_reset_frame 表示图像包含保留帧 ID 10000。
    pub fn update(&mut self, now: u32, budget: usize, has_reset_frame: bool) -> Result<bool> {
        self.advance(now, budget, has_reset_frame, None)
    }

    pub fn update_with_random(
        &mut self,
        now: u32,
        budget: usize,
        has_reset_frame: bool,
        random: &mut impl FnMut() -> u32,
    ) -> Result<bool> {
        self.advance(now, budget, has_reset_frame, Some(random))
    }

    fn advance(
        &mut self,
        now: u32,
        budget: usize,
        has_reset_frame: bool,
        random: Option<&mut dyn FnMut() -> u32>,
    ) -> Result<bool> {
        self.frame_change = None;
        let Some(timeline) = self.timeline.as_mut() else {
            self.invocation = None;
            return Ok(true);
        };
        if self.invocation.is_none() {
            if self.playing == 0 {
                return Ok(true);
            }
            if self.reset_time_on_update {
                timeline.origin = now;
                self.time = now;
                self.reset_time_on_update = false;
            }
            self.invocation = Some(now == u32::MAX && has_reset_frame);
        }
        let result = match random {
            Some(mut random) => timeline.update_frame_with_random(now, budget, &mut random),
            None => timeline.update_frame(now, budget),
        };
        match result {
            Ok(false) => Ok(false),
            Ok(true) => {
                let reset_frame = self.invocation.take().unwrap();
                if timeline.display_changed() {
                    self.display = timeline.interpreter.display;
                    if reset_frame {
                        self.display.image = 10000;
                    }
                    self.frame_change = Some(self.display.image);
                }
                Ok(true)
            }
            Err(error) => {
                self.invocation = None;
                Err(error)
            }
        }
    }
}
