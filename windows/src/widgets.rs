use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align2, Color32, CornerRadius, FontId, Galley, Id, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind,
    TextFormat, Ui, Vec2, pos2, vec2,
};

use crate::model::{StatusKind, SyncState};
use crate::theme::{self, ICON_COMPACT, ICON_SIZE, Palette, RADIUS_CONTROL, ROW_COMPACT, ROW_HEIGHT, SPACE2, SPACE3};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Primary,
    Secondary,
    Ghost,
}

/// Lays out `text` on a single line, eliding with `…` when it exceeds `max_width`.
pub fn truncated(ui: &Ui, text: &str, font: FontId, color: Color32, max_width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
    job.wrap = TextWrapping {
        max_width: max_width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

pub fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), font, Color32::WHITE).size().x
}

/// Paints single-line text vertically centred in `rect`, truncated to its width.
pub fn paint_text(ui: &Ui, rect: Rect, text: &str, font: FontId, color: Color32, align_right: bool) -> f32 {
    let galley = truncated(ui, text, font, color, rect.width());
    let size = galley.size();
    let x = if align_right { rect.right() - size.x } else { rect.left() };
    let pos = pos2(x, rect.center().y - size.y / 2.0);
    ui.painter().galley(pos, galley, color);
    size.x
}

fn paint_glyph(ui: &Ui, center: Pos2, glyph: &str, size: f32, color: Color32) {
    ui.painter().text(center, Align2::CENTER_CENTER, glyph, theme::icon_font(size), color);
}

/// Text button in the three variants used across the app.
pub fn button(
    ui: &mut Ui,
    label: &str,
    icon: Option<&str>,
    variant: Variant,
    compact: bool,
    enabled: bool,
) -> Response {
    let p = theme::palette(ui.ctx());
    let font = if compact { theme::meta_medium() } else { theme::body_medium() };
    let icon_size = if compact { ICON_COMPACT } else { ICON_SIZE };
    let height = if compact { ROW_COMPACT } else { ROW_HEIGHT };
    let gap = if compact { 6.0 } else { SPACE2 };

    let text_w = text_width(ui, label, font.clone());
    let icon_w = if icon.is_some() { icon_size + gap } else { 0.0 };
    let size = vec2(SPACE3 * 2.0 + icon_w + text_w, height);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);

    if ui.is_rect_visible(rect) {
        let hovered = enabled && response.hovered();
        let (fill, stroke, text_color, icon_color) = match variant {
            Variant::Primary => {
                (if hovered { p.accent_hover } else { p.accent }, Stroke::NONE, p.accent_text, p.accent_text)
            }
            Variant::Secondary => (
                if hovered { p.surface2 } else { p.surface1 },
                Stroke::new(1.0, p.border_strong),
                p.text_primary,
                if hovered { p.text_primary } else { p.text_secondary },
            ),
            Variant::Ghost => (
                if hovered { p.surface2 } else { Color32::TRANSPARENT },
                Stroke::NONE,
                if hovered { p.text_primary } else { p.text_secondary },
                if hovered { p.text_primary } else { p.text_secondary },
            ),
        };
        let fade = |c: Color32| if enabled { c } else { c.gamma_multiply(0.45) };
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(RADIUS_CONTROL), fade(fill));
        if stroke != Stroke::NONE {
            painter.rect_stroke(
                rect,
                CornerRadius::same(RADIUS_CONTROL),
                Stroke::new(stroke.width, fade(stroke.color)),
                StrokeKind::Inside,
            );
        }
        let mut x = rect.left() + SPACE3;
        if let Some(icon) = icon {
            paint_glyph(ui, pos2(x + icon_size / 2.0, rect.center().y), icon, icon_size - 1.0, fade(icon_color));
            x += icon_w;
        }
        painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, font, fade(text_color));
    }
    response
}

