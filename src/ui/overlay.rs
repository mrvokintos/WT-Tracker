use crate::state::Ctl;
use crate::t;
use egui::{Align2, Color32, FontId, Pos2, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const OV_SIZE: [f32; 2] = [720.0, 480.0];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OverlayModel {
    pub visible: bool,
    pub setup: bool,
    pub size: f32,
    pub alpha: f32,
    pub x: i32,
    pub y: i32,
    pub scan_msg: Option<String>,
    pub lines: Vec<(String, bool)>,
    /// UI language, so the gamescope child process renders the same texts
    pub lang: String,
}

impl OverlayModel {
    pub fn from_ctl(ctl: &Ctl) -> Self {
        ctl.with(|s| OverlayModel {
            visible: s.overlay_visible || s.setup_mode,
            setup: s.setup_mode,
            size: s.cfg.overlay.size.clamp(8, 96) as f32,
            alpha: s.cfg.overlay.opacity.clamp(5, 100) as f32 / 100.0,
            x: s.cfg.overlay.x,
            y: s.cfg.overlay.y,
            scan_msg: s.current_scan_message().map(str::to_string),
            lines: if s.logic.in_battle {
                s.logic.overlay_lines(&s.cfg).into_iter().map(|l| (l.text, l.nuke)).collect()
            } else {
                vec![]
            },
            lang: crate::i18n::current_code(),
        })
    }
}

pub struct OverlayHost {
    spawn_pos: Option<Pos2>,
}

impl OverlayHost {
    pub fn new() -> Self {
        Self { spawn_pos: None }
    }

    pub fn show(&mut self, ctx: &egui::Context, ctl: &Ctl) {
        let (visible, setup, pos, external) = ctl.with(|s| {
            (
                s.overlay_visible,
                s.setup_mode,
                Pos2::new(s.cfg.overlay.x as f32, s.cfg.overlay.y as f32),
                s.external_overlay.is_some(),
            )
        });
        if external || (!visible && !setup) {
            self.spawn_pos = None;
            return;
        }
        let spawn = *self.spawn_pos.get_or_insert(pos);
        let ctl = ctl.clone();
        ctx.show_viewport_deferred(Ctl::overlay_id(), builder(spawn, !setup), move |ctx, _| {
            let m = OverlayModel::from_ctl(&ctl);
            if let Some(p) = draw(ctx, &m, true, Vec2::ZERO) {
                ctl.with(|s| {
                    if (s.cfg.overlay.x, s.cfg.overlay.y) != p {
                        (s.cfg.overlay.x, s.cfg.overlay.y) = p;
                        s.mark_dirty();
                    }
                });
            }
        });
    }

    pub fn move_to(&self, ctx: &egui::Context, x: i32, y: i32) {
        ctx.send_viewport_cmd_to(
            Ctl::overlay_id(),
            egui::ViewportCommand::OuterPosition(Pos2::new(x as f32, y as f32)),
        );
    }
}

pub fn builder(pos: Pos2, passthrough: bool) -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title("WT Tracker Overlay")
        .with_app_id("wt-tracker-overlay")
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_taskbar(false)
        .with_resizable(false)
        .with_active(false)
        .with_mouse_passthrough(passthrough)
        .with_inner_size(OV_SIZE)
        .with_position(pos)
}

pub fn draw(ctx: &egui::Context, m: &OverlayModel, draggable: bool, offset: Vec2) -> Option<(i32, i32)> {
    let fade = |c: Color32| c.gamma_multiply(m.alpha);
    let mut pos = None;
    egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
        let full = ui.max_rect();
        let rect = if offset == Vec2::ZERO {
            full
        } else {
            egui::Rect::from_min_size(full.min + offset, Vec2::from(OV_SIZE)).intersect(full)
        };
        let painter = ui.painter();
        let font = FontId::proportional(m.size);
        let line_h = m.size * 1.3;
        let mut y = rect.top() + 4.0;
        let mut text = |s: &str, color: Color32| {
            let p = Pos2::new(rect.left() + 6.0, y);
            painter.text(p + Vec2::splat(1.5), Align2::LEFT_TOP, s, font.clone(), fade(Color32::from_black_alpha(200)));
            painter.text(p, Align2::LEFT_TOP, s, font.clone(), fade(color));
            y += line_h;
        };
        if !m.visible {
            return;
        }
        if m.setup {
            painter.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(2.0_f32, Color32::from_white_alpha(150)));
            text(&if draggable { t!("overlay.setup_drag") } else { t!("overlay.setup_fields") }, Color32::RED);
            text(&t!("overlay.enemies"), Color32::WHITE);
            text(&format!("🛡 Leopard 2A7V (Hans) — {}", t!("overlay.sample_note")), Color32::RED);
            text("☢ T-90M (Ivan) [9 K]", Color32::from_rgb(0, 255, 255));
            if draggable {
                let resp = ui.interact(rect, ui.id().with("drag"), Sense::drag());
                if resp.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                pos = ctx.input(|i| i.viewport().outer_rect).map(|r| (r.min.x.round() as i32, r.min.y.round() as i32));
            }
            return;
        }
        if let Some(msg) = &m.scan_msg {
            text(msg, Color32::YELLOW);
        }
        if !m.lines.is_empty() {
            text(&t!("overlay.enemies"), Color32::WHITE);
            for (t, nuke) in &m.lines {
                text(t, if *nuke { Color32::from_rgb(0, 255, 255) } else { Color32::RED });
            }
        }
    });
    ctx.request_repaint_after(Duration::from_secs(1));
    pos
}
