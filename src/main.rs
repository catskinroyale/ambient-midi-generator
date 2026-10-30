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
mod song;
mod synth;
mod theory;
mod update;

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
    eframe::run_native(
        "Ambient MIDI Generator",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
