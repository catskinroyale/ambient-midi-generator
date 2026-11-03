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
    Tintinnabuli,
    Chorale,
    Canon,
    TidalDrone,
    FeltBallad,
    BerlinSchool,
    DubChords,
    Aurora,
    Lullaby,
    CelticAir,
    KotoGarden,
    Gamelan,
    Fingerstyle,
}

impl Style {
    pub const ALL: [Style; 20] = [
        Style::EtherealPads,
        Style::Gymnopedie,
        Style::GenerativeLoops,
        Style::MinimalArpeggios,
        Style::Rainfall,
        Style::CinematicSwell,
        Style::NocturneDrift,
        Style::Tintinnabuli,
        Style::Chorale,
        Style::Canon,
        Style::TidalDrone,
        Style::FeltBallad,
        Style::BerlinSchool,
        Style::DubChords,
        Style::Aurora,
        Style::Lullaby,
        Style::CelticAir,
        Style::KotoGarden,
        Style::Gamelan,
        Style::Fingerstyle,
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
            Style::Tintinnabuli => "Tintinnabuli",
            Style::Chorale => "Hymn Chorale",
            Style::Canon => "Canon on a Ground",
            Style::TidalDrone => "Tidal Drone",
            Style::FeltBallad => "Felt Ballad",
            Style::BerlinSchool => "Berlin School",
            Style::DubChords => "Dub Chords",
            Style::Aurora => "Aurora",
            Style::Lullaby => "Music Box Lullaby",
            Style::CelticAir => "Celtic Air",
            Style::KotoGarden => "Koto Garden",
            Style::Gamelan => "Gamelan Shimmer",
            Style::Fingerstyle => "Fingerstyle Guitar",
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
            Style::Tintinnabuli => {
                "In the spirit of Arvo Pärt: a slow 6/4 melody circling one note, chiming triads, a mirrored bass."
            }
            Style::Chorale => {
                "A slow four-part hymn: block chords with smooth voice leading, suspensions and breaths between phrases."
            }
            Style::Canon => {
                "A Pachelbel-style repeating bass, with the melody taken up in turn by a second and third voice."
            }
            Style::TidalDrone => {
                "A deep tonic drone under slow, wide chords that swell in waves, with high harmonics and distant bells."
            }
            Style::FeltBallad => {
                "A late-night ballad: warm seventh and ninth chords, gently syncopated comping and a lightly swung tune."
            }
            Style::BerlinSchool => {
                "Electronic, in the spirit of Tangerine Dream: a pulsing sequencer pattern over a pedal bass, warm pads and a slow lead."
            }
            Style::DubChords => {
                "Electronic dub: offbeat seventh-chord stabs echoing away into the distance, over a deep sub bass and a sweeping pad."
            }
            Style::Aurora => {
                "Electronic: glowing chord clusters that bloom note by note and fade together, a choir above and crystal bells."
            }
            Style::Lullaby => {
                "A gentle 3/4 lullaby on a music box, over a rocking harp or piano and soft strings."
            }
            Style::CelticAir => {
                "A slow 6/8 air: a flute tune with grace notes over rolling harp, a drone on the tonic and fifth, and soft strings."
            }
            Style::KotoGarden => {
                "A five-note scale on koto, with plucks, slides and tremolo, a breathy shakuhachi and a quiet open-fifth drone. Minor modes give the hirajoshi scale."
            }
            Style::Gamelan => {
                "Interlocking metallophone patterns around a slow core melody in a five-note scale, a gong closing each cycle, and a hazy pad."
            }
            Style::Fingerstyle => {
                "Travis-picked nylon guitar: a steady alternating bass under syncopated treble notes, with soft strings and a simple tune."
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
            Style::Tintinnabuli => 60.0,
            Style::Chorale => 54.0,
            Style::Canon => 58.0,
            Style::TidalDrone => 52.0,
            Style::FeltBallad => 66.0,
            Style::BerlinSchool => 84.0,
            Style::DubChords => 68.0,
            Style::Aurora => 48.0,
            Style::Lullaby => 66.0,
            Style::CelticAir => 60.0,
            Style::KotoGarden => 54.0,
            Style::Gamelan => 72.0,
            Style::Fingerstyle => 76.0,
        }
    }

    /// (numerator, denominator)
    pub fn meter(self) -> (u32, u32) {
        match self {
            Style::Gymnopedie => (3, 4),
            Style::NocturneDrift => (6, 8),
            Style::Tintinnabuli => (6, 4),
            Style::Lullaby => (3, 4),
            Style::CelticAir => (6, 8),
            _ => (4, 4),
        }
    }

