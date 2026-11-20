//! Offscreen host for the public game session.
//!
//! Run with `cargo run --example runprobe -- <game-directory> [frames]`.

#[path = "support/probe/mod.rs"]
mod probe;
mod support;

use katze_system2::{
    graphics::SoftwareBackend,
    host::graphics::GraphicsBackend,
    session::{FrameOutcome, FrameTime},
};
use probe::{AudioStats, ProbeWindow, ScriptedDevice, inspect_runtime, write_shot};
use support::{load_game, prepare_game};

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .ok_or("用法：runprobe <游戏目录> [帧数]")?;
    let frames: u32 = std::env::args()
        .nth(2)
        .map_or(Ok(600), |value| value.parse())?;
    let game = load_game(&root)?;
    let inspect = std::env::vars_os().any(|(key, value)| {
        let key = key.to_string_lossy();
        match key.as_ref() {
            "KATZE_PROBE_CLICKS" | "KATZE_PROBE_AUDIO_STATS" => false,
            "KATZE_PROBE_SHOT" => value.to_string_lossy().ends_with("@canvas"),
            _ => key.starts_with("KATZE_PROBE_"),
        }
    });
    if inspect {
        inspect_runtime(game, frames)
    } else {
        run_session(game, frames)
    }
}

fn run_session(
    game: support::Game,
    frames: u32,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let diagnostics = support::diagnostics::Output::from_env(game.router.script_encoding);
    let entry = game.start.clone();
    let (mut session, keys, size) = prepare_game(game)?;
    session.set_input_device(ScriptedDevice::new(keys, None, size))?;
    session.set_diagnostic_trace(diagnostics.trace());
    if let Some(trace) = diagnostics.native_trace() {
        session.set_native_trace(trace);
    }
    let mut session = session.start_game(entry.as_bytes())?;
    let mut window = ProbeWindow {
        fullscreen: session
            .window_preferences()
            .is_some_and(|prefs| prefs.fullscreen),
    };
    let mut audio = std::env::var_os("KATZE_PROBE_AUDIO_STATS")
        .is_some()
        .then(AudioStats::default);
    for round in 0..frames {
        let tick = round + 1;
        let outcome = session.advance(FrameTime {
            tick,
            increment: 1,
            system_time_ms: (u64::from(tick) * 1000 / 60) as u32,
        });
        diagnostics.report(session.take_script_messages());
        if outcome? == FrameOutcome::Exited || session.process_window_requests(&mut window)? {
            break;
        }
        if let Some(stats) = &mut audio {
            let mut pcm = [0.; 1600];
            session.mix_audio_stereo(48_000, &mut pcm)?;
            stats.add(round, &pcm);
        }
    }
    if let Some(stats) = audio {
        stats.report();
    }
    if let Some(path) = std::env::var_os("KATZE_PROBE_SHOT") {
        let surface =
            session.with_frame(|draw, canvas| SoftwareBackend.render(draw, canvas, size))?;
        write_shot(&path, size, |x, y| surface.pixel(x, y))?;
    }
    Ok(())
}
