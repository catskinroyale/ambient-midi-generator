//! Synthesises the bundled "Ambient Electronic" SoundFont to soundfonts/Ambient-Electronic.sf2.
//! Every sample is built here from sine partials (plus a little seeded noise), so the bank
//! contains no third-party material and ships under the project's MIT licence.
//! Run with: cargo run --release --example make_soundfont
//! Check with: cargo test --release --example make_soundfont -- --nocapture
//!
//! Sustained sounds loop without clicks because each looped sample is exactly periodic:
//! every partial completes a whole number of cycles per loop, and the small pitch error
//! that rounding leaves is put back in the sample header's pitch correction.

use std::f64::consts::{PI, TAU};

/// The score kit's drums and effects, shared with the app's built-in synth.
#[path = "../src/kit.rs"]
#[allow(dead_code)]
mod kit;

const SR: f64 = 44100.0;
/// Highest partial synthesised: a sample played 5 semitones above its root stays under ~0.45·sr.
const MAX_HZ: f64 = 14_500.0;
/// Peak level of every sample, about -3 dBFS.
const PEAK: f64 = 0.708;
/// Periodic samples copied either side of a loop, so interpolating players read valid data.
const GUARD: usize = 8;
/// Pad loop: 1.5 s, long enough that detuned voices beat slowly rather than warble.
const PAD_LOOP: usize = 66_150;
/// Keys and bass loop: 0.5 s. These are plain harmonic series, so nothing needs to beat.
const SHORT_LOOP: usize = 22_050;
const OUT_PATH: &str = "soundfonts/Ambient-Electronic.sf2";

// SoundFont generator numbers (SF2.01 section 8.1.2).
const INITIAL_FC: u16 = 8;
const FILTER_Q: u16 = 9;
const MOD_LFO_TO_FC: u16 = 10;
const MOD_ENV_TO_FC: u16 = 11;
const VIB_LFO_TO_PITCH: u16 = 6;
const DELAY_MOD_LFO: u16 = 21;
const FREQ_MOD_LFO: u16 = 22;
const DELAY_VIB_LFO: u16 = 23;
const FREQ_VIB_LFO: u16 = 24;
const ATTACK_MOD_ENV: u16 = 26;
const DECAY_MOD_ENV: u16 = 28;
const SUSTAIN_MOD_ENV: u16 = 29;
const RELEASE_MOD_ENV: u16 = 30;
const ATTACK_VOL: u16 = 34;
const DECAY_VOL: u16 = 36;
const SUSTAIN_VOL: u16 = 37;
const RELEASE_VOL: u16 = 38;
const KEYNUM_TO_VOL_DECAY: u16 = 40;
const INSTRUMENT: u16 = 41;
const KEY_RANGE: u16 = 43;
const VEL_RANGE: u16 = 44;
const ATTENUATION: u16 = 48;
const SAMPLE_ID: u16 = 53;
const SAMPLE_MODES: u16 = 54;
const ROOT_KEY: u16 = 58;

fn midi_hz(key: f64) -> f64 {
    440.0 * 2f64.powf((key - 69.0) / 12.0)
}

/// Seconds as SoundFont timecents.
fn tc(secs: f64) -> i16 {
    (1200.0 * secs.log2()).round() as i16
}

/// Hz as SoundFont absolute cents (relative to 8.176 Hz).
fn abs_cents(hz: f64) -> i16 {
    (1200.0 * (hz / 8.176).log2()).round() as i16
}

/// Magnitude response of a low-pass of the given order (2 = 12 dB/oct, 4 = 24 dB/oct).
fn lowpass(hz: f64, fc: f64, order: i32) -> f64 {
    1.0 / (1.0 + (hz / fc).powi(order)).sqrt()
}

/// Decay that reaches exactly zero (with zero slope) at `end` seconds, so a transient is
/// gone by the time the loop starts.
fn gate(t: f64, tau: f64, end: f64) -> f64 {
    if t >= end { 0.0 } else { (-t / tau).exp() * 0.5 * (1.0 + (PI * t / end).cos()) }
}

/// Deterministic noise (splitmix64), so regenerating gives a byte-identical file.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The fundamental for `root` as a whole number of cycles per `len` samples, plus the cents
/// that pull the rounded pitch back in tune.
fn fit(root: u8, len: usize) -> (usize, i8) {
    let f0 = midi_hz(root as f64);
    let k = (f0 * len as f64 / SR).round().max(1.0);
    let cents = 1200.0 * (f0 / (k * SR / len as f64)).log2();
    (k as usize, cents.round() as i8)
}

#[derive(Clone, Copy)]
struct Partial {
    bin: usize, // cycles per loop
    amp: f64,
    phase: f64, // 0..1
}

/// Sums partials over one loop. Phases come from an integer table index, so the result
/// repeats exactly every `len` samples.
fn periodic(len: usize, partials: &[Partial]) -> Vec<f64> {
    let table: Vec<f64> = (0..len).map(|i| (TAU * i as f64 / len as f64).sin()).collect();
    let mut out = vec![0.0; len];
    for p in partials {
        let step = p.bin % len;
        let mut idx = (p.phase * len as f64) as usize % len;
        for o in out.iter_mut() {
            *o += p.amp * table[idx];
            idx += step;
            if idx >= len {
                idx -= len;
            }
        }
    }
    out
}