    /// The sounds this style is written for, if they differ from the defaults.
    pub fn suggested_sounds(self) -> Option<SuggestedSounds> {
        let electronic = |keys, pad, lead, bass, pluck| SuggestedSounds {
            sounds: Sounds { keys, pad, lead, bass, pluck },
            electronic: true,
        };
        let acoustic = |keys, pad, lead, bass, pluck| SuggestedSounds {
            sounds: Sounds { keys, pad, lead, bass, pluck },
            electronic: false,
        };
        match self {
            // Sub bass, analogue pluck and saw lead, warm or sweeping pads, crystal bells
            Style::BerlinSchool => Some(electronic(98, 88, 81, 38, 45)),
            Style::DubChords => Some(electronic(4, 95, 89, 38, 45)),
            Style::Aurora => Some(electronic(98, 94, 91, 88, 0)),
            // music box, harp, slow strings, flute, cello
            Style::Lullaby => Some(acoustic(10, 49, 73, 42, 46)),
            // harp, slow strings, flute, cello drone, harp
            Style::CelticAir => Some(acoustic(46, 49, 73, 42, 46)),
            // koto, warm pad, shakuhachi, cello, koto
            Style::KotoGarden => Some(acoustic(107, 89, 77, 42, 107)),
            // vibraphone, warm pad, pan flute, tubular bells as the gong, glockenspiel
            Style::Gamelan => Some(acoustic(11, 89, 75, 14, 9)),
            // nylon guitar, slow strings, flute, acoustic bass, harp
            Style::Fingerstyle => Some(acoustic(24, 49, 73, 32, 46)),
            _ => None,
        }
    }

    /// Sounds that suit each part of the style. The first of each is the
    /// style's own (its suggested sound, or the default); a seed picks among them.
    pub fn palette(self) -> Palette {
        const fn pal(
            keys: &'static [u8],
            pad: &'static [u8],
            lead: &'static [u8],
            bass: &'static [u8],
            pluck: &'static [u8],
        ) -> Palette {
            Palette { keys, pad, lead, bass, pluck }
        }
        match self {
            Style::EtherealPads => pal(&[0, 8, 11, 46, 4], &[49, 48, 89, 94, 52], &[40, 73, 41, 53], &[42, 43, 41], &[45, 46, 108]),
            Style::Gymnopedie => pal(&[0, 4, 1], &[49, 48, 52], &[40, 73, 71, 68], &[42, 32], &[45, 46]),
            Style::GenerativeLoops => {
                pal(&[0, 11, 4, 12, 108, 8], &[49, 89, 94, 88, 53], &[40, 73, 75, 52], &[42, 43], &[45, 46, 12, 108])
            }
            Style::MinimalArpeggios => pal(&[0, 4, 1, 12, 46], &[49, 48, 50], &[40, 41, 73, 71], &[42, 43], &[45, 46, 12]),
            Style::Rainfall => pal(&[0, 4, 8, 11, 108, 46], &[49, 89, 94, 48], &[40, 73, 75], &[42, 43], &[45, 46, 108]),
            Style::CinematicSwell => pal(&[0, 1, 46], &[49, 48, 52, 50], &[40, 60, 73, 41], &[42, 43, 70], &[45, 46]),
            Style::NocturneDrift => pal(&[0, 4, 1], &[49, 48, 52], &[40, 41, 71, 73], &[42, 43], &[45, 46]),
            Style::Tintinnabuli => pal(&[0, 8, 11, 9], &[49, 52, 53, 48], &[40, 41, 73], &[42, 43], &[45, 46, 14]),
            Style::Chorale => pal(&[0, 1], &[49, 52, 48, 53], &[40, 41, 68, 60], &[42, 43, 70], &[45, 46]),
            Style::Canon => pal(&[0, 46, 24], &[49, 48], &[40, 41, 73, 68], &[42, 43, 70], &[45, 46, 24]),
            Style::TidalDrone => pal(&[0, 11, 8, 9], &[49, 89, 94, 52, 95], &[40, 53, 73], &[42, 43], &[45, 46, 11]),
            Style::FeltBallad => pal(&[0, 4, 5, 1], &[49, 89, 48], &[40, 73, 71, 11], &[42, 32], &[45, 46, 24]),
            // the electronic styles keep to presets in the Ambient Electronic SoundFont
            Style::BerlinSchool => pal(&[98, 0, 4], &[88, 89, 94, 50, 95], &[81, 91], &[38], &[45, 98, 0]),
            Style::DubChords => pal(&[4, 0, 98], &[95, 89, 88, 94], &[89, 91, 81], &[38], &[45, 4]),
            Style::Aurora => pal(&[98, 0, 4], &[94, 91, 88, 89, 50], &[91, 94], &[88, 89], &[0, 98, 45]),
            Style::Lullaby => pal(&[10, 8, 9, 108], &[49, 48, 52], &[73, 40, 75], &[42, 32], &[46, 24, 45]),
            Style::CelticAir => pal(&[46, 24, 0], &[49, 48, 52], &[73, 75, 40, 71], &[42, 43], &[46, 24]),
            Style::KotoGarden => pal(&[107, 46, 108], &[89, 52, 94, 49], &[77, 73, 75], &[42, 32], &[107, 46]),
            Style::Gamelan => pal(&[11, 12, 8, 108], &[89, 94, 88, 52], &[75, 73, 77], &[14, 11], &[9, 108, 12, 98]),
            Style::Fingerstyle => pal(&[24, 25, 46], &[49, 89, 48], &[73, 40, 71, 11], &[32, 42], &[46, 24, 11]),
        }
    }
}

