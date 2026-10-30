//! Loaded SoundFonts, and suggestions for which preset each part should use.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::song::{FontPreset, Inst, sound_name};

#[derive(Clone, Debug, PartialEq)]
pub struct PresetInfo {
    pub bank: u8,
    pub preset: u8,
    pub name: String,
}

pub struct LoadedFont {
    /// File name, which is how parts refer to the font.
    pub name: String,
    pub font: Arc<rustysynth::SoundFont>,
    /// Melodic presets, sorted by bank then program. Drum kits are left out.
    pub presets: Vec<PresetInfo>,
    /// Where the font came from, so a project can save it.
    pub source: Option<FontSource>,
}

#[derive(Clone)]
pub enum FontSource {
    File(PathBuf),
    /// Loaded from inside a project file.
    Bytes(Arc<Vec<u8>>),
}

/// Words that suggest a preset suits a part.
fn role_words(inst: Inst) -> &'static [&'static str] {
    match inst {
        Inst::Piano => &["piano", "keys", "rhodes", "wurli", "grand", "celesta"],
        Inst::StringPad => &["pad", "string", "choir", "ensemble", "warm", "aah", "ooh", "halo", "atmos"],
        Inst::Violin => &["violin", "flute", "solo", "lead", "oboe", "clarinet", "whistle"],
        Inst::Cello => &["cello", "bass", "contrabass", "bassoon"],
        Inst::Pizzicato => &["pizz", "harp", "pluck", "guitar", "kalimba", "marimba"],
    }
}

impl LoadedFont {
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let font = rustysynth::SoundFont::new(&mut f).map_err(|e| format!("{e:?}"))?;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut loaded = Self::from_font(name, Arc::new(font));
        loaded.source = Some(FontSource::File(path.to_path_buf()));
        Ok(loaded)
    }

    pub fn from_bytes(name: String, bytes: Vec<u8>) -> Result<Self, String> {
        let font = rustysynth::SoundFont::new(&mut std::io::Cursor::new(&bytes)).map_err(|e| format!("{e:?}"))?;
        let mut loaded = Self::from_font(name, Arc::new(font));
        loaded.source = Some(FontSource::Bytes(Arc::new(bytes)));
        Ok(loaded)
    }

    /// The font file's contents, for saving inside a project.
    pub fn bytes(&self) -> Result<Arc<Vec<u8>>, String> {
        match &self.source {
            Some(FontSource::File(p)) => std::fs::read(p).map(Arc::new).map_err(|e| format!("{}: {e}", p.display())),
            Some(FontSource::Bytes(b)) => Ok(b.clone()),
            None => Err(format!("{} has no source file", self.name)),
        }
    }

    pub fn from_font(name: String, font: Arc<rustysynth::SoundFont>) -> Self {
        let mut presets: Vec<PresetInfo> = font
            .get_presets()
            .iter()
            .filter(|p| (0..128).contains(&p.get_bank_number()) && (0..128).contains(&p.get_patch_number()))
            .map(|p| PresetInfo {
                bank: p.get_bank_number() as u8,
                preset: p.get_patch_number() as u8,
                name: p.get_name().trim().to_string(),
            })
            .collect();
        presets.sort_by_key(|p| (p.bank, p.preset));
        presets.dedup_by_key(|p| (p.bank, p.preset));
        LoadedFont { name, font, presets, source: None }
    }

    pub fn is_general_midi(&self) -> bool {
        is_general_midi(&self.presets)
    }

    pub fn find(&self, bank: u8, preset: u8) -> Option<&PresetInfo> {
        self.presets.iter().find(|p| p.bank == bank && p.preset == preset)
    }

    pub fn preset_for(&self, p: &PresetInfo) -> FontPreset {
        FontPreset { font: self.name.clone(), bank: p.bank, preset: p.preset, name: p.name.clone() }
    }

    pub fn suggest(&self, inst: Inst, program: u8) -> Option<(&PresetInfo, bool)> {
        suggest(&self.presets, inst, program)
    }
}

