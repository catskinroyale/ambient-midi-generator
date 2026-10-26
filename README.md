# Ambient MIDI Generator

A Rust desktop app (egui) that generates calm piano and string music in several ambient styles. You can play it in the app, save it as a MIDI file or export it as a WAV.

```sh
cargo run --release
```

## Styles

| Style | Feel |
|---|---|
| Ethereal Pads | slow string chords, swells, scattered high piano |
| Gymnopédie Waltz | Satie-like 3/4: low bass, offbeat chords, floating melody |
| Generative Loops | Eno-style loops of different lengths drifting out of phase |
| Minimal Arpeggios | Einaudi / Glass broken-chord patterns with a simple theme |
| Rainfall | sparse random pentatonic piano over a quiet string bed |
| Cinematic Swell | repeating chord cycle that adds cello, pads, violin theme, pulses |
| Nocturne Drift | 6/8 nocturne with wide rolling arpeggios and a singing melody |

## Controls

- **Key / mode**: any root note, in major, lydian, mixolydian, dorian, minor or phrygian.
- **Instruments**: piano, strings, or both. Each style re-orchestrates itself for the choice you make.
- **Length**: set a target duration (mm:ss) or a number of bars. The piece ends on the tonic, with a ring-out.
- **Density, Variation, Dynamics, Humanize, Reverb**: shape the result.
- **Parts**: each piece is arranged into up to five parts: Keys, Pad, Lead, Bass and Pluck. For each part you can:
  - switch it on or off. Switching a part off leaves the others note-for-note identical, and the piece keeps the same length.
  - choose its sound from about 35 General MIDI sounds: pianos, electric piano, celesta, glockenspiel, music box, vibraphone, marimba, kalimba, harp, guitar, strings, choirs, pads, flute, clarinet, oboe, bassoon and French horn.
  Both choices are saved in the recipe and in the MIDI file. Parts the current style doesn't use are greyed out.
- **Seed**: the same settings and seed always give exactly the same piece. Each musical layer (harmony, melody, texture…) has its own random stream, so changing one setting doesn't reshuffle everything else. Use 🎲 for a new seed, ◀ ▶ to step through seeds, or ✨ to randomise every setting.
- **Recipe**: a JSON copy of all the settings. Copy it, paste it back with Apply, or use **From .mid…** to recreate a piece from any MIDI file this app saved. The recipe is stored inside the file.

## Sound

The built-in synth (piano, electric piano, mallets and bells, plucked strings, bowed strings, choirs and pads, winds, and reverb) works without any other files. For more realistic instruments, use **Load SoundFont (.sf2)…** to load any General MIDI SoundFont, such as GeneralUser GS or FluidR3_GM. The SoundFont is used for both playback and WAV export.

This repo includes [GeneralUser GS](https://schristiancollins.com/generaluser.php) by S. Christian Collins in `soundfonts/`, and the app loads it automatically at startup. Its licence is in `soundfonts/GeneralUser-GS-LICENSE.txt`.

To build a macOS app bundle with the icon and SoundFont, run `./scripts/bundle_macos.sh`. The bundle is written to `dist/`.

Saved MIDI files are standard type-1 files with one track per instrument and General MIDI programs set. They open in any DAW.

Click or drag on the piano roll to seek. Space bar plays and pauses.

## Tests

```sh
cargo test --release
```

The tests check that output is deterministic, that pieces hit the target length, that every style works with every instrument choice, that the recipe survives a MIDI round trip, and that render levels and speed are reasonable.
