//! The interface theme: colours, fonts and spacing.
//!
//! Adapted from BW Darkroom's theme (https://github.com/catskinroyale/bnw-darkroom),
//! itself adapted from LightCraft's `crates/ui-egui/src/theme.rs`
//! (https://github.com/storytold/lightcraft), Copyright (c) 2026 ArtCraft Team and the
//! LightCraft contributors, licensed under the MIT License (see
//! `assets/LICENSE-LightCraft-MIT`). Two themes (LightCraft's Charcoal for dark, Silver
//! for light) chosen by egui's own theme preference. The Inter typeface is under the SIL
//! Open Font License 1.1 (`assets/fonts/OFL-Inter.txt`).

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke,
    TextStyle, Visuals,
};

/// The named font family for Inter SemiBold (headers, small buttons).
pub const FONT_SEMIBOLD: &str = "semibold";

/// Colours and sizes shared by the custom widgets.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    /// Side panels, top bar, tool strip.
    pub chrome: Color32,
    /// Behind the piano roll.
    pub canvas: Color32,
    pub divider: Color32,
    /// Inset boxes (expandable rows).
    pub inset: Color32,
    pub field: Color32,
    pub button: Color32,
    pub button_border: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub tool_active: Color32,
    pub text: Color32,
    pub text_label: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    pub icon: Color32,
    pub track: Color32,
    pub thumb: Color32,
    pub thumb_hover: Color32,
    pub accent: Color32,
    /// Icons of the selected tool.
    pub pick: Color32,
    pub caution: Color32,
}

const fn rgb(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// LightCraft's Charcoal theme.
const CHARCOAL: Tokens = Tokens {
    chrome: rgb(0x2d2d2d),
    canvas: rgb(0x1c1c1c),
    divider: rgb(0x1c1c1c),
    inset: rgb(0x232323),
    field: rgb(0x222222),
    button: rgb(0x242424),
    button_border: rgb(0x4c4c4c),
    hover: rgb(0x3a3a3a),
    pressed: rgb(0x3f3f3f),
    tool_active: rgb(0x3f3f3f),
    text: rgb(0xe2e2e2),
    text_label: rgb(0xbcbcbc),
    text_dim: rgb(0x8e8e8e),
    text_disabled: rgb(0x5c5c5c),
    icon: rgb(0x9a9a9a),
    track: rgb(0x5a5a5a),
    thumb: rgb(0xa0a0a0),
    thumb_hover: rgb(0xe0e0e0),
    accent: rgb(0x0165dd),
    pick: rgb(0xf0f0f0),
    caution: rgb(0xe3a83c),
};

/// LightCraft's Silver theme.
const SILVER: Tokens = Tokens {
    chrome: rgb(0xe6e6e6),
    canvas: rgb(0xcfcfcf),
    divider: rgb(0xc4c4c4),
    inset: rgb(0xdcdcdc),
    field: rgb(0xf6f6f6),
    button: rgb(0xf2f2f2),
    button_border: rgb(0xb0b0b0),
    hover: rgb(0xd2d2d2),
    pressed: rgb(0xc4c4c4),
    tool_active: rgb(0xcacaca),
    text: rgb(0x1c1c1c),
    text_label: rgb(0x383838),
    text_dim: rgb(0x6a6a6a),
    text_disabled: rgb(0xa2a2a2),
    icon: rgb(0x555555),
    track: rgb(0xa8a8a8),
    thumb: rgb(0x5e5e5e),
    thumb_hover: rgb(0x262626),
    accent: rgb(0x0a62d0),
    pick: rgb(0x2a2a2a),
    caution: rgb(0xb0761a),
};

impl Tokens {
    /// The tokens of the theme egui is showing now (dark or light).
    pub fn get(ctx: &egui::Context) -> Tokens {
        match ctx.theme() {
            egui::Theme::Dark => CHARCOAL,
            egui::Theme::Light => SILVER,
        }
    }

    pub fn font(&self, size: f32) -> FontId {
        FontId::proportional(size)
    }

    pub fn semibold(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(FONT_SEMIBOLD.into()))
    }
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "Inter-SemiBold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    // egui's own faces stay as fallbacks for symbols Inter lacks.
    let defaults = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut regular = vec!["Inter".to_string()];
    regular.extend(defaults.iter().cloned());
    fonts.families.insert(FontFamily::Proportional, regular);
    let mut semibold = vec!["Inter-SemiBold".to_string()];
    semibold.extend(defaults);
    fonts
        .families
        .insert(FontFamily::Name(FONT_SEMIBOLD.into()), semibold);
    fonts
}

fn visuals(t: &Tokens, dark: bool) -> Visuals {
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = t.chrome;
    v.window_fill = t.chrome;
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.inset;
    v.window_stroke = Stroke::new(1.0, t.button_border);
    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(6);
    v.selection.bg_fill = t.accent.gamma_multiply(0.6);
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.override_text_color = Some(t.text_label);
    v.hyperlink_color = if dark { rgb(0x5aa2ff) } else { t.accent };
    let (popup, window) = if dark { (140, 160) } else { (50, 60) };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(popup),
    };
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 30,
        spread: 0,
        color: Color32::from_black_alpha(window),
    };
    let w = &mut v.widgets;
    for (state, fill) in [
        (&mut w.noninteractive, t.chrome),
        (&mut w.inactive, t.button),
        (&mut w.hovered, t.hover),
        (&mut w.active, t.pressed),
        (&mut w.open, t.hover),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.corner_radius = CornerRadius::same(4);
        state.fg_stroke = Stroke::new(1.0, t.text_label);
    }
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.divider);
    w.inactive.bg_stroke = Stroke::new(1.0, t.button_border);
    w.hovered.fg_stroke = Stroke::new(1.0, t.text);
    w.active.fg_stroke = Stroke::new(1.0, t.text);
    v
}

/// Install fonts, both themes' visuals and the spacing. egui's theme preference (Auto,
/// Light, Dark) then picks between them.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&CHARCOAL, true));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&SILVER, false));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.spacing.interact_size.y = 22.0;
        let semibold = FontFamily::Name(FONT_SEMIBOLD.into());
        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(13.0));
        s.text_styles
            .insert(TextStyle::Button, FontId::proportional(13.0));
        s.text_styles
            .insert(TextStyle::Small, FontId::proportional(11.0));
        s.text_styles
            .insert(TextStyle::Heading, FontId::new(16.0, semibold));
        s.animation_time = 0.08;
    });
}
