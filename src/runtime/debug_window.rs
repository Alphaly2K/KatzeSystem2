use super::{BaseObject, ObjectFactory, ObjectHost, Runtime, TaskObject, kind};
use crate::kcs::{Host, Interpreter, NativeReturn, decode_utf8_units, format_string};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    fn query_quiz_debug(&mut self, index: u32, target: u32, pc: usize) -> Result<NativeReturn> {
        let receiver = self.tree.find_reachable(self.host.task_id, target);
        if !receiver.is_some_and(|node| node.kind == kind::QUIZ_DEBUG)
            || !self
                .objects
                .get(&target)
                .is_some_and(|object| object.control().deletion_requested == 0)
        {
            // 无结果时原包装会将调用者指针压栈。
            return Err(Error::new(pc, "问答调试查询没有可用的结果接收者"));
        }
        let command = [520, 522, 521, 523][(index - 839) as usize];
        Ok(NativeReturn::Value(
            self.command_target(target, command, [0; 3])?,
        ))
    }

    pub(super) fn debug_window_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if index == 837 && matches!(arguments.len(), 4 | 8) && read::u32_at(arguments, 0)? == 0 {
            return Ok(NativeReturn::Continue);
        }
        let count = if matches!(index, 280..=282 | 288 | 837 | 838) {
            8
        } else {
            4
        };
        if arguments.len() != count {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let target = read::u32_at(arguments, 0)?;
        if matches!(index, 839..=842) {
            return self.query_quiz_debug(index, target, vm.pc);
        } else if index == 281 {
            let address = read::u32_at(arguments, 4)?;
            if vm.read(address, 1).is_err() {
                return Ok(NativeReturn::Continue);
            }
            let mut record = vm.read(address, 352)?.to_vec();
            self.command_target_with_buffers(
                target,
                330,
                [0; 3],
                &mut CommandBuffers {
                    data1: Some(&mut record),
                    ..Default::default()
                },
            )?;
            self.command_target(target, 333, [0, vm.stack_bytes().len() as u32, 0])?;
        } else if index == 282 {
            let source = format_string(vm, read::u32_at(arguments, 4)?, 0)?;
            if source.len() >= 128 {
                return Err(Error::new(vm.pc, "调试窗口文字超过基线缓冲区范围"));
            }
            let mut text: Vec<u8> = decode_utf8_units(&source, false)?
                .into_iter()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            self.command_target_with_buffers(
                target,
                331,
                [0; 3],
                &mut CommandBuffers {
                    data1: Some(&mut text),
                    ..Default::default()
                },
            )?;
        } else {
            let code = match index {
                279 => 328,
                280 => 329,
                283 => 332,
                287 => 334,
                288 => 335,
                837 => 518,
                838 => 519,
                843 => 524,
                _ => unreachable!(),
            };
            let value = if count == 8 {
                read::u32_at(arguments, 4)?
            } else {
                0
            };
            self.command_target(target, code, [0, value, 0])?;
        }
        Ok(NativeReturn::Continue)
    }
}

/// 问答调试面板的状态。索引从零开始，题号保留原始 DWORD。
#[derive(Debug)]
pub struct QuizDebugSession {
    bank: u32,
    pub packs: Vec<Vec<u8>>,
    pub pack: Option<usize>,
    pub questions: Vec<u32>,
    pub question: Option<usize>,
    pub visible: bool,
    pub enabled: bool,
    pub lock_question_and_answers: bool,
    pub pause_counting: bool,
    pub auto_advance: bool,
}

/// 已打开会话接收的面板输入。
#[derive(Clone, Copy, Debug)]
pub enum QuizDebugInput {
    Close,
    Enable(bool),
    SelectPack(usize),
    SelectQuestion(usize),
    LockQuestionAndAnswers(bool),
    PauseCounting(bool),
    AutoAdvance(bool),
}

impl QuizDebugSession {
    fn new(bank: u32, packs: Vec<Vec<u8>>) -> Self {
        Self {
            bank,
            pack: (!packs.is_empty()).then_some(0),
            packs,
            questions: Vec::new(),
            question: None,
            visible: false,
            enabled: false,
            lock_question_and_answers: false,
            pause_counting: false,
            auto_advance: false,
        }
    }

    fn advance(&mut self) {
        if self.enabled && self.auto_advance && !self.questions.is_empty() {
            self.question = Some(
                self.question
                    .map_or(0, |id| (id + 1) % self.questions.len()),
            );
        }
    }

