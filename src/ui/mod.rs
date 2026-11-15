pub mod overlay;
mod region;

use crate::config::{nation_display, type_display, NATIONS, TYPES};
use crate::t;
use crate::state::{Ctl, RegionPick};
use egui::{Color32, RichText};
use std::time::Duration;

pub struct TrackerApp {
    ctl: Ctl,
    overlay: overlay::OverlayHost,
    region: region::RegionPicker,
    hotkey_edit: [String; 2],
    hotkey_error: Option<String>,
    tab: Tab,
    shortcut_msg: Option<Result<String, String>>,
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Tracker,
    Settings,
}

enum Action {
    MoveOverlay(i32, i32),
    ApplyHotkey(usize),
    PickOnHotkey,
    PickDelayed,
    TestScan,
    UpdateDb,
    CreateShortcut,
}

fn section(ui: &mut egui::Ui, title: String) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).strong().size(15.0));
    ui.separator();
}

impl TrackerApp {
    pub fn new(ctl: Ctl) -> Self {
        let hotkey_edit = ctl.with(|s| [s.cfg.hotkeys.scan.clone(), s.cfg.hotkeys.overlay_toggle.clone()]);
        Self {
            ctl,
            overlay: overlay::OverlayHost::new(),
            region: region::RegionPicker::default(),
            hotkey_edit,
            hotkey_error: None,
            tab: Tab::Tracker,
            shortcut_msg: None,
        }
    }

