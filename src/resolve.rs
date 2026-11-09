//! Handing a slideshow to a video editor such as DaVinci Resolve: the
//! photos in a folder, a timeline with each photo at its change and a cross
//! dissolve between them over the soundtrack (FCPXML, which Resolve imports
//! with File › Import › Timeline), and the image changes as timeline markers
//! (an EDL, for Timelines › Import › Timeline Markers from EDL).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::slideshow::Plan;

/// The order photos are shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhotoOrder {
    /// By file name, numbers counted as numbers (IMG_2 before IMG_10).
    Name,
    /// By when each file was last modified, oldest first.
    Date,
}

/// The frame rates offered for the timeline.
pub const FRAME_RATES: [u32; 5] = [24, 25, 30, 50, 60];

const PHOTO_TYPES: [&str; 11] = ["jpg", "jpeg", "png", "tif", "tiff", "heic", "heif", "bmp", "webp", "dng", "gif"];

/// The photos in a folder (not its subfolders), in order.
pub fn photos(folder: &Path, order: PhotoOrder) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(folder).map_err(|e| format!("Could not read {}: {e}", folder.display()))?;
    let mut found: Vec<(PathBuf, std::time::SystemTime)> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let hidden = p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'));
            let photo = p.extension().and_then(|x| x.to_str()).is_some_and(|x| PHOTO_TYPES.contains(&x.to_ascii_lowercase().as_str()));
            p.is_file() && photo && !hidden
        })
        .map(|p| {
            let modified = std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            (p, modified)
        })
        .collect();
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    match order {
        PhotoOrder::Name => found.sort_by(|a, b| natural(&name(&a.0), &name(&b.0))),
        PhotoOrder::Date => found.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| natural(&name(&a.0), &name(&b.0)))),
    }
    Ok(found.into_iter().map(|(p, _)| p).collect())
}

/// Compare names with runs of digits as numbers.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| {
        let mut parts: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let digit = c.is_ascii_digit();
            match parts.last_mut() {
                Some((d, run)) if *d == digit => run.push(c),
                _ => parts.push((digit, c.to_string())),
            }
        }
        parts
    };
    let (x, y) = (split(a), split(b));
    for (p, q) in x.iter().zip(y.iter()) {
        let ord = match (p.0, q.0) {
            (true, true) => {
                let (m, n) = (p.1.trim_start_matches('0'), q.1.trim_start_matches('0'));
                m.len().cmp(&n.len()).then_with(|| m.cmp(n))
            }
            _ => p.1.cmp(&q.1),
        };
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }
    x.len().cmp(&y.len()).then_with(|| a.cmp(b))
}

/// The pixel size of a JPEG or PNG, read from its header.
pub fn image_size(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(path).ok()?.take(256 * 1024).read_to_end(&mut head).ok()?;
    if head.starts_with(b"\x89PNG\r\n\x1a\n") && head.len() >= 24 {
        let be = |i: usize| u32::from_be_bytes([head[i], head[i + 1], head[i + 2], head[i + 3]]);
        return Some((be(16), be(20)));
    }
    if head.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 9 < head.len() {
            if head[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = head[i + 1];
            let len = u16::from_be_bytes([head[i + 2], head[i + 3]]) as usize;
            // the start of frame markers, not the huffman, arithmetic or restart ones
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                let h = u16::from_be_bytes([head[i + 5], head[i + 6]]) as u32;
                let w = u16::from_be_bytes([head[i + 7], head[i + 8]]) as u32;
                return Some((w, h));
            }
            i += 2 + len;
        }
    }
    None
}

