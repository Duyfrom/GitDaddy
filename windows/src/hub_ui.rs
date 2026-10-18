//! Center list, inspector, and clone dialog for the sidebar's GitHub views.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

use chrono::Local;
use egui::load::TexturePoll;
use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Align2, Color32, CornerRadius, Frame, Galley, Id, Layout, Margin, Rect, Response, RichText, ScrollArea,
    Sense, Stroke, StrokeKind, TextEdit, TextFormat, Ui, UiBuilder, pos2, vec2,
};
use egui_commonmark::CommonMarkCache;
use egui_phosphor::regular as icon;

use crate::hub::{
    CloneStage, Connection, EXPLORE_LANGUAGES, ExploreQuery, Feed, Hub, Involvement, Item, ReadmeState, Repo,
    RepoLayout, Section, Span,
};
use crate::markdown::{self, Segment};
use crate::model;
use crate::store::Store;
use crate::theme::{self, Palette, RADIUS_CONTROL, ROW_COMPACT, ROW_HEIGHT, SPACE1, SPACE2, SPACE3, SPACE6};
use crate::util;
use crate::widgets::{self, Variant};

pub enum HubAction {
    Open(Section),
    Refresh(Section),
    SetInvolvement(Section, Involvement),
    SetExplore(ExploreQuery),
    SetLayout(RepoLayout),
    Select(Section, String),
    LoadReadme(String),
    Star(String, bool),
    OpenClone(String),
    CloneBrowse,
    CloneConfirm,
    CloneRetry,
    CloseClone,
    /// Check out a pull request (by URL) in its local clone.
    Checkout(String),
    /// Show a local clone in the repository list.
    ShowRepository(String),
    OpenUrl(String),
    CopyText(String),
    Connect,
    SignIn,
}

const LIST_ROW: f32 = 54.0;

/// Local clones by lowercase `owner/name`, including the originals that forks came from.
pub fn local_clones(store: &Store) -> HashMap<String, String> {
    let mut clones = HashMap::new();
    for repo in &store.repos {
        if let Some(slug) = repo.github_slug() {
            clones.entry(slug.to_lowercase()).or_insert_with(|| repo.path.clone());
        }
        if let Some((host, path)) = repo.fork_parent_url.as_deref().and_then(model::parse_remote)
            && host == "github.com"
        {
            clones.entry(path.to_lowercase()).or_insert_with(|| repo.path.clone());
        }
    }
    clones
}

fn hex_color(hex: &str) -> Option<Color32> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8))
}

fn centered(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    let rect = ui.available_rect_before_wrap();
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.y = SPACE2;
        ui.add_space((rect.height() / 2.0 - 110.0).max(SPACE6));
        content(ui);
    });
}

fn spinner_line(ui: &mut Ui, text: &str) {
    let p = theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    widgets::paint_spinner(ui, rect.center(), 16.0);
    ui.label(RichText::new(text).font(theme::body()).color(p.text_secondary));
}

fn empty_state(ui: &mut Ui, glyph: &str, title: &str, detail: &str, buttons: impl FnOnce(&mut Ui)) {
    let p = theme::palette(ui.ctx());
    centered(ui, |ui| {
        ui.label(RichText::new(glyph).font(theme::icon_font(34.0)).color(p.text_tertiary));
        ui.label(RichText::new(title).font(theme::title()).color(p.text_primary));
        ui.add(
            egui::Label::new(RichText::new(detail).font(theme::body()).color(p.text_secondary))
                .halign(Align::Center)
                .wrap(),
        );
        ui.add_space(SPACE1);
        // Buttons stack under the text, each centered.
        buttons(ui);
    });
}

/// Small rounded tag: tinted background, colored text.
fn chip(ui: &Ui, right_edge: f32, center_y: f32, text: &str, color: Color32) -> f32 {
    let font = theme::meta_medium();
    let width = widgets::text_width(ui, text, font.clone()) + 2.0 * SPACE2;
    let rect = Rect::from_min_size(pos2(right_edge - width, center_y - 10.0), vec2(width, 20.0));
    ui.painter().rect_filled(rect, CornerRadius::same(RADIUS_CONTROL), color.gamma_multiply(0.16));
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, font, color);
    width
}

fn review_chip(review: &str, p: &Palette) -> Option<(&'static str, Color32)> {
    match review {
        "APPROVED" => Some(("Approved", p.good)),
        "CHANGES_REQUESTED" => Some(("Changes requested", p.bad)),
        "REVIEW_REQUIRED" => Some(("Review required", p.notice)),
        _ => None,
    }
}

fn checks_glyph(checks: &str, p: &Palette) -> (&'static str, Color32, &'static str) {
    match checks {
        "SUCCESS" => (icon::CHECK_CIRCLE, p.good, "Checks passed"),
        "FAILURE" | "ERROR" => (icon::X_CIRCLE, p.bad, "Checks failed"),
        _ => (icon::CLOCK, p.notice, "Checks running"),
    }
}

// MARK: - Toolbar

fn search_box(ui: &mut Ui, search: &mut String, hint: &str) {
    let p = theme::palette(ui.ctx());
    let width = ui.available_width().clamp(150.0, 220.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW_COMPACT), Sense::hover());
    let id = Id::new("hub-search");
    let focused = ui.memory(|m| m.has_focus(id));
    let radius = CornerRadius::same(RADIUS_CONTROL);
    ui.painter().rect_filled(rect, radius, p.canvas);
    let stroke = Stroke::new(1.0, if focused { p.accent } else { p.border_strong });
    ui.painter().rect_stroke(rect, radius, stroke, StrokeKind::Inside);
    ui.painter().text(
        pos2(rect.left() + SPACE3 + 6.0, rect.center().y),
        Align2::CENTER_CENTER,
        icon::MAGNIFYING_GLASS,
        theme::icon_font(13.0),
        p.text_tertiary,
    );
    let edit_rect =
        Rect::from_min_max(pos2(rect.left() + SPACE3 + 18.0, rect.top()), pos2(rect.right() - SPACE2, rect.bottom()));
    ui.scope_builder(UiBuilder::new().max_rect(edit_rect).layout(Layout::left_to_right(Align::Center)), |ui| {
        ui.add(
            TextEdit::singleline(search)
                .id(id)
                .hint_text(RichText::new(hint).color(p.text_tertiary))
                .font(theme::body())
                .frame(Frame::NONE)
                .margin(Margin::ZERO)
                .desired_width(edit_rect.width()),
        );
    });
}

