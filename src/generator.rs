//! The music generator. `generate(&params)` is a pure function: identical params
//! (including the seed) always produce an identical song.

use std::collections::HashMap;

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

pub fn generate(p: &Params) -> Song {
    let meter = p.style.meter();
    let bar = PPQ * 4 * meter.0 / meter.1;
    let bpm = p.tempo.clamp(30.0, 200.0) as f64;
    let secs_per_bar = bar as f64 / PPQ as f64 * 60.0 / bpm;
    let bars = match p.length_mode {
        // one extra bar of ring-out is appended after the last bar
        LengthMode::Seconds => ((p.target_seconds as f64 / secs_per_bar).round() as i64 - 1).max(4) as u32,
        LengthMode::Bars => p.target_bars.max(4),
    };

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
    }

    ctx.finish(bpm, meter)
}

impl<'a> Ctx<'a> {
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
        let exts = [Ext::Triad, Ext::Add9, Ext::Seventh, Ext::Ninth, Ext::Sus2, Ext::Sus4];

        let fix = |s: &Scale, mut d: i32| {
            let mut guard = 0;
            while s.is_diminished(d) && guard < 7 {
                d = (d + 5).rem_euclid(7);
                guard += 1;
            }
            d
        };

        // Three section themes.
        let mut themes: Vec<Vec<Chord>> = Vec::new();
        for _ in 0..3 {
            let prog = *rng.pick(&pool);
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
        let form: &[u8] = if v > 0.6 { &[0, 0, 1, 0, 2, 1, 0, 0] } else { &[0, 0, 1, 0] };

        let slots = self.bars.div_ceil(hr) as usize;
        let mut slot_chords = Vec::with_capacity(slots);
        let mut slot_section = Vec::with_capacity(slots);
        for s in 0..slots {
            let sec = form[(s / 4) % form.len()];
            let mut c = themes[sec as usize][s % 4].clone();
            if s % 4 != 0 && rng.chance(v * 0.3) {
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
        for b in (0..self.bars).step_by(hr as usize) {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let span = hr.min(self.bars - b) * self.bar;
            let start = b * self.bar;
            let voicing = voice_lead(&chord.pitch_classes(&self.scale), &prev, 50, 76, rng.chance(0.5));
            prev = voicing.clone();
            self.pad(start, span + overlap, &voicing, 45.0 + 40.0 * it);
            if self.strings {
                self.swell(Inst::StringPad, start, span, 45.0 + 25.0 * it, 75.0 + 45.0 * it, 55.0 + 25.0 * it);
                if it > 0.3 {
                    let k = self.bass_key(&chord, 36);
                    self.note(Inst::Cello, start, span + overlap, k, 40.0 + 35.0 * it);
                }
            } else {
                let k = self.bass_key(&chord, 36);
                self.note(Inst::Piano, start, span, k, 38.0 + 15.0 * it);
            }
        }

        // High, sparse sparkles.
        let mut rng = self.rng("sparkles");
        let d = self.p.density as f64;
        let e = self.ppq / 2;
        for b in 0..self.bars {
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
                    self.note(Inst::Piano, pos, dur, key, 26.0 + 32.0 * it + rng.gauss() * 4.0);
                } else {
                    let dur = rng.range(4, 8) as u32 * self.ppq;
                    self.note(Inst::Violin, pos, dur, key, 22.0 + 25.0 * it);
                }
            }
        }

        // A slow violin line at the high point of the piece.
        if self.strings && d > 0.3 {
            let insts: Vec<Option<Inst>> =
                (0..self.bars).map(|b| (self.it(b) > 0.7).then_some(Inst::Violin)).collect();
            let mut rng = self.rng("melody");
            let cfg = MelodyCfg {
                lo: 7,
                hi: 16,
                patterns: PAT_SLOW_44,
                phrase_bars: 4,
                vel: 52.0,
                rest_bar_prob: 0.2,
                ornaments: false,
            };
            let m = self.melody(&mut rng, &cfg, &insts);
            self.notes.extend(m);
        }
    }