/// Where each photo goes, in whole frames: (photo, first frame, frames),
/// and the cross dissolve's length in frames. Only as many photos as the
/// plan has images, or images as there are photos.
fn frames(plan: &Plan, photos: usize, fade: f64, fps: u32) -> (Vec<(usize, u64, u64)>, u64) {
    let at = |secs: f64| (secs * fps as f64).round() as u64;
    let spans: Vec<(f64, f64)> = plan.spans().take(photos).collect();
    let clips: Vec<(usize, u64, u64)> =
        spans.iter().enumerate().map(|(i, (a, b))| (i, at(*a), at(*b).saturating_sub(at(*a)).max(1))).collect();
    // even, so it centres on the cut, and at most half the shortest image
    let shortest = clips.iter().map(|c| c.2).min().unwrap_or(0);
    let dissolve = (at(fade).min(shortest / 2) / 2) * 2;
    (clips, dissolve)
}

/// A timeline for Final Cut Pro XML 1.9, which DaVinci Resolve imports: the
/// photos on the video track at their changes, cross dissolves between
/// them, and the soundtrack beneath, at `fps` frames a second in 1080p.
pub fn fcpxml(plan: &Plan, photos: &[PathBuf], audio: &Path, audio_secs: f64, fade: f64, fps: u32, title: &str) -> String {
    let (clips, dissolve) = frames(plan, photos.len(), fade, fps);
    let t = |frames: u64| if frames == 0 { "0s".to_string() } else { format!("{frames}/{fps}s") };
    let audio_frames = (audio_secs * fps as f64).floor() as u64;
    let mut out = String::new();
    out += "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n<fcpxml version=\"1.9\">\n  <resources>\n";
    out += &format!(
        "    <format id=\"r1\" name=\"FFVideoFormat1080p{fps}\" frameDuration=\"1/{fps}s\" width=\"1920\" height=\"1080\" colorSpace=\"1-1-1 (Rec. 709)\"/>\n"
    );
    // a still's format: its size, where it can be read
    let mut sizes: Vec<Option<(u32, u32)>> = Vec::new();
    let mut format_of = Vec::new();
    for (i, _, _) in &clips {
        let size = image_size(&photos[*i]);
        let k = sizes.iter().position(|s| *s == size).unwrap_or_else(|| {
            sizes.push(size);
            sizes.len() - 1
        });
        format_of.push(k);
    }
    for (k, size) in sizes.iter().enumerate() {
        match size {
            Some((w, h)) => out += &format!("    <format id=\"f{k}\" name=\"FFVideoFormatRateUndefined\" width=\"{w}\" height=\"{h}\" colorSpace=\"1-13-1\"/>\n"),
            None => out += &format!("    <format id=\"f{k}\" name=\"FFVideoFormatRateUndefined\"/>\n"),
        }
    }
    for (j, (i, _, _)) in clips.iter().enumerate() {
        let p = &photos[*i];
        out += &format!(
            "    <asset id=\"p{j}\" name=\"{}\" src=\"{}\" start=\"0s\" duration=\"0s\" hasVideo=\"1\" format=\"f{}\" videoSources=\"1\"/>\n",
            xml(&file_name(p)),
            xml(&file_url(p)),
            format_of[j]
        );
    }
    out += &format!(
        "    <asset id=\"a1\" name=\"{}\" src=\"{}\" start=\"0s\" duration=\"{}\" hasAudio=\"1\" audioSources=\"1\" audioChannels=\"2\" audioRate=\"44100\"/>\n",
        xml(&file_name(audio)),
        xml(&file_url(audio)),
        t(audio_frames)
    );
    out += "  </resources>\n  <library>\n";
    out += &format!("    <event name=\"{}\">\n      <project name=\"{}\">\n", xml(title), xml(title));
    out += &format!(
        "        <sequence format=\"r1\" duration=\"{}\" tcStart=\"0s\" tcFormat=\"NDF\" audioLayout=\"stereo\" audioRate=\"48k\">\n          <spine>\n",
        t(audio_frames)
    );
    // offset in the time of the clip it hangs from
    let music = |offset: &str| {
        format!(
            "              <asset-clip ref=\"a1\" lane=\"-1\" offset=\"{offset}\" name=\"{}\" start=\"0s\" duration=\"{}\" audioRole=\"music\"/>\n",
            xml(&file_name(audio)),
            t(audio_frames)
        )
    };
    // the soundtrack hangs from the first thing on the timeline: the intro's
    // gap, or the first photo
    let first = clips.first().map(|c| c.1).unwrap_or(audio_frames);
    let mut hung = false;
    if first > 0 {
        out += &format!("            <gap name=\"Gap\" offset=\"0s\" start=\"0s\" duration=\"{}\">\n", t(first));
        out += &music("0s");
        hung = true;
        out += "            </gap>\n";
    }
    for (j, &(i, start, len)) in clips.iter().enumerate() {
        if j > 0 && dissolve > 0 {
            out += &format!("            <transition name=\"Cross Dissolve\" offset=\"{}\" duration=\"{}\"/>\n", t(start - dissolve / 2), t(dissolve));
        }
        let name = xml(&file_name(&photos[i]));
        // a still starts an hour in, as Final Cut writes them, so a dissolve
        // has picture to draw on before it
        let from = t(3600 * fps as u64);
        match std::mem::replace(&mut hung, true) {
            false => {
                out += &format!("            <video ref=\"p{j}\" offset=\"{}\" name=\"{name}\" start=\"{from}\" duration=\"{}\">\n", t(start), t(len));
                out += &music(&from);
                out += "            </video>\n";
            }
            true => out += &format!("            <video ref=\"p{j}\" offset=\"{}\" name=\"{name}\" start=\"{from}\" duration=\"{}\"/>\n", t(start), t(len)),
        }
    }
    // the outro, in black, to the end of the soundtrack
    let end = clips.last().map(|c| c.1 + c.2).unwrap_or(first);
    if audio_frames > end && !clips.is_empty() {
        out += &format!("            <gap name=\"Gap\" offset=\"{}\" start=\"0s\" duration=\"{}\"/>\n", t(end), t(audio_frames - end));
    }
    out += "          </spine>\n        </sequence>\n      </project>\n    </event>\n  </library>\n</fcpxml>\n";
    out
}

