use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use katze_system2::{
    Error, Result,
    kcs::{Host, Interpreter, OrdinaryTask, Program, TaskClock},
    runtime::{
        BaseObject, KcsFactory, ObjectFactory, OrdinaryObject, ProgramLoader, Runtime, TaskObject,
        kind,
    },
    task::{Command, CommandOutcome, Control},
};

const DATA: usize = 512;
const STACK: usize = 512;

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
    code.push(opcode);
    for arg in args {
        code.push(3);
        code.extend(arg.to_le_bytes());
    }
}

/// 合成指令序列；标签只用于计算分支目标。
#[derive(Default)]
struct Asm {
    code: Vec<u8>,
    labels: BTreeMap<&'static str, usize>,
    fixups: Vec<(&'static str, usize)>,
}

impl Asm {
    fn ins(&mut self, opcode: u8, args: &[u32]) -> &mut Self {
        ins(&mut self.code, opcode, args);
        self
    }

    fn label(&mut self, name: &'static str) -> &mut Self {
        self.labels.insert(name, self.code.len());
        self
    }

    fn jump(&mut self, opcode: u8, target: &'static str) -> &mut Self {
        let payload = self.code.len() + 2;
        ins(&mut self.code, opcode, &[0]);
        self.fixups.push((target, payload));
        self
    }

    fn build(&self) -> Vec<u8> {
        let mut code = self.code.clone();
        for (label, payload) in &self.fixups {
            let offset = self.labels[label] as u32;
            code[*payload..*payload + 4].copy_from_slice(&offset.to_le_bytes());
        }
        code
    }
}

struct Source {
    programs: BTreeMap<Vec<u8>, (Vec<u8>, Vec<u8>)>,
}

impl Source {
    fn new() -> Self {
        Self {
            programs: BTreeMap::new(),
        }
    }

    fn add(&mut self, name: &[u8], code: Vec<u8>, data: Vec<u8>) {
        self.programs.insert(name.to_vec(), (code, data));
    }
}

impl ProgramLoader for Source {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        let Some((code, data)) = self.programs.get(name) else {
            return Ok(None);
        };
        Ok(Some(Program::from_code(code, data.clone(), STACK)?))
    }
}

fn data() -> Vec<u8> {
    vec![0; DATA]
}

fn runtime(source: Source) -> Runtime<KcsFactory<Source>> {
    Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        katze_system2::expression::State::new(8),
        KcsFactory::new(source),
    )
}

fn words(vm: &Interpreter, address: u32, count: usize) -> Vec<u32> {
    vm.read(address, count * 4)
        .unwrap()
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect()
}

/// 等待一条消息，把接收到的消息类型和数据项留在程序数据区。
fn waiting_child() -> Vec<u8> {
    let mut asm = Asm::default();
    asm.label("wait")
        .ins(0x25, &[32])
        .ins(0x2b, &[71, 4])
        .ins(0x26, &[60])
        .ins(0x2f, &[36])
        .ins(0x26, &[52])
        .ins(0x2f, &[40])
        .ins(0x26, &[56])
        .jump(0x29, "wait");
    asm.build()
}

