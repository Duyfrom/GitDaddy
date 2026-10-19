use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use egui::load::SizedTexture;
use egui::{
    Align, Align2, Color32, CornerRadius, Frame, Id, Key, KeyboardShortcut, Layout, Margin, Modifiers, Panel, Popup,
    Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, TextureHandle, ThemePreference, Ui, UiBuilder,
    pos2, vec2,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use egui_phosphor::regular as icon;

use crate::github::{self, ForkDone, ForkEvent, ForkFlow, ForkPhase, ForkStage, ForkTarget};
use crate::hub::{Hub, HubEvent, Section};
use crate::hub_ui::{self, HubAction};
use crate::model::{GitRepo, PullStrategy, RepoFilter, RepoSortOrder, SyncState};
use crate::store::{Counts, Scope, Settings, Store, ThemeChoice};
use crate::theme::{
    self, ICON_COMPACT, ICON_SIZE, Palette, RADIUS_CONTROL, ROW_COMPACT, ROW_HEIGHT, SPACE1, SPACE2, SPACE3, SPACE4,
    SPACE6,
};
use crate::update::{self, Phase, Updater};
use crate::widgets::{self, Variant};
use crate::{git, markdown, platform, util};

const SETTINGS_KEY: &str = "gitdaddy.settings";
const LOGO_PNG: &[u8] = include_bytes!("../assets/logo-128.png");
const SEARCH_ID: &str = "repo-search";
const SIDEBAR_WIDTH: f32 = 224.0;
const INSPECTOR_WIDTH: f32 = 360.0;
const MAX_VISIBLE_CHANGES: usize = 30;
const UPDATE_CHECK_INTERVAL_SECS: i64 = 24 * 60 * 60;

enum Action {
    Focus(String),
    ToggleSelect(String),
    Check(String),
    Pull(String),
    Push(String),
    RequestDelete(Vec<String>),
    ConfirmDelete { to_recycle_bin: bool },
    CancelDelete,
    OpenExplorer(String),
    OpenTerminal(String),
    CopyPath(String),
    CommitAndPush(String),
    AddRemote(String),
    SetFilter(RepoFilter),
    SetScope(Scope),
    ResetFilters,
    FocusSearch,
    AddFolder,
    RemoveFolder(String),
    Rescan { check_remote: bool },
    SetScanNested(bool),
    SetPullStrategy(PullStrategy),
    SetAutoStash(bool),
    SetTheme(ThemeChoice),
    CheckAll,
    CheckSelected,
    PullSelected,
    PullAllOutdated,
    PullOutdatedOrAll,
    PushSelected,
    PushAllUnpushed,
    SelectAllVisible,
    SelectOutdated,
    SelectUnpushed,
    ClearSelection,
    ShowActivityLog(bool),
    ClearActivityLog,
    ToggleInspector,
    ToggleReadme,
    CheckForUpdates,
    ShowUpdateDialog,
    DownloadUpdate,
    RetryUpdate,
    CancelUpdate,
    DismissUpdate,
    SetAutoUpdateCheck(bool),
    OpenUrl(String),
    OpenFork(String),
    ForkSetTarget(ForkTarget),
    ForkBrowse,
    ForkConfirm,
    ForkRecheck,
    ForkRetry,
    Hub(HubAction),
    ForkSignIn,
    ForkClose,
}

/// UI-only state that is not part of the repository model.
struct ViewState {
    /// A GitHub view in the center, or `None` for the repository list.
    section: Option<Section>,
    scope: Scope,
    filter: RepoFilter,
    sort: RepoSortOrder,
    search: String,
    show_activity_log: bool,
    pending_delete: Vec<String>,
    commit_messages: HashMap<String, String>,
    remote_urls: HashMap<String, String>,
    readme_expanded: bool,
    readme_cache: HashMap<String, (u64, Rc<Vec<markdown::Segment>>)>,
    md_cache: CommonMarkCache,
    logo: Option<TextureHandle>,
}

pub struct GitDaddyApp {
    store: Store,
    view: ViewState,
    updater: Updater,
    fork: Option<ForkFlow>,
    hub: Hub,
    applied_theme: Option<ThemeChoice>,
    closing: bool,
}

impl GitDaddyApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings: Settings =
            cc.storage.and_then(|storage| eframe::get_value(storage, SETTINGS_KEY)).unwrap_or_default();
        Self::with_settings(cc, settings, true)
    }

    fn with_settings(cc: &eframe::CreationContext<'_>, settings: Settings, scan_on_start: bool) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        theme::install_style(&cc.egui_ctx);
        crate::images::install(&cc.egui_ctx);

        let logo = image::load_from_memory(LOGO_PNG).ok().map(|img| {
            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
            cc.egui_ctx.load_texture("gitdaddy-logo", color, egui::TextureOptions::LINEAR)
        });

        let mut updater = Updater::new(cc.egui_ctx.clone());
        let update_due = settings.last_update_check.is_none_or(|last| unix_now() - last >= UPDATE_CHECK_INTERVAL_SECS);
        if scan_on_start {
            update::remove_old_downloads();
            if settings.check_updates_automatically && update_due {
                updater.check(false);
            }
        }

        let mut hub = Hub::new(cc.egui_ctx.clone());
        if scan_on_start {
            // Quietly check the GitHub sign-in so the sidebar can show counts.
            hub.connect();
        }

        let mut store = Store::new(cc.egui_ctx.clone(), settings);
        if git::git_executable().is_none() {
            store.status_banner =
                "Git for Windows was not found on PATH. Install it from git-scm.com, then restart GitDaddy.".into();
        }
        if scan_on_start {
            store.scan_directories(true);
        }

        Self {
            store,
            view: ViewState {
                section: None,
                scope: Scope::All,
                filter: RepoFilter::All,
                sort: RepoSortOrder::StatusPriority,
                search: String::new(),
                show_activity_log: false,
                pending_delete: Vec::new(),
                commit_messages: HashMap::new(),
                remote_urls: HashMap::new(),
                readme_expanded: true,
                readme_cache: HashMap::new(),
                md_cache: CommonMarkCache::default(),
                logo,
            },
            updater,
            fork: None,
            hub,
            applied_theme: None,
            closing: false,
        }
    }

    fn apply_hub(&mut self, ctx: &egui::Context, action: HubAction) {
        let hub = &mut self.hub;
        match action {
            HubAction::Open(section) => {
                self.view.section = Some(section);
                hub.open(section);
            }
            HubAction::Refresh(section) => hub.refresh(section),
            HubAction::SetInvolvement(section, involvement) => hub.set_involvement(section, involvement),
            HubAction::SetExplore(query) => hub.set_explore(query),
            HubAction::ShowLive => hub.show_live(),
            HubAction::SetLiveFilter(platform) => hub.live_filter = platform,
            HubAction::SetLayout(layout) => self.store.settings.repo_layout = layout,
            HubAction::Select(section, key) => {
                hub.selected.insert(section, key);
            }
            HubAction::SelectMention(id) => hub.live_selected = Some(id),
            HubAction::LoadReadme(slug) => hub.ensure_readme(&slug),
            HubAction::Star(slug, star) => hub.set_star(&slug, star),
            HubAction::OpenClone(slug) => {
                // Default to the folder in view, else the first watched folder, else the home folder.
                let parent = match &self.view.scope {
                    Scope::Folder(folder) => Some(folder.clone()),
                    _ => self.store.settings.scan_folders.first().cloned(),
                }
                .or_else(|| dirs::home_dir().map(|h| h.to_string_lossy().into_owned()))
                .unwrap_or_default();
                hub.open_clone(&slug, parent);
            }
            HubAction::CloneBrowse => {
                if let Some(dialog) = hub.clone.as_mut() {
                    let mut picker = rfd::FileDialog::new().set_title("Choose Where to Clone");
                    if std::path::Path::new(&dialog.parent).is_dir() {
                        picker = picker.set_directory(&dialog.parent);
                    }
                    if let Some(folder) = picker.pick_folder() {
                        dialog.parent = folder.to_string_lossy().into_owned();
                    }
                }
            }
            HubAction::CloneConfirm => hub.start_clone(),
            HubAction::CloneRetry => {
                if let Some(dialog) = hub.clone.as_mut() {
                    dialog.stage = crate::hub::CloneStage::Choosing;
                }
            }
            HubAction::CloseClone => hub.clone = None,
            HubAction::Checkout(url) => {
                let item = [&hub.pulls, &hub.reviews]
                    .iter()
                    .find_map(|feed| feed.items.iter().find(|i| i.url == url))
                    .cloned();
                let clones = hub_ui::local_clones(&self.store);
                if let Some(item) = item
                    && let Some(repo) = clones.get(&item.repo.to_lowercase()).and_then(|path| self.store.repo(path))
                {
                    hub.checkout_pr(&item, repo);
                }
            }
            HubAction::ShowRepository(path) => {
                hub.clone = None;
                self.view.section = None;
                self.view.scope = Scope::All;
                self.view.filter = RepoFilter::All;
                self.view.search.clear();
                self.store.focused = Some(path);
            }
            HubAction::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            HubAction::CopyText(text) => {
                self.store.status_banner = format!("Copied {text}");
                ctx.copy_text(text);
            }
            HubAction::Connect => {
                hub.connection = crate::hub::Connection::Unknown;
                if let Some(section) = self.view.section {
                    hub.open(section);
                } else {
                    hub.connect();
                }
            }
            HubAction::SignIn => {
                if let Err(err) = github::open_sign_in_terminal() {
                    self.store.status_banner = err;
                }
            }
        }
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let choice = self.store.settings.theme;
        if self.applied_theme == Some(choice) {
            return;
        }
        ctx.set_theme(match choice {
            ThemeChoice::System => ThemePreference::System,
            ThemeChoice::Light => ThemePreference::Light,
            ThemeChoice::Dark => ThemePreference::Dark,
        });
        self.applied_theme = Some(choice);
    }

    fn apply(&mut self, ctx: &egui::Context, actions: Vec<Action>) {
        for action in actions {
            let store = &mut self.store;
            let view = &mut self.view;
            match action {
                Action::Focus(path) => store.focused = Some(path),
                Action::ToggleSelect(path) => store.toggle_selection(&path),
                Action::Check(path) => store.check_repos(&[path]),
                Action::Pull(path) => store.pull_repos(&[path]),
                Action::Push(path) => store.push_repos(&[path]),
                Action::RequestDelete(paths) => view.pending_delete = paths,
                Action::ConfirmDelete { to_recycle_bin } => {
                    let paths = std::mem::take(&mut view.pending_delete);
                    store.delete_repositories(&paths, to_recycle_bin);
                }
                Action::CancelDelete => view.pending_delete.clear(),
                Action::OpenExplorer(path) => platform::open_in_explorer(&path),
                Action::OpenTerminal(path) => {
                    if let Err(err) = platform::open_in_terminal(&path) {
                        store.status_banner = format!("Could not open a terminal: {err}");
                    }
                }
                Action::CopyPath(path) => {
                    store.status_banner = format!("Copied {path}");
                    ctx.copy_text(path);
                }
                Action::CommitAndPush(path) => {
                    if let Some(message) = view.commit_messages.remove(&path) {
                        store.commit_and_push(&path, &message);
                    }
                }
                Action::AddRemote(path) => {
                    if let Some(url) = view.remote_urls.remove(&path) {
                        store.add_remote_origin(&path, &url);
                    }
                }
                Action::SetFilter(filter) => {
                    view.section = None;
                    view.filter = filter;
                }
                Action::SetScope(scope) => {
                    view.section = None;
                    view.scope = scope;
                }
                Action::ResetFilters => {
                    view.section = None;
                    view.scope = Scope::All;
                    view.filter = RepoFilter::All;
                    view.search.clear();
                }
                Action::FocusSearch => ctx.memory_mut(|m| m.request_focus(Id::new(SEARCH_ID))),
                Action::AddFolder => store.prompt_to_add_folder(),
                Action::RemoveFolder(folder) => {
                    if view.scope == Scope::Folder(folder.clone()) {
                        view.scope = Scope::All;
                    }
                    store.remove_scan_folder(&folder);
                }
                Action::Rescan { check_remote } => store.scan_directories(check_remote),
                Action::SetScanNested(nested) => {
                    store.settings.scan_nested_repos = nested;
                    store.scan_directories(false);
                }
                Action::SetPullStrategy(strategy) => store.settings.pull_strategy = strategy,
                Action::SetAutoStash(value) => store.settings.auto_stash_on_pull = value,
                Action::SetTheme(choice) => store.settings.theme = choice,
                Action::CheckAll => store.check_scope(&view.scope),
                Action::CheckSelected => store.check_repos(&store.selected_paths()),
                Action::PullSelected => store.pull_repos(&store.selected_paths()),
                Action::PullAllOutdated => store.pull_all_outdated(&view.scope),
                Action::PullOutdatedOrAll => {
                    if store.counts_in(&view.scope).outdated > 0 {
                        store.pull_all_outdated(&view.scope);
                    } else {
                        store.pull_all(&view.scope);
                    }
                }
                Action::PushSelected => store.push_repos(&store.selected_paths()),
                Action::PushAllUnpushed => store.push_all_unpushed(&view.scope),
                Action::SelectAllVisible => {
                    let visible: Vec<String> = store
                        .filtered_indices(&view.scope, view.filter, view.sort, &view.search)
                        .into_iter()
                        .map(|i| store.repos[i].path.clone())
                        .collect();
                    store.select_paths(visible);
                }
                Action::SelectOutdated => store.select_all_outdated(&view.scope),
                Action::SelectUnpushed => store.select_all_unpushed(&view.scope),
                Action::ClearSelection => store.clear_selection(),
                Action::ShowActivityLog(show) => view.show_activity_log = show,
                Action::ClearActivityLog => store.clear_activity_log(),
                Action::ToggleInspector => store.settings.show_inspector = !store.settings.show_inspector,
                Action::ToggleReadme => view.readme_expanded = !view.readme_expanded,
                Action::CheckForUpdates => self.updater.check(true),
                Action::ShowUpdateDialog => self.updater.show_dialog = true,
                Action::DownloadUpdate => {
                    if !store.busy() {
                        self.updater.start_download();
                    }
                }
                Action::RetryUpdate => self.updater.retry(),
                Action::CancelUpdate => self.updater.cancel_download(),
                Action::DismissUpdate => self.updater.dismiss(),
                Action::SetAutoUpdateCheck(value) => store.settings.check_updates_automatically = value,
                Action::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                Action::OpenFork(path) => {
                    if let Some(repo) = store.repo(&path) {
                        self.fork = Some(ForkFlow::start(repo, ctx.clone()));
                    }
                }
                Action::ForkSetTarget(target) => {
                    if let Some(flow) = self.fork.as_mut()
                        && let Some(repo) = store.repo(&flow.repo_path)
                    {
                        flow.set_target(target, repo);
                    }
                }
                Action::ForkBrowse => {
                    if let Some(flow) = self.fork.as_mut() {
                        let current = std::path::PathBuf::from(flow.local_path.trim());
                        let name = current.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| "fork".into());
                        let mut dialog = rfd::FileDialog::new().set_title("Choose Where to Put the Copy");
                        if let Some(parent) = current.parent().filter(|p| p.is_dir()) {
                            dialog = dialog.set_directory(parent);
                        }
                        if let Some(folder) = dialog.pick_folder() {
                            flow.local_path = folder.join(name).to_string_lossy().into_owned();
                        }
                    }
                }
                Action::ForkConfirm => {
                    if let Some(flow) = self.fork.as_mut()
                        && let Some(repo) = store.repo(&flow.repo_path)
                    {
                        match flow.target {
                            ForkTarget::GitHub => flow.fork_on_github(repo),
                            ForkTarget::Local => flow.fork_locally(repo),
                        }
                    }
                }
                Action::ForkRetry => {
                    if let Some(flow) = self.fork.as_mut()
                        && let Some(repo) = store.repo(&flow.repo_path)
                    {
                        match flow.target {
                            ForkTarget::GitHub => flow.check(repo),
                            ForkTarget::Local => flow.stage = ForkStage::Choosing,
                        }
                    }
                }
                Action::ForkRecheck => {
                    if let Some(flow) = self.fork.as_mut()
                        && let Some(repo) = store.repo(&flow.repo_path)
                    {
                        flow.check(repo);
                    }
                }
                Action::ForkSignIn => {
                    if let Err(err) = github::open_sign_in_terminal() {
                        store.status_banner = err;
                    }
                }
                Action::ForkClose => self.fork = None,
                Action::Hub(action) => self.apply_hub(ctx, action),
            }
        }
    }
}

