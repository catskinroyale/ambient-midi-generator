//! The egui front end.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

use crate::generator::generate;
use crate::midi_export;
use crate::player::{Cmd, Player, Timeline, render_wav};
use crate::rng::Rng;
use crate::song::{Inst, Instrumentation, LengthMode, Params, SOUNDS, Song, Style, sound_name};
use crate::synth::{BuiltinSynth, SoundFontSynth, SynthEngine};
use crate::theory::{Mode, NOTE_NAMES};

struct Export {
    progress: Arc<AtomicU32>,
    handle: Option<std::thread::JoinHandle<Result<(), String>>>,
    path: PathBuf,
}

pub struct App {
    params: Params,
    generated_from: Params,
    song: Song,
    timeline: Arc<Timeline>,
    player: Option<Player>,
    audio_error: Option<String>,
    soundfont: Option<(String, Arc<rustysynth::SoundFont>)>,
    auto_regen: bool,
    looping: bool,
    volume: f32,
    recipe_text: String,
    status: String,
    export: Option<Export>,
    seed_history: Vec<u64>,
}

fn time_seed() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
}

/// The first .sf2 in a `soundfonts` folder in the working directory or next to the
/// executable (or the project root, when run from target/release).
fn default_soundfont() -> Option<PathBuf> {
    let mut dirs = vec![PathBuf::from("soundfonts")];
    if let Ok(exe) = std::env::current_exe() {
        dirs.extend(exe.ancestors().skip(1).take(3).map(|d| d.join("soundfonts")));
        // inside a macOS .app bundle: Contents/Resources/soundfonts
        if let Some(contents) = exe.parent().and_then(|d| d.parent()) {
            dirs.insert(0, contents.join("Resources/soundfonts"));
        }
    }
    dirs.into_iter().find_map(|d| {
        let mut fonts: Vec<PathBuf> = std::fs::read_dir(d)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sf2")))
            .collect();
        fonts.sort();
        fonts.into_iter().next()
    })
}

