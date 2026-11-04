use katze_system2::{
    Error, Result, expression,
    graphics::{Drawable, Drawables, Rect},
    input::{
        ButtonObject, CoordinateMap, InputDevice, InputFactory, KeyEvent, KeyEventKind, KeySample,
        PointerSample,
    },
    kcs::{Host, Interpreter, NativeReturn, Program},
    runtime::{BaseObject, ObjectFactory, RootObject, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock},
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}
fn packet(target: u32, sender: u32, code: u32, data: [u32; 3]) -> Command {
    Command {
        target,
        sender,
        code,
        data,
    }
}
fn rect(values: [i32; 4]) -> Rect {
    let [left, top, right, bottom] = values;
    Rect {
        left,
        top,
        right,
        bottom,
    }
}

#[derive(Debug, PartialEq)]
struct Record {
    command: Command,
    words: Vec<u32>,
}

struct Fixture {
    records: Vec<Record>,
    drawings: Drawables,
    reply: Option<u32>,
    fail: bool,
    raw: u32,
    position: [f32; 3],
    base: [f32; 3],
}
impl Fixture {
    fn new() -> Self {
        Self {
            records: vec![],
            drawings: Drawables::default(),
            reply: None,
            fail: false,
            raw: 2,
            position: [10.5, -20.75, 3.],
            base: [2.25, -4.5, 5000.],
        }
    }
    fn send(
        &mut self,
        button: &mut ButtonObject,
        code: u32,
        data: [u32; 3],
        words: &[u32],
    ) -> Result<u32> {
        let mut buffer = bytes(words);
        let mut output = 0x13579bdf;
        let outcome = button.dispatch_command_with_buffers(
            kind::BUTTON,
            &packet(7, 99, code, data),
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut buffer),
                external: None,
            },
            &mut output,
            self,
            1,
        )?;
        assert_eq!(outcome, CommandOutcome::finished(false));
        Ok(output)
    }
    fn codes(&self) -> Vec<u32> {
        self.records
            .iter()
            .map(|record| record.command.code)
            .collect()
    }
}
impl Host for Fixture {
    fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
        unreachable!()
    }
    fn input_coordinates(&self) -> CoordinateMap {
        CoordinateMap {
            logical_size: [800, 600],
            ..CoordinateMap::default()
        }
    }
    fn drawables(&mut self) -> Option<&mut Drawables> {
        Some(&mut self.drawings)
    }
    fn parent_task_id(&self) -> Result<u32> {
        Ok(50)
    }
    fn dispatch_task_command(
        &mut self,
        command: Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        let words = match command.code {
            127 => 8,
            130 => 4,
            _ => 0,
        };
        self.records.push(Record {
            command,
            words: buffers
                .data1
                .as_deref()
                .unwrap_or_default()
                .chunks_exact(4)
                .take(words)
                .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
                .collect(),
        });
        match command.code {
            34 => *output = 8,
            61 | 62 => {
                let vector = if command.code == 61 {
                    self.base
                } else {
                    self.position
                };
                buffers.data1.as_deref_mut().unwrap()[..12]
                    .copy_from_slice(&bytes(&vector.map(f32::to_bits)));
            }
            137 => *output = self.raw,
            _ => {}
        }
        Ok(true)
    }
    fn dispatch_graphics_event(
        &mut self,
        response: &mut u32,
        command: Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        self.dispatch_task_command(command, buffers, output)?;
        if matches!(command.code, 6 | 9 | 10 | 65) {
            if let Some(reply) = self.reply {
                *response = reply;
            }
            *output = 0xabcdef01;
        }
        if self.fail {
            return Err(Error::new(0, "合成回调失败"));
        }
        Ok(true)
    }
}

