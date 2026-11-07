//! Note expression: how each note is shaped while it sounds (a swell or fade,
//! vibrato, a slide into or away from the pitch), and the changes made to
//! notes in the piano roll. It runs after generation and writes the shapes as
//! controllers (CC11 expression, CC1 modulation) and pitch bends on the part's
//! channel.

use crate::song::{Bend, Ctrl, Expr, Inst, Note, NoteEdit, Song, Style};

/// Modulation wheel: the engines turn it into vibrato.
pub const CC_VIBRATO: u8 = 1;
const CC_EXPRESSION: u8 = 11;
/// Vibrato either side of the note, in cents, at full modulation.
pub const VIBRATO_CENTS: f32 = 35.0;
/// Deepest slide into a note, in cents.
const SCOOP_CENTS: f32 = 100.0;
/// Deepest fall at the end of a note, in cents.
const FALL_CENTS: f32 = 200.0;
/// The longest fall, in seconds: it carries on into the note's release.
const FALL_SECS: f64 = 0.9;

pub fn apply(song: &mut Song) {
    let edits: Vec<NoteEdit> = song.params.piece_edits().cloned().collect();
    let auto = song.params.version >= 3;
    let mut prev = [None; Inst::ALL.len()];
    let mut notes = Vec::with_capacity(song.notes.len());
    let mut exprs = Vec::with_capacity(song.notes.len());
    let mut off = Vec::new();
    for n in &song.notes {
        let slot = part_index(n.inst);
        let mut expr = if auto { auto_expr(song, n, prev[slot]) } else { Expr::default() };
        prev[slot] = Some(n.key);
        let mut n = *n;
        if let Some(e) = edits.iter().find(|e| e.matches(&n)) {
            if e.off {
                off.push(n);
                continue;
            }
            if let Some(v) = e.vel {
                n.vel = v.clamp(1, 127);
            }
            if let Some(x) = e.expr {
                expr = x;
            }
        }
        notes.push(n);
        exprs.push(expr);
    }
    song.notes = notes;
    song.exprs = exprs;
    song.off = off;

    for inst in Inst::ALL {
        write_swells(song, inst);
        write_vibrato(song, inst);
        write_bends(song, inst);
    }
    song.ctrls.sort_by_key(|c| c.tick);
    song.bends.sort_by_key(|b| b.tick);
    // every note of a bent chord writes the same bend
    song.bends.dedup_by(|a, b| (a.tick, a.inst, a.cents) == (b.tick, b.inst, b.cents));
}

fn part_index(inst: Inst) -> usize {
    Inst::ALL.iter().position(|i| *i == inst).unwrap_or(0)
}

