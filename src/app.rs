//! The egui front end.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, Vec2};

use crate::about::About;
use crate::theme::{self, Tokens};
use crate::widgets::{self, Icon};
use crate::fonts::{FontSource, LoadedFont};
use crate::project::{self, FontEntry, Project};
use crate::generator::generate;
use crate::midi_export;
use crate::player::{Cmd, Player, Timeline, render_wav};
use crate::rng::Rng;
use crate::slideshow::{self, Fit, Grid, Plan, Slideshow};
use crate::song::{
    ELECTRONIC_FONT, Fx, Inst, Instrumentation, LengthMode, Params, SOUNDS, Song, Sounds, Space, Style, Tuning, sound_name,
};
use crate::synth::{BuiltinSynth, MixSynth, SoundFontSynth, SynthEngine};
use crate::theory::{Mode, NOTE_NAMES};
use crate::update::{self, UpdateStatus};

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
    fonts: Vec<LoadedFont>,
    /// The SoundFont that plays every part without its own preset; None means
    /// the built-in synth.
    main_font: Option<String>,
    /// Bumped whenever a font is loaded or removed, so the engine is rebuilt.
    fonts_version: u32,
    engine_sig: Option<EngineSig>,
    assign: Option<Assign>,
    auto_regen: bool,
    looping: bool,
    volume: f32,
    recipe_text: String,
    status: String,
    export: Option<Export>,
    seed_history: Vec<u64>,
    /// Effects last sent to the player.
    sent_fx: Option<Fx>,
    theme: egui::ThemePreference,
    /// Whether the control panel is shown.
    show_controls: bool,
    /// Fitting a photo slideshow to the music.
    slideshow: Slideshow,
    about: About,
    #[cfg(target_os = "macos")]
    menu: Option<crate::menu::AppMenu>,
    /// Look for a newer release on GitHub at startup.
    check_updates: bool,
    update: Option<UpdateStatus>,
    update_rx: Option<std::sync::mpsc::Receiver<UpdateStatus>>,
    /// The SoundFont shipped with the app, which projects refer to rather than embed.
    bundled_fonts: Vec<PathBuf>,
    /// The project file last opened or saved.
    project_path: Option<PathBuf>,
    /// The window title last set.
    title: String,
    /// Set for automated UI checks (MIDIGEN_SCREENSHOT).
    snapshot: Option<crate::snapshot::Snapshot>,
}

/// Settings remembered between sessions.
const KEY_THEME: &str = "theme";
const KEY_CHECK_UPDATES: &str = "check_updates";
const KEY_SLIDESHOW: &str = "slideshow";


/// What the audio engine was built from: the main font, and which channels go
/// to which other font.
type EngineSig = (u32, Option<String>, Vec<(u8, String)>);

/// The "assign SoundFont to parts" dialog.
struct Assign {
    font: String,
    general_midi: bool,
    as_main: bool,
    rows: Vec<AssignRow>,
}

struct AssignRow {
    inst: Inst,
    on: bool,
    pick: (u8, u8),
    suggested: bool,
}

fn preset_label(bank: u8, preset: u8, name: &str) -> String {
    if bank == 0 { format!("{:03} {name}", preset + 1) } else { format!("{bank}:{:03} {name}", preset + 1) }
}

/// A drop-down of a font's presets. Returns true if the choice changed.
fn preset_combo(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, font: &LoadedFont, pick: &mut (u8, u8), width: f32) -> bool {
    let before = *pick;
    let text = font.find(pick.0, pick.1).map(|p| p.name.clone()).unwrap_or_else(|| "(missing preset)".into());
    egui::ComboBox::from_id_salt(id).width(width).height(420.0).truncate().selected_text(text).show_ui(ui, |ui| {
        for p in &font.presets {
            ui.selectable_value(pick, (p.bank, p.preset), preset_label(p.bank, p.preset, &p.name));
        }
    });
    *pick != before
}

fn time_seed() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
}

