//! Fitting a photo slideshow to the music: how many images a track holds, or
//! how long a track a number of images needs. Images change on bar lines
//! (or, when no whole number of bars fits, on half bars or beats), after an
//! intro and before an outro of whole bars.

use crate::player::REVERB_TAIL;

use serde::{Deserialize, Serialize};

/// The shortest and longest time an image may be on screen, fades included.
pub const MIN_HOLD: f64 = 4.0;
pub const MAX_HOLD: f64 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fit {
    /// Count the images the current track holds.
    ImagesToTrack,
    /// Size the track to hold a given number of images.
    TrackToImages,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Slideshow {
    pub fit: Fit,
    /// Images to fit the track to (TrackToImages).
    pub images: u32,
    /// Preferred seconds on screen per image, fades included (MIN_HOLD..=MAX_HOLD).
    pub hold: f32,
    /// Crossfade between images, in seconds.
    pub fade: f32,
    /// Bars of music before the first image.
    pub intro_bars: u32,
    /// Bars of music after the last image; at least one, the final ring-out.
    pub outro_bars: u32,
    /// Mark the image changes on the timeline.
    pub show_on_timeline: bool,
}

impl Default for Slideshow {
    fn default() -> Self {
        Slideshow {
            fit: Fit::ImagesToTrack,
            images: 30,
            hold: 6.0,
            fade: 1.0,
            intro_bars: 2,
            outro_bars: 2,
            show_on_timeline: true,
        }
    }
}

/// The beat grid of a piece.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    /// Seconds per beat (the meter's lower note value).
    pub beat: f64,
    pub beats_per_bar: u32,
    pub bpm: f64,
    pub meter: (u32, u32),
}

impl Grid {
    pub fn new(bpm: f64, meter: (u32, u32)) -> Self {
        Grid { beat: 60.0 / bpm * 4.0 / meter.1 as f64, beats_per_bar: meter.0, bpm, meter }
    }

    pub fn bar(&self) -> f64 {
        self.beat * self.beats_per_bar as f64
    }

    /// "bar.beat" (both from 1) of a time on the grid.
    pub fn position(&self, secs: f64) -> String {
        let beats = (secs / self.beat).round() as u32;
        format!("{}.{}", beats / self.beats_per_bar + 1, beats % self.beats_per_bar + 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Bar,
    HalfBar,
    Beat,
}

/// How long each image stays up, in whole units of the grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub count: u32,
    pub unit: Unit,
    pub secs: f64,
}

impl Slot {
    /// The time on screen nearest `preferred`, between MIN_HOLD and MAX_HOLD,
    /// in whole bars if any number of bars fits: the music moves on bar lines
    /// (a change mid-bar usually lands on nothing), and chords often last two
    /// bars, so a tie goes to an even number of bars. Only when a bar is too
    /// long or too short does it fall back to half bars, then beats.
    pub fn choose(grid: &Grid, preferred: f64) -> Slot {
        let preferred = preferred.clamp(MIN_HOLD, MAX_HOLD);
        let best = |unit: Unit, len: f64| {
            let lo = (MIN_HOLD / len - 1e-9).ceil().max(1.0) as u32;
            let hi = (MAX_HOLD / len + 1e-9).floor() as u32;
            (lo..=hi).map(|count| Slot { count, unit, secs: count as f64 * len }).min_by(|a, b| {
                (a.secs - preferred)
                    .abs()
                    .total_cmp(&(b.secs - preferred).abs())
                    .then((a.count % 2).cmp(&(b.count % 2)))
                    .then(b.count.cmp(&a.count))
            })
        };
        let half = (grid.beats_per_bar % 2 == 0).then(|| best(Unit::HalfBar, grid.bar() / 2.0)).flatten();
        best(Unit::Bar, grid.bar())
            .or(half)
            // a beat is at most 4 s even at 30 bpm in x/2, so some number of beats always fits
            .or_else(|| best(Unit::Beat, grid.beat))
            .unwrap_or(Slot { count: 1, unit: Unit::Beat, secs: grid.beat })
    }

