use katze_system2::{
    Error, Result,
    audio::SoundFactory,
    expression::State,
    graphics::GraphicsFactory,
    input::InputFactory,
    kcs::{OrdinaryTask, Program, ResourceServices},
    message::MessageFactory,
    resource::ResourceMode,
    runtime::{
        KcsFactory, ObjectFactory, OrdinaryObject, ProgramLoader, RootObject, Runtime,
        fes::FesFactory, kind,
    },
    task::{Command, CommandBuffers},
};
use std::{
    cell::{Cell, RefCell},
    io::Write,
    rc::Rc,
};

fn ins(code: &mut Vec<u8>, op: u8, values: &[u32]) {
    code.push(op);
    for value in values {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}

fn program() -> Program {
    let mut code = Vec::new();
    ins(&mut code, 0x1e, &[4, 1]);
    ins(&mut code, 0x2d, &[]);
    ins(&mut code, 0x2b, &[65, 0]);
    ins(&mut code, 0x1e, &[4, 10]);
    ins(&mut code, 0x2d, &[]);
    code.push(0x3b);
    Program::from_code(&code, vec![0; 64], 128).unwrap()
}

struct Loader {
    fail: Rc<Cell<bool>>,
    loads: Rc<RefCell<Vec<Vec<u8>>>>,
}

impl ProgramLoader for Loader {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        self.loads.borrow_mut().push(name.to_vec());
        if self.fail.get() {
            return Err(Error::new(0, "合成程序读取失败"));
        }
        Ok((name != b"missing.kcs").then(program))
    }
}

fn command(target: u32, code: u32, data: [u32; 3]) -> Command {
    Command {
        target,
        sender: 0,
        code,
        data,
    }
}

fn load<F: ObjectFactory>(
    runtime: &mut Runtime<F>,
    target: u32,
    mode: u32,
    name: &[u8],
) -> Result<u32> {
    let mut name = name.to_vec();
    runtime.dispatch_command_with_buffers(
        0,
        command(target, 161, [mode, 1, 0]),
        &mut CommandBuffers {
            data1: Some(&mut name),
            ..CommandBuffers::default()
        },
    )
}

fn counter<F: ObjectFactory>(runtime: &Runtime<F>, target: u32) -> u32 {
    u32::from_le_bytes(
        runtime
            .object(target)
            .unwrap()
            .interpreter()
            .unwrap()
            .read(4, 4)
            .unwrap()
            .try_into()
            .unwrap(),
    )
}

