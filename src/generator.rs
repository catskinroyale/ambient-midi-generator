//! The music generator. `generate(&params)` is a pure function: identical params
//! (including the seed) always produce an identical song.

use std::collections::HashMap;

use crate::kit::{self, drum, fx};
use crate::rng::Rng;
use crate::song::{Ctrl, Inst, LengthMode, Note, Params, Song, Style};
use crate::theory::{Chord, Ext, Scale, voice_lead};

const PPQ: u32 = 480;

/// (onset, duration) in eighth notes.
type Pattern = &'static [(u32, u32)];

const PAT_SLOW_44: &[Pattern] = &[
    &[(0, 8)],
    &[(0, 4), (4, 4)],
    &[(0, 6), (6, 2)],
    &[(0, 3), (3, 5)],
    &[(2, 6)],
    &[(0, 4), (4, 2), (6, 2)],
];
const PAT_MOVING_44: &[Pattern] = &[
    &[(0, 8)],
    &[(0, 4), (4, 4)],
    &[(0, 2), (2, 2), (4, 4)],
    &[(0, 6), (6, 2)],
    &[(0, 3), (3, 3), (6, 2)],
    &[(0, 2), (2, 4), (6, 2)],
];
const PAT_WALTZ: &[Pattern] = &[
    &[(0, 6)],
    &[(0, 4), (4, 2)],
    &[(0, 2), (2, 4)],
    &[(2, 2), (4, 2)],
    &[(0, 2), (2, 2), (4, 2)],
    &[(2, 4)],
];
const PAT_68: &[Pattern] = &[
    &[(0, 6)],
    &[(0, 3), (3, 3)],
    &[(0, 2), (2, 1), (3, 3)],
    &[(0, 4), (4, 1), (5, 1)],
    &[(0, 3), (3, 2), (5, 1)],
];

/// How a section is rearranged (version 2).
#[derive(Clone, Copy, Debug)]
enum Treat {
    /// The bass, pulse and the keys' left hand drop away.
    Breakdown,
    /// The tune rests.
    LeadRests,
    /// The tune an octave higher.
    Lift,
    /// The accompaniment thins out.
    Thin,
    /// The pad drops out, leaving the keys and tune bare.
    Bare,
}

struct MelodyCfg {
    lo: i32,
    hi: i32,
    patterns: &'static [Pattern],
    phrase_bars: u32,
    vel: f64,
    rest_bar_prob: f64,
    ornaments: bool,
}

struct Ctx<'a> {
    p: &'a Params,
    scale: Scale,
    ppq: u32,
    bar: u32,
    bars: u32,
    piano: bool,
    strings: bool,
    harmony: Vec<Chord>,
    /// index into the section theme for each bar (used to pick consistent patterns)
    section: Vec<u8>,
    intensity: Vec<f64>,
    notes: Vec<Note>,
    ctrls: Vec<Ctrl>,
}

/// Seconds per bar at the piece's tempo and meter.
pub fn secs_per_bar(p: &Params) -> f64 {
    let meter = p.style.meter();
    let bar = PPQ * 4 * meter.0 / meter.1;
    bar as f64 / PPQ as f64 * 60.0 / p.tempo.clamp(30.0, 200.0) as f64
}

/// How many bars the piece has, not counting the ring-out bar: from a
/// score's sections when they are set by hand, else from the length.
pub fn planned_bars(p: &Params) -> u32 {
    if let Some(sections) = p.custom_sections() {
        return sections.iter().sum::<u32>().max(4);
    }
    match p.length_mode {
        // one extra bar of ring-out is appended after the last bar
        LengthMode::Seconds => ((p.target_seconds as f64 / secs_per_bar(p)).round() as i64 - 1).max(4) as u32,
        LengthMode::Bars => p.target_bars.max(4),
    }
}

pub fn generate(p: &Params) -> Song {
    let meter = p.style.meter();
    let bar = PPQ * 4 * meter.0 / meter.1;
    let bpm = p.tempo.clamp(30.0, 200.0) as f64;
    let secs_per_bar = secs_per_bar(p);
    let bars = planned_bars(p);

    let mut ctx = Ctx {
        p,
        scale: Scale::new(p.root, p.mode),
        ppq: PPQ,
        bar,
        bars,
        piano: p.instrumentation.piano(),
        strings: p.instrumentation.strings(),
        harmony: Vec::new(),
        section: Vec::new(),
        intensity: Vec::new(),
        notes: Vec::new(),
        ctrls: Vec::new(),
    };
    ctx.plan_intensity();

    match p.style {
        Style::EtherealPads => ctx.ethereal_pads(),
        Style::Gymnopedie => ctx.gymnopedie(),
        Style::GenerativeLoops => ctx.generative_loops(),
        Style::MinimalArpeggios => ctx.minimal_arpeggios(),
        Style::Rainfall => ctx.rainfall(),
        Style::CinematicSwell => ctx.cinematic_swell(),
        Style::NocturneDrift => ctx.nocturne(),
        Style::Tintinnabuli => ctx.tintinnabuli(),
        Style::Chorale => ctx.chorale(),
        Style::Canon => ctx.canon(),
        Style::TidalDrone => ctx.tidal_drone(),
        Style::FeltBallad => ctx.felt_ballad(),
        Style::BerlinSchool => ctx.berlin_school(),
        Style::DubChords => ctx.dub_chords(),
        Style::Aurora => ctx.aurora(),
        Style::Lullaby => ctx.lullaby(),
        Style::CelticAir => ctx.celtic_air(),
        Style::KotoGarden => ctx.koto_garden(),
        Style::Gamelan => ctx.gamelan(),
        Style::Fingerstyle => ctx.fingerstyle(),
        Style::NeonNoir => ctx.neon_noir(),
        Style::TechTrailer => ctx.tech_trailer(),
        Style::MegacityRain => ctx.megacity_rain(),
        Style::StarCathedral => ctx.star_cathedral(),
        Style::NeonGlide => ctx.neon_glide(),
    }
    if p.style.is_score() {
        ctx.score_dynamics();
    }
    ctx.arrange(secs_per_bar);

    let mut song = ctx.finish(bpm, meter);
    crate::expression::apply(&mut song);
    song
}

/// The bars of each section of a score `n` bars long, when they aren't set
/// by hand: the arc scaled to the length.
pub fn auto_sections(style: Style, n: u32) -> Vec<u32> {
    let n = n.max(4);
    let at = |x: f64| ((n as f64 * x) as u32).min(n - 1);
    let starts = match style {
        Style::TechTrailer => {
            let even = |x: f64| ((n as f64 * x) as u32 / 2 * 2).max(2);
            let outro = ((n as f64 * 0.88) as u32).clamp(1, n.saturating_sub(2).max(1));
            let peak = even(0.6).min(outro.saturating_sub(2)).max(1);
            let intro = even(0.2).min(peak.saturating_sub(4)).max(1).min(peak);
            vec![intro, peak, outro]
        }
        Style::MegacityRain => {
            let lead_in = at(0.08).max(1).min(n - 1);
            let body = (at(0.22) / 2 * 2).max(lead_in);
            let recede = at(0.82).max(body);
            vec![lead_in, body, recede]
        }
        Style::StarCathedral => {
            let outro = at(0.87).max(1);
            let hold = outro.saturating_sub(if n >= 24 { 3 } else { 1 }).max(1);
            let climb = ((hold as f64 - n as f64 * 0.6).max(0.0) as u32 / 4 * 4).max(4).min(hold - 1);
            let peak = hold - climb;
            let build = at(0.17).max(1).min(peak);
            vec![build, peak, hold, outro]
        }
        Style::NeonGlide => {
            let even = |x: f64| at(x) / 2 * 2;
            let finale = even(0.8).max(1);
            let bridge = even(0.68).min(finale);
            let bloom = even(0.45).min(bridge);
            let groove = even(0.12).max(1).min(bloom);
            vec![groove, bloom, bridge, finale]
        }
        _ => return Vec::new(),
    };
    std::iter::once(0).chain(starts.iter().copied()).zip(starts.iter().copied().chain([n])).map(|(a, b)| b - a).collect()
}

/// Where each section of a score `bars` long after the first begins: as set
/// by hand, or the arc scaled to the length. None for other styles.
pub fn section_starts(p: &Params, bars: u32) -> Vec<u32> {
    let lengths = match p.custom_sections() {
        Some(s) if s.iter().sum::<u32>() == bars => s.to_vec(),
        _ => auto_sections(p.style, bars),
    };
    lengths.iter().scan(0, |at, len| {
        *at += len;
        Some(*at)
    }).take(lengths.len().saturating_sub(1)).collect()
}

impl<'a> Ctx<'a> {
    /// Where each section of a score after the first begins.
    fn bounds(&self) -> Vec<u32> {
        section_starts(self.p, self.bars)
    }

    /// For a score's layers: how much later than written (above 1) or earlier
    /// (below 1) they come in, from the density. Unchanged at the default.
    fn late(&self) -> f64 {
        (1.5 - self.density()).clamp(0.5, 1.5)
    }

    /// From the variation, now and then replace a chord of a score's
    /// progression with a relative a third away, as it comes round again.
    /// The first chord of each section and those in `keep` stay.
    fn vary_harmony(&mut self, keep: &[std::ops::Range<u32>]) {
        let v = self.p.variation as f64;
        let mut rng = self.rng("score-substitutions");
        let n = self.bars;
        let starts: Vec<u32> = (0..n).filter(|b| *b == 0 || self.section[*b as usize] != self.section[*b as usize - 1]).collect();
        let mut b = 0;
        while b + 2 < n {
            let section_start = *starts.iter().rev().find(|s| **s <= b).unwrap_or(&0);
            let run = self.harmony[b as usize].clone();
            let mut len = 1;
            while b + len < n && self.harmony[(b + len) as usize].root == run.root && self.harmony[(b + len) as usize].ext == run.ext && len < 2 {
                len += 1;
            }
            let protected = b == section_start || b + len + 2 > n || keep.iter().any(|r| r.contains(&b));
            if !protected && rng.chance(0.6 * v * v) {
                let root = self.fix_dim(run.root + *rng.pick(&[2, -2]));
                let ext = if rng.chance(v) { *rng.pick(&[Ext::Add9, Ext::Sus2, Ext::Sus4]) } else { run.ext };
                for k in b..b + len {
                    self.harmony[k as usize] = Chord::new(root, ext);
                }
            }
            b += len;
        }
    }