fn fmt_time(s: f64) -> String {
    let s = s.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn inst_color(i: Inst) -> Color32 {
    match i {
        Inst::Piano => Color32::from_rgb(236, 196, 120),
        Inst::StringPad => Color32::from_rgb(106, 186, 204),
        Inst::Violin => Color32::from_rgb(186, 156, 236),
        Inst::Cello => Color32::from_rgb(222, 128, 150),
        Inst::Pizzicato => Color32::from_rgb(150, 212, 140),
    }
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let params = Params::default();
        let song = generate(&params);
        let timeline = Arc::new(Timeline::from_song(&song));
        let (player, audio_error) =
            match Player::new(|sr| Box::new(BuiltinSynth::new(sr)) as Box<dyn SynthEngine>) {
                Ok(p) => (Some(p), None),
                Err(e) => (None, Some(e)),
            };
        if let Some(p) = &player {
            p.send(Cmd::SetTimeline(timeline.clone(), 0.0));
        }
        let mut app = App {
            recipe_text: params.to_recipe(),
            generated_from: params.clone(),
            params,
            song,
            timeline,
            player,
            audio_error,
            soundfont: None,
            auto_regen: true,
            looping: false,
            volume: 0.9,
            status: String::new(),
            export: None,
            seed_history: Vec::new(),
        };
        if let Some(path) = default_soundfont() {
            app.load_soundfont_from(&path);
        }
        app
    }

    fn regenerate(&mut self) {
        self.song = generate(&self.params);
        self.timeline = Arc::new(Timeline::from_song(&self.song));
        self.generated_from = self.params.clone();
        self.recipe_text = self.params.to_recipe();
        if let Some(p) = &self.player {
            let pos = p.position().min(self.timeline.duration - 0.5).max(0.0);
            p.send(Cmd::SetTimeline(self.timeline.clone(), pos));
        }
        self.status = format!(
            "Generated {} notes · {} bars · {}",
            self.song.notes.len(),
            self.song.bars,
            fmt_time(self.song.duration_secs())
        );
    }

    fn new_seed(&mut self) {
        self.seed_history.push(self.params.seed);
        self.params.seed = Rng::new(time_seed()).next_u64() % 1_000_000_000;
    }

    fn surprise(&mut self) {
        let mut r = Rng::new(time_seed());
        let p = &mut self.params;
        self.seed_history.push(p.seed);
        p.seed = r.next_u64() % 1_000_000_000;
        p.style = *r.pick(&Style::ALL);
        p.root = r.range(0, 11) as u8;
        p.mode = *r.pick(&Mode::ALL[..5]);
        p.instrumentation = *r.pick(&[
            Instrumentation::Piano,
            Instrumentation::Strings,
            Instrumentation::PianoAndStrings,
            Instrumentation::PianoAndStrings,
        ]);
        p.tempo = (p.style.default_tempo() * r.range_f(0.85, 1.15) as f32).round();
        p.density = r.range_f(0.25, 0.8) as f32;
        p.variation = r.range_f(0.1, 0.8) as f32;
        p.dynamics = r.range_f(0.3, 0.9) as f32;
        p.humanize = r.range_f(0.2, 0.6) as f32;
    }

    fn engine_for(&self, sr: u32) -> Result<Box<dyn SynthEngine>, String> {
        Ok(match &self.soundfont {
            Some((_, font)) => Box::new(SoundFontSynth::new(font, sr)?),
            None => Box::new(BuiltinSynth::new(sr)),
        })
    }

    fn load_soundfont(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("SoundFont", &["sf2"]).pick_file() {
            self.load_soundfont_from(&path);
        }
    }

    fn load_soundfont_from(&mut self, path: &std::path::Path) {
        let res = std::fs::File::open(path)
            .map_err(|e| e.to_string())
            .and_then(|mut f| rustysynth::SoundFont::new(&mut f).map_err(|e| format!("{e:?}")));
        match res {
            Ok(font) => {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                self.soundfont = Some((name.clone(), Arc::new(font)));
                self.status = format!("Loaded SoundFont {name}");
                if let Some(p) = &self.player {
                    match self.engine_for(p.sample_rate) {
                        Ok(e) => p.send(Cmd::SetEngine(e)),
                        Err(e) => self.status = e,
                    }
                }
            }
            Err(e) => self.status = format!("Could not load SoundFont: {e}"),
        }
    }

    fn use_builtin(&mut self) {
        self.soundfont = None;
        if let Some(p) = &self.player {
            p.send(Cmd::SetEngine(Box::new(BuiltinSynth::new(p.sample_rate))));
        }
        self.status = "Using built-in synth".into();
    }

    fn save_midi(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("MIDI", &["mid"])
            .set_file_name(format!("{}.mid", self.params.file_stem()))
            .save_file()
        else {
            return;
        };
        self.status = match midi_export::save(&self.song, &path) {
            Ok(()) => format!("Saved {}", path.display()),
            Err(e) => format!("Save failed: {e}"),
        };
    }

    fn open_midi_recipe(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("MIDI", &["mid", "midi"]).pick_file() else { return };
        match midi_export::read_recipe(&path) {
            Ok(p) => {
                self.params = p;
                self.regenerate();
                self.status = format!("Recreated from {}", path.display());
            }
            Err(e) => self.status = e,
        }
    }

    fn export_wav(&mut self) {
        if self.export.is_some() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("WAV audio", &["wav"])
            .set_file_name(format!("{}.wav", self.params.file_stem()))
            .save_file()
        else {
            return;
        };
        let engine = match self.engine_for(44100) {
            Ok(e) => e,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        let progress = Arc::new(AtomicU32::new(0));
        let song = self.song.clone();
        let (pr, p2) = (progress.clone(), path.clone());
        let handle = std::thread::spawn(move || render_wav(&song, engine, 44100, &p2, &pr));
        self.export = Some(Export { progress, handle: Some(handle), path });
    }

    // ------------------------------------------------------------------ ui

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.heading("Ambient Generator");
        ui.add_space(4.0);

        egui::Grid::new("params").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
            let p = &mut self.params;
            ui.label("Style");
            let before = p.style;
            egui::ComboBox::from_id_salt("style").width(190.0).selected_text(p.style.label()).show_ui(ui, |ui| {
                for s in Style::ALL {
                    ui.selectable_value(&mut p.style, s, s.label()).on_hover_text(s.description());
                }
            });
            if p.style != before {
                p.tempo = p.style.default_tempo();
            }
            ui.end_row();

            ui.label("");
            ui.add(egui::Label::new(egui::RichText::new(p.style.description()).small().weak()).wrap());
            ui.end_row();

            ui.label("Key");
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("root").width(50.0).selected_text(NOTE_NAMES[p.root as usize]).show_ui(
                    ui,
                    |ui| {
                        for (i, n) in NOTE_NAMES.iter().enumerate() {
                            ui.selectable_value(&mut p.root, i as u8, *n);
                        }
                    },
                );
                egui::ComboBox::from_id_salt("mode").width(130.0).selected_text(p.mode.label()).show_ui(ui, |ui| {
                    for m in Mode::ALL {
                        ui.selectable_value(&mut p.mode, m, m.label());
                    }
                });
            });
            ui.end_row();

            ui.label("Instruments");
            ui.horizontal(|ui| {
                for i in [Instrumentation::Piano, Instrumentation::Strings, Instrumentation::PianoAndStrings] {
                    ui.selectable_value(&mut p.instrumentation, i, i.label());
                }
            });
            ui.end_row();

            ui.label("Tempo");
            ui.add(egui::Slider::new(&mut p.tempo, 40.0..=120.0).suffix(" bpm").step_by(1.0));
            ui.end_row();

            ui.label("Length by");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut p.length_mode, LengthMode::Seconds, "Duration");
                ui.selectable_value(&mut p.length_mode, LengthMode::Bars, "Bars");
            });
            ui.end_row();

            match p.length_mode {
                LengthMode::Seconds => {
                    ui.label("Target length");
                    ui.add(
                        egui::Slider::new(&mut p.target_seconds, 20.0..=1800.0)
                            .logarithmic(true)
                            .step_by(5.0)
                            .custom_formatter(|v, _| fmt_time(v))
                            .custom_parser(|s| {
                                let mut it = s.split(':');
                                let a: f64 = it.next()?.trim().parse().ok()?;
                                match it.next() {
                                    Some(b) => Some(a * 60.0 + b.trim().parse::<f64>().ok()?),
                                    None => Some(a),
                                }
                            }),
                    );
                }
                LengthMode::Bars => {
                    ui.label("Target bars");
                    ui.add(egui::Slider::new(&mut p.target_bars, 4..=600).logarithmic(true));
                }
            }
            ui.end_row();

            let slider = |ui: &mut egui::Ui, label: &str, v: &mut f32, hint: &str| {
                ui.label(label).on_hover_text(hint);
                ui.add(egui::Slider::new(v, 0.0..=1.0).fixed_decimals(2)).on_hover_text(hint);
                ui.end_row();
            };
            slider(ui, "Density", &mut p.density, "How many notes: sparse to busy");
            slider(ui, "Variation", &mut p.variation, "Harmonic substitutions, melodic leaps, evolving loops");
            slider(ui, "Dynamics", &mut p.dynamics, "How much the piece builds towards a peak and recedes");
            slider(ui, "Humanize", &mut p.humanize, "Timing and velocity looseness");
            slider(ui, "Reverb", &mut p.reverb, "Reverb send (CC91) written to the MIDI file");

            ui.label("Seed");
            ui.horizontal(|ui| {
                if ui.small_button("◀").on_hover_text("Previous seed").clicked() {
                    p.seed = p.seed.saturating_sub(1);
                }
                ui.add(egui::DragValue::new(&mut p.seed).speed(1.0));
                if ui.small_button("▶").on_hover_text("Next seed").clicked() {
                    p.seed = p.seed.wrapping_add(1);
                }
            });
            ui.end_row();
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("🎲 New seed").on_hover_text("Same settings, new random material").clicked() {
                self.new_seed();
            }
            if ui.button("✨ Surprise me").on_hover_text("Randomise every setting").clicked() {
                self.surprise();
            }
            if !self.seed_history.is_empty() && ui.button("↶ Undo").on_hover_text("Back to the previous seed").clicked()
            {
                if let Some(s) = self.seed_history.pop() {
                    self.params.seed = s;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.auto_regen, "Regenerate on change");
            if !self.auto_regen || self.params != self.generated_from {
                if ui.button("Generate").clicked() {
                    self.regenerate();
                }
            }
        });

        ui.add_space(8.0);
        ui.separator();
        self.parts_ui(ui);

        ui.add_space(8.0);
        ui.separator();
        ui.label(egui::RichText::new("Recipe").strong())
            .on_hover_text("These settings + seed always reproduce exactly the same piece.");
        ui.add(
            egui::TextEdit::multiline(&mut self.recipe_text)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Monospace),
        );
        ui.horizontal(|ui| {
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(self.params.to_recipe());
                self.status = "Recipe copied to clipboard".into();
            }
            if ui.button("Apply").clicked() {
                match Params::from_recipe(&self.recipe_text) {
                    Ok(p) => {
                        self.params = p;
                        self.regenerate();
                    }
                    Err(e) => self.status = format!("Invalid recipe: {e}"),
                }
            }
            if ui.button("From .mid…").on_hover_text("Recreate from a MIDI file saved by this app").clicked() {
                self.open_midi_recipe();
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.label(egui::RichText::new("Sound").strong());
        ui.horizontal(|ui| {
            let name = self.soundfont.as_ref().map(|s| s.0.clone()).unwrap_or_else(|| "Built-in synth".into());
            ui.label(name);
        });
        ui.horizontal(|ui| {
            if ui.button("Load SoundFont (.sf2)…").on_hover_text("Any General MIDI SoundFont, for realistic instruments").clicked() {
                self.load_soundfont();
            }
            if self.soundfont.is_some() && ui.button("Built-in").clicked() {
                self.use_builtin();
            }
        });
    }

    /// Per-part on/off switches and sound choice.
    fn parts_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Parts").strong()).on_hover_text(
            "Switch parts on or off and choose the sound each one plays. \
             Switching a part off leaves the other parts exactly as they were.",
        );
        let parts = self.song.parts.clone();
        egui::Grid::new("parts").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            for inst in Inst::ALL {
                let used = parts.contains(&inst);
                let p = &mut self.params;
                ui.add_enabled_ui(used, |ui| {
                    ui.horizontal(|ui| {
                        let mut on = !p.muted.contains(&inst);
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                        let col = if used && on { inst_color(inst) } else { inst_color(inst).gamma_multiply(0.3) };
                        ui.painter().rect_filled(rect, 2.0, col);
                        let hint = if used {
                            inst.role_hint().to_string()
                        } else {
                            format!("{} — not used by this style / instrument choice", inst.role_hint())
                        };
                        if ui.checkbox(&mut on, inst.role()).on_hover_text(hint).changed() {
                            if on {
                                p.muted.retain(|m| *m != inst);
                            } else {
                                p.muted.push(inst);
                                p.muted.sort();
                            }
                        }
                    });
                });
                ui.add_enabled_ui(used, |ui| {
                    let current = p.sounds.get(inst);
                    egui::ComboBox::from_id_salt(("sound", inst))
                        .width(170.0)
                        .selected_text(sound_name(current))
                        .height(420.0)
                        .show_ui(ui, |ui| {
                            let mut family = "";
                            for snd in SOUNDS {
                                if snd.family != family {
                                    family = snd.family;
                                    ui.label(egui::RichText::new(family).small().weak());
                                }
                                ui.selectable_value(p.sounds.get_mut(inst), snd.program, snd.name);
                            }
                        });
                });
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            if ui.small_button("All on").clicked() {
                self.params.muted.clear();
            }
            if ui.small_button("Default sounds").clicked() {
                self.params.sounds = Default::default();
            }
        });
    }

    fn transport(&mut self, ui: &mut egui::Ui) {
        let pos = self.player.as_ref().map(|p| p.position()).unwrap_or(0.0);
        let playing = self.player.as_ref().map(|p| p.is_playing()).unwrap_or(false);
        let dur = self.song.duration_secs();
        ui.horizontal(|ui| {
            ui.add_enabled_ui(self.player.is_some(), |ui| {
                let label = if playing { "⏸ Pause" } else { "▶ Play" };
                if ui.add(egui::Button::new(egui::RichText::new(label).size(16.0)).min_size(Vec2::new(90.0, 28.0))).clicked()
                {
                    self.toggle_play();
                }
                if ui.add(egui::Button::new("⏹ Stop").min_size(Vec2::new(60.0, 28.0))).clicked() {
                    if let Some(p) = &self.player {
                        p.send(Cmd::Pause);
                        p.send(Cmd::Seek(0.0));
                    }
                }
                if ui.checkbox(&mut self.looping, "Loop").changed() {
                    if let Some(p) = &self.player {
                        p.send(Cmd::Looping(self.looping));
                    }
                }
                ui.label("Vol");
                if ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.5).show_value(false)).changed() {
                    if let Some(p) = &self.player {
                        p.send(Cmd::Volume(self.volume));
                    }
                }
            });
            ui.label(egui::RichText::new(format!("{} / {}", fmt_time(pos.min(dur)), fmt_time(dur))).monospace());
            ui.separator();
            if ui.button("💾 Save MIDI…").clicked() {
                self.save_midi();
            }
            let exporting = self.export.is_some();
            if ui.add_enabled(!exporting, egui::Button::new("🎧 Export WAV…")).clicked() {
                self.export_wav();
            }
            if let Some(ex) = &self.export {
                let f = ex.progress.load(Ordering::Relaxed) as f32 / 1000.0;
                ui.add(egui::ProgressBar::new(f).desired_width(120.0).show_percentage());
            }
        });
        if let Some(e) = &self.audio_error {
            ui.colored_label(Color32::LIGHT_RED, format!("Audio unavailable ({e}) — you can still save MIDI files."));
        }
        if !self.status.is_empty() {
            ui.label(egui::RichText::new(&self.status).weak());
        }
    }

    fn toggle_play(&mut self) {
        if let Some(p) = &self.player {
            if p.is_playing() {
                p.send(Cmd::Pause);
            } else {
                p.send(Cmd::Play);
            }
        }
    }

    fn piano_roll(&mut self, ui: &mut egui::Ui) {
        let song = &self.song;
        let size = ui.available_size();
        let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let rect = resp.rect;
        let visuals = ui.visuals();
        let dark = visuals.dark_mode;
        painter.rect_filled(rect, 6.0, if dark { Color32::from_rgb(20, 22, 30) } else { Color32::from_rgb(244, 244, 248) });

        let header = 22.0;
        let footer = 18.0;
        let roll = Rect::from_min_max(rect.min + Vec2::new(0.0, header), rect.max - Vec2::new(0.0, footer));
        let total = song.total_ticks.max(1) as f32;
        let x_of = |t: u32| roll.left() + roll.width() * t as f32 / total;

        let (lo, hi) = song
            .notes
            .iter()
            .fold((127u8, 0u8), |(lo, hi), n| (lo.min(n.key), hi.max(n.key)));
        let (lo, hi) = if lo > hi { (48.0, 84.0) } else { (lo as f32 - 2.0, hi as f32 + 3.0) };
        let row_h = roll.height() / (hi - lo);
        let y_of = |k: u8| roll.bottom() - (k as f32 - lo) * row_h;

        // C lines
        let grid = if dark { Color32::from_white_alpha(10) } else { Color32::from_black_alpha(14) };
        for k in (lo as u8)..=(hi as u8) {
            if k % 12 == 0 {
                painter.hline(roll.x_range(), y_of(k), Stroke::new(1.0, grid));
                painter.text(
                    Pos2::new(roll.left() + 3.0, y_of(k) - 1.0),
                    Align2::LEFT_BOTTOM,
                    format!("C{}", k as i32 / 12 - 1),
                    FontId::proportional(9.0),
                    grid.gamma_multiply(6.0),
                );
            }
        }
        // bar lines
        let bt = song.bar_ticks();
        let px_per_bar = roll.width() / (total / bt as f32);
        let every = if px_per_bar > 14.0 { 1 } else { ((14.0 / px_per_bar).ceil() as u32).next_power_of_two() };
        for b in (0..=song.bars).step_by(every as usize) {
            let x = x_of(b * bt);
            let strong = b % (every * 4) == 0;
            painter.vline(x, roll.y_range(), Stroke::new(1.0, if strong { grid.gamma_multiply(2.0) } else { grid }));
        }

        // chords
        let mut last_x = f32::NEG_INFINITY;
        let text_col = if dark { Color32::from_gray(170) } else { Color32::from_gray(70) };
        for (t, name) in &song.chords {
            let x = x_of(*t);
            if x - last_x > 46.0 {
                painter.text(Pos2::new(x + 2.0, rect.top() + 4.0), Align2::LEFT_TOP, name, FontId::proportional(11.0), text_col);
                last_x = x;
            }
        }

        // notes
        for n in &song.notes {
            let x0 = x_of(n.start);
            let x1 = x_of(n.end()).max(x0 + 1.5);
            let y = y_of(n.key);
            let alpha = 0.35 + 0.65 * (n.vel as f32 / 110.0).min(1.0);
            let c = inst_color(n.inst).gamma_multiply(alpha);
            painter.rect_filled(Rect::from_min_max(Pos2::new(x0, y - row_h * 0.9), Pos2::new(x1, y)), 1.5, c);
        }

        // legend
        let mut lx = rect.left() + 6.0;
        for i in song.parts.iter().copied() {
            let muted = song.params.muted.contains(&i);
            let col = if muted { inst_color(i).gamma_multiply(0.25) } else { inst_color(i) };
            painter.rect_filled(Rect::from_min_size(Pos2::new(lx, rect.bottom() - 13.0), Vec2::splat(9.0)), 2.0, col);
            let label = format!(
                "{}: {}{}",
                i.role(),
                sound_name(song.program(i)),
                if muted { " (off)" } else { "" }
            );
            let tc = if muted { text_col.gamma_multiply(0.5) } else { text_col };
            let g = painter.layout_no_wrap(label, FontId::proportional(11.0), tc);
            let w = g.size().x;
            painter.galley(Pos2::new(lx + 13.0, rect.bottom() - 15.0), g, text_col);
            lx += w + 28.0;
        }
        let info = format!(
            "{} · {} bpm · {}/{} · {} bars",
            song.params.title(),
            song.bpm,
            song.meter.0,
            song.meter.1,
            song.bars
        );
        painter.text(rect.right_bottom() - Vec2::new(6.0, 3.0), Align2::RIGHT_BOTTOM, info, FontId::proportional(11.0), text_col);

        // playhead + seeking
        if let Some(p) = &self.player {
            let pos = p.position();
            let tick = song.secs_to_tick(pos).min(song.total_ticks);
            let x = x_of(tick);
            painter.vline(x, roll.y_range(), Stroke::new(2.0, Color32::from_rgb(255, 120, 90)));
            if let Some(ptr) = resp.interact_pointer_pos() {
                if resp.clicked() || resp.dragged() {
                    let f = ((ptr.x - roll.left()) / roll.width()).clamp(0.0, 1.0);
                    p.send(Cmd::Seek(song.tick_to_secs((f * total) as u32)));
                }
            }
        }
        painter.rect_stroke(rect, 6.0, Stroke::new(1.0, grid), StrokeKind::Inside);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Space)) && ui.ctx().memory(|m| m.focused().is_none()) {
            self.toggle_play();
        }

        if let Some(ex) = &mut self.export {
            if ex.handle.as_ref().map(|h| h.is_finished()).unwrap_or(true) {
                let res = ex.handle.take().map(|h| h.join().unwrap_or(Err("export thread panicked".into())));
                self.status = match res {
                    Some(Ok(())) => format!("Exported {}", ex.path.display()),
                    Some(Err(e)) => format!("Export failed: {e}"),
                    None => String::new(),
                };
                self.export = None;
            }
        }

        egui::Panel::left("controls").resizable(false).exact_size(340.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
        });
        egui::Panel::bottom("transport").show(ui, |ui| {
            ui.add_space(6.0);
            self.transport(ui);
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show(ui, |ui| self.piano_roll(ui));

        // Wait until a slider drag is released so playback isn't constantly restarted.
        let dragging = ui.ctx().input(|i| i.pointer.any_down());
        if self.auto_regen && self.params != self.generated_from && !dragging {
            self.regenerate();
        }

        let busy = self.player.as_ref().map(|p| p.is_playing()).unwrap_or(false) || self.export.is_some();
        if busy {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
    }
}