#[test]
fn load_initialization_preserves_task_state_and_missing_program_removes_the_vm() {
    for budget in [1, 4096] {
        let loads = Rc::new(RefCell::new(Vec::new()));
        let fail = Rc::new(Cell::new(false));
        let mut parent =
            OrdinaryTask::new(Program::from_code(&[0x3b], vec![], 0).unwrap(), vec![]).unwrap();
        parent.creation_context = b"parent context".to_vec();
        let mut runtime = Runtime::new(
            0,
            kind::ORDINARY,
            Box::new(OrdinaryObject::new(parent)),
            State::new(8),
            KcsFactory::new(Loader {
                fail,
                loads: loads.clone(),
            }),
        );
        runtime.set_budget(budget, 10000);
        let id = runtime.create_unloaded_ordinary(0, b"old").unwrap();
        assert!(loads.borrow().is_empty());
        assert!(runtime.object(id).unwrap().interpreter().is_none());
        let (_, clock) = runtime.object_mut(id).unwrap().state();
        clock.origin = 7;
        clock.time = 19;
        clock.paused = -1;
        runtime
            .dispatch_command(0, command(id, 24, [0, 37, 0]))
            .unwrap();

        load(&mut runtime, id, 0, b"child.kcs").unwrap();
        assert_eq!(counter(&runtime, id), 0);
        assert_eq!(
            runtime.object(id).unwrap().creation_context(),
            Some(b"parent context".as_slice())
        );
        let clock = runtime.object(id).unwrap().clock().clone();
        let mut available = [0; 4];
        runtime
            .dispatch_command_with_buffers(
                0,
                command(id, 177, [0, 1, 0]),
                &mut CommandBuffers {
                    data1: Some(&mut available),
                    ..CommandBuffers::default()
                },
            )
            .unwrap();
        assert_eq!(u32::from_le_bytes(available), 1);
        runtime
            .object_mut(id)
            .unwrap()
            .enqueue_message([0, 2, 3, 4])
            .unwrap();
        load(&mut runtime, id, 1, b"child.kcs").unwrap();
        assert_eq!(counter(&runtime, id), 1);
        assert_eq!(
            runtime.object(id).unwrap().queued_messages().unwrap(),
            [[0, 2, 3, 4]]
        );
        assert_eq!(runtime.object(id).unwrap().clock(), &clock);
        assert_eq!(runtime.object(id).unwrap().control().property, 37);
        runtime
            .dispatch_command(0, command(id, 172, [0; 3]))
            .unwrap();
        assert_eq!(counter(&runtime, id), 1);
        runtime.update(50, 3).unwrap();
        assert_eq!(counter(&runtime, id), 11);

        load(&mut runtime, id, 1, b"missing.kcs").unwrap();
        assert!(runtime.object(id).unwrap().interpreter().is_none());
        assert_eq!(runtime.object(id).unwrap().control().property, 37);
        assert_eq!(
            runtime.object(id).unwrap().creation_context(),
            Some(b"parent context".as_slice())
        );
        runtime
            .dispatch_command(0, command(id, 172, [99, 1, 0]))
            .unwrap();
        runtime.update(60, 3).unwrap();
        assert_eq!(runtime.object(id).unwrap().clock().time, 19);
        load(&mut runtime, id, u32::MAX, b"child.kcs").unwrap();
        assert_eq!(counter(&runtime, id), 1);
        runtime.dispatch_command(0, command(id, 3, [0; 3])).unwrap();
        load(&mut runtime, id, 1, b"ignored.kcs").unwrap();
        assert_eq!(loads.borrow().len(), 4);
    }
}

struct Files(Vec<u8>);
impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"kcs");
        assert_eq!(name, b"child");
        Ok(b"child.kcs\0ignored".to_vec())
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(name, b"parent.fes");
        assert_eq!(mode, ResourceMode::LooseFirst);
        Ok(Some(self.0.clone()))
    }
}

