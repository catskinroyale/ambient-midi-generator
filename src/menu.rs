//! The native macOS application menu: About, Check for Updates…, Services,
//! Hide, Quit and a Window menu. Choosing About or Check for Updates sets a
//! flag the UI polls and wakes the event loop.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

const ABOUT_ID: &str = "about";
const CHECK_UPDATES_ID: &str = "check-updates";

/// Owns the menu; it must live as long as the app.
pub struct AppMenu {
    _menu: Menu,
    about_requested: Arc<AtomicBool>,
    check_requested: Arc<AtomicBool>,
}

impl AppMenu {
    /// Build the menu and install it as the application's main menu.
    pub fn install(ctx: &egui::Context) -> Option<AppMenu> {
        let about = MenuItem::with_id(ABOUT_ID, "About Ambient MIDI Generator", true, None);
        let check = MenuItem::with_id(CHECK_UPDATES_ID, "Check for Updates…", true, None);
        let separator = PredefinedMenuItem::separator;
        let app = Submenu::with_items(
            "Ambient MIDI Generator",
            true,
            &[
                &about,
                &check,
                &separator(),
                &PredefinedMenuItem::services(None),
                &separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &separator(),
                &PredefinedMenuItem::quit(None),
            ],
        )
        .ok()?;
        let window = Submenu::with_items(
            "Window",
            true,
            &[
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::fullscreen(None),
                &separator(),
                &PredefinedMenuItem::bring_all_to_front(None),
            ],
        )
        .ok()?;
        let menu = Menu::with_items(&[&app, &window]).ok()?;
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();

        let about_requested = Arc::new(AtomicBool::new(false));
        let check_requested = Arc::new(AtomicBool::new(false));
        let (about_flag, check_flag) = (about_requested.clone(), check_requested.clone());
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let flag = if event.id == ABOUT_ID {
                &about_flag
            } else if event.id == CHECK_UPDATES_ID {
                &check_flag
            } else {
                return;
            };
            flag.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        }));
        Some(AppMenu { _menu: menu, about_requested, check_requested })
    }

    /// Whether About was chosen since the last call.
    pub fn take_about_request(&self) -> bool {
        self.about_requested.swap(false, Ordering::Relaxed)
    }

    /// Whether Check for Updates was chosen since the last call.
    pub fn take_check_request(&self) -> bool {
        self.check_requested.swap(false, Ordering::Relaxed)
    }
}
