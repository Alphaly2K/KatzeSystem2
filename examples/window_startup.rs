//! 以本机游戏目录打开桌面窗口，按帧驱动启动链并把键鼠输入桥接给输入任务。
//!
//! 用法：`cargo run --release --example window_startup -- <游戏目录>`。
//! macOS 默认使用 Metal；KATZE_WINDOW_RENDERER=software 选择软件绘制。
//! 关闭窗口执行退出确认，键盘输入交给游戏。

#[path = "support/audio_output.rs"]
mod audio_output;
#[path = "support/clipboard_output.rs"]
mod clipboard_output;
#[cfg(target_os = "macos")]
#[path = "support/metal_output.rs"]
mod metal_output;
mod support;
#[path = "support/system_output.rs"]
mod system_output;
#[path = "support/window_actions.rs"]
mod window_actions;
#[path = "support/window_input.rs"]
mod window_input;
#[path = "support/window_output.rs"]
mod window_output;

use katze_system2::{
    graphics::{Projection, RenderTarget, TextureFilter, Transform},
    language::LanguageTarget,
    runtime::kind,
};
use std::cell::RefCell;
use std::rc::Rc;
use support::{advance_frame, build_runtime, load_game};
use window_input::{Shared, WindowDevice};

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .ok_or("用法：window_startup <游戏目录>")?;
    let mut game = load_game(&root)?;
    let clipboard_encoding = game.router.script_encoding;
    let mut clipboard = clipboard_output::Output::default();
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
    runtime.set_system_services(system_output::Output);
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
    let mut audio = audio_output::Output::new(
        runtime
            .object_mut(sound)
            .and_then(|object| object.audio_state_mut())
            .ok_or("缺少声音状态")?,
    )?;
    println!("输入 {input} 声音 {sound}");
    let task = runtime.create(0, kind::ORDINARY, b"main", b"")?;
    println!("主任务 {task}");

    let title_source = katze_system2::resource::XmlDocument::parse(startup.as_bytes())?
        .wide_text(b"APP/title")?
        .unwrap_or_else(|| "katze-system2".encode_utf16().collect());
    let mut title = String::from_utf16_lossy(runtime.host().string_tables.resolve(&title_source));
    let mut window = window_output::Output::new(&title, [1280, 720])?;
    runtime.host_mut().windowed_mode = Some(1);
    if runtime
        .host()
        .system_settings
        .as_ref()
        .is_some_and(|settings| settings.fullscreen != 0)
    {
        window
            .window
            .set_fullscreen(sdl2::video::FullscreenType::Desktop)?;
    }
    #[cfg(target_os = "macos")]
    let accelerated = if std::env::var("KATZE_WINDOW_RENDERER").as_deref() == Ok("software") {
        false
    } else {
        runtime.set_graphics_backend(metal_output::Output::new(&window.window)?);
        true
    };
    #[cfg(not(target_os = "macos"))]
    let accelerated = false;
    let mut output = if accelerated {
        None
    } else {
        Some(RenderTarget::new([1280, 720])?)
    };
    let mut menu_pending_noted = false;
    let limit = std::env::var("KATZE_WINDOW_FRAMES")
        .ok()
        .and_then(|s| s.parse::<u32>().ok());
    let mut now = 0u32;
    let started = std::time::Instant::now();
    let mut report_started = started;
    let mut update_time = std::time::Duration::ZERO;
    let mut render_time = std::time::Duration::ZERO;
    loop {
        let frame_started = std::time::Instant::now();
        window.configure_close(&runtime.host().string_tables, &title);
        shared.borrow_mut().begin_frame();
        let events = window.poll();
        let size = window.window.size();
        let mut close = false;
        for event in events {
            close |= shared.borrow_mut().event(
                &event,
                window.window.id(),
                [size.0, size.1],
                [1280, 720],
            );
        }
        if close && window.confirm_close() {
            break;
        }
        {
            let mut shared = shared.borrow_mut();
            shared.set_foreground(window.window.has_input_focus());
            let mouse = window.events.mouse_state();
            shared.sample_pointer([mouse.x(), mouse.y()], [size.0, size.1], [1280, 720]);
            shared.set_mouse_focus(window.window.has_mouse_focus());
            window.context.mouse().show_cursor(shared.cursor());
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
            for id in 0..128 {
                if let Some(vm) = runtime.object(id).and_then(|object| object.interpreter()) {
                    eprintln!("  任务 {id} pc {}", vm.pc);
                }
                if let Some(scene) = runtime.object(id).and_then(|object| object.fes_scene()) {
                    eprintln!("  FES {id}: {:?}", scene.interpreter().pending_pc());
                }
            }
            break;
        }
        if std::env::var_os("KATZE_TRACE_TIMING").is_some()
            && frame_started.elapsed().as_millis() >= 20
        {
            eprintln!(
                "[slow-update] frame={now} ms={:.2}",
                frame_started.elapsed().as_secs_f64() * 1000.0
            );
        }
        if window_actions::pump(&mut runtime, input, &mut window)? {
            break;
        }
        if !runtime.host().window_requests.is_empty() && !menu_pending_noted {
            eprintln!("窗口菜单重建尚待接入");
            menu_pending_noted = true;
        }
        if let Some(position) = shared.borrow_mut().take_warp() {
            let size = window.window.size();
            window.context.mouse().warp_mouse_in_window(
                &window.window,
                (i64::from(position[0]) * i64::from(size.0) / 1280) as i32,
                (i64::from(position[1]) * i64::from(size.1) / 720) as i32,
            );
        }
        clipboard.pump(runtime.host_mut(), clipboard_encoding);
        audio.pump()?;
        if refresh_title {
            let next =
                String::from_utf16_lossy(runtime.host().string_tables.resolve(&title_source));
            if title != next {
                window.window.set_title(&next)?;
                title = next;
            }
        }
        update_time += frame_started.elapsed();
        let render_started = std::time::Instant::now();
        if !window.window.is_minimized() {
            if accelerated {
                runtime.present_frame([1280, 720])?;
            }
            if let Some(output) = &mut output {
                output.clear_rect(katze_system2::graphics::Rect::from_xywh(0, 0, 1280, 720));
                runtime.render_frame(
                    output,
                    &Projection::default(),
                    *Transform::default().matrix(),
                    TextureFilter::Point,
                )?;
            }
            render_time += render_started.elapsed();
            if let Some(output) = &output {
                window.present(output)?;
                runtime.capture_sequence_frame(output)?;
            }
        }
        let frame_limit = std::time::Duration::from_secs_f64(1.0 / 60.0);
        if let Some(remaining) = frame_limit.checked_sub(frame_started.elapsed()) {
            std::thread::sleep(remaining);
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