    fn select_pack(
        &mut self,
        index: usize,
        resources: &mut crate::kcs::ResourceState,
    ) -> Result<()> {
        let pack = self
            .packs
            .get(index)
            .ok_or_else(|| Error::new(0, "问答调试题库选项越界"))?;
        self.questions = resources.quiz_debug_select(self.bank, pack)?;
        self.pack = Some(index);
        self.question = (!self.questions.is_empty()).then_some(0);
        Ok(())
    }
}

impl<F: ObjectFactory> Runtime<F> {
    /// 查询该任务已打开的调试面板。
    pub fn quiz_debug_session(&self, id: u32) -> Option<&QuizDebugSession> {
        let object = self.object(id)?;
        (object.control().deletion_requested == 0)
            .then(|| object.quiz_debug_session())
            .flatten()
    }

    /// 枚举当前要求宿主显示的会话，忽略已隐藏或标记删除的任务。
    pub fn visible_quiz_debug_sessions(&self) -> impl Iterator<Item = (u32, &QuizDebugSession)> {
        self.objects.keys().copied().filter_map(|id| {
            let session = self.quiz_debug_session(id)?;
            session.visible.then_some((id, session))
        })
    }

    /// 提交面板输入。会话不可用或控件被禁用时返回 false。
    pub fn quiz_debug_input(&mut self, id: u32, input: QuizDebugInput) -> Result<bool> {
        let Some(object) = self.objects.get_mut(&id) else {
            return Ok(false);
        };
        if object.control().deletion_requested != 0 {
            return Ok(false);
        }
        let Some(session) = object.quiz_debug_session_mut() else {
            return Ok(false);
        };
        match input {
            QuizDebugInput::Close => session.visible = false,
            QuizDebugInput::Enable(enabled) => {
                if let Some(index) = session.pack.filter(|_| enabled) {
                    session.select_pack(index, &mut self.resources.raw)?;
                }
                session.enabled = enabled;
            }
            _ if !session.enabled => return Ok(false),
            QuizDebugInput::SelectPack(index) => {
                session.select_pack(index, &mut self.resources.raw)?;
            }
            QuizDebugInput::SelectQuestion(index) => {
                if index >= session.questions.len() {
                    return Err(Error::new(0, "问答调试题目选项越界"));
                }
                session.question = Some(index);
            }
            QuizDebugInput::LockQuestionAndAnswers(value) => {
                session.lock_question_and_answers = value
            }
            QuizDebugInput::PauseCounting(value) => session.pause_counting = value,
            QuizDebugInput::AutoAdvance(value) => session.auto_advance = value,
        }
        Ok(true)
    }
}

#[derive(Default)]
pub(super) struct QuizDebugObject(BaseObject, Option<QuizDebugSession>);

impl TaskObject for QuizDebugObject {
    fn control(&self) -> &Control {
        self.0.control()
    }

    fn clock(&self) -> &TaskClock {
        self.0.clock()
    }

    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.0.state()
    }

    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }

    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }

    fn quiz_debug_session(&self) -> Option<&QuizDebugSession> {
        self.1.as_ref()
    }

    fn quiz_debug_session_mut(&mut self) -> Option<&mut QuizDebugSession> {
        self.1.as_mut()
    }

    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.0.set_creation_context(context)
    }

    fn update_body(&mut self, host: &mut dyn Host, increment: u32, budget: usize) -> Result<bool> {
        self.0.update_body(host, increment, budget)
    }

    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        if self.0.control.deletion_requested == 0 {
            match command.code {
                518 => {
                    let bank = command.data[1];
                    self.1 = Some(QuizDebugSession::new(bank, host.quiz_debug_packs(bank)?));
                }
                519 => {
                    if let Some(session) = &mut self.1 {
                        session.visible = command.data[1] != 0;
                    }
                }
                520..=523 => {
                    *output =
                        self.1
                            .as_ref()
                            .map_or(command.data[1], |session| match command.code {
                                520 => u32::from(session.enabled),
                                521 => u32::from(session.lock_question_and_answers),
                                522 => u32::from(session.pause_counting),
                                523 => session.question.map_or(u32::MAX, |id| id as u32),
                                _ => unreachable!(),
                            });
                }
                524 => {
                    if let Some(session) = &mut self.1 {
                        session.advance();
                    }
                    *output = command.data[1];
                }
                _ => return self.0.dispatch_command(kind, command, output, host, budget),
            }
            return Ok(CommandOutcome::finished(false));
        }
        self.0.dispatch_command(kind, command, output, host, budget)
    }

    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.0.enqueue_message(message)
    }

    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        self.0.dispatch_message(message, output, host, budget)
    }

    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.1 = None;
        self.0.request_delete(host, budget)
    }
}
