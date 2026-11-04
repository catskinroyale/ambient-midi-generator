//! The stereo mixer. Every part plays through its own engine, without reverb;
//! the mixer sends each part through its channel strip (fader, pan, width,
//! mute and solo), feeds the shared reverb and echo from the strips' sends,
//! and sums everything on the master bus, through the ensemble chorus (all
//! but the bass). It also measures the levels for the meters.

use std::f32::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::reverb::Reverb;
use crate::song::{Echo, Fx, MAX_STRIPS, Mix};
use crate::synth::{SynthEngine, soft_clip};

/// Peak levels since the meters were last read: one per strip, then the
/// master's left and right.
pub struct Meters {
    peaks: [AtomicU32; MAX_STRIPS + 2],
}

impl Meters {
    pub fn new() -> Arc<Meters> {
        Arc::new(Meters { peaks: std::array::from_fn(|_| AtomicU32::new(0)) })
    }

    /// The peak since the last call, which starts the next one.
    pub fn take(&self, i: usize) -> f32 {
        f32::from_bits(self.peaks[i].swap(0, Ordering::Relaxed))
    }

    pub fn master(i: usize) -> usize {
        MAX_STRIPS + i
    }

    fn push(&self, i: usize, peak: f32) {
        // for positive floats the bit patterns sort like the values
        self.peaks[i].fetch_max(peak.to_bits(), Ordering::Relaxed);
    }
}

/// A strip's settings as gains, gliding to new values over a block so faders
/// don't click.
#[derive(Clone, Copy, Default)]
struct Gains {
    /// Left and right out of left, and of right.
    ll: f32,
    lr: f32,
    rl: f32,
    rr: f32,
    reverb: f32,
    echo: f32,
}

impl Gains {
    fn of(mix: &Mix, strip: usize) -> Gains {
        let Some(s) = mix.strips.get(strip) else { return Gains::default() };
        let g = if mix.audible(strip) { s.gain() } else { 0.0 };
        // width: blend each side with the other (0 = mono, 1 = as is, 2 = wider)
        let w = s.width.clamp(0.0, 2.0);
        let same = 0.5 + 0.5 * w;
        let cross = 0.5 - 0.5 * w;
        // pan: turn one side down and move part of it across
        let p = s.pan.clamp(-1.0, 1.0);
        let (keep_l, move_l) = if p > 0.0 { (1.0 - p, p * 0.6) } else { (1.0, 0.0) };
        let (keep_r, move_r) = if p < 0.0 { (1.0 + p, -p * 0.6) } else { (1.0, 0.0) };
        // out_l = keep_l * (same*l + cross*r) + move_r * (same*r + cross*l), and likewise for the right
        Gains {
            ll: g * (keep_l * same + move_r * cross),
            rl: g * (keep_l * cross + move_r * same),
            lr: g * (keep_r * cross + move_l * same),
            rr: g * (keep_r * same + move_l * cross),
            reverb: g * s.reverb.clamp(0.0, 1.0) * 2.0,
            echo: g * s.echo.clamp(0.0, 1.0),
        }
    }

    fn lerp(a: Gains, b: Gains, t: f32) -> Gains {
        let m = |x: f32, y: f32| x + (y - x) * t;
        Gains {
            ll: m(a.ll, b.ll),
            lr: m(a.lr, b.lr),
            rl: m(a.rl, b.rl),
            rr: m(a.rr, b.rr),
            reverb: m(a.reverb, b.reverb),
            echo: m(a.echo, b.echo),
        }
    }
}

struct Part {
    ch: u8,
    engine: Box<dyn SynthEngine>,
    l: Vec<f32>,
    r: Vec<f32>,
}

