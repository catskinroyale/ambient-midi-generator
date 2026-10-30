//! A stereo algorithmic reverb: pre-delay, early reflections, input diffusion
//! and an eight-line feedback delay network with slowly modulated delays (which
//! keeps the tail smooth rather than metallic) and high frequencies that die
//! away faster than lows. Five spaces share the design, from a small room to a
//! huge, swirling "black hole"; the shimmer space pitches its tail up an octave.

use std::f32::consts::{PI, TAU};

use crate::song::{Fx, Space};
/// Delay line lengths in milliseconds, chosen to avoid common factors.
const LINES_MS: [f32; 8] = [37.1, 41.9, 47.3, 53.9, 61.7, 67.1, 73.9, 83.3];
/// Early reflections: (milliseconds, gain, side).
const TAPS: [(f32, f32, usize); 8] = [
    (7.3, 0.62, 0),
    (11.1, 0.55, 1),
    (16.9, 0.47, 0),
    (21.7, 0.42, 1),
    (29.3, 0.35, 0),
    (34.1, 0.31, 1),
    (43.7, 0.24, 0),
    (51.1, 0.2, 1),
];
const DIFFUSE_MS: [[f32; 4]; 2] = [[4.77, 3.59, 12.73, 9.31], [4.31, 3.97, 11.83, 8.93]];

struct Delay {
    buf: Vec<f32>,
    idx: usize,
}

impl Delay {
    fn new(len: usize) -> Self {
        Delay { buf: vec![0.0; len.max(2)], idx: 0 }
    }

    #[inline]
    fn push(&mut self, x: f32) {
        self.buf[self.idx] = x;
        self.idx = (self.idx + 1) % self.buf.len();
    }

    /// The sample written `d` samples ago (fractional, linear interpolation).
    #[inline]
    fn read(&self, d: f32) -> f32 {
        let n = self.buf.len();
        let pos = self.idx as f32 + n as f32 - d;
        let i = pos as usize;
        let f = pos - i as f32;
        let a = self.buf[i % n];
        let b = self.buf[(i + 1) % n];
        a + (b - a) * f
    }

    #[inline]
    fn tap(&self, d: usize) -> f32 {
        let n = self.buf.len();
        self.buf[(self.idx + n - d.min(n - 1)) % n]
    }
}

/// Schroeder allpass used for diffusion.
struct Allpass {
    buf: Vec<f32>,
    idx: usize,
    g: f32,
}

impl Allpass {
    fn new(len: usize, g: f32) -> Self {
        Allpass { buf: vec![0.0; len.max(1)], idx: 0, g }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let d = self.buf[self.idx];
        let v = x + d * self.g;
        self.buf[self.idx] = v;
        self.idx = (self.idx + 1) % self.buf.len();
        d - v * self.g
    }
}

/// The fixed character of each space; `Fx` scales it.
struct Preset {
    /// Delay line length multiplier.
    scale: f32,
    /// Natural RT60 in seconds.
    decay: f32,
    pre_ms: f32,
    early: f32,
    /// Modulation depth in milliseconds.
    depth_ms: f32,
    /// Modulation speed multiplier.
    rate: f32,
    damp: f32,
    diffuse_scale: f32,
    diffuse_g: f32,
    /// Octave-up feedback level (shimmer).
    shimmer: f32,
    /// Output level, which balances the louder long tails against short ones.
    wet: f32,
}

