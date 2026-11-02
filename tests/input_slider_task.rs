use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{GraphicsFactory, Rect, RenderTarget},
    input::{
        InputDevice, InputFactory, KeyEvent, KeyEventKind, KeySample, PointerMotion, PointerSample,
    },
    kcs::{Host, Interpreter, ResourceServices},
    resource::ResourceMode,
    runtime::{ObjectFactory, RootObject, Runtime, TaskObject, kind},
    task::{Command, CommandOutcome, Control, TaskClock},
};
use std::{cell::RefCell, io::Write, rc::Rc};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}
struct Parent {
    root: RootObject,
    events: Rc<RefCell<Vec<Command>>>,
}
impl TaskObject for Parent {
    fn control(&self) -> &Control {
        self.root.control()
    }
    fn clock(&self) -> &TaskClock {
        self.root.clock()
    }
    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.root.state()
    }
    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }
    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }
    fn set_creation_context(&mut self, c: &[u8]) -> Result<()> {
        self.root.set_creation_context(c)
    }
    fn update_body(&mut self, h: &mut dyn Host, n: u32, b: usize) -> Result<bool> {
        self.root.update_body(h, n, b)
    }
    fn dispatch_command(
        &mut self,
        k: u32,
        c: &Command,
        o: &mut u32,
        h: &mut dyn Host,
        b: usize,
    ) -> Result<CommandOutcome> {
        if matches!(c.code, 27 | 28) {
            self.events.borrow_mut().push(*c);
            Ok(CommandOutcome::finished(false))
        } else {
            self.root.dispatch_command(k, c, o, h, b)
        }
    }
    fn enqueue_message(&mut self, m: [u32; 4]) -> Result<()> {
        self.root.enqueue_message(m)
    }
    fn dispatch_message(
        &mut self,
        m: &mut [u32; 4],
        o: &mut u32,
        h: &mut dyn Host,
        b: usize,
    ) -> Result<bool> {
        self.root.dispatch_message(m, o, h, b)
    }
    fn request_delete(&mut self, h: &mut dyn Host, b: usize) -> Result<bool> {
        self.root.request_delete(h, b)
    }
}
struct Files(Vec<u8>);
impl ResourceServices for Files {
    fn resolve(&mut self, _: &[u8], _: &[u8]) -> Result<Vec<u8>> {
        unreachable!()
    }
    fn read(&mut self, n: &[u8], m: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(m, ResourceMode::LooseFirst);
        Ok((n == b"synthetic").then(|| self.0.clone()))
    }
}
#[derive(Default)]
struct DeviceState {
    point: [i32; 2],
    touches: usize,
}
struct Device(Rc<RefCell<DeviceState>>);
impl InputDevice for Device {
    fn poll(&mut self) -> Result<()> {
        Ok(())
    }
    fn pointer(&mut self) -> Result<PointerSample> {
        Ok(PointerSample {
            position: self.0.borrow().point,
            active: 1,
        })
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
    fn set_pointer(&mut self, p: [i32; 2]) -> Result<()> {
        self.0.borrow_mut().point = p;
        Ok(())
    }
    fn clear_keys(&mut self) -> Result<()> {
        Ok(())
    }
    fn clear_touches(&mut self) -> Result<()> {
        self.0.borrow_mut().touches += 1;
        Ok(())
    }
}

type Engine = Runtime<GraphicsFactory<InputFactory<Reject>>>;
fn packet(target: u32, code: u32, a: u32, b: u32) -> Command {
    Command {
        target,
        sender: 0,
        code,
        data: [a, b, 0],
    }
}
fn send(e: &mut Engine, id: u32, code: u32, a: u32, b: u32) -> u32 {
    e.dispatch_command(0, packet(id, code, a, b)).unwrap()
}
fn bytes(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|n| n.to_le_bytes()).collect()
}
fn buffer(e: &mut Engine, id: u32, code: u32, data: &mut [u8]) {
    e.dispatch_command_with_buffers(
        0,
        packet(id, code, 0, 0),
        &mut katze_system2::task::CommandBuffers {
            data0: None,
            data1: Some(data),
        },
    )
    .unwrap();
}
fn fixture() -> Vec<u8> {
    let zip = |input: &[u8]| {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
        z.write_all(input).unwrap();
        z.finish().unwrap()
    };
    let delta = 0xff12_3456u32
        .to_le_bytes()
        .map(|v| (v << 1) ^ 0u8.wrapping_sub(v >> 7));
    let planes: Vec<_> = [6, 4, 2, 0]
        .into_iter()
        .map(|shift| {
            delta
                .iter()
                .enumerate()
                .fold(0, |p, (i, v)| p | (((v >> shift) & 3) << (2 * i)))
        })
        .collect();
    let data = zip(&planes);
    let control = zip(&[9, 1]);
    let mut out = b"HG-3".to_vec();
    out.extend(bytes(&[12, 0x300, 0, 0]));
    out.extend(b"stdinfo\0");
    out.extend(bytes(&[56, 40, 1, 1, 32, 1, 1, 3, 3, 1, 0, 0]));
    out.extend(b"img0000\0");
    out.extend(bytes(&[
        0,
        (24 + data.len() + control.len()) as u32,
        0,
        1,
        data.len() as u32,
        4,
        control.len() as u32,
        2,
    ]));
    out.extend(data);
    out.extend(control);
    out
}
struct Fixture {
    engine: Engine,
    list: u32,
    slider: u32,
    events: Rc<RefCell<Vec<Command>>>,
    device: Rc<RefCell<DeviceState>>,
    configuration: Vec<u8>,
}
fn setup() -> Fixture {
    let events = Rc::default();
    let mut e = Runtime::new(
        0,
        kind::ROOT,
        Box::new(Parent {
            root: RootObject::new(1, 0),
            events: Rc::clone(&events),
        }),
        State::new(0),
        GraphicsFactory {
            fallback: InputFactory { fallback: Reject },
        },
    );
    e.host_mut().surface_size = [32, 16];
    e.input_coordinates_mut().logical_size = [32, 16];
    let d = Rc::new(RefCell::new(DeviceState {
        point: [10, 2],
        ..Default::default()
    }));
    e.set_input_device(Device(d.clone())).unwrap();
    assert_eq!(e.create(0, kind::INPUT, b"input", b"").unwrap(), 1);
    e.drawing_mut().enabled = true;
    e.drawing_mut().active = true;
    e.drawing_mut().target = Some(RenderTarget::new([32, 16]).unwrap());
    let list = e.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
    send(&mut e, list, 285, 32, 16);
    e.set_resources(Files(fixture()));
    let slider = e.create(0, kind::SLIDER, b"slider", b"").unwrap();
    let mut cfg = vec![0; 0x124];
    cfg[..4].copy_from_slice(&list.to_le_bytes());
    cfg[4..13].copy_from_slice(b"synthetic");
    cfg[0x108..].copy_from_slice(&bytes(&[5, 2, 2, 22, 6, 1, 2]));
    buffer(&mut e, slider, 365, &mut cfg);
    Fixture {
        engine: e,
        list,
        slider,
        events,
        device: d,
        configuration: cfg,
    }
}
fn draw(e: &mut Engine, list: u32, x: u32, y: u32) -> u32 {
    send(e, list, 298, 0, 0);
    e.drawing().target.as_ref().unwrap().pixel(x, y).unwrap()
}
fn event(pressed: u32) -> KeyEvent {
    KeyEvent {
        kind: KeyEventKind::Change,
        code: 4,
        key: 0,
        pressed,
        mask: 2,
        point: [10, 2],
    }
}