#[test]
fn registration_and_geometry_keep_the_cached_rectangle_separate() {
    let mut host = Fixture::new();
    let mut button = ButtonObject::new(7);
    host.send(&mut button, 93, [0, 1, 3], &[]).unwrap();
    assert!(host.records.is_empty());
    assert_eq!(
        host.send(&mut button, 90, [0, 0, 1], &[3, 0, 12, 600, 7, 88])
            .unwrap(),
        1
    );
    assert_eq!(
        (
            button.event_target(),
            button.event_parameter(),
            button.response()
        ),
        (99, 88, 1)
    );
    assert_eq!(button.rectangle(), rect([3, 0, 12, 600]));
    assert_eq!(host.records[1].words, [7, 7, 7, 3, 0, 12, 600, 0]);
    host.records.clear();
    assert_eq!(host.send(&mut button, 90, [0; 3], &[]).unwrap(), 0);
    assert!(host.records.is_empty());
    host.send(&mut button, 93, [0, 0, 3], &[]).unwrap();
    host.send(&mut button, 94, [0, 9, 2], &[]).unwrap();
    host.send(&mut button, 99, [0, 5, 1], &[]).unwrap();
    host.send(&mut button, 100, [0, 9, 1], &[]).unwrap();
    assert_eq!(host.codes(), [34, 135, 133, 34, 133, 34, 136, 34, 154]);
    assert_eq!(host.records[1].command.data, [7, 0, 3]);
    host.records.clear();
    host.send(&mut button, 101, [0; 3], &[0, 0, 800, 600])
        .unwrap();
    assert!(button.full_screen());
    assert_eq!(button.rectangle(), rect([3, 0, 12, 600]));
    assert_eq!(
        host.records[1].words,
        [-100_000i32 as u32, -100_000i32 as u32, 100_000, 100_000]
    );
    host.send(&mut button, 19, [0; 3], &[]).unwrap();
    assert_eq!(host.records[3].words, host.records[1].words);
    host.records.clear();
    host.send(&mut button, 91, [0, 0, 3], &[]).unwrap();
    assert_eq!(host.records[0].command.data, [0; 3]);
    assert_eq!(host.records[1].command.data, [0, 7, 3]);
    assert_eq!((button.event_target(), button.response()), (u32::MAX, 1));
    host.records.clear();
    host.send(&mut button, 90, [0; 3], &[0, 0, 800, 600, 7, 88])
        .unwrap();
    assert_eq!(
        button.rectangle(),
        rect([-100_000, -100_000, 100_000, 100_000])
    );
    host.records.clear();
    host.send(&mut button, 3, [0, 0, 5], &[]).unwrap();
    assert_eq!(host.records[0].command.data, [0, 0, 5]);
    assert_eq!(host.records[1].command.data, [0, 7, 5]);
    assert_eq!(button.control().deletion_requested, 1);
    button.request_delete(&mut host, 1).unwrap();
    assert_eq!(host.records.len(), 2);
}

#[test]
fn callback_response_is_consumed_only_after_a_successful_consuming_event() {
    let mut host = Fixture::new();
    let mut button = ButtonObject::new(7);
    host.send(&mut button, 97, [0, 99, 0], &[]).unwrap();
    host.send(&mut button, 95, [0, 73, 0], &[]).unwrap();
    for code in [7, 8] {
        assert_eq!(
            host.send(&mut button, code, [0; 3], &[]).unwrap(),
            0x13579bdf
        );
        assert_eq!(button.response(), 73);
    }
    assert_eq!(host.records[0].command, packet(99, 7, 64, [7, 1, 0]));
    assert_eq!(host.records[1].command, packet(99, 7, 64, [7, 0, 0]));
    assert_eq!(host.send(&mut button, 4, [12, 1, 0], &[]).unwrap(), 73);
    assert_eq!(button.response(), 0);
    assert_eq!(host.records[2].command, packet(99, 7, 9, [7, 12, 0]));
    host.reply = Some(19);
    for (code, delivered, data) in [(4, 10, [7, 12, 0]), (5, 65, [7, 12, 0]), (6, 6, [12, 0, 0])] {
        assert_eq!(host.send(&mut button, code, [12, 0, 0], &[]).unwrap(), 19);
        assert_eq!(
            host.records.last().unwrap().command,
            packet(99, 7, delivered, data)
        );
        assert_eq!(button.response(), 0);
    }
    host.fail = true;
    let mut output = 77;
    assert!(
        button
            .dispatch_command(
                kind::BUTTON,
                &packet(7, 99, 4, [0, 1, 0]),
                &mut output,
                &mut host,
                1
            )
            .is_err()
    );
    assert_eq!((output, button.response()), (1, 19));
    host.reply = Some(23);
    output = 77;
    assert!(
        button
            .dispatch_command(
                kind::BUTTON,
                &packet(7, 99, 6, [0; 3]),
                &mut output,
                &mut host,
                1
            )
            .is_err()
    );
    assert_eq!((output, button.response()), (77, 23));
}