fn toolbar(ui: &mut Ui, section: Section, hub: &mut Hub, layout: RepoLayout, actions: &mut Vec<HubAction>) {
    let p = theme::palette(ui.ctx());
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    ui.painter().rect_filled(bar, 0.0, p.surface1);
    let today = Local::now().date_naive();
    let (loading, summary) = match section {
        Section::PullRequests => (hub.pulls.loading, format!("{} open", hub.pulls.total)),
        Section::Issues => (hub.issues.loading, format!("{} open", hub.issues.total)),
        Section::ReviewRequests => (hub.reviews.loading, format!("{} waiting", hub.reviews.total)),
        Section::Starred => (hub.starred.loading, format!("{} starred", hub.starred.total)),
        Section::Explore => (hub.explore.loading, format!("{} new repos", util::compact_count(hub.explore.total))),
    };
    let ready = hub.login().is_some();

    ui.scope_builder(
        UiBuilder::new().max_rect(bar.shrink2(vec2(SPACE3, 0.0))).layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = SPACE2;
            match section {
                Section::PullRequests | Section::Issues => {
                    let current = if section == Section::PullRequests { hub.pull_filter } else { hub.issue_filter };
                    let options: Vec<(&str, bool, &str)> =
                        Involvement::ALL.iter().map(|i| (i.label(), true, "")).collect();
                    let selected = Involvement::ALL.iter().position(|i| *i == current).unwrap_or(0);
                    if let Some(index) = widgets::segmented(ui, &options, selected) {
                        actions.push(HubAction::SetInvolvement(section, Involvement::ALL[index]));
                    }
                    search_box(ui, &mut hub.search, "Filter by title or repository…");
                }
                Section::ReviewRequests | Section::Starred => {
                    search_box(ui, &mut hub.search, "Filter by name or repository…");
                }
                Section::Explore => {
                    let query = hub.explore_query.clone();
                    let options: Vec<(&str, bool, &str)> = Span::ALL.iter().map(|s| (s.label(), true, "")).collect();
                    let selected = Span::ALL.iter().position(|s| *s == query.span).unwrap_or(1);
                    if let Some(index) = widgets::segmented(ui, &options, selected) {
                        actions.push(HubAction::SetExplore(ExploreQuery {
                            span: Span::ALL[index],
                            offset: 0,
                            ..query.clone()
                        }));
                    }
                    ui.add_space(SPACE1);
                    if widgets::icon_button(ui, icon::CARET_LEFT, "Earlier", query.can_go_back(today)).clicked() {
                        actions.push(HubAction::SetExplore(ExploreQuery { offset: query.offset + 1, ..query.clone() }));
                    }
                    let label = query.period_label(today);
                    let (label_rect, _) = ui.allocate_exact_size(vec2(150.0, ROW_COMPACT), Sense::hover());
                    ui.painter().text(
                        label_rect.center(),
                        Align2::CENTER_CENTER,
                        &label,
                        theme::body_medium(),
                        p.text_primary,
                    );
                    if widgets::icon_button(ui, icon::CARET_RIGHT, "Later", query.offset > 0).clicked() {
                        actions.push(HubAction::SetExplore(ExploreQuery { offset: query.offset - 1, ..query.clone() }));
                    }
                    ui.add_space(SPACE1);
                    let language_label = query.language.clone().unwrap_or_else(|| "Any language".into());
                    widgets::combo_box(ui, "explore-language", &language_label, 130.0, |ui| {
                        if ui.add(egui::Button::selectable(query.language.is_none(), "Any language")).clicked() {
                            actions.push(HubAction::SetExplore(ExploreQuery { language: None, ..query.clone() }));
                        }
                        for language in EXPLORE_LANGUAGES {
                            let chosen = query.language.as_deref() == Some(language);
                            if ui.add(egui::Button::selectable(chosen, language)).clicked() {
                                actions.push(HubAction::SetExplore(ExploreQuery {
                                    language: Some(language.into()),
                                    ..query.clone()
                                }));
                            }
                        }
                    });
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = SPACE1;
                if widgets::icon_button(ui, icon::ARROW_SQUARE_OUT, "Open on GitHub", true).clicked() {
                    actions.push(HubAction::OpenUrl(section.web_url().into()));
                }
                if loading {
                    let (rect, _) = ui.allocate_exact_size(vec2(ROW_COMPACT, ROW_COMPACT), Sense::hover());
                    widgets::paint_spinner(ui, rect.center(), 14.0);
                } else if widgets::icon_button(ui, icon::ARROW_CLOCKWISE, "Refresh", ready).clicked() {
                    actions.push(HubAction::Refresh(section));
                }
                if matches!(section, Section::Starred | Section::Explore) {
                    let options = [(icon::SQUARES_FOUR, true, "Cards"), (icon::ROWS, true, "List")];
                    let selected = usize::from(layout == RepoLayout::List);
                    if let Some(index) = widgets::segmented(ui, &options, selected) {
                        actions.push(HubAction::SetLayout(if index == 0 {
                            RepoLayout::Cards
                        } else {
                            RepoLayout::List
                        }));
                    }
                }
                if ready && section != Section::Explore && !summary.starts_with('0') {
                    ui.label(RichText::new(&summary).font(theme::meta()).color(p.text_tertiary));
                }
            });
        },
    );
}

// MARK: - Center

