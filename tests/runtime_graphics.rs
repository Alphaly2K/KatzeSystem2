use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{CanvasLayout, Drawable, GraphicsFactory},
    kcs::{Host, Interpreter, Program},
    runtime::{BaseObject, KcsFactory, ObjectFactory, ProgramLoader, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock},
};

struct BaseFactory;

impl ObjectFactory for BaseFactory {
    fn create(&mut self, _id: u32, object_kind: u32, _name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if object_kind == kind::BASE {
            Ok(Box::new(BaseObject::new()))
        } else {
            Err(Error::new(0, "合成工厂未提供该类型"))
        }
    }
}

#[test]
fn plane_factory_commands_and_lifecycle_share_the_runtime_drawing_registry() {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory {
            fallback: BaseFactory,
        },
    );
    let id = runtime.create(0, kind::PLANE, b"plane", b"").unwrap();
    assert_eq!(id, 1);
    assert!(runtime.object(id).unwrap().interpreter().is_none());
    assert!(runtime.drawables().get(id).is_none());
    runtime
        .drawables_mut()
        .insert(id, Drawable::default())
        .unwrap();
    let command = |code, a, b| Command {
        target: id,
        sender: 0,
        code,
        data: [a, b, 0],
    };
    assert_eq!(
        runtime.dispatch_command(0, command(13, 0, 0)).unwrap(),
        kind::PLANE
    );
    runtime
        .dispatch_command(0, command(59, u32::MAX, 0x0012_3456))
        .unwrap();
    assert_eq!(
        runtime.drawables().get(id).unwrap().color(),
        (0x0012_3456, u32::MAX)
    );
    runtime.dispatch_command(0, command(63, 0, 0x1234)).unwrap();
    assert_eq!(
        runtime.drawables().get(id).unwrap().color(),
        (0x3412_3456, 1)
    );
    runtime.dispatch_command(0, command(81, 0, 5)).unwrap();
    assert_eq!(runtime.drawables().get(id).unwrap().blend_mode(), 5);
    runtime
        .dispatch_command(0, command(81, 0, u32::MAX))
        .unwrap();
    assert_eq!(runtime.drawables().get(id).unwrap().blend_mode(), 0);
    runtime.dispatch_command(0, command(24, 77, 19)).unwrap();
    assert_eq!(runtime.dispatch_command(0, command(23, 0, 0)).unwrap(), 19);
    runtime
        .dispatch_command(0, command(29, 0, u32::MAX))
        .unwrap();
    assert_eq!(runtime.object(id).unwrap().clock().paused, -1);
    for code in [60, 71, 85, 86] {
        runtime
            .dispatch_command(0, command(code, u32::MAX, 0))
            .unwrap();
    }
    let error = runtime
        .dispatch_command(0, command(82, 0x1234, 0))
        .unwrap_err();
    assert!(error.message.contains("二维对象命令 82"));
    runtime.dispatch_command(0, command(68, 0, 0)).unwrap();
    assert!(runtime.drawables().get(id).is_none());
    assert!(runtime.object(id).is_some());
    runtime
        .drawables_mut()
        .insert(id, Drawable::default())
        .unwrap();
    runtime.dispatch_command(0, command(3, 0, 0)).unwrap();
    assert!(runtime.drawables().get(id).is_none());
    assert_eq!(runtime.object(id).unwrap().control().deletion_requested, 1);
    assert_eq!(runtime.dispatch_command(0, command(13, 0, 0)).unwrap(), 0);
    runtime.update(10, 1).unwrap();
    assert!(runtime.object(id).is_none());
    assert_eq!(runtime.create(0, kind::PLANE, b"next", b"").unwrap(), id);
    runtime
        .drawables_mut()
        .insert(id, Drawable::default())
        .unwrap();
    runtime.destroy(id).unwrap();
    assert!(runtime.drawables().get(id).is_none());
    runtime.update(20, 1).unwrap();
    assert!(
        runtime
            .create(0, kind::PLANE, b"context", b"unsupported")
            .is_err()
    );
    assert_eq!(runtime.create(0, kind::BASE, b"base", b"").unwrap(), id);
}

fn command(target: u32, code: u32) -> Command {
    Command {
        target,
        sender: 0,
        code,
        data: [0; 3],
    }
}

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn buffered<F: ObjectFactory>(
    runtime: &mut Runtime<F>,
    id: u32,
    code: u32,
    data: &mut [u8],
) -> Result<u32> {
    runtime.dispatch_command_with_buffers(
        0,
        command(id, code),
        &mut CommandBuffers {
            data0: None,
            data1: Some(data),
        },
    )
}

