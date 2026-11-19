//! 以本机游戏目录打开桌面窗口，按帧驱动启动链并把键鼠输入桥接给输入任务。
//!
//! 用法：`cargo run --release -p katze-desktop --bin window_startup -- <游戏目录>`。
//! macOS 默认使用 Metal；KATZE_WINDOW_RENDERER=software 选择软件绘制。
//! KATZE_EXPORT_ROOT 可指定接收游戏导出文件的现有目录。
//! KATZE_IMPORT_FILE 可预选游戏图片导入任务使用的文件。
//! 关闭窗口执行退出确认，键盘输入交给游戏。

#[path = "support/audio_output.rs"]
mod audio_output;
#[path = "support/clipboard_output.rs"]
mod clipboard_output;
#[path = "support/export_output.rs"]
mod export_output;
#[cfg(target_os = "macos")]
#[path = "support/metal_output.rs"]
mod metal_output;
#[path = "../../support/mod.rs"]
mod support;
#[path = "support/system_output.rs"]
mod system_output;
#[path = "support/window_import.rs"]
mod window_import;
#[path = "support/window_input.rs"]
mod window_input;
#[path = "support/window_output.rs"]
mod window_output;
#[path = "support/window_quiz.rs"]
mod window_quiz;
#[path = "support/window_text.rs"]
mod window_text;