    pub fn label(&self) -> String {
        let unit = match self.unit {
            Unit::Bar => "bar",
            Unit::HalfBar => "half bar",
            Unit::Beat => "beat",
        };
        format!("{} {unit}{}", self.count, if self.count == 1 { "" } else { "s" })
    }
}

/// Where the images fall in a track.
#[derive(Clone, Debug)]
pub struct Plan {
    pub grid: Grid,
    pub slot: Slot,
    pub images: u32,
    /// Music before the first image.
    pub intro: f64,
    /// Music after the last image, including any time too short for another
    /// image. The reverb tail follows it.
    pub outro: f64,
    /// The music's length, in whole bars.
    pub total: f64,
    /// Silence for the reverb after the music; the audio is `total + tail` long.
    pub tail: f64,
}

impl Plan {
    /// Count the images a piece of `total` seconds holds.
    pub fn for_track(s: &Slideshow, grid: Grid, total: f64) -> Plan {
        let slot = Slot::choose(&grid, s.hold as f64);
        let intro = s.intro_bars as f64 * grid.bar();
        let min_outro = s.outro_bars.max(1) as f64 * grid.bar();
        let room = (total - intro - min_outro).max(0.0);
        let images = (room / slot.secs + 1e-6).floor() as u32;
        let outro = total - intro - images as f64 * slot.secs;
        Plan { grid, slot, images, intro, outro, total, tail: REVERB_TAIL }
    }

    /// The track needed for the given number of images; `bars` gives its
    /// length in whole bars, final ring-out bar included.
    pub fn for_images(s: &Slideshow, grid: Grid) -> Plan {
        let slot = Slot::choose(&grid, s.hold as f64);
        let images = s.images.max(1);
        let intro = s.intro_bars as f64 * grid.bar();
        let pictures = images as f64 * slot.secs;
        let picture_bars = (pictures / grid.bar() - 1e-6).ceil();
        // at least the generator's shortest piece: four bars and the ring-out
        let bars = (s.intro_bars as f64 + picture_bars + s.outro_bars.max(1) as f64).max(5.0);
        let total = bars * grid.bar();
        Plan { grid, slot, images, intro, outro: total - intro - pictures, total, tail: REVERB_TAIL }
    }

    /// The length of the audio, reverb tail included.
    pub fn audio(&self) -> f64 {
        self.total + self.tail
    }

    /// The music's length in whole bars.
    pub fn bars(&self) -> u32 {
        (self.total / self.grid.bar()).round() as u32
    }

    /// When each image appears.
    pub fn starts(&self) -> impl Iterator<Item = f64> + '_ {
        (0..self.images).map(|i| self.intro + i as f64 * self.slot.secs)
    }

    /// One row per image, for a video editor. `start` is when the image
    /// starts to fade in, `end` when the next one does. Editors that overlap
    /// clips to crossfade them (most consumer editors) shorten the timeline by
    /// the fade at every change, so each image there must be `clip` long:
    /// its time on screen plus the fade.
    pub fn cue_csv(&self, fade: f64) -> String {
        let mut out = String::from("image,start,end,seconds,clip,bar.beat,fade\n");
        for (i, t) in self.starts().enumerate() {
            out.push_str(&format!(
                "{},{},{},{:.2},{:.2},{},{:.2}\n",
                i + 1,
                timecode(t),
                timecode(t + self.slot.secs),
                self.slot.secs,
                self.slot.secs + fade,
                self.grid.position(t),
                fade
            ));
        }
        out
    }
}