/// Sounds that suit each part of a style (General MIDI programs).
pub struct Palette {
    pub keys: &'static [u8],
    pub pad: &'static [u8],
    pub lead: &'static [u8],
    pub bass: &'static [u8],
    pub pluck: &'static [u8],
}

impl Palette {
    /// A seed's choice: the style's own sound half the time, otherwise another.
    pub fn pick(&self, seed: u64) -> Sounds {
        let mut rng = crate::rng::Rng::stream(seed, "sounds");
        let mut one = |list: &[u8]| {
            if list.len() < 2 || rng.chance(0.5) { list[0] } else { list[rng.range(1, list.len() as i32 - 1) as usize] }
        };
        Sounds {
            keys: one(self.keys),
            pad: one(self.pad),
            lead: one(self.lead),
            bass: one(self.bass),
            pluck: one(self.pluck),
        }
    }

    pub fn first(&self) -> Sounds {
        Sounds { keys: self.keys[0], pad: self.pad[0], lead: self.lead[0], bass: self.bass[0], pluck: self.pluck[0] }
    }
}

/// A style's sounds: General MIDI programs, and whether the parts should play the
/// bundled Ambient Electronic SoundFont (whose presets sit at those programs).
pub struct SuggestedSounds {
    pub sounds: Sounds,
    pub electronic: bool,
}

/// The SoundFont synthesised for the electronic styles, shipped with the app.
pub const ELECTRONIC_FONT: &str = "Ambient-Electronic.sf2";

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
    /// Parts played by a preset from a loaded SoundFont instead of their
    /// General MIDI sound.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<(Inst, FontPreset)>,
    /// Reverb space and tuning. These only change the sound, not the notes.
    #[serde(default)]
    pub fx: Fx,
    /// The generator version the recipe was made with, so recipes saved by
    /// earlier versions keep their music. Version 2 added song forms, more
    /// progressions and arrangement changes between sections.
    #[serde(default = "first_version")]
    pub version: u32,
}

pub const GENERATOR_VERSION: u32 = 2;

fn first_version() -> u32 {
    1
}

/// The reverb's character.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Space {
    Room,
    Hall,
    Cathedral,
    Shimmer,
    BlackHole,
}

impl Space {
    pub const ALL: [Space; 5] = [Space::Room, Space::Hall, Space::Cathedral, Space::Shimmer, Space::BlackHole];

    pub fn label(self) -> &'static str {
        match self {
            Space::Room => "Room",
            Space::Hall => "Hall",
            Space::Cathedral => "Cathedral",
            Space::Shimmer => "Shimmer",
            Space::BlackHole => "Black Hole",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Space::Room => "A small, close room with a short tail",
            Space::Hall => "A concert hall, about three seconds long",
            Space::Cathedral => "A large stone space with a long, dark tail",
            Space::Shimmer => "A long hall whose tail rises an octave, like a halo of high strings",
            Space::BlackHole => "Enormous, slowly swirling and almost endless: notes dissolve into a cloud",
        }
    }
}

/// Reference pitch for A above middle C.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tuning {
    A440,
    A432,
    A415,
}

impl Tuning {
    pub const ALL: [Tuning; 3] = [Tuning::A440, Tuning::A432, Tuning::A415];

