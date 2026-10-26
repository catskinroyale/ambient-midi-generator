//! Sequencing a `Song` into synth events, real-time playback through cpal, and
//! offline rendering to WAV.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::song::Song;
use crate::synth::SynthEngine;

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Program(u8),
    Ctrl(u8, u8),
    On(u8, u8),
    Off(u8),
}

#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub time: f64,
    pub ch: u8,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug)]
struct Span {
    start: f64,
    end: f64,
    ch: u8,
    key: u8,
    vel: u8,
}

pub struct Timeline {
    events: Vec<Event>,
    spans: Vec<Span>,
    pub duration: f64,
}

impl Timeline {
    pub fn from_song(song: &Song) -> Self {
        let mut events = Vec::new();
        for inst in song.instruments() {
            events.push(Event { time: 0.0, ch: inst.channel(), kind: Kind::Program(song.program(inst)) });
        }
        for c in &song.ctrls {
            events.push(Event { time: song.tick_to_secs(c.tick), ch: c.inst.channel(), kind: Kind::Ctrl(c.cc, c.value) });
        }
        let mut spans = Vec::new();
        for n in &song.notes {
            let start = song.tick_to_secs(n.start);
            let end = song.tick_to_secs(n.end());
            let ch = n.inst.channel();
            events.push(Event { time: start, ch, kind: Kind::On(n.key, n.vel) });
            events.push(Event { time: end, ch, kind: Kind::Off(n.key) });
            if crate::song::program_sustains(song.program(n.inst)) {
                spans.push(Span { start, end, ch, key: n.key, vel: n.vel });
            }
        }
        let order = |k: &Kind| match k {
            Kind::Program(_) => 0,
            Kind::Off(_) => 1,
            Kind::Ctrl(..) => 2,
            Kind::On(..) => 3,
        };
        events.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap().then(order(&a.kind).cmp(&order(&b.kind))));
        // allow reverb tails to finish
        let duration = song.duration_secs() + 3.0;
        Timeline { events, spans, duration }
    }
}

/// Plays a timeline through an engine, sample-accurately to 64-sample blocks.
pub struct Sequencer {
    pub engine: Box<dyn SynthEngine>,
    pub timeline: Option<Arc<Timeline>>,
    idx: usize,
    pub pos: f64,
    sr: f64,
}

const BLOCK: usize = 64;

impl Sequencer {
    pub fn new(engine: Box<dyn SynthEngine>, sr: u32) -> Self {
        Sequencer { engine, timeline: None, idx: 0, pos: 0.0, sr: sr as f64 }
    }

    pub fn set_timeline(&mut self, tl: Arc<Timeline>, at: f64) {
        self.timeline = Some(tl);
        self.seek(at);
    }

    /// Jump to `t` seconds: reset the engine, replay programs/controllers up to
    /// that point and re-start any sustained string notes that span it.
    pub fn seek(&mut self, t: f64) {
        self.engine.reset();
        self.pos = t.max(0.0);
        self.idx = 0;
        let Some(tl) = self.timeline.clone() else { return };
        while self.idx < tl.events.len() && tl.events[self.idx].time < self.pos {
            let e = tl.events[self.idx];
            match e.kind {
                Kind::Program(p) => self.engine.program(e.ch, p),
                Kind::Ctrl(c, v) => self.engine.control(e.ch, c, v),
                _ => {}
            }
            self.idx += 1;
        }
        if self.pos > 0.0 {
            for s in tl.spans.iter().filter(|s| s.start < self.pos && s.end > self.pos + 0.1) {
                self.engine.note_on(s.ch, s.key, s.vel);
            }
        }
    }

    /// Render into the buffers, advancing the playhead. Returns false once the
    /// end of the timeline has been reached.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) -> bool {
        let n = left.len();
        let mut off = 0;
        while off < n {
            let len = BLOCK.min(n - off);
            let block_end = self.pos + len as f64 / self.sr;
            if let Some(tl) = &self.timeline {
                while self.idx < tl.events.len() && tl.events[self.idx].time < block_end {
                    let e = tl.events[self.idx];
                    match e.kind {
                        Kind::Program(p) => self.engine.program(e.ch, p),
                        Kind::Ctrl(c, v) => self.engine.control(e.ch, c, v),
                        Kind::On(k, v) => self.engine.note_on(e.ch, k, v),
                        Kind::Off(k) => self.engine.note_off(e.ch, k),
                    }
                    self.idx += 1;
                }
            }
            self.engine.render(&mut left[off..off + len], &mut right[off..off + len]);
            self.pos = block_end;
            off += len;
        }
        match &self.timeline {
            Some(tl) => self.pos < tl.duration,
            None => false,
        }
    }
}