/// A number in 0..1 fixed for a note, so a note's expression doesn't change
/// when other notes or parts are switched off.
fn note_hash(seed: u64, n: &Note, salt: u64) -> f32 {
    let mut h = seed ^ 0x9E37_79B9_7F4A_7C15;
    for x in [n.start as u64, n.key as u64, part_index(n.inst) as u64, salt] {
        h ^= x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h = h.rotate_left(27).wrapping_mul(0x94D0_49BB_1331_11EB);
    }
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Whether nothing else starts on the note's part around its end or in the
/// beat after it: a breath in which it can fall away.
fn breath_after(song: &Song, n: &Note) -> bool {
    // notes of its own chord entering late (or nudged by humanising) don't count
    let from = n.end().saturating_sub(song.ppq / 2).max(n.start + song.ppq / 2);
    let to = n.end() + song.ppq;
    !song.notes.iter().any(|m| m.inst == n.inst && m.start > from && m.start <= to)
}

/// The expression a player would give a note: vibrato that grows on long
/// notes of the tune and bass, a swell on the longest, and now and then a
/// slide up into a note after a leap. Only sustaining sounds get it.
fn auto_expr(song: &Song, n: &Note, prev: Option<u8>) -> Expr {
    if !song.sustains(n.inst) {
        return Expr::default();
    }
    // Megacity Rain's brass and leads play like Neon Noir's
    if matches!(song.params.style, Style::NeonNoir | Style::MegacityRain) {
        return noir_expr(song, n, prev);
    }
    // a score shapes its own swells
    if song.params.style.is_score() {
        return Expr::default();
    }
    if !matches!(n.inst, Inst::Violin | Inst::Cello) {
        return Expr::default();
    }
    let secs = (song.tick_to_secs(n.end()) - song.tick_to_secs(n.start)) as f32;
    let h = |salt| note_hash(song.params.seed, n, salt);
    let mut e = Expr::default();
    if n.inst == Inst::Violin {
        e.vibrato = (0.25 + 0.35 * h(1)) * ((secs - 0.6) / 1.4).clamp(0.0, 1.0);
        if secs >= 2.0 && h(2) < 0.7 {
            e.swell = 0.25 + 0.3 * h(3);
        }
        if let Some(p) = prev {
            if n.key >= p + 5 && secs > 0.4 && h(4) < 0.3 {
                e.bend = 0.3 + 0.3 * h(5);
            }
        }
    } else {
        e.vibrato = (0.15 + 0.15 * h(1)) * ((secs - 1.2) / 1.5).clamp(0.0, 1.0);
    }
    e
}

/// Neon Noir plays its sustained parts like a CS-80: the brass chords sag in
/// pitch as they die away at the end of a phrase, and the lead sings with a
/// deep, slow-building vibrato, sliding up into notes after a leap.
fn noir_expr(song: &Song, n: &Note, prev: Option<u8>) -> Expr {
    let secs = (song.tick_to_secs(n.end()) - song.tick_to_secs(n.start)) as f32;
    let h = |salt| note_hash(song.params.seed, n, salt);
    let mut e = Expr::default();
    match n.inst {
        Inst::StringPad => {
            // decided for the whole chord: its notes end together, to within
            // humanising
            let chord = Note { start: (n.end() + song.ppq / 2) / song.ppq, key: 0, ..*n };
            let h = note_hash(song.params.seed, &chord, 8);
            if secs > 1.5 && h < 0.7 && breath_after(song, n) {
                e.bend = -(0.3 + 0.35 * h);
            }
        }
        // the lead, and Megacity Rain's sax-like line on the brass part
        Inst::Violin | Inst::Brass => {
            e.vibrato = (0.35 + 0.4 * h(1)) * ((secs - 0.5) / 1.2).clamp(0.0, 1.0);
            if secs >= 1.5 && h(2) < 0.6 {
                e.swell = 0.3 + 0.3 * h(3);
            }
            if let Some(p) = prev {
                if n.key >= p + 3 && secs > 0.4 && h(4) < 0.5 {
                    e.bend = 0.4 + 0.4 * h(5);
                }
            }
            if e.bend == 0.0 && secs > 1.2 && h(6) < 0.3 && breath_after(song, n) {
                e.bend = -0.25;
            }
        }
        Inst::Cello => e.vibrato = 0.12 * ((secs - 2.0) / 2.0).clamp(0.0, 1.0),
        _ => {}
    }
    e
}

/// Spans covered by any of the given (start, end) pairs, merged.
fn regions(mut spans: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    spans.sort();
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (a, b) in spans {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn step(song: &Song) -> u32 {
    (song.ppq / 8).max(1)
}

/// Swells and fades: the part's expression controller is multiplied by each
/// shaped note's curve while it sounds.
fn write_swells(song: &mut Song, inst: Inst) {
    let shaped: Vec<(Note, f32)> = song
        .notes
        .iter()
        .zip(&song.exprs)
        .filter(|(n, e)| n.inst == inst && e.swell != 0.0)
        .map(|(n, e)| (*n, e.swell.clamp(-1.0, 1.0)))
        .collect();
    if shaped.is_empty() {
        return;
    }
    let mut base: Vec<(u32, u8)> = song
        .ctrls
        .iter()
        .filter(|c| c.inst == inst && c.cc == CC_EXPRESSION)
        .map(|c| (c.tick, c.value))
        .collect();
    base.sort_by_key(|b| b.0);
    let base_at = |t: u32| base.iter().take_while(|b| b.0 <= t).last().map_or(127, |b| b.1) as f32;
    let spans = regions(shaped.iter().map(|(n, _)| (n.start, n.end())).collect());
    let inside = |t: u32| spans.iter().any(|(a, b)| t >= *a && t < *b);
    song.ctrls.retain(|c| !(c.inst == inst && c.cc == CC_EXPRESSION && inside(c.tick)));
    let step = step(song);
    for (a, b) in spans {
        let mut last = None;
        let mut t = a;
        while t < b {
            let mut v = base_at(t);
            for (n, s) in shaped.iter().filter(|(n, _)| t >= n.start && t < n.end()) {
                let x = (t - n.start) as f32 / n.dur.max(1) as f32;
                v *= if *s > 0.0 {
                    1.0 - 0.6 * s + 0.6 * s * (std::f32::consts::PI * x).sin()
                } else {
                    1.0 - 0.8 * s.abs() * x.powf(0.8)
                };
            }
            let v = v.round().clamp(0.0, 127.0) as u8;
            if last != Some(v) {
                song.ctrls.push(Ctrl { tick: t, inst, cc: CC_EXPRESSION, value: v });
                last = Some(v);
            }
            t += step;
        }
        song.ctrls.push(Ctrl { tick: b, inst, cc: CC_EXPRESSION, value: base_at(b) as u8 });
    }
}

/// Vibrato: the modulation controller rises a little after each note starts.
fn write_vibrato(song: &mut Song, inst: Inst) {
    let min_delay = song.secs_to_tick(0.15);
    let vib: Vec<(Note, f32)> = song
        .notes
        .iter()
        .zip(&song.exprs)
        .filter(|(n, e)| n.inst == inst && e.vibrato > 0.0)
        .map(|(n, e)| (*n, e.vibrato.min(1.0)))
        .collect();
    if vib.is_empty() {
        return;
    }
    let env = |n: &Note, depth: f32, t: u32| {
        let dur = n.dur.max(1) as f32;
        let delay = (min_delay as f32).max(0.25 * dur).min(0.6 * dur);
        let rise = (0.35 * dur).max(1.0);
        let x = ((t - n.start) as f32 - delay) / rise;
        depth * x.clamp(0.0, 1.0)
    };
    let step = step(song);
    for (a, b) in regions(vib.iter().map(|(n, _)| (n.start, n.end())).collect()) {
        let mut last = None;
        let mut t = a;
        while t < b {
            let d = vib
                .iter()
                .filter(|(n, _)| t >= n.start && t < n.end())
                .fold(0.0f32, |m, (n, depth)| m.max(env(n, *depth, t)));
            let v = (d * 127.0).round() as u8;
            if last != Some(v) {
                song.ctrls.push(Ctrl { tick: t, inst, cc: CC_VIBRATO, value: v });
                last = Some(v);
            }
            t += step;
        }
        song.ctrls.push(Ctrl { tick: b, inst, cc: CC_VIBRATO, value: 0 });
    }
}

/// Slides: up into the note from below, or falling away at its end. A fall
/// begins just before the note ends and goes on sagging into its release,
/// staying down until the part's next note.
fn write_bends(song: &mut Song, inst: Inst) {
    let glide_max = song.secs_to_tick(0.25);
    let fall_max = song.secs_to_tick(FALL_SECS);
    let hold = song.secs_to_tick(3.0);
    let step = (song.ppq / 16).max(1);
    let mut starts: Vec<u32> = song.notes.iter().filter(|n| n.inst == inst).map(|n| n.start).collect();
    starts.sort_unstable();
    // A part has one pitch wheel, so the notes of a chord share a bend; and
    // humanising leaves them starting and ending a little apart. Each scoop
    // is written once for the notes starting together, and each fall once
    // for the notes ending together: (start, end, depth).
    let gather = |scoop: bool, near: u32| {
        let mut bent: Vec<(u32, u32, f32)> = song
            .notes
            .iter()
            .zip(&song.exprs)
            .filter(|(n, e)| n.inst == inst && (e.bend > 0.0) == scoop && e.bend != 0.0)
            .map(|(n, e)| (n.start, n.end(), e.bend.clamp(-1.0, 1.0)))
            .collect();
        bent.sort_by_key(|&(a, b, _)| if scoop { a } else { b });
        let mut out: Vec<(u32, u32, f32)> = Vec::new();
        for (a, b, depth) in bent {
            match out.last_mut() {
                Some(g) if scoop && a <= g.0 + near => {
                    g.1 = g.1.min(b);
                    g.2 = g.2.max(depth);
                }
                Some(g) if !scoop && b <= g.1 + near => {
                    g.0 = g.0.min(a);
                    g.1 = b;
                    g.2 = g.2.min(depth);
                }
                _ => out.push((a, b, depth)),
            }
        }
        out
    };
    for (a, b, depth) in gather(true, song.ppq / 8) {
        let glide = (((b - a) as f32 * 0.4) as u32).min(glide_max).max(step);
        let mut t = a;
        while t < a + glide {
            let u = (t - a) as f32 / glide as f32;
            let cents = -depth * SCOOP_CENTS * (1.0 - u) * (1.0 - u);
            song.bends.push(Bend { tick: t, inst, cents: cents.round() as i16 });
            t += step;
        }
        song.bends.push(Bend { tick: a + glide, inst, cents: 0 });
    }
    for (a, b, depth) in gather(false, song.ppq / 2) {
        let from = b.saturating_sub(((b - a) / 3).min(fall_max / 3)).max(a);
        // the part's next note, which the fall mustn't reach
        let next = starts.iter().find(|s| **s > from).copied().unwrap_or(u32::MAX);
        let glide = fall_max.min(next.saturating_sub(from)).max(step);
        let mut t = from;
        while t < from + glide {
            let u = (t - from) as f32 / glide as f32;
            let cents = depth * FALL_CENTS * u * u;
            song.bends.push(Bend { tick: t, inst, cents: cents.round() as i16 });
            t += step;
        }
        song.bends.push(Bend { tick: from + glide, inst, cents: (depth * FALL_CENTS).round() as i16 });
        song.bends.push(Bend { tick: next.min(from + glide + hold), inst, cents: 0 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::generate;
    use crate::song::Params;

    /// Version 3 gives long notes of the tune vibrato, written as modulation;
    /// older recipes get none.
    #[test]
    fn long_lead_notes_get_vibrato() {
        let p = Params { style: Style::CinematicSwell, seed: 7, ..Params::default() };
        let song = generate(&p);
        assert!(song.ctrls.iter().any(|c| c.cc == CC_VIBRATO && c.inst == Inst::Violin && c.value > 0));
        let old = generate(&Params { version: 2, ..p });
        assert!(!old.ctrls.iter().any(|c| c.cc == CC_VIBRATO) && old.bends.is_empty());
    }

    /// Neon Noir's brass chords sag at the breaths between phrases: the fall
    /// starts before the chord ends, carries on into its release, and is
    /// undone before the next chord. Every variant does it.
    #[test]
    fn noir_brass_sags_in_the_breaths() {
        for seed in 1..=12 {
            let p = Params { style: Style::NeonNoir, seed, target_seconds: 120.0, ..Params::default() };
            let song = generate(&p);
            let falls: Vec<&Bend> = song.bends.iter().filter(|b| b.inst == Inst::StringPad && b.cents < -40).collect();
            assert!(!falls.is_empty(), "seed {seed}: no falls");
            for f in falls {
                // a chord is sounding or just released; none starts during the fall
                let next = song.notes.iter().filter(|n| n.inst == Inst::StringPad && n.start > f.tick).map(|n| n.start).min();
                let reset = song.bends.iter().find(|b| b.inst == Inst::StringPad && b.tick > f.tick && b.cents == 0);
                let reset = reset.expect("the bend comes back").tick;
                assert!(next.is_none_or(|n| reset <= n), "seed {seed}: next chord at {next:?} starts bent (reset {reset})");
            }
            // the bends are tidy: no duplicates from the chord's several notes
            for w in song.bends.windows(2) {
                assert!((w[0].tick, w[0].inst) != (w[1].tick, w[1].inst) || w[0].cents != w[1].cents);
            }
            assert!(song.ctrls.iter().any(|c| c.cc == CC_VIBRATO && c.inst == Inst::Violin) || song.notes.iter().all(|n| n.inst != Inst::Violin));
        }
    }

    /// A note switched off is kept aside and can be switched on again; an
    /// edited velocity and expression take effect; nothing else moves.
    #[test]
    fn edits_apply_to_their_piece_only() {
        let mut p = Params { style: Style::Gymnopedie, seed: 11, ..Params::default() };
        let song = generate(&p);
        let target = song.notes[song.notes.len() / 2];
        p.edit_mut(&target).off = true;
        let other = song.notes[song.notes.len() / 3];
        let e = p.edit_mut(&other);
        e.vel = Some(20);
        e.expr = Some(Expr { swell: -1.0, vibrato: 0.0, bend: 0.5 });
        let edited = generate(&p);
        assert_eq!(edited.notes.len(), song.notes.len() - 1);
        assert_eq!(edited.off.len(), 1);
        assert!(edited.off[0].start == target.start && edited.off[0].key == target.key);
        let i = edited.notes.iter().position(|n| n.start == other.start && n.key == other.key && n.inst == other.inst).unwrap();
        assert_eq!(edited.notes[i].vel, 20);
        assert_eq!(edited.exprs[i].bend, 0.5);
        assert!(edited.bends.iter().any(|b| b.inst == other.inst && b.tick == other.start && b.cents == -50));
        // another seed is another piece: the edits don't reach it
        let elsewhere = generate(&Params { seed: 12, ..p.clone() });
        assert!(elsewhere.off.is_empty());
        // and the same piece with a different sound keeps them
        let resounded = generate(&Params { sounds: crate::song::Sounds { keys: 4, ..p.sounds.clone() }, ..p });
        assert_eq!(resounded.off.len(), 1);
    }
}