#[test]
fn creation_registers_minimum_id_and_inherits_raw_context() {
    let mut parent_data = data();
    parent_data[100..106].copy_from_slice(b"child\0");
    let mut asm = Asm::default();
    asm.ins(0x25, &[kind::ORDINARY])
        .ins(0x2b, &[68, 0])
        .ins(0x25, &[100])
        .ins(0x2b, &[66, 12])
        .ins(0x26, &[0])
        .ins(0x2f, &[0])
        .ins(0x25, &[7])
        .ins(0x25, &[70])
        .ins(0x25, &[80])
        .ins(0x2b, &[69, 16])
        .ins(0x26, &[16])
        .ins(0x25, &[0])
        .ins(0x2b, &[75, 4])
        .ins(0x26, &[20])
        .ins(0x2b, &[76, 0])
        .ins(0x26, &[24])
        .ins(0x3b, &[]);
    let mut source = Source::new();
    source.add(b"parent", asm.build(), parent_data);
    source.add(b"child", waiting_child(), data());
    let mut runtime = runtime(source);

    let parent = runtime
        .create(0, kind::ORDINARY, b"parent", b"pctx")
        .unwrap();
    assert_eq!(parent, 1);
    assert!(runtime.update(10, 0).unwrap());

    let node = runtime.tree().node(2).expect("子任务应已登记");
    assert_eq!(node.parent, Some(parent));
    assert_eq!(node.kind, kind::ORDINARY);
    let parent_vm = runtime.object(parent).unwrap().interpreter().unwrap();
    assert_eq!(words(parent_vm, 0, 1), [2], "命令 66 返回新编号");
    // 子任务尚未等待消息，同步分派返回零。
    assert_eq!(words(parent_vm, 16, 1), [0]);
    assert_eq!(words(parent_vm, 20, 1), [2], "命令 75 枚举后代");
    assert_eq!(words(parent_vm, 24, 1), [0], "命令 76 返回直接父级");

    // 命令 26 保存的原始上下文不展开格式引用；命令 25 把它写进目标 VM。
    let output = runtime
        .dispatch_command(
            parent,
            Command {
                target: 2,
                sender: parent,
                code: 25,
                data: [0, 128, 0],
            },
        )
        .unwrap();
    assert_eq!(output, 0);
    assert_eq!(
        runtime
            .object(2)
            .unwrap()
            .interpreter()
            .unwrap()
            .read_string(128)
            .unwrap(),
        b"pctx"
    );
    // 新建子任务挂在本轮已遍历完子树的任务下，因此下一轮才执行到消息等待。
    assert!(
        !runtime
            .object(2)
            .unwrap()
            .interpreter()
            .unwrap()
            .is_waiting_for_message()
    );
    assert!(runtime.update(20, 0).unwrap());
    assert!(
        runtime
            .object(2)
            .unwrap()
            .interpreter()
            .unwrap()
            .is_waiting_for_message()
    );
    assert_eq!(runtime.tree().next_id(), 3);
}

fn looping_parent(send: u32) -> Vec<u8> {
    let mut asm = Asm::default();
    asm.label("start")
        .ins(0x2f, &[12])
        .jump(0x27, "create")
        .jump(0x29, "send")
        .label("create")
        .ins(0x25, &[kind::ORDINARY])
        .ins(0x25, &[0])
        .ins(0x25, &[100])
        .ins(0x2b, &[66, 12])
        .ins(0x26, &[0])
        .ins(0x25, &[1])
        .ins(0x26, &[12])
        .label("send")
        .ins(0x2f, &[0])
        .ins(0x25, &[7])
        .ins(0x16, &[4])
        .ins(0x25, &[0])
        .ins(0x2b, &[send, 16])
        .ins(0x26, &[8])
        .ins(0x2b, &[65, 0])
        .jump(0x29, "start");
    asm.build()
}

fn loop_runtime(send: u32, budget: Option<usize>) -> Runtime<KcsFactory<Source>> {
    let mut parent_data = data();
    parent_data[100..106].copy_from_slice(b"child\0");
    let mut source = Source::new();
    source.add(b"parent", looping_parent(send), parent_data);
    source.add(b"child", waiting_child(), data());
    let mut runtime = runtime(source);
    if let Some(budget) = budget {
        runtime.set_budget(budget, 1 << 20);
    }
    runtime.create(0, kind::ORDINARY, b"parent", b"").unwrap();
    runtime
}

#[test]
fn synchronous_messages_reach_a_waiting_task_and_report_activity() {
    let mut runtime = loop_runtime(69, None);
    assert!(runtime.update(10, 0).unwrap());
    let parent_vm = runtime.object(1).unwrap().interpreter().unwrap();
    assert_eq!(words(parent_vm, 0, 1), [2], "首轮创建子任务并取得编号");
    // 首轮分派时子任务还没有等待，返回零。
    assert_eq!(words(parent_vm, 8, 1), [0]);
    assert!(
        runtime
            .object(2)
            .unwrap()
            .interpreter()
            .unwrap()
            .is_waiting_for_message()
    );

    assert!(runtime.update(20, 0).unwrap());
    let parent_vm = runtime.object(1).unwrap().interpreter().unwrap();
    assert_eq!(words(parent_vm, 8, 1), [1], "第二轮同步分派返回活动结果");
    let child_vm = runtime.object(2).unwrap().interpreter().unwrap();
    assert_eq!(words(child_vm, 52, 1), [7], "消息类型写入接收地址");
    assert_eq!(words(child_vm, 56, 1), [2], "第二数据项随消息到达");
}

