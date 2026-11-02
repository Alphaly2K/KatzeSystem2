use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{GraphicsFactory, Projection, RenderTarget, TextureFilter, Transform},
    input::{InputDevice, InputFactory, KeyEvent, KeyEventKind, KeySample, PointerSample},
    kcs::{Host, Interpreter, ResourceServices},
    resource::ResourceMode,
    runtime::{BaseObject, ObjectFactory, RootObject, Runtime, TaskObject, fes::FesFactory, kind},
    task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock},
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::Write,
    rc::Rc,
};

type Queries = Rc<RefCell<Vec<(u32, u32, bool)>>>;
type TestRuntime = Runtime<ButtonFactory<FesFactory<GraphicsFactory<InputFactory<Factory>>>>>;

struct ButtonFactory<F> {
    fallback: F,
    fail: Rc<Cell<bool>>,
    attempts: Rc<Cell<u32>>,
}
impl<F: ObjectFactory> ObjectFactory for ButtonFactory<F> {
    fn create(&mut self, id: u32, kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if kind == kind::BUTTON && name == b"low button" {
            self.attempts.set(self.attempts.get() + 1);
        }
        if kind == kind::BUTTON && self.fail.get() {
            return Err(Error::new(0, "合成按钮创建失败"));
        }
        self.fallback.create(id, kind, name)
    }
}

struct Factory {
    child: Rc<Cell<Option<u32>>>,
}
impl ObjectFactory for Factory {
    fn create(&mut self, id: u32, kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if kind != kind::BASE {
            return Err(Error::new(0, "合成工厂没有该类型"));
        }
        if name == b"driver" || name == b"delete parent" {
            Ok(Box::new(Driver {
                base: BaseObject {
                    clock: TaskClock {
                        origin: if name == b"driver" { 400 } else { 0 },
                        ..TaskClock::default()
                    },
                    ..BaseObject::new()
                },
                id,
                child: self.child.clone(),
                context: Vec::new(),
                delete_parent: name == b"delete parent",
            }))
        } else {
            Ok(Box::new(BaseObject::new()))
        }
    }
}

struct Files {
    files: BTreeMap<Vec<u8>, Vec<u8>>,
    reads: Rc<RefCell<Vec<Vec<u8>>>>,
    fail_media: Rc<Cell<bool>>,
}
impl ResourceServices for Files {
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.reads.borrow_mut().push(name.to_vec());
        if self.fail_media.get() && name != b"complete.fes" {
            return Err(Error::new(0, "合成媒体读取失败"));
        }
        Ok(self.files.get(name).cloned())
    }
}