pub fn center(
    ui: &mut Ui,
    section: Section,
    hub: &mut Hub,
    store: &Store,
    layout: RepoLayout,
    actions: &mut Vec<HubAction>,
) {
    let p = theme::palette(ui.ctx());
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    toolbar(ui, section, hub, layout, actions);
    widgets::hline(ui, p.border);

    match &hub.connection {
        Connection::Unknown | Connection::Checking => {
            centered(ui, |ui| spinner_line(ui, "Connecting to GitHub…"));
            return;
        }
        Connection::NeedsCli => {
            empty_state(
                ui,
                icon::GITHUB_LOGO,
                "Connect GitHub",
                "This view uses the GitHub CLI to read your pull requests, issues, and stars. Install it, then sign in once.",
                |ui| {
                    if widgets::button(ui, "Get GitHub CLI", None, Variant::Primary, false, true).clicked() {
                        actions.push(HubAction::OpenUrl(crate::github::CLI_DOWNLOAD_URL.into()));
                    }
                    if widgets::button(ui, "Check Again", None, Variant::Secondary, false, true).clicked() {
                        actions.push(HubAction::Connect);
                    }
                },
            );
            return;
        }
        Connection::NeedsSignIn => {
            empty_state(
                ui,
                icon::GITHUB_LOGO,
                "Sign in to GitHub",
                "Sign In opens a terminal that finishes in your browser. Come back and click Check Again when it says you're logged in.",
                |ui| {
                    if widgets::button(ui, "Sign In…", None, Variant::Primary, false, true).clicked() {
                        actions.push(HubAction::SignIn);
                    }
                    if widgets::button(ui, "Check Again", None, Variant::Secondary, false, true).clicked() {
                        actions.push(HubAction::Connect);
                    }
                },
            );
            return;
        }
        Connection::Failed(message) => {
            let message = message.clone();
            empty_state(ui, icon::WARNING, "Couldn't reach GitHub", &message, |ui| {
                if widgets::button(ui, "Try Again", None, Variant::Primary, false, true).clicked() {
                    actions.push(HubAction::Connect);
                }
            });
            return;
        }
        Connection::Ready { .. } => {}
    }

    if section == Section::Explore {
        // Say plainly what "trending" means here: GitHub has no trending API.
        let (start, end) = hub.explore_query.range(Local::now().date_naive());
        let period = if start == end {
            format!("on {}", start.format("%b %-d, %Y"))
        } else {
            format!("between {} and {}", start.format("%b %-d"), end.format("%b %-d, %Y"))
        };
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_COMPACT), Sense::hover());
        let text = format!("Most-starred repositories created {period}.");
        widgets::paint_text(ui, bar.shrink2(vec2(SPACE3, 0.0)), &text, theme::meta(), p.text_tertiary, false);
        widgets::hline(ui, p.border);
    }

    let clones = local_clones(store);
    let query = hub.search.trim().to_lowercase();
    let selected = hub.selected.get(&section).cloned();
    match section {
        Section::PullRequests | Section::Issues | Section::ReviewRequests => {
            let feed = match section {
                Section::PullRequests => &hub.pulls,
                Section::Issues => &hub.issues,
                _ => &hub.reviews,
            };
            let involvement = if section == Section::PullRequests { hub.pull_filter } else { hub.issue_filter };
            item_list(ui, section, involvement, feed, &query, selected.as_deref(), &clones, actions);
        }
        Section::Starred | Section::Explore => {
            let feed = if section == Section::Starred { &hub.starred } else { &hub.explore };
            repo_list(ui, section, feed, &query, selected.as_deref(), &clones, layout, actions);
        }
    }
}

fn feed_placeholder<T>(ui: &mut Ui, feed: &Feed<T>, section: Section, actions: &mut Vec<HubAction>) -> bool {
    if feed.items.is_empty() && (feed.loading || !feed.has_loaded() && feed.error.is_none()) {
        centered(ui, |ui| spinner_line(ui, "Loading…"));
        return true;
    }
    if let Some(error) = feed.error.clone().filter(|_| feed.items.is_empty()) {
        empty_state(ui, icon::WARNING, "Couldn't load this list", &error, |ui| {
            if widgets::button(ui, "Try Again", None, Variant::Primary, false, true).clicked() {
                actions.push(HubAction::Refresh(section));
            }
        });
        return true;
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn item_list(
    ui: &mut Ui,
    section: Section,
    involvement: Involvement,
    feed: &Feed<Item>,
    query: &str,
    selected: Option<&str>,
    clones: &HashMap<String, String>,
    actions: &mut Vec<HubAction>,
) {
    if feed_placeholder(ui, feed, section, actions) {
        return;
    }
    let items: Vec<&Item> = feed
        .items
        .iter()
        .filter(|i| query.is_empty() || i.title.to_lowercase().contains(query) || i.repo.to_lowercase().contains(query))
        .collect();
    if items.is_empty() {
        let (title, detail) = if !query.is_empty() {
            ("No matches".to_string(), format!("Nothing here matches “{query}”."))
        } else {
            let kind = if section == Section::Issues { "issues" } else { "pull requests" };
            match (section, involvement) {
                (Section::ReviewRequests, _) => {
                    ("All caught up".into(), "Nobody is waiting on your review right now.".into())
                }
                (_, Involvement::Created) => {
                    (format!("No open {kind}"), format!("You haven't opened any {kind} that are still open."))
                }
                (_, Involvement::Assigned) => {
                    (format!("No open {kind}"), format!("No open {kind} are assigned to you."))
                }
                (_, Involvement::Mentioned) => (format!("No open {kind}"), format!("No open {kind} mention you.")),
            }
        };
        empty_state(ui, section.glyph(), &title, &detail, |_| {});
        return;
    }
    let p = theme::palette(ui.ctx());
    ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, LIST_ROW + 1.0, items.len(), |ui, range| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        for item in &items[range] {
            let has_clone = clones.contains_key(&item.repo.to_lowercase());
            let row = item_row(ui, item, selected == Some(item.url.as_str()), has_clone);
            if row.clicked() {
                actions.push(HubAction::Select(section, item.url.clone()));
            }
            if row.double_clicked() {
                actions.push(HubAction::OpenUrl(item.url.clone()));
            }
            widgets::hline(ui, p.border);
        }
    });
}

fn row_frame(ui: &mut Ui, selected: bool) -> (Rect, Response) {
    let p = theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), LIST_ROW), Sense::click());
    let hovered = ui.rect_contains_pointer(rect);
    let bg = if selected || hovered { p.surface2 } else { p.surface1 };
    ui.painter().rect_filled(rect, 0.0, bg);
    if selected {
        ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 0.0, p.accent);
    }
    (rect, response)
}

