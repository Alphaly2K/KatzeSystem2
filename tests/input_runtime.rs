use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use katze_system2::{
    Error, Result, expression,
    input::{
        HitMask, InputDevice, InputFactory, KeyEvent, KeyEventKind, KeySample, PointerMotion,
        PointerSample,
    },
    kcs::{Host, Interpreter, NativeReturn, Program},
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock},
};

#[derive(Debug)]
struct Record {
    packet: Command,
    selected: u32,
    raw: u32,
    points: Vec<u8>,
}

type Records = Rc<RefCell<Vec<Record>>>;

struct Factory {
    mode: Rc<Cell<u32>>,
    records: Records,
}

struct Receiver {
    base: BaseObject,
    id: u32,
    mode: Rc<Cell<u32>>,
    records: Records,
}

fn send(host: &mut dyn Host, sender: u32, code: u32, a: u32, b: u32) -> Result<u32> {
    let mut output = 0x13579bdf;
    host.dispatch_task_command(
        Command {
            sender,
            target: 1,
            code,
            data: [a, b, 0],
        },
        &mut CommandBuffers::default(),
        &mut output,
    )?;
    Ok(output)
}

fn region(id: u32, bounds: [i32; 4]) -> [u8; 60] {
    let mut bytes = [0; 60];
    for (index, value) in [(0, id), (2, 2)].into_iter().chain(
        bounds
            .into_iter()
            .enumerate()
            .map(|(i, n)| (i + 3, n as u32)),
    ) {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

impl ObjectFactory for Factory {
    fn create(&mut self, id: u32, object_kind: u32, _name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if object_kind != kind::BASE {
            return Err(Error::new(0, "测试对象类型无效"));
        }
        Ok(Box::new(Receiver {
            base: BaseObject::new(),
            id,
            mode: self.mode.clone(),
            records: self.records.clone(),
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
        packet: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        self.dispatch_command_with_buffers(
            kind,
            packet,
            &mut CommandBuffers::default(),
            output,
            host,
            budget,
        )
    }
    fn dispatch_command_with_buffers(
        &mut self,
        kind: u32,
        packet: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        if !matches!(packet.code, 4..=8 | 22) {
            return self
                .base
                .dispatch_command(kind, packet, output, host, budget);
        }
        let selected = send(host, self.id, 134, self.id, 0)?;
        let raw = send(host, self.id, 137, 0, 0)?;
        let mut points = Vec::new();
        if packet.code == 6 {
            points.extend_from_slice(buffers.data0.as_deref().unwrap());
            points.extend_from_slice(buffers.data1.as_deref().unwrap());
        } else if packet.code == 22 {
            let mut shown = [0xff; 8];
            host.dispatch_task_command(
                Command {
                    sender: self.id,
                    target: 1,
                    code: 149,
                    data: [0; 3],
                },
                &mut CommandBuffers {
                    data0: None,
                    data1: Some(&mut shown),
                },
                &mut 0,
            )?;
            assert_eq!(&shown[4..], &[0xff; 4]);
            points.extend_from_slice(&shown[..4]);
        } else if self.mode.get() == 8 && packet.code == 4 {
            let mut vm = Interpreter::new(Program::from_code(&[], vec![0; 8], 16)?, vec![])?;
            let arguments: Vec<_> = [0u32, 4].into_iter().flat_map(u32::to_le_bytes).collect();
            assert_eq!(host.call(149, &arguments, &mut vm)?, NativeReturn::Continue);
            points.extend_from_slice(vm.read(0, 8)?);
        }
        self.records.borrow_mut().push(Record {
            packet: *packet,
            selected,
            raw,
            points,
        });
        match (self.mode.get(), packet.code, self.id) {
            (1, 4, 2) => {
                send(host, self.id, 24, 0, 0x12345678)?;
                send(host, self.id, 29, 0, 7)?;
                send(host, self.id, 132, self.id, 0)?;
                let mut bytes = region(3, [0; 4]);
                host.dispatch_task_command(
                    Command {
                        sender: self.id,
                        target: 1,
                        code: 127,
                        data: [0; 3],
                    },
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut bytes),
                    },
                    &mut 0,
                )?;
                *output = 1;
            }
            (2 | 3, 4, _) => {
                send(host, self.id, 151, 1, 0)?;
                *output = 1;
                if self.mode.get() == 3 {
                    send(host, self.id, 24, 0, 0x87654321)?;
                    send(host, self.id, 138, 0, 0)?;
                }
            }
            (4, 4, _) => {
                let mut vm = Interpreter::new(Program::from_code(&[], vec![], 16)?, vec![])?;
                assert_eq!(
                    host.call(67, &1u32.to_le_bytes(), &mut vm)?,
                    NativeReturn::Continue
                );
                assert_eq!(send(host, self.id, 137, 0, 0)?, 0x13579bdf);
                *output = 0;
            }
            (5, 8, _) => {
                send(host, self.id, 133, self.id, 99)?;
            }
            (5, 6, 2) => {
                let previous = buffers.data0.as_deref_mut().unwrap();
                previous[..4].copy_from_slice(&5i32.to_le_bytes());
                previous[4..].copy_from_slice(&5i32.to_le_bytes());
                let current = buffers.data1.as_deref_mut().unwrap();
                current[..4].copy_from_slice(&(-7i32).to_le_bytes());
                current[4..].copy_from_slice(&17i32.to_le_bytes());
            }
            (6, 4, _) => {}
            (7, 22, 2) if packet.data[1] == 0 => {
                send(host, self.id, 150, 0, 0)?;
                *output = 0;
            }
            _ => *output = 0,
        }
        Ok(CommandOutcome::finished(false))
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

fn runtime(mode: u32) -> (TestRuntime, Rc<Cell<u32>>, Records) {
    let mode = Rc::new(Cell::new(mode));
    let records = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        expression::State::new(0),
        InputFactory {
            fallback: Factory {
                mode: mode.clone(),
                records: records.clone(),
            },
        },
    );
    assert_eq!(runtime.create(0, kind::INPUT, b"input", b"").unwrap(), 1);
    assert_eq!(runtime.create(0, kind::BASE, b"first", b"").unwrap(), 2);
    assert_eq!(runtime.create(0, kind::BASE, b"second", b"").unwrap(), 3);
    (runtime, mode, records)
}

fn register(runtime: &mut TestRuntime, id: u32, priority: u32, bounds: [i32; 4]) {
    let mut bytes = region(id, bounds);
    runtime
        .dispatch_command_with_buffers(
            0,
            Command {
                sender: 0,
                target: 1,
                code: 127,
                data: [priority, 0, 0],
            },
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut bytes),
            },
        )
        .unwrap();
}

fn event(mask: u32) -> KeyEvent {
    KeyEvent {
        kind: KeyEventKind::Change,
        code: 4,
        key: 0,
        pressed: 1,
        mask,
        point: [0; 2],
    }
}

#[test]
fn input_callbacks_query_live_state_and_append_a_same_dispatch_target() -> Result<()> {
    let (mut runtime, _, records) = runtime(1);
    register(&mut runtime, 2, 10, [0; 4]);
    runtime.host_mut().task_id = 0;
    runtime.host_mut().task_time = 99;
    runtime.host_mut().task_origin = 88;
    runtime.set_budget(1, 1024);
    assert!(runtime.dispatch_input_key(1, event(2))?);
    let state = runtime.input_state(1).unwrap();
    assert_eq!(
        state
            .regions()
            .records()
            .iter()
            .map(|r| r.id())
            .collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(state.regions().get(2).unwrap().event_mask(), 0);
    assert_eq!(state.events().captures()[0], 3);
    assert_eq!(runtime.object(1).unwrap().control().property, 0x12345678);
    assert_eq!(runtime.object(1).unwrap().clock().paused, 7);
    assert_eq!(
        (
            runtime.host().task_id,
            runtime.host().task_time,
            runtime.host().task_origin
        ),
        (0, 99, 88)
    );
    let records = records.borrow();
    assert_eq!(
        records.iter().map(|r| r.packet.target).collect::<Vec<_>>(),
        [2, 3]
    );
    assert!(
        records
            .iter()
            .all(|r| r.raw == 2 && r.packet.sender == 1 && r.packet.data == [0, 1, 0])
    );
    Ok(())
}

#[test]
fn scanning_callbacks_modify_later_keys_and_restore_all_state_on_errors() -> Result<()> {
    let (mut runtime, mode, _) = runtime(2);
    register(&mut runtime, 2, 10, [0; 4]);
    let mut queried = Vec::new();
    assert!(!runtime.scan_input_keys(1, [0; 2], |key, group| {
        queried.push((key, group));
        Ok(KeySample {
            mask: if key < 2 && group == 0 { 2 } else { 0 },
            repeat: 1,
        })
    })?);
    assert!(queried.iter().all(|&(key, _)| key != 1));
    assert_eq!(queried.len(), 316);
    let state = runtime.input_state(1).unwrap();
    assert_eq!(state.keys().enabled()[1], 0);
    assert_eq!(state.keys().records()[0][0].count, 1);
    assert_eq!(state.keys().records()[1][0].count, 0);
    mode.set(3);
    let error = runtime
        .scan_input_keys(1, [0; 2], |key, group| {
            Ok(KeySample {
                mask: if key == 0 && group == 0 { 3 } else { 0 },
                repeat: 1,
            })
        })
        .unwrap_err();
    assert!(error.message.contains("138"));
    assert_eq!(
        runtime.input_state(1).unwrap().keys().records()[0][0].mask,
        3
    );
    assert_eq!(runtime.input_state(1).unwrap().events().raw_state(), 3);
    assert_eq!(runtime.object(1).unwrap().control().property, 0x87654321);
    assert_eq!(
        runtime.dispatch_command(
            0,
            Command {
                sender: 0,
                target: 1,
                code: 137,
                data: [0; 3]
            }
        )?,
        3
    );
    mode.set(6);
    assert!(!runtime.dispatch_input_key(1, event(2))?);
    assert_eq!(runtime.input_state(1).unwrap().events().captures()[0], 0);
    mode.set(0);
    assert!(runtime.dispatch_input_key(1, event(2))?);
    assert!(
        runtime
            .dispatch_input_key(2, event(2))
            .unwrap_err()
            .message
            .contains("输入对象")
    );
    assert!(runtime.object(2).is_some());
    Ok(())
}

#[test]
fn pointer_notifications_keep_selection_timing_and_share_mutable_buffers() -> Result<()> {
    let (mut runtime, _, records) = runtime(5);
    register(&mut runtime, 2, 10, [0, 0, 2, 2]);
    register(&mut runtime, 3, 0, [5, 5, 8, 8]);
    runtime.update_input_selection(1, Some([1, 1]))?;
    assert_eq!(
        runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(2)
            .unwrap()
            .selected(),
        1
    );
    runtime.update_input_selection(1, None)?;
    assert_eq!(
        runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(2)
            .unwrap()
            .selected(),
        0
    );
    runtime.dispatch_input_motion(
        1,
        PointerMotion {
            code: 6,
            previous: [1, 1],
            current: [9, 9],
        },
    )?;
    let records = records.borrow();
    assert_eq!(
        records
            .iter()
            .map(|r| (r.packet.target, r.packet.code, r.selected))
            .collect::<Vec<_>>(),
        [(2, 7, 1), (2, 8, 1), (2, 6, 0), (3, 6, 0)]
    );
    let points: Vec<_> = records[3]
        .points
        .chunks_exact(4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(points, [5, 5, -7, 17]);
    Ok(())
}

#[test]
fn input_deletion_in_a_callback_is_deferred_and_recreation_has_fresh_state() -> Result<()> {
    let (mut runtime, _, _) = runtime(4);
    register(&mut runtime, 2, 10, [0; 4]);
    assert!(
        runtime
            .update(1, 1)
            .unwrap_err()
            .message
            .contains("自动设备更新")
    );
    assert!(runtime.dispatch_input_key(1, event(2))?);
    assert_eq!(runtime.object(1).unwrap().control().deletion_requested, 1);
    assert!(runtime.update(2, 1)?);
    assert!(runtime.object(1).is_none());
    assert!(runtime.input_state(1).is_none());
    assert_eq!(runtime.create(0, kind::INPUT, b"new", b"")?, 1);
    let state = runtime.input_state(1).unwrap();
    assert!(state.regions().records().is_empty());
    assert_eq!(state.keys().enabled(), &[1; 80]);
    assert_eq!(state.events().captures(), &[0; 80]);
    assert_eq!(state.events().raw_state(), 0);
    assert!(
        runtime
            .create(0, kind::INPUT, b"context", b"nonempty")
            .is_err()
    );
    Ok(())
}

struct DeviceState {
    sample: PointerSample,
    positions: VecDeque<[i32; 2]>,
    pointer_calls: usize,
    fail_pointer: Option<usize>,
    key_mask: u32,
    queries: Vec<(u32, u32, bool)>,
    wheel: u32,
    cursor: u32,
    shows: Vec<u32>,
    calls: Vec<&'static str>,
}

impl DeviceState {
    fn new(position: [i32; 2]) -> Self {
        Self {
            sample: PointerSample {
                position,
                active: 1,
            },
            positions: VecDeque::new(),
            pointer_calls: 0,
            fail_pointer: None,
            key_mask: 0,
            queries: Vec::new(),
            wheel: 0,
            cursor: 1,
            shows: Vec::new(),
            calls: Vec::new(),
        }
    }
}

struct Device(Rc<RefCell<DeviceState>>);

impl InputDevice for Device {
    fn poll(&mut self) -> Result<()> {
        self.0.borrow_mut().calls.push("poll");
        Ok(())
    }
    fn pointer(&mut self) -> Result<PointerSample> {
        let mut state = self.0.borrow_mut();
        state.calls.push("pointer");
        state.pointer_calls += 1;
        if state.fail_pointer == Some(state.pointer_calls) {
            return Err(Error::new(0, "合成设备查询失败"));
        }
        let mut sample = state.sample;
        if let Some(position) = state.positions.pop_front() {
            sample.position = position;
        }
        Ok(sample)
    }
    fn key(&mut self, key: u32, group: u32, repeat: bool) -> Result<KeySample> {
        let mut state = self.0.borrow_mut();
        state.calls.push("key");
        state.queries.push((key, group, repeat));
        Ok(KeySample {
            mask: if key == 0 && group == 0 {
                state.key_mask
            } else {
                0
            },
            repeat: 1,
        })
    }
    fn take_wheel(&mut self) -> Result<u32> {
        let mut state = self.0.borrow_mut();
        state.calls.push("wheel");
        Ok(std::mem::take(&mut state.wheel))
    }
    fn is_foreground(&mut self) -> Result<bool> {
        self.0.borrow_mut().calls.push("foreground");
        Ok(true)
    }
    fn finish(&mut self) -> Result<()> {
        self.0.borrow_mut().calls.push("finish");
        Ok(())
    }
    fn commit(&mut self) -> Result<()> {
        self.0.borrow_mut().calls.push("commit");
        Ok(())
    }
    fn history(&mut self) -> Result<[u8; 32]> {
        self.0.borrow_mut().calls.push("history");
        Ok(std::array::from_fn(|i| i as u8))
    }
    fn show_cursor(&mut self, visible: u32) -> Result<()> {
        let mut state = self.0.borrow_mut();
        state.calls.push("show");
        state.cursor = visible;
        state.shows.push(visible);
        Ok(())
    }
    fn cursor_visible(&mut self) -> Result<u32> {
        Ok(self.0.borrow().cursor)
    }
}

fn input_command(runtime: &mut TestRuntime, code: u32, data: [u32; 3]) -> Result<u32> {
    runtime.dispatch_command(
        0,
        Command {
            sender: 0,
            target: 1,
            code,
            data,
        },
    )
}

fn integers(bytes: &[u8]) -> Vec<i32> {
    bytes
        .chunks_exact(4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}

#[test]
fn device_update_preserves_query_times_and_continues_after_key_consumption() -> Result<()> {
    let (mut runtime, _, records) = runtime(8);
    register(&mut runtime, 2, 0, [0; 4]);
    let state = Rc::new(RefCell::new(DeviceState::new([10, 20])));
    state.borrow_mut().positions = [[10, 20], [10, 20], [30, 40], [50, 60]].into();
    state.borrow_mut().key_mask = 2;
    runtime.input_coordinates_mut().device_scale = 2.;
    runtime.set_input_device(Device(state.clone()))?;
    assert_eq!(runtime.input_state(1).unwrap().pointer().position, [10, 20]);
    assert!(runtime.update(1, 1)?);
    let records = records.borrow();
    assert_eq!(
        records.iter().map(|r| r.packet.code).collect::<Vec<_>>(),
        [4, 6]
    );
    assert_eq!(integers(&records[0].points), [25, 30]);
    assert_eq!(integers(&records[1].points), [15, 20, 5, 10]);
    let input = runtime.input_state(1).unwrap();
    assert_eq!(input.pointer().position, [10, 20]);
    assert_eq!(input.pointer().origin, [10, 20]);
    assert_eq!(input.events().suppressed(), 1);
    assert_eq!(input.events().captures()[0], 2);
    assert_eq!(state.borrow().pointer_calls, 4);
    assert_eq!(
        state.borrow().queries,
        [(0, 0, true), (0, 0, false), (0, 1, false)]
    );
    assert!(
        state
            .borrow()
            .calls
            .ends_with(&["finish", "commit", "history"])
    );

    state.borrow_mut().sample.position = [14, -18];
    let mut x = [0xaa; 8];
    let mut y = [0xbb; 8];
    runtime.dispatch_command_with_buffers(
        0,
        Command {
            sender: 0,
            target: 1,
            code: 126,
            data: [0; 3],
        },
        &mut CommandBuffers {
            data0: Some(&mut x),
            data1: Some(&mut y),
        },
    )?;
    assert_eq!(integers(&x[..4]), [7]);
    assert_eq!(integers(&y[..4]), [-9]);
    assert_eq!(&x[4..], &[0xaa; 4]);
    assert_eq!(&y[4..], &[0xbb; 4]);
    let id = runtime.create(0, kind::INPUT, b"second-input", b"")?;
    assert_eq!(
        runtime.input_state(id).unwrap().pointer().position,
        [14, -18]
    );
    assert_eq!(state.borrow().pointer_calls, 6);
    Ok(())
}

#[test]
fn automatic_cursor_broadcast_exposes_live_state_and_keeps_callback_changes() -> Result<()> {
    let (mut runtime, _, records) = runtime(7);
    let state = Rc::new(RefCell::new(DeviceState::new([0; 2])));
    runtime.set_input_device(Device(state.clone()))?;
    let pointer = runtime.input_state_mut(1).unwrap().pointer_mut();
    pointer.auto_hide = 1;
    pointer.idle_limit = 2;
    assert!(runtime.update(1, 1)?);
    assert!(records.borrow().is_empty());
    assert!(runtime.update(2, 1)?);
    assert_eq!(
        runtime.input_state(1).unwrap().pointer().cursor_requested,
        0
    );
    assert_eq!(runtime.input_state(1).unwrap().pointer().cursor_shown, 0);
    state.borrow_mut().sample.position = [1; 2];
    assert!(runtime.update(3, 1)?);
    assert_eq!(state.borrow().shows, [0, 0]);
    input_command(&mut runtime, 150, [0, 1, 0])?;
    input_command(&mut runtime, 146, [0; 3])?;
    assert!(runtime.update(4, 1)?);
    assert_eq!(runtime.input_state(1).unwrap().pointer().cursor_shown, 0);
    state.borrow_mut().sample.position = [2; 2];
    assert!(runtime.update(5, 1)?);
    assert_eq!(state.borrow().shows, [0, 0, 1, 1]);
    let records = records.borrow();
    assert_eq!(
        records
            .iter()
            .map(|r| (r.packet.target, r.packet.data))
            .collect::<Vec<_>>(),
        [
            (2, [0, 0, 0]),
            (3, [0, 0, 0]),
            (2, [0, 1, 0]),
            (3, [0, 1, 0])
        ]
    );
    assert!(records.iter().all(|r| r.packet.sender == 1
        && r.packet.code == 22
        && integers(&r.points) == [r.packet.data[1] as i32]));
    Ok(())
}

#[test]
fn wheel_ignores_event_masks_and_snapshot_commands_preserve_buffer_tails() -> Result<()> {
    let (mut runtime, _, records) = runtime(0);
    register(&mut runtime, 2, 0, [0, 0, 8, 8]);
    input_command(&mut runtime, 132, [2, 0, 0])?;
    input_command(&mut runtime, 136, [2, 0, 0])?;
    input_command(&mut runtime, 154, [2, 0, 0])?;
    runtime
        .input_state_mut(1)
        .unwrap()
        .regions_mut()
        .set_hit_mask(2, Some(HitMask::new([8, 8], vec![0; 8])?));
    let state = Rc::new(RefCell::new(DeviceState::new([4; 2])));
    state.borrow_mut().wheel = 1;
    runtime.set_input_device(Device(state.clone()))?;
    assert!(runtime.update(1, 1)?);
    assert!(records.borrow().is_empty());
    runtime
        .input_state_mut(1)
        .unwrap()
        .regions_mut()
        .set_hit_mask(2, None);
    state.borrow_mut().wheel = u32::MAX;
    assert!(runtime.update(2, 1)?);
    let records = records.borrow();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].packet,
        Command {
            sender: 1,
            target: 2,
            code: 5,
            data: [0; 3]
        }
    );
    let mut history = [0xaa; 36];
    runtime.dispatch_command_with_buffers(
        0,
        Command {
            sender: 0,
            target: 1,
            code: 160,
            data: [0; 3],
        },
        &mut CommandBuffers {
            data0: None,
            data1: Some(&mut history),
        },
    )?;
    assert_eq!(
        &history[..32],
        &std::array::from_fn::<_, 32, _>(|i| i as u8)
    );
    assert_eq!(&history[32..], &[0xaa; 4]);
    input_command(&mut runtime, 160, [0; 3])?;
    assert_eq!(input_command(&mut runtime, 153, [0; 3])?, 0);
    state.borrow_mut().sample.active = 0;
    assert!(runtime.update(3, 1)?);
    assert_eq!(input_command(&mut runtime, 153, [0; 3])?, 1);
    input_command(&mut runtime, 3, [0; 3])?;
    input_command(&mut runtime, 3, [0; 3])?;
    assert_eq!(state.borrow().shows, [1]);
    Ok(())
}

#[test]
fn failed_press_position_query_retains_key_state_and_restores_object_access() -> Result<()> {
    let (mut runtime, _, _) = runtime(0);
    register(&mut runtime, 2, 0, [0; 4]);
    let state = Rc::new(RefCell::new(DeviceState::new([3; 2])));
    state.borrow_mut().key_mask = 2;
    state.borrow_mut().fail_pointer = Some(3);
    runtime.set_input_device(Device(state.clone()))?;
    assert!(
        runtime
            .update(1, 1)
            .unwrap_err()
            .message
            .contains("合成设备查询失败")
    );
    let input = runtime.input_state(1).unwrap();
    assert_eq!(input.keys().records()[0][0].count, 1);
    assert_eq!(input.keys().records()[0][0].mask, 2);
    assert_eq!(input.regions().records().len(), 1);
    assert!(runtime.object(1).is_some());
    assert_eq!(input_command(&mut runtime, 134, [2, 0, 0])?, 0);
    Ok(())
}
