#[path = "../examples/support/window_actions.rs"]
mod window_actions;
#[allow(dead_code)]
#[path = "../examples/support/window_input.rs"]
mod window_input;

use katze_system2::{
    Error, Result,
    expression::State,
    input::{DeviceBinding, DeviceKeys, InputDevice, InputFactory},
    kcs::{Program, WindowAction, WindowCloseRequest, WindowRequest},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    settings::SystemSettings,
};
use sdl2::{
    event::{Event, WindowEvent},
    keyboard::{Mod, Scancode},
    mouse::MouseButton,
};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
use window_input::{Shared, WindowDevice};
fn keys() -> DeviceKeys {
    let mut bindings = [DeviceBinding::default(); 80];
    for (slot, source, code) in [
        (0, 1, 1),
        (1, 0, 0x0d),
        (2, 0, 0x10),
        (3, 0, 0x1b),
        (4, 1, 6),
    ] {
        bindings[slot].enabled = 1;
        bindings[slot].codes[source][0] = code;
    }
    DeviceKeys::new(bindings)
}
fn key(scan: Scancode, down: bool) -> Event {
    if down {
        Event::KeyDown {
            timestamp: 0,
            window_id: 7,
            keycode: None,
            scancode: Some(scan),
            keymod: Mod::NOMOD,
            repeat: false,
        }
    } else {
        Event::KeyUp {
            timestamp: 0,
            window_id: 7,
            keycode: None,
            scancode: Some(scan),
            keymod: Mod::NOMOD,
            repeat: false,
        }
    }
}
fn event(shared: &Rc<RefCell<Shared>>, event: Event) -> bool {
    shared
        .borrow_mut()
        .event(&event, 7, [1920, 1080], [1280, 720])
}
#[test]
fn clicks_between_frames_keep_the_pulse_and_event_position() -> Result<()> {
    let shared = Rc::new(RefCell::new(Shared::default()));
    shared.borrow_mut().set_foreground(true);
    let mut device = WindowDevice::new(keys(), shared.clone());
    shared.borrow_mut().begin_frame();
    event(
        &shared,
        Event::MouseButtonDown {
            timestamp: 0,
            window_id: 7,
            which: 0,
            mouse_btn: MouseButton::Left,
            clicks: 1,
            x: 960,
            y: 540,
        },
    );
    event(
        &shared,
        Event::MouseButtonUp {
            timestamp: 0,
            window_id: 7,
            which: 0,
            mouse_btn: MouseButton::Left,
            clicks: 1,
            x: 960,
            y: 540,
        },
    );
    shared
        .borrow_mut()
        .sample_pointer([1800, 1000], [1920, 1080], [1280, 720]);
    device.poll()?;
    assert_eq!(device.pointer()?.position, [640, 360]);
    assert_eq!(device.pointer()?.active, 1);
    shared.borrow_mut().set_mouse_focus(false);
    assert_eq!(device.pointer()?.active, 0);
    shared.borrow_mut().set_mouse_focus(true);
    assert_eq!(device.pointer()?.active, 1);
    assert_eq!(device.key(0, 0, false)?.mask, 2);
    device.commit()?;
    assert_eq!(device.key(0, 0, false)?.mask, 0);
    event(
        &shared,
        Event::MouseWheel {
            timestamp: 0,
            window_id: 7,
            which: 0,
            x: 0,
            y: i32::MIN,
            direction: sdl2::mouse::MouseWheelDirection::Flipped,
            precise_x: 0.0,
            precise_y: 0.0,
            mouse_x: 0,
            mouse_y: 0,
        },
    );
    device.poll()?;
    assert_eq!(device.take_wheel()?, 1);
    assert_eq!(device.take_wheel()?, 0);
    assert_eq!(device.key(4, 0, false)?.mask, 2);
    device.set_pointer([400, 200])?;
    assert_eq!(shared.borrow_mut().take_warp(), Some([400, 200]));
    assert_eq!(shared.borrow_mut().take_warp(), None);
    Ok(())
}
#[test]
fn keyboard_aliases_remain_held_and_focus_loss_clears_pending_input() -> Result<()> {
    let shared = Rc::new(RefCell::new(Shared::default()));
    let mut device = WindowDevice::new(keys(), shared.clone());
    shared.borrow_mut().set_foreground(true);
    event(&shared, key(Scancode::Return, true));
    device.poll()?;
    device.commit()?;
    event(&shared, key(Scancode::KpEnter, false));
    device.poll()?;
    device.commit()?;
    assert_eq!(device.key(1, 0, false)?.mask, 1);
    device.clear_keys()?;
    assert_eq!(device.key(1, 0, false)?.mask, 0);
    device.poll()?;
    assert_eq!(device.key(1, 0, false)?.mask, 1);
    event(&shared, key(Scancode::LShift, true));
    device.poll()?;
    device.commit()?;
    event(&shared, key(Scancode::RShift, false));
    device.poll()?;
    device.commit()?;
    assert_eq!(device.key(2, 0, false)?.mask, 1);
    event(&shared, key(Scancode::Escape, true));
    assert!(!event(
        &shared,
        Event::Window {
            timestamp: 0,
            window_id: 7,
            win_event: WindowEvent::FocusLost
        }
    ));
    device.poll()?;
    assert!(!device.is_foreground()?);
    assert_eq!(device.key(1, 0, false)?.mask, 0);
    assert_eq!(device.key(3, 0, false)?.mask, 0);
    event(
        &shared,
        Event::Window {
            timestamp: 0,
            window_id: 7,
            win_event: WindowEvent::FocusGained,
        },
    );
    device.poll()?;
    assert!(device.is_foreground()?);
    assert_eq!(device.key(1, 0, false)?.mask, 0);
    assert!(!event(&shared, key(Scancode::Escape, true)));
    device.poll()?;
    assert_eq!(device.key(3, 0, false)?.mask, 1);
    Ok(())
}
struct Loader;
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(None)
    }
}
struct Window {
    events: Vec<&'static str>,
    replies: VecDeque<bool>,
    fullscreen: bool,
    fail: bool,
}
impl window_actions::Services for Window {
    fn toggle_fullscreen(&mut self) -> Result<bool> {
        self.events.push("fullscreen");
        if self.fail {
            return Err(Error::new(0, "synthetic window failure"));
        }
        self.fullscreen = !self.fullscreen;
        Ok(self.fullscreen)
    }
    fn minimize(&mut self) -> Result<()> {
        self.events.push("minimize");
        Ok(())
    }
    fn confirm_close(&mut self) -> bool {
        self.events.push("confirm");
        self.replies.pop_front().unwrap()
    }
}
#[test]
fn window_actions_apply_state_after_success_reset_input_before_close_and_keep_pending_requests()
-> Result<()> {
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        InputFactory {
            fallback: KcsFactory::new(Loader),
        },
    );
    let input = rt.create(0, kind::INPUT, b"input", b"")?;
    rt.host_mut().system_settings = Some(SystemSettings::default());
    rt.host_mut().windowed_mode = Some(1);
    rt.input_state_mut(input).unwrap().pointer_mut().idle_count = 42;
    let mut window = Window {
        events: vec![],
        replies: [false, true].into(),
        fullscreen: false,
        fail: false,
    };
    rt.host_mut().window_requests = [
        WindowAction::ToggleFullscreen,
        WindowAction::OpenSystemProperties,
        WindowAction::Minimize,
        WindowAction::RebuildMenu,
        WindowAction::CloseFromMenu,
    ]
    .map(|action| WindowRequest { task_id: 0, action })
    .to_vec();
    assert!(!window_actions::pump(&mut rt, input, &mut window)?);
    assert_eq!(window.events, ["fullscreen", "minimize", "confirm"]);
    assert_eq!(rt.host().windowed_mode, Some(1));
    assert_eq!(rt.host().system_settings_dirty, 1);
    assert_eq!(rt.host().system_settings.as_ref().unwrap().fullscreen, 1);
    assert_eq!(rt.input_state(input).unwrap().pointer().idle_count, 0);
    assert_eq!(
        rt.host()
            .window_requests
            .iter()
            .map(|request| request.action)
            .collect::<Vec<_>>(),
        [
            WindowAction::OpenSystemProperties,
            WindowAction::RebuildMenu
        ]
    );
    rt.host_mut().window_requests.push(WindowRequest {
        task_id: 0,
        action: WindowAction::ToggleFullscreen,
    });
    window.fail = true;
    rt.host_mut().system_settings_dirty = 0;
    assert!(window_actions::pump(&mut rt, input, &mut window).is_err());
    assert_eq!(rt.host().windowed_mode, Some(1));
    assert_eq!(rt.host().system_settings_dirty, 0);
    assert_eq!(rt.host().system_settings.as_ref().unwrap().fullscreen, 1);
    rt.host_mut()
        .window_close_requests
        .push(WindowCloseRequest {
            task_id: 0,
            notify: false,
        });
    let before = window.events.len();
    assert!(window_actions::pump(&mut rt, input, &mut window)?);
    assert_eq!(window.events.len(), before + 1);
    rt.host_mut()
        .window_close_requests
        .push(WindowCloseRequest {
            task_id: 0,
            notify: true,
        });
    let before = window.events.len();
    assert!(window_actions::pump(&mut rt, input, &mut window)?);
    assert_eq!(window.events.len(), before);
    Ok(())
}