/// Detuned copies of a harmonic series: `voices` holds (cents, gain) and `shape(n, hz)` sets
/// harmonic n's level. Each harmonic rounds to its own bin, so a detuned voice drifts very
/// slightly against itself, which only adds to the chorus.
fn stack(
    k: usize,
    len: usize,
    voices: &[(f64, f64)],
    shape: &dyn Fn(usize, f64) -> f64,
    rng: &mut Rng,
) -> Vec<Partial> {
    let mut out = Vec::new();
    for &(cents, gain) in voices {
        let ratio = 2f64.powf(cents / 1200.0);
        for n in 1.. {
            let bin = (n as f64 * k as f64 * ratio).round() as usize;
            if bin as f64 * SR / len as f64 > MAX_HZ {
                break;
            }
            let amp = gain * shape(n, bin as f64 * SR / len as f64);
            if amp > 1e-5 {
                out.push(Partial { bin, amp, phase: rng.next() });
            }
        }
    }
    out
}

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
}

struct Sample {
    name: String,
    data: Vec<i16>,
    looped: Option<(usize, usize)>,
    root: u8,
    cents: i8,
}

fn note_name(root: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[root as usize % 12], root as i32 / 12 - 1)
}

fn quantise(x: &[f64]) -> Vec<i16> {
    let peak = x.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-9);
    x.iter().map(|v| (v * PEAK / peak * 32767.0).round() as i16).collect()
}

/// An optional one-shot `head` followed by one period, `cycle`, which loops. The head must
/// run continuously into the cycle.
fn looped(name: &str, root: u8, cents: i8, head: &[f64], cycle: &[f64]) -> Sample {
    let mut data = Vec::with_capacity(head.len() + cycle.len() + 2 * GUARD);
    if head.is_empty() {
        data.extend_from_slice(&cycle[cycle.len() - GUARD..]);
    } else {
        data.extend_from_slice(head);
    }
    let start = data.len();
    data.extend_from_slice(cycle);
    data.extend_from_slice(&cycle[..GUARD]);
    Sample {
        name: format!("{name} {}", note_name(root)),
        data: quantise(&data),
        looped: Some((start, start + cycle.len())),
        root,
        cents,
    }
}

fn one_shot(name: &str, root: u8, mut data: Vec<f64>) -> Sample {
    // A short fade so the tail ends on silence.
    let fade = (0.08 * SR) as usize;
    let n = data.len();
    for i in 0..fade {
        data[n - 1 - i] *= i as f64 / fade as f64;
    }
    Sample { name: format!("{name} {}", note_name(root)), data: quantise(&data), looped: None, root, cents: 0 }
}

// ---- The sounds ------------------------------------------------------------------------

/// Soft sine keys: a steady sine with a little 2nd/3rd harmonic, struck with brighter
/// harmonics that fade over the first second (before the loop starts).
fn sine_keys(root: u8) -> Sample {
    let len = SHORT_LOOP;
    let head = 2 * len;
    let (k, cents) = fit(root, len);
    let f = k as f64 * SR / len as f64;
    let steady = periodic(len, &[
        Partial { bin: k, amp: 1.0, phase: 0.0 },
        Partial { bin: 2 * k, amp: 0.18, phase: 0.0 },
        Partial { bin: 3 * k, amp: 0.06, phase: 0.0 },
    ]);
    let mut full: Vec<f64> = (0..head + len).map(|n| steady[n % len]).collect();
    let end = head as f64 / SR;
    for (ratio, amp, tau) in [(2.0, 0.35, 0.35), (3.0, 0.2, 0.25), (4.0, 0.12, 0.15), (6.8, 0.08, 0.06)] {
        if ratio * f > MAX_HZ {
            continue;
        }
        for (n, s) in full[..head].iter_mut().enumerate() {
            let t = n as f64 / SR;
            *s += amp * gate(t, tau, end) * (TAU * ratio * f * t).sin();
        }
    }
    looped("SineKeys", root, cents, &full[..head], &full[head..])
}

/// Two-operator FM tine (1:1), with a brief high "tine" partial. The modulation index falls
/// to a fixed value by the loop start, so the tail is a static periodic wave.
fn fm_piano(root: u8, bright: bool) -> Sample {
    let len = SHORT_LOOP;
    let head = 2 * len;
    let (k, cents) = fit(root, len);
    let f = k as f64 * SR / len as f64;
    let (i0, i_sus, tine) = if bright { (2.8, 0.75, 0.22) } else { (1.2, 0.3, 0.07) };
    let end = head as f64 / SR;
    let tine = if 14.0 * f < MAX_HZ { tine } else { 0.0 };
    let full: Vec<f64> = (0..head + len)
        .map(|n| {
            let t = n as f64 / SR;
            let ph = TAU * ((k * n) % len) as f64 / len as f64;
            let index = i_sus + (i0 - i_sus) * gate(t, 0.45, end);
            (ph + index * ph.sin()).sin() + tine * gate(t, 0.04, end) * (14.0 * ph).sin()
        })
        .collect();
    let name = if bright { "FMEP bright" } else { "FMEP soft" };
    looped(name, root, cents, &full[..head], &full[head..])
}

/// Sine with a touch of in-phase 2nd/3rd harmonic, as if gently saturated.
fn sub_bass(root: u8) -> Sample {
    let (k, cents) = fit(root, SHORT_LOOP);
    let cycle = periodic(SHORT_LOOP, &[
        Partial { bin: k, amp: 1.0, phase: 0.0 },
        Partial { bin: 2 * k, amp: 0.22, phase: 0.0 },
        Partial { bin: 3 * k, amp: 0.06, phase: 0.0 },
    ]);
    looped("SubBass", root, cents, &[], &cycle)
}