#[test]
fn parent_binding_preserves_size_and_uses_the_parent_display_gate() {
    let mut host = Fixture::new();
    host.drawings.insert(50, Drawable::default()).unwrap();
    host.drawings.set_display(50, 73);
    host.drawings.insert(7, Drawable::default()).unwrap();
    let mut button = ButtonObject::new(7);
    host.send(&mut button, 90, [0; 3], &[3, 0, 12, 600, 7, 88])
        .unwrap();
    host.records.clear();
    let offset = [1.25f32, -2.5, 3.];
    host.send(&mut button, 53, [0; 3], &offset.map(f32::to_bits))
        .unwrap();
    assert_eq!(host.codes(), [62, 61, 34, 130]);
    assert_eq!(button.rectangle(), rect([14, -27, 23, 573]));
    let geometry = host.drawings.get(7).unwrap().transform();
    assert_eq!(geometry.base_point(), [-1.25, 2.5, 5003.]);
    assert_eq!(geometry.position(), host.position);
    host.records.clear();
    host.send(&mut button, 47, [0, 0, 2], &[]).unwrap();
    assert_eq!(host.codes(), [34, 135]);
    assert_eq!(host.records[1].command.data, [7, 0, 2]);
    host.records.clear();
    host.drawings.set_display(50, 0);
    host.send(&mut button, 47, [0, 9, 2], &[]).unwrap();
    assert_eq!(host.codes(), [34, 135, 133]);
    host.records.clear();
    host.drawings.remove(50);
    host.send(&mut button, 47, [0, 7, 2], &[]).unwrap();
    assert_eq!(button.display(), 7);
    assert_eq!(host.codes(), [34]);
    host.records.clear();
    host.send(&mut button, 54, [0; 3], &[]).unwrap();
    assert_eq!(button.parent_offset(), None);
    assert_eq!(button.binding_offset(), offset);
    host.send(&mut button, 50, [0; 3], &[]).unwrap();
    assert!(host.records.is_empty());
    host.send(&mut button, 48, [0, 5, 1], &[]).unwrap();
    assert_eq!(host.records[1].command.data, [7, 5, 1]);
}

