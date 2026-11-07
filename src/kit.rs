//! The score kit: trailer drums, and effects (a sub drone, 808-style booms,
//! braams, risers, reversed cymbals, whooshes, impacts and glassy grains), all
//! synthesised here from oscillators and seeded noise. The built-in synth
//! renders them at its own sample rate, and examples/make_soundfont.rs bakes
//! them into the Ambient Electronic SoundFont, so this file uses nothing else
//! from the crate.

use std::f64::consts::{PI, TAU};
use std::sync::Arc;

/// Keys of the drum part. They sit where General MIDI puts similar drums, so
/// another kit plays something like them.
pub mod drum {
    /// A soft, muffled kick, played in pairs like a heartbeat.
    pub const HEARTBEAT: u8 = 35;
    pub const KICK: u8 = 36;
    /// A short filtered tick.
    pub const TICK: u8 = 37;
    pub const SNARE: u8 = 38;
    pub const CLAP: u8 = 39;
    pub const TOM_LOW: u8 = 41;
    pub const HAT: u8 = 42;
    pub const TOM_MID: u8 = 43;
    pub const TOM_HIGH: u8 = 45;
    pub const CRASH: u8 = 49;
}

/// Keys of the effects part. The pitched sounds take an octave of keys each:
/// play the note wanted in that octave. The drone sounds at the key; the
/// booms, braams and grains an octave below it.
pub mod fx {
    pub const DRONE: u8 = 24;
    pub const BOOM: u8 = 36;
    pub const BRAAM: u8 = 48;
    pub const RISER: u8 = 60;
    pub const RISER_LONG: u8 = 61;
    pub const REVERSE: u8 = 62;
    pub const REVERSE_LONG: u8 = 63;
    pub const WHOOSH: u8 = 64;
    pub const DOWNLIFTER: u8 = 65;
    pub const IMPACT: u8 = 67;
    pub const GRAINS: u8 = 72;
}

/// The drum kit's General MIDI program: the Orchestra kit's place, so a General
/// MIDI SoundFont plays concert drums instead.
pub const KIT_PROGRAM: u8 = 48;
/// The effects' program: General MIDI's Reverse Cymbal.
pub const FX_PROGRAM: u8 = 119;

/// How long the risers and reversed cymbals take to reach their peak, which
/// is where they end.
pub const RISER_SECS: f64 = 4.0;
pub const RISER_LONG_SECS: f64 = 8.0;
pub const REVERSE_SECS: f64 = 2.0;
pub const REVERSE_LONG_SECS: f64 = 4.0;
/// When a whoosh is loudest.
pub const WHOOSH_PEAK_SECS: f64 = 0.9;

/// One sound of the kit.
pub struct Sound {
    /// The SoundFont builder names its samples with it.
    #[allow(dead_code)]
    pub name: &'static str,
    /// The keys it plays on: one, or an octave for a pitched sound.
    pub keys: (u8, u8),
    /// The key on which the sample plays at the pitch it was made.
    pub root: u8,
    pub data: Arc<[f32]>,
    /// Loop start and end for a sustained sound. The samples just past the
    /// end repeat the loop's start, for interpolation.
    pub looped: Option<(usize, usize)>,
}

impl Sound {
    pub fn pitched(&self) -> bool {
        self.keys.0 != self.keys.1
    }
}

/// The drum part's sounds.
pub fn drums(sr: f64) -> Vec<Sound> {
    vec![
        one(drum::HEARTBEAT, "Heartbeat", 0.85, heartbeat(sr)),
        one(drum::KICK, "Trailer Kick", 1.0, kick(sr)),
        one(drum::TICK, "Tick", 0.3, tick(sr)),
        one(drum::SNARE, "Big Snare", 0.75, snare(sr)),
        one(drum::CLAP, "Clap", 0.55, clap(sr)),
        one(drum::TOM_LOW, "Taiko Low", 1.0, tom(sr, 58.0, 0.5, 11)),
        one(drum::HAT, "Filtered Hat", 0.22, hat(sr)),
        one(drum::TOM_MID, "Taiko Mid", 0.9, tom(sr, 78.0, 0.42, 12)),
        one(drum::TOM_HIGH, "Taiko High", 0.8, tom(sr, 104.0, 0.34, 13)),
        one(drum::CRASH, "Crash", 0.4, cymbal(sr, 4.5, 1.1, 14)),
    ]
}

