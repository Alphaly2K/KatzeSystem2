//! 以本机游戏目录打开桌面窗口，按帧驱动启动链并把键鼠输入桥接给输入任务。
//!
//! 用法：`cargo run --example window_startup -- <游戏目录>`。
//! 关闭窗口或按 ESC 结束。

mod support;

use katze_system2::{
    Result,
    input::{ButtonSource, DeviceBinding, DeviceKeys, InputDevice, KeySample, PointerSample},
    runtime::kind,
};
use minifb::{Key, MouseButton, MouseMode, Window, WindowOptions};
use std::cell::RefCell;
use std::rc::Rc;
use support::{build_runtime, load_game};

/// minifb 键到 Windows 虚拟键码的映射；键槽绑定按 VK 低八位匹配。
const KEY_VK: &[(Key, u8)] = &[
    (Key::Backspace, 0x08),
    (Key::Tab, 0x09),
    (Key::Enter, 0x0D),
    (Key::NumPadEnter, 0x0D),
    (Key::Pause, 0x13),
    (Key::CapsLock, 0x14),
    (Key::Escape, 0x1B),
    (Key::Space, 0x20),
    (Key::PageUp, 0x21),
    (Key::PageDown, 0x22),
    (Key::End, 0x23),
    (Key::Home, 0x24),
    (Key::Left, 0x25),
    (Key::Up, 0x26),
    (Key::Right, 0x27),
    (Key::Down, 0x28),
    (Key::Insert, 0x2D),
    (Key::Delete, 0x2E),
    (Key::Key0, 0x30),
    (Key::Key1, 0x31),
    (Key::Key2, 0x32),
    (Key::Key3, 0x33),
    (Key::Key4, 0x34),
    (Key::Key5, 0x35),
    (Key::Key6, 0x36),
    (Key::Key7, 0x37),
    (Key::Key8, 0x38),
    (Key::Key9, 0x39),
    (Key::A, 0x41),
    (Key::B, 0x42),
    (Key::C, 0x43),
    (Key::D, 0x44),
    (Key::E, 0x45),
    (Key::F, 0x46),
    (Key::G, 0x47),
    (Key::H, 0x48),
    (Key::I, 0x49),
    (Key::J, 0x4A),
    (Key::K, 0x4B),
    (Key::L, 0x4C),
    (Key::M, 0x4D),
    (Key::N, 0x4E),
    (Key::O, 0x4F),
    (Key::P, 0x50),
    (Key::Q, 0x51),
    (Key::R, 0x52),
    (Key::S, 0x53),
    (Key::T, 0x54),
    (Key::U, 0x55),
    (Key::V, 0x56),
    (Key::W, 0x57),
    (Key::X, 0x58),
    (Key::Y, 0x59),
    (Key::Z, 0x5A),
    (Key::LeftSuper, 0x5B),
    (Key::RightSuper, 0x5C),
    (Key::NumPad0, 0x60),
    (Key::NumPad1, 0x61),
    (Key::NumPad2, 0x62),
    (Key::NumPad3, 0x63),
    (Key::NumPad4, 0x64),
    (Key::NumPad5, 0x65),
    (Key::NumPad6, 0x66),
    (Key::NumPad7, 0x67),
    (Key::NumPad8, 0x68),
    (Key::NumPad9, 0x69),
    (Key::NumPadAsterisk, 0x6A),
    (Key::NumPadPlus, 0x6B),
    (Key::NumPadMinus, 0x6D),
    (Key::NumPadDot, 0x6E),
    (Key::NumPadSlash, 0x6F),
    (Key::F1, 0x70),
    (Key::F2, 0x71),
    (Key::F3, 0x72),
    (Key::F4, 0x73),
    (Key::F5, 0x74),
    (Key::F6, 0x75),
    (Key::F7, 0x76),
    (Key::F8, 0x77),
    (Key::F9, 0x78),
    (Key::F10, 0x79),
    (Key::F11, 0x7A),
    (Key::F12, 0x7B),
    (Key::NumLock, 0x90),
    (Key::ScrollLock, 0x91),
    (Key::LeftShift, 0xA0),
    (Key::RightShift, 0xA1),
    (Key::LeftCtrl, 0xA2),
    (Key::RightCtrl, 0xA3),
    (Key::LeftAlt, 0xA4),
    (Key::RightAlt, 0xA5),
    (Key::Semicolon, 0xBA),
    (Key::Equal, 0xBB),
    (Key::Comma, 0xBC),
    (Key::Minus, 0xBD),
    (Key::Period, 0xBE),
    (Key::Slash, 0xBF),
    (Key::Backquote, 0xC0),
    (Key::LeftBracket, 0xDB),
    (Key::Backslash, 0xDC),
    (Key::RightBracket, 0xDD),
    (Key::Apostrophe, 0xDE),
];