/// Does bank 0 cover (nearly) the whole General MIDI set?
pub fn is_general_midi(presets: &[PresetInfo]) -> bool {
    presets.iter().filter(|p| p.bank == 0).count() >= 100
}

/// The best preset for a part currently set to General MIDI `program`, and
/// whether it's a confident match. A General MIDI font gives the same program;
/// otherwise preset names are matched against the sound's name and the part's
/// role. With no match, the first preset is offered.
pub fn suggest(presets: &[PresetInfo], inst: Inst, program: u8) -> Option<(&PresetInfo, bool)> {
    if is_general_midi(presets) {
        if let Some(p) = presets.iter().find(|p| p.bank == 0 && p.preset == program) {
            return Some((p, true));
        }
    }
    let sound = sound_name(program).to_lowercase();
    let sound_words: Vec<&str> = sound.split_whitespace().filter(|w| w.len() > 3).collect();
    let score = |p: &PresetInfo| {
        let name = p.name.to_lowercase();
        sound_words.iter().filter(|w| name.contains(*w)).count() * 2
            + role_words(inst).iter().filter(|w| name.contains(*w)).count()
    };
    // highest score, then bank 0, then the earliest preset
    let best = presets
        .iter()
        .max_by_key(|p| (score(p), p.bank == 0, std::cmp::Reverse((p.bank, p.preset))))?;
    Some((best, score(best) > 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(names: &[(u8, u8, &str)]) -> Vec<PresetInfo> {
        names.iter().map(|(b, p, n)| PresetInfo { bank: *b, preset: *p, name: n.to_string() }).collect()
    }

    #[test]
    fn suggests_by_name_for_small_fonts() {
        let f = list(&[(0, 0, "Felt Piano"), (0, 1, "Soft Strings"), (0, 2, "Cello Sustain"), (0, 3, "Harp")]);
        assert_eq!(suggest(&f, Inst::Piano, 0).unwrap().0.name, "Felt Piano");
        assert_eq!(suggest(&f, Inst::StringPad, 49).unwrap().0.name, "Soft Strings");
        assert_eq!(suggest(&f, Inst::Cello, 42).unwrap().0.name, "Cello Sustain");
        assert_eq!(suggest(&f, Inst::Pizzicato, 46).unwrap().0.name, "Harp");
        // nothing fits a flute: offer something, but not confidently
        let (p, sure) = suggest(&f, Inst::Violin, 73).unwrap();
        assert_eq!(p.name, "Felt Piano");
        assert!(!sure);
        assert!(suggest(&[], Inst::Piano, 0).is_none());
    }

    #[test]
    fn general_midi_font_keeps_programs() {
        let Ok(f) = LoadedFont::load(Path::new("soundfonts/GeneralUser-GS.sf2")) else { return };
        assert!(f.is_general_midi());
        for inst in Inst::ALL {
            let (p, sure) = f.suggest(inst, 52).unwrap();
            assert!(sure);
            assert_eq!((p.bank, p.preset), (0, 52));
        }
    }

    /// A font embedded in a project (loaded from bytes) matches the file, and
    /// gives its bytes back for the next save.
    #[test]
    fn embedded_font_matches_file() {
        let path = Path::new("soundfonts/GeneralUser-GS.sf2");
        let from_file = LoadedFont::load(path).unwrap();
        let bytes = from_file.bytes().unwrap();
        let from_bytes = LoadedFont::from_bytes(from_file.name.clone(), bytes.to_vec()).unwrap();
        assert_eq!(from_file.presets, from_bytes.presets);
        assert_eq!(from_bytes.bytes().unwrap().len(), std::fs::metadata(path).unwrap().len() as usize);
        assert!(LoadedFont::from_bytes("x.sf2".into(), b"not a font".to_vec()).is_err());
    }
}
