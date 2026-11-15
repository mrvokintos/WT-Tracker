// gamescope's host window is fullscreen, a normal topmost window ends up under it.
// so we run a second copy on gamescope's own display and mark its window
// GAMESCOPE_EXTERNAL_OVERLAY (same trick mangohud uses)

use crate::state::Ctl;
use crate::ui::overlay::{self, OverlayModel};
use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const GAME_PROCESSES: &[&str] = &["aces", "aces_be"];

pub fn detect_display() -> Option<String> {
    let me = std::env::var("DISPLAY").unwrap_or_default();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().filter(|s| s.bytes().all(|b| b.is_ascii_digit())) else { continue };
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if !GAME_PROCESSES.contains(&comm.trim()) {
            continue;
        }
        let Ok(env) = std::fs::read(format!("/proc/{pid}/environ")) else { continue };
        let mut display = None;
        let mut in_gamescope = false;
        for kv in env.split(|&b| b == 0) {
            let kv = String::from_utf8_lossy(kv);
            if let Some(v) = kv.strip_prefix("DISPLAY=") {
                display = Some(v.to_string());
            } else if kv.starts_with("GAMESCOPE_WAYLAND_DISPLAY=") {
                in_gamescope = true;
            }
        }
        if let Some(d) = display.filter(|d| in_gamescope && !d.is_empty() && *d != me) {
            return Some(d);
        }
    }
    None
}

struct ChildProc {
    child: Child,
    stdin: ChildStdin,
    display: String,
}

impl Drop for ChildProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn spawn_manager(ctl: Ctl) {
    std::thread::Builder::new()
        .name("gamescope-overlay".into())
        .spawn(move || {
            let mut child: Option<ChildProc> = None;
            let mut display: Option<String> = None;
            let mut last_detect = Instant::now() - Duration::from_secs(60);
            let mut last_sent: Option<(OverlayModel, Instant)> = None;
            loop {
                if last_detect.elapsed() > Duration::from_secs(3) {
                    last_detect = Instant::now();
                    let enabled = ctl.with(|s| s.cfg.overlay.gamescope);
                    display = if enabled { detect_display() } else { None };
                }
                if let Some(c) = &mut child {
                    if matches!(c.child.try_wait(), Ok(Some(_))) || display.as_deref() != Some(c.display.as_str()) {
                        log::info!("Оверлей в gamescope ({}) остановлен", c.display);
                        child = None;
                    }
                }
                if child.is_none() {
                    if let Some(d) = &display {
                        match start_child(d) {
                            Ok(c) => {
                                log::info!("Игра в gamescope (DISPLAY={d}) — оверлей запущен внутри gamescope");
                                child = Some(c);
                                last_sent = None;
                            }
                            Err(e) => {
                                log::error!("Не удалось запустить оверлей в gamescope: {e}");
                                display = None;
                            }
                        }
                    }
                }
                let ext = child.as_ref().map(|c| c.display.clone());
                if ctl.with(|s| std::mem::replace(&mut s.external_overlay, ext.clone()) != ext) {
                    let size = ext.as_deref().and_then(screen_size_of);
                    ctl.with(|s| s.external_screen = size);
                    ctl.repaint();
                }
                if let Some(c) = &mut child {
                    let m = OverlayModel::from_ctl(&ctl);
                    let due = last_sent.as_ref().is_none_or(|(prev, t)| *prev != m || t.elapsed() > Duration::from_secs(2));
                    if due {
                        let line = serde_json::to_string(&m).unwrap_or_default();
                        if writeln!(c.stdin, "{line}").and_then(|_| c.stdin.flush()).is_err() {
                            child = None;
                        } else {
                            last_sent = Some((m, Instant::now()));
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(33));
            }
        })
        .expect("gamescope thread");
}

fn start_child(display: &str) -> std::io::Result<ChildProc> {
    let exe = std::env::current_exe()?;
    let mut child = Command::new(exe)
        .arg("--overlay-child")
        .env("DISPLAY", display)
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::piped())
        .spawn()?;
    let stdin = child.stdin.take().ok_or_else(|| std::io::Error::other("no stdin"))?;
    Ok(ChildProc { child, stdin, display: display.to_string() })
}

struct ChildApp {
    model: Arc<Mutex<OverlayModel>>,
}

impl eframe::App for ChildApp {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let m = self.model.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if !m.lang.is_empty() && m.lang != crate::i18n::current_code() {
            crate::i18n::set(&m.lang);
        }
        // gamescope puts external overlays at 0,0 regardless of window pos
        let offset = egui::vec2(m.x as f32, m.y as f32) / ctx.pixels_per_point();
        overlay::draw(ctx, &m, false, offset);
    }
}