/// The .sf2 files in a `soundfonts` folder in the working directory or next to the
/// executable (or the project root, when run from target/release).
fn bundled_soundfonts() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("soundfonts")];
    if let Ok(exe) = std::env::current_exe() {
        dirs.extend(exe.ancestors().skip(1).take(3).map(|d| d.join("soundfonts")));
        // inside a macOS .app bundle: Contents/Resources/soundfonts
        if let Some(contents) = exe.parent().and_then(|d| d.parent()) {
            dirs.insert(0, contents.join("Resources/soundfonts"));
        }
    }
    dirs.into_iter()
        .find_map(|d| {
            let mut fonts: Vec<PathBuf> = std::fs::read_dir(d)
                .ok()?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sf2")))
                .collect();
            fonts.sort();
            (!fonts.is_empty()).then_some(fonts)
        })
        .unwrap_or_default()
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
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let storage = cc.storage;
        let theme = match storage.and_then(|s| eframe::get_value::<String>(s, KEY_THEME)).as_deref() {
            Some("system") => egui::ThemePreference::System,
            Some("light") => egui::ThemePreference::Light,
            _ => egui::ThemePreference::Dark,
        };
        let check_updates = storage.and_then(|s| eframe::get_value(s, KEY_CHECK_UPDATES)).unwrap_or(true);
        let slideshow = storage.and_then(|s| eframe::get_value(s, KEY_SLIDESHOW)).unwrap_or_default();
        theme::install(&cc.egui_ctx);
        cc.egui_ctx.set_theme(theme);
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
            fonts: Vec::new(),
            main_font: None,
            fonts_version: 0,
            engine_sig: None,
            assign: None,
            auto_regen: true,
            looping: false,
            volume: 0.9,
            status: String::new(),
            export: None,
            seed_history: Vec::new(),
            sent_fx: None,
            theme,
            show_controls: true,
            slideshow,
            about: About::default(),
            #[cfg(target_os = "macos")]
            menu: crate::menu::AppMenu::install(&cc.egui_ctx),
            check_updates,
            update: None,
            update_rx: None,
            bundled_fonts: bundled_soundfonts(),
            project_path: None,
            title: String::new(),
            snapshot: crate::snapshot::Snapshot::from_env(),
        };
        if let Some(s) = &app.snapshot {
            if s.wants("sections") {
                widgets::set_open(&cc.egui_ctx, &["piece", "parts", "fx", "fonts", "recipe"], true);
            }
            if s.wants("slideshow") {
                widgets::set_open(&cc.egui_ctx, &["piece", "parts", "fx"], false);
                widgets::set_open(&cc.egui_ctx, &["slideshow", "fonts", "recipe"], true);
            }
            if s.wants("fonts") {
                widgets::set_open(&cc.egui_ctx, &["piece", "slideshow", "parts", "fx"], false);
                widgets::set_open(&cc.egui_ctx, &["fonts", "recipe"], true);
            }
            if s.wants("images") {
                app.slideshow.fit = Fit::TrackToImages;
                app.slideshow.images = 50;
            }
            if s.wants("sound") {
                widgets::set_open(&cc.egui_ctx, &["piece", "recipe"], false);
                widgets::set_open(&cc.egui_ctx, &["parts", "fx", "fonts"], true);
            }

            app.about.open = s.wants("about") || s.wants("libraries");
            if s.wants("libraries") {
                app.about.scroll_to = Some(560.0);
                app.about.selected = Some(1);
            }
        }
        if check_updates {
            app.start_update_check(&cc.egui_ctx);
        }
        // every bundled font is loaded; a General MIDI one is the main sound
        for path in app.bundled_fonts.clone() {
            app.add_font(&path);
        }
        app.main_font = app
            .fonts
            .iter()
            .find(|f| f.is_general_midi())
            .or(app.fonts.first())
            .map(|f| f.name.clone());
        if !app.fonts.is_empty() {
            app.status = format!("Loaded {} SoundFonts", app.fonts.len());
        }
        if app.snapshot.as_ref().is_some_and(|s| s.wants("electronic")) {
            let old = app.params.style;
            app.params.style = Style::BerlinSchool;
            app.params.tempo = Style::BerlinSchool.default_tempo();
            app.apply_style_sounds(old);
            app.regenerate();
        }
        app.sync_engine();
        app
    }

    fn regenerate(&mut self) {
        // A recipe may name SoundFonts that aren't loaded here.
        let missing: Vec<String> = self
            .params
            .fonts
            .iter()
            .filter(|(_, f)| self.font(&f.font).is_none())
            .map(|(i, f)| format!("{} ({})", i.role(), f.font))
            .collect();
        self.params.fonts.retain(|(_, f)| self.fonts.iter().any(|l| l.name == f.font));
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
        if !missing.is_empty() {
            self.status = format!(
                "SoundFont not loaded for {}, so using the General MIDI sound instead",
                missing.join(", ")
            );
        }
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
        let old_style = p.style;
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
        self.apply_style_sounds(old_style);
    }

    fn font(&self, name: &str) -> Option<&LoadedFont> {
        self.fonts.iter().find(|f| f.name == name)
    }

    fn engine_signature(&self) -> EngineSig {
        let routes = self
            .song
            .params
            .fonts
            .iter()
            .filter(|(_, f)| Some(&f.font) != self.main_font.as_ref())
            .map(|(i, f)| (i.channel(), f.font.clone()))
            .collect();
        (self.fonts_version, self.main_font.clone(), routes)
    }

    /// The main engine, plus a SoundFont synth for each other font a part uses.
    fn engine_for(&self, sr: u32) -> Result<Box<dyn SynthEngine>, String> {
        let main: Box<dyn SynthEngine> = match self.main_font.as_deref().and_then(|n| self.font(n)) {
            Some(f) => Box::new(SoundFontSynth::new(&f.font, sr)?),
            None => Box::new(BuiltinSynth::new(sr)),
        };
        let mut mix = MixSynth::new(main);
        let mut added: Vec<(&str, usize)> = Vec::new();
        for (inst, fp) in &self.song.params.fonts {
            if Some(&fp.font) == self.main_font.as_ref() {
                continue; // the main font plays it, with the part's bank and program
            }
            let Some(f) = self.font(&fp.font) else { continue };
            let idx = match added.iter().find(|(n, _)| *n == fp.font) {
                Some((_, i)) => *i,
                None => {
                    let i = mix.add(Box::new(SoundFontSynth::new(&f.font, sr)?));
                    added.push((&fp.font, i));
                    i
                }
            };
            mix.route(inst.channel(), idx);
        }
        mix.set_fx(&self.params.fx);
        Ok(Box::new(mix))
    }

    /// Rebuild the playback engine if the fonts or their routing changed.
    fn sync_engine(&mut self) {
        let sig = self.engine_signature();
        if self.engine_sig.as_ref() == Some(&sig) {
            return;
        }
        self.engine_sig = Some(sig);
        if let Some(p) = &self.player {
            match self.engine_for(p.sample_rate) {
                Ok(e) => p.send(Cmd::SetEngine(e)),
                Err(e) => self.status = e,
            }
        }
    }

    /// Ask GitHub for the latest release, in the background.
    fn start_update_check(&mut self, ctx: &egui::Context) {
        if matches!(self.update, Some(UpdateStatus::Checking)) {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::check());
            ctx.request_repaint();
        });
        self.update = Some(UpdateStatus::Checking);
        self.update_rx = Some(rx);
    }

    fn poll_update_check(&mut self) {
        if let Some(rx) = &self.update_rx {
            if let Ok(status) = rx.try_recv() {
                self.update = Some(status);
                self.update_rx = None;
            }
        }
    }

    /// Send changed reverb and tuning settings to the player. They apply
    /// straight away, without regenerating.
    fn sync_fx(&mut self) {
        if self.sent_fx == Some(self.params.fx) {
            return;
        }
        self.sent_fx = Some(self.params.fx);
        self.recipe_text = self.params.to_recipe();
        if let Some(p) = &self.player {
            p.send(Cmd::SetFx(self.params.fx));
        }
    }

    fn load_soundfont(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("SoundFont", &["sf2"]).pick_file() {
            if let Some(name) = self.add_font(&path) {
                self.open_assign(&name);
            }
        }
    }

    /// Load a font (replacing one with the same file name) and return its name.
    fn add_font(&mut self, path: &std::path::Path) -> Option<String> {
        match LoadedFont::load(path) {
            Ok(font) => {
                let name = font.name.clone();
                self.status = format!("Loaded SoundFont {name} ({} presets)", font.presets.len());
                self.fonts.retain(|f| f.name != name);
                self.fonts.push(font);
                self.fonts_version += 1;
                Some(name)
            }
            Err(e) => {
                self.status = format!("Could not load SoundFont: {e}");
                None
            }
        }
    }

    fn remove_font(&mut self, name: &str) {
        self.fonts.retain(|f| f.name != name);
        if self.main_font.as_deref() == Some(name) {
            self.main_font = None;
        }
        self.params.fonts.retain(|(_, f)| f.font != name);
        self.fonts_version += 1;
        self.status = format!("Removed SoundFont {name}");
    }

    /// Open the dialog that assigns a font's presets to parts, pre-filled with
    /// suggestions.
    fn open_assign(&mut self, name: &str) {
        let Some(font) = self.font(name) else { return };
        let general_midi = font.is_general_midi();
        let rows = Inst::ALL
            .into_iter()
            .filter_map(|inst| {
                let current = self.params.font(inst).filter(|f| f.font == name).map(|f| (f.bank, f.preset));
                let (best, sure) = font.suggest(inst, self.params.sounds.get(inst))?;
                // A General MIDI font becomes the main sound instead, so parts
                // only get their own preset when the font has a clear match.
                let on = current.is_some() || (!general_midi && sure && self.song.parts.contains(&inst));
                let pick = current.unwrap_or((best.bank, best.preset));
                Some(AssignRow { inst, on, pick, suggested: sure && !general_midi })
            })
            .collect();
        self.assign = Some(Assign { font: name.to_string(), general_midi, as_main: general_midi, rows });
    }

    fn apply_assign(&mut self, a: Assign) {
        let Some(font) = self.font(&a.font) else { return };
        let mut changes = Vec::new();
        for row in &a.rows {
            let preset = font.find(row.pick.0, row.pick.1).map(|p| font.preset_for(p));
            if row.on {
                changes.push((row.inst, preset));
            } else if self.params.font(row.inst).is_some_and(|f| f.font == a.font) {
                changes.push((row.inst, None));
            }
        }
        for (inst, preset) in changes {
            self.params.set_font(inst, preset);
        }
        if a.as_main {
            self.main_font = Some(a.font.clone());
        }
        let parts = a.rows.iter().filter(|r| r.on).count();
        self.status = match (a.as_main, parts) {
            (true, 0) => format!("{} is now the main sound", a.font),
            (true, n) => format!("{} is now the main sound, with its own presets for {n} part(s)", a.font),
            (false, 0) => format!("{} is loaded. Choose it for a part under Parts", a.font),
            (false, n) => format!("{} now plays {n} part(s)", a.font),
        };
    }

    fn assign_window(&mut self, ctx: &egui::Context) {
        let Some(mut a) = self.assign.take() else { return };
        let Some(font) = self.font(&a.font) else { return };
        let parts = &self.song.parts;
        let (mut apply, mut keep) = (false, true);
        egui::Window::new("Assign SoundFont")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(&a.font).strong());
                ui.label(egui::RichText::new(format!("{} presets", font.presets.len())).weak());
                ui.add_space(4.0);
                if a.general_midi {
                    ui.label("This is a General MIDI SoundFont, so it has a sound for every part.");
                    ui.checkbox(&mut a.as_main, "Use it as the main sound")
                        .on_hover_text("Plays every part that isn't given its own preset below");
                    ui.label(
                        egui::RichText::new("Tick a part to give it a specific preset from this font.").small().weak(),
                    );
                } else {
                    ui.label("Choose the parts this SoundFont should play.");
                    ui.label(
                        egui::RichText::new("Parts are pre-ticked where a preset name matches the part.")
                            .small()
                            .weak(),
                    );
                }
                ui.add_space(6.0);
                egui::Grid::new("assign").num_columns(3).spacing([10.0, 6.0]).show(ui, |ui| {
                    for row in &mut a.rows {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                            ui.painter().rect_filled(rect, 2.0, inst_color(row.inst));
                            ui.checkbox(&mut row.on, row.inst.role()).on_hover_text(row.inst.role_hint());
                        });
                        if preset_combo(ui, ("assign", row.inst), font, &mut row.pick, 200.0) {
                            row.on = true;
                        }
                        let note = if !parts.contains(&row.inst) {
                            "not in this piece"
                        } else if row.suggested {
                            "suggested"
                        } else {
                            ""
                        };
                        ui.label(egui::RichText::new(note).small().weak());
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    apply = ui.button("Apply").clicked();
                    if ui.button("Cancel").on_hover_text("The SoundFont stays loaded, for use under Parts").clicked() {
                        keep = false;
                    }
                });
            });
        if apply {
            self.apply_assign(a);
        } else if keep {
            self.assign = Some(a);
        }
    }

    fn is_bundled(&self, font: &LoadedFont) -> bool {
        match &font.source {
            Some(FontSource::File(p)) => self.bundled_fonts.contains(p),
            _ => false,
        }
    }

    /// Save every setting and the loaded SoundFonts to a project file. Asks for
    /// a file name the first time, or always with `save_as`.
    fn save_project(&mut self, save_as: bool) {
        let path = match (&self.project_path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let name = self
                    .project_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("{}.{}", self.params.file_stem(), project::EXTENSION));
                let Some(p) = rfd::FileDialog::new()
                    .add_filter("Ambient project", &[project::EXTENSION])
                    .set_file_name(name)
                    .save_file()
                else {
                    return;
                };
                p
            }
        };
        let mut entries = Vec::new();
        let mut data = Vec::new();
        for f in &self.fonts {
            let path_text = match &f.source {
                Some(FontSource::File(p)) => Some(p.display().to_string()),
                _ => None,
            };
            let bundled = self.is_bundled(f);
            let embedded = if bundled {
                None
            } else {
                match f.bytes() {
                    Ok(b) => {
                        let n = b.len() as u64;
                        data.push(b);
                        Some(n)
                    }
                    Err(e) => {
                        self.status = format!("Could not save project: can't read SoundFont {e}");
                        return;
                    }
                }
            };
            entries.push(FontEntry { name: f.name.clone(), path: path_text, bundled, embedded });
        }
        let settings = project::Settings {
            volume: self.volume,
            looping: self.looping,
            auto_regen: self.auto_regen,
            slideshow: self.slideshow.clone(),
        };
        let proj = Project::new(self.params.clone(), self.main_font.clone(), entries, settings);
        let slices: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
        self.status = match project::save(&path, &proj, &slices) {
            Ok(()) => {
                let embedded = data.len();
                self.project_path = Some(path.clone());
                format!(
                    "Saved project {}{}",
                    path.display(),
                    if embedded > 0 { format!(" with {embedded} SoundFont(s) inside") } else { String::new() }
                )
            }
            Err(e) => format!("Could not save project: {e}"),
        };
    }

    fn open_project(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Ambient project", &[project::EXTENSION]).pick_file() else {
            return;
        };
        let (proj, data) = match project::load(&path) {
            Ok(x) => x,
            Err(e) => {
                self.status = format!("Could not open {}: {e}", path.display());
                return;
            }
        };
        let mut data = data.into_iter();
        let mut fonts = Vec::new();
        let mut missing = Vec::new();
        for entry in &proj.fonts {
            let loaded = if entry.embedded.is_some() {
                data.next().ok_or_else(|| "missing data".to_string()).and_then(|d| LoadedFont::from_bytes(entry.name.clone(), d))
            } else {
                // the bundled font from this copy of the app, else the original file
                let candidates = [
                    self.bundled_fonts
                        .iter()
                        .find(|p| entry.bundled && p.file_name().is_some_and(|n| n.to_string_lossy() == entry.name))
                        .cloned(),
                    entry.path.as_ref().map(PathBuf::from),
                ];
                candidates
                    .into_iter()
                    .flatten()
                    .filter(|p| p.exists())
                    .find_map(|p| LoadedFont::load(&p).ok())
                    .map(|mut f| {
                        f.name = entry.name.clone();
                        f
                    })
                    .ok_or_else(|| "file not found".to_string())
            };
            match loaded {
                Ok(f) => fonts.push(f),
                Err(e) => missing.push(format!("{} ({e})", entry.name)),
            }
        }
        self.fonts = fonts;
        self.fonts_version += 1;
        self.main_font = proj.main_font.filter(|n| self.fonts.iter().any(|f| &f.name == n));
        self.params = proj.params;
        self.volume = proj.settings.volume;
        self.looping = proj.settings.looping;
        self.auto_regen = proj.settings.auto_regen;
        self.slideshow = proj.settings.slideshow;
        if let Some(p) = &self.player {
            p.send(Cmd::Volume(self.volume));
            p.send(Cmd::Looping(self.looping));
        }
        self.project_path = Some(path.clone());
        self.regenerate();
        if missing.is_empty() {
            self.status = format!("Opened {}", path.display());
        } else {
            self.status = format!("Opened {}, but couldn't load SoundFont {}", path.display(), missing.join(", "));
        }
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
        widgets::section(ui, "piece", "Piece", true, |ui| self.piece_ui(ui));
        widgets::section(ui, "slideshow", "Slideshow", false, |ui| self.slideshow_ui(ui));
        widgets::section(ui, "parts", "Parts", true, |ui| self.parts_ui(ui));
        widgets::section(ui, "fx", "Reverb & Tuning", false, |ui| self.fx_ui(ui));
        widgets::section(ui, "fonts", "Sound Fonts", false, |ui| self.sound_ui(ui));
        widgets::section(ui, "recipe", "Recipe", false, |ui| self.recipe_ui(ui));
    }

    /// The bar above everything: the panel toggle, project files, theme and About.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if widgets::icon_button(ui, Icon::Sidebar, self.show_controls, "Show or hide the controls").clicked() {
                self.show_controls = !self.show_controls;
            }
            ui.separator();
            let cmd = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
            if widgets::text_button(ui, "Open…", false, true).on_hover_text(format!("Open a project ({cmd}O)")).clicked() {
                self.open_project();
            }
            if widgets::text_button(ui, "Save", false, true)
                .on_hover_text(format!("Save every setting and the loaded SoundFonts in one project file ({cmd}S)"))
                .clicked()
            {
                self.save_project(false);
            }
            if widgets::text_button(ui, "Save As…", false, true)
                .on_hover_text(format!("Save the project as a new file ({cmd}⇧S)"))
                .clicked()
            {
                self.save_project(true);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(ui, Icon::Info, self.about.open, "About, licences and updates").clicked() {
                    self.about.open = !self.about.open;
                }
                // One button steps through the themes: Auto → Light → Dark → Auto.
                let (icon, next, help) = match self.theme {
                    egui::ThemePreference::System => {
                        (Icon::Auto, egui::ThemePreference::Light, "Theme: Auto, following the system. Click for Light.")
                    }
                    egui::ThemePreference::Light => (Icon::Sun, egui::ThemePreference::Dark, "Theme: Light. Click for Dark."),
                    egui::ThemePreference::Dark => (Icon::Moon, egui::ThemePreference::System, "Theme: Dark. Click for Auto."),
                };
                if widgets::icon_button(ui, icon, false, help).clicked() {
                    self.theme = next;
                    ui.ctx().set_theme(next);
                }
                if let Some(UpdateStatus::Available { version, url }) = &self.update {
                    ui.separator();
                    let notice = egui::RichText::new(format!("Update: {version}")).color(ui.visuals().hyperlink_color);
                    ui.hyperlink_to(notice, url)
                        .on_hover_text(format!("Version {version} is available (you have {}). Click to download.", update::VERSION));
                }
            });
        });
    }

    fn piece_ui(&mut self, ui: &mut egui::Ui) {
        let slideshow_bars = (self.slideshow.fit == Fit::TrackToImages).then(|| self.slide_plan());
        let p = &mut self.params;
        let before = p.style;
        widgets::dropdown(ui, "style", "Style", p.style.label(), "The kind of piece", |ui| {
            for s in Style::ALL {
                ui.selectable_value(&mut p.style, s, s.label()).on_hover_text(s.description());
            }
        });
        let style_from = (p.style != before).then_some(before);
        if style_from.is_some() {
            p.tempo = p.style.default_tempo();
        }
        widgets::note(ui, p.style.description(), "");
        ui.add_space(4.0);
        widgets::dropdown(ui, "root", "Key", NOTE_NAMES[p.root as usize], "The key's root note", |ui| {
            for (i, n) in NOTE_NAMES.iter().enumerate() {
                ui.selectable_value(&mut p.root, i as u8, *n);
            }
        });
        widgets::dropdown(ui, "mode", "Mode", p.mode.label(), "The scale: brighter at the top, darker at the bottom", |ui| {
            for m in Mode::ALL {
                ui.selectable_value(&mut p.mode, m, m.label());
            }
        });
        widgets::subtitle(ui, "Instruments", "Each style re-orchestrates itself for the choice you make");
        let choices = [Instrumentation::Piano, Instrumentation::Strings, Instrumentation::PianoAndStrings];
        let segs: Vec<widgets::Segment> = choices
            .iter()
            .map(|i| widgets::Segment { label: i.label(), selected: p.instrumentation == *i, enabled: true, help: "" })
            .collect();
        if let Some(i) = widgets::segmented(ui, &segs, 3) {
            p.instrumentation = choices[i];
        }
        ui.add_space(4.0);
        let mut tempo = p.tempo as f64;
        let spec = widgets::SliderSpec {
            id: "tempo",
            label: "Tempo",
            help: "Beats per minute",
            min: 40.0,
            max: 120.0,
            default: p.style.default_tempo() as f64,
            step: 1.0,
            decimals: 0,
            suffix: " bpm",
            ends: Some(("slower", "faster")),
            ..Default::default()
        };
        if widgets::slider(ui, &spec, &mut tempo, true) {
            p.tempo = tempo as f32;
        }
        widgets::subtitle(ui, "Length", "Aim for a duration or a number of bars; the piece ends on the tonic");
        if let Some(plan) = slideshow_bars {
            widgets::note(
                ui,
                format!(
                    "Set by the Slideshow section: {} bars ({}) for {} images.",
                    plan.bars(),
                    fmt_time(plan.total),
                    plan.images
                ),
                "",
            );
        } else {
        let segs = [
            widgets::Segment { label: "Duration", selected: p.length_mode == LengthMode::Seconds, enabled: true, help: "" },
            widgets::Segment { label: "Bars", selected: p.length_mode == LengthMode::Bars, enabled: true, help: "" },
        ];
        match widgets::segmented(ui, &segs, 2) {
            Some(0) => p.length_mode = LengthMode::Seconds,
            Some(_) => p.length_mode = LengthMode::Bars,
            None => {}
        }
        match p.length_mode {
            LengthMode::Seconds => {
                let mut v = p.target_seconds as f64;
                let spec = widgets::SliderSpec {
                    id: "seconds",
                    label: "Target length",
                    help: "How long the piece should be (type m:ss)",
                    min: 20.0,
                    max: 1800.0,
                    default: 180.0,
                    step: 5.0,
                    log: true,
                    time: true,
                    ends: Some(("shorter", "longer")),
                    ..Default::default()
                };
                if widgets::slider(ui, &spec, &mut v, true) {
                    p.target_seconds = v as f32;
                }
            }
            LengthMode::Bars => {
                let mut v = p.target_bars as f64;
                let spec = widgets::SliderSpec {
                    id: "bars",
                    label: "Target bars",
                    help: "How many bars the piece should have",
                    min: 4.0,
                    max: 600.0,
                    default: 48.0,
                    step: 1.0,
                    decimals: 0,
                    log: true,
                    ends: Some(("fewer", "more")),
                    ..Default::default()
                };
                if widgets::slider(ui, &spec, &mut v, true) {
                    p.target_bars = v as u32;
                }
            }
        }
        }
        widgets::subtitle(ui, "Character", "");
        amount(ui, "density", "Density", &mut p.density, ("sparse", "busy"), "How many notes, in every part");
        amount(ui, "variation", "Variation", &mut p.variation, ("plain", "varied"), "Harmonic substitutions, melodic leaps, evolving loops");
        amount(ui, "dynamics", "Dynamics", &mut p.dynamics, ("even", "dramatic"), "How much the piece builds towards a peak and recedes");
        amount(ui, "humanize", "Humanize", &mut p.humanize, ("exact", "loose"), "Looseness of timing and velocity");

        widgets::subtitle(ui, "Seed", "The same settings and seed always give exactly the same piece");
        widgets::inset(ui, |ui| {
            ui.horizontal(|ui| {
                if widgets::text_button(ui, "◀", false, true).on_hover_text("Previous seed").clicked() {
                    self.params.seed = self.params.seed.saturating_sub(1);
                }
                ui.add(egui::DragValue::new(&mut self.params.seed).speed(1.0));
                if widgets::text_button(ui, "▶", false, true).on_hover_text("Next seed").clicked() {
                    self.params.seed = self.params.seed.wrapping_add(1);
                }
                if widgets::text_button(ui, "New", false, true).on_hover_text("Same settings, new random material").clicked() {
                    self.new_seed();
                }
                if widgets::text_button(ui, "Surprise me", false, true).on_hover_text("Randomise every setting").clicked() {
                    self.surprise();
                }
                if widgets::text_button(ui, "Undo", false, !self.seed_history.is_empty())
                    .on_hover_text("Back to the previous seed")
                    .clicked()
                {
                    if let Some(s) = self.seed_history.pop() {
                        self.params.seed = s;
                    }
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.auto_regen, "Regenerate on change");
                let stale = !self.params.same_music(&self.generated_from);
                if (!self.auto_regen || stale) && widgets::text_button(ui, "Generate", stale, true).clicked() {
                    self.regenerate();
                }
            });
        });
        if let Some(old) = style_from {
            self.apply_style_sounds(old);
        }
    }

    /// Where a slideshow's images fall: in the generated track, or in the track
    /// the images need.
    fn slide_plan(&self) -> Plan {
        match self.slideshow.fit {
            Fit::ImagesToTrack => {
                Plan::for_track(&self.slideshow, Grid::new(self.song.bpm, self.song.meter), self.song.duration_secs())
            }
            Fit::TrackToImages => Plan::for_images(
                &self.slideshow,
                Grid::new(self.params.tempo.clamp(30.0, 200.0) as f64, self.params.style.meter()),
            ),
        }
    }

    /// When the track is fitted to the images, set its length in bars.
    fn sync_slideshow_length(&mut self) {
        if self.slideshow.fit != Fit::TrackToImages {
            return;
        }
        // the generator adds the final ring-out bar itself
        let bars = self.slide_plan().bars().saturating_sub(1).max(4);
        if self.params.length_mode != LengthMode::Bars || self.params.target_bars != bars {
            self.params.length_mode = LengthMode::Bars;
            self.params.target_bars = bars;
        }
    }

    fn save_cue_list(&mut self, plan: &Plan) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("CSV", &["csv"])
            .set_file_name(format!("{}_slides.csv", self.params.file_stem()))
            .save_file()
        else {
            return;
        };
        self.status = match std::fs::write(&path, plan.cue_csv(self.slideshow.fade as f64)) {
            Ok(()) => format!("Saved cue list {}", path.display()),
            Err(e) => format!("Could not save cue list: {e}"),
        };
    }

    /// Fit a photo slideshow to the music: count the images the track holds, or
    /// size the track for a number of images.
    fn slideshow_ui(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.slideshow;
        let segs = [
            widgets::Segment {
                label: "Images for track",
                selected: s.fit == Fit::ImagesToTrack,
                enabled: true,
                help: "Count how many images the current track holds",
            },
            widgets::Segment {
                label: "Track for images",
                selected: s.fit == Fit::TrackToImages,
                enabled: true,
                help: "Set the track's length to hold a number of images",
            },
        ];
        match widgets::segmented(ui, &segs, 2) {
            Some(0) => s.fit = Fit::ImagesToTrack,
            Some(_) => s.fit = Fit::TrackToImages,
            None => {}
        }
        ui.add_space(4.0);
        if s.fit == Fit::TrackToImages {
            let mut v = s.images as f64;
            let spec = widgets::SliderSpec {
                id: "slide_images",
                label: "Images",
                help: "How many images the slideshow has; the track's length follows",
                min: 1.0,
                max: 500.0,
                default: 30.0,
                step: 1.0,
                decimals: 0,
                log: true,
                ends: Some(("fewer", "more")),
                ..Default::default()
            };
            if widgets::slider(ui, &spec, &mut v, true) {
                s.images = v as u32;
            }
        }
        let mut v = s.hold as f64;
        let spec = widgets::SliderSpec {
            id: "slide_hold",
            label: "Time per image",
            help: "How long you'd like each image on screen, fades included. It is matched to whole bars \
                   (or beats), between 4 and 8 seconds.",
            min: slideshow::MIN_HOLD,
            max: slideshow::MAX_HOLD,
            default: 6.0,
            step: 0.5,
            decimals: 1,
            suffix: " s",
            ends: Some(("brisk", "lingering")),
            ..Default::default()
        };
        if widgets::slider(ui, &spec, &mut v, true) {
            s.hold = v as f32;
        }
        let mut v = s.fade as f64;
        let spec = widgets::SliderSpec {
            id: "slide_fade",
            label: "Crossfade",
            help: "How long one image takes to fade into the next. It overlaps the time on screen.",
            min: 0.0,
            max: 2.0,
            default: 1.0,
            step: 0.1,
            decimals: 1,
            suffix: " s",
            ends: Some(("cut", "dissolve")),
            ..Default::default()
        };
        if widgets::slider(ui, &spec, &mut v, true) {
            s.fade = v as f32;
        }
        let mut v = s.intro_bars as f64;
        let spec = widgets::SliderSpec {
            id: "slide_intro",
            label: "Intro",
            help: "Bars of music before the first image, e.g. for a title",
            min: 0.0,
            max: 16.0,
            default: 2.0,
            step: 1.0,
            decimals: 0,
            suffix: " bars",
            ends: Some(("straight in", "long")),
            ..Default::default()
        };
        if widgets::slider(ui, &spec, &mut v, true) {
            s.intro_bars = v as u32;
        }
        let mut v = s.outro_bars as f64;
        let spec = widgets::SliderSpec {
            id: "slide_outro",
            label: "Outro",
            help: "Bars of music after the last image, e.g. for credits. The last bar is the music ringing out.",
            min: 1.0,
            max: 16.0,
            default: 2.0,
            step: 1.0,
            decimals: 0,
            suffix: " bars",
            ends: Some(("short", "long")),
            ..Default::default()
        };
        if widgets::slider(ui, &spec, &mut v, true) {
            s.outro_bars = v as u32;
        }

        let plan = self.slide_plan();
        let t = Tokens::get(ui.ctx());
        let s = &self.slideshow;
        widgets::subtitle(ui, "Plan", "");
        let mut copy = false;
        let mut save = false;
        let mut show = s.show_on_timeline;
        widgets::inset(ui, |ui| {
            let headline = match s.fit {
                Fit::ImagesToTrack => format!("{} images", plan.images),
                Fit::TrackToImages => format!("{} track", fmt_time(plan.total)),
            };
            ui.label(egui::RichText::new(headline).font(t.semibold(20.0)).color(t.text));
            ui.add_space(2.0);
            let fade = (s.fade as f64).min(plan.slot.secs / 2.0);
            let bar = plan.grid.bar();
            let bars = |secs: f64| {
                let n = secs / bar;
                if (n - n.round()).abs() < 1e-3 { format!("{} bars", n.round()) } else { format!("{n:.1} bars") }
            };
            let rows = [
                (
                    "Each image",
                    format!("{} · {:.1} s, {:.1} s clear of fades", plan.slot.label(), plan.slot.secs, plan.slot.secs - fade),
                ),
                ("Intro", format!("{} · {:.1} s", bars(plan.intro), plan.intro)),
                ("Images", format!("{} · {}", plan.images, fmt_time(plan.images as f64 * plan.slot.secs))),
                ("Outro", format!("{} · {:.1} s", bars(plan.outro), plan.outro)),
                (
                    "Track",
                    format!(
                        "{} · {} bars · {:.0} bpm in {}/{}",
                        fmt_time(plan.total),
                        plan.bars(),
                        plan.grid.bpm,
                        plan.grid.meter.0,
                        plan.grid.meter.1
                    ),
                ),
            ];
            for (k, v) in rows {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(84.0, 18.0), Sense::hover());
                    ui.painter().text(r.left_center(), Align2::LEFT_CENTER, k, t.font(12.0), t.text_dim);
                    ui.add(egui::Label::new(egui::RichText::new(v).font(t.font(12.0)).color(t.text)).truncate());
                });
            }
            if s.fit == Fit::ImagesToTrack && plan.images == 0 {
                widgets::note(ui, "The track is too short for an image between the intro and outro.", "");
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                copy = widgets::text_button(ui, "Copy cue list", false, plan.images > 0)
                    .on_hover_text("Copy each image's start and end time, as CSV, for your video editor")
                    .clicked();
                save = widgets::text_button(ui, "Save cue list…", false, plan.images > 0)
                    .on_hover_text("Save each image's start and end time as a CSV file")
                    .clicked();
            });
            ui.add_space(4.0);
            ui.checkbox(&mut show, "Show image changes on the timeline");
        });
        self.slideshow.show_on_timeline = show;
        if copy {
            ui.ctx().copy_text(plan.cue_csv(self.slideshow.fade as f64));
            self.status = format!("Copied the cue list for {} images", plan.images);
        }
        if save {
            self.save_cue_list(&plan);
        }
    }

    /// After a style change: switch to the new style's suggested sounds (the
    /// electronic styles play the bundled Ambient Electronic SoundFont), or back to
    /// the defaults when leaving a style that had its own.
    fn apply_style_sounds(&mut self, old: Style) {
        let new = self.params.style.suggested_sounds();
        if new.is_none() && old.suggested_sounds().is_none() {
            return;
        }
        let p = &mut self.params;
        p.fonts.retain(|(_, f)| f.font != ELECTRONIC_FONT);
        let Some(suggested) = new else {
            p.sounds = Sounds::default();
            return;
        };
        p.sounds = suggested.sounds;
        if !suggested.electronic {
            return;
        }
        let Some(font) = self.fonts.iter().find(|f| f.name == ELECTRONIC_FONT) else {
            self.status = format!("{ELECTRONIC_FONT} isn't loaded, so this style uses General MIDI synth sounds");
            return;
        };
        for inst in Inst::ALL {
            if let Some(preset) = font.find(0, p.sounds.get(inst)) {
                p.set_font(inst, Some(font.preset_for(preset)));
            }
        }
    }

    fn recipe_ui(&mut self, ui: &mut egui::Ui) {
        widgets::note(ui, "These settings and seed always reproduce exactly the same piece.", "");
        widgets::inset(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.recipe_text)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
            ui.horizontal(|ui| {
                if widgets::text_button(ui, "Copy", false, true).clicked() {
                    ui.ctx().copy_text(self.params.to_recipe());
                    self.status = "Recipe copied to clipboard".into();
                }
                if widgets::text_button(ui, "Apply", false, true).clicked() {
                    match Params::from_recipe(&self.recipe_text) {
                        Ok(p) => {
                            self.params = p;
                            self.regenerate();
                        }
                        Err(e) => self.status = format!("Invalid recipe: {e}"),
                    }
                }
                if widgets::text_button(ui, "From .mid…", false, true)
                    .on_hover_text("Recreate from a MIDI file saved by this app")
                    .clicked()
                {
                    self.open_midi_recipe();
                }
            });
        });
    }

    fn sound_ui(&mut self, ui: &mut egui::Ui) {
        let main = self.main_font.clone().unwrap_or_else(|| "Built-in synth".into());
        let fonts = &self.fonts;
        let main_font = &mut self.main_font;
        widgets::dropdown(ui, "main_font", "Main", &main, "Plays every part that doesn't have its own SoundFont preset", |ui| {
            ui.selectable_value(main_font, None, "Built-in synth");
            for f in fonts {
                ui.selectable_value(main_font, Some(f.name.clone()), &f.name);
            }
        });
        let mut assign = None;
        let mut remove = None;
        let mut load = false;
        widgets::inset(ui, |ui| {
            for f in &self.fonts {
                // buttons on the right; a long name is shortened to fit beside them. The row's
                // height is fixed: a right-to-left layout would otherwise fill all the height left.
                let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y + 4.0);
                ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::text_button(ui, "×", false, true).on_hover_text("Unload this SoundFont").clicked() {
                        remove = Some(f.name.clone());
                    }
                    if widgets::text_button(ui, "Assign…", false, true)
                        .on_hover_text("Choose which parts this SoundFont plays")
                        .clicked()
                    {
                        assign = Some(f.name.clone());
                    }
                    let kind = if f.is_general_midi() { "General MIDI" } else { "presets" };
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(&f.name).truncate())
                            .on_hover_text(format!("{}\n{} {kind}", f.name, f.presets.len()));
                    });
                });
            }
            ui.add_space(4.0);
            load = widgets::text_button(ui, "Load SoundFont (.sf2)…", false, true)
                .on_hover_text("Load a SoundFont, then choose which parts it plays. You can load several.")
                .clicked();
        });
        if load {
            self.load_soundfont();
        }
        if let Some(name) = assign {
            self.open_assign(&name);
        }
        if let Some(name) = remove {
            self.remove_font(&name);
        }
    }

    /// Reverb space and tuning. Apart from the reverb amount, which is written to
    /// the MIDI file, these change the sound without regenerating.
    fn fx_ui(&mut self, ui: &mut egui::Ui) {
        let p = &mut self.params;
        widgets::note(ui, "These change how the piece sounds, not its notes.", "");
        widgets::dropdown(ui, "space", "Space", p.fx.space.label(), "The kind of reverb", |ui| {
            for s in Space::ALL {
                ui.selectable_value(&mut p.fx.space, s, s.label()).on_hover_text(s.description());
            }
        });
        widgets::note(ui, p.fx.space.description(), "");
        ui.add_space(4.0);
        amount(ui, "reverb", "Amount", &mut p.reverb, ("dry", "wet"), "How much of each part goes into the reverb (also saved in the MIDI file)");
        amount(ui, "size", "Size", &mut p.fx.size, ("short", "long"), "How long the tail lasts, from half to double the space's natural length");
        amount(ui, "tone", "Tone", &mut p.fx.tone, ("dark", "bright"), "How bright the reverb is");
        amount(ui, "motion", "Motion", &mut p.fx.motion, ("still", "swirling"), "How much the tail slowly moves and shimmers in pitch");
        if p.fx.space == Space::Shimmer {
            amount(
                ui,
                "shimmer",
                "Feedback",
                &mut p.fx.shimmer,
                ("subtle", "intense"),
                "How much of the octave-up tail is fed back in: more climbs higher and leaves more shimmer artefacts",
            );
            amount(
                ui,
                "shimmer_detune",
                "Feedback detune",
                &mut p.fx.shimmer_detune,
                ("pure", "wide"),
                "Splits the shimmer feedback into a sharp and a flat copy, up to 35 cents either side of the octave, \
                 so they beat and spread for more depth",
            );
        }
        widgets::subtitle(ui, "Tuning", "");
        amount(
            ui,
            "detune",
            "Detune",
            &mut p.fx.detune,
            ("in tune", "detuned"),
            "Pulls each part and note slightly off pitch and widens ensemble sounds, for a warmer, chorused sound",
        );
        amount(ui, "drift", "Drift", &mut p.fx.drift, ("steady", "wobbly"), "Slow pitch wander, like an old tape or a worn record");
        widgets::dropdown(ui, "tuning", "Pitch", p.fx.tuning.label(), "The reference pitch for A above middle C", |ui| {
            for t in Tuning::ALL {
                ui.selectable_value(&mut p.fx.tuning, t, t.label());
            }
        });
        if p.fx != Fx::default() {
            widgets::inset(ui, |ui| {
                if widgets::text_button(ui, "Reset reverb & tuning", false, true).clicked() {
                    p.fx = Fx::default();
                }
            });
        }
    }

    /// Per-part on/off switches and sound choice.
    fn parts_ui(&mut self, ui: &mut egui::Ui) {
        let parts = self.song.parts.clone();
        let main_name = self.main_font.clone().unwrap_or_else(|| "Built-in synth".into());
        widgets::note(ui, "Switching a part off leaves the others exactly as they were.", "");
        ui.add_space(4.0);
        widgets::inset(ui, |ui| {
            // share the panel's width between the font and sound drop-downs, so a
            // long font or preset name is shortened rather than overflowing
            let rest = (ui.available_width() - 100.0).max(200.0);
            let source_w = (rest * 0.38).clamp(70.0, 220.0);
            let sound_w = rest - source_w - 16.0;
            egui::Grid::new("parts").num_columns(3).spacing([6.0, 6.0]).show(ui, |ui| {
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
                    let fonts = &self.fonts;
                    let font = p.font(inst).and_then(|fp| fonts.iter().find(|f| f.name == fp.font));
                    ui.add_enabled_ui(used, |ui| {
                        let source = font.map_or("Main", |f| f.name.as_str());
                        egui::ComboBox::from_id_salt(("source", inst))
                            .width(source_w)
                            .truncate()
                            .selected_text(source)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(font.is_none(), "Main")
                                    .on_hover_text(format!("A General MIDI sound, played by {main_name}"))
                                    .clicked()
                                {
                                    p.set_font(inst, None);
                                }
                                for f in fonts {
                                    let on = font.is_some_and(|c| c.name == f.name);
                                    if ui.selectable_label(on, &f.name).clicked() && !on {
                                        let pick = f.suggest(inst, p.sounds.get(inst)).map(|(b, _)| f.preset_for(b));
                                        p.set_font(inst, pick);
                                    }
                                }
                            })
                            .response
                            .on_hover_text("Where this part's sound comes from");
                    });
                    ui.add_enabled_ui(used, |ui| {
                        if let (Some(f), Some(fp)) = (font, p.font(inst)) {
                            let mut pick = (fp.bank, fp.preset);
                            if preset_combo(ui, ("preset", inst), f, &mut pick, sound_w) {
                                let preset = f.find(pick.0, pick.1).map(|x| f.preset_for(x));
                                p.set_font(inst, preset);
                            }
                            return;
                        }
                        let current = p.sounds.get(inst);
                        egui::ComboBox::from_id_salt(("sound", inst))
                            .width(sound_w)
                            .truncate()
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
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if widgets::text_button(ui, "All on", false, true).clicked() {
                    self.params.muted.clear();
                }
                if widgets::text_button(ui, "Default sounds", false, true)
                    .on_hover_text("General MIDI defaults, from the main sound")
                    .clicked()
                {
                    self.params.sounds = Default::default();
                    self.params.fonts.clear();
                }
            });
        });
    }

    /// The bar below the piano roll: transport, volume, MIDI and WAV export, status.
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let pos = self.player.as_ref().map(|p| p.position()).unwrap_or(0.0);
        let playing = self.player.as_ref().map(|p| p.is_playing()).unwrap_or(false);
        let dur = self.song.duration_secs();
        let t = Tokens::get(ui.ctx());
        ui.horizontal_centered(|ui| {
            ui.add_space(6.0);
            ui.add_enabled_ui(self.player.is_some(), |ui| {
                let (icon, help) = if playing { (Icon::Pause, "Pause (Space)") } else { (Icon::Play, "Play (Space)") };
                if widgets::icon_button(ui, icon, playing, help).clicked() {
                    self.toggle_play();
                }
                if widgets::icon_button(ui, Icon::Stop, false, "Stop and go back to the start").clicked() {
                    if let Some(p) = &self.player {
                        p.send(Cmd::Pause);
                        p.send(Cmd::Seek(0.0));
                    }
                }
                if widgets::icon_button(ui, Icon::Loop, self.looping, "Loop: play again from the start").clicked() {
                    self.looping = !self.looping;
                    if let Some(p) = &self.player {
                        p.send(Cmd::Looping(self.looping));
                    }
                }
                ui.label(
                    egui::RichText::new(format!("{} / {}", fmt_time(pos.min(dur)), fmt_time(dur)))
                        .font(t.font(12.5))
                        .color(t.text),
                );
                ui.add_space(8.0);
                widgets::icon_button(ui, Icon::Speaker, false, "Volume");
                ui.spacing_mut().slider_width = 90.0;
                if ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.5).show_value(false)).changed() {
                    if let Some(p) = &self.player {
                        p.send(Cmd::Volume(self.volume));
                    }
                }
            });
            ui.separator();
            if widgets::text_button(ui, "Save MIDI…", false, true).on_hover_text("Save the piece as a MIDI file").clicked() {
                self.save_midi();
            }
            let exporting = self.export.is_some();
            if widgets::text_button(ui, "Export WAV…", exporting, !exporting)
                .on_hover_text("Render the piece to a WAV file")
                .clicked()
            {
                self.export_wav();
            }
            if let Some(ex) = &self.export {
                let f = ex.progress.load(Ordering::Relaxed) as f32 / 1000.0;
                ui.add(egui::ProgressBar::new(f).desired_width(100.0).show_percentage());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                if let Some(e) = &self.audio_error {
                    ui.colored_label(t.caution, format!("Audio unavailable ({e}): you can still save MIDI files."));
                } else if !self.status.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(&self.status).font(t.font(11.5)).color(t.text_dim)).truncate());
                }
            });
        });
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
        let tk = Tokens::get(ui.ctx());
        let dark = ui.visuals().dark_mode;
        painter.rect_filled(rect, 0.0, tk.canvas);

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
                    tk.font(9.0),
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
        let text_col = tk.text_dim;
        for (t, name) in &song.chords {
            let x = x_of(*t);
            if x - last_x > 46.0 {
                painter.text(Pos2::new(x + 2.0, rect.top() + 4.0), Align2::LEFT_TOP, name, tk.font(11.0), text_col);
                last_x = x;
            }
        }

        // slideshow: shaded intro and outro, a line at each image change
        if self.slideshow.show_on_timeline {
            let plan = self.slide_plan();
            let x_s = |secs: f64| x_of(song.secs_to_tick(secs)).min(roll.right());
            let shade = if dark { Color32::from_white_alpha(8) } else { Color32::from_black_alpha(10) };
            let images_end = plan.intro + plan.images as f64 * plan.slot.secs;
            for (a, b, label) in [(0.0, plan.intro, "intro"), (images_end, plan.total, "outro")] {
                let r = Rect::from_x_y_ranges(x_s(a)..=x_s(b), roll.y_range());
                if r.width() > 0.5 {
                    painter.rect_filled(r, 0.0, shade);
                    if r.width() > 34.0 {
                        painter.text(r.left_bottom() + Vec2::new(3.0, -3.0), Align2::LEFT_BOTTOM, label, tk.font(10.0), text_col);
                    }
                }
            }
            let line = tk.accent.gamma_multiply(if dark { 0.35 } else { 0.25 });
            let mut last_x = f32::NEG_INFINITY;
            for (i, t) in plan.starts().enumerate() {
                let x = x_s(t);
                painter.vline(x, roll.y_range(), Stroke::new(1.0, line));
                if x - last_x > 22.0 {
                    painter.text(Pos2::new(x + 2.0, roll.top() + 2.0), Align2::LEFT_TOP, format!("{}", i + 1), tk.font(9.0), line);
                    last_x = x;
                }
            }
            if plan.images > 0 {
                painter.vline(x_s(images_end), roll.y_range(), Stroke::new(1.0, line));
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
                song.sound_name(i),
                if muted { " (off)" } else { "" }
            );
            let tc = if muted { text_col.gamma_multiply(0.5) } else { text_col };
            let g = painter.layout_no_wrap(label, tk.font(11.0), tc);
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
        painter.text(rect.right_bottom() - Vec2::new(6.0, 3.0), Align2::RIGHT_BOTTOM, info, tk.font(11.0), text_col);

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
    }
}