/// m:ss.cc
pub fn timecode(secs: f64) -> String {
    let cs = (secs.max(0.0) * 100.0).round() as u64;
    format!("{}:{:02}.{:02}", cs / 6000, cs / 100 % 60, cs % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_fall_between_four_and_eight_seconds() {
        for bpm in [30.0, 40.0, 48.0, 60.0, 72.0, 84.0, 120.0, 200.0] {
            for meter in [(4, 4), (3, 4), (6, 8), (6, 4)] {
                let grid = Grid::new(bpm, meter);
                for hold in [4.0, 6.0, 8.0] {
                    let s = Slot::choose(&grid, hold);
                    assert!((MIN_HOLD - 1e-6..=MAX_HOLD + 1e-6).contains(&s.secs), "{bpm} {meter:?} {s:?}");
                }
            }
        }
    }

    #[test]
    fn prefers_whole_bars() {
        // 4/4 at 60: a bar is 4 s
        let grid = Grid::new(60.0, (4, 4));
        assert_eq!(Slot::choose(&grid, 8.0), Slot { count: 2, unit: Unit::Bar, secs: 8.0 });
        assert_eq!(Slot::choose(&grid, 4.5), Slot { count: 1, unit: Unit::Bar, secs: 4.0 });
        // 6 s is as far from one bar as from two: two, an even number
        assert_eq!(Slot::choose(&grid, 6.0), Slot { count: 2, unit: Unit::Bar, secs: 8.0 });
        // 4/4 at 120: a bar is 2 s; 6 s is three bars
        assert_eq!(Slot::choose(&Grid::new(120.0, (4, 4)), 6.0).count, 3);
        // 3/4 at 66 has no half bars: a bar is 2.7 s, two are 5.5 s
        assert_eq!(Slot::choose(&Grid::new(66.0, (3, 4)), 6.0).unit, Unit::Bar);
        // 6/4 at 40: a bar is 9 s, too long; a half bar is 4.5 s
        let s = Slot::choose(&Grid::new(40.0, (6, 4)), 6.0);
        assert_eq!((s.count, s.unit), (1, Unit::HalfBar));
        // 5/4 at 30: a bar is 10 s and has no half bar, so it changes on beats
        let s = Slot::choose(&Grid::new(30.0, (5, 4)), 6.0);
        assert_eq!((s.count, s.unit), (3, Unit::Beat));
    }

    #[test]
    fn images_round_trip_through_a_track() {
        for (bpm, meter) in [(60.0, (4, 4)), (66.0, (3, 4)), (40.0, (6, 4)), (84.0, (4, 4))] {
            let grid = Grid::new(bpm, meter);
            for n in [1, 7, 30, 100] {
                let s = Slideshow { images: n, ..Slideshow::default() };
                let need = Plan::for_images(&s, grid);
                assert_eq!(need.images, n);
                assert!(need.outro >= grid.bar() * 2.0 - 1e-6);
                // the track it asks for holds at least that many images
                let got = Plan::for_track(&s, grid, need.total);
                assert!(got.images >= n, "{bpm} {meter:?} {n}: {}", got.images);
                assert_eq!(need.bars() as f64 * grid.bar(), need.total);
            }
        }
    }

    #[test]
    fn track_plan_adds_up() {
        let grid = Grid::new(60.0, (4, 4));
        let s = Slideshow::default();
        let p = Plan::for_track(&s, grid, 180.0);
        assert!(p.images > 0);
        assert!((p.intro + p.images as f64 * p.slot.secs + p.outro - 180.0).abs() < 1e-6);
        assert!(p.outro >= 2.0 * grid.bar() - 1e-6 && p.outro < 2.0 * grid.bar() + p.slot.secs);
        let csv = p.cue_csv(1.0);
        assert_eq!(csv.lines().count(), p.images as usize + 1);
        assert_eq!(csv.lines().nth(1).unwrap(), "1,0:08.00,0:16.00,8.00,9.00,3.1,1.00");
        assert_eq!(p.audio(), 180.0 + REVERB_TAIL);
    }

    #[test]
    fn timecodes() {
        assert_eq!(timecode(0.0), "0:00.00");
        assert_eq!(timecode(65.5), "1:05.50");
        assert_eq!(Grid::new(60.0, (4, 4)).position(8.0), "3.1");
    }
}

#[cfg(test)]
mod music {
    use super::*;
    use crate::generator::generate;
    use crate::song::{Params, Style};

    /// In the styles with a regular pulse, every image change lands on a note.
    /// (Generative Loops phases freely, Tidal Drone and Aurora hold chords for
    /// four slow bars, and Koto Garden's phrases breathe across two.)
    #[test]
    fn image_changes_land_on_the_music() {
        let free = [Style::GenerativeLoops, Style::TidalDrone, Style::Aurora, Style::KotoGarden];
        for style in Style::ALL.into_iter().filter(|s| !free.contains(s)) {
            for seed in [1, 2] {
                let p = Params { style, seed, tempo: style.default_tempo(), target_seconds: 120.0, ..Params::default() };
                let song = generate(&p);
                let plan = Plan::for_track(&Slideshow::default(), Grid::new(song.bpm, song.meter), song.duration_secs());
                let onsets: Vec<f64> = song.notes.iter().map(|n| song.tick_to_secs(n.start)).collect();
                let missed: Vec<f64> =
                    plan.starts().filter(|t| !onsets.iter().any(|o| (o - t).abs() <= 0.05)).collect();
                assert!(missed.is_empty(), "{} seed {seed}: no note at {missed:?}", style.label());
            }
        }
    }
}