fn preset(space: Space) -> Preset {
    match space {
        Space::Room => Preset {
            scale: 0.5,
            decay: 1.0,
            pre_ms: 6.0,
            early: 0.4,
            depth_ms: 0.25,
            rate: 1.0,
            damp: 0.4,
            diffuse_scale: 0.6,
            diffuse_g: 0.65,
            shimmer: 0.0,
            wet: 1.1,
        },
        Space::Hall => Preset {
            scale: 1.25,
            decay: 3.2,
            pre_ms: 22.0,
            early: 0.25,
            depth_ms: 0.45,
            rate: 1.0,
            damp: 0.32,
            diffuse_scale: 1.0,
            diffuse_g: 0.72,
            shimmer: 0.0,
            wet: 1.0,
        },
        Space::Cathedral => Preset {
            scale: 2.0,
            decay: 6.5,
            pre_ms: 45.0,
            early: 0.15,
            depth_ms: 0.8,
            rate: 0.8,
            damp: 0.42,
            diffuse_scale: 1.5,
            diffuse_g: 0.74,
            shimmer: 0.0,
            wet: 0.95,
        },
        Space::Shimmer => Preset {
            scale: 1.6,
            decay: 7.0,
            pre_ms: 30.0,
            early: 0.1,
            depth_ms: 0.9,
            rate: 0.8,
            damp: 0.3,
            diffuse_scale: 1.2,
            diffuse_g: 0.74,
            shimmer: 0.3,
            wet: 0.8,
        },
        Space::BlackHole => Preset {
            scale: 3.2,
            decay: 24.0,
            pre_ms: 0.0,
            early: 0.0,
            depth_ms: 3.0,
            rate: 0.45,
            damp: 0.2,
            diffuse_scale: 3.0,
            diffuse_g: 0.78,
            shimmer: 0.0,
            wet: 0.65,
        },
    }
}

struct Line {
    delay: Delay,
    len: f32,
    gain: f32,
    lp: f32,
    phase: f32,
    rate: f32,
}

/// Pitch shifter: two read heads sweeping through a short delay, crossfaded
/// with Hann windows.
struct Shifter {
    delay: Delay,
    win: f32,
    phase: f32,
    hp: f32,
    hp_coef: f32,
    /// Pitch ratio: 2 is an octave up.
    ratio: f32,
}