struct Device;
impl katze_system2::input::InputDevice for Device {
    fn poll(&mut self) -> Result<()> {
        Ok(())
    }
    fn pointer(&mut self) -> Result<katze_system2::input::PointerSample> {
        Ok(Default::default())
    }
    fn key(&mut self, _: u32, _: u32, _: bool) -> Result<katze_system2::input::KeySample> {
        Ok(Default::default())
    }
    fn take_wheel(&mut self) -> Result<u32> {
        Ok(0)
    }
    fn is_foreground(&mut self) -> Result<bool> {
        Ok(false)
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
        Ok(0)
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

#[test]
fn fes_child_load_retries_on_the_same_object_then_returns_status_and_destroys_normally() {
    for budget in [1, 4096] {
        let fail = Rc::new(Cell::new(true));
        let loads = Rc::new(RefCell::new(Vec::new()));
        let factory = MessageFactory {
            fallback: FesFactory {
                fallback: GraphicsFactory {
                    fallback: InputFactory {
                        fallback: SoundFactory {
                            enabled: false,
                            fallback: KcsFactory::new(Loader {
                                fail: fail.clone(),
                                loads: loads.clone(),
                            }),
                        },
                    },
                },
            },
        };
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(RootObject::new(1, 2)),
            State::new(8),
            factory,
        );
        runtime.set_budget(budget, 10000);
        runtime.set_input_device(Device).unwrap();
        runtime.drawing_mut().target =
            Some(katze_system2::graphics::RenderTarget::new([8, 6]).unwrap());
        runtime.create(0, kind::INPUT, b"input", b"").unwrap();
        runtime.create(0, kind::SOUND, b"sound", b"").unwrap();
        runtime.set_resources(Files(packed(b"#START\nexeckcs child ($7+=100)\n$0+=1\nwait 1\nwait 1\n$1=_EXITCODE_\nendkcs\n$2+=1\nwait 100\n")));
        let parent = runtime.create(0, kind::FES, b"parent", b"").unwrap();
        runtime
            .dispatch_command(0, command(parent, 337, [0; 3]))
            .unwrap();
        runtime
            .object_mut(parent)
            .unwrap()
            .set_creation_context(b"parent context")
            .unwrap();
        let list = runtime
            .create(parent, kind::IMAGE_LIST, b"list", b"")
            .unwrap();
        runtime
            .dispatch_command(0, command(list, 285, [8, 6, 0]))
            .unwrap();
        runtime
            .dispatch_command(0, command(parent, 340, [0, list, 0]))
            .unwrap();
        let mut name = b"parent.fes\0".to_vec();
        runtime
            .dispatch_command_with_buffers(
                0,
                command(parent, 338, [0, 1, 0]),
                &mut CommandBuffers {
                    data1: Some(&mut name),
                    ..CommandBuffers::default()
                },
            )
            .unwrap();
        let error = runtime
            .dispatch_command(0, command(parent, 339, [0; 3]))
            .unwrap_err();
        assert_eq!(error.message, "合成程序读取失败");
        let children: Vec<_> = runtime
            .tree()
            .children(parent)
            .unwrap()
            .filter(|&id| runtime.tree().node(id).unwrap().kind == kind::ORDINARY)
            .collect();
        assert_eq!(children.len(), 1);
        let child = children[0];
        assert!(runtime.object(child).unwrap().interpreter().is_none());
        assert_eq!(runtime.numbers().shared[0], 0);
        fail.set(false);
        runtime
            .dispatch_command(0, command(parent, 339, [0; 3]))
            .unwrap();
        assert_eq!(counter(&runtime, child), 1);
        assert_eq!(
            runtime.object(child).unwrap().creation_context(),
            Some(b"parent context".as_slice())
        );
        assert_eq!(runtime.numbers().shared[0], 1);
        assert_eq!(runtime.numbers().shared[7], 0);
        assert_eq!(
            *loads.borrow(),
            [b"child.kcs".to_vec(), b"child.kcs".to_vec()]
        );
        runtime
            .dispatch_command(0, command(child, 24, [0, 37, 0]))
            .unwrap();
        runtime.update(1, 1).unwrap();
        assert_eq!(counter(&runtime, child), 11);
        runtime.update(2, 1).unwrap();
        assert_eq!(runtime.numbers().shared[1], 37);
        assert_eq!(
            runtime.object(child).unwrap().control().deletion_requested,
            1
        );
        runtime.update(3, 1).unwrap();
        assert!(runtime.object(child).is_none());
        assert_eq!(runtime.numbers().shared[2], 1);
    }
}

#[test]
fn native_load_initializes_before_binding_and_nested_messages_share_the_live_external_data() {
    use katze_system2::kcs::SharedMemory;
    use katze_system2::runtime::BaseObject;

    let assign = |code: &mut Vec<u8>, address, value| {
        ins(code, 0x15, &[address, value]);
        code.push(0x2d);
    };
    let call = |code: &mut Vec<u8>, index, arguments: &[u32]| {
        for &value in arguments {
            ins(code, 0x25, &[value]);
        }
        ins(code, 0x2b, &[index, arguments.len() as u32 * 4]);
    };
    let mut caller = vec![];
    ins(&mut caller, 0x25, &[0xdead_beef]);
    call(&mut caller, 143, &[1, 128, 1]);
    ins(&mut caller, 0x2f, &[0x8000_0000]);
    ins(&mut caller, 0x26, &[0]);
    assign(&mut caller, 0x8000_0000, 0x1122_3344);
    call(&mut caller, 69, &[1, 99, 0, 0]);
    ins(&mut caller, 0x26, &[8]);
    ins(&mut caller, 0x2f, &[0x8000_0000]);
    ins(&mut caller, 0x26, &[4]);
    call(&mut caller, 65, &[]);
    caller.push(0x3b);
    let mut data = vec![0; 256];
    data[128..134].copy_from_slice(b"child\0");
    let caller = Program::from_code(&caller, data, 128).unwrap();
    let mut child = vec![];
    assign(&mut child, 0x8000_0000, 0xaabb_ccdd);
    call(&mut child, 71, &[32]);
    ins(&mut child, 0x26, &[0]);
    ins(&mut child, 0x2f, &[0x8000_0000]);
    ins(&mut child, 0x26, &[4]);
    assign(&mut child, 0x8000_0000, 0x5566_7788);
    call(&mut child, 71, &[32]);
    child.push(0x3b);
    let child = Program::from_code(&child, vec![0; 64], 128).unwrap();

    struct SharedLoader {
        caller: Program,
        child: Program,
    }
    impl ProgramLoader for SharedLoader {
        fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
            Ok(match name {
                b"caller" => Some(self.caller.clone()),
                b"child.kcs" => Some(self.child.clone()),
                _ => None,
            })
        }
    }
    struct Names;
    impl ResourceServices for Names {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"kcs");
            assert_eq!(name, b"child");
            Ok(b"child.kcs".to_vec())
        }
        fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            panic!("the program loader owns program reads")
        }
    }
    let word = |vm: &katze_system2::kcs::Interpreter, address| {
        u32::from_le_bytes(vm.read(address, 4).unwrap().try_into().unwrap())
    };
    for budget in [1, 4096] {
        let mut factory = KcsFactory::new(SharedLoader {
            caller: caller.clone(),
            child: child.clone(),
        });
        factory.external_bytes = 16;
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(8),
            factory,
        );
        runtime.set_budget(budget, 10000);
        runtime.set_resources(Names);
        let target = runtime.create_unloaded_ordinary(0, b"target").unwrap();
        let source = runtime.create(0, kind::ORDINARY, b"caller", b"").unwrap();
        assert_eq!((target, source), (1, 2));
        let old = runtime.object(target).unwrap().external_storage().unwrap();
        let mut bytes = vec![0; 16];
        bytes[..4].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        runtime
            .dispatch_command_with_buffers(
                0,
                command(source, 172, [16, 0, 0]),
                &mut CommandBuffers {
                    external: Some(SharedMemory::new(bytes).unwrap()),
                    ..CommandBuffers::default()
                },
            )
            .unwrap();
        runtime.update(0, 1).unwrap();
        assert_eq!(&*old.read(0, 4).unwrap(), &0xaabb_ccddu32.to_le_bytes());
        let source_vm = runtime.object(source).unwrap().interpreter().unwrap();
        assert_eq!(word(source_vm, 0), 0);
        assert_eq!(word(source_vm, 0x8000_0000), 0x0102_0304);
        assert_eq!(source_vm.stack_bytes(), 0xdead_beefu32.to_le_bytes());
        let target_vm = runtime.object(target).unwrap().interpreter().unwrap();
        assert_eq!(word(target_vm, 0x8000_0000), 0x0102_0304);
        assert!(target_vm.is_waiting_for_message());
        runtime
            .dispatch_command(0, command(target, 163, [0; 3]))
            .unwrap();
        runtime.update(1, 1).unwrap();
        let source_vm = runtime.object(source).unwrap().interpreter().unwrap();
        assert_eq!(
            [word(source_vm, 0), word(source_vm, 4), word(source_vm, 8)],
            [0x0102_0304, 0x5566_7788, 1]
        );
        assert_eq!(source_vm.stack_bytes(), 0xdead_beefu32.to_le_bytes());
        assert_eq!(
            word(runtime.object(target).unwrap().interpreter().unwrap(), 4),
            0x1122_3344
        );
        runtime.destroy(source).unwrap();
        assert_eq!(
            word(
                runtime.object(target).unwrap().interpreter().unwrap(),
                0x8000_0000
            ),
            0x5566_7788
        );
        load(&mut runtime, target, 0, b"missing.kcs").unwrap();
        assert!(runtime.object(target).unwrap().interpreter().is_none());
        load(&mut runtime, target, 0, b"child.kcs").unwrap();
        assert_eq!(
            word(
                runtime.object(target).unwrap().interpreter().unwrap(),
                0x8000_0000
            ),
            0x5566_7788
        );
    }
}