fn item_row(ui: &mut Ui, item: &Item, selected: bool, has_clone: bool) -> Response {
    let p = theme::palette(ui.ctx());
    let (rect, response) = row_frame(ui, selected);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let line1 = rect.top() + 18.0;
    let line2 = rect.top() + 37.0;
    let left = rect.left() + SPACE3;
    let (glyph, color) = if item.is_pr {
        (icon::GIT_PULL_REQUEST, if item.is_draft { p.text_tertiary } else { p.good })
    } else {
        (icon::RECORD, p.good)
    };
    ui.painter().text(pos2(left + 8.0, line1), Align2::CENTER_CENTER, glyph, theme::icon_font(16.0), color);
    let text_left = left + 16.0 + SPACE3;

    // Right side: checks and review on the first line, comments on the second.
    let mut right1 = rect.right() - SPACE3;
    if let Some(checks) = &item.checks {
        let (glyph, color, _) = checks_glyph(checks, p);
        ui.painter().text(pos2(right1 - 8.0, line1), Align2::CENTER_CENTER, glyph, theme::icon_font(15.0), color);
        right1 -= 16.0 + SPACE2;
    }
    if let Some((label, color)) = item.review_decision.as_deref().and_then(|r| review_chip(r, p)) {
        right1 -= chip(ui, right1, line1, label, color) + SPACE2;
    }
    let mut right2 = rect.right() - SPACE3;
    if item.comments > 0 {
        let count = item.comments.to_string();
        let r = ui.painter().text(pos2(right2, line2), Align2::RIGHT_CENTER, &count, theme::meta(), p.text_tertiary);
        ui.painter().text(
            pos2(r.left() - 9.0, line2),
            Align2::CENTER_CENTER,
            icon::CHAT_CIRCLE,
            theme::icon_font(13.0),
            p.text_tertiary,
        );
        right2 = r.left() - 18.0 - SPACE2;
    }

    let title_rect = Rect::from_min_max(pos2(text_left, line1 - 9.0), pos2(right1, line1 + 9.0));
    widgets::paint_text(ui, title_rect, &item.title, theme::body_medium(), p.text_primary, false);

    let mut meta = format!("{} #{}", item.repo, item.number);
    if item.is_draft {
        meta.push_str(" · Draft");
    }
    meta.push_str(&format!(" · updated {} · {}", util::relative_time(&item.updated_at), item.author));
    if has_clone {
        meta.push_str(" · cloned");
    }
    let meta_rect = Rect::from_min_max(pos2(text_left, line2 - 9.0), pos2(right2, line2 + 9.0));
    let mut x = text_left + widgets::paint_text(ui, meta_rect, &meta, theme::meta(), p.text_tertiary, false) + SPACE2;
    for label in item.labels.iter().take(3) {
        let width = widgets::text_width(ui, &label.name, theme::meta()) + 2.0 * SPACE2 + 10.0;
        if x + width > right2 {
            break;
        }
        let chip_rect = Rect::from_min_size(pos2(x, line2 - 9.0), vec2(width, 18.0));
        let color = hex_color(&label.color).unwrap_or(p.text_tertiary);
        ui.painter().rect_stroke(
            chip_rect,
            CornerRadius::same(9),
            Stroke::new(1.0, p.border_strong),
            StrokeKind::Inside,
        );
        ui.painter().circle_filled(pos2(chip_rect.left() + SPACE2 + 2.0, line2), 3.5, color);
        ui.painter().text(
            pos2(chip_rect.left() + SPACE2 + 10.0, line2),
            Align2::LEFT_CENTER,
            &label.name,
            theme::meta(),
            p.text_secondary,
        );
        x += width + SPACE1;
    }
    response
}

#[allow(clippy::too_many_arguments)]
fn repo_list(
    ui: &mut Ui,
    section: Section,
    feed: &Feed<Repo>,
    query: &str,
    selected: Option<&str>,
    clones: &HashMap<String, String>,
    layout: RepoLayout,
    actions: &mut Vec<HubAction>,
) {
    if feed_placeholder(ui, feed, section, actions) {
        return;
    }
    let repos: Vec<&Repo> = feed
        .items
        .iter()
        .filter(|r| {
            query.is_empty()
                || r.full_name.to_lowercase().contains(query)
                || r.description.to_lowercase().contains(query)
        })
        .collect();
    if repos.is_empty() {
        if !query.is_empty() {
            empty_state(ui, icon::MAGNIFYING_GLASS, "No matches", &format!("Nothing here matches “{query}”."), |_| {});
        } else if section == Section::Starred {
            empty_state(
                ui,
                icon::STAR,
                "No stars yet",
                "Star repositories on GitHub or in Explore and they show up here.",
                |ui| {
                    if widgets::button(ui, "Go to Explore", Some(icon::COMPASS), Variant::Primary, false, true)
                        .clicked()
                    {
                        actions.push(HubAction::Open(Section::Explore));
                    }
                },
            );
        } else {
            empty_state(
                ui,
                icon::COMPASS,
                "Nothing new",
                "No repositories were created in this period. Try another one.",
                |_| {},
            );
        }
        return;
    }
    if layout == RepoLayout::Cards {
        repo_cards(ui, section, &repos, selected, clones, actions);
        return;
    }
    let p = theme::palette(ui.ctx());
    ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, LIST_ROW + 1.0, repos.len(), |ui, range| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let first = range.start;
        for (offset, repo) in repos[range].iter().enumerate() {
            let rank = (section == Section::Explore).then_some(first + offset + 1);
            let cloned = clones.contains_key(&repo.full_name.to_lowercase());
            let row = repo_row(ui, repo, selected == Some(repo.full_name.as_str()), cloned, rank);
            if row.clicked() {
                actions.push(HubAction::Select(section, repo.full_name.clone()));
            }
            if row.double_clicked() {
                actions.push(HubAction::OpenUrl(repo.url.clone()));
            }
            widgets::hline(ui, p.border);
        }
    });
}

fn repo_row(ui: &mut Ui, repo: &Repo, selected: bool, cloned: bool, rank: Option<usize>) -> Response {
    let p = theme::palette(ui.ctx());
    let (rect, response) = row_frame(ui, selected);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let line1 = rect.top() + 18.0;
    let line2 = rect.top() + 37.0;
    let left = rect.left() + SPACE3;
    match rank {
        Some(rank) => {
            let rank_rect = Rect::from_min_size(pos2(left, line1 - 9.0), vec2(22.0, 18.0));
            widgets::paint_text(ui, rank_rect, &rank.to_string(), theme::meta_medium(), p.text_tertiary, true);
        }
        None => {
            ui.painter().text(
                pos2(left + 9.0, line1),
                Align2::CENTER_CENTER,
                icon::BOOK_BOOKMARK,
                theme::icon_font(15.0),
                p.text_secondary,
            );
        }
    }
    let text_left = left + 22.0 + SPACE3;

    // Right side: stars on the first line, language on the second.
    let stars = util::compact_count(repo.stars);
    let star_color = if repo.starred { p.notice } else { p.text_tertiary };
    let r = ui.painter().text(
        pos2(rect.right() - SPACE3, line1),
        Align2::RIGHT_CENTER,
        &stars,
        theme::meta_medium(),
        p.text_secondary,
    );
    ui.painter().text(
        pos2(r.left() - 9.0, line1),
        Align2::CENTER_CENTER,
        icon::STAR,
        theme::icon_font(13.0),
        star_color,
    );
    let mut right1 = r.left() - 18.0 - SPACE2;
    let mut right2 = rect.right() - SPACE3;
    if let Some((language, color)) = &repo.language {
        let r = ui.painter().text(pos2(right2, line2), Align2::RIGHT_CENTER, language, theme::meta(), p.text_tertiary);
        ui.painter().circle_filled(pos2(r.left() - 8.0, line2), 4.0, hex_color(color).unwrap_or(p.text_tertiary));
        right2 = r.left() - 14.0 - SPACE2;
    }
    if cloned {
        right1 -= chip(ui, right1, line1, "Cloned", p.good) + SPACE2;
    }

    let title_rect = Rect::from_min_max(pos2(text_left, line1 - 9.0), pos2(right1, line1 + 9.0));
    widgets::paint_text(ui, title_rect, &repo.full_name, theme::body_medium(), p.text_primary, false);
    let description = if repo.description.is_empty() { "No description" } else { repo.description.as_str() };
    let description_rect = Rect::from_min_max(pos2(text_left, line2 - 9.0), pos2(right2, line2 + 9.0));
    widgets::paint_text(ui, description_rect, description, theme::meta(), p.text_secondary, false);
    response
}