#[test]
fn queued_messages_are_consumed_by_the_next_task_update() {
    let mut runtime = loop_runtime(70, None);
    assert!(runtime.update(10, 0).unwrap());
    let parent_vm = runtime.object(1).unwrap().interpreter().unwrap();
    assert_eq!(words(parent_vm, 8, 1), [1], "排队入口返回一");
    // 首轮子任务尚未等待，队首消息按契约丢弃。
    assert_eq!(
        words(runtime.object(2).unwrap().interpreter().unwrap(), 52, 1),
        [0]
    );

    assert!(runtime.update(20, 0).unwrap());
    let child_vm = runtime.object(2).unwrap().interpreter().unwrap();
    assert_eq!(words(child_vm, 52, 1), [7]);
    assert_eq!(words(child_vm, 56, 1), [2]);
}

#[test]
fn budget_continuation_still_completes_a_synchronous_dispatch() {
    for budget in [0, 1] {
        let mut runtime = loop_runtime(69, Some(budget));
        assert!(runtime.update(10, 0).unwrap());
        assert!(runtime.update(20, 0).unwrap());
        let child_vm = runtime.object(2).unwrap().interpreter().unwrap();
        assert_eq!(words(child_vm, 56, 1), [2]);
        let parent_vm = runtime.object(1).unwrap().interpreter().unwrap();
        assert_eq!(words(parent_vm, 8, 1), [1]);
    }
}

#[test]
fn messages_route_between_sibling_branches() {
    let mut asm = Asm::default();
    asm.ins(0x25, &[1])
        .ins(0x25, &[9])
        .ins(0x25, &[11])
        .ins(0x25, &[0])
        .ins(0x2b, &[69, 16])
        .ins(0x26, &[0])
        .ins(0x3b, &[]);
    let mut source = Source::new();
    source.add(b"sender", asm.build(), data());
    source.add(b"target", waiting_child(), data());
    let mut runtime = runtime(source);

    let target = runtime.create(0, kind::ORDINARY, b"target", b"").unwrap();
    assert!(runtime.update(10, 0).unwrap());
    assert!(
        runtime
            .object(target)
            .unwrap()
            .interpreter()
            .unwrap()
            .is_waiting_for_message()
    );
    // 兄弟分支之间沿共同祖先路由，不要求目标位于发送者子树内。
    let sender = runtime.create(0, kind::ORDINARY, b"sender", b"").unwrap();
    assert_eq!(sender, 2);
    assert!(runtime.update(20, 0).unwrap());
    let sender_vm = runtime.object(sender).unwrap().interpreter().unwrap();
    assert_eq!(words(sender_vm, 0, 1), [1]);
    let target_vm = runtime.object(target).unwrap().interpreter().unwrap();
    assert_eq!(words(target_vm, 52, 1), [9]);
    assert_eq!(words(target_vm, 56, 1), [11]);
}

#[test]
fn task_natives_read_and_write_another_tasks_vm() {
    let mut asm = Asm::default();
    asm.ins(0x25, &[1])
        .ins(0x25, &[0])
        .ins(0x25, &[4])
        .ins(0x25, &[64])
        .ins(0x2b, &[72, 16])
        .ins(0x25, &[1])
        .ins(0x25, &[0])
        .ins(0x25, &[4])
        .ins(0x25, &[68])
        .ins(0x2b, &[73, 16])
        .ins(0x3b, &[]);
    let mut parent_data = data();
    parent_data[68..72].copy_from_slice(b"ABCD");
    let mut source = Source::new();
    source.add(b"parent", asm.build(), parent_data);
    source.add(b"child", vec![0x3b], data());
    let mut runtime = runtime(source);

    let child = runtime.create(0, kind::ORDINARY, b"child", b"").unwrap();
    let parent = runtime.create(0, kind::ORDINARY, b"parent", b"").unwrap();
    assert_eq!((child, parent), (1, 2));
    runtime
        .object_mut(child)
        .unwrap()
        .interpreter_mut()
        .unwrap()
        .write(0, b"WXYZ")
        .unwrap();
    assert!(runtime.update(10, 0).unwrap());

    let parent_vm = runtime.object(parent).unwrap().interpreter().unwrap();
    assert_eq!(parent_vm.read(64, 4).unwrap(), b"WXYZ");
    let child_vm = runtime.object(child).unwrap().interpreter().unwrap();
    assert_eq!(child_vm.read(0, 4).unwrap(), b"ABCD");
}