/// The effects part's sounds.
pub fn effects(sr: f64) -> Vec<Sound> {
    let (drone, looped) = drone(sr);
    let reverse = |secs: f64, seed| {
        let mut c = cymbal(sr, secs, secs / 3.0, seed);
        c.reverse();
        fade(&mut c, sr, 0.04, 0.006);
        c
    };
    vec![
        Sound { looped: Some(looped), ..pitched(fx::DRONE, "Sub Drone", 0.5, drone) },
        pitched(fx::BOOM, "Sub Boom", 1.0, boom(sr)),
        pitched(fx::BRAAM, "Braam", 0.85, braam(sr)),
        one(fx::RISER, "Riser", 0.42, riser(sr, RISER_SECS, 21)),
        one(fx::RISER_LONG, "Long Riser", 0.42, riser(sr, RISER_LONG_SECS, 22)),
        one(fx::REVERSE, "Reverse Cymbal", 0.45, reverse(REVERSE_SECS, 23)),
        one(fx::REVERSE_LONG, "Long Reverse Cymbal", 0.45, reverse(REVERSE_LONG_SECS, 24)),
        one(fx::WHOOSH, "Whoosh", 0.4, whoosh(sr)),
        one(fx::DOWNLIFTER, "Downlifter", 0.4, downlifter(sr)),
        one(fx::IMPACT, "Impact", 1.0, impact(sr)),
        pitched(fx::GRAINS, "Glass Grains", 0.32, grains(sr)),
    ]
}

/// The pitch the pitched sounds are made at, as a key: F#, in the middle of
/// their octave, so no key moves the sample more than half an octave.
const MADE_AT: u8 = 6;

/// The kit's loudest sounds peak this far below full scale.
const LEVEL: f64 = 0.75;

fn finish(data: Vec<f64>, gain: f64) -> Arc<[f32]> {
    let peak = data.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-9);
    data.iter().map(|v| (v / peak * gain * LEVEL) as f32).collect()
}

fn one(key: u8, name: &'static str, gain: f64, data: Vec<f64>) -> Sound {
    Sound { name, keys: (key, key), root: key, data: finish(data, gain), looped: None }
}

/// A sound for the octave of keys from `base`, which plays the sample as made
/// on `base + MADE_AT`. Sounds that play an octave below their keys are made
/// an octave down.
fn pitched(base: u8, name: &'static str, gain: f64, data: Vec<f64>) -> Sound {
    Sound { name, keys: (base, base + 11), root: base + MADE_AT, data: finish(data, gain), looped: None }
}

/// The frequency of the made-at note in the octave of keys from `base`.
fn made_hz(base: u8) -> f64 {
    440.0 * 2f64.powf((base as f64 + MADE_AT as f64 - 69.0) / 12.0)
}

// ------------------------------------------------------------------ helpers

/// Deterministic noise (splitmix64), so the kit is the same every time.
struct Noise(u64);

impl Noise {
    fn new(seed: u64) -> Self {
        Noise(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 0x5EED_0000_0000_0000)
    }

    /// Uniform in 0..1.
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in -1..1.
    fn next(&mut self) -> f64 {
        self.unit() * 2.0 - 1.0
    }
}

/// A state-variable filter (Simper's trapezoidal form), which can be retuned
/// every sample.
#[derive(Default)]
struct Svf {
    ic1: f64,
    ic2: f64,
}

struct Bands {
    low: f64,
    band: f64,
    high: f64,
}

impl Svf {
    fn run(&mut self, x: f64, hz: f64, q: f64, sr: f64) -> Bands {
        let g = (PI * (hz / sr).clamp(1e-5, 0.49)).tan();
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        Bands { low: v2, band: v1, high: x - k * v1 - v2 }
    }
}

/// A band-limited sawtooth (PolyBLEP).
#[derive(Default)]
struct Saw {
    phase: f64,
}

impl Saw {
    fn next(&mut self, hz: f64, sr: f64) -> f64 {
        let dt = (hz / sr).min(0.5);
        let t = self.phase;
        let mut y = 2.0 * t - 1.0;
        if t < dt {
            let x = t / dt;
            y -= x + x - x * x - 1.0;
        } else if t > 1.0 - dt {
            let x = (t - 1.0) / dt;
            y -= x * x + x + x + 1.0;
        }
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        y
    }
}

/// A sine whose frequency can change every sample.
#[derive(Default)]
struct Osc {
    phase: f64,
}

impl Osc {
    fn next(&mut self, hz: f64, sr: f64) -> f64 {
        let y = (TAU * self.phase).sin();
        self.phase = (self.phase + hz / sr).fract();
        y
    }
}

