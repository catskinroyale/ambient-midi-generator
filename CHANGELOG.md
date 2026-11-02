# Changelog

All notable changes to this project are documented here. The project uses
[semantic versioning](https://semver.org/).

## [Unreleased]

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

[Unreleased]: https://github.com/catskinroyale/ambient-midi-generator/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/catskinroyale/ambient-midi-generator/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/catskinroyale/ambient-midi-generator/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/catskinroyale/ambient-midi-generator/releases/tag/v0.1.0