struct Device {
    queries: Queries,
    mask: Rc<Cell<bool>>,
    pointer: Rc<Cell<PointerSample>>,
}
impl InputDevice for Device {
    fn poll(&mut self) -> Result<()> {
        Ok(())
    }
    fn pointer(&mut self) -> Result<PointerSample> {
        Ok(self.pointer.get())
    }
    fn key(&mut self, key: u32, group: u32, repeat: bool) -> Result<KeySample> {
        self.queries.borrow_mut().push((key, group, repeat));
        Ok(KeySample {
            mask: if self.mask.get() && key == 12 {
                [0, 2, 0x80000000, 8][group as usize]
            } else {
                0
            },
            repeat: 0,
        })
    }
    fn take_wheel(&mut self) -> Result<u32> {
        Ok(0)
    }
    fn is_foreground(&mut self) -> Result<bool> {
        Ok(true)
    }
    fn finish(&mut self) -> Result<()> {
        Ok(())
    }
    fn commit(&mut self) -> Result<()> {
        Ok(())
    }
    fn history(&mut self) -> Result<[u8; 32]> {
        Ok([0; 32])
    }
    fn show_cursor(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
    fn cursor_visible(&mut self) -> Result<u32> {
        Ok(1)
    }
    fn set_pointer(&mut self, _: [i32; 2]) -> Result<()> {
        Ok(())
    }
    fn clear_keys(&mut self) -> Result<()> {
        Ok(())
    }
}

fn packed(source: &[u8]) -> Vec<u8> {
    let mut writer = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    writer.write_all(source).unwrap();
    let compressed = writer.finish().unwrap();
    let mut bytes = b"FES\0".to_vec();
    for value in [compressed.len() as u32, source.len() as u32, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(compressed);
    bytes
}

fn command(target: u32, code: u32, a: u32, b: u32) -> Command {
    Command {
        target,
        sender: 0,
        code,
        data: [a, b, 0],
    }
}

fn send(runtime: &mut TestRuntime, target: u32, code: u32, a: u32, b: u32) -> u32 {
    runtime
        .dispatch_command(0, command(target, code, a, b))
        .unwrap()
}

fn file(runtime: &mut TestRuntime, target: u32, name: &[u8]) -> Result<u32> {
    let mut name = name.to_vec();
    runtime.dispatch_command_with_buffers(
        0,
        command(target, 338, 0, 1),
        &mut CommandBuffers {
            data0: None,
            data1: Some(&mut name),
        },
    )
}

struct Setup {
    runtime: TestRuntime,
    queries: Queries,
    mask: Rc<Cell<bool>>,
    reads: Rc<RefCell<Vec<Vec<u8>>>>,
    child: Rc<Cell<Option<u32>>>,
    fail_button: Rc<Cell<bool>>,
    button_attempts: Rc<Cell<u32>>,
    pointer: Rc<Cell<PointerSample>>,
    fail_media: Rc<Cell<bool>>,
}

fn setup(budget: usize, source: &[u8]) -> Setup {
    let queries = Rc::new(RefCell::new(Vec::new()));
    let mask = Rc::new(Cell::new(false));
    let child = Rc::new(Cell::new(None));
    let reads = Rc::new(RefCell::new(Vec::new()));
    let fail_button = Rc::new(Cell::new(false));
    let button_attempts = Rc::new(Cell::new(0));
    let pointer = Rc::new(Cell::new(PointerSample::default()));
    let fail_media = Rc::new(Cell::new(false));
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(RootObject::new(1, 2)),
        State::new(8),
        ButtonFactory {
            fail: fail_button.clone(),
            attempts: button_attempts.clone(),
            fallback: FesFactory {
                fallback: GraphicsFactory {
                    fallback: InputFactory {
                        fallback: Factory {
                            child: child.clone(),
                        },
                    },
                },
            },
        },
    );
    runtime.set_budget(budget, 10000);
    runtime.host_mut().surface_size = [8, 6];
    runtime
        .set_input_device(Device {
            queries: queries.clone(),
            mask: mask.clone(),
            pointer: pointer.clone(),
        })
        .unwrap();
    runtime.set_resources(Files {
        files: BTreeMap::from([(b"complete.fes".to_vec(), packed(source))]),
        reads: reads.clone(),
        fail_media: fail_media.clone(),
    });
    runtime.drawing_mut().enabled = true;
    runtime.drawing_mut().active = true;
    runtime.drawing_mut().target = Some(RenderTarget::new([8, 6]).unwrap());
    assert_eq!(runtime.create(0, kind::INPUT, b"input", b"").unwrap(), 1);
    assert_eq!(
        runtime
            .create(0, kind::BASE, b"unused sound slot", b"")
            .unwrap(),
        2
    );
    Setup {
        runtime,
        queries,
        mask,
        reads,
        child,
        fail_button,
        button_attempts,
        pointer,
        fail_media,
    }
}

#[test]
fn fes_tasks_load_without_starting_then_drive_pixels_and_keep_exit_properties() {
    let source = b"#DEFINE\nCIMAGE sprite\nPLANE view\n#OBJECT SIZE COLOR PRI POS DISP\nsprite 2,2 0xff123456 5 1,1 1\n#OBJECT POS SIZE VRAM PRI DISP\nview 0,0 8,6 0,0 500 1\n#START\n$0=$0+1\nwait 2\n$1=$1+1\nexit 37\n";
    for budget in [1, 4096] {
        let Setup {
            mut runtime,
            queries,
            reads,
            ..
        } = setup(budget, source);
        runtime.update(100, 7).unwrap();
        let fes = runtime
            .create(0, kind::FES, b"ignored task name", b"original")
            .unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        assert_eq!(
            runtime.object(fes).unwrap().creation_context(),
            Some(b"".as_slice())
        );
        let list = runtime.create(fes, kind::IMAGE_LIST, b"list", b"").unwrap();
        send(&mut runtime, list, 285, 8, 6);
        send(&mut runtime, list, 289, 1, 0);
        send(&mut runtime, fes, 340, 0, list);
        file(&mut runtime, fes, b"complete.fes\0").unwrap();
        assert_eq!(runtime.numbers().shared[0], 0);
        assert_eq!(runtime.tree().children(fes).unwrap().count(), 1);
        queries.borrow_mut().clear();
        send(&mut runtime, fes, 339, 0, 0);
        assert_eq!(runtime.numbers().shared[0], 1);
        assert_eq!(runtime.numbers().shared[1], 0);
        assert_eq!(
            *queries.borrow(),
            [
                (12, 0, false),
                (12, 1, false),
                (12, 2, false),
                (12, 3, false)
            ]
        );
        assert_eq!(runtime.tree().children(fes).unwrap().count(), 3);
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(2, 1),
            Some(0)
        );
        runtime.update(101, 11).unwrap();
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(2, 1),
            Some(0xff123456)
        );
        let mut output = RenderTarget::new([8, 6]).unwrap();
        runtime
            .render_frame(
                &mut output,
                &Projection::default(),
                *Transform::default().matrix(),
                TextureFilter::Point,
            )
            .unwrap();
        assert_eq!(output.pixel(1, 1), Some(0xff123456));
        send(&mut runtime, fes, 29, 0, 1);
        runtime.update(102, 1).unwrap();
        assert_eq!(runtime.object(fes).unwrap().clock().time, 101);
        assert_eq!(runtime.numbers().shared[1], 1);
        assert_eq!(send(&mut runtime, fes, 23, 0, 0), 37);
        send(&mut runtime, fes, 24, 0, 77);
        runtime.update(103, 1).unwrap();
        assert_eq!(send(&mut runtime, fes, 23, 0, 0), 77);
        assert_eq!(runtime.numbers().shared[0..2], [2, 1]);
        assert_eq!(*reads.borrow(), [b"complete.fes".to_vec()]);
        runtime.destroy(fes).unwrap();
        assert!(runtime.object(fes).is_none());
        assert!(runtime.object(list).is_none());
        assert_eq!(runtime.drawables_mut().order().len(), 0);
        assert!(runtime.update(104, 1).unwrap());
    }
}

