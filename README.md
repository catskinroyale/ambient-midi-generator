# Ambient MIDI Generator

A Rust desktop app (egui) that generates calm piano, string and electronic music in sixteen ambient styles. You can play it in the app, save it as a MIDI file or export it as a WAV.

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
| Tintinnabuli | Arvo Pärt-like 6/4: a melody circling one note, chiming triads, a mirrored bass |
| Hymn Chorale | slow four-part chorale with smooth voice leading, suspensions and breaths |
| Canon on a Ground | Pachelbel-style repeating bass; the melody is taken up in canon by up to three voices |
| Tidal Drone | tonic drone under slow, wide chords swelling in waves, with high harmonics and distant bells |
| Felt Ballad | late-night ballad: 7th and 9th chords, syncopated rootless comping, a lightly swung tune |
| Berlin School | electronic, Tangerine Dream-like: a pulsing sequencer pattern over a pedal bass, warm pads and a slow lead |
| Dub Chords | electronic dub: offbeat seventh-chord stabs echoing away over a deep sub bass and a sweeping pad |
| Aurora | electronic: chord clusters that bloom note by note and fade together, a choir above and crystal bells |
| Music Box Lullaby | a gentle 3/4 lullaby on a music box over a rocking harp or piano and soft strings |

The electronic styles and the lullaby come with their own sounds: choosing one sets the parts to them (the electronic styles play the bundled Ambient Electronic SoundFont), and choosing another style afterwards puts the defaults back. You can still change any part's sound.

## Controls

- **Key / mode**: any root note, in major, lydian, mixolydian, dorian, minor or phrygian.
- **Instruments**: piano, strings, or both. Each style re-orchestrates itself for the choice you make.
- **Length**: set a target duration (mm:ss) or a number of bars. The piece ends on the tonic, with a ring-out.
- **Density, Variation, Dynamics, Humanize**: shape the result. Density thins or fills every part: melody, accompaniment rhythm, bass movement and how soon strings and pads join in. Each slider is labelled at both ends with which way is less and which is more, such as sparse ◀ ▶ busy.
- **Parts**: each piece is arranged into up to five parts: Keys, Pad, Lead, Bass and Pluck. For each part you can:
  - switch it on or off. Switching a part off leaves the others note-for-note identical, and the piece keeps the same length.
  - choose its sound from about 35 General MIDI sounds: pianos, electric piano, celesta, glockenspiel, music box, vibraphone, marimba, kalimba, harp, guitar, strings, choirs, pads, flute, clarinet, oboe, bassoon and French horn.
  Both choices are saved in the recipe and in the MIDI file. Parts the current style doesn't use are greyed out.
- **Reverb & tuning**: these change the sound but not the notes, and apply straight away while the piece plays:
  - **Space**: Room, Hall, Cathedral, Shimmer (the tail rises an octave) or Black Hole (enormous, slowly swirling and almost endless).
  - **Amount** (dry ◀ ▶ wet, also written to the MIDI file as CC91), **Size** (short ◀ ▶ long), **Tone** (dark ◀ ▶ bright) and **Motion** (still ◀ ▶ swirling).
  - With **Shimmer**, two more controls appear. **Feedback** (subtle ◀ ▶ intense) sets how much of the octave-up tail is fed back in; more climbs higher and leaves more shimmer artefacts. **Fb. detune** (pure ◀ ▶ wide) splits that feedback into a sharp and a flat copy, up to 35 cents either side of the octave, so they beat against each other and spread across the stereo field.
  - **Detune** pulls each part and note slightly off pitch and widens ensemble sounds. **Drift** adds a slow, tape-like pitch wander. **Tuning** sets A to 440, 432 or 415 Hz.
  They work with both the built-in synth and SoundFonts, and are saved in the recipe. Tuning, detune and drift aren't written to the MIDI file.
- **Seed**: the same settings and seed always give exactly the same piece. Each musical layer (harmony, melody, texture…) has its own random stream, so changing one setting doesn't reshuffle everything else. Use 🎲 for a new seed, ◀ ▶ to step through seeds, or ✨ to randomise every setting.
- **Recipe**: a JSON copy of all the settings. Copy it, paste it back with Apply, or use **From .mid…** to recreate a piece from any MIDI file this app saved. The recipe is stored inside the file.

## Sound

The built-in synth (piano, electric piano, mallets and bells, plucked strings, bowed strings, choirs and pads, winds, and a hall reverb) works without any other files. The reverb is a modulated feedback-delay-network design with early reflections and five spaces, and SoundFonts are played through it too. For more realistic instruments, load SoundFonts (.sf2):

- **Main sound**: the built-in synth or a General MIDI SoundFont, such as GeneralUser GS or FluidR3_GM. It plays every part that doesn't have its own preset.
- **Per part**: each part can instead play a preset from any loaded SoundFont, such as a felt piano font for Keys and a choir font for Pad. In **Parts**, choose the font in the middle drop-down and the preset in the right one.
- **Assigning**: after **Load SoundFont (.sf2)…**, a dialog suggests which parts the font should play. A General MIDI font is offered as the main sound. For other fonts, parts are pre-ticked where a preset's name matches the part, such as "cello" for Bass. Reopen the dialog any time with **Assign…**.

You can load several SoundFonts at once. They are used for both playback and WAV export. Per-part presets are saved in the recipe by font file name; if a recipe names a font that isn't loaded, that part falls back to its General MIDI sound. Saved MIDI files include the preset's bank and program.

This repo includes two SoundFonts in `soundfonts/`, and the app loads both at startup:

