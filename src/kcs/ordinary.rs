use super::{Host, Interpreter, Program, TaskClock};
use crate::{Error, Result, Step, read};

enum UpdatePhase {
    Messages(usize),
    Resume(usize),
}

/// 已装载的普通 KCS 任务；子任务树与对象命令由外层宿主管理。
pub struct OrdinaryTask {
    pub interpreter: Interpreter,
    pub clock: TaskClock,
    pub running: i32,
    pub task_id: u32,
    pub accumulated_increment: u32,
    messages: Vec<[u32; 4]>,
    phase: Option<UpdatePhase>,
}

impl OrdinaryTask {
    pub fn new(program: Program, external: Vec<u8>) -> Result<Self> {
        Ok(Self {
            interpreter: Interpreter::new(program, external)?,
            clock: TaskClock::default(),
            running: 1,
            task_id: 0,
            accumulated_increment: 0,
            messages: Vec::new(),
            phase: None,
        })
    }

    pub fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        if self.phase.is_some() {
            return Err(Error::new(self.interpreter.pc, "普通任务更新尚未完成"));
        }
        if self.messages.len() >= read::MAX_ITEMS {
            return Err(Error::new(self.interpreter.pc, "普通任务消息数量超过上限"));
        }
        self.messages.push(message);
        Ok(())
    }

    pub fn queued_messages(&self) -> &[[u32; 4]] {
        &self.messages
    }

    pub fn is_pending(&self) -> bool {
        self.phase.is_some()
    }

    /// 按入队顺序分派消息后进行一次普通恢复；false 表示预算续跑。
    pub fn update(
        &mut self,
        now: u32,
        increment: u32,
        host: &mut impl Host,
        budget_per_call: usize,
    ) -> Result<bool> {
        if self.phase.is_none() {
            self.clock.update(now);
            if self.running == 0 {
                return Ok(true);
            }
            self.accumulated_increment = self.accumulated_increment.wrapping_add(increment);
            self.phase = Some(UpdatePhase::Messages(0));
        }
        host.set_task_clock(&self.clock);
        host.set_task_id(self.task_id);
        while let Some(UpdatePhase::Messages(index)) = self.phase {
            if index == self.messages.len() {
                self.messages.clear();
                self.phase = Some(UpdatePhase::Resume(self.interpreter.pc));
                break;
            }
            let step = self.interpreter.dispatch_message(
                &mut self.messages[index],
                host,
                budget_per_call,
            )?;
            if matches!(step, Some(Step::Yield)) {
                return Ok(false);
            }
            self.phase = Some(UpdatePhase::Messages(index + 1));
        }
        let step = self.interpreter.resume(host, budget_per_call)?;
        if matches!(step, Step::Yield) {
            return Ok(false);
        }
        let Some(UpdatePhase::Resume(entry_pc)) = self.phase else {
            unreachable!()
        };
        // 取指过程中到达代码末尾仍返回活动，下一轮从末尾进入才停止。
        let reached_end = self.interpreter.pc == self.interpreter.program.code_len
            && entry_pc != self.interpreter.pc;
        self.running = (!matches!(step, Step::Halted) || reached_end) as i32;
        self.phase = None;
        Ok(true)
    }
}