#[test]
fn slider_buttons_drive_pixels_drag_notifications_and_touch_reset() {
    let Fixture {
        engine: mut e,
        list,
        slider,
        events,
        device,
        ..
    } = setup();
    let children: Vec<_> = e.tree().children(slider).unwrap().collect();
    assert_eq!(children.len(), 2);
    let [track, knob] = [children[0], children[1]];
    for id in children {
        assert_eq!(e.tree().node(id).unwrap().kind, kind::BUTTON);
    }
    let regions = e.input_state(1).unwrap().regions();
    assert_eq!(
        regions.get(track).unwrap().bounds(),
        Rect::from_xywh(2, 2, 20, 4)
    );
    assert_eq!(
        regions.get(knob).unwrap().bounds(),
        Rect::from_xywh(3, 3, 1, 1)
    );
    assert_eq!(regions.get(knob).unwrap().priority(), 6);
    assert_eq!(send(&mut e, slider, 370, 0, 0), 0);
    assert_eq!(draw(&mut e, list, 4, 5), 0xff12_3456);
    buffer(&mut e, slider, 368, &mut bytes(&[0, 10]));
    send(&mut e, slider, 369, 1, 5);
    assert_eq!(events.borrow().last().unwrap().data, [0, 5, 0]);
    assert_eq!(
        e.input_state(1)
            .unwrap()
            .regions()
            .get(knob)
            .unwrap()
            .bounds(),
        Rect::from_xywh(10, 2, 1, 1)
    );
    assert_eq!(draw(&mut e, list, 12, 5), 0xff12_3456);
    // 缺失焦点帧不提交有效来源，列表继续持有旧像素。
    device.borrow_mut().point = [10, 2];
    e.update_input_selection(1, Some([10, 2])).unwrap();
    assert_eq!(draw(&mut e, list, 12, 5), 0xff12_3456);
    send(&mut e, slider, 372, 2, 0);
    send(&mut e, slider, 372, 3, 0);
    send(&mut e, slider, 373, 0, 0);
    send(&mut e, knob, 95, 0, 0);
    assert!(e.dispatch_input_key(1, event(1)).unwrap());
    assert_eq!(
        e.input_state(1)
            .unwrap()
            .regions()
            .get(knob)
            .unwrap()
            .bounds(),
        Rect::default()
    );
    e.dispatch_input_motion(
        1,
        PointerMotion {
            code: 6,
            previous: [10, 2],
            current: [15, 2],
        },
    )
    .unwrap();
    assert_eq!(events.borrow().last().unwrap().data, [0, 8, 0]);
    assert_eq!(draw(&mut e, list, 17, 5), 0xff12_3456);
    let notifications = events.borrow().len();
    assert!(e.dispatch_input_key(1, event(0)).unwrap());
    assert_eq!(events.borrow().len(), notifications);
    assert_eq!(
        e.input_state(1)
            .unwrap()
            .regions()
            .get(knob)
            .unwrap()
            .bounds(),
        Rect::from_xywh(15, 2, 1, 1)
    );
    send(&mut e, slider, 65, knob, 0);
    assert_eq!(device.borrow().touches, 1);
    send(&mut e, slider, 367, 0, 0);
    assert_eq!(
        e.input_state(1)
            .unwrap()
            .regions()
            .get(track)
            .unwrap()
            .enabled(),
        0
    );
}