/// Soft saturation that keeps the level of quiet signals.
fn drive(x: f64, amount: f64) -> f64 {
    (x * amount).tanh() / amount.tanh()
}

/// Fade in over `inn` and out over `out` seconds.
fn fade(data: &mut [f64], sr: f64, inn: f64, out: f64) {
    let (a, b) = ((inn * sr) as usize, (out * sr) as usize);
    for (i, v) in data.iter_mut().take(a).enumerate() {
        *v *= i as f64 / a as f64;
    }
    for (i, v) in data.iter_mut().rev().take(b).enumerate() {
        *v *= i as f64 / b as f64;
    }
}

fn len(sr: f64, secs: f64) -> usize {
    (secs * sr) as usize
}

// -------------------------------------------------------------------- drums

/// A deep, soft kick with no click: felt more than heard.
fn heartbeat(sr: f64) -> Vec<f64> {
    let mut osc = Osc::default();
    let mut lp = Svf::default();
    let mut noise = Noise::new(1);
    let mut out: Vec<f64> = (0..len(sr, 0.9))
        .map(|i| {
            let t = i as f64 / sr;
            let body = osc.next(40.0 + 34.0 * (-t / 0.03).exp(), sr) * (-t / 0.17).exp();
            let thump = lp.run(noise.next(), 260.0, 0.7, sr).low * (-t / 0.012).exp();
            drive(body + 0.6 * thump, 1.3)
        })
        .collect();
    fade(&mut out, sr, 0.002, 0.05);
    out
}

/// A big, round kick: a falling sine with a long sub tail and a beater click.
fn kick(sr: f64) -> Vec<f64> {
    let mut osc = Osc::default();
    let mut hp = Svf::default();
    let mut noise = Noise::new(2);
    let mut out: Vec<f64> = (0..len(sr, 1.4))
        .map(|i| {
            let t = i as f64 / sr;
            let amp = 0.7 * (-t / 0.22).exp() + 0.3 * (-t / 0.6).exp();
            let body = osc.next(46.0 + 120.0 * (-t / 0.032).exp(), sr) * amp;
            let click = hp.run(noise.next(), 3000.0, 0.7, sr).high * (-t / 0.003).exp();
            drive(body + 0.35 * click, 1.8)
        })
        .collect();
    fade(&mut out, sr, 0.0005, 0.1);
    out
}

fn tick(sr: f64) -> Vec<f64> {
    let mut osc = Osc::default();
    let mut bp = Svf::default();
    let mut noise = Noise::new(3);
    let mut out: Vec<f64> = (0..len(sr, 0.2))
        .map(|i| {
            let t = i as f64 / sr;
            bp.run(noise.next(), 4200.0, 3.0, sr).band * (-t / 0.012).exp() + 0.3 * osc.next(2600.0, sr) * (-t / 0.006).exp()
        })
        .collect();
    fade(&mut out, sr, 0.0005, 0.03);
    out
}

/// A closed hat, filtered down to a soft tick of air.
fn hat(sr: f64) -> Vec<f64> {
    let mut bp = Svf::default();
    let mut hp = Svf::default();
    let mut noise = Noise::new(4);
    let mut out: Vec<f64> = (0..len(sr, 0.25))
        .map(|i| {
            let t = i as f64 / sr;
            let x = hp.run(noise.next(), 5000.0, 0.7, sr).high;
            bp.run(x, 7500.0, 1.2, sr).band * (-t / 0.03).exp()
        })
        .collect();
    fade(&mut out, sr, 0.0005, 0.04);
    out
}

/// A big snare: a short tuned body, a wire rattle and a little room after it.
fn snare(sr: f64) -> Vec<f64> {
    let mut osc = Osc::default();
    let (mut bp, mut hp, mut lp) = (Svf::default(), Svf::default(), Svf::default());
    let mut noise = Noise::new(5);
    let mut out: Vec<f64> = (0..len(sr, 1.2))
        .map(|i| {
            let t = i as f64 / sr;
            let n = noise.next();
            let body = osc.next(180.0 + 70.0 * (-t / 0.02).exp(), sr) * (-t / 0.07).exp();
            let wires = (bp.run(n, 2200.0, 0.8, sr).band + 0.6 * hp.run(n, 3500.0, 0.7, sr).high) * (-t / 0.16).exp();
            let room = lp.run(n, 3000.0, 0.7, sr).low * 0.12 * (-t / 0.4).exp() * (1.0 - (-t / 0.02).exp());
            drive(0.8 * body + wires + room, 1.5)
        })
        .collect();
    fade(&mut out, sr, 0.0005, 0.15);
    out
}