impl eframe::App for GitDaddyApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.store.process_messages();
        if self.updater.poll() {
            self.store.settings.last_update_check = Some(unix_now());
        }
        for event in self.hub.poll() {
            match event {
                HubEvent::Cloned { path, log } => self.store.adopt_repository(&path, log),
                HubEvent::CheckedOut { path, log } => self.store.log_and_refresh(&path, log),
                HubEvent::Notice(message) => self.store.status_banner = message,
            }
        }
        let repos = &self.store.repos;
        self.hub.tick(self.view.section, || repos.iter().filter_map(|r| r.github_slug()).collect());
        match self.fork.as_mut().and_then(ForkFlow::poll) {
            Some(ForkEvent::Rewired(outcome)) => {
                let github::ForkOutcome { repo, log, .. } = *outcome;
                self.store.apply_repo_update(repo, log);
            }
            Some(ForkEvent::Copied { path, log }) => self.store.adopt_repository(&path, log),
            Some(ForkEvent::Failed { log }) => self.store.record_log(log),
            None => {}
        }
        let ctx = ui.ctx().clone();
        self.apply_theme(&ctx);
        if self.updater.quit_requested && !self.closing {
            // The installer is running; get out of its way so it can replace GitDaddy.exe.
            self.closing = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        let mut actions = Vec::new();
        collect_shortcuts(&ctx, &self.store, &mut actions);

        let p = theme::palette(&ctx);
        let counts = self.store.counts_in(&self.view.scope);
        let visible =
            self.store.filtered_indices(&self.view.scope, self.view.filter, self.view.sort, &self.view.search);
        let store = &self.store;
        let view = &mut self.view;
        let updater = &self.updater;
        let hub = &mut self.hub;
        let mut hub_actions: Vec<HubAction> = Vec::new();

        Panel::top("header")
            .exact_size(44.0)
            .frame(Frame::NONE.fill(p.surface1).inner_margin(Margin::symmetric(16, 0)))
            .show(ui, |ui| header(ui, store, view, updater, &counts, &mut actions));

        Panel::bottom("status-bar")
            .exact_size(ROW_HEIGHT)
            .frame(Frame::NONE.fill(p.surface1).inner_margin(Margin::symmetric(12, 0)))
            .show(ui, |ui| status_bar(ui, store, &counts, &mut actions));

        Panel::left("sidebar")
            .resizable(false)
            .exact_size(SIDEBAR_WIDTH)
            .frame(Frame::NONE.fill(p.surface1).inner_margin(Margin::same(12)))
            .show(ui, |ui| sidebar(ui, store, view, hub, &mut actions));

        if store.settings.show_inspector {
            // Renamed from "inspector" so widths saved while README tables could stretch the panel are dropped.
            Panel::right("inspector-panel")
                .resizable(true)
                .default_size(INSPECTOR_WIDTH)
                .size_range(300.0..=640.0)
                .frame(Frame::NONE.fill(p.surface1))
                .show(ui, |ui| match (view.section, store.focused_repo()) {
                    (Some(section), _) => {
                        hub_ui::inspector(ui, section, hub, store, &mut view.md_cache, &mut hub_actions)
                    }
                    (None, Some(repo)) => inspector(ui, store, view, repo, &mut actions),
                    (None, None) => empty_inspector(ui),
                });
        }

        egui::CentralPanel::default().frame(Frame::NONE.fill(p.canvas)).show(ui, |ui| match view.section {
            Some(section) => hub_ui::center(ui, section, hub, store, store.settings.repo_layout, &mut hub_actions),
            None => center(ui, store, view, &counts, &visible, &mut actions),
        });

        delete_dialog(&ctx, store, view, &mut actions);
        if view.show_activity_log {
            activity_log_dialog(&ctx, store, &mut actions);
        }
        update_dialog(&ctx, updater, store, view, &mut actions);
        if let Some(flow) = self.fork.as_mut() {
            fork_dialog(&ctx, flow, store, &mut actions);
        }
        hub_ui::clone_dialog(&ctx, hub, store, &mut hub_actions);
        actions.extend(hub_actions.into_iter().map(Action::Hub));

        self.apply(&ctx, actions);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_KEY, &self.store.settings);
    }

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(5)
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}

fn collect_shortcuts(ctx: &egui::Context, store: &Store, actions: &mut Vec<Action>) {
    let ctrl = Modifiers::COMMAND;
    let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
    // Shift variants first: a plain Ctrl+key shortcut also matches when Shift is held.
    let bindings: [(Modifiers, Key, Action); 10] = [
        (ctrl_shift, Key::R, Action::Rescan { check_remote: false }),
        (ctrl_shift, Key::P, Action::PullAllOutdated),
        (ctrl_shift, Key::U, Action::PushAllUnpushed),
        (ctrl_shift, Key::A, Action::SelectAllVisible),
        (ctrl, Key::O, Action::AddFolder),
        (ctrl, Key::R, Action::CheckAll),
        (ctrl, Key::P, Action::PullSelected),
        (ctrl, Key::U, Action::PushSelected),
        (ctrl, Key::F, Action::FocusSearch),
        (Modifiers::NONE, Key::F5, Action::CheckAll),
    ];
    let busy = store.busy();
    for (modifiers, key, action) in bindings {
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key))) {
            let blocked =
                busy && matches!(action, Action::CheckAll | Action::PullAllOutdated | Action::PushAllUnpushed);
            if !blocked {
                actions.push(action);
            }
        }
    }
}

// MARK: - Header

fn header(ui: &mut Ui, store: &Store, view: &ViewState, updater: &Updater, counts: &Counts, actions: &mut Vec<Action>) {
    let p = theme::palette(ui.ctx());
    // Measured: full tab labels plus the right-hand buttons need about 1220pt, the update chip about 140pt more.
    let chip_width = if updater.available_release().is_some() { 140.0 } else { 0.0 };
    let compact_tabs = ui.available_width() < 1220.0 + chip_width;
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE2;
        if let Some(logo) = &view.logo {
            ui.add(egui::Image::new(SizedTexture::new(logo.id(), vec2(22.0, 22.0))));
        }
        ui.label(RichText::new("GitDaddy").font(theme::title()).color(p.text_primary));
        vertical_divider(ui, p);

        ui.spacing_mut().item_spacing.x = SPACE1;
        match view.section {
            None => {
                for filter in RepoFilter::ALL {
                    let shown_label = (!compact_tabs).then_some(filter.tab_label());
                    let count = counts.for_filter(filter);
                    let response =
                        widgets::filter_tab(ui, shown_label, filter.status_preview(), count, view.filter == filter);
                    let response = if compact_tabs { response.on_hover_text(filter.label()) } else { response };
                    if response.clicked() {
                        actions.push(Action::SetFilter(filter));
                    }
                }
            }
            // The status tabs filter local repositories, so they only show with the repository list.
            Some(section) => {
                ui.add_space(SPACE1);
                ui.label(RichText::new(section.glyph()).font(theme::icon_font(15.0)).color(p.text_secondary));
                ui.label(RichText::new(section.label()).font(theme::body_medium()).color(p.text_primary));
            }
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = SPACE2;
            let menu = widgets::icon_button(ui, icon::LIST, "Menu", true);
            Popup::menu(&menu).show(|ui| app_menu(ui, store, counts, actions));

            let inspector_tip = if store.settings.show_inspector { "Hide Inspector" } else { "Show Inspector" };
            if widgets::icon_button(ui, icon::SIDEBAR_SIMPLE, inspector_tip, true).clicked() {
                actions.push(Action::ToggleInspector);
            }
            if widgets::icon_button(ui, icon::LIST_BULLETS, "Activity Log", true).clicked() {
                actions.push(Action::ShowActivityLog(true));
            }
            match updater.available_release() {
                Some(release) => {
                    let label = format!("New version {}", release.version);
                    if widgets::button(ui, &label, Some(icon::DOWNLOAD_SIMPLE), Variant::Secondary, true, true)
                        .on_hover_text("A new version of GitDaddy is available")
                        .clicked()
                    {
                        actions.push(Action::ShowUpdateDialog);
                    }
                }
                None => {
                    if widgets::icon_button(ui, icon::CLOUD_ARROW_DOWN, "Check for Updates", true).clicked() {
                        actions.push(Action::CheckForUpdates);
                    }
                }
            }

            let busy = store.busy();
            let pull_label =
                if counts.outdated > 0 { format!("Update Outdated ({})", counts.outdated) } else { "Pull All".into() };
            let pull_enabled = !busy && !store.repos.is_empty();
            if widgets::button(ui, &pull_label, Some(icon::ARROW_DOWN), Variant::Primary, true, pull_enabled).clicked()
            {
                actions.push(Action::PullOutdatedOrAll);
            }
            if counts.unpushed > 0
                && widgets::button(
                    ui,
                    &format!("Push Unpushed ({})", counts.unpushed),
                    Some(icon::ARROW_UP),
                    Variant::Secondary,
                    true,
                    !busy,
                )
                .clicked()
            {
                actions.push(Action::PushAllUnpushed);
            }
            let check_enabled = !busy && !store.repos.is_empty();
            if widgets::button(ui, "Check All", Some(icon::ARROWS_CLOCKWISE), Variant::Secondary, true, check_enabled)
                .on_hover_text(format!("Fetch every repository ({}R / F5)", platform::MOD_KEY))
                .clicked()
            {
                actions.push(Action::CheckAll);
            }
        });
    });
}

fn vertical_divider(ui: &mut Ui, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, 16.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p.border_strong);
}

fn menu_item(ui: &mut Ui, label: &str, shortcut: Option<&str>, enabled: bool) -> bool {
    let mut button = egui::Button::new(label);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(shortcut);
    }
    let clicked = ui.add_enabled(enabled, button).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

fn app_menu(ui: &mut Ui, store: &Store, counts: &Counts, actions: &mut Vec<Action>) {
    ui.set_min_width(250.0);
    let busy = store.busy();
    let has_selection = !store.selected.is_empty();
    let m = platform::MOD_KEY;
    let ms = platform::MOD_SHIFT_KEY;
    if menu_item(ui, "Add Folder to Scan…", Some(&format!("{m}O")), true) {
        actions.push(Action::AddFolder);
    }
    if menu_item(ui, "Rescan Watched Folders", Some(&format!("{ms}R")), !store.is_scanning) {
        actions.push(Action::Rescan { check_remote: false });
    }
    ui.separator();
    if menu_item(ui, "Check All Remotes (git fetch)", Some(&format!("{m}R")), !busy && !store.repos.is_empty()) {
        actions.push(Action::CheckAll);
    }
    if menu_item(ui, "Pull Selected Repositories", Some(&format!("{m}P")), has_selection) {
        actions.push(Action::PullSelected);
    }
    if menu_item(ui, "Update All Outdated Repositories", Some(&format!("{ms}P")), !busy && counts.outdated > 0) {
        actions.push(Action::PullAllOutdated);
    }
    ui.separator();
    if menu_item(ui, "Push Selected Repositories", Some(&format!("{m}U")), has_selection) {
        actions.push(Action::PushSelected);
    }
    if menu_item(ui, "Push All Unpushed Repositories", Some(&format!("{ms}U")), !busy && counts.unpushed > 0) {
        actions.push(Action::PushAllUnpushed);
    }
    ui.separator();
    if menu_item(ui, "Select All Visible", Some(&format!("{ms}A")), true) {
        actions.push(Action::SelectAllVisible);
    }
    if menu_item(ui, "Select Outdated Only", None, counts.outdated > 0) {
        actions.push(Action::SelectOutdated);
    }
    if menu_item(ui, "Select Unpushed Only", None, counts.unpushed > 0) {
        actions.push(Action::SelectUnpushed);
    }
    if menu_item(ui, "Clear Selection", None, has_selection) {
        actions.push(Action::ClearSelection);
    }
    ui.separator();
    ui.menu_button("Appearance", |ui| {
        for (choice, label) in
            [(ThemeChoice::System, "Use System Setting"), (ThemeChoice::Light, "Light"), (ThemeChoice::Dark, "Dark")]
        {
            if ui.add(egui::Button::selectable(store.settings.theme == choice, label)).clicked() {
                actions.push(Action::SetTheme(choice));
                ui.close();
            }
        }
    });
    if menu_item(ui, "Activity Log", None, true) {
        actions.push(Action::ShowActivityLog(true));
    }
    if menu_item(ui, "Check for Updates…", None, true) {
        actions.push(Action::CheckForUpdates);
    }
    ui.separator();
    if menu_item(ui, "GitDaddy on GitHub", None, true) {
        ui.ctx().open_url(egui::OpenUrl::new_tab("https://github.com/Duyfrom/GitDaddy"));
    }
    ui.add_enabled(false, egui::Button::new(format!("Version {}", env!("CARGO_PKG_VERSION"))));
}

// MARK: - Sidebar

