use std::cell::{Cell, RefCell};
use std::rc::Rc;

use katze_system2::{
    Error, Result, expression,
    graphics::{GraphicsFactory, RenderTarget},
    input::{InputFactory, KeyEvent, KeyEventKind},
    kcs::{Host, Interpreter, NativeReturn, Program},
    runtime::{BaseObject, ObjectFactory, RootObject, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock},
};

type Records = Rc<RefCell<Vec<Command>>>;

struct NoHostCalls;

impl Host for NoHostCalls {
    fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
        unreachable!("基础平面主体不调用宿主")
    }
}

struct Factory {
    records: Records,
    response: Rc<Cell<u32>>,
    fail: Rc<Cell<bool>>,
}

struct Receiver {
    base: BaseObject,
    records: Records,
    response: Rc<Cell<u32>>,
    fail: Rc<Cell<bool>>,
}

impl ObjectFactory for Factory {
    fn create(&mut self, _: u32, object_kind: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        if object_kind != kind::BASE {
            return Err(Error::new(0, "合成工厂未提供该类型"));
        }
        Ok(Box::new(Receiver {
            base: BaseObject::new(),
            records: self.records.clone(),
            response: self.response.clone(),
            fail: self.fail.clone(),
        }))
    }
}

impl TaskObject for Receiver {
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
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.base.set_creation_context(context)
    }
    fn update_body(&mut self, host: &mut dyn Host, increment: u32, budget: usize) -> Result<bool> {
        self.base.update_body(host, increment, budget)
    }
    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        self.dispatch_command_with_buffers(
            kind,
            command,
            &mut CommandBuffers::default(),
            output,
            host,
            budget,
        )
    }
    fn dispatch_command_with_buffers(
        &mut self,
        kind: u32,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        if command.code == 800 {
            let mut vm = Interpreter::new(Program::from_code(&[], vec![], 16)?, vec![])?;
            let NativeReturn::Value(handle) = host.call(112, &bytes(&[3, 2]), &mut vm)? else {
                unreachable!();
            };
            for (x, y, color) in [(0, 1, 0x010000ff), (2, 0, 0x8000ff00)] {
                assert_eq!(
                    host.call(117, &bytes(&[handle, x, y, 1, 1, color]), &mut vm)?,
                    NativeReturn::Continue
                );
            }
            host.dispatch_task_command(
                packet(3, command.target, 98, [0, handle, 1]),
                &mut CommandBuffers::default(),
                &mut 0,
            )?;
            assert_eq!(
                host.call(114, &bytes(&[handle]), &mut vm)?,
                NativeReturn::Continue
            );
            *output = handle;
            return Ok(CommandOutcome::finished(false));
        }
        if matches!(command.code, 6 | 9 | 10 | 48 | 64 | 65) {
            self.records.borrow_mut().push(*command);
            if matches!(command.code, 6 | 9 | 10 | 65) {
                let mut reply = 0x13579bdf;
                host.dispatch_task_command(
                    Command {
                        target: command.sender,
                        sender: command.target,
                        code: 95,
                        data: [0, self.response.get(), 1],
                    },
                    &mut CommandBuffers::default(),
                    &mut reply,
                )?;
                assert_eq!(reply, 0x13579bdf);
                if command.code == 6 {
                    buffers.data0.as_deref_mut().unwrap()[0] = 0x12;
                    buffers.data1.as_deref_mut().unwrap()[0] = 0x34;
                }
                if self.fail.get() {
                    return Err(Error::new(0, "合成事件接收失败"));
                }
                *output = 0xabcdef01;
            }
            return Ok(CommandOutcome::finished(false));
        }
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

type TestRuntime = Runtime<GraphicsFactory<InputFactory<Factory>>>;

fn packet(target: u32, sender: u32, code: u32, data: [u32; 3]) -> Command {
    Command {
        target,
        sender,
        code,
        data,
    }
}

fn send(runtime: &mut TestRuntime, target: u32, code: u32, value: u32) -> u32 {
    runtime
        .dispatch_command(0, packet(target, 2, code, [0, value, 0]))
        .unwrap()
}

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn states() -> Vec<u8> {
    bytes(&[1, 31, 41, 1, 51, 61, 1, 71, 81, 1, 91, 101])
}

fn setup() -> (TestRuntime, Records, Rc<Cell<u32>>, Rc<Cell<bool>>) {
    let records = Records::default();
    let response = Rc::new(Cell::new(0));
    let fail = Rc::new(Cell::new(false));
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(RootObject::new(1, 77)),
        expression::State::new(0),
        GraphicsFactory {
            fallback: InputFactory {
                fallback: Factory {
                    records: records.clone(),
                    response: response.clone(),
                    fail: fail.clone(),
                },
            },
        },
    );
    assert_eq!(runtime.create(0, kind::INPUT, b"input", b"").unwrap(), 1);
    assert_eq!(runtime.create(0, kind::BASE, b"receiver", b"").unwrap(), 2);
    assert_eq!(runtime.create(0, kind::PLANE, b"plane", b"").unwrap(), 3);
    runtime.input_coordinates_mut().logical_size = [800, 600];
    let mut rectangle = bytes(&[0, 0, 0, 100f32.to_bits(), 50f32.to_bits(), 0, 0, u32::MAX]);
    runtime
        .dispatch_command_with_buffers(
            0,
            packet(3, 2, 42, [0; 3]),
            &mut CommandBuffers {
                data1: Some(&mut rectangle),
                ..CommandBuffers::default()
            },
        )
        .unwrap();
    for (code, vector) in [(49, [10f32, 20., 4000.5]), (51, [30.25, 50.75, 9.25])] {
        let mut vector = bytes(&vector.map(f32::to_bits));
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(3, 2, code, [0; 3]),
                &mut CommandBuffers {
                    data1: Some(&mut vector),
                    ..CommandBuffers::default()
                },
            )
            .unwrap();
    }
    (runtime, records, response, fail)
}

