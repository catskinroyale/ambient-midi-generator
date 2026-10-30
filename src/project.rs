//! Project files (.ambient): every setting, plus the loaded SoundFonts. Fonts
//! the user loaded are stored inside the file so the project opens anywhere;
//! the SoundFont bundled with the app is only referred to.
//!
//! Layout: the magic line, the JSON header's length (u64, little-endian), the
//! JSON header, then the embedded fonts' bytes one after another, in the order
//! and with the lengths the header lists.

use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::song::Params;

pub const EXTENSION: &str = "ambient";
const MAGIC: &[u8] = b"AMBIENT-PROJECT\n";
const FORMAT: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format: u32,
    /// The app version that saved it, for information.
    pub app_version: String,
    pub params: Params,
    /// The font that plays every part without its own preset; None is the built-in synth.
    pub main_font: Option<String>,
    pub fonts: Vec<FontEntry>,
    pub settings: Settings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontEntry {
    /// File name, which is how parts refer to the font.
    pub name: String,
    /// Where it was loaded from, used if it isn't embedded.
    pub path: Option<String>,
    /// The font shipped with the app.
    pub bundled: bool,
    /// Length of the embedded font data, if it is embedded.
    pub embedded: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub volume: f32,
    pub looping: bool,
    pub auto_regen: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { volume: 0.9, looping: false, auto_regen: true }
    }
}

/// Write a project. `data` holds the bytes for each font with `embedded` set,
/// in order.
pub fn save(path: &Path, project: &Project, data: &[&[u8]]) -> Result<(), String> {
    let header = serde_json::to_vec_pretty(project).map_err(|e| e.to_string())?;
    let embedded: Vec<u64> = project.fonts.iter().filter_map(|f| f.embedded).collect();
    if embedded.len() != data.len() || embedded.iter().zip(data).any(|(n, d)| *n != d.len() as u64) {
        return Err("embedded font data doesn't match the header".into());
    }
    // write to a temporary file first, so a failed save never damages the old one
    let tmp = path.with_extension(format!("{EXTENSION}.tmp"));
    let write = || -> std::io::Result<()> {
        let mut f = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        f.write_all(MAGIC)?;
        f.write_all(&(header.len() as u64).to_le_bytes())?;
        f.write_all(&header)?;
        for d in data {
            f.write_all(d)?;
        }
        f.into_inner().map_err(|e| e.into_error())?.sync_all()
    };
    write().and_then(|_| std::fs::rename(&tmp, path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Read a project and the bytes of each embedded font, in order.
pub fn load(path: &Path) -> Result<(Project, Vec<Vec<u8>>), String> {
    let mut f = std::io::BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?);
    let bad = || "not an Ambient MIDI Generator project".to_string();
    let mut magic = vec![0u8; MAGIC.len()];
    f.read_exact(&mut magic).map_err(|_| bad())?;
    if magic != MAGIC {
        return Err(bad());
    }
    let mut len = [0u8; 8];
    f.read_exact(&mut len).map_err(|_| bad())?;
    let len = u64::from_le_bytes(len);
    if len > 64 << 20 {
        return Err(bad());
    }
    let mut header = vec![0u8; len as usize];
    f.read_exact(&mut header).map_err(|_| bad())?;
    let project: Project = serde_json::from_slice(&header).map_err(|e| format!("damaged project: {e}"))?;
    if project.format > FORMAT {
        return Err("this project was saved by a newer version of the app".into());
    }
    let mut data = Vec::new();
    for n in project.fonts.iter().filter_map(|f| f.embedded) {
        let mut d = Vec::new();
        f.by_ref().take(n).read_to_end(&mut d).map_err(|e| e.to_string())?;
        if d.len() as u64 != n {
            return Err("the project file is incomplete".into());
        }
        data.push(d);
    }
    Ok((project, data))
}

impl Project {
    pub fn new(params: Params, main_font: Option<String>, fonts: Vec<FontEntry>, settings: Settings) -> Self {
        Project { format: FORMAT, app_version: crate::update::VERSION.into(), params, main_font, fonts, settings }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::{Fx, Inst, Space, Style};

    #[test]
    fn round_trip_with_embedded_fonts() {
        let dir = std::env::temp_dir().join(format!("midigen-project-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.ambient");
        let mut params = Params { style: Style::Canon, seed: 42, ..Params::default() };
        params.fx = Fx { space: Space::Shimmer, shimmer: 0.9, ..Fx::default() };
        params.muted = vec![Inst::Cello];
        let fonts = vec![
            FontEntry { name: "a.sf2".into(), path: Some("/x/a.sf2".into()), bundled: false, embedded: Some(5) },
            FontEntry { name: "GeneralUser-GS.sf2".into(), path: None, bundled: true, embedded: None },
            FontEntry { name: "b.sf2".into(), path: None, bundled: false, embedded: Some(3) },
        ];
        let settings = Settings { volume: 0.5, looping: true, auto_regen: false };
        let project = Project::new(params, Some("a.sf2".into()), fonts, settings);
        save(&path, &project, &[b"hello", b"sf2"]).unwrap();
        let (back, data) = load(&path).unwrap();
        assert_eq!(back, project);
        assert_eq!(data, vec![b"hello".to_vec(), b"sf2".to_vec()]);
        // mismatched data is refused, and leaves the saved file alone
        assert!(save(&path, &project, &[b"hello"]).is_err());
        assert!(load(&path).is_ok());
        // other files are rejected
        std::fs::write(&path, b"not a project").unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