pub struct Mixer {
    parts: Vec<Part>,
    /// Reverb send (CC91) of each MIDI channel, 0..1.
    sends: [f32; 16],
    mix: Mix,
    bpm: f64,
    /// Gains in use for each MIDI channel's part.
    gains: [Gains; 16],
    master: f32,
    reverb: Reverb,
    echo: EchoFx,
    ensemble: Ensemble,
    duck: f32,
    duck_env: f32,
    duck_attack: f32,
    duck_release: f32,
    meters: Arc<Meters>,
    rev_l: Vec<f32>,
    rev_r: Vec<f32>,
    echo_l: Vec<f32>,
    echo_r: Vec<f32>,
    /// The dry parts that go through the ensemble.
    ens_l: Vec<f32>,
    ens_r: Vec<f32>,
    strip_peaks: [f32; MAX_STRIPS],
}

impl Mixer {
    pub fn new(sample_rate: u32, meters: Arc<Meters>) -> Self {
        let sr = sample_rate as f32;
        let fx = Fx::default();
        Mixer {
            parts: Vec::new(),
            sends: [0.4; 16],
            mix: Mix::default(),
            bpm: 60.0,
            gains: [Gains::default(); 16],
            master: 1.0,
            reverb: Reverb::new(sr, &fx),
            echo: EchoFx::new(sr),
            ensemble: Ensemble::new(sr),
            duck: 0.0,
            duck_env: 0.0,
            duck_attack: 1.0 - (-1.0 / (0.01 * sr)).exp(),
            duck_release: 1.0 - (-1.0 / (0.35 * sr)).exp(),
            meters,
            rev_l: Vec::new(),
            rev_r: Vec::new(),
            echo_l: Vec::new(),
            echo_r: Vec::new(),
            ens_l: Vec::new(),
            ens_r: Vec::new(),
            strip_peaks: [0.0; MAX_STRIPS],
        }
    }

    /// The engine that plays MIDI channel `ch`. It should render without reverb.
    pub fn add_part(&mut self, ch: u8, engine: Box<dyn SynthEngine>) {
        self.parts.retain(|p| p.ch != ch);
        self.parts.push(Part { ch: ch & 15, engine, l: Vec::new(), r: Vec::new() });
        self.update_gains(true);
    }

    fn part(&mut self, ch: u8) -> Option<&mut Part> {
        self.parts.iter_mut().find(|p| p.ch == ch & 15)
    }

    /// The strip a MIDI channel plays through.
    fn strip_of_channel(&self, ch: u8) -> Option<usize> {
        crate::song::Inst::ALL.iter().find(|i| i.channel() == ch).map(|i| self.mix.strip_of(*i))
    }

    fn target_gains(&self, ch: u8) -> Gains {
        self.strip_of_channel(ch).map_or_else(Gains::default, |s| Gains::of(&self.mix, s))
    }

    fn update_gains(&mut self, jump: bool) {
        if jump {
            for ch in 0..16u8 {
                self.gains[ch as usize] = self.target_gains(ch);
            }
            self.master = master_gain(&self.mix);
        }
    }
}

fn master_gain(mix: &Mix) -> f32 {
    if mix.master_db <= crate::song::SILENT_DB { 0.0 } else { 10f32.powf(mix.master_db / 20.0) }
}

impl SynthEngine for Mixer {
    fn note_on(&mut self, ch: u8, key: u8, vel: u8) {
        if let Some(p) = self.part(ch) {
            p.engine.note_on(ch, key, vel);
        }
    }

    fn note_off(&mut self, ch: u8, key: u8) {
        if let Some(p) = self.part(ch) {
            p.engine.note_off(ch, key);
        }
    }

    fn control(&mut self, ch: u8, cc: u8, value: u8) {
        if cc == 91 {
            self.sends[(ch & 15) as usize] = value as f32 / 127.0;
        }
        if let Some(p) = self.part(ch) {
            p.engine.control(ch, cc, value);
        }
    }

    fn program(&mut self, ch: u8, program: u8) {
        if let Some(p) = self.part(ch) {
            p.engine.program(ch, program);
        }
    }

    fn bend(&mut self, ch: u8, cents: f32) {
        if let Some(p) = self.part(ch) {
            p.engine.bend(ch, cents);
        }
    }

    fn reset(&mut self) {
        for p in self.parts.iter_mut() {
            p.engine.reset();
        }
        self.sends = [0.4; 16];
        self.reverb.clear();
        self.echo.clear();
        self.ensemble.clear();
        self.duck_env = 0.0;
    }