#[test]
fn slider_missing_reload_and_deferred_destruction_remove_cross_task_entries() {
    for budget in [1, 4096] {
        let Fixture {
            engine: mut e,
            list,
            slider,
            configuration: mut cfg,
            ..
        } = setup();
        e.set_budget(budget, 1000);
        let children: Vec<_> = e.tree().children(slider).unwrap().collect();
        buffer(&mut e, slider, 368, &mut bytes(&[0, 10]));
        send(&mut e, slider, 369, 0, 4);
        cfg[4..0x108].fill(0);
        cfg[4..11].copy_from_slice(b"missing");
        buffer(&mut e, slider, 365, &mut cfg);
        assert_eq!(send(&mut e, list, 294, 0, 0), 0);
        assert_eq!(
            e.tree().children(slider).unwrap().collect::<Vec<_>>(),
            children
        );
        cfg[4..0x108].fill(0);
        cfg[4..13].copy_from_slice(b"synthetic");
        buffer(&mut e, slider, 365, &mut cfg);
        assert_eq!(
            e.tree().children(slider).unwrap().collect::<Vec<_>>(),
            children
        );
        assert_eq!(send(&mut e, list, 294, 0, 0), 1);
        send(&mut e, slider, 3, 0, 0);
        assert_eq!(send(&mut e, list, 294, 0, 0), 1);
        e.update(1, 1).unwrap();
        assert!(e.object(slider).is_none());
        for child in children {
            assert!(e.object(child).is_none());
            assert!(e.input_state(1).unwrap().regions().get(child).is_none());
        }
        assert_eq!(send(&mut e, list, 294, 0, 0), 0);
        assert_eq!(draw(&mut e, list, 4, 5), 0);
        let next = e.create(0, kind::SLIDER, b"next", b"").unwrap();
        assert_eq!(next, slider);
        buffer(&mut e, next, 365, &mut cfg);
        e.destroy(0).unwrap();
        assert!(e.tree().node(0).is_none());
    }
}