#[derive(Clone, Default)]
struct Log(Rc<RefCell<Vec<(u32, &'static str)>>>);

impl Log {
    fn push(&self, id: u32, event: &'static str) {
        self.0.borrow_mut().push((id, event));
    }

    fn events(&self) -> Vec<(u32, &'static str)> {
        self.0.borrow().clone()
    }
}

/// 记录生命周期时点的基类对象；只用于核对顺序。
struct Recorded {
    id: u32,
    base: BaseObject,
    log: Log,
}

impl TaskObject for Recorded {
    fn control(&self) -> &Control {
        self.base.control()
    }

    fn clock(&self) -> &TaskClock {
        self.base.clock()
    }

    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.base.state()
    }

    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }

    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }

    fn set_creation_context(&mut self, _context: &[u8]) -> Result<()> {
        Ok(())
    }

    fn update_body(
        &mut self,
        _host: &mut dyn Host,
        _increment: u32,
        _budget: usize,
    ) -> Result<bool> {
        self.log.push(self.id, "body");
        Ok(true)
    }

    fn dispatch_command(
        &mut self,
        _kind: u32,
        command: &Command,
        _output: &mut u32,
        _host: &mut dyn Host,
        _budget: usize,
    ) -> Result<CommandOutcome> {
        if command.code == 3 && self.base.control.deletion_requested == 0 {
            self.log.push(self.id, "delete");
            self.base.control.deletion_requested = 1;
        }
        Ok(CommandOutcome::finished(true))
    }

    fn enqueue_message(&mut self, _message: [u32; 4]) -> Result<()> {
        Ok(())
    }

    fn dispatch_message(
        &mut self,
        _message: &mut [u32; 4],
        _output: &mut u32,
        _host: &mut dyn Host,
        _budget: usize,
    ) -> Result<bool> {
        Ok(true)
    }

    fn request_delete(&mut self, _host: &mut dyn Host, _budget: usize) -> Result<bool> {
        if self.base.control.deletion_requested == 0 {
            self.log.push(self.id, "delete");
            self.base.control.deletion_requested = 1;
        }
        Ok(true)
    }

    fn mark_destroying(&mut self, _host: &mut dyn Host, _budget: usize) -> Result<bool> {
        if self.base.control.deletion_requested == 0 {
            self.log.push(self.id, "mark");
            self.base.control.deletion_requested = 1;
        }
        Ok(true)
    }

    fn release(&mut self) -> Result<()> {
        self.log.push(self.id, "release");
        Ok(())
    }
}

struct RecordingFactory(Log);

impl ObjectFactory for RecordingFactory {
    fn create(&mut self, id: u32, _kind: u32, _name: &[u8]) -> Result<Box<dyn TaskObject>> {
        Ok(Box::new(Recorded {
            id,
            base: BaseObject::new(),
            log: self.0.clone(),
        }))
    }
}

#[test]
fn deletion_notifies_children_before_releasing_subtrees_and_reuses_ids() {
    let log = Log::default();
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(Recorded {
            id: 0,
            base: BaseObject::new(),
            log: log.clone(),
        }),
        katze_system2::expression::State::new(8),
        RecordingFactory(log.clone()),
    );
    let branch = runtime.create(0, 2, b"branch", b"").unwrap();
    let leaf = runtime.create(branch, 2, b"leaf", b"").unwrap();
    let other = runtime.create(0, 2, b"other", b"").unwrap();
    assert_eq!((branch, leaf, other), (1, 2, 3));

    // 直接标记删除相当于命令 3 已完成；实际销毁发生在下一轮树更新。
    runtime
        .object_mut(branch)
        .unwrap()
        .state()
        .0
        .deletion_requested = 1;
    assert!(runtime.update(10, 0).unwrap());
    assert_eq!(
        log.events(),
        [
            (2, "body"),
            (2, "delete"),
            (2, "release"),
            (1, "release"),
            (3, "body"),
            (0, "body"),
        ]
    );
    assert!(runtime.object(branch).is_none());
    assert!(runtime.object(leaf).is_none());
    assert!(runtime.object(other).is_some());
    // 根层清理后，自动编号复用被释放的最小编号。
    assert_eq!(runtime.tree().next_id(), 1);
    assert_eq!(runtime.create(0, 2, b"again", b"").unwrap(), 1);

    // 直接销毁未标记的任务时，先执行自身通知再释放。
    log.0.borrow_mut().clear();
    runtime.destroy(other).unwrap();
    assert_eq!(log.events(), [(3, "mark"), (3, "release")]);
    assert!(runtime.tree().node(other).is_none());
}