    fn set_fx(&mut self, fx: &Fx) {
        for p in self.parts.iter_mut() {
            p.engine.set_fx(fx);
        }
        self.reverb.set(fx);
        self.echo.set(&fx.echo, self.bpm);
        self.duck = fx.duck.clamp(0.0, 1.0);
        self.ensemble.target = fx.ensemble.clamp(0.0, 1.0);
    }

    fn set_mix(&mut self, mix: &Mix, bpm: f64) {
        self.mix = mix.clone();
        if bpm != self.bpm {
            self.bpm = bpm;
            self.echo.set_tempo(bpm);
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        for buf in [&mut self.rev_l, &mut self.rev_r, &mut self.echo_l, &mut self.echo_r, &mut self.ens_l, &mut self.ens_r] {
            buf.clear();
            buf.resize(n, 0.0);
        }
        left.fill(0.0);
        right.fill(0.0);
        self.strip_peaks = [0.0; MAX_STRIPS];
        let inv = 1.0 / n.max(1) as f32;
        for pi in 0..self.parts.len() {
            let ch = self.parts[pi].ch;
            let (from, to) = (self.gains[ch as usize], self.target_gains(ch));
            self.gains[ch as usize] = to;
            let strip = self.strip_of_channel(ch).unwrap_or(0).min(MAX_STRIPS - 1);
            let send = self.sends[ch as usize];
            let part = &mut self.parts[pi];
            part.l.resize(n, 0.0);
            part.r.resize(n, 0.0);
            part.engine.render(&mut part.l, &mut part.r);
            let wet = part.engine.wet_gain() * send;
            let mut peak = self.strip_peaks[strip];
            // the bass stays out of the ensemble, so the low end stays solid
            let chorused = self.ensemble.active() && ch != crate::song::Inst::Cello.channel();
            let (out_l, out_r) = if chorused { (&mut self.ens_l[..], &mut self.ens_r[..]) } else { (&mut *left, &mut *right) };
            for i in 0..n {
                let g = Gains::lerp(from, to, (i + 1) as f32 * inv);
                let (l, r) = (part.l[i], part.r[i]);
                let ol = l * g.ll + r * g.rl;
                let or = l * g.lr + r * g.rr;
                out_l[i] += ol;
                out_r[i] += or;
                self.rev_l[i] += ol * g.reverb * wet;
                self.rev_r[i] += or * g.reverb * wet;
                self.echo_l[i] += ol * g.echo;
                self.echo_r[i] += or * g.echo;
                peak = peak.max(ol.abs()).max(or.abs());
            }
            self.strip_peaks[strip] = peak;
        }
        for (s, p) in self.strip_peaks.iter().enumerate() {
            self.meters.push(s, *p);
        }
        if self.ensemble.active() {
            for i in 0..n {
                let (l, r) = self.ensemble.process(self.ens_l[i], self.ens_r[i]);
                left[i] += l;
                right[i] += r;
            }
        }

        let (m0, m1) = (self.master, master_gain(&self.mix));
        self.master = m1;
        let (mut pl, mut pr) = (0.0f32, 0.0f32);
        for i in 0..n {
            let (el, er) = self.echo.process(self.echo_l[i], self.echo_r[i]);
            let to_rev = self.echo.to_reverb;
            let (wl, wr) = self.reverb.process(self.rev_l[i] + el * to_rev, self.rev_r[i] + er * to_rev);
            // ducking: the reverb dips while the music is loud
            let level = left[i].abs().max(right[i].abs());
            let k = if level > self.duck_env { self.duck_attack } else { self.duck_release };
            self.duck_env += (level - self.duck_env) * k;
            let duck = 1.0 / (1.0 + self.duck * 10.0 * self.duck_env);
            let g = m0 + (m1 - m0) * (i + 1) as f32 * inv;
            left[i] = soft_clip((left[i] + wl * duck + el) * g);
            right[i] = soft_clip((right[i] + wr * duck + er) * g);
            pl = pl.max(left[i].abs());
            pr = pr.max(right[i].abs());
        }
        self.meters.push(Meters::master(0), pl);
        self.meters.push(Meters::master(1), pr);
    }
}

/// A string-machine ensemble: three delays per side, swept by a slow and a
/// fast wobble a third of a cycle apart, so one voice sounds like several
/// slightly out-of-tune ones. Left and right sweep out of step, for width.
struct Ensemble {
    sr: f32,
    lines: [Vec<f32>; 2],
    idx: usize,
    slow: f32,
    fast: f32,
    /// The amount, gliding to `target`.
    amount: f32,
    target: f32,
    lp: [f32; 2],
    lp_coef: f32,
}

impl Ensemble {
    fn new(sr: f32) -> Self {
        let len = (sr * 0.02) as usize;
        Ensemble {
            sr,
            lines: [vec![0.0; len], vec![0.0; len]],
            idx: 0,
            slow: 0.0,
            fast: 0.0,
            amount: 0.0,
            target: 0.0,
            lp: [0.0; 2],
            lp_coef: (-TAU * 7000.0 / sr).exp(),
        }
    }