struct Driver {
    base: BaseObject,
    id: u32,
    child: Rc<Cell<Option<u32>>>,
    context: Vec<u8>,
    delete_parent: bool,
}
impl TaskObject for Driver {
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
    fn creation_context(&self) -> Option<&[u8]> {
        Some(&self.context)
    }
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.context = context.to_vec();
        Ok(())
    }
    fn update_body(&mut self, host: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        if self.delete_parent {
            let parent = host.parent_task_id()?;
            host.dispatch_task_command(
                Command {
                    sender: self.id,
                    ..command(parent, 3, 0, 0)
                },
                &mut CommandBuffers::default(),
                &mut 0,
            )?;
            return Ok(true);
        }
        if self.child.get().is_some() {
            return Ok(true);
        }
        self.context = b"updated parent".to_vec();
        host.set_task_id(self.id);
        host.set_task_clock(&self.base.clock);
        host.set_task_context(&self.context);
        let child = host.create_task_child(kind::FES, b"nested", b"passed")?;
        self.child.set(Some(child));
        let mut file = b"complete.fes\0".to_vec();
        for code in [337, 338, 339] {
            let mut output = 0x13579bdf;
            let mut buffers = CommandBuffers {
                data0: None,
                data1: if code == 338 { Some(&mut file) } else { None },
            };
            assert!(host.dispatch_task_command(
                Command {
                    sender: self.id,
                    ..command(child, code, 0, 0)
                },
                &mut buffers,
                &mut output
            )?);
            assert_eq!(output, 0x13579bdf);
        }
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
        self.base
            .dispatch_command(kind, command, output, host, budget)
    }
    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.base.enqueue_message(message)
    }
    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        self.base.dispatch_message(message, output, host, budget)
    }
    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.base.request_delete(host, budget)
    }
}