/// Square ghost icon button with a tooltip, placed by the surrounding layout.
pub fn icon_button(ui: &mut Ui, icon: &str, tooltip: &str, enabled: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(ROW_COMPACT), sense);
    paint_icon_button(ui, rect, &response, icon, enabled);
    response.on_hover_text(tooltip)
}

/// Icon button at a fixed rect (for table cells).
pub fn icon_button_at(ui: &mut Ui, rect: Rect, id: Id, icon: &str, tooltip: &str, enabled: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(rect, id, sense);
    paint_icon_button(ui, rect, &response, icon, enabled);
    response.on_hover_text(tooltip)
}

fn paint_icon_button(ui: &Ui, rect: Rect, response: &Response, icon: &str, enabled: bool) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = theme::palette(ui.ctx());
    let hovered = enabled && (response.hovered() || response.is_pointer_button_down_on());
    if hovered {
        ui.painter().rect_filled(rect, CornerRadius::same(RADIUS_CONTROL), p.surface2);
    }
    let color = if hovered { p.text_primary } else { p.text_secondary };
    let color = if enabled { color } else { color.gamma_multiply(0.35) };
    paint_glyph(ui, rect.center(), icon, ICON_SIZE - 1.0, color);
}

/// Rotating arc used while a repository is busy.
pub fn paint_spinner(ui: &Ui, center: Pos2, size: f32) {
    let p = theme::palette(ui.ctx());
    let radius = ((size - 3.0) / 2.0).max(2.0);
    let t = ui.input(|i| i.time);
    let start = ((t % 0.75) / 0.75) as f32 * std::f32::consts::TAU;
    let sweep = 250f32.to_radians();
    let painter = ui.painter();
    painter.circle_stroke(center, radius, Stroke::new(1.5, p.border_strong));
    let points: Vec<Pos2> = (0..=32)
        .map(|i| {
            let a = start + sweep * i as f32 / 32.0;
            center + vec2(a.cos(), a.sin()) * radius
        })
        .collect();
    painter.add(Shape::line(points, Stroke::new(1.5, p.notice)));
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
}

pub fn paint_status_icon(ui: &Ui, center: Pos2, state: &SyncState, size: f32) {
    if state.is_busy() {
        paint_spinner(ui, center, size);
        return;
    }
    let p = theme::palette(ui.ctx());
    let kind = state.status_kind();
    paint_glyph(ui, center, theme::status_glyph(kind), size, p.status_color(kind));
}

/// Status icon followed by an optional label, laid out inline.
pub fn status_icon(ui: &mut Ui, state: &SyncState, compact: bool, show_label: bool) -> Response {
    let size = if compact { ICON_COMPACT } else { ICON_SIZE };
    let p = theme::palette(ui.ctx());
    let label = show_label.then(|| state.label());
    let gap = if compact { 6.0 } else { SPACE2 };
    let label_w = label.as_ref().map_or(0.0, |l| gap + text_width(ui, l, theme::body()));
    let (rect, response) = ui.allocate_exact_size(vec2(size + label_w, size.max(18.0)), Sense::hover());
    paint_status_icon(ui, pos2(rect.left() + size / 2.0, rect.center().y), state, size);
    if let Some(label) = label {
        ui.painter().text(
            pos2(rect.left() + size + gap, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            theme::body(),
            p.text_primary,
        );
    }
    response.on_hover_text(format!("{} — {}", state.label(), state.inspector_explanation()))
}

/// Header filter chip: status icon, label, and tabular count.
pub fn filter_tab(ui: &mut Ui, label: Option<&str>, kind: StatusKind, count: usize, selected: bool) -> Response {
    let p = theme::palette(ui.ctx());
    let label_font = if selected { theme::meta_medium() } else { theme::meta() };
    let count_text = count.to_string();
    let count_font = if selected || count > 0 { theme::meta_medium() } else { theme::meta() };
    let icon = ICON_COMPACT;
    let label_w = label.map_or(0.0, |l| 6.0 + text_width(ui, l, label_font.clone()));
    let count_w = 6.0 + text_width(ui, &count_text, count_font.clone());
    let (rect, response) =
        ui.allocate_exact_size(vec2(SPACE2 * 2.0 + icon + label_w + count_w, ROW_COMPACT), Sense::click());

    let semantic = p.semantic_color(kind);
    let painter = ui.painter();
    let radius = CornerRadius::same(RADIUS_CONTROL);
    if selected {
        painter.rect_filled(rect, radius, semantic.gamma_multiply(0.14));
        painter.rect_stroke(rect, radius, Stroke::new(1.0, semantic.gamma_multiply(0.45)), StrokeKind::Inside);
    } else if response.hovered() {
        painter.rect_filled(rect, radius, p.surface2);
    }
    let mut x = rect.left() + SPACE2;
    paint_glyph(ui, pos2(x + icon / 2.0, rect.center().y), theme::status_glyph(kind), icon - 1.0, p.status_color(kind));
    x += icon + 6.0;
    if let Some(label) = label {
        let color = if selected { p.text_primary } else { p.text_secondary };
        let r = painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, label_font, color);
        x = r.right() + 6.0;
    }
    let count_color = if count > 0 { p.text_primary } else { p.text_tertiary };
    painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, count_text, count_font, count_color);
    response
}