    /// Off, and not on its way in or out.
    fn active(&self) -> bool {
        self.target > 0.0 || self.amount > 1e-4
    }

    fn clear(&mut self) {
        for l in self.lines.iter_mut() {
            l.fill(0.0);
        }
        self.lp = [0.0; 2];
        self.amount = self.target;
    }

    #[inline]
    fn read(&self, side: usize, d: f32) -> f32 {
        let line = &self.lines[side];
        let n = line.len();
        let pos = self.idx as f32 + n as f32 - d;
        let i = pos as usize;
        let f = pos - i as f32;
        let a = line[i % n];
        let b = line[(i + 1) % n];
        a + (b - a) * f
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.amount += (self.target - self.amount) * 0.0005;
        let n = self.lines[0].len();
        self.lines[0][self.idx] = l;
        self.lines[1][self.idx] = r;
        self.slow = (self.slow + 0.6 / self.sr).fract();
        self.fast = (self.fast + 6.1 / self.sr).fract();
        let a = self.amount;
        let ms = self.sr * 0.001;
        let (base, slow, fast) = (6.0 * ms, (0.6 + 1.6 * a) * ms, (0.1 + 0.15 * a) * ms);
        let mut out = [0.0f32; 2];
        for (s, o) in out.iter_mut().enumerate() {
            let mut sum = 0.0;
            for k in 0..3 {
                let off = k as f32 / 3.0 + s as f32 / 6.0;
                let d = base + slow * ((self.slow + off) * TAU).sin() + fast * ((self.fast + off) * TAU).sin();
                sum += self.read(s, d);
            }
            // the bucket-brigade chips' dull top end
            // (the taps add up mostly out of phase, so scale them by the root of their number)
            self.lp[s] = self.lp_coef * self.lp[s] + (1.0 - self.lp_coef) * sum * 0.577;
            *o = self.lp[s];
        }
        self.idx = (self.idx + 1) % n;
        // the dry level falls as the chorus comes in, keeping the loudness about even
        let (dry, wet) = (1.0 - 0.4 * a, 0.9 * a);
        (l * dry + out[0] * wet, r * dry + out[1] * wet)
    }
}

/// The echo: a stereo, tempo-synced delay with darkening, slightly wobbling
/// repeats, which can bounce from side to side.
struct EchoFx {
    sr: f32,
    lines: [Vec<f32>; 2],
    idx: usize,
    /// Delay in samples, and the value it glides to when the tempo or time changes.
    delay: f32,
    target: f32,
    beats: f64,
    feedback: f32,
    ping_pong: bool,
    lp: [f32; 2],
    lp_coef: f32,
    hp: [f32; 2],
    hp_coef: f32,
    wow: f32,
    phase: f32,
    to_reverb: f32,
}

/// The longest echo, in seconds.
const MAX_ECHO_SECS: f32 = 3.0;

impl EchoFx {
    fn new(sr: f32) -> Self {
        let len = (sr * (MAX_ECHO_SECS + 0.1)) as usize;
        let mut e = EchoFx {
            sr,
            lines: [vec![0.0; len], vec![0.0; len]],
            idx: 0,
            delay: sr * 0.75,
            target: sr * 0.75,
            beats: 0.75,
            feedback: 0.4,
            ping_pong: true,
            lp: [0.0; 2],
            lp_coef: 0.0,
            hp: [0.0; 2],
            hp_coef: (-TAU * 140.0 / sr).exp(),
            wow: 0.0,
            phase: 0.0,
            to_reverb: 0.5,
        };
        e.set(&Echo::default(), 60.0);
        e.delay = e.target;
        e
    }

