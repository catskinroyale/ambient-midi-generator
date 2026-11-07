//! The mixer panel: a strip per mixer channel with its parts, pan, width,
//! reverb and echo sends, fader, meter, mute and solo, and the master.

use std::time::Instant;

use eframe::egui::{self, Align2, Color32, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::mixer::Meters;
use crate::song::{Inst, MAX_STRIPS, Mix, SILENT_DB, Strip};
use crate::theme::Tokens;

const STRIP_W: f32 = 104.0;
const FADER_H: f32 = 96.0;
/// The top of the fader and meter scales.
const TOP_DB: f32 = 6.0;

/// The meters' falling levels, between frames.
pub struct MixerView {
    levels: [f32; MAX_STRIPS + 2],
    last: Instant,
}

impl Default for MixerView {
    fn default() -> Self {
        MixerView { levels: [0.0; MAX_STRIPS + 2], last: Instant::now() }
    }
}

/// What a part looks like on the mixer.
pub struct PartInfo {
    pub inst: Inst,
    pub color: Color32,
    pub sound: String,
}

impl MixerView {
    /// Whether any meter is still showing a level.
    pub fn active(&self) -> bool {
        self.levels.iter().any(|l| *l > 1e-4)
    }

    /// Read the peaks since the last frame; levels fall back at about 24 dB a second.
    fn update(&mut self, meters: &Meters) {
        let dt = self.last.elapsed().as_secs_f32().min(0.25);
        self.last = Instant::now();
        let fall = 10f32.powf(-24.0 * dt / 20.0);
        for (i, l) in self.levels.iter_mut().enumerate() {
            *l = meters.take(i).max(*l * fall);
        }
    }

    /// Returns true when the mix changed. Reset goes back to `reset`, the style's own mix.
    pub fn show(&mut self, ui: &mut Ui, mix: &mut Mix, reset: &Mix, parts: &[PartInfo], meters: &Meters) -> bool {
        self.update(meters);
        let before = mix.clone();
        let t = Tokens::get(ui.ctx());
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let mut remove = None;
                for s in 0..mix.strips.len() {
                    if self.strip(ui, mix, s, parts, &t) {
                        remove = Some(s);
                    }
                }
                if let Some(s) = remove {
                    mix.remove_strip(s);
                }
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    let can_add = mix.strips.len() < MAX_STRIPS;
                    if crate::widgets::text_button(ui, "+ Channel", false, can_add)
                        .on_hover_text("Add a channel, then move parts onto it to mix them together")
                        .clicked()
                        && can_add
                    {
                        mix.add_strip();
                    }
                    ui.add_space(4.0);
                    if crate::widgets::text_button(ui, "Reset", false, mix != reset)
                        .on_hover_text("Back to the style's own mix: a channel for each part, in the centre")
                        .clicked()
                    {
                        *mix = reset.clone();
                    }
                });
                ui.add_space(8.0);
                self.master(ui, mix, &t);
            });
        });
        *mix != before
    }

    /// One channel strip. Returns true when it should be removed.
    fn strip(&mut self, ui: &mut Ui, mix: &mut Mix, s: usize, parts: &[PartInfo], t: &Tokens) -> bool {
        let mut remove = false;
        let audible = mix.audible(s);
        let here: Vec<&PartInfo> = parts.iter().filter(|p| mix.strip_of(p.inst) == s).collect();
        let frame = egui::Frame::NONE
            .fill(t.inset)
            .stroke(Stroke::new(1.0, t.button_border.gamma_multiply(0.5)))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::same(6));
        frame.show(ui, |ui| ui.vertical(|ui| {
            ui.set_width(STRIP_W - 12.0);
            ui.spacing_mut().item_spacing.y = 3.0;
            // name, and a menu to rename, move parts here, or remove
            ui.horizontal(|ui| {
                let name = mix.strips[s].name.clone();
                let col = if audible { t.text } else { t.text_disabled };
                ui.add(egui::Label::new(egui::RichText::new(&name).font(t.semibold(12.0)).color(col)).truncate())
                    .on_hover_text(format!("Channel {}: {}", s + 1, name));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button(egui::RichText::new("⋯").font(t.font(13.0)), |ui| {
                        ui.set_min_width(190.0);
                        ui.label(egui::RichText::new("Name").small().weak());
                        ui.text_edit_singleline(&mut mix.strips[s].name);
                        ui.separator();
                        ui.label(egui::RichText::new("Parts on this channel").small().weak());
                        for p in parts {
                            let on = mix.strip_of(p.inst) == s;
                            let label = format!("{} ({})", p.inst.role(), p.sound);
                            if ui.selectable_label(on, label).on_hover_text("Play this part through this channel").clicked() && !on {
                                mix.set_route(p.inst, s);
                            }
                        }
                        ui.separator();
                        if ui.button("Reset channel").clicked() {
                            let name = mix.strips[s].name.clone();
                            mix.strips[s] = Strip::named(&name);
                            ui.close();
                        }
                        let can_remove = mix.strips.len() > 1;
                        if ui
                            .add_enabled(can_remove, egui::Button::new("Remove channel"))
                            .on_hover_text("Its parts move to the first channel")
                            .clicked()
                        {
                            remove = true;
                            ui.close();
                        }
                    });
                });
            });
            // the parts playing through it
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                if here.is_empty() {
                    ui.label(egui::RichText::new("no parts").font(t.font(10.5)).color(t.text_dim));
                }
                for p in &here {
                    let g = ui.painter().layout_no_wrap(p.inst.role().to_string(), t.font(10.0), t.text);
                    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 8.0, 15.0), Sense::hover());
                    ui.painter().rect_filled(r, 3.0, p.color.gamma_multiply(if audible { 0.45 } else { 0.18 }));
                    ui.painter().galley(r.center() - g.size() / 2.0, g, t.text);
                    resp.on_hover_text(format!("{}: {}", p.inst.role(), p.sound));
                }
            });
            let st = &mut mix.strips[s];
            knob(ui, "Pan", &mut st.pan, -1.0..=1.0, 0.0, pan_text, "Left or right. Double-click to centre.", t);
            knob(
                ui,
                "Width",
                &mut st.width,
                0.0..=2.0,
                1.0,
                |v| format!("{:.0}%", v * 100.0),
                "Stereo width: 0% is mono, 100% as played, 200% wider",
                t,
            );
            knob(
                ui,
                "Reverb",
                &mut st.reverb,
                0.0..=1.0,
                0.5,
                |v| format!("{:.0}%", v * 200.0),
                "Reverb send: 100% is the piece's reverb amount",
                t,
            );
            knob(ui, "Echo", &mut st.echo, 0.0..=1.0, 0.0, |v| format!("{:.0}%", v * 100.0), "Echo send", t);
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                fader(ui, &mut st.gain_db, t);
                meter(ui, &[self.levels[s]], audible, t);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    toggle(ui, "M", &mut st.mute, t.caution, "Mute this channel");
                    toggle(ui, "S", &mut st.solo, t.accent, "Solo: hear only the soloed channels");
                });
            });
            ui.label(egui::RichText::new(db_text(st.gain_db)).font(t.font(10.5)).color(t.text_dim));
        }));
        remove
    }

    fn master(&mut self, ui: &mut Ui, mix: &mut Mix, t: &Tokens) {
        let frame = egui::Frame::NONE
            .fill(t.inset)
            .stroke(Stroke::new(1.0, t.button_border))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::same(6));
        frame.show(ui, |ui| ui.vertical(|ui| {
            ui.set_width(70.0);
            ui.label(egui::RichText::new("Master").font(t.semibold(12.0)).color(t.text));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                fader(ui, &mut mix.master_db, t);
                meter(ui, &[self.levels[Meters::master(0)], self.levels[Meters::master(1)]], true, t);
            });
            ui.label(egui::RichText::new(db_text(mix.master_db)).font(t.font(10.5)).color(t.text_dim));
        }));
    }
}