/// Two slightly detuned saws through a low-pass whose cutoff falls quickly after the pluck.
fn pluck(root: u8) -> Sample {
    let f0 = midi_hz(root as f64);
    let n = (1.5 * SR) as usize;
    let mut rng = Rng(0x5EED_0045 ^ root as u64);
    let mut out = vec![0.0; n];
    let cutoff: Vec<f64> = (0..n).map(|i| 2.0 * f0 + 4500.0 * (-(i as f64 / SR) / 0.18).exp()).collect();
    let env: Vec<f64> = (0..n).map(|i| (-(i as f64 / SR) * 3.2).exp()).collect();
    for cents in [-4.0, 4.0] {
        let f = f0 * 2f64.powf(cents / 1200.0);
        let mut h = 1;
        while h as f64 * f < MAX_HZ {
            let hz = h as f64 * f;
            let (w, ph) = (TAU * hz / SR, TAU * rng.next());
            for (i, o) in out.iter_mut().enumerate() {
                *o += lowpass(hz, cutoff[i], 4) * env[i] / h as f64 * (w * i as f64 + ph).sin();
            }
            h += 1;
        }
    }
    one_shot("Pluck", root, out)
}

/// A Chowning-style FM bell (1:1.4) plus the inharmonic modes of a struck bar.
fn bells(root: u8) -> Sample {
    let f = midi_hz(root as f64);
    let secs = 7.0 - (root as f64 - 36.0) / 12.0 * 0.7;
    let n = (secs * SR) as usize;
    let tau = secs / 7.0; // about -60 dB by the end
    // Keep the FM sidebands below MAX_HZ.
    let i0 = ((0.8 * MAX_HZ / f - 1.0) / 1.4 - 1.0).clamp(0.3, 4.0);
    let modes = [(1.0, 0.5, 1.0), (2.756, 0.35, 0.6), (5.404, 0.2, 0.35), (8.933, 0.1, 0.2)];
    let out = (0..n)
        .map(|i| {
            let t = i as f64 / SR;
            let index = i0 * (-t / 1.2).exp();
            let mut y = 0.6 * (TAU * f * t + index * (TAU * 1.4 * f * t).sin()).sin() * (-t / tau).exp();
            for (ratio, amp, scale) in modes {
                if ratio * f < MAX_HZ {
                    y += amp * (TAU * ratio * f * t).sin() * (-t / (tau * scale)).exp();
                }
            }
            y
        })
        .collect();
    one_shot("Bells", root, out)
}

/// A looped pad from one or more detuned stacks: each entry is (voices, shape).
fn pad(name: &str, root: u8, layers: &[(&[(f64, f64)], &dyn Fn(usize, f64) -> f64)]) -> Sample {
    let (k, cents) = fit(root, PAD_LOOP);
    let mut rng = Rng(name.bytes().fold(root as u64, |h, b| h.wrapping_mul(131).wrapping_add(b as u64)));
    let mut parts = Vec::new();
    for (voices, shape) in layers {
        parts.extend(stack(k, PAD_LOOP, voices, *shape, &mut rng));
    }
    looped(name, root, cents, &[], &periodic(PAD_LOOP, &parts))
}

/// Vowel formants ("aah", rounded a little towards "oh"), plus some chest-voice body.
fn formant(hz: f64) -> f64 {
    let peaks = [(680.0, 1.0, 130.0), (1120.0, 0.55, 150.0), (2600.0, 0.22, 220.0), (3300.0, 0.1, 280.0)];
    let mut g = 0.02 + 0.5 / (1.0 + (hz / 300.0).powi(2));
    for (fc, a, bw) in peaks {
        g += a / (1.0 + ((hz - fc) / bw).powi(2));
    }
    g
}

/// A small ensemble of saws through vowel formants, with a breath of noise. The noise is
/// also built from whole-cycle partials, so it loops as cleanly as the voices.
fn choir(root: u8) -> Sample {
    let (k, cents) = fit(root, PAD_LOOP);
    let mut rng = Rng(0xC401_0091 ^ root as u64);
    let voices = [(-12.0, 1.0), (-4.0, 1.0), (5.0, 1.0), (13.0, 1.0)];
    let shape = |n: usize, hz: f64| formant(hz) * lowpass(hz, 6000.0, 2) / n as f64;
    let voice = periodic(PAD_LOOP, &stack(k, PAD_LOOP, &voices, &shape, &mut rng));
    let lo = (800.0 * PAD_LOOP as f64 / SR) as usize;
    let hi = (7000.0 * PAD_LOOP as f64 / SR) as usize;
    let breath: Vec<Partial> = (lo..hi)
        .map(|bin| Partial { bin, amp: formant(bin as f64 * SR / PAD_LOOP as f64), phase: rng.next() })
        .collect();
    let breath = periodic(PAD_LOOP, &breath);
    let mix = 0.06 * rms(&voice) / rms(&breath);
    let cycle: Vec<f64> = voice.iter().zip(&breath).map(|(v, b)| v + mix * b).collect();
    looped("Choir", root, cents, &[], &cycle)
}