    fn set(&mut self, echo: &Echo, bpm: f64) {
        self.beats = echo.time.beats();
        self.feedback = echo.feedback.clamp(0.0, 1.0) * 0.88;
        self.ping_pong = echo.ping_pong;
        self.lp_coef = (-TAU * 1500.0 * 8f32.powf(echo.tone.clamp(0.0, 1.0)) / self.sr).exp();
        self.wow = echo.wow.clamp(0.0, 1.0);
        self.to_reverb = echo.to_reverb.clamp(0.0, 1.0);
        self.set_tempo(bpm);
    }

    fn set_tempo(&mut self, bpm: f64) {
        let secs = (self.beats * 60.0 / bpm.max(1.0)) as f32;
        self.target = secs.min(MAX_ECHO_SECS) * self.sr;
    }

    fn clear(&mut self) {
        for l in self.lines.iter_mut() {
            l.fill(0.0);
        }
        self.lp = [0.0; 2];
        self.hp = [0.0; 2];
        self.delay = self.target;
    }

    #[inline]
    fn read(&self, side: usize, d: f32) -> f32 {
        let line = &self.lines[side];
        let n = line.len();
        let pos = self.idx as f32 + n as f32 - d;
        let i = pos as usize;
        let f = pos - i as f32;
        let a = line[i % n];
        let b = line[(i + 1) % n];
        a + (b - a) * f
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        // glide to a new time slowly, like turning a tape echo's speed knob
        self.delay += (self.target - self.delay) * 0.0002;
        self.phase = (self.phase + 0.55 / self.sr).fract();
        let wobble = (self.phase * TAU).sin() * self.wow * 0.004 * self.sr;
        let d = (self.delay + wobble).clamp(1.0, self.lines[0].len() as f32 - 2.0);
        let out = [self.read(0, d), self.read(1, d)];
        let mut fb = [0.0f32; 2];
        for s in 0..2 {
            self.lp[s] = self.lp_coef * self.lp[s] + (1.0 - self.lp_coef) * out[s];
            self.hp[s] = self.hp_coef * self.hp[s] + (1.0 - self.hp_coef) * self.lp[s];
            fb[s] = (self.lp[s] - self.hp[s]) * self.feedback;
        }
        let (in_l, in_r) = if self.ping_pong {
            // a mono input starts on the left; each repeat crosses over
            ((l + r) * 0.5 + fb[1], fb[0])
        } else {
            (l + fb[0], r + fb[1])
        };
        let n = self.lines[0].len();
        self.lines[0][self.idx] = in_l.tanh();
        self.lines[1][self.idx] = in_r.tanh();
        self.idx = (self.idx + 1) % n;
        (out[0], out[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::{Inst, Strip};

    /// Records which channels reached it; plays a constant level.
    struct Probe(Arc<std::sync::Mutex<Vec<u8>>>);

    impl SynthEngine for Probe {
        fn note_on(&mut self, ch: u8, _: u8, _: u8) {
            self.0.lock().unwrap().push(ch);
        }
        fn note_off(&mut self, _: u8, _: u8) {}
        fn control(&mut self, _: u8, _: u8, _: u8) {}
        fn program(&mut self, _: u8, _: u8) {}
        fn reset(&mut self) {}
        fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
            l.fill(0.1);
            r.fill(0.1);
        }
    }

    fn mixer(mix: &Mix) -> (Mixer, Arc<std::sync::Mutex<Vec<u8>>>) {
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut m = Mixer::new(44100, Meters::new());
        for inst in [Inst::Piano, Inst::StringPad] {
            m.add_part(inst.channel(), Box::new(Probe(log.clone())));
        }
        m.set_mix(mix, 60.0);
        m.update_gains(true);
        // no reverb, so only the dry mix is measured
        for ch in 0..2 {
            m.control(ch, 91, 0);
        }
        (m, log)
    }

    fn last(m: &mut Mixer) -> (f32, f32) {
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        m.render(&mut l, &mut r);
        m.render(&mut l, &mut r);
        (l[255], r[255])
    }

    #[test]
    fn routes_channels_and_sums_parts() {
        let (mut m, log) = mixer(&Mix::default());
        for ch in 0..4 {
            m.note_on(ch, 60, 90);
        }
        // channels without a part go nowhere
        assert_eq!(*log.lock().unwrap(), vec![0, 1]);
        let (l, r) = last(&mut m);
        assert!((l - 0.2).abs() < 1e-4 && (r - 0.2).abs() < 1e-4, "{l} {r}");
    }

    #[test]
    fn faders_pans_mutes_and_solos() {
        let mut mix = Mix::default();
        mix.strips[0].gain_db = -6.0206;
        let (mut m, _) = mixer(&mix);
        let (l, _) = last(&mut m);
        assert!((l - 0.15).abs() < 1e-3, "{l}");

        let mut mix = Mix::default();
        mix.strips[0].pan = 1.0;
        mix.strips[1].mute = true;
        let (mut m, _) = mixer(&mix);
        let (l, r) = last(&mut m);
        assert!(l.abs() < 1e-6 && r > 0.1, "hard right: {l} {r}");

        let mut mix = Mix::default();
        mix.strips[1].solo = true;
        let (mut m, _) = mixer(&mix);
        let (l, _) = last(&mut m);
        assert!((l - 0.1).abs() < 1e-4, "solo: {l}");
    }

    /// Two parts sharing a channel move together with its fader.
    #[test]
    fn parts_can_share_a_channel() {
        let mut mix = Mix::default();
        let bus = mix.add_strip().unwrap();
        mix.strips[bus] = Strip { gain_db: crate::song::SILENT_DB, ..Strip::named("Bus") };
        mix.set_route(Inst::Piano, bus);
        mix.set_route(Inst::StringPad, bus);
        let (mut m, _) = mixer(&mix);
        let (l, r) = last(&mut m);
        assert!(l.abs() < 1e-6 && r.abs() < 1e-6);
        // and removing the bus sends them to the first channel
        mix.remove_strip(bus);
        assert_eq!(mix.strip_of(Inst::StringPad), 0);
        assert_eq!((mix.strip_of(Inst::Piano), mix.strips.len()), (0, 5));
    }

    /// With the mixer at its defaults a piece sounds as it did through one
    /// engine with its own reverb: about the same level, with the reverb.
    #[test]
    fn default_mix_matches_a_single_engine() {
        use crate::generator::generate;
        use crate::player::{Sequencer, Timeline};
        use crate::song::Params;
        use crate::synth::BuiltinSynth;
        let song = generate(&Params { target_seconds: 30.0, ..Params::default() });
        let render = |engine: Box<dyn SynthEngine>| {
            let mut seq = Sequencer::new(engine, 44100);
            seq.set_timeline(Arc::new(Timeline::from_song(&song)), 0.0);
            let (mut l, mut r) = (vec![0.0f32; 1024], vec![0.0f32; 1024]);
            let (mut sum, mut n) = (0.0f64, 0usize);
            while seq.render(&mut l, &mut r) {
                for x in l.iter().chain(r.iter()) {
                    assert!(x.is_finite());
                    sum += (*x as f64).powi(2);
                    n += 1;
                }
            }
            10.0 * (sum / n as f64).log10()
        };
        let single = render(Box::new(BuiltinSynth::new(44100)));
        let mut m = Mixer::new(44100, Meters::new());
        for inst in Inst::ALL {
            m.add_part(inst.channel(), Box::new(BuiltinSynth::dry(44100)));
        }
        let mixed = render(Box::new(m));
        println!("one engine {single:.2} dB, mixer {mixed:.2} dB");
        assert!((single - mixed).abs() < 0.5);
    }

    /// The ensemble leaves the bass alone, keeps the level about the same,
    /// and its wet signal differs between left and right.
    #[test]
    fn ensemble_spreads_all_but_the_bass() {
        let sr = 44100.0;
        let mut e = Ensemble::new(sr);
        e.target = 1.0;
        e.amount = 1.0;
        let n = (sr * 2.0) as usize;
        let (mut dry, mut wet, mut diff) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..n {
            let x = (TAU * 440.0 * i as f32 / sr).sin() * 0.5;
            let (l, r) = e.process(x, x);
            if i > n / 4 {
                dry += (x as f64).powi(2);
                wet += ((l * l + r * r) as f64) * 0.5;
                diff += ((l - r) as f64).powi(2);
            }
        }
        let db = 10.0 * (wet / dry).log10();
        println!("ensemble level {db:.2} dB, side/mid {:.3}", diff / dry);
        assert!(db.abs() < 3.0 && diff / dry > 0.01);

        let fx = Fx { ensemble: 1.0, ..Fx::default() };
        let mut mix = Mix::default();
        mix.strips.iter_mut().for_each(|s| s.reverb = 0.0);
        let mut m = Mixer::new(44100, Meters::new());
        m.add_part(Inst::Cello.channel(), Box::new(Probe(Default::default())));
        m.set_mix(&mix, 60.0);
        m.set_fx(&fx);
        let (l, r) = last(&mut m);
        assert!((l - 0.1).abs() < 1e-4 && (r - 0.1).abs() < 1e-4, "bass {l} {r}");
    }

    /// The echo repeats at the beat, getting quieter, and never runs away.
    #[test]
    fn echo_repeats_on_the_beat() {
        let sr = 44100.0;
        let mut e = EchoFx::new(sr);
        e.set(&Echo { feedback: 1.0, ping_pong: false, wow: 0.0, ..Echo::default() }, 120.0);
        e.delay = e.target;
        let n = (sr * 6.0) as usize;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let x = if i < 64 { 0.8 } else { 0.0 };
            out.push(e.process(x, x).0);
        }
        // dotted eighth at 120 bpm is 0.375 s
        let at = (0.375 * sr) as usize;
        let first = out[at - 200..at + 400].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let early = out[..at - 200].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(first > 0.3 && early < 1e-3, "first {first}, early {early}");
        assert!(out.iter().all(|x| x.is_finite() && x.abs() < 1.5));
        let end = out[n - 4410..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(end < first, "end {end}");
    }
}

#[cfg(test)]
mod speed {
    use super::*;
    use crate::generator::generate;
    use crate::player::{Sequencer, Timeline};
    use crate::song::{Inst, Params, Style};
    use crate::synth::SoundFontSynth;

    #[test]
    fn soundfont_mixer_speed() {
        let Ok(mut f) = std::fs::File::open("soundfonts/GeneralUser-GS.sf2") else { return };
        let font = Arc::new(rustysynth::SoundFont::new(&mut f).unwrap());
        let song = generate(&Params { style: Style::CinematicSwell, target_seconds: 60.0, density: 0.8, ..Params::default() });
        let mut m = Mixer::new(44100, Meters::new());
        for inst in Inst::ALL {
            m.add_part(inst.channel(), Box::new(SoundFontSynth::new(&font, 44100).unwrap().dry()));
        }
        m.set_fx(&crate::song::Fx { space: crate::song::Space::Cloud, ..Default::default() });
        let mut seq = Sequencer::new(Box::new(m), 44100);
        seq.set_timeline(Arc::new(Timeline::from_song(&song)), 0.0);
        let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
        let t = std::time::Instant::now();
        let mut n = 0;
        while seq.render(&mut l, &mut r) {
            n += 512;
        }
        let speed = n as f64 / 44100.0 / t.elapsed().as_secs_f64();
        println!("five SoundFont parts through the mixer, cloud reverb: {speed:.0}x realtime");
        assert!(speed > 4.0);
    }
}