// MARK: - Card view

const CARD_MIN_WIDTH: f32 = 220.0;
const CARD_GAP: f32 = 12.0;
/// Space under the preview image: padding, two title lines, three description lines, meta line.
const CARD_TEXT_HEIGHT: f32 = 148.0;

/// Lays `text` out over at most `max_rows` lines of `width`, ending in `…` if it is longer.
fn clamped(ui: &Ui, text: &str, font: egui::FontId, color: Color32, width: f32, max_rows: usize) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
    job.wrap =
        TextWrapping { max_width: width.max(1.0), max_rows, break_anywhere: false, overflow_character: Some('…') };
    ui.painter().layout_job(job)
}

/// Paints the repository's social preview image, cropped to fill `rect`, or a placeholder.
fn paint_preview(ui: &Ui, rect: Rect, repo: &Repo, corner: CornerRadius) {
    let p = theme::palette(ui.ctx());
    let texture =
        repo.image_url.as_deref().and_then(|url| match egui::Image::new(url).load_for_size(ui.ctx(), rect.size()) {
            Ok(TexturePoll::Ready { texture }) => Some(texture),
            _ => None,
        });
    match texture {
        Some(texture) => {
            // Center-crop to the card's aspect ratio, like CSS `object-fit: cover`.
            let (tw, th) = (texture.size.x.max(1.0), texture.size.y.max(1.0));
            let (target, actual) = (rect.width() / rect.height(), tw / th);
            let uv = if actual > target {
                let f = target / actual;
                Rect::from_min_max(pos2((1.0 - f) / 2.0, 0.0), pos2((1.0 + f) / 2.0, 1.0))
            } else {
                let f = actual / target;
                Rect::from_min_max(pos2(0.0, (1.0 - f) / 2.0), pos2(1.0, (1.0 + f) / 2.0))
            };
            egui::Image::from_texture(texture).uv(uv).corner_radius(corner).paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, corner, p.surface2);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                icon::BOOK_BOOKMARK,
                theme::icon_font(30.0),
                p.text_tertiary,
            );
        }
    }
    // A strip in the language's color along the bottom edge, as on GitHub's cards.
    if let Some(color) = repo.language.as_ref().and_then(|(_, c)| hex_color(c)) {
        let strip = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 3.0), rect.max);
        ui.painter().rect_filled(strip, 0.0, color);
    }
}

fn repo_card(ui: &mut Ui, rect: Rect, repo: &Repo, rank: Option<usize>, selected: bool, cloned: bool) -> Response {
    let p = theme::palette(ui.ctx());
    let response = ui.interact(rect, Id::new(("repo-card", &repo.full_name)), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let radius = theme::RADIUS_CONTAINER;
    ui.painter().rect_filled(rect, CornerRadius::same(radius), p.surface1);
    let image_rect = Rect::from_min_size(rect.min, vec2(rect.width(), (rect.width() / 2.0).round()));
    paint_preview(ui, image_rect, repo, CornerRadius { nw: radius, ne: radius, sw: 0, se: 0 });

    let inner =
        Rect::from_min_max(image_rect.left_bottom() + vec2(SPACE3, SPACE3), rect.right_bottom() - vec2(SPACE3, SPACE3));
    let title = clamped(ui, &repo.full_name, theme::body_medium(), p.text_primary, inner.width(), 2);
    let title_height = title.size().y;
    ui.painter().galley(inner.min, title, p.text_primary);
    let description = if repo.description.is_empty() { "No description" } else { repo.description.as_str() };
    let description = clamped(ui, description, theme::meta(), p.text_secondary, inner.width(), 3);
    ui.painter().galley(inner.min + vec2(0.0, title_height + SPACE1), description, p.text_secondary);

    // Bottom line: stars, language, last push; the Explore rank and a Cloned chip on the right.
    let y = inner.bottom() - 8.0;
    let star_color = if repo.starred { p.notice } else { p.text_tertiary };
    ui.painter().text(
        pos2(inner.left() + 6.0, y),
        Align2::CENTER_CENTER,
        icon::STAR,
        theme::icon_font(13.0),
        star_color,
    );
    let mut x = ui
        .painter()
        .text(
            pos2(inner.left() + 15.0, y),
            Align2::LEFT_CENTER,
            util::compact_count(repo.stars),
            theme::meta_medium(),
            p.text_secondary,
        )
        .right()
        + SPACE3;
    if let Some((language, color)) = &repo.language {
        ui.painter().circle_filled(pos2(x + 4.0, y), 4.0, hex_color(color).unwrap_or(p.text_tertiary));
        x = ui.painter().text(pos2(x + 12.0, y), Align2::LEFT_CENTER, language, theme::meta(), p.text_tertiary).right()
            + SPACE3;
    }
    let mut right = inner.right();
    if let Some(rank) = rank {
        let text = format!("#{rank}");
        right =
            ui.painter().text(pos2(right, y), Align2::RIGHT_CENTER, text, theme::meta_medium(), p.text_tertiary).left()
                - SPACE2;
    }
    if cloned {
        right -= chip(ui, right, y, "Cloned", p.good) + SPACE2;
    }
    if !repo.pushed_at.is_empty() && x < right {
        let when = Rect::from_min_max(pos2(x, y - 9.0), pos2(right, y + 9.0));
        widgets::paint_text(ui, when, &util::relative_time(&repo.pushed_at), theme::meta(), p.text_tertiary, false);
    }

    let stroke = if selected {
        Stroke::new(2.0, p.accent)
    } else if response.hovered() {
        Stroke::new(1.0, p.border_strong)
    } else {
        Stroke::new(1.0, p.border)
    };
    ui.painter().rect_stroke(rect, CornerRadius::same(radius), stroke, StrokeKind::Inside);
    response.on_hover_text(&repo.full_name)
}

fn repo_cards(
    ui: &mut Ui,
    section: Section,
    repos: &[&Repo],
    selected: Option<&str>,
    clones: &HashMap<String, String>,
    actions: &mut Vec<HubAction>,
) {
    let available = ui.available_width() - 2.0 * CARD_GAP;
    let columns = ((available + CARD_GAP) / (CARD_MIN_WIDTH + CARD_GAP)).floor().max(1.0) as usize;
    let card_width = ((available - CARD_GAP * (columns as f32 - 1.0)) / columns as f32).floor();
    let card_height = (card_width / 2.0).round() + CARD_TEXT_HEIGHT;
    let rows = repos.len().div_ceil(columns);
    ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, card_height + CARD_GAP, rows, |ui, range| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        for row in range {
            let (row_rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), card_height + CARD_GAP), Sense::hover());
            for column in 0..columns {
                let index = row * columns + column;
                let Some(repo) = repos.get(index) else { break };
                let x = row_rect.left() + CARD_GAP + column as f32 * (card_width + CARD_GAP);
                let rect = Rect::from_min_size(pos2(x, row_rect.top() + CARD_GAP), vec2(card_width, card_height));
                let rank = (section == Section::Explore).then_some(index + 1);
                let cloned = clones.contains_key(&repo.full_name.to_lowercase());
                let card = repo_card(ui, rect, repo, rank, selected == Some(repo.full_name.as_str()), cloned);
                if card.clicked() {
                    actions.push(HubAction::Select(section, repo.full_name.clone()));
                }
                if card.double_clicked() {
                    actions.push(HubAction::OpenUrl(repo.url.clone()));
                }
            }
        }
        // Breathing room under the last row.
        ui.add_space(CARD_GAP);
    });
}