fn configure(runtime: &mut TestRuntime, slots: &mut [u8]) {
    runtime
        .dispatch_command_with_buffers(
            0,
            packet(3, 2, 72, [0, 73, 1]),
            &mut CommandBuffers {
                data0: Some(slots),
                ..CommandBuffers::default()
            },
        )
        .unwrap();
}

fn event(pressed: u32) -> KeyEvent {
    KeyEvent {
        kind: KeyEventKind::Change,
        code: 4,
        key: 0,
        pressed,
        mask: 2,
        point: [25, 35],
    }
}

#[test]
fn plane_region_selection_and_pointer_callbacks_share_the_runtime() {
    let (mut runtime, records, response, _) = setup();
    assert_eq!(send(&mut runtime, 0, 34, 0), 1);
    assert_eq!(send(&mut runtime, 0, 35, 0), 77);
    configure(&mut runtime, &mut states());
    let region = runtime.input_state(1).unwrap().regions().get(3).unwrap();
    assert_eq!(region.priority(), 4000);
    assert_eq!(region.event_mask(), 7);
    assert_eq!(
        [
            region.bounds().left,
            region.bounds().top,
            region.bounds().right,
            region.bounds().bottom
        ],
        [20, 30, 120, 80]
    );
    assert_eq!(
        runtime.drawables().get(3).unwrap().transform().position(),
        [31., 41., 9.25]
    );
    runtime.update_input_selection(1, Some([25, 35])).unwrap();
    assert_eq!(send(&mut runtime, 3, 74, 0), 1);
    assert_eq!(
        records.borrow().last().unwrap(),
        &packet(2, 3, 64, [3, 1, 0])
    );
    assert!(runtime.dispatch_input_key(1, event(1)).unwrap());
    assert_eq!(send(&mut runtime, 3, 74, 0), 2);
    assert_eq!(runtime.input_state(1).unwrap().events().captures()[0], 3);
    assert_eq!(
        records.borrow().last().unwrap(),
        &packet(2, 3, 9, [3, 0, 0])
    );
    assert!(runtime.dispatch_input_key(1, event(0)).unwrap());
    assert_eq!(send(&mut runtime, 3, 74, 0), 1);
    assert_eq!(
        records.borrow().last().unwrap(),
        &packet(2, 3, 10, [3, 0, 0])
    );
    response.set(7);
    assert!(!runtime.dispatch_input_key(1, event(1)).unwrap());
    assert_eq!(runtime.input_state(1).unwrap().events().captures()[0], 0);
    send(&mut runtime, 3, 73, 0x80000003);
    let count = records.borrow().len();
    runtime.dispatch_input_key(1, event(1)).unwrap();
    assert_eq!(records.borrow().len(), count);
    assert_eq!(send(&mut runtime, 3, 74, 0), 0x80000003);
    send(&mut runtime, 3, 73, 0);
    let child = runtime.create(3, kind::BASE, b"child", b"").unwrap();
    send(&mut runtime, 3, 47, 0);
    assert_eq!(
        runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(3)
            .unwrap()
            .enabled(),
        1
    );
    assert_eq!(runtime.drawables().get(3).unwrap().display(), 0);
    assert_eq!(
        records.borrow().last().unwrap(),
        &packet(child, 3, 48, [0, 0, 0])
    );
    send(&mut runtime, 3, 48, 5);
    let region = runtime.input_state(1).unwrap().regions().get(3).unwrap();
    assert_eq!((region.enabled(), region.selected()), (0, 0));
    assert_eq!(send(&mut runtime, 3, 74, 0), 0);
    send(&mut runtime, 3, 47, 1);
    assert_eq!(
        runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(3)
            .unwrap()
            .enabled(),
        0
    );
    send(&mut runtime, 3, 48, 5);
    assert_eq!(
        runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(3)
            .unwrap()
            .enabled(),
        5
    );
    assert_eq!(runtime.drawables().get(3).unwrap().display(), 5);
    assert_eq!(
        records.borrow().last().unwrap(),
        &packet(child, 3, 48, [0, 5, 0])
    );
}

