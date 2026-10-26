//! Audio engines. `BuiltinSynth` is a small self-contained synthesiser (additive
//! piano, bowed strings, plucks and a Freeverb-style reverb) so playback works out
//! of the box. `SoundFontSynth` wraps rustysynth for any General MIDI .sf2 file.

use std::f32::consts::{PI, TAU};
use std::sync::{Arc, OnceLock};

pub trait SynthEngine: Send {
    fn note_on(&mut self, ch: u8, key: u8, vel: u8);
    fn note_off(&mut self, ch: u8, key: u8);
    fn control(&mut self, ch: u8, cc: u8, value: u8);
    fn program(&mut self, ch: u8, program: u8);
    /// Silence everything and reset controllers.
    fn reset(&mut self);
    fn render(&mut self, left: &mut [f32], right: &mut [f32]);
}

// ------------------------------------------------------------------ helpers

const TABLE: usize = 4096;

fn sine_table() -> &'static [f32] {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| (0..=TABLE).map(|i| (i as f32 / TABLE as f32 * TAU).sin()).collect())
}

#[inline]
fn fast_sin(table: &[f32], phase: f32) -> f32 {
    let x = phase * TABLE as f32;
    let i = x as usize;
    let f = x - i as f32;
    let i = i & (TABLE - 1);
    table[i] + (table[i + 1] - table[i]) * f
}

fn midi_hz(key: f32) -> f32 {
    440.0 * 2f32.powf((key - 69.0) / 12.0)
}

/// Multiplier per sample that gives a 60 dB decay over `t60` seconds.
fn decay_coef(t60: f32, sr: f32) -> f32 {
    (0.001f32.ln() / (t60 * sr)).exp()
}

#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// Cheap deterministic noise for per-voice variation.
fn hash01(a: u32) -> f32 {
    let mut x = a.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 13;
    x = x.wrapping_mul(0xC2B2_AE35);
    x ^= x >> 16;
    (x & 0xFFFF) as f32 / 65535.0
}

// ------------------------------------------------------- struck / plucked

