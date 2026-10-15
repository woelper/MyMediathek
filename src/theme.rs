//! Look and feel borrowed from ../transcribe: white cards floating on a
//! lavender ground, IBM Plex type, one violet accent.

use eframe::egui;

pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0x26, 0x28, 0x3d);
pub const GROUND: egui::Color32 = egui::Color32::from_rgb(0xf1, 0xf2, 0xf9);
pub const CARD: egui::Color32 = egui::Color32::WHITE;
pub const FIELD: egui::Color32 = egui::Color32::from_rgb(0xf3, 0xf4, 0xfa);
const BUTTON: egui::Color32 = egui::Color32::from_rgb(0xec, 0xee, 0xf6);
const BUTTON_HOVER: egui::Color32 = egui::Color32::from_rgb(0xe1, 0xe4, 0xf1);
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x6c, 0x5c, 0xe7);
pub const ACCENT_SOFT: egui::Color32 = egui::Color32::from_rgb(0xe9, 0xe6, 0xfb);
pub const GREEN: egui::Color32 = egui::Color32::from_rgb(0x2b, 0xd8, 0x8f);
/// Star of a video that isn't bookmarked.
pub const TRACK_OFF: egui::Color32 = egui::Color32::from_rgb(0xc4, 0xc8, 0xd8);
/// Bookmarked star.
pub const GOLD: egui::Color32 = egui::Color32::from_rgb(0xf5, 0xa6, 0x23);
pub const RED: egui::Color32 = egui::Color32::from_rgb(0xef, 0x50, 0x6e);
/// Faint outline on buttons and dropdowns.
const FRAME: egui::Color32 = egui::Color32::from_rgb(0xdd, 0xe0, 0xec);
const SHADOW: egui::Color32 = egui::Color32::from_rgba_premultiplied(5, 5, 8, 16);
const POPUP_SHADOW: egui::Color32 = egui::Color32::from_rgba_premultiplied(8, 8, 14, 64);

/// Corner radius shared by buttons, fields, and progress bars.
const WIDGET_RADIUS: u8 = 12;

/// White rounded card with a soft drop shadow.
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(egui::CornerRadius::same(16))
        .shadow(egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: SHADOW })
        .inner_margin(egui::Margin::same(14))
}

pub fn progress_bar(fraction: f32) -> egui::ProgressBar {
    egui::ProgressBar::new(fraction).corner_radius(egui::CornerRadius::same(WIDGET_RADIUS))
}

/// Solid violet button for the one action that matters.
pub fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(text.into())
            .color(egui::Color32::WHITE)
            .family(egui::FontFamily::Name("plex-medium".into())),
    )
    .fill(ACCENT)
    .stroke(egui::Stroke::NONE)
}

pub fn setup(ctx: &egui::Context) {
    use egui::{CornerRadius, FontFamily, FontId, Shadow, Stroke, TextStyle, vec2};

    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        ("plex-sans", &include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf")[..]),
        ("plex-sans-medium", &include_bytes!("../assets/fonts/IBMPlexSans-Medium.ttf")[..]),
        ("plex-mono", &include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf")[..]),
    ] {
        fonts.font_data.insert(name.into(), egui::FontData::from_static(bytes).into());
    }
    fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "plex-sans".into());
    fonts.families.get_mut(&FontFamily::Monospace).unwrap().insert(0, "plex-mono".into());
    fonts.families.insert(
        FontFamily::Name("plex-medium".into()),
        vec!["plex-sans-medium".into(), "plex-sans".into()],
    );
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Fill);
    // add_to_fonts only extends the built-in families; the medium family needs icons too.
    fonts.families.get_mut(&FontFamily::Name("plex-medium".into())).unwrap().push("phosphor".into());
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::ThemePreference::Light);
    let mut style = (*ctx.style_of(egui::Theme::Light)).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::new(20.0, FontFamily::Name("plex-medium".into()))),
        (TextStyle::Body, FontId::new(16.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(16.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        (TextStyle::Small, FontId::new(13.0, FontFamily::Proportional)),
    ]
    .into();
    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.button_padding = vec2(16.0, 9.0);
    style.spacing.interact_size = vec2(44.0, 38.0);

    let mut v = egui::Visuals::light();
    v.override_text_color = Some(TEXT);
    v.panel_fill = GROUND;
    v.window_fill = CARD;
    v.window_stroke = Stroke::NONE;
    v.window_corner_radius = CornerRadius::same(16);
    v.window_shadow = Shadow { offset: [0, 8], blur: 32, spread: 0, color: SHADOW };
    v.popup_shadow = Shadow { offset: [0, 8], blur: 28, spread: 0, color: POPUP_SHADOW };
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = FIELD;
    v.selection.bg_fill = ACCENT_SOFT;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(WIDGET_RADIUS);
        w.bg_stroke = Stroke::new(1.0, FRAME);
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.corner_radius = CornerRadius::same(WIDGET_RADIUS);
    v.widgets.noninteractive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.weak_bg_fill = BUTTON;
    v.widgets.inactive.bg_fill = BUTTON;
    v.widgets.hovered.weak_bg_fill = BUTTON_HOVER;
    v.widgets.hovered.bg_fill = BUTTON_HOVER;
    v.widgets.active.weak_bg_fill = ACCENT_SOFT;
    v.widgets.active.bg_fill = ACCENT_SOFT;
    v.widgets.open.weak_bg_fill = BUTTON_HOVER;
    v.widgets.open.bg_fill = BUTTON_HOVER;
    style.visuals = v;
    ctx.set_style_of(egui::Theme::Light, style);
}
