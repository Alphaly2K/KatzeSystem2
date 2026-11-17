use katze_system2::{
    Result,
    input::{ButtonSource, DeviceKeys, InputDevice, KeySample, PointerSample, WheelButtons},
};
use sdl2::{
    event::{Event, WindowEvent},
    keyboard::Scancode,
    mouse::MouseButton,
};
use std::{cell::RefCell, rc::Rc};

const KEY_VK: &[(Scancode, u8)] = &[
    (Scancode::Backspace, 0x08),
    (Scancode::Tab, 0x09),
    (Scancode::Return, 0x0D),
    (Scancode::KpEnter, 0x0D),
    (Scancode::Pause, 0x13),
    (Scancode::CapsLock, 0x14),
    (Scancode::Escape, 0x1B),
    (Scancode::Space, 0x20),
    (Scancode::PageUp, 0x21),
    (Scancode::PageDown, 0x22),
    (Scancode::End, 0x23),
    (Scancode::Home, 0x24),
    (Scancode::Left, 0x25),
    (Scancode::Up, 0x26),
    (Scancode::Right, 0x27),
    (Scancode::Down, 0x28),
    (Scancode::Insert, 0x2D),
    (Scancode::Delete, 0x2E),
    (Scancode::Num0, 0x30),
    (Scancode::Num1, 0x31),
    (Scancode::Num2, 0x32),
    (Scancode::Num3, 0x33),
    (Scancode::Num4, 0x34),
    (Scancode::Num5, 0x35),
    (Scancode::Num6, 0x36),
    (Scancode::Num7, 0x37),
    (Scancode::Num8, 0x38),
    (Scancode::Num9, 0x39),
    (Scancode::A, 0x41),
    (Scancode::B, 0x42),
    (Scancode::C, 0x43),
    (Scancode::D, 0x44),
    (Scancode::E, 0x45),
    (Scancode::F, 0x46),
    (Scancode::G, 0x47),
    (Scancode::H, 0x48),
    (Scancode::I, 0x49),
    (Scancode::J, 0x4A),
    (Scancode::K, 0x4B),
    (Scancode::L, 0x4C),
    (Scancode::M, 0x4D),
    (Scancode::N, 0x4E),
    (Scancode::O, 0x4F),
    (Scancode::P, 0x50),
    (Scancode::Q, 0x51),
    (Scancode::R, 0x52),
    (Scancode::S, 0x53),
    (Scancode::T, 0x54),
    (Scancode::U, 0x55),
    (Scancode::V, 0x56),
    (Scancode::W, 0x57),
    (Scancode::X, 0x58),
    (Scancode::Y, 0x59),
    (Scancode::Z, 0x5A),
    (Scancode::LGui, 0x5B),
    (Scancode::RGui, 0x5C),
    (Scancode::Kp0, 0x60),
    (Scancode::Kp1, 0x61),
    (Scancode::Kp2, 0x62),
    (Scancode::Kp3, 0x63),
    (Scancode::Kp4, 0x64),
    (Scancode::Kp5, 0x65),
    (Scancode::Kp6, 0x66),
    (Scancode::Kp7, 0x67),
    (Scancode::Kp8, 0x68),
    (Scancode::Kp9, 0x69),
    (Scancode::KpMultiply, 0x6A),
    (Scancode::KpPlus, 0x6B),
    (Scancode::KpMinus, 0x6D),
    (Scancode::KpPeriod, 0x6E),
    (Scancode::KpDivide, 0x6F),
    (Scancode::F1, 0x70),
    (Scancode::F2, 0x71),
    (Scancode::F3, 0x72),
    (Scancode::F4, 0x73),
    (Scancode::F5, 0x74),
    (Scancode::F6, 0x75),
    (Scancode::F7, 0x76),
    (Scancode::F8, 0x77),
    (Scancode::F9, 0x78),
    (Scancode::F10, 0x79),
    (Scancode::F11, 0x7A),
    (Scancode::F12, 0x7B),
    (Scancode::NumLockClear, 0x90),
    (Scancode::ScrollLock, 0x91),
    (Scancode::LShift, 0xA0),
    (Scancode::RShift, 0xA1),
    (Scancode::LCtrl, 0xA2),
    (Scancode::RCtrl, 0xA3),
    (Scancode::LAlt, 0xA4),
    (Scancode::RAlt, 0xA5),
    (Scancode::LShift, 0x10),
    (Scancode::RShift, 0x10),
    (Scancode::LCtrl, 0x11),
    (Scancode::RCtrl, 0x11),
    (Scancode::LAlt, 0x12),
    (Scancode::RAlt, 0x12),
    (Scancode::Semicolon, 0xBA),
    (Scancode::Equals, 0xBB),
    (Scancode::Comma, 0xBC),
    (Scancode::Minus, 0xBD),
    (Scancode::Period, 0xBE),
    (Scancode::Slash, 0xBF),
    (Scancode::Grave, 0xC0),
    (Scancode::LeftBracket, 0xDB),
    (Scancode::Backslash, 0xDC),
    (Scancode::RightBracket, 0xDD),
    (Scancode::Apostrophe, 0xDE),
];