/// One entry in the sidebar's scope lists (a folder, an owner, or "all").
pub struct ScopeRow<'a> {
    pub glyph: &'a str,
    pub title: &'a str,
    /// Secondary text after the title, such as a path or host.
    pub detail: Option<&'a str>,
    pub count: usize,
    /// Repositories in this scope with updates to pull; shown as a yellow dot.
    pub behind: usize,
    pub selected: bool,
    /// Tooltip for a remove button shown on hover, if the entry can be removed.
    pub remove_tooltip: Option<&'a str>,
}

pub struct ScopeRowResponse {
    pub response: Response,
    pub remove_clicked: bool,
}

/// Full-width selectable sidebar row with an icon, title, detail, and count.
pub fn scope_row(ui: &mut Ui, row: ScopeRow<'_>) -> ScopeRowResponse {
    let p = theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_COMPACT), Sense::click());
    let hovered = ui.rect_contains_pointer(rect);
    let painter = ui.painter();
    if row.selected || hovered {
        painter.rect_filled(rect, CornerRadius::same(RADIUS_CONTROL), p.surface2);
    }
    if row.selected {
        let bar = Rect::from_center_size(pos2(rect.left() + 1.0, rect.center().y), vec2(2.0, 16.0));
        painter.rect_filled(bar, CornerRadius::same(1), p.accent);
    }
    let glyph_color = if row.selected { p.text_primary } else { p.text_secondary };
    paint_glyph(
        ui,
        pos2(rect.left() + SPACE2 + ICON_COMPACT / 2.0, rect.center().y),
        row.glyph,
        ICON_COMPACT - 1.0,
        glyph_color,
    );

    // Right edge: a remove button while hovering removable rows, otherwise the count.
    let show_remove = hovered && row.remove_tooltip.is_some();
    let mut right = rect.right() - SPACE2;
    let mut remove_clicked = false;
    if show_remove {
        let x_rect = Rect::from_min_size(pos2(rect.right() - ROW_COMPACT, rect.top()), vec2(ROW_COMPACT, ROW_COMPACT));
        let tooltip = row.remove_tooltip.unwrap_or_default();
        remove_clicked =
            icon_button_at(ui, x_rect, response.id.with("remove"), egui_phosphor::regular::X, tooltip, true).clicked();
        right = x_rect.left();
    } else if row.count > 0 {
        let count_color = if row.selected { p.text_primary } else { p.text_secondary };
        let count_rect = Rect::from_min_max(pos2(rect.left(), rect.top()), pos2(right, rect.bottom()));
        right -= paint_text(ui, count_rect, &row.count.to_string(), theme::meta_medium(), count_color, true) + SPACE2;
    }
    if row.behind > 0 && !show_remove {
        let dot = pos2(right - 3.0, rect.center().y);
        ui.painter().circle_filled(dot, 3.0, p.notice);
        right -= 6.0 + SPACE2;
    }

    let mut x = rect.left() + SPACE2 + ICON_COMPACT + SPACE2;
    let (font, color) =
        if row.selected { (theme::body_medium(), p.text_primary) } else { (theme::body(), p.text_secondary) };
    let title_rect = Rect::from_min_max(pos2(x, rect.top()), pos2(right, rect.bottom()));
    x += paint_text(ui, title_rect, row.title, font, color, false) + 6.0;
    if let Some(detail) = row.detail
        && x < right
    {
        let detail_rect = Rect::from_min_max(pos2(x, rect.top()), pos2(right, rect.bottom()));
        paint_text(ui, detail_rect, detail, theme::meta_mono(), p.text_tertiary, false);
    }
    ScopeRowResponse { response, remove_clicked }
}

