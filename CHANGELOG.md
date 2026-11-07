# Changelog

All notable changes to this project are documented here. The project uses
[semantic versioning](https://semver.org/).

## [Unreleased]

### Added
- Scores: pieces written as one arc (intro, build, peak and outro, scaled to the chosen length) for a full orchestra, listed under **Scores** in the Style menu. The first is **Tech Trailer**, a hybrid trailer score for technology and the future: a lone felt piano motif over a sub drone and glassy grains; a build of pulsing synth arpeggio, heartbeat kick, ticking percussion, low string ostinato, rising strings, brass swells, risers and braams; a peak of big drums, sub hits, braams, brass and choir that can double up in the final third; and a lone piano to close on a soft hit. The seed varies the motif, arpeggio, ostinato, drums, brass and chords. Choosing a score switches a major key to minor and sets up its effects and mix.
- Three parts for the scores: Brass, Drums (on the General MIDI drum channel, with a choice of drum kit) and FX. They only show in Parts, the mixer and the assign dialog when the piece uses them.
- Trailer Kit and Trailer FX presets in the Ambient Electronic SoundFont (16 in all): heartbeat and trailer kicks, taiko toms, snare, clap, ticks, hats and crash; a sub drone, 808-style sub booms, braams, risers, reversed cymbals, a whoosh, a downlifter, an impact and glassy grains, all synthesised. The built-in synth plays the same sounds.
- Brass Section and Trombone sounds.

### Changed
- The mixer's **Reset** returns to the style's own mix, rather than always to plain channels.
- The part legend under the piano roll shows only the parts' names when their sounds don't fit beside the piece's details.

## [0.5.0] - 2026-11-04

### Added
- Neon Noir, an electronic style in the spirit of Vangelis's *Blade Runner* (21 styles in all): CS-80-style brass swells that sag in pitch at the end of each phrase, a smoky lead with deep vibrato and slides over a low drone, bells falling like rain, and in some pieces a slow heartbeat or a ticking pulse. Its variants change how the brass enters, the pulse, the lead, the rain and the drone. Choosing it also sets its effects and echo sends.
- CS Brass and Smoky Lead presets in the Ambient Electronic SoundFont (14 in all).
- The Noir reverb space: a vast, dark 1980s digital hall with a long pre-delay and a slow, chorused tail.
- Ensemble: a string-machine chorus over every part but the bass.
- Style variants (generator version 3): the seed chooses among different treatments of each style, such as the accompaniment figure, bass line, voicing, pulse or comping rhythm, drone and the tune's phrasing, so pieces in the same style differ in texture as well as notes. Recipes from earlier versions play exactly as before.
- A stereo mixer: a channel strip for each part with a fader, level meter, pan, stereo width, reverb and echo sends, mute and solo, and a master fader with a stereo meter. Parts can be moved onto shared channels (up to eight), so one fader moves them together. The mix is saved in recipes and projects, applied to WAV export, and written to MIDI files as volume, pan and reverb send.
- Note editing in the piano roll: **Edit notes** lets you click notes to switch them off and on, or drag across several. Right-click a note to set its velocity, swell or fade, vibrato, and a slide into or away from it. Edits are kept per piece and saved in recipes, projects and MIDI files, and playback carries on while you edit.
- Note expression (generator version 3): vibrato on long notes of the tune and bass, swells on the longest notes of the tune, and occasional slides up into a note after a leap, for sustaining sounds. Written to MIDI files as CC1, CC11 and pitch bend. Falls carry on sagging into the note's release, and the notes of a chord share one slide.
- Piano roll zoom: pinch or ⌘/Ctrl + scroll around the pointer, scroll to move along, and **Fit**. Bar numbers show when zoomed in.
- Reverb: Plate and Cloud (granular) spaces; pre-delay, low cut, width and ducking controls; Freeze, which holds the tail forever; and shimmer intervals (a fifth up, two octaves up, an octave down) as well as an octave up.
- A tempo-synced echo with repeats, tone, tape wow, ping-pong and a send into the reverb, fed from the mixer's echo sends.

### Changed
- "Reverb & Tuning" is now "Effects & Tuning".
- Each part plays through its own synth engine, and SoundFont parts each get their own reverb send rather than sharing the loudest one.

## [0.4.0] - 2026-11-03

### Added
- Four new styles (twenty in all): Celtic Air, Koto Garden, Gamelan Shimmer and Fingerstyle Guitar.
- Sound palettes: every style has sounds that suit each part. With **Sounds follow the seed** (on by default), each new seed or style picks from them. **Style's sounds** and **Shuffle** buttons in Parts.
- Koto, Steel Guitar, Tubular Bells and Shakuhachi sounds.
- Song forms: each seed picks a form (A A B A, A B A C, A A B B A, A B C A and longer ones), from more chord progressions, including I-IV-ii-V, IV-V-iii-vi and the Andalusian i-VII-VI-v. Sections that come back are a little changed.
- Arrangement that develops: a breakdown past the middle (bass, pulse and left hand drop away), often followed by the tune an octave higher, and elsewhere sections where the tune rests, the accompaniment thins or the pad drops out.

### Changed
- Recipes carry a generator `version`. Recipes without one, saved by earlier versions, play exactly as before.
- "Default sounds" in Parts is now "Style's sounds".

### Fixed
- Slideshow images change on whole bars whenever a number of bars fits 4 to 8 seconds, preferring an even number, instead of on half bars, where every other change fell mid-bar with nothing playing. Half bars and beats are only used when a bar is too long or short.
- The slideshow plan counts the 3-second reverb tail at the end of the audio, and exported WAV files are now exactly the plan's length (they used to run past it to the end of a 4096-sample block).
- The cue list has a `clip` column: the length to give each image in editors that overlap clips to crossfade them (iMovie, Clipchamp, CapCut, Photos), so the images don't drift earlier by the fade at every change.

## [0.3.0] - 2026-11-02

### Added
- Four new styles (sixteen in all): Berlin School, Dub Chords and Aurora (electronic), and Music Box Lullaby. Choosing one sets the parts to the sounds it is written for.
- The Ambient Electronic SoundFont, bundled: 12 synthesiser presets (keys, FM electric piano, sub bass, pluck, supersaw strings, saw lead, warm, glass, halo and sweep pads, airy choir, crystal bells), synthesised from code by `examples/make_soundfont.rs` and released under the MIT License.
- Synth Bass, Saw Lead and Crystal sounds for the built-in synth.
- Slideshow planning: count the images (4 to 8 seconds each, crossfade included) that a track holds after an intro and before an outro, or set the track's length for a number of images. Images change on bars, half bars or beats. Copy or save a CSV cue list, and see the image changes on the piano roll.
- `MIDIGEN_SCREENSHOT` for automated UI checks: the app saves a picture of its window and quits.

### Changed
- A new look following BW Darkroom and LightCraft: Charcoal and Silver themes, the Inter typeface, slider rows with the value beside the label and less/more words under the track (double-click to reset, click the value to type, Shift for fine control), segmented buttons, accordion sections, a top bar and a transport toolbar with line icons.
- The theme button steps through Auto, Light and Dark; the control panel can be hidden.
- Every bundled SoundFont is loaded at startup; the General MIDI one is the main sound.

### Fixed
- Opening Sound Fonts no longer adds a screenful of empty space under each SoundFont, which pushed Recipe off the screen.

## [0.2.0] - 2026-10-30

### Added
- Five new styles: Tintinnabuli, Hymn Chorale, Canon on a Ground, Tidal Drone and Felt Ballad (twelve in all).
- SoundFonts per part: any part can play a preset from any loaded SoundFont. After loading a font, a dialog suggests which parts it should play.
- A new reverb: a modulated feedback-delay network with early reflections, used by the built-in synth and SoundFonts alike.
- Reverb spaces: Room, Hall, Cathedral, Shimmer and Black Hole, with Size, Tone and Motion controls. Shimmer adds Feedback and Feedback detune.
- Tuning: Detune, Drift (tape-like wander) and reference pitch (A = 440, 432 or 415 Hz).
- Projects: save and open `.ambient` files with every setting and the loaded SoundFonts embedded (⌘S / ⌘O).
- Dark and light themes with a switch; collapsible control sections; a resizable control panel.
- An About window with the version and git commit, this app's and the SoundFont's licences, and every third-party library with its licence and full licence text.
- An update check against GitHub releases, at startup (optional) and from About or the macOS app menu.
- A macOS app menu with About and Check for Updates.
- Windows builds, tested and packaged by GitHub Actions.

### Changed
- Density now shapes every part (accompaniment rhythm, bass movement, when strings and pads join), not only the melody. The default density gives the same notes as before.
- Sliders are labelled at both ends with which way is less and which is more.
- Long SoundFont and preset names are shortened to fit instead of overflowing the panel.

## [0.1.0] - 2026-10-26

First release: seven ambient styles, seeded and repeatable generation, a built-in synth, GeneralUser GS SoundFont playback, MIDI and WAV export, and per-part switches and sounds.

[Unreleased]: https://github.com/catskinroyale/ambient-midi-generator/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/catskinroyale/ambient-midi-generator/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/catskinroyale/ambient-midi-generator/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/catskinroyale/ambient-midi-generator/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/catskinroyale/ambient-midi-generator/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/catskinroyale/ambient-midi-generator/releases/tag/v0.1.0