use katze_system2::{
    host::{Rect, RenderTarget},
    session::{FrameOutcome, FrameTime},
};
use sdl2::event::{Event, WindowEvent};
use std::cell::RefCell;
use std::rc::Rc;
use support::image_import::{Import, ImportPicker, import_directory};
use support::{build_runtime, load_game};
use window_input::{Shared, WindowDevice};

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .ok_or("用法：window_startup <游戏目录>")?;
    let game = load_game(&root)?;
    let clipboard_encoding = game.router.script_encoding;
    let diagnostics = support::diagnostics::Output::from_env(clipboard_encoding);
    let mut clipboard = clipboard_output::Output::default();
    let startup = game.startup.clone();
    let entry = game.start.clone();
    let (mut runtime, keys, surface) = build_runtime(game)?;
    runtime.set_diagnostic_trace(diagnostics.trace());
    if let Some(trace) = diagnostics.native_trace() {
        runtime.set_native_trace(trace);
    }
    runtime.set_system_services(system_output::Output);
    if let Some(path) = std::env::var_os("KATZE_EXPORT_ROOT") {
        runtime.set_export_services(export_output::Directory::new(path.into())?);
    }
    let shared = Rc::new(RefCell::new(Shared::default()));
    runtime.set_input_device(WindowDevice::new(keys, shared.clone()))?;
    let mut runtime = runtime.start_game(entry.as_bytes())?;
    let mut audio = audio_output::Output::new(runtime.connect_audio_output()?)?;

    let startup_xml = katze_system2::resource::XmlDocument::parse(startup.as_bytes())?;
    let mut image_import = if std::env::var_os("KATZE_IMPORT_FILE").is_none() {
        match import_directory(&root, &startup_xml) {
            Ok(Some(directory)) => {
                let picker = ImportPicker::default();
                runtime.set_platform_job_services(Import::interactive(
                    directory.clone(),
                    picker.clone(),
                ));
                Some(window_import::WindowImport::new(directory, picker))
            }
            Ok(None) => None,
            Err(error) => {
                eprintln!("图片导入目录：{error}");
                None
            }
        }
    } else {
        None
    };
    let title_source = startup_xml
        .wide_text(b"APP/title")?
        .unwrap_or_else(|| "katze-system2".encode_utf16().collect());
    let mut title = String::from_utf16_lossy(runtime.string_tables().resolve(&title_source));
    let mut window = window_output::Output::new(&title, surface)?;
    window.window.subsystem().text_input().stop();
    let mut text_window = window_text::TextWindow::default();
    let mut quiz_windows = window_quiz::QuizWindows::default();
    runtime.set_windowed_mode(Some(1));
    if runtime
        .system_settings()
        .is_some_and(|settings| settings.fullscreen != 0)
    {
        window
            .window
            .set_fullscreen(sdl2::video::FullscreenType::Desktop)?;
    }
    #[cfg(target_os = "macos")]
    let metal_output = if std::env::var("KATZE_WINDOW_RENDERER").as_deref() == Ok("software") {
        None
    } else {
        let output = metal_output::Output::new(&window.window)?;
        runtime.set_graphics_backend(output.capture_backend());
        Some(output)
    };
    #[cfg(target_os = "macos")]
    let accelerated = metal_output.is_some();
    #[cfg(not(target_os = "macos"))]
    let accelerated = false;
    let mut output = if accelerated {
        None
    } else {
        Some(RenderTarget::new(surface)?)
    };
    let mut window_pending_noted = false;
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
        window.configure_close(runtime.string_tables(), &title);
        shared.borrow_mut().begin_frame();
        let events = window.poll();
        let size = window.window.size();
        let importing = image_import
            .as_ref()
            .is_some_and(window_import::WindowImport::active);
        let editing = text_window.active() || importing;
        let mut close = false;
        for event in events {
            if quiz_windows.event(&event, &mut runtime)? {
                continue;
            }
            if importing {
                image_import.as_mut().unwrap().event(&event);
            } else if editing {
                text_window.event(&event, |request, text| {
                    runtime.complete_text_entry(request, text)
                })?;
            }
            if !editing
                || matches!(
                    event,
                    Event::Quit { .. }
                        | Event::Window {
                            win_event: WindowEvent::Close,
                            ..
                        }
                )
            {
                close |= shared.borrow_mut().event(
                    &event,
                    window.window.id(),
                    [size.0, size.1],
                    surface,
                );
            }
        }
        if close && window.confirm_close() {
            break;
        }
        {
            let mut shared = shared.borrow_mut();
            shared.set_foreground(!editing && window.window.has_input_focus());
            let mouse = window.events.mouse_state();
            shared.sample_pointer([mouse.x(), mouse.y()], [size.0, size.1], surface);
            shared.set_mouse_focus(window.window.has_mouse_focus());
        }
        now = now.wrapping_add(1);
        let advancement = runtime.advance(FrameTime {
            tick: now,
            increment: 1,
            system_time_ms: started.elapsed().as_millis() as u32,
        });
        diagnostics.report(runtime.take_script_messages());
        let refresh_title = match advancement {
            Ok(FrameOutcome::Running {
                interface_language_refreshed,
            }) => interface_language_refreshed,
            Ok(FrameOutcome::Exited) => break,
            Err(error) => {
                eprintln!("pc {}：{}", error.offset, error.message);
                let runtime = runtime.into_task_runtime();
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
        };
        if diagnostics.timing && frame_started.elapsed().as_millis() >= 20 {
            eprintln!(
                "[slow-update] frame={now} ms={:.2}",
                frame_started.elapsed().as_secs_f64() * 1000.0
            );
        }
        if runtime.process_window_requests(&mut window)? {
            break;
        }
        if !image_import
            .as_ref()
            .is_some_and(window_import::WindowImport::active)
        {
            text_window.sync(&window.window, runtime.text_entry_request())?;
        }
        if let Some(importer) = &mut image_import {
            importer.sync(
                &window.window,
                runtime.platform_settings_mut(),
                text_window.active(),
            );
            if importer.active() {
                shared.borrow_mut().set_foreground(false);
            }
        }
        window.context.mouse().show_cursor(
            text_window.active()
                || image_import
                    .as_ref()
                    .is_some_and(window_import::WindowImport::active)
                || shared.borrow().cursor(runtime.uses_image_cursor()),
        );
        if !runtime.pending_window_requests().is_empty() && !window_pending_noted {
            eprintln!(
                "窗口动作尚未接入：{:?}",
                runtime.pending_window_requests()[0].action
            );
            window_pending_noted = true;
        }
        if let Some(position) = shared.borrow_mut().take_warp() {
            let size = window.window.size();
            window.context.mouse().warp_mouse_in_window(
                &window.window,
                (i64::from(position[0]) * i64::from(size.0) / i64::from(surface[0])) as i32,
                (i64::from(position[1]) * i64::from(size.1) / i64::from(surface[1])) as i32,
            );
        }
        clipboard.pump(runtime.take_clipboard_requests(), clipboard_encoding);
        audio.pump()?;
        if refresh_title {
            let next = String::from_utf16_lossy(runtime.string_tables().resolve(&title_source));
            if title != next {
                window.window.set_title(&next)?;
                title = next;
            }
        }
        update_time += frame_started.elapsed();
        let render_started = std::time::Instant::now();
        if !window.window.is_minimized() {
            #[cfg(target_os = "macos")]
            if let Some(output) = &metal_output {
                output.present(&mut runtime, surface)?;
            }
            if let Some(output) = &mut output {
                runtime.present_frame(|draw, canvas, capture| {
                    output.clear_rect(Rect::from_xywh(0, 0, surface[0] as i32, surface[1] as i32));
                    draw.render_into(output, Some(canvas))?;
                    window
                        .present(output)
                        .map_err(|error| katze_system2::Error::new(0, error))?;
                    if let Some(capture) = capture {
                        capture.write_render_target(output)?;
                    }
                    Ok(())
                })?;
            }
            render_time += render_started.elapsed();
        }
        text_window.draw()?;
        if let Some(importer) = &mut image_import {
            importer.draw()?;
        }
        quiz_windows.sync(&window.window, &runtime, clipboard_encoding)?;
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