impl eframe::App for App {
    fn persist_egui_memory(&self) -> bool {
        self.snapshot.is_none()
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.snapshot.is_some() {
            return;
        }
        let theme = match self.theme {
            egui::ThemePreference::System => "system",
            egui::ThemePreference::Light => "light",
            egui::ThemePreference::Dark => "dark",
        };
        eframe::set_value(storage, KEY_THEME, &theme.to_string());
        eframe::set_value(storage, KEY_CHECK_UPDATES, &self.check_updates);
        eframe::set_value(storage, KEY_SLIDESHOW, &self.slideshow);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Space)) && ui.ctx().memory(|m| m.focused().is_none()) {
            self.toggle_play();
        }
        let shift_cmd = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        if ui.ctx().input_mut(|i| i.consume_key(shift_cmd, egui::Key::S)) {
            self.save_project(true);
        } else if ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save_project(false);
        }
        if ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O)) {
            self.open_project();
        }
        let title = match &self.project_path {
            Some(p) => format!(
                "{} — Ambient MIDI Generator",
                p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            ),
            None => "Ambient MIDI Generator".to_string(),
        };
        if title != self.title {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
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

        let tokens = Tokens::get(ui.ctx());
        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui));
        // drag the panel's edge to resize it; egui remembers the width
        let mut show_controls = self.show_controls;
        egui::Panel::left("controls_panel")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(0, 4)))
            .resizable(true)
            .default_size(380.0)
            .size_range(340.0..=900.0)
            .show_collapsible(ui, &mut show_controls, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
            });
        self.show_controls = show_controls;
        egui::Panel::bottom("toolbar").resizable(false).exact_size(40.0).show(ui, |ui| self.toolbar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).fill(tokens.canvas))
            .show(ui, |ui| self.piano_roll(ui));

        self.sync_slideshow_length();
        // Wait until a slider drag is released so playback isn't constantly restarted.
        let dragging = ui.ctx().input(|i| i.pointer.any_down());
        if self.auto_regen && !self.params.same_music(&self.generated_from) && !dragging {
            self.regenerate();
        }
        self.sync_fx();
        self.assign_window(ui.ctx());
        self.poll_update_check();
        #[cfg(target_os = "macos")]
        if let Some(menu) = &self.menu {
            if menu.take_about_request() {
                self.about.open = true;
            }
            if menu.take_check_request() {
                self.about.open = true;
                self.about.check_requested = true;
            }
        }
        self.about.show(ui.ctx(), &self.update, &mut self.check_updates);
        if std::mem::take(&mut self.about.check_requested) {
            self.start_update_check(ui.ctx());
        }
        self.sync_engine();

        if let Some(s) = &mut self.snapshot {
            s.tick(ui.ctx());
        }
        let busy = self.player.as_ref().map(|p| p.is_playing()).unwrap_or(false) || self.export.is_some();
        if busy {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
    }
}

/// A 0..1 slider row with words at its ends saying which way is less and which more.
fn amount(ui: &mut egui::Ui, id: &str, label: &str, v: &mut f32, ends: (&str, &str), help: &str) {
    let spec = widgets::SliderSpec { id, label, help, ends: Some(ends), ..Default::default() };
    let mut x = *v as f64;
    if widgets::slider(ui, &spec, &mut x, true) {
        *v = x as f32;
    }
}