/// Rounded checkbox with a trailing label; returns true when toggled.
pub fn checkbox(ui: &mut Ui, value: &mut bool, label: &str) -> bool {
    let p = theme::palette(ui.ctx());
    let box_size = 14.0;
    let label_w = text_width(ui, label, theme::meta());
    let (rect, response) = ui.allocate_exact_size(vec2(box_size + SPACE2 + label_w, 22.0), Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    let box_rect = Rect::from_center_size(pos2(rect.left() + box_size / 2.0, rect.center().y), Vec2::splat(box_size));
    let painter = ui.painter();
    if *value {
        painter.rect_filled(box_rect, CornerRadius::same(4), p.accent);
        paint_glyph(ui, box_rect.center(), egui_phosphor::regular::CHECK, 11.0, p.accent_text);
    } else {
        let fill = if response.hovered() { p.surface2 } else { p.canvas };
        painter.rect_filled(box_rect, CornerRadius::same(4), fill);
        painter.rect_stroke(box_rect, CornerRadius::same(4), Stroke::new(1.0, p.border_strong), StrokeKind::Inside);
    }
    let color = if response.hovered() { p.text_primary } else { p.text_secondary };
    painter.text(pos2(box_rect.right() + SPACE2, rect.center().y), Align2::LEFT_CENTER, label, theme::meta(), color);
    response.clicked()
}

pub fn section_header(ui: &mut Ui, text: &str) {
    let p = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_space(SPACE2);
        ui.label(egui::RichText::new(text).font(theme::meta_medium()).color(p.text_tertiary));
    });
}

pub fn hline(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, color);
}

/// Rounded container used for metadata, commit lists, and output panes.
pub fn container_frame(p: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(p.canvas)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(theme::RADIUS_CONTAINER))
}

/// Drop-down matching the 28px control height, with an up/down caret like a native picker.
pub fn combo_box(ui: &mut Ui, id: &str, selected: &str, width: f32, content: impl FnOnce(&mut Ui)) {
    let color = theme::palette(ui.ctx()).text_secondary;
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = ROW_COMPACT;
        egui::ComboBox::from_id_salt(id)
            .selected_text(egui::RichText::new(selected).font(theme::meta()))
            .width(width)
            .icon(move |ui, rect, _visuals, _open| {
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    egui_phosphor::regular::CARET_UP_DOWN,
                    theme::icon_font(12.0),
                    color,
                );
            })
            .show_ui(ui, content);
    });
}