/// A handclap: a few quick bursts, then a short tail.
fn clap(sr: f64) -> Vec<f64> {
    let mut bp = Svf::default();
    let mut noise = Noise::new(6);
    let bursts = [0.0, 0.011, 0.023, 0.034];
    let mut out: Vec<f64> = (0..len(sr, 0.8))
        .map(|i| {
            let t = i as f64 / sr;
            let env: f64 = bursts
                .iter()
                .enumerate()
                .map(|(k, &at)| {
                    if t < at {
                        0.0
                    } else {
                        let tau = if k + 1 == bursts.len() { 0.18 } else { 0.006 };
                        (-(t - at) / tau).exp()
                    }
                })
                .sum();
            bp.run(noise.next(), 1300.0, 1.4, sr).band * env
        })
        .collect();
    fade(&mut out, sr, 0.0005, 0.1);
    out
}

/// A taiko-like drum: the skin's pitch falls as it rings, with a second mode
/// above it and the slap of the stick.
fn tom(sr: f64, hz: f64, decay: f64, seed: u64) -> Vec<f64> {
    let (mut a, mut b) = (Osc::default(), Osc::default());
    let mut lp = Svf::default();
    let mut noise = Noise::new(seed);
    let mut out: Vec<f64> = (0..len(sr, 2.2))
        .map(|i| {
            let t = i as f64 / sr;
            let f = hz * (1.0 + 0.6 * (-t / 0.05).exp());
            let body = a.next(f, sr) * (-t / decay).exp() + 0.35 * b.next(1.52 * f, sr) * (-t / (0.5 * decay)).exp();
            let slap = lp.run(noise.next(), 1100.0, 0.8, sr).low * (-t / 0.025).exp();
            drive(body + 0.8 * slap, 2.0)
        })
        .collect();
    fade(&mut out, sr, 0.0005, 0.2);
    out
}

/// A crash or ride: bright noise and a cluster of inharmonic partials.
fn cymbal(sr: f64, secs: f64, decay: f64, seed: u64) -> Vec<f64> {
    let mut noise = Noise::new(seed);
    let (mut hp, mut bp) = (Svf::default(), Svf::default());
    let partials: Vec<(f64, f64)> = (0..24)
        .map(|_| (400.0 * (22.0f64).powf(noise.unit()), noise.unit()))
        .collect();
    let mut oscs: Vec<Osc> = partials.iter().map(|_| Osc { phase: noise.unit() }).collect();
    let mut out: Vec<f64> = (0..len(sr, secs))
        .map(|i| {
            let t = i as f64 / sr;
            let n = noise.next();
            let hiss = hp.run(n, 4000.0, 0.7, sr).high + 0.5 * bp.run(n, 7000.0, 1.0, sr).band;
            let ring: f64 = oscs.iter_mut().zip(&partials).map(|(o, (f, a))| o.next(*f, sr) * a).sum::<f64>() / 8.0;
            let env = (-t / decay).exp() * (1.0 - (-t / 0.003).exp());
            (hiss + 0.4 * ring) * env
        })
        .collect();
    fade(&mut out, sr, 0.0, 0.3);
    out
}

// ------------------------------------------------------------------ effects

/// A deep sine with a little warmth above it, looped. It is a whole number of
/// cycles long, so the loop is seamless; the rounding moves the pitch by a
/// tiny fraction of a cent.
fn drone(sr: f64) -> (Vec<f64>, (usize, usize)) {
    let f0 = made_hz(fx::DRONE);
    let cycles = 24.0;
    let n = (cycles * sr / f0).round() as usize;
    let mut out: Vec<f64> = (0..n)
        .map(|i| {
            let ph = TAU * cycles * i as f64 / n as f64;
            ph.sin() + 0.2 * (2.0 * ph).sin() + 0.05 * (3.0 * ph).sin()
        })
        .collect();
    let guard: Vec<f64> = out[..8].to_vec();
    out.extend(guard);
    (out, (0, n))
}

/// An 808-style sub hit: a short drop into a long, gently driven sine.
fn boom(sr: f64) -> Vec<f64> {
    let f0 = made_hz(fx::DRONE);
    let mut osc = Osc::default();
    let mut out: Vec<f64> = (0..len(sr, 3.5))
        .map(|i| {
            let t = i as f64 / sr;
            let f = f0 * (1.0 + 1.3 * (-t / 0.022).exp());
            drive(osc.next(f, sr) * (-t / 0.95).exp(), 2.4)
        })
        .collect();
    fade(&mut out, sr, 0.001, 0.4);
    out
}

