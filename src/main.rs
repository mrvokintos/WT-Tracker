#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod api;
mod audio;
mod config;
mod dbupdate;
mod fonts;
mod game;
#[cfg(target_os = "linux")]
mod gamescope;
mod hotkeys;
mod i18n;
mod killfeed;
mod logic;
mod paths;
mod record;
mod scan;
mod shortcut;
mod state;
mod ui;
mod vehicles;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

/// The main window runs as a native Wayland client (shown in settings, the choice applies on restart).
pub static NATIVE_WAYLAND: AtomicBool = AtomicBool::new(false);

const HELP: &str = "\
WT Tracker — трекер врагов War Thunder

Использование:
  wt-tracker                      запустить трекер
  wt-tracker --calibrate <img>    проверить распознавание флагов на скриншоте
      [--flags <папка>] [--conf <0.0-1.0>]
  wt-tracker --pick-region        сразу снять экран и открыть выбор зоны сканирования
  wt-tracker --wayland            (Linux) нативное Wayland-окно вместо XWayland
                                  (оверлей может не держаться поверх игры)
  wt-tracker --x11                (Linux) через X11/XWayland, даже если в настройках Wayland

Переменные окружения:
  WT_TRACKER_DIR   папка с config.json / vehicles.json / flags
  WT_API_URL       адрес API игры (по умолчанию http://localhost:8111)
  RUST_LOG         уровень логов (info, debug)
";

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return;
    }
    #[cfg(target_os = "linux")]
    if args.iter().any(|a| a == "--overlay-child") {
        if let Err(e) = gamescope::run_child() {
            log::error!("overlay-child: {e}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--calibrate") {
        std::process::exit(calibrate(&args, i));
    }
    // None = take it from config.json (ui.backend)
    let native_wayland = if args.iter().any(|a| a == "--wayland") {
        Some(true)
    } else if args.iter().any(|a| a == "--x11") {
        Some(false)
    } else {
        None
    };
    let pick_region = args.iter().any(|a| a == "--pick-region");
    if let Err(e) = run(native_wayland, pick_region) {
        log::error!("{e}");
        std::process::exit(1);
    }
}

fn arg_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn calibrate(args: &[String], i: usize) -> i32 {
    use crate::t;
    i18n::set("auto");
    let Some(img_path) = args.get(i + 1) else {
        eprintln!("--calibrate: укажи путь к скриншоту");
        return 2;
    };
    let conf: f32 = arg_value(args, "--conf").and_then(|s| s.parse().ok()).unwrap_or(0.70);
    let flags = arg_value(args, "--flags").map(PathBuf::from).unwrap_or_else(|| paths::file(paths::FLAGS_DIR));
    let img = match image::open(img_path) {
        Ok(i) => i.to_rgb8(),
        Err(e) => {
            eprintln!("Не удалось открыть {img_path}: {e}");
            return 1;
        }
    };
    let tpls = scan::matcher::templates(&flags);
    println!("Скриншот {}x{}, шаблонов: {}, порог {conf:.2}", img.width(), img.height(), tpls.len());
    let t = std::time::Instant::now();
    let out = scan::matcher::scan(&img, &tpls, conf);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    for r in &out.results {
        println!("{}", scan::matcher::describe(r, conf));
    }
    if out.fallback {
        println!("{}", t!("report.fallback"));
    } else {
        println!("мест флагов: {}", out.slots.len());
    }
    println!("({ms:.0} мс)");
    0
}

fn run(native_wayland: Option<bool>, pick_region: bool) -> Result<(), String> {
    log::info!("WT Tracker {}, рабочая папка: {}", env!("CARGO_PKG_VERSION"), paths::base_dir().display());
    let cfg = config::Config::load(&paths::file(paths::CONFIG_FILE));
    let native_wayland = native_wayland.unwrap_or(cfg.ui.backend == "wayland");
    i18n::set(&cfg.ui.language);
    let db_path = paths::find(paths::VEHICLES_FILE).unwrap_or_else(|| paths::file(paths::VEHICLES_FILE));
    let db = vehicles::VehicleDb::load(&db_path, paths::EMBEDDED_VEHICLES);
    let legacy = db.is_legacy();
    let mut state = state::AppState::new(cfg, logic::Logic::new(db));
    if legacy {
        log::warn!(
            "{} is from the Python version (no vehicle ids); delete it or update the database",
            db_path.display()
        );
        state.db_status = crate::t!("db.legacy");
    }

    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("WT Tracker {}", env!("CARGO_PKG_VERSION")))
            .with_app_id("wt-tracker")
            .with_inner_size([400.0, 640.0])
            .with_min_inner_size([340.0, 400.0])
            // overlay needs an alpha visual
            .with_transparent(true),
        ..Default::default()
    };

    #[cfg(target_os = "linux")]
    {
        let has_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        if has_wayland && (native_wayland || std::env::var_os("DISPLAY").is_none()) {
            NATIVE_WAYLAND.store(true, std::sync::atomic::Ordering::Relaxed);
            log::info!("Нативное Wayland-окно (оверлей может не держаться поверх игры)");
        } else if std::env::var_os("DISPLAY").is_some() {
            // xwayland on purpose: native wayland windows can't stay above the game without layer-shell
            options.event_loop_builder = Some(Box::new(|b| {
                use winit::platform::x11::EventLoopBuilderExtX11;
                b.with_x11();
            }));
            log::info!("Окна через X11/XWayland (Wayland: настройки или --wayland)");
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = native_wayland;

    eframe::run_native(
        "WT Tracker",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            fonts::install(&cc.egui_ctx);
            let ctl = state::Ctl::new(state, cc.egui_ctx.clone(), audio::spawn());
            ctl.start_hotkeys();
            api::spawn(ctl.clone());
            if ctl.with(|s| s.cfg.db.auto_update) {
                dbupdate::spawn_check(ctl.clone(), false);
            }
            #[cfg(target_os = "linux")]
            gamescope::spawn_manager(ctl.clone());
            if pick_region {
                ctl.capture_for_pick(std::time::Duration::from_millis(300));
            }
            Ok(Box::new(ui::TrackerApp::new(ctl)))
        }),
    )
    .map_err(|e| e.to_string())
}