#[test]
fn plane_binding_reconfiguration_and_deletion_keep_the_verified_boundaries() {
    let (mut runtime, records, _, _) = setup();
    configure(&mut runtime, &mut states());
    let original = runtime
        .input_state(1)
        .unwrap()
        .regions()
        .get(3)
        .unwrap()
        .clone();
    let mut replacement = bytes(&[1, 11, 21, 1, 12, 22, 0, 0, 0, 1, 13, 23]);
    runtime
        .dispatch_command_with_buffers(
            0,
            packet(3, 0, 72, [0, 99, 0]),
            &mut CommandBuffers {
                data0: Some(&mut replacement),
                ..CommandBuffers::default()
            },
        )
        .unwrap();
    assert_eq!(runtime.input_state(1).unwrap().regions().records().len(), 1);
    assert_eq!(
        runtime.input_state(1).unwrap().regions().get(3).unwrap(),
        &original
    );
    assert_eq!(
        runtime.drawables().get(3).unwrap().transform().position(),
        [11., 21., 9.25]
    );
    send(&mut runtime, 3, 7, 0);
    assert_eq!(records.borrow().last().unwrap().target, 2);
    send(&mut runtime, 3, 73, 0x80000003);
    send(&mut runtime, 3, 91, 0);
    assert!(
        runtime
            .input_state(1)
            .unwrap()
            .regions()
            .records()
            .is_empty()
    );
    assert_eq!(send(&mut runtime, 3, 74, 0), 0x80000003);
    send(&mut runtime, 3, 73, 0);
    assert_eq!(send(&mut runtime, 3, 74, 0), 3);
    runtime.input_coordinates_mut().logical_size = [100, 50];
    let mut base = bytes(&[13f32.to_bits(), 23f32.to_bits(), 7f32.to_bits()]);
    runtime
        .dispatch_command_with_buffers(
            0,
            packet(3, 2, 49, [0; 3]),
            &mut CommandBuffers {
                data1: Some(&mut base),
                ..CommandBuffers::default()
            },
        )
        .unwrap();
    assert!(
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(3, 2, 72, [0; 3]),
                &mut CommandBuffers::default()
            )
            .is_err()
    );
    assert_eq!(runtime.input_state(1).unwrap().regions().records().len(), 1);
    let bounds = runtime
        .input_state(1)
        .unwrap()
        .regions()
        .get(3)
        .unwrap()
        .bounds();
    assert_eq!(
        [bounds.left, bounds.top, bounds.right, bounds.bottom],
        [-100_000, -100_000, 100_000, 100_000]
    );
    send(&mut runtime, 3, 52, 0);
    let bounds = runtime
        .input_state(1)
        .unwrap()
        .regions()
        .get(3)
        .unwrap()
        .bounds();
    assert_eq!(
        [bounds.left, bounds.top, bounds.right, bounds.bottom],
        [0, 0, 100, 50]
    );
    configure(&mut runtime, &mut states());
    assert_eq!(runtime.input_state(1).unwrap().regions().records().len(), 1);
    runtime.destroy(3).unwrap();
    assert!(
        runtime
            .input_state(1)
            .unwrap()
            .regions()
            .records()
            .is_empty()
    );
    assert!(runtime.drawables().get(3).is_none());
    runtime.destroy(1).unwrap();
    assert_eq!(send(&mut runtime, 0, 34, 0), 1);
    runtime
        .dispatch_command(0, packet(0, 0, 3, [0; 3]))
        .unwrap();
    assert_eq!(send(&mut runtime, 0, 34, 0), 0);
    assert_eq!(send(&mut runtime, 0, 35, 0), 0);
}