/// The trailer horn: a wall of detuned saws, with an octave below, through a
/// resonant filter that blares open and slowly closes, then driven hard.
fn braam(sr: f64) -> Vec<f64> {
    let f0 = made_hz(fx::BOOM);
    let voices: [(f64, f64); 6] = [(-9.0, 1.0), (-3.0, 1.0), (4.0, 1.0), (10.0, 1.0), (-1205.0, 0.8), (-1195.0, 0.8)];
    let mut saws: Vec<Saw> = (0..voices.len()).map(|k| Saw { phase: k as f64 / 7.0 }).collect();
    let mut sub = Osc::default();
    let (mut f1, mut f2) = (Svf::default(), Svf::default());
    let mut out: Vec<f64> = (0..len(sr, 5.5))
        .map(|i| {
            let t = i as f64 / sr;
            let scoop = 1.0 - 0.03 * (-t / 0.08).exp();
            let mut x = 0.0;
            for (saw, (cents, amp)) in saws.iter_mut().zip(&voices) {
                x += amp * saw.next(f0 * scoop * 2f64.powf(cents / 1200.0), sr);
            }
            x = x / 4.0 + 0.6 * sub.next(f0 / 2.0 * scoop, sr);
            let open = (1.0 - (-t / 0.05).exp()) * (0.3 + 0.7 * (-t / 0.8).exp());
            let fc = 180.0 + 3000.0 * open;
            let y = f2.run(f1.run(x, fc, 1.6, sr).low, fc * 1.2, 0.8, sr).low;
            let amp = (1.0 - (-t / 0.02).exp()) * (-t / 2.4).exp();
            drive(y * amp, 2.8)
        })
        .collect();
    fade(&mut out, sr, 0.0, 0.6);
    out
}

/// A rising sweep that peaks at its end: noise through a band-pass climbing
/// five octaves, and a tone climbing with it, trembling faster as it goes.
fn riser(sr: f64, secs: f64, seed: u64) -> Vec<f64> {
    let mut noise = Noise::new(seed);
    let mut bp = Svf::default();
    let (mut tone, mut trem) = (Osc::default(), Osc::default());
    let mut out: Vec<f64> = (0..len(sr, secs))
        .map(|i| {
            let x = i as f64 / sr / secs;
            let swept = bp.run(noise.next(), 250.0 * 32f64.powf(x), 2.0 + 4.0 * x, sr).band;
            let flutter = 1.0 - 0.45 * x * (0.5 + 0.5 * trem.next(3.0 + 14.0 * x, sr));
            let rising = tone.next(110.0 * 16f64.powf(x), sr) * flutter;
            (swept + 0.25 * rising) * x.powf(2.2)
        })
        .collect();
    fade(&mut out, sr, 0.0, 0.015);
    out
}

/// Air rushing past: band-passed noise that sweeps up as it nears and down as
/// it goes.
fn whoosh(sr: f64) -> Vec<f64> {
    let secs = 2.2;
    let peak = WHOOSH_PEAK_SECS / secs;
    let mut noise = Noise::new(31);
    let mut bp = Svf::default();
    let mut out: Vec<f64> = (0..len(sr, secs))
        .map(|i| {
            let x = i as f64 / sr / secs;
            let near = if x < peak { (x / peak).powf(2.0) } else { (-(x - peak) / 0.18).exp() };
            bp.run(noise.next(), 300.0 + 3500.0 * near, 1.5, sr).band * near
        })
        .collect();
    fade(&mut out, sr, 0.0, 0.05);
    out
}

/// The opposite of a riser: noise and a tone falling away.
fn downlifter(sr: f64) -> Vec<f64> {
    let secs = 3.0;
    let mut noise = Noise::new(32);
    let mut bp = Svf::default();
    let mut tone = Osc::default();
    let mut out: Vec<f64> = (0..len(sr, secs))
        .map(|i| {
            let x = i as f64 / sr / secs;
            let swept = bp.run(noise.next(), 6000.0 * (-3.4 * x).exp(), 1.8, sr).band;
            (swept * (-2.5 * x).exp() + 0.3 * tone.next(300.0 * 2f64.powf(-3.0 * x), sr) * (-2.0 * x).exp())
                * (1.0 - (-x * secs / 0.01).exp())
        })
        .collect();
    fade(&mut out, sr, 0.0, 0.1);
    out
}

