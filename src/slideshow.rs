//! Fitting a photo slideshow to the music: how many images a track holds, or
//! how long a track a number of images needs. Images change on bar lines
//! (or, when no whole number of bars fits, on half bars or beats), after an
//! intro and before an outro of whole bars. Following the music, each
//! change moves to where the music marks a moment (a new section, a phrase,
//! a chord change, a part coming in or dropping out), so the pictures turn
//! with it rather than on an even count across it.

use crate::player::REVERB_TAIL;
use crate::song::{Inst, Song};

use serde::{Deserialize, Serialize};

/// The shortest and longest time an image may be on screen, fades included.
pub const MIN_HOLD: f64 = 4.0;
pub const MAX_HOLD: f64 = 16.0;
/// The longest crossfade: half the longest time on screen.
pub const MAX_FADE: f64 = MAX_HOLD / 2.0;

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
    /// Change images where the music marks a moment, letting each image's
    /// time vary around `hold`, rather than on an even count.
    pub follow_music: bool,
}

impl Default for Slideshow {
    fn default() -> Self {
        Slideshow {
            fit: Fit::ImagesToTrack,
            images: 30,
            hold: 12.0,
            fade: 6.0,
            intro_bars: 2,
            outro_bars: 2,
            show_on_timeline: true,
            follow_music: true,
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
}

/// Where the images fall in a track.
#[derive(Clone, Debug)]
pub struct Plan {
    pub grid: Grid,
    /// Each image's time on screen on an even count; following the music,
    /// the time it varies around.
    pub slot: Slot,
    pub images: u32,
    /// When each image appears.
    pub starts: Vec<f64>,
    /// When the last image gives way to the outro.
    pub end: f64,
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
        Plan::even(grid, slot, images, intro, outro, total)
    }

    fn even(grid: Grid, slot: Slot, images: u32, intro: f64, outro: f64, total: f64) -> Plan {
        let starts = (0..images).map(|i| intro + i as f64 * slot.secs).collect();
        let end = intro + images as f64 * slot.secs;
        Plan { grid, slot, images, starts, end, intro, outro, total, tail: REVERB_TAIL }
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
        Plan::even(grid, slot, images, intro, total - intro - pictures, total)
    }

    /// Move the image changes onto the music's accents, on the same grid as
    /// the even count: each image stays between two thirds of `hold` and half
    /// as long again (within MIN_HOLD and MAX_HOLD), kept near it by a cost that grows with the square of how far off it
    /// is, and a change is drawn to a strong moment and away from one where
    /// nothing happens. Counting the images for a track, their number
    /// follows; fitting a track to a number of images, it stays, and the
    /// last ends no later than the even count's last. The best placement
    /// over the whole track is found in one pass (dynamic programming).
    /// Unchanged if the accents are of a different track.
    pub fn follow(self, s: &Slideshow, accents: &Accents) -> Plan {
        if self.images == 0 || (accents.bpm - self.grid.bpm).abs() > 1e-6 || accents.meter != self.grid.meter {
            return self;
        }
        let unit = self.slot.secs / self.slot.count as f64;
        let beats = (unit / self.grid.beat).round().max(1.0) as usize;
        let at = |secs: f64| (secs / unit).round() as usize;
        let (intro, last) = (at(self.intro), at(self.total - s.outro_bars.max(1) as f64 * self.grid.bar()));
        if last <= intro {
            return self;
        }
        let prefer = (s.hold as f64).clamp(MIN_HOLD, MAX_HOLD);
        // from two thirds to half as long again as asked, so the pace holds
        let lo = (prefer * 2.0 / 3.0).max(MIN_HOLD).min(self.slot.secs);
        let hi = (prefer * 1.5).min(MAX_HOLD).max(self.slot.secs);
        let gaps: Vec<usize> = (1..=last - intro).filter(|g| (lo - 1e-6..=hi + 1e-6).contains(&(*g as f64 * unit))).collect();
        let cost = |g: usize| 6.0 * ((g as f64 * unit - prefer) / prefer).powi(2);
        // only how much a moment stands out from the rest counts, so more
        // changes earn nothing in themselves
        let mark = |p: usize| accents.at(p * beats) - accents.typical;
        // a longer outro than asked, in images' worth
        let spare = |p: usize| 2.0 * (last - p) as f64 * unit / prefer;
        let span = last - intro;
        let fixed = (self.slot.count as usize * self.images as usize).min(span);
        let Some(&shortest) = gaps.first() else {
            return self;
        };
        // best[k][p]: the best score of k images from the intro to a change at p
        let rows = if s.fit == Fit::TrackToImages { self.images as usize } else { span / shortest };
        let mut best = vec![vec![f64::NEG_INFINITY; span + 1]; rows + 1];
        let mut from = vec![vec![0usize; span + 1]; rows + 1];
        best[0][0] = mark(intro);
        for k in 1..=rows {
            for p in 1..=span {
                for &g in gaps.iter().filter(|g| **g <= p) {
                    let v = best[k - 1][p - g] - cost(g);
                    if v > best[k][p] {
                        best[k][p] = v;
                        from[k][p] = p - g;
                    }
                }
                if best[k][p] > f64::NEG_INFINITY {
                    best[k][p] += mark(intro + p);
                }
            }
        }
        let ends = |k: usize| (1..=span).map(move |p| (k, p));
        let candidates: Vec<(usize, usize)> = match s.fit {
            Fit::TrackToImages => ends(rows).filter(|(_, p)| *p <= fixed).collect(),
            Fit::ImagesToTrack => (1..=rows).flat_map(ends).collect(),
        };
        let Some((mut k, mut p)) = candidates
            .into_iter()
            .filter(|(k, p)| best[*k][*p] > f64::NEG_INFINITY)
            .max_by(|a, b| (best[a.0][a.1] - spare(intro + a.1)).total_cmp(&(best[b.0][b.1] - spare(intro + b.1))))
        else {
            return self;
        };
        let end = (intro + p) as f64 * unit;
        let mut starts = Vec::new();
        while k > 0 {
            p = from[k][p];
            starts.push((intro + p) as f64 * unit);
            k -= 1;
        }
        starts.reverse();
        Plan { images: starts.len() as u32, starts, end, outro: self.total - end, ..self }
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
        self.starts.iter().copied()
    }

    /// Each image's start and end.
    pub fn spans(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.starts.iter().copied().zip(self.starts.iter().skip(1).copied().chain([self.end]))
    }

    /// The shortest and longest time an image is on screen.
    pub fn holds(&self) -> (f64, f64) {
        self.spans().fold((f64::INFINITY, 0.0f64), |(lo, hi), (a, b)| (lo.min(b - a), hi.max(b - a)))
    }

    /// One row per image, for a video editor. `start` is when the image
    /// starts to fade in, `end` when the next one does. Editors that overlap
    /// clips to crossfade them (most consumer editors) shorten the timeline by
    /// the fade at every change, so each image there must be `clip` long:
    /// its time on screen plus the fade.
    pub fn cue_csv(&self, fade: f64) -> String {
        let mut out = String::from("image,start,end,seconds,clip,bar.beat,fade\n");
        for (i, (t, end)) in self.spans().enumerate() {
            out.push_str(&format!(
                "{},{},{},{:.2},{:.2},{},{:.2}\n",
                i + 1,
                timecode(t),
                timecode(end),
                end - t,
                end - t + fade,
                self.grid.position(t),
                fade
            ));
        }
        out
    }
}

/// How strongly the music marks each beat of a track: notes starting on it
/// (the bass and drums most), a chord change, parts coming in or dropping
/// out and the music thickening or thinning across a bar line, the start of
/// a score's section and of the four- and eight-bar phrases that count from
/// it; and, against all of these, a beat where nothing starts at all.
#[derive(Clone, Debug, Default)]
pub struct Accents {
    pub bpm: f64,
    pub meter: (u32, u32),
    pub beats: Vec<f64>,
    /// The median accent on the bar lines.
    pub typical: f64,
}

impl Accents {
    pub fn of(song: &Song) -> Accents {
        let beat = song.ppq * 4 / song.meter.1;
        let per_bar = song.meter.0 as usize;
        let bar = song.bar_ticks();
        let count = (song.total_ticks / beat) as usize + 1;
        let bars = count / per_bar + 1;
        let weight = |i: Inst| match i {
            Inst::Cello => 1.0,
            Inst::Drums | Inst::Fx => 0.7,
            Inst::Pizzicato => 0.3,
            _ => 0.5,
        };
        // a little either side, for humanised and rubato timing
        let (early, late) = (song.ppq / 8, song.ppq / 4);
        let mut onset = vec![0.0f64; count];
        // the parts starting notes in each bar
        let mut parts = vec![[0u32; 8]; bars];
        for n in &song.notes {
            let j = ((n.start + early) / beat) as usize;
            if j < count && n.start + early - (j as u32 * beat) <= early + late {
                onset[j] += n.vel as f64 / 127.0 * weight(n.inst);
            }
            if let Some(b) = parts.get_mut((n.start / bar) as usize) {
                b[n.inst as usize] += 1;
            }
        }
        let changes: Vec<usize> = song.chords.iter().map(|(t, _)| (*t / beat) as usize).collect();
        let sections: Vec<u32> = crate::generator::section_starts(&song.params, song.bars);
        let sum = |a: usize, z: usize, i: usize| (a..z.min(bars)).map(|b| parts[b][i]).sum::<u32>();
        let beats: Vec<f64> = (0..count)
            .map(|j| {
                let mut a = if onset[j] > 0.0 { 1.0 - (-onset[j] / 1.5).exp() } else { -2.0 };
                if j > 0 && changes.contains(&j) {
                    a += 0.8;
                }
                if j % per_bar == 0 {
                    let b = j / per_bar;
                    if b > 0 {
                        let (lo, hi) = (b.saturating_sub(2), b + 2);
                        let moved = (0..8).filter(|&i| (sum(lo, b, i) == 0) != (sum(b, hi, i) == 0)).count();
                        a += 0.7 * moved.min(3) as f64;
                        let (x, y) = ((0..8).map(|i| sum(lo, b, i)).sum::<u32>(), (0..8).map(|i| sum(b, hi, i)).sum::<u32>());
                        a += 0.4 * ((y as f64 + 2.0) / (x as f64 + 2.0)).ln().abs();
                    }
                    if sections.contains(&(b as u32)) {
                        a += 2.0;
                    }
                    let from = sections.iter().copied().filter(|s| *s as usize <= b).max().unwrap_or(0) as usize;
                    let into = b - from;
                    a += if into % 8 == 0 { 0.5 } else if into % 4 == 0 { 0.3 } else if into % 2 == 0 { 0.1 } else { 0.0 };
                }
                a
            })
            .collect();
        let mut downs: Vec<f64> = beats.iter().step_by(per_bar).copied().collect();
        downs.sort_by(|a, b| a.total_cmp(b));
        let typical = downs.get(downs.len() / 2).copied().unwrap_or(0.0);
        Accents { bpm: song.bpm, meter: song.meter, beats, typical }
    }