#[derive(Clone, Copy)]
enum Tone {
    Piano,
    EPiano,
    /// Mallet/bell instruments: (frequency ratio, amplitude) partials and a decay time.
    Bell { t60: f32, ratios: &'static [(f32, f32)] },
    /// Plucked strings: decay scale and brightness (lower = brighter).
    Pluck { t60: f32, dark: f32 },
}

const CELESTA: Tone = Tone::Bell { t60: 2.2, ratios: &[(1.0, 1.0), (2.0, 0.25), (3.0, 0.08), (4.1, 0.04)] };
const GLOCK: Tone = Tone::Bell { t60: 3.5, ratios: &[(1.0, 1.0), (2.76, 0.4), (5.4, 0.2), (8.93, 0.08)] };
const MUSIC_BOX: Tone = Tone::Bell { t60: 2.6, ratios: &[(1.0, 1.0), (3.0, 0.22), (5.8, 0.1)] };
const VIBES: Tone = Tone::Bell { t60: 4.5, ratios: &[(1.0, 1.0), (4.0, 0.3), (10.0, 0.05)] };
const MARIMBA: Tone = Tone::Bell { t60: 0.9, ratios: &[(1.0, 1.0), (3.93, 0.3), (9.2, 0.07)] };
const KALIMBA: Tone = Tone::Bell { t60: 1.3, ratios: &[(1.0, 1.0), (5.9, 0.22), (13.1, 0.04)] };

struct Partial {
    phase: f32,
    inc: f32,
    amp: f32,
    decay: f32,
}

struct Struck {
    partials: Vec<Partial>,
    attack: f32,
    attack_inc: f32,
    release: f32,
    released: bool,
    release_coef: f32,
    lp: f32,
    lp_coef: f32,
}

impl Struck {
    fn new(sr: f32, key: u8, vel: f32, tone: Tone, seed: u32) -> Self {
        let f0 = midi_hz(key as f32);
        let k = key as f32;
        // (frequency, amplitude, t60) per partial
        let mut spec: Vec<(f32, f32, f32)> = Vec::new();
        let (attack_time, release_time, cutoff_mult) = match tone {
            Tone::Piano => {
                let t60 = 20.0 * 0.5f32.powf((k - 21.0) / 24.0);
                let inharm = 0.00025 * 2f32.powf((k - 60.0) / 24.0);
                let bright = 2.3 - 1.3 * vel;
                for n in 1..=14 {
                    let nf = n as f32;
                    let f = f0 * nf * (1.0 + inharm * nf * nf).sqrt();
                    // hammer strike position ~1/7 of the string length shapes the spectrum
                    let strike = (PI * nf * 0.13).sin().abs() * 0.85 + 0.15;
                    let amp = strike / nf.powf(bright);
                    let t = t60 / (1.0 + 0.45 * (nf - 1.0));
                    spec.push((f, amp, t));
                    // second, slightly detuned string for the lowest partials gives the piano "bloom"
                    if n <= 3 {
                        let cents = 0.6 + 0.6 * hash01(seed * 7 + n);
                        spec.push((f * 2f32.powf(cents / 1200.0), amp * 0.6, t * 1.3));
                    }
                }
                (0.004 - 0.002 * vel, 0.9, 4.0 + 10.0 * vel)
            }
            Tone::EPiano => {
                let t60 = 9.0 * 0.5f32.powf((k - 21.0) / 30.0);
                spec.push((f0, 1.0, t60));
                spec.push((f0 * 2.0, 0.28 * (0.4 + vel), t60 * 0.6));
                spec.push((f0 * 3.0, 0.08 * vel, t60 * 0.4));
                spec.push((f0 * 4.0, 0.03, t60 * 0.3));
                spec.push((f0 * 7.1, 0.25 * vel, 0.12)); // tine "ping"
                (0.002, 0.35, 12.0)
            }
            Tone::Bell { t60, ratios } => {
                let t60 = t60 * 2f32.powf(-(k - 72.0) / 36.0);
                for (i, (r, a)) in ratios.iter().enumerate() {
                    let a = if i == 0 { *a } else { a * (0.5 + 0.8 * vel) };
                    spec.push((f0 * r, a, t60 / (1.0 + 0.6 * i as f32)));
                }
                (0.0015, 0.5, 20.0)
            }
            Tone::Pluck { t60, dark } => {
                let t60 = t60 * (0.4 + 0.6 * (1.0 - (k - 36.0) / 60.0).clamp(0.0, 1.0));
                let bright = dark - 0.8 * vel;
                for n in 1..=12 {
                    let nf = n as f32;
                    spec.push((f0 * nf, 1.0 / nf.powf(bright), t60 / (1.0 + 0.5 * (nf - 1.0))));
                }
                (0.002, 0.25, 4.0 + 10.0 * vel)
            }
        };
        let mut partials: Vec<Partial> = spec
            .iter()
            .enumerate()
            .filter(|(_, (f, _, _))| *f < sr * 0.42)
            .map(|(i, (f, a, t))| Partial {
                phase: hash01(seed + i as u32 * 13),
                inc: f / sr,
                amp: *a,
                decay: decay_coef(*t, sr),
            })
            .collect();
        let norm: f32 = partials.iter().map(|p| p.amp).sum::<f32>().max(1e-6);
        for p in partials.iter_mut() {
            p.amp /= norm;
        }
        let cutoff = (f0 * cutoff_mult).min(sr * 0.45);
        Struck {
            partials,
            attack: 0.0,
            attack_inc: 1.0 / (attack_time * sr),
            release: 1.0,
            released: false,
            release_coef: decay_coef(release_time, sr),
            lp: 0.0,
            lp_coef: 1.0 - (-TAU * cutoff / sr).exp(),
        }
    }

