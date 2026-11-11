//! 以本机游戏目录打开桌面窗口，按帧驱动启动链并把键鼠输入桥接给输入任务。
//!
//! 用法：`cargo run --release --example window_startup -- <游戏目录>`。
//! macOS 默认使用 Metal；KATZE_WINDOW_RENDERER=software 选择软件绘制。
//! 关闭窗口或按 ESC 结束。

mod support;

use katze_system2::{
    Result,
    graphics::{Projection, RenderTarget, TextureFilter, Transform},
    input::{ButtonSource, DeviceKeys, InputDevice, KeySample, PointerSample, WheelButtons},
    language::LanguageTarget,
    runtime::kind,
};
use minifb::{Key, MouseButton, MouseMode, Window, WindowOptions};
use std::cell::RefCell;
use std::rc::Rc;
use support::{advance_frame, build_runtime, load_game};

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn attach_metal(
    window: &Window,
    renderer: &katze_system2::graphics::metal::Renderer,
) -> Result<()> {
    use metal::{
        foreign_types::ForeignTypeRef,
        objc::{msg_send, runtime::Object, sel, sel_impl},
    };
    // minifb 的 macOS 句柄是 NSWindow，最后一个内容子视图是其 MTKView。
    unsafe {
        let native = window.get_window_handle().cast::<Object>();
        let content: *mut Object = msg_send![native, contentView];
        let views: *mut Object = msg_send![content, subviews];
        let view: *mut Object = msg_send![views, lastObject];
        if view.is_null() {
            return Err(katze_system2::Error::new(0, "窗口缺少 Metal 视图"));
        }
        let _: () = msg_send![view, setDelegate: std::ptr::null_mut::<Object>()];
        let _: () = msg_send![view, setWantsLayer: true];
        let _: () = msg_send![view, setLayer: renderer.layer().as_ptr()];
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn mouse_event_position(window: &Window) -> Option<(f32, f32)> {
    use metal::objc::{class, msg_send, runtime::Object, sel, sel_impl};
    #[repr(C)]
    struct Point {
        x: f64,
        y: f64,
    }
    // 点击与全局指针轮询可能来自不同采样时刻，优先使用当前鼠标事件的位置。
    unsafe {
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let event: *mut Object = msg_send![app, currentEvent];
        if event.is_null() {
            return None;
        }
        let event_window: *mut Object = msg_send![event, window];
        let event_type: u64 = msg_send![event, type];
        if event_window != window.get_window_handle().cast::<Object>()
            || !matches!(event_type, 1..=7 | 22 | 25..=27)
        {
            return None;
        }
        let point: Point = msg_send![event, locationInWindow];
        Some((point.x as f32, window.get_size().1 as f32 - point.y as f32))
    }
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn window_active(window: &Window) -> bool {
    use metal::objc::{msg_send, runtime::Object, sel, sel_impl};
    // 当前 minifb macOS 包装反转了活动标记，直接查询原生键窗口。
    unsafe { msg_send![window.get_window_handle().cast::<Object>(), isKeyWindow] }
}

#[cfg(not(target_os = "macos"))]
fn window_active(window: &Window) -> bool {
    window.is_active()
}

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
    wheel: WheelButtons,
}

impl WindowDevice {
    fn new(keys: DeviceKeys, shared: Rc<RefCell<Shared>>) -> Self {
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
        for (source, code, pressed) in shared.events.drain(..) {
            if source == ButtonSource::Mouse && matches!(code, 6 | 7) && pressed != 0 {
                self.wheel.scroll(code == 6, |code, pressed| {
                    self.keys.button(ButtonSource::Mouse, code, pressed);
                });
            } else {
                self.keys.button(source, code, pressed);
            }
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
    let keys = runtime
        .host_mut()
        .configuration_keys
        .take()
        .ok_or("缺少启动按键配置")?;
    runtime.set_input_device(WindowDevice::new(keys, shared.clone()))?;
    runtime.input_coordinates_mut().logical_size = [1280, 720];
    let input = runtime.create(0, kind::INPUT, b"input", b"")?;
    let sound = runtime.create(0, kind::SOUND, b"sound", b"")?;
    println!("输入 {input} 声音 {sound}");
    let task = runtime.create(0, kind::ORDINARY, b"main", b"")?;
    println!("主任务 {task}");

    let title_source = katze_system2::resource::XmlDocument::parse(startup.as_bytes())?
        .wide_text(b"APP/title")?
        .unwrap_or_else(|| "katze-system2".encode_utf16().collect());
    let mut title = String::from_utf16_lossy(runtime.host().string_tables.resolve(&title_source));
    let mut window = Window::new(&title, 1280, 720, WindowOptions::default())?;
    runtime.host_mut().windowed_mode = Some(1);
    window.set_target_fps(60);
    #[cfg(target_os = "macos")]
    let mut gpu = if std::env::var("KATZE_WINDOW_RENDERER").as_deref() == Ok("software") {
        None
    } else {
        let renderer =
            metal::objc::rc::autoreleasepool(katze_system2::graphics::metal::Renderer::new)?;
        attach_metal(&window, &renderer)?;
        println!("绘制设备：Metal / {}", renderer.device_name());
        Some(renderer)
    };
    #[cfg(target_os = "macos")]
    let accelerated = gpu.is_some();
    #[cfg(not(target_os = "macos"))]
    let accelerated = false;
    let mut output = if accelerated {
        None
    } else {
        Some(RenderTarget::new([1280, 720])?)
    };
    let mut buffer = if accelerated {
        Vec::new()
    } else {
        vec![0; 1280 * 720]
    };
    let limit = std::env::var("KATZE_WINDOW_FRAMES")
        .ok()
        .and_then(|s| s.parse::<u32>().ok());
    let mut now = 0u32;
    let started = std::time::Instant::now();
    let mut report_started = started;
    let mut update_time = std::time::Duration::ZERO;
    let mut render_time = std::time::Duration::ZERO;
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let frame_started = std::time::Instant::now();
        {
            let mut shared = shared.borrow_mut();
            let pointer = window.get_mouse_pos(MouseMode::Discard);
            #[cfg(target_os = "macos")]
            let pointer = mouse_event_position(&window)
                .or(pointer)
                .filter(|&(x, y)| (0.0..1280.).contains(&x) && (0.0..720.).contains(&y));
            if let Some((x, y)) = pointer {
                shared.position = [x as i32, y as i32];
            }
            shared.active = u32::from(pointer.is_some() && window_active(&window));
            for (button, code) in [
                (MouseButton::Left, 1),
                (MouseButton::Right, 3),
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
                if y != 0. {
                    shared
                        .events
                        .push((ButtonSource::Mouse, if y > 0. { 6 } else { 7 }, 1));
                }
            }
            shared.foreground = window_active(&window);
            window.set_cursor_visibility(shared.cursor != 0);
        }
        now = now.wrapping_add(1);
        let refresh_title = runtime
            .host()
            .language_change_requests
            .iter()
            .any(|request| request.target == LanguageTarget::Interface)
            && runtime
                .host()
                .string_table_config
                .as_ref()
                .is_some_and(|config| {
                    (runtime.host().languages.primary_index as usize) < config.prefixes.len()
                });
        if let Err(error) = advance_frame(&mut runtime, now, started.elapsed().as_millis() as u32) {
            eprintln!("pc {}：{}", error.offset, error.message);
            for id in 0..64 {
                if let Some(vm) = runtime.object(id).and_then(|object| object.interpreter()) {
                    eprintln!("  任务 {id} pc {}", vm.pc);
                }
            }
            break;
        }
        if !runtime.host_mut().window_close_requests.is_empty() {
            break;
        }
        if refresh_title {
            let next =
                String::from_utf16_lossy(runtime.host().string_tables.resolve(&title_source));
            if title != next {
                window.set_title(&next);
                title = next;
            }
        }
        update_time += frame_started.elapsed();
        let render_started = std::time::Instant::now();
        #[cfg(target_os = "macos")]
        if let Some(renderer) = &mut gpu {
            let triangles = runtime
                .drawables_mut()
                .frame_triangles(&Projection::default(), *Transform::default().matrix())?;
            let canvas = runtime
                .drawing_mut()
                .target
                .as_mut()
                .ok_or("运行时缺少画布")?;
            metal::objc::rc::autoreleasepool(|| {
                renderer.present(&triangles, canvas, [1280, 720], TextureFilter::Point)
            })?;
        }
        if let Some(output) = &mut output {
            output.clear_rect(katze_system2::graphics::Rect::from_xywh(0, 0, 1280, 720));
            runtime.render_frame(
                output,
                &Projection::default(),
                *Transform::default().matrix(),
                TextureFilter::Point,
            )?;
            let [width, height] = output.size();
            for y in 0..height {
                for x in 0..width {
                    buffer[(y * width + x) as usize] =
                        output.pixel(x, y).unwrap_or(0) & 0x00ff_ffff;
                }
            }
        }
        render_time += render_started.elapsed();
        if accelerated {
            window.update();
        } else {
            window.update_with_buffer(&buffer, 1280, 720)?;
        }
        if now.is_multiple_of(120) {
            println!(
                "帧 {now}：{:.1} FPS，更新 {:.2} ms，绘制 {:.2} ms",
                120. / report_started.elapsed().as_secs_f64(),
                update_time.as_secs_f64() * 1000. / 120.,
                render_time.as_secs_f64() * 1000. / 120.
            );
            update_time = std::time::Duration::ZERO;
            render_time = std::time::Duration::ZERO;
            report_started = std::time::Instant::now();
        }
        if limit.is_some_and(|limit| now >= limit) {
            break;
        }
    }
    Ok(())
}
