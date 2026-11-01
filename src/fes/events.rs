use super::{Event, Interpreter};
use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventContext {
    pub object: String,
    pub bank: i32,
    pub event: String,
}

pub(super) struct Invocation {
    pub context: EventContext,
    main: (usize, usize),
    pending: Option<Event>,
    depth: usize,
    object_block: Option<usize>,
    default: bool,
    pub(super) system: bool,
}

impl Interpreter {
    pub fn event_context(&self) -> Option<&EventContext> {
        self.invocation.as_ref().map(|i| &i.context)
    }

    /// 由对象宿主提供目标、银行号及默认事件组；返回是否存在可执行处理器。
    pub fn dispatch_event(
        &mut self,
        object: &str,
        bank: i32,
        event: &str,
        default_handler: Option<&str>,
    ) -> Result<bool> {
        if !matches!(
            event,
            "FOCUS"
                | "UNFOCUS"
                | "PUSH_L"
                | "PUSH_R"
                | "RELEASE_L"
                | "RELEASE_R"
                | "SCRL_P"
                | "TOUCH"
        ) {
            return Err(Error::new(0, "未支持的 FES 对象事件"));
        }
        if self.invocation.is_some()
            || !self.definitions.is_empty()
            || self.pre_declarations.is_some()
            || !self.initializers.is_empty()
            || self.declarations.is_some()
        {
            return Err(Error::new(0, "FES 无法在当前状态分派事件"));
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|e| !matches!(e, Event::Wait(_) | Event::Frame))
        {
            return Err(Error::new(0, "FES 需要先完成当前宿主操作"));
        }
        let find = |name: &str| {
            self.program
                .blocks
                .iter()
                .position(|b| b.name == format!("{name}.{event}"))
        };
        let object_block = find(object);
        let default_block = default_handler.filter(|s| !s.is_empty()).and_then(find);
        let Some(block) = default_block.or(object_block) else {
            return Ok(false);
        };
        self.invocation = Some(Invocation {
            context: EventContext {
                object: object.to_owned(),
                bank,
                event: event.to_owned(),
            },
            main: (self.block, self.pc),
            pending: self.pending.take(),
            depth: self.calls.len(),
            object_block,
            default: default_block.is_some(),
            system: false,
        });
        self.dispatch_result = None;
        self.block = block;
        self.pc = 0;
        Ok(true)
    }

    pub(crate) fn dispatch_system_block(&mut self, name: &str) -> Result<bool> {
        if self.invocation.is_some() || self.initialization_pending() {
            return Err(Error::new(0, "FES 无法在当前状态分派系统块"));
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|event| !matches!(event, Event::Wait(_) | Event::Frame))
        {
            return Err(Error::new(0, "FES 需要先完成当前宿主操作"));
        }
        let Some(block) = self
            .program
            .blocks
            .iter()
            .position(|block| block.name == name)
        else {
            return Ok(false);
        };
        self.invocation = Some(Invocation {
            context: EventContext {
                object: String::new(),
                bank: 0,
                event: name.to_owned(),
            },
            main: (self.block, self.pc),
            pending: self.pending.take(),
            depth: self.calls.len(),
            object_block: Some(block),
            default: false,
            system: true,
        });
        self.dispatch_result = None;
        self.block = block;
        self.pc = 0;
        Ok(true)
    }

    pub(crate) fn finish_system_block(&mut self) -> Result<()> {
        if !self
            .invocation
            .as_ref()
            .is_some_and(|invocation| invocation.system)
        {
            return Err(Error::new(0, "FES 没有正在执行的系统块"));
        }
        self.finish_event(false);
        Ok(())
    }

    pub(crate) fn take_dispatch_result(&mut self) -> Option<bool> {
        self.dispatch_result.take()
    }

    pub(super) fn event_block_ended(&mut self) -> bool {
        let invocation = self.invocation.as_mut().unwrap();
        if invocation.default {
            invocation.default = false;
            if let Some(block) = invocation.object_block {
                self.block = block;
                self.pc = 0;
                return false;
            }
        } else if self.calls.len() > invocation.depth {
            (self.block, self.pc) = self.calls.pop().unwrap();
            return self.event_transferred();
        }
        self.finish_event(false);
        true
    }

    pub(super) fn event_transferred(&mut self) -> bool {
        if self.invocation.as_ref().is_some_and(|i| {
            !i.default && Some(self.block) != i.object_block && self.calls.len() == i.depth
        }) {
            self.finish_event(true);
            true
        } else {
            false
        }
    }

    fn finish_event(&mut self, transfer: bool) {
        let invocation = self.invocation.take().unwrap();
        self.dispatch_result = Some(transfer);
        if !transfer {
            (self.block, self.pc) = invocation.main;
            self.pending = invocation.pending;
        }
    }
}