    /// The accent on a beat, counted from the start; past the end, nothing.
    pub fn at(&self, beat: usize) -> f64 {
        self.beats.get(beat).copied().unwrap_or(-2.0)
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
    fn slots_fall_between_the_shortest_and_longest() {
        for bpm in [30.0, 40.0, 48.0, 60.0, 72.0, 84.0, 120.0, 200.0] {
            for meter in [(4, 4), (3, 4), (6, 8), (6, 4)] {
                let grid = Grid::new(bpm, meter);
                for hold in [4.0, 6.0, 8.0, 12.0, 16.0] {
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
        // 6/4 at 40: a bar is 9 s, which fits, so whole bars win over 4.5 s half bars
        let s = Slot::choose(&Grid::new(40.0, (6, 4)), 6.0);
        assert_eq!((s.count, s.unit), (1, Unit::Bar));
        // 6/4 at 20 (below the tempo range): a bar is 18 s, too long; a half bar is 9 s
        let s = Slot::choose(&Grid::new(20.0, (6, 4)), 6.0);
        assert_eq!((s.count, s.unit), (1, Unit::HalfBar));
        // 5/4 at 15: a bar is 20 s and has no half bar, so it changes on beats
        // of 4 s; 6 s is as near one as two, so two
        let s = Slot::choose(&Grid::new(15.0, (5, 4)), 6.0);
        assert_eq!((s.count, s.unit), (2, Unit::Beat));
        // the default 12 s at 80 bpm in 4/4 (3 s bars) is four bars
        assert_eq!(Slot::choose(&Grid::new(80.0, (4, 4)), 12.0).count, 4);
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
        assert_eq!(csv.lines().nth(1).unwrap(), "1,0:08.00,0:20.00,12.00,13.00,3.1,1.00");
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
    /// four slow bars, and Koto Garden's phrases and Neon Noir's brass chords
    /// breathe across two. Megacity Rain plays loosely, almost out of time,
    /// with long silences.)
    #[test]
    fn image_changes_land_on_the_music() {
        let free = [Style::GenerativeLoops, Style::TidalDrone, Style::Aurora, Style::KotoGarden, Style::NeonNoir, Style::MegacityRain];
        for style in Style::ALL.into_iter().filter(|s| !free.contains(s)) {
            for seed in [1, 2] {
                let p = Params { style, seed, tempo: style.default_tempo(), target_seconds: 120.0, ..Params::default() };
                let song = generate(&p);
                // images of about 6 s, which most often fall between chords
                let show = Slideshow { hold: 6.0, fade: 1.0, ..Slideshow::default() };
                let plan = Plan::for_track(&show, Grid::new(song.bpm, song.meter), song.duration_secs());
                let onsets: Vec<f64> = song.notes.iter().map(|n| song.tick_to_secs(n.start)).collect();
                let missed: Vec<f64> =
                    plan.starts().filter(|t| !onsets.iter().any(|o| (o - t).abs() <= 0.05)).collect();
                assert!(missed.is_empty(), "{} seed {seed}: no note at {missed:?}", style.label());
            }
        }
    }

    /// Following the music, every change lands on a note in the styles with a
    /// regular pulse, at
    /// a brisk and a lingering pace, and on each section of a score.
    #[test]
    fn changes_follow_the_music() {
        let free = [Style::GenerativeLoops, Style::TidalDrone, Style::Aurora, Style::KotoGarden, Style::NeonNoir, Style::MegacityRain];
        for style in Style::ALL {
            for seed in [1, 2] {
                let p = Params { style, seed, tempo: style.default_tempo(), target_seconds: 150.0, ..Params::default() };
                let song = generate(&p);
                let accents = Accents::of(&song);
                let grid = Grid::new(song.bpm, song.meter);
                let onsets: Vec<f64> = song.notes.iter().map(|n| song.tick_to_secs(n.start)).collect();
                for hold in [6.0, 12.0] {
                    let show = Slideshow { hold, ..Slideshow::default() };
                    let plan = Plan::for_track(&show, grid, song.duration_secs()).follow(&show, &accents);
                    assert!(plan.images > 0);
                    assert!((plan.end + plan.outro - plan.total).abs() < 1e-6);
                    assert!(plan.outro >= 2.0 * grid.bar() - 1e-6);
                    for (a, b) in plan.spans() {
                        assert!((MIN_HOLD - 1e-6..=MAX_HOLD + 1e-6).contains(&(b - a)), "{} {a} {b}", style.label());
                    }
                    let missed: Vec<f64> = plan.starts().filter(|t| !onsets.iter().any(|o| (o - t).abs() <= 0.05)).collect();
                    // (some styles leave longer than an image between notes on the bar lines)
                    assert!(missed.is_empty() || free.contains(&style), "{} seed {seed} hold {hold}: no note at {missed:?}", style.label());
                    if hold == 12.0 {
                        for s in crate::generator::section_starts(&song.params, song.bars) {
                            let t = s as f64 * grid.bar();
                            // (one starting just after the first image, too soon for a change, aside)
                            if t >= plan.intro + hold as f64 / 2.0 && t <= plan.end + 1e-6 {
                                let hit = plan.starts().chain([plan.end]).any(|x| (x - t).abs() < 1e-6);
                                assert!(hit, "{} seed {seed}: no change at section bar {s}", style.label());
                            }
                        }
                    }
                }
            }
        }
    }

    /// Fitting the track to the images keeps their number, inside the track.
    #[test]
    fn following_keeps_the_count() {
        let p = Params { style: Style::NeonGlide, seed: 1, tempo: 94.0, ..Params::default() };
        for images in [3, 12, 40] {
            let show = Slideshow { fit: Fit::TrackToImages, images, ..Slideshow::default() };
            let grid = Grid::new(94.0, (4, 4));
            let even = Plan::for_images(&show, grid);
            let song = generate(&Params { length_mode: crate::song::LengthMode::Bars, target_bars: even.bars() - 1, ..p.clone() });
            assert!((song.duration_secs() - even.total).abs() < 0.01);
            let plan = even.clone().follow(&show, &Accents::of(&song));
            assert_eq!(plan.images, images);
            assert!(plan.end <= even.end + 1e-6);
            assert_eq!(plan.cue_csv(1.0).lines().count(), images as usize + 1);
        }
    }
}