    pub fn label(self) -> &'static str {
        match self {
            Tuning::A440 => "A = 440 Hz (standard)",
            Tuning::A432 => "A = 432 Hz (warmer)",
            Tuning::A415 => "A = 415 Hz (baroque)",
        }
    }

    /// Offset from standard pitch in cents.
    pub fn cents(self) -> f32 {
        match self {
            Tuning::A440 => 0.0,
            Tuning::A432 => 1200.0 * (432f32 / 440.0).log2(),
            Tuning::A415 => 1200.0 * (415f32 / 440.0).log2(),
        }
    }
}

/// Playback effects: the reverb space and tuning. All amounts are 0..1 with
/// 0.5 as the space's natural setting.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fx {
    pub space: Space,
    /// Reverb length: 0 = half, 1 = double the space's natural decay.
    pub size: f32,
    /// Reverb brightness: 0 = dark, 1 = bright.
    pub tone: f32,
    /// Reverb modulation: 0 = still, 1 = swirling.
    pub motion: f32,
    /// How far each part and note is pulled off pitch: 0 = in tune.
    pub detune: f32,
    /// Slow tape-like pitch wander: 0 = steady.
    pub drift: f32,
    pub tuning: Tuning,
    /// Shimmer feedback: how much of the octave-up tail is fed back in.
    pub shimmer: f32,
    /// Detunes the shimmer feedback: 0 = a pure octave; higher splits it into
    /// a sharp and a flat copy that beat against each other.
    pub shimmer_detune: f32,
}

impl Default for Fx {
    fn default() -> Self {
        Fx {
            space: Space::Hall,
            size: 0.5,
            tone: 0.5,
            motion: 0.5,
            detune: 0.0,
            drift: 0.0,
            tuning: Tuning::A440,
            shimmer: 0.5,
            shimmer_detune: 0.0,
        }
    }
}

/// A preset in a SoundFont, chosen for one part. The font is identified by its
/// file name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontPreset {
    pub font: String,
    pub bank: u8,
    pub preset: u8,
    pub name: String,
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
    snd(14, "Tubular Bells", "Mallets & bells"),
    snd(11, "Vibraphone", "Mallets & bells"),
    snd(12, "Marimba", "Mallets & bells"),
    snd(108, "Kalimba", "Mallets & bells"),
    snd(46, "Harp", "Plucked"),
    snd(45, "Pizzicato Strings", "Plucked"),
    snd(24, "Nylon Guitar", "Plucked"),
    snd(25, "Steel Guitar", "Plucked"),
    snd(107, "Koto", "Plucked"),
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
    snd(38, "Synth Bass", "Synth"),
    snd(81, "Saw Lead", "Synth"),
    snd(98, "Crystal", "Synth"),
    snd(73, "Flute", "Winds"),
    snd(75, "Pan Flute", "Winds"),
    snd(77, "Shakuhachi", "Winds"),
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
            fonts: Vec::new(),
            fx: Fx::default(),
            version: GENERATOR_VERSION,
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

    pub fn font(&self, inst: Inst) -> Option<&FontPreset> {
        self.fonts.iter().find(|(i, _)| *i == inst).map(|(_, f)| f)
    }

    pub fn set_font(&mut self, inst: Inst, font: Option<FontPreset>) {
        self.fonts.retain(|(i, _)| *i != inst);
        if let Some(f) = font {
            self.fonts.push((inst, f));
            self.fonts.sort_by_key(|(i, _)| *i);
        }
    }

    /// Whether two settings give the same notes (they may still sound different).
    pub fn same_music(&self, other: &Params) -> bool {
        Params { fx: other.fx, ..self.clone() } == *other
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
        match self.params.font(inst) {
            Some(f) => f.preset,
            None => self.params.sounds.get(inst),
        }
    }

    /// Bank select (CC0) for the part: non-zero only for some SoundFont presets.
    pub fn bank(&self, inst: Inst) -> u8 {
        self.params.font(inst).map_or(0, |f| f.bank)
    }

    /// The name of the sound a part plays.
    pub fn sound_name(&self, inst: Inst) -> String {
        match self.params.font(inst) {
            Some(f) => f.name.clone(),
            None => sound_name(self.params.sounds.get(inst)),
        }
    }

    /// Does the part's sound keep going while a key is held? For SoundFont
    /// presets this isn't known, so it goes by the part's role.
    pub fn sustains(&self, inst: Inst) -> bool {
        match self.params.font(inst) {
            Some(_) => inst.sustains(),
            None => program_sustains(self.params.sounds.get(inst)),
        }
    }

    /// Parts that actually sound (i.e. not muted).
    pub fn instruments(&self) -> Vec<Inst> {
        Inst::ALL
            .into_iter()
            .filter(|i| self.notes.iter().any(|n| n.inst == *i))
            .collect()
    }
}