#[test]
fn unsupported_creation_targets_and_missing_message_targets_report_diagnostics() {
    let mut source = Source::new();
    source.add(b"child", waiting_child(), data());
    let mut runtime = runtime(source);
    assert!(
        runtime
            .create(0, kind::PLANE, b"plane", b"")
            .unwrap_err()
            .message
            .contains("子系统工厂")
    );
    assert!(runtime.create(99, kind::ORDINARY, b"x", b"").is_err());

    let parent = runtime.create(0, kind::ORDINARY, b"child", b"").unwrap();
    assert!(runtime.send_message(parent, 99, [parent, 1, 0, 0]).is_err());
    assert_eq!(
        runtime
            .dispatch_command(
                parent,
                Command {
                    target: 99,
                    sender: parent,
                    code: 17,
                    data: [0; 3],
                },
            )
            .unwrap(),
        0
    );
    assert!(
        runtime
            .dispatch_command(
                parent,
                Command {
                    target: parent,
                    sender: parent,
                    code: 11,
                    data: [0; 3],
                },
            )
            .is_err()
    );
}

#[test]
fn oversized_external_memory_is_rejected_before_allocation() {
    let mut factory = KcsFactory::new(Source::new());
    factory.external_bytes = usize::MAX;
    let error = factory
        .create(1, kind::ORDINARY, b"missing")
        .err()
        .expect("超大外部存储应被拒绝");
    assert!(error.message.contains("外部存储"));
}

#[test]
fn commands_route_by_tree_and_preserve_outputs() {
    let mut source = Source::new();
    source.add(b"child", waiting_child(), data());
    let mut runtime = runtime(source);
    runtime.create(0, kind::ORDINARY, b"child", b"").unwrap();

    // 基类属性初值为 u32::MAX，命令 24 保存原始 DWORD，命令 13 查询对象类型。
    let output = runtime
        .dispatch_command(
            0,
            Command {
                target: 1,
                sender: 0,
                code: 23,
                data: [0; 3],
            },
        )
        .unwrap();
    assert_eq!(output, u32::MAX);
    runtime
        .dispatch_command(
            0,
            Command {
                target: 1,
                sender: 0,
                code: 24,
                data: [0, 0x8000_0001, 0],
            },
        )
        .unwrap();
    assert_eq!(
        runtime
            .dispatch_command(
                0,
                Command {
                    target: 1,
                    sender: 0,
                    code: 23,
                    data: [0; 3],
                },
            )
            .unwrap(),
        0x8000_0001
    );
    assert_eq!(
        runtime
            .dispatch_command(
                0,
                Command {
                    target: 1,
                    sender: 0,
                    code: 13,
                    data: [0; 3],
                },
            )
            .unwrap(),
        kind::ORDINARY
    );
}

#[test]
fn creation_context_overflow_leaves_no_task_registered() {
    let mut source = Source::new();
    source.add(b"child", waiting_child(), data());
    let mut runtime = runtime(source);
    let context = [b'x'; 32];
    let error = runtime
        .create(0, kind::ORDINARY, b"child", &context)
        .unwrap_err();
    assert!(error.message.contains("创建上下文"));
    assert!(runtime.tree().node(1).is_none());
    assert_eq!(runtime.tree().next_id(), 1);
    assert!(runtime.object(1).is_none());
}

#[test]
fn root_shutdown_closes_the_tree() {
    let mut source = Source::new();
    source.add(b"child", waiting_child(), data());
    let mut runtime = runtime(source);
    runtime.create(0, kind::ORDINARY, b"child", b"").unwrap();
    runtime.destroy(0).unwrap();
    assert!(runtime.tree().node(0).is_none());
    assert!(runtime.object(0).is_none());
    assert!(runtime.update(10, 0).is_err());
}