    #[inline]
    fn next(&mut self, table: &[f32]) -> f32 {
        let mut s = 0.0;
        for p in self.partials.iter_mut() {
            s += fast_sin(table, p.phase) * p.amp;
            p.phase += p.inc;
            if p.phase >= 1.0 {
                p.phase -= 1.0;
            }
            p.amp *= p.decay;
        }
        if self.attack < 1.0 {
            self.attack = (self.attack + self.attack_inc).min(1.0);
        }
        if self.released {
            self.release *= self.release_coef;
        }
        self.lp += (s - self.lp) * self.lp_coef;
        self.lp * self.attack * self.release
    }

    fn level(&self) -> f32 {
        self.partials.iter().map(|p| p.amp).sum::<f32>() * self.release
    }
}

// ------------------------------------------------------------------- bowed

/// Detuned band-limited saws through a low-pass: strings, choirs and pads.
#[derive(Clone, Copy)]
struct BowCfg {
    detunes: &'static [f32],
    attack: f32,
    release: f32,
    vibrato: f32,
    bright: f32,
}

const ENSEMBLE: BowCfg = BowCfg { detunes: &[-7.0, 0.5, 6.5], attack: 1.1, release: 1.8, vibrato: 7.0, bright: 1.0 };
const SYNTH_STRINGS: BowCfg = BowCfg { detunes: &[-12.0, 0.0, 11.0], attack: 0.6, release: 1.5, vibrato: 0.0, bright: 1.4 };
const VIOLIN: BowCfg = BowCfg { detunes: &[0.0], attack: 0.35, release: 0.7, vibrato: 14.0, bright: 1.0 };
const VIOLA: BowCfg = BowCfg { detunes: &[0.0], attack: 0.38, release: 0.8, vibrato: 12.0, bright: 0.85 };
const CELLO: BowCfg = BowCfg { detunes: &[-2.0, 2.5], attack: 0.45, release: 1.0, vibrato: 9.0, bright: 1.0 };
const CONTRABASS: BowCfg = BowCfg { detunes: &[-2.0, 2.5], attack: 0.5, release: 1.0, vibrato: 6.0, bright: 0.7 };
const CHOIR: BowCfg = BowCfg { detunes: &[-9.0, 0.0, 8.0], attack: 0.8, release: 1.5, vibrato: 5.0, bright: 0.5 };
const PAD: BowCfg = BowCfg { detunes: &[-10.0, 0.0, 10.0], attack: 1.6, release: 2.8, vibrato: 0.0, bright: 0.6 };
const HALO: BowCfg = BowCfg { detunes: &[-14.0, 0.0, 13.0], attack: 1.4, release: 3.0, vibrato: 3.0, bright: 0.8 };

struct Bowed {
    phases: [f32; 3],
    incs: [f32; 3],
    n: usize,
    vib_phase: f32,
    vib_inc: f32,
    vib_depth: f32,
    vib_onset: f32,
    vib_onset_inc: f32,
    env: f32,
    attack_coef: f32,
    release_coef: f32,
    released: bool,
    // TPT state-variable low-pass
    ic1: f32,
    ic2: f32,
    g: f32,
    k: f32,
    lp2: f32,
    lp2_coef: f32,
}

impl Bowed {
    fn new(sr: f32, key: u8, vel: f32, cfg: BowCfg, seed: u32) -> Self {
        let f0 = midi_hz(key as f32);
        let mut phases = [0.0; 3];
        let mut incs = [0.0; 3];
        for (i, d) in cfg.detunes.iter().take(3).enumerate() {
            phases[i] = hash01(seed + i as u32 * 17);
            incs[i] = f0 * 2f32.powf(d / 1200.0) / sr;
        }
        // softer notes bow in more slowly
        let attack = cfg.attack * (1.0 - 0.45 * vel);
        let cutoff = ((f0 * (2.5 + 3.5 * vel) + 500.0 + 900.0 * vel) * cfg.bright).min(sr * 0.42);
        let g = (PI * cutoff / sr).tan();
        Bowed {
            phases,
            incs,
            n: cfg.detunes.len().min(3),
            vib_phase: hash01(seed + 99),
            vib_inc: (4.6 + 1.0 * hash01(seed + 5)) / sr,
            vib_depth: cfg.vibrato,
            vib_onset: 0.0,
            vib_onset_inc: 1.0 / (0.5 * sr),
            env: 0.0,
            attack_coef: 1.0 - (-1.0 / ((attack / 3.0).max(0.01) * sr)).exp(),
            release_coef: 1.0 - (-1.0 / ((cfg.release / 4.0) * sr)).exp(),
            released: false,
            ic1: 0.0,
            ic2: 0.0,
            g,
            k: 1.2,
            lp2: 0.0,
            lp2_coef: 1.0 - (-TAU * (cutoff * 1.6).min(sr * 0.45) / sr).exp(),
        }
    }