#[test]
fn graphics_event_response_survives_errors_and_forwards_motion_buffers() {
    let (mut runtime, records, response, fail) = setup();
    configure(&mut runtime, &mut states());
    response.set(7);
    fail.set(true);
    let error = runtime
        .dispatch_command(0, packet(3, 0, 4, [9, 1, 0]))
        .unwrap_err();
    assert!(error.message.contains("合成事件接收失败"));
    assert_eq!(
        records.borrow().last().unwrap(),
        &packet(2, 3, 9, [3, 9, 0])
    );
    fail.set(false);
    send(&mut runtime, 3, 97, u32::MAX);
    assert_eq!(
        runtime
            .dispatch_command(0, packet(3, 0, 5, [0; 3]))
            .unwrap(),
        7
    );
    send(&mut runtime, 3, 97, 2);
    response.set(5);
    assert_eq!(
        runtime
            .dispatch_command(0, packet(3, 0, 5, [19, 23, 1]))
            .unwrap(),
        5
    );
    assert_eq!(
        records.borrow().last().unwrap(),
        &packet(2, 3, 65, [3, 19, 0])
    );
    let mut previous = [0; 8];
    let mut current = [0; 8];
    assert_eq!(
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(3, 0, 6, [0x1234, 0x5678, 1]),
                &mut CommandBuffers {
                    data0: Some(&mut previous),
                    data1: Some(&mut current),
                }
            )
            .unwrap(),
        5
    );
    assert_eq!((previous[0], current[0]), (0x12, 0x34));
    assert_eq!(
        records.borrow().last().unwrap(),
        &packet(2, 3, 6, [0x1234, 0x5678, 0])
    );
    send(&mut runtime, 3, 97, u32::MAX);
    assert_eq!(
        runtime
            .dispatch_command(0, packet(3, 0, 5, [0; 3]))
            .unwrap(),
        0
    );
    send(&mut runtime, 3, 95, 13);
    assert_eq!(
        runtime
            .dispatch_command(0, packet(3, 0, 5, [0; 3]))
            .unwrap(),
        13
    );
    assert_eq!(
        runtime
            .dispatch_command(0, packet(3, 0, 5, [0; 3]))
            .unwrap(),
        0
    );
    send(&mut runtime, 3, 95, 13);
    runtime
        .object_mut(3)
        .unwrap()
        .update_body(&mut NoHostCalls, 1, 1)
        .unwrap();
    assert_eq!(
        runtime
            .dispatch_command(0, packet(3, 0, 5, [0; 3]))
            .unwrap(),
        0
    );
}

