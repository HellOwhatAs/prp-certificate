use eframe::egui::{self, Color32};

use crate::theme::{ACCENT, BORDER, CARD_RADIUS, CONTROL_RADIUS, SURFACE};

pub(crate) fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(egui::Stroke::new(1.0_f32, BORDER))
        .corner_radius(CARD_RADIUS)
        .inner_margin(20)
}

/// Keep primary actions consistent without replacing their layout or enabled rules.
pub(crate) fn primary_button(
    ui: &mut egui::Ui,
    enabled: bool,
    size: impl Into<egui::Vec2>,
    label: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| {
        let visuals = ui.visuals_mut();
        visuals.override_text_color = Some(SURFACE);
        for (widget, fill) in [
            (&mut visuals.widgets.inactive, ACCENT),
            (
                &mut visuals.widgets.hovered,
                Color32::from_rgb(23, 133, 119),
            ),
            (&mut visuals.widgets.active, Color32::from_rgb(19, 94, 84)),
        ] {
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.bg_stroke = egui::Stroke::new(1.0_f32, fill);
        }
        ui.add_sized(size, egui::Button::new(label).corner_radius(CONTROL_RADIUS))
    })
    .inner
}

/// Icons retain their own geometry; hover, press, focus and hit-area feedback are shared.
pub(crate) fn icon_button(
    ui: &egui::Ui,
    response: egui::Response,
    label: &str,
    active: bool,
) -> egui::Response {
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if ui.is_enabled()
        && (active
            || response.hovered()
            || response.has_focus()
            || response.is_pointer_button_down_on())
    {
        let fill = if response.is_pointer_button_down_on() {
            Color32::from_rgb(211, 225, 218)
        } else {
            Color32::from_rgb(231, 239, 235)
        };
        ui.painter()
            .rect_filled(response.rect, CONTROL_RADIUS, fill);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.shrink(1.0),
            CONTROL_RADIUS,
            egui::Stroke::new(1.0_f32, ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_text(label)
}
