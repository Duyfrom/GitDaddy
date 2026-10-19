use std::path::PathBuf;
use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, FontTweak, Stroke, TextStyle, Theme, Visuals,
};

use crate::model::StatusKind;

// Geometry on a 4px grid, matching the macOS app.
pub const ROW_HEIGHT: f32 = 32.0;
pub const ROW_COMPACT: f32 = 28.0;
pub const ICON_SIZE: f32 = 16.0;
pub const ICON_COMPACT: f32 = 14.0;
pub const RADIUS_CONTROL: u8 = 6;
pub const RADIUS_CONTAINER: u8 = 8;

pub const SPACE1: f32 = 4.0;
pub const SPACE2: f32 = 8.0;
pub const SPACE3: f32 = 12.0;
pub const SPACE4: f32 = 16.0;
pub const SPACE6: f32 = 24.0;

pub const FS_META: f32 = 12.0;
pub const FS_BODY: f32 = 13.0;
pub const FS_TITLE: f32 = 15.0;

pub struct Palette {
    pub canvas: Color32,
    pub surface1: Color32,
    pub surface2: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_tertiary: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_text: Color32,
    pub good: Color32,
    pub notice: Color32,
    pub bad: Color32,
}

const ACCENT: Color32 = Color32::from_rgb(0x5B, 0x8D, 0xEF);

static DARK: Palette = Palette {
    canvas: Color32::from_rgb(0x11, 0x11, 0x13),
    surface1: Color32::from_rgb(0x18, 0x18, 0x1B),
    surface2: Color32::from_rgb(0x20, 0x20, 0x24),
    border: Color32::from_rgb(0x2B, 0x2B, 0x2E),
    border_strong: Color32::from_rgb(0x3A, 0x3A, 0x3D),
    text_primary: Color32::from_rgb(0xEC, 0xEC, 0xED),
    text_secondary: Color32::from_rgb(0xA4, 0xA4, 0xA7),
    text_tertiary: Color32::from_rgb(0x77, 0x77, 0x7B),
    accent: ACCENT,
    accent_hover: Color32::from_rgb(0x6D, 0x9A, 0xF1),
    accent_text: Color32::WHITE,
    good: Color32::from_rgb(0x34, 0xD3, 0x99),
    notice: Color32::from_rgb(0xFB, 0xBF, 0x24),
    bad: Color32::from_rgb(0xF8, 0x71, 0x71),
};

static LIGHT: Palette = Palette {
    canvas: Color32::from_rgb(0xF4, 0xF4, 0xF5),
    surface1: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    surface2: Color32::from_rgb(0xF0, 0xF0, 0xF2),
    border: Color32::from_rgb(0xE6, 0xE6, 0xE8),
    border_strong: Color32::from_rgb(0xD5, 0xD5, 0xD8),
    text_primary: Color32::from_rgb(0x1F, 0x1F, 0x21),
    text_secondary: Color32::from_rgb(0x6A, 0x6A, 0x6E),
    text_tertiary: Color32::from_rgb(0x8E, 0x8E, 0x93),
    accent: ACCENT,
    accent_hover: Color32::from_rgb(0x4A, 0x7E, 0xE6),
    accent_text: Color32::WHITE,
    // Slightly deeper status colors so they keep contrast on white.
    good: Color32::from_rgb(0x10, 0xA3, 0x70),
    notice: Color32::from_rgb(0xD9, 0x8E, 0x04),
    bad: Color32::from_rgb(0xDC, 0x3F, 0x3F),
};

pub fn palette(ctx: &egui::Context) -> &'static Palette {
    match ctx.theme() {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
    }
}

impl Palette {
    /// Green = good, yellow = notice, red = bad.
    pub fn status_color(&self, kind: StatusKind) -> Color32 {
        match kind {
            StatusKind::AllRepos => self.text_secondary,
            StatusKind::Success => self.good,
            StatusKind::NeedsUpdate
            | StatusKind::Dirty
            | StatusKind::Unpushed
            | StatusKind::Unpublished
            | StatusKind::InProgress => self.notice,
            StatusKind::Diverged | StatusKind::Danger | StatusKind::Disabled => self.bad,
            StatusKind::Draft => self.text_tertiary,
        }
    }