#[test]
fn geometry_changes_refresh_bounds_without_reprioritizing_or_scaling_the_region() {
    let (mut runtime, _, _, _) = setup();
    let bounds = |runtime: &TestRuntime| {
        let rect = runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(3)
            .unwrap()
            .bounds();
        [rect.left, rect.top, rect.right, rect.bottom]
    };
    let vector = |runtime: &mut TestRuntime, code, value: [f32; 3]| {
        let mut data = bytes(&value.map(f32::to_bits));
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(3, 2, code, [0; 3]),
                &mut CommandBuffers {
                    data1: Some(&mut data),
                    ..CommandBuffers::default()
                },
            )
            .unwrap();
    };
    assert!(!runtime.input_state(1).unwrap().regions().geometry_dirty());
    configure(&mut runtime, &mut states());
    vector(&mut runtime, 51, [40.75, -10.25, 11.]);
    assert_eq!(bounds(&runtime), [30, -30, 130, 19]);
    vector(&mut runtime, 49, [5.5, 6.25, 1000.]);
    assert_eq!(bounds(&runtime), [35, -16, 135, 33]);
    assert_eq!(
        runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(3)
            .unwrap()
            .priority(),
        4000
    );
    vector(&mut runtime, 55, [1., 2., 3.]);
    vector(&mut runtime, 66, [9., 8., 7.]);
    let mut matrix = bytes(&[1f32.to_bits(); 16]);
    runtime
        .dispatch_command_with_buffers(
            0,
            packet(3, 2, 57, [0; 3]),
            &mut CommandBuffers {
                data1: Some(&mut matrix),
                ..CommandBuffers::default()
            },
        )
        .unwrap();
    assert_eq!(bounds(&runtime), [35, -16, 135, 33]);
    for code in [50, 52, 56, 58, 67] {
        let mut changed = bytes(&[999; 4]);
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(1, 2, 130, [3, 0, 0]),
                &mut CommandBuffers {
                    data1: Some(&mut changed),
                    ..CommandBuffers::default()
                },
            )
            .unwrap();
        runtime
            .input_state_mut(1)
            .unwrap()
            .regions_mut()
            .clear_geometry_dirty();
        if code == 52 {
            runtime
                .dispatch_command(0, packet(3, 2, 52, [1, 0, 1]))
                .unwrap();
            assert_eq!(bounds(&runtime), [999; 4]);
            assert!(!runtime.input_state(1).unwrap().regions().geometry_dirty());
        }
        runtime
            .dispatch_command(0, packet(3, 2, code, [0, 0, 1]))
            .unwrap();
        assert_eq!(bounds(&runtime), [35, -16, 135, 33]);
        assert!(runtime.input_state(1).unwrap().regions().geometry_dirty());
    }
    runtime.drawing_mut().target = Some(RenderTarget::new([128, 64]).unwrap());
    let mut rectangle = bytes(&[0, 0, 0, 100f32.to_bits(), 50f32.to_bits(), 0, 0, 0]);
    runtime
        .dispatch_command_with_buffers(
            0,
            packet(3, 2, 41, [0; 3]),
            &mut CommandBuffers {
                data1: Some(&mut rectangle),
                ..CommandBuffers::default()
            },
        )
        .unwrap();
    vector(&mut runtime, 49, [5.5, 6.25, 1000.]);
    vector(&mut runtime, 51, [40.75, -10.25, 11.]);
    let mut source = bytes(&[999f32.to_bits(), 888f32.to_bits(), 777f32.to_bits()]);
    runtime
        .dispatch_command_with_buffers(
            0,
            packet(3, 2, 51, [1, 0, 0]),
            &mut CommandBuffers {
                data1: Some(&mut source),
                ..CommandBuffers::default()
            },
        )
        .unwrap();
    assert_eq!(bounds(&runtime), [35, -16, 135, 33]);
    assert_eq!(
        runtime
            .drawables()
            .get(3)
            .unwrap()
            .geometry_transform(true)
            .unwrap()
            .position(),
        [999., 888., 777.]
    );
    vector(&mut runtime, 51, [f32::NAN, f32::INFINITY, 11.]);
    assert_eq!(bounds(&runtime), [i32::MIN; 4]);
    send(&mut runtime, 3, 91, 0);
    runtime
        .input_state_mut(1)
        .unwrap()
        .regions_mut()
        .clear_geometry_dirty();
    vector(&mut runtime, 51, [0.; 3]);
    assert!(
        runtime
            .input_state(1)
            .unwrap()
            .regions()
            .records()
            .is_empty()
    );
    assert!(runtime.input_state(1).unwrap().regions().geometry_dirty());
}

