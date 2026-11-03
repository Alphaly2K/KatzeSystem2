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
        self.dispatch_event_inner(object, bank, event, default_handler, false)
    }

    pub(crate) fn dispatch_event_at_host_boundary(
        &mut self,
        object: &str,
        bank: i32,
        event: &str,
        default_handler: Option<&str>,
    ) -> Result<bool> {
        self.dispatch_event_inner(object, bank, event, default_handler, true)
    }

    fn dispatch_event_inner(
        &mut self,
        object: &str,
        bank: i32,
        event: &str,
        default_handler: Option<&str>,
        host_boundary: bool,
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
                | "PLAY"
        ) {
            return Err(Error::new(0, "未支持的 FES 对象事件"));
        }
        if event == "PLAY" && default_handler.is_some_and(|name| !name.is_empty()) {
            return Err(Error::new(0, "FES SOUND 非空媒体别名尚未接入"));
        }
        if self.invocation.is_some()
            || !self.definitions.is_empty()
            || self.pre_declarations.is_some()
            || !self.initializers.is_empty()
            || self.declarations.is_some()
        {
            return Err(Error::new(0, "FES 无法在当前状态分派事件"));
        }
        if !host_boundary
            && self
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
        });
        self.dispatch_result = None;
        self.block = block;
        self.pc = 0;
        Ok(true)
    }

    /// 结束事件内的 wait／exit；默认事件组结束后仍执行对象自身处理器。
    /// 保留进入分派前的主流程等待以及事件内尚未返回的调用。
    pub fn finish_event_handler(&mut self) -> Result<()> {
        let invocation = self
            .invocation
            .as_mut()
            .ok_or_else(|| Error::new(0, "FES 没有正在执行的事件"))?;
        self.pending = None;
        if invocation.default {
            invocation.default = false;
            if let Some(block) = invocation.object_block {
                self.block = block;
                self.pc = 0;
                return Ok(());
            }
        }
        self.finish_event(false);
        Ok(())
    }

    pub(crate) fn object_handler_exists(&self) -> bool {
        self.invocation
            .as_ref()
            .is_some_and(|invocation| invocation.object_block.is_some())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Step, expression::State, fes::Program};

    #[test]
    fn host_callback_restores_the_pending_command_and_main_call_position() {
        for budget in [1, 64] {
            let program = Program::from_text(b"#START\ncall MAIN\n$0=$0+1\nwait 99\n#MAIN\ndecode\n$1=$1+1\n#bar.SCRL_P\n$2=$2+1\nwait 3\n$2=999\n").unwrap();
            let mut vm = Interpreter::new(program).unwrap();
            let mut numbers = State::new(8);
            let pending = loop {
                match vm.run(&mut numbers, budget).unwrap() {
                    Step::Yield => (),
                    Step::Event(event @ Event::Command(_)) => break event,
                    step => panic!("{step:?}"),
                }
            };
            assert!(vm.dispatch_event("bar", 0, "SCRL_P", None).is_err());
            assert!(
                vm.dispatch_event_at_host_boundary("bar", 0, "SCRL_P", None)
                    .unwrap()
            );
            assert!(
                vm.dispatch_event_at_host_boundary("bar", 0, "SCRL_P", None)
                    .is_err()
            );
            loop {
                match vm.run(&mut numbers, budget).unwrap() {
                    Step::Yield => (),
                    Step::Event(Event::Wait(3)) => vm.finish_event_handler().unwrap(),
                    step => panic!("{step:?}"),
                }
                if vm.take_dispatch_result().is_some() {
                    break;
                }
            }
            assert_eq!(vm.run(&mut numbers, budget).unwrap(), Step::Event(pending));
            vm.resume();
            loop {
                match vm.run(&mut numbers, budget).unwrap() {
                    Step::Yield => (),
                    Step::Event(Event::Wait(99)) => break,
                    step => panic!("{step:?}"),
                }
            }
            assert_eq!(&numbers.shared[..3], &[1, 1, 1]);
        }
    }
}