#[test]
fn runaway_program_stops_at_the_synchronous_instruction_limit() {
    let mut asm = Asm::default();
    asm.label("loop").jump(0x29, "loop");
    let mut source = Source::new();
    source.add(b"child", asm.build(), data());
    let mut runtime = runtime(source);
    runtime.create(0, kind::ORDINARY, b"child", b"").unwrap();
    runtime.set_budget(1024, 4096);
    let error = runtime.update(10, 0).unwrap_err();
    assert!(error.message.contains("同步执行"));
}

#[test]
fn delete_native_marks_a_new_child_for_destruction_on_the_next_update() {
    let mut parent_data = data();
    parent_data[100..106].copy_from_slice(b"child\0");
    let mut asm = Asm::default();
    asm.ins(0x25, &[kind::ORDINARY])
        .ins(0x2b, &[68, 0])
        .ins(0x25, &[100])
        .ins(0x2b, &[66, 12])
        .ins(0x26, &[0])
        .ins(0x2f, &[0])
        .ins(0x2b, &[67, 4])
        .ins(0x3b, &[]);
    let mut source = Source::new();
    source.add(b"parent", asm.build(), parent_data);
    source.add(b"child", vec![0x3b], data());
    let mut runtime = runtime(source);
    let parent = runtime.create(0, kind::ORDINARY, b"parent", b"").unwrap();
    assert!(runtime.update(10, 0).unwrap());
    let child = parent + 1;
    // 67 只标记删除，命令本身不销毁对象。
    assert_eq!(
        runtime.object(child).unwrap().control().deletion_requested,
        1
    );
    assert!(runtime.tree().node(child).is_some());
    // 子任务挂在已遍历完子树的任务下，下一轮更新才执行析构。
    assert!(runtime.update(20, 0).unwrap());
    assert!(runtime.tree().node(child).is_none());
    assert!(runtime.object(child).is_none());
    assert!(runtime.object(parent).is_some());
    assert_eq!(runtime.tree().next_id(), child);
}

/// 命令录制：目标处理器收到的原始命令，顺序即树遍历顺序。
#[derive(Clone, Default)]
struct Commands(Rc<RefCell<Vec<Command>>>);

impl Commands {
    fn all(&self) -> Vec<Command> {
        self.0.borrow().clone()
    }

    fn clear(&self) {
        self.0.borrow_mut().clear();
    }
}

/// 只有基类控制和命令记录的对象，用于核对路由与传播。
struct CommandRecorder {
    base: BaseObject,
    commands: Commands,
}

impl TaskObject for CommandRecorder {
    fn control(&self) -> &Control {
        &self.base.control
    }

    fn clock(&self) -> &TaskClock {
        &self.base.clock
    }

    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        (&mut self.base.control, &mut self.base.clock)
    }

    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }

    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }

    fn set_creation_context(&mut self, _context: &[u8]) -> Result<()> {
        Ok(())
    }

    fn update_body(
        &mut self,
        _host: &mut dyn Host,
        _increment: u32,
        _budget: usize,
    ) -> Result<bool> {
        Ok(true)
    }

    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        self.commands.0.borrow_mut().push(*command);
        <BaseObject as TaskObject>::dispatch_command(
            &mut self.base,
            kind,
            command,
            output,
            host,
            budget,
        )
    }

    fn enqueue_message(&mut self, _message: [u32; 4]) -> Result<()> {
        Ok(())
    }

    fn dispatch_message(
        &mut self,
        _message: &mut [u32; 4],
        _output: &mut u32,
        _host: &mut dyn Host,
        _budget: usize,
    ) -> Result<bool> {
        Ok(true)
    }

    fn request_delete(&mut self, _host: &mut dyn Host, _budget: usize) -> Result<bool> {
        self.base.control.deletion_requested = 1;
        Ok(true)
    }

    fn release(&mut self) -> Result<()> {
        Ok(())
    }
}

/// 按类型提供录制对象，其余类型按名称装载普通 KCS。
struct MixedFactory {
    programs: Source,
    commands: Commands,
}

impl ObjectFactory for MixedFactory {
    fn create(&mut self, id: u32, kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if kind == kind::PLANE {
            return Ok(Box::new(CommandRecorder {
                base: BaseObject::new(),
                commands: self.commands.clone(),
            }));
        }
        let program = self
            .programs
            .load(name)?
            .ok_or_else(|| Error::new(0, "KCS 程序不存在"))?;
        let mut task = OrdinaryTask::new(program, Vec::new())?;
        task.task_id = id;
        Ok(Box::new(OrdinaryObject::new(task)))
    }
}