    #[inline]
    fn next(&mut self, table: &[f32]) -> f32 {
        let vib = fast_sin(table, self.vib_phase) * self.vib_depth * self.vib_onset;
        self.vib_phase += self.vib_inc;
        if self.vib_phase >= 1.0 {
            self.vib_phase -= 1.0;
        }
        if self.vib_onset < 1.0 {
            self.vib_onset += self.vib_onset_inc;
        }
        let ratio = 1.0 + vib * 0.000_577_8; // cents -> ratio (approx)
        let mut s = 0.0;
        for i in 0..self.n {
            let dt = self.incs[i] * ratio;
            let t = self.phases[i];
            s += 2.0 * t - 1.0 - poly_blep(t, dt);
            self.phases[i] += dt;
            if self.phases[i] >= 1.0 {
                self.phases[i] -= 1.0;
            }
        }
        s /= self.n as f32;
        // 12 dB SVF low-pass
        let a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        let a2 = self.g * a1;
        let a3 = self.g * a2;
        let v3 = s - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        self.lp2 += (v2 - self.lp2) * self.lp2_coef;
        let target = if self.released { 0.0 } else { 1.0 };
        let c = if self.released { self.release_coef } else { self.attack_coef };
        self.env += (target - self.env) * c;
        self.lp2 * self.env * 0.55
    }

    fn level(&self) -> f32 {
        if self.released { self.env } else { 1.0 }
    }
}

// -------------------------------------------------------------------- winds

/// Additive harmonic profile plus breath noise: flutes, reeds and horns.
#[derive(Clone, Copy)]
struct WindCfg {
    harmonics: &'static [f32],
    noise: f32,
    vibrato: f32,
    attack: f32,
    release: f32,
}

const FLUTE: WindCfg = WindCfg { harmonics: &[1.0, 0.22, 0.07, 0.03], noise: 0.05, vibrato: 8.0, attack: 0.12, release: 0.25 };
const CLARINET: WindCfg = WindCfg {
    harmonics: &[1.0, 0.04, 0.42, 0.03, 0.22, 0.02, 0.1, 0.02, 0.05],
    noise: 0.012,
    vibrato: 3.0,
    attack: 0.08,
    release: 0.2,
};
const OBOE: WindCfg = WindCfg {
    harmonics: &[0.55, 1.0, 0.75, 0.45, 0.3, 0.2, 0.12, 0.07, 0.04],
    noise: 0.01,
    vibrato: 9.0,
    attack: 0.07,
    release: 0.2,
};
const BASSOON: WindCfg = WindCfg {
    harmonics: &[0.8, 1.0, 0.55, 0.4, 0.25, 0.15, 0.08],
    noise: 0.01,
    vibrato: 5.0,
    attack: 0.09,
    release: 0.25,
};
const HORN: WindCfg = WindCfg { harmonics: &[1.0, 0.55, 0.3, 0.16, 0.08, 0.04], noise: 0.004, vibrato: 4.0, attack: 0.15, release: 0.35 };

struct Wind {
    phase: f32,
    inc: f32,
    amps: [f32; 12],
    n: usize,
    noise: f32,
    noise_state: u32,
    noise_lp: f32,
    noise_coef: f32,
    vib_phase: f32,
    vib_inc: f32,
    vib_depth: f32,
    vib_onset: f32,
    vib_onset_inc: f32,
    env: f32,
    attack_coef: f32,
    release_coef: f32,
    released: bool,
}

impl Wind {
    fn new(sr: f32, key: u8, vel: f32, cfg: WindCfg, seed: u32) -> Self {
        let f0 = midi_hz(key as f32);
        let mut amps = [0.0; 12];
        let mut n = 0;
        for (h, a) in cfg.harmonics.iter().enumerate().take(12) {
            if f0 * (h + 1) as f32 > sr * 0.42 {
                break;
            }
            // louder notes are brighter
            amps[h] = a * (0.45 + 0.55 * vel).powi(h as i32);
            n = h + 1;
        }
        let norm: f32 = amps.iter().sum::<f32>().max(1e-6);
        amps.iter_mut().for_each(|a| *a /= norm);
        Wind {
            phase: hash01(seed),
            inc: f0 / sr,
            amps,
            n,
            noise: cfg.noise,
            noise_state: seed | 1,
            noise_lp: 0.0,
            noise_coef: 1.0 - (-TAU * (f0 * 3.0).min(6000.0) / sr).exp(),
            vib_phase: hash01(seed + 3),
            vib_inc: (4.8 + 0.8 * hash01(seed + 9)) / sr,
            vib_depth: cfg.vibrato,
            vib_onset: 0.0,
            vib_onset_inc: 1.0 / (0.4 * sr),
            env: 0.0,
            attack_coef: 1.0 - (-1.0 / ((cfg.attack / 3.0) * sr)).exp(),
            release_coef: 1.0 - (-1.0 / ((cfg.release / 4.0) * sr)).exp(),
            released: false,
        }
    }

