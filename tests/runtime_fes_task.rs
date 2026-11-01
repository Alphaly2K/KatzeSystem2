use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{GraphicsFactory, Projection, RenderTarget, TextureFilter, Transform},
    input::{InputDevice, InputFactory, KeySample, PointerSample},
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
type TestRuntime = Runtime<FesFactory<GraphicsFactory<InputFactory<Factory>>>>;

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
}
impl ResourceServices for Files {
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.reads.borrow_mut().push(name.to_vec());
        Ok(self.files.get(name).cloned())
    }
}

struct Device {
    queries: Queries,
    mask: Rc<Cell<bool>>,
}
impl InputDevice for Device {
    fn poll(&mut self) -> Result<()> {
        Ok(())
    }
    fn pointer(&mut self) -> Result<PointerSample> {
        Ok(PointerSample::default())
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
}

fn setup(budget: usize, source: &[u8]) -> Setup {
    let queries = Rc::new(RefCell::new(Vec::new()));
    let mask = Rc::new(Cell::new(false));
    let child = Rc::new(Cell::new(None));
    let reads = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(RootObject::new(1, 2)),
        State::new(8),
        FesFactory {
            fallback: GraphicsFactory {
                fallback: InputFactory {
                    fallback: Factory {
                        child: child.clone(),
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
        })
        .unwrap();
    runtime.set_resources(Files {
        files: BTreeMap::from([(b"complete.fes".to_vec(), packed(source))]),
        reads: reads.clone(),
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
        assert_eq!(runtime.tree().children(fes).unwrap().count(), 2);
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
        assert_eq!(runtime.numbers().shared[0..2], [1, 1]);
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
        assert_eq!(runtime.numbers().shared[0..2], [1, 1]);
        assert_eq!(*reads.borrow(), [b"complete.fes".to_vec()]);
    }
}

#[test]
fn failed_loads_preserve_the_task_and_input_queries_gate_script_execution() {
    let Setup {
        mut runtime,
        queries,
        mask,
        reads,
        ..
    } = setup(1, b"#START\n$0=$0+(_TOPOBJ_!=0)\nexit 3\n");
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
    assert!(
        runtime
            .dispatch_command(0, command(fes, 339, 0, 0))
            .is_err()
    );
    assert_eq!(runtime.numbers().shared[0], 0);
    mask.set(false);
    send(&mut runtime, fes, 339, 0, 0);
    assert_eq!(runtime.numbers().shared[0], 1);
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
