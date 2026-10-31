use super::{DisplayState, Event, Interpreter, Program};
use crate::{Result, Step};

struct Update {
    remaining: i32,
    before: DisplayState,
}

/// 按基线的整数时钟推进动画；同次预算续跑保留已换算的剩余时间。
pub struct Timeline {
    pub interpreter: Interpreter,
    pub speed: f32,
    pub origin: u32,
    pub duration: i32,
    invocation: Option<Update>,
    finished: bool,
    changed: bool,
}

impl Timeline {
    pub fn new(program: Program) -> Self {
        Self {
            interpreter: Interpreter::new(program),
            speed: 1.0,
            origin: 0,
            duration: 0,
            invocation: None,
            finished: false,
            changed: false,
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn is_pending(&self) -> bool {
        self.invocation.is_some()
    }

    pub fn display_changed(&self) -> bool {
        self.changed
    }

    /// 从入口重启并恢复默认显示，保留变量、速度和时间基点。
    pub fn restart(&mut self) {
        self.interpreter.pc = 0;
        self.interpreter.display = DisplayState::default();
        self.duration = 0;
        self.invocation = None;
        self.finished = false;
        self.changed = false;
    }

    /// 对象时钟以零基点表示未开始，u32::MAX 使用当前基点。
    pub fn update_frame(&mut self, now: u32, budget: usize) -> Result<bool> {
        let now = self.frame_time(now);
        self.advance(now, budget, None)
    }

    pub fn update_frame_with_random(
        &mut self,
        now: u32,
        budget: usize,
        random: &mut impl FnMut() -> u32,
    ) -> Result<bool> {
        let now = self.frame_time(now);
        self.advance(now, budget, Some(random))
    }

    fn frame_time(&mut self, now: u32) -> u32 {
        if self.invocation.is_some() {
            return now;
        }
        let now = if now == u32::MAX { self.origin } else { now };
        if self.origin == 0 {
            self.origin = now;
        }
        now
    }

    /// true 为本轮完成，false 为预算让出；显示状态从 interpreter.display 读取。
    pub fn update(&mut self, now: u32, budget: usize) -> Result<bool> {
        self.advance(now, budget, None)
    }

    pub fn update_with_random(
        &mut self,
        now: u32,
        budget: usize,
        random: &mut impl FnMut() -> u32,
    ) -> Result<bool> {
        self.advance(now, budget, Some(random))
    }

    fn advance(
        &mut self,
        now: u32,
        budget: usize,
        mut random: Option<&mut dyn FnMut() -> u32>,
    ) -> Result<bool> {
        if self.invocation.is_none() {
            self.changed = false;
            if self.finished {
                return Ok(true);
            }
            let scaled = now.wrapping_sub(self.origin) as i32 as f32 * self.speed;
            let remaining = if (-2147483648.0..2147483648.0).contains(&scaled) {
                scaled as i32
            } else {
                i32::MIN
            };
            if remaining < self.duration {
                return Ok(true);
            }
            self.invocation = Some(Update {
                remaining,
                before: self.interpreter.display,
            });
        }
        for _ in 0..budget {
            let result = match random.as_mut() {
                Some(random) => self.interpreter.advance(1, Some(&mut **random)),
                None => self.interpreter.advance(1, None),
            };
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    self.invocation = None;
                    return Err(error);
                }
            };
            match result {
                Step::Event(Event::Image { duration, .. } | Event::Wait { duration }) => {
                    self.origin = self.origin.wrapping_add(self.duration as u32);
                    let update = self.invocation.as_mut().unwrap();
                    update.remaining = update.remaining.wrapping_sub(self.duration);
                    self.duration = duration;
                }
                Step::Halted => self.finished = true,
                Step::Yield | Step::Paused => (),
            }
            if self.finished || self.invocation.as_ref().unwrap().remaining < self.duration {
                self.changed = self.invocation.take().unwrap().before != self.interpreter.display;
                return Ok(true);
            }
        }
        Ok(false)
    }
}