impl Shifter {
    fn new(sr: f32, win_ms: f32) -> Self {
        let win = win_ms * 0.001 * sr;
        Shifter {
            delay: Delay::new(win as usize + 4),
            win,
            phase: 0.0,
            hp: 0.0,
            hp_coef: (-TAU * 300.0 / sr).exp(),
            ratio: 2.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        self.hp = self.hp_coef * self.hp + (1.0 - self.hp_coef) * x;
        self.delay.push(x - self.hp);
        // the read point gains (ratio - 1) samples per sample on the write: at 2, twice the pitch
        self.phase += (self.ratio - 1.0) / self.win;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let mut out = 0.0;
        for k in 0..2 {
            let ph = (self.phase + k as f32 * 0.5).fract();
            let w = (PI * ph).sin();
            out += self.delay.read(1.0 + (1.0 - ph) * self.win) * w * w;
        }
        out
    }
}

pub struct Reverb {
    sr: f32,
    space: Space,
    pre: [Delay; 2],
    pre_len: usize,
    hp: [f32; 2],
    hp_coef: f32,
    diffuse: [Vec<Allpass>; 2],
    lines: Vec<Line>,
    damp: f32,
    depth: f32,
    tone: [f32; 2],
    tone_coef: f32,
    taps: Vec<(usize, f32, usize)>,
    early: f32,
    /// Two octave-up shifters: the first alone when the feedback is in tune;
    /// detuned, one goes sharp and the other flat, feeding different lines.
    shimmer: Option<[Shifter; 2]>,
    shimmer_gain: f32,
    /// How much of the shimmer goes through the second (flat) shifter.
    shimmer_mix: f32,
    wet: f32,
}

impl Reverb {
    pub fn new(sr: f32, fx: &Fx) -> Self {
        let p = preset(fx.space);
        let ms = |m: f32| m * 0.001 * sr;
        let pre_len = ms(p.pre_ms) as usize;
        let tap_scale = p.scale / 1.25;
        let longest_tap = ms(TAPS[TAPS.len() - 1].0 * tap_scale) as usize;
        // room for the deepest modulation the Motion control allows
        let max_depth = ms(p.depth_ms) * 2.0;
        let lines = LINES_MS
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let len = ms(*m * p.scale);
                Line {
                    delay: Delay::new((len + max_depth) as usize + 4),
                    len,
                    gain: 0.0,
                    lp: 0.0,
                    phase: i as f32 / 8.0,
                    rate: 0.0,
                }
            })
            .collect();
        let diffuse = DIFFUSE_MS.map(|side| {
            side.iter()
                .enumerate()
                .map(|(i, m)| Allpass::new(ms(*m * p.diffuse_scale) as usize, if i < 2 { p.diffuse_g } else { p.diffuse_g - 0.1 }))
                .collect()
        });
        let mut r = Reverb {
            sr,
            space: fx.space,
            pre: [Delay::new(pre_len + longest_tap + 2), Delay::new(pre_len + longest_tap + 2)],
            pre_len,
            hp: [0.0; 2],
            hp_coef: (-TAU * 160.0 / sr).exp(),
            diffuse,
            lines,
            damp: p.damp,
            depth: 0.0,
            tone: [0.0; 2],
            tone_coef: 0.0,
            taps: TAPS.iter().map(|(m, g, side)| (ms(*m * tap_scale) as usize, *g, *side)).collect(),
            early: p.early,
            shimmer: (p.shimmer > 0.0).then(|| [Shifter::new(sr, 60.0), Shifter::new(sr, 47.0)]),
            shimmer_gain: p.shimmer,
            shimmer_mix: 0.0,
            wet: p.wet,
        };
        r.set(fx);
        r
    }

    /// Apply new settings. Size, tone and motion change smoothly while sound
    /// plays; a new space rebuilds the reverb (and clears its tail).
    pub fn set(&mut self, fx: &Fx) {
        if fx.space != self.space {
            *self = Reverb::new(self.sr, fx);
            return;
        }
        let p = preset(fx.space);
        let decay = p.decay * 2f32.powf((fx.size - 0.5) * 2.0);
        let rate = p.rate * (0.5 + fx.motion);
        let sr = self.sr;
        for (i, line) in self.lines.iter_mut().enumerate() {
            line.gain = 10f32.powf(-3.0 * line.len / (sr * decay));
            line.rate = (0.11 + 0.07 * i as f32) * rate / sr;
        }
        self.depth = p.depth_ms * 0.001 * sr * 2.0 * fx.motion;
        // Tone: the damping in the loop and the colour of the output.
        self.damp = (p.damp + (0.5 - fx.tone) * 0.5).clamp(0.05, 0.85);
        self.tone_coef = (-TAU * 3000.0 * 2f32.powf(fx.tone * 2.6) / sr).exp();
        // Longer tails build up more; keep the overall level about the same.
        self.wet = p.wet * (p.decay / decay).powf(0.35);
        // Shimmer: the feedback level, and the two shifters pulled apart by up
        // to 35 cents either side of the octave.
        // the default (0.5) is the space's natural level; the top end is about three times it
        self.shimmer_gain = p.shimmer * (2.0 * fx.shimmer).powf(1.6);
        let cents = 35.0 * fx.shimmer_detune;
        self.shimmer_mix = (fx.shimmer_detune * 4.0).min(1.0) * 0.5;
        if let Some([a, b]) = &mut self.shimmer {
            a.ratio = 2.0 * 2f32.powf(cents / 1200.0);
            b.ratio = 2.0 * 2f32.powf(-cents / 1200.0);
        }
    }

    pub fn clear(&mut self) {
        for d in self.pre.iter_mut() {
            d.buf.fill(0.0);
        }
        for side in self.diffuse.iter_mut() {
            for a in side.iter_mut() {
                a.buf.fill(0.0);
            }
        }
        for l in self.lines.iter_mut() {
            l.delay.buf.fill(0.0);
            l.lp = 0.0;
        }
        for s in self.shimmer.iter_mut().flatten() {
            s.delay.buf.fill(0.0);
            s.hp = 0.0;
        }
        self.hp = [0.0; 2];
        self.tone = [0.0; 2];
    }

    /// One stereo sample in, the wet signal out.
    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        // High-pass the send so low notes don't muddy the tail.
        let mut x = [l, r];
        for s in 0..2 {
            self.hp[s] = self.hp_coef * self.hp[s] + (1.0 - self.hp_coef) * x[s];
            x[s] -= self.hp[s];
            self.pre[s].push(x[s]);
        }
        let mut early = [0.0f32; 2];
        if self.early > 0.0 {
            for &(d, g, side) in &self.taps {
                early[side] += self.pre[side].tap(self.pre_len + d) * g;
            }
        }
        let mut input = [self.pre[0].tap(self.pre_len), self.pre[1].tap(self.pre_len)];
        for s in 0..2 {
            for a in self.diffuse[s].iter_mut() {
                input[s] = a.process(input[s]);
            }
        }

        // Read each line, with a slowly wandering delay time, and damp it.
        let mut y = [0.0f32; 8];
        for (i, line) in self.lines.iter_mut().enumerate() {
            line.phase += line.rate;
            if line.phase >= 1.0 {
                line.phase -= 1.0;
            }
            let wobble = (line.phase * TAU).sin() * self.depth;
            let v = line.delay.read(line.len + wobble);
            line.lp += (1.0 - self.damp) * (v - line.lp);
            if line.lp.abs() < 1e-20 {
                line.lp = 0.0;
            }
            y[i] = line.lp * line.gain;
        }
        let out_l = y[0] - y[1] + y[2] - y[3] + y[4] - y[5] + y[6] - y[7];
        let out_r = y[0] + y[1] - y[2] - y[3] + y[4] + y[5] - y[6] - y[7];

        // Shimmer: an octave-up copy of the tail is fed back in.
        let (sharp, flat) = match &mut self.shimmer {
            Some([a, b]) => {
                // a soft ceiling keeps strong feedback from running away
                let x = (y[0] + y[3] + y[5] + y[6]) * 0.5;
                let x = if x.abs() < 0.5 { x } else { x.signum() * (0.5 + 0.5 * ((x.abs() - 0.5) / 0.5).tanh()) };
                let g = self.shimmer_gain;
                let sharp = a.process(x) * g * (1.0 - self.shimmer_mix);
                let flat = if self.shimmer_mix > 0.0 { b.process(x) * g * self.shimmer_mix } else { 0.0 };
                (sharp, flat)
            }
            None => (0.0, 0.0),
        };

        // Mix the lines with a Hadamard matrix (lossless), then feed back.
        hadamard8(&mut y);
        let norm = 1.0 / 8f32.sqrt();
        for (i, line) in self.lines.iter_mut().enumerate() {
            let sh = match i % 3 {
                0 => sharp,
                1 => flat,
                _ => 0.0,
            };
            line.delay.push(y[i] * norm + input[i & 1] * 0.5 + sh);
        }

        let mut wet = [
            (out_l * 0.35 + early[0] * self.early) * self.wet,
            (out_r * 0.35 + early[1] * self.early) * self.wet,
        ];
        for s in 0..2 {
            self.tone[s] = self.tone_coef * self.tone[s] + (1.0 - self.tone_coef) * wet[s];
            wet[s] = self.tone[s];
        }
        (wet[0], wet[1])
    }
}

