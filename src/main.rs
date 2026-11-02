// No console window behind the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod about;
mod app;
mod fonts;
mod generator;
#[cfg(target_os = "macos")]
mod menu;
mod midi_export;
mod player;
mod project;
mod reverb;
mod rng;
mod slideshow;
mod snapshot;
mod song;
mod synth;
mod theme;
mod theory;
mod update;
mod widgets;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Ambient MIDI Generator")
            .with_inner_size([1280.0, 780.0])
            .with_min_inner_size([900.0, 560.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
                    .expect("valid icon png"),
            ),
        ..Default::default()
    };
    // Automated screenshots keep their own settings, so they start from defaults
    // and never touch the user's.
    let app_name = if snapshot::Snapshot::from_env().is_some() {
        "Ambient MIDI Generator (screenshot)"
    } else {
        "Ambient MIDI Generator"
    };
    eframe::run_native(
        app_name,
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