fn pan_text(p: f32) -> String {
    let n = (p * 100.0).round() as i32;
    match n {
        0 => "C".into(),
        n if n < 0 => format!("L{}", -n),
        n => format!("R{n}"),
    }
}

fn db_text(db: f32) -> String {
    if db <= SILENT_DB { "−∞ dB".into() } else { format!("{db:+.1} dB") }
}

/// A one-line labelled slider: a thin track filled from its resting value,
/// with the value shown while hovered. Double-click resets it.
#[allow(clippy::too_many_arguments)]
fn knob(
    ui: &mut Ui,
    label: &str,
    v: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    default: f32,
    value: impl Fn(f32) -> String,
    help: &str,
    t: &Tokens,
) {
    let (lo, hi) = (*range.start(), *range.end());
    let w = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(w, 18.0), Sense::hover());
    let label_w = 44.0;
    let track = Rect::from_min_max(pos2(row.left() + label_w, row.center().y - 7.0), pos2(row.right() - 4.0, row.center().y + 7.0));
    let resp = ui.interact(track.expand2(vec2(4.0, 2.0)), ui.make_persistent_id(("knob", label, row.min.x as i32, row.min.y as i32)), Sense::click_and_drag());
    if resp.dragged() || resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let x = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
            *v = lo + x * (hi - lo);
        }
    }
    if resp.double_clicked() {
        *v = default;
    }
    let painter = ui.painter();
    let active = resp.hovered() || resp.dragged();
    let text = if active { value(*v) } else { label.to_string() };
    painter.text(pos2(row.left(), row.center().y), Align2::LEFT_CENTER, text, t.font(10.5), if active { t.text } else { t.text_label });
    let x_of = |x: f32| track.left() + track.width() * ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    let y = track.center().y;
    painter.hline(track.x_range(), y, Stroke::new(2.0, t.track));
    let (a, b) = (x_of(default), x_of(*v));
    painter.hline(a.min(b)..=a.max(b), y, Stroke::new(2.0, t.accent));
    painter.vline(a, (y - 3.0)..=(y + 3.0), Stroke::new(1.0, t.text_dim.gamma_multiply(0.6)));
    let col = if active { t.thumb_hover } else { t.thumb };
    painter.circle_filled(pos2(b, y), 4.0, col);
    resp.on_hover_text(format!("{label}: {} · {help}", value(*v)));
}