    #[inline]
    fn next(&mut self, table: &[f32]) -> f32 {
        let vib = fast_sin(table, self.vib_phase) * self.vib_depth * self.vib_onset;
        self.vib_phase += self.vib_inc;
        if self.vib_phase >= 1.0 {
            self.vib_phase -= 1.0;
        }
        if self.vib_onset < 1.0 {
            self.vib_onset += self.vib_onset_inc;
        }
        let mut s = 0.0;
        for h in 0..self.n {
            let ph = self.phase * (h + 1) as f32;
            s += fast_sin(table, ph - ph.floor()) * self.amps[h];
        }
        self.phase += self.inc * (1.0 + vib * 0.000_577_8);
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        // breath
        self.noise_state ^= self.noise_state << 13;
        self.noise_state ^= self.noise_state >> 17;
        self.noise_state ^= self.noise_state << 5;
        let white = (self.noise_state as f32 / u32::MAX as f32) * 2.0 - 1.0;
        self.noise_lp += (white - self.noise_lp) * self.noise_coef;
        let target = if self.released { 0.0 } else { 1.0 };
        let c = if self.released { self.release_coef } else { self.attack_coef };
        self.env += (target - self.env) * c;
        (s + self.noise_lp * self.noise * 4.0) * self.env
    }

    fn level(&self) -> f32 {
        if self.released { self.env } else { 1.0 }
    }
}

// ------------------------------------------------------------------ voices

enum VoiceKind {
    Struck(Struck),
    Bowed(Bowed),
    Wind(Wind),
}

struct Voice {
    ch: u8,
    key: u8,
    kind: VoiceKind,
    gain: f32,
    pan_l: f32,
    pan_r: f32,
    age: u64,
    released: bool,
}

impl Voice {
    fn level(&self) -> f32 {
        match &self.kind {
            VoiceKind::Struck(s) => s.level(),
            VoiceKind::Bowed(b) => b.level(),
            VoiceKind::Wind(w) => w.level(),
        }
    }
    fn release(&mut self) {
        self.released = true;
        match &mut self.kind {
            VoiceKind::Struck(s) => s.released = true,
            VoiceKind::Bowed(b) => b.released = true,
            VoiceKind::Wind(w) => w.released = true,
        }
    }
}

#[derive(Clone, Copy)]
struct Channel {
    program: u8,
    volume: f32,
    expr: f32,
    expr_target: f32,
    send: f32,
}

impl Default for Channel {
    fn default() -> Self {
        Channel { program: 0, volume: 100.0 / 127.0, expr: 1.0, expr_target: 1.0, send: 0.4 }
    }
}

// ------------------------------------------------------------------ reverb

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}
struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