    fn gymnopedie(&mut self) {
        self.plan_harmony(1, [0.2, 0.2, 0.6, 0.0, 0.0, 0.0], &[&[0, 3, 0, 3], &[3, 0, 3, 0]]);
        let mut rng = self.rng("accomp");
        let mut prev = Vec::new();
        let beat = self.ppq;
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
            if self.piano {
                self.note(Inst::Piano, start, 3 * beat, bass, 50.0 + 15.0 * it + swing * 0.3);
                for k in &voicing {
                    self.note(Inst::Piano, start + beat, 2 * beat, *k, 34.0 + 14.0 * it);
                }
                if self.strings && it > 0.45 {
                    for k in &voicing {
                        self.note(Inst::StringPad, start, self.bar + beat / 4, *k, 25.0 + 40.0 * it);
                    }
                    if it > 0.6 {
                        self.note(Inst::Cello, start, self.bar, bass, 35.0 + 30.0 * it);
                    }
                }
            } else {
                self.note(Inst::Cello, start, 3 * beat, bass, 50.0 + 20.0 * it);
                for k in &voicing {
                    self.note(Inst::StringPad, start + beat, 2 * beat, *k, 38.0 + 22.0 * it);
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
        let cfg = MelodyCfg {
            lo: 7,
            hi: 16,
            patterns: PAT_WALTZ,
            phrase_bars: 4,
            vel: 50.0,
            rest_bar_prob: 0.05 + 0.25 * (1.0 - self.p.density as f64),
            ornaments: true,
        };
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
        for vi in 0..voices {
            let inst = match (self.piano, self.strings) {
                (true, true) => {
                    if vi % 2 == 0 { Inst::Piano } else { Inst::StringPad }
                }
                (true, false) => Inst::Piano,
                _ => [Inst::StringPad, Inst::Violin, Inst::Cello][vi % 3],
            };
            let loop_len = (rng.range_f(14.0, 32.0) * 4.0).round() / 4.0;
            let k = rng.range(1, 3);
            let mut phrase: Vec<(f64, i32, f64)> = (0..k)
                .map(|_| {
                    let off = (rng.range_f(0.0, loop_len * 0.6) * 2.0).round() / 2.0;
                    let mut deg = *rng.pick(&set);
                    if inst == Inst::Cello {
                        deg -= 7;
                    }
                    let dur = if inst == Inst::Piano { rng.range_f(3.0, 6.0) } else { rng.range_f(4.0, 8.0) };
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
                    if rng.chance(0.35 * (1.0 - it)) {
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
        // Drone
        let seg = 8 * self.bar;
        let tonic = self.bass_key(&Chord::new(0, Ext::Triad), 36);
        let mut b = 0;
        while b < self.bars {
            let start = b * self.bar;
            let span = seg.min((self.bars - b) * self.bar);
            let it = self.it(b);
            if self.strings {
                self.note(Inst::Cello, start, span + self.ppq, tonic, 30.0 + 25.0 * it);
                self.note(Inst::Cello, start, span + self.ppq, tonic + 7, 24.0 + 20.0 * it);
                self.swell(Inst::Cello, start, span, 60.0, 90.0 + 20.0 * it, 60.0);
            } else {
                self.note(Inst::Piano, start, span, tonic, 30.0 + 12.0 * it);
                self.note(Inst::Piano, start + self.ppq / 6, span, tonic + 7, 26.0 + 10.0 * it);
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
        let sec_pat = [rng.range(0, 3) as usize, rng.range(0, 3) as usize, rng.range(0, 3) as usize];
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
            let pat = PATS[sec_pat[self.section[b as usize] as usize]];
            let fast = it > 0.75 && self.p.density > 0.5;
            let (step, count) = if fast { (e / 2, 16) } else { (e, 8) };
            for i in 0..count {
                let k = keys[pat[i % 8]] + if fast && i >= 8 { 12 } else { 0 };
                let accent = if i == 0 { 10.0 } else if i % 4 == 0 { 4.0 } else { 0.0 };
                let dur = if arp_inst == Inst::Piano { step * 3 } else { step };
                self.note(arp_inst, start + i as u32 * step, dur, k, 40.0 + 20.0 * it + accent);
            }
            if self.strings && self.piano {
                if it > 0.35 {
                    let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 55, 76, true);
                    prev = v.clone();
                    for k in &v {
                        self.note(Inst::StringPad, start, self.bar + e / 2, *k, 30.0 + 45.0 * it);
                    }
                    self.swell(Inst::StringPad, start, self.bar, 60.0, 80.0 + 30.0 * it, 65.0);
                }
                if it > 0.2 {
                    self.note(Inst::Cello, start, self.bar + e / 2, keys[0], 40.0 + 35.0 * it);
                }
            } else if self.strings {
                self.note(Inst::Cello, start, self.bar + e / 2, keys[0], 45.0 + 30.0 * it);
                let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 55, 76, true);
                prev = v.clone();
                for k in &v {
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
        let cfg = MelodyCfg {
            lo: 7,
            hi: 15,
            patterns: PAT_MOVING_44,
            phrase_bars: 4,
            vel: 52.0,
            rest_bar_prob: 0.25 * (1.0 - self.p.density as f64),
            ornaments: false,
        };
        let m = self.melody(&mut rng, &cfg, &insts);
        self.notes.extend(m);
    }

    fn rainfall(&mut self) {
        let hr = 2;
        self.plan_harmony(hr, [0.2, 0.4, 0.1, 0.0, 0.3, 0.0], &[]);
        let mut rng = self.rng("bed");
        let mut prev = Vec::new();
        for b in (0..self.bars).step_by(hr as usize) {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let span = hr.min(self.bars - b) * self.bar;
            let start = b * self.bar;
            let bass = self.bass_key(&chord, 36);
            if self.piano {
                self.note(Inst::Piano, start, span, bass, 32.0 + 12.0 * it);
            }
            if self.strings {
                let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 48, 72, rng.chance(0.5));
                prev = v.clone();
                for k in &v {
                    self.note(Inst::StringPad, start, span + self.ppq, *k, 26.0 + 30.0 * it);
                }
                self.swell(Inst::StringPad, start, span, 45.0, 70.0 + 30.0 * it, 50.0);
                if it > 0.5 || !self.piano {
                    self.note(Inst::Cello, start, span + self.ppq, bass, 30.0 + 30.0 * it);
                }
            }
        }
        let mut rng = self.rng("drops");
        let d = self.p.density as f64;
        let s16 = self.ppq / 4;
        let drop_inst = if self.piano { Inst::Piano } else { Inst::Pizzicato };
        for b in 0..self.bars {
            let it = self.it(b);
            let n = rng.poisson(0.6 + d * 4.5 * it);
            for _ in 0..n {
                let pos = b * self.bar + rng.range(0, (self.bar / s16) as i32 - 1) as u32 * s16;
                let mut deg = rng.range(5, 20);
                while !self.scale.is_pentatonic(deg) {
                    deg += 1;
                }
                let cascade = if rng.chance(0.15) { rng.range(2, 4) } else { 1 };
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
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let start = b * self.bar;
            let bass = self.bass_key(&chord, 36);
            let tail = self.ppq / 3;
            if self.strings {
                self.note(Inst::Cello, start, self.bar + tail, bass, 45.0 + 40.0 * it);
                self.swell(Inst::Cello, start, self.bar, 60.0 + 20.0 * it, 90.0 + 30.0 * it, 70.0);
                if it > 0.3 {
                    let v = voice_lead(&chord.pitch_classes(&self.scale), &prev, 50, 72, false);
                    prev = v.clone();
                    for k in &v {
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
            let (step, n) = if it < 0.4 { (2 * self.ppq, 2) } else { (self.ppq, 4) };
            if it > 0.2 || !self.strings {
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
            (0..self.bars).map(|b| (self.it(b) > 0.5).then_some(theme_inst)).collect();
        let mut rng = self.rng("melody");
        let cfg = MelodyCfg {
            lo: 9,
            hi: 17,
            patterns: PAT_SLOW_44,
            phrase_bars: 4,
            vel: 58.0,
            rest_bar_prob: 0.0,
            ornaments: false,
        };
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
        for b in 0..self.bars {
            let chord = self.harmony[b as usize].clone();
            let it = self.it(b);
            let start = b * self.bar;
            let low = self.scale.degree_near(self.bass_key(&chord, 36));
            let third = if matches!(chord.ext, Ext::Sus4) { 3 } else if matches!(chord.ext, Ext::Sus2) { 1 } else { 2 };
            let offs = [0, 4, 7 + third, 11, 7 + third, 4];
            for (i, o) in offs.iter().enumerate() {
                let key = self.scale.midi(low + o);
                let dur = if arp_inst == Inst::Piano { (6 - i as u32) * e + e } else { e };
                let vel = if i == 0 { 48.0 } else { 36.0 } + 15.0 * it;
                self.note(arp_inst, start + i as u32 * e, dur, key, vel);
            }
            if self.strings {
                if !self.piano || it > 0.65 {
                    self.note(Inst::Cello, start, self.bar + e / 2, self.scale.midi(low), 38.0 + 30.0 * it);
                }
                if it > 0.5 || !self.piano {
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
        let cfg = MelodyCfg {
            lo: 7,
            hi: 17,
            patterns: PAT_68,
            phrase_bars: 4,
            vel: 54.0,
            rest_bar_prob: 0.15 * (1.0 - self.p.density as f64),
            ornaments: true,
        };
        let m = self.melody(&mut rng, &cfg, &insts);
        self.notes.extend(m);
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

    #[test]
    fn all_instrumentations_work() {
        for style in Style::ALL {
            for inst in [Instrumentation::Piano, Instrumentation::Strings, Instrumentation::PianoAndStrings] {
                let p = Params { style, instrumentation: inst, ..Params::default() };
                let s = generate(&p);
                assert!(!s.notes.is_empty());
                if inst == Instrumentation::Piano {
                    assert!(s.notes.iter().all(|n| n.inst == Inst::Piano), "{style:?}");
                }
                if inst == Instrumentation::Strings {
                    assert!(s.notes.iter().all(|n| n.inst != Inst::Piano), "{style:?}");
                }
            }
        }
    }
}