#[test]
fn bound_planes_query_running_ancestors_and_stop_inheriting_after_unbind() {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory {
            fallback: BaseFactory,
        },
    );
    let parent = runtime.create(0, kind::PLANE, b"parent", b"").unwrap();
    let child = runtime.create(parent, kind::PLANE, b"child", b"").unwrap();
    let grandchild = runtime
        .create(child, kind::PLANE, b"grandchild", b"")
        .unwrap();
    for id in [parent, grandchild] {
        runtime
            .drawables_mut()
            .insert(id, Drawable::default())
            .unwrap();
    }
    runtime
        .drawables_mut()
        .insert(
            child,
            Drawable::from_canvas_source(
                Drawable::default(),
                CanvasLayout::new([8, 8], [1, 1], [8, 8]).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    let vector = |runtime: &mut Runtime<_>, id, code, value: [f32; 3]| {
        buffered(runtime, id, code, &mut bytes(&value.map(f32::to_bits))).unwrap();
    };
    vector(&mut runtime, parent, 49, [10., 20., 100.]);
    vector(&mut runtime, parent, 51, [100., 200., 3.]);
    vector(&mut runtime, child, 53, [2., 3., 4.]);
    vector(&mut runtime, grandchild, 53, [1., 2., 5.]);
    assert_eq!(
        runtime
            .drawables()
            .get(child)
            .unwrap()
            .transform()
            .base_point(),
        [8., 17., 104.]
    );
    assert_eq!(
        runtime
            .drawables()
            .get(grandchild)
            .unwrap()
            .transform()
            .base_point(),
        [7., 15., 109.]
    );
    let mut source_position = bytes(&[7f32, 8., 9.].map(f32::to_bits));
    let mut source_command = command(child, 51);
    source_command.data[0] = 1;
    runtime
        .dispatch_command_with_buffers(
            0,
            source_command,
            &mut CommandBuffers {
                data1: Some(&mut source_position),
                ..CommandBuffers::default()
            },
        )
        .unwrap();
    assert_eq!(
        runtime
            .drawables()
            .get(child)
            .unwrap()
            .geometry_transform(true)
            .unwrap()
            .position(),
        [7., 8., 9.]
    );
    assert_eq!(
        runtime
            .drawables()
            .get(child)
            .unwrap()
            .transform()
            .position(),
        [100., 200., 3.]
    );

    vector(&mut runtime, parent, 51, [30., 40., 50.]);
    for id in [child, grandchild] {
        assert_eq!(
            runtime.drawables().get(id).unwrap().transform().position(),
            [30., 40., 50.]
        );
    }
    // 绑定的自身位置命令重新查询父级，不需要读取传入向量。
    runtime.dispatch_command(0, command(child, 51)).unwrap();
    assert_eq!(
        runtime
            .drawables()
            .get(child)
            .unwrap()
            .transform()
            .position(),
        [30., 40., 50.]
    );
    vector(&mut runtime, parent, 55, [0., 0., 0.75]);
    vector(&mut runtime, parent, 66, [2., 3., 4.]);
    for id in [child, grandchild] {
        let transform = runtime.drawables().get(id).unwrap().transform();
        assert_eq!(transform.rotation(), [0., 0., 0.75]);
        assert_eq!(transform.scale(), [2., 3., 4.]);
    }
    let matrix = [1f32; 16];
    buffered(
        &mut runtime,
        parent,
        57,
        &mut bytes(&matrix.map(f32::to_bits)),
    )
    .unwrap();
    assert_eq!(
        runtime
            .drawables()
            .get(grandchild)
            .unwrap()
            .transform()
            .matrix_override(),
        Some(&matrix)
    );

    runtime.dispatch_command(0, command(child, 54)).unwrap();
    vector(&mut runtime, parent, 51, [70., 80., 90.]);
    vector(&mut runtime, parent, 55, [0.25, 0., 0.]);
    let transform = runtime.drawables().get(child).unwrap().transform();
    assert_eq!(transform.position(), [30., 40., 50.]);
    assert_eq!(transform.rotation(), [0., 0., 0.75]);
    // 未绑定的中间节点仍向下传播；孙级绑定的是它的坐标。
    assert_eq!(
        runtime
            .drawables()
            .get(grandchild)
            .unwrap()
            .transform()
            .position(),
        [30., 40., 50.]
    );
    assert_eq!(
        runtime
            .drawables()
            .get(grandchild)
            .unwrap()
            .transform()
            .rotation(),
        [0.25, 0., 0.]
    );
}

#[test]
fn buffered_geometry_keeps_endpoints_queries_and_replacement_state() {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory {
            fallback: BaseFactory,
        },
    );
    let id = runtime.create(0, kind::PLANE, b"plane", b"").unwrap();
    // Endpoints round before the object derives its dimensions.
    let mut rectangle = bytes(&[
        16777216f32.to_bits(),
        (-7f32).to_bits(),
        6f32.to_bits(),
        1f32.to_bits(),
        (-5f32).to_bits(),
        0,
        0,
        0xff123456,
    ]);
    buffered(&mut runtime, id, 42, &mut rectangle).unwrap();
    let drawable = runtime.drawables().get(id).unwrap();
    assert_eq!(
        drawable.bounds(),
        [[16777216., -7., 0.], [16777216., -12., 0.]]
    );
    assert_eq!(drawable.triangles()[0][2].position, [16777216., -12., 6.]);
    assert_eq!(drawable.triangles()[1][1].position, [16777216., -12., 6.]);
    let base = [0x80000000, 0x7fc12345, 5000f32.to_bits()];
    let position = [(-3f32).to_bits(), 7f32.to_bits(), 8f32.to_bits()];
    buffered(&mut runtime, id, 49, &mut bytes(&base)).unwrap();
    buffered(&mut runtime, id, 51, &mut bytes(&position)).unwrap();
    buffered(&mut runtime, id, 66, &mut bytes(&[2f32.to_bits(); 3])).unwrap();
    let mut matrix = [0u32; 16];
    for i in [0, 5, 10, 15] {
        matrix[i] = 1f32.to_bits();
    }
    matrix[12] = 9f32.to_bits();
    buffered(&mut runtime, id, 57, &mut bytes(&matrix)).unwrap();
    assert!(
        runtime
            .drawables()
            .get(id)
            .unwrap()
            .transform()
            .matrix_override()
            .is_some()
    );
    buffered(&mut runtime, id, 55, &mut bytes(&[0; 3])).unwrap();
    assert!(
        runtime
            .drawables()
            .get(id)
            .unwrap()
            .transform()
            .matrix_override()
            .is_none()
    );
    assert_eq!(
        runtime.drawables().get(id).unwrap().transform().scale(),
        [2.; 3]
    );
    for (code, expected) in [(61, base), (62, position)] {
        let mut output = [0xa5; 16];
        buffered(&mut runtime, id, code, &mut output).unwrap();
        assert_eq!(&output[..12], bytes(&expected));
        assert_eq!(&output[12..], &[0xa5; 4]);
    }
    for (code, a_expected, b_expected) in [
        (75, vec![0], vec![5f32.to_bits()]),
        (76, vec![0; 3], vec![0, 5f32.to_bits(), 0]),
    ] {
        let (mut a, mut b) = ([0xa5; 16], [0xa5; 16]);
        runtime
            .dispatch_command_with_buffers(
                0,
                command(id, code),
                &mut CommandBuffers {
                    data0: Some(&mut a),
                    data1: Some(&mut b),
                },
            )
            .unwrap();
        for (actual, expected) in [(a, a_expected), (b, b_expected)] {
            let expected = bytes(&expected);
            assert_eq!(&actual[..expected.len()], expected);
            assert!(actual[expected.len()..].iter().all(|b| *b == 0xa5));
        }
    }
    runtime
        .dispatch_command(
            0,
            Command {
                data: [0, 2, 0],
                ..command(id, 84)
            },
        )
        .unwrap();
    buffered(&mut runtime, id, 42, &mut rectangle).unwrap();
    assert!(runtime.drawables().get(id).is_none());
    // Missing registrations leave vector output untouched, even without a buffer.
    runtime.dispatch_command(0, command(id, 61)).unwrap();
    runtime
        .dispatch_command(
            0,
            Command {
                data: [0, 1, 0],
                ..command(id, 84)
            },
        )
        .unwrap();
    buffered(&mut runtime, id, 42, &mut rectangle).unwrap();
    assert_eq!(
        runtime.drawables().get(id).unwrap().transform().position(),
        [0.; 3]
    );
    runtime.dispatch_command(0, command(id, 45)).unwrap();
    assert!(runtime.drawables().get(id).unwrap().triangles().is_empty());
    runtime.dispatch_command(0, command(id, 68)).unwrap();
    let (mut a, mut b) = ([0xa5; 4], [0xa5; 4]);
    runtime
        .dispatch_command_with_buffers(
            0,
            command(id, 75),
            &mut CommandBuffers {
                data0: Some(&mut a),
                data1: Some(&mut b),
            },
        )
        .unwrap();
    assert_eq!((a, b), ([0; 4], [0; 4]));
}