struct Reverb {
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    feedback: f32,
    damp: f32,
    predelay: Vec<f32>,
    pd_idx: usize,
}

impl Reverb {
    fn new(sr: f32) -> Self {
        const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        const APS: [usize; 4] = [556, 441, 341, 225];
        let scale = sr / 44100.0 * 1.25; // a slightly larger room than stock Freeverb
        let mk = |spread: usize| {
            (
                COMBS
                    .iter()
                    .map(|l| Comb { buf: vec![0.0; ((l + spread) as f32 * scale) as usize], idx: 0, store: 0.0 })
                    .collect::<Vec<_>>(),
                APS.iter()
                    .map(|l| Allpass { buf: vec![0.0; ((l + spread) as f32 * scale) as usize], idx: 0 })
                    .collect::<Vec<_>>(),
            )
        };
        let (cl, al) = mk(0);
        let (cr, ar) = mk(23);
        Reverb {
            combs: [cl, cr],
            allpasses: [al, ar],
            feedback: 0.86,
            damp: 0.3,
            predelay: vec![0.0; (0.025 * sr) as usize],
            pd_idx: 0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> (f32, f32) {
        let x = self.predelay[self.pd_idx];
        self.predelay[self.pd_idx] = input;
        self.pd_idx = (self.pd_idx + 1) % self.predelay.len();
        let x = x * 0.015;
        let mut out = [0.0f32; 2];
        for side in 0..2 {
            let mut acc = 0.0;
            for c in self.combs[side].iter_mut() {
                let y = c.buf[c.idx];
                c.store = y * (1.0 - self.damp) + c.store * self.damp;
                c.buf[c.idx] = x + c.store * self.feedback;
                c.idx = (c.idx + 1) % c.buf.len();
                acc += y;
            }
            for a in self.allpasses[side].iter_mut() {
                let b = a.buf[a.idx];
                a.buf[a.idx] = acc + b * 0.5;
                a.idx = (a.idx + 1) % a.buf.len();
                acc = b - acc;
            }
            out[side] = acc;
        }
        (out[0], out[1])
    }
}

// ------------------------------------------------------------------ synth

pub struct BuiltinSynth {
    sr: f32,
    voices: Vec<Voice>,
    channels: [Channel; 16],
    reverb: Reverb,
    counter: u64,
    expr_coef: f32,
}

const MAX_VOICES: usize = 72;
const MASTER: f32 = 3.4;

impl BuiltinSynth {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate as f32;
        sine_table();
        BuiltinSynth {
            sr,
            voices: Vec::with_capacity(MAX_VOICES),
            channels: [Channel::default(); 16],
            reverb: Reverb::new(sr),
            counter: 0,
            expr_coef: 1.0 - (-1.0 / (0.05 * sr)).exp(),
        }
    }
}

impl SynthEngine for BuiltinSynth {
    fn note_on(&mut self, ch: u8, key: u8, vel: u8) {
        if vel == 0 {
            return self.note_off(ch, key);
        }
        let ch = ch & 15;
        for v in self.voices.iter_mut() {
            if v.ch == ch && v.key == key && !v.released {
                v.release();
            }
        }
        if self.voices.len() >= MAX_VOICES {
            // steal the quietest released voice, else the oldest
            let idx = self
                .voices
                .iter()
                .enumerate()
                .filter(|(_, v)| v.released)
                .min_by(|a, b| a.1.level().partial_cmp(&b.1.level()).unwrap())
                .map(|(i, _)| i)
                .or_else(|| self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i));
            if let Some(i) = idx {
                self.voices.swap_remove(i);
            }
        }
        self.counter += 1;
        let seed = (self.counter as u32).wrapping_mul(31) ^ key as u32;
        let v = vel as f32 / 127.0;
        let program = self.channels[ch as usize].program;
        let (sr, v) = (self.sr, v);
        let struck = |t: Tone, g: f32| (VoiceKind::Struck(Struck::new(sr, key, v, t, seed)), g);
        let bowed = |c: BowCfg, g: f32| (VoiceKind::Bowed(Bowed::new(sr, key, v, c, seed)), g);
        let wind = |c: WindCfg, g: f32| (VoiceKind::Wind(Wind::new(sr, key, v, c, seed)), g);
        let (kind, gain) = match program {
            0..=3 => struck(Tone::Piano, 0.55),
            4..=7 => struck(Tone::EPiano, 0.33),
            8 => struck(CELESTA, 0.3),
            9 | 14 => struck(GLOCK, 0.28),
            10 => struck(MUSIC_BOX, 0.32),
            11 => struck(VIBES, 0.3),
            12 | 13 | 47 => struck(MARIMBA, 0.3),
            108 => struck(KALIMBA, 0.3),
            15 | 24..=31 | 104..=107 => struck(Tone::Pluck { t60: 3.0, dark: 2.2 }, 0.34),
            32..=39 => struck(Tone::Pluck { t60: 2.5, dark: 2.8 }, 0.42),
            46 => struck(Tone::Pluck { t60: 4.0, dark: 2.4 }, 0.34),
            45 | 109..=119 => struck(Tone::Pluck { t60: 1.0, dark: 2.6 }, 0.4),
            40 => bowed(VIOLIN, 0.32),
            41 => bowed(VIOLA, 0.32),
            42 => bowed(CELLO, 0.36),
            43 => bowed(CONTRABASS, 0.38),
            50 | 51 => bowed(SYNTH_STRINGS, 0.2),
            52..=54 | 91 => bowed(CHOIR, 0.3),
            94 => bowed(HALO, 0.24),
            88..=95 => bowed(PAD, 0.28),
            56..=67 => wind(HORN, 0.3),
            68 | 69 => wind(OBOE, 0.26),
            70 => wind(BASSOON, 0.3),
            71 => wind(CLARINET, 0.3),
            72..=79 => wind(FLUTE, 0.24),
            _ => bowed(ENSEMBLE, 0.22),
        };
        // perceptual velocity curve
        let gain = gain * (0.08 + 0.92 * v * v);
        let pan = ((key as f32 - 64.0) / 48.0).clamp(-0.6, 0.6) + (hash01(seed) - 0.5) * 0.2;
        let angle = (pan + 1.0) * PI / 4.0;
        self.voices.push(Voice {
            ch,
            key,
            kind,
            gain,
            pan_l: angle.cos(),
            pan_r: angle.sin(),
            age: self.counter,
            released: false,
        });
    }

