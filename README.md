# Ambient MIDI Generator

A Rust desktop app (egui) that generates calm piano, string and electronic music in twenty-one styles, and cinematic scores. You can play it in the app, save it as a MIDI file or export it as a WAV.

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
| Celtic Air | a slow 6/8 flute air with grace notes over rolling harp, a tonic-and-fifth drone and soft strings |
| Koto Garden | koto in a five-note scale (hirajoshi in minor) with pulls, falling runs and tremolo, a breathy shakuhachi and an open-fifth drone |
| Gamelan Shimmer | interlocking metallophone figures around a slow core melody in a five-note scale, a gong closing each cycle |
| Fingerstyle Guitar | Travis-picked nylon guitar: alternating bass under syncopated treble, soft strings and a simple tune |
| Neon Noir | electronic, in the spirit of Vangelis's *Blade Runner*: CS-80-style brass swells that sag in pitch at the end of each phrase, a smoky lead with deep vibrato over a low drone, bells falling like rain, and sometimes a slow heartbeat or a ticking pulse |

## Scores

A score is written as one arc rather than in sections that repeat: an intro, a build, a peak and an outro, scaled to the length you choose. Scores are listed under **Scores** in the Style menu and play a full orchestra with three extra parts: **Brass**, **Drums** and **FX** (a sub drone, 808-style sub booms, braams, risers, reversed cymbals, whooshes, impacts and glassy grains). Choosing a score switches a major key to minor, and sets up its effects and a mixer channel for each of its parts.

| Score | Arc |
|---|---|
| Tech Trailer | a hybrid trailer score for technology and the future, around 80 bpm. A lone felt piano motif with long reverb tails over a sub drone and glassy grains; a build that layers a pulsing synth arpeggio, a heartbeat kick, ticking filtered percussion, a low string ostinato, strings rising in pitch, brass swells, risers, reversed cymbals and braams; a breath, then a climax of big half-time drums, sub hits, braams, soaring strings, brass and choir, often doubling up in the final third; and an outro back to a lone piano, ending on a soft hit |

Its variants change the piano motif's rhythm and notes, the arpeggio, the string ostinato, whether the peak is led by taiko drums or the kit, whether the drums double up, the brass voicing and the chords, which lift from minor towards major at the peak.

Every style has a palette of sounds that suit it. With **Sounds follow the seed** on (in Parts), each new seed or style picks its sounds from the palette, so pieces don't all use the same instruments. The electronic styles keep to the bundled Ambient Electronic SoundFont. **Style's sounds** puts back the sounds a style is written for, and **Shuffle** picks others. Turn the option off to keep the sounds you chose; then only the electronic styles and those with their own instruments change the sounds when you choose them. Neon Noir also brings its own effects: the Noir hall, the ensemble chorus and an echo on the lead and bells. Choosing another style puts them back, unless you've changed them.

Pieces develop as they go. Each seed picks a song form, such as A A B A, A B A C or A B C A, from a larger set of chord progressions, and sections come back a little changed. The arrangement also changes from section to section:
- **Breakdown:** somewhere past the middle, the bass, pulse and the keys' left hand drop away, and the tune often returns an octave higher afterwards.
- **Rests and thinning:** elsewhere the tune may rest, the accompaniment thin out, or the pad drop out.
- **Opening and ending:** the first and last sections play as the style wrote them.

Each style also comes in variants, which the seed chooses separately for each part of the texture. Depending on the style, these include the accompaniment figure, the bass line, the voicing, the rhythm of the pulse or comping, the drone and the phrasing of the tune. For example, Gymnopédie Waltz may roll its chords or alternate root and fifth in the bass, Berlin School may loop its sequence every 12 steps against the bar, and Dub Chords may echo in quavers, dotted quavers or crotchets. Two seeds of the same style can sound like different pieces.

Recipes saved by earlier versions keep their `"version"` and play exactly as before.

## Controls