#[inline]
fn hadamard8(v: &mut [f32; 8]) {
    let mut h = 1;
    while h < 8 {
        for i in (0..8).step_by(h * 2) {
            for j in i..i + h {
                let (a, b) = (v[j], v[j + h]);
                v[j] = a + b;
                v[j + h] = a - b;
            }
        }
        h *= 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An impulse decays smoothly at about the requested rate, without blowing up.
    #[test]
    fn decays_at_the_right_rate() {
        let sr = 44100.0;
        let mut r = Reverb::new(sr, &Fx { size: 0.5 + 0.5 * (3.0f32 / 3.2).log2(), ..Fx::default() });
        let n = (sr * 4.0) as usize;
        let mut env = Vec::new();
        let (mut acc, mut count) = (0.0f64, 0);
        for i in 0..n {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let (a, b) = r.process(x, x);
            assert!(a.is_finite() && b.is_finite());
            acc += (a as f64).powi(2) + (b as f64).powi(2);
            count += 1;
            if count == 4410 {
                env.push(10.0 * (acc / count as f64).log10());
                acc = 0.0;
                count = 0;
            }
        }
        // from 0.5 s to 2.5 s the level should fall about 40 dB (60 dB per 3 s)
        let drop = env[5] - env[25];
        println!("decay envelope (dB per 100 ms): {env:.1?}; 0.5-2.5 s drop {drop:.1} dB");
        assert!((30.0..55.0).contains(&drop), "drop {drop}");
        // and the tail is smooth: no 100 ms block jumps by more than 3 dB
        assert!(env[3..35].windows(2).all(|w| (w[1] - w[0]).abs() < 3.0));
    }

    /// Full shimmer feedback, detuned or not, stays bounded and dies away.
    #[test]
    fn strong_shimmer_is_stable() {
        let sr = 44100.0;
        for detune in [0.0, 0.5, 1.0] {
            let fx = Fx { space: Space::Shimmer, size: 1.0, tone: 1.0, shimmer: 1.0, shimmer_detune: detune, ..Fx::default() };
            let mut r = Reverb::new(sr, &fx);
            let mut seed = 7u32;
            let (mut peak, mut on, mut end) = (0.0f32, 0.0f64, 0.0f64);
            let n = (sr * 60.0) as usize;
            for i in 0..n {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let x = if i < sr as usize * 3 { seed as f32 / u32::MAX as f32 - 0.5 } else { 0.0 };
                let (a, b) = r.process(x, -x);
                assert!(a.is_finite() && b.is_finite());
                peak = peak.max(a.abs()).max(b.abs());
                if i < sr as usize * 3 {
                    on += (a as f64).powi(2);
                } else if i > n - sr as usize {
                    end += (a as f64).powi(2);
                }
            }
            let (on, end) = (10.0 * (on / (sr as f64 * 3.0)).log10(), 10.0 * (end / sr as f64 + 1e-30).log10());
            println!("shimmer detune {detune}: on {on:.1} dB, last second {end:.1} dB, peak {peak:.2}");
            assert!(peak < 2.0 && end < on - 30.0, "detune {detune} runs away");
        }
    }

    /// Every space stays finite and decays away; longer spaces take longer.
    #[test]
    fn every_space_is_stable() {
        let sr = 44100.0;
        let mut tails = Vec::new();
        for space in Space::ALL {
            for (size, motion) in [(0.5, 0.5), (1.0, 1.0), (0.0, 0.0)] {
                let mut r = Reverb::new(sr, &Fx { space, size, motion, tone: 1.0, ..Fx::default() });
                // two seconds of loud noise, then silence
                let mut seed = 1u32;
                let mut peak = 0.0f32;
                let mut level = |secs: f32, input: bool, r: &mut Reverb| {
                    let mut acc = 0.0f64;
                    let n = (sr * secs) as usize;
                    for _ in 0..n {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        let x = if input { seed as f32 / u32::MAX as f32 - 0.5 } else { 0.0 };
                        let (a, b) = r.process(x, -x);
                        assert!(a.is_finite() && b.is_finite(), "{space:?} blew up");
                        peak = peak.max(a.abs()).max(b.abs());
                        acc += (a as f64).powi(2);
                    }
                    10.0 * (acc / n as f64 + 1e-30).log10()
                };
                let on = level(2.0, true, &mut r);
                let tail = level(2.0, false, &mut r);
                let late = level(56.0, false, &mut r);
                let end = level(1.0, false, &mut r);
                println!("{space:?} size {size} motion {motion}: on {on:.1} dB, tail {tail:.1}, end {end:.1}, peak {peak:.2}");
                assert!(peak < 2.0, "{space:?} peak {peak}");
                assert!(end < on - 30.0 && late < on, "{space:?} doesn't decay");
                if size == 0.5 {
                    tails.push((space, on, tail - on));
                }
            }
        }
        // longer spaces hold on longer after the input stops
        let drop = |s: Space| tails.iter().find(|t| t.0 == s).unwrap().2;
        assert!(drop(Space::Room) < drop(Space::Hall) && drop(Space::Hall) < drop(Space::Cathedral));
        assert!(drop(Space::Cathedral) < drop(Space::BlackHole));
    }
}