/// 主循环每帧从窗口写入的设备快照；按钮事件排队到 poll 时统一应用。
struct Shared {
    position: [i32; 2],
    active: u32,
    wheel: u32,
    foreground: bool,
    cursor: u32,
    forced: Option<[i32; 2]>,
    forced_origin: Option<[i32; 2]>,
    events: Vec<(ButtonSource, u32, u32)>,
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
        }
    }
}

struct WindowDevice {
    shared: Rc<RefCell<Shared>>,
    keys: DeviceKeys,
}

impl WindowDevice {
    fn new(config: &[u8], shared: Rc<RefCell<Shared>>) -> Result<Self> {
        let mut keys = DeviceKeys::from_xml(config).unwrap_or_default();
        // 启动配置未提供 KEYCUSTOMIZE 时登记鼠标左键为槽零组零。
        if keys.bindings().iter().all(|binding| binding.enabled == 0) {
            let mut binding = DeviceBinding {
                enabled: 1,
                ..Default::default()
            };
            binding.codes[ButtonSource::Mouse as usize][0] = 0;
            keys.set_binding(0, binding)?;
        }
        Ok(Self { shared, keys })
    }
}

impl InputDevice for WindowDevice {
    fn poll(&mut self) -> Result<()> {
        let mut shared = self.shared.borrow_mut();
        for (source, code, pressed) in shared.events.drain(..) {
            self.keys.button(source, code, pressed);
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
        shared.forced_origin = Some(shared.position);
        Ok(())
    }
    fn clear_keys(&mut self) -> Result<()> {
        self.keys.clear_pressed();
        Ok(())
    }
    fn clear_touches(&mut self) -> Result<()> {
        Ok(())
    }
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .ok_or("用法：window_startup <游戏目录>")?;
    let mut game = load_game(&root)?;
    let startup = game.startup.clone();
    let programs = std::mem::take(&mut game.programs);
    let mut runtime = build_runtime(
        support::Game {
            root: game.root,
            router: game.router,
            startup: game.startup,
            start: game.start,
            programs,
        },
        [1280, 720],
    )?;
    let shared = Rc::new(RefCell::new(Shared::default()));
    runtime.set_input_device(WindowDevice::new(startup.as_bytes(), shared.clone())?)?;
    runtime.input_coordinates_mut().logical_size = [1280, 720];
    let input = runtime.create(0, kind::INPUT, b"input", b"")?;
    let sound = runtime.create(0, kind::SOUND, b"sound", b"")?;
    println!("输入 {input} 声音 {sound}");
    let task = runtime.create(0, kind::ORDINARY, b"main", b"")?;
    println!("主任务 {task}");

    let mut window = Window::new("katze-system2", 1280, 720, WindowOptions::default())?;
    window.set_target_fps(60);
    let mut now = 0;
    while window.is_open() && !window.is_key_down(Key::Escape) {
        {
            let mut shared = shared.borrow_mut();
            let (x, y) = window
                .get_mouse_pos(MouseMode::Clamp)
                .unwrap_or((640., 360.));
            shared.position = [x as i32, y as i32];
            shared.active = u32::from(window.get_mouse_down(MouseButton::Left));
            for (button, code) in [
                (MouseButton::Left, 0),
                (MouseButton::Right, 1),
                (MouseButton::Middle, 2),
            ] {
                shared.events.push((
                    ButtonSource::Mouse,
                    code,
                    u32::from(window.get_mouse_down(button)),
                ));
            }
            for &(key, vk) in KEY_VK {
                shared.events.push((
                    ButtonSource::Keyboard,
                    u32::from(vk),
                    u32::from(window.is_key_down(key)),
                ));
            }
            if let Some((x, y)) = window.get_scroll_wheel() {
                if x != 0. || y != 0. {
                    shared.wheel = shared.wheel.wrapping_add(1);
                }
            }
            shared.foreground = window.is_active();
            window.set_cursor_visibility(shared.cursor != 0);
        }
        now += 16;
        if let Err(error) = runtime.update(now, 16) {
            eprintln!("pc {}：{}", error.offset, error.message);
            for id in 0..64 {
                if let Some(vm) = runtime.object(id).and_then(|object| object.interpreter()) {
                    eprintln!("  任务 {id} pc {}", vm.pc);
                }
            }
            break;
        }
        let target = runtime.drawing().target.as_ref().ok_or("缺少渲染目标")?;
        let [width, height] = target.size();
        let mut buffer = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                buffer.push(target.pixel(x, y).unwrap_or(0) & 0x00ff_ffff);
            }
        }
        window.update_with_buffer(&buffer, width as usize, height as usize)?;
    }
    Ok(())
}