// MARK: - Inspector

fn prepared(hub: &mut Hub, key: &str, text: &str) -> Rc<Vec<Segment>> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    let hash = hasher.finish();
    if let Some((cached, segments)) = hub.md_segments.get(key)
        && *cached == hash
    {
        return Rc::clone(segments);
    }
    let segments = Rc::new(markdown::split_tables(&markdown::prepare_remote(text)));
    hub.md_segments.insert(key.to_string(), (hash, Rc::clone(&segments)));
    segments
}

fn placeholder(ui: &mut Ui, text: &str) {
    let p = theme::palette(ui.ctx());
    centered(ui, |ui| {
        ui.add(
            egui::Label::new(RichText::new(text).font(theme::body()).color(p.text_tertiary))
                .halign(Align::Center)
                .wrap(),
        );
    });
}

fn meta_line(ui: &mut Ui, label: &str, value: &str, monospace: bool) {
    let p = theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_COMPACT), Sense::hover());
    let inner = rect.shrink2(vec2(SPACE3, 0.0));
    let label_rect = Rect::from_min_size(inner.min, vec2(72.0, inner.height()));
    widgets::paint_text(ui, label_rect, label, theme::meta(), p.text_tertiary, false);
    let value_rect = Rect::from_min_max(pos2(label_rect.right() + SPACE2, inner.top()), inner.max);
    let font = if monospace { theme::meta_mono() } else { theme::meta() };
    widgets::paint_text(ui, value_rect, value, font, p.text_primary, false);
    response.on_hover_text(value);
}

pub fn inspector(
    ui: &mut Ui,
    section: Section,
    hub: &mut Hub,
    store: &Store,
    md_cache: &mut CommonMarkCache,
    actions: &mut Vec<HubAction>,
) {
    let Some(key) = hub.selected.get(&section).cloned() else {
        let text = match section {
            Section::PullRequests | Section::ReviewRequests => "Select a pull request to see its details.",
            Section::Issues => "Select an issue to see its details.",
            Section::Starred | Section::Explore => "Select a repository to see its README and clone it.",
        };
        placeholder(ui, text);
        return;
    };
    let clones = local_clones(store);
    match section {
        Section::PullRequests | Section::Issues | Section::ReviewRequests => {
            let feed = match section {
                Section::PullRequests => &hub.pulls,
                Section::Issues => &hub.issues,
                _ => &hub.reviews,
            };
            let Some(item) = feed.items.iter().find(|i| i.url == key).cloned() else {
                placeholder(ui, "This item is no longer in the list.");
                return;
            };
            let clone = clones.get(&item.repo.to_lowercase()).cloned();
            let checking_out = hub.checking_out.contains(&item.url);
            let segments = (!item.body.trim().is_empty()).then(|| prepared(hub, &item.url, &item.body));
            item_inspector(
                ui,
                &item,
                clone.as_deref(),
                checking_out,
                segments.as_ref().map(|s| s.as_slice()),
                md_cache,
                actions,
            );
        }
        Section::Starred | Section::Explore => {
            let feed = if section == Section::Starred { &hub.starred } else { &hub.explore };
            let Some(repo) = feed.items.iter().find(|r| r.full_name == key).cloned() else {
                placeholder(ui, "This repository is no longer in the list.");
                return;
            };
            let clone = clones.get(&repo.full_name.to_lowercase()).cloned();
            let readme = match hub.readmes.get(&repo.full_name) {
                None => {
                    actions.push(HubAction::LoadReadme(repo.full_name.clone()));
                    ReadmeView::Loading
                }
                Some(ReadmeState::Loading) => ReadmeView::Loading,
                Some(ReadmeState::Missing) => ReadmeView::Missing,
                Some(ReadmeState::Failed(message)) => ReadmeView::Failed(message.clone()),
                Some(ReadmeState::Ready(text)) => {
                    let text = text.clone();
                    ReadmeView::Ready(prepared(hub, &repo.full_name, &text))
                }
            };
            repo_inspector(ui, &repo, clone.as_deref(), readme, md_cache, actions);
        }
    }
}

enum ReadmeView {
    Loading,
    Missing,
    Failed(String),
    Ready(Rc<Vec<Segment>>),
}

