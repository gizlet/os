//! The Gizlet launcher: a home screen, settings and a button test, drawn with
//! raylib in logical units so one layout fits the R36S's 640×480 panel, a
//! desktop window and anything between.
//!
//! Options: `--probe` prints what the device reports and exits; `--version`.
//!
//! Environment:
//! - `GIZLET_WINDOW=WIDTHxHEIGHT`: the window size on desktops (default 1280x720).
//! - `GIZLET_FULLSCREEN=1`: a borderless full-screen window.
//! - `GIZLET_FAKE_DEVICES=1`: simulated battery, backlight, volume, Wi-Fi and ssh.
//! - `GIZLET_LAUNCHER_DATA`: the folder for settings.
//! - `GIZLET_SCRIPT`: scripted input, see `script.rs`.

mod app;
mod clock;
#[cfg(all(target_os = "linux", feature = "gizlet-os"))]
mod evdev;
mod fonts;
mod i18n;
mod icons;
mod input;
mod keyboard;
mod painter;
mod script;
mod settings;
mod system;
mod theme;
mod ui;
mod zones;

use raylib_sys::{self as ffi, ConfigFlags as F};
use std::ffi::CString;

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Without a window system: raylib's DRM platform on the whole panel.
const KMS: bool = cfg!(feature = "gizlet-os");
/// Frames per second while anything moves; controls are polled as often when idle.
const ACTIVE_FPS: i32 = 60;
/// The focus pulses this long after the last input, then fades out.
const PULSE_FOR_SECONDS: f64 = 3.0;
const PULSE_FADE_SECONDS: f32 = 1.0;
/// A still screen is redrawn this often, for the clock.
const STILL_REDRAW_SECONDS: f64 = 1.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        Some("--version") => {
            println!("gizlet-launcher {VERSION}");
            return;
        }
        Some("--probe") => {
            probe();
            return;
        }
        Some(other) => {
            eprintln!("usage: gizlet-launcher [--probe | --version]\nunknown option {other}");
            std::process::exit(2);
        }
    }
    println!("Gizlet launcher {VERSION}");
    if KMS && !has_display() {
        println!("Gizlet launcher: no display (/dev/dri/card0 is missing)");
        std::process::exit(1);
    }
    run();
    println!("Leaving the launcher.");
}

fn has_display() -> bool {
    std::fs::read_dir("/dev/dri")
        .map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("card")))
        .unwrap_or(false)
}

/// Prints one line per device, for tests and for looking at a device over ssh.
fn probe() {
    let s = system::probe(settings::data_dir());
    let show = |name: &str, v: String| println!("{name}: {v}");
    show("battery", format!("{:?}", s.battery));
    show("brightness", format!("{:?}", s.brightness));
    show("volume", format!("{:?}", s.volume));
    show("wifi", format!("{:?}", s.wifi));
    show("ssh", format!("{:?}", s.ssh));
    show("clock", format!("{:?}", s.clock));
    show("power", format!("{:?}", s.power));
    show("hostname", format!("{:?}", s.hostname));
    show("ip", format!("{:?}", s.ip));
    show("model", format!("{:?}", s.info.model));
}

fn window_size() -> (i32, i32) {
    if KMS {
        return (0, 0);
    }
    std::env::var("GIZLET_WINDOW")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
        })
        .unwrap_or((1280, 720))
}

/// Every interface string in the current language, so the first frames need no atlas rebuild.
fn seed(app: &app::App) -> Vec<String> {
    let mut seed: Vec<String> = i18n::T::ALL.iter().map(|t| t.get(app.lang()).to_string()).collect();
    seed.push("0123456789:%+-·×←→•一二三四五六日月星期简体中文".into());
    seed.extend(zones::ZONES.iter().map(|z| z.label(app.lang())));
    seed
}