/// A reed's resonances ("oo" towards "ah"), on a falling slope: the body of a
/// saxophone-like lead.
fn reed(hz: f64) -> f64 {
    let peaks = [(520.0, 1.0, 140.0), (1450.0, 0.55, 260.0), (2700.0, 0.22, 400.0)];
    let mut g = 0.12 / (1.0 + (hz / 1800.0).powi(2));
    for (fc, a, bw) in peaks {
        g += a / (1.0 + ((hz - fc) / bw).powi(2));
    }
    g
}

/// Two saws a few cents apart through reed resonances, with a breath of noise,
/// both looping cleanly like the choir.
fn smoky_lead(root: u8) -> Sample {
    let (k, cents) = fit(root, PAD_LOOP);
    let mut rng = Rng(0x5A0C_0066 ^ root as u64);
    let f0 = midi_hz(root as f64);
    // the odd harmonics a little stronger, for a hollow, reedy edge
    let shape = move |n: usize, hz: f64| {
        let odd = if n % 2 == 1 { 1.0 } else { 0.7 };
        odd * (0.35 + reed(hz)) * lowpass(hz, 5000f64.max(6.0 * f0), 2) / n as f64
    };
    let voice = periodic(PAD_LOOP, &stack(k, PAD_LOOP, &[(-3.0, 1.0), (3.0, 0.8)], &shape, &mut rng));
    let lo = (900.0 * PAD_LOOP as f64 / SR) as usize;
    let hi = (6000.0 * PAD_LOOP as f64 / SR) as usize;
    let breath: Vec<Partial> = (lo..hi)
        .map(|bin| Partial { bin, amp: reed(bin as f64 * SR / PAD_LOOP as f64) + 0.1, phase: rng.next() })
        .collect();
    let breath = periodic(PAD_LOOP, &breath);
    let mix = 0.05 * rms(&voice) / rms(&breath);
    let cycle: Vec<f64> = voice.iter().zip(&breath).map(|(v, b)| v + mix * b).collect();
    looped("SmokyLead", root, cents, &[], &cycle)
}

// ---- Bank layout -----------------------------------------------------------------------

struct Zone {
    keys: (u8, u8),
    vels: (u8, u8),
    sample: usize,
    /// Generators for this zone alone, over the instrument's.
    gens: Vec<(u16, i16)>,
}

struct Instrument {
    name: &'static str,
    bank: u16,
    program: u16,
    gens: Vec<(u16, i16)>,
    zones: Vec<Zone>,
}

#[derive(Default)]
struct Bank {
    samples: Vec<Sample>,
    instruments: Vec<Instrument>,
}

impl Bank {
    /// One preset/instrument with a zone per root; each root's zone runs from 6 semitones
    /// below it to 5 above, and the outermost zones stretch to the ends of the keyboard.
    fn add(
        &mut self,
        name: &'static str,
        program: u16,
        gens: Vec<(u16, i16)>,
        roots: &[u8],
        make: impl Fn(u8) -> Vec<((u8, u8), Sample)>,
    ) {
        let mut zones = Vec::new();
        for (i, &root) in roots.iter().enumerate() {
            let lo = if i == 0 { 0 } else { root - 6 };
            let hi = if i + 1 == roots.len() { 127 } else { root + 5 };
            for (vels, sample) in make(root) {
                self.samples.push(sample);
                zones.push(Zone { keys: (lo, hi), vels, sample: self.samples.len() - 1, gens: Vec::new() });
            }
        }
        self.instruments.push(Instrument { name, bank: 0, program, gens, zones });
    }

    /// A preset of the score kit's sounds, each on its own keys. Its samples
    /// keep the kit's levels rather than being normalised.
    fn add_kit(&mut self, name: &'static str, bank: u16, program: u16, gens: Vec<(u16, i16)>, sounds: Vec<kit::Sound>) {
        let mut zones = Vec::new();
        for s in sounds {
            let data: Vec<i16> = s.data.iter().map(|v| (*v as f64 * PEAK * 32767.0).round() as i16).collect();
            // a sustained sound swells in and dies away slowly
            let gens = if s.looped.is_some() { env(s.fades.0, s.fades.1) } else { Vec::new() };
            self.samples.push(Sample { name: s.name.to_string(), data, looped: s.looped, root: s.root, cents: 0 });
            zones.push(Zone { keys: s.keys, vels: (0, 127), sample: self.samples.len() - 1, gens });
        }
        self.instruments.push(Instrument { name, bank, program, gens, zones });
    }
}

fn roots(lo: u8, hi: u8) -> Vec<u8> {
    (lo..=hi).step_by(12).collect()
}

fn single(s: Sample) -> Vec<((u8, u8), Sample)> {
    vec![((0, 127), s)]
}

/// Amplitude envelope: attack and release in seconds, sustaining at full level.
fn env(attack: f64, release: f64) -> Vec<(u16, i16)> {
    vec![(ATTACK_VOL, tc(attack)), (RELEASE_VOL, tc(release))]
}

/// Initial attenuation in centibels. rustysynth (like Polyphone) applies only 40% of it.
fn atten(cb: i16) -> (u16, i16) {
    (ATTENUATION, cb)
}