#[test]
fn buffer_errors_are_diagnostic_and_deletion_gates_precede_buffer_access() {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory {
            fallback: BaseFactory,
        },
    );
    let id = runtime.create(0, kind::PLANE, b"plane", b"").unwrap();
    runtime.dispatch_command(0, command(id, 45)).unwrap();
    for (code, length) in [(49, 11), (57, 63), (61, 11)] {
        assert!(
            buffered(&mut runtime, id, code, &mut vec![0; length])
                .unwrap_err()
                .message
                .contains("缓冲区")
        );
    }
    assert!(buffered(&mut runtime, id, 42, &mut [0; 31]).is_err());
    assert!(runtime.drawables().get(id).is_none());
    runtime.dispatch_command(0, command(id, 3)).unwrap();
    for code in [42, 49, 57, 61, 75, 76] {
        assert_eq!(runtime.dispatch_command(0, command(id, code)).unwrap(), 0);
    }
}

type Records = std::rc::Rc<std::cell::RefCell<Vec<[u32; 4]>>>;
struct Recorder {
    id: u32,
    base: BaseObject,
    records: Records,
}
impl TaskObject for Recorder {
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
    fn set_creation_context(&mut self, _: &[u8]) -> Result<()> {
        Ok(())
    }
    fn update_body(&mut self, _: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        Ok(true)
    }
    fn dispatch_command(
        &mut self,
        _: u32,
        _: &Command,
        _: &mut u32,
        _: &mut dyn Host,
        _: usize,
    ) -> Result<CommandOutcome> {
        panic!("buffered dispatch required")
    }
    fn dispatch_command_with_buffers(
        &mut self,
        _: u32,
        c: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
        _: usize,
    ) -> Result<CommandOutcome> {
        assert!(matches!(c.data, [1 | 2, 0, 0]));
        let data = buffers.data1.as_deref_mut().unwrap();
        let value = u32::from_le_bytes(data[..4].try_into().unwrap());
        self.records
            .borrow_mut()
            .push([self.id, c.sender, c.code, value]);
        data[..4].copy_from_slice(&(value ^ 0x00100000).to_le_bytes());
        *output += self.id;
        if c.data[0] == 2 && self.id == 4 {
            let mut vm =
                Interpreter::new(Program::from_code(&[0x3b], b"new\0".to_vec(), 16)?, vec![])?;
            assert_eq!(
                host.call(66, &bytes(&[kind::BASE, 1, 0]), &mut vm)?,
                katze_system2::kcs::NativeReturn::Value(5)
            );
        }
        Ok(CommandOutcome::finished(false))
    }
    fn enqueue_message(&mut self, _: [u32; 4]) -> Result<()> {
        unreachable!()
    }
    fn dispatch_message(
        &mut self,
        _: &mut [u32; 4],
        _: &mut u32,
        _: &mut dyn Host,
        _: usize,
    ) -> Result<bool> {
        unreachable!()
    }
    fn request_delete(&mut self, _: &mut dyn Host, _: usize) -> Result<bool> {
        Ok(true)
    }
}
struct RecordFactory(Records);
impl ObjectFactory for RecordFactory {
    fn create(&mut self, id: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Ok(Box::new(Recorder {
            id,
            base: BaseObject::new(),
            records: self.0.clone(),
        }))
    }
}

