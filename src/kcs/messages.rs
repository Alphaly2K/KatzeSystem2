use super::{Event, Host, Interpreter};
use crate::{Error, Result, Step, read};

#[derive(Default)]
pub(super) struct MessageState {
    address: Option<u32>,
    waiting: bool,
    delivering: bool,
}

impl Interpreter {
    pub fn wait_for_message(&mut self, address: u32) -> Result<()> {
        self.read(address, 16)?;
        self.messages.address = Some(address);
        self.messages.waiting = true;
        Ok(())
    }

    pub fn is_waiting_for_message(&self) -> bool {
        self.messages.waiting
    }

    /// 普通任务的一次无消息推进；预算续跑继续使用本入口。
    pub fn resume(&mut self, host: &mut impl Host, budget: usize) -> Result<Step<Event>> {
        if self.messages.delivering {
            return Err(Error::new(self.pc, "KCS 消息分派尚未完成"));
        }
        if self.execution_mode == 0 {
            return Ok(Step::Paused);
        }
        if self.messages.waiting {
            self.push(0)?;
            self.messages.waiting = false;
        }
        self.run(host, budget)
    }

    /// 向等待中的任务分派四个 DWORD；预算让出后继续传入同一消息。
    pub fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        host: &mut impl Host,
        budget: usize,
    ) -> Result<Option<Step<Event>>> {
        if !self.messages.delivering {
            if !self.messages.waiting {
                return Ok(None);
            }
            let address = self.messages.address.unwrap();
            for (i, value) in message.iter().enumerate() {
                self.write(address + i as u32 * 4, &value.to_le_bytes())?;
            }
            self.push(1)?;
            self.messages.waiting = false;
            self.messages.delivering = true;
        }
        let result = self.run(host, budget);
        if matches!(result, Ok(Step::Yield)) {
            return result.map(Some);
        }
        self.messages.delivering = false;
        let step = result?;
        // 恢复后可能再次等待，原入口从最新的接收地址回写第二项。
        message[1] = read::u32_at(&self.read(self.messages.address.unwrap() + 4, 4)?, 0)?;
        Ok(Some(step))
    }
}
