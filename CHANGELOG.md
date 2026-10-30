# Changelog

All notable changes to this project are documented here. The project uses
[semantic versioning](https://semver.org/).

## [Unreleased]

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

[Unreleased]: https://github.com/catskinroyale/ambient-midi-generator/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/catskinroyale/ambient-midi-generator/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/catskinroyale/ambient-midi-generator/releases/tag/v0.1.0
