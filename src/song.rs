//! Generation parameters and the generated song representation.

use serde::{Deserialize, Serialize};

use crate::theory::{Mode, NOTE_NAMES};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Style {
    EtherealPads,
    Gymnopedie,
    GenerativeLoops,
    MinimalArpeggios,
    Rainfall,
    CinematicSwell,
    NocturneDrift,
}

impl Style {
    pub const ALL: [Style; 7] = [
        Style::EtherealPads,
        Style::Gymnopedie,
        Style::GenerativeLoops,
        Style::MinimalArpeggios,
        Style::Rainfall,
        Style::CinematicSwell,
        Style::NocturneDrift,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Style::EtherealPads => "Ethereal Pads",
            Style::Gymnopedie => "Gymnopédie Waltz",
            Style::GenerativeLoops => "Generative Loops",
            Style::MinimalArpeggios => "Minimal Arpeggios",
            Style::Rainfall => "Rainfall",
            Style::CinematicSwell => "Cinematic Swell",
            Style::NocturneDrift => "Nocturne Drift",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Style::EtherealPads => {
                "Slow-moving string chords with gentle swells and scattered high piano notes."
            }
            Style::Gymnopedie => {
                "A slow 3/4 waltz in the spirit of Satie: low bass, soft offbeat chords, a floating melody."
            }
            Style::GenerativeLoops => {
                "Eno-style phasing: short phrases loop at different lengths, so they keep drifting into new alignments."
            }
            Style::MinimalArpeggios => {
                "Neo-classical broken-chord patterns (Einaudi / Glass) under a simple, repeating melody."
            }
            Style::Rainfall => {
                "Sparse pentatonic piano notes falling at random, over a quiet string bed."
            }
            Style::CinematicSwell => {
                "Repeating chord cycle that slowly adds layers: cello, pads, violin theme, pulsing piano."
            }
            Style::NocturneDrift => {
                "A lyrical 6/8 nocturne: wide rolling left-hand arpeggios under a singing melody."
            }
        }
    }

    pub fn default_tempo(self) -> f32 {
        match self {
            Style::EtherealPads => 60.0,
            Style::Gymnopedie => 72.0,
            Style::GenerativeLoops => 60.0,
            Style::MinimalArpeggios => 84.0,
            Style::Rainfall => 66.0,
            Style::CinematicSwell => 64.0,
            Style::NocturneDrift => 66.0,
        }
    }

    /// (numerator, denominator)
    pub fn meter(self) -> (u32, u32) {
        match self {
            Style::Gymnopedie => (3, 4),
            Style::NocturneDrift => (6, 8),
            _ => (4, 4),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Instrumentation {
    Piano,
    Strings,
    PianoAndStrings,
}

impl Instrumentation {
    pub fn label(self) -> &'static str {
        match self {
            Instrumentation::Piano => "Piano",
            Instrumentation::Strings => "Strings",
            Instrumentation::PianoAndStrings => "Piano + Strings",
        }
    }
    pub fn piano(self) -> bool {
        !matches!(self, Instrumentation::Strings)
    }
    pub fn strings(self) -> bool {
        !matches!(self, Instrumentation::Piano)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LengthMode {
    Seconds,
    Bars,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Params {
    pub seed: u64,
    pub style: Style,
    pub root: u8,
    pub mode: Mode,
    pub instrumentation: Instrumentation,
    pub tempo: f32,
    pub length_mode: LengthMode,
    pub target_seconds: f32,
    pub target_bars: u32,
    /// How many notes: 0 = very sparse, 1 = busy.
    pub density: f32,
    /// Harmonic and melodic adventurousness.
    pub variation: f32,
    /// How much the arrangement builds and recedes.
    pub dynamics: f32,
    /// Timing / velocity looseness.
    pub humanize: f32,
    /// Reverb send written to the MIDI (CC91) and used by the built-in synth.
    pub reverb: f32,
    /// General MIDI sound chosen for each part.
    #[serde(default)]
    pub sounds: Sounds,
    /// Parts switched off. They are removed after generation, so the remaining
    /// parts are note-for-note identical whether or not others are muted.
    #[serde(default)]
    pub muted: Vec<Inst>,
}

/// GM program per part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sounds {
    pub keys: u8,
    pub pad: u8,
    pub lead: u8,
    pub bass: u8,
    pub pluck: u8,
}

impl Default for Sounds {
    fn default() -> Self {
        Sounds { keys: 0, pad: 49, lead: 40, bass: 42, pluck: 45 }
    }
}

impl Sounds {
    pub fn get(&self, inst: Inst) -> u8 {
        match inst {
            Inst::Piano => self.keys,
            Inst::StringPad => self.pad,
            Inst::Violin => self.lead,
            Inst::Cello => self.bass,
            Inst::Pizzicato => self.pluck,
        }
    }
    pub fn get_mut(&mut self, inst: Inst) -> &mut u8 {
        match inst {
            Inst::Piano => &mut self.keys,
            Inst::StringPad => &mut self.pad,
            Inst::Violin => &mut self.lead,
            Inst::Cello => &mut self.bass,
            Inst::Pizzicato => &mut self.pluck,
        }
    }
}

pub struct Sound {
    pub program: u8,
    pub name: &'static str,
    pub family: &'static str,
}

const fn snd(program: u8, name: &'static str, family: &'static str) -> Sound {
    Sound { program, name, family }
}

/// The sounds offered in the UI (General MIDI programs, 0-based).
pub const SOUNDS: &[Sound] = &[
    snd(0, "Grand Piano", "Keys"),
    snd(1, "Bright Piano", "Keys"),
    snd(4, "Electric Piano", "Keys"),
    snd(5, "Electric Piano 2", "Keys"),
    snd(8, "Celesta", "Mallets & bells"),
    snd(9, "Glockenspiel", "Mallets & bells"),
    snd(10, "Music Box", "Mallets & bells"),
    snd(11, "Vibraphone", "Mallets & bells"),
    snd(12, "Marimba", "Mallets & bells"),
    snd(108, "Kalimba", "Mallets & bells"),
    snd(46, "Harp", "Plucked"),
    snd(45, "Pizzicato Strings", "Plucked"),
    snd(24, "Nylon Guitar", "Plucked"),
    snd(32, "Acoustic Bass", "Plucked"),
    snd(49, "Slow Strings", "Strings"),
    snd(48, "String Ensemble", "Strings"),
    snd(50, "Synth Strings", "Strings"),
    snd(40, "Violin", "Strings"),
    snd(41, "Viola", "Strings"),
    snd(42, "Cello", "Strings"),
    snd(43, "Contrabass", "Strings"),
    snd(52, "Choir Aahs", "Voices & pads"),
    snd(53, "Voice Oohs", "Voices & pads"),
    snd(88, "New Age Pad", "Voices & pads"),
    snd(89, "Warm Pad", "Voices & pads"),
    snd(91, "Choir Pad", "Voices & pads"),
    snd(94, "Halo Pad", "Voices & pads"),
    snd(95, "Sweep Pad", "Voices & pads"),
    snd(73, "Flute", "Winds"),
    snd(75, "Pan Flute", "Winds"),
    snd(71, "Clarinet", "Winds"),
    snd(68, "Oboe", "Winds"),
    snd(70, "Bassoon", "Winds"),
    snd(60, "French Horn", "Winds"),
];

pub fn sound_name(program: u8) -> String {
    SOUNDS
        .iter()
        .find(|s| s.program == program)
        .map(|s| s.name.to_string())
        .unwrap_or_else(|| format!("GM program {}", program + 1))
}

/// Does this GM program sustain while a key is held (vs. a struck/plucked decay)?
pub fn program_sustains(program: u8) -> bool {
    !matches!(program, 0..=15 | 24..=39 | 45..=47 | 104..=119)
}

impl Default for Params {
    fn default() -> Self {
        Self {
            seed: 20261026,
            style: Style::EtherealPads,
            root: 2,
            mode: Mode::Ionian,
            instrumentation: Instrumentation::PianoAndStrings,
            tempo: Style::EtherealPads.default_tempo(),
            length_mode: LengthMode::Seconds,
            target_seconds: 180.0,
            target_bars: 48,
            density: 0.5,
            variation: 0.4,
            dynamics: 0.6,
            humanize: 0.4,
            reverb: 0.6,
            sounds: Sounds::default(),
            muted: Vec::new(),
        }
    }
}

impl Params {
    pub fn title(&self) -> String {
        format!(
            "{} in {} {} #{}",
            self.style.label(),
            NOTE_NAMES[self.root as usize % 12],
            match self.mode {
                Mode::Ionian => "major",
                Mode::Aeolian => "minor",
                Mode::Lydian => "lydian",
                Mode::Mixolydian => "mixolydian",
                Mode::Dorian => "dorian",
                Mode::Phrygian => "phrygian",
            },
            self.seed
        )
    }

    pub fn file_stem(&self) -> String {
        self.title()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
            .replace("__", "_")
    }

    pub fn to_recipe(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_recipe(s: &str) -> Result<Self, String> {
        serde_json::from_str(s.trim()).map_err(|e| e.to_string())
    }
}

/// A part in the arrangement. Each part has its own MIDI channel; its sound is
/// chosen in `Params::sounds`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Inst {
    Piano,
    StringPad,
    Violin,
    Cello,
    Pizzicato,
}

impl Inst {
    pub const ALL: [Inst; 5] = [
        Inst::Piano,
        Inst::StringPad,
        Inst::Violin,
        Inst::Cello,
        Inst::Pizzicato,
    ];

    pub fn channel(self) -> u8 {
        match self {
            Inst::Piano => 0,
            Inst::StringPad => 1,
            Inst::Violin => 2,
            Inst::Cello => 3,
            Inst::Pizzicato => 4,
        }
    }

    pub fn volume(self) -> u8 {
        match self {
            Inst::Piano => 104,
            Inst::StringPad => 92,
            Inst::Violin => 88,
            Inst::Cello => 92,
            Inst::Pizzicato => 96,
        }
    }

    /// The part's role in the arrangement.
    pub fn role(self) -> &'static str {
        match self {
            Inst::Piano => "Keys",
            Inst::StringPad => "Pad",
            Inst::Violin => "Lead",
            Inst::Cello => "Bass",
            Inst::Pizzicato => "Pluck",
        }
    }

    pub fn role_hint(self) -> &'static str {
        match self {
            Inst::Piano => "Main piano part: chords, arpeggios, melodies, sparkles",
            Inst::StringPad => "Sustained chords",
            Inst::Violin => "Melodies and high lines (strings arrangements)",
            Inst::Cello => "Bass notes and drones",
            Inst::Pizzicato => "Plucked patterns when there's no piano",
        }
    }

    /// Is this a sustained part in the arrangement (legato phrasing)?
    pub fn sustains(self) -> bool {
        matches!(self, Inst::StringPad | Inst::Violin | Inst::Cello)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Note {
    pub start: u32,
    pub dur: u32,
    pub key: u8,
    pub vel: u8,
    pub inst: Inst,
}

impl Note {
    pub fn end(&self) -> u32 {
        self.start + self.dur
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ctrl {
    pub tick: u32,
    pub inst: Inst,
    pub cc: u8,
    pub value: u8,
}

#[derive(Clone, Debug)]
pub struct Song {
    pub params: Params,
    pub ppq: u32,
    pub bpm: f64,
    pub meter: (u32, u32),
    pub bars: u32,
    pub notes: Vec<Note>,
    pub ctrls: Vec<Ctrl>,
    /// (tick, chord name) for display
    pub chords: Vec<(u32, String)>,
    /// Every part the arrangement uses, including muted ones.
    pub parts: Vec<Inst>,
    pub total_ticks: u32,
}

impl Song {
    pub fn bar_ticks(&self) -> u32 {
        self.ppq * 4 * self.meter.0 / self.meter.1
    }

    pub fn tick_to_secs(&self, t: u32) -> f64 {
        t as f64 / self.ppq as f64 * 60.0 / self.bpm
    }

    pub fn secs_to_tick(&self, s: f64) -> u32 {
        (s.max(0.0) * self.bpm / 60.0 * self.ppq as f64) as u32
    }

    pub fn duration_secs(&self) -> f64 {
        self.tick_to_secs(self.total_ticks)
    }

    pub fn program(&self, inst: Inst) -> u8 {
        self.params.sounds.get(inst)
    }

    /// Parts that actually sound (i.e. not muted).
    pub fn instruments(&self) -> Vec<Inst> {
        Inst::ALL
            .into_iter()
            .filter(|i| self.notes.iter().any(|n| n.inst == *i))
            .collect()
    }
}