#[test]
fn fes_start_reads_the_running_parent_elapsed_clock_and_current_context() {
    for budget in [1, 4096] {
        let Setup {
            mut runtime,
            child,
            reads,
            ..
        } = setup(budget, b"#START\n$0=_FRAME_\n$1=$1+1\nwait 3\nexit 13\n");
        let parent = runtime
            .create(0, kind::BASE, b"driver", b"old parent")
            .unwrap();
        runtime.update(500, 9).unwrap();
        let child = child.get().unwrap();
        assert_eq!(runtime.tree().node(child).unwrap().parent, Some(parent));
        assert_eq!(runtime.object(child).unwrap().clock().time, 100);
        assert_eq!(
            runtime.object(child).unwrap().creation_context(),
            Some(b"updated parent".as_slice())
        );
        assert_eq!(runtime.numbers().shared[0..2], [1, 1]);
        runtime.update(501, 3).unwrap();
        assert_eq!(send(&mut runtime, child, 23, 0, 0), 13);
        runtime.update(503, 1).unwrap();
        assert_eq!(send(&mut runtime, child, 23, 0, 0), 13);
        assert_eq!(runtime.numbers().shared[0..2], [404, 2]);
        assert_eq!(*reads.borrow(), [b"complete.fes".to_vec()]);
    }
}

#[test]
fn failed_loads_preserve_the_task_and_input_modes_publish_system_values() {
    let Setup {
        mut runtime,
        queries,
        mask,
        reads,
        ..
    } = setup(
        1,
        b"#START\n$0=$0+(_TOPOBJ_!=0)\n$2=_CLICK_L_\nwait 1000\n$3=_CLICK_L_\nwait 1\n$4=_CLICK_L_\nexit 3\n",
    );
    let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
    send(&mut runtime, fes, 337, 0, 0);
    file(&mut runtime, fes, b"missing").unwrap();
    assert_eq!(send(&mut runtime, fes, 23, 0, 0), (-2i32) as u32);
    file(&mut runtime, fes, b"complete.fes").unwrap();
    mask.set(true);
    send(&mut runtime, 1, 151, 12, 0);
    queries.borrow_mut().clear();
    assert_eq!(send(&mut runtime, 1, 145, 12, u32::MAX), 0x8000000a);
    assert_eq!(
        *queries.borrow(),
        [
            (12, 0, false),
            (12, 1, false),
            (12, 2, false),
            (12, 3, false)
        ]
    );
    send(&mut runtime, fes, 339, 0, 0);
    assert_eq!(runtime.numbers().shared[0], 1);
    assert_eq!(runtime.numbers().shared[2], -1);
    runtime.update(1, 1).unwrap();
    assert_eq!(runtime.numbers().shared[0], 1);
    assert_eq!(runtime.numbers().shared[3], -1);
    mask.set(false);
    runtime.update(2, 1).unwrap();
    assert_eq!(runtime.numbers().shared[4], 0);
    assert_eq!(send(&mut runtime, fes, 23, 0, 0), 3);
    assert_eq!(
        *reads.borrow(),
        [b"missing".to_vec(), b"complete.fes".to_vec()]
    );
    assert!(runtime.object(fes).is_some());
}

#[test]
fn immediate_child_updates_can_delete_the_running_fes_owner_before_start() {
    for budget in [1, 4096] {
        let Setup {
            mut runtime,
            queries,
            ..
        } = setup(budget, b"#START\n$0=$0+1\nexit 3\n");
        let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        let child = runtime
            .create(fes, kind::BASE, b"delete parent", b"")
            .unwrap();
        queries.borrow_mut().clear();
        send(&mut runtime, fes, 339, 0, 0);
        assert_eq!(runtime.numbers().shared[0], 0);
        assert!(queries.borrow().is_empty());
        assert_eq!(runtime.object(fes).unwrap().control().deletion_requested, 1);
        assert!(runtime.update(1, 1).unwrap());
        assert!(runtime.object(fes).is_none());
        assert!(runtime.object(child).is_none());
    }
}