/// The sidebar picks *where* to look (a folder or a remote owner); the header tabs pick
/// *which state* to show within it.
fn sidebar(ui: &mut Ui, store: &Store, view: &ViewState, hub: &Hub, actions: &mut Vec<Action>) {
    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = SPACE1;
        github_section(ui, view, hub, actions);
        ui.add_space(SPACE6 - SPACE1);

        // Clicking the selected entry again goes back to everything.
        let in_repos = view.section.is_none();
        let pick = |scope: Scope| if in_repos && view.scope == scope { Scope::All } else { scope };

        ui.horizontal(|ui| {
            widgets::section_header(ui, "Folders");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::icon_button(ui, icon::ARROW_CLOCKWISE, "Rescan Watched Folders", !store.is_scanning)
                    .clicked()
                {
                    actions.push(Action::Rescan { check_remote: true });
                }
            });
        });
        let (total, total_behind) = store.scope_summary(&Scope::All);
        let all = widgets::scope_row(
            ui,
            widgets::ScopeRow {
                glyph: icon::STACK,
                title: "All Repositories",
                detail: None,
                count: total,
                behind: total_behind,
                selected: in_repos && view.scope == Scope::All,
                remove_tooltip: None,
            },
        );
        if all.response.clicked() {
            actions.push(Action::SetScope(Scope::All));
        }
        for folder in &store.settings.scan_folders {
            let scope = Scope::Folder(folder.clone());
            let (count, behind) = store.scope_summary(&scope);
            let selected = in_repos && view.scope == scope;
            let name = util::folder_name(folder);
            let detail = util::abbreviate_home(folder);
            let row = widgets::scope_row(
                ui,
                widgets::ScopeRow {
                    glyph: if selected { icon::FOLDER_OPEN } else { icon::FOLDER },
                    title: &name,
                    detail: Some(&detail),
                    count,
                    behind,
                    selected,
                    remove_tooltip: Some("Stop watching this folder"),
                },
            );
            if row.remove_clicked {
                actions.push(Action::RemoveFolder(folder.clone()));
            } else if row.response.on_hover_text(folder).clicked() {
                actions.push(Action::SetScope(pick(scope)));
            }
        }
        ui.add_space(SPACE1);
        if widgets::button(ui, "Add Folder…", Some(icon::PLUS), Variant::Secondary, true, true)
            .on_hover_text(format!("Add a workspace folder or a single repository ({}O)", platform::MOD_KEY))
            .clicked()
        {
            actions.push(Action::AddFolder);
        }
        ui.add_space(SPACE1);
        let mut nested = store.settings.scan_nested_repos;
        if widgets::checkbox(ui, &mut nested, "Scan nested repos") {
            actions.push(Action::SetScanNested(nested));
        }

        let owners = store.owners();
        let (no_remote, no_remote_behind) = store.scope_summary(&Scope::NoRemote);
        if !owners.is_empty() || no_remote > 0 {
            ui.add_space(SPACE6 - SPACE1);
            widgets::section_header(ui, "Owners");
            for (owner, count, behind) in &owners {
                let scope = Scope::Owner(owner.key());
                let selected = in_repos && view.scope == scope;
                let glyph = match owner.host.as_str() {
                    "github.com" => icon::GITHUB_LOGO,
                    host if host.contains("gitlab") => icon::GITLAB_LOGO_SIMPLE,
                    _ => icon::GLOBE_SIMPLE,
                };
                // github.com is the common case, so only other hosts are spelled out.
                let detail = (owner.host != "github.com").then_some(owner.host.as_str());
                let row = widgets::scope_row(
                    ui,
                    widgets::ScopeRow {
                        glyph,
                        title: &owner.name,
                        detail,
                        count: *count,
                        behind: *behind,
                        selected,
                        remove_tooltip: None,
                    },
                );
                if row.response.on_hover_text(owner.key()).clicked() {
                    actions.push(Action::SetScope(pick(scope)));
                }
            }
            if no_remote > 0 {
                let row = widgets::scope_row(
                    ui,
                    widgets::ScopeRow {
                        glyph: icon::HARD_DRIVES,
                        title: "No remote",
                        detail: None,
                        count: no_remote,
                        behind: no_remote_behind,
                        selected: in_repos && view.scope == Scope::NoRemote,
                        remove_tooltip: None,
                    },
                );
                if row.response.on_hover_text("Local-only repositories with no remote configured").clicked() {
                    actions.push(Action::SetScope(pick(Scope::NoRemote)));
                }
            }
        }

        ui.add_space(SPACE6 - SPACE1);
        widgets::section_header(ui, "Pull Mode");
        let current = store.settings.pull_strategy;
        let width = ui.available_width().min(190.0);
        widgets::combo_box(ui, "pull-mode", current.label(), width, |ui| {
            for strategy in PullStrategy::ALL {
                if ui.add(egui::Button::selectable(current == strategy, strategy.label())).clicked() {
                    actions.push(Action::SetPullStrategy(strategy));
                }
            }
        });
        ui.add_space(SPACE1);
        let mut auto_stash = store.settings.auto_stash_on_pull;
        if widgets::checkbox(ui, &mut auto_stash, "Auto-stash local edits") {
            actions.push(Action::SetAutoStash(auto_stash));
        }

        if git::git_executable().is_none() {
            ui.add_space(SPACE6);
            let p = theme::palette(ui.ctx());
            ui.label(
                RichText::new(format!(
                    "{} Git was not found. Install Git for Windows and restart GitDaddy.",
                    icon::WARNING
                ))
                .font(theme::meta())
                .color(p.bad),
            );
            ui.hyperlink_to(
                RichText::new("Download Git for Windows").font(theme::meta()),
                "https://git-scm.com/download/win",
            );
        }
    });
}

/// The sidebar's GitHub entries. They swap the center list for a GitHub view.
fn github_section(ui: &mut Ui, view: &ViewState, hub: &Hub, actions: &mut Vec<Action>) {
    let p = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        widgets::section_header(ui, "GitHub");
        if let Some(login) = hub.login() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(SPACE2);
                ui.label(RichText::new(login).font(theme::meta()).color(p.text_tertiary))
                    .on_hover_text("Signed in with the GitHub CLI");
            });
        }
    });
    for section in Section::ALL {
        let count = hub.counts.map_or(0, |c| match section {
            Section::PullRequests => c.pulls,
            Section::Issues => c.issues,
            Section::ReviewRequests => c.reviews,
            Section::Starred => c.starred,
            Section::Explore => 0,
        }) as usize;
        let row = widgets::scope_row(
            ui,
            widgets::ScopeRow {
                glyph: section.glyph(),
                title: section.label(),
                detail: None,
                count,
                // A dot when someone is waiting on your review.
                behind: if section == Section::ReviewRequests { count } else { 0 },
                selected: view.section == Some(section),
                remove_tooltip: None,
            },
        );
        if row.response.clicked() {
            actions.push(Action::Hub(HubAction::Open(section)));
        }
    }
}

// MARK: - Center table

struct Columns {
    check: Rect,
    status: Rect,
    name: Rect,
    branch: Rect,
    summary: Rect,
    delta: Rect,
    actions: Rect,
}

fn columns(row: Rect) -> Columns {
    let inner = row.shrink2(vec2(SPACE3, 0.0));
    let w = inner.width();
    let name_w = (w * 0.17).clamp(136.0, 240.0);
    let branch_w = (w * 0.13).clamp(108.0, 180.0);
    let fixed = [20.0, 24.0, name_w, branch_w, 72.0, 118.0];
    let gaps = SPACE2 * 6.0;
    let summary_w = (w - fixed.iter().sum::<f32>() - gaps).max(60.0);
    let widths = [20.0, 24.0, name_w, branch_w, summary_w, 72.0, 118.0];
    let mut x = inner.left();
    let mut rects = widths.map(|width| {
        let r = Rect::from_min_max(pos2(x, row.top()), pos2(x + width, row.bottom()));
        x += width + SPACE2;
        r
    });
    // Keep the actions pinned to the right edge even when the summary hits its minimum.
    let overflow = (x - SPACE2) - inner.right();
    if overflow > 0.0 {
        for r in rects.iter_mut().skip(4) {
            *r = r.translate(vec2(-overflow, 0.0));
        }
    }
    let [check, status, name, branch, summary, delta, actions] = rects;
    Columns { check, status, name, branch, summary, delta, actions }
}

fn center(
    ui: &mut Ui,
    store: &Store,
    view: &mut ViewState,
    counts: &Counts,
    visible: &[usize],
    actions: &mut Vec<Action>,
) {
    let p = theme::palette(ui.ctx());
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

    filter_bar(ui, store, view, counts, visible.len(), actions);
    widgets::hline(ui, p.border);
    column_headers(ui, p);
    widgets::hline(ui, p.border);

    if store.is_scanning && store.repos.is_empty() {
        centered(ui, |ui| {
            widgets::paint_spinner(ui, ui.cursor().center_top() + vec2(0.0, 10.0), ICON_SIZE);
            ui.add_space(26.0);
            ui.label(
                RichText::new("Scanning directories for Git repositories…")
                    .font(theme::body_medium())
                    .color(p.text_primary),
            );
            let folders: Vec<String> = store.settings.scan_folders.iter().map(|f| util::abbreviate_home(f)).collect();
            ui.label(RichText::new(folders.join(", ")).font(theme::meta_mono()).color(p.text_tertiary));
        });
        return;
    }
    if visible.is_empty() {
        empty_repos(ui, store, view.logo.as_ref(), actions);
        return;
    }

    let row_stride = ROW_HEIGHT + 1.0;
    ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_stride, visible.len(), |ui, range| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        for &index in &visible[range] {
            let repo = &store.repos[index];
            let selected = store.selected.contains(&repo.path);
            let focused = store.focused.as_deref() == Some(repo.path.as_str());
            repo_row(ui, repo, selected, focused, actions);
            widgets::hline(ui, p.border);
        }
    });
}

fn filter_bar(
    ui: &mut Ui,
    store: &Store,
    view: &mut ViewState,
    counts: &Counts,
    visible_count: usize,
    actions: &mut Vec<Action>,
) {
    let p = theme::palette(ui.ctx());
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    ui.painter().rect_filled(bar, 0.0, p.surface1);
    let inner = bar.shrink2(vec2(SPACE3, 0.0));
    ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = SPACE2;
        search_field(ui, &mut view.search);
        let sort = &mut view.sort;
        widgets::combo_box(ui, "sort-order", sort.label(), 132.0, |ui| {
            for order in RepoSortOrder::ALL {
                if ui.add(egui::Button::selectable(*sort == order, order.label())).clicked() {
                    *sort = order;
                }
            }
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = SPACE1;
            if !store.selected.is_empty() && widgets::button(ui, "Clear", None, Variant::Ghost, true, true).clicked() {
                actions.push(Action::ClearSelection);
            }
            if counts.unpushed > 0
                && widgets::button(ui, &format!("Unpushed ({})", counts.unpushed), None, Variant::Ghost, true, true)
                    .on_hover_text("Select repositories with commits to push")
                    .clicked()
            {
                actions.push(Action::SelectUnpushed);
            }
            if counts.outdated > 0
                && widgets::button(ui, &format!("Outdated ({})", counts.outdated), None, Variant::Ghost, true, true)
                    .on_hover_text("Select repositories with updates to pull")
                    .clicked()
            {
                actions.push(Action::SelectOutdated);
            }
            if widgets::button(
                ui,
                &format!("Select All ({visible_count})"),
                None,
                Variant::Ghost,
                true,
                visible_count > 0,
            )
            .on_hover_text(format!("Select every visible repository ({}A)", platform::MOD_SHIFT_KEY))
            .clicked()
            {
                actions.push(Action::SelectAllVisible);
            }
        });
    });
}

fn search_field(ui: &mut Ui, search: &mut String) {
    let p = theme::palette(ui.ctx());
    let width = ui.available_width().clamp(160.0, 240.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW_COMPACT), Sense::hover());
    let id = Id::new(SEARCH_ID);
    let focused = ui.memory(|m| m.has_focus(id));
    let radius = CornerRadius::same(RADIUS_CONTROL);
    ui.painter().rect_filled(rect, radius, p.canvas);
    let stroke = if focused { Stroke::new(1.0, p.accent) } else { Stroke::new(1.0, p.border_strong) };
    ui.painter().rect_stroke(rect, radius, stroke, StrokeKind::Inside);
    ui.painter().text(
        pos2(rect.left() + SPACE3 + 6.0, rect.center().y),
        Align2::CENTER_CENTER,
        icon::MAGNIFYING_GLASS,
        theme::icon_font(ICON_COMPACT - 1.0),
        p.text_tertiary,
    );
    let clear_w = if search.is_empty() { 0.0 } else { 22.0 };
    let edit_rect = Rect::from_min_max(
        pos2(rect.left() + SPACE3 + 18.0, rect.top()),
        pos2(rect.right() - SPACE2 - clear_w, rect.bottom()),
    );
    ui.scope_builder(UiBuilder::new().max_rect(edit_rect).layout(Layout::left_to_right(Align::Center)), |ui| {
        ui.add(
            TextEdit::singleline(search)
                .id(id)
                .hint_text(RichText::new("Filter repositories, branches, paths…").color(p.text_tertiary))
                .font(theme::body())
                .text_color(p.text_primary)
                .frame(Frame::NONE)
                .margin(Margin::ZERO)
                .desired_width(edit_rect.width()),
        );
    });
    if !search.is_empty() {
        let x_rect = Rect::from_center_size(pos2(rect.right() - SPACE2 - 9.0, rect.center().y), vec2(20.0, 20.0));
        if widgets::icon_button_at(ui, x_rect, id.with("clear"), icon::X, "Clear search", true).clicked() {
            search.clear();
        }
    }
}

fn column_headers(ui: &mut Ui, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_COMPACT), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p.surface1);
    let c = columns(rect);
    let font = theme::meta_medium();
    let color = p.text_tertiary;
    ui.painter().text(c.status.center(), Align2::CENTER_CENTER, "St", font.clone(), color);
    widgets::paint_text(ui, c.name, "Repository", font.clone(), color, false);
    widgets::paint_text(ui, c.branch, "Branch", font.clone(), color, false);
    widgets::paint_text(ui, c.summary, "Status / Latest Commit", font.clone(), color, false);
    widgets::paint_text(ui, c.delta, "Delta", font.clone(), color, true);
    widgets::paint_text(ui, c.actions, "Actions", font, color, true);
}