- **Key / mode**: any root note, in major, lydian, mixolydian, dorian, minor or phrygian.
- **Instruments**: piano, strings, or both. Each style re-orchestrates itself for the choice you make. Scores always play their whole orchestra.
- **Length**: set a target duration (mm:ss) or a number of bars. The piece ends on the tonic, with a ring-out.
- **Density, Variation, Dynamics, Humanize**: shape the result. Density thins or fills every part: melody, accompaniment rhythm, bass movement and how soon strings and pads join in. Each slider is labelled at both ends with which way is less and which is more, such as sparse ◀ ▶ busy.
- **Parts**: each piece is arranged into up to five parts: Keys, Pad, Lead, Bass and Pluck. Scores add Brass, Drums and FX. For each part you can:
  - switch it on or off. Switching a part off leaves the others note-for-note identical, and the piece keeps the same length.
  - choose its sound from about 45 General MIDI sounds: pianos, electric piano, celesta, glockenspiel, music box, vibraphone, marimba, kalimba, tubular bells, harp, guitars, koto, strings, choirs, pads, flute, pan flute, shakuhachi, clarinet, oboe, bassoon, French horn, brass section, trombone, tenor sax and synth brass. The drums choose a drum kit.
  Both choices are saved in the recipe and in the MIDI file. Parts the current style doesn't use are greyed out.
- **Effects & tuning**: these change the sound but not the notes, and apply straight away while the piece plays:
  - **Space**: Room, Plate (bright and dense, blooming at once), Hall, Cathedral, Noir (a vast, dark 1980s digital hall: a long pause before it blooms, then a slow, chorused tail), Shimmer (the tail rises an octave), Cloud (granular: the reverb is fed scattered, slightly detuned fragments of what was just played) or Black Hole (enormous, slowly swirling and almost endless).
  - **Amount** (dry ◀ ▶ wet, also written to the MIDI file as CC91), **Size** (short ◀ ▶ long), **Tone** (dark ◀ ▶ bright) and **Motion** (still ◀ ▶ swirling; in Cloud, how far the grains wander).
  - With **Shimmer**, three more controls appear. **Feedback** (subtle ◀ ▶ intense) sets how much of the pitched tail is fed back in; more climbs higher and leaves more shimmer artefacts. **Feedback detune** (pure ◀ ▶ wide) splits that feedback into a sharp and a flat copy, up to 35 cents either side, so they beat against each other and spread across the stereo field. **Interval** pitches the tail an octave up, a fifth up, two octaves up or an octave down.
  - **Pre-delay** adds up to a quarter second before the reverb starts, so each note is heard clearly first. **Low cut** keeps low notes out of the reverb (40 to 640 Hz). **Width** spreads the reverb from mono to extra wide. **Ducking** dips the reverb while the music plays, so it blooms in the gaps.
  - **Freeze the tail** holds the reverb as it is, ringing on forever, while new notes play dry over it.
  - **Echo**: a tempo-synced echo (1/8 to 1/2, dotted values included) with **Repeats**, **Tone** (each repeat darker, like tape), **Wow**, **Into reverb** and **Ping-pong**. How much of each part goes into it is set on the mixer.
  - **Detune** pulls each part and note slightly off pitch and widens ensemble sounds. **Drift** adds a slow, tape-like pitch wander. **Tuning** sets A to 440, 432 or 415 Hz. **Ensemble** (off ◀ ▶ lush) is a string-machine chorus over every part but the bass, turning each sound into a slightly out-of-tune ensemble, like a 1970s polysynth.
  They work with both the built-in synth and SoundFonts, and are saved in the recipe. Tuning, detune, drift and the ensemble aren't written to the MIDI file.