fn design() -> Bank {
    let mut bank = Bank::default();
    let keys = |decay: f64, release: f64, cb: i16| {
        let mut g = env(0.004, release);
        g.extend([(DECAY_VOL, tc(decay)), (SUSTAIN_VOL, 1000), (KEYNUM_TO_VOL_DECAY, 20), atten(cb)]);
        g
    };

    bank.add("Sine Keys", 0, keys(6.0, 0.4, 0), &roots(24, 96), |r| single(sine_keys(r)));
    bank.add("FM Electric Piano", 4, keys(5.0, 0.35, 0), &roots(24, 96), |r| {
        vec![((0, 84), fm_piano(r, false)), ((85, 127), fm_piano(r, true))]
    });
    bank.add("Analog Sub Bass", 38, [env(0.005, 0.12), vec![atten(150)]].concat(), &roots(24, 60), |r| {
        single(sub_bass(r))
    });
    bank.add("Analog Pluck", 45, [env(0.002, 0.25), vec![atten(0)]].concat(), &roots(36, 96), |r| {
        single(pluck(r))
    });

    let saw = |fc: f64| move |n: usize, hz: f64| lowpass(hz, fc, 4) / n as f64;
    let triangle = |n: usize, _: f64| if n % 2 == 1 { 1.0 / (n * n) as f64 } else { 0.0 };
    let fundamental = |n: usize, _: f64| if n == 1 { 1.0 } else { 0.0 };

    let strings = saw(4500.0);
    let supersaw = [(-28.0, 0.8), (-17.0, 0.8), (-7.0, 0.8), (0.0, 1.0), (7.0, 0.8), (17.0, 0.8), (28.0, 0.8)];
    bank.add("Supersaw Strings", 50, [env(0.6, 1.5), vec![atten(0)]].concat(), &roots(36, 96), |r| {
        single(pad("Supersaw", r, &[(&supersaw, &strings)]))
    });

    let lead = saw(3200.0);
    let mut lead_gens = env(0.015, 0.3);
    lead_gens.extend([(DELAY_VIB_LFO, tc(0.5)), (FREQ_VIB_LFO, abs_cents(5.0)), (VIB_LFO_TO_PITCH, 10), atten(70)]);
    bank.add("Soft Saw Lead", 81, lead_gens, &roots(36, 96), |r| {
        single(pad("SawLead", r, &[(&[(-5.0, 1.0), (5.0, 1.0)], &lead)]))
    });

    // Baked-in darkness, then the synth's own filter breathes slowly with the mod LFO.
    let mut warm_gens = env(1.5, 3.0);
    warm_gens.extend([
        (INITIAL_FC, abs_cents(1400.0)),
        (FREQ_MOD_LFO, abs_cents(0.15)),
        (DELAY_MOD_LFO, tc(0.3)),
        (MOD_LFO_TO_FC, 1000),
        atten(30),
    ]);
    bank.add("Warm Analog Pad", 88, warm_gens, &roots(24, 84), |r| {
        let f0 = midi_hz(r as f64);
        let shape = move |n: usize, hz: f64| lowpass(hz, 1100f64.max(2.0 * f0), 4) / n as f64;
        single(pad("WarmPad", r, &[(&[(-11.0, 1.0), (-4.0, 1.0), (4.0, 1.0), (11.0, 1.0)], &shape)]))
    });

    let glass = |n: usize, hz: f64| match n {
        2 => 0.12,
        4 => 0.05,
        _ => triangle(n, hz),
    };
    bank.add("Glass Pad", 89, [env(1.0, 3.0), vec![atten(140)]].concat(), &roots(36, 96), |r| {
        single(pad("GlassPad", r, &[
            (&[(-4.0, 1.0), (4.0, 1.0)], &glass),
            (&[(1197.0, 0.2), (1203.0, 0.2)], &fundamental),
        ]))
    });

    let mut choir_gens = env(0.8, 2.0);
    choir_gens.extend([(DELAY_VIB_LFO, tc(0.6)), (FREQ_VIB_LFO, abs_cents(4.6)), (VIB_LFO_TO_PITCH, 5), atten(50)]);
    bank.add("Airy Choir", 91, choir_gens, &roots(36, 84), |r| single(choir(r)));

    bank.add("Halo Pad", 94, [env(2.0, 4.0), vec![atten(0)]].concat(), &roots(24, 84), |r| {
        let f0 = midi_hz(r as f64);
        let base = move |n: usize, hz: f64| lowpass(hz, 1600f64.max(3.0 * f0), 4) / (n as f64).powf(1.2);
        let shimmer = |n: usize, hz: f64| if n <= 5 { triangle(n, hz) } else { 0.0 };
        single(pad("HaloPad", r, &[
            (&[(-7.0, 1.0), (0.0, 1.0), (7.0, 1.0)], &base),
            (&[(1195.0, 0.3), (1205.0, 0.3), (2396.0, 0.15), (2404.0, 0.15)], &shimmer),
        ]))
    });

    // A bright sample behind a resonant filter that the mod envelope opens over ~6 s.
    let mut sweep_gens = env(1.0, 3.0);
    sweep_gens.extend([
        (INITIAL_FC, abs_cents(350.0)),
        (FILTER_Q, 60),
        (ATTACK_MOD_ENV, tc(6.0)),
        (DECAY_MOD_ENV, tc(8.0)),
        (SUSTAIN_MOD_ENV, 500),
        (RELEASE_MOD_ENV, tc(3.0)),
        (MOD_ENV_TO_FC, 4800),
        atten(0),
    ]);
    let bright = |n: usize, hz: f64| lowpass(hz, 10_000.0, 2) / n as f64;
    bank.add("Sweep Pad", 95, sweep_gens, &roots(24, 84), |r| {
        single(pad("SweepPad", r, &[(&[(-16.0, 1.0), (-6.0, 1.0), (6.0, 1.0), (16.0, 1.0)], &bright)]))
    });

    // The CS-80's brass: bright, slightly detuned saws (and one an octave down)
    // behind a resonant filter that swells open as the note starts and then
    // settles, with vibrato creeping in on held chords.
    let mut brass_gens = env(0.12, 1.2);
    brass_gens.extend([
        (INITIAL_FC, abs_cents(480.0)),
        (FILTER_Q, 40),
        (ATTACK_MOD_ENV, tc(0.45)),
        (DECAY_MOD_ENV, tc(2.5)),
        (SUSTAIN_MOD_ENV, 450),
        (RELEASE_MOD_ENV, tc(1.2)),
        (MOD_ENV_TO_FC, 3900),
        (DELAY_VIB_LFO, tc(1.2)),
        (FREQ_VIB_LFO, abs_cents(5.5)),
        (VIB_LFO_TO_PITCH, 6),
        atten(40),
    ]);
    bank.add("CS Brass", 62, brass_gens, &roots(36, 84), |r| {
        let full = |n: usize, hz: f64| lowpass(hz, 9000.0, 2) / n as f64;
        single(pad("CSBrass", r, &[(&[(-7.0, 1.0), (6.0, 1.0), (-1197.0, 0.35)], &full)]))
    });

    // A dark stab: detuned saws and an octave below, behind a filter that
    // snaps open and shuts again at once, dying away like a struck chord.
    let mut stab_gens = env(0.004, 0.9);
    stab_gens.extend([
        (DECAY_VOL, tc(1.4)),
        (SUSTAIN_VOL, 600),
        (INITIAL_FC, abs_cents(260.0)),
        (FILTER_Q, 30),
        (ATTACK_MOD_ENV, tc(0.003)),
        (DECAY_MOD_ENV, tc(0.35)),
        (SUSTAIN_MOD_ENV, 1000),
        (MOD_ENV_TO_FC, 3400),
        atten(20),
    ]);
    bank.add("Dark Stab", 63, stab_gens, &roots(24, 84), |r| {
        let full = |n: usize, hz: f64| lowpass(hz, 8000.0, 2) / n as f64;
        single(pad("DarkStab", r, &[(&[(-9.0, 1.0), (8.0, 1.0), (-1200.0, 0.6)], &full)]))
    });

    // A lead for slow, smoky tunes: it breathes in, brightens a little as it's
    // blown, and its vibrato arrives late.
    let mut smoky_gens = env(0.07, 0.6);
    smoky_gens.extend([
        (INITIAL_FC, abs_cents(1800.0)),
        (ATTACK_MOD_ENV, tc(0.2)),
        (DECAY_MOD_ENV, tc(1.2)),
        (SUSTAIN_MOD_ENV, 350),
        (MOD_ENV_TO_FC, 2400),
        (DELAY_VIB_LFO, tc(0.4)),
        (FREQ_VIB_LFO, abs_cents(5.2)),
        (VIB_LFO_TO_PITCH, 12),
        atten(60),
    ]);
    bank.add("Smoky Lead", 66, smoky_gens, &roots(36, 84), |r| single(smoky_lead(r)));

    bank.add("Crystal Bells", 98, [env(0.001, 1.5), vec![atten(0)]].concat(), &roots(36, 96), |r| {
        single(bells(r))
    });

    // The score kit: trailer drums in the drum bank, at the Orchestra kit's
    // place, and the trailer effects at General MIDI's Reverse Cymbal. Their
    // long releases let a hit ring on after its note ends.
    bank.add_kit("Trailer Kit", 128, kit::KIT_PROGRAM as u16, env(0.001, 1.5), kit::drums(SR));
    bank.add_kit("Trailer FX", 0, kit::FX_PROGRAM as u16, env(0.001, 6.0), kit::effects(SR));
    bank
}