fn recording_runtime(programs: Source) -> (Runtime<MixedFactory>, Commands) {
    let commands = Commands::default();
    let runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(CommandRecorder {
            base: BaseObject::new(),
            commands: commands.clone(),
        }),
        katze_system2::expression::State::new(8),
        MixedFactory {
            programs,
            commands: commands.clone(),
        },
    );
    (runtime, commands)
}

fn targets(commands: &Commands) -> Vec<u32> {
    commands
        .all()
        .into_iter()
        .map(|command| command.target)
        .collect()
}

#[test]
fn native_110_sends_two_words_as_command_97() {
    let mut asm = Asm::default();
    asm.ins(0x25, &[1])
        .ins(0x25, &[0x1122_3344])
        .ins(0x2b, &[110, 8])
        .ins(0x3b, &[]);
    let mut source = Source::new();
    source.add(b"sender", asm.build(), data());
    let (mut runtime, commands) = recording_runtime(source);
    let target = runtime.create(0, kind::PLANE, b"target", b"").unwrap();
    let sender = runtime.create(0, kind::ORDINARY, b"sender", b"").unwrap();
    assert_eq!((target, sender), (1, 2));

    assert!(runtime.update(10, 0).unwrap());
    assert_eq!(
        commands.all(),
        [Command {
            target,
            sender,
            code: 97,
            data: [0, 0x1122_3344, 0],
        }]
    );
}

#[test]
fn dispatch_command_keeps_the_sender_given_by_the_caller() {
    let (mut runtime, commands) = recording_runtime(Source::new());
    let target = runtime.create(0, kind::PLANE, b"target", b"").unwrap();
    let sender = runtime.create(0, kind::PLANE, b"sender", b"").unwrap();
    assert_eq!((target, sender), (1, 2));

    // 路由起点是根任务，发送者仍是命令自带的编号。
    let output = runtime
        .dispatch_command(
            0,
            Command {
                target,
                sender,
                code: 99,
                data: [0; 3],
            },
        )
        .unwrap();
    assert_eq!(output, 0);
    assert_eq!(
        commands.all(),
        [Command {
            target,
            sender,
            code: 99,
            data: [0; 3],
        }]
    );
}

#[test]
fn tree_dispatch_propagates_unknown_commands_and_stops_at_consumers() {
    let (mut runtime, commands) = recording_runtime(Source::new());
    let branch = runtime.create(0, kind::PLANE, b"branch", b"").unwrap();
    let leaf = runtime.create(branch, kind::PLANE, b"leaf", b"").unwrap();
    let other = runtime.create(0, kind::PLANE, b"other", b"").unwrap();
    assert_eq!((branch, leaf, other), (1, 2, 3));

    // 未识别的编号不修改状态，允许按 data[2] 传播到整棵子树。
    runtime
        .dispatch_command(
            0,
            Command {
                target: 0,
                sender: 0,
                code: 99,
                data: [0, 0, 1],
            },
        )
        .unwrap();
    assert_eq!(targets(&commands), [0, branch, leaf, other]);
    commands.clear();

    // 基类命令 13 由目标消费，即使 data[2] 非零也不进入子树。
    runtime
        .dispatch_command(
            0,
            Command {
                target: 0,
                sender: 0,
                code: 13,
                data: [0, 0, 1],
            },
        )
        .unwrap();
    assert_eq!(targets(&commands), [0]);
    commands.clear();

    // 已有删除标记的对象消费命令，当前分支到此结束；同层其他分支仍会收到。
    runtime
        .object_mut(branch)
        .unwrap()
        .state()
        .0
        .deletion_requested = 1;
    runtime
        .dispatch_command(
            0,
            Command {
                target: 0,
                sender: 0,
                code: 99,
                data: [0, 0, 1],
            },
        )
        .unwrap();
    assert_eq!(targets(&commands), [0, branch, other]);
}

// 自引用路径的最小复现：执行中的对象在调用期间移出存储，以下目标语义尚未实现。
// 原入口已核实的基线写在各个测试里；当前运行时在 67 和同步回信处返回
// 「任务对象不存在或正在执行」，在 70 处返回「任务对象不存在」。去掉 ignore 即可观察。