fn repo_row(ui: &mut Ui, repo: &GitRepo, selected: bool, focused: bool, actions: &mut Vec<Action>) {
    let p = theme::palette(ui.ctx());
    let id = Id::new(("repo-row", &repo.path));
    let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let hovered = ui.rect_contains_pointer(rect);
    let painter = ui.painter();
    let bg = if focused || selected || hovered { p.surface2 } else { p.surface1 };
    painter.rect_filled(rect, 0.0, bg);
    if focused || selected {
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 0.0, p.accent);
    }

    let c = columns(rect);
    let state = &repo.sync_state;

    // Selection checkbox.
    let check_rect = Rect::from_center_size(c.check.center(), vec2(20.0, ROW_COMPACT));
    let check = ui.interact(check_rect, id.with("select"), Sense::click());
    let (glyph, color) = if selected {
        (icon::CHECK_SQUARE, p.accent)
    } else if check.hovered() {
        (icon::SQUARE, p.text_primary)
    } else {
        (icon::SQUARE, p.text_secondary)
    };
    ui.painter().text(check_rect.center(), Align2::CENTER_CENTER, glyph, theme::icon_font(ICON_SIZE), color);
    if check.on_hover_text(if selected { "Deselect" } else { "Select" }).clicked() {
        actions.push(Action::ToggleSelect(repo.path.clone()));
    }

    // Status icon.
    widgets::paint_status_icon(ui, c.status.center(), state, ICON_SIZE);
    ui.interact(c.status, id.with("status"), Sense::hover()).on_hover_text(format!(
        "{} — {}",
        state.label(),
        state.inspector_explanation()
    ));

    // Name and branch.
    widgets::paint_text(ui, c.name, &repo.name, theme::body_medium(), p.text_primary, false);
    ui.painter().text(
        pos2(c.branch.left() + 6.0, c.branch.center().y),
        Align2::CENTER_CENTER,
        icon::GIT_BRANCH,
        theme::icon_font(12.0),
        p.text_tertiary,
    );
    let branch_rect = Rect::from_min_max(pos2(c.branch.left() + 16.0, c.branch.top()), c.branch.max);
    widgets::paint_text(ui, branch_rect, &repo.branch, theme::meta_mono(), p.text_secondary, false);

    // Summary with the relative commit date on the right.
    let mut summary_rect = c.summary;
    if !repo.last_commit_relative_date.is_empty() {
        let date_w = widgets::paint_text(
            ui,
            c.summary,
            &short_relative(&repo.last_commit_relative_date),
            theme::meta(),
            p.text_tertiary,
            true,
        );
        summary_rect.max.x -= date_w + SPACE2;
    }
    let emphasize = state.needs_update() || state.is_issue_or_no_upstream();
    let summary_color = if emphasize { p.text_primary } else { p.text_secondary };
    widgets::paint_text(ui, summary_rect, &repo.status_or_commit_summary(), theme::body(), summary_color, false);

    // Delta counts, painted right to left.
    let delta_color = if state.is_diverged() { p.bad } else { p.notice };
    let mut parts: Vec<(String, Color32)> = Vec::new();
    if state.behind_count() > 0 {
        parts.push((format!("{}{}", icon::ARROW_DOWN, state.behind_count()), delta_color));
    }
    if state.ahead_count() > 0 {
        parts.push((format!("{}{}", icon::ARROW_UP, state.ahead_count()), delta_color));
    } else if state.is_unpublished_branch() {
        parts.push(("new".into(), p.notice));
    }
    if repo.is_dirty() {
        parts.push((format!("{}M", repo.local_changes.len()), p.notice));
    }
    let mut x = c.delta.right();
    for (text, color) in parts.iter().rev() {
        let r =
            ui.painter().text(pos2(x, c.delta.center().y), Align2::RIGHT_CENTER, text, theme::meta_medium(), *color);
        x = r.left() - 6.0;
    }
    if repo.is_dirty() {
        ui.interact(c.delta, id.with("delta"), Sense::hover())
            .on_hover_text(format!("{} uncommitted file change(s)", repo.local_changes.len()));
    }

    // Row actions.
    let button = |i: usize| {
        let right = c.actions.right() - (3 - i) as f32 * (ROW_COMPACT + 2.0);
        Rect::from_min_size(
            pos2(right - ROW_COMPACT, rect.center().y - ROW_COMPACT / 2.0),
            vec2(ROW_COMPACT, ROW_COMPACT),
        )
    };
    let path = &repo.path;
    let check_tip = if repo.has_remote() { "Check Remote (git fetch)" } else { "No remote URL configured" };
    if widgets::icon_button_at(
        ui,
        button(0),
        id.with("check"),
        icon::ARROWS_CLOCKWISE,
        check_tip,
        !state.is_busy() && repo.has_remote(),
    )
    .clicked()
    {
        actions.push(Action::Check(path.clone()));
    }
    let pull_tip = if state.behind_count() > 0 {
        format!("Pull {} Update(s) (git pull)", state.behind_count())
    } else {
        "Pull from Remote (git pull)".into()
    };
    if widgets::icon_button_at(ui, button(1), id.with("pull"), icon::ARROW_DOWN, &pull_tip, state.can_pull()).clicked()
    {
        actions.push(Action::Pull(path.clone()));
    }
    let push_tip = if state.is_unpublished_branch() {
        "Publish Branch (git push -u)".to_string()
    } else if state.ahead_count() > 0 {
        format!("Push {} Commit(s)", state.ahead_count())
    } else {
        "Push to Remote".into()
    };
    let can_push = !state.is_busy() && repo.has_remote() && repo.has_commits();
    if widgets::icon_button_at(ui, button(2), id.with("push"), icon::ARROW_UP, &push_tip, can_push).clicked() {
        actions.push(Action::Push(path.clone()));
    }
    let more = widgets::icon_button_at(ui, button(3), id.with("more"), icon::DOTS_THREE, "More Actions", true);
    Popup::menu(&more).show(|ui| repo_menu(ui, repo, selected, false, actions));

    if row.clicked() {
        actions.push(Action::Focus(path.clone()));
    }
    if row.double_clicked() {
        actions.push(Action::OpenExplorer(path.clone()));
    }
    row.context_menu(|ui| repo_menu(ui, repo, selected, true, actions));
}

/// "3 days ago" → "3d ago" to keep the date column narrow, like the macOS app.
fn short_relative(relative: &str) -> String {
    let units = [
        (" seconds", "s"),
        (" second", "s"),
        (" minutes", "m"),
        (" minute", "m"),
        (" hours", "h"),
        (" hour", "h"),
        (" days", "d"),
        (" day", "d"),
        (" weeks", "w"),
        (" week", "w"),
        (" months", "mo"),
        (" month", "mo"),
        (" years", "y"),
        (" year", "y"),
    ];
    let mut out = relative.to_string();
    for (long, short) in units {
        if out.contains(long) {
            out = out.replacen(long, short, 1);
        }
    }
    out.replace(", ", " ")
}

fn repo_menu(ui: &mut Ui, repo: &GitRepo, selected: bool, include_selection: bool, actions: &mut Vec<Action>) {
    ui.set_min_width(220.0);
    let p = theme::palette(ui.ctx());
    let state = &repo.sync_state;
    let path = &repo.path;
    if menu_item(ui, "Check Remote (git fetch)", None, repo.has_remote() && !state.is_busy()) {
        actions.push(Action::Check(path.clone()));
    }
    if menu_item(ui, "Pull Updates (git pull)", None, state.can_pull()) {
        actions.push(Action::Pull(path.clone()));
    }
    let push_label =
        if state.is_unpublished_branch() { "Publish Branch (git push -u)" } else { "Push Commits (git push)" };
    if menu_item(ui, push_label, None, repo.has_remote() && repo.has_commits() && !state.is_busy()) {
        actions.push(Action::Push(path.clone()));
    }
    if menu_item(ui, "Fork…", None, repo.has_commits() && !state.is_busy()) {
        actions.push(Action::OpenFork(path.clone()));
    }
    ui.separator();
    if include_selection {
        if menu_item(ui, if selected { "Deselect Repository" } else { "Select Repository" }, None, true) {
            actions.push(Action::ToggleSelect(path.clone()));
        }
        ui.separator();
    }
    if menu_item(ui, &format!("Open in {}", platform::FILE_MANAGER_NAME), None, true) {
        actions.push(Action::OpenExplorer(path.clone()));
    }
    if menu_item(ui, "Open in Terminal", None, true) {
        actions.push(Action::OpenTerminal(path.clone()));
    }
    if menu_item(ui, "Copy Repository Path", None, true) {
        actions.push(Action::CopyPath(path.clone()));
    }
    ui.separator();
    let delete = egui::Button::new(RichText::new("Delete Local Repository…").color(p.bad));
    if ui.add_enabled(!state.is_busy(), delete).clicked() {
        actions.push(Action::RequestDelete(vec![path.clone()]));
        ui.close();
    }
}

fn centered(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    let rect = ui.available_rect_before_wrap();
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.y = SPACE2;
        ui.add_space((rect.height() / 2.0 - 110.0).max(SPACE6));
        content(ui);
    });
}

fn empty_repos(ui: &mut Ui, store: &Store, logo: Option<&TextureHandle>, actions: &mut Vec<Action>) {
    let p = theme::palette(ui.ctx());
    centered(ui, |ui| {
        if store.settings.scan_folders.is_empty() {
            if let Some(logo) = logo {
                ui.add(egui::Image::new(SizedTexture::new(logo.id(), vec2(64.0, 64.0))));
                ui.add_space(SPACE1);
            }
            ui.label(RichText::new("Welcome to GitDaddy").font(theme::title()).color(p.text_primary));
            ui.add(
                egui::Label::new(
                    RichText::new("Select the workspace folder where you keep your Git projects to scan and monitor all repositories in one place.")
                        .font(theme::body())
                        .color(p.text_secondary),
                )
                .halign(Align::Center)
                .wrap(),
            );
            ui.add_space(SPACE1);
            if widgets::button(ui, "Choose Workspace Folder…", Some(icon::FOLDER_PLUS), Variant::Primary, false, true)
                .clicked()
            {
                actions.push(Action::AddFolder);
            }
        } else {
            let title = if store.repos.is_empty() {
                "No Git repositories found in watched folders"
            } else {
                "No matching repositories"
            };
            ui.label(RichText::new(title).font(theme::title()).color(p.text_primary));
            let hint = if store.repos.is_empty() {
                "Add another folder or enable 'Scan nested repos' in the sidebar."
            } else {
                "Clear the current filter or search query to view all repositories."
            };
            ui.label(RichText::new(hint).font(theme::body()).color(p.text_secondary));
            ui.add_space(SPACE1);
            if store.repos.is_empty() {
                if widgets::button(ui, "Add Folder…", Some(icon::PLUS), Variant::Primary, false, true).clicked() {
                    actions.push(Action::AddFolder);
                }
            } else if widgets::button(ui, "Reset Filters", None, Variant::Secondary, false, true).clicked() {
                actions.push(Action::ResetFilters);
            }
        }
    });
}

// MARK: - Status bar

fn status_bar(ui: &mut Ui, store: &Store, counts: &Counts, actions: &mut Vec<Action>) {
    let p = theme::palette(ui.ctx());
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE2;
        let batch = store.is_batch_running();
        let state = if batch || store.is_scanning {
            SyncState::Checking
        } else if counts.outdated > 0 {
            SyncState::Behind(counts.outdated)
        } else {
            SyncState::UpToDate
        };
        widgets::status_icon(ui, &state, true, false);
        if batch {
            let (done, total) = store.batch_progress();
            ui.label(RichText::new(format!("{done}/{total}")).font(theme::meta_mono()).color(p.text_primary));
        }

        let selection = store.selected.len();
        let buttons_w = if selection > 0 { 380.0 } else { 0.0 };
        let banner_w = (ui.available_width() - buttons_w).max(80.0);
        let (banner_rect, banner) = ui.allocate_exact_size(vec2(banner_w, ROW_HEIGHT), Sense::hover());
        widgets::paint_text(ui, banner_rect, &store.status_banner, theme::meta(), p.text_secondary, false);
        banner.on_hover_text(&store.status_banner);

        if selection > 0 {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let enabled = !store.is_batch_running();
                if widgets::button(
                    ui,
                    &format!("Pull ({selection})"),
                    Some(icon::ARROW_DOWN),
                    Variant::Primary,
                    true,
                    enabled,
                )
                .clicked()
                {
                    actions.push(Action::PullSelected);
                }
                if widgets::button(
                    ui,
                    &format!("Push ({selection})"),
                    Some(icon::ARROW_UP),
                    Variant::Secondary,
                    true,
                    enabled,
                )
                .clicked()
                {
                    actions.push(Action::PushSelected);
                }
                if widgets::button(
                    ui,
                    &format!("Check ({selection})"),
                    Some(icon::ARROWS_CLOCKWISE),
                    Variant::Secondary,
                    true,
                    enabled,
                )
                .clicked()
                {
                    actions.push(Action::CheckSelected);
                }
                if widgets::button(
                    ui,
                    &format!("Delete ({selection})"),
                    Some(icon::TRASH),
                    Variant::Secondary,
                    true,
                    enabled,
                )
                .clicked()
                {
                    actions.push(Action::RequestDelete(store.selected_paths()));
                }
            });
        }
    });
}

// MARK: - Inspector

fn empty_inspector(ui: &mut Ui) {
    let p = theme::palette(ui.ctx());
    centered(ui, |ui| {
        ui.label(RichText::new("No repository selected").font(theme::body_medium()).color(p.text_secondary));
        ui.add(
            egui::Label::new(
                RichText::new("Select a row to inspect commits, working tree changes, and git output.")
                    .font(theme::meta())
                    .color(p.text_tertiary),
            )
            .halign(Align::Center)
            .wrap(),
        );
    });
}

fn inspector(ui: &mut Ui, store: &Store, view: &mut ViewState, repo: &GitRepo, actions: &mut Vec<Action>) {
    let p = theme::palette(ui.ctx());
    ui.spacing_mut().item_spacing = vec2(SPACE2, 0.0);

    Frame::NONE.inner_margin(Margin::same(16)).show(ui, |ui| {
        inspector_header(ui, repo, actions);
    });
    widgets::hline(ui, p.border);
    Frame::NONE.inner_margin(Margin::same(16)).show(ui, |ui| {
        sync_section(ui, repo, view, actions);
    });
    widgets::hline(ui, p.border);

    ScrollArea::vertical().id_salt(("inspector-scroll", &repo.path)).auto_shrink([false, false]).show(ui, |ui| {
        Frame::NONE.inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = SPACE2;
            readme_section(ui, repo, view, actions);
            if !repo.incoming_commits.is_empty() {
                ui.add_space(SPACE4);
                commits_section(ui, "Incoming Updates", &repo.incoming_commits);
            }
            if !repo.unpushed_commits.is_empty() {
                ui.add_space(SPACE4);
                let title =
                    if repo.sync_state.is_unpublished_branch() { "Local Branch Commits" } else { "Unpushed Commits" };
                commits_section(ui, title, &repo.unpushed_commits);
            }
            if repo.is_dirty() {
                ui.add_space(SPACE4);
                local_changes_section(ui, store, repo, view, actions);
            }
            if !repo.last_command_output.is_empty() {
                ui.add_space(SPACE4);
                output_section(ui, repo);
            }
        });
    });
}