- [GeneralUser GS](https://schristiancollins.com/generaluser.php) by S. Christian Collins, a General MIDI font and the main sound. Its licence is in `soundfonts/GeneralUser-GS-LICENSE.txt`.
- **Ambient Electronic**, 12 synthesiser presets made for this app: Sine Keys, FM Electric Piano, Analog Sub Bass, Analog Pluck, Supersaw Strings, Soft Saw Lead, Warm Analog Pad, Glass Pad, Airy Choir, Halo Pad, Sweep Pad and Crystal Bells. They sit at their General MIDI program numbers. It contains no recorded samples: `examples/make_soundfont.rs` synthesises every sound, and `cargo run --release --example make_soundfont` rebuilds the file exactly. It is under the MIT License, like the code.

To build a macOS app bundle with the icon and SoundFont, run `./scripts/bundle_macos.sh`. The bundle is written to `dist/`.

Saved MIDI files are standard type-1 files with one track per instrument and General MIDI programs set. They open in any DAW.

Click or drag on the piano roll to seek. Space bar plays and pauses.

## Slideshows

The **Slideshow** section fits a photo slideshow (for example, a YouTube video) to the music. Each image stays on screen for 4 to 8 seconds, crossfade included. Images change on bar lines when a whole number of bars comes within a second of the time you choose. Otherwise they change on half bars, or on beats. Set an intro (music before the first image, e.g. for a title) and an outro (music after the last, e.g. for credits) in bars.

- **Images for track** counts how many images the current track holds. Any time too short for one more image goes to the outro.
- **Track for images** takes a number of images and sets the piece's length in bars to fit them, intro and outro included.

The plan shows the time per image (and how much of it is clear of the fades), the intro, the images, the outro and the track's length. **Copy cue list** and **Save cue list…** give each image's start and end time and its bar and beat as CSV, for your video editor. The image changes can also be marked on the piano roll. The slideshow settings are saved in projects.

## Projects

**💾 Save project** (⌘S / Ctrl+S) saves everything in one `.ambient` file:
- every setting, including parts, sounds, reverb and tuning, and the seed
- volume, loop and "Regenerate on change"
- the slideshow settings
- the loaded SoundFonts, which one is the main sound and which parts use which font

SoundFonts you loaded are stored inside the file, so the project opens on another computer even without them. The GeneralUser GS font that comes with the app isn't copied in: the project refers to it, which keeps project files small. **📂 Open…** (⌘O / Ctrl+O) restores everything. ⌘⇧S / Ctrl+Shift+S saves to a new file. The window title shows the open project.

## Layout, theme and updates

- The look follows [BW Darkroom](https://github.com/catskinroyale/bnw-darkroom), adapted from [LightCraft](https://github.com/storytold/lightcraft): Charcoal (dark) and Silver (light) themes, the Inter typeface, slider rows with the value beside the label and the less/more ends under the track, segmented buttons and accordion sections. Double-click a slider to reset it, click its value to type one, and hold Shift while dragging for fine control.
- The controls are in sections: Piece, Parts, Reverb & Tuning, Sound Fonts and Recipe. The app remembers which ones are open, and the panel can be resized or hidden with the sidebar button.
- The theme button in the top bar steps through Auto (following the system), Light and Dark. Your choice is remembered.
- The top bar also has Open…, Save and Save As… for projects. Play, stop, loop, volume, Save MIDI… and Export WAV… are in the bar under the piano roll.
- ⓘ (or **About Ambient MIDI Generator** in the macOS app menu) opens **About**:
  - the version and the git commit it was built from, and **Check for Updates…**
  - **Licences**: this app's MIT licence and the bundled SoundFont's licence, with full texts
  - **Data and components**: where the SoundFont, playback, MIDI, audio and interface code come from, and their licences
  - **Third-party components**: every library built into the app, with its version and licence. You can filter the list and click a library to read its licence.
  - **Licence texts**: the full text of every licence the libraries use
- At startup the app asks GitHub for the latest release and shows a download link if there's a newer version. Nothing about you is sent. Turn this off, or check by hand, in **About** or the macOS app menu's **Check for Updates…**.

## Tests

```sh
cargo test --release
```

The tests check that output is deterministic, that pieces hit the target length, that every style works with every instrument choice, that the recipe survives a MIDI round trip, and that render levels and speed are reasonable.

To check the interface without clicking through it, run the app with `MIDIGEN_SCREENSHOT=window.bmp`. It saves a picture of its window and quits, using its own settings rather than yours. `MIDIGEN_SCREENSHOT_SHOW` can open parts first, comma-separated: `sections`, `sound`, `fonts`, `slideshow`, `images` (the slideshow fitting a track to 50 images), `electronic`, `about` or `libraries`.

## Licence

The library list and licence texts in **About** come from `assets/licences.json`. After changing dependencies, regenerate it with `python3 scripts/gen_licenses.py`. A test fails if the list is out of date.

The code is released under the [MIT License](LICENSE). The bundled GeneralUser GS SoundFont in `soundfonts/` is not covered by it. It has its own licence, in `soundfonts/GeneralUser-GS-LICENSE.txt`. The Ambient Electronic SoundFont is synthesised by this project and is under the MIT License (`soundfonts/Ambient-Electronic-LICENSE.txt`).

The interface theme and widgets are adapted from BW Darkroom and LightCraft (MIT, `assets/LICENSE-LightCraft-MIT`). The Inter typeface is under the SIL Open Font License 1.1 (`assets/fonts/OFL-Inter.txt`).