pub fn run_child() -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut first = String::new();
    stdin.lock().read_line(&mut first).map_err(|e| e.to_string())?;
    let initial: OverlayModel = serde_json::from_str(&first).unwrap_or_default();
    crate::i18n::set(if initial.lang.is_empty() { "auto" } else { &initial.lang });
    let model = Arc::new(Mutex::new(initial.clone()));

    let (sw, sh) = screen_size().unwrap_or((2560, 1440));
    let mut options = eframe::NativeOptions {
        viewport: overlay::builder(egui::pos2(0.0, 0.0), true).with_inner_size([sw as f32, sh as f32]),
        ..Default::default()
    };
    options.event_loop_builder = Some(Box::new(|b| {
        use winit::platform::x11::EventLoopBuilderExtX11;
        b.with_x11();
    }));
    eframe::run_native(
        "WT Tracker Overlay",
        options,
        Box::new(move |cc| {
            crate::fonts::install(&cc.egui_ctx);
            if let Err(e) = mark_external_overlay(cc) {
                log::warn!("GAMESCOPE_EXTERNAL_OVERLAY не установлен: {e}");
            }
            let (m, ctx) = (model.clone(), cc.egui_ctx.clone());
            std::thread::spawn(move || {
                for line in std::io::stdin().lock().lines() {
                    let Ok(line) = line else { break };
                    if let Ok(v) = serde_json::from_str::<OverlayModel>(&line) {
                        *m.lock().unwrap_or_else(|e| e.into_inner()) = v;
                        ctx.request_repaint();
                    }
                }
                std::process::exit(0);
            });
            Ok(Box::new(ChildApp { model }))
        }),
    )
    .map_err(|e| e.to_string())
}

fn screen_size() -> Option<(u16, u16)> {
    use x11rb::connection::Connection;
    let (conn, n) = x11rb::connect(None).ok()?;
    let s = &conn.setup().roots[n];
    Some((s.width_in_pixels, s.height_in_pixels))
}

fn screen_size_of(display: &str) -> Option<(u32, u32)> {
    use x11rb::connection::Connection;
    let (conn, n) = x11rb::connect(Some(display)).ok()?;
    let s = &conn.setup().roots[n];
    Some((s.width_in_pixels as u32, s.height_in_pixels as u32))
}

fn mark_external_overlay(cc: &eframe::CreationContext<'_>) -> Result<(), String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
    use x11rb::wrapper::ConnectionExt as _;

    let window = match cc.window_handle().map_err(|e| e.to_string())?.as_raw() {
        RawWindowHandle::Xlib(h) => h.window as u32,
        RawWindowHandle::Xcb(h) => h.window.get(),
        other => return Err(format!("not an X11 window: {other:?}")),
    };
    let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
    let atom = conn
        .intern_atom(false, b"GAMESCOPE_EXTERNAL_OVERLAY")
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?
        .atom;
    conn.change_property32(PropMode::REPLACE, window, atom, AtomEnum::CARDINAL, &[1])
        .map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())?;
    log::info!("Окно 0x{window:x} помечено GAMESCOPE_EXTERNAL_OVERLAY");
    Ok(())
}