fn inspector_header(ui: &mut Ui, repo: &GitRepo, actions: &mut Vec<Action>) {
    let p = theme::palette(ui.ctx());
    let icon_count = 5.0;
    let icons_w = icon_count * ROW_COMPACT + (icon_count - 1.0) * SPACE1;
    ui.horizontal(|ui| {
        let text_w = (ui.available_width() - icons_w - SPACE2).max(60.0);
        ui.allocate_ui_with_layout(vec2(text_w, 40.0), Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = SPACE1;
            ui.add(egui::Label::new(RichText::new(&repo.name).font(theme::title()).color(p.text_primary)).truncate());
            ui.add(
                egui::Label::new(RichText::new(repo.display_path()).font(theme::meta_mono()).color(p.text_tertiary))
                    .truncate(),
            )
            .on_hover_text(&repo.path);
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = SPACE1;
            let path = &repo.path;
            if widgets::icon_button(ui, icon::TRASH, "Delete Local Repository", !repo.sync_state.is_busy()).clicked() {
                actions.push(Action::RequestDelete(vec![path.clone()]));
            }
            if widgets::icon_button(ui, icon::COPY, "Copy Repository Path", true).clicked() {
                actions.push(Action::CopyPath(path.clone()));
            }
            let tooltip =
                if repo.can_fork() { "Fork on GitHub or on this PC…" } else { "Fork to a new folder on this PC…" };
            let enabled = repo.has_commits() && !repo.sync_state.is_busy();
            if widgets::icon_button(ui, icon::GIT_FORK, tooltip, enabled).clicked() {
                actions.push(Action::OpenFork(path.clone()));
            }
            if widgets::icon_button(ui, icon::TERMINAL_WINDOW, "Open in Terminal", true).clicked() {
                actions.push(Action::OpenTerminal(path.clone()));
            }
            if widgets::icon_button(ui, icon::FOLDER, &format!("Open in {}", platform::FILE_MANAGER_NAME), true).clicked() {
                actions.push(Action::OpenExplorer(path.clone()));
            }
        });
    });
    ui.add_space(SPACE3);

    let branch = match &repo.upstream_branch {
        Some(upstream) => format!("{}  →  {upstream}", repo.branch),
        None => repo.branch.clone(),
    };
    let remote = repo.formatted_remote_host();
    let head = if repo.has_commits() {
        format!("{}  {}", repo.last_commit_hash, repo.last_commit_message)
    } else {
        "No commits yet".into()
    };
    widgets::container_frame(p).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        meta_row(ui, "Branch", &branch, "", true);
        widgets::hline(ui, p.border);
        meta_row(ui, "Remote", remote.as_deref().unwrap_or("None (local only)"), "", remote.is_some());
        widgets::hline(ui, p.border);
        if let Some(parent) = repo.fork_parent_display() {
            meta_row(ui, "Fork of", &parent, "upstream", true);
            widgets::hline(ui, p.border);
        }
        meta_row(ui, "HEAD", &head, &short_relative(&repo.last_commit_relative_date), false);
    });
}

fn meta_row(ui: &mut Ui, label: &str, value: &str, trailing: &str, monospace: bool) {
    let p = theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_COMPACT), Sense::hover());
    let inner = rect.shrink2(vec2(SPACE3, 0.0));
    let label_rect = Rect::from_min_size(inner.min, vec2(56.0, inner.height()));
    widgets::paint_text(ui, label_rect, label, theme::meta(), p.text_tertiary, false);
    let mut value_rect = Rect::from_min_max(pos2(label_rect.right() + SPACE2, inner.top()), inner.max);
    if !trailing.is_empty() {
        let w = widgets::paint_text(ui, inner, trailing, theme::meta(), p.text_tertiary, true);
        value_rect.max.x -= w + SPACE1;
    }
    let font = if monospace { theme::meta_mono() } else { theme::meta() };
    widgets::paint_text(ui, value_rect, value, font, p.text_primary, false);
    response.on_hover_text(value);
}

fn sync_section(ui: &mut Ui, repo: &GitRepo, view: &mut ViewState, actions: &mut Vec<Action>) {
    let p = theme::palette(ui.ctx());
    let state = &repo.sync_state;
    ui.spacing_mut().item_spacing.y = SPACE3;
    ui.horizontal(|ui| {
        widgets::status_icon(ui, state, false, true);
        if let Some(checked) = repo.last_checked_at {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(util::format_clock(checked, false)).font(theme::meta()).color(p.text_tertiary))
                    .on_hover_text("Last checked");
            });
        }
    });
    ui.add(
        egui::Label::new(RichText::new(state.inspector_explanation()).font(theme::meta()).color(p.text_secondary))
            .wrap(),
    );

    if !repo.has_remote() {
        let url = view.remote_urls.entry(repo.path.clone()).or_default();
        ui.horizontal(|ui| {
            let button_w =
                widgets::text_width(ui, "Add Remote", theme::body_medium()) + 2.0 * SPACE3 + ICON_SIZE + SPACE2;
            let field_w = (ui.available_width() - button_w - SPACE2).max(80.0);
            ui.add_sized(
                vec2(field_w, ROW_HEIGHT),
                TextEdit::singleline(url)
                    .hint_text(RichText::new("https://github.com/user/repo.git").color(p.text_tertiary))
                    .font(theme::meta_mono())
                    .margin(Margin::symmetric(10, 6)),
            );
            let enabled = !url.trim().is_empty() && !state.is_busy();
            if widgets::button(ui, "Add Remote", Some(icon::LINK), Variant::Primary, false, enabled).clicked() {
                actions.push(Action::AddRemote(repo.path.clone()));
            }
        });
    }

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE2;
        let path = &repo.path;
        if widgets::button(
            ui,
            "Check",
            Some(icon::ARROWS_CLOCKWISE),
            Variant::Secondary,
            false,
            !state.is_busy() && repo.has_remote(),
        )
        .clicked()
        {
            actions.push(Action::Check(path.clone()));
        }
        let pull_label =
            if state.behind_count() > 0 { format!("Pull ({})", state.behind_count()) } else { "Pull".into() };
        let pull_variant = if state.needs_update() { Variant::Primary } else { Variant::Secondary };
        if widgets::button(ui, &pull_label, Some(icon::ARROW_DOWN), pull_variant, false, state.can_pull()).clicked() {
            actions.push(Action::Pull(path.clone()));
        }
        let push_label = if state.is_unpublished_branch() {
            "Publish".to_string()
        } else if state.ahead_count() > 0 {
            format!("Push ({})", state.ahead_count())
        } else {
            "Push".into()
        };
        let push_variant =
            if !state.needs_update() && state.can_push() { Variant::Primary } else { Variant::Secondary };
        let can_push = !state.is_busy() && repo.has_remote() && repo.has_commits();
        if widgets::button(ui, &push_label, Some(icon::ARROW_UP), push_variant, false, can_push).clicked() {
            actions.push(Action::Push(path.clone()));
        }
    });
}

fn section_title(ui: &mut Ui, title: &str, count: Option<usize>, trailing: Option<&str>) {
    let p = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(theme::meta_medium()).color(p.text_secondary));
        if let Some(count) = count {
            ui.label(RichText::new(count.to_string()).font(theme::meta()).color(p.text_tertiary));
        }
        if let Some(trailing) = trailing {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(trailing).font(theme::meta()).color(p.text_tertiary));
            });
        }
    });
}

fn readme_section(ui: &mut Ui, repo: &GitRepo, view: &mut ViewState, actions: &mut Vec<Action>) {
    let p = theme::palette(ui.ctx());
    let expanded = view.readme_expanded;

    let (rect, toggle) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_COMPACT), Sense::click());
    let fill = if toggle.hovered() { p.surface2 } else { p.surface2.gamma_multiply(0.6) };
    ui.painter().rect_filled(rect, CornerRadius::same(RADIUS_CONTROL), fill);
    let mut x = rect.left() + SPACE2 + 6.0;
    let caret = if expanded { icon::CARET_DOWN } else { icon::CARET_RIGHT };
    ui.painter().text(pos2(x, rect.center().y), Align2::CENTER_CENTER, caret, theme::icon_font(11.0), p.text_secondary);
    x += 6.0 + SPACE2 + 6.0;
    ui.painter().text(
        pos2(x, rect.center().y),
        Align2::CENTER_CENTER,
        icon::FILE_TEXT,
        theme::icon_font(13.0),
        p.text_secondary,
    );
    x += 6.0 + SPACE2;
    let title = repo.readme_file_name.as_deref().unwrap_or("README & Description");
    ui.painter().text(pos2(x, rect.center().y), Align2::LEFT_CENTER, title, theme::meta_medium(), p.text_secondary);
    ui.painter().text(
        pos2(rect.right() - SPACE2, rect.center().y),
        Align2::RIGHT_CENTER,
        if expanded { "Hide" } else { "Show" },
        theme::meta(),
        p.text_tertiary,
    );
    if toggle.clicked() {
        actions.push(Action::ToggleReadme);
    }

    let description = repo.repo_description.as_deref().filter(|d| !d.is_empty());
    if !expanded {
        if let Some(desc) = description {
            ui.add(egui::Label::new(RichText::new(desc).font(theme::meta()).color(p.text_secondary)).wrap());
        }
        return;
    }

    widgets::container_frame(p).inner_margin(Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = SPACE1;
        if let Some(desc) = description {
            ui.label(RichText::new("Description").font(theme::meta_medium()).color(p.text_tertiary));
            ui.add(egui::Label::new(RichText::new(desc).font(theme::body()).color(p.text_primary)).wrap());
        }
        match repo.readme_content.as_deref().filter(|c| !c.is_empty()) {
            Some(readme) => {
                if description.is_some() {
                    ui.add_space(SPACE2);
                    widgets::hline(ui, p.border);
                    ui.add_space(SPACE2);
                }
                let segments = prepared_readme(view, repo, readme);
                let base_uri = format!("file:///{}/", repo.path.replace('\\', "/").trim_end_matches('/'));
                widgets::markdown(ui, &mut view.md_cache, &segments, &base_uri, ("readme", repo.path.as_str()));
            }
            None if description.is_none() => {
                ui.label(
                    RichText::new("No README file or description found in this repository.")
                        .font(theme::meta())
                        .color(p.text_tertiary),
                );
            }
            None => {}
        }
    });
}

fn prepared_readme(view: &mut ViewState, repo: &GitRepo, readme: &str) -> Rc<Vec<markdown::Segment>> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    readme.hash(&mut hasher);
    let hash = hasher.finish();
    if let Some((cached_hash, segments)) = view.readme_cache.get(&repo.path)
        && *cached_hash == hash
    {
        return Rc::clone(segments);
    }
    let prepared = markdown::prepare_readme(readme, repo.readme_file_name.as_deref());
    let segments = Rc::new(markdown::split_tables(&prepared));
    view.readme_cache.insert(repo.path.clone(), (hash, Rc::clone(&segments)));
    segments
}

fn commits_section(ui: &mut Ui, title: &str, commits: &[crate::model::CommitInfo]) {
    let p = theme::palette(ui.ctx());
    section_title(ui, title, Some(commits.len()), None);
    widgets::container_frame(p).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (i, commit) in commits.iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_COMPACT), Sense::hover());
            let inner = rect.shrink2(vec2(SPACE3, 0.0));
            let hash_rect = Rect::from_min_size(inner.min, vec2(56.0, inner.height()));
            widgets::paint_text(ui, hash_rect, &commit.hash, theme::meta_mono(), p.text_secondary, false);
            let date_w = widgets::paint_text(
                ui,
                inner,
                &short_relative(&commit.relative_date),
                theme::meta(),
                p.text_tertiary,
                true,
            );
            let subject_rect = Rect::from_min_max(
                pos2(hash_rect.right() + SPACE2, inner.top()),
                pos2(inner.right() - date_w - SPACE2, inner.bottom()),
            );
            widgets::paint_text(ui, subject_rect, &commit.subject, theme::body(), p.text_primary, false);
            response.on_hover_text(format!("{}\n{} • {}", commit.subject, commit.author, commit.relative_date));
            if i + 1 < commits.len() {
                widgets::hline(ui, p.border);
            }
        }
    });
}

fn local_changes_section(ui: &mut Ui, store: &Store, repo: &GitRepo, view: &mut ViewState, actions: &mut Vec<Action>) {
    let p = theme::palette(ui.ctx());
    let trailing = store.settings.auto_stash_on_pull.then_some("Auto-stash active");
    section_title(ui, "Uncommitted Changes", Some(repo.local_changes.len()), trailing);

    let message = view.commit_messages.entry(repo.path.clone()).or_default();
    let button_label = if repo.has_remote() { "Commit & Push" } else { "Commit" };
    let mut submit = false;
    ui.horizontal(|ui| {
        let button_w = widgets::text_width(ui, button_label, theme::body_medium()) + 2.0 * SPACE3;
        let field_w = (ui.available_width() - button_w - SPACE2).max(80.0);
        let hint = if repo.has_remote() { "Commit message to stage & push…" } else { "Commit message…" };
        let edit = ui.add_sized(
            vec2(field_w, ROW_HEIGHT),
            TextEdit::singleline(message)
                .hint_text(RichText::new(hint).color(p.text_tertiary))
                .font(theme::body())
                .margin(Margin::symmetric(10, 6)),
        );
        let ready = !message.trim().is_empty() && !repo.sync_state.is_busy();
        if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && ready {
            submit = true;
        }
        if widgets::button(ui, button_label, None, Variant::Secondary, false, ready).clicked() {
            submit = true;
        }
    });
    if submit {
        actions.push(Action::CommitAndPush(repo.path.clone()));
    }

    widgets::container_frame(p).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let visible = &repo.local_changes[..repo.local_changes.len().min(MAX_VISIBLE_CHANGES)];
        for (i, change) in visible.iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_COMPACT), Sense::hover());
            let inner = rect.shrink2(vec2(SPACE3, 0.0));
            let status_rect = Rect::from_min_size(inner.min, vec2(24.0, inner.height()));
            widgets::paint_text(ui, status_rect, &change.status, theme::meta_medium(), p.text_secondary, false);
            let path_rect = Rect::from_min_max(pos2(status_rect.right() + SPACE2, inner.top()), inner.max);
            widgets::paint_text(ui, path_rect, &change.path, theme::meta_mono(), p.text_primary, false);
            response.on_hover_text(&change.path);
            if i + 1 < visible.len() {
                widgets::hline(ui, p.border);
            }
        }
        let hidden = repo.local_changes.len().saturating_sub(MAX_VISIBLE_CHANGES);
        if hidden > 0 {
            widgets::hline(ui, p.border);
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_COMPACT), Sense::hover());
            widgets::paint_text(
                ui,
                rect.shrink2(vec2(SPACE3, 0.0)),
                &format!("…and {hidden} more"),
                theme::meta(),
                p.text_tertiary,
                false,
            );
        }
    });
}

fn output_section(ui: &mut Ui, repo: &GitRepo) {
    let p = theme::palette(ui.ctx());
    section_title(ui, "Git Output", None, None);
    widgets::container_frame(p).inner_margin(Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let mut text = repo.last_command_output.as_str();
        ui.add(
            TextEdit::multiline(&mut text)
                .font(theme::meta_mono())
                .text_color(p.text_secondary)
                .frame(Frame::NONE)
                .margin(Margin::ZERO)
                .desired_rows(1)
                .desired_width(f32::INFINITY),
        );
    });
}

// MARK: - Dialogs

fn dialog_frame(p: &Palette) -> Frame {
    Frame::new()
        .fill(p.surface1)
        .stroke(Stroke::new(1.0, p.border_strong))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(20))
}