#[test]
fn first_update_creates_one_button_before_loading_or_starting_the_scene() {
    for depth in [1, 3, 5] {
        let Setup {
            mut runtime,
            button_attempts,
            ..
        } = setup(1, b"#START\nexit 0\n");
        let mut parent = 0;
        for _ in 1..depth {
            parent = runtime
                .create(parent, kind::BASE, b"ancestor", b"")
                .unwrap();
        }
        let fes = runtime.create(parent, kind::FES, b"unloaded", b"").unwrap();
        runtime.input_coordinates_mut().logical_size = [800, 600];
        runtime.host_mut().surface_size = [320, 240];
        runtime.update(123, 1).unwrap();
        let children: Vec<_> = runtime.tree().children(fes).unwrap().collect();
        assert_eq!(children.len(), 1);
        let button = children[0];
        assert_eq!(runtime.tree().node(button).unwrap().kind, kind::BUTTON);
        assert_eq!(runtime.object(button).unwrap().clock().time, 0);
        assert!(runtime.drawables().get(button).is_none());
        let region = runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(button)
            .unwrap();
        assert_eq!(region.priority(), depth);
        assert_eq!(region.event_mask(), 7);
        assert_eq!(
            (
                region.bounds().left,
                region.bounds().top,
                region.bounds().right,
                region.bounds().bottom
            ),
            (0, 0, 320, 240)
        );
        assert_eq!(region.enabled(), 1);
        runtime.host_mut().surface_size = [640, 480];
        runtime.update(124, 1).unwrap();
        assert_eq!(button_attempts.get(), 1);
        assert_eq!(runtime.tree().children(fes).unwrap().count(), 1);
        assert_eq!(runtime.object(button).unwrap().clock().time, 124);
        assert_eq!(
            runtime
                .input_state(1)
                .unwrap()
                .regions()
                .get(button)
                .unwrap()
                .bounds()
                .right,
            320
        );
        send(&mut runtime, button, 3, 0, 0);
        runtime.update(125, 1).unwrap();
        assert_eq!(runtime.tree().children(fes).unwrap().count(), 0);
        assert_eq!(button_attempts.get(), 1);
    }
}

#[test]
fn failed_first_button_creation_is_reported_once_and_does_not_reserve_an_id() {
    let Setup {
        mut runtime,
        fail_button,
        button_attempts,
        ..
    } = setup(1, b"#START\nexit 0\n");
    let fes = runtime.create(0, kind::FES, b"unloaded", b"").unwrap();
    fail_button.set(true);
    assert!(
        runtime
            .update(1, 1)
            .unwrap_err()
            .message
            .contains("按钮创建失败")
    );
    assert_eq!(button_attempts.get(), 1);
    assert_eq!(runtime.tree().children(fes).unwrap().count(), 0);
    fail_button.set(false);
    runtime.update(2, 1).unwrap();
    assert_eq!(button_attempts.get(), 1);
    assert_eq!(runtime.tree().children(fes).unwrap().count(), 0);
    assert_eq!(
        runtime.create(0, kind::BASE, b"unused", b"").unwrap(),
        fes + 1
    );
}

#[test]
fn auxiliary_input_reaches_script_values_and_transients_clear_after_the_round() {
    for budget in [1, 4096] {
        let Setup { mut runtime, pointer, .. } = setup(budget, b"#START\nwait 1000\n$0=_CLICK_L_\n$2=_MOUSE_X_\n$3=_MOUSE_Y_\n$4=_SCRL_U_\nwait 1\n$1=_CLICK_L_\n$5=_SCRL_U_\n$6=_MOUSE_X_\n$7=_MOUSE_Y_\nwait 20\n");
        pointer.set(PointerSample {
            position: [2, 3],
            active: 1,
        });
        runtime.input_coordinates_mut().logical_size = [8, 6];
        let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        runtime.update(100, 1).unwrap();
        send(&mut runtime, fes, 339, 0, 0);
        let button = runtime
            .tree()
            .children(fes)
            .unwrap()
            .find(|id| runtime.tree().node(*id).unwrap().kind == kind::BUTTON)
            .unwrap();
        runtime.update_input_selection(1, Some([2, 3])).unwrap();
        assert!(
            !runtime
                .dispatch_input_key(
                    1,
                    KeyEvent {
                        kind: KeyEventKind::Change,
                        code: 4,
                        key: 0,
                        pressed: 1,
                        mask: 2,
                        point: [2, 3],
                    }
                )
                .unwrap()
        );
        assert_eq!(send(&mut runtime, button, 4, 8, 1), 1);
        runtime.update(101, 1).unwrap();
        assert_eq!(runtime.numbers().shared[0], -1);
        assert_eq!(&runtime.numbers().shared[2..5], &[2, 3, -1]);
        pointer.set(PointerSample::default());
        runtime.update(102, 1).unwrap();
        assert_eq!(
            (runtime.numbers().shared[1], runtime.numbers().shared[5]),
            (0, 0)
        );
        assert_eq!(&runtime.numbers().shared[6..8], &[i32::MIN; 2]);
        assert_eq!(send(&mut runtime, button, 4, 0, 0), 1);
        assert_eq!(send(&mut runtime, fes, 64, button, 1), 0);
        assert_eq!(send(&mut runtime, fes, 65, button, 1), 0);
    }
}