#[test]
fn property_commands_forward_with_shared_buffers_and_return_slots_through_nested_planes() {
    let records = Records::default();
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory {
            fallback: RecordFactory(records.clone()),
        },
    );
    let parent = runtime.create(0, kind::PLANE, b"parent", b"").unwrap();
    let nested = runtime.create(parent, kind::PLANE, b"nested", b"").unwrap();
    let sibling = runtime.create(parent, kind::BASE, b"sibling", b"").unwrap();
    let leaf = runtime.create(nested, kind::BASE, b"leaf", b"").unwrap();
    for (code, forwarded) in [
        (49, 50),
        (51, 52),
        (55, 56),
        (57, 58),
        (66, 67),
        (50, 50),
        (52, 52),
        (56, 56),
        (58, 58),
        (67, 67),
    ] {
        let mut data = bytes(&[1f32.to_bits(); 16]);
        let output = runtime
            .dispatch_command_with_buffers(
                0,
                Command {
                    data: [1, 0, 0],
                    ..command(parent, code)
                },
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut data),
                },
            )
            .unwrap();
        assert_eq!(output, leaf + sibling);
        assert_eq!(
            &*records.borrow(),
            &[
                [leaf, nested, forwarded, 1f32.to_bits()],
                [sibling, parent, forwarded, 1f32.to_bits() ^ 0x00100000]
            ]
        );
        assert_eq!(&data[..4], &1f32.to_le_bytes());
        records.borrow_mut().clear();
    }
    let mut data = bytes(&[1f32.to_bits(); 16]);
    let output = runtime
        .dispatch_command_with_buffers(
            0,
            Command {
                data: [2, 0, 0],
                ..command(parent, 52)
            },
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut data),
            },
        )
        .unwrap();
    assert_eq!(output, leaf + sibling + 5);
    assert_eq!(
        &*records.borrow(),
        &[
            [leaf, nested, 52, 1f32.to_bits()],
            [sibling, parent, 52, 1f32.to_bits() ^ 0x00100000],
            [5, parent, 52, 1f32.to_bits()]
        ]
    );
}

