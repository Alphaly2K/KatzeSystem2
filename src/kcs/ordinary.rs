use super::{Host, Interpreter, Program, TaskClock};
use crate::task::Control;
use crate::{Error, Result, Step, read};

enum UpdatePhase {
    Messages(usize),
    Resume(usize),
}

enum CommandPhase {
    Message(usize),
    Delete([u32; 4]),
}

/// 已装载的普通 KCS 任务；子任务树与对象命令由外层宿主管理。
pub struct OrdinaryTask {
    pub interpreter: Interpreter,
    pub clock: TaskClock,
    pub control: Control,
    pub creation_context: Vec<u8>,
    pub running: i32,
    pub task_id: u32,
    pub accumulated_increment: u32,
    messages: Vec<[u32; 4]>,
    phase: Option<UpdatePhase>,
    command_phase: Option<CommandPhase>,
}

impl OrdinaryTask {
    pub fn new(program: Program, external: Vec<u8>) -> Result<Self> {
        Ok(Self {
            interpreter: Interpreter::new(program, external)?,
            clock: TaskClock::default(),
            control: Control::default(),
            creation_context: Vec::new(),
            running: 1,
            task_id: 0,
            accumulated_increment: 0,
            messages: Vec::new(),
            phase: None,
            command_phase: None,
        })
    }

    pub fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.require_idle()?;
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
        self.phase.is_some() || self.command_phase.is_some()
    }

    fn require_idle(&self) -> Result<()> {
        if self.is_pending() {
            return Err(Error::new(self.interpreter.pc, "普通任务执行尚未完成"));
        }
        Ok(())
    }

    fn synchronize(&self, host: &mut impl Host) {
        host.set_task_clock(&self.clock);
        host.set_task_id(self.task_id);
        host.set_task_context(&self.creation_context);
    }

    fn active_result(&self, step: &Step<super::Event>, entry_pc: usize) -> bool {
        // 取指过程中到达代码末尾仍返回活动，下一次从末尾进入才停止。
        !matches!(step, Step::Halted)
            || (self.interpreter.pc == self.interpreter.program.code_len
                && entry_pc != self.interpreter.pc)
    }

    /// 对应命令 162；source 为第一数据项，与当前任务编号比较。
    pub fn start_execution(&mut self, source: u32, mode: u32) -> Result<()> {
        self.require_idle()?;
        if self.control.deletion_requested == 0 {
            self.running = 1;
            self.interpreter
                .set_execution_mode(if source == self.task_id { mode } else { 1 });
        }
        Ok(())
    }

    /// 对应命令 163，同时关闭任务运行和 VM 普通恢复。
    pub fn stop_execution(&mut self) -> Result<()> {
        self.require_idle()?;
        if self.control.deletion_requested == 0 {
            self.running = 0;
            self.interpreter.set_execution_mode(0);
        }
        Ok(())
    }

    /// 同步消息分派；false 表示继续传入同一消息和 output 续跑。
    pub fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut impl Host,
        budget: usize,
    ) -> Result<bool> {
        let entry_pc = match self.command_phase {
            Some(CommandPhase::Message(pc)) => pc,
            _ => {
                self.require_idle()?;
                if self.control.deletion_requested != 0 {
                    return Ok(true);
                }
                let pc = self.interpreter.pc;
                self.command_phase = Some(CommandPhase::Message(pc));
                pc
            }
        };
        self.synchronize(host);
        let step = self.interpreter.dispatch_message(message, host, budget)?;
        if matches!(step, Some(Step::Yield)) {
            return Ok(false);
        }
        *output = step
            .as_ref()
            .is_some_and(|step| self.active_result(step, entry_pc)) as u32;
        self.command_phase = None;
        Ok(true)
    }

    /// 先尝试同步删除通知，再标记删除；false 表示通知仍需预算续跑。
    pub fn request_delete(&mut self, host: &mut impl Host, budget: usize) -> Result<bool> {
        let mut message = match self.command_phase {
            Some(CommandPhase::Delete(message)) => message,
            _ => {
                self.require_idle()?;
                if self.control.deletion_requested != 0 {
                    return Ok(true);
                }
                [u32::MAX, 0xffff_0006, 0, 0]
            }
        };
        self.synchronize(host);
        self.command_phase = Some(CommandPhase::Delete(message));
        let step = self
            .interpreter
            .dispatch_message(&mut message, host, budget)?;
        if matches!(step, Some(Step::Yield)) {
            self.command_phase = Some(CommandPhase::Delete(message));
            return Ok(false);
        }
        self.control.deletion_requested = 1;
        self.command_phase = None;
        Ok(true)
    }

    /// 按入队顺序分派消息后进行一次普通恢复；false 表示预算续跑。
    pub fn update(
        &mut self,
        now: u32,
        increment: u32,
        host: &mut impl Host,
        budget_per_call: usize,
    ) -> Result<bool> {
        if self.command_phase.is_some() {
            return Err(Error::new(self.interpreter.pc, "普通任务命令尚未完成"));
        }
        if self.phase.is_none() {
            self.clock.update(now);
            if self.control.deletion_requested != 0 || self.running == 0 {
                return Ok(true);
            }
            self.accumulated_increment = self.accumulated_increment.wrapping_add(increment);
            self.phase = Some(UpdatePhase::Messages(0));
        }
        self.synchronize(host);
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
        self.running = self.active_result(&step, entry_pc) as i32;
        self.phase = None;
        Ok(true)
    }
}