/// Two or more mutually exclusive options in one rounded control. Returns the clicked index.
/// Disabled options show `tooltip` text explaining why.
pub fn segmented(ui: &mut Ui, options: &[(&str, bool, &str)], selected: usize) -> Option<usize> {
    let p = theme::palette(ui.ctx());
    let font = theme::body_medium();
    let widths: Vec<f32> =
        options.iter().map(|(label, _, _)| text_width(ui, label, font.clone()) + 2.0 * SPACE3).collect();
    let total = widths.iter().sum::<f32>() + 4.0;
    let (outer, _) = ui.allocate_exact_size(vec2(total, ROW_HEIGHT), Sense::hover());
    ui.painter().rect_filled(outer, CornerRadius::same(RADIUS_CONTROL + 1), p.canvas);
    ui.painter().rect_stroke(
        outer,
        CornerRadius::same(RADIUS_CONTROL + 1),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let mut clicked = None;
    let mut x = outer.left() + 2.0;
    for (i, ((label, enabled, tooltip), width)) in options.iter().zip(widths).enumerate() {
        let rect = Rect::from_min_size(pos2(x, outer.top() + 2.0), vec2(width, outer.height() - 4.0));
        x += width;
        let sense = if *enabled { Sense::click() } else { Sense::hover() };
        let response = ui.interact(rect, ui.id().with(("segment", i)), sense);
        let is_selected = i == selected;
        if is_selected {
            ui.painter().rect_filled(rect, CornerRadius::same(RADIUS_CONTROL), p.surface2);
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(RADIUS_CONTROL),
                Stroke::new(1.0, p.border_strong),
                StrokeKind::Inside,
            );
        }
        let color = match (is_selected, *enabled, response.hovered()) {
            (true, _, _) => p.text_primary,
            (false, false, _) => p.text_tertiary.gamma_multiply(0.7),
            (false, true, true) => p.text_primary,
            (false, true, false) => p.text_secondary,
        };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, *label, font.clone(), color);
        let response = if tooltip.is_empty() {
            response
        } else if *enabled {
            response.on_hover_text(*tooltip)
        } else {
            response.on_disabled_hover_text(*tooltip).on_hover_text(*tooltip)
        };
        if response.clicked() && !is_selected {
            clicked = Some(i);
        }
    }
    clicked
}

/// Lays out `add` in a column exactly as wide as the space available now. Anything wider is
/// clipped instead of widening the surrounding panel.
pub fn clipped_column(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    let origin = ui.cursor().min;
    let column = Rect::from_min_size(origin, vec2(width, f32::INFINITY));
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(column).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(child.clip_rect().intersect(column));
    add(&mut child);
    ui.allocate_rect(Rect::from_min_size(origin, vec2(width, child.min_rect().height())), Sense::hover());
}

/// Renders prepared markdown segments. Relative links and images resolve against `base_uri`;
/// tables scroll sideways so they never widen the panel.
pub fn markdown(
    ui: &mut Ui,
    cache: &mut egui_commonmark::CommonMarkCache,
    segments: &[crate::markdown::Segment],
    base_uri: &str,
    id: impl std::hash::Hash + std::fmt::Debug + Copy,
) {
    let p = theme::palette(ui.ctx());
    let image_width = ui.available_width().max(64.0) as usize;
    clipped_column(ui, |ui| {
        ui.style_mut().override_text_style = None;
        ui.visuals_mut().override_text_color = Some(p.text_secondary);
        let viewer = || {
            egui_commonmark::CommonMarkViewer::new()
                .max_image_width(Some(image_width))
                .default_implicit_uri_scheme(base_uri.to_string())
        };
        for (i, segment) in segments.iter().enumerate() {
            // Each segment gets its own id scope; otherwise every segment's first table would
            // claim the same grid id.
            ui.scope_builder(egui::UiBuilder::new().id_salt((id, "md-segment", i)), |ui| match segment {
                crate::markdown::Segment::Text(text) => {
                    viewer().show(ui, cache, text);
                }
                crate::markdown::Segment::Table(text) => {
                    egui::ScrollArea::horizontal().id_salt((id, "md-table", i)).show(ui, |ui| {
                        viewer().show(ui, cache, text);
                    });
                }
            });
        }
    });
}
