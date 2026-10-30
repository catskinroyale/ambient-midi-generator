//! Audio engines. `BuiltinSynth` is a small self-contained synthesiser (additive
//! piano, bowed strings, plucks and a hall reverb) so playback works out
//! of the box. `SoundFontSynth` wraps rustysynth for any General MIDI .sf2 file.

use std::f32::consts::{PI, TAU};
use std::sync::{Arc, OnceLock};

use crate::reverb::Reverb;
use crate::song::Fx;

pub trait SynthEngine: Send {
    fn note_on(&mut self, ch: u8, key: u8, vel: u8);
    fn note_off(&mut self, ch: u8, key: u8);
    fn control(&mut self, ch: u8, cc: u8, value: u8);
    fn program(&mut self, ch: u8, program: u8);
    /// Silence everything and reset controllers.
    fn reset(&mut self);
    fn render(&mut self, left: &mut [f32], right: &mut [f32]);
    /// Change the reverb and tuning while playing.
    fn set_fx(&mut self, _fx: &Fx) {}
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

/// Pitch offset in cents for a channel at time `t`: the reference pitch, a
/// fixed per-part detune, and slow drift (a shared tape-like wow plus a little
/// wander of the part's own).
pub fn channel_cents(fx: &Fx, ch: u8, t: f64) -> f32 {
    let mut cents = fx.tuning.cents() + (hash01(ch as u32 * 7919 + 3) - 0.5) * 2.0 * fx.detune * 9.0;
    if fx.drift > 0.0 {
        let c = ch as f64;
        let wow = (t * std::f64::consts::TAU * 0.23).sin() * 0.6 + (t * std::f64::consts::TAU * 0.071 + 1.3).sin() * 0.4;
        let own = (t * std::f64::consts::TAU * (0.13 + 0.017 * c) + c * 2.1).sin();
        cents += fx.drift * 16.0 * (wow * 0.7 + own * 0.3) as f32;
    }
    cents
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
    fn new(sr: f32, key: u8, vel: f32, tone: Tone, seed: u32, cents: f32) -> Self {
        let f0 = midi_hz(key as f32 + cents * 0.01);
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
    fn next(&mut self, table: &[f32], ratio: f32) -> f32 {
        let mut s = 0.0;
        for p in self.partials.iter_mut() {
            s += fast_sin(table, p.phase) * p.amp;
            p.phase += p.inc * ratio;
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
    fn new(sr: f32, key: u8, vel: f32, cfg: BowCfg, seed: u32, cents: f32, spread: f32) -> Self {
        let f0 = midi_hz(key as f32 + cents * 0.01);
        let mut phases = [0.0; 3];
        let mut incs = [0.0; 3];
        for (i, d) in cfg.detunes.iter().take(3).enumerate() {
            phases[i] = hash01(seed + i as u32 * 17);
            incs[i] = f0 * 2f32.powf(d * spread / 1200.0) / sr;
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
    fn next(&mut self, table: &[f32], ratio: f32) -> f32 {
        let vib = fast_sin(table, self.vib_phase) * self.vib_depth * self.vib_onset;
        self.vib_phase += self.vib_inc;
        if self.vib_phase >= 1.0 {
            self.vib_phase -= 1.0;
        }
        if self.vib_onset < 1.0 {
            self.vib_onset += self.vib_onset_inc;
        }
        let ratio = ratio * (1.0 + vib * 0.000_577_8); // cents -> ratio (approx)
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
    fn new(sr: f32, key: u8, vel: f32, cfg: WindCfg, seed: u32, cents: f32) -> Self {
        let f0 = midi_hz(key as f32 + cents * 0.01);
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
    fn next(&mut self, table: &[f32], ratio: f32) -> f32 {
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
        self.phase += self.inc * ratio * (1.0 + vib * 0.000_577_8);
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

// ------------------------------------------------------------------ synth

pub struct BuiltinSynth {
    sr: f32,
    voices: Vec<Voice>,
    channels: [Channel; 16],
    reverb: Reverb,
    counter: u64,
    expr_coef: f32,
    fx: Fx,
    /// Samples rendered, for the drift.
    clock: u64,
}

const MAX_VOICES: usize = 72;
const MASTER: f32 = 3.4;
/// Reverb level for both engines.
const WET: f32 = 2.0;
/// Pitch drift is updated this often (samples).
const DRIFT_BLOCK: usize = 32;
/// SoundFont presets are brighter and fuller, so they need less.
const SF_WET: f32 = 1.2;

impl BuiltinSynth {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate as f32;
        sine_table();
        BuiltinSynth {
            sr,
            voices: Vec::with_capacity(MAX_VOICES),
            channels: [Channel::default(); 16],
            reverb: Reverb::new(sr, &Fx::default()),
            counter: 0,
            expr_coef: 1.0 - (-1.0 / (0.05 * sr)).exp(),
            fx: Fx::default(),
            clock: 0,
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
        // each note lands a little off pitch, and ensemble sounds spread wider
        let cents = (hash01(seed ^ 0x5bd1) - 0.5) * 2.0 * self.fx.detune * 6.0;
        let spread = 1.0 + self.fx.detune * 1.5;
        let struck = |t: Tone, g: f32| (VoiceKind::Struck(Struck::new(sr, key, v, t, seed, cents)), g);
        let bowed = |c: BowCfg, g: f32| (VoiceKind::Bowed(Bowed::new(sr, key, v, c, seed, cents, spread)), g);
        let wind = |c: WindCfg, g: f32| (VoiceKind::Wind(Wind::new(sr, key, v, c, seed, cents)), g);
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
        self.reverb.clear();
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let table = sine_table();
        for c in self.channels.iter_mut() {
            // expression smoothing at block rate is plenty
            c.expr += (c.expr_target - c.expr) * (self.expr_coef * left.len() as f32).min(1.0);
        }
        let mut ratios = [1.0f32; 16];
        for i in 0..left.len() {
            if self.clock % DRIFT_BLOCK as u64 == 0 {
                let t = self.clock as f64 / self.sr as f64;
                for (ch, r) in ratios.iter_mut().enumerate() {
                    *r = 2f32.powf(channel_cents(&self.fx, ch as u8, t) / 1200.0);
                }
            }
            self.clock += 1;
            let mut l = 0.0;
            let mut r = 0.0;
            let (mut sl, mut sr) = (0.0, 0.0);
            for v in self.voices.iter_mut() {
                let ch = &self.channels[v.ch as usize];
                let ratio = ratios[v.ch as usize];
                let s = match &mut v.kind {
                    VoiceKind::Struck(x) => x.next(table, ratio),
                    VoiceKind::Bowed(x) => x.next(table, ratio),
                    VoiceKind::Wind(x) => x.next(table, ratio),
                } * v.gain
                    * ch.volume
                    * ch.expr;
                l += s * v.pan_l;
                r += s * v.pan_r;
                sl += s * v.pan_l * ch.send;
                sr += s * v.pan_r * ch.send;
            }
            let (wl, wr) = self.reverb.process(sl, sr);
            left[i] = soft_clip((l + wl * WET) * MASTER);
            right[i] = soft_clip((r + wr * WET) * MASTER);
        }
        self.voices.retain(|v| !(v.released || matches!(v.kind, VoiceKind::Struck(_))) || v.level() > 2e-4);
    }

    fn set_fx(&mut self, fx: &Fx) {
        self.fx = *fx;
        self.reverb.set(fx);
    }
}

#[inline]
fn soft_clip(x: f32) -> f32 {
    if x.abs() < 0.6 { x } else { x.signum() * (0.6 + 0.4 * ((x.abs() - 0.6) / 0.4).tanh()) }
}

// --------------------------------------------------------------- soundfont

/// A SoundFont player. Its own (Freeverb) reverb is switched off and the
/// shared hall reverb used instead, driven by the channels' reverb sends.
pub struct SoundFontSynth {
    synth: rustysynth::Synthesizer,
    reverb: Reverb,
    sends: [f32; 16],
    fx: Fx,
    sr: f32,
    clock: u64,
    /// Pitch bend last sent to each channel.
    bends: [u16; 16],
}

/// Pitch bend range in semitones, GM's default.
const BEND_RANGE: f32 = 2.0;

impl SoundFontSynth {
    pub fn new(font: &Arc<rustysynth::SoundFont>, sample_rate: u32) -> Result<Self, String> {
        let mut settings = rustysynth::SynthesizerSettings::new(sample_rate as i32);
        settings.enable_reverb_and_chorus = false;
        let mut synth = rustysynth::Synthesizer::new(font, &settings).map_err(|e| format!("{e:?}"))?;
        // The generator uses soft velocities; bring SoundFonts up to a similar level as the built-in synth.
        synth.set_master_volume(3.5);
        let sr = sample_rate as f32;
        Ok(Self {
            synth,
            reverb: Reverb::new(sr, &Fx::default()),
            sends: [0.4; 16],
            fx: Fx::default(),
            sr,
            clock: 0,
            bends: [8192; 16],
        })
    }

    /// Bend every channel to its tuning, detune and drift.
    fn retune(&mut self) {
        let t = self.clock as f64 / self.sr as f64;
        for ch in 0..16u8 {
            let cents = channel_cents(&self.fx, ch, t);
            let bend = (8192.0 + cents / (BEND_RANGE * 100.0) * 8192.0).round().clamp(0.0, 16383.0) as u16;
            if bend != self.bends[ch as usize] {
                self.bends[ch as usize] = bend;
                self.synth.process_midi_message(ch as i32, 0xE0, (bend & 0x7f) as i32, (bend >> 7) as i32);
            }
        }
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
        if cc == 91 {
            self.sends[(ch & 15) as usize] = value as f32 / 127.0;
        }
        self.synth.process_midi_message(ch as i32, 0xB0, cc as i32, value as i32);
    }
    fn program(&mut self, ch: u8, program: u8) {
        self.synth.process_midi_message(ch as i32, 0xC0, program as i32, 0);
    }
    fn reset(&mut self) {
        self.synth.note_off_all(true);
        self.synth.reset_all_controllers();
        self.reverb.clear();
        self.sends = [0.4; 16];
        // controllers were reset, so the bends must be sent again
        self.bends = [8192; 16];
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        // render in short pieces so the drift moves smoothly
        let mut off = 0;
        while off < left.len() {
            let len = (DRIFT_BLOCK * 2).min(left.len() - off);
            self.retune();
            self.synth.render(&mut left[off..off + len], &mut right[off..off + len]);
            self.clock += len as u64;
            off += len;
        }
        // The mix arrives already summed, so one send level (the highest) is used.
        let send = self.sends.iter().fold(0.0f32, |m, s| m.max(*s));
        for i in 0..left.len() {
            let (wl, wr) = self.reverb.process(left[i] * send, right[i] * send);
            left[i] = soft_clip(left[i] + wl * SF_WET);
            right[i] = soft_clip(right[i] + wr * SF_WET);
        }
    }
    fn set_fx(&mut self, fx: &Fx) {
        self.fx = *fx;
        self.reverb.set(fx);
    }
}

// ------------------------------------------------------------------- mixer

/// Sends each MIDI channel to one of several engines and mixes their output.
/// Engine 0 is the main engine; the others are SoundFonts chosen for single parts.
pub struct MixSynth {
    engines: Vec<Box<dyn SynthEngine>>,
    route: [usize; 16],
    tmp_l: Vec<f32>,
    tmp_r: Vec<f32>,
}

impl MixSynth {
    pub fn new(main: Box<dyn SynthEngine>) -> Self {
        MixSynth { engines: vec![main], route: [0; 16], tmp_l: Vec::new(), tmp_r: Vec::new() }
    }

    /// Add an engine and return its index, for `route`.
    pub fn add(&mut self, engine: Box<dyn SynthEngine>) -> usize {
        self.engines.push(engine);
        self.engines.len() - 1
    }

    pub fn route(&mut self, ch: u8, engine: usize) {
        self.route[(ch & 15) as usize] = engine.min(self.engines.len() - 1);
    }

    fn engine(&mut self, ch: u8) -> &mut Box<dyn SynthEngine> {
        &mut self.engines[self.route[(ch & 15) as usize]]
    }
}

impl SynthEngine for MixSynth {
    fn note_on(&mut self, ch: u8, key: u8, vel: u8) {
        self.engine(ch).note_on(ch, key, vel);
    }
    fn note_off(&mut self, ch: u8, key: u8) {
        self.engine(ch).note_off(ch, key);
    }
    fn control(&mut self, ch: u8, cc: u8, value: u8) {
        self.engine(ch).control(ch, cc, value);
    }
    fn program(&mut self, ch: u8, program: u8) {
        self.engine(ch).program(ch, program);
    }
    fn reset(&mut self) {
        self.engines.iter_mut().for_each(|e| e.reset());
    }
    fn set_fx(&mut self, fx: &Fx) {
        self.engines.iter_mut().for_each(|e| e.set_fx(fx));
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let (first, rest) = self.engines.split_first_mut().expect("main engine");
        first.render(left, right);
        if rest.is_empty() {
            return;
        }
        self.tmp_l.resize(left.len(), 0.0);
        self.tmp_r.resize(left.len(), 0.0);
        for e in rest {
            e.render(&mut self.tmp_l, &mut self.tmp_r);
            for i in 0..left.len() {
                left[i] += self.tmp_l[i];
                right[i] += self.tmp_r[i];
            }
        }
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

    /// Records which engine each event reached.
    struct Probe(std::sync::Arc<std::sync::Mutex<Vec<(u8, u8)>>>, u8);

    impl SynthEngine for Probe {
        fn note_on(&mut self, ch: u8, _: u8, _: u8) {
            self.0.lock().unwrap().push((self.1, ch));
        }
        fn note_off(&mut self, _: u8, _: u8) {}
        fn control(&mut self, _: u8, _: u8, _: u8) {}
        fn program(&mut self, _: u8, _: u8) {}
        fn reset(&mut self) {}
        fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
            l.fill(0.25);
            r.fill(0.25);
        }
    }

    #[test]
    fn mix_routes_channels_and_sums() {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut mix = MixSynth::new(Box::new(Probe(log.clone(), 0)));
        let second = mix.add(Box::new(Probe(log.clone(), 1)));
        mix.route(2, second);
        for ch in 0..4 {
            mix.note_on(ch, 60, 90);
        }
        assert_eq!(*log.lock().unwrap(), vec![(0, 0), (0, 1), (1, 2), (0, 3)]);
        let (mut l, mut r) = (vec![0.0; 8], vec![0.0; 8]);
        mix.render(&mut l, &mut r);
        assert!((l[0] - 0.5).abs() < 1e-6 && (r[7] - 0.5).abs() < 1e-6);
    }

    /// Tuning moves the pitch, and detune and drift keep it near the note.
    #[test]
    fn tuning_and_detune_shift_pitch() {
        use crate::song::Tuning;
        let sr = 44100;
        let pitch = |fx: Fx| {
            let mut s = BuiltinSynth::new(sr);
            s.set_fx(&fx);
            s.program(0, 73); // flute: nearly a pure tone
            s.control(0, 91, 0);
            s.note_on(0, 69, 100);
            let (mut l, mut r) = (vec![0.0f32; sr as usize], vec![0.0f32; sr as usize]);
            s.render(&mut l, &mut r);
            // count rising zero crossings in the second half second
            let x = &l[sr as usize / 2..];
            x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f32 * 2.0
        };
        let a440 = pitch(Fx::default());
        let a432 = pitch(Fx { tuning: Tuning::A432, ..Fx::default() });
        let loose = pitch(Fx { detune: 1.0, drift: 1.0, ..Fx::default() });
        println!("A440 {a440} Hz, A432 {a432} Hz, detuned and drifting {loose} Hz");
        assert!((a440 - 440.0).abs() <= 4.0 && (a432 - 432.0).abs() <= 4.0);
        assert!((loose - 440.0).abs() <= 15.0);
    }
}