fn run() {
    let mut app = app::App::new(settings::data_dir(), VERSION);
    app.can_quit = true;
    let (w, h) = window_size();
    let mut flags = F::FLAG_VSYNC_HINT as u32;
    if !KMS {
        flags |= F::FLAG_MSAA_4X_HINT as u32 | F::FLAG_WINDOW_RESIZABLE as u32 | F::FLAG_WINDOW_HIGHDPI as u32;
    }
    unsafe {
        // On the card raylib's display warnings go to the boot logs; on a
        // computer only errors, as fonts warn about glyph sizes.
        let level = if KMS { ffi::TraceLogLevel::LOG_INFO } else { ffi::TraceLogLevel::LOG_ERROR };
        ffi::SetTraceLogLevel(level as i32);
        ffi::SetConfigFlags(flags);
        ffi::InitWindow(w, h, c"Gizlet".as_ptr());
        if !ffi::IsWindowReady() {
            println!("Gizlet launcher: the display could not be opened");
            std::process::exit(1);
        }
        ffi::SetExitKey(0);
        ffi::SetTargetFPS(ACTIVE_FPS);
        if !KMS {
            ffi::SetWindowMinSize(480, 360);
            if std::env::var("GIZLET_FULLSCREEN").is_ok_and(|v| v == "1") {
                ffi::ToggleBorderlessWindowed();
            }
        }
    }

    let mut painter = painter::Painter::new(fonts::Fonts::new(seed(&app).iter().map(String::as_str)), theme::palette(app.settings.theme));
    let mut ui = ui::Ui::default();
    let mut controls = input::Controls::default();
    let mut script = script::Script::from_env();
    let mut frame: u64 = 0;
    let mut last = unsafe { ffi::GetTime() };
    let mut active_at = 0.0;
    let mut drawn: Option<(f64, u64)> = None;

    while !unsafe { ffi::WindowShouldClose() } {
        // Measured here rather than by raylib, which counts drawn frames only.
        let now = unsafe { ffi::GetTime() };
        let dt = ((now - last) as f32).min(0.1);
        last = now;
        painter.resize();
        painter.fonts.rebuild_if_needed();
        let (mut input, mut pad) = controls.read(dt, app.time, app.text_input_active(), app.settings.swap_ab);
        if frame % 30 == 0 {
            app.gamepads = controls.gamepads();
        }
        frame += 1;

        let mut shot = None;
        match script.step(&mut input, &mut pad) {
            Some(script::Step::Quit) => break,
            Some(script::Step::Shot(path)) => shot = Some(path),
            None => {}
        }
        if !app.update(&input, dt) {
            break;
        }
        // Every frame is drawn while the user acts and transitions run. Then
        // the focus pulse settles and the screen stands still: it is drawn
        // again only for a device report, a resize or the clock, and the
        // controls are polled in between without touching the GPU.
        if !input.pressed.is_empty() || !input.held.is_empty() || !input.text.is_empty() || app.animating() || script.running() {
            active_at = app.time;
        }
        let quiet = (app.time - active_at - PULSE_FOR_SECONDS) as f32;
        painter.pulse = 1.0 - (quiet / PULSE_FADE_SECONDS).clamp(0.0, 1.0);
        let still = painter.pulse == 0.0 && shot.is_none() && !unsafe { ffi::IsWindowResized() };
        if let Some((at, revision)) = drawn {
            if still && revision == app.revision && app.time - at < STILL_REDRAW_SECONDS {
                unsafe { ffi::PollInputEvents() };
                std::thread::sleep(std::time::Duration::from_secs_f64(1.0 / ACTIVE_FPS as f64));
                continue;
            }
        }
        drawn = Some((app.time, app.revision));

        painter.time = app.time;
        unsafe {
            ffi::BeginDrawing();
            ffi::ClearBackground(painter.pal.bg_bottom);
        }
        ui.draw(&mut painter, &app, &pad, dt);
        unsafe { ffi::EndDrawing() };

        if let Some(path) = shot {
            if let Ok(c) = CString::new(path) {
                unsafe {
                    let image = ffi::LoadImageFromScreen();
                    ffi::ExportImage(image, c.as_ptr());
                    ffi::UnloadImage(image);
                }
            }
        }
    }
    drop(painter);
    unsafe { ffi::CloseWindow() };
}