/// A cinematic hit: a sub drop, a burst of noise closing down, a metallic
/// shimmer and a long low rumble.
fn impact(sr: f64) -> Vec<f64> {
    let mut noise = Noise::new(33);
    let mut osc = Osc::default();
    let (mut burst, mut rumble, mut metal) = (Svf::default(), Svf::default(), Svf::default());
    let mut out: Vec<f64> = (0..len(sr, 6.0))
        .map(|i| {
            let t = i as f64 / sr;
            let n = noise.next();
            let sub = osc.next(38.0 * (1.0 + 2.0 * (-t / 0.03).exp()), sr) * (-t / 1.4).exp();
            let crack = burst.run(n, 150.0 + 4000.0 * (-t / 0.15).exp(), 0.8, sr).low * (-t / 0.25).exp();
            let shimmer = metal.run(n, 6000.0, 0.7, sr).high * 0.12 * (-t / 1.8).exp();
            let low = rumble.run(n, 120.0, 0.7, sr).low * 0.9 * (-t / 2.0).exp();
            drive(sub + 0.7 * crack + shimmer + low, 1.6)
        })
        .collect();
    fade(&mut out, sr, 0.0005, 0.8);
    out
}

/// A glassy granular texture: short, soft grains of the root, fifth, octave,
/// ninth and twelfth, scattered over ten seconds.
fn grains(sr: f64) -> Vec<f64> {
    let secs = 10.0;
    let f0 = made_hz(fx::BOOM + 12 * 2);
    let mut noise = Noise::new(34);
    let mut out = vec![0.0; len(sr, secs)];
    const STEPS: [f64; 7] = [0.0, 7.0, 12.0, 12.0, 14.0, 19.0, 24.0];
    for _ in 0..150 {
        let at = noise.unit() * (secs - 0.4);
        let dur = 0.08 + 0.3 * noise.unit();
        let step = STEPS[(noise.unit() * STEPS.len() as f64) as usize % STEPS.len()];
        let hz = f0 * 2f64.powf((step + 0.06 * noise.next()) / 12.0);
        let drift = 1.0 + 0.004 * noise.next();
        let amp = 0.3 + 0.7 * noise.unit();
        let phase = noise.unit();
        let (a, n) = (len(sr, at), len(sr, dur));
        for k in 0..n.min(out.len() - a) {
            let x = k as f64 / n as f64;
            let window = (PI * x).sin().powi(2);
            let t = k as f64 / sr;
            out[a + k] += amp * window * (TAU * (phase + hz * t * (1.0 + (drift - 1.0) * x))).sin();
        }
    }
    // fade in, hold, then thin away
    let n = out.len();
    for (i, v) in out.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let env = (t / 2.5).min(1.0) * if t > 6.0 { 1.0 - (t - 6.0) / (secs - 6.0) } else { 1.0 };
        *v *= env.max(0.0);
        if i + 1 == n {
            *v = 0.0;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_is_finite_and_ends_quietly() {
        for s in drums(44100.0).into_iter().chain(effects(44100.0)) {
            assert!(s.data.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{}", s.name);
            let peak = s.data.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.1, "{} is silent", s.name);
            if s.looped.is_none() {
                assert!(s.data.last().unwrap().abs() < 1e-3, "{} ends with a click", s.name);
            }
            assert!((s.keys.0..=s.keys.1).contains(&s.root) || s.keys.0 == s.keys.1, "{}", s.name);
        }
    }

    #[test]
    fn the_drone_loops_seamlessly() {
        let fx = effects(48000.0);
        let d = fx.iter().find(|s| s.name == "Sub Drone").unwrap();
        let (a, b) = d.looped.unwrap();
        assert_eq!(d.data[b..b + 8], d.data[a..a + 8]);
        // its pitch: F#1 played on F#1
        assert_eq!(d.root, fx::DRONE + 6);
    }

    #[test]
    fn risers_end_at_their_peak() {
        let fx = effects(44100.0);
        for name in ["Riser", "Long Riser", "Reverse Cymbal"] {
            let s = fx.iter().find(|s| s.name == name).unwrap();
            let n = s.data.len();
            let rms = |a: usize, b: usize| (s.data[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32).sqrt();
            let tenth = n / 10;
            assert!(rms(n - tenth, n) > 3.0 * rms(0, tenth), "{name} doesn't build");
        }
    }
}