#[test]
#[ignore = "执行中的对象暂时移出存储：native 67 无法删除自身"]
fn delete_native_can_target_the_running_task() {
    let mut asm = Asm::default();
    asm.ins(0x2b, &[68, 0])
        .ins(0x2b, &[67, 4])
        .ins(0x25, &[7])
        .ins(0x26, &[4])
        .ins(0x3b, &[]);
    let mut source = Source::new();
    source.add(b"self", asm.build(), data());
    let mut runtime = runtime(source);
    let id = runtime.create(0, kind::ORDINARY, b"self", b"").unwrap();

    // 原入口：删除标记为 1，后续指令仍执行。
    assert!(runtime.update(10, 0).unwrap());
    assert_eq!(runtime.object(id).unwrap().control().deletion_requested, 1);
    let vm = runtime.object(id).unwrap().interpreter().unwrap();
    assert_eq!(words(vm, 4, 1), [7], "删除命令之后的指令仍执行");
}

#[test]
#[ignore = "执行中的对象暂时移出存储：native 70 无法向自身排队"]
fn queue_native_can_target_the_running_task() {
    let mut asm = Asm::default();
    asm.ins(0x2b, &[68, 0])
        .ins(0x25, &[7])
        .ins(0x25, &[0])
        .ins(0x25, &[0])
        .ins(0x2b, &[70, 16])
        .ins(0x26, &[0])
        .ins(0x25, &[32])
        .ins(0x2b, &[71, 4])
        .ins(0x26, &[4])
        .ins(0x3b, &[]);
    let mut source = Source::new();
    source.add(b"self", asm.build(), data());
    let mut runtime = runtime(source);
    let id = runtime.create(0, kind::ORDINARY, b"self", b"").unwrap();

    // 原入口：排队入口返回一，队列新增一项并在下一轮投递给自身的等待地址。
    assert!(runtime.update(10, 0).unwrap());
    assert_eq!(
        words(runtime.object(id).unwrap().interpreter().unwrap(), 0, 1),
        [1],
        "排队入口返回一"
    );
    assert!(runtime.update(20, 0).unwrap());
    let vm = runtime.object(id).unwrap().interpreter().unwrap();
    assert_eq!(words(vm, 32, 2), [1, 7], "下一轮收到自身排队的消息");
}

#[test]
#[ignore = "执行中的对象暂时移出存储：接收者无法向仍在执行的发送者回信"]
fn message_handler_can_reply_to_the_running_sender() {
    // 等待消息后把首项（发送者）作为目标，再同步发送一条消息。
    let mut receiver = Asm::default();
    receiver
        .label("wait")
        .ins(0x25, &[32])
        .ins(0x2b, &[71, 4])
        .ins(0x26, &[0])
        .ins(0x2f, &[32])
        .ins(0x25, &[7])
        .ins(0x25, &[0])
        .ins(0x25, &[0])
        .ins(0x2b, &[69, 16])
        .ins(0x26, &[4])
        .jump(0x29, "wait");
    let mut sender = Asm::default();
    sender
        .ins(0x25, &[1])
        .ins(0x25, &[7])
        .ins(0x25, &[0])
        .ins(0x25, &[0])
        .ins(0x2b, &[69, 16])
        .ins(0x26, &[0])
        .ins(0x3b, &[]);
    let mut source = Source::new();
    source.add(b"receiver", receiver.build(), data());
    source.add(b"sender", sender.build(), data());
    let mut runtime = runtime(source);
    let receiver = runtime.create(0, kind::ORDINARY, b"receiver", b"").unwrap();
    let sender = runtime.create(0, kind::ORDINARY, b"sender", b"").unwrap();
    assert_eq!((receiver, sender), (1, 2));

    // 原入口：回信返回零，接收者回到等待，外层 native 69 返回一。
    assert!(runtime.update(10, 0).unwrap());
    let receiver_vm = runtime.object(receiver).unwrap().interpreter().unwrap();
    assert_eq!(words(receiver_vm, 4, 1), [0], "回信返回零");
    assert!(receiver_vm.is_waiting_for_message(), "接收者回到消息等待");
    let sender_vm = runtime.object(sender).unwrap().interpreter().unwrap();
    assert_eq!(words(sender_vm, 0, 1), [1], "外层 native 69 返回一");
}