type Records = Rc<RefCell<Vec<Command>>>;
struct Factory {
    records: Records,
    reply: Rc<Cell<Option<u32>>>,
    fail: Rc<Cell<bool>>,
}
struct Receiver {
    base: BaseObject,
    records: Records,
    reply: Rc<Cell<Option<u32>>>,
    fail: Rc<Cell<bool>>,
    write_parent: bool,
}
impl ObjectFactory for Factory {
    fn create(&mut self, _: u32, object_kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        assert_eq!(object_kind, kind::BASE);
        Ok(Box::new(Receiver {
            base: BaseObject::new(),
            records: self.records.clone(),
            reply: self.reply.clone(),
            fail: self.fail.clone(),
            write_parent: name == b"write-parent",
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
    fn update_body(&mut self, host: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        if self.write_parent {
            host.dispatch_task_command(
                packet(host.parent_task_id()?, 4, 95, [0, 73, 0]),
                &mut CommandBuffers::default(),
                &mut 0,
            )?;
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
            let native = if command.data[0] == 0 {
                104
            } else {
                command.data[0]
            };
            assert_eq!(
                host.call(native, buffers.data1.as_deref().unwrap(), &mut vm)?,
                NativeReturn::Continue
            );
            return Ok(CommandOutcome::finished(false));
        }
        if matches!(command.code, 6 | 9 | 10 | 64 | 65) {
            if command.code == 9 {
                let query = packet(command.sender, command.target, 96, [0, 1, 1]);
                let mut value = 0x13579bdf;
                host.dispatch_task_command(query, &mut CommandBuffers::default(), &mut value)?;
                self.records.borrow_mut().push(Command {
                    data: [value, 0, 0],
                    ..query
                });
            }
            self.records.borrow_mut().push(*command);
            if let Some(reply) = self.reply.get() {
                host.dispatch_task_command(
                    packet(command.sender, command.target, 95, [0, reply, 0]),
                    &mut CommandBuffers::default(),
                    &mut 0,
                )?;
            }
            if command.code == 6 {
                buffers.data0.as_deref_mut().unwrap()[0] = 0x12;
                buffers.data1.as_deref_mut().unwrap()[0] = 0x34;
            }
            if self.fail.get() {
                return Err(Error::new(0, "合成接收失败"));
            }
            *output = 0xabcdef01;
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
type TestRuntime = Runtime<InputFactory<Factory>>;
type TestSetup = (TestRuntime, Records, Rc<Cell<Option<u32>>>, Rc<Cell<bool>>);
struct IdleDevice;
impl InputDevice for IdleDevice {
    fn poll(&mut self) -> Result<()> {
        Ok(())
    }
    fn pointer(&mut self) -> Result<PointerSample> {
        Ok(PointerSample::default())
    }
    fn key(&mut self, _: u32, _: u32, _: bool) -> Result<KeySample> {
        Ok(KeySample::default())
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
fn setup() -> TestSetup {
    let records = Records::default();
    let reply = Rc::new(Cell::new(None));
    let fail = Rc::new(Cell::new(false));
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(RootObject::new(1, 77)),
        expression::State::new(0),
        InputFactory {
            fallback: Factory {
                records: records.clone(),
                reply: reply.clone(),
                fail: fail.clone(),
            },
        },
    );
    runtime.set_input_device(IdleDevice).unwrap();
    assert_eq!(runtime.create(0, kind::INPUT, b"input", b"").unwrap(), 1);
    assert_eq!(runtime.create(0, kind::BASE, b"receiver", b"").unwrap(), 2);
    assert_eq!(runtime.create(0, kind::BUTTON, b"button", b"").unwrap(), 3);
    runtime.input_coordinates_mut().logical_size = [800, 600];
    let mut buffer = bytes(&[0, 0, 800, 600, 7, 88]);
    assert_eq!(
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(3, 2, 90, [0; 3]),
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut buffer),
                    external: None,
                }
            )
            .unwrap(),
        1
    );
    (runtime, records, reply, fail)
}
fn send(runtime: &mut TestRuntime, code: u32, b: u32) -> u32 {
    runtime
        .dispatch_command(0, packet(3, 2, code, [0, b, 0]))
        .unwrap()
}

#[test]
fn real_input_callbacks_share_response_buffers_and_deletion_cleanup() {
    let (mut runtime, records, reply, fail) = setup();
    send(&mut runtime, 93, 1);
    runtime.update_input_selection(1, Some([25, 35])).unwrap();
    assert_eq!(
        records.borrow().last().unwrap(),
        &packet(2, 3, 64, [3, 1, 0])
    );
    reply.set(Some(0));
    let event = KeyEvent {
        kind: KeyEventKind::Change,
        code: 4,
        key: 0,
        pressed: 1,
        mask: 2,
        point: [25, 35],
    };
    assert!(runtime.dispatch_input_key(1, event).unwrap());
    assert_eq!(runtime.input_state(1).unwrap().events().captures()[0], 3);
    assert_eq!(
        records.borrow().last().unwrap(),
        &packet(2, 3, 9, [3, 0, 0])
    );
    assert!(records.borrow().contains(&packet(3, 2, 96, [2, 0, 0])));
    assert_eq!(send(&mut runtime, 96, 0), 2);
    let mut a = [0; 8];
    let mut b = [0; 8];
    fail.set(true);
    reply.set(Some(73));
    assert!(
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(3, 2, 6, [0; 3]),
                &mut CommandBuffers {
                    data0: Some(&mut a),
                    data1: Some(&mut b),
                    external: None,
                }
            )
            .is_err()
    );
    assert_eq!((a[0], b[0]), (0x12, 0x34));
    fail.set(false);
    reply.set(None);
    assert_eq!(send(&mut runtime, 4, 0), 73);
    assert_eq!(send(&mut runtime, 4, 0), 0);
    send(&mut runtime, 3, 0);
    assert!(runtime.input_state(1).unwrap().regions().get(3).is_none());
    assert_eq!(runtime.object(3).unwrap().control().deletion_requested, 1);
    runtime.update(1, 1).unwrap();
    assert!(runtime.object(3).is_none());
    assert_eq!(
        runtime
            .create(0, kind::BUTTON, b"replacement", b"")
            .unwrap(),
        3
    );
}

#[test]
fn frame_reset_happens_before_children_can_write_the_response() {
    let (mut runtime, _, _, _) = setup();
    runtime.update(1, 1).unwrap();
    assert_eq!(send(&mut runtime, 4, 1), 0);
    assert_eq!(
        runtime.create(3, kind::BASE, b"write-parent", b"").unwrap(),
        4
    );
    runtime.update(2, 1).unwrap();
    assert_eq!(send(&mut runtime, 4, 1), 73);
    assert_eq!(send(&mut runtime, 4, 1), 0);
}

#[test]
fn ordinary_rectangle_registration_reaches_button_and_input_with_wrapped_endpoints() {
    for budget in [1, 4096] {
        let (mut runtime, _, _, _) = setup();
        runtime.set_budget(budget, 1 << 20);
        let button = runtime.create(0, kind::BUTTON, b"native", b"").unwrap();
        let mut arguments = bytes(&[
            button,
            i32::MAX as u32,
            (-10i32) as u32,
            10,
            20,
            0x8000_0000,
            0xfedc_ba98,
        ]);
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(2, 0, 800, [0; 3]),
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut arguments),
                    external: None,
                },
            )
            .unwrap();
        let region = runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(button)
            .unwrap();
        assert_eq!(region.bounds(), rect([i32::MAX, -10, i32::MIN + 9, 10]));
        assert_eq!(region.priority(), i32::MIN);
        assert_eq!(region.event_mask(), 7);
        let mut arguments = bytes(&[button, 0x8000_0001]);
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(2, 0, 800, [105, 0, 0]),
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut arguments),
                    external: None,
                },
            )
            .unwrap();
        let region = runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(button)
            .unwrap();
        assert_eq!(region.enabled(), 0x8000_0001);
        let mut arguments = bytes(&[button, 0xffff_fffe]);
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(2, 0, 800, [848, 0, 0]),
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut arguments),
                    external: None,
                },
            )
            .unwrap();
        let region = runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(button)
            .unwrap();
        assert_eq!(region.selection_enabled(), 0xffff_fffe);
        let mut arguments = bytes(&[button, 0x8000_0002]);
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(2, 0, 800, [849, 0, 0]),
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut arguments),
                    external: None,
                },
            )
            .unwrap();
        let region = runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(button)
            .unwrap();
        assert_eq!(region.motion_enabled(), 0x8000_0002);
        let initial = region.clone();
        let mut arguments = bytes(&[button, 0, 0, 800, 600, 9, 0]);
        runtime
            .dispatch_command_with_buffers(
                0,
                packet(2, 0, 800, [0; 3]),
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut arguments),
                    external: None,
                },
            )
            .unwrap();
        assert_eq!(
            runtime
                .input_state(1)
                .unwrap()
                .regions()
                .get(button)
                .unwrap(),
            &initial
        );
        let mut arguments = bytes(&[0]);
        assert!(
            runtime
                .dispatch_command_with_buffers(
                    0,
                    packet(2, 0, 800, [0; 3]),
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut arguments),
                        external: None,
                    },
                )
                .is_err()
        );
    }
}
