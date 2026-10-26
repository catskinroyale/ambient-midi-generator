//! Scales, chords and voicings. Melodic and harmonic material is generated in
//! "scale degree" space (0 = tonic, 7 = tonic an octave up) so everything stays diatonic.

use serde::{Deserialize, Serialize};

pub const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    Ionian,
    Lydian,
    Mixolydian,
    Dorian,
    Aeolian,
    Phrygian,
}

impl Mode {
    pub const ALL: [Mode; 6] = [
        Mode::Ionian,
        Mode::Lydian,
        Mode::Mixolydian,
        Mode::Dorian,
        Mode::Aeolian,
        Mode::Phrygian,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Ionian => "Major (Ionian)",
            Mode::Lydian => "Lydian (dreamy)",
            Mode::Mixolydian => "Mixolydian",
            Mode::Dorian => "Dorian (wistful)",
            Mode::Aeolian => "Minor (Aeolian)",
            Mode::Phrygian => "Phrygian (dark)",
        }
    }

    pub fn intervals(self) -> [i32; 7] {
        match self {
            Mode::Ionian => [0, 2, 4, 5, 7, 9, 11],
            Mode::Lydian => [0, 2, 4, 6, 7, 9, 11],
            Mode::Mixolydian => [0, 2, 4, 5, 7, 9, 10],
            Mode::Dorian => [0, 2, 3, 5, 7, 9, 10],
            Mode::Aeolian => [0, 2, 3, 5, 7, 8, 10],
            Mode::Phrygian => [0, 1, 3, 5, 7, 8, 10],
        }
    }

    pub fn is_minor(self) -> bool {
        matches!(self, Mode::Dorian | Mode::Aeolian | Mode::Phrygian)
    }

    /// Five "safe" degrees (a pentatonic subset) for floating, consonant lines.
    pub fn pentatonic(self) -> [i32; 5] {
        match self {
            Mode::Ionian | Mode::Mixolydian => [0, 1, 2, 4, 5],
            Mode::Lydian => [0, 1, 3, 4, 5],
            Mode::Dorian | Mode::Aeolian | Mode::Phrygian => [0, 2, 3, 4, 6],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Scale {
    pub root: i32,
    pub mode: Mode,
}

impl Scale {
    pub fn new(root: u8, mode: Mode) -> Self {
        Self { root: root as i32 % 12, mode }
    }

    /// MIDI pitch of scale degree `deg`; degree 0 is the tonic nearest middle C.
    pub fn midi(&self, deg: i32) -> i32 {
        let iv = self.mode.intervals();
        let oct = deg.div_euclid(7);
        let base = 60 + if self.root > 6 { self.root - 12 } else { self.root };
        base + 12 * oct + iv[deg.rem_euclid(7) as usize]
    }

    /// Scale degree whose pitch is closest to (and not above, on ties) `midi`.
    pub fn degree_near(&self, midi: i32) -> i32 {
        let mut d = (midi - 60) * 7 / 12;
        while self.midi(d) > midi {
            d -= 1;
        }
        while self.midi(d + 1) <= midi {
            d += 1;
        }
        d
    }

    pub fn pc(&self, deg: i32) -> i32 {
        self.midi(deg).rem_euclid(12)
    }

    /// Is the triad on this degree diminished (to be avoided for ambient music)?
    pub fn is_diminished(&self, deg: i32) -> bool {
        let r = self.midi(deg);
        self.midi(deg + 4) - r == 6
    }

    pub fn is_pentatonic(&self, deg: i32) -> bool {
        self.mode.pentatonic().contains(&deg.rem_euclid(7))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ext {
    Triad,
    Add9,
    Seventh,
    Ninth,
    Sus2,
    Sus4,
}

#[derive(Clone, Debug)]
pub struct Chord {
    /// Scale degree of the chord root (0..7).
    pub root: i32,
    pub ext: Ext,
}

impl Chord {
    pub fn new(root: i32, ext: Ext) -> Self {
        Self { root: root.rem_euclid(7), ext }
    }

    /// Degree offsets from the root.
    pub fn offsets(&self) -> &'static [i32] {
        match self.ext {
            Ext::Triad => &[0, 2, 4],
            Ext::Add9 => &[0, 2, 4, 8],
            Ext::Seventh => &[0, 2, 4, 6],
            Ext::Ninth => &[0, 2, 4, 6, 8],
            Ext::Sus2 => &[0, 1, 4],
            Ext::Sus4 => &[0, 3, 4],
        }
    }

    pub fn degrees(&self) -> Vec<i32> {
        self.offsets().iter().map(|o| self.root + o).collect()
    }

    /// Is the scale degree one of this chord's tones (any octave)?
    pub fn contains(&self, deg: i32) -> bool {
        let rel = (deg - self.root).rem_euclid(7);
        self.offsets().iter().any(|o| o.rem_euclid(7) == rel)
    }

    /// Triad tones only (the stable ones to land a melody on).
    pub fn is_core_tone(&self, deg: i32) -> bool {
        let rel = (deg - self.root).rem_euclid(7);
        match self.ext {
            Ext::Sus2 => rel == 0 || rel == 1 || rel == 4,
            Ext::Sus4 => rel == 0 || rel == 3 || rel == 4,
            _ => rel == 0 || rel == 2 || rel == 4,
        }
    }

    pub fn pitch_classes(&self, scale: &Scale) -> Vec<i32> {
        let mut v: Vec<i32> = self.degrees().iter().map(|d| scale.pc(*d)).collect();
        v.dedup();
        v
    }

    pub fn name(&self, scale: &Scale) -> String {
        let r = scale.midi(self.root);
        let third = scale.midi(self.root + 2) - r;
        let fifth = scale.midi(self.root + 4) - r;
        let seventh = scale.midi(self.root + 6) - r;
        let base = NOTE_NAMES[r.rem_euclid(12) as usize];
        let minor = third == 3;
        let qual = match (third, fifth) {
            (3, 6) => "dim",
            (3, _) => "m",
            _ => "",
        };
        let sev = if seventh == 11 { "maj7" } else { "7" };
        match self.ext {
            Ext::Triad => format!("{base}{qual}"),
            Ext::Add9 => format!("{base}{qual}(add9)"),
            Ext::Seventh => format!("{base}{}{sev}", if minor { "m" } else { "" }),
            Ext::Ninth => format!("{base}{}{}", if minor { "m" } else { "" }, sev.replace('7', "9")),
            Ext::Sus2 => format!("{base}sus2"),
            Ext::Sus4 => format!("{base}sus4"),
        }
    }
}

/// Choose a voicing of `pcs` within [lo, hi] that moves as little as possible
/// from `prev`. `spread` picks open (wider) voicings.
pub fn voice_lead(pcs: &[i32], prev: &[i32], lo: i32, hi: i32, spread: bool) -> Vec<i32> {
    let n = pcs.len();
    let mut candidates: Vec<Vec<i32>> = Vec::new();
    for inv in 0..n {
        for base_oct in 2..7 {
            let mut v = Vec::with_capacity(n);
            let mut last = -1;
            for i in 0..n {
                let pc = pcs[(inv + i) % n];
                let mut p = base_oct * 12 + pc;
                while p <= last {
                    p += 12;
                }
                v.push(p);
                last = p;
            }
            if spread && n >= 3 {
                // drop-2 style: raise the second voice an octave
                v[1] += 12;
                v.sort();
            }
            if v[0] >= lo && *v.last().unwrap() <= hi {
                candidates.push(v);
            }
        }
    }
    if candidates.is_empty() {
        let mut v: Vec<i32> = pcs.iter().map(|pc| lo - lo.rem_euclid(12) + pc).collect();
        v.sort();
        return v;
    }
    let centre = (lo + hi) as f64 / 2.0;
    let cost = |v: &Vec<i32>| -> f64 {
        if prev.is_empty() {
            let mean = v.iter().sum::<i32>() as f64 / v.len() as f64;
            return (mean - centre).abs();
        }
        let mut c = 0.0;
        for (i, p) in v.iter().enumerate() {
            let j = (i * prev.len()) / v.len();
            c += (p - prev[j.min(prev.len() - 1)]).abs() as f64;
        }
        let mean = v.iter().sum::<i32>() as f64 / v.len() as f64;
        c + 0.15 * (mean - centre).abs()
    };
    candidates
        .into_iter()
        .min_by(|a, b| cost(a).partial_cmp(&cost(b)).unwrap())
        .unwrap()
}