fn delete_dialog(ctx: &egui::Context, store: &Store, view: &ViewState, actions: &mut Vec<Action>) {
    if view.pending_delete.is_empty() {
        return;
    }
    let targets: Vec<&GitRepo> = view.pending_delete.iter().filter_map(|path| store.repo(path)).collect();
    if targets.is_empty() {
        actions.push(Action::CancelDelete);
        return;
    }
    let p = theme::palette(ctx);
    let modal = egui::Modal::new(Id::new("delete-dialog")).frame(dialog_frame(p)).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.spacing_mut().item_spacing.y = SPACE2;
        let title = match targets.as_slice() {
            [single] => format!("Delete local repository \"{}\"?", single.name),
            many => format!("Delete {} local repositories?", many.len()),
        };
        ui.label(RichText::new(title).font(theme::title()).color(p.text_primary));

        let mut lines: Vec<String> = targets.iter().take(5).map(|r| format!("• {}  ({})", r.name, r.display_path())).collect();
        if targets.len() > 5 {
            lines.push(format!("…and {} more.", targets.len() - 5));
        }
        ui.label(RichText::new(lines.join("\n")).font(theme::meta()).color(p.text_secondary));
        ui.add(
            egui::Label::new(
                RichText::new("This removes the folder(s) from this PC. Remote repositories on GitHub, GitLab, etc. are not affected.")
                    .font(theme::body())
                    .color(p.text_secondary),
            )
            .wrap(),
        );

        let unpushed = targets.iter().filter(|r| r.sync_state.ahead_count() > 0 || r.sync_state.is_unpublished_branch()).count();
        let dirty = targets.iter().filter(|r| r.is_dirty()).count();
        if unpushed > 0 || dirty > 0 {
            let mut parts = Vec::new();
            if unpushed > 0 {
                parts.push(format!("{unpushed} repo(s) have unpushed or unpublished commits"));
            }
            if dirty > 0 {
                parts.push(format!("{dirty} repo(s) have uncommitted local changes"));
            }
            ui.add(
                egui::Label::new(
                    RichText::new(format!("{} Warning: {}.", icon::WARNING, parts.join(" and ")))
                        .font(theme::body_medium())
                        .color(p.notice),
                )
                .wrap(),
            );
        }

        ui.add_space(SPACE2);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = SPACE2;
            if widgets::button(ui, platform::TRASH_BUTTON_LABEL, Some(icon::TRASH), Variant::Primary, false, true).clicked() {
                actions.push(Action::ConfirmDelete { to_recycle_bin: true });
            }
            if widgets::button(ui, "Delete Permanently", None, Variant::Secondary, false, true)
                .on_hover_text(format!("Skips {}. This cannot be undone.", platform::TRASH_NOUN))
                .clicked()
            {
                actions.push(Action::ConfirmDelete { to_recycle_bin: false });
            }
            if widgets::button(ui, "Cancel", None, Variant::Ghost, false, true).clicked() {
                actions.push(Action::CancelDelete);
            }
        });
    });
    if modal.should_close() {
        actions.push(Action::CancelDelete);
    }
}

fn activity_log_dialog(ctx: &egui::Context, store: &Store, actions: &mut Vec<Action>) {
    let p = theme::palette(ctx);
    let screen = ctx.content_rect();
    let size = vec2(680.0f32.min(screen.width() - 48.0), 480.0f32.min(screen.height() - 48.0));
    let modal =
        egui::Modal::new(Id::new("activity-log")).frame(dialog_frame(p).inner_margin(Margin::ZERO)).show(ctx, |ui| {
            ui.set_width(size.x);
            ui.set_height(size.y);
            ui.spacing_mut().item_spacing = vec2(SPACE2, 0.0);
            Frame::NONE.inner_margin(Margin::symmetric(16, 8)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Activity Log").font(theme::title()).color(p.text_primary));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, "Close", None, Variant::Primary, true, true).clicked() {
                            actions.push(Action::ShowActivityLog(false));
                        }
                        if !store.activity_logs.is_empty()
                            && widgets::button(ui, "Clear", None, Variant::Secondary, true, true).clicked()
                        {
                            actions.push(Action::ClearActivityLog);
                        }
                    });
                });
            });
            widgets::hline(ui, p.border);

            if store.activity_logs.is_empty() {
                centered(ui, |ui| {
                    ui.label(
                        RichText::new("No git operations recorded yet.").font(theme::body()).color(p.text_secondary),
                    );
                });
                return;
            }
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, entry) in store.activity_logs.iter().enumerate() {
                    Frame::NONE.inner_margin(Margin::symmetric(16, 8)).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = SPACE1;
                        let (rect, header) =
                            ui.allocate_exact_size(vec2(ui.available_width(), ROW_COMPACT), Sense::hover());
                        header.on_hover_text(format!("$ {}", entry.command));
                        let (glyph, color) =
                            if entry.succeeded { (icon::CHECK_CIRCLE, p.good) } else { (icon::X_CIRCLE, p.bad) };
                        ui.painter().text(
                            pos2(rect.left() + 7.0, rect.center().y),
                            Align2::CENTER_CENTER,
                            glyph,
                            theme::icon_font(13.0),
                            color,
                        );
                        let time = util::format_clock(entry.timestamp, true);
                        let time_w = widgets::paint_text(ui, rect, &time, theme::meta_mono(), p.text_tertiary, true);
                        let mut x = rect.left() + 22.0;
                        let right = rect.right() - time_w - SPACE2;
                        for (text, font, color) in [
                            (entry.repo_name.as_str(), theme::body_medium(), p.text_primary),
                            (entry.action.as_str(), theme::meta(), p.text_secondary),
                            (entry.summary.as_str(), theme::meta(), p.text_tertiary),
                        ] {
                            if x >= right {
                                break;
                            }
                            let cell = Rect::from_min_max(pos2(x, rect.top()), pos2(right, rect.bottom()));
                            x += widgets::paint_text(ui, cell, text, font, color, false) + SPACE2;
                        }
                        if !entry.output.is_empty() {
                            Frame::new()
                                .fill(p.canvas)
                                .stroke(Stroke::new(1.0, p.border))
                                .corner_radius(CornerRadius::same(RADIUS_CONTROL))
                                .inner_margin(Margin::same(8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    let mut text = entry.output.as_str();
                                    ui.add(
                                        TextEdit::multiline(&mut text)
                                            .font(theme::meta_mono())
                                            .text_color(p.text_secondary)
                                            .frame(Frame::NONE)
                                            .margin(Margin::ZERO)
                                            .desired_rows(1)
                                            .desired_width(f32::INFINITY),
                                    );
                                });
                        }
                    });
                    if i + 1 < store.activity_logs.len() {
                        widgets::hline(ui, p.border);
                    }
                }
            });
        });
    if modal.should_close() {
        actions.push(Action::ShowActivityLog(false));
    }
}

/// One "remote → repository (push/pull)" line in the fork preview.
fn remote_line(ui: &mut Ui, remote: &str, slug: &str, glyph: &str, role: &str, highlight: bool) {
    let p = theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let inner = rect.shrink2(vec2(SPACE3, 0.0));
    let remote_rect = Rect::from_min_size(inner.min, vec2(72.0, inner.height()));
    widgets::paint_text(ui, remote_rect, remote, theme::meta_mono(), p.text_tertiary, false);

    // Role chip on the right: "↑ push" or "↓ pull".
    let chip_font = theme::meta_medium();
    let chip_text = format!("{glyph} {role}");
    let chip_w = widgets::text_width(ui, &chip_text, chip_font.clone()) + 2.0 * SPACE2;
    let chip = Rect::from_center_size(pos2(inner.right() - chip_w / 2.0, inner.center().y), vec2(chip_w, 20.0));
    let chip_color = if highlight { p.accent } else { p.text_secondary };
    ui.painter().rect_filled(chip, CornerRadius::same(RADIUS_CONTROL), chip_color.gamma_multiply(0.14));
    ui.painter().text(chip.center(), Align2::CENTER_CENTER, chip_text, chip_font, chip_color);

    let slug_rect =
        Rect::from_min_max(pos2(remote_rect.right() + SPACE2, inner.top()), pos2(chip.left() - SPACE2, inner.bottom()));
    let slug_color = if highlight { p.text_primary } else { p.text_secondary };
    widgets::paint_text(ui, slug_rect, slug, theme::meta_mono(), slug_color, false);
}

fn fork_dialog(ctx: &egui::Context, flow: &mut ForkFlow, store: &Store, actions: &mut Vec<Action>) {
    let Some(repo) = store.repo(&flow.repo_path) else {
        actions.push(Action::ForkClose);
        return;
    };
    let p = theme::palette(ctx);
    let original = repo.github_slug().unwrap_or_else(|| repo.display_path());
    let original_owner =
        repo.github_slug().and_then(|s| s.split_once('/').map(|(o, _)| o.to_string())).unwrap_or_default();
    let repo_name = repo
        .github_slug()
        .and_then(|s| s.split_once('/').map(|(_, n)| n.to_string()))
        .unwrap_or_else(|| repo.name.clone());
    // Copy what the GitHub step shows so the destination picker can borrow `flow` mutably.
    let ready = match &flow.phase {
        Some(ForkPhase::Ready { login, orgs, plan }) => Some((login.clone(), orgs.clone(), plan.clone())),
        _ => None,
    };
    let already_yours = ready.as_ref().is_some_and(|(login, _, _)| login.eq_ignore_ascii_case(&original_owner));
    let same_owner = flow.destination.eq_ignore_ascii_case(&original_owner);
    let path_problem = github::local_path_problem(&flow.local_path);
    let can_confirm = !repo.sync_state.is_busy()
        && match flow.target {
            ForkTarget::GitHub => ready.is_some() && !already_yours && !same_owner,
            ForkTarget::Local => path_problem.is_none(),
        };

    let modal = egui::Modal::new(Id::new("fork-dialog")).frame(dialog_frame(p)).show(ctx, |ui| {
        ui.set_width(480.0);
        ui.spacing_mut().item_spacing.y = SPACE2;
        ui.horizontal(|ui| {
            let (badge, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
            ui.painter().rect_filled(badge, CornerRadius::same(theme::RADIUS_CONTAINER), p.accent.gamma_multiply(0.16));
            ui.painter().text(badge.center(), Align2::CENTER_CENTER, icon::GIT_FORK, theme::icon_font(21.0), p.accent);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(format!("Fork {}", repo.name)).font(theme::title()).color(p.text_primary));
                ui.add(egui::Label::new(RichText::new(&original).font(theme::meta_mono()).color(p.text_tertiary)).truncate());
            });
        });

        let wrapped = |ui: &mut Ui, text: &str, color: Color32| {
            ui.add(egui::Label::new(RichText::new(text).font(theme::body()).color(color)).wrap());
        };
        let note = |ui: &mut Ui, text: &str, color: Color32| {
            ui.add(egui::Label::new(RichText::new(text).font(theme::meta()).color(color)).wrap());
        };

        match &flow.stage {
            ForkStage::Choosing => {
                ui.add_space(SPACE1);
                let github_reason = match repo.fork_parent_display() {
                    Some(parent) => format!("Already a fork of {parent}"),
                    None if repo.github_slug().is_none() => "Only repositories hosted on GitHub can be forked there".into(),
                    None => "Create your own copy on GitHub".into(),
                };
                let options = [
                    ("On GitHub", repo.can_fork(), github_reason.as_str()),
                    (platform::LOCAL_COPY_TAB, true, "Copy the repository into a new folder"),
                ];
                let selected = usize::from(flow.target == ForkTarget::Local);
                if let Some(index) = widgets::segmented(ui, &options, selected) {
                    actions.push(Action::ForkSetTarget(if index == 0 { ForkTarget::GitHub } else { ForkTarget::Local }));
                }
                ui.add_space(SPACE1);

                match flow.target {
                    ForkTarget::Local => {
                        wrapped(
                            ui,
                            "Make an independent copy in a new folder, with every branch, tag, and remote. Experiment there without touching the original.",
                            p.text_secondary,
                        );
                        ui.label(RichText::new("New folder").font(theme::meta_medium()).color(p.text_tertiary));
                        ui.horizontal(|ui| {
                            let browse_w = widgets::text_width(ui, "Browse…", theme::body_medium()) + 2.0 * SPACE3;
                            let field_w = (ui.available_width() - browse_w - SPACE2).max(120.0);
                            ui.add_sized(
                                vec2(field_w, ROW_HEIGHT),
                                TextEdit::singleline(&mut flow.local_path).font(theme::meta_mono()).margin(Margin::symmetric(10, 7)),
                            );
                            if widgets::button(ui, "Browse…", None, Variant::Secondary, false, true)
                                .on_hover_text("Pick the folder to put the copy in")
                                .clicked()
                            {
                                actions.push(Action::ForkBrowse);
                            }
                        });
                        if let Some(problem) = path_problem {
                            note(ui, problem, p.notice);
                        }
                        if repo.is_dirty() {
                            let n = repo.local_changes.len();
                            note(
                                ui,
                                &format!("{n} uncommitted {} stay in the original; commit first to bring them along.", util::plural(n, "change", "changes")),
                                p.text_tertiary,
                            );
                        }
                    }
                    ForkTarget::GitHub => match &flow.phase {
                        None | Some(ForkPhase::Checking) => spinner_row(ui, "Checking your GitHub sign-in…"),
                        Some(ForkPhase::NeedsCli) => wrapped(
                            ui,
                            "Forking on GitHub uses the GitHub CLI, which isn't installed. Install it, sign in once, then click Check Again.",
                            p.text_secondary,
                        ),
                        Some(ForkPhase::NeedsSignIn) => wrapped(
                            ui,
                            "Sign in to GitHub once so GitDaddy can create forks for you. Sign In opens a terminal that finishes in your browser. Come back and click Check Again when it says you're logged in.",
                            p.text_secondary,
                        ),
                        Some(ForkPhase::Unavailable { message }) => wrapped(ui, &format!("{}  {message}", icon::WARNING), p.bad),
                        Some(ForkPhase::Ready { .. }) if already_yours => wrapped(
                            ui,
                            "This repository already belongs to you, so there's nothing to fork on GitHub. You can still copy it on this PC.",
                            p.text_secondary,
                        ),
                        Some(ForkPhase::Ready { .. }) => {
                            let (login, orgs, plan) = ready.clone().unwrap_or_default();
                            wrapped(
                                ui,
                                "Make your own copy on GitHub and point this clone at it. Your work stays where it is; nothing is pushed until you push.",
                                p.text_secondary,
                            );
                            if !orgs.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Fork to").font(theme::body()).color(p.text_secondary));
                                    let label = if flow.destination == login { format!("{login} (you)") } else { flow.destination.clone() };
                                    let destination = &mut flow.destination;
                                    widgets::combo_box(ui, "fork-destination", &label, 220.0, |ui| {
                                        for account in std::iter::once(&login).chain(orgs.iter()) {
                                            let text = if *account == login { format!("{login} (you)") } else { account.clone() };
                                            if ui.add(egui::Button::selectable(destination == account, text)).clicked() {
                                                *destination = account.clone();
                                            }
                                        }
                                    });
                                });
                            }
                            ui.add_space(SPACE1);
                            ui.label(RichText::new("This clone after forking").font(theme::meta_medium()).color(p.text_tertiary));
                            let fork_slug = format!("{}/{repo_name}", flow.destination);
                            widgets::container_frame(p).show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                remote_line(ui, &plan.fork_remote, &fork_slug, icon::ARROW_UP, "push", true);
                                widgets::hline(ui, p.border);
                                remote_line(ui, &plan.original_remote, &original, icon::ARROW_DOWN, "pull", false);
                            });
                            if same_owner {
                                note(ui, &format!("{original_owner} already owns the original. Pick another account."), p.notice);
                            } else {
                                note(ui, "Pulls keep you up to date with the original. Pushes go to your fork.", p.text_tertiary);
                            }
                        }
                    },
                }
            }
            ForkStage::Working { message } => {
                ui.add_space(SPACE1);
                spinner_row(ui, message);
                note(ui, "This usually takes a few seconds.", p.text_tertiary);
            }
            ForkStage::Done(done) => {
                ui.add_space(SPACE1);
                let (headline, detail) = match done {
                    ForkDone::GitHub { fork } => (
                        format!("Forked to {fork}"),
                        format!("Pushes from this clone now go to your fork, and pulls keep bringing in updates from {original}."),
                    ),
                    ForkDone::Local { path } => (
                        format!("Copied to {}", util::abbreviate_home(path)),
                        "The copy is in your list and selected. Work there never touches the original folder.".to_string(),
                    ),
                };
                ui.label(RichText::new(format!("{}  {headline}", icon::CHECK_CIRCLE)).font(theme::body_medium()).color(p.good));
                wrapped(ui, &detail, p.text_secondary);
            }
            ForkStage::Failed { message } => {
                ui.add_space(SPACE1);
                wrapped(ui, &format!("{}  {message}", icon::WARNING), p.bad);
            }
        }

        ui.add_space(SPACE2);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = SPACE2;
            let mut button = |label: &str, glyph: Option<&str>, variant: Variant, enabled: bool, action: Action| {
                if widgets::button(ui, label, glyph, variant, false, enabled).clicked() {
                    actions.push(action);
                }
            };
            match (&flow.stage, flow.target, &flow.phase) {
                (ForkStage::Choosing, ForkTarget::GitHub, Some(ForkPhase::NeedsCli)) => {
                    button("Get GitHub CLI", None, Variant::Primary, true, Action::OpenUrl(github::CLI_DOWNLOAD_URL.into()));
                    button("Check Again", None, Variant::Secondary, true, Action::ForkRecheck);
                    button("Cancel", None, Variant::Ghost, true, Action::ForkClose);
                }
                (ForkStage::Choosing, ForkTarget::GitHub, Some(ForkPhase::NeedsSignIn)) => {
                    button("Sign In…", None, Variant::Primary, true, Action::ForkSignIn);
                    button("Check Again", None, Variant::Secondary, true, Action::ForkRecheck);
                    button("Cancel", None, Variant::Ghost, true, Action::ForkClose);
                }
                (ForkStage::Choosing, ForkTarget::GitHub, Some(ForkPhase::Unavailable { .. })) => {
                    button("Check Again", None, Variant::Primary, true, Action::ForkRecheck);
                    button("Cancel", None, Variant::Ghost, true, Action::ForkClose);
                }
                (ForkStage::Choosing, _, _) => {
                    button("Fork", Some(icon::GIT_FORK), Variant::Primary, can_confirm, Action::ForkConfirm);
                    button("Cancel", None, Variant::Ghost, true, Action::ForkClose);
                }
                (ForkStage::Working { .. }, _, _) => {}
                (ForkStage::Done(ForkDone::GitHub { fork }), _, _) => {
                    button("Done", None, Variant::Primary, true, Action::ForkClose);
                    button(
                        "Open on GitHub",
                        Some(icon::ARROW_SQUARE_OUT),
                        Variant::Secondary,
                        true,
                        Action::OpenUrl(format!("https://github.com/{fork}")),
                    );
                }
                (ForkStage::Done(ForkDone::Local { path }), _, _) => {
                    button("Done", None, Variant::Primary, true, Action::ForkClose);
                    button("Open Folder", Some(icon::FOLDER), Variant::Secondary, true, Action::OpenExplorer(path.clone()));
                }
                (ForkStage::Failed { .. }, _, _) => {
                    button("Try Again", None, Variant::Primary, true, Action::ForkRetry);
                    button("Close", None, Variant::Ghost, true, Action::ForkClose);
                }
            }
        });
    });
    if modal.should_close() && !flow.is_busy() {
        actions.push(Action::ForkClose);
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

fn spinner_row(ui: &mut Ui, text: &str) {
    let p = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(ICON_SIZE, ICON_SIZE), Sense::hover());
        widgets::paint_spinner(ui, rect.center(), ICON_SIZE);
        ui.label(RichText::new(text).font(theme::body()).color(p.text_primary));
    });
}