    /// Like `status_color`, but "all repos" uses the accent for selection chrome.
    pub fn semantic_color(&self, kind: StatusKind) -> Color32 {
        if kind == StatusKind::AllRepos { self.accent } else { self.status_color(kind) }
    }
}

pub fn status_glyph(kind: StatusKind) -> &'static str {
    use egui_phosphor::regular as icon;
    match kind {
        StatusKind::AllRepos => icon::TRAY,
        StatusKind::Success => icon::CHECK_CIRCLE,
        StatusKind::InProgress | StatusKind::Draft => icon::CIRCLE_DASHED,
        StatusKind::NeedsUpdate => icon::ARROW_CIRCLE_DOWN,
        StatusKind::Dirty => icon::PENCIL_SIMPLE,
        StatusKind::Unpushed => icon::ARROW_CIRCLE_UP,
        StatusKind::Unpublished => icon::ARROW_CIRCLE_UP_RIGHT,
        StatusKind::Diverged => icon::ARROWS_DOWN_UP,
        StatusKind::Danger => icon::WARNING,
        StatusKind::Disabled => icon::PROHIBIT,
    }
}

// MARK: Fonts

pub fn semibold_family() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn meta() -> FontId {
    FontId::new(FS_META, FontFamily::Proportional)
}

pub fn meta_medium() -> FontId {
    FontId::new(FS_META, semibold_family())
}

pub fn meta_mono() -> FontId {
    FontId::new(FS_META, FontFamily::Monospace)
}

pub fn body() -> FontId {
    FontId::new(FS_BODY, FontFamily::Proportional)
}

pub fn body_medium() -> FontId {
    FontId::new(FS_BODY, semibold_family())
}

pub fn title() -> FontId {
    FontId::new(FS_TITLE, semibold_family())
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("icons".into()))
}

fn system_font(file_names: &[&str]) -> Option<Vec<u8>> {
    let windir = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let user_fonts = dirs::data_local_dir().map(|d| d.join("Microsoft").join("Windows").join("Fonts"));
    let dirs = [
        Some(windir.join("Fonts")),
        user_fonts,
        Some(PathBuf::from("/System/Library/Fonts")),
        Some(PathBuf::from("/System/Library/Fonts/Supplemental")),
    ];
    file_names.iter().find_map(|name| {
        dirs.iter()
            .flatten()
            .find_map(|dir| std::fs::read(dir.join(name)).ok())
    })
}

/// System UI fonts, Phosphor for icons, and OS CJK fallback fonts (Microsoft YaHei + Malgun
/// Gothic on Windows; Apple Symbols + Arial Unicode on macOS); egui's bundled fonts stay at
/// the end of every family as a fallback.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let default_proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let default_monospace = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();

    let mut add = |name: &str, bytes: Vec<u8>, tweak: FontTweak| {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_owned(bytes).tweak(tweak)));
        name.to_owned()
    };
    let plain = FontTweak::default();
    // Cascadia Mono / Consolas sit higher in their line box than Segoe UI; nudge them onto its baseline.
    let lowered = FontTweak { y_offset_factor: 0.3, ..FontTweak::default() };

    let regular = system_font(&["segoeui.ttf"]).map(|b| add("segoe-ui", b, plain.clone()));
    let semibold = system_font(&["seguisb.ttf", "segoeuib.ttf"]).map(|b| add("segoe-ui-semibold", b, plain.clone()));
    let mono = system_font(&["CascadiaMono.ttf", "consola.ttf"]).map(|b| add("mono", b, lowered));
    let symbols =
        system_font(&["seguisym.ttf", "Apple Symbols.ttf"]).map(|b| add("system-symbols", b, plain.clone()));
    // Segoe UI and Ubuntu have no CJK glyphs. Microsoft YaHei + Malgun Gothic cover CJK on
    // Windows; Arial Unicode covers Chinese, Japanese, and Korean on macOS.
    let cjk: Vec<String> = [
        ("cjk", &["msyh.ttc", "YuGothR.ttc", "Arial Unicode.ttf"][..]),
        ("hangul", &["malgun.ttf"][..]),
    ]
    .into_iter()
    .filter_map(|(name, files)| system_font(files).map(|b| add(name, b, plain.clone())))
    .collect();
    fonts
        .font_data
        .insert("phosphor".into(), Arc::new(FontData::from_static(egui_phosphor::Variant::Regular.font_bytes())));

    let family = |primary: Option<&String>, defaults: &[String]| {
        let mut list: Vec<String> = primary.into_iter().cloned().collect();
        list.push("phosphor".into());
        list.extend(symbols.iter().cloned());
        list.extend(cjk.iter().cloned());
        list.extend(defaults.iter().cloned());
        list
    };

    let proportional = family(regular.as_ref(), &default_proportional);
    let semibold_list = family(semibold.as_ref().or(regular.as_ref()), &default_proportional);
    let mut monospace = family(mono.as_ref(), &default_monospace);
    monospace.extend(regular.iter().cloned());

    fonts.families.insert(FontFamily::Proportional, proportional);
    fonts.families.insert(semibold_family(), semibold_list);
    fonts.families.insert(FontFamily::Monospace, monospace);
    // Icons get their own family so no text font can shadow Phosphor's private-use glyphs.
    let mut icons = vec!["phosphor".to_string()];
    icons.extend(default_proportional.iter().cloned());
    fonts.families.insert(FontFamily::Name("icons".into()), icons);
    ctx.set_fonts(fonts);
}

