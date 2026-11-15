use crate::config::{nation_display, Config};
use crate::hotkeys::{self, Bindings, HotkeyEvent};
use crate::logic::{Logic, SoundKind};
use crate::paths;
use crate::scan;
use crate::t;
use image::RgbaImage;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub const OVERLAY_ID: &str = "wt-overlay";

pub enum RegionPick {
    Idle,
    WaitingHotkey,
    Capturing,
    Picking { img: Arc<RgbaImage>, origin: (i32, i32) },
}

pub struct AppState {
    pub cfg: Config,
    pub logic: Logic,
    pub overlay_visible: bool,
    pub setup_mode: bool,
    pub scan_message: Option<(String, Instant)>,
    pub scanning: bool,
    pub region_pick: RegionPick,
    pub last_scan_report: Vec<String>,
    pub hotkey_backend: String,
    pub cfg_dirty: Option<Instant>,
    pub external_overlay: Option<String>,
    pub external_screen: Option<(u32, u32)>,
    pub db_status: String,
    pub db_version: Option<String>,
}

impl AppState {
    pub fn new(cfg: Config, logic: Logic) -> Self {
        Self {
            cfg,
            logic,
            overlay_visible: true,
            setup_mode: false,
            scan_message: None,
            scanning: false,
            region_pick: RegionPick::Idle,
            last_scan_report: Vec::new(),
            hotkey_backend: String::new(),
            cfg_dirty: None,
            external_overlay: None,
            external_screen: None,
            db_status: String::new(),
            db_version: crate::dbupdate::installed_version(),
        }
    }

    pub fn mark_dirty(&mut self) {
        self.cfg_dirty = Some(Instant::now());
    }

    pub fn set_scan_message(&mut self, msg: impl Into<String>) {
        self.scan_message = Some((msg.into(), Instant::now() + Duration::from_secs(10)));
    }

    pub fn current_scan_message(&self) -> Option<&str> {
        self.scan_message
            .as_ref()
            .filter(|(_, until)| Instant::now() < *until)
            .map(|(m, _)| m.as_str())
    }
}

#[derive(Clone)]
pub struct Ctl {
    state: Arc<Mutex<AppState>>,
    pub ctx: egui::Context,
    sounds: Sender<SoundKind>,
    pub bindings: Bindings,
    last_hotkey: Arc<Mutex<HashMap<HotkeyEvent, Instant>>>,
}

impl Ctl {
    pub fn new(state: AppState, ctx: egui::Context, sounds: Sender<SoundKind>) -> Self {
        let bindings = hotkeys::bindings(&state.cfg.hotkeys.scan, &state.cfg.hotkeys.overlay_toggle);
        Self {
            state: Arc::new(Mutex::new(state)),
            ctx,
            sounds,
            bindings,
            last_hotkey: Default::default(),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, AppState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut AppState) -> R) -> R {
        f(&mut self.lock())
    }

    pub fn overlay_id() -> egui::ViewportId {
        egui::ViewportId::from_hash_of(OVERLAY_ID)
    }

    pub fn repaint(&self) {
        self.ctx.request_repaint();
        self.ctx.request_repaint_of(Self::overlay_id());
    }

    pub fn flush_sounds(&self) {
        let (sounds, enabled) = self.with(|s| (s.logic.take_sounds(), s.cfg.audio.enabled));
        if enabled {
            for k in sounds {
                let _ = self.sounds.send(k);
            }
        }
    }

    pub fn update_bindings(&self) {
        let (scan, ov) = self.with(|s| (s.cfg.hotkeys.scan.clone(), s.cfg.hotkeys.overlay_toggle.clone()));
        *self.bindings.lock().unwrap_or_else(|e| e.into_inner()) =
            [hotkeys::keys::parse(&scan), hotkeys::keys::parse(&ov)];
    }

    pub fn start_hotkeys(&self) {
        let (tx, rx) = std::sync::mpsc::channel();
        let backend = hotkeys::start(self.bindings.clone(), tx);
        log::info!("Хоткеи: {backend}");
        self.with(|s| s.hotkey_backend = backend);
        let ctl = self.clone();
        std::thread::Builder::new()
            .name("hotkey-dispatch".into())
            .spawn(move || {
                for ev in rx {
                    ctl.on_hotkey(ev);
                }
            })
            .expect("hotkey thread");
    }

    fn on_hotkey(&self, ev: HotkeyEvent) {
        // x11 + portal can both fire for one press
        {
            let mut last = self.last_hotkey.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            if last.get(&ev).is_some_and(|t| now.duration_since(*t) < Duration::from_millis(250)) {
                return;
            }
            last.insert(ev, now);
        }
        match ev {
            HotkeyEvent::Scan => self.start_scan(),
            HotkeyEvent::ToggleOverlay => {
                self.with(|s| s.overlay_visible = !s.overlay_visible);
                self.repaint();
            }
        }
    }