fn item_inspector(
    ui: &mut Ui,
    item: &Item,
    clone: Option<&str>,
    checking_out: bool,
    body: Option<&[Segment]>,
    md_cache: &mut CommonMarkCache,
    actions: &mut Vec<HubAction>,
) {
    let p = theme::palette(ui.ctx());
    ScrollArea::vertical().id_salt(("hub-item", &item.url)).auto_shrink([false, false]).show(ui, |ui| {
        Frame::NONE.inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = SPACE2;
            let (glyph, color) = if item.is_pr {
                (icon::GIT_PULL_REQUEST, if item.is_draft { p.text_tertiary } else { p.good })
            } else {
                (icon::RECORD, p.good)
            };
            ui.add(
                egui::Label::new(
                    RichText::new(format!("{glyph}  {}", item.title)).font(theme::title()).color(p.text_primary),
                )
                .wrap(),
            );
            ui.label(
                RichText::new(format!("{} #{}", item.repo, item.number))
                    .font(theme::meta_mono())
                    .color(p.text_tertiary),
            );

            // Status chips.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE1, SPACE1);
                let tag = |ui: &mut Ui, text: &str, color: Color32| {
                    let width = widgets::text_width(ui, text, theme::meta_medium()) + 2.0 * SPACE2;
                    let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
                    chip(ui, rect.right(), rect.center().y, text, color);
                };
                let state = if item.is_draft { ("Draft", p.text_secondary) } else { ("Open", color) };
                tag(ui, state.0, state.1);
                if let Some((label, color)) = item.review_decision.as_deref().and_then(|r| review_chip(r, p)) {
                    tag(ui, label, color);
                }
                if let Some(checks) = &item.checks {
                    let (_, color, label) = checks_glyph(checks, p);
                    tag(ui, label, color);
                }
            });

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE2;
                if widgets::button(ui, "Open on GitHub", Some(icon::ARROW_SQUARE_OUT), Variant::Primary, false, true)
                    .clicked()
                {
                    actions.push(HubAction::OpenUrl(item.url.clone()));
                }
                if let Some(path) = clone {
                    if item.is_pr {
                        let label = if checking_out { "Checking Out…" } else { "Check Out" };
                        if widgets::button(ui, label, Some(icon::GIT_BRANCH), Variant::Secondary, false, !checking_out)
                            .on_hover_text("Switch your local clone to this pull request's branch (gh pr checkout)")
                            .clicked()
                        {
                            actions.push(HubAction::Checkout(item.url.clone()));
                        }
                    }
                    if widgets::button(ui, "Show Repository", None, Variant::Ghost, false, true)
                        .on_hover_text(path)
                        .clicked()
                    {
                        actions.push(HubAction::ShowRepository(path.to_string()));
                    }
                } else if widgets::button(ui, "Copy Link", Some(icon::LINK), Variant::Ghost, false, true).clicked() {
                    actions.push(HubAction::CopyText(item.url.clone()));
                }
            });

            widgets::container_frame(p).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                meta_line(ui, "Author", &item.author, false);
                widgets::hline(ui, p.border);
                if let (Some(head), Some(base)) = (&item.head_ref, &item.base_ref) {
                    meta_line(ui, "Branch", &format!("{head}  →  {base}"), true);
                    widgets::hline(ui, p.border);
                }
                meta_line(ui, "Opened", &util::relative_time(&item.created_at), false);
                widgets::hline(ui, p.border);
                meta_line(ui, "Updated", &util::relative_time(&item.updated_at), false);
                widgets::hline(ui, p.border);
                meta_line(ui, "Comments", &item.comments.to_string(), false);
                if !item.labels.is_empty() {
                    widgets::hline(ui, p.border);
                    let labels: Vec<&str> = item.labels.iter().map(|l| l.name.as_str()).collect();
                    meta_line(ui, "Labels", &labels.join(", "), false);
                }
            });

            ui.add_space(SPACE2);
            ui.label(RichText::new("Description").font(theme::meta_medium()).color(p.text_secondary));
            widgets::container_frame(p).inner_margin(Margin::same(12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                match body {
                    Some(segments) => {
                        let base = format!("https://github.com/{}/blob/HEAD/", item.repo);
                        widgets::markdown(ui, md_cache, segments, &base, ("hub-body", item.number));
                    }
                    None => {
                        ui.label(RichText::new("No description provided.").font(theme::meta()).color(p.text_tertiary));
                    }
                }
            });
        });
    });
}

fn repo_inspector(
    ui: &mut Ui,
    repo: &Repo,
    clone: Option<&str>,
    readme: ReadmeView,
    md_cache: &mut CommonMarkCache,
    actions: &mut Vec<HubAction>,
) {
    let p = theme::palette(ui.ctx());
    ScrollArea::vertical().id_salt(("hub-repo", &repo.full_name)).auto_shrink([false, false]).show(ui, |ui| {
        Frame::NONE.inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = SPACE2;
            let name = repo.full_name.split('/').nth(1).unwrap_or(&repo.full_name);
            ui.add(egui::Label::new(RichText::new(name).font(theme::title()).color(p.text_primary)).truncate());
            ui.label(RichText::new(&repo.full_name).font(theme::meta_mono()).color(p.text_tertiary));
            if !repo.description.is_empty() {
                ui.add(
                    egui::Label::new(RichText::new(&repo.description).font(theme::body()).color(p.text_secondary))
                        .wrap(),
                );
            }

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(SPACE3, SPACE1);
                let star_color = if repo.starred { p.notice } else { p.text_tertiary };
                ui.label(
                    RichText::new(format!("{} {}", icon::STAR, util::compact_count(repo.stars)))
                        .font(theme::meta())
                        .color(star_color),
                );
                ui.label(
                    RichText::new(format!("{} {}", icon::GIT_FORK, util::compact_count(repo.forks)))
                        .font(theme::meta())
                        .color(p.text_tertiary),
                );
                if let Some((language, color)) = &repo.language {
                    let dot = hex_color(color).unwrap_or(p.text_tertiary);
                    ui.label(RichText::new("●").font(theme::meta()).color(dot));
                    ui.add_space(-SPACE2);
                    ui.label(RichText::new(language).font(theme::meta()).color(p.text_tertiary));
                }
                if !repo.pushed_at.is_empty() {
                    ui.label(
                        RichText::new(format!("updated {}", util::relative_time(&repo.pushed_at)))
                            .font(theme::meta())
                            .color(p.text_tertiary),
                    );
                }
            });
            if !repo.topics.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(SPACE1, SPACE1);
                    for topic in &repo.topics {
                        let width = widgets::text_width(ui, topic, theme::meta_medium()) + 2.0 * SPACE2;
                        let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
                        chip(ui, rect.right(), rect.center().y, topic, p.accent);
                    }
                });
            }

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE2;
                match clone {
                    Some(path) => {
                        if widgets::button(ui, "Show Repository", None, Variant::Primary, false, true)
                            .on_hover_text(path)
                            .clicked()
                        {
                            actions.push(HubAction::ShowRepository(path.to_string()));
                        }
                    }
                    None => {
                        if widgets::button(ui, "Clone…", Some(icon::DOWNLOAD_SIMPLE), Variant::Primary, false, true)
                            .on_hover_text("Clone into one of your watched folders")
                            .clicked()
                        {
                            actions.push(HubAction::OpenClone(repo.full_name.clone()));
                        }
                    }
                }
                let (label, tip) =
                    if repo.starred { ("Starred", "Remove your star") } else { ("Star", "Star on GitHub") };
                if widgets::button(ui, label, Some(icon::STAR), Variant::Secondary, false, true)
                    .on_hover_text(tip)
                    .clicked()
                {
                    actions.push(HubAction::Star(repo.full_name.clone(), !repo.starred));
                }
                if widgets::icon_button(ui, icon::ARROW_SQUARE_OUT, "Open on GitHub", true).clicked() {
                    actions.push(HubAction::OpenUrl(repo.url.clone()));
                }
            });

            ui.add_space(SPACE2);
            ui.label(RichText::new("README").font(theme::meta_medium()).color(p.text_secondary));
            widgets::container_frame(p).inner_margin(Margin::same(12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                match readme {
                    ReadmeView::Loading => {
                        ui.horizontal(|ui| spinner_line(ui, "Loading README…"));
                    }
                    ReadmeView::Missing => {
                        ui.label(
                            RichText::new("This repository has no README.").font(theme::meta()).color(p.text_tertiary),
                        );
                    }
                    ReadmeView::Failed(message) => {
                        ui.label(
                            RichText::new(format!("{}  {message}", icon::WARNING)).font(theme::meta()).color(p.bad),
                        );
                    }
                    ReadmeView::Ready(segments) => {
                        let base = format!("https://github.com/{}/blob/HEAD/", repo.full_name);
                        widgets::markdown(ui, md_cache, &segments, &base, ("hub-readme", repo.stars));
                    }
                }
            });
        });
    });
}