    fn note_off(&mut self, ch: u8, key: u8) {
        for v in self.voices.iter_mut() {
            if v.ch == ch && v.key == key && !v.released {
                v.release();
            }
        }
    }

    fn control(&mut self, ch: u8, cc: u8, value: u8) {
        let c = &mut self.channels[(ch & 15) as usize];
        let x = value as f32 / 127.0;
        match cc {
            7 => c.volume = x,
            11 => c.expr_target = x,
            91 => c.send = x,
            120 | 123 => self.voices.retain(|v| v.ch != ch),
            _ => {}
        }
    }

    fn program(&mut self, ch: u8, program: u8) {
        self.channels[(ch & 15) as usize].program = program;
    }

    fn reset(&mut self) {
        self.voices.clear();
        for c in self.channels.iter_mut() {
            let program = c.program;
            *c = Channel::default();
            c.program = program;
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let table = sine_table();
        for c in self.channels.iter_mut() {
            // expression smoothing at block rate is plenty
            c.expr += (c.expr_target - c.expr) * (self.expr_coef * left.len() as f32).min(1.0);
        }
        for i in 0..left.len() {
            let mut l = 0.0;
            let mut r = 0.0;
            let mut send = 0.0;
            for v in self.voices.iter_mut() {
                let ch = &self.channels[v.ch as usize];
                let s = match &mut v.kind {
                    VoiceKind::Struck(x) => x.next(table),
                    VoiceKind::Bowed(x) => x.next(table),
                    VoiceKind::Wind(x) => x.next(table),
                } * v.gain
                    * ch.volume
                    * ch.expr;
                l += s * v.pan_l;
                r += s * v.pan_r;
                send += s * ch.send;
            }
            let (wl, wr) = self.reverb.process(send);
            left[i] = soft_clip((l + wl * 2.2) * MASTER);
            right[i] = soft_clip((r + wr * 2.2) * MASTER);
        }
        self.voices.retain(|v| !(v.released || matches!(v.kind, VoiceKind::Struck(_))) || v.level() > 2e-4);
    }
}

#[inline]
fn soft_clip(x: f32) -> f32 {
    if x.abs() < 0.6 { x } else { x.signum() * (0.6 + 0.4 * ((x.abs() - 0.6) / 0.4).tanh()) }
}

// --------------------------------------------------------------- soundfont

pub struct SoundFontSynth {
    synth: rustysynth::Synthesizer,
}

impl SoundFontSynth {
    pub fn new(font: &Arc<rustysynth::SoundFont>, sample_rate: u32) -> Result<Self, String> {
        let settings = rustysynth::SynthesizerSettings::new(sample_rate as i32);
        let mut synth = rustysynth::Synthesizer::new(font, &settings).map_err(|e| format!("{e:?}"))?;
        // The generator uses soft velocities; bring SoundFonts up to a similar level as the built-in synth.
        synth.set_master_volume(3.5);
        Ok(Self { synth })
    }
}

impl SynthEngine for SoundFontSynth {
    fn note_on(&mut self, ch: u8, key: u8, vel: u8) {
        self.synth.note_on(ch as i32, key as i32, vel as i32);
    }
    fn note_off(&mut self, ch: u8, key: u8) {
        self.synth.note_off(ch as i32, key as i32);
    }
    fn control(&mut self, ch: u8, cc: u8, value: u8) {
        self.synth.process_midi_message(ch as i32, 0xB0, cc as i32, value as i32);
    }
    fn program(&mut self, ch: u8, program: u8) {
        self.synth.process_midi_message(ch as i32, 0xC0, program as i32, 0);
    }
    fn reset(&mut self) {
        self.synth.note_off_all(true);
        self.synth.reset_all_controllers();
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.synth.render(left, right);
        for x in left.iter_mut().chain(right.iter_mut()) {
            *x = soft_clip(*x);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::SOUNDS;

    /// Every selectable sound makes a clean, audible note at a sensible level.
    #[test]
    fn every_sound_renders() {
        let sr = 44100;
        for snd in SOUNDS {
            let mut peaks = Vec::new();
            for key in [40u8, 64, 84] {
                let mut s = BuiltinSynth::new(sr);
                s.program(0, snd.program);
                s.control(0, 91, 0);
                s.note_on(0, key, 80);
                let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
                let mut peak = 0.0f32;
                for block in 0..(sr as usize * 2 / 512) {
                    if block == sr as usize / 512 {
                        s.note_off(0, key);
                    }
                    s.render(&mut l, &mut r);
                    for x in l.iter().chain(r.iter()) {
                        assert!(x.is_finite(), "{} produced NaN", snd.name);
                        peak = peak.max(x.abs());
                    }
                }
                peaks.push(peak);
            }
            println!("{:<18} peaks {:.2} {:.2} {:.2}", snd.name, peaks[0], peaks[1], peaks[2]);
            assert!(peaks.iter().all(|p| *p > 0.03 && *p < 0.95), "{} level out of range: {peaks:?}", snd.name);
        }
    }
}