#[test]
fn button_load_creates_each_declaration_before_start_and_rebinding_keeps_the_tasks() {
    let source = b"#DEFINE\nBUTTON item[2]\nBUTTON item[3]\n#OBJECT SIZE PRI ENABLE\nitem 2,2 5 21\nitem[2] 2,2 7 21\n#START\n$0=$0+1\nwait 99\n";
    for budget in [1, 4096] {
        let Setup {
            mut runtime, reads, ..
        } = setup(budget, source);
        let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        let buttons: Vec<_> = runtime.tree().children(fes).unwrap().collect();
        assert_eq!(buttons.len(), 5);
        for &id in &buttons {
            assert_eq!(runtime.tree().node(id).unwrap().kind, kind::BUTTON);
            assert!(runtime.input_state(1).unwrap().regions().get(id).is_none());
        }
        assert_eq!(runtime.numbers().shared[0], 0);
        let list = runtime.create(fes, kind::IMAGE_LIST, b"list", b"").unwrap();
        send(&mut runtime, list, 285, 8, 6);
        send(&mut runtime, fes, 340, 0, list);
        send(&mut runtime, fes, 339, 0, 0);
        assert_eq!(runtime.numbers().shared[0], 1);
        assert_eq!(*reads.borrow(), [b"complete.fes".to_vec()]);
        for (index, &id) in buttons.iter().enumerate() {
            let region = runtime.input_state(1).unwrap().regions().get(id);
            if index < 2 {
                assert!(region.is_none());
            } else {
                assert_eq!(region.unwrap().enabled(), 21);
            }
        }
        assert_eq!(runtime.tree().children(fes).unwrap().count(), 7);
        for button in buttons {
            assert!(
                runtime
                    .dispatch_command(0, command(fes, 64, button, 1))
                    .unwrap_err()
                    .message
                    .contains("BUTTON 对象事件尚未接入")
            );
        }
        assert_eq!(runtime.numbers().shared[0], 1);
        runtime.destroy(fes).unwrap();
        assert!(
            runtime
                .input_state(1)
                .unwrap()
                .regions()
                .records()
                .is_empty()
        );
    }
}

#[test]
fn dedicated_pause_hides_refreshed_media_and_wakes_with_current_fes_display() {
    let source = b"#DEFINE\nCIMAGE sprite\nCPLANE panel\nPLANE view\nBUTTON control\n#OBJECT SIZE PRI ENABLE\ncontrol 2,2 8 21\n#OBJECT SIZE COLOR PRI POS DISP\nsprite 2,2 0xff123456 5 1,1 21\npanel 1,1 0xff654321 100 5,4 94\n#OBJECT POS SIZE VRAM PRI DISP\nview 0,0 8,6 0,0 500 57\n#START\n$0=$0+1\nwait 77\n$1=$1+1\n#_FES_SLEEP_\n$2=$2+1\nsprite size 3 3\nsprite pri ($4+=1)\n#_FES_WAKE_\n$3=$3+1\nsprite size 4 4\npanel pri 99\n";
    for budget in [1, 4096] {
        let Setup { mut runtime, .. } = setup(budget, source);
        let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        let list = runtime.create(fes, kind::IMAGE_LIST, b"list", b"").unwrap();
        send(&mut runtime, list, 285, 8, 6);
        send(&mut runtime, fes, 340, 0, list);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        let control = runtime
            .tree()
            .children(fes)
            .unwrap()
            .find(|id| runtime.tree().node(*id).unwrap().kind == kind::BUTTON)
            .unwrap();
        send(&mut runtime, fes, 339, 0, 0);
        assert_eq!(
            runtime
                .input_state(1)
                .unwrap()
                .regions()
                .get(control)
                .unwrap()
                .enabled(),
            21
        );
        runtime.update(1, 1).unwrap();
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
            Some(0xff123456)
        );
        let planes: Vec<_> = runtime
            .tree()
            .children(fes)
            .unwrap()
            .filter(|id| runtime.tree().node(*id).unwrap().kind == kind::PLANE)
            .collect();
        assert_eq!(planes.len(), 2);
        for (value, count) in [(0, 0), (1, 1), (1, 1), (2, 2)] {
            send(&mut runtime, fes, 348, 0, value);
            assert_eq!(runtime.numbers().shared[2], count);
            assert_eq!(runtime.numbers().shared[4], count);
            assert_eq!(
                runtime
                    .input_state(1)
                    .unwrap()
                    .regions()
                    .get(control)
                    .unwrap()
                    .enabled(),
                if value == 0 { 21 } else { 0 }
            );
        }
        runtime.update(123, 1).unwrap();
        assert_eq!(&runtime.numbers().shared[..4], &[1, 0, 2, 0]);
        assert_eq!(runtime.object(fes).unwrap().clock().time, 123);
        assert_eq!(runtime.object(list).unwrap().clock().time, 123);
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
            Some(0)
        );
        for &id in &planes {
            assert_eq!(runtime.drawables().get(id).unwrap().display(), 0);
            assert_eq!(runtime.object(id).unwrap().clock().time, 123);
        }
        send(&mut runtime, list, 310, 0, 0);
        send(&mut runtime, fes, 348, 0, 0);
        assert_eq!(
            runtime
                .input_state(1)
                .unwrap()
                .regions()
                .get(control)
                .unwrap()
                .enabled(),
            21
        );
        assert_eq!(runtime.numbers().shared[3], 1);
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
            Some(0xff123456)
        );
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(4, 4),
            Some(0xff123456)
        );
        assert_eq!(
            runtime
                .drawables()
                .get(planes[0])
                .unwrap()
                .transform()
                .base_point()[2],
            99.
        );
        assert_eq!(runtime.drawables().get(planes[0]).unwrap().display(), 94);
        assert_eq!(runtime.drawables().get(planes[1]).unwrap().display(), 57);
        send(&mut runtime, fes, 348, 0, 0);
        assert_eq!(runtime.numbers().shared[3], 1);
        runtime.update(124, 1).unwrap();
        assert_eq!(&runtime.numbers().shared[..4], &[1, 1, 2, 1]);
    }
}