/// A vertical fader in dB. Double-click resets it to 0 dB.
fn fader(ui: &mut Ui, db: &mut f32, t: &Tokens) {
    let (r, resp) = ui.allocate_exact_size(vec2(18.0, FADER_H), Sense::click_and_drag());
    let y_of = |d: f32| r.bottom() - 6.0 - (r.height() - 12.0) * db_pos(d);
    if resp.dragged() || resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let x = ((r.bottom() - 6.0 - p.y) / (r.height() - 12.0)).clamp(0.0, 1.0);
            *db = pos_db(x);
        }
    }
    if resp.double_clicked() {
        *db = 0.0;
    }
    let painter = ui.painter();
    let track = Rect::from_center_size(pos2(r.center().x, r.center().y), vec2(3.0, r.height() - 12.0));
    painter.rect_filled(track, 1.5, t.track);
    let zero = y_of(0.0);
    painter.hline(r.x_range(), zero, Stroke::new(1.0, t.text_dim.gamma_multiply(0.6)));
    let y = y_of(*db);
    let cap = Rect::from_center_size(pos2(r.center().x, y), vec2(16.0, 9.0));
    let fill = if resp.hovered() || resp.dragged() { t.thumb_hover } else { t.thumb };
    painter.rect_filled(cap, 2.5, fill);
    painter.hline(cap.x_range().shrink(3.0), y, Stroke::new(1.0, t.inset));
    resp.on_hover_text(format!("{} · drag to set, double-click for 0 dB", db_text(*db)));
}

/// Fader travel: the top quarter covers -6 to +6 dB, the rest down to silence.
fn db_pos(db: f32) -> f32 {
    if db <= SILENT_DB {
        0.0
    } else if db >= -6.0 {
        0.75 + 0.25 * (db + 6.0) / (TOP_DB + 6.0)
    } else {
        0.75 * ((db - SILENT_DB) / (-6.0 - SILENT_DB)).powf(1.6)
    }
}

fn pos_db(x: f32) -> f32 {
    if x <= 0.0 {
        SILENT_DB
    } else if x >= 0.75 {
        -6.0 + (x - 0.75) / 0.25 * (TOP_DB + 6.0)
    } else {
        SILENT_DB + (x / 0.75).powf(1.0 / 1.6) * (-6.0 - SILENT_DB)
    }
}

/// Peak meters, one bar per level, on the fader's scale.
fn meter(ui: &mut Ui, levels: &[f32], active: bool, t: &Tokens) {
    let w = 5.0 * levels.len() as f32 + 2.0 * (levels.len() as f32 - 1.0);
    let (r, _) = ui.allocate_exact_size(vec2(w, FADER_H), Sense::hover());
    let painter = ui.painter();
    let span = r.height() - 12.0;
    for (i, l) in levels.iter().enumerate() {
        let bar = Rect::from_min_size(pos2(r.left() + i as f32 * 7.0, r.top() + 6.0), vec2(5.0, span));
        painter.rect_filled(bar, 1.0, t.canvas);
        let db = if *l > 1e-6 { 20.0 * l.log10() } else { SILENT_DB };
        let h = span * db_pos(db);
        if h > 0.5 {
            let col = if db > -0.5 {
                Color32::from_rgb(232, 84, 72)
            } else if db > -6.0 {
                t.caution
            } else {
                Color32::from_rgb(92, 184, 120)
            };
            let col = if active { col } else { col.gamma_multiply(0.35) };
            painter.rect_filled(Rect::from_min_max(pos2(bar.left(), bar.bottom() - h), bar.max), 1.0, col);
        }
        painter.rect_stroke(bar, 1.0, Stroke::new(1.0, t.button_border.gamma_multiply(0.5)), StrokeKind::Outside);
    }
}

fn toggle(ui: &mut Ui, label: &str, on: &mut bool, color: Color32, help: &str) {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 20.0), Sense::click());
    let fill = if *on { color } else if resp.hovered() { t.hover } else { t.button };
    ui.painter().rect_filled(r, 3.0, fill);
    ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    let text = if *on { Color32::WHITE } else { t.text_label };
    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, t.semibold(11.0), text);
    if resp.clicked() {
        *on = !*on;
    }
    resp.on_hover_text(help);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fader_scale_round_trips() {
        for db in [-60.0, -40.0, -12.0, -6.0, -3.0, 0.0, 4.5, 6.0] {
            assert!((pos_db(db_pos(db)) - db).abs() < 1e-3, "{db}");
        }
        assert!(db_pos(0.0) > 0.8 && db_pos(0.0) < 0.9);
    }
}