// ------------------------------------------------------------- live player

pub enum Cmd {
    SetTimeline(Arc<Timeline>, f64),
    SetEngine(Box<dyn SynthEngine>),
    Play,
    Pause,
    Seek(f64),
    Volume(f32),
    Looping(bool),
}

pub struct Player {
    tx: Sender<Cmd>,
    pos_bits: Arc<AtomicU64>,
    playing: Arc<AtomicBool>,
    pub sample_rate: u32,
    _stream: cpal::Stream,
}

impl Player {
    pub fn new(make_engine: impl FnOnce(u32) -> Box<dyn SynthEngine>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("No audio output device found")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let sample_rate = config.sample_rate;
        let channels = config.channels as usize;

        let (tx, rx) = channel::<Cmd>();
        let pos_bits = Arc::new(AtomicU64::new(0f64.to_bits()));
        let playing = Arc::new(AtomicBool::new(false));
        let mut state = AudioState {
            seq: Sequencer::new(make_engine(sample_rate), sample_rate),
            rx,
            pos_bits: pos_bits.clone(),
            playing: playing.clone(),
            volume: 0.9,
            looping: false,
            left: vec![0.0; 4096],
            right: vec![0.0; 4096],
        };
        let err_fn = |e| eprintln!("audio stream error: {e}");
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    state.fill(data, channels, |x| x)
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    state.fill(data, channels, |x| (x.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                },
                err_fn,
                None,
            ),
            other => return Err(format!("Unsupported output sample format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Player { tx, pos_bits, playing, sample_rate, _stream: stream })
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn position(&self) -> f64 {
        f64::from_bits(self.pos_bits.load(Ordering::Relaxed))
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
}

struct AudioState {
    seq: Sequencer,
    rx: Receiver<Cmd>,
    pos_bits: Arc<AtomicU64>,
    playing: Arc<AtomicBool>,
    volume: f32,
    looping: bool,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl AudioState {
    fn fill<T: Copy>(&mut self, data: &mut [T], channels: usize, conv: impl Fn(f32) -> T) {
        while let Ok(cmd) = self.rx.try_recv() {
            match cmd {
                Cmd::SetTimeline(tl, at) => self.seq.set_timeline(tl, at),
                Cmd::SetEngine(e) => {
                    let pos = self.seq.pos;
                    self.seq.engine = e;
                    self.seq.seek(pos);
                }
                Cmd::Play => {
                    if let Some(tl) = &self.seq.timeline {
                        if self.seq.pos >= tl.duration - 0.05 {
                            self.seq.seek(0.0);
                        }
                    }
                    self.playing.store(true, Ordering::Relaxed);
                }
                Cmd::Pause => {
                    self.playing.store(false, Ordering::Relaxed);
                    let pos = self.seq.pos;
                    self.seq.seek(pos);
                }
                Cmd::Seek(t) => self.seq.seek(t),
                Cmd::Volume(v) => self.volume = v,
                Cmd::Looping(l) => self.looping = l,
            }
        }
        let frames = data.len() / channels.max(1);
        if !self.playing.load(Ordering::Relaxed) {
            data.iter_mut().for_each(|s| *s = conv(0.0));
            return;
        }
        if self.left.len() < frames {
            self.left.resize(frames, 0.0);
            self.right.resize(frames, 0.0);
        }
        let (l, r) = (&mut self.left[..frames], &mut self.right[..frames]);
        let more = self.seq.render(l, r);
        for (i, frame) in data.chunks_mut(channels).enumerate() {
            let (a, b) = (l[i] * self.volume, r[i] * self.volume);
            for (c, s) in frame.iter_mut().enumerate() {
                *s = conv(if c % 2 == 0 { a } else { b });
            }
        }
        if !more {
            if self.looping {
                self.seq.seek(0.0);
            } else {
                self.playing.store(false, Ordering::Relaxed);
            }
        }
        self.pos_bits.store(self.seq.pos.to_bits(), Ordering::Relaxed);
    }
}

// ------------------------------------------------------------- WAV export

pub fn render_wav(
    song: &Song,
    engine: Box<dyn SynthEngine>,
    sample_rate: u32,
    path: &Path,
    progress: &AtomicU32,
) -> Result<(), String> {
    let tl = Arc::new(Timeline::from_song(song));
    let total = tl.duration;
    let mut seq = Sequencer::new(engine, sample_rate);
    seq.set_timeline(tl, 0.0);
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    let mut l = vec![0.0f32; 4096];
    let mut r = vec![0.0f32; 4096];
    loop {
        let more = seq.render(&mut l, &mut r);
        for i in 0..l.len() {
            w.write_sample((l[i].clamp(-1.0, 1.0) * 32767.0) as i16).map_err(|e| e.to_string())?;
            w.write_sample((r[i].clamp(-1.0, 1.0) * 32767.0) as i16).map_err(|e| e.to_string())?;
        }
        progress.store(((seq.pos / total).min(1.0) * 1000.0) as u32, Ordering::Relaxed);
        if !more {
            break;
        }
    }
    w.finalize().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::generate;
    use crate::song::{Instrumentation, Params, Style};
    use crate::synth::BuiltinSynth;

    #[test]
    fn builtin_render_levels_and_speed() {
        let sr = 44100;
        for style in Style::ALL {
            for inst in [Instrumentation::Piano, Instrumentation::Strings, Instrumentation::PianoAndStrings] {
                let p = Params { style, instrumentation: inst, target_seconds: 60.0, density: 0.8, ..Params::default() };
                let song = generate(&p);
                let mut seq = Sequencer::new(Box::new(BuiltinSynth::new(sr)), sr);
                seq.set_timeline(Arc::new(Timeline::from_song(&song)), 0.0);
                let (mut l, mut r) = (vec![0.0f32; 1024], vec![0.0f32; 1024]);
                let (mut peak, mut sum, mut n) = (0.0f32, 0.0f64, 0usize);
                let t = std::time::Instant::now();
                while seq.render(&mut l, &mut r) {
                    for x in l.iter().chain(r.iter()) {
                        assert!(x.is_finite());
                        peak = peak.max(x.abs());
                        sum += (*x as f64).powi(2);
                        n += 1;
                    }
                }
                let secs = n as f64 / 2.0 / sr as f64;
                let rms_db = 10.0 * (sum / n as f64).log10();
                let speed = secs / t.elapsed().as_secs_f64();
                println!("{style:?} {inst:?}: peak {peak:.2} rms {rms_db:.1} dB, {speed:.0}x realtime");
                assert!(peak > 0.02, "silent");
                assert!(speed > 4.0, "too slow");
            }
        }
    }

    #[test]
    fn midi_roundtrip_recipe() {
        let p = Params { seed: 4242, style: Style::NocturneDrift, ..Params::default() };
        let song = generate(&p);
        let dir = std::env::temp_dir().join("midigen_test.mid");
        crate::midi_export::save(&song, &dir).unwrap();
        let back = crate::midi_export::read_recipe(&dir).unwrap();
        assert_eq!(back, p);
        let data = std::fs::read(&dir).unwrap();
        let smf = midly::Smf::parse(&data).unwrap();
        let ons: usize = smf
            .tracks
            .iter()
            .flatten()
            .filter(|e| matches!(e.kind, midly::TrackEventKind::Midi { message: midly::MidiMessage::NoteOn { .. }, .. }))
            .count();
        assert_eq!(ons, song.notes.len());
    }
}

#[cfg(test)]
mod soundfont_tests {
    use super::*;
    use crate::generator::generate;
    use crate::song::{Instrumentation, Params, Style};
    use crate::synth::SoundFontSynth;

    /// Runs only if a SoundFont has been placed in soundfonts/.
    #[test]
    fn renders_with_generaluser_gs() {
        let path = std::path::Path::new("soundfonts/GeneralUser-GS.sf2");
        let Ok(mut f) = std::fs::File::open(path) else { return };
        let font = Arc::new(rustysynth::SoundFont::new(&mut f).unwrap());
        for inst in [Instrumentation::Piano, Instrumentation::Strings, Instrumentation::PianoAndStrings] {
            let p = Params { style: Style::CinematicSwell, instrumentation: inst, target_seconds: 60.0, density: 0.8, ..Params::default() };
            let song = generate(&p);
            let mut seq = Sequencer::new(Box::new(SoundFontSynth::new(&font, 44100).unwrap()), 44100);
            seq.set_timeline(Arc::new(Timeline::from_song(&song)), 0.0);
            let (mut l, mut r) = (vec![0.0f32; 1024], vec![0.0f32; 1024]);
            let mut peak = 0.0f32;
            while seq.render(&mut l, &mut r) {
                peak = l.iter().chain(r.iter()).fold(peak, |m, x| m.max(x.abs()));
            }
            println!("SF2 {inst:?}: peak {peak:.2}");
            assert!(peak > 0.02);
        }
    }
}
