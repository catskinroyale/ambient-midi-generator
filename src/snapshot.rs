//! Automated UI checks. With `MIDIGEN_SCREENSHOT=window.bmp` the app saves a picture
//! of its window once the first frames have settled, then quits, without saving
//! any settings. `MIDIGEN_SCREENSHOT_SHOW` can name things to show first,
//! comma-separated: `sections` (open every section), `sound` (open only the
//! sound sections), `electronic` (start on an electronic style), `noir` (start on Neon Noir), `effects` (only the
//! effects section, on the shimmer space), `mixer` (show the mixer), `edit` (note
//! editing on), `zoom` (the piano roll zoomed in), `about`, and `libraries` (About,
//! scrolled to the library list with one selected).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;

pub struct Snapshot {
    path: PathBuf,
    pub show: Vec<String>,
    frame: u32,
    done: Arc<AtomicBool>,
}

impl Snapshot {
    pub fn from_env() -> Option<Snapshot> {
        let path = PathBuf::from(std::env::var_os("MIDIGEN_SCREENSHOT")?);
        let show = std::env::var("MIDIGEN_SCREENSHOT_SHOW")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Some(Snapshot { path, show, frame: 0, done: Arc::new(AtomicBool::new(false)) })
    }

    pub fn wants(&self, what: &str) -> bool {
        self.show.iter().any(|s| s == what)
    }

    /// Call once a frame: takes the picture after a short settling time, and
    /// closes the window once it is saved.
    pub fn tick(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        if self.done.load(Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.frame == 45 {
            let (path, done) = (self.path.clone(), self.done.clone());
            ctx.request_screenshot(move |image| {
                if let Err(e) = write_bmp(&path, &image) {
                    eprintln!("screenshot failed: {e}");
                }
                done.store(true, Ordering::Relaxed);
            });
        }
        ctx.request_repaint();
    }
}

/// A 24-bit uncompressed BMP: simple enough to need no image library.
fn write_bmp(path: &std::path::Path, image: &egui::ColorImage) -> std::io::Result<()> {
    let [w, h] = image.size;
    let row = (w * 3).div_ceil(4) * 4;
    let size = 54 + row * h;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in (0..h).rev() {
        let start = out.len();
        for x in 0..w {
            let c = image.pixels[y * w + x];
            out.extend_from_slice(&[c.b(), c.g(), c.r()]);
        }
        out.resize(start + row, 0);
    }
    std::fs::write(path, out)
}