// MARK: Style

pub fn install_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, meta()),
            (TextStyle::Body, body()),
            (TextStyle::Button, body()),
            (TextStyle::Monospace, meta_mono()),
            (TextStyle::Heading, title()),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(SPACE2, SPACE1);
        style.spacing.button_padding = egui::vec2(SPACE2, SPACE1);
        style.spacing.interact_size.y = ROW_COMPACT - 4.0;
        style.spacing.combo_height = 320.0;
        style.spacing.scroll.floating = true;
        style.spacing.scroll.bar_width = 8.0;
        style.interaction.tooltip_delay = 0.35;
        style.interaction.selectable_labels = false;
    });
    ctx.set_visuals_of(Theme::Dark, visuals(&DARK, Visuals::dark()));
    ctx.set_visuals_of(Theme::Light, visuals(&LIGHT, Visuals::light()));
}

fn visuals(p: &Palette, mut v: Visuals) -> Visuals {
    let control = CornerRadius::same(RADIUS_CONTROL);
    v.override_text_color = None;
    v.panel_fill = p.surface1;
    v.window_fill = p.surface1;
    v.window_stroke = Stroke::new(1.0, p.border_strong);
    v.window_corner_radius = CornerRadius::same(RADIUS_CONTAINER + 2);
    v.menu_corner_radius = CornerRadius::same(RADIUS_CONTAINER);
    v.extreme_bg_color = p.canvas;
    v.faint_bg_color = p.surface2;
    v.code_bg_color = p.surface2;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.text_cursor.stroke = Stroke::new(1.5, p.accent);
    v.warn_fg_color = p.notice;
    v.error_fg_color = p.bad;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.surface1;
    w.noninteractive.weak_bg_fill = p.surface1;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text_secondary);
    w.noninteractive.corner_radius = control;

    w.inactive.bg_fill = p.surface2;
    w.inactive.weak_bg_fill = p.surface1;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border_strong);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text_primary);
    w.inactive.corner_radius = control;

    w.hovered.bg_fill = p.surface2;
    w.hovered.weak_bg_fill = p.surface2;
    w.hovered.bg_stroke = Stroke::new(1.0, p.border_strong);
    w.hovered.fg_stroke = Stroke::new(1.0, p.text_primary);
    w.hovered.corner_radius = control;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.surface2;
    w.active.weak_bg_fill = p.surface2;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.0, p.text_primary);
    w.active.corner_radius = control;
    w.active.expansion = 0.0;

    w.open.bg_fill = p.surface2;
    w.open.weak_bg_fill = p.surface2;
    w.open.bg_stroke = Stroke::new(1.0, p.border_strong);
    w.open.fg_stroke = Stroke::new(1.0, p.text_primary);
    w.open.corner_radius = control;
    v
}