#[test]
fn associated_hint_uses_input_bounds_and_context_width_and_keeps_display_gates() {
    let (mut runtime, _, _, _) = setup();
    runtime.host_mut().surface_size = [800, 600];
    runtime.input_coordinates_mut().logical_size = [320, 240];
    configure(&mut runtime, &mut states());
    let hint = runtime.create(0, kind::PLANE, b"hint", b"").unwrap();
    let rectangle = |runtime: &mut TestRuntime, width: f32| {
        let mut data = bytes(&[0, 0, 0, width.to_bits(), 20.5f32.to_bits(), 0, 0, u32::MAX]);
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(hint, 2, 42, [0; 3]),
                &mut CommandBuffers {
                    data1: Some(&mut data),
                    ..CommandBuffers::default()
                },
            )
            .unwrap();
    };
    rectangle(&mut runtime, 40.5);
    send(&mut runtime, 3, 77, hint);
    assert_eq!(runtime.drawables().get(hint).unwrap().display(), 0);
    send(&mut runtime, 3, 73, 1);
    let drawing = runtime.drawables().get(hint).unwrap();
    assert_eq!(drawing.transform().position(), [760., 10., 0.]);
    assert_eq!(drawing.display(), 1);
    send(&mut runtime, 3, 78, 0);
    send(&mut runtime, 3, 73, 1);
    assert_eq!(runtime.drawables().get(hint).unwrap().display(), 1);
    send(&mut runtime, 3, 73, 0);
    send(&mut runtime, 3, 73, 1);
    assert_eq!(runtime.drawables().get(hint).unwrap().display(), 0);
    send(&mut runtime, 3, 78, 2);
    send(&mut runtime, 3, 73, 1);
    assert_eq!(runtime.drawables().get(hint).unwrap().display(), 1);
    rectangle(&mut runtime, f32::NAN);
    send(&mut runtime, 3, 73, 1);
    assert_eq!(
        runtime
            .drawables()
            .get(hint)
            .unwrap()
            .transform()
            .position(),
        [800., 10., 0.]
    );
    // 隐藏自身仍先处理当前选择，最后将关联提示隐藏。
    runtime
        .dispatch_command(0, packet(1, 0, 133, [3, 1, 0]))
        .unwrap();
    send(&mut runtime, 3, 47, 0);
    assert_eq!(send(&mut runtime, 3, 74, 0), 1);
    assert_eq!(runtime.drawables().get(hint).unwrap().display(), 0);
    send(&mut runtime, 3, 73, 1);
    let next = runtime.create(0, kind::PLANE, b"next", b"").unwrap();
    runtime
        .drawables_mut()
        .insert(next, katze_system2::graphics::Drawable::default())
        .unwrap();
    runtime.drawables_mut().set_display(next, 7);
    send(&mut runtime, 3, 77, next);
    assert_eq!(runtime.drawables().get(hint).unwrap().display(), 1);
    assert_eq!(runtime.drawables().get(next).unwrap().display(), 0);
}

#[test]
fn bound_self_display_uses_parent_visibility_and_preserves_state_when_parent_drawing_is_missing() {
    let (mut runtime, _, _, _) = setup();
    let child = runtime.create(3, kind::PLANE, b"child", b"").unwrap();
    runtime
        .drawables_mut()
        .insert(child, katze_system2::graphics::Drawable::default())
        .unwrap();
    send(&mut runtime, child, 90, 0);
    let mut offset = [0; 12];
    runtime
        .dispatch_command_with_buffers(
            0,
            packet(child, 2, 53, [0; 3]),
            &mut CommandBuffers {
                data1: Some(&mut offset),
                ..CommandBuffers::default()
            },
        )
        .unwrap();
    let display = |runtime: &TestRuntime| {
        (
            runtime.drawables().get(child).unwrap().display(),
            runtime
                .input_state(1)
                .unwrap()
                .regions()
                .get(child)
                .unwrap()
                .enabled(),
        )
    };
    runtime.drawables_mut().set_display(3, 0);
    send(&mut runtime, child, 47, 1);
    assert_eq!(display(&runtime), (0, 0));
    runtime.drawables_mut().set_display(3, 7);
    send(&mut runtime, child, 47, 2);
    assert_eq!(display(&runtime), (7, 7));
    runtime.drawables_mut().remove(3);
    send(&mut runtime, child, 47, 0);
    assert_eq!(display(&runtime), (7, 7));
    send(&mut runtime, child, 48, 9);
    assert_eq!(display(&runtime), (0, 0));
}

