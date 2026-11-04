//! Standard MIDI File (type 1) export. The generation recipe is embedded as a
//! text meta event so any saved file can be regenerated exactly.

use std::path::Path;

use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};

use crate::song::{Inst, Song};

pub const RECIPE_PREFIX: &str = "midigen-recipe:";

enum Ev {
    Off(u8),
    Ctrl(u8, u8),
    /// Pitch bend in cents.
    Bend(i16),
    On(u8, u8),
}

/// Pitch bend range in cents (General MIDI's default of two semitones).
const BEND_RANGE_CENTS: f32 = 200.0;

pub fn to_bytes(song: &Song) -> std::io::Result<Vec<u8>> {
    let title = song.params.title();
    let recipe = format!("{RECIPE_PREFIX}{}", song.params.to_recipe());
    let names: Vec<(Inst, String)> = song
        .instruments()
        .into_iter()
        .map(|i| (i, format!("{} ({})", song.sound_name(i), i.role())))
        .collect();

    let mut tracks: Vec<Vec<TrackEvent>> = Vec::new();

    // Conductor track
    let us_per_quarter = (60_000_000.0 / song.bpm).round() as u32;
    let denom_pow = (song.meter.1 as f32).log2() as u8;
    let mut t0 = vec![
        meta(0, MetaMessage::TrackName(title.as_bytes())),
        meta(0, MetaMessage::Text(recipe.as_bytes())),
        meta(0, MetaMessage::Tempo(u24::new(us_per_quarter))),
        meta(0, MetaMessage::TimeSignature(song.meter.0 as u8, denom_pow, 24, 8)),
    ];
    t0.push(meta(song.total_ticks, MetaMessage::EndOfTrack));
    tracks.push(t0);

    for (inst, name) in &names {
        let ch = u4::new(inst.channel());
        let mut abs: Vec<(u32, Ev)> = Vec::new();
        // The mixer's fader, pan and reverb send are written into the part's
        // volume, pan and reverb controllers. Its mute and solo are left out:
        // they are for listening, and every part is kept.
        let mix = &song.params.mix;
        let strip = mix.strips.get(mix.strip_of(*inst));
        let gain = strip.map_or(1.0, |s| s.gain());
        let send = strip.map_or(1.0, |s| s.reverb * 2.0);
        let pan = strip.map_or(0.0, |s| s.pan);
        abs.push((0, Ev::Ctrl(10, (64.0 + pan * 63.5).round().clamp(0.0, 127.0) as u8)));
        for c in song.ctrls.iter().filter(|c| c.inst == *inst) {
            let value = match c.cc {
                7 => (c.value as f32 * gain).round().clamp(0.0, 127.0) as u8,
                91 => (c.value as f32 * send).round().clamp(0.0, 127.0) as u8,
                _ => c.value,
            };
            abs.push((c.tick, Ev::Ctrl(c.cc, value)));
        }
        for b in song.bends.iter().filter(|b| b.inst == *inst) {
            abs.push((b.tick, Ev::Bend(b.cents)));
        }
        for n in song.notes.iter().filter(|n| n.inst == *inst) {
            abs.push((n.start, Ev::On(n.key, n.vel)));
            abs.push((n.end(), Ev::Off(n.key)));
        }
        // offs before controllers before ons at the same tick
        abs.sort_by_key(|(t, e)| {
            (*t, match e {
                Ev::Off(_) => 0,
                Ev::Ctrl(..) | Ev::Bend(_) => 1,
                Ev::On(..) => 2,
            })
        });

        let mut track = vec![meta(0, MetaMessage::TrackName(name.as_bytes()))];
        let bank = song.bank(*inst);
        if bank != 0 {
            track.push(TrackEvent {
                delta: u28::new(0),
                kind: TrackEventKind::Midi {
                    channel: ch,
                    message: MidiMessage::Controller { controller: u7::new(0), value: u7::new(bank) },
                },
            });
        }
        track.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Midi {
                channel: ch,
                message: MidiMessage::ProgramChange { program: u7::new(song.program(*inst)) },
            },
        });
        let mut last = 0u32;
        for (t, e) in abs {
            let message = match e {
                Ev::Off(k) => MidiMessage::NoteOff { key: u7::new(k), vel: u7::new(0) },
                Ev::On(k, v) => MidiMessage::NoteOn { key: u7::new(k), vel: u7::new(v.max(1)) },
                Ev::Ctrl(c, v) => MidiMessage::Controller { controller: u7::new(c), value: u7::new(v) },
                Ev::Bend(cents) => {
                    let v = (cents as f32 / BEND_RANGE_CENTS * 8192.0).round().clamp(-8192.0, 8191.0) as i16;
                    MidiMessage::PitchBend { bend: midly::PitchBend::from_int(v) }
                }
            };
            track.push(TrackEvent {
                delta: u28::new(t - last),
                kind: TrackEventKind::Midi { channel: ch, message },
            });
            last = t;
        }
        track.push(meta(song.total_ticks.saturating_sub(last), MetaMessage::EndOfTrack));
        tracks.push(track);
    }

    let mut smf = Smf::new(Header::new(Format::Parallel, Timing::Metrical(u15::new(song.ppq as u16))));
    smf.tracks = tracks;
    let mut out = Vec::new();
    smf.write_std(&mut out)?;
    Ok(out)
}

pub fn save(song: &Song, path: &Path) -> std::io::Result<()> {
    std::fs::write(path, to_bytes(song)?)
}

/// Pull the embedded recipe back out of a MIDI file written by this app.
pub fn read_recipe(path: &Path) -> Result<crate::song::Params, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let smf = Smf::parse(&data).map_err(|e| e.to_string())?;
    for track in &smf.tracks {
        for ev in track {
            if let TrackEventKind::Meta(MetaMessage::Text(t)) = ev.kind {
                if let Some(r) = std::str::from_utf8(t).ok().and_then(|s| s.strip_prefix(RECIPE_PREFIX)) {
                    return crate::song::Params::from_recipe(r);
                }
            }
        }
    }
    Err("No midigen recipe found in this MIDI file".into())
}

fn meta(delta: u32, m: MetaMessage<'_>) -> TrackEvent<'_> {
    TrackEvent { delta: u28::new(delta), kind: TrackEventKind::Meta(m) }
}
