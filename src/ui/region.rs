use crate::state::{Ctl, RegionPick};
use crate::t;
use egui::{Color32, Key, PointerButton, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2};
use std::sync::Arc;

#[derive(Default)]
pub struct RegionPicker {
    texture: Option<(usize, TextureHandle)>,
    sel: Option<Rect>,
    drag_start: Option<Pos2>,
    scale: f32,
    offset: Vec2,
    need_fit: bool,
}

const UV_FULL: Rect = Rect { min: Pos2::ZERO, max: Pos2 { x: 1.0, y: 1.0 } };

impl RegionPicker {
    pub fn show(&mut self, ctx: &egui::Context, ctl: &Ctl) {
        let pick = ctl.with(|s| match &s.region_pick {
            RegionPick::Picking { img, origin } => Some((img.clone(), *origin)),
            _ => None,
        });
        let Some((img, origin)) = pick else {
            self.texture = None;
            self.sel = None;
            return;
        };
        let key = Arc::as_ptr(&img) as usize;
        if self.texture.as_ref().map(|(k, _)| *k) != Some(key) {
            let ci = egui::ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                img.as_raw(),
            );
            self.texture = Some((key, ctx.load_texture("region-shot", ci, egui::TextureOptions::NEAREST)));
            self.sel = None;
            self.drag_start = None;
            self.need_fit = true;
        }
        let tex = self.texture.as_ref().unwrap().1.clone();
        let img_size = Vec2::new(img.width() as f32, img.height() as f32);

        let mut done: Option<Option<[i32; 4]>> = None;
        let vp = egui::ViewportBuilder::default()
            .with_title(t!("picker.title"))
            .with_inner_size([1400.0, 850.0])
            .with_maximized(true);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("region-pick"), vp, |ctx, _| {
            let (enter, esc) = ctx.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if esc || ctx.input(|i| i.viewport().close_requested()) {
                done = Some(None);
            }
            let valid = self.sel.is_some_and(|r| r.width() >= 5.0 && r.height() >= 5.0);
            let region = |r: Rect| {
                [r.min.x as i32 + origin.0, r.min.y as i32 + origin.1, r.width() as i32, r.height() as i32]
            };

            egui::TopBottomPanel::top("rp-top").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(t!("picker.instructions"))
                    .strong(),
                );
                ui.label(egui::RichText::new(t!("picker.pan_hint")).weak());
                ui.horizontal(|ui| {
                    if ui.button(t!("picker.fit")).clicked() {
                        self.need_fit = true;
                    }
                    if ui.button("1:1").clicked() {
                        self.scale = 1.0;
                    }
                    ui.label(t!("picker.zoom", pct = format!("{:.0}", self.scale * 100.0)));
                    ui.separator();
                    if ui.add_enabled(valid, egui::Button::new(t!("picker.save"))).clicked() {
                        done = Some(Some(region(self.sel.unwrap())));
                    }
                    if ui.button(t!("picker.cancel")).clicked() {
                        done = Some(None);
                    }
                });
                ui.add_space(4.0);
            });

            egui::SidePanel::right("rp-preview").exact_width(340.0).show(ctx, |ui| {
                ui.heading(t!("picker.selected"));
                match self.sel {
                    Some(r) if r.width() >= 1.0 && r.height() >= 1.0 => {
                        let [x, y, w, h] = region(r);
                        ui.label(format!("X={x}  Y={y}  W={w}  H={h}"));
                        let avail = ui.available_width();
                        let k = (avail / r.width()).min(240.0 / r.height()).min(8.0);
                        let (pr, _) = ui.allocate_exact_size(r.size() * k, Sense::hover());
                        let uv = Rect::from_min_max(
                            (r.min.to_vec2() / img_size).to_pos2(),
                            (r.max.to_vec2() / img_size).to_pos2(),
                        );
                        ui.painter().image(tex.id(), pr, uv, Color32::WHITE);
                        ui.painter().rect_stroke(pr, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(0, 255, 0)));
                        ui.label(t!("picker.magnified", k = format!("{k:.1}")));
                        if !valid {
                            ui.colored_label(Color32::YELLOW, t!("picker.too_small"));
                        } else {
                            ui.label(t!("picker.ok_hint"));
                        }
                    }
                    _ => {
                        ui.label(t!("picker.none"));
                    }
                }
            });

            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(Color32::from_gray(20)))
                .show(ctx, |ui| {
                    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
                    let fit = (rect.width() / img_size.x).min(rect.height() / img_size.y);
                    if self.need_fit || self.scale <= 0.0 {
                        self.scale = fit;
                        self.offset = (img_size - rect.size() / fit) / 2.0;
                        self.need_fit = false;
                    }
                    let to_img = |p: Pos2, scale: f32, offset: Vec2| offset + (p - rect.min) / scale;
                    let clamp = |v: Vec2| {
                        Pos2::new(v.x.clamp(0.0, img_size.x).round(), v.y.clamp(0.0, img_size.y).round())
                    };

                    if let Some(hover) = resp.hover_pos() {
                        let (scroll, zoom) = ui.input(|i| (i.raw_scroll_delta.y, i.zoom_delta()));
                        let factor = (scroll * 0.002).exp() * zoom;
                        if (factor - 1.0).abs() > 1e-4 {
                            let anchor = to_img(hover, self.scale, self.offset);
                            self.scale = (self.scale * factor).clamp(fit * 0.5, 16.0);
                            self.offset = anchor - (hover - rect.min) / self.scale;
                        }
                    }
                    if resp.dragged_by(PointerButton::Secondary) || resp.dragged_by(PointerButton::Middle) {
                        self.offset -= resp.drag_delta() / self.scale;
                    }
                    if resp.drag_started_by(PointerButton::Primary) {
                        let origin_pt = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
                        self.drag_start = origin_pt.map(|p| clamp(to_img(p, self.scale, self.offset)));
                    }
                    if resp.dragged_by(PointerButton::Primary) {
                        if let (Some(a), Some(p)) = (self.drag_start, resp.interact_pointer_pos()) {
                            self.sel = Some(Rect::from_two_pos(a, clamp(to_img(p, self.scale, self.offset))));
                        }
                    }
                    if resp.drag_stopped() {
                        self.drag_start = None;
                    }

                    let painter = ui.painter_at(rect);
                    let img_rect = Rect::from_min_size(rect.min - self.offset * self.scale, img_size * self.scale);
                    painter.image(tex.id(), img_rect, UV_FULL, Color32::WHITE);
                    if let Some(r) = self.sel {
                        let sr = Rect::from_min_max(
                            rect.min + (r.min.to_vec2() - self.offset) * self.scale,
                            rect.min + (r.max.to_vec2() - self.offset) * self.scale,
                        );
                        painter.rect_filled(sr, 0.0, Color32::from_rgba_unmultiplied(0, 255, 0, 30));
                        painter.rect_stroke(sr.expand(1.0), 0.0, Stroke::new(2.0_f32, Color32::from_rgb(0, 255, 0)));
                    }
                    if resp.hovered() {
                        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                    }
                });

            if enter && valid {
                done = Some(Some(region(self.sel.unwrap())));
            }
        });

        if let Some(result) = done {
            ctl.with(|s| {
                if let Some(r) = result {
                    s.cfg.scanner.region = Some(r);
                    s.mark_dirty();
                    log::info!("Зона сканирования: {r:?}");
                }
                s.region_pick = RegionPick::Idle;
            });
        }
    }
}