// MARK: - Clone dialog

pub fn clone_dialog(ctx: &egui::Context, hub: &mut Hub, store: &Store, actions: &mut Vec<HubAction>) {
    let Some(dialog) = hub.clone.as_mut() else { return };
    let p = theme::palette(ctx);
    let destination = dialog.destination();
    let problem = crate::github::local_path_problem(&destination)
        .or_else(|| dialog.name.trim().is_empty().then_some("Enter a folder name."));
    let working = matches!(dialog.stage, CloneStage::Working);

    let modal = egui::Modal::new(Id::new("clone-dialog"))
        .frame(
            Frame::new()
                .fill(p.surface1)
                .stroke(Stroke::new(1.0, p.border_strong))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(20)),
        )
        .show(ctx, |ui| {
            ui.set_width(460.0);
            ui.spacing_mut().item_spacing.y = SPACE2;
            ui.horizontal(|ui| {
                let (badge, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                ui.painter().rect_filled(
                    badge,
                    CornerRadius::same(theme::RADIUS_CONTAINER),
                    p.accent.gamma_multiply(0.16),
                );
                ui.painter().text(
                    badge.center(),
                    Align2::CENTER_CENTER,
                    icon::DOWNLOAD_SIMPLE,
                    theme::icon_font(21.0),
                    p.accent,
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let name = dialog.slug.split('/').nth(1).unwrap_or(&dialog.slug).to_string();
                    ui.label(RichText::new(format!("Clone {name}")).font(theme::title()).color(p.text_primary));
                    ui.label(RichText::new(&dialog.slug).font(theme::meta_mono()).color(p.text_tertiary));
                });
            });
            ui.add_space(SPACE1);

            match &dialog.stage {
                CloneStage::Choosing => {
                    ui.label(RichText::new("Into").font(theme::meta_medium()).color(p.text_tertiary));
                    ui.horizontal(|ui| {
                        let browse_w = widgets::text_width(ui, "Browse…", theme::body_medium()) + 2.0 * SPACE3;
                        let combo_w = (ui.available_width() - browse_w - SPACE2).max(120.0);
                        let current = util::abbreviate_home(&dialog.parent);
                        let parent = &mut dialog.parent;
                        widgets::combo_box(ui, "clone-parent", &current, combo_w - 24.0, |ui| {
                            for folder in &store.settings.scan_folders {
                                let label = format!("{}  {}", util::folder_name(folder), util::abbreviate_home(folder));
                                if ui.add(egui::Button::selectable(parent == folder, label)).clicked() {
                                    *parent = folder.clone();
                                }
                            }
                        });
                        if widgets::button(ui, "Browse…", None, Variant::Secondary, false, true).clicked() {
                            actions.push(HubAction::CloneBrowse);
                        }
                    });
                    ui.label(RichText::new("Folder name").font(theme::meta_medium()).color(p.text_tertiary));
                    ui.add_sized(
                        vec2(ui.available_width(), ROW_HEIGHT),
                        TextEdit::singleline(&mut dialog.name)
                            .font(theme::meta_mono())
                            .margin(Margin::symmetric(10, 7)),
                    );
                    match problem {
                        Some(problem) => {
                            ui.label(RichText::new(problem).font(theme::meta()).color(p.notice));
                        }
                        None => {
                            ui.label(
                                RichText::new(format!("Clones to {}", util::abbreviate_home(&destination)))
                                    .font(theme::meta())
                                    .color(p.text_tertiary),
                            );
                        }
                    }
                }
                CloneStage::Working => {
                    ui.horizontal(|ui| spinner_line(ui, &format!("Cloning {}…", dialog.slug)));
                }
                CloneStage::Done { path } => {
                    ui.label(
                        RichText::new(format!("{}  Cloned to {}", icon::CHECK_CIRCLE, util::abbreviate_home(path)))
                            .font(theme::body_medium())
                            .color(p.good),
                    );
                    ui.label(
                        RichText::new("It's in your repository list now.").font(theme::body()).color(p.text_secondary),
                    );
                }
                CloneStage::Failed { message } => {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{}  {message}", icon::WARNING)).font(theme::body()).color(p.bad),
                        )
                        .wrap(),
                    );
                }
            }

            ui.add_space(SPACE2);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = SPACE2;
                match &dialog.stage {
                    CloneStage::Choosing => {
                        if widgets::button(
                            ui,
                            "Clone",
                            Some(icon::DOWNLOAD_SIMPLE),
                            Variant::Primary,
                            false,
                            problem.is_none(),
                        )
                        .clicked()
                        {
                            actions.push(HubAction::CloneConfirm);
                        }
                        if widgets::button(ui, "Cancel", None, Variant::Ghost, false, true).clicked() {
                            actions.push(HubAction::CloseClone);
                        }
                    }
                    CloneStage::Working => {}
                    CloneStage::Done { path } => {
                        if widgets::button(ui, "Done", None, Variant::Primary, false, true).clicked() {
                            actions.push(HubAction::CloseClone);
                        }
                        if widgets::button(ui, "Show Repository", None, Variant::Secondary, false, true).clicked() {
                            actions.push(HubAction::ShowRepository(path.clone()));
                        }
                    }
                    CloneStage::Failed { .. } => {
                        if widgets::button(ui, "Try Again", None, Variant::Primary, false, true).clicked() {
                            actions.push(HubAction::CloneRetry);
                        }
                        if widgets::button(ui, "Close", None, Variant::Ghost, false, true).clicked() {
                            actions.push(HubAction::CloseClone);
                        }
                    }
                }
            });
        });
    if modal.should_close() && !working {
        actions.push(HubAction::CloseClone);
    }
}