/// 主循环每帧从窗口写入的设备快照；按钮事件排队到 poll 时统一应用。
pub struct Shared {
    position: [i32; 2],
    active: u32,
    wheel: u32,
    foreground: bool,
    cursor: u32,
    forced: Option<[i32; 2]>,
    forced_origin: Option<[i32; 2]>,
    events: Vec<(ButtonSource, u32, u32)>,
    down: std::collections::BTreeSet<i32>,
    mouse_down: std::collections::BTreeSet<u32>,
    clear: bool,
    pointer_event: bool,
    pointer_inside: bool,
    mouse_focus: bool,
    warp: Option<[i32; 2]>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            position: [0; 2],
            active: 0,
            wheel: 0,
            foreground: false,
            cursor: 1,
            forced: None,
            forced_origin: None,
            events: Vec::new(),
            down: Default::default(),
            mouse_down: Default::default(),
            clear: false,
            pointer_event: false,
            pointer_inside: false,
            mouse_focus: true,
            warp: None,
        }
    }
}

pub struct WindowDevice {
    shared: Rc<RefCell<Shared>>,
    keys: DeviceKeys,
    wheel: WheelButtons,
}

impl WindowDevice {
    pub fn new(keys: DeviceKeys, shared: Rc<RefCell<Shared>>) -> Self {
        Self {
            shared,
            keys,
            wheel: WheelButtons::default(),
        }
    }
}