    /// The dynamics of a score: its arc flattened towards even (low) or
    /// pushed further apart (high), on every note and swell. Each part's
    /// loudest moments stay as they are; the quieter ones are lifted or
    /// sunk. Unchanged at the default.
    fn score_dynamics(&mut self) {
        let k = 0.25 + 1.25 * self.p.dynamics as f64;
        if (k - 1.0).abs() < 1e-6 {
            return;
        }
        // near the top of what each part plays, so a few accents don't set it
        let loud = |mut v: Vec<f64>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v.get(v.len() * 9 / 10).copied().unwrap_or(127.0).max(1.0)
        };
        let shape = |v: f64, top: f64| if v >= top { v } else { top * (v / top).powf(k) };
        for inst in Inst::ALL {
            let top = loud(self.notes.iter().filter(|n| n.inst == inst).map(|n| n.vel as f64).collect());
            for n in self.notes.iter_mut().filter(|n| n.inst == inst) {
                n.vel = shape(n.vel as f64, top).round().clamp(1.0, 127.0) as u8;
            }
            let top = loud(self.ctrls.iter().filter(|c| c.inst == inst && c.cc == 11).map(|c| c.value as f64).collect());
            for c in self.ctrls.iter_mut().filter(|c| c.inst == inst && c.cc == 11) {
                c.value = shape(c.value as f64, top).round().clamp(0.0, 127.0) as u8;
            }
        }
    }

    fn rng(&self, name: &str) -> Rng {
        Rng::stream(self.p.seed, name)
    }

    fn it(&self, bar: u32) -> f64 {
        self.intensity[(bar as usize).min(self.intensity.len() - 1)]
    }

    /// The arrangement's energy curve: quiet start, a peak somewhere past the
    /// middle, quiet ending. `dynamics` controls how pronounced it is.
    fn plan_intensity(&mut self) {
        let mut rng = self.rng("intensity");
        let peak = rng.range_f(0.55, 0.75);
        let phase = rng.range_f(0.0, std::f64::consts::TAU);
        let d = self.p.dynamics as f64;
        let n = self.bars.max(2);
        let smooth = |x: f64| x * x * (3.0 - 2.0 * x);
        self.intensity = (0..n)
            .map(|b| {
                let x = b as f64 / (n - 1) as f64;
                let shape = if x < peak {
                    smooth(x / peak)
                } else {
                    1.0 - 0.85 * smooth((x - peak) / (1.0 - peak))
                };
                let lvl = 0.15 + 0.85 * shape;
                let wave = 0.08 * d * (b as f64 * std::f64::consts::TAU / 16.0 + phase).sin();
                ((1.0 - d) * 0.6 + d * lvl + wave).clamp(0.05, 1.0)
            })
            .collect();
    }

    /// Intensity shifted by density, for deciding when supporting layers play:
    /// at higher density they come in earlier and stay longer. At the default
    /// density (0.5) it equals the intensity.
    fn full(&self, bar: u32) -> f64 {
        self.it(bar) + (self.p.density as f64 - 0.5) * 0.6
    }

    fn density(&self) -> f64 {
        self.p.density as f64
    }

    /// At low density, drop the middle note of a chord of four or more.
    fn thin(&self, mut voicing: Vec<i32>) -> Vec<i32> {
        if self.density() < 0.3 && voicing.len() >= 4 {
            voicing.remove(voicing.len() / 2);
        }
        voicing
    }

    /// From version 3, a seeded choice among `n` treatments of one aspect of a
    /// style (its accompaniment, bass, ...), each aspect chosen independently,
    /// so pieces in the same style differ in more than their notes. 0 is the
    /// way earlier versions played it.
    fn variant(&self, axis: &str, n: u32) -> u32 {
        if self.p.version < 3 || n < 2 {
            return 0;
        }
        Rng::stream(self.p.seed, &format!("{:?}-{axis}", self.p.style)).range(0, n as i32 - 1) as u32
    }

    /// From version 3, the tune's phrasing varies with the seed: short
    /// answering phrases with more space, long lines with grace notes, or the
    /// tune moved up (or down, if it is already high).
    fn vary_melody(&self, cfg: &mut MelodyCfg) {
        match self.variant("melody", 4) {
            1 => {
                cfg.phrase_bars = (cfg.phrase_bars / 2).max(2);
                cfg.rest_bar_prob = (cfg.rest_bar_prob + 0.15).min(0.6);
            }
            2 => {
                cfg.phrase_bars *= 2;
                cfg.ornaments = true;
            }
            3 => {
                let shift = if cfg.hi >= 16 { -2 } else { 2 };
                cfg.lo += shift;
                cfg.hi += shift;
            }
            _ => {}
        }
    }

    fn note(&mut self, inst: Inst, start: u32, dur: u32, key: i32, vel: f64) {
        let key = key.clamp(21, 108) as u8;
        let vel = vel.round().clamp(1.0, 127.0) as u8;
        self.notes.push(Note { start, dur: dur.max(1), key, vel, inst });
    }

    /// CC11 expression rise-and-fall over a span (strings breathing).
    fn swell(&mut self, inst: Inst, start: u32, dur: u32, from: f64, peak: f64, to: f64) {
        let step = self.ppq / 4;
        let n = (dur / step).max(2);
        let peak_at = 0.45;
        let mut last = 255u8;
        for i in 0..=n {
            let x = i as f64 / n as f64;
            let v = if x < peak_at {
                from + (peak - from) * (x / peak_at)
            } else {
                peak + (to - peak) * ((x - peak_at) / (1.0 - peak_at))
            };
            let v = v.round().clamp(0.0, 127.0) as u8;
            if v != last {
                self.ctrls.push(Ctrl { tick: start + i * step, inst, cc: 11, value: v });
                last = v;
            }
        }
    }

    /// Sustained chord: strings if available, otherwise a softly rolled piano chord.
    fn pad(&mut self, start: u32, dur: u32, voicing: &[i32], vel: f64) {
        if self.strings {
            for k in voicing {
                self.note(Inst::StringPad, start, dur, *k, vel);
            }
        } else {
            let roll = self.ppq / 10;
            for (i, k) in voicing.iter().enumerate() {
                self.note(Inst::Piano, start + i as u32 * roll, dur, *k, vel * 0.7);
            }
            if dur >= 2 * self.bar {
                let half = start + (dur / 2 / self.bar) * self.bar;
                for (i, k) in voicing.iter().enumerate() {
                    self.note(Inst::Piano, half + i as u32 * roll, dur - (half - start), *k, vel * 0.55);
                }
            }
        }
    }

    fn bass_key(&self, chord: &Chord, lo: i32) -> i32 {
        let mut k = self.scale.midi(chord.root);
        while k >= lo + 12 {
            k -= 12;
        }
        while k < lo {
            k += 12;
        }
        k
    }

    /// Plan one chord per `hr` bars using a pool of ambient-friendly progressions,
    /// arranged as A A B A sections with seeded substitutions for variety.
    fn plan_harmony(&mut self, hr: u32, ext_w: [f64; 6], extra_pool: &[&[i32]]) {
        let mut rng = self.rng("harmony");
        let v = self.p.variation as f64;
        let minor = self.p.mode.is_minor();
        let mut pool: Vec<&[i32]> = if minor {
            vec![
                &[0, 5, 2, 6],
                &[0, 6, 5, 6],
                &[0, 3, 5, 4],
                &[5, 6, 0, 0],
                &[0, 2, 6, 3],
                &[0, 5, 3, 6],
                &[0, 3, 0, 5],
            ]
        } else {
            vec![
                &[0, 4, 5, 3],
                &[0, 5, 3, 4],
                &[3, 0, 4, 5],
                &[5, 3, 0, 4],
                &[0, 2, 3, 0],
                &[0, 3, 5, 4],
                &[0, 1, 3, 0],
            ]
        };
        match self.p.mode {
            crate::theory::Mode::Lydian => pool.push(&[0, 1, 0, 1]),
            crate::theory::Mode::Dorian => pool.push(&[0, 3, 0, 3]),
            crate::theory::Mode::Mixolydian => pool.push(&[0, 6, 3, 0]),
            _ => {}
        }
        pool.extend_from_slice(extra_pool);
        let v2 = self.p.version >= 2;
        if v2 {
            // I-IV-ii-V, the "royal road" IV-V-iii-vi, I-vi-ii-V ...; and in
            // minor the Andalusian i-VII-VI-v, i-iv-VII-III ...
            let more: &[&[i32]] = if minor {
                &[&[0, 6, 5, 4], &[0, 3, 6, 2], &[5, 6, 0, 0], &[0, 2, 5, 4], &[3, 5, 0, 6]]
            } else {
                &[&[0, 3, 1, 4], &[3, 4, 2, 5], &[0, 5, 1, 4], &[0, 2, 3, 4], &[5, 3, 4, 0]]
            };
            pool.extend_from_slice(more);
        }
        let exts = [Ext::Triad, Ext::Add9, Ext::Seventh, Ext::Ninth, Ext::Sus2, Ext::Sus4];

        let fix = |s: &Scale, mut d: i32| {
            let mut guard = 0;
            while s.is_diminished(d) && guard < 7 {
                d = (d + 5).rem_euclid(7);
                guard += 1;
            }
            d
        };

        // Three section themes; from version 2, three different progressions.
        let mut themes: Vec<Vec<Chord>> = Vec::new();
        let mut used: Vec<&[i32]> = Vec::new();
        for _ in 0..3 {
            let mut prog = *rng.pick(&pool);
            for _ in 0..6 {
                if !v2 || !used.contains(&prog) {
                    break;
                }
                prog = *rng.pick(&pool);
            }
            used.push(prog);
            themes.push(
                prog.iter()
                    .map(|d| Chord::new(fix(&self.scale, *d), exts[rng.weighted(&ext_w)]))
                    .collect(),
            );
        }
        // Make B different from A where possible.
        if themes[1].iter().map(|c| c.root).eq(themes[0].iter().map(|c| c.root)) {
            themes[1].rotate_left(2);
        }
        let form: &[u8] = if v2 {
            // each seed its own form; the longer ones visit a third section
            const FORMS: [&[u8]; 5] = [&[0, 0, 1, 0], &[0, 1, 0, 2], &[0, 0, 1, 1, 0], &[0, 1, 2, 0], &[0, 1, 0, 1, 2, 0]];
            const LONG: [&[u8]; 3] = [&[0, 0, 1, 0, 2, 1, 0, 0], &[0, 1, 0, 2, 2, 1, 0, 0], &[0, 0, 1, 2, 0, 1, 0]];
            let mut frng = self.rng("form");
            if frng.chance(v * 0.8) { frng.pick(&LONG) } else { frng.pick(&FORMS) }
        } else if v > 0.6 {
            &[0, 0, 1, 0, 2, 1, 0, 0]
        } else {
            &[0, 0, 1, 0]
        };

        let slots = self.bars.div_ceil(hr) as usize;
        let mut slot_chords = Vec::with_capacity(slots);
        let mut slot_section = Vec::with_capacity(slots);
        for s in 0..slots {
            let sec = form[(s / 4) % form.len()];
            let mut c = themes[sec as usize][s % 4].clone();
            // from version 2, a section heard before comes back a little changed
            let heard = v2 && form[..(s / 4) % form.len()].contains(&sec);
            let sub = if heard { v * 0.3 + 0.15 } else { v * 0.3 };
            if s % 4 != 0 && rng.chance(sub) {
                let delta = *rng.pick(&[2, -2, 3]);
                c = Chord::new(fix(&self.scale, c.root + delta), c.ext);
            }
            if rng.chance(v * 0.25) {
                c.ext = exts[rng.weighted(&ext_w)];
            }
            slot_chords.push(c);
            slot_section.push(sec);
        }
        // Cadence: settle on the tonic, approached by IV (or V).
        if slots >= 3 {
            let pen = if rng.chance(0.6) || self.scale.is_diminished(4) { 3 } else { 4 };
            slot_chords[slots - 2] = Chord::new(pen, Ext::Add9);
        }
        let last_ext = *rng.pick(&[Ext::Add9, Ext::Triad, Ext::Sus2]);
        slot_chords[slots - 1] = Chord::new(0, last_ext);

        self.harmony = (0..self.bars).map(|b| slot_chords[(b / hr) as usize].clone()).collect();
        self.section = (0..self.bars).map(|b| slot_section[(b / hr) as usize]).collect();
    }

    fn random_step(&self, rng: &mut Rng) -> i32 {
        let v = self.p.variation as f64;
        const STEPS: [i32; 9] = [-3, -2, -1, 0, 1, 2, 3, 4, -4];
        let w = [0.3 * v + 0.05, 1.0, 2.2, 0.6, 2.2, 1.0, 0.3 * v + 0.05, 0.15 * v, 0.15 * v];
        STEPS[rng.weighted(&w)]
    }

    fn nearest_tone(&self, d: i32, chord: &Chord, core: bool) -> i32 {
        for off in [0, -1, 1, -2, 2, -3, 3] {
            let c = d + off;
            if (core && chord.is_core_tone(c)) || (!core && chord.contains(c)) {
                return c;
            }
        }
        d
    }

    /// Phrase-based melody: a theme (rhythm + contour) is generated, then mostly
    /// repeated with the pitches re-fitted to the current chords.
    fn melody(&self, rng: &mut Rng, cfg: &MelodyCfg, insts: &[Option<Inst>]) -> Vec<Note> {
        let e = self.ppq / 2;
        let v = self.p.variation as f64;
        let centre = (cfg.lo + cfg.hi) / 2;
        let mut deg = centre;
        let mut theme: Option<(Vec<usize>, Vec<i32>)> = None;
        let mut out = Vec::new();
        let mut bar = 0;
        while bar < self.bars {
            let pb = cfg.phrase_bars.min(self.bars - bar);
            let reuse = theme.is_some() && rng.chance(0.8 - 0.5 * v);
            let (pats, steps) = if reuse {
                theme.clone().unwrap()
            } else {
                let pats: Vec<usize> = (0..cfg.phrase_bars)
                    .map(|i| {
                        if i == cfg.phrase_bars - 1 && rng.chance(0.6) {
                            0
                        } else {
                            rng.range(0, cfg.patterns.len() as i32 - 1) as usize
                        }
                    })
                    .collect();
                let steps: Vec<i32> = (0..48).map(|_| self.random_step(rng)).collect();
                (pats, steps)
            };
            if theme.is_none() || (!reuse && rng.chance(0.35)) {
                theme = Some((pats.clone(), steps.clone()));
            }
            let mut si = 0;
            for i in 0..pb {
                let b = bar + i;
                let Some(inst) = insts[b as usize] else { continue };
                let last_bar = i == pb - 1;
                if !last_bar && rng.chance(cfg.rest_bar_prob) {
                    continue;
                }
                let chord = self.harmony[b as usize].clone();
                let pat = cfg.patterns[pats[i as usize] % cfg.patterns.len()];
                for (j, &(on, dur)) in pat.iter().enumerate() {
                    let step = steps[si % steps.len()];
                    si += 1;
                    let mut d = deg + step;
                    // pull back towards the centre of the range
                    if d > cfg.hi || (d > centre + 4 && step > 0 && rng.chance(0.4)) {
                        d = deg - step.abs().max(1);
                    }
                    if d < cfg.lo || (d < centre - 4 && step < 0 && rng.chance(0.4)) {
                        d = deg + step.abs().max(1);
                    }
                    d = d.clamp(cfg.lo, cfg.hi);
                    let final_note = last_bar && j == pat.len() - 1;
                    if on == 0 || dur >= 4 || final_note {
                        d = self.nearest_tone(d, &chord, final_note);
                    }
                    deg = d;
                    let start = b * self.bar + on * e;
                    let mut len = dur * e;
                    if final_note {
                        len = len.max(self.bar - on * e);
                    }
                    if inst.sustains() {
                        len += e / 3;
                    } else {
                        len += e; // let piano notes ring a little
                    }
                    let accent = if on == 0 { 6.0 } else { 0.0 };
                    let vel = cfg.vel * (0.7 + 0.4 * self.it(b)) + accent;
                    let key = self.scale.midi(d);
                    if cfg.ornaments && dur >= 3 && on > 0 && rng.chance(0.25 * v) {
                        let grace = self.ppq / 8;
                        out.push(Note {
                            start: start.saturating_sub(grace),
                            dur: grace,
                            key: self.scale.midi(d + 1).clamp(21, 108) as u8,
                            vel: (vel * 0.7).clamp(1.0, 127.0) as u8,
                            inst,
                        });
                    }
                    out.push(Note {
                        start,
                        dur: len,
                        key: key.clamp(21, 108) as u8,
                        vel: vel.clamp(1.0, 127.0) as u8,
                        inst,
                    });
                }
            }
            bar += pb;
        }
        out
    }

    // ------------------------------------------------------------------ styles

    fn ethereal_pads(&mut self) {
        let hr = 2;
        self.plan_harmony(hr, [0.15, 0.35, 0.25, 0.05, 0.15, 0.05], &[]);
        let mut rng = self.rng("pads");
        let mut prev = Vec::new();
        let overlap = self.ppq / 2;
        // the pads close, spread wide, or high and airy
        let voicing_kind = self.variant("voicing", 3);
        let (lo, hi) = match voicing_kind {
            1 => (45, 81),
            2 => (57, 83),
            _ => (50, 76),
        };
        for b in (0..self.bars).step_by(hr as usize) {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let span = hr.min(self.bars - b) * self.bar;
            let start = b * self.bar;
            let spread = rng.chance(0.5) || voicing_kind == 1;
            let voicing = voice_lead(&chord.pitch_classes(&self.scale), &prev, lo, hi, spread);
            prev = voicing.clone();
            self.pad(start, span + overlap, &self.thin(voicing), 45.0 + 40.0 * it);
            let k = self.bass_key(&chord, 36);
            if self.strings {
                self.swell(Inst::StringPad, start, span, 45.0 + 25.0 * it, 75.0 + 45.0 * it, 55.0 + 25.0 * it);
                if self.full(b) > 0.3 {
                    self.note(Inst::Cello, start, span + overlap, k, 40.0 + 35.0 * it);
                }
            } else {
                self.note(Inst::Piano, start, span, k, 38.0 + 15.0 * it);
            }
            // busier: the bass moves to the fifth halfway through each chord
            if self.density() > 0.65 && span >= 2 * self.bar && self.full(b) > 0.4 {
                let inst = if self.strings { Inst::Cello } else { Inst::Piano };
                self.note(inst, start + span / 2, span / 2, k + 7, 34.0 + 22.0 * it);
            }
        }

        // High, sparse sparkles: scattered, each echoed an octave down, or
        // falling chimes at each chord change.
        let mut rng = self.rng("sparkles");
        let d = self.p.density as f64;
        let e = self.ppq / 2;
        let sparkle = self.variant("sparkle", 3);
        if sparkle == 2 {
            for b in (0..self.bars).step_by(hr as usize) {
                let it = self.it(b);
                if !rng.chance(0.3 + 0.6 * d * it) {
                    continue;
                }
                let chord = self.harmony[b as usize].clone();
                let mut deg = rng.range(12, 16);
                while !chord.contains(deg) {
                    deg += 1;
                }
                let inst = if self.piano { Inst::Piano } else { Inst::Violin };
                let at = b * self.bar + rng.range(1, 3) as u32 * e;
                for i in 0..3u32 {
                    let key = self.scale.midi(deg);
                    self.note(inst, at + i * 3 * e, 4 * self.ppq, key, 24.0 + 30.0 * it - i as f64 * 4.0);
                    deg -= 1;
                    while !chord.contains(deg) {
                        deg -= 1;
                    }
                }
            }
        }
        for b in 0..self.bars {
            if sparkle == 2 {
                break;
            }
            let it = self.it(b);
            let n = rng.poisson(0.3 + d * 2.5 * it);
            let chord = self.harmony[b as usize].clone();
            for _ in 0..n {
                let pos = b * self.bar + rng.range(0, (self.bar / e) as i32 - 1) as u32 * e;
                let mut deg = rng.range(7, 17);
                if !chord.contains(deg) && !self.scale.is_pentatonic(deg) {
                    deg += 1;
                }
                let key = self.scale.midi(deg);
                if self.piano {
                    let dur = rng.range(2, 6) as u32 * self.ppq;
                    let vel = 26.0 + 32.0 * it + rng.gauss() * 4.0;
                    self.note(Inst::Piano, pos, dur, key, vel);
                    if sparkle == 1 {
                        self.note(Inst::Piano, pos + 3 * e, dur, key - 12, vel * 0.7);
                    }
                } else {
                    let dur = rng.range(4, 8) as u32 * self.ppq;
                    self.note(Inst::Violin, pos, dur, key, 22.0 + 25.0 * it);
                }
            }
        }

        // A slow violin line at the high point of the piece (from version 3,
        // sometimes a longer one, lower down).
        let long_line = self.variant("line", 2) == 1;
        if self.strings && d > 0.3 {
            let when = if long_line { 0.5 } else { 0.7 };
            let insts: Vec<Option<Inst>> =
                (0..self.bars).map(|b| (self.full(b) > when).then_some(Inst::Violin)).collect();
            let mut rng = self.rng("melody");
            let mut cfg = MelodyCfg {
                lo: if long_line { 4 } else { 7 },
                hi: if long_line { 12 } else { 16 },
                patterns: PAT_SLOW_44,
                phrase_bars: 4,
                vel: 52.0,
                rest_bar_prob: 0.2,
                ornaments: false,
            };
            self.vary_melody(&mut cfg);
            let m = self.melody(&mut rng, &cfg, &insts);
            self.notes.extend(m);
        }
    }

    fn gymnopedie(&mut self) {
        self.plan_harmony(1, [0.2, 0.2, 0.6, 0.0, 0.0, 0.0], &[&[0, 3, 0, 3], &[3, 0, 3, 0]]);
        let mut rng = self.rng("accomp");
        let mut prev = Vec::new();
        let beat = self.ppq;
        // the left hand: bass and chord, oom-pah-pah with the bass alternating
        // root and fifth, or the chord rolled upwards
        let accomp = self.variant("accomp", 3);
        let bass_fifth = |k: i32| if k + 7 > 50 { k - 5 } else { k + 7 };
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let start = b * self.bar;
            let bass = self.bass_key(&chord, 38);
            let mut pcs = chord.pitch_classes(&self.scale);
            if pcs.len() > 3 {
                pcs.remove(0); // rootless upper chord, the bass has the root
            }
            let voicing = voice_lead(&pcs, &prev, 53, 72, false);
            prev = voicing.clone();
            let swing = rng.range(-10, 10) as f64;
            let full = self.full(b);
            // sparse: a two-note chord; busy: the chord is played again on beat 3
            let mut chord_keys = voicing.clone();
            if self.density() < 0.3 && chord_keys.len() >= 3 {
                chord_keys.remove(1);
            }
            let hits: &[(u32, u32)] = if (self.density() > 0.65 && full > 0.5) || accomp == 1 {
                &[(1, 1), (2, 1)]
            } else {
                &[(1, 2)]
            };
            // from version 3: the bass alternates root and fifth bar by bar
            let bass = if accomp == 1 && b % 2 == 1 && b + 1 < self.bars { bass_fifth(bass) } else { bass };
            if self.piano {
                self.note(Inst::Piano, start, 3 * beat, bass, 50.0 + 15.0 * it + swing * 0.3);
                if accomp == 2 {
                    // the chord rolled upwards in quavers from beat 2
                    for (i, k) in chord_keys.iter().enumerate() {
                        let at = start + beat + i as u32 * beat / 2;
                        self.note(Inst::Piano, at, 2 * beat - i as u32 * beat / 2, *k, 32.0 + 14.0 * it - i as f64 * 2.0);
                    }
                }
                for &(on, len) in hits {
                    if accomp == 2 {
                        break;
                    }
                    for k in &chord_keys {
                        let soft = if on == 2 { 5.0 } else { 0.0 };
                        self.note(Inst::Piano, start + on * beat, len * beat, *k, 34.0 + 14.0 * it - soft);
                    }
                }
                if self.strings && full > 0.45 {
                    for k in &chord_keys {
                        self.note(Inst::StringPad, start, self.bar + beat / 4, *k, 25.0 + 40.0 * it);
                    }
                    if full > 0.6 {
                        self.note(Inst::Cello, start, self.bar, bass, 35.0 + 30.0 * it);
                    }
                }
            } else {
                self.note(Inst::Cello, start, 3 * beat, bass, 50.0 + 20.0 * it);
                for &(on, len) in hits {
                    for k in &chord_keys {
                        self.note(Inst::StringPad, start + on * beat, len * beat, *k, 38.0 + 22.0 * it);
                    }
                }
            }
        }
        if self.strings {
            self.ctrls.push(Ctrl { tick: 0, inst: Inst::StringPad, cc: 11, value: 100 });
        }
        let mel_inst = if self.piano { Inst::Piano } else { Inst::Violin };
        let insts: Vec<Option<Inst>> = (0..self.bars)
            .map(|b| (b >= 2 && b + 1 < self.bars).then_some(mel_inst))
            .collect();
        let mut rng = self.rng("melody");
        let mut cfg = MelodyCfg {
            lo: 7,
            hi: 16,
            patterns: PAT_WALTZ,
            phrase_bars: 4,
            vel: 50.0,
            rest_bar_prob: 0.05 + 0.25 * (1.0 - self.p.density as f64),
            ornaments: true,
        };
        self.vary_melody(&mut cfg);
        let m = self.melody(&mut rng, &cfg, &insts);
        self.notes.extend(m);
    }

    fn generative_loops(&mut self) {
        // A static, modal harmony: everything floats over the tonic.
        self.harmony = vec![Chord::new(0, Ext::Add9); self.bars as usize];
        self.section = vec![0; self.bars as usize];
        let mut rng = self.rng("loops");
        let d = self.p.density as f64;
        let v = self.p.variation as f64;
        let penta = self.p.mode.pentatonic();
        let mut set: Vec<i32> = (2..17i32).filter(|deg| penta.contains(&deg.rem_euclid(7))).collect();
        while set.len() > 8 {
            let i = rng.range(0, set.len() as i32 - 1) as usize;
            set.remove(i);
        }
        let beat_t = self.ppq as f64;
        let total_beats = (self.bars * self.bar) as f64 / beat_t;
        let voices = 3 + (d * 4.0).round() as usize;
        // from version 3: short cells phasing quickly, or long tape loops
        let loops = self.variant("loops", 3);
        let (len_lo, len_hi, notes_hi) = match loops {
            1 => (7.0, 13.0, 2),
            2 => (24.0, 44.0, 4),
            _ => (14.0, 32.0, 3),
        };
        for vi in 0..voices {
            let inst = match (self.piano, self.strings) {
                (true, true) => {
                    if vi % 2 == 0 { Inst::Piano } else { Inst::StringPad }
                }
                (true, false) => Inst::Piano,
                _ => [Inst::StringPad, Inst::Violin, Inst::Cello][vi % 3],
            };
            let loop_len = (rng.range_f(len_lo, len_hi) * 4.0).round() / 4.0;
            let k = rng.range(1, notes_hi);
            let mut phrase: Vec<(f64, i32, f64)> = (0..k)
                .map(|_| {
                    let off = (rng.range_f(0.0, loop_len * 0.6) * 2.0).round() / 2.0;
                    let mut deg = *rng.pick(&set);
                    if inst == Inst::Cello {
                        deg -= 7;
                    }
                    let dur = if inst == Inst::Piano { rng.range_f(3.0, 6.0) } else { rng.range_f(4.0, 8.0) };
                    let dur = if loops == 1 { dur * 0.6 } else { dur };
                    (off, deg, dur)
                })
                .collect();
            phrase.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let entry = vi as f64 * rng.range_f(2.0, 8.0);
            let exit = total_beats - rng.range_f(0.0, 8.0) - 2.0;
            let mut t = entry;
            while t < exit {
                for (off, deg, dur) in phrase.iter_mut() {
                    let time = t + *off;
                    if time >= exit {
                        break;
                    }
                    let b = (time * beat_t) as u32 / self.bar;
                    let it = self.it(b);
                    if rng.chance((0.35 + 0.4 * (0.5 - d)) * (1.0 - it)) {
                        continue;
                    }
                    if rng.chance(v * 0.12) {
                        // the loop slowly evolves
                        *deg += *rng.pick(&[-1, 1]);
                        while !self.scale.is_pentatonic(*deg) {
                            *deg += 1;
                        }
                    }
                    let vel = match inst {
                        Inst::Piano => 34.0 + 28.0 * it,
                        _ => 32.0 + 30.0 * it,
                    } + rng.gauss() * 4.0;
                    let key = self.scale.midi(*deg);
                    self.note(inst, (time * beat_t) as u32, (*dur * beat_t) as u32, key, vel);
                }
                t += loop_len;
            }
        }
        // Drone: tonic and fifth; from version 3 sometimes a hollow tonic and
        // octave, or one that only sounds in the fuller stretches.
        let seg = 8 * self.bar;
        let tonic = self.bass_key(&Chord::new(0, Ext::Triad), 36);
        let drone = self.variant("drone", 3);
        let upper = if drone == 1 { tonic + 12 } else { tonic + 7 };
        let mut b = 0;
        while b < self.bars {
            let start = b * self.bar;
            let span = seg.min((self.bars - b) * self.bar);
            let it = self.it(b);
            if drone == 2 && self.full(b) < 0.45 && b > 0 {
                b += 8;
                continue;
            }
            if self.strings {
                self.note(Inst::Cello, start, span + self.ppq, tonic, 30.0 + 25.0 * it);
                self.note(Inst::Cello, start, span + self.ppq, upper, 24.0 + 20.0 * it);
                self.swell(Inst::Cello, start, span, 60.0, 90.0 + 20.0 * it, 60.0);
            } else {
                self.note(Inst::Piano, start, span, tonic, 30.0 + 12.0 * it);
                self.note(Inst::Piano, start + self.ppq / 6, span, upper, 26.0 + 10.0 * it);
            }
            b += 8;
        }
    }

    fn minimal_arpeggios(&mut self) {
        let hr = if self.p.density < 0.35 { 2 } else { 1 };
        self.plan_harmony(hr, [0.3, 0.3, 0.2, 0.0, 0.2, 0.0], &[]);
        let mut rng = self.rng("arp");
        const PATS: [[usize; 8]; 4] = [
            [0, 1, 2, 3, 4, 3, 2, 1],
            [0, 2, 1, 3, 2, 4, 3, 2],
            [0, 1, 2, 1, 3, 2, 4, 2],
            [0, 2, 3, 4, 0, 2, 3, 4],
        ];
        // From version 3, other figures: Alberti-like, falling and leaping
        // patterns, or groups of three crossing the beat.
        const MORE: [[usize; 8]; 4] = [
            [0, 2, 1, 2, 0, 2, 1, 2],
            [4, 3, 2, 1, 0, 1, 2, 3],
            [0, 4, 1, 3, 2, 4, 1, 3],
            [0, 2, 4, 0, 2, 4, 0, 2],
        ];
        let sec_pat = [rng.range(0, 3) as usize, rng.range(0, 3) as usize, rng.range(0, 3) as usize];
        let figures = if self.variant("figure", 2) == 1 { MORE } else { PATS };
        // from version 3, the cello may pulse in crotchets instead of holding
        let pulse_bass = self.variant("bass", 2) == 1;
        let e = self.ppq / 2;
        let arp_inst = if self.piano { Inst::Piano } else { Inst::Pizzicato };
        let mut prev = Vec::new();
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let start = b * self.bar;
            let low = self.scale.degree_near(self.bass_key(&chord, 40));
            let offs: [i32; 5] = match chord.ext {
                Ext::Sus2 => [0, 4, 7, 8, 11],
                Ext::Sus4 => [0, 4, 7, 10, 11],
                Ext::Add9 | Ext::Ninth => [0, 4, 7, 8, 9],
                _ => [0, 4, 7, 9, 11],
            };
            let keys: Vec<i32> = offs.iter().map(|o| self.scale.midi(low + o)).collect();
            let pat = figures[sec_pat[self.section[b as usize] as usize]];
            let fast = it > 0.75 && self.p.density > 0.5;
            // sparse: crotchets in the quiet stretches
            let slow = self.density() < 0.3 && self.full(b) < 0.5;
            let (step, count) = if fast { (e / 2, 16) } else if slow { (2 * e, 4) } else { (e, 8) };
            for i in 0..count {
                let k = keys[pat[(if slow { i * 2 } else { i }) % 8]] + if fast && i >= 8 { 12 } else { 0 };
                let accent = if i == 0 { 10.0 } else if i % 4 == 0 { 4.0 } else { 0.0 };
                let dur = if arp_inst == Inst::Piano { step * 3 } else { step };
                self.note(arp_inst, start + i as u32 * step, dur, k, 40.0 + 20.0 * it + accent);
            }
            if self.strings && self.piano {
                if self.full(b) > 0.35 {
                    let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 55, 76, true);
                    prev = v.clone();
                    for k in &self.thin(v) {
                        self.note(Inst::StringPad, start, self.bar + e / 2, *k, 30.0 + 45.0 * it);
                    }
                    self.swell(Inst::StringPad, start, self.bar, 60.0, 80.0 + 30.0 * it, 65.0);
                }
                if pulse_bass && self.full(b) > 0.45 && b + 1 < self.bars {
                    for i in 0..self.bar / self.ppq {
                        let accent = if i == 0 { 6.0 } else { 0.0 };
                        self.note(Inst::Cello, start + i * self.ppq, self.ppq * 3 / 4, keys[0], 36.0 + 30.0 * it + accent);
                    }
                } else if self.full(b) > 0.2 {
                    self.note(Inst::Cello, start, self.bar + e / 2, keys[0], 40.0 + 35.0 * it);
                }
            } else if self.strings {
                self.note(Inst::Cello, start, self.bar + e / 2, keys[0], 45.0 + 30.0 * it);
                let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 55, 76, true);
                prev = v.clone();
                for k in &self.thin(v) {
                    self.note(Inst::StringPad, start, self.bar + e / 2, *k, 30.0 + 35.0 * it);
                }
            }
        }
        let insts: Vec<Option<Inst>> = (0..self.bars)
            .map(|b| {
                let it = self.it(b);
                if it > 0.6 && self.strings {
                    Some(Inst::Violin)
                } else if it > 0.3 && self.piano {
                    Some(Inst::Piano)
                } else if it > 0.3 {
                    Some(Inst::Violin)
                } else {
                    None
                }
            })
            .collect();
        let mut rng = self.rng("melody");
        let mut cfg = MelodyCfg {
            lo: 7,
            hi: 15,
            patterns: PAT_MOVING_44,
            phrase_bars: 4,
            vel: 52.0,
            rest_bar_prob: 0.25 * (1.0 - self.p.density as f64),
            ornaments: false,
        };
        self.vary_melody(&mut cfg);
        let m = self.melody(&mut rng, &cfg, &insts);
        self.notes.extend(m);
    }

    fn rainfall(&mut self) {
        let hr = 2;
        self.plan_harmony(hr, [0.2, 0.4, 0.1, 0.0, 0.3, 0.0], &[]);
        let mut rng = self.rng("bed");
        let mut prev = Vec::new();
        // from version 3, the left hand may ripple slowly up through the chord
        let ripple = self.variant("bed", 2) == 1;
        for b in (0..self.bars).step_by(hr as usize) {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let span = hr.min(self.bars - b) * self.bar;
            let start = b * self.bar;
            let bass = self.bass_key(&chord, 36);
            if self.piano && ripple && span >= 2 * self.bar {
                let mut third = self.scale.midi(chord.root + Self::third_of(&chord));
                while third - bass > 19 {
                    third -= 12;
                }
                for (i, k) in [bass, bass + 7, bass + 12, third].iter().enumerate() {
                    let at = start + i as u32 * span / 4;
                    self.note(Inst::Piano, at, span - i as u32 * span / 4, *k, 32.0 + 12.0 * it - i as f64 * 3.0);
                }
            } else if self.piano {
                self.note(Inst::Piano, start, span, bass, 32.0 + 12.0 * it);
                // busy: a soft fifth and octave answer the bass
                if self.density() > 0.65 && self.full(b) > 0.4 && span >= 2 * self.bar {
                    self.note(Inst::Piano, start + span / 2, span / 2, bass + 7, 26.0 + 10.0 * it);
                    self.note(Inst::Piano, start + span / 2 + self.ppq, span / 2 - self.ppq, bass + 12, 24.0 + 10.0 * it);
                }
            }
            if self.strings {
                let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 48, 72, rng.chance(0.5));
                prev = v.clone();
                for k in &self.thin(v) {
                    self.note(Inst::StringPad, start, span + self.ppq, *k, 26.0 + 30.0 * it);
                }
                self.swell(Inst::StringPad, start, span, 45.0, 70.0 + 30.0 * it, 50.0);
                if self.full(b) > 0.5 || !self.piano {
                    self.note(Inst::Cello, start, span + self.ppq, bass, 30.0 + 30.0 * it);
                }
            }
        }
        let mut rng = self.rng("drops");
        let d = self.p.density as f64;
        let s16 = self.ppq / 4;
        let drop_inst = if self.piano { Inst::Piano } else { Inst::Pizzicato };
        // from version 3: a light drizzle of single high drops, or showers,
        // where quiet bars alternate with bursts of falling runs
        let rain = self.variant("drops", 3);
        for b in 0..self.bars {
            let it = self.it(b);
            let mean = match rain {
                1 => 0.9 + d * 5.0 * it,
                2 if b % 4 >= 2 => 1.0 + d * 6.0 * it,
                2 => 0.3 + d * 1.5 * it,
                _ => 0.6 + d * 4.5 * it,
            };
            let n = rng.poisson(mean);
            for _ in 0..n {
                let pos = b * self.bar + rng.range(0, (self.bar / s16) as i32 - 1) as u32 * s16;
                let mut deg = if rain == 1 { rng.range(9, 22) } else { rng.range(5, 20) };
                while !self.scale.is_pentatonic(deg) {
                    deg += 1;
                }
                let cascade = match rain {
                    1 => 1,
                    2 if rng.chance(0.35) => rng.range(3, 6),
                    _ if rng.chance(0.15) => rng.range(2, 4),
                    _ => 1,
                };
                for c in 0..cascade {
                    let key = self.scale.midi(deg);
                    let vel = 24.0 + 34.0 * it + rng.gauss() * 6.0 - c as f64 * 4.0;
                    let dur = if drop_inst == Inst::Piano {
                        rng.range(3, 8) as u32 * self.ppq
                    } else {
                        self.ppq / 2
                    };
                    self.note(drop_inst, pos + c as u32 * s16, dur, key, vel);
                    deg -= 1;
                    while !self.scale.is_pentatonic(deg) {
                        deg -= 1;
                    }
                }
            }
        }
    }

    fn cinematic_swell(&mut self) {
        self.plan_harmony(1, [0.35, 0.25, 0.2, 0.0, 0.1, 0.1], &[]);
        let mut prev = Vec::new();
        let mut prev_pulse = Vec::new();
        // the pulse: repeated chords; from version 3 also a broken-chord
        // ostinato, or chords in a 3+3+2 rhythm
        let pulse = self.variant("pulse", 3);
        let e = self.ppq / 2;
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let start = b * self.bar;
            let bass = self.bass_key(&chord, 36);
            let tail = self.ppq / 3;
            let full = self.full(b);
            if self.strings {
                self.note(Inst::Cello, start, self.bar + tail, bass, 45.0 + 40.0 * it);
                self.swell(Inst::Cello, start, self.bar, 60.0 + 20.0 * it, 90.0 + 30.0 * it, 70.0);
                if full > 0.3 {
                    let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 50, 72, false);
                    prev = v.clone();
                    for k in &self.thin(v) {
                        self.note(Inst::StringPad, start, self.bar + tail, *k, 30.0 + 55.0 * it);
                    }
                    self.swell(Inst::StringPad, start, self.bar, 55.0 + 20.0 * it, 85.0 + 40.0 * it, 60.0 + 20.0 * it);
                }
            } else {
                self.note(Inst::Piano, start, self.bar, bass, 45.0 + 20.0 * it);
                self.note(Inst::Piano, start, self.bar, bass - 12, 35.0 + 20.0 * it);
            }
            // Pulsing chords
            let pulse_inst = if self.piano { Inst::Piano } else { Inst::Pizzicato };
            let v = voice_lead(&chord.pitch_classes(&self.scale), &prev_pulse, 60, 79, false);
            prev_pulse = v.clone();
            let (step, n) = if full < 0.4 {
                (2 * self.ppq, 2)
            } else if self.density() > 0.65 && full > 0.8 {
                (self.ppq / 2, 8)
            } else {
                (self.ppq, 4)
            };
            let v = if self.density() < 0.3 { self.thin(v) } else { v };
            if (full > 0.2 || !self.strings) && pulse == 1 && full >= 0.4 {
                const ORDER: [usize; 8] = [0, 1, 2, 1, 3, 1, 2, 1];
                for (i, o) in ORDER.iter().enumerate() {
                    let k = v[o % v.len()];
                    let accent = if i == 0 { 6.0 } else if i % 2 == 0 { 2.0 } else { 0.0 };
                    self.note(pulse_inst, start + i as u32 * e, 2 * e, k, 30.0 + 26.0 * it + accent);
                }
            } else if (full > 0.2 || !self.strings) && pulse == 2 && full >= 0.4 {
                for (on, len) in [(0, 3), (3, 3), (6, 2)] {
                    for k in &v {
                        let accent = if on == 0 { 6.0 } else { 2.0 };
                        self.note(pulse_inst, start + on * e, len * e, *k, 28.0 + 26.0 * it + accent);
                    }
                }
            } else if full > 0.2 || !self.strings {
                for i in 0..n {
                    for k in &v {
                        let accent = if i == 0 { 6.0 } else { 0.0 };
                        self.note(pulse_inst, start + i * step, step, *k, 28.0 + 26.0 * it + accent);
                    }
                }
            }
        }
        let theme_inst = if self.strings { Inst::Violin } else { Inst::Piano };
        let insts: Vec<Option<Inst>> =
            (0..self.bars).map(|b| (self.full(b) > 0.5).then_some(theme_inst)).collect();
        let mut rng = self.rng("melody");
        let mut cfg = MelodyCfg {
            lo: 9,
            hi: 17,
            patterns: PAT_SLOW_44,
            phrase_bars: 4,
            vel: 58.0,
            rest_bar_prob: 0.0,
            ornaments: false,
        };
        self.vary_melody(&mut cfg);
        let m = self.melody(&mut rng, &cfg, &insts);
        // Double the theme an octave down at the climax.
        let climax: Vec<Note> = m
            .iter()
            .filter(|n| self.it(n.start / self.bar) > 0.85 && self.strings)
            .map(|n| Note { key: n.key.saturating_sub(12), vel: n.vel.saturating_sub(8), ..*n })
            .collect();
        self.notes.extend(m);
        self.notes.extend(climax);
    }

    fn nocturne(&mut self) {
        self.plan_harmony(1, [0.3, 0.3, 0.3, 0.0, 0.0, 0.1], &[]);
        let e = self.ppq / 2;
        let arp_inst = if self.piano { Inst::Piano } else { Inst::Pizzicato };
        let mut prev = Vec::new();
        // the left hand's figure; from version 3 also one climbing through
        // the octave, or a wide sweep over two octaves
        let figure = self.variant("figure", 3);
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let start = b * self.bar;
            let low = self.scale.degree_near(self.bass_key(&chord, 36));
            let third = if matches!(chord.ext, Ext::Sus4) { 3 } else if matches!(chord.ext, Ext::Sus2) { 1 } else { 2 };
            let offs = match figure {
                1 => [0, 4, 7, 7 + third, 11, 7 + third],
                2 => [0, 7, 7 + third, 14, 7 + third, 7],
                _ => [0, 4, 7 + third, 11, 7 + third, 4],
            };
            let full = self.full(b);
            for (i, o) in offs.iter().enumerate() {
                // sparse: bass, the top of the arpeggio and the turn back
                if self.density() < 0.3 && matches!(i, 1 | 5) {
                    continue;
                }
                let key = self.scale.midi(low + o);
                let dur = if arp_inst == Inst::Piano { (6 - i as u32) * e + e } else { e };
                let vel = if i == 0 { 48.0 } else { 36.0 } + 15.0 * it;
                self.note(arp_inst, start + i as u32 * e, dur, key, vel);
                // busy: soft semiquavers ripple between the upper notes
                if self.density() > 0.65 && full > 0.5 && (1..5).contains(&i) {
                    let up = self.scale.midi(low + offs[i + 1] + 7);
                    self.note(arp_inst, start + i as u32 * e + e / 2, e, up, vel - 12.0);
                }
            }
            if self.strings {
                if !self.piano || full > 0.65 {
                    self.note(Inst::Cello, start, self.bar + e / 2, self.scale.midi(low), 38.0 + 30.0 * it);
                }
                if full > 0.5 || !self.piano {
                    let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 55, 74, false);
                    prev = v.clone();
                    for k in &v {
                        self.note(Inst::StringPad, start, self.bar + e / 2, *k, 22.0 + 40.0 * it);
                    }
                }
            }
        }
        if self.strings {
            self.ctrls.push(Ctrl { tick: 0, inst: Inst::StringPad, cc: 11, value: 95 });
        }
        let mel_inst = if self.piano { Inst::Piano } else { Inst::Violin };
        let insts: Vec<Option<Inst>> = (0..self.bars)
            .map(|b| (b >= 1 && b + 1 < self.bars).then_some(mel_inst))
            .collect();
        let mut rng = self.rng("melody");
        let mut cfg = MelodyCfg {
            lo: 7,
            hi: 17,
            patterns: PAT_68,
            phrase_bars: 4,
            vel: 54.0,
            rest_bar_prob: 0.15 * (1.0 - self.p.density as f64),
            ornaments: true,
        };
        self.vary_melody(&mut cfg);
        let m = self.melody(&mut rng, &cfg, &insts);
        self.notes.extend(m);
    }

    /// Move a chord root off the scale's diminished triad.
    fn fix_dim(&self, mut d: i32) -> i32 {
        let mut guard = 0;
        while self.scale.is_diminished(d) && guard < 7 {
            d = (d + 5).rem_euclid(7);
            guard += 1;
        }
        d
    }

    /// The nearest scale degree to `deg` whose pitch is a tone of the tonic
    /// triad, strictly below it (`below`) or strictly above it.
    fn triad_tone(&self, deg: i32, below: bool) -> i32 {
        let mut d = deg;
        loop {
            d += if below { -1 } else { 1 };
            if matches!(d.rem_euclid(7), 0 | 2 | 4) {
                return d;
            }
        }
    }

    fn tintinnabuli(&mut self) {
        // One tonic triad throughout: it is the "bells" every other voice rings against.
        self.harmony = vec![Chord::new(0, Ext::Triad); self.bars as usize];
        self.section = vec![0; self.bars as usize];
        let mut rng = self.rng("mvoice");
        let d = self.p.density as f64;
        let v = self.p.variation as f64;
        let beat = self.ppq;

        // The melody: phrases that walk stepwise back to a centre note, each pair
        // (one from above, one from below) a step longer, then shorter again.
        // from version 3 the bells may fall instead of rise, or take the
        // triad tone above the melody (Pärt's "superior" position), and the
        // melody may move at half speed
        let bells = self.variant("bells", 3);
        let slow = self.variant("pace", 2) == 1 && d <= 0.65;
        let unit = if d > 0.65 {
            self.bar / 2
        } else if slow {
            2 * self.bar
        } else {
            self.bar
        };
        let units = (self.bars * self.bar).div_ceil(unit) as usize;
        let centre = if rng.chance(0.5) { 7 } else { 4 };
        let longest = 3 + (v * 4.0).round() as i32;
        let mut mel: Vec<Option<i32>> = Vec::new();
        let (mut len, mut grow, mut above) = (1, true, true);
        while mel.len() < units {
            for i in (0..=len).rev() {
                mel.push(Some(centre + if above { i } else { -i }));
            }
            if rng.chance(0.65 - 0.45 * d) {
                mel.push(None);
            }
            if !above {
                if grow && len >= longest {
                    grow = false;
                } else if !grow && len <= 1 {
                    grow = true;
                }
                len += if grow { 1 } else { -1 };
            }
            above = !above;
        }
        mel.truncate(units);
        // the first unit is the accompaniment alone; the last settles on the tonic
        mel[0] = None;
        if let Some(last) = mel.last_mut() {
            *last = Some(7);
        }

        let m_inst = if self.strings { Inst::Violin } else { Inst::Piano };
        let fig_inst = if self.piano { Inst::Piano } else { Inst::Pizzicato };
        let mut current = centre;
        for (u, m) in mel.iter().enumerate() {
            let start = u as u32 * unit;
            let b = start / self.bar;
            let it = self.it(b);
            if let Some(m) = *m {
                current = m;
                let key = self.scale.midi(m);
                self.note(m_inst, start, unit + beat / 4, key, 44.0 + 26.0 * it);
                if m_inst == Inst::Violin {
                    self.swell(Inst::Violin, start, unit, 55.0, 85.0 + 25.0 * it, 60.0);
                }
            }
            // The bells: a rising triad figure ending on the triad tone just
            // below the melody, a crotchet at a time.
            let top = self.triad_tone(current, bells != 2) - 7;
            let fig = [self.triad_tone(self.triad_tone(top, true), true), self.triad_tone(top, true), top];
            let fig = if bells == 1 { [fig[2], fig[1], fig[0]] } else { fig };
            // sparse: a bell every other beat; busy: quavers at the fuller moments
            let step = if d < 0.3 {
                2 * beat
            } else if d > 0.65 && self.full(b) > 0.6 {
                beat / 2
            } else {
                beat
            };
            for i in 0..(unit / step) {
                let deg = fig[(i % 3) as usize];
                let vel = 30.0 + 14.0 * it + if i % 3 == 0 { 5.0 } else { 0.0 } - if step < beat { 4.0 } else { 0.0 };
                let dur = if fig_inst == Inst::Piano { 2 * step } else { step };
                self.note(fig_inst, start + i * step, dur, self.scale.midi(deg), vel);
            }
            // The mirror: the melody reflected around the centre, low down.
            if start % self.bar == 0 {
                let mirror = self.triad_tone(centre - (current - centre) + 1, true) - 14;
                let key = self.scale.midi(mirror);
                let len = self.bar.max(unit);
                if self.piano {
                    self.note(Inst::Piano, start, len, key, 36.0 + 14.0 * it);
                }
                if self.strings && (!self.piano || self.full(b) > 0.6) {
                    self.note(Inst::Cello, start, len + beat / 4, key, 34.0 + 26.0 * it);
                }
            }
        }
        // A faint sustained triad behind it all at the fuller moments.
        if self.strings {
            let triad = [self.scale.midi(0), self.scale.midi(2), self.scale.midi(4)];
            for b in (0..self.bars).step_by(2) {
                let it = self.it(b);
                if self.full(b) > 0.5 || !self.piano {
                    let span = 2.min(self.bars - b) * self.bar;
                    for k in triad {
                        self.note(Inst::StringPad, b * self.bar, span + beat / 2, k, 18.0 + 26.0 * it);
                    }
                }
            }
        }
    }

    fn chorale(&mut self) {
        self.plan_harmony(1, [0.45, 0.15, 0.15, 0.0, 0.1, 0.15], &[]);
        let mut rng = self.rng("chorale");
        let v = self.p.variation as f64;
        let half = self.bar / 2;
        // from version 3 the chorale may breathe every two bars
        let phrase = if self.variant("phrase", 2) == 1 { 2 } else { 4 };
        // from version 3: the piano may roll its chords like a harp, or the
        // whole chorale sit higher, lighter, like trebles
        let texture = self.variant("texture", 3);
        let (vlo, vhi, blo) = if texture == 2 { (60, 79, 45) } else { (55, 74, 38) };
        let mut prev: Vec<i32> = Vec::new();
        let mut prev_bass = self.bass_key(&self.harmony[0], blo);
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let phrase_end = b % phrase == phrase - 1 || b + 1 == self.bars;
            if b % phrase == 0 && self.strings {
                let span = phrase.min(self.bars - b) * self.bar;
                self.swell(Inst::StringPad, b * self.bar, span, 50.0 + 20.0 * it, 80.0 + 35.0 * it, 50.0 + 20.0 * it);
            }
            // Suspensions resolve on beat 3; otherwise a passing chord sometimes leads on.
            let second = if phrase_end {
                None
            } else if matches!(chord.ext, Ext::Sus4 | Ext::Sus2) {
                Some(Chord::new(chord.root, Ext::Triad))
            } else if rng.chance((0.2 + 0.4 * v) * (0.4 + 1.2 * self.density())) {
                Some(Chord::new(self.fix_dim(chord.root + *rng.pick(&[3, 5, -2])), Ext::Triad))
            } else {
                None
            };
            let halves = match second {
                Some(c2) => vec![(0, half, chord), (half, half, c2)],
                None => vec![(0, self.bar, chord)],
            };
            for (off, dur, c) in halves {
                let start = b * self.bar + off;
                // a breath before the next phrase
                let len = if phrase_end && off + dur == self.bar { dur - self.ppq / 3 } else { dur + self.ppq / 8 };
                let mut pcs = c.pitch_classes(&self.scale);
                pcs.truncate(4);
                let voicing = voice_lead(&pcs, &prev, vlo, vhi, false);
                prev = voicing.clone();
                // Bass: the root, unless the third or fifth moves more smoothly (an inversion).
                let root = self.bass_key(&c, blo);
                let mut bass = root;
                if !phrase_end {
                    for o in [2, 4] {
                        let k = self.bass_key(&Chord::new(c.root + o, Ext::Triad), blo);
                        if (k - prev_bass).abs() + 3 < (bass - prev_bass).abs() {
                            bass = k;
                        }
                    }
                }
                prev_bass = bass;
                let vel = 40.0 + 30.0 * it;
                let (top, inner) = voicing.split_last().map(|(t, r)| (*t, r.to_vec())).unwrap_or((72, Vec::new()));
                // busy: over a whole-bar chord the bass steps to another chord tone on beat 3
                let walk = self.density() > 0.65 && dur == self.bar && !phrase_end;
                let bass_notes: Vec<(u32, u32, i32)> = if walk {
                    let mut other = self.bass_key(&Chord::new(c.root + 4, Ext::Triad), blo);
                    if other == bass {
                        other = self.bass_key(&Chord::new(c.root + 2, Ext::Triad), blo);
                    }
                    vec![(0, half, bass), (half, len - half, other)]
                } else {
                    vec![(0, len, bass)]
                };
                if self.strings {
                    self.note(Inst::Violin, start, len, top, vel + 4.0);
                    for k in &inner {
                        self.note(Inst::StringPad, start, len, *k, vel - 4.0);
                    }
                    for &(o, l, k) in &bass_notes {
                        self.note(Inst::Cello, start + o, l, k, vel);
                    }
                }
                // sparse: with strings, the piano leaves the chorale to them
                if self.piano && (!self.strings || self.density() >= 0.3) {
                    let soft = if self.strings { 0.7 } else { 1.0 };
                    // rolled: from the bass up, a little apart
                    let roll = if texture == 1 { self.ppq / 10 } else { 0 };
                    let n = inner.len() as u32 + 1;
                    self.note(Inst::Piano, start + n * roll, len - n * roll, top, (vel + 6.0) * soft);
                    for (i, k) in inner.iter().enumerate() {
                        self.note(Inst::Piano, start + (i as u32 + 1) * roll, len - (i as u32 + 1) * roll, *k, (vel - 6.0) * soft);
                    }
                    for &(o, l, k) in &bass_notes {
                        self.note(Inst::Piano, start + o, l, k, vel * soft);
                    }
                }
            }
        }
    }

    fn canon(&mut self) {
        // A four-bar ground, repeated throughout.
        let mut rng = self.rng("ground");
        let pool: &[[i32; 4]] = if self.p.mode.is_minor() {
            &[[0, 6, 5, 4], [0, 5, 3, 4], [0, 3, 6, 2], [0, 6, 5, 6]]
        } else {
            &[[0, 4, 5, 2], [0, 4, 5, 3], [0, 5, 3, 4], [0, 2, 3, 4], [0, 5, 1, 4]]
        };
        let ground = *rng.pick(pool);
        let g: Vec<Chord> = ground
            .iter()
            .map(|d| Chord::new(self.fix_dim(*d), *rng.pick(&[Ext::Triad, Ext::Triad, Ext::Add9, Ext::Sus2])))
            .collect();
        let gl = 4u32;
        self.harmony = (0..self.bars).map(|b| g[(b % gl) as usize].clone()).collect();
        let last = self.bars as usize - 1;
        self.harmony[last] = Chord::new(0, Ext::Add9);
        self.section = vec![0; self.bars as usize];

        // The bass: the root, then the chord's third, every bar. From version
        // 3 it may walk in crotchets throughout, or swing between the root and
        // the fifth below.
        let half = self.bar / 2;
        let bass_inst = if self.strings { Inst::Cello } else { Inst::Piano };
        let bass_kind = self.variant("bass", 3);
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let root = self.bass_key(&chord, 36);
            let start = b * self.bar;
            if b as usize == last {
                self.note(bass_inst, start, self.bar, root, 44.0 + 20.0 * it);
                continue;
            }
            let mut third = self.scale.midi(chord.root + 2);
            while third > root + 6 {
                third -= 12;
            }
            while third < root - 5 {
                third += 12;
            }
            let d = self.density();
            if bass_kind == 2 && d >= 0.3 {
                let below = if root - 5 >= 33 { root - 5 } else { root + 7 };
                self.note(bass_inst, start, half + self.ppq / 8, root, 44.0 + 22.0 * it);
                self.note(bass_inst, start + half, half + self.ppq / 8, below, 36.0 + 22.0 * it);
            } else if d < 0.3 {
                // sparse: one bass note a bar
                self.note(bass_inst, start, self.bar + self.ppq / 8, root, 44.0 + 22.0 * it);
            } else if (d > 0.65 && self.full(b) > 0.5) || (bass_kind == 1 && self.full(b) > 0.3) {
                // busy: a crotchet bass through the chord
                let mut fifth = self.scale.midi(chord.root + 4);
                while fifth > root + 8 {
                    fifth -= 12;
                }
                let walk = [root, fifth, third, fifth];
                for (i, k) in walk.iter().enumerate() {
                    let vel = if i == 0 { 44.0 } else { 36.0 } + 22.0 * it;
                    self.note(bass_inst, start + i as u32 * self.ppq, self.ppq + self.ppq / 8, *k, vel);
                }
            } else {
                self.note(bass_inst, start, half + self.ppq / 8, root, 44.0 + 22.0 * it);
                self.note(bass_inst, start + half, half + self.ppq / 8, third, 38.0 + 22.0 * it);
            }
        }
        // With only the piano, a soft broken chord fills the middle.
        if !self.strings {
            for b in 0..self.bars {
                let chord = self.harmony[b as usize].clone();
                let it = self.it(b);
                let degs = chord.degrees();
                // sparse: minims; busy: quavers
                let d = self.density();
                let (step, n) = if d < 0.3 { (2 * self.ppq, 2) } else if d > 0.65 { (self.ppq / 2, 8) } else { (self.ppq, 4) };
                for i in 0..n {
                    let deg = degs[(i as usize) % 3] - 7 + if i % 4 == 3 { 7 } else { 0 };
                    let key = self.scale.midi(deg);
                    self.note(Inst::Piano, b * self.bar + i * step, 2 * step, key, 26.0 + 14.0 * it);
                }
            }
        }

        // The canon: a leader plays a new line over each ground; each follower
        // plays the same line one ground later, so it always fits the harmony.
        let d = self.p.density as f64;
        let (leader, voices): (Inst, Vec<(Inst, i32, f64)>) = match (self.piano, self.strings) {
            (true, true) => (Inst::Violin, vec![(Inst::Piano, 0, 0.85), (Inst::StringPad, -12, 0.75)]),
            (true, false) => (Inst::Piano, vec![(Inst::Piano, 0, 0.8)]),
            _ => (Inst::Violin, vec![(Inst::StringPad, -12, 0.85), (Inst::Pizzicato, 0, 0.8)]),
        };
        let entry = if self.bars >= 16 { gl } else { 0 };
        let insts: Vec<Option<Inst>> = (0..self.bars).map(|b| (b >= entry).then_some(leader)).collect();
        let mut rng = self.rng("melody");
        let cfg = MelodyCfg {
            lo: 7,
            hi: 14,
            // from version 3 the line may move more, with grace notes, or less
            patterns: match self.variant("line", 3) {
                1 => PAT_MOVING_44,
                2 => PAT_SLOW_44,
                _ if d > 0.5 => PAT_MOVING_44,
                _ => PAT_SLOW_44,
            },
            phrase_bars: gl,
            vel: 50.0,
            rest_bar_prob: 0.0,
            ornaments: self.variant("line", 3) == 1,
        };
        let line = self.melody(&mut rng, &cfg, &insts);
        let end = last as u32 * self.bar;
        for (n, (inst, shift, gain)) in voices.into_iter().enumerate() {
            let delay = (n as u32 + 1) * gl * self.bar;
            for note in &line {
                let start = note.start + delay;
                if start >= end {
                    continue;
                }
                let b = start / self.bar;
                // the third voice only joins when the music is fuller
                if n == 1 && (self.it(b) < 0.45 || d < 0.3) {
                    continue;
                }
                let key = (note.key as i32 + shift).clamp(21, 108) as u8;
                let vel = (note.vel as f64 * gain).max(1.0) as u8;
                self.notes.push(Note { start, dur: note.dur.min(end - start + self.bar), key, vel, inst });
            }
        }
        self.notes.extend(line);
    }

    fn tidal_drone(&mut self) {
        let hr = 4;
        self.plan_harmony(hr, [0.1, 0.4, 0.0, 0.0, 0.35, 0.15], &[&[0, 3, 0, 4], &[0, 5, 0, 3]]);
        let tonic = self.bass_key(&Chord::new(0, Ext::Triad), 36);
        // from version 3: slower tides, an octave above the drone, the fifth
        // entering a bar after the tonic, and bells that ring in pairs
        let wave = if self.variant("tide", 2) == 1 { 8 * self.bar } else { 4 * self.bar };
        let drone = self.variant("drone", 3);
        let pairs = self.variant("bells", 2) == 1;

        // The drone: tonic and fifth, renewed every eight bars, overlapping.
        let mut b = 0;
        while b < self.bars {
            let it = self.it(b);
            let start = b * self.bar;
            let span = (8.min(self.bars - b)) * self.bar;
            if self.strings {
                self.note(Inst::Cello, start, span + self.bar / 2, tonic, 34.0 + 24.0 * it);
                let late = if drone == 2 { self.bar.min(span / 2) } else { 0 };
                self.note(Inst::Cello, start + late, span - late + self.bar / 2, tonic + 7, 28.0 + 20.0 * it);
                if drone == 1 {
                    self.note(Inst::Cello, start, span + self.bar / 2, tonic + 12, 22.0 + 18.0 * it);
                }
            } else {
                for k in (0..span).step_by((2 * self.bar) as usize) {
                    self.note(Inst::Piano, start + k, 2 * self.bar, tonic, 30.0 + 12.0 * it);
                    self.note(Inst::Piano, start + k + self.ppq / 5, 2 * self.bar, tonic + 7, 24.0 + 10.0 * it);
                }
            }
            b += 8;
        }
        // Tides: everything sustained breathes in and out every four bars.
        if self.strings {
            let mut t = 0;
            while t < self.bars * self.bar {
                let it = self.it(t / self.bar);
                self.swell(Inst::Cello, t, wave, 55.0, 85.0 + 25.0 * it, 55.0);
                self.swell(Inst::StringPad, t, wave, 35.0 + 20.0 * it, 75.0 + 45.0 * it, 40.0 + 20.0 * it);
                t += wave;
            }
        }

        // Wide, slow chords.
        let mut rng = self.rng("pads");
        let mut prev = Vec::new();
        for b in (0..self.bars).step_by(hr as usize) {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let span = hr.min(self.bars - b) * self.bar;
            let voicing = voice_lead(&chord.pitch_classes(&self.scale), &prev, 50, 81, true);
            prev = voicing.clone();
            if self.density() > 0.65 && span >= 4 * self.bar {
                // busy: the chord turns over halfway, moving to a new voicing
                let half = span / 2;
                self.pad(b * self.bar, half + self.bar / 2, &voicing, 38.0 + 36.0 * it);
                let turn = voice_lead(&chord.pitch_classes(&self.scale), &voicing, 50, 81, false);
                self.pad(b * self.bar + half, half + self.bar / 2, &turn, 34.0 + 34.0 * it);
            } else {
                self.pad(b * self.bar, span + self.bar / 2, &self.thin(voicing), 38.0 + 36.0 * it);
            }
            // High harmonics shimmering above, now and then.
            if self.strings && rng.chance(0.25 + 0.6 * self.p.density as f64 * it) {
                let degs = chord.degrees();
                let deg = degs[rng.range(0, degs.len() as i32 - 1) as usize].rem_euclid(7) + 14;
                let at = b * self.bar + rng.range(0, 1) as u32 * self.bar;
                let len = (span - (at - b * self.bar)).max(self.bar);
                self.note(Inst::Violin, at, len, self.scale.midi(deg), 24.0 + 26.0 * it);
            }
        }
        if self.strings {
            self.ctrls.push(Ctrl { tick: 0, inst: Inst::Violin, cc: 11, value: 80 });
        }

        // Distant bells.
        let mut rng = self.rng("bells");
        let bell = if self.piano { Inst::Piano } else { Inst::Pizzicato };
        for b in 0..self.bars {
            let it = self.it(b);
            let n = rng.poisson(0.12 + 0.6 * self.p.density as f64 * it);
            let chord = self.harmony[b as usize].clone();
            for _ in 0..n {
                let pos = b * self.bar + rng.range(0, 7) as u32 * (self.ppq / 2);
                let mut deg = rng.range(9, 16);
                while !chord.contains(deg) {
                    deg += 1;
                }
                let dur = rng.range(3, 6) as u32 * self.ppq;
                let vel = 22.0 + 24.0 * it + rng.gauss() * 3.0;
                self.note(bell, pos, dur, self.scale.midi(deg), vel);
                if pairs {
                    // the next chord tone up answers it
                    let mut up = deg + 1;
                    while !chord.contains(up) {
                        up += 1;
                    }
                    self.note(bell, pos + self.ppq / 2, dur, self.scale.midi(up), vel * 0.8);
                }
            }
        }
    }

    fn felt_ballad(&mut self) {
        let extra: &[&[i32]] = if self.p.mode.is_minor() {
            &[&[3, 6, 2, 5], &[0, 3, 6, 2]]
        } else {
            &[&[1, 4, 0, 5], &[3, 2, 1, 4], &[0, 5, 1, 4]]
        };
        self.plan_harmony(1, [0.05, 0.2, 0.45, 0.25, 0.05, 0.0], extra);
        let mut rng = self.rng("comp");
        let v = self.p.variation as f64;
        let e = self.ppq / 2;
        // Comping rhythms in eighths (onset, length), one for each section.
        const COMP: [&[(u32, u32)]; 4] = [&[(0, 6), (6, 2)], &[(0, 3), (3, 5)], &[(0, 4), (5, 3)], &[(0, 8)]];
        // from version 3, other comping: a 3-3-2 lilt, a soft chord just
        // after the beat, and pushes on beats 2 and 4
        const MORE: [&[(u32, u32)]; 4] = [&[(0, 3), (3, 3), (6, 2)], &[(1, 3), (4, 4)], &[(0, 2), (2, 4), (6, 2)], &[(0, 4), (4, 4)]];
        let sec_comp = [rng.range(0, 3) as usize, rng.range(0, 3) as usize, rng.range(0, 3) as usize];
        let comps = if self.variant("comp", 2) == 1 { MORE } else { COMP };
        // and the left hand may walk into the next chord
        let walk = self.variant("bass", 2) == 1;
        let mut prev = Vec::new();
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let next = self.harmony[((b + 1) as usize).min(self.harmony.len() - 1)].clone();
            let it = self.it(b);
            let start = b * self.bar;
            let bass = self.bass_key(&chord, 36);
            // rootless voicings: the bass has the root
            let mut pcs = chord.pitch_classes(&self.scale);
            if pcs.len() > 3 {
                pcs.remove(0);
            }
            let voicing = voice_lead(&pcs, &prev, 52, 71, false);
            prev = voicing.clone();
            let last = b + 1 == self.bars;
            let full = self.full(b);
            let comp: &[(u32, u32)] = if last || self.density() < 0.3 {
                &[(0, 8)]
            } else if self.density() > 0.65 && full > 0.5 {
                // busy: an extra push on the last quaver
                match comps[sec_comp[self.section[b as usize] as usize]] {
                    [(0, 8)] => &[(0, 4), (4, 3), (7, 1)],
                    [(0, 6), (6, 2)] => &[(0, 3), (3, 3), (6, 2)],
                    [(0, 3), (3, 5)] => &[(0, 3), (3, 4), (7, 1)],
                    [(0, 4), (5, 3)] => &[(0, 4), (5, 2), (7, 1)],
                    other => other,
                }
            } else {
                comps[sec_comp[self.section[b as usize] as usize]]
            };
            if self.piano && walk && !last {
                // root, fifth, then a scale step leading to the next root
                let to = self.bass_key(&next, 36);
                let approach = self.scale.midi(self.scale.degree_near(if to >= bass { to - 1 } else { to + 1 }));
                let fifth = if bass + 7 > 52 { bass - 5 } else { bass + 7 };
                self.note(Inst::Piano, start, 4 * e, bass, 40.0 + 14.0 * it);
                self.note(Inst::Piano, start + 4 * e, 2 * e, fifth, 32.0 + 10.0 * it);
                let lead = if approach == to { fifth } else { approach };
                self.note(Inst::Piano, start + 6 * e, 2 * e, lead, 30.0 + 10.0 * it);
            } else if self.piano {
                self.note(Inst::Piano, start, self.bar, bass, 40.0 + 14.0 * it);
                if !last && rng.chance(0.3 + 0.4 * v) {
                    self.note(Inst::Piano, start + 4 * e, 4 * e, bass + 7, 32.0 + 10.0 * it);
                }
            }
            if self.piano {
                for (on, dur) in comp {
                    for (i, k) in voicing.iter().enumerate() {
                        let roll = i as u32 * self.ppq / 24;
                        let vel = 30.0 + 16.0 * it + if *on == 0 { 4.0 } else { 0.0 };
                        self.note(Inst::Piano, start + on * e + roll, dur * e + e / 2, *k, vel);
                    }
                }
            }
            if self.strings {
                if !self.piano {
                    // a plucked walking bass: root, fifth, then a step into the next chord
                    let fifth = if bass + 7 > 52 { bass - 5 } else { bass + 7 };
                    let to = self.bass_key(&next, 36);
                    let approach = if to >= bass { to - 1 } else { to + 1 };
                    let approach = self.scale.midi(self.scale.degree_near(approach));
                    let d = self.density();
                    let steps: &[(u32, i32)] = if last {
                        &[(0, bass)]
                    } else if d < 0.3 {
                        &[(0, bass), (4, fifth)]
                    } else if d > 0.65 {
                        &[(0, bass), (2, bass + 12), (4, fifth), (6, approach)]
                    } else {
                        &[(0, bass), (4, fifth), (6, approach)]
                    };
                    for (on, k) in steps {
                        self.note(Inst::Pizzicato, start + on * e, 2 * e, *k, 46.0 + 16.0 * it);
                    }
                }
                if !self.piano || full > 0.4 {
                    for k in &voicing {
                        self.note(Inst::StringPad, start, self.bar + e / 2, *k, 22.0 + 34.0 * it);
                    }
                }
                if self.piano && full > 0.6 {
                    self.note(Inst::Cello, start, self.bar + e / 2, bass, 30.0 + 26.0 * it);
                }
            }
        }

        let mel_inst = if self.piano { Inst::Piano } else { Inst::Violin };
        let insts: Vec<Option<Inst>> =
            (0..self.bars).map(|b| (b >= 1 && b + 1 < self.bars).then_some(mel_inst)).collect();
        let mut rng = self.rng("melody");
        let mut cfg = MelodyCfg {
            lo: 7,
            hi: 15,
            patterns: PAT_MOVING_44,
            phrase_bars: 4,
            vel: 50.0,
            rest_bar_prob: 0.1 + 0.3 * (1.0 - self.p.density as f64),
            ornaments: true,
        };
        self.vary_melody(&mut cfg);
        let mut m = self.melody(&mut rng, &cfg, &insts);
        // Swing the off-beat eighths a little (from version 3: straight, or a
        // lazier swing).
        let lag = match self.variant("swing", 3) {
            1 => 0,
            2 => self.ppq / 4,
            _ => self.ppq / 6,
        };
        for n in m.iter_mut() {
            if n.start % self.ppq == e {
                n.start += lag;
                n.dur = n.dur.saturating_sub(lag).max(1);
            }
        }
        self.notes.extend(m);
    }

    /// Scale steps above a chord's root for its third, adjusted for suspensions.
    fn third_of(chord: &Chord) -> i32 {
        match chord.ext {
            Ext::Sus2 => 1,
            Ext::Sus4 => 3,
            _ => 2,
        }
    }

    fn berlin_school(&mut self) {
        self.plan_harmony(4, [0.2, 0.3, 0.1, 0.0, 0.3, 0.1], &[&[0, 0, 5, 5], &[0, 3, 0, 4], &[0, 5, 3, 4]]);
        let s16 = self.ppq / 4;
        let d = self.density();
        let v = self.p.variation as f64;
        let mut rng = self.rng("sequence");
        // Sequencer patterns in scale steps above the chord's root: 2 and 9 are the
        // third, 4 and 11 the fifth, 7 the octave.
        const PATTERNS: [[i32; 16]; 4] = [
            [0, 7, 4, 7, 2, 7, 4, 9, 0, 7, 4, 7, 2, 9, 4, 11],
            [0, 4, 7, 4, 9, 4, 7, 4, 0, 4, 7, 11, 9, 7, 4, 2],
            [0, 0, 7, 0, 4, 0, 9, 0, 0, 0, 7, 0, 11, 0, 9, 7],
            [0, 2, 4, 7, 4, 2, 0, 7, 0, 2, 4, 9, 7, 4, 2, 4],
        ];
        // from version 3, other sequences: rising and falling scales, a
        // pumping octave figure and a gated chord
        const MORE: [[i32; 16]; 4] = [
            [0, 2, 4, 7, 9, 11, 9, 7, 4, 2, 0, 2, 4, 7, 4, 2],
            [0, 7, 0, 7, 4, 11, 4, 11, 2, 9, 2, 9, 4, 11, 4, 7],
            [0, 4, 7, 11, 14, 11, 7, 4, 0, 4, 7, 11, 9, 7, 4, 2],
            [0, 0, 4, 4, 7, 7, 4, 4, 0, 0, 9, 9, 7, 7, 4, 4],
        ];
        let sec_pat = [rng.range(0, 3) as usize, rng.range(0, 3) as usize, rng.range(0, 3) as usize];
        let seq_inst = if self.strings { Inst::Pizzicato } else { Inst::Piano };
        let patterns = if self.variant("sequence", 2) == 1 { MORE } else { PATTERNS };
        // and the sequence may loop every 12 steps against the 16-step bar,
        // so it shifts against the beat as it goes round
        let loop_len = if self.variant("loop", 3) == 2 { 12 } else { 16 };
        let bass_kind = self.variant("bass", 3);
        let mut pattern = patterns[sec_pat[0]];
        let mut last_sec = 0;
        let mut prev = Vec::new();
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let full = self.full(b);
            let start = b * self.bar;
            let sec = self.section[b as usize] as usize;
            if sec != last_sec {
                pattern = patterns[sec_pat[sec % 3]];
                last_sec = sec;
            }
            // the pattern mutates slowly: now and then one step changes
            if b % 4 == 0 && b > 0 && rng.chance(v * 0.5) {
                let i = rng.range(1, 15) as usize;
                pattern[i] = *rng.pick(&[0, 2, 4, 7, 9, 11]);
            }
            let low = self.scale.degree_near(self.bass_key(&chord, 45));
            let third = Self::third_of(&chord);
            let last = b + 1 == self.bars;
            // sparse or quiet: quavers; otherwise semiquavers
            let step = if d < 0.3 || full < 0.25 { 2 } else { 1 };
            let steps = if last { 4 } else { 16 };
            for i in (0..steps).step_by(step) {
                if i > 0 && rng.chance(0.3 * (1.0 - d) * (1.0 - it)) {
                    continue;
                }
                let idx = if loop_len == 16 { i } else { (b as usize * 16 + i) % loop_len };
                let off = match pattern[idx] {
                    2 => third,
                    9 => 7 + third,
                    o => o,
                };
                let key = self.scale.midi(low + off);
                // accents on the beat and every third step, a three-against-four pulse
                let accent = if i == 0 { 10.0 } else if i % 3 == 0 { 5.0 } else { 0.0 };
                let vel = 30.0 + 26.0 * it + accent + rng.gauss() * 2.0;
                self.note(seq_inst, start + i as u32 * s16, step as u32 * s16, key, vel);
                // busy, at the peak: an octave echo on the off-steps
                if d > 0.65 && full > 0.8 && i % 2 == 1 {
                    self.note(seq_inst, start + i as u32 * s16, s16, key + 12, vel - 10.0);
                }
            }

            // The bass: a pedal on the chord's root, pulsing in quavers when busy.
            let bass = self.bass_key(&chord, 33);
            let bass_inst = if self.strings { Inst::Cello } else { Inst::Piano };
            if bass_kind == 1 && full > 0.4 && !last {
                // octave-jumping quavers
                for i in 0..8u32 {
                    let k = if i % 2 == 0 { bass } else { bass + 12 };
                    let vel = 36.0 + 22.0 * it + if i % 2 == 0 { 6.0 } else { 0.0 };
                    self.note(bass_inst, start + i * 2 * s16, 2 * s16, k, vel);
                }
            } else if bass_kind == 2 && !last {
                // a long pedal, held across each pair of bars
                if b % 2 == 0 {
                    let span = 2.min(self.bars - 1 - b).max(1) * self.bar;
                    self.note(bass_inst, start, span + s16, bass, 40.0 + 26.0 * it);
                }
            } else if d > 0.65 && full > 0.5 && !last {
                for i in 0..8u32 {
                    let vel = 38.0 + 22.0 * it + if i % 2 == 0 { 6.0 } else { 0.0 };
                    self.note(bass_inst, start + i * 2 * s16, 2 * s16, bass, vel);
                }
            } else {
                self.note(bass_inst, start, self.bar + s16, bass, 40.0 + 26.0 * it);
            }

            // Pads every four bars, once the piece has warmed up.
            if b % 4 == 0 && full > 0.3 {
                let span = 4.min(self.bars - b) * self.bar;
                let voicing = voice_lead(&chord.pitch_classes(&self.scale), &prev, 52, 76, false);
                prev = voicing.clone();
                self.pad(start, span + s16 * 2, &self.thin(voicing), 30.0 + 36.0 * it);
                if self.strings {
                    self.swell(Inst::StringPad, start, span, 45.0 + 20.0 * it, 80.0 + 35.0 * it, 55.0 + 20.0 * it);
                }
            }

            // Keys: a few high bell notes when there's room above the sequence.
            if self.piano && self.strings {
                let n = rng.poisson(0.15 + 1.2 * d * it);
                for _ in 0..n {
                    let pos = start + rng.range(0, 7) as u32 * 2 * s16;
                    let mut deg = rng.range(14, 18);
                    while !chord.contains(deg) {
                        deg += 1;
                    }
                    let dur = rng.range(2, 4) as u32 * self.ppq;
                    self.note(Inst::Piano, pos, dur, self.scale.midi(deg), 24.0 + 22.0 * it);
                }
            }
        }
        // A slow lead over the fuller stretches.
        if self.strings {
            let insts: Vec<Option<Inst>> = (0..self.bars).map(|b| (self.full(b) > 0.55).then_some(Inst::Violin)).collect();
            let mut rng = self.rng("melody");
            let mut cfg = MelodyCfg {
                lo: 9,
                hi: 16,
                patterns: PAT_SLOW_44,
                phrase_bars: 4,
                vel: 50.0,
                rest_bar_prob: 0.3,
                ornaments: false,
            };
            self.vary_melody(&mut cfg);
            let m = self.melody(&mut rng, &cfg, &insts);
            self.notes.extend(m);
        }
    }

    fn dub_chords(&mut self) {
        self.plan_harmony(2, [0.0, 0.15, 0.5, 0.25, 0.1, 0.0], &[&[0, 0, 3, 3], &[0, 5, 0, 5], &[0, 3, 5, 3]]);
        let s16 = self.ppq / 4;
        let d = self.density();
        let mut rng = self.rng("stabs");
        // Stab rhythms in semiquavers from the start of the bar, one for each section.
        const RHYTHMS: [&[u32]; 4] = [&[6, 14], &[2, 10], &[6, 11, 14], &[3, 6, 14]];
        // from version 3 also a skank on every off-beat, and sparser stabs
        const MORE: [&[u32]; 4] = [&[2, 6, 10, 14], &[4, 12], &[6, 9, 14], &[2, 7, 14]];
        let sec_r = [rng.range(0, 3) as usize, rng.range(0, 3) as usize, rng.range(0, 3) as usize];
        let rhythms = if self.variant("stabs", 2) == 1 { MORE } else { RHYTHMS };
        // the echo: dotted quavers, quavers, or crotchets
        let echo = match self.variant("echo", 3) {
            1 => 2 * s16,
            2 => 4 * s16,
            _ => 3 * s16,
        };
        // and the bass may play a line rather than a pulse
        let line = self.variant("bass", 2) == 1;
        let stab_inst = if self.piano { Inst::Piano } else { Inst::Pizzicato };
        let end = self.bars * self.bar;
        let mut prev = Vec::new();
        let mut prev_pad = Vec::new();
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let full = self.full(b);
            let start = b * self.bar;
            let last = b + 1 == self.bars;
            // rootless voicings: the bass has the root
            let mut pcs = chord.pitch_classes(&self.scale);
            if pcs.len() > 3 {
                pcs.remove(0);
            }
            let voicing = voice_lead(&pcs, &prev, 55, 72, false);
            prev = voicing.clone();
            let rhythm = rhythms[sec_r[self.section[b as usize] as usize % 3]];
            let hits: &[u32] = if last {
                &[0]
            } else if d < 0.3 {
                &rhythm[..1]
            } else {
                rhythm
            };
            // Each stab echoes away on dotted quavers, more of them when busy.
            let repeats = 2 + (d * 3.0).round() as u32 + u32::from(it > 0.6);
            for &h in hits {
                let mut t = start + h * s16;
                let mut vel = 40.0 + 26.0 * it;
                for r in 0..=repeats {
                    if t >= end + self.bar || vel < 8.0 {
                        break;
                    }
                    for k in &voicing {
                        self.note(stab_inst, t, 2 * s16, *k, vel);
                    }
                    // the echoes darken: their top note drops out
                    if r == 0 {
                        vel *= 0.62;
                    } else {
                        vel *= 0.6;
                    }
                    t += echo;
                }
            }

            // The sub bass: the root on the beat, and a push before beat 4.
            let bass = self.bass_key(&chord, 31);
            let mut bass_line_played = false;
            let bass_inst = if self.strings { Inst::Cello } else { Inst::Piano };
            if line && !last && d >= 0.3 {
                let fifth = if bass + 7 > 50 { bass - 5 } else { bass + 7 };
                for (on, len, k) in [(0, 5, bass), (6, 2, bass), (8, 3, fifth), (12, 2, bass + 12), (14, 2, fifth)] {
                    let accent = if on == 0 { 8.0 } else { 0.0 };
                    self.note(bass_inst, start + on * s16, len * s16, k, 38.0 + 22.0 * it + accent);
                }
                bass_line_played = true;
            } else {
                self.note(bass_inst, start, 6 * s16, bass, 46.0 + 22.0 * it);
            }
            if !bass_line_played && !last && d >= 0.3 {
                self.note(bass_inst, start + 10 * s16, 4 * s16, bass, 38.0 + 20.0 * it);
            }
            if !bass_line_played && !last && d > 0.65 && full > 0.5 {
                self.note(bass_inst, start + 14 * s16, 2 * s16, bass + 7, 32.0 + 18.0 * it);
            }

            // A pad sweeping slowly underneath, two bars at a time.
            if self.strings && b % 2 == 0 && full > 0.25 {
                let span = 2.min(self.bars - b) * self.bar;
                let v = voice_lead(&chord.pitch_classes(&self.scale), &prev_pad, 48, 67, false);
                prev_pad = v.clone();
                for k in &self.thin(v) {
                    self.note(Inst::StringPad, start, span + s16 * 2, *k, 26.0 + 28.0 * it);
                }
                self.swell(Inst::StringPad, start, span, 40.0, 80.0 + 30.0 * it, 50.0);
            }

            // Now and then a long, high note floats over the top.
            if self.strings && b % 4 == 0 && !last && rng.chance(0.2 + 0.5 * d * it) {
                let mut deg = rng.range(9, 14);
                while !chord.contains(deg) {
                    deg += 1;
                }
                let at = start + rng.range(0, 1) as u32 * self.bar;
                let len = (2 * self.bar).min(end - at);
                self.note(Inst::Violin, at, len, self.scale.midi(deg), 26.0 + 24.0 * it);
            }
        }
    }

    fn aurora(&mut self) {
        let hr = 4;
        self.plan_harmony(hr, [0.05, 0.45, 0.05, 0.1, 0.3, 0.05], &[&[0, 3, 0, 5], &[0, 5, 3, 0]]);
        let beat = self.ppq;
        let d = self.density();
        let mut rng = self.rng("aurora");
        let mut prev = Vec::new();
        let pad_inst = if self.strings { Inst::StringPad } else { Inst::Piano };
        // from version 3 the cluster may bloom from the top down, or from the
        // middle outwards; and the glow may sway between two notes
        let bloom = self.variant("bloom", 3);
        let sway = self.variant("glow", 2) == 1;
        for b in (0..self.bars).step_by(hr as usize) {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let full = self.full(b);
            let span = hr.min(self.bars - b) * self.bar;
            let start = b * self.bar;
            let voicing = voice_lead(&chord.pitch_classes(&self.scale), &prev, 48, 79, true);
            prev = voicing.clone();
            // The cluster blooms: its notes enter one by one, lowest first, and fade together.
            let voicing = self.thin(voicing);
            let gap = beat / 2 + rng.range(0, 2) as u32 * beat / 4;
            let n = voicing.len();
            for (i, k) in voicing.iter().enumerate() {
                let order = match bloom {
                    1 => n - 1 - i,
                    2 => (2 * i as i32 - n as i32 + 1).unsigned_abs() as usize / 2,
                    _ => i,
                };
                let delay = (order as u32 * gap).min(span / 2);
                let vel = if pad_inst == Inst::Piano { 30.0 + 24.0 * it } else { 40.0 + 40.0 * it };
                self.note(pad_inst, start + delay, span - delay + self.bar / 2, *k, vel);
            }
            if self.strings {
                self.swell(Inst::StringPad, start, span, 30.0 + 20.0 * it, 80.0 + 40.0 * it, 40.0 + 20.0 * it);
            }
            // A glow above: the chord's ninth, held high once the piece opens up.
            if self.strings && full > 0.35 && span > self.bar {
                let mut k = self.scale.midi(chord.root + 1 + 14);
                while k > 88 {
                    k -= 12;
                }
                if sway {
                    // the ninth and the fifth below it in turn, a bar each
                    let fifth = self.scale.midi(chord.root + 4 + 7);
                    let fifth = if fifth > k { fifth - 12 } else { fifth };
                    for (n, bb) in (1..span / self.bar).enumerate() {
                        let key = if n % 2 == 0 { k } else { fifth };
                        self.note(Inst::Violin, start + bb * self.bar, self.bar + beat / 2, key, 30.0 + 30.0 * it);
                    }
                } else {
                    self.note(Inst::Violin, start + self.bar, span - self.bar + beat, k, 32.0 + 32.0 * it);
                }
                self.swell(Inst::Violin, start + self.bar, span - self.bar, 40.0, 85.0 + 25.0 * it, 45.0);
            }
            // The drone: the chord's root, low and quiet.
            if full > 0.2 || !self.piano {
                let k = self.bass_key(&chord, 36);
                if self.strings {
                    self.note(Inst::Cello, start, span + beat, k, 34.0 + 26.0 * it);
                } else {
                    self.note(Inst::Piano, start, span, k, 26.0 + 12.0 * it);
                }
            }
        }
        // Crystal bells drifting down from above.
        let bell = if self.piano { Inst::Piano } else { Inst::Pizzicato };
        let mut rng = self.rng("bells");
        for b in 0..self.bars {
            let it = self.it(b);
            let chord = self.harmony[b as usize].clone();
            let n = rng.poisson(0.15 + 0.9 * d * it);
            for _ in 0..n {
                let pos = b * self.bar + rng.range(0, 7) as u32 * (beat / 2);
                let mut deg = rng.range(9, 18);
                while !chord.contains(deg) && !self.scale.is_pentatonic(deg) {
                    deg += 1;
                }
                let dur = rng.range(3, 6) as u32 * beat;
                self.note(bell, pos, dur, self.scale.midi(deg), 28.0 + 26.0 * it + rng.gauss() * 3.0);
            }
        }
    }

    fn lullaby(&mut self) {
        self.plan_harmony(1, [0.7, 0.2, 0.0, 0.0, 0.1, 0.0], &[&[0, 3, 0, 4], &[0, 5, 3, 4]]);
        let beat = self.ppq;
        let e = beat / 2;
        let d = self.density();
        // With strings, a harp rocks under the music box; otherwise the piano does.
        let rock = if self.strings { Inst::Pizzicato } else { Inst::Piano };
        let mut prev = Vec::new();
        // from version 3 the rocking may be a broken chord throughout, or a
        // barcarolle lilt: a long bass, then the chord on the third beat
        let rocking = self.variant("rock", 3);
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let full = self.full(b);
            let start = b * self.bar;
            let low = self.scale.degree_near(self.bass_key(&chord, 43));
            let third = Self::third_of(&chord);
            let vel = 42.0 + 16.0 * it;
            if b + 1 == self.bars {
                self.note(rock, start, self.bar, self.scale.midi(low), vel);
                self.note(rock, start + e / 2, self.bar, self.scale.midi(low + 4), vel - 6.0);
                continue;
            }
            if rocking == 2 && d >= 0.3 {
                self.note(rock, start, 2 * beat, self.scale.midi(low), vel + 2.0);
                self.note(rock, start + beat, beat, self.scale.midi(low + 4), vel - 10.0);
                for o in [7, 7 + third] {
                    self.note(rock, start + 2 * beat, beat + e, self.scale.midi(low + o), vel - 6.0);
                }
            } else if (d > 0.65 && full > 0.4) || (rocking == 1 && full > 0.25) {
                // busy: a broken chord rocking in quavers
                for (i, o) in [0, 4, 7, 7 + third, 7, 4].iter().enumerate() {
                    let v = if i == 0 { vel + 4.0 } else { vel - 6.0 };
                    self.note(rock, start + i as u32 * e, 2 * e, self.scale.midi(low + o), v);
                }
            } else {
                self.note(rock, start, 3 * beat, self.scale.midi(low), vel + 2.0);
                // sparse: the root alone; otherwise the chord on beats 2 and 3
                if d >= 0.3 {
                    for k in 1..3u32 {
                        for o in [4, 7 + third] {
                            self.note(rock, start + k * beat, beat, self.scale.midi(low + o), vel - 8.0);
                        }
                    }
                }
            }
            if self.strings && full > 0.4 {
                let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 55, 72, false);
                prev = v.clone();
                for k in &self.thin(v) {
                    self.note(Inst::StringPad, start, self.bar + beat / 4, *k, 22.0 + 28.0 * it);
                }
                if !self.piano || full > 0.6 {
                    self.note(Inst::Cello, start, self.bar, self.bass_key(&chord, 36), 26.0 + 20.0 * it);
                }
            }
        }
        if self.strings {
            self.ctrls.push(Ctrl { tick: 0, inst: Inst::StringPad, cc: 11, value: 90 });
        }
        let mel_inst = if self.piano { Inst::Piano } else { Inst::Violin };
        let insts: Vec<Option<Inst>> = (0..self.bars).map(|b| (b >= 2 && b + 1 < self.bars).then_some(mel_inst)).collect();
        let mut rng = self.rng("melody");
        let mut cfg = MelodyCfg {
            lo: 9,
            hi: 16,
            patterns: PAT_WALTZ,
            phrase_bars: 4,
            vel: 54.0,
            rest_bar_prob: 0.05 + 0.3 * (1.0 - d),
            ornaments: false,
        };
        self.vary_melody(&mut cfg);
        let m = self.melody(&mut rng, &cfg, &insts);
        self.notes.extend(m);
    }

    /// The scale degree at or below `deg` that is in `set` (degrees within the octave).
    fn snap_to(deg: i32, set: &[i32]) -> i32 {
        let mut d = deg;
        while !set.contains(&d.rem_euclid(7)) {
            d -= 1;
        }
        d
    }

    /// The next degree in `set` above (`dir` > 0) or below `deg`.
    fn step_in(deg: i32, set: &[i32], dir: i32) -> i32 {
        let mut d = deg + dir.signum();
        while !set.contains(&d.rem_euclid(7)) {
            d += dir.signum();
        }
        d
    }

    fn celtic_air(&mut self) {
        let hr = if self.density() < 0.5 { 2 } else { 1 };
        self.plan_harmony(hr, [0.5, 0.2, 0.0, 0.0, 0.15, 0.15], &[&[0, 6, 0, 6], &[0, 3, 0, 6], &[0, 6, 3, 0]]);
        let e = self.ppq / 2;
        let d = self.density();
        let mut rng = self.rng("harp");
        // Rolling harp figures in quavers, upwards from the chord's root (scale
        // steps: 4 the fifth, 7 the octave, and the third an octave up).
        const ROLLS: [[i32; 6]; 3] = [[0, 4, 7, 9, 7, 4], [0, 4, 7, 11, 9, 7], [0, 7, 9, 4, 7, 11]];
        let sec_roll = [rng.range(0, 2) as usize, rng.range(0, 2) as usize, rng.range(0, 2) as usize];
        let harp = if self.strings { Inst::Pizzicato } else { Inst::Piano };
        // from version 3: other rolls, rocking low or turning back down; the
        // harp in a lilting crotchet-quaver rhythm; and the low strings may
        // follow the chords, or hold a high drone like pipes
        const MORE: [[i32; 6]; 3] = [[0, 4, 7, 4, 0, 4], [0, 7, 11, 9, 7, 4], [0, 4, 9, 7, 4, 0]];
        let roll_kind = self.variant("roll", 3);
        let rolls = if roll_kind == 1 { MORE } else { ROLLS };
        let drone_kind = self.variant("drone", 3);
        let mut prev = Vec::new();
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let full = self.full(b);
            let start = b * self.bar;
            let low = self.scale.degree_near(self.bass_key(&chord, 43));
            let third = Self::third_of(&chord);
            let vel = 36.0 + 20.0 * it;
            if b + 1 == self.bars {
                for (i, o) in [0, 4, 7, 7 + third].iter().enumerate() {
                    self.note(harp, start + i as u32 * e / 3, self.bar, self.scale.midi(low + o), vel - i as f64 * 3.0);
                }
            } else if d < 0.3 || full < 0.2 {
                // sparse: the two beats of the bar
                self.note(harp, start, 3 * e, self.scale.midi(low), vel);
                self.note(harp, start + 3 * e, 3 * e, self.scale.midi(low + 7 + third), vel - 8.0);
            } else {
                let roll = rolls[sec_roll[self.section[b as usize] as usize % 3]];
                for (i, o) in roll.iter().enumerate() {
                    // the lilt: crotchet, quaver, crotchet, quaver
                    if roll_kind == 2 && (i == 1 || i == 4) {
                        continue;
                    }
                    let o = if *o == 9 || *o == 2 { o - 2 + third } else { *o };
                    let v = if i % 3 == 0 { vel + 4.0 } else { vel - 6.0 };
                    self.note(harp, start + i as u32 * e, 3 * e, self.scale.midi(low + o), v);
                }
            }
            // the drone: the tonic, and its fifth when the music fills out
            if self.strings && drone_kind == 1 {
                if b % hr == 0 {
                    let span = hr.min(self.bars - b) * self.bar;
                    self.note(Inst::Cello, start, span, self.bass_key(&chord, 36), 30.0 + 18.0 * it);
                }
            } else if self.strings && b % 2 == 0 {
                let span = 2.min(self.bars - b) * self.bar;
                let tonic = self.bass_key(&crate::theory::Chord::new(0, Ext::Triad), 38);
                let tonic = if drone_kind == 2 { tonic + 12 } else { tonic };
                self.note(Inst::Cello, start, span, tonic, 30.0 + 18.0 * it);
                if full > 0.55 || drone_kind == 2 {
                    self.note(Inst::Cello, start, span, tonic + 7, 24.0 + 14.0 * it);
                }
            }
            if self.strings && full > 0.45 && b % hr == 0 {
                let span = hr.min(self.bars - b) * self.bar;
                let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 55, 74, false);
                prev = v.clone();
                self.pad(start, span, &self.thin(v), 22.0 + 30.0 * it);
            }
        }
        // The air, with grace notes, on flute (or the piano's top when alone).
        let lead = if self.strings { Inst::Violin } else { Inst::Piano };
        let insts: Vec<Option<Inst>> = (0..self.bars)
            .map(|b| (b >= 2 && b + 1 < self.bars && self.full(b) > 0.2).then_some(lead))
            .collect();
        let mut rng = self.rng("melody");
        let mut cfg = MelodyCfg {
            lo: 7,
            hi: 15,
            patterns: PAT_68,
            phrase_bars: 4,
            vel: 52.0,
            rest_bar_prob: 0.1,
            ornaments: true,
        };
        self.vary_melody(&mut cfg);
        let m = self.melody(&mut rng, &cfg, &insts);
        // more grace notes than the other styles: a cut from above before long notes
        let mut graces = Vec::new();
        for n in &m {
            if n.dur >= 2 * e && n.start > 0 && rng.chance(0.25 + 0.35 * self.p.variation as f64) {
                let deg = self.scale.degree_near(n.key as i32);
                graces.push(Note {
                    start: n.start.saturating_sub(self.ppq / 8),
                    dur: self.ppq / 8,
                    key: self.scale.midi(deg + 1).clamp(21, 108) as u8,
                    vel: n.vel.saturating_sub(14).max(1),
                    inst: n.inst,
                });
            }
        }
        self.notes.extend(m);
        self.notes.extend(graces);
    }

    fn koto_garden(&mut self) {
        self.plan_harmony(4, [0.3, 0.0, 0.0, 0.0, 0.5, 0.2], &[&[0, 0, 0, 5], &[0, 3, 0, 0], &[0, 5, 0, 3]]);
        // Five notes: major pentatonic in the major modes, hirajoshi in minor,
        // an in-like scale in Phrygian.
        const SET: [i32; 5] = [0, 1, 2, 4, 5];
        let beat = self.ppq;
        let e = beat / 2;
        let s16 = beat / 4;
        let d = self.density();
        let v = self.p.variation as f64;
        let koto = if self.piano { Inst::Piano } else { Inst::Pizzicato };
        let mut rng = self.rng("koto");
        // from version 3: rising sweeps among the gestures, or a sparer,
        // meditative player; and the low fifth only every other phrase
        let player = self.variant("player", 3);
        let open_every = if self.variant("drone", 2) == 1 { 2 } else { 1 };
        let mut phrase = 0;
        let mut b = 0;
        while b < self.bars {
            let len = 2.min(self.bars - b);
            let it = self.it(b);
            let start = b * self.bar;
            let end = (b + len) * self.bar;
            let last = b + len >= self.bars;
            // a low open fifth on the tonic begins each phrase
            let root = self.scale.midi(-7);
            if phrase % open_every == 0 || last {
                self.note(koto, start, len * self.bar, root, 34.0 + 16.0 * it);
                if d > 0.35 {
                    self.note(koto, start + s16, len * self.bar, root + 7, 28.0 + 14.0 * it);
                }
            } else {
                // a single low note marks the phrase instead
                self.note(koto, start, len * self.bar, root + 7, 26.0 + 12.0 * it);
            }
            phrase += 1;
            if last {
                self.note(koto, start + e, self.bar, self.scale.midi(Self::snap_to(4, &SET)), 34.0);
                break;
            }
            let mut t = start + rng.range(1, 4) as u32 * e;
            let gestures = if player == 2 { 1 + rng.poisson(0.2 + 1.2 * d * it) } else { 1 + rng.poisson(0.4 + 2.6 * d * it) };
            for _ in 0..gestures {
                if t + beat >= end {
                    break;
                }
                let vel = 40.0 + 22.0 * it + rng.gauss() * 3.0;
                let deg = Self::snap_to(rng.range(6, 13), &SET);
                let weights = match player {
                    1 => [2.0, 1.5, 0.4 + v, 2.0],
                    2 => [5.0, 1.0, 0.3 + v * 0.5, 0.0],
                    _ => [3.0, 2.0, 0.6 + v, 0.0],
                };
                let pick = if player == 0 { rng.weighted(&weights[..3]) } else { rng.weighted(&weights) };
                match pick {
                    // a single pluck, often pulled into from the note above
                    0 => {
                        if rng.chance(0.45) {
                            self.note(koto, t, s16 / 2, self.scale.midi(Self::step_in(deg, &SET, 1)), vel - 10.0);
                            t += s16 / 2;
                        }
                        self.note(koto, t, 3 * beat, self.scale.midi(deg), vel);
                        t += rng.range(3, 6) as u32 * e;
                        if player == 2 {
                            t += rng.range(2, 4) as u32 * e;
                        }
                    }
                    // a falling run, quickening
                    1 => {
                        let mut dd = Self::step_in(deg, &SET, 1);
                        let n = rng.range(3, 5);
                        let mut gap = e;
                        for i in 0..n {
                            self.note(koto, t, 2 * beat, self.scale.midi(dd), vel - i as f64 * 3.0);
                            t += gap;
                            gap = (gap * 3 / 4).max(s16);
                            dd = Self::step_in(dd, &SET, -1);
                        }
                        t += rng.range(3, 5) as u32 * e;
                    }
                    // a rising sweep across the strings onto a long note
                    3 => {
                        let mut dd = Self::step_in(deg, &SET, -1);
                        for _ in 0..3 {
                            dd = Self::step_in(dd, &SET, -1);
                        }
                        for i in 0..4 {
                            self.note(koto, t + i * s16 / 2, s16, self.scale.midi(dd), vel - 12.0 + i as f64 * 2.0);
                            dd = Self::step_in(dd, &SET, 1);
                        }
                        t += 2 * s16;
                        self.note(koto, t, 3 * beat, self.scale.midi(dd), vel + 2.0);
                        t += rng.range(4, 7) as u32 * e;
                    }
                    // tremolo on one note, swelling and dying away
                    _ => {
                        let n = 10;
                        for i in 0..n {
                            let x = i as f64 / n as f64;
                            let sw = (x * std::f64::consts::PI).sin();
                            self.note(koto, t + i * s16 / 2, s16, self.scale.midi(deg), vel - 14.0 + 16.0 * sw);
                        }
                        t += 5 * beat / 2 + rng.range(2, 4) as u32 * e;
                    }
                }
            }
            b += len;
        }
        if !self.strings {
            return;
        }
        // The drone: an open fifth on each chord's root, swelling slowly.
        for b in (0..self.bars).step_by(4) {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let span = 4.min(self.bars - b) * self.bar;
            let r = self.bass_key(&chord, 50);
            self.note(Inst::StringPad, b * self.bar, span, r, 26.0 + 22.0 * it);
            self.note(Inst::StringPad, b * self.bar, span, r + 7, 22.0 + 20.0 * it);
            self.swell(Inst::StringPad, b * self.bar, span, 40.0, 70.0 + 30.0 * it, 45.0);
            if self.full(b) > 0.6 {
                self.note(Inst::Cello, b * self.bar, span, self.bass_key(&chord, 33), 24.0 + 16.0 * it);
            }
        }
        // The shakuhachi: a few long breathed notes, sliding up into some.
        let mut rng = self.rng("shakuhachi");
        let mut deg = 9;
        for b in (2..self.bars.saturating_sub(1)).step_by(4) {
            if self.full(b) < 0.35 || !rng.chance(0.4 + 0.5 * d) {
                continue;
            }
            let it = self.it(b);
            let mut t = b * self.bar + rng.range(0, 3) as u32 * beat;
            let end = (b + 4).min(self.bars - 1) * self.bar;
            for _ in 0..rng.range(1, 3) {
                let len = rng.range(3, 6) as u32 * beat;
                if t + len > end {
                    break;
                }
                deg = Self::snap_to((deg + rng.range(-2, 2)).clamp(5, 13), &SET);
                let key = self.scale.midi(deg);
                if rng.chance(0.4) {
                    self.note(Inst::Violin, t, e, self.scale.midi(Self::step_in(deg, &SET, -1)), 38.0 + 14.0 * it);
                    t += e;
                }
                self.note(Inst::Violin, t, len, key, 44.0 + 18.0 * it);
                self.swell(Inst::Violin, t, len, 40.0, 95.0 + 20.0 * it, 30.0);
                t += len + rng.range(1, 3) as u32 * beat;
            }
        }
    }

    fn gamelan(&mut self) {
        // No chords: everything turns around the tonic, the gong tone.
        self.harmony = vec![Chord::new(0, Ext::Sus2); self.bars as usize];
        self.section = (0..self.bars).map(|b| ((b / 16) % 3) as u8).collect();
        let set = self.p.mode.pentatonic();
        let beat = self.ppq;
        let s16 = beat / 4;
        let d = self.density();
        let v = self.p.variation as f64;
        let mut rng = self.rng("balungan");
        // from version 3 the gong may close eight-bar cycles instead of four
        let cycle_bars = if self.variant("cycle", 2) == 1 { 8u32 } else { 4u32 };
        let beats_per_cycle = cycle_bars * 4;
        // The core melody: one note a beat over a four-bar cycle, ending on the tonic.
        let new_cycle = |rng: &mut Rng| -> Vec<i32> {
            let mut deg = 0;
            let mut out = Vec::new();
            for i in 0..beats_per_cycle {
                if i + 1 == beats_per_cycle {
                    out.push(0);
                } else {
                    let step = *rng.pick(&[-2, -1, -1, 1, 1, 2, 0]);
                    deg = Self::step_in(deg, &set, if step == 0 { 1 } else { step });
                    if step.abs() == 2 {
                        deg = Self::step_in(deg, &set, step);
                    }
                    deg = deg.clamp(-3, 6);
                    deg = Self::snap_to(deg, &set);
                    out.push(deg);
                }
            }
            out
        };
        let mut cycle = new_cycle(&mut rng);
        // Elaborations leading into each core note, as steps in the scale from
        // it, for the three sixteenths before the beat.
        const FIGURES: [[i32; 3]; 5] = [[1, 0, 1], [-1, 0, -1], [0, 1, -1], [2, 1, -1], [1, 2, 1]];
        // from version 3, other elaborations, or two parts interlocking (imbal):
        // one on the beat's off-sixteenths above, the other between, below
        const MORE: [[i32; 3]; 5] = [[-1, 1, 0], [2, 0, 1], [-2, -1, 1], [1, -1, 1], [0, 2, 1]];
        let figure = self.variant("figure", 3);
        let figures = if figure == 1 { MORE } else { FIGURES };
        let sec_fig = [rng.range(0, 4) as usize, rng.range(0, 4) as usize, rng.range(0, 4) as usize];
        let core = if self.piano { Inst::Piano } else { Inst::Pizzicato };
        let total_beats = self.bars * 4;
        for i in 0..total_beats {
            let c = i % beats_per_cycle;
            if c == 0 && i > 0 && rng.chance(0.25 + 0.5 * v) {
                // the next cycle keeps its ending but changes along the way
                let fresh = new_cycle(&mut rng);
                for k in 0..beats_per_cycle as usize - 1 {
                    if rng.chance(0.5) {
                        cycle[k] = fresh[k];
                    }
                }
            }
            let b = i / 4;
            let it = self.it(b);
            let full = self.full(b);
            let t = i * beat;
            let target = cycle[c as usize];
            let last = b + 1 == self.bars;
            // sparse or quiet: the core every other beat
            if c % 2 == 1 || d >= 0.3 || full > 0.5 {
                let len = if last { 4 * beat } else { 2 * beat };
                self.note(core, t, len, self.scale.midi(target), 38.0 + 22.0 * it + if c % 4 == 3 { 6.0 } else { 0.0 });
            }
            if last {
                continue;
            }
            // interlocking figures, an octave up, once the music fills out
            if self.strings && full > 0.35 && figure == 2 {
                let next = cycle[((c + 1) % beats_per_cycle) as usize];
                let dense = d > 0.55 && full > 0.6;
                for k in 0..4u32 {
                    if !dense && k % 2 == 0 {
                        continue;
                    }
                    let mut deg = next + 5;
                    deg = Self::step_in(deg, &set, if k % 2 == 1 { 1 } else { -1 });
                    let vel = 30.0 + 22.0 * it + if k == 3 { 4.0 } else { 0.0 };
                    self.note(Inst::Pizzicato, t + k * s16, s16 * 2, self.scale.midi(deg), vel);
                }
            } else if self.strings && full > 0.35 {
                let fig = figures[sec_fig[self.section[b as usize] as usize % 3]];
                let next = cycle[((c + 1) % beats_per_cycle) as usize];
                let dense = d > 0.55 && full > 0.6;
                for (k, off) in fig.iter().enumerate() {
                    if !dense && k != 1 {
                        continue;
                    }
                    let mut deg = next + 5;
                    for _ in 0..off.abs() {
                        deg = Self::step_in(deg, &set, *off);
                    }
                    let vel = 30.0 + 22.0 * it + if k == 1 { 4.0 } else { 0.0 };
                    self.note(Inst::Pizzicato, t + (k as u32 + 1) * s16, s16 * 2, self.scale.midi(deg), vel);
                }
            }
        }
        if !self.strings {
            return;
        }
        // The gong closes each cycle; a smaller one marks its middle.
        let mut b = 0;
        while b < self.bars {
            let it = self.it(b);
            let span = cycle_bars.min(self.bars - b) * self.bar;
            let tonic = self.scale.midi(-14);
            self.note(Inst::Cello, b * self.bar, span, tonic, 46.0 + 20.0 * it);
            let mid = cycle_bars / 2;
            if b + mid < self.bars && self.full(b) > 0.4 {
                self.note(Inst::Cello, (b + mid) * self.bar, mid * self.bar, self.scale.midi(Self::snap_to(-10, &set)), 34.0 + 14.0 * it);
            }
            // a hazy drone under it
            if self.full(b) > 0.3 {
                self.note(Inst::StringPad, b * self.bar, span, self.scale.midi(-7), 24.0 + 20.0 * it);
                self.note(Inst::StringPad, b * self.bar, span, self.scale.midi(-3), 20.0 + 16.0 * it);
                self.swell(Inst::StringPad, b * self.bar, span, 35.0, 70.0 + 30.0 * it, 40.0);
            }
            b += cycle_bars;
        }
        // The suling: a few floating notes over the fuller cycles.
        let mut rng = self.rng("suling");
        let mut deg = 9;
        for b in (cycle_bars..self.bars.saturating_sub(1)).step_by(cycle_bars as usize) {
            if self.full(b) < 0.5 || !rng.chance(0.3 + 0.5 * d) {
                continue;
            }
            let mut t = b * self.bar + rng.range(1, 4) as u32 * beat;
            let end = (b + cycle_bars).min(self.bars - 1) * self.bar;
            for _ in 0..rng.range(2, 4) {
                let len = rng.range(2, 5) as u32 * beat;
                if t + len > end {
                    break;
                }
                deg = Self::snap_to((deg + rng.range(-2, 2)).clamp(7, 14), &set);
                self.note(Inst::Violin, t, len, self.scale.midi(deg), 40.0 + 16.0 * self.it(b));
                t += len + rng.range(0, 2) as u32 * beat;
            }
        }
    }

    fn fingerstyle(&mut self) {
        let hr = if self.density() < 0.4 { 2 } else { 1 };
        self.plan_harmony(hr, [0.4, 0.3, 0.15, 0.0, 0.15, 0.0], &[&[0, 3, 0, 4], &[0, 5, 3, 4]]);
        let e = self.ppq / 2;
        let d = self.density();
        let mut rng = self.rng("picking");
        // Treble notes over the alternating thumb: (quaver, voice), voices being
        // the third, fifth and octave an octave above the bass. Slot 0 pinches
        // with the thumb.
        const PICKS: [&[(u32, usize)]; 4] = [
            &[(0, 2), (3, 0), (5, 1), (7, 0)],
            &[(1, 0), (3, 1), (5, 0), (7, 2)],
            &[(0, 2), (2, 1), (3, 0), (6, 1)],
            &[(0, 1), (1, 2), (3, 0), (5, 2), (6, 1)],
        ];
        // from version 3, other picking patterns; and the thumb may keep to
        // the root on every beat, or rock between root and octave
        const MORE: [&[(u32, usize)]; 4] = [
            &[(0, 2), (2, 0), (3, 1), (5, 0), (6, 2)],
            &[(1, 1), (2, 2), (4, 0), (6, 1)],
            &[(0, 0), (1, 1), (2, 2), (4, 1), (5, 0), (7, 1)],
            &[(0, 2), (3, 1), (4, 2), (6, 0)],
        ];
        let sec_pick = [rng.range(0, 3) as usize, rng.range(0, 3) as usize, rng.range(0, 3) as usize];
        let picks_set = if self.variant("picks", 2) == 1 { MORE } else { PICKS };
        let thumb = self.variant("thumb", 3);
        let guitar = if self.piano { Inst::Piano } else { Inst::Pizzicato };
        let mut prev = Vec::new();
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let full = self.full(b);
            let start = b * self.bar;
            let low = self.scale.degree_near(self.bass_key(&chord, 40));
            let third = Self::third_of(&chord);
            let voices = [7 + third, 7 + 4, 14];
            let vel = 40.0 + 20.0 * it;
            if b + 1 == self.bars {
                // a slow strum to finish
                for (i, o) in [0, 4, 7, 7 + third, 11, 14].iter().enumerate() {
                    self.note(guitar, start + i as u32 * self.ppq / 12, 2 * self.bar, self.scale.midi(low + o), vel - i as f64 * 2.0);
                }
                continue;
            }
            // the thumb: root and fifth in turn (just the root when sparse)
            for k in 0..4u32 {
                if d < 0.3 && k % 2 == 1 {
                    continue;
                }
                let o = match thumb {
                    1 => 0,
                    2 if k % 2 == 1 => 7,
                    _ if k % 2 == 0 => 0,
                    _ => 4,
                };
                let o = if k == 2 && rng.chance(0.3) && thumb != 1 { -3 } else { o };
                self.note(guitar, start + k * 2 * e, 2 * e, self.scale.midi(low + o), vel + if k == 0 { 4.0 } else { 0.0 });
            }
            let picks = if d < 0.3 || full < 0.2 {
                &[(0, 2), (5, 0)][..]
            } else {
                picks_set[sec_pick[self.section[b as usize] as usize % 3]]
            };
            for &(slot, voice) in picks {
                if slot > 0 && rng.chance(0.15 * (1.0 - d)) {
                    continue;
                }
                let v = vel - 8.0 + if slot % 2 == 1 { 3.0 } else { 0.0 } + rng.gauss() * 2.0;
                self.note(guitar, start + slot * e, 3 * e, self.scale.midi(low + voices[voice]), v);
            }
            if self.strings && full > 0.45 && b % 2 == 0 {
                let span = 2.min(self.bars - b) * self.bar;
                let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 57, 76, false);
                prev = v.clone();
                self.pad(start, span, &self.thin(v), 20.0 + 28.0 * it);
            }
            if self.strings && full > 0.6 && d > 0.4 {
                self.note(Inst::Cello, start, self.bar, self.bass_key(&chord, 31), 36.0 + 18.0 * it);
            }
        }
        if self.strings {
            let insts: Vec<Option<Inst>> = (0..self.bars)
                .map(|b| (b >= 4 && b + 1 < self.bars && self.full(b) > 0.5).then_some(Inst::Violin))
                .collect();
            let mut rng = self.rng("melody");
            let mut cfg = MelodyCfg {
                lo: 9,
                hi: 16,
                patterns: PAT_SLOW_44,
                phrase_bars: 4,
                vel: 48.0,
                rest_bar_prob: 0.25,
                ornaments: false,
            };
            self.vary_melody(&mut cfg);
            let m = self.melody(&mut rng, &cfg, &insts);
            self.notes.extend(m);
        }
    }

    // ------------------------------------------------------------- arrangement

    /// From version 2: change the texture from one section to the next, so a
    /// piece develops. Somewhere past the middle comes a breakdown, where the
    /// bass, pulse and left hand drop away, often followed by the tune an
    /// octave higher; elsewhere the tune may rest or rise, the accompaniment
    /// thin out, or the pad drop out. The first and last sections stay as the
    /// style wrote them.
    /// In the spirit of Vangelis's Blade Runner: slow brass chords that swell
    /// and sag in pitch in the breath at the end of each phrase (written as
    /// expression; see `expression.rs`), a deep drone, a smoky lead, bells
    /// falling like rain and, in some pieces, a slow pulse.
    fn neon_noir(&mut self) {
        let hr = 2;
        self.plan_harmony(
            hr,
            [0.15, 0.3, 0.2, 0.1, 0.15, 0.1],
            &[&[0, 5, 0, 5], &[0, 5, 3, 6], &[0, 3, 5, 6], &[5, 6, 0, 0]],
        );
        let beat = self.ppq;
        let s16 = beat / 4;
        let d = self.density();
        let end = self.bars * self.bar;
        // the brass: held swells, a bloom from the bottom up, or a chord
        // struck afresh each bar, doubled an octave up at the peak
        let brass = self.variant("brass", 3);
        // the pulse: a heartbeat, a ticking ostinato, or none
        let pulse = self.variant("pulse", 3);
        // the lead: over the fuller stretches, answering in alternate
        // phrases, or a few long, lonely notes
        let lead = self.variant("lead", 3);
        // the rain: scattered bells, or little falling figures
        let rain = self.variant("rain", 2);
        // the drone: a pedal on the tonic, or the chords' roots
        let follow = self.variant("drone", 2) == 1;
        let mut rng = self.rng("noir");
        let mut prev = Vec::new();
        let bass_inst = if self.strings { Inst::Cello } else { Inst::Piano };
        for b in (0..self.bars).step_by(hr as usize) {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let start = b * self.bar;
            let span = hr.min(self.bars - b) * self.bar;
            let last = b + hr >= self.bars;
            // every fourth chord is cut short, leaving a breath for the sag
            let breath = (b / hr) % 4 == 3 && !last;

            let voicing = voice_lead(&chord.pitch_classes(&self.scale), &prev, 50, 74, false);
            prev = voicing.clone();
            let voicing = self.thin(voicing);
            let len = if last {
                span + self.bar
            } else if breath {
                span - beat - beat / 2
            } else {
                span + s16
            };
            let vel = 36.0 + 40.0 * it;
            if self.strings {
                let n = voicing.len() as u32;
                for (i, k) in voicing.iter().enumerate() {
                    match brass {
                        1 => {
                            // each note enters a little after the one below
                            let delay = (i as u32 * beat / 2).min(len / 2);
                            self.note(Inst::StringPad, start + delay, len - delay, *k, vel - 3.0 * i as f64);
                        }
                        2 if !last => {
                            // struck again each bar, softer the second time
                            let bars = span / self.bar;
                            for j in 0..bars {
                                let on = start + j * self.bar;
                                let l = if j + 1 == bars { len - j * self.bar } else { self.bar - s16 };
                                self.note(Inst::StringPad, on, l, *k, vel - 8.0 * j as f64);
                            }
                            if i as u32 + 1 == n && it > 0.6 {
                                self.note(Inst::StringPad, start, len, k + 12, vel - 10.0);
                            }
                        }
                        _ => self.note(Inst::StringPad, start, len, *k, vel),
                    }
                }
                let to = if breath || last { 35.0 } else { 60.0 + 20.0 * it };
                self.swell(Inst::StringPad, start, span, 35.0 + 15.0 * it, 85.0 + 40.0 * it, to);
            } else {
                self.pad(start, len, &voicing, vel * 0.8);
            }

            // The drone, low and long: one note for each phrase on the tonic,
            // or the chord's root.
            if follow {
                let k = self.bass_key(&chord, 31);
                self.note(bass_inst, start, len.max(span), k, 40.0 + 24.0 * it);
            } else if (b / hr) % 4 == 0 {
                let k = self.bass_key(&Chord::new(0, Ext::Triad), 31);
                let phrase = (4 * hr * self.bar).min(end - start);
                self.note(bass_inst, start, phrase + beat, k, 40.0 + 24.0 * it);
            }

            // The pulse, plucked low, once the piece has opened up.
            for bar in b..(b + hr).min(self.bars) {
                let bs = bar * self.bar;
                let it = self.it(bar);
                if self.full(bar) < 0.45 || bar + 1 == self.bars {
                    continue;
                }
                let root = self.bass_key(&chord, 43);
                match pulse {
                    0 => {
                        // a slow heartbeat on beats one and three
                        for at in [0, 2 * beat] {
                            self.note(Inst::Pizzicato, bs + at, beat / 2, root, 34.0 + 22.0 * it);
                            self.note(Inst::Pizzicato, bs + at + beat / 3, beat / 2, root, 24.0 + 18.0 * it);
                        }
                    }
                    1 => {
                        // a ticking figure in quavers: root, octave, fifth, octave
                        let fifth = self.scale.midi(self.scale.degree_near(root) + 4);
                        for (i, k) in [root, root + 12, fifth, root + 12].iter().cycle().take(8).enumerate() {
                            let accent = if i % 4 == 0 { 8.0 } else { 0.0 };
                            self.note(Inst::Pizzicato, bs + i as u32 * beat / 2, beat / 2, *k, 26.0 + 20.0 * it + accent);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Rain: bells high above, a drop at a time, or in little falling runs.
        let bell = if self.piano { Inst::Piano } else { Inst::Pizzicato };
        for b in 0..self.bars {
            let it = self.it(b);
            let chord = self.harmony[b as usize].clone();
            let start = b * self.bar;
            let scale = self.scale;
            let tone = |deg: i32| {
                let mut deg = deg;
                while !chord.contains(deg) && !scale.is_pentatonic(deg) {
                    deg += 1;
                }
                deg
            };
            if rain == 1 {
                if b % 2 == 0 && rng.chance(0.25 + 0.6 * d * it) {
                    let at = start + rng.range(0, 5) as u32 * beat / 2;
                    let mut deg = tone(rng.range(16, 20));
                    let n = rng.range(3, 5);
                    for i in 0..n {
                        let t = at + i as u32 * beat / 3;
                        self.note(bell, t, 3 * beat, self.scale.midi(deg), 30.0 + 22.0 * it - 3.0 * i as f64);
                        deg = tone(deg - 2);
                    }
                }
            } else {
                for _ in 0..rng.poisson(0.2 + 1.0 * d * it) {
                    let at = start + rng.range(0, 7) as u32 * beat / 2;
                    let deg = tone(rng.range(14, 20));
                    let dur = rng.range(2, 5) as u32 * beat;
                    self.note(bell, at, dur, self.scale.midi(deg), 24.0 + 24.0 * it + rng.gauss() * 3.0);
                }
            }
        }

        // The smoky lead.
        if self.strings {
            let phrase = 4 * hr;
            let insts: Vec<Option<Inst>> = (0..self.bars)
                .map(|b| {
                    let on = match lead {
                        1 => (b / phrase) % 2 == 1 && self.full(b) > 0.3,
                        2 => self.full(b) > 0.35,
                        _ => self.full(b) > 0.5,
                    };
                    on.then_some(Inst::Violin)
                })
                .collect();
            const PAT_LONG: &[Pattern] = &[&[(0, 8)], &[(2, 6)], &[(0, 6)], &[(4, 4)]];
            let mut cfg = MelodyCfg {
                lo: 4,
                hi: 12,
                patterns: if lead == 2 { PAT_LONG } else { PAT_SLOW_44 },
                phrase_bars: 4,
                vel: 52.0,
                rest_bar_prob: if lead == 2 { 0.4 } else { 0.25 },
                ornaments: false,
            };
            self.vary_melody(&mut cfg);
            let mut rng = self.rng("melody");
            let m = self.melody(&mut rng, &cfg, &insts);
            self.notes.extend(m);
        }
    }

    // ------------------------------------------------------------------ scores

    /// Tech Trailer: a hybrid trailer score in one arc. A sparse, glassy intro
    /// (a felt piano motif over a sub drone and grains); a build that layers a
    /// synth arpeggio, a heartbeat kick, ticking percussion, a low string
    /// ostinato, rising strings, brass swells, risers and braams; a peak of big
    /// drums, sub hits, braams, brass and choir, doubling up in the final
    /// third; and an outro back to the piano, ending on a soft hit.
    fn tech_trailer(&mut self) {
        let beat = self.ppq;
        let bar = self.bar;
        let s16 = beat / 4;
        let n = self.bars;
        let ticks = |secs: f64| (secs * self.p.tempo.clamp(30.0, 200.0) as f64 / 60.0 * beat as f64) as u32;

        // The arc: intro, build (whose second half adds the heavier layers),
        // peak and outro.
        let bounds = self.bounds();
        let (intro, peak, outro) = (bounds[0], bounds[1], bounds[2]);
        let mid = intro + (peak - intro) / 4 * 2;
        // when the layers of the build come in, with the density
        let late = self.late();
        let layer = |x: f64| (intro + ((peak - intro) as f64 * (x * late).min(1.0)).round() as u32).min(peak);
        let d = self.density();
        // the last bar of the build holds its breath before the peak
        let drop = if peak - intro >= 4 { peak - 1 } else { peak };
        // the drums can double up in the final third
        let double = (self.variant("double", 4) != 3).then(|| (n * 2 / 3).max(peak + (outro - peak) / 2));

        self.intensity = (0..n)
            .map(|b| {
                let x = |a: u32, z: u32| (b - a) as f64 / (z - a).max(1) as f64;
                if b < intro {
                    0.15 + 0.1 * x(0, intro)
                } else if b < peak {
                    0.3 + 0.55 * x(intro, peak)
                } else if b < outro {
                    0.9 + 0.1 * x(peak, outro)
                } else {
                    0.3 - 0.25 * x(outro, n)
                }
            })
            .collect();
        // how far into the build, 0..1
        let rise = |b: u32| ((b.max(intro) - intro) as f64 / (peak - intro).max(1) as f64).min(1.0);

        // Harmony: simple, open chords, two bars each. The intro and build
        // turn one progression over; the peak lifts to another; the outro
        // rests on the tonic.
        let mut rng = self.rng("score-harmony");
        let (verse, lift): (&[&[i32]], &[&[i32]]) = if self.p.mode.is_minor() {
            (&[&[0, 5, 2, 6], &[0, 5, 3, 6], &[0, 2, 5, 6], &[0, 6, 5, 6]], &[&[5, 6, 0, 0], &[5, 6, 2, 2], &[3, 5, 2, 6], &[5, 2, 6, 6]])
        } else {
            (&[&[5, 3, 0, 4], &[0, 4, 5, 3], &[0, 5, 3, 4], &[5, 4, 3, 4]], &[&[3, 4, 0, 0], &[3, 4, 5, 5], &[5, 3, 0, 4], &[3, 0, 4, 4]])
        };
        let exts = [Ext::Triad, Ext::Add9, Ext::Sus2, Ext::Sus4];
        let prog = |rng: &mut Rng, pool: &[&[i32]]| -> Vec<Chord> {
            let degs = *rng.pick(pool);
            degs.iter().map(|d| Chord::new(self.fix_dim(*d), exts[rng.weighted(&[0.35, 0.3, 0.2, 0.15])])).collect()
        };
        let verse = prog(&mut rng, verse);
        let lift = prog(&mut rng, lift);
        let home = Chord::new(0, *rng.pick(&[Ext::Add9, Ext::Sus2, Ext::Triad]));
        self.harmony = (0..n)
            .map(|b| {
                if b >= outro {
                    home.clone()
                } else if b >= peak {
                    lift[((b - peak) / 2 % 4) as usize].clone()
                } else {
                    verse[(b / 2 % 4) as usize].clone()
                }
            })
            .collect();
        self.section = (0..n).map(|b| [intro, peak, outro].iter().filter(|s| b >= **s).count() as u8).collect();
        self.vary_harmony(&[outro..n]);

        let motif_kind = self.variant("motif", 3);
        let arp_kind = self.variant("arp", 3);
        let ostinato = self.variant("ostinato", 3);
        let taiko = self.variant("drums", 2) == 0;
        let horns = self.variant("brass", 2) == 0;

        // ---- the piano motif: a few chord tones in a fixed order, the same
        // figure over every chord
        let mut mrng = self.rng("motif");
        let len = if motif_kind == 2 { 3 } else { 4 };
        let mut cell: Vec<usize> = (0..len).map(|_| mrng.range(0, 4) as usize).collect();
        // with the variation, the figure evolves a note at a time as it repeats
        let mut erng = self.rng("motif-evolves");
        let v = self.p.variation as f64;
        let onsets: &[u32] = match motif_kind {
            0 => &[0, 2, 4, 6, 8, 10, 12, 14],
            1 => &[0, 3, 6, 8, 11, 14],
            _ => &[0, 3, 8, 11],
        };
        let e8 = beat / 2;
        for b in (0..n.saturating_sub(2)).step_by(2) {
            let in_outro = b >= outro;
            if (b >= drop && b < peak) || (in_outro && b >= outro + 2) {
                continue;
            }
            if b >= 8 && erng.chance(0.35 * v) {
                let i = erng.range(1, len as i32 - 1) as usize;
                cell[i] = erng.range(0, 4) as usize;
            }
            let chord = self.harmony[b as usize].clone();
            let root = if chord.root > 3 { chord.root - 7 } else { chord.root };
            let third = if chord.ext == Ext::Sus2 { 1 } else if chord.ext == Ext::Sus4 { 3 } else { 2 };
            let tones = [root, root + third, root + 4, root + 7, root + 8];
            let it = self.it(b);
            for (i, on) in onsets.iter().enumerate() {
                let start = b * bar + on * e8;
                if b < peak && start >= drop * bar {
                    break;
                }
                // the intro starts with half the figure; the outro thins it out
                if (b == 0 && i >= onsets.len() / 2 && motif_kind != 2) || (in_outro && mrng.chance(0.4)) {
                    continue;
                }
                let deg = tones[cell[i % len]];
                let key = self.scale.midi(deg);
                let next = onsets.get(i + 1).map_or(16, |o| *o);
                let dur = (next - on) * e8 + beat;
                let vel = if b < peak { 40.0 + 16.0 * rise(b) } else if in_outro { 38.0 } else { 52.0 } + if i == 0 { 6.0 } else { 0.0 };
                self.note(Inst::Piano, start, dur.max(2 * beat), key, vel + mrng.gauss() * 2.0);
                // doubled at the peak, and through the build's second half when busy
                if (peak..outro).contains(&b) || (d > 0.7 && b >= mid && b < peak) {
                    self.note(Inst::Piano, start, dur.max(2 * beat), key + 12, vel - 10.0 + it);
                }
            }
        }
        // The close: the figure's first note alone on each bar, then one low
        // piano note left ringing, and a soft final hit.
        let last = n - 1;
        let tonic = self.scale.midi(0);
        let lone = self.scale.midi([0, 2, 4, 7, 8][cell[0]]);
        for b in (outro + 2)..last.saturating_sub(1) {
            let fade = (b - outro) as f64 / (n - outro) as f64;
            self.note(Inst::Piano, b * bar, 2 * bar, lone, 40.0 - 10.0 * fade);
        }
        self.note(Inst::Piano, last.saturating_sub(1) * bar, 3 * bar, tonic - 12, 40.0);
        self.note(Inst::Piano, last.saturating_sub(1) * bar, 3 * bar, tonic, 34.0);
        let pc = |key: i32| key.rem_euclid(12);
        let tonic_pc = pc(tonic);
        self.note(Inst::Fx, last * bar, ticks(6.0), (fx::IMPACT) as i32, 52.0);
        self.note(Inst::Fx, last * bar, ticks(3.5), fx::BOOM as i32 + tonic_pc, 56.0);

        // ---- the sub drone and glassy grains
        let drone_key = fx::DRONE as i32 + tonic_pc;
        self.note(Inst::Fx, 0, drop * bar, drone_key, 62.0);
        self.note(Inst::Fx, outro * bar, (n - outro) * bar + bar, drone_key, 52.0);
        for b in (0..n).step_by(4) {
            if b < mid || b == outro {
                let root = pc(self.scale.midi(self.harmony[b as usize].root));
                self.note(Inst::Fx, b * bar, 4 * bar, fx::GRAINS as i32 + root, 56.0 + 10.0 * self.it(b));
            }
        }

        // ---- strings: the pad rises through the build, soars at the peak and
        // fades away in the outro
        let pad_from = layer(0.25);
        let mut prev = Vec::new();
        for b in (pad_from..outro).step_by(2) {
            let chord = self.harmony[b as usize].clone();
            let start = b * bar;
            let span = 2.min(outro - b) * bar;
            let (lo, hi, spread) = if b < peak {
                let lo = 48 + (10.0 * rise(b)).round() as i32;
                (lo, lo + 19, false)
            } else {
                (52, 86, true)
            };
            let voicing = voice_lead(&chord.pitch_classes(&self.scale), &prev, lo, hi, spread);
            prev = voicing.clone();
            let t = rise(b);
            let vel = if b < peak { 40.0 + 20.0 * t } else { 56.0 };
            for k in &voicing {
                self.note(Inst::StringPad, start, span + s16, *k, vel);
            }
            if b >= peak && voicing.last().is_some_and(|k| k + 12 <= 96) {
                self.note(Inst::StringPad, start, span + s16, voicing.last().unwrap() + 12, vel - 12.0);
            }
            if b + 2 >= peak && b < peak {
                // the last chord of the build swells into the hit
                self.swell(Inst::StringPad, start, span, 50.0, 90.0, 115.0);
            } else if b < peak {
                self.swell(Inst::StringPad, start, span, 30.0 + 35.0 * t, 70.0 + 35.0 * t, 50.0 + 35.0 * t);
            } else {
                self.swell(Inst::StringPad, start, span, 80.0, 105.0, 90.0);
            }
        }
        {
            let voicing = voice_lead(&home.pitch_classes(&self.scale), &prev, 48, 72, false);
            let start = outro * bar;
            let span = (n - outro) * bar;
            for k in &voicing {
                self.note(Inst::StringPad, start, span, *k, 56.0);
            }
            self.swell(Inst::StringPad, start, span, 80.0, 70.0, 0.0);
        }

        // ---- the synth arpeggio, pulsing in sixteenths through the build and peak
        for b in intro..outro {
            if b >= drop && b < peak {
                continue;
            }
            let chord = self.harmony[b as usize].clone();
            let base = self.bass_key(&chord, 45);
            let r = self.scale.degree_near(base);
            let third = if chord.ext == Ext::Sus2 { 1 } else if chord.ext == Ext::Sus4 { 3 } else { 2 };
            let tones = [base, self.scale.midi(r + third), self.scale.midi(r + 4), base + 12];
            let order: &[usize] = match arp_kind {
                0 => &[0, 1, 2, 3],
                1 => &[0, 1, 2, 3, 2, 1],
                _ => &[0, 0, 3, 0, 2, 0, 3, 0],
            };
            let vel = if b < peak { 36.0 + 30.0 * rise(b) } else { 64.0 };
            // sparse: in eighths
            let step = if d < 0.3 { 2 } else { 1 };
            for i in (0..16u32).step_by(step) {
                let key = tones[order[i as usize % order.len()]];
                let accent = if i % 4 == 0 { 8.0 } else { 0.0 };
                self.note(Inst::Pizzicato, b * bar + i * s16, step as u32 * s16 * 9 / 10, key, vel + accent);
            }
        }

        // ---- the low string ostinato, from the middle of the build
        for b in layer(0.5)..outro {
            if b >= drop && b < peak {
                continue;
            }
            let chord = self.harmony[b as usize].clone();
            let root = self.bass_key(&chord, 36);
            let (steps, octave_at): (&[u32], &[u32]) = match ostinato {
                0 => (&[0, 2, 4, 6, 8, 10, 12, 14], &[4, 12]),
                1 => (&[0, 2, 3, 4, 6, 7, 8, 10, 11, 12, 14, 15], &[8]),
                _ => (&[0, 3, 6, 8, 11, 14], &[6, 14]),
            };
            let vel = if b < peak { 48.0 + 24.0 * rise(b) } else { 70.0 };
            for s in steps {
                if d < 0.3 && s % 4 != 0 {
                    continue;
                }
                let key = if octave_at.contains(s) { root + 12 } else { root };
                let accent = if s % 4 == 0 { 10.0 } else { 0.0 };
                let start = b * bar + s * s16;
                self.note(Inst::Cello, start, s16 * 8 / 5, key, vel + accent);
                if b >= peak && key - 12 >= 28 {
                    self.note(Inst::Cello, start, s16 * 8 / 5, key - 12, vel + accent - 8.0);
                }
            }
        }

        // ---- brass: swells through the second half of the build, then full
        // at the peak
        // on the chords' two-bar boundaries
        let brass_from = layer(0.5) + (layer(0.5) - intro) % 2;
        for b in (brass_from..outro).step_by(2) {
            let chord = self.harmony[b as usize].clone();
            let start = b * bar;
            let span = 2.min(outro - b) * bar;
            let root = self.bass_key(&chord, 43);
            let fifth = self.scale.midi(self.scale.degree_near(root) + 4);
            let keys: Vec<i32> = if b >= peak && !horns {
                voice_lead(&chord.pitch_classes(&self.scale), &[], 48, 67, false)
            } else if b >= peak {
                vec![root, fifth, root + 12]
            } else {
                vec![root, fifth]
            };
            let vel = if b < peak { 60.0 + 20.0 * rise(b) } else { 80.0 };
            for k in &keys {
                self.note(Inst::Brass, start, span + s16, *k, vel);
            }
            if b + 2 >= peak && b < peak {
                self.swell(Inst::Brass, start, span, 20.0, 80.0, 127.0);
            } else if b < peak {
                self.swell(Inst::Brass, start, span, 15.0, 55.0 + 50.0 * rise(b), 25.0);
            } else {
                self.swell(Inst::Brass, start, span, 70.0, 120.0, 95.0);
            }
        }

        // ---- the choir, high above the peak (entering softly just before it)
        let mut prev = Vec::new();
        for b in (peak.saturating_sub(if d > 0.7 { 8 } else if d < 0.3 { 0 } else { 4 }).max(mid)..outro).step_by(2) {
            if b >= drop && b < peak {
                continue;
            }
            let chord = self.harmony[b as usize].clone();
            let voicing = voice_lead(&chord.pitch_classes(&self.scale), &prev, 67, 86, false);
            prev = voicing.clone();
            let start = b * bar;
            let span = if b < peak { (drop - b).min(2) } else { 2.min(outro - b) } * bar;
            for k in &voicing {
                self.note(Inst::Violin, start, span + s16, *k, if b < peak { 50.0 } else { 66.0 });
            }
            if b < peak {
                self.swell(Inst::Violin, start, span, 15.0, 55.0, 60.0);
            } else {
                self.swell(Inst::Violin, start, span, 70.0, 115.0, 90.0);
            }
        }

        // ---- drums
        let mut drng = self.rng("score-drums");
        let tick_from = layer(0.25);
        for b in intro..outro {
            let bs = b * bar;
            let t = rise(b);
            if b >= drop && b < peak {
                continue;
            }
            if b < peak {
                // the heartbeat, joined by beat three halfway through
                let beats: &[u32] = if b >= mid { &[0, 2] } else { &[0] };
                for at in beats {
                    let v = 66.0 + 30.0 * t;
                    self.note(Inst::Drums, bs + at * beat, beat / 2, drum::HEARTBEAT as i32, v);
                    self.note(Inst::Drums, bs + at * beat + beat * 3 / 8, beat / 2, drum::HEARTBEAT as i32, v * 0.7);
                }
                if b >= tick_from {
                    for i in (0..16u32).step_by(if d < 0.3 { 2 } else { 1 }) {
                        let v = [1.0, 0.45, 0.7, 0.45][i as usize % 4] * (30.0 + 30.0 * t);
                        self.note(Inst::Drums, bs + i * s16, s16, drum::HAT as i32, v + drng.gauss() * 2.0);
                    }
                    for at in [6, 14] {
                        self.note(Inst::Drums, bs + at * s16, s16, drum::TICK as i32, 40.0 + 25.0 * t);
                    }
                }
                // the toms gather in the two bars before the breath
                if drop >= 2 && b + 2 >= drop && b >= mid {
                    let roll = b + 1 == drop;
                    let step = if roll { s16 } else { beat / 2 };
                    let count = bar / step;
                    for i in 0..count {
                        let x = (i as f64 + if roll { count as f64 } else { 0.0 }) / (2 * count) as f64;
                        let k = if i % 2 == 0 { drum::TOM_LOW } else { drum::TOM_MID };
                        self.note(Inst::Drums, bs + i * step, step, k as i32, 55.0 + 60.0 * x);
                        if roll {
                            self.note(Inst::Drums, bs + i * step, step, drum::SNARE as i32, 30.0 + 70.0 * x);
                        }
                    }
                }
                continue;
            }
            // the peak: half time, then doubling up
            let doubled = double.is_some_and(|d| b >= d);
            let fill = b + 1 == outro;
            let crash_every = if doubled { 2 } else { 4 };
            if (b - peak) % crash_every == 0 {
                self.note(Inst::Drums, bs, 2 * bar, drum::CRASH as i32, 84.0);
            }
            if fill {
                for i in 0..16u32 {
                    let k = [drum::TOM_HIGH, drum::TOM_MID, drum::TOM_LOW, drum::TOM_LOW][(i / 4) as usize];
                    self.note(Inst::Drums, bs + i * s16, s16, k as i32, 68.0 + 2.0 * i as f64);
                    if i % 2 == 0 {
                        self.note(Inst::Drums, bs + i * s16, s16, drum::SNARE as i32, 50.0 + 2.5 * i as f64);
                    }
                }
                continue;
            }
            let e = beat / 2;
            if doubled {
                for at in [0, 3, 4] {
                    self.note(Inst::Drums, bs + at * e, e, drum::KICK as i32, if at == 3 { 74.0 } else { 94.0 });
                }
                for at in [2, 6] {
                    self.note(Inst::Drums, bs + at * e, e, drum::SNARE as i32, 90.0);
                    self.note(Inst::Drums, bs + at * e, e, drum::CLAP as i32, 76.0);
                }
                for i in 0..8u32 {
                    let k = if i % 2 == 0 { drum::TOM_LOW } else { drum::TOM_MID };
                    let accent = if i % 2 == 0 { 15.0 } else { 0.0 };
                    self.note(Inst::Drums, bs + i * e, e, k as i32, 66.0 + accent);
                }
                for i in 0..16u32 {
                    self.note(Inst::Drums, bs + i * s16, s16, drum::HAT as i32, if i % 2 == 0 { 50.0 } else { 32.0 });
                }
            } else {
                self.note(Inst::Drums, bs, e, drum::KICK as i32, 96.0);
                if (b - peak) % 2 == 1 {
                    self.note(Inst::Drums, bs + 5 * e, e, drum::KICK as i32, 72.0);
                }
                self.note(Inst::Drums, bs + 4 * e, e, drum::SNARE as i32, 92.0);
                self.note(Inst::Drums, bs + 4 * e, e, drum::CLAP as i32, 78.0);
                if taiko {
                    for (at, k, v) in [(0, drum::TOM_LOW, 88.0), (3, drum::TOM_MID, 74.0), (6, drum::TOM_LOW, 80.0), (7, drum::TOM_HIGH, 70.0)] {
                        self.note(Inst::Drums, bs + at * e, e, k as i32, v);
                    }
                } else if (b - peak) % 4 == 3 {
                    for i in 8..16u32 {
                        let k = if i < 12 { drum::TOM_MID } else { drum::TOM_LOW };
                        self.note(Inst::Drums, bs + i * s16, s16, k as i32, 60.0 + 3.0 * (i - 8) as f64);
                    }
                }
                for i in 0..8u32 {
                    self.note(Inst::Drums, bs + i * e, e, drum::HAT as i32, if i % 2 == 0 { 44.0 } else { 30.0 });
                }
            }
        }

        // ---- trailer effects
        let fx_at = |s: &mut Self, end_bar: u32, key: u8, secs: f64, vel: f64| {
            let end = end_bar * bar;
            let len = ticks(secs);
            s.note(Inst::Fx, end.saturating_sub(len), len, key as i32, vel);
        };
        // into the build, its second half, and the peak
        fx_at(self, intro, fx::REVERSE_LONG, kit::REVERSE_LONG_SECS, 70.0);
        if mid > intro {
            fx_at(self, mid, fx::RISER, kit::RISER_SECS, 72.0);
            fx_at(self, mid, fx::REVERSE, kit::REVERSE_SECS, 76.0);
        }
        fx_at(self, peak, fx::RISER_LONG, kit::RISER_LONG_SECS, 96.0);
        fx_at(self, peak, fx::REVERSE_LONG, kit::REVERSE_LONG_SECS, 92.0);
        // a whoosh as the pads come in, and across the peak's eight-bar lines
        let whoosh = ticks(kit::WHOOSH_PEAK_SECS);
        for b in std::iter::once(pad_from).chain((peak + 8..outro).step_by(8)) {
            if b > 0 {
                self.note(Inst::Fx, (b * bar).saturating_sub(whoosh), ticks(2.2), fx::WHOOSH as i32, 70.0);
            }
        }
        // braams and sub booms on the chord's root: now and then in the
        // build, then hammering the peak
        let root_pc = |s: &Self, b: u32| pc(s.scale.midi(s.harmony[b as usize].root));
        for b in mid..outro {
            if b >= drop && b < peak {
                continue;
            }
            let r = root_pc(self, b);
            let bs = b * bar;
            let braam = if b < peak { (b - mid) % 8 == 0 } else { (b - peak) % 4 == 0 };
            if braam {
                let v = if b == peak { 104.0 } else if b < peak { 76.0 } else { 88.0 };
                self.note(Inst::Fx, bs, ticks(5.0), fx::BRAAM as i32 + r, v);
            }
            let doubled = double.is_some_and(|d| b >= d);
            let boom = if b < peak { braam } else { doubled || (b - peak) % 2 == 0 };
            if boom {
                self.note(Inst::Fx, bs, ticks(3.0), fx::BOOM as i32 + r, if b < peak { 72.0 } else { 90.0 });
            }
            if b >= peak && b > peak && (b - peak) % 4 == 0 {
                fx_at(self, b, fx::REVERSE, kit::REVERSE_SECS, 70.0);
            }
        }
        self.note(Inst::Fx, peak * bar, ticks(6.0), fx::IMPACT as i32, 100.0);
        if outro < n {
            self.note(Inst::Fx, outro * bar, ticks(3.0), fx::DOWNLIFTER as i32, 82.0);
            self.note(Inst::Fx, outro * bar, ticks(6.0), fx::IMPACT as i32, 80.0);
        }
    }

    /// Megacity Rain: a dystopian neo-noir synth score. Rain, distant thunder,
    /// machinery and an evolving analogue drone; a lone, mournful lead; then
    /// CS-80 brass rolling in slow waves, sub-bass pulses, dark stabs, soft
    /// distant drums and a smoky sax-like line answering the lead, building
    /// hypnotically, never to a climax, and sinking back into the rain.
    fn megacity_rain(&mut self) {
        let beat = self.ppq;
        let bar = self.bar;
        let n = self.bars;
        let ticks = |secs: f64| (secs * self.p.tempo.clamp(30.0, 200.0) as f64 / 60.0 * beat as f64) as u32;
        let bounds = self.bounds();
        let (lead_in, body, recede) = (bounds[0], bounds[1], bounds[2]);
        // the drums and sax come in partway through the body: earlier when
        // busy, later when sparse
        let late = self.late();
        let into_body = |x: f64| (body + ((recede - body) as f64 * (x * late).min(1.0)).round() as u32).min(recede);
        let drums_from = into_body(0.27);
        let sax_from = into_body(0.38);
        let d = self.density();

        self.intensity = (0..n)
            .map(|b| {
                let x = |a: u32, z: u32| (b.max(a) - a) as f64 / (z - a).max(1) as f64;
                if b < body {
                    0.15 + 0.1 * x(0, body)
                } else if b < recede {
                    0.35 + 0.5 * x(body, recede)
                } else {
                    0.4 - 0.25 * x(recede, n)
                }
            })
            .collect();

        // Minor and modal harmony, suspended and unresolved, two bars a chord:
        // one progression for the opening and close, another for the body.
        let mut rng = self.rng("rain-harmony");
        let pool: &[&[i32]] = if self.p.mode.is_minor() {
            &[&[0, 5, 0, 5], &[0, 6, 5, 6], &[0, 3, 0, 6], &[0, 5, 3, 6], &[0, 2, 5, 6]]
        } else {
            &[&[5, 3, 5, 3], &[0, 3, 0, 4], &[5, 4, 3, 4], &[0, 5, 3, 5]]
        };
        let exts = [Ext::Triad, Ext::Add9, Ext::Sus2, Ext::Sus4];
        let prog = |rng: &mut Rng| -> Vec<Chord> {
            let degs = *rng.pick(pool);
            degs.iter().map(|d| Chord::new(self.fix_dim(*d), exts[rng.weighted(&[0.15, 0.25, 0.3, 0.3])])).collect()
        };
        let outer = prog(&mut rng);
        let inner = prog(&mut rng);
        self.harmony = (0..n)
            .map(|b| {
                if b + 2 >= n {
                    // it ends hanging, on a suspended tonic
                    Chord::new(0, Ext::Sus2)
                } else if (body..recede).contains(&b) {
                    inner[((b - body) / 2 % 4) as usize].clone()
                } else {
                    outer[(b / 2 % 4) as usize].clone()
                }
            })
            .collect();
        self.section = (0..n).map(|b| [body, recede].iter().filter(|s| b >= **s).count() as u8).collect();
        self.vary_harmony(&[]);

        let pulse = self.variant("pulse", 2);
        let stabs = self.variant("stabs", 2);
        let kick_kind = self.variant("kick", 2);
        let pc = |key: i32| key.rem_euclid(12);
        let tonic_pc = pc(self.scale.midi(0));
        let end = n * bar + bar;

        // ---- the city: rain throughout, machinery far off, an evolving
        // analogue drone, a sub drone, thunder, distant metal and neon
        self.note(Inst::Fx, 0, end, fx::RAIN as i32, 74.0);
        self.note(Inst::Fx, 0, recede * bar, fx::HUM as i32, 58.0);
        self.note(Inst::Fx, 0, body * bar + beat, fx::DRONE as i32 + tonic_pc, 58.0);
        self.note(Inst::Fx, recede * bar, end - recede * bar, fx::DRONE as i32 + tonic_pc, 54.0);
        for b in (0..n).step_by(3) {
            self.note(Inst::Fx, b * bar, ticks(16.0), fx::ANALOG_DRONE as i32 + tonic_pc, 62.0 + 22.0 * self.it(b));
        }
        let mut frng = self.rng("rain-city");
        let mut b = 1;
        while b < n {
            self.note(Inst::Fx, b * bar + frng.range(0, 3) as u32 * beat, ticks(9.0), fx::THUNDER as i32, frng.range_f(55.0, 85.0));
            b += frng.range(10, 18) as u32;
        }
        let mut b = frng.range(2, 5) as u32;
        while b < recede {
            self.note(Inst::Fx, b * bar + frng.range(0, 3) as u32 * beat, ticks(8.0), fx::METAL as i32, frng.range_f(42.0, 60.0));
            b += frng.range(5, 9) as u32;
        }
        for b in (body..recede).step_by(12) {
            self.note(Inst::Fx, b * bar, 8.min(recede - b) * bar, fx::NEON as i32, 52.0);
        }

        // ---- CS-80 brass rolling in slow waves through the body; every
        // fourth chord breathes, and sags as it dies away
        let mut prev = Vec::new();
        for b in (body..recede).step_by(2) {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let start = b * bar;
            let span = 2.min(recede - b) * bar;
            let breath = (b - body) / 2 % 4 == 3;
            let len = if breath { span - beat - beat / 2 } else { span + beat / 4 };
            let voicing = self.thin(voice_lead(&chord.pitch_classes(&self.scale), &prev, 48, 72, false));
            prev = voicing.clone();
            for k in &voicing {
                self.note(Inst::StringPad, start, len, *k, 44.0 + 30.0 * it);
            }
            if it > 0.65 - 0.4 * (d - 0.5).max(0.0) {
                if let Some(top) = voicing.last() {
                    self.note(Inst::StringPad, start, len, top + 12, 36.0 + 20.0 * it);
                }
            }
            let to = if breath { 30.0 } else { 40.0 + 25.0 * it };
            self.swell(Inst::StringPad, start, span, 25.0 + 30.0 * it, 80.0 + 40.0 * it, to);
        }
        if recede < n {
            // the last wave, dying away into the rain
            let chord = self.harmony[recede as usize].clone();
            let voicing = voice_lead(&chord.pitch_classes(&self.scale), &prev, 48, 72, false);
            let span = (n - recede).min(8) * bar;
            for k in &voicing {
                self.note(Inst::StringPad, recede * bar, span, *k, 50.0);
            }
            self.swell(Inst::StringPad, recede * bar, span, 70.0, 60.0, 0.0);
        }

        // ---- sub-bass pulses and dark, low stabs
        for b in body..recede {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let bs = b * bar;
            let root = self.bass_key(&chord, 31);
            // the pulse thins to the downbeats when sparse, and fills in when busy
            if pulse == 0 {
                let at: &[u32] = if d < 0.3 { &[0] } else if d > 0.7 { &[0, 2, 3] } else { &[0, 2] };
                for at in at {
                    self.note(Inst::Cello, bs + at * beat, beat * 3 / 2, root, 52.0 + 28.0 * it);
                }
            } else {
                for i in (0..8u32).step_by(if d < 0.3 { 2 } else { 1 }) {
                    let wave = 0.75 + 0.25 * (i as f64 * std::f64::consts::TAU / 8.0).cos();
                    self.note(Inst::Cello, bs + i * beat / 2, beat * 2 / 5, root, (48.0 + 30.0 * it) * wave);
                }
            }
        }

        // ---- dark, low stabs: now and then, off the beat, often answered a
        // bar later by a softer one
        let mut srng = self.rng("rain-stabs");
        let mut b = body + 2;
        while b + 1 < recede {
            let it = self.it(b);
            if srng.chance((0.45 + 0.35 * it) * (0.5 + d)) {
                let chord = self.harmony[b as usize].clone();
                let voicing = voice_lead(&chord.pitch_classes(&self.scale), &[], 40, 58, false);
                let at = if stabs == 0 { *srng.pick(&[0, 3, 5]) } else { *srng.pick(&[3, 5, 7]) } * beat / 2;
                let start = b * bar + at;
                let vel = 70.0 + 22.0 * it;
                for k in voicing.iter().take(3) {
                    self.note(Inst::Pizzicato, start, beat, *k, vel);
                }
                if srng.chance(0.5) {
                    for k in voicing.iter().take(2) {
                        self.note(Inst::Pizzicato, start + bar, beat, *k, vel * 0.7);
                    }
                }
            }
            b += 2;
        }

        // ---- soft, distant drums in two-bar phrases that fill out slowly: a
        // dull, heavy kick, a gated snare far off, and later a hi-hat ticking
        // like a clock now and then
        let mut drng = self.rng("rain-drums");
        let last = recede.saturating_sub(1);
        let snare_from = drums_from + 4;
        let ticks_from = drums_from + last.saturating_sub(drums_from) / 3;
        for b in drums_from..last {
            let it = self.it(b);
            let bs = b * bar;
            let e = beat / 2;
            let kick = 72.0 + 14.0 * it;
            if (b - drums_from) % 2 == 0 {
                self.note(Inst::Drums, bs, e, drum::DEEP_KICK as i32, kick);
                continue;
            }
            // the answering bar: the kick lands late, then the snare
            let late: &[u32] = if kick_kind == 0 { &[3] } else { &[0, 5] };
            for at in late {
                self.note(Inst::Drums, bs + at * e, e, drum::DEEP_KICK as i32, kick - 10.0);
            }
            if b >= snare_from {
                self.note(Inst::Drums, bs + 2 * beat, e, drum::GATED_SNARE as i32, 56.0 + 16.0 * it);
            }
            if b >= ticks_from && drng.chance((0.4 + 0.3 * it) * (0.5 + d)) {
                for (i, at) in [1, 3, 5, 7].iter().enumerate() {
                    self.note(Inst::Drums, bs + at * e, e, drum::HAT as i32, 34.0 - 3.0 * i as f64);
                }
            }
        }

        // ---- distant glassy notes, now and then
        let mut krng = self.rng("rain-glass");
        for b in body..recede {
            if krng.chance((0.15 + 0.1 * self.it(b)) * (0.5 + d)) {
                let deg = loop {
                    let d = krng.range(14, 19);
                    if self.scale.is_pentatonic(d) {
                        break d;
                    }
                };
                let on = krng.range(0, 7) as u32 * beat / 2;
                self.note(Inst::Piano, b * bar + on, 3 * beat, self.scale.midi(deg), 44.0 + 14.0 * self.it(b));
            }
        }

        // ---- the lone lead, long and mournful with generous silence, and in
        // the body a smoky sax-like line answering it
        const PAT_DRIFT: &[Pattern] = &[&[(0, 8)], &[(2, 6)], &[(0, 6)], &[(0, 4), (4, 4)], &[(3, 5)]];
        let lead_on = |b: u32| {
            if b < lead_in || b + 2 >= n {
                false
            } else if b < body {
                (b - lead_in) % 6 < 4
            } else if b < recede {
                (b - body) % 8 < 4
            } else {
                b - recede < 4
            }
        };
        let sax_on = |b: u32| b >= sax_from && b < recede && (4..7).contains(&((b - body) % 8));
        let lead: Vec<Option<Inst>> = (0..n).map(|b| lead_on(b).then_some(Inst::Violin)).collect();
        let sax: Vec<Option<Inst>> = (0..n).map(|b| sax_on(b).then_some(Inst::Brass)).collect();
        let mut cfg =
            MelodyCfg { lo: 2, hi: 11, patterns: PAT_DRIFT, phrase_bars: 4, vel: 64.0, rest_bar_prob: 0.2 + 0.3 * (0.5 - d), ornaments: false };
        self.vary_melody(&mut cfg);
        let mut mrng = self.rng("melody");
        let mut line = self.melody(&mut mrng, &cfg, &lead);
        let cfg =
            MelodyCfg { lo: -2, hi: 7, patterns: PAT_SLOW_44, phrase_bars: 4, vel: 54.0, rest_bar_prob: 0.3 + 0.3 * (0.5 - d), ornaments: true };
        let mut srng = self.rng("sax");
        line.extend(self.melody(&mut srng, &cfg, &sax));
        // played loosely, almost out of time, as loose as the humanize
        let mut rubato = self.rng("rubato");
        let loose = (self.p.humanize as f64 / 0.4).min(2.5);
        for note in line.iter_mut() {
            note.start = (note.start as f64 + rubato.gauss() * loose * beat as f64 / 8.0).max(0.0) as u32;
        }
        self.notes.extend(line);
    }

    /// Cathedral of Stars: an epic score for space and time. A soft organ
    /// figure over a low drone; a ticking clock, a low string ostinato,
    /// swelling cello and viola lines, a tender piano theme, choir, rising
    /// strings and sub swells gathering like a tide; a climax of full organ,
    /// strings, brass, choir and timpani with the theme repeated and growing,
    /// its last chord held open and shimmering; then the organ figure alone.
    fn star_cathedral(&mut self) {
        let beat = self.ppq;
        let bar = self.bar;
        let e8 = beat / 2;
        let s16 = beat / 4;
        let n = self.bars;

        // The arc: the opening, the build, the climax (in four-bar phrases),
        // its last chord held, and the outro.
        let bounds = self.bounds();
        let (build, peak, hold, outro) = (bounds[0], bounds[1], bounds[2], bounds[3]);
        let d = self.density();
        let v = self.p.variation as f64;
        let smooth = |x: f64| x * x * (3.0 - 2.0 * x);

        self.intensity = (0..n)
            .map(|b| {
                let x = |a: u32, z: u32| (b.max(a) - a) as f64 / (z - a).max(1) as f64;
                if b < build {
                    0.1 + 0.08 * x(0, build)
                } else if b < peak {
                    0.2 + 0.65 * smooth(x(build, peak))
                } else if b < hold {
                    0.92 + 0.08 * x(peak, hold)
                } else if b < outro {
                    0.85 - 0.35 * x(hold, outro)
                } else {
                    0.3 - 0.25 * x(outro, n)
                }
            })
            .collect();
        // how far into the build, 0..1, and the bar it reaches a point
        let rise = |b: u32| ((b.max(build) - build) as f64 / (peak - build).max(1) as f64).min(1.0);
        // when each layer comes in: earlier when busy, later when sparse
        let late = self.late();
        let from = |x: f64| (build + ((peak - build) as f64 * (x * late).min(1.0)).round() as u32).min(peak);
        let ostinato_from = from(0.12);
        let lines_from = from(0.25);
        let piano_from = from(0.35);
        let choir_from = from(0.45);
        let swells_from = from(0.45);
        let pads_from = from(0.55);
        let brass_from = from(0.72);
        let kick_from = from(0.75);
        // the timpani roll into the climax
        let roll = peak.saturating_sub(2).max(build);

        // Harmony, two bars a chord: a minor key that lifts toward major. The
        // climax turns on the major chords, its last chord is held with a
        // major third, and the opening progression returns to close.
        let minor = self.p.mode.is_minor();
        let mut rng = self.rng("stars-harmony");
        let (opening, tide, climax): (&[&[i32]], &[&[i32]], &[&[i32]]) = if minor {
            (
                &[&[0, 5, 0, 5], &[0, 5, 2, 6], &[0, 3, 0, 5], &[0, 5, 3, 0]],
                &[&[0, 5, 2, 6], &[0, 3, 5, 6], &[0, 5, 3, 6], &[0, 2, 5, 6]],
                &[&[5, 2, 6, 2], &[5, 6, 2, 2], &[5, 2, 6, 6], &[5, 6, 2, 6]],
            )
        } else {
            (
                &[&[5, 3, 5, 3], &[5, 3, 0, 4], &[0, 5, 3, 4], &[5, 4, 3, 4]],
                &[&[5, 3, 0, 4], &[0, 4, 5, 3], &[5, 4, 3, 4], &[3, 4, 5, 5]],
                &[&[3, 4, 0, 0], &[3, 0, 4, 4], &[0, 4, 5, 3], &[3, 4, 0, 4]],
            )
        };
        let prog = |rng: &mut Rng, pool: &[&[i32]], w: &[f64]| -> Vec<Chord> {
            let degs = *rng.pick(pool);
            let exts = [Ext::Triad, Ext::Add9, Ext::Sus2, Ext::Sus4];
            degs.iter().map(|d| Chord::new(self.fix_dim(*d), exts[rng.weighted(w)])).collect()
        };
        let opening = prog(&mut rng, opening, &[0.3, 0.4, 0.3, 0.0]);
        let tide = prog(&mut rng, tide, &[0.1, 0.3, 0.35, 0.25]);
        let climax = prog(&mut rng, climax, &[0.45, 0.35, 0.2, 0.0]);
        let home = Chord::new(0, if minor { Ext::Sus2 } else { Ext::Add9 });
        self.harmony = (0..n)
            .map(|b| {
                if b >= hold && b < outro || b + 2 >= n {
                    home.clone()
                } else if b >= outro {
                    opening[((b - outro) / 2 % 4) as usize].clone()
                } else if b >= peak {
                    climax[((b - peak) / 2 % 4) as usize].clone()
                } else if b >= build {
                    tide[((b - build) / 2 % 4) as usize].clone()
                } else {
                    opening[(b / 2 % 4) as usize].clone()
                }
            })
            .collect();
        self.section = (0..n).map(|b| [build, peak, hold, outro].iter().filter(|s| b >= **s).count() as u8).collect();
        self.vary_harmony(&[hold..outro]);

        let figure = self.variant("figure", 3);
        let ostinato = self.variant("ostinato", 3);
        let offbeats = self.variant("tick", 2) == 1 || d > 0.7;
        // with the variation, the figure can turn into another at the climax
        // and in the outro
        let mut frng = self.rng("stars-figure");
        let figures = [figure, if frng.chance(0.7 * v) { (figure + 1 + frng.range(0, 1) as u32) % 3 } else { figure }];
        let pc = |key: i32| key.rem_euclid(12);
        let tonic = self.scale.midi(0);
        let tonic_pc = pc(tonic);
        // the held chord: open, and in a minor key lifted to major
        let held: Vec<i32> = {
            let mut pcs = vec![tonic_pc, pc(tonic + 7), pc(tonic + 2)];
            pcs.push(pc(tonic + 4));
            pcs
        };
        // the bass under a chord: the tonic held as a pedal through the
        // build, unless the chord would rub against it
        let pedal = |s: &Self, b: u32, lo: i32| {
            let chord = &s.harmony[b as usize];
            let rubs = chord.pitch_classes(&s.scale).iter().any(|p| matches!((p - tonic_pc).rem_euclid(12), 1 | 11));
            if b >= build && b < peak && b < build + (peak - build) / 2 && !rubs {
                s.bass_key(&Chord::new(0, Ext::Triad), lo)
            } else {
                s.bass_key(chord, lo)
            }
        };

        // version 4: more organ figures and string ostinatos, and no drums
        // From version 4, the organ has a wider set of figures, built on a
        // ladder of six chord tones over two octaves: broken chords in eighths
        // (rocking, sweeping, circling a held top note, or grouped 3+3+2) and
        // rolling triplets. With the variation it may answer each second
        // phrase with the figure turned upside down, and double into
        // sixteenths at the climax.
        const ORGAN: &[(u32, &[usize])] = &[
            (8, &[0, 2, 1, 2, 0, 2, 1, 2]),
            (8, &[0, 1, 2, 3, 2, 1, 2, 1]),
            (8, &[0, 2, 3, 2, 1, 2, 3, 2]),
            (8, &[0, 1, 2, 3, 4, 3, 2, 1]),
            (8, &[0, 4, 1, 4, 2, 4, 1, 4]),
            (8, &[0, 2, 4, 0, 2, 4, 1, 3]),
            (8, &[4, 2, 0, 2, 3, 1, 2, 1]),
            (12, &[0, 2, 4, 0, 2, 4, 1, 3, 5, 1, 3, 5]),
            (12, &[0, 1, 2, 3, 4, 5, 4, 3, 2, 1, 2, 3]),
        ];
        let new_figures = self.p.version >= 4;
        let mut orng = self.rng("stars-organ");
        let organ = [self.variant("organ", ORGAN.len() as u32) as usize; 2];
        let organ = if new_figures && orng.chance(0.7 * v) { [organ[0], (organ[0] + 1 + orng.range(0, ORGAN.len() as i32 - 2) as usize) % ORGAN.len()] } else { organ };
        let answer = new_figures && orng.chance(0.6 * v);
        let double = new_figures && orng.chance(0.25 + 0.4 * v) && d >= 0.3;

        // ---- the organ: the figure, rolling over each chord from the first
        // bar to the last, with a low pedal beneath
        let mut prev = Vec::new();
        for b in 0..n.saturating_sub(1) {
            let in_hold = b >= hold && b < outro;
            let chord = self.harmony[b as usize].clone();
            let voicing = voice_lead(&chord.pitch_classes(&self.scale), &prev, 55, 74, false);
            prev = voicing.clone();
            let it = self.it(b);
            let climax_bar = b >= peak && b < hold;
            // the tones, the order they are played in, and how many to a bar
            let (tones, order, per_bar): (Vec<i32>, Vec<usize>, u32) = if new_figures {
                let ladder: Vec<i32> = voicing.iter().copied().chain(voicing.iter().map(|k| k + 12)).take(6).collect();
                let (per, fig) = ORGAN[organ[(b >= peak) as usize]];
                let mut order = fig.to_vec();
                let section_start = [0, build, peak, outro].into_iter().filter(|s| *s <= b).max().unwrap_or(0);
                if answer && (b - section_start) / 4 % 2 == 1 {
                    let top = ladder.len() - 1;
                    order = order.iter().map(|o| top - (*o).min(top)).collect();
                }
                if double && climax_bar && per == 8 {
                    (ladder, order.repeat(2), 16)
                } else {
                    (ladder, order, per)
                }
            } else {
                let t = vec![voicing[0], voicing[1], voicing[voicing.len() - 1], voicing[0] + 12];
                let order = match figures[(b >= peak) as usize] {
                    0 => vec![0, 2, 1, 2, 0, 2, 1, 2],
                    1 => vec![0, 1, 2, 3, 2, 1, 2, 1],
                    _ => vec![0, 2, 3, 2, 1, 2, 3, 2],
                };
                (t, order, 8)
            };
            if !in_hold {
                let fade = if b >= outro { 1.0 - (b - outro) as f64 / (n - outro) as f64 } else { 1.0 };
                let vel = if climax_bar { 72.0 } else { (38.0 + 50.0 * it) * (0.5 + 0.5 * fade) };
                let up = if climax_bar { 12 } else { 0 };
                // sparse: half as many
                let step = if d < 0.3 { 2 } else { 1 };
                let len = bar / per_bar;
                // accents where the figure's groups begin
                let group = if per_bar == 12 { 3 } else { 4 * per_bar as usize / 8 };
                for (i, o) in order.iter().enumerate().step_by(step) {
                    let accent = if i % group == 0 { 6.0 } else { 0.0 };
                    self.note(Inst::Piano, b * bar + i as u32 * len, step as u32 * len, tones[(*o).min(tones.len() - 1)] + up, vel + accent);
                }
            }
            // the pedal: a soft drone at first, two octaves deep at the climax
            if b % 2 == 0 && b < outro {
                let low = pedal(self, b, 36);
                let span = 2.min(n - b) * bar;
                let vel = if b >= peak { 80.0 } else { 40.0 + 40.0 * it };
                self.note(Inst::Piano, b * bar, span, low, vel);
                if b >= peak {
                    self.note(Inst::Piano, b * bar, span, low - 12, vel);
                }
            }
            // full organ at the climax: wide chords, held
            if climax_bar && (b - peak) % 2 == 0 {
                let v = voice_lead(&chord.pitch_classes(&self.scale), &[], 48, 76, true);
                for k in v {
                    self.note(Inst::Piano, b * bar, 2 * bar, k, 74.0);
                }
            }
        }
        // the held chord, full organ, dying away into the outro
        for k in voice_lead(&held, &[], 48, 79, true).into_iter().chain([tonic - 24, tonic - 12]) {
            self.note(Inst::Piano, hold * bar, (outro - hold) * bar + beat, k, 80.0);
        }
        // the organ's swell pedal: the whole arc, from a whisper to full and
        // back to silence
        for b in 0..n {
            let x = |a: u32, z: u32| (b - a) as f64 / (z - a).max(1) as f64;
            let (a, z) = if b < build {
                (66.0, 72.0)
            } else if b < peak {
                let (x0, x1) = (x(build, peak), (x(build, peak) + 1.0 / (peak - build).max(1) as f64).min(1.0));
                (72.0 + 20.0 * smooth(x0), 72.0 + 20.0 * smooth(x1))
            } else if b < hold {
                (92.0, 100.0)
            } else if b < outro {
                (100.0, 76.0)
            } else {
                let f = |b: u32| 80.0 * (1.0 - (b - outro) as f64 / (n - outro) as f64);
                (f(b), f(b + 1))
            };
            self.swell(Inst::Piano, b * bar, bar, a, (a + z) / 2.0, z);
        }

        // ---- the theme: four bars, simple and long-breathed, shaped by the
        // seed, fitted to whatever chord is under it
        let mut mrng = self.rng("stars-theme");
        const RHYTHMS: &[&[(u32, u32)]] = &[&[(0, 8)], &[(0, 4), (4, 4)], &[(0, 6), (6, 2)], &[(0, 3), (3, 5)], &[(2, 6)]];
        let mut theme: Vec<(u32, u32, i32)> = Vec::new();
        let mut deg = *mrng.pick(&[2, 4, 4, 7]);
        for i in 0..4u32 {
            let rhythm = if i == 3 { RHYTHMS[0] } else { *mrng.pick(&RHYTHMS[..if i == 0 { 4 } else { 5 }]) };
            for (on, len) in rhythm {
                theme.push((i * 8 + on, *len, deg));
                // steps, and with the variation leaps
                let step = if mrng.chance(0.4 * v) { *mrng.pick(&[-4, -3, 3, 4]) } else { [-1, 1, -2, 2, -1, 1][mrng.range(0, 5) as usize] };
                deg = (deg + step).clamp(1, 8);
            }
        }
        let fit = |s: &Self, b: u32, d: i32| {
            let chord = &s.harmony[b as usize];
            [0, -1, 1, -2, 2].iter().map(|o| d + o).find(|d| chord.is_core_tone(*d)).unwrap_or(d)
        };
        let drop = (0.45 - 0.4 * d).max(0.0);
        let play_theme = |s: &mut Self, theme: &[(u32, u32, i32)], b0: u32, inst: Inst, octaves: &[i32], vel: f64, rng: &mut Rng, sparse: bool| {
            for (i, (on, len, d)) in theme.iter().enumerate() {
                let b = b0 + on / 8;
                if b >= n || (sparse && i > 0 && i + 1 < theme.len() && rng.chance(drop)) {
                    continue;
                }
                let d = if *len >= 3 { fit(s, b, *d) } else { *d };
                for o in octaves {
                    s.note(inst, b0 * bar + on * e8, len * e8 + e8 / 2, s.scale.midi(d + 7 * o), vel);
                }
            }
        };

        // ---- the piano: sparse, tender, the theme in the high register,
        // and now and then a low note under it
        let mut prng = self.rng("stars-piano");
        let mut b = piano_from;
        while b + 4 <= peak {
            let vel = 42.0 + 16.0 * rise(b);
            play_theme(self, &theme, b, Inst::Pizzicato, &[1], vel, &mut prng, true);
            for k in (0..4).step_by(2) {
                let low = self.bass_key(&self.harmony[(b + k) as usize], 48);
                self.note(Inst::Pizzicato, (b + k) * bar, 2 * bar, low, vel - 12.0);
            }
            b += 8;
        }
        // ---- at the climax, the theme four bars at a time, growing each
        // time: the organ's top, the violins, then an octave higher, the piano
        for (r, b) in (peak..hold).step_by(4).enumerate() {
            if b + 4 > hold {
                break;
            }
            // with the variation, each time round a note of it can move
            if r > 0 && mrng.chance(v) && theme.len() > 2 {
                let i = mrng.range(1, theme.len() as i32 - 2) as usize;
                theme[i].2 = (theme[i].2 + *mrng.pick(&[-1, 1, 2])).clamp(1, 8);
            }
            let grow = 6.0 * r as f64;
            play_theme(self, &theme, b, Inst::Piano, &[1], 84.0 + grow, &mut prng, false);
            play_theme(self, &theme, b, Inst::StringPad, if r == 0 { &[1] } else { &[1, 2] }, 84.0 + grow, &mut prng, false);
            play_theme(self, &theme, b, Inst::Pizzicato, &[1, 2], 72.0 + grow, &mut prng, false);
        }

        // ---- strings: two lines, cellos and violas, swelling slowly; then
        // pads rising through the build; huge at the climax; the held chord
        let mut prev = Vec::new();
        for b in (lines_from..hold).step_by(2) {
            let chord = self.harmony[b as usize].clone();
            let start = b * bar;
            let span = 2.min(hold - b) * bar;
            let t = rise(b);
            if b < pads_from {
                let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 48, 67, false);
                prev = v.clone();
                for k in [v[0], v[v.len() - 1]] {
                    self.note(Inst::StringPad, start, span + s16, k, 50.0 + 20.0 * t);
                }
                self.swell(Inst::StringPad, start, span, 20.0, 55.0 + 35.0 * t, 30.0);
            } else if b < peak {
                let lo = 48 + (10.0 * t).round() as i32;
                let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, lo, lo + 21, false);
                prev = v.clone();
                for k in &v {
                    self.note(Inst::StringPad, start, span + s16, *k, 50.0 + 25.0 * t);
                }
                if b + 2 >= peak {
                    self.swell(Inst::StringPad, start, span, 50.0, 90.0, 120.0);
                } else {
                    self.swell(Inst::StringPad, start, span, 35.0 + 30.0 * t, 70.0 + 35.0 * t, 50.0 + 35.0 * t);
                }
            } else {
                let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 48, 84, true);
                prev = v.clone();
                for k in &v {
                    self.note(Inst::StringPad, start, span + s16, *k, 80.0);
                }
                self.swell(Inst::StringPad, start, span, 80.0, 105.0, 90.0);
            }
        }
        // the held chord: strings high and wide, the top shimmering an
        // octave up, ringing away
        let hold_span = (outro - hold) * bar + bar;
        let wide = voice_lead(&held, &prev, 52, 88, true);
        for k in wide.iter().chain(wide.last().map(|k| k + 12).iter()) {
            self.note(Inst::StringPad, hold * bar, hold_span, *k, 80.0);
        }
        self.swell(Inst::StringPad, hold * bar, hold_span, 100.0, 92.0, 0.0);

        // ---- the choir: soft pads high above, wide at the climax
        let mut prev = Vec::new();
        for b in (choir_from..hold).step_by(2) {
            let chord = self.harmony[b as usize].clone();
            let start = b * bar;
            let span = 2.min(hold - b) * bar;
            let climax = b >= peak;
            let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, if climax { 55 } else { 62 }, if climax { 84 } else { 77 }, climax);
            prev = v.clone();
            for k in &v {
                self.note(Inst::Violin, start, span + s16, *k, if climax { 76.0 } else { 46.0 + 20.0 * rise(b) });
            }
            if climax {
                self.swell(Inst::Violin, start, span, 75.0, 105.0, 90.0);
            } else {
                self.swell(Inst::Violin, start, span, 15.0, 45.0 + 45.0 * rise(b), 35.0 + 40.0 * rise(b));
            }
        }
        for k in voice_lead(&held, &prev, 57, 84, true) {
            self.note(Inst::Violin, hold * bar, hold_span, k, 76.0);
        }
        self.swell(Inst::Violin, hold * bar, hold_span, 100.0, 90.0, 0.0);

        // From version 4, more ostinatos, in sixteenths: (start, length,
        // rung), the rungs climbing the chord's tones above the bass note:
        // rocking root and octave, a broken chord up and down, groups of
        // 3+3+2, a gallop, a dotted swing, and a climb to the tenth. With the
        // variation, the climax may take another.
        const OSTINATO: &[&[(u32, u32, usize)]] = &[
            &[(0, 2, 0), (2, 2, 0), (4, 2, 2), (6, 2, 0), (8, 2, 0), (10, 2, 0), (12, 2, 2), (14, 2, 0)],
            &[(0, 2, 0), (2, 2, 1), (4, 2, 2), (6, 2, 3), (8, 2, 2), (10, 2, 1), (12, 2, 2), (14, 2, 1)],
            &[(0, 2, 0), (2, 2, 1), (4, 2, 2), (6, 2, 0), (8, 2, 1), (10, 2, 2), (12, 2, 0), (14, 2, 2)],
            &[(0, 2, 0), (2, 1, 2), (3, 1, 1), (4, 2, 0), (6, 1, 2), (7, 1, 1), (8, 2, 0), (10, 1, 2), (11, 1, 1), (12, 2, 0), (14, 1, 2), (15, 1, 1)],
            &[(0, 3, 0), (3, 1, 0), (4, 3, 1), (7, 1, 0), (8, 3, 2), (11, 1, 0), (12, 3, 1), (15, 1, 0)],
            &[(0, 2, 0), (2, 2, 1), (4, 2, 2), (6, 2, 3), (8, 2, 4), (10, 2, 3), (12, 2, 2), (14, 2, 1)],
            &[(0, 4, 0), (4, 2, 2), (6, 2, 1), (8, 4, 0), (12, 2, 2), (14, 2, 3)],
        ];
        let mut srng = self.rng("stars-ostinato");
        let strings = self.variant("strings", OSTINATO.len() as u32) as usize;
        let strings = [strings, if srng.chance(0.6 * v) { (strings + 1 + srng.range(0, OSTINATO.len() as i32 - 2) as usize) % OSTINATO.len() } else { strings }];

        // ---- the low string ostinato, on the pedal; at the climax doubled
        // an octave down and held beneath
        for b in (ostinato_from..hold).filter(|_| !new_figures) {
            let root = pedal(self, b, 36);
            let (steps, octave_at): (&[u32], &[u32]) = match if d < 0.3 { 3 } else { ostinato } {
                0 => (&[0, 1, 2, 3, 4, 5, 6, 7], &[2, 6]),
                1 => (&[0, 1, 2, 3, 4, 5, 6, 7], &[1, 3, 5, 7]),
                2 => (&[0, 2, 3, 4, 6, 7], &[3, 7]),
                _ => (&[0, 2, 4, 6], &[6]),
            };
            let climax = b >= peak;
            let vel = if climax { 70.0 } else { 40.0 + 34.0 * rise(b) };
            for s in steps {
                let key = if octave_at.contains(s) { root + 12 } else { root };
                let accent = if s % 4 == 0 { 8.0 } else { 0.0 };
                self.note(Inst::Cello, b * bar + s * e8, e8 * 4 / 5, key, vel + accent);
            }
            if climax && (b - peak) % 2 == 0 {
                self.note(Inst::Cello, b * bar, 2 * bar, root - 12, 78.0);
            }
        }
        for b in (ostinato_from..hold).filter(|_| new_figures) {
            let root = pedal(self, b, 36);
            // the bass note, then the chord's tones above it
            let pcs = self.harmony[b as usize].pitch_classes(&self.scale);
            let above: Vec<i32> = (root + 1..=root + 24).filter(|k| pcs.contains(&k.rem_euclid(12))).collect();
            let rungs: Vec<i32> = [root].into_iter().chain(above.iter().copied().filter(|k| *k - root >= 5)).collect();
            let climax = b >= peak;
            let vel = if climax { 70.0 } else { 40.0 + 34.0 * rise(b) };
            // sparse: a quarter-note pulse on the root and octave
            let quarters: &[(u32, u32, usize)] = &[(0, 4, 0), (4, 4, 0), (8, 4, 0), (12, 4, 2)];
            let pattern = if d < 0.3 { quarters } else { OSTINATO[strings[climax as usize]] };
            for &(on, len, rung) in pattern {
                // the octave above the bass for the third rung, whatever the chord
                let key = if rung == 2 { root + 12 } else { rungs[rung.min(rungs.len() - 1)] };
                let accent = if on % 8 == 0 { 8.0 } else if on % 4 == 0 { 3.0 } else { 0.0 };
                self.note(Inst::Cello, b * bar + on * s16, len * s16 * 4 / 5, key, vel + accent);
            }
            if climax && (b - peak) % 2 == 0 {
                self.note(Inst::Cello, b * bar, 2 * bar, root - 12, 78.0);
            }
        }
        self.note(Inst::Cello, hold * bar, hold_span, tonic - 24, 78.0);
        self.note(Inst::Cello, hold * bar, hold_span, tonic - 36, 72.0);

        // ---- brass: swells late in the build, full at the climax
        for b in (brass_from..hold).step_by(2) {
            let chord = self.harmony[b as usize].clone();
            let start = b * bar;
            let span = 2.min(hold - b) * bar;
            let root = self.bass_key(&chord, 43);
            if b < peak {
                let fifth = self.scale.midi(self.scale.degree_near(root) + 4);
                for k in [root, fifth] {
                    self.note(Inst::Brass, start, span + s16, k, 64.0 + 16.0 * rise(b));
                }
                if b + 2 >= peak {
                    self.swell(Inst::Brass, start, span, 15.0, 70.0, 127.0);
                } else {
                    self.swell(Inst::Brass, start, span, 10.0, 45.0 + 50.0 * rise(b), 20.0);
                }
            } else {
                for k in voice_lead(&chord.pitch_classes(&self.scale), &[], 46, 67, false) {
                    self.note(Inst::Brass, start, span + s16, k, 84.0);
                }
                self.swell(Inst::Brass, start, span, 75.0, 120.0, 95.0);
            }
        }
        for k in voice_lead(&held, &[], 45, 69, false) {
            self.note(Inst::Brass, hold * bar, hold_span, k, 88.0);
        }
        self.swell(Inst::Brass, hold * bar, hold_span, 118.0, 80.0, 0.0);

        // ---- the clock, ticking from the build to the very end; low drums
        // gathering late in the build and pounding at the climax. From
        // version 4 there are no drums: the timpani carries the climax.
        for b in (build..n.saturating_sub(1)).filter(|_| !new_figures) {
            if b >= hold && b < outro {
                continue;
            }
            let bs = b * bar;
            let vel = if b >= outro {
                42.0 * (1.0 - (b - outro) as f64 / (n - outro) as f64) + 8.0
            } else if b >= peak {
                56.0
            } else {
                30.0 + 22.0 * rise(b)
            };
            for i in 0..4 {
                self.note(Inst::Drums, bs + i * beat, e8, drum::TICK as i32, vel + if i == 0 { 6.0 } else { 0.0 });
                if offbeats && b >= pads_from && b < hold {
                    self.note(Inst::Drums, bs + i * beat + e8, e8, drum::TICK as i32, vel * 0.55);
                }
            }
        }
        for b in (kick_from..roll).filter(|_| !new_figures) {
            if (b - kick_from) % 2 == 0 {
                self.note(Inst::Drums, b * bar, beat, drum::DEEP_KICK as i32, 56.0 + 16.0 * rise(b));
            }
        }
        for b in (peak..hold).filter(|_| !new_figures) {
            let bs = b * bar;
            let phrase = (b - peak) % 4;
            if phrase == 0 && (b - peak) % 8 == 0 {
                self.note(Inst::Drums, bs, 2 * bar, drum::CRASH as i32, 68.0);
            }
            self.note(Inst::Drums, bs, beat, drum::DEEP_KICK as i32, 96.0);
            self.note(Inst::Drums, bs + 2 * beat, beat, drum::TOM_LOW as i32, 82.0);
            if phrase % 2 == 1 {
                self.note(Inst::Drums, bs + 3 * beat, e8, drum::TOM_LOW as i32, 70.0);
                self.note(Inst::Drums, bs + 3 * beat + e8, e8, drum::TOM_MID as i32, 78.0);
            }
        }

        // ---- timpani: a roll swelling into the climax, the downbeats
        // thundering through it, and a roll dying away under the held chord
        let timp = |pc_of: i32| fx::TIMPANI as i32 + pc(pc_of);
        let fifth = self.scale.midi(4);
        if roll < peak {
            let steps = (peak - roll) * bar / s16;
            for i in 0..steps {
                let x = i as f64 / steps as f64;
                self.note(Inst::Fx, roll * bar + i * s16, s16, timp(tonic), 22.0 + 80.0 * x * x);
            }
        }
        for b in peak..hold {
            let bs = b * bar;
            let root = self.scale.midi(self.harmony[b as usize].root);
            self.note(Inst::Fx, bs, beat, timp(root), 88.0);
            if (b - peak) % 2 == 1 {
                self.note(Inst::Fx, bs + 2 * beat, beat, timp(fifth), 80.0);
            }
            if (b - peak) % 4 == 3 {
                self.note(Inst::Fx, bs + 3 * beat, e8, timp(tonic), 80.0);
                self.note(Inst::Fx, bs + 3 * beat + e8, e8, timp(fifth), 90.0);
            }
        }
        {
            let steps = (outro - hold) * bar / e8;
            for i in 0..steps {
                let x = i as f64 / steps as f64;
                self.note(Inst::Fx, hold * bar + i * e8, e8, timp(tonic), 100.0 * (1.0 - x).powi(2) + 10.0);
            }
        }

        // ---- the sub drone, analogue swells, sub hits and starry grains
        let drone = fx::DRONE as i32 + tonic_pc;
        self.note(Inst::Fx, 0, peak * bar, drone, 50.0);
        self.note(Inst::Fx, peak * bar, (outro - peak) * bar, drone, 64.0);
        self.note(Inst::Fx, outro * bar, (n - outro).saturating_sub(2).max(1) * bar, drone, 42.0);
        for b in (swells_from..hold).step_by(4) {
            let root = self.scale.midi(self.harmony[b as usize].root);
            self.note(Inst::Fx, b * bar, 4 * bar, fx::ANALOG_DRONE as i32 + pc(root), 50.0 + 30.0 * self.it(b));
        }
        if roll > build {
            let len = (kit::REVERSE_LONG_SECS * self.p.tempo.clamp(30.0, 200.0) as f64 / 60.0 * beat as f64) as u32;
            self.note(Inst::Fx, (peak * bar).saturating_sub(len), len, fx::REVERSE_LONG as i32, 58.0);
        }
        self.note(Inst::Fx, peak * bar, 2 * bar, fx::BOOM as i32 + tonic_pc, 84.0);
        self.note(Inst::Fx, hold * bar, 2 * bar, fx::BOOM as i32 + tonic_pc, 72.0);
        for b in (0..pads_from).step_by(8).chain([hold, outro]) {
            let root = self.scale.midi(self.harmony[b as usize].root);
            let vel = if b == hold { 64.0 } else { 44.0 };
            self.note(Inst::Fx, b * bar, 4.min(n - b) * bar, fx::GRAINS as i32 + pc(root), vel);
        }
    }

    /// Neon Glide: a lush, retro-futuristic synth score for a neon city at
    /// night, gliding on a slow groove. A CS-80 pad swelling in over rain and
    /// a kalimba pulse in dotted eighths rolling against the beat; then a
    /// patient groove of muted kick, filtered snare and ghost notes, shaker,
    /// tabla in threes against fours and wooden clicks, a swung sub bass,
    /// electric piano in minor sevenths and ninths, and a lonely, sax-like
    /// lead with a simple theme; the groove opening up, the pads blooming and
    /// the lead growing; a sparse bridge; and a last swell fading to shimmer.
    fn neon_glide(&mut self) {
        let beat = self.ppq;
        let bar = self.bar;
        let e8 = beat / 2;
        let s16 = beat / 4;
        let n = self.bars;
        let ticks = |secs: f64| (secs * self.p.tempo.clamp(30.0, 200.0) as f64 / 60.0 * beat as f64) as u32;
        let bounds = self.bounds();
        let (groove, bloom, bridge, finale) = (bounds[0], bounds[1], bounds[2], bounds[3]);
        let d = self.density();
        let v = self.p.variation as f64;
        // the last chord, held as it fades
        let held = n - ((n - finale) / 3).clamp(1, 4).min(n - finale);
        let in_groove = |b: u32| (b >= groove && b < bridge) || (b >= finale && b < held);
        let in_bridge = |b: u32| b >= bridge && b < finale;

        self.intensity = (0..n)
            .map(|b| {
                let x = |a: u32, z: u32| (b.max(a) - a) as f64 / (z - a).max(1) as f64;
                if b < groove {
                    0.2 + 0.1 * x(0, groove)
                } else if b < bloom {
                    0.4 + 0.2 * x(groove, bloom)
                } else if b < bridge {
                    0.6 + 0.35 * x(bloom, bridge)
                } else if b < finale {
                    0.3
                } else if b < held {
                    0.85 + 0.15 * x(finale, held)
                } else {
                    0.6 - 0.5 * x(held, n)
                }
            })
            .collect();
        // when each layer of the groove comes in: earlier when busy, later when sparse
        let late = self.late();
        let into = |x: f64| (groove + ((bloom - groove) as f64 * (x * late).min(1.0)).round() as u32).min(bloom);
        let kalimba_from = (groove as f64 * (0.4 * late).min(1.0)).round() as u32;
        let shaker_from = into(0.1);
        let tabla_from = into(0.3);
        let wood_from = into(0.5);
        // the lead comes in at the start of its theme, eight bars on
        let lead_from = (groove + (into(0.2) - groove).div_ceil(8) * 8).min(bloom.max(groove));

        // Harmony, two bars a chord: minor sevenths and ninths, and suspensions
        // left hanging. The groove, the bloom and the finale share a
        // progression; the bridge has its own; the last chord is held.
        let mut rng = self.rng("glide-harmony");
        let mains: [&[i32]; 5] = [&[0, 5, 3, 4], &[0, 3, 5, 4], &[0, 5, 2, 6], &[0, 6, 5, 4], &[0, 2, 5, 3]];
        let middles: [&[i32]; 4] = [&[5, 3, 5, 4], &[3, 0, 5, 4], &[5, 6, 3, 4], &[3, 5, 3, 4]];
        let exts = [Ext::Seventh, Ext::Ninth, Ext::Sus4, Ext::Add9];
        let prog = |rng: &mut Rng, pool: &[&[i32]], w: &[f64]| -> Vec<Chord> {
            let degs = *rng.pick(pool);
            degs.iter().map(|d| Chord::new(self.fix_dim(*d), exts[rng.weighted(w)])).collect()
        };
        let main = prog(&mut rng, &mains, &[0.35, 0.35, 0.15, 0.15]);
        let middle = prog(&mut rng, &middles, &[0.3, 0.25, 0.3, 0.15]);
        let home = Chord::new(0, if rng.chance(0.5) { Ext::Ninth } else { Ext::Add9 });
        // where the chords count from: each chord lasts two bars from here
        let anchor = |b: u32| [0, groove, bridge, finale, held].into_iter().filter(|a| *a <= b).max().unwrap_or(0);
        self.harmony = (0..n)
            .map(|b| {
                let i = ((b - anchor(b)) / 2 % 4) as usize;
                if b >= held {
                    home.clone()
                } else if in_bridge(b) {
                    middle[i].clone()
                } else {
                    main[i].clone()
                }
            })
            .collect();
        self.section = (0..n).map(|b| bounds.iter().filter(|s| b >= **s).count() as u8).collect();
        self.vary_harmony(&[held..n]);
        // each chord: (first bar, bars)
        let mut chords: Vec<(u32, u32)> = Vec::new();
        for b in 0..n {
            if b == anchor(b) || (b - anchor(b)) % 2 == 0 {
                chords.push((b, 1));
            } else if let Some(last) = chords.last_mut() {
                last.1 += 1;
            }
        }
        let tonic_pc = self.scale.midi(0).rem_euclid(12);
        let end = n * bar + bar;

        // ---- the CS-80: chords swelling in and out, every fourth breathing
        // (and sagging, see expression); in the bloom and the finale they open
        // out an octave, and the sweep pad blooms above them
        let mut prev = Vec::new();
        let mut wide = Vec::new();
        for (i, &(b, len)) in chords.iter().enumerate() {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let start = b * bar;
            let span = len * bar;
            let last = b >= held;
            let breath = i % 4 == 3 && !last;
            let dur = if last { span } else if breath { span - beat - e8 } else { span + beat / 4 };
            let voicing = self.thin(voice_lead(&chord.pitch_classes(&self.scale), &prev, 48, 72, false));
            prev = voicing.clone();
            for k in &voicing {
                self.note(Inst::StringPad, start, dur, *k, 46.0 + 30.0 * it);
            }
            let blooming = (b >= bloom && b < bridge) || b >= finale;
            if blooming {
                if let Some(top) = voicing.last() {
                    self.note(Inst::StringPad, start, dur, top + 12, 34.0 + 24.0 * it);
                }
                let spread = voice_lead(&chord.pitch_classes(&self.scale), &wide, 60, 86, true);
                wide = spread.clone();
                for k in &spread {
                    self.note(Inst::Brass, start, dur, *k, 50.0 + 30.0 * it);
                }
                let to = if last { 0.0 } else { 50.0 };
                self.swell(Inst::Brass, start, span, 20.0, if last { 95.0 } else { 70.0 + 30.0 * it }, to);
            }
            if b < groove {
                // the opening: each chord swells in slowly from nothing
                self.swell(Inst::StringPad, start, span, 0.0, 85.0 + 15.0 * it, 60.0);
            } else if last {
                self.swell(Inst::StringPad, start, span + bar, 95.0, 100.0, 0.0);
            } else {
                let to = if breath { 30.0 } else { 50.0 + 20.0 * it };
                self.swell(Inst::StringPad, start, span, 30.0 + 30.0 * it, 80.0 + 40.0 * it, to);
            }
        }

        // ---- the electric piano: slow, rolled chords without their root (the
        // bass has it), from the groove to the end; alone with the pad in
        // the bridge, where it rolls each chord out slowly
        let keys = self.variant("keys", 2);
        let mut prev = Vec::new();
        for &(b, len) in chords.iter().filter(|(b, _)| *b >= groove) {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let mut pcs = chord.pitch_classes(&self.scale);
            if pcs.len() >= 4 {
                pcs.remove(0);
            }
            let voicing = voice_lead(&pcs, &prev, 52, 74, false);
            prev = voicing.clone();
            let start = b * bar;
            let span = len * bar;
            let vel = 54.0 + 16.0 * it;
            // anticipated by an eighth, now and then
            let early = keys == 1 && b > groove && b < held && (b / 2) % 2 == 1;
            let at = if early { start - e8 } else { start };
            let roll = if in_bridge(b) { e8 } else { 25 };
            for (j, k) in voicing.iter().enumerate() {
                self.note(Inst::Piano, at + j as u32 * roll, span - j as u32 * roll, *k, vel - 3.0 * j as f64);
            }
            // a soft touch again in the chord's second bar, on the "and" of two
            if len >= 2 && b < held && d >= 0.3 {
                let again = start + bar + 3 * e8;
                for k in voicing.iter().rev().take(2) {
                    self.note(Inst::Piano, again, bar - 3 * e8, *k, vel - 14.0);
                }
            }
        }

        // ---- the kalimba: chord tones in dotted eighths, so the pattern rolls
        // three against four and comes round to the bar every three bars;
        // half as often in the bridge, and fading at the end
        let pattern: &[usize] = match self.variant("kalimba", 3) {
            0 => &[0, 2, 1, 3],
            1 => &[0, 1, 2, 3, 2, 1],
            _ => &[0, 3, 1, 2],
        };
        let mut k = 0usize;
        let mut t = kalimba_from * bar;
        while t < (n - 1) * bar {
            let b = t / bar;
            let thin = in_bridge(b) || d < 0.3;
            if !thin || k % 2 == 0 {
                let chord = self.harmony[b as usize].clone();
                let tones = voice_lead(&chord.pitch_classes(&self.scale), &[], 62, 86, false);
                let key = tones[pattern[k % pattern.len()] % tones.len()];
                let fading = if b >= held { 1.0 - (t - held * bar) as f64 / ((n - 1 - held).max(1) * bar) as f64 } else { 1.0 };
                let accent = if t % beat == 0 { 8.0 } else { 0.0 };
                self.note(Inst::Pizzicato, t, 3 * s16, key, (44.0 + 16.0 * self.it(b) + accent) * fading.max(0.3));
            }
            k += 1;
            t += 3 * s16;
        }

        // ---- the sub bass: a warm, swung figure under the groove, leaning
        // into the next chord a half step below; one long note at the end
        let pat: &[(u32, u32, i32)] =
            if self.variant("bass", 2) == 0 { &[(0, 3, 0), (3, 1, 0), (4, 3, 7), (7, 1, 12)] } else { &[(0, 2, 0), (3, 2, 0), (5, 1, 7), (6, 2, 0)] };
        let swing = e8 / 6;
        for b in (0..n).filter(|b| in_groove(*b)) {
            let chord = self.harmony[b as usize].clone();
            let root = self.bass_key(&chord, 33);
            let next = self.harmony.get(b as usize + 1).map(|c| self.bass_key(c, 33));
            let it = self.it(b);
            for &(on, len, iv) in pat {
                if d < 0.3 && on == 7 {
                    continue;
                }
                let start = b * bar + on * e8 + if on % 2 == 1 { swing } else { 0 };
                let mut key = root + iv;
                if on >= 6 && next.is_some_and(|x| x != root) {
                    key = next.unwrap() - 1;
                }
                self.note(Inst::Cello, start, len * e8 - e8 / 4, key, 72.0 + 14.0 * it + if on == 0 { 8.0 } else { 0.0 });
            }
            if d > 0.7 {
                self.note(Inst::Cello, b * bar + 15 * s16 + swing / 2, s16, root + 12, 60.0);
            }
        }
        if held < n {
            let root = self.bass_key(&self.harmony[held as usize], 33);
            self.note(Inst::Cello, held * bar, (n - held) * bar, root, 70.0);
        }

        // ---- the groove: a muted kick, a soft filtered snare on three with
        // ghost notes around it, a swung shaker, tabla in threes against the
        // beat, and wooden clicks; in the bridge only the hand percussion,
        // soft and far off in its echoes
        let kick_kind = self.variant("kick", 2);
        let tabla_kind = self.variant("tabla", 2);
        let mut grng = self.rng("glide-ghosts");
        let wood_at: Vec<u32> = (0..2).map(|_| *grng.pick(&[2, 5, 13, 14])).collect();
        let swung = |i: u32| i * s16 + if i % 2 == 1 { s16 / 5 } else { 0 };
        for b in groove..held {
            let bs = b * bar;
            let it = self.it(b);
            let quiet = in_bridge(b);
            let phrase = (b - anchor(b)) % 2;
            if !quiet {
                let kicks: &[u32] = match (kick_kind, phrase) {
                    (0, 0) => &[0, 10],
                    (0, _) => &[0],
                    (_, 0) => &[0, 7],
                    _ => &[0, 10],
                };
                for at in kicks {
                    self.note(Inst::Drums, bs + at * s16, e8, drum::DEEP_KICK as i32, if *at == 0 { 84.0 } else { 70.0 } + 8.0 * it);
                }
                if d > 0.7 && phrase == 1 {
                    self.note(Inst::Drums, bs + swung(11), e8, drum::DEEP_KICK as i32, 64.0);
                }
                self.note(Inst::Drums, bs + 2 * beat, e8, drum::GATED_SNARE as i32, 46.0 + 12.0 * it);
                let ghosts = (1.0 + 2.0 * d + it).round() as usize;
                for _ in 0..ghosts {
                    let at = *grng.pick(&[3, 6, 11, 13, 14, 15]);
                    self.note(Inst::Drums, bs + swung(at), s16, drum::GATED_SNARE as i32, grng.range_f(18.0, 28.0));
                }
            }
            if b >= shaker_from {
                let step = if d < 0.3 || quiet { 2 } else { 1 };
                for i in (0..16).step_by(step) {
                    let vel = [22.0, 30.0, 26.0, 36.0][i as usize % 4] + 6.0 * it - if quiet { 10.0 } else { 0.0 };
                    self.note(Inst::Drums, bs + swung(i), s16, drum::SHAKER as i32, vel);
                }
            }
            if b >= tabla_from {
                // three strokes to every two beats, or a stroke every three sixteenths
                let (gap, count) = if tabla_kind == 0 { (2 * beat / 3, 6) } else { (3 * s16, 5) };
                let strokes = [drum::TABLA_HIGH, drum::TABLA_LOW, drum::TABLA_HIGH, drum::TABLA_HIGH, drum::TABLA_LOW, drum::TABLA_HIGH];
                for i in 0..count {
                    if (quiet && i % 3 != 0) || grng.chance(0.25 * (1.0 - d)) {
                        continue;
                    }
                    let key = strokes[i as usize % strokes.len()];
                    let vel = if key == drum::TABLA_LOW { 52.0 } else { 44.0 } + 12.0 * it - if quiet { 8.0 } else { 0.0 };
                    self.note(Inst::Drums, bs + i * gap, e8, key as i32, vel);
                }
            }
            if b >= wood_from {
                let at = wood_at[(phrase as usize).min(1)];
                self.note(Inst::Drums, bs + swung(at), s16, drum::WOOD as i32, 38.0 + 8.0 * it);
            }
        }

        // ---- the lead: a simple theme of two four-bar phrases, the second
        // answering the first, each three bars of long notes and wide leaps
        // and a bar of rest. It comes in during the groove, rises an octave
        // as the pads bloom, rests through the bridge and sings out once more
        // in the finale.
        const CELLS: &[&[(u32, u32)]] =
            &[&[(0, 6), (6, 2)], &[(2, 6)], &[(0, 3), (3, 5)], &[(0, 8)], &[(1, 3), (4, 4)], &[(0, 2), (2, 6)], &[(0, 5), (5, 3)]];
        let mut trng = self.rng("glide-theme");
        let mut theme: Vec<(u32, u32, u32, i32)> = Vec::new();
        let mut deg = trng.range(4, 6);
        for phrase in 0..2u32 {
            for pb in 0..3u32 {
                let cell = if pb == 2 { *trng.pick(&[&CELLS[1], &CELLS[3]]) } else { *trng.pick(CELLS) };
                for &(on, dur) in cell {
                    let step = if dur >= 4 {
                        *trng.pick(&[-4, -3, 3, 4, 5, 7, -2, 2])
                    } else {
                        *trng.pick(&[-2, -1, 1, 2])
                    };
                    deg += step;
                    if !(2..=12).contains(&deg) {
                        deg -= 2 * step;
                    }
                    deg = deg.clamp(2, 12);
                    theme.push((phrase * 4 + pb, on, dur, deg));
                }
            }
        }
        let mut vrng = self.rng("glide-lead-variation");
        let mut line = Vec::new();
        for b in (0..n).filter(|b| (*b >= lead_from && *b < bridge) || (*b >= finale && *b < held)) {
            let tb = (b - if b >= finale { finale } else { groove }) % 8;
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let lift = if tb >= 4 && ((b >= bloom && b < bridge) || b >= finale) { 7 } else { 0 };
            for (i, &(_, on, dur, deg)) in theme.iter().enumerate().filter(|(_, t)| t.0 == tb) {
                let mut dd = deg;
                // with the variation, a note moves now and then as it grows
                if b >= bloom && vrng.chance(0.3 * v) {
                    dd += *vrng.pick(&[-2, -1, 1, 2]);
                }
                let long = dur >= 4;
                if long || on == 0 {
                    dd = self.nearest_tone(dd, &chord, false);
                }
                // the first phrase ends hanging, off the root
                let phrase_end = theme.get(i + 1).is_none_or(|t| t.0 / 4 != tb / 4);
                if phrase_end && tb < 4 && (dd - chord.root).rem_euclid(7) == 0 {
                    dd += 2;
                }
                let key = self.scale.midi(dd + lift);
                let start = b * bar + on * e8;
                let vel = 60.0 + 30.0 * it + if on == 0 { 5.0 } else { 0.0 };
                // a bluesy slip up from a half step below, now and then
                if long && on > 0 && vrng.chance(0.2 + 0.2 * v) {
                    line.push(Note { start: start - s16 / 2, dur: s16 / 2, key: (key - 1).clamp(21, 108) as u8, vel: (vel * 0.6) as u8, inst: Inst::Violin });
                }
                let len = if phrase_end { dur * e8 + beat } else { dur * e8 + e8 / 3 };
                line.push(Note { start, dur: len, key: key.clamp(21, 108) as u8, vel: vel.clamp(1.0, 127.0) as u8, inst: Inst::Violin });
            }
        }
        // played a little loosely, like a soloist
        let mut rubato = self.rng("rubato");
        let loose = (self.p.humanize as f64 / 0.4).min(2.5);
        for note in line.iter_mut() {
            note.start = (note.start as f64 + rubato.gauss() * loose * beat as f64 / 14.0).max(0.0) as u32;
        }
        self.notes.extend(line);

        // ---- the city: rain throughout, distant machinery and an analogue
        // drone in the opening, thunder far off, a reversed cymbal into the
        // bloom and the finale, and glassy shimmer as it fades
        self.note(Inst::Fx, 0, end, fx::RAIN as i32, 58.0);
        self.note(Inst::Fx, 0, (groove + 4).min(n) * bar, fx::HUM as i32, 46.0);
        self.note(Inst::Fx, 0, ticks(16.0), fx::ANALOG_DRONE as i32 + tonic_pc, 50.0);
        if bridge < finale {
            self.note(Inst::Fx, bridge * bar, ticks(16.0), fx::ANALOG_DRONE as i32 + tonic_pc, 46.0);
        }
        let mut frng = self.rng("glide-city");
        for (a, z) in [(1, groove.max(2)), (bridge, finale)] {
            if z > a {
                let b = frng.range(a as i32, z as i32 - 1) as u32;
                self.note(Inst::Fx, b * bar, ticks(9.0), fx::THUNDER as i32, 50.0);
            }
        }
        let rev = ticks(kit::REVERSE_LONG_SECS);
        for at in [bloom, finale] {
            if at > 0 && at < n && at * bar > rev {
                self.note(Inst::Fx, at * bar - rev, rev, fx::REVERSE_LONG as i32, 56.0);
            }
        }
        for b in (held..n).step_by(2) {
            self.note(Inst::Fx, b * bar, 2 * bar, fx::GRAINS as i32 + tonic_pc, 54.0);
        }
    }

    fn arrange(&mut self, secs_per_bar: f64) {
        // a score is arranged as it is written
        if self.p.version < 2 || self.p.style.is_score() {
            return;
        }
        let mut rng = self.rng("arrange");
        let v = self.p.variation as f64;
        // sections of about half a minute, in whole pairs of bars
        let sec_bars = (((30.0 / secs_per_bar) / 2.0).round() as u32 * 2).clamp(4, 16);
        let n = self.bars / sec_bars;
        if n < 4 {
            return;
        }
        let breakdown = ((n as f64 * rng.range_f(0.55, 0.7)) as u32).clamp(1, n - 2);
        for s in 1..n - 1 {
            let t = if s == breakdown {
                Treat::Breakdown
            } else if s == breakdown + 1 && rng.chance(0.6) {
                Treat::Lift
            } else if rng.chance(0.15 + 0.4 * v) {
                *rng.pick(&[Treat::LeadRests, Treat::Thin, Treat::Lift, Treat::Bare])
            } else {
                continue;
            };
            let a = s * sec_bars * self.bar;
            self.treat(t, a, a + sec_bars * self.bar, &mut rng);
        }
    }

    fn treat(&mut self, t: Treat, a: u32, b: u32, rng: &mut Rng) {
        let inside = |n: &Note| n.start >= a && n.start < b;
        let count = |notes: &[Note]| notes.iter().filter(|n| inside(n)).count();
        let before = count(&self.notes);
        if before == 0 {
            return;
        }
        let only_keys = self.notes.iter().filter(|n| inside(n)).all(|n| n.inst == Inst::Piano);
        let mut notes = self.notes.clone();
        match t {
            Treat::Breakdown => {
                // keep the downbeat, so the low end drops away after it
                let from = a + self.bar / 2;
                notes.retain(|n| {
                    !(inside(n)
                        && n.start >= from
                        && (matches!(n.inst, Inst::Cello | Inst::Pizzicato) || (n.inst == Inst::Piano && n.key < 55)))
                });
            }
            Treat::LeadRests => {
                if only_keys {
                    return;
                }
                notes.retain(|n| !(inside(n) && n.inst == Inst::Violin));
            }
            Treat::Lift => {
                let top = notes.iter().filter(|n| inside(n) && n.inst == Inst::Violin).map(|n| n.key).max();
                if top.is_none_or(|k| k + 12 > 93) {
                    return;
                }
                for n in notes.iter_mut().filter(|n| inside(n) && n.inst == Inst::Violin) {
                    n.key += 12;
                    n.vel = n.vel.saturating_sub(4).max(1);
                }
            }
            Treat::Bare => {
                if only_keys {
                    return;
                }
                notes.retain(|n| !(inside(n) && n.inst == Inst::StringPad));
            }
            Treat::Thin => {
                let bar = self.bar;
                notes.retain(|n| {
                    !(inside(n)
                        && matches!(n.inst, Inst::Piano | Inst::Pizzicato)
                        && n.start % bar != 0
                        && rng.chance(0.45))
                });
            }
        }
        // never leave a section nearly empty
        if count(&notes) * 4 >= before {
            self.notes = notes;
        }
    }

    // ---------------------------------------------------------------- finalise

    fn finish(mut self, bpm: f64, meter: (u32, u32)) -> Song {
        let end = self.bars * self.bar;
        let ring = self.bar;
        let tick_per_sec = bpm / 60.0 * self.ppq as f64;

        // Humanise timing and velocity.
        let mut rng = self.rng("humanize");
        let h = self.p.humanize as f64;
        for n in self.notes.iter_mut() {
            let jitter = rng.gauss() * h * 0.022 * tick_per_sec;
            let vj = rng.gauss() * h * 6.0;
            if n.start > 0 {
                n.start = (n.start as f64 + jitter).max(0.0) as u32;
            }
            n.vel = (n.vel as f64 + vj).round().clamp(1.0, 127.0) as u8;
        }

        // Nothing starts after the end; whatever is still sounding rings out.
        self.notes.retain(|n| n.start < end);
        for n in self.notes.iter_mut() {
            if n.end() + self.bar / 2 >= end {
                n.dur = end + ring - n.start;
            }
        }

        // Repeated notes on the same key/instrument must not overlap (a later
        // note-off would otherwise cut the newer note short).
        self.notes.sort_by_key(|n| (n.inst, n.key, n.start));
        for i in 1..self.notes.len() {
            let (a, b) = self.notes.split_at_mut(i);
            let prev = &mut a[i - 1];
            let cur = &b[0];
            if prev.inst == cur.inst && prev.key == cur.key && prev.end() > cur.start {
                prev.dur = cur.start.saturating_sub(prev.start).saturating_sub(10).max(1);
            }
        }
        self.notes.retain(|n| n.dur >= 20);
        self.notes.sort_by_key(|n| (n.start, n.inst, n.key));

        // Every part the arrangement uses; then drop the muted ones.
        let parts: Vec<Inst> = Inst::ALL
            .into_iter()
            .filter(|i| self.notes.iter().any(|n| n.inst == *i))
            .collect();
        let full_end = self.notes.iter().map(|n| n.end()).max().unwrap_or(0);
        let muted = &self.p.muted;
        self.notes.retain(|n| !muted.contains(&n.inst));

        // Initial controllers for each instrument used.
        let used: Vec<Inst> = Inst::ALL
            .into_iter()
            .filter(|i| self.notes.iter().any(|n| n.inst == *i))
            .collect();
        let mut init = Vec::new();
        let rev = (self.p.reverb.clamp(0.0, 1.0) * 127.0) as u8;
        let has_expr: HashMap<Inst, bool> = used
            .iter()
            .map(|i| (*i, self.ctrls.iter().any(|c| c.inst == *i && c.cc == 11 && c.tick == 0)))
            .collect();
        for i in &used {
            init.push(Ctrl { tick: 0, inst: *i, cc: 7, value: i.volume() });
            init.push(Ctrl { tick: 0, inst: *i, cc: 91, value: rev });
            init.push(Ctrl { tick: 0, inst: *i, cc: 93, value: 0 });
            if !has_expr[i] {
                init.push(Ctrl { tick: 0, inst: *i, cc: 11, value: 110 });
            }
        }
        self.ctrls.retain(|c| c.tick < end + ring && used.contains(&c.inst));
        self.ctrls.sort_by_key(|c| c.tick);
        init.extend(self.ctrls);

        let mut chords: Vec<(u32, String)> = Vec::new();
        for (b, c) in self.harmony.iter().enumerate() {
            let name = c.name(&self.scale);
            if chords.last().map(|l| l.1 != name).unwrap_or(true) {
                chords.push((b as u32 * self.bar, name));
            }
        }

        // Length is the same whatever is muted.
        let total_ticks = full_end.max(end + ring);
        Song {
            params: self.p.clone(),
            ppq: self.ppq,
            bpm,
            meter,
            bars: self.bars,
            notes: self.notes,
            ctrls: init,
            bends: Vec::new(),
            exprs: Vec::new(),
            off: Vec::new(),
            chords,
            parts,
            total_ticks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::Instrumentation;

    #[test]
    fn deterministic_for_same_seed() {
        for style in Style::ALL {
            let p = Params { style, ..Params::default() };
            let a = generate(&p);
            let b = generate(&p);
            assert_eq!(a.notes.len(), b.notes.len());
            assert!(a.notes.iter().zip(&b.notes).all(|(x, y)| x.start == y.start
                && x.key == y.key
                && x.vel == y.vel
                && x.dur == y.dur));
            assert!(!a.notes.is_empty(), "{style:?} produced no notes");
        }
    }

    #[test]
    fn different_seeds_differ() {
        let a = generate(&Params::default());
        let b = generate(&Params { seed: 7, ..Params::default() });
        assert!(a.notes.len() != b.notes.len() || a.notes.iter().zip(&b.notes).any(|(x, y)| x.key != y.key));
    }

    #[test]
    fn hits_target_length() {
        for style in Style::ALL {
            for secs in [45.0, 180.0, 600.0] {
                let p = Params { style, target_seconds: secs, ..Params::default() };
                let s = generate(&p);
                let d = s.duration_secs();
                let bar = s.tick_to_secs(s.bar_ticks());
                assert!((d - secs as f64).abs() <= bar * 1.5 + 1.0, "{style:?} {secs} -> {d}");
            }
        }
    }

    #[test]
    fn muting_keeps_other_parts_identical() {
        for style in Style::ALL {
            let p = Params { style, ..Params::default() };
            let full = generate(&p);
            for m in full.parts.clone() {
                let muted = generate(&Params { muted: vec![m], ..p.clone() });
                let expect: Vec<_> = full.notes.iter().filter(|n| n.inst != m).collect();
                assert_eq!(expect.len(), muted.notes.len(), "{style:?} muting {m:?}");
                assert!(expect.iter().zip(&muted.notes).all(|(a, b)| a.start == b.start && a.key == b.key));
                assert_eq!(full.total_ticks, muted.total_ticks);
                assert_eq!(full.parts, muted.parts);
            }
            let silent = generate(&Params { muted: Inst::ALL.to_vec(), ..p.clone() });
            assert!(silent.notes.is_empty());
        }
    }

    #[test]
    fn old_recipes_without_sounds_still_load() {
        let mut v: serde_json::Value = serde_json::from_str(&Params::default().to_recipe()).unwrap();
        v.as_object_mut().unwrap().remove("sounds");
        v.as_object_mut().unwrap().remove("muted");
        let p = Params::from_recipe(&v.to_string()).unwrap();
        assert_eq!(p, Params::default());
    }

    /// The trailer score keeps to its arc: a sparse intro of piano and
    /// effects, drums only from the build, the big hit at the peak, and a quiet
    /// close on a soft hit.
    #[test]
    fn the_trailer_has_its_arc() {
        use crate::kit::{drum, fx};
        for seed in 1..9u64 {
            let p = Params { style: Style::TechTrailer, seed, tempo: 80.0, ..Params::default() };
            let s = generate(&p);
            let bar = s.bar_ticks();
            let parts_in = |a: u32, b: u32| -> Vec<Inst> {
                let mut v: Vec<Inst> = s.notes.iter().filter(|n| n.start >= a * bar && n.start < b * bar).map(|n| n.inst).collect();
                v.sort();
                v.dedup();
                v
            };
            assert_eq!(parts_in(0, 8), vec![Inst::Piano, Inst::Fx], "seed {seed}: the intro is piano and effects");
            assert_eq!(s.parts.len(), Inst::ALL.len(), "seed {seed}: every part plays");
            // one big impact at the peak, and a soft one to end
            let impacts: Vec<&Note> = s.notes.iter().filter(|n| n.inst == Inst::Fx && n.key == fx::IMPACT).collect();
            assert_eq!(impacts.len(), 3, "seed {seed}");
            // the bar a note is played on, allowing for humanising
            let at = |n: &Note| (n.start + 40) / bar;
            let peak = at(impacts[0]);
            assert!(impacts[0].vel > impacts[2].vel);
            assert_eq!(at(impacts[2]), s.bars - 1);
            // the bar before the peak holds its breath, and the peak hits hard
            assert!(!s.notes.iter().any(|n| n.inst == Inst::Drums && at(n) == peak - 1), "seed {seed}");
            assert!(s.notes.iter().any(|n| n.inst == Inst::Fx && (fx::BRAAM..fx::BRAAM + 12).contains(&n.key) && at(n) == peak));
            assert!(s.notes.iter().any(|n| n.inst == Inst::Drums && n.key == drum::KICK && at(n) == peak));
            // the long riser ends on the hit
            let riser = s.notes.iter().find(|n| n.key == fx::RISER_LONG && n.inst == Inst::Fx).unwrap();
            assert!(riser.end().abs_diff(peak * bar) < bar / 8, "seed {seed}");
        }
    }

    /// Megacity Rain broods: rain from the first bar to the last, an opening
    /// of only the city and the lead, soft drums only in the body, and an
    /// ending left hanging on a suspended chord.
    #[test]
    fn the_rain_score_broods() {
        use crate::kit::{drum, fx};
        for seed in 1..9u64 {
            let p = Params { style: Style::MegacityRain, seed, tempo: 66.0, ..Params::default() };
            let s = generate(&p);
            let bar = s.bar_ticks();
            let rain = s.notes.iter().find(|n| n.inst == Inst::Fx && n.key == fx::RAIN).unwrap();
            assert!(rain.start == 0 && rain.end() >= s.bars * bar);
            let opening: Vec<Inst> = s.notes.iter().filter(|n| n.start < 8 * bar).map(|n| n.inst).collect();
            assert!(opening.iter().all(|i| matches!(i, Inst::Fx | Inst::Violin)), "seed {seed}");
            let drums: Vec<&Note> = s.notes.iter().filter(|n| n.inst == Inst::Drums).collect();
            assert!(!drums.is_empty());
            assert!(drums.iter().all(|n| n.start > s.bars * bar / 4 && n.start < s.bars * bar * 9 / 10));
            assert!(drums.iter().all(|n| n.vel < 100), "seed {seed}: the drums stay soft");
            assert!(drums.iter().any(|n| n.key == drum::GATED_SNARE));
            assert_eq!(s.chords.last().map(|c| c.1.ends_with("sus2")), Some(true));
        }
    }

    /// The seed picks among many organ figures and string ostinatos: their
    /// rhythms and contours in a bar of the build differ from seed to seed.
    #[test]
    fn the_stars_figures_vary() {
        let shape = |s: &Song, inst: Inst, b: u32| {
            let bar = s.bar_ticks();
            let notes: Vec<&Note> = s.notes.iter().filter(|n| n.inst == inst && n.start >= b * bar && n.start < (b + 1) * bar && n.dur < bar).collect();
            let first = notes.first().map(|n| n.key as i32).unwrap_or(0);
            notes.iter().map(|n| (n.start - b * bar, n.key as i32 - first)).collect::<Vec<_>>()
        };
        let (mut organ, mut strings) = (Vec::new(), Vec::new());
        for seed in 1..=16 {
            let p = Params { style: Style::StarCathedral, seed, tempo: 64.0, humanize: 0.0, target_seconds: 200.0, ..Params::default() };
            let s = generate(&p);
            let peak = section_starts(&p, s.bars)[1];
            organ.push(shape(&s, Inst::Piano, 12));
            strings.push(shape(&s, Inst::Cello, peak - 2));
        }
        for v in [&mut organ, &mut strings] {
            v.sort();
            v.dedup();
        }
        assert!(organ.len() >= 8, "{} organ figures", organ.len());
        assert!(strings.len() >= 6, "{} ostinatos", strings.len());
    }

    #[test]
    fn the_stars_rise_and_fall() {
        use crate::kit::{drum, fx};
        for seed in 1..9u64 {
            let p = Params { style: Style::StarCathedral, seed, tempo: 64.0, ..Params::default() };
            let s = generate(&p);
            let bar = s.bar_ticks();
            let end = s.bars * bar;
            // the organ alone over the drone
            let opening: Vec<Inst> = s.notes.iter().filter(|n| n.start < 4 * bar).map(|n| n.inst).collect();
            assert!(opening.iter().all(|i| matches!(i, Inst::Piano | Inst::Fx)), "seed {seed}");
            // no drums
            assert!(s.notes.iter().all(|n| n.inst != Inst::Drums), "seed {seed}");
            // the climax: everything else, loud, with timpani
            let mid = |n: &&Note| n.start > end * 2 / 3 && n.start < end * 3 / 4;
            for inst in Inst::ALL.into_iter().filter(|i| *i != Inst::Drums) {
                assert!(s.notes.iter().filter(mid).any(|n| n.inst == inst), "seed {seed}: no {inst:?} at the climax");
            }
            assert!(s.notes.iter().filter(mid).any(|n| n.inst == Inst::Fx && (fx::TIMPANI..fx::TIMPANI + 12).contains(&n.key)));
            let loud = |a: u32, b: u32| {
                let v: Vec<f64> = s.notes.iter().filter(|n| n.inst == Inst::Piano && n.start >= a && n.start < b).map(|n| n.vel as f64).collect();
                v.iter().sum::<f64>() / v.len().max(1) as f64
            };
            assert!(loud(end * 2 / 3, end * 3 / 4) > loud(0, end / 8) + 20.0, "seed {seed}");
            // the outro: the organ figure, nothing else begins
            let outro: Vec<&Note> = s.notes.iter().filter(|n| n.start >= end * 9 / 10).collect();
            assert!(outro.iter().all(|n| matches!(n.inst, Inst::Piano | Inst::Fx)), "seed {seed}");
        }
        // recipes from version 3 keep their clock and drums
        let old = generate(&Params { style: Style::StarCathedral, seed: 1, tempo: 64.0, version: 3, ..Params::default() });
        assert!(old.notes.iter().any(|n| n.inst == Inst::Drums && n.key == drum::TICK));
    }

    /// Neon Glide: the opening is pad, kalimba and the city; the groove is
    /// soft, with tabla in threes against the beat; the bridge drops to the
    /// keys, pad and hand percussion; the lead leaves room between phrases;
    /// and it ends on a held chord with no drums or groove.
    #[test]
    fn the_glide_sways() {
        use crate::kit::drum;
        for seed in 1..9u64 {
            let p = Params { style: Style::NeonGlide, seed, tempo: 94.0, humanize: 0.0, ..Params::default() };
            let s = generate(&p);
            let bar = s.bar_ticks();
            let end = s.bars * bar;
            let sec = auto_sections(Style::NeonGlide, s.bars);
            let starts: Vec<u32> = sec.iter().scan(0, |a, l| { *a += l; Some(*a - l) }).collect();
            let (groove, bridge, finale) = (starts[1], starts[3], starts[4]);
            let during = |a: u32, b: u32| s.notes.iter().filter(move |n| n.start >= a * bar && n.start < b * bar);
            assert!(during(0, groove).all(|n| matches!(n.inst, Inst::StringPad | Inst::Pizzicato | Inst::Fx)), "seed {seed}: opening");
            assert!(s.notes.iter().filter(|n| n.inst == Inst::Drums).all(|n| n.vel < 100), "seed {seed}: the drums stay soft");
            assert!(during(groove, bridge).any(|n| n.inst == Inst::Drums && matches!(n.key, drum::TABLA_HIGH | drum::TABLA_LOW)));
            // the bridge: keys, pad, kalimba and hand percussion only
            for n in during(bridge, finale) {
                assert!(matches!(n.inst, Inst::Piano | Inst::StringPad | Inst::Pizzicato | Inst::Drums | Inst::Fx), "seed {seed}: {:?} in the bridge", n.inst);
                assert!(n.inst != Inst::Drums || !matches!(n.key, drum::DEEP_KICK | drum::GATED_SNARE), "seed {seed}");
            }
            // the lead rests a bar in every four of its theme
            let lead_bars: Vec<u32> = s.notes.iter().filter(|n| n.inst == Inst::Violin).map(|n| n.start / bar).collect();
            assert!(!lead_bars.is_empty(), "seed {seed}");
            assert!(lead_bars.iter().all(|b| (b - if *b >= finale { finale } else { groove }) % 4 != 3), "seed {seed}");
            // the end: a chord held as it fades, with no drums or bass figure
            let last = during(s.bars - 1, s.bars + 1).collect::<Vec<_>>();
            assert!(last.iter().all(|n| !matches!(n.inst, Inst::Drums | Inst::Violin)), "seed {seed}");
            assert!(s.notes.iter().any(|n| n.inst == Inst::StringPad && n.end() >= end), "seed {seed}");
        }
    }

    #[test]
    fn scores_take_any_sections() {
        let mut rng = Rng::stream(7, "sections");
        for style in Style::ALL.into_iter().filter(|s| s.is_score()) {
            let k = style.score_sections().len();
            assert_eq!(auto_sections(style, 47).iter().sum::<u32>(), 47);
            for seed in 0..40u64 {
                let sections: Vec<u32> = (0..k).map(|_| rng.range(1, if seed < 20 { 4 } else { 24 }) as u32).collect();
                let p = Params { style, seed, tempo: style.default_tempo(), sections: sections.clone(), ..Params::default() };
                let s = generate(&p);
                let total = sections.iter().sum::<u32>().max(4);
                assert_eq!(s.bars, total, "{style:?} {sections:?}");
                assert!(!s.notes.is_empty());
                assert!(s.notes.iter().all(|n| n.start < total * s.bar_ticks()), "{style:?} {sections:?}");
            }
        }
    }

    #[test]
    fn the_character_shapes_a_score() {
        let spread = |s: &Song| {
            let v: Vec<f64> = s.notes.iter().map(|n| n.vel as f64).collect();
            let mean = v.iter().sum::<f64>() / v.len() as f64;
            (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
        };
        for style in Style::ALL.into_iter().filter(|s| s.is_score()) {
            let mut harmony_moved = false;
            for seed in 1..5u64 {
                let p = Params { style, seed, tempo: style.default_tempo(), humanize: 0.0, ..Params::default() };
                let at = |f: &dyn Fn(&mut Params)| {
                    let mut q = p.clone();
                    f(&mut q);
                    generate(&q)
                };
                let (sparse, busy) = (at(&|q| q.density = 0.1), at(&|q| q.density = 0.9));
                assert!(busy.notes.len() as f64 > 1.2 * sparse.notes.len() as f64, "{style:?} {seed}: density");
                let (even, dramatic) = (at(&|q| q.dynamics = 0.0), at(&|q| q.dynamics = 1.0));
                assert!(spread(&dramatic) > 1.4 * spread(&even), "{style:?} {seed}: dynamics {:.1} {:.1}", spread(&even), spread(&dramatic));
                let (plain, varied) = (at(&|q| q.variation = 0.0), at(&|q| q.variation = 1.0));
                harmony_moved |= plain.chords != varied.chords;
                let key = |s: &Song| s.notes.iter().map(|n| (n.start, n.key, n.vel)).collect::<Vec<_>>();
                assert!(key(&plain) != key(&varied), "{style:?} {seed}: variation");
                let loose = at(&|q| q.humanize = 1.0);
                assert!(loose.notes.iter().zip(&generate(&p).notes).any(|(a, b)| a.start != b.start), "{style:?}: humanize");
            }
            assert!(harmony_moved, "{style:?}: variation never changes the chords");
        }
    }

    #[test]
    fn all_instrumentations_work() {
        for style in Style::ALL {
            for inst in [Instrumentation::Piano, Instrumentation::Strings, Instrumentation::PianoAndStrings] {
                let p = Params { style, instrumentation: inst, ..Params::default() };
                let s = generate(&p);
                assert!(!s.notes.is_empty());
                // a score always plays its whole orchestra
                if style.is_score() {
                    continue;
                }
                if inst == Instrumentation::Piano {
                    assert!(s.notes.iter().all(|n| n.inst == Inst::Piano), "{style:?}");
                }
                if inst == Instrumentation::Strings {
                    assert!(s.notes.iter().all(|n| n.inst != Inst::Piano), "{style:?}");
                }
            }
        }
    }

    #[test]
    #[ignore]
    fn clash_report() {
        for style in Style::ALL {
            let mut clashes = 0usize;
            let mut pairs = 0usize;
            let mut keys = (127u8, 0u8);
            let mut notes = 0;
            for seed in 1..6u64 {
                for inst in [Instrumentation::Piano, Instrumentation::Strings, Instrumentation::PianoAndStrings] {
                    let mut s = generate(&Params { style, seed, instrumentation: inst, ..Params::default() });
                    // the drums and effects keys aren't pitches
                    s.notes.retain(|n| !matches!(n.inst, Inst::Drums | Inst::Fx));
                    notes += s.notes.len();
                    for n in &s.notes {
                        keys = (keys.0.min(n.key), keys.1.max(n.key));
                    }
                    // at every beat, which notes are sounding
                    for t in (0..s.total_ticks).step_by(s.ppq as usize) {
                        let on: Vec<u8> = s.notes.iter().filter(|n| n.start <= t && n.end() > t + 10).map(|n| n.key).collect();
                        for i in 0..on.len() {
                            for j in i + 1..on.len() {
                                pairs += 1;
                                let ic = (on[i] as i32 - on[j] as i32).rem_euclid(12);
                                if ic == 1 || ic == 11 || ic == 6 {
                                    clashes += 1;
                                }
                            }
                        }
                    }
                }
            }
            println!("{:<20} clashes {:>5.1}%  notes/run {:>5}  range {}-{}", style.label(), 100.0 * clashes as f64 / pairs.max(1) as f64, notes / 15, keys.0, keys.1);
        }
    }
}