/// The image changes as markers, in an EDL as DaVinci Resolve writes them,
/// from 01:00:00:00, where its timelines start.
pub fn marker_edl(plan: &Plan, fps: u32, title: &str) -> String {
    let tc = |frame: u64| {
        let f = frame + 3600 * fps as u64;
        let s = f / fps as u64;
        format!("{:02}:{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60, f % fps as u64)
    };
    let mut out = format!("TITLE: {title}\nFCM: NON-DROP FRAME\n\n");
    let changes: Vec<(String, f64)> = plan
        .starts()
        .enumerate()
        .map(|(i, t)| (format!("Image {}", i + 1), t))
        .chain((plan.images > 0).then(|| ("Outro".to_string(), plan.end)))
        .collect();
    for (n, (name, secs)) in changes.iter().enumerate() {
        let f = (secs * fps as f64).round() as u64;
        out += &format!(
            "{:03}  001      V     C        {} {} {} {}  \n |C:ResolveColorBlue |M:{name} |D:1\n\n",
            n + 1,
            tc(f),
            tc(f + 1),
            tc(f),
            tc(f + 1)
        );
    }
    out
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A file URL, with anything but letters, digits and a few safe marks
/// percent-encoded.
fn file_url(p: &Path) -> String {
    let path = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().replace('\\', "/");
    let path = if path.starts_with('/') { path } else { format!("/{path}") };
    let mut out = String::from("file://");
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~:".contains(&b) {
            out.push(b as char);
        } else {
            out += &format!("%{b:02X}");
        }
    }
    out
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slideshow::{Grid, Slideshow};

    fn plan() -> Plan {
        // 60 bpm in 4/4: 4 s bars, 8 s images after a two-bar intro
        let s = Slideshow { hold: 8.0, ..Slideshow::default() };
        Plan::for_track(&s, Grid::new(60.0, (4, 4)), 60.0)
    }

    #[test]
    fn names_sort_as_numbers() {
        let mut v = vec!["img10.jpg", "img2.jpg", "IMG1.jpg", "a.jpg"];
        v.sort_by(|a, b| natural(&a.to_lowercase(), &b.to_lowercase()));
        assert_eq!(v, ["a.jpg", "IMG1.jpg", "img2.jpg", "img10.jpg"]);
    }

    #[test]
    fn timeline_places_each_photo() {
        let p = plan();
        let photos: Vec<PathBuf> = (1..=p.images).map(|i| PathBuf::from(format!("/photos/my trip & co/{i}.jpg"))).collect();
        let x = fcpxml(&p, &photos, Path::new("/music/track.wav"), p.audio(), 2.0, 25, "Trip");
        assert_eq!(x.matches("<video ").count(), p.images as usize);
        assert_eq!(x.matches("<transition ").count(), p.images as usize - 1);
        assert_eq!(x.matches("<asset-clip ").count(), 1);
        // the first image after the two-bar intro, 8 s = 200 frames
        assert!(x.contains("offset=\"200/25s\" name=\"1.jpg\""));
        // dissolves of 2 s, centred on each cut at 16 s
        assert!(x.contains("<transition name=\"Cross Dissolve\" offset=\"375/25s\" duration=\"50/25s\"/>"));
        assert!(x.contains("file:///photos/my%20trip%20%26%20co/1.jpg"));
        assert!(!x.contains("& "));
        // the soundtrack runs the whole length
        assert!(x.contains(&format!("duration=\"{}/25s\" audioRole=\"music\"", (p.audio() * 25.0).floor() as u64)));
    }

    #[test]
    fn soundtrack_without_an_intro() {
        let s = Slideshow { hold: 8.0, intro_bars: 0, ..Slideshow::default() };
        let p = Plan::for_track(&s, Grid::new(60.0, (4, 4)), 60.0);
        let photos: Vec<PathBuf> = (1..=p.images).map(|i| PathBuf::from(format!("/{i}.jpg"))).collect();
        let x = fcpxml(&p, &photos, Path::new("/t.wav"), p.audio(), 1.0, 25, "T");
        assert!(!x.contains("<gap name=\"Gap\" offset=\"0s\""));
        // hung from the first photo, an hour into its picture
        assert!(x.contains("<asset-clip ref=\"a1\" lane=\"-1\" offset=\"90000/25s\""));
    }

    #[test]
    fn fewer_photos_than_images() {
        let p = plan();
        let photos = vec![PathBuf::from("/a.jpg"), PathBuf::from("/b.jpg")];
        let x = fcpxml(&p, &photos, Path::new("/t.wav"), p.audio(), 1.0, 30, "T");
        assert_eq!(x.matches("<video ").count(), 2);
    }

    #[test]
    fn markers_from_an_hour_in() {
        let p = plan();
        let edl = marker_edl(&p, 25, "Trip");
        assert!(edl.contains("001  001      V     C        01:00:08:00 01:00:08:01 01:00:08:00 01:00:08:01"));
        assert!(edl.contains("|M:Image 1 |D:1"));
        assert_eq!(edl.matches("|M:").count(), p.images as usize + 1);
    }

    #[test]
    fn png_and_jpeg_sizes() {
        let dir = std::env::temp_dir().join(format!("midigen-size-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("a.png");
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend(640u32.to_be_bytes());
        b.extend(480u32.to_be_bytes());
        std::fs::write(&png, &b).unwrap();
        assert_eq!(image_size(&png), Some((640, 480)));
        let jpg = dir.join("b.jpg");
        let mut j = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 11, 8];
        j.extend(1080u16.to_be_bytes());
        j.extend(1920u16.to_be_bytes());
        j.extend([3, 0, 0, 0, 0, 0, 0]);
        std::fs::write(&jpg, &j).unwrap();
        assert_eq!(image_size(&jpg), Some((1920, 1080)));
        let found = photos(&dir, PhotoOrder::Name).unwrap();
        assert_eq!(found.iter().map(|p| file_name(p)).collect::<Vec<_>>(), ["a.png", "b.jpg"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