    /// Status lines and the tab bar, shown on both tabs.
    fn header_ui(&mut self, ui: &mut egui::Ui) {
        let st = self.ctl.lock();
        let (txt, color) = if !st.logic.api_available {
            (t!("status.no_game"), Color32::from_rgb(255, 68, 68))
        } else if !st.logic.in_battle {
            (t!("status.hangar"), Color32::from_rgb(255, 165, 0))
        } else {
            (t!("status.battle"), Color32::from_rgb(0, 255, 0))
        };
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(txt).strong().color(color).size(16.0));
            let key = &st.cfg.hotkeys.overlay_toggle;
            let (ov_txt, ov_color) = if st.overlay_visible {
                (t!("overlay.visible", key = key), Color32::from_rgb(0, 200, 0))
            } else {
                (t!("overlay.hidden", key = key), Color32::from_rgb(255, 68, 68))
            };
            ui.label(RichText::new(ov_txt).color(ov_color));
        });
        drop(st);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Tracker, RichText::new(t!("tab.tracker")).size(15.0));
            ui.selectable_value(&mut self.tab, Tab::Settings, RichText::new(t!("tab.settings")).size(15.0));
        });
        ui.separator();
    }

    fn tracker_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let ctl = self.ctl.clone();
        let mut st = ctl.lock();
        let mut dirty = false;

        #[cfg(windows)]
        ui.label(RichText::new(t!("hint.windows_fullscreen")).small().weak());
        if let Some(d) = &st.external_overlay {
            ui.label(RichText::new(t!("hint.gamescope_active", display = d)).small().weak());
        }
        ui.horizontal(|ui| {
            ui.label(t!("player.nickname")).on_hover_text(t!("player.nickname_hint"));
            dirty |= ui
                .add(egui::TextEdit::singleline(&mut st.cfg.player.nickname).desired_width(160.0))
                .on_hover_text(t!("player.nickname_hint"))
                .changed();
        });
        ui.add_space(4.0);
        ui.label(RichText::new(t!("filters.nations")).strong());
        egui::Grid::new("nations").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
            for (i, key) in NATIONS.iter().enumerate() {
                let mut on = st.cfg.has_nation(key);
                if ui.checkbox(&mut on, nation_display(key)).changed() {
                    st.cfg.set_nation(key, on);
                    dirty = true;
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.add_space(4.0);
        ui.label(RichText::new(t!("filters.types")).strong());
        ui.horizontal_wrapped(|ui| {
            for key in TYPES {
                let mut on = st.cfg.has_type(key);
                if ui.checkbox(&mut on, type_display(key)).changed() {
                    st.cfg.set_type(key, on);
                    dirty = true;
                }
            }
        });
        ui.add_space(4.0);
        dirty |= ui.checkbox(&mut st.cfg.display.show_notes, t!("display.notes")).changed();
        dirty |= ui.checkbox(&mut st.cfg.display.show_names, t!("display.names")).changed();
        dirty |= ui.checkbox(&mut st.cfg.audio.enabled, t!("audio.nuke")).changed();

        ui.separator();
        let types_line = {
            let (_, types) = st.logic.db_counts();
            let n = |t: &str| types.get(t).copied().unwrap_or(0);
            t!("db.by_type", tank = n("tank"), plane = n("plane"), heli = n("heli"), zsu = n("zsu"))
        };
        ui.horizontal(|ui| {
            let count = st.logic.db_counts().0;
            let summary = match st.db_version.as_deref() {
                Some(v) => t!("db.summary_version", count = count, version = v),
                None => t!("db.summary", count = count),
            };
            ui.label(RichText::new(summary).small().weak());
            if ui.small_button(t!("db.update")).on_hover_text(t!("db.update_hint")).clicked() {
                actions.push(Action::UpdateDb);
            }
            // short results ("updated", "up to date") sit next to the button, errors get their own line
            if !st.db_status.is_empty() && st.db_status.chars().count() <= 24 {
                ui.label(RichText::new(&st.db_status).small().weak());
            }
        });
        ui.label(RichText::new(types_line).small().weak());
        if st.db_status.chars().count() > 24 {
            ui.label(RichText::new(&st.db_status).small().weak());
        }
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(t!("enemies.count", count = st.logic.enemy_count())).small().weak());
            });
        });
        if dirty {
            st.mark_dirty();
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let ctl = self.ctl.clone();
        let mut st = ctl.lock();
        let mut dirty = false;

        section(ui, t!("settings.overlay"));
        ui.label(RichText::new(t!("settings.drag_hint")).small().weak());
        #[cfg(target_os = "linux")]
        {
            dirty |= ui.checkbox(&mut st.cfg.overlay.gamescope, t!("settings.gamescope")).changed();
        }
        dirty |= ui.add(egui::Slider::new(&mut st.cfg.overlay.size, 10..=60).text(t!("settings.text_size"))).changed();
        dirty |= ui.add(egui::Slider::new(&mut st.cfg.overlay.opacity, 10..=100).text(t!("settings.opacity"))).changed();
        ui.horizontal(|ui| {
            ui.label(t!("settings.position"));
            let a = ui.add(egui::DragValue::new(&mut st.cfg.overlay.x).prefix("X "));
            let b = ui.add(egui::DragValue::new(&mut st.cfg.overlay.y).prefix("Y "));
            if a.changed() || b.changed() {
                dirty = true;
                actions.push(Action::MoveOverlay(st.cfg.overlay.x, st.cfg.overlay.y));
            }
        });
        if let Some((sw, sh)) = st.external_screen {
            ui.label(RichText::new(t!("settings.minimap_hint")).small().weak());
            if overlay_minimap(ui, &mut st.cfg.overlay, sw, sh) {
                dirty = true;
            }
        }

        section(ui, t!("settings.hotkeys"));
        for (i, label) in [t!("settings.scan_key"), t!("settings.hide_key")].iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.add(egui::TextEdit::singleline(&mut self.hotkey_edit[i]).desired_width(90.0));
                if ui.button(t!("settings.set")).clicked() {
                    actions.push(Action::ApplyHotkey(i));
                }
            });
        }
        if let Some(e) = &self.hotkey_error {
            ui.colored_label(Color32::from_rgb(255, 80, 80), e);
        }
        ui.label(RichText::new(t!("settings.hotkey_backend", backend = st.hotkey_backend)).small().weak());

        section(ui, t!("settings.zone"));
        match st.cfg.scanner.region {
            Some([x, y, w, h]) => ui.colored_label(Color32::from_rgb(0, 200, 0), format!("X={x}, Y={y}, W={w}, H={h}")),
            None => ui.colored_label(Color32::from_rgb(255, 68, 68), t!("settings.zone_unset")),
        };
        match st.region_pick {
            RegionPick::WaitingHotkey => {
                ui.colored_label(Color32::YELLOW, t!("settings.zone_wait", key = st.cfg.hotkeys.scan));
                if ui.button(t!("common.cancel")).clicked() {
                    st.region_pick = RegionPick::Idle;
                }
            }
            RegionPick::Capturing => {
                ui.colored_label(Color32::YELLOW, t!("settings.capturing"));
            }
            RegionPick::Picking { .. } => {
                ui.label(t!("settings.pick_in_window"));
            }
            RegionPick::Idle => {
                ui.horizontal_wrapped(|ui| {
                    if ui.button(t!("settings.pick_on_hotkey")).clicked() {
                        actions.push(Action::PickOnHotkey);
                    }
                    if ui.button(t!("settings.pick_delayed")).clicked() {
                        actions.push(Action::PickDelayed);
                    }
                });
            }
        }
        dirty |= ui
            .add(egui::Slider::new(&mut st.cfg.scanner.confidence_threshold, 0.50..=0.99).text(t!("settings.confidence")))
            .changed();
        ui.horizontal(|ui| {
            if ui.add_enabled(!st.scanning, egui::Button::new(t!("settings.test_scan"))).clicked() {
                actions.push(Action::TestScan);
            }
            if st.scanning {
                ui.spinner();
            }
        });
        if !st.last_scan_report.is_empty() {
            egui::CollapsingHeader::new(t!("settings.last_scan")).id_salt("last_scan").show(ui, |ui| {
                for l in &st.last_scan_report {
                    ui.label(RichText::new(l).small().monospace());
                }
            });
        }

        section(ui, t!("settings.app"));
        ui.horizontal(|ui| {
            ui.label(t!("settings.language"));
            let langs = crate::i18n::languages();
            let selected = if st.cfg.ui.language == "auto" {
                t!("settings.language_auto")
            } else {
                langs.iter().find(|l| l.code == st.cfg.ui.language).map_or(st.cfg.ui.language.clone(), |l| l.name.clone())
            };
            egui::ComboBox::from_id_salt("lang").selected_text(selected).show_ui(ui, |ui| {
                let mut choice = st.cfg.ui.language.clone();
                ui.selectable_value(&mut choice, "auto".to_string(), t!("settings.language_auto"));
                for l in langs {
                    ui.selectable_value(&mut choice, l.code.clone(), &l.name);
                }
                if choice != st.cfg.ui.language {
                    crate::i18n::set(&choice);
                    st.cfg.ui.language = choice;
                    dirty = true;
                }
            });
        });
        #[cfg(target_os = "linux")]
        {
            ui.horizontal(|ui| {
                ui.label(t!("settings.backend"));
                let name = |b: &str| if b == "wayland" { t!("settings.backend_wayland") } else { t!("settings.backend_x11") };
                egui::ComboBox::from_id_salt("backend").selected_text(name(&st.cfg.ui.backend)).show_ui(ui, |ui| {
                    for b in ["x11", "wayland"] {
                        dirty |= ui.selectable_value(&mut st.cfg.ui.backend, b.to_string(), name(b)).changed();
                    }
                });
            });
            let running = if crate::NATIVE_WAYLAND.load(std::sync::atomic::Ordering::Relaxed) { "wayland" } else { "x11" };
            if st.cfg.ui.backend != running {
                ui.label(RichText::new(t!("settings.backend_restart")).small().color(Color32::YELLOW));
            } else {
                ui.label(RichText::new(t!("settings.backend_hint")).small().weak());
            }
        }
        dirty |= ui.checkbox(&mut st.cfg.db.auto_update, t!("db.auto_hint")).changed();
        dirty |= ui.checkbox(&mut st.cfg.record.enabled, t!("record.enabled")).changed();
        ui.add_space(4.0);
        if ui.button(t!("shortcut.create")).on_hover_text(t!("shortcut.hint")).clicked() {
            actions.push(Action::CreateShortcut);
        }
        match &self.shortcut_msg {
            Some(Ok(path)) => {
                ui.label(RichText::new(t!("shortcut.done", path = path)).small().color(Color32::from_rgb(0, 200, 0)));
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::from_rgb(255, 80, 80), t!("shortcut.failed", error = e));
            }
            None => {}
        }
        if dirty {
            st.mark_dirty();
        }
    }

    fn run_actions(&mut self, ctx: &egui::Context, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::MoveOverlay(x, y) => self.overlay.move_to(ctx, x, y),
                Action::ApplyHotkey(i) => {
                    let text = self.hotkey_edit[i].trim().to_lowercase();
                    match crate::hotkeys::keys::parse(&text) {
                        Some(k) => {
                            self.hotkey_error = None;
                            self.hotkey_edit[i] = k.name().to_string();
                            self.ctl.with(|s| {
                                if i == 0 {
                                    s.cfg.hotkeys.scan = k.name().to_string();
                                } else {
                                    s.cfg.hotkeys.overlay_toggle = k.name().to_string();
                                }
                                s.mark_dirty();
                            });
                            self.ctl.update_bindings();
                        }
                        None => self.hotkey_error = Some(t!("settings.unknown_key", key = text)),
                    }
                }
                Action::PickOnHotkey => self.ctl.with(|s| s.region_pick = RegionPick::WaitingHotkey),
                Action::PickDelayed => self.ctl.capture_for_pick(Duration::from_secs(5)),
                Action::TestScan => self.ctl.start_scan(),
                Action::UpdateDb => crate::dbupdate::spawn_check(self.ctl.clone(), true),
                Action::CreateShortcut => {
                    let r = crate::shortcut::create().map(|p| p.display().to_string());
                    match &r {
                        Ok(p) => log::info!("shortcut created: {p}"),
                        Err(e) => log::warn!("shortcut: {e}"),
                    }
                    self.shortcut_msg = Some(r);
                }
            }
        }
    }
}