    pub fn start_scan(&self) {
        enum Next {
            Nothing,
            Pick,
            Scan([i32; 4], f32),
        }
        let next = self.with(|s| {
            if s.scanning {
                return Next::Nothing;
            }
            if matches!(s.region_pick, RegionPick::WaitingHotkey) {
                return Next::Pick;
            }
            match s.cfg.scanner.region {
                Some(r) => {
                    s.scanning = true;
                    Next::Scan(r, s.cfg.scanner.confidence_threshold)
                }
                None => {
                    s.set_scan_message(t!("scan.no_zone"));
                    Next::Nothing
                }
            }
        });
        match next {
            Next::Nothing => self.repaint(),
            Next::Pick => self.capture_for_pick(Duration::ZERO),
            Next::Scan(region, conf) => {
                let ctl = self.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(250));
                    let res = scan::scan_region(region, conf);
                    ctl.with(|s| {
                        s.scanning = false;
                        match res {
                            Ok(out) => {
                                s.last_scan_report = out.report;
                                if out.black_frame {
                                    s.set_scan_message(t!("scan.black"));
                                } else if out.nations.is_empty() {
                                    s.set_scan_message(t!("scan.nothing"));
                                } else {
                                    // the scoreboard shows operator flags (Russia, FRG, Taiwan...), the filters use tree nations
                                    let enemies = s.logic.flags_to_nations(&out.nations);
                                    if out.own.is_empty() {
                                        let names: Vec<String> = enemies.iter().map(|n| nation_display(n)).collect();
                                        s.set_scan_message(t!("scan.found", nations = names.join(", ")));
                                    } else {
                                        // both teams: trees present on both sides are resolved through the kill graph
                                        let ctx = s.logic.team_context(&out.nations, &out.own);
                                        let display = |pred: &dyn Fn(&String) -> bool| -> String {
                                            let v: Vec<String> = enemies.iter().filter(|n| pred(n)).map(|n| nation_display(n)).collect();
                                            if v.is_empty() { "—".into() } else { v.join(", ") }
                                        };
                                        let only = display(&|n| !ctx.own_trees.contains(n));
                                        let both = display(&|n| ctx.own_trees.contains(n));
                                        s.set_scan_message(t!("scan.found_teams", enemies = only, both = both));
                                        s.logic.set_teams(ctx);
                                    }
                                    for n in crate::config::NATIONS {
                                        s.cfg.set_nation(n, enemies.iter().any(|e| e == n));
                                    }
                                    s.mark_dirty();
                                }
                            }
                            Err(e) => {
                                log::error!("scan: {e}");
                                s.set_scan_message(t!("scan.capture_error"));
                                s.last_scan_report = vec![e];
                            }
                        }
                    });
                    ctl.repaint();
                });
            }
        }
    }

    pub fn capture_for_pick(&self, delay: Duration) {
        self.with(|s| s.region_pick = RegionPick::Capturing);
        self.repaint();
        let ctl = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            let res = scan::capture::capture_full();
            ctl.with(|s| match res {
                Ok(cap) if cap.img.pixels().all(|p| p.0[..3].iter().all(|&c| c <= 12)) => {
                    s.last_scan_report = vec![t!("scan.black")];
                    s.set_scan_message(t!("scan.black"));
                    s.region_pick = RegionPick::Idle;
                }
                Ok(cap) => s.region_pick = RegionPick::Picking { img: Arc::new(cap.img), origin: cap.origin },
                Err(e) => {
                    log::error!("{e}");
                    s.last_scan_report = vec![e];
                    s.region_pick = RegionPick::Idle;
                }
            });
            ctl.repaint();
        });
    }

    pub fn save_now(&self) {
        let (cfg, unknown) = self.with(|s| {
            s.cfg_dirty = None;
            (s.cfg.clone(), s.logic.take_unknowns())
        });
        let path = paths::file(paths::CONFIG_FILE);
        match cfg.save(&path) {
            Ok(()) => log::info!("Конфиг сохранён: {}", path.display()),
            Err(e) => log::error!("Не удалось сохранить {}: {e}", path.display()),
        }
        if let Err(e) = crate::vehicles::save_unknowns(&paths::file(paths::UNKNOWN_FILE), &unknown) {
            log::error!("Не удалось сохранить unknown.txt: {e}");
        }
    }

    pub fn save_if_dirty(&self) {
        let due = self.with(|s| s.cfg_dirty.is_some_and(|t| t.elapsed() > Duration::from_secs(1)));
        if due {
            self.save_now();
        }
    }
}