- **Mixer** (the **Mixer** button below the piano roll): a stereo mixer with a channel for each part. Each channel has a fader, a level meter, **Pan**, stereo **Width**, **Reverb** and **Echo** sends, and **M**ute and **S**olo; the **Master** has a fader and a stereo meter. Parts can share a channel: **+ Channel** adds one (up to eight), and its **⋯** menu names it and chooses which parts play through it, so one fader moves them together. Double-click a fader or slider to reset it. The mixer is saved in the recipe and the project. Exported WAV files are mixed as you hear them. Saved MIDI files carry each part's fader, pan and reverb send as volume (CC7), pan (CC10) and reverb (CC91); mutes and solos are left out of the MIDI file, since they are for listening.
- **Note expression**: long notes of the tune and bass get vibrato that grows after the note starts, the longest notes of the tune swell, and now and then the tune slides up into a note after a leap. This is written to the MIDI file as modulation (CC1, for vibrato), expression (CC11) and pitch bend. Only sustaining sounds (strings, winds, pads) get it, and only pieces made by version 3 of the generator. Neon Noir plays its own way, like a CS-80: its brass chords sag in pitch as they die away in the breath at the end of a phrase, and its lead has a deeper vibrato and slides up into notes more often.
- **Editing notes**: turn on **Edit notes** above the piano roll, then click a note to switch it off (it stays as an outline, and clicking it again switches it back on). Drag across notes to switch several. Right-click any note, in or out of edit mode, to set its velocity and shape it: **Swell** (below 0 it fades away, above 0 it swells up and back), **Vibrato** and **Bend** (above 0 it slides up into the note, below 0 it falls away at the end). The shapes act on the part's channel, so on parts that play chords they shape the other notes sounding at the time too. Edits belong to the piece they were made on: a new seed or setting that changes the notes leaves them aside, and they come back if you return to it. **Undo edits** removes them all. They are saved in the recipe, the project and the MIDI file. While the piece plays, editing a note doesn't interrupt it.
- **Zoom**: pinch, or scroll with ⌘ (Ctrl on Windows), on the piano roll to zoom in around the pointer; scroll to move along. **Fit** shows the whole piece again.
- **Seed**: the same settings and seed always give exactly the same piece. Each musical layer (harmony, melody, texture…) has its own random stream, so changing one setting doesn't reshuffle everything else. Use 🎲 for a new seed, ◀ ▶ to step through seeds, or ✨ to randomise every setting.
- **Recipe**: a JSON copy of all the settings. Copy it, paste it back with Apply, or use **From .mid…** to recreate a piece from any MIDI file this app saved. The recipe is stored inside the file.

## Sound

The built-in synth (piano, electric piano, mallets and bells, plucked strings, bowed strings, choirs and pads, winds, and a hall reverb) works without any other files. Every part plays through its own engine into the mixer, which feeds one shared reverb and echo. The reverb is a modulated feedback-delay-network design with early reflections and eight spaces, and SoundFonts are played through it too. For more realistic instruments, load SoundFonts (.sf2):

- **Main sound**: the built-in synth or a General MIDI SoundFont, such as GeneralUser GS or FluidR3_GM. It plays every part that doesn't have its own preset.
- **Per part**: each part can instead play a preset from any loaded SoundFont, such as a felt piano font for Keys and a choir font for Pad. In **Parts**, choose the font in the middle drop-down and the preset in the right one.
- **Assigning**: after **Load SoundFont (.sf2)…**, a dialog suggests which parts the font should play. A General MIDI font is offered as the main sound. For other fonts, parts are pre-ticked where a preset's name matches the part, such as "cello" for Bass. Reopen the dialog any time with **Assign…**.

You can load several SoundFonts at once. They are used for both playback and WAV export. Per-part presets are saved in the recipe by font file name; if a recipe names a font that isn't loaded, that part falls back to its General MIDI sound. Saved MIDI files include the preset's bank and program.

This repo includes two SoundFonts in `soundfonts/`, and the app loads both at startup:

- [GeneralUser GS](https://schristiancollins.com/generaluser.php) by S. Christian Collins, a General MIDI font and the main sound. Its licence is in `soundfonts/GeneralUser-GS-LICENSE.txt`.
- **Ambient Electronic**, 16 synthesiser presets made for this app: Sine Keys, FM Electric Piano, Analog Sub Bass, Analog Pluck, Supersaw Strings, CS Brass (detuned saws behind a filter that swells open, CS-80 style), Smoky Lead (a breathy, reedy lead), Soft Saw Lead, Warm Analog Pad, Glass Pad, Airy Choir, Halo Pad, Sweep Pad and Crystal Bells, plus the scores' **Trailer Kit** and **Trailer FX**. They sit at their General MIDI program numbers: the Trailer Kit is in the drum bank at the Orchestra kit's place (48), and Trailer FX at Reverse Cymbal (119). The kit's keys follow the General MIDI drum map (heartbeat kick 35, kick 36, tick 37, snare 38, clap 39, taiko toms 41, 43 and 45, filtered hat 42, crash 49). On Trailer FX, keys 24–35 hold the sub drone, 36–47 the sub booms and 48–59 the braams (both an octave below the key), 60 and 61 risers of 4 and 8 seconds, 62 and 63 reversed cymbals of 2 and 4 seconds, 64 a whoosh, 65 a downlifter, 67 an impact, and 72–83 the glassy grains. The built-in synth plays the same kit and effects, so scores sound much the same without the SoundFont. It contains no recorded samples: `examples/make_soundfont.rs` synthesises every sound, and `cargo run --release --example make_soundfont` rebuilds the file exactly. It is under the MIT License, like the code.

To build a macOS app bundle with the icon and SoundFont, run `./scripts/bundle_macos.sh`. The bundle is written to `dist/`.

Saved MIDI files are standard type-1 files with one track per instrument and General MIDI programs set. They open in any DAW. A score's drums are on the General MIDI drum channel (10). Its FX track is meant for the Trailer FX preset: a General MIDI player plays it as reversed cymbals.

Click or drag on the piano roll to seek. Space bar plays and pauses.

## Slideshows

The **Slideshow** section fits a photo slideshow (for example, a YouTube video) to the music. Each image stays on screen for 4 to 8 seconds, crossfade included. Images change on bar lines, at the whole number of bars nearest the time you choose (an even number when two are equally near, since chords often last two bars). Only when no whole number of bars fits 4 to 8 seconds do they change on half bars, or on beats. Set an intro (music before the first image, e.g. for a title) and an outro (music after the last, e.g. for credits) in bars. The audio ends with 3 seconds of reverb tail after the outro.

- **Images for track** counts how many images the current track holds. Any time too short for one more image goes to the outro.
- **Track for images** takes a number of images and sets the piece's length in bars to fit them, intro and outro included.

The plan shows the time per image (and how much of it is clear of the fades), the intro, the images, the outro and the track's length. **Copy cue list** and **Save cue list…** give each image's start and end time and its bar and beat as CSV, for your video editor. A start is when that image begins to fade in.

Many editors (iMovie, Clipchamp, CapCut, Photos) overlap neighbouring clips to crossfade them. That pulls every later image earlier by the fade, so the images drift off the beat. In those editors, make each image as long as the cue list's `clip` column (its time plus the fade). The image changes can also be marked on the piano roll. The slideshow settings are saved in projects.

## Projects

**💾 Save project** (⌘S / Ctrl+S) saves everything in one `.ambient` file:
- every setting, including parts, sounds, effects and tuning, the mixer, note edits, and the seed
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

To check the interface without clicking through it, run the app with `MIDIGEN_SCREENSHOT=window.bmp`. It saves a picture of its window and quits, using its own settings rather than yours. `MIDIGEN_SCREENSHOT_SHOW` can open parts first, comma-separated: `sections`, `sound`, `fonts`, `slideshow`, `images` (the slideshow fitting a track to 50 images), `electronic`, `noir`, `trailer` (start on the Tech Trailer score), `mixer`, `about` or `libraries`.

## Licence

The library list and licence texts in **About** come from `assets/licences.json`. After changing dependencies, regenerate it with `python3 scripts/gen_licenses.py`. A test fails if the list is out of date.

The code is released under the [MIT License](LICENSE). The bundled GeneralUser GS SoundFont in `soundfonts/` is not covered by it. It has its own licence, in `soundfonts/GeneralUser-GS-LICENSE.txt`. The Ambient Electronic SoundFont is synthesised by this project and is under the MIT License (`soundfonts/Ambient-Electronic-LICENSE.txt`).

The interface theme and widgets are adapted from BW Darkroom and LightCraft (MIT, `assets/LICENSE-LightCraft-MIT`). The Inter typeface is under the SIL Open Font License 1.1 (`assets/fonts/OFL-Inter.txt`).