impl InputDevice for WindowDevice {
    fn key_configuration(&self) -> Option<&DeviceKeys> {
        Some(&self.keys)
    }
    fn poll(&mut self) -> Result<()> {
        let mut shared = self.shared.borrow_mut();
        if std::mem::take(&mut shared.clear) {
            self.keys.clear_pressed();
            self.wheel.cancel_repeat();
        }
        for (source, code, pressed) in shared.events.drain(..) {
            if source == ButtonSource::Mouse && matches!(code, 6 | 7) && pressed != 0 {
                self.wheel.scroll(code == 6, |code, pressed| {
                    self.keys.button(ButtonSource::Mouse, code, pressed);
                });
            } else {
                self.keys.button(source, code, pressed);
            }
        }
        for &(scan, code) in KEY_VK {
            if shared.down.contains(&(scan as i32)) {
                self.keys.button(ButtonSource::Keyboard, u32::from(code), 1);
            }
        }
        for &code in &shared.mouse_down {
            self.keys.button(ButtonSource::Mouse, code, 1);
        }
        if shared
            .forced_origin
            .is_some_and(|origin| origin != shared.position)
        {
            shared.forced = None;
            shared.forced_origin = None;
        }
        Ok(())
    }
    fn pointer(&mut self) -> Result<PointerSample> {
        let shared = self.shared.borrow();
        Ok(PointerSample {
            position: shared.forced.unwrap_or(shared.position),
            active: shared.active,
        })
    }
    fn key(&mut self, key: u32, group: u32, repeat: bool) -> Result<KeySample> {
        self.keys.sample(key, group, repeat)
    }
    fn take_wheel(&mut self) -> Result<u32> {
        let mut shared = self.shared.borrow_mut();
        Ok(std::mem::take(&mut shared.wheel))
    }
    fn is_foreground(&mut self) -> Result<bool> {
        Ok(self.shared.borrow().foreground)
    }
    fn finish(&mut self) -> Result<()> {
        self.wheel.finish(|code, pressed| {
            self.keys.button(ButtonSource::Mouse, code, pressed);
        });
        Ok(())
    }
    fn commit(&mut self) -> Result<()> {
        self.keys.commit();
        Ok(())
    }
    fn history(&mut self) -> Result<[u8; 32]> {
        Ok([0; 32])
    }
    fn show_cursor(&mut self, visible: u32) -> Result<()> {
        self.shared.borrow_mut().cursor = visible;
        Ok(())
    }
    fn cursor_visible(&mut self) -> Result<u32> {
        Ok(self.shared.borrow().cursor)
    }
    fn set_pointer(&mut self, position: [i32; 2]) -> Result<()> {
        let mut shared = self.shared.borrow_mut();
        shared.forced = Some(position);
        shared.warp = Some(position);
        shared.forced_origin = Some(shared.position);
        Ok(())
    }
    fn clear_keys(&mut self) -> Result<()> {
        self.keys.clear_pressed();
        Ok(())
    }
    fn cancel_wheel_repeat(&mut self) -> Result<()> {
        self.wheel.cancel_repeat();
        Ok(())
    }
    fn clear_touches(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Shared {
    pub fn begin_frame(&mut self) {
        self.pointer_event = false;
    }
    pub fn sample_pointer(&mut self, position: [i32; 2], size: [u32; 2], logical: [u32; 2]) {
        if !self.pointer_event {
            self.move_pointer(position, size, logical);
        }
    }
    pub fn take_warp(&mut self) -> Option<[i32; 2]> {
        self.warp.take()
    }

    pub fn event(&mut self, event: &Event, window: u32, size: [u32; 2], logical: [u32; 2]) -> bool {
        match *event {
            Event::Quit { .. } => return true,
            Event::Window {
                window_id,
                win_event: WindowEvent::Close,
                ..
            } if window_id == window => return true,
            Event::Window {
                window_id,
                win_event: WindowEvent::FocusLost,
                ..
            } if window_id == window => {
                self.foreground = false;
                self.active = 0;
                self.down.clear();
                self.mouse_down.clear();
                self.events.clear();
                self.wheel = 0;
                self.clear = true;
            }
            Event::Window {
                window_id,
                win_event: WindowEvent::FocusGained,
                ..
            } if window_id == window => self.set_foreground(true),
            Event::Window {
                window_id,
                win_event: WindowEvent::Leave,
                ..
            } if window_id == window => self.set_mouse_focus(false),
            Event::Window {
                window_id,
                win_event: WindowEvent::Enter,
                ..
            } if window_id == window => self.set_mouse_focus(true),
            Event::MouseMotion {
                window_id, x, y, ..
            } if window_id == window => {
                self.pointer_event = true;
                self.move_pointer([x, y], size, logical);
            }
            Event::MouseButtonDown {
                window_id,
                mouse_btn,
                x,
                y,
                ..
            }
            | Event::MouseButtonUp {
                window_id,
                mouse_btn,
                x,
                y,
                ..
            } if window_id == window => {
                self.pointer_event = true;
                self.move_pointer([x, y], size, logical);
                let code = match mouse_btn {
                    MouseButton::Left => 1,
                    MouseButton::Middle => 2,
                    MouseButton::Right => 3,
                    MouseButton::X1 => 4,
                    MouseButton::X2 => 5,
                    _ => return false,
                };
                if matches!(*event, Event::MouseButtonDown { .. }) {
                    self.mouse_down.insert(code);
                } else {
                    self.mouse_down.remove(&code);
                }
                self.events.push((
                    ButtonSource::Mouse,
                    code,
                    u32::from(matches!(*event, Event::MouseButtonDown { .. })),
                ));
            }
            Event::MouseWheel {
                window_id,
                x,
                y,
                direction,
                ..
            } if window_id == window => {
                if x != 0 || y != 0 {
                    self.wheel = self.wheel.wrapping_add(1);
                }
                if y != 0 {
                    let up = (y > 0) != (direction == sdl2::mouse::MouseWheelDirection::Flipped);
                    self.events
                        .push((ButtonSource::Mouse, if up { 6 } else { 7 }, 1));
                }
            }
            Event::KeyDown {
                window_id,
                scancode: Some(key),
                ..
            }
            | Event::KeyUp {
                window_id,
                scancode: Some(key),
                ..
            } if window_id == window => {
                let pressed = matches!(*event, Event::KeyDown { .. });
                if pressed {
                    self.down.insert(key as i32);
                } else {
                    self.down.remove(&(key as i32));
                }
                for (_, vk) in KEY_VK.iter().filter(|(scan, _)| *scan == key) {
                    let held = KEY_VK
                        .iter()
                        .any(|(scan, value)| value == vk && self.down.contains(&(*scan as i32)));
                    self.events
                        .push((ButtonSource::Keyboard, u32::from(*vk), u32::from(held)));
                }
            }
            _ => {}
        }
        false
    }
    pub fn move_pointer(&mut self, position: [i32; 2], size: [u32; 2], logical: [u32; 2]) {
        if size.contains(&0) {
            self.pointer_inside = false;
            self.active = 0;
            return;
        }
        self.position = std::array::from_fn(|i| {
            ((i64::from(position[i]) * i64::from(logical[i])) / i64::from(size[i])) as i32
        });
        self.pointer_inside = (0..size[0] as i32).contains(&position[0])
            && (0..size[1] as i32).contains(&position[1]);
        self.update_active();
    }
    pub fn set_foreground(&mut self, value: bool) {
        if !value && self.foreground {
            self.down.clear();
            self.mouse_down.clear();
            self.events.clear();
            self.wheel = 0;
            self.clear = true;
            self.active = 0;
        }
        self.foreground = value;
        self.update_active();
    }
    pub fn set_mouse_focus(&mut self, value: bool) {
        self.mouse_focus = value;
        self.update_active();
    }
    fn update_active(&mut self) {
        self.active = u32::from(self.foreground && self.mouse_focus && self.pointer_inside);
    }
    pub fn cursor(&self, game_cursor: bool) -> bool {
        self.cursor != 0 && !(game_cursor && self.active != 0)
    }
}