// ---- RIFF writer -----------------------------------------------------------------------

fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = id.to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn list(kind: &[u8; 4], chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut body = kind.to_vec();
    chunks.iter().for_each(|c| body.extend_from_slice(c));
    chunk(b"LIST", &body)
}

/// Zero-terminated string padded to an even length, as INFO sub-chunks require.
fn zstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    if v.len() % 2 == 1 {
        v.push(0);
    }
    v
}

fn name20(s: &str) -> [u8; 20] {
    assert!(s.len() <= 20, "name too long: {s}");
    let mut out = [0u8; 20];
    out[..s.len()].copy_from_slice(s.as_bytes());
    out
}

fn gen_record(out: &mut Vec<u8>, op: u16, amount: i16) {
    out.extend(op.to_le_bytes());
    out.extend(amount.to_le_bytes());
}

fn range(lo: u8, hi: u8) -> i16 {
    u16::from_le_bytes([lo, hi]) as i16
}

fn write_sf2(bank: &Bank) -> Vec<u8> {
    let info = list(b"INFO", &[
        chunk(b"ifil", &[2, 0, 1, 0]),
        chunk(b"isng", &zstr("EMU8000")),
        chunk(b"INAM", &zstr("Ambient Electronic")),
        chunk(b"IENG", &zstr("catskinroyale")),
        chunk(b"ICOP", &zstr("Copyright (c) 2026 catskinroyale. MIT License.")),
        chunk(b"ICMT", &zstr(
            "Synthesised entirely by examples/make_soundfont.rs in the midigen repository. \
             Contains no third-party samples.",
        )),
        chunk(b"ISFT", &zstr("midigen make_soundfont")),
    ]);

    // Each sample is followed by 46 zero samples, as the spec requires.
    let mut smpl = Vec::new();
    let mut shdr = Vec::new();
    let mut pos = 0u32;
    for s in &bank.samples {
        smpl.extend(s.data.iter().flat_map(|v| v.to_le_bytes()));
        smpl.extend([0u8; 46 * 2]);
        let end = pos + s.data.len() as u32;
        let (ls, le) = match s.looped {
            Some((a, b)) => (pos + a as u32, pos + b as u32),
            None => (end - 40, end - 8), // unused, but kept valid and inside the sample
        };
        shdr.extend(name20(&s.name));
        for v in [pos, end, ls, le, SR as u32] {
            shdr.extend(v.to_le_bytes());
        }
        shdr.push(s.root);
        shdr.push(s.cents as u8);
        shdr.extend(0u16.to_le_bytes()); // sample link
        shdr.extend(1u16.to_le_bytes()); // mono
        pos = end + 46;
    }
    shdr.extend(name20("EOS"));
    shdr.extend([0u8; 26]);
    let sdta = list(b"sdta", &[chunk(b"smpl", &smpl)]);

    let (mut phdr, mut pbag, mut pgen) = (Vec::new(), Vec::new(), Vec::new());
    let (mut inst, mut ibag, mut igen) = (Vec::new(), Vec::new(), Vec::new());
    let mut ibag_count = 0u16;
    for (i, ins) in bank.instruments.iter().enumerate() {
        phdr.extend(name20(ins.name));
        phdr.extend(ins.program.to_le_bytes());
        phdr.extend(ins.bank.to_le_bytes());
        phdr.extend((i as u16).to_le_bytes());
        phdr.extend([0u8; 12]); // library, genre, morphology
        pbag.extend(((i as u16).to_le_bytes()).iter().chain(&0u16.to_le_bytes()));
        gen_record(&mut pgen, INSTRUMENT, i as i16);

        inst.extend(name20(ins.name));
        inst.extend(ibag_count.to_le_bytes());
        // Global zone: shared envelope, filter and level settings.
        ibag.extend(((igen.len() / 4) as u16).to_le_bytes());
        ibag.extend(0u16.to_le_bytes());
        ibag_count += 1;
        for &(op, amount) in &ins.gens {
            gen_record(&mut igen, op, amount);
        }
        for z in &ins.zones {
            let s = &bank.samples[z.sample];
            ibag.extend(((igen.len() / 4) as u16).to_le_bytes());
            ibag.extend(0u16.to_le_bytes());
            ibag_count += 1;
            gen_record(&mut igen, KEY_RANGE, range(z.keys.0, z.keys.1));
            gen_record(&mut igen, VEL_RANGE, range(z.vels.0, z.vels.1));
            for &(op, amount) in &z.gens {
                gen_record(&mut igen, op, amount);
            }
            gen_record(&mut igen, ROOT_KEY, s.root as i16);
            gen_record(&mut igen, SAMPLE_MODES, s.looped.is_some() as i16);
            gen_record(&mut igen, SAMPLE_ID, z.sample as i16);
        }
    }
    let n = bank.instruments.len() as u16;
    phdr.extend(name20("EOP"));
    phdr.extend([0u8; 4]);
    phdr.extend(n.to_le_bytes());
    phdr.extend([0u8; 12]);
    pbag.extend(n.to_le_bytes().iter().chain(&0u16.to_le_bytes()));
    gen_record(&mut pgen, 0, 0);
    inst.extend(name20("EOI"));
    inst.extend(ibag_count.to_le_bytes());
    ibag.extend(((igen.len() / 4) as u16).to_le_bytes());
    ibag.extend(0u16.to_le_bytes());
    gen_record(&mut igen, 0, 0);

    let pdta = list(b"pdta", &[
        chunk(b"phdr", &phdr),
        chunk(b"pbag", &pbag),
        chunk(b"pmod", &[0u8; 10]),
        chunk(b"pgen", &pgen),
        chunk(b"inst", &inst),
        chunk(b"ibag", &ibag),
        chunk(b"imod", &[0u8; 10]),
        chunk(b"igen", &igen),
        chunk(b"shdr", &shdr),
    ]);

    let mut body = b"sfbk".to_vec();
    for part in [info, sdta, pdta] {
        body.extend(part);
    }
    chunk(b"RIFF", &body)
}