struct Programs;
impl ProgramLoader for Programs {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        fn ins(code: &mut Vec<u8>, opcode: u8, values: &[u32]) {
            code.push(opcode);
            for value in values {
                code.push(3);
                code.extend(value.to_le_bytes());
            }
        }
        fn call(code: &mut Vec<u8>, native: u32, values: &[u32]) {
            for value in values {
                ins(code, 0x25, &[*value]);
            }
            ins(code, 0x2b, &[native, values.len() as u32 * 4]);
        }
        let mut code = vec![];
        ins(&mut code, 0x25, &[0xdeadbeef]);
        for native in [79, 80, 83, 84, 100, 101] {
            call(&mut code, native, &[0]);
        }
        call(
            &mut code,
            79,
            &[999, 2f32.to_bits(), 3f32.to_bits(), 0xff123456],
        );
        call(
            &mut code,
            79,
            &[1, (-2f32).to_bits(), 3f32.to_bits(), 0x80445566],
        );
        call(
            &mut code,
            84,
            &[1, 4f32.to_bits(), (-5f32).to_bits(), 6f32.to_bits(), 1],
        );
        call(
            &mut code,
            101,
            &[1, 2f32.to_bits(), 3f32.to_bits(), 4f32.to_bits(), 1],
        );
        call(&mut code, 100, &[1, 0x70123456, u32::MAX]);
        if name == b"empty" {
            call(&mut code, 80, &[1]);
        }
        if name == b"replaced" {
            call(
                &mut code,
                79,
                &[1, 7f32.to_bits(), 8f32.to_bits(), 0xffaabbcc],
            );
        }
        if name == b"cleared" {
            call(&mut code, 83, &[1]);
        }
        ins(&mut code, 0x26, &[0]);
        ins(&mut code, 0x3b, &[]);
        Ok(Some(Program::from_code(&code, vec![0; 16], 128)?))
    }
}

#[test]
fn ordinary_graphics_natives_preserve_stack_and_replace_geometry_under_both_budgets() {
    for budget in [1, 4096] {
        for name in [b"rectangle".as_slice(), b"empty", b"replaced", b"cleared"] {
            let mut runtime = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                GraphicsFactory {
                    fallback: KcsFactory::new(Programs),
                },
            );
            runtime.set_budget(budget, 1 << 20);
            assert_eq!(runtime.create(0, kind::PLANE, b"plane", b"").unwrap(), 1);
            let caller = runtime.create(0, kind::ORDINARY, name, b"").unwrap();
            runtime.update(1, 1).unwrap();
            let vm = runtime.object(caller).unwrap().interpreter().unwrap();
            assert_eq!(vm.read(0, 4).unwrap(), 0xdeadbeefu32.to_le_bytes());
            assert!(vm.stack_bytes().is_empty());
            let drawable = runtime.drawables().get(1);
            if name == b"cleared" {
                assert!(drawable.is_none());
                continue;
            }
            let drawable = drawable.unwrap();
            if name == b"rectangle" {
                assert_eq!(drawable.triangles().len(), 2);
                assert_eq!(drawable.triangles()[0][1].position, [-2., 0., 0.]);
                assert_eq!(drawable.triangles()[0][0].color, 0x80445566);
                assert_eq!(drawable.transform().position(), [4., -5., 6.]);
                assert_eq!(drawable.transform().scale(), [2., 3., 4.]);
                assert_eq!(drawable.color(), (0x70123456, u32::MAX));
            } else {
                assert_eq!(drawable.transform().position(), [0.; 3]);
                assert_eq!(drawable.transform().scale(), [1.; 3]);
                assert_eq!(drawable.color(), (u32::MAX, 0));
                if name == b"empty" {
                    assert!(drawable.triangles().is_empty());
                } else {
                    assert_eq!(drawable.triangles()[0][2].position, [7., 8., 0.]);
                }
            }
        }
    }
}