#[test]
fn inherited_geometry_refreshes_regions_after_parent_changes_but_not_on_binding() {
    let (mut runtime, _, _, _) = setup();
    let child = runtime.create(3, kind::PLANE, b"child", b"").unwrap();
    let mut rectangle = bytes(&[0, 0, 0, 20f32.to_bits(), 10f32.to_bits(), 0, 0, u32::MAX]);
    runtime
        .dispatch_command_with_buffers(
            0,
            packet(child, 2, 42, [0; 3]),
            &mut CommandBuffers {
                data1: Some(&mut rectangle),
                ..CommandBuffers::default()
            },
        )
        .unwrap();
    send(&mut runtime, child, 90, 0);
    let bounds = |runtime: &TestRuntime| {
        let region = runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(child)
            .unwrap();
        let r = region.bounds();
        ([r.left, r.top, r.right, r.bottom], region.priority())
    };
    assert_eq!(bounds(&runtime), ([0, 0, 20, 10], 5000));
    let vector = |runtime: &mut TestRuntime, target, code, source, value: [f32; 3]| {
        let mut data = bytes(&value.map(f32::to_bits));
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(target, 2, code, [source, 0, 0]),
                &mut CommandBuffers {
                    data1: Some(&mut data),
                    ..CommandBuffers::default()
                },
            )
            .unwrap();
    };
    vector(&mut runtime, child, 53, 0, [2., 3., 4.]);
    assert_eq!(bounds(&runtime), ([0, 0, 20, 10], 5000));
    assert_eq!(
        runtime
            .drawables()
            .get(child)
            .unwrap()
            .transform()
            .base_point(),
        [8., 17., 4004.5]
    );
    vector(&mut runtime, 3, 51, 0, [50.75, 70.25, 11.]);
    assert_eq!(bounds(&runtime), ([42, 53, 62, 63], 5000));
    vector(&mut runtime, 3, 51, 1, [100., 200., 12.]);
    assert_eq!(bounds(&runtime), ([42, 53, 62, 63], 5000));
    assert_eq!(
        runtime
            .drawables()
            .get(child)
            .unwrap()
            .transform()
            .position(),
        [50.75, 70.25, 11.]
    );
    vector(&mut runtime, child, 51, 0, [999.; 3]);
    assert_eq!(bounds(&runtime), ([92, 183, 112, 193], 5000));
    send(&mut runtime, child, 54, 0);
    vector(&mut runtime, child, 51, 0, [5., 6., 7.]);
    assert_eq!(bounds(&runtime), ([-3, -11, 17, -1], 5000));
}

#[test]
fn allocated_image_masks_survive_release_and_gate_plane_selection_and_capture() {
    let (mut runtime, records, _, _) = setup();
    assert_eq!(send(&mut runtime, 3, 98, u32::MAX), 0);
    configure(&mut runtime, &mut states());
    let handle = send(&mut runtime, 2, 800, 0);
    assert_ne!(handle, 0);
    assert!(runtime.images().get(handle).is_err());
    let mask = runtime
        .input_state(1)
        .unwrap()
        .regions()
        .get(3)
        .unwrap()
        .hit_mask()
        .unwrap();
    assert_eq!(mask.size(), [3, 2]);
    assert_eq!(mask.bytes(), [1, 4]);
    runtime.update_input_selection(1, Some([20, 30])).unwrap();
    assert_eq!(send(&mut runtime, 3, 74, 0), 1);
    let pressed = KeyEvent {
        point: [20, 30],
        ..event(1)
    };
    assert!(runtime.dispatch_input_key(1, pressed).unwrap());
    assert_eq!(runtime.input_state(1).unwrap().events().captures()[0], 3);
    let released = KeyEvent {
        point: [21, 30],
        ..event(0)
    };
    assert!(runtime.dispatch_input_key(1, released).unwrap());
    runtime.update_input_selection(1, Some([21, 30])).unwrap();
    assert_eq!(send(&mut runtime, 3, 74, 0), 0);
    let count = records.borrow().len();
    assert!(
        !runtime
            .dispatch_input_key(
                1,
                KeyEvent {
                    point: [21, 30],
                    ..event(1)
                }
            )
            .unwrap()
    );
    assert_eq!(records.borrow().len(), count);
    assert!(
        runtime
            .dispatch_command(0, packet(3, 2, 98, [0, handle, 0]))
            .is_err()
    );
    assert!(
        runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(3)
            .unwrap()
            .hit_mask()
            .is_none()
    );
    send(&mut runtime, 3, 98, 0);
    runtime.update_input_selection(1, Some([21, 30])).unwrap();
    assert_eq!(send(&mut runtime, 3, 74, 0), 1);
}