fn build() -> Vec<u8> {
    write_sf2(&design())
}

fn main() {
    let bytes = build();
    std::fs::create_dir_all("soundfonts").expect("create soundfonts/");
    std::fs::write(OUT_PATH, &bytes).expect("write soundfont");
    println!("wrote {OUT_PATH} ({:.1} MB)", bytes.len() as f64 / 1e6);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
    use std::sync::Arc;

    /// (bank, program, name, keys to try)
    const EXPECTED: [(i32, i32, &str, [i32; 2]); 17] = [
        (0, 0, "Sine Keys", MELODIC),
        (0, 4, "FM Electric Piano", MELODIC),
        (0, 38, "Analog Sub Bass", MELODIC),
        (0, 45, "Analog Pluck", MELODIC),
        (0, 50, "Supersaw Strings", MELODIC),
        (0, 62, "CS Brass", MELODIC),
        (0, 63, "Dark Stab", MELODIC),
        (0, 66, "Smoky Lead", MELODIC),
        (0, 81, "Soft Saw Lead", MELODIC),
        (0, 88, "Warm Analog Pad", MELODIC),
        (0, 89, "Glass Pad", MELODIC),
        (0, 91, "Airy Choir", MELODIC),
        (0, 94, "Halo Pad", MELODIC),
        (0, 95, "Sweep Pad", MELODIC),
        (0, 98, "Crystal Bells", MELODIC),
        (0, 119, "Trailer FX", [48, 67]),
        (128, 48, "Trailer Kit", [36, 41]),
    ];
    const MELODIC: [i32; 2] = [48, 72];

    /// Holds `key` for 3 s then releases it for 2 s; returns (peak, held RMS, max step in
    /// the steady part of the held section).
    fn play(sf: &Arc<SoundFont>, bank: i32, program: i32, key: i32) -> (f32, f32, f32) {
        let mut settings = SynthesizerSettings::new(44100);
        settings.enable_reverb_and_chorus = false;
        let mut synth = Synthesizer::new(sf, &settings).unwrap();
        // the drum bank plays on the drum channel
        let ch = if bank == 128 { 9 } else { 0 };
        synth.process_midi_message(ch, 0xC0, program, 0);
        let (held, tail) = (3 * 44100, 2 * 44100);
        let (mut l, mut r) = (vec![0.0f32; held + tail], vec![0.0f32; held + tail]);
        synth.note_on(ch, key, 100);
        synth.render(&mut l[..held], &mut r[..held]);
        synth.note_off(ch, key);
        synth.render(&mut l[held..], &mut r[held..]);
        let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(l.iter().chain(&r).all(|v| v.is_finite()));
        let rms = (l[..held].iter().map(|v| v * v).sum::<f32>() / held as f32).sqrt();
        let step = l[44100..held].windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
        (peak, rms, step)
    }

    #[test]
    fn loads_and_plays() {
        let bytes = build();
        if let Ok(on_disk) = std::fs::read(OUT_PATH) {
            assert!(on_disk == bytes, "{OUT_PATH} is stale or generation is not deterministic");
        }
        println!("size: {} bytes", bytes.len());
        let sf = Arc::new(SoundFont::new(&mut bytes.as_slice()).unwrap());
        let info = sf.get_info();
        assert_eq!(info.get_bank_name(), "Ambient Electronic");
        assert_eq!(sf.get_presets().len(), EXPECTED.len());
        for (bank, program, name, keys) in EXPECTED {
            let p = sf
                .get_presets()
                .iter()
                .find(|p| p.get_patch_number() == program && p.get_bank_number() == bank)
                .unwrap();
            assert_eq!(p.get_name(), name);
            for key in keys {
                let (peak, rms, step) = play(&sf, bank, program, key);
                println!("{program:3} {name:18} key {key}: peak {peak:.3} rms {rms:.4} max step {step:.4}");
                assert!(peak > 0.02 && peak < 1.0, "{name} key {key}: peak {peak}");
            }
        }

        // Loops: the data just past the loop end repeats the loop start exactly, and the step
        // across the seam is no bigger than the steps inside the loop.
        let wave = sf.get_wave_data();
        let mut worst: f32 = 0.0;
        let mut looped = 0;
        for h in sf.get_sample_headers() {
            let (ls, le) = (h.get_start_loop() as usize, h.get_end_loop() as usize);
            if le - ls < 1000 {
                continue; // a one-shot sample
            }
            looped += 1;
            assert_eq!(wave[le..le + GUARD], wave[ls..ls + GUARD], "{}", h.get_name());
            let seam = (wave[ls] as i32 - wave[le - 1] as i32).abs();
            let inner = wave[ls..le].windows(2).map(|w| (w[1] as i32 - w[0] as i32).abs()).max().unwrap();
            worst = worst.max(seam as f32 / inner as f32);
        }
        println!("{looped} looped samples; worst seam step / max in-loop step = {worst:.3}");
        assert!(worst <= 1.0);

        // Tuning: the sub bass is near-sine, so rising zero crossings give its pitch, both at a
        // root and between roots.
        for key in [36, 45, 50] {
            let mut settings = SynthesizerSettings::new(44100);
            settings.enable_reverb_and_chorus = false;
            let mut synth = Synthesizer::new(&sf, &settings).unwrap();
            synth.process_midi_message(0, 0xC0, 38, 0);
            synth.note_on(0, key, 100);
            let (mut l, mut r) = (vec![0.0f32; 3 * 44100], vec![0.0f32; 3 * 44100]);
            synth.render(&mut l, &mut r);
            let crossings: Vec<f64> = (44100..l.len() - 1)
                .filter(|&i| l[i] <= 0.0 && l[i + 1] > 0.0)
                .map(|i| i as f64 + (l[i] / (l[i] - l[i + 1])) as f64)
                .collect();
            let hz = (crossings.len() - 1) as f64 * 44100.0 / (crossings.last().unwrap() - crossings[0]);
            let error = 1200.0 * (hz / midi_hz(key as f64)).log2();
            println!("sub bass key {key}: {hz:.3} Hz ({error:+.2} cents)");
            assert!(error.abs() < 2.0);
        }
    }
}