fn overlay_minimap(ui: &mut egui::Ui, ov: &mut crate::config::OverlayCfg, sw: u32, sh: u32) -> bool {
    let (sw, sh) = (sw.max(1) as f32, sh.max(1) as f32);
    let w = ui.available_width().min(360.0);
    let k = w / sw;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, sh * k), egui::Sense::click_and_drag());
    let size = ov.size as f32;
    let box_px = egui::vec2(size * 16.0, size * 1.3 * 5.0);
    let mut changed = false;
    if resp.dragged() {
        ov.x += (resp.drag_delta().x / k).round() as i32;
        ov.y += (resp.drag_delta().y / k).round() as i32;
        changed = true;
    } else if resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            ov.x = ((p.x - rect.min.x) / k).round() as i32;
            ov.y = ((p.y - rect.min.y) / k).round() as i32;
            changed = true;
        }
    }
    ov.x = ov.x.clamp(0, (sw - box_px.x).max(0.0) as i32);
    ov.y = ov.y.clamp(0, (sh - box_px.y).max(0.0) as i32);
    let p = ui.painter();
    p.rect_filled(rect, 2.0, Color32::from_gray(25));
    p.rect_stroke(rect, 2.0, egui::Stroke::new(1.0_f32, Color32::from_gray(90)));
    p.text(rect.center(), egui::Align2::CENTER_CENTER, format!("{}×{}", sw as u32, sh as u32), egui::FontId::proportional(11.0), Color32::from_gray(80));
    let b = egui::Rect::from_min_size(rect.min + egui::vec2(ov.x as f32, ov.y as f32) * k, box_px * k);
    p.rect_filled(b, 1.0, Color32::from_rgba_unmultiplied(255, 60, 60, 70));
    p.rect_stroke(b, 1.0, egui::Stroke::new(1.0_f32, Color32::from_rgb(255, 80, 80)));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    changed
}

impl eframe::App for TrackerApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut actions = Vec::new();
        egui::CentralPanel::default().show(ctx, |ui| {
            self.header_ui(ui);
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| match self.tab {
                Tab::Tracker => self.tracker_ui(ui, &mut actions),
                Tab::Settings => self.settings_ui(ui, &mut actions),
            });
        });
        // overlay frame is draggable while the settings tab is open
        let setup = self.tab == Tab::Settings;
        self.ctl.with(|s| s.setup_mode = setup);
        self.run_actions(ctx, actions);
        self.overlay.show(ctx, &self.ctl);
        self.region.show(ctx, &self.ctl);
        self.ctl.save_if_dirty();
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.ctl.save_now();
    }
}