#[test]
fn pause_propagates_to_nested_fes_and_skips_the_first_body_until_wake() {
    for budget in [1, 4096] {
        let Setup {
            mut runtime,
            button_attempts,
            ..
        } = setup(
            budget,
            b"#START\n$0=$0+1\nwait 100\n#_FES_SLEEP_\n$2=$2*10+1\n#_FES_WAKE_\n$3=$3*10+2\n",
        );
        let parent = runtime.create(0, kind::FES, b"parent", b"").unwrap();
        let child = runtime.create(parent, kind::FES, b"child", b"").unwrap();
        for id in [parent, child] {
            send(&mut runtime, id, 337, 0, 0);
            file(&mut runtime, id, b"complete.fes").unwrap();
        }
        runtime
            .dispatch_command(
                0,
                Command {
                    data: [0, 2, 1],
                    ..command(parent, 348, 0, 2)
                },
            )
            .unwrap();
        assert_eq!(runtime.numbers().shared[2], 11);
        runtime.update(100, 1).unwrap();
        assert_eq!(button_attempts.get(), 0);
        assert_eq!(runtime.object(parent).unwrap().clock().time, 100);
        assert_eq!(runtime.object(child).unwrap().clock().time, 100);
        send(&mut runtime, parent, 339, 0, 0);
        send(&mut runtime, child, 339, 0, 0);
        assert_eq!(runtime.numbers().shared[0], 0);
        runtime
            .dispatch_command(
                0,
                Command {
                    data: [0, 0, 1],
                    ..command(parent, 348, 0, 0)
                },
            )
            .unwrap();
        assert_eq!(runtime.numbers().shared[3], 22);
        runtime.update(101, 1).unwrap();
        assert_eq!(button_attempts.get(), 2);
        assert_eq!(runtime.numbers().shared[0], 2);
    }
}

#[test]
fn media_failure_retries_pause_without_repeating_the_system_block() {
    for budget in [1, 4096] {
        let Setup { mut runtime, fail_media, reads, .. } = setup(budget,
            b"#DEFINE\nIMAGE sprite\n#OBJECT FILE PATHMODE SIZE PRI DISP\nsprite absent 1 2,2 5 1\n#START\nwait 77\n#_FES_SLEEP_\n$2=$2+1\n");
        let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        let list = runtime.create(fes, kind::IMAGE_LIST, b"list", b"").unwrap();
        send(&mut runtime, list, 285, 8, 6);
        send(&mut runtime, fes, 340, 0, list);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        fail_media.set(true);
        for _ in 0..2 {
            assert!(
                runtime
                    .dispatch_command(0, command(fes, 348, 0, 1))
                    .unwrap_err()
                    .message
                    .contains("媒体读取失败")
            );
            assert_eq!(runtime.numbers().shared[2], 1);
        }
        assert!(
            runtime
                .dispatch_command(0, command(fes, 348, 0, 0))
                .is_err()
        );
        fail_media.set(false);
        send(&mut runtime, fes, 348, 0, 1);
        assert_eq!(runtime.numbers().shared[2], 1);
        assert_eq!(
            reads
                .borrow()
                .iter()
                .filter(|name| name.as_slice() == b"absent")
                .count(),
            3
        );
        runtime.update(100, 1).unwrap();
    }
}