fn update_dialog(
    ctx: &egui::Context,
    updater: &Updater,
    store: &Store,
    view: &mut ViewState,
    actions: &mut Vec<Action>,
) {
    if !updater.show_dialog {
        return;
    }
    let p = theme::palette(ctx);
    let current = update::Version::current();
    let installed = updater.installed;
    let busy = store.busy();

    let modal = egui::Modal::new(Id::new("update-dialog")).frame(dialog_frame(p)).show(ctx, |ui| {
        ui.set_width(480.0);
        ui.spacing_mut().item_spacing.y = SPACE2;
        ui.horizontal(|ui| {
            if let Some(logo) = &view.logo {
                ui.add(egui::Image::new(SizedTexture::new(logo.id(), vec2(40.0, 40.0))));
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Software Update").font(theme::title()).color(p.text_primary));
                ui.label(RichText::new(format!("You have GitDaddy {current}")).font(theme::meta()).color(p.text_tertiary));
            });
        });
        ui.add_space(SPACE1);

        let wrapped = |ui: &mut Ui, text: &str, font: egui::FontId, color: Color32| {
            ui.add(egui::Label::new(RichText::new(text).font(font).color(color)).wrap());
        };
        match &updater.phase {
            Phase::Idle | Phase::Checking => spinner_row(ui, "Checking GitHub for a newer version…"),
            Phase::UpToDate(latest) => {
                ui.label(
                    RichText::new(format!("{}  GitDaddy {current} is up to date.", icon::CHECK_CIRCLE))
                        .font(theme::body_medium())
                        .color(p.good),
                );
                if let Some(latest) = latest {
                    let date = if latest.published.is_empty() { String::new() } else { format!(", {}", latest.published) };
                    ui.label(
                        RichText::new(format!("Newest Windows release: {}{date}", latest.tag))
                            .font(theme::meta())
                            .color(p.text_tertiary),
                    );
                }
            }
            Phase::NoRelease => {
                wrapped(ui, "No Windows release has been published on GitHub yet.", theme::body(), p.text_secondary)
            }
            Phase::Available(release) => {
                ui.label(
                    RichText::new(format!("GitDaddy {} is available", release.version))
                        .font(theme::body_medium())
                        .color(p.text_primary),
                );
                let mut facts = vec![format!("{} installer", megabytes(release.installer.size))];
                if !release.published.is_empty() {
                    facts.insert(0, format!("Released {}", release.published));
                }
                ui.label(RichText::new(facts.join(" • ")).font(theme::meta()).color(p.text_tertiary));
                if !release.notes.trim().is_empty() {
                    widgets::container_frame(p).inner_margin(Margin::same(12)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                            ui.visuals_mut().override_text_color = Some(p.text_secondary);
                            CommonMarkViewer::new().show(ui, &mut view.md_cache, &release.notes);
                        });
                    });
                }
                if !installed {
                    wrapped(
                        ui,
                        "You're running the portable version. Download the new zip from GitHub and replace GitDaddy.exe.",
                        theme::meta(),
                        p.text_secondary,
                    );
                } else if busy {
                    wrapped(ui, "Wait for the running Git operations to finish before updating.", theme::meta(), p.notice);
                } else {
                    wrapped(ui, "GitDaddy will close, install the update, and reopen.", theme::meta(), p.text_secondary);
                }
            }
            Phase::Downloading { release, progress, .. } => {
                ui.label(
                    RichText::new(format!("Downloading GitDaddy {}…", release.version))
                        .font(theme::body_medium())
                        .color(p.text_primary),
                );
                let done = progress.load(std::sync::atomic::Ordering::Relaxed);
                let total = release.installer.size.max(1);
                ui.add(
                    egui::ProgressBar::new(done as f32 / total as f32)
                        .desired_width(ui.available_width())
                        .desired_height(6.0)
                        .fill(p.accent),
                );
                ui.label(
                    RichText::new(format!("{} of {}", megabytes(done), megabytes(release.installer.size)))
                        .font(theme::meta())
                        .color(p.text_tertiary),
                );
            }
            Phase::Installing(release) => {
                spinner_row(ui, &format!("Starting the installer for GitDaddy {}…", release.version));
                wrapped(ui, "GitDaddy will reopen when the update finishes.", theme::meta(), p.text_secondary);
            }
            Phase::Failed { message, .. } => {
                wrapped(ui, &format!("{}  {message}", icon::WARNING), theme::body(), p.bad);
            }
        }

        ui.add_space(SPACE2);
        ui.horizontal(|ui| {
            let mut automatic = store.settings.check_updates_automatically;
            if widgets::checkbox(ui, &mut automatic, "Check automatically") {
                actions.push(Action::SetAutoUpdateCheck(automatic));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = SPACE2;
                match &updater.phase {
                    Phase::Available(release) => {
                        if installed {
                            if widgets::button(ui, "Download & Install", Some(icon::DOWNLOAD_SIMPLE), Variant::Primary, true, !busy)
                                .clicked()
                            {
                                actions.push(Action::DownloadUpdate);
                            }
                        } else if widgets::button(ui, "Open Download Page", None, Variant::Primary, true, true).clicked() {
                            actions.push(Action::OpenUrl(release.page_url.clone()));
                        }
                        if installed
                            && widgets::button(ui, "View on GitHub", None, Variant::Secondary, true, true).clicked()
                        {
                            actions.push(Action::OpenUrl(release.page_url.clone()));
                        }
                        if widgets::button(ui, "Later", None, Variant::Ghost, true, true).clicked() {
                            actions.push(Action::DismissUpdate);
                        }
                    }
                    Phase::Downloading { .. } => {
                        if widgets::button(ui, "Cancel", None, Variant::Secondary, true, true).clicked() {
                            actions.push(Action::CancelUpdate);
                        }
                    }
                    Phase::Installing(_) => {}
                    Phase::Failed { .. } => {
                        if widgets::button(ui, "Try Again", None, Variant::Primary, true, true).clicked() {
                            actions.push(Action::RetryUpdate);
                        }
                        if widgets::button(ui, "Open Releases Page", None, Variant::Secondary, true, true).clicked() {
                            actions.push(Action::OpenUrl(update::RELEASES_PAGE.into()));
                        }
                        if widgets::button(ui, "Close", None, Variant::Ghost, true, true).clicked() {
                            actions.push(Action::DismissUpdate);
                        }
                    }
                    Phase::NoRelease => {
                        if widgets::button(ui, "Close", None, Variant::Primary, true, true).clicked() {
                            actions.push(Action::DismissUpdate);
                        }
                        if widgets::button(ui, "Open Releases Page", None, Variant::Secondary, true, true).clicked() {
                            actions.push(Action::OpenUrl(update::RELEASES_PAGE.into()));
                        }
                    }
                    Phase::Idle | Phase::Checking | Phase::UpToDate(_) => {
                        if widgets::button(ui, "Close", None, Variant::Primary, true, true).clicked() {
                            actions.push(Action::DismissUpdate);
                        }
                    }
                }
            });
        });
    });
    let locked = matches!(updater.phase, Phase::Downloading { .. } | Phase::Installing(_));
    if modal.should_close() && !locked {
        actions.push(Action::DismissUpdate);
    }
}

#[cfg(test)]
mod screenshots {
    use std::path::PathBuf;

    use egui_kittest::Harness;

    use super::*;
    use crate::demo;

