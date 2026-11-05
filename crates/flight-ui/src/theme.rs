use eframe::egui::{self, Color32};

pub(crate) const ACCENT: Color32 = Color32::from_rgb(27, 118, 106);
pub(crate) const INK: Color32 = Color32::from_rgb(29, 54, 59);
pub(crate) const MUTED: Color32 = Color32::from_rgb(98, 119, 119);
pub(crate) const RED: Color32 = Color32::from_rgb(166, 54, 47);
pub(crate) const SURFACE: Color32 = Color32::WHITE;
// The selected file tab, CSV header and PDF toolbar share this surface.
pub(crate) const BACKGROUND: Color32 = Color32::from_rgb(245, 248, 247);
pub(crate) const RECESSED: Color32 = Color32::from_rgb(237, 242, 239);
pub(crate) const BORDER: Color32 = Color32::from_rgb(207, 221, 215);
pub(crate) const RULE: Color32 = Color32::from_rgb(224, 233, 229);
pub(crate) const CONTROL_RADIUS: u8 = 6;
pub(crate) const CARD_RADIUS: u8 = 8;

pub(crate) fn configure(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".into(),
        egui::FontData::from_static(crate::certificate::FONT).into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "cjk".into());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push("cjk".into());
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::Theme::Light);

    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.override_text_color = Some(INK);
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.window_fill = SURFACE;
    style.visuals.window_stroke = egui::Stroke::new(1.0_f32, BORDER);
    style.visuals.window_corner_radius = CARD_RADIUS.into();
    style.visuals.hyperlink_color = ACCENT;
    style.visuals.text_edit_bg_color = Some(Color32::from_rgb(241, 247, 244));
    style.visuals.selection.bg_fill = Color32::from_rgb(211, 233, 226);
    style.visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(234, 240, 237);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(234, 240, 237);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(211, 233, 226);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(211, 233, 226);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(190, 219, 208);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(190, 219, 208);
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    style.visuals.widgets.hovered.bg_stroke =
        egui::Stroke::new(1.0_f32, Color32::from_rgb(96, 156, 142));
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, ACCENT);
    let widgets = &mut style.visuals.widgets;
    for widget in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        widget.corner_radius = CONTROL_RADIUS.into();
    }
    style.spacing.item_spacing = egui::vec2(12.0, 12.0);
    style.spacing.button_padding = egui::vec2(14.0, 9.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(16.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(25.0));
    ctx.set_style_of(egui::Theme::Light, style);
}