#[test]
fn system_exit_notifications_wait_for_the_body_and_main_exits_continue_the_script() {
    let source = b"#START\n$0=$0+1\nwait 77\nexit 37\n$1=$1+1\nwait 5\n#_FES_SLEEP_\n$2=$2+1\nexit 9\n$2=999\n#_FES_WAKE_\nexit 11\n";
    for budget in [1, 4096] {
        for drain_while_waiting in [false, true] {
            let Setup { mut runtime, .. } = setup(budget, source);
            let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
            send(&mut runtime, fes, 337, 0, 0);
            file(&mut runtime, fes, b"complete.fes").unwrap();
            send(&mut runtime, fes, 339, 0, 0);
            send(&mut runtime, fes, 24, 0, 73);
            send(&mut runtime, fes, 348, 0, 1);
            runtime.update(20, 1).unwrap();
            assert_eq!(send(&mut runtime, fes, 23, 0, 0), 73);
            send(&mut runtime, fes, 348, 0, 2);
            send(&mut runtime, fes, 348, 0, 0);
            assert_eq!(send(&mut runtime, fes, 23, 0, 0), 73);
            assert_eq!(runtime.numbers().shared[2], 2);
            if drain_while_waiting {
                runtime.update(21, 1).unwrap();
                assert_eq!(send(&mut runtime, fes, 23, 0, 0), 11);
                send(&mut runtime, fes, 24, 0, 91);
                runtime.update(22, 1).unwrap();
                assert_eq!(send(&mut runtime, fes, 23, 0, 0), 91);
            }
            runtime.update(77, 1).unwrap();
            assert_eq!(send(&mut runtime, fes, 23, 0, 0), 37);
            assert_eq!(&runtime.numbers().shared[..3], &[1, 1, 2]);
            send(&mut runtime, fes, 24, 0, 73);
            for now in [78, 82, 83] {
                runtime.update(now, 1).unwrap();
                assert_eq!(send(&mut runtime, fes, 23, 0, 0), 73);
            }
            assert_eq!(runtime.numbers().shared[0], 2);
            runtime.update(160, 1).unwrap();
            assert_eq!(send(&mut runtime, fes, 23, 0, 0), 37);
            assert_eq!(&runtime.numbers().shared[..3], &[2, 2, 2]);
        }
    }
}

#[test]
fn load_retry_keeps_a_record_wrapper_created_before_a_failed_button() {
    for budget in [1, 4096] {
        let mut snapshots = Vec::new();
        for fail in [false, true] {
            let Setup {
                mut runtime,
                fail_button,
                reads,
                ..
            } = setup(
                budget,
                b"#DEFINE\nBUTTON item\nBUTTON item\n#START\n$0=$0+1\nwait 99\n",
            );
            let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
            send(&mut runtime, fes, 337, 0, 0);
            fail_button.set(fail);
            if fail {
                assert!(
                    file(&mut runtime, fes, b"complete.fes")
                        .unwrap_err()
                        .message
                        .contains("按钮创建失败")
                );
                assert_eq!(runtime.tree().children(fes).unwrap().count(), 0);
                fail_button.set(false);
            }
            file(&mut runtime, fes, b"complete.fes").unwrap();
            let children: Vec<_> = runtime.tree().children(fes).unwrap().collect();
            assert_eq!(children.len(), 2);
            assert_eq!(runtime.numbers().shared[0], 0);
            assert_eq!(*reads.borrow(), [b"complete.fes".to_vec()]);
            snapshots.push((
                children,
                runtime.host().handle_allocator().allocate().unwrap(),
            ));
        }
        assert_eq!(snapshots[0], snapshots[1]);
    }
}