    /// Stand-ins for GitHub's social preview cards, so card view renders without the network:
    /// a pale card with placeholder text bars, an avatar dot, and the language strip, or a dark
    /// gradient for repositories with a custom image.
    fn demo_previews(app: &mut GitDaddyApp) {
        let dir = std::env::temp_dir().join(format!("gitdaddy-previews-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create preview dir");
        let hub = &mut app.hub;
        for (index, repo) in hub.explore.items.iter_mut().chain(hub.starred.items.iter_mut()).enumerate() {
            let accent = repo.language.as_ref().and_then(|(_, c)| u32::from_str_radix(c, 16).ok()).unwrap_or(0x8b949e);
            let [r, g, b] = [(accent >> 16) as u8, (accent >> 8) as u8, accent as u8];
            let custom = index % 3 == 1;
            let image = image::RgbImage::from_fn(600, 300, |x, y| {
                let bar = |x0: u32, y0: u32, w: u32, h: u32| x >= x0 && x < x0 + w && y >= y0 && y < y0 + h;
                let (dx, dy) = (x as i32 - 520, y as i32 - 70);
                if y >= 288 {
                    image::Rgb([r, g, b])
                } else if custom {
                    let t = (x + y) as f32 / 900.0;
                    let mix = |base: f32, c: u8| (base + t * f32::from(c) * 0.7) as u8;
                    image::Rgb([mix(18.0, r), mix(22.0, g), mix(38.0, b)])
                } else if dx * dx + dy * dy < 40 * 40 {
                    image::Rgb([r, g, b])
                } else if bar(30, 40, 230, 24) || bar(30, 74, 330, 30) {
                    image::Rgb([36, 41, 47])
                } else if bar(30, 130, 400, 10) || bar(30, 148, 300, 10) {
                    image::Rgb([140, 149, 159])
                } else if bar(30, 228, 50, 14) || bar(110, 228, 50, 14) || bar(190, 228, 50, 14) {
                    image::Rgb([175, 184, 193])
                } else {
                    image::Rgb([255, 255, 255])
                }
            });
            let file = dir.join(format!("preview-{index}.png"));
            image.save(&file).expect("save preview");
            repo.image_url = Some(format!("file:///{}", file.to_string_lossy().replace('\\', "/")));
        }
    }

    fn output_dir() -> PathBuf {
        std::env::var_os("GITDADDY_SCREENSHOT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("screenshots"))
    }

    fn render(name: &str, theme_choice: ThemeChoice, with_repos: bool, setup: impl FnOnce(&mut GitDaddyApp)) {
        let mut harness =
            Harness::builder().with_size(vec2(1360.0, 800.0)).with_pixels_per_point(2.0).wgpu().build_eframe(|cc| {
                let settings = Settings {
                    scan_folders: if with_repos { demo::folders() } else { Vec::new() },
                    theme: theme_choice,
                    ..Settings::default()
                };
                let mut app = GitDaddyApp::with_settings(cc, settings, false);
                if with_repos {
                    app.store.repos = demo::repositories();
                    app.store.status_banner = "Check complete • 2 repositories have updates available.".into();
                }
                app
            });
        setup(harness.state_mut());
        harness.run_steps(4);
        // Let background image loads (README images, card previews) finish first.
        for _ in 0..100 {
            if !harness.ctx.has_pending_images() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            harness.step();
        }
        harness.run_steps(2);
        let image = harness.render().expect("render screenshot");
        let dir = output_dir();
        std::fs::create_dir_all(&dir).expect("create screenshot dir");
        image.save(dir.join(format!("{name}.png"))).expect("save screenshot");
    }

    /// Renders the inspector for a real repository (README images included) over many frames,
    /// which is where layout feedback loops such as a growing side panel show up.
    #[test]
    #[ignore = "set GITDADDY_DEBUG_REPO to a repository path; run with `cargo test real_repo_inspector -- --ignored`"]
    fn real_repo_inspector() {
        let path = std::env::var("GITDADDY_DEBUG_REPO").expect("GITDADDY_DEBUG_REPO");
        let mut harness =
            Harness::builder().with_size(vec2(1280.0, 800.0)).with_pixels_per_point(1.0).wgpu().build_eframe(|cc| {
                let settings = Settings { theme: ThemeChoice::Dark, ..Settings::default() };
                let mut app = GitDaddyApp::with_settings(cc, settings, false);
                let repo = GitRepo::new(util::folder_name(&path), path.clone());
                app.store.repos = vec![git::inspect_local_repository(&repo)];
                app.store.focused = Some(path.clone());
                app
            });
        for _ in 0..60 {
            harness.step();
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        let inspector_width = egui::containers::panel::PanelState::load(&harness.ctx, Id::new("inspector-panel"))
            .map(|s| s.outer_rect.width());
        eprintln!("inspector panel rect width: {inspector_width:?}");
        let image = harness.render().expect("render");
        let dir = output_dir();
        std::fs::create_dir_all(&dir).expect("create screenshot dir");
        image.save(dir.join("real-repo-inspector.png")).expect("save");
    }

    /// The live feed against real Hacker News and Mastodon posts.
    #[test]
    #[ignore = "reads Hacker News, Mastodon, and GitHub; run with `cargo test live_mentions_feed -- --ignored`"]
    fn live_mentions_feed() {
        let mut harness =
            Harness::builder().with_size(vec2(1360.0, 800.0)).with_pixels_per_point(2.0).wgpu().build_eframe(|cc| {
                let settings = Settings { theme: ThemeChoice::Dark, ..Settings::default() };
                let mut app = GitDaddyApp::with_settings(cc, settings, false);
                app.hub.live_fill_mentions();
                app.hub.live_selected = app.hub.live.items.first().map(|m| m.id.clone());
                app.view.section = Some(Section::Explore);
                app
            });
        let started = std::time::Instant::now();
        harness.run_steps(4);
        while started.elapsed().as_secs() < 20 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            harness.step();
            let readme_done =
                harness.state().hub.readmes.values().all(|r| !matches!(r, crate::hub::ReadmeState::Loading));
            if readme_done && !harness.state().hub.readmes.is_empty() {
                break;
            }
        }
        harness.run_steps(4);
        let image = harness.render().expect("render");
        let dir = output_dir();
        std::fs::create_dir_all(&dir).expect("create screenshot dir");
        image.save(dir.join("live-mentions.png")).expect("save");
    }

    /// Card view against this week's real Explore results and GitHub's real preview images.
    #[test]
    #[ignore = "reads GitHub through gh and downloads preview images; run with `cargo test live_explore_cards -- --ignored`"]
    fn live_explore_cards() {
        let mut harness =
            Harness::builder().with_size(vec2(1360.0, 800.0)).with_pixels_per_point(2.0).wgpu().build_eframe(|cc| {
                let settings = Settings { theme: ThemeChoice::Dark, ..Settings::default() };
                let mut app = GitDaddyApp::with_settings(cc, settings, false);
                app.hub.live_fill_explore();
                app.store.settings.repo_layout = crate::hub::RepoLayout::Cards;
                app.store.settings.show_inspector = false;
                app.view.section = Some(Section::Explore);
                app
            });
        let started = std::time::Instant::now();
        harness.run_steps(4);
        while harness.ctx.has_pending_images() && started.elapsed().as_secs() < 30 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            harness.step();
        }
        harness.run_steps(2);
        eprintln!("previews settled after {:.1}s", started.elapsed().as_secs_f32());
        let image = harness.render().expect("render");
        let dir = output_dir();
        std::fs::create_dir_all(&dir).expect("create screenshot dir");
        image.save(dir.join("live-explore-cards.png")).expect("save");
    }

    /// Width of the inspector panel after rendering a README for many frames.
    fn inspector_width_for(readme: &str, setup_dir: impl FnOnce(&std::path::Path)) -> f32 {
        let dir = std::env::temp_dir().join(format!("gitdaddy-probe-{}-{}", std::process::id(), readme.len()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README.md"), readme).unwrap();
        setup_dir(&dir);
        let path = dir.to_string_lossy().into_owned();
        let mut harness =
            Harness::builder().with_size(vec2(1280.0, 800.0)).with_pixels_per_point(1.0).build_eframe(|cc| {
                let mut app = GitDaddyApp::with_settings(cc, Settings::default(), false);
                let mut repo = GitRepo::new("probe".into(), path.clone());
                repo.readme_file_name = Some("README.md".into());
                repo.readme_content = Some(readme.to_string());
                app.store.repos = vec![repo];
                app.store.focused = Some(path.clone());
                app
            });
        for _ in 0..40 {
            harness.step();
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        let width = egui::containers::panel::PanelState::load(&harness.ctx, Id::new("inspector-panel"))
            .map_or(0.0, |s| s.outer_rect.width());
        let _ = std::fs::remove_dir_all(&dir);
        width
    }

    /// The inspector must keep its default width whatever the README contains.
    #[test]
    fn readme_content_never_widens_the_inspector() {
        let long_line = format!(
            "```sh\ncurl -fsSL https://example.com/{}/install.sh | bash\n```",
            "very-long-path-segment".repeat(6)
        );
        let table = "| File type | Display |\n| --- | --- |\n| Markdown (`.md`, `.mdx`, `.markdown`, `.txt`) | Rich text editor with frontmatter support, cover images, and wiki-style links |";
        let long_word = format!("See https://example.com/{}", "x".repeat(220));
        let cases: Vec<(&str, String)> = vec![
            ("plain", "# Title\n\nShort text.".into()),
            ("image", "![shot](big.png)".into()),
            ("code", long_line),
            ("table", table.into()),
            ("long word", long_word),
        ];
        let mut failures = Vec::new();
        for (name, readme) in &cases {
            let width = inspector_width_for(readme, |dir| {
                image::RgbaImage::from_pixel(3000, 1200, image::Rgba([40, 80, 120, 255]))
                    .save(dir.join("big.png"))
                    .unwrap();
            });
            eprintln!("{name:>10}: inspector width {width}");
            if (width - INSPECTOR_WIDTH).abs() > 1.0 {
                failures.push(format!("{name}: {width}"));
            }
        }
        assert!(failures.is_empty(), "inspector widened: {failures:?}");
    }

    #[test]
    #[ignore = "renders README screenshots; run with `cargo test -- --ignored`"]
    fn readme_screenshots() {
        render("screenshot-windows-dashboard", ThemeChoice::Dark, true, |app| {
            let first = app.store.repos[0].path.clone();
            app.store.focused = Some(first.clone());
            app.store.selected.insert(first);
        });
        render("screenshot-windows-inspector", ThemeChoice::Dark, true, |app| {
            let cli = app.store.repos[1].path.clone();
            let auth = app.store.repos[2].path.clone();
            app.store.focused = Some(cli.clone());
            app.store.selected.extend([cli, auth]);
            app.view.readme_expanded = false;
        });
        render("screenshot-windows-light", ThemeChoice::Light, true, |app| {
            app.store.focused = Some(app.store.repos[0].path.clone());
        });
    }

    #[test]
    #[ignore = "renders dialogs for visual review; run with `cargo test -- --ignored`"]
    fn dialog_screenshots() {
        fn sample_release() -> update::Release {
            update::Release {
                version: update::Version::parse("1.2.0").unwrap(),
                tag: "v1.2.0".into(),
                notes: "## What's new\n\n- **Check for Updates** in the header and the ☰ menu.\n- Faster scans of large workspace folders.\n- Fixed the README viewer cutting off code blocks.".into(),
                page_url: update::RELEASES_PAGE.into(),
                published: "2026-11-02".into(),
                installer: update::Asset { name: "GitDaddy-1.2.0-windows-x64-setup.exe".into(), url: String::new(), size: 7_173_716 },
                checksums: None,
            }
        }
        render("dialog-update-available", ThemeChoice::Dark, true, |app| {
            app.updater.installed = true;
            app.updater.phase = Phase::Available(sample_release());
            app.updater.show_dialog = true;
        });
        render("dialog-update-downloading", ThemeChoice::Light, true, |app| {
            app.updater.installed = true;
            let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(3_100_000));
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            app.updater.phase = Phase::Downloading { release: sample_release(), progress, cancel };
            app.updater.show_dialog = true;
        });
        render("dialog-update-current", ThemeChoice::Dark, true, |app| {
            let mut latest = sample_release();
            latest.version = update::Version::current();
            latest.tag = format!("v{}", latest.version);
            app.updater.phase = Phase::UpToDate(Some(latest));
            app.updater.show_dialog = true;
        });
        render("header-update-chip", ThemeChoice::Dark, true, |app| {
            app.updater.phase = Phase::Available(sample_release());
        });
        render("scoped-owner", ThemeChoice::Dark, true, |app| {
            app.view.scope = Scope::Owner("github.com/acme-corp".into());
            app.view.filter = RepoFilter::NeedsUpdate;
        });
        let fork_scene = |target: ForkTarget, phase: Option<ForkPhase>, stage: ForkStage| {
            move |app: &mut GitDaddyApp| {
                let docs = app.store.repos.iter().find(|r| r.name == "docs-portal").unwrap().clone();
                app.store.focused = Some(docs.path.clone());
                app.fork = Some(ForkFlow::preview(&docs, app.store.egui_ctx(), target, phase, stage));
            }
        };
        let plan = github::ForkPlan { fork_remote: "origin".into(), original_remote: "upstream".into() };
        let ready = |orgs: Vec<String>| Some(ForkPhase::Ready { login: "jordan-dev".into(), orgs, plan: plan.clone() });
        render(
            "fork-ready",
            ThemeChoice::Dark,
            true,
            fork_scene(ForkTarget::GitHub, ready(vec!["acme-corp".into()]), ForkStage::Choosing),
        );
        render(
            "fork-ready-light",
            ThemeChoice::Light,
            true,
            fork_scene(ForkTarget::GitHub, ready(Vec::new()), ForkStage::Choosing),
        );
        render(
            "fork-signin",
            ThemeChoice::Dark,
            true,
            fork_scene(ForkTarget::GitHub, Some(ForkPhase::NeedsSignIn), ForkStage::Choosing),
        );
        render(
            "fork-done",
            ThemeChoice::Dark,
            true,
            fork_scene(
                ForkTarget::GitHub,
                ready(Vec::new()),
                ForkStage::Done(ForkDone::GitHub { fork: "jordan-dev/docs-portal".into() }),
            ),
        );
        render("fork-local", ThemeChoice::Dark, true, fork_scene(ForkTarget::Local, None, ForkStage::Choosing));
        render(
            "fork-local-done",
            ThemeChoice::Light,
            true,
            fork_scene(
                ForkTarget::Local,
                None,
                ForkStage::Done(ForkDone::Local { path: github::default_local_fork_path(&demo::folders()[1]) }),
            ),
        );
        render("forked-inspector", ThemeChoice::Dark, true, |app| {
            let docs = app.store.repos.iter_mut().find(|r| r.name == "docs-portal").unwrap();
            docs.fork_parent_url = docs.remote_url.take();
            docs.remote_url = Some("git@github.com:jordan-dev/docs-portal.git".into());
            docs.push_remote = Some("origin".into());
            docs.upstream_branch = Some("upstream/main".into());
            app.store.focused = Some(docs.path.clone());
            app.view.readme_expanded = false;
        });
        let hub_scene = |section: Section, selected: Option<&'static str>| {
            move |app: &mut GitDaddyApp| {
                app.hub.demo_fill();
                app.view.section = Some(section);
                if let Some(key) = selected {
                    app.hub.selected.insert(section, key.to_string());
                }
            }
        };
        render(
            "hub-pulls",
            ThemeChoice::Dark,
            true,
            hub_scene(Section::PullRequests, Some("https://github.com/acme-corp/desktop-client/pull/482")),
        );
        render("hub-reviews", ThemeChoice::Light, true, hub_scene(Section::ReviewRequests, None));
        render("hub-explore", ThemeChoice::Dark, true, hub_scene(Section::Explore, Some("pixelforge/photocraft")));
        render("hub-explore-light", ThemeChoice::Light, true, hub_scene(Section::Explore, Some("tinygrid/strata")));
        render("hub-live", ThemeChoice::Dark, true, |app| {
            app.hub.demo_fill();
            app.view.section = Some(Section::Explore);
            app.hub.explore_live = true;
            app.hub.live_selected = Some("hn:1".into());
        });
        render("hub-live-mastodon", ThemeChoice::Light, true, |app| {
            app.hub.demo_fill();
            app.view.section = Some(Section::Explore);
            app.hub.explore_live = true;
            app.hub.live_filter = Some(crate::trending::Platform::Mastodon);
            app.hub.live_selected = Some("mastodon:2".into());
        });
        render("hub-explore-empty", ThemeChoice::Dark, true, |app| {
            app.hub.demo_fill();
            app.view.section = Some(Section::Explore);
            app.hub.explore.items.clear();
            app.hub.history_since = None;
        });
        render("hub-issues", ThemeChoice::Dark, true, hub_scene(Section::Issues, None));
        let cards_scene = |section: Section, selected: Option<&'static str>| {
            move |app: &mut GitDaddyApp| {
                app.hub.demo_fill();
                demo_previews(app);
                app.store.settings.repo_layout = crate::hub::RepoLayout::Cards;
                app.view.section = Some(section);
                if let Some(key) = selected {
                    app.hub.selected.insert(section, key.to_string());
                }
            }
        };
        render(
            "hub-explore-cards",
            ThemeChoice::Dark,
            true,
            cards_scene(Section::Explore, Some("pixelforge/photocraft")),
        );
        render("hub-starred-cards", ThemeChoice::Light, true, cards_scene(Section::Starred, None));
        render("hub-explore-cards-wide", ThemeChoice::Dark, true, |app| {
            cards_scene(Section::Explore, None)(app);
            app.store.settings.show_inspector = false;
        });
        render("hub-signin", ThemeChoice::Dark, true, |app| {
            app.view.section = Some(Section::Starred);
            app.hub.connection = crate::hub::Connection::NeedsSignIn;
        });
        render("hub-clone", ThemeChoice::Dark, true, |app| {
            app.hub.demo_fill();
            app.view.section = Some(Section::Explore);
            app.hub.selected.insert(Section::Explore, "orbital-labs/laya".into());
            let parent = app.store.settings.scan_folders[0].clone();
            app.hub.open_clone("orbital-labs/laya", parent);
        });
        render("onboarding", ThemeChoice::Dark, false, |_| {});
        render("dialog-delete", ThemeChoice::Dark, true, |app| {
            app.store.focused = Some(app.store.repos[1].path.clone());
            app.view.pending_delete = vec![app.store.repos[1].path.clone(), app.store.repos[8].path.clone()];
        });
        render("dialog-activity-log", ThemeChoice::Dark, true, |app| {
            for repo in app.store.repos.clone().iter().take(3) {
                let (succeeded, summary) = if repo.name == "auth-service" {
                    (false, "fatal: could not read from remote repository.")
                } else {
                    (true, "Up to Date")
                };
                app.store.activity_logs.push_front(crate::model::ActivityLogEntry::new(
                    repo,
                    "Check Remote",
                    "git fetch --all --prune --quiet",
                    succeeded,
                    summary,
                    repo.last_command_output.clone(),
                ));
            }
            app.view.show_activity_log = true;
        });
    }
}
