//! The sidebar's GitHub section: pull requests, issues, review requests, starred and trending
//! repositories. GitHub data goes through the GitHub CLI's GraphQL access, so it uses the same
//! sign-in as forking; Explore's rankings and live mentions are built in `trending`.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Days, Months, NaiveDate, Utc};
use serde_json::Value;

use crate::github::{self, CliStatus};
use crate::model::{ActivityLogEntry, GitRepo};
use crate::trending::{self, History, Mention, Momentum, Platform, Window};
use crate::util::first_line;

/// Feeds older than this are refetched when their view is opened again.
const STALE_AFTER: Duration = Duration::from_secs(5 * 60);
const PAGE_SIZE: u32 = 50;
/// Star counts of tracked repositories are recorded this often while GitDaddy runs.
const TRACK_EVERY: Duration = Duration::from_secs(60 * 60);
/// A fresh batch of likely risers joins the tracked set about once a day.
const SEED_EVERY_SECS: i64 = 20 * 60 * 60;
/// Live mentions refresh this often while they are on screen.
const LIVE_REFRESH: Duration = Duration::from_secs(2 * 60);
/// How far back the live feed reaches on Hacker News.
const LIVE_HN_SECS: i64 = 2 * 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    PullRequests,
    Issues,
    ReviewRequests,
    Starred,
    Explore,
}

impl Section {
    pub const ALL: [Self; 5] = [Self::PullRequests, Self::Issues, Self::ReviewRequests, Self::Starred, Self::Explore];

    pub fn label(self) -> &'static str {
        match self {
            Self::PullRequests => "Pull Requests",
            Self::Issues => "Issues",
            Self::ReviewRequests => "Review Requests",
            Self::Starred => "Starred",
            Self::Explore => "Explore",
        }
    }

    pub fn glyph(self) -> &'static str {
        use egui_phosphor::regular as icon;
        match self {
            Self::PullRequests => icon::GIT_PULL_REQUEST,
            Self::Issues => icon::RECORD,
            Self::ReviewRequests => icon::EYE,
            Self::Starred => icon::STAR,
            Self::Explore => icon::COMPASS,
        }
    }

    /// The matching page on github.com.
    pub fn web_url(self) -> &'static str {
        match self {
            Self::PullRequests => "https://github.com/pulls",
            Self::Issues => "https://github.com/issues",
            Self::ReviewRequests => "https://github.com/pulls/review-requested",
            Self::Starred => "https://github.com/stars",
            Self::Explore => "https://github.com/trending",
        }
    }
}

/// How Starred and Explore show repositories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum RepoLayout {
    #[default]
    List,
    Cards,
}

/// How you are involved in a pull request or issue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Involvement {
    Created,
    Assigned,
    Mentioned,
}

impl Involvement {
    pub const ALL: [Self; 3] = [Self::Created, Self::Assigned, Self::Mentioned];

    pub fn label(self) -> &'static str {
        match self {
            Self::Created => "Created",
            Self::Assigned => "Assigned",
            Self::Mentioned => "Mentioned",
        }
    }

    fn qualifier(self) -> &'static str {
        match self {
            Self::Created => "author:@me",
            Self::Assigned => "assignee:@me",
            Self::Mentioned => "mentions:@me",
        }
    }
}

/// Length of an Explore period. Periods are calendar periods in UTC, as on Trendshift.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Span {
    Day,
    Week,
    Month,
    Year,
}

impl Span {
    pub const ALL: [Self; 4] = [Self::Day, Self::Week, Self::Month, Self::Year];

    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "Daily",
            Self::Week => "Weekly",
            Self::Month => "Monthly",
            Self::Year => "Yearly",
        }
    }
}

/// The oldest period Explore can step back to.
fn github_launch() -> NaiveDate {
    NaiveDate::from_ymd_opt(2008, 4, 10).expect("valid date")
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExploreQuery {
    pub span: Span,
    /// 0 is the current period, 1 the one before, and so on.
    pub offset: u32,
    pub language: Option<String>,
}

impl Default for ExploreQuery {
    fn default() -> Self {
        Self { span: Span::Week, offset: 0, language: None }
    }
}

impl ExploreQuery {
    /// Inclusive UTC date range: a day, an ISO week (from Monday), a month, or a year, stepped
    /// back `offset` periods. The current period ends today.
    pub fn range(&self, today: NaiveDate) -> (NaiveDate, NaiveDate) {
        let current = self.offset == 0;
        match self.span {
            Span::Day => {
                let day = today - Days::new(u64::from(self.offset));
                (day, day)
            }
            Span::Week => {
                let monday = today - Days::new(u64::from(today.weekday().num_days_from_monday()));
                let start = monday - Days::new(7 * u64::from(self.offset));
                (start, if current { today } else { start + Days::new(6) })
            }
            Span::Month => {
                let start = today.with_day(1).expect("day 1 exists") - Months::new(self.offset);
                (start, if current { today } else { start + Months::new(1) - Days::new(1) })
            }
            Span::Year => {
                let year = today.year() - self.offset as i32;
                let start = NaiveDate::from_ymd_opt(year, 1, 1).expect("January 1 exists");
                (
                    start,
                    if current { today } else { NaiveDate::from_ymd_opt(year, 12, 31).expect("December 31 exists") },
                )
            }
        }
    }

    /// The period as Unix seconds; a current period runs up to `now`.
    pub fn window(&self, now: DateTime<Utc>) -> Window {
        let (start, end) = self.range(now.date_naive());
        let midnight = |day: NaiveDate| day.and_hms_opt(0, 0, 0).expect("midnight exists").and_utc().timestamp();
        let current = self.offset == 0;
        Window {
            start: midnight(start),
            end: if current { now.timestamp() } else { midnight(end + Days::new(1)) },
            current,
            year: end.year(),
        }
    }

    pub fn can_go_back(&self, today: NaiveDate) -> bool {
        self.range(today).0 > github_launch()
    }

    pub fn period_label(&self, today: NaiveDate) -> String {
        let (start, end) = self.range(today);
        match (self.span, self.offset) {
            (Span::Day, 0) => "Today".into(),
            (Span::Day, 1) => "Yesterday".into(),
            (Span::Day, _) => start.format("%b %-d, %Y").to_string(),
            (Span::Week, 0) => "This week".into(),
            (Span::Week, 1) => "Last week".into(),
            (Span::Week, _) if start.year() == end.year() => {
                format!("{} – {}", start.format("%b %-d"), end.format("%b %-d, %Y"))
            }
            (Span::Week, _) => format!("{} – {}", start.format("%b %-d, %Y"), end.format("%b %-d, %Y")),
            (Span::Month, 0) => "This month".into(),
            (Span::Month, _) => start.format("%B %Y").to_string(),
            (Span::Year, 0) => "This year".into(),
            (Span::Year, _) => start.format("%Y").to_string(),
        }
    }

    /// Identifies the results: the same query on a new day is a different ranking.
    fn key(&self, today: NaiveDate) -> String {
        format!("{:?} {} {:?}", self.span, self.range(today).0, self.language)
    }

    pub fn search_string(&self, today: NaiveDate) -> String {
        let (start, end) = self.range(today);
        let mut q = format!("created:{}..{} sort:stars", start.format("%Y-%m-%d"), end.format("%Y-%m-%d"));
        if let Some(language) = &self.language {
            q.push_str(&format!(" language:\"{language}\""));
        }
        q
    }
}

pub const EXPLORE_LANGUAGES: [&str; 16] = [
    "Rust",
    "TypeScript",
    "Python",
    "Go",
    "JavaScript",
    "Java",
    "C++",
    "C#",
    "C",
    "Swift",
    "Kotlin",
    "Zig",
    "Ruby",
    "PHP",
    "Shell",
    "Dart",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub name: String,
    /// Hex color without `#`, as GitHub returns it.
    pub color: String,
}

/// A pull request or issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub is_pr: bool,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub repo: String,
    pub author: String,
    pub created_at: String,
    pub updated_at: String,
    pub is_draft: bool,
    pub comments: u64,
    pub labels: Vec<Label>,
    /// `APPROVED`, `CHANGES_REQUESTED`, or `REVIEW_REQUIRED`.
    pub review_decision: Option<String>,
    /// `SUCCESS`, `FAILURE`, `ERROR`, `PENDING`, or `EXPECTED`.
    pub checks: Option<String>,
    pub head_ref: Option<String>,
    pub base_ref: Option<String>,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub full_name: String,
    pub description: String,
    pub url: String,
    pub stars: u64,
    pub forks: u64,
    /// Name and hex color.
    pub language: Option<(String, String)>,
    pub topics: Vec<String>,
    pub pushed_at: String,
    pub created_at: String,
    pub is_fork: bool,
    pub starred: bool,
    /// GitHub's social preview card (custom or generated), for card view.
    pub image_url: Option<String>,
    /// Gains in the Explore period this repository was ranked for.
    pub momentum: Option<Momentum>,
}

impl Repo {
    fn counts(&self) -> trending::Counts {
        trending::Counts {
            slug: self.full_name.clone(),
            stars: self.stars,
            forks: self.forks,
            created_at: self.created_at.clone(),
            language: self.language.as_ref().map(|(name, _)| name.clone()),
        }
    }
}

pub struct Feed<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub loading: bool,
    pub error: Option<String>,
    fetched_at: Option<Instant>,
    /// The query the items belong to.
    key: String,
    /// The query currently being fetched.
    pending: Option<String>,
}

impl<T> Default for Feed<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            total: 0,
            loading: false,
            error: None,
            fetched_at: None,
            key: String::new(),
            pending: None,
        }
    }
}

impl<T> Feed<T> {
    fn needs_fetch(&self, key: &str) -> bool {
        self.pending.as_deref() != Some(key)
            && (self.key != key || self.fetched_at.is_none_or(|t| t.elapsed() > STALE_AFTER))
    }

    pub fn has_loaded(&self) -> bool {
        self.fetched_at.is_some()
    }

    fn start(&mut self, key: &str) {
        self.loading = true;
        self.error = None;
        if self.key != key {
            self.items.clear();
        }
        self.pending = Some(key.to_string());
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Connection {
    Unknown,
    Checking,
    NeedsCli,
    NeedsSignIn,
    Ready { login: String },
    Failed(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub pulls: u64,
    pub issues: u64,
    pub reviews: u64,
    pub starred: u64,
}

pub enum ReadmeState {
    Loading,
    Ready(String),
    Missing,
    Failed(String),
}

pub enum CloneStage {
    Choosing,
    Working,
    Done { path: String },
    Failed { message: String },
}

pub struct CloneDialog {
    pub slug: String,
    pub parent: String,
    pub name: String,
    pub stage: CloneStage,
}

impl CloneDialog {
    pub fn destination(&self) -> String {
        Path::new(self.parent.trim()).join(self.name.trim()).to_string_lossy().into_owned()
    }
}

/// Things the store should know about once background work finishes.
pub enum HubEvent {
    Cloned {
        path: String,
        log: ActivityLogEntry,
    },
    /// A pull request was checked out (or failed to) in an existing clone.
    CheckedOut {
        path: String,
        log: ActivityLogEntry,
    },
    Notice(String),
}

enum Msg {
    Status(Result<CliStatus, String>),
    Counts(Result<Counts, String>),
    Items { section: Section, key: String, result: Result<(u64, Vec<Item>), String> },
    Repos { section: Section, key: String, result: Result<(u64, Vec<Repo>), String> },
    Live(Result<(Vec<Mention>, HashMap<String, Repo>), String>),
    Tracked { first_recorded: Option<i64> },
    Readme { slug: String, result: Result<Option<String>, String> },
    Starred { slug: String, starred: bool, result: Result<(), String> },
    Cloned { path: String, log: ActivityLogEntry, result: Result<(), String> },
    CheckedOut { url: String, path: String, log: ActivityLogEntry },
}

pub struct Hub {
    pub connection: Connection,
    pub counts: Option<Counts>,
    pub pulls: Feed<Item>,
    pub issues: Feed<Item>,
    pub reviews: Feed<Item>,
    pub starred: Feed<Repo>,
    pub explore: Feed<Repo>,
    pub pull_filter: Involvement,
    pub issue_filter: Involvement,
    pub explore_query: ExploreQuery,
    /// Selected item per view: an item URL or a repository's `owner/name`.
    pub selected: HashMap<Section, String>,
    /// Text filter for the current list.
    pub search: String,
    pub readmes: HashMap<String, ReadmeState>,
    pub clone: Option<CloneDialog>,
    /// Prepared markdown for bodies and READMEs, keyed by URL or slug, with the source hash.
    pub md_segments: HashMap<String, (u64, Rc<Vec<crate::markdown::Segment>>)>,
    /// Pull requests being checked out, by URL.
    pub checking_out: Vec<String>,
    /// Explore shows live mentions instead of a ranking.
    pub explore_live: bool,
    pub live: Feed<Mention>,
    /// Repositories the live mentions link, by lowercase `owner/name`.
    pub live_repos: HashMap<String, Repo>,
    pub live_filter: Option<Platform>,
    /// Selected mention ID in the live feed.
    pub live_selected: Option<String>,
    /// When GitDaddy's oldest recorded star count was taken, for Explore's caption.
    pub history_since: Option<i64>,
    history: Arc<Mutex<History>>,
    tracking: bool,
    last_tracked: Option<Instant>,
    live_attempted: Option<Instant>,
    /// View to load once the connection check succeeds.
    wanted: Option<Section>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    ctx: egui::Context,
}

impl Hub {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        // Tests and previews never touch the real history file.
        let history = if cfg!(test) { History::default() } else { History::load() };
        Self {
            explore_live: false,
            live: Feed::default(),
            live_repos: HashMap::new(),
            live_filter: None,
            live_selected: None,
            history_since: history.first_recorded(),
            history: Arc::new(Mutex::new(history)),
            tracking: false,
            last_tracked: None,
            live_attempted: None,
            connection: Connection::Unknown,
            counts: None,
            pulls: Feed::default(),
            issues: Feed::default(),
            reviews: Feed::default(),
            starred: Feed::default(),
            explore: Feed::default(),
            pull_filter: Involvement::Created,
            issue_filter: Involvement::Assigned,
            explore_query: ExploreQuery::default(),
            selected: HashMap::new(),
            search: String::new(),
            readmes: HashMap::new(),
            clone: None,
            md_segments: HashMap::new(),
            checking_out: Vec::new(),
            wanted: None,
            tx,
            rx,
            ctx,
        }
    }

    fn spawn(&self, work: impl FnOnce() -> Msg + Send + 'static) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
    }

    pub fn login(&self) -> Option<&str> {
        match &self.connection {
            Connection::Ready { login } => Some(login),
            _ => None,
        }
    }

    /// Checks the GitHub CLI sign-in (again).
    pub fn connect(&mut self) {
        if self.connection == Connection::Checking {
            return;
        }
        self.connection = Connection::Checking;
        self.spawn(|| Msg::Status(github::cli_status()));
    }

    /// Shows a view: connects if needed, then loads its data unless it is fresh.
    pub fn open(&mut self, section: Section) {
        self.search.clear();
        match self.connection {
            Connection::Ready { .. } => self.fetch(section, false),
            Connection::Checking => self.wanted = Some(section),
            _ => {
                self.wanted = Some(section);
                self.connect();
            }
        }
    }

    pub fn refresh(&mut self, section: Section) {
        if self.login().is_some() {
            self.fetch(section, true);
            self.refresh_counts();
        } else {
            self.open(section);
        }
    }

    fn refresh_counts(&mut self) {
        self.spawn(|| Msg::Counts(fetch_counts()));
    }

    fn item_query(&self, section: Section) -> String {
        let (kind, involvement) = match section {
            Section::PullRequests => ("is:pr", self.pull_filter.qualifier()),
            Section::Issues => ("is:issue", self.issue_filter.qualifier()),
            _ => ("is:pr", "review-requested:@me"),
        };
        format!("is:open {kind} {involvement} archived:false sort:updated-desc")
    }

    fn fetch(&mut self, section: Section, force: bool) {
        let today = Utc::now().date_naive();
        match section {
            Section::Explore if self.explore_live => self.fetch_live(force),
            Section::Explore => {
                let key = self.explore_query.key(today);
                if !force && !self.explore.needs_fetch(&key) {
                    return;
                }
                self.explore.start(&key);
                let (query, history) = (self.explore_query.clone(), Arc::clone(&self.history));
                self.spawn(move || {
                    let result = fetch_explore(&query, &history);
                    Msg::Repos { section, key, result }
                });
            }
            Section::PullRequests | Section::Issues | Section::ReviewRequests => {
                let key = self.item_query(section);
                let feed = match section {
                    Section::PullRequests => &mut self.pulls,
                    Section::Issues => &mut self.issues,
                    _ => &mut self.reviews,
                };
                if !force && !feed.needs_fetch(&key) {
                    return;
                }
                feed.start(&key);
                let query = key.clone();
                self.spawn(move || Msg::Items { section, key, result: fetch_items(&query) });
            }
            Section::Starred => {
                let key = "starred".to_string();
                if !force && !self.starred.needs_fetch(&key) {
                    return;
                }
                self.starred.start(&key);
                self.spawn(move || Msg::Repos { section, key, result: fetch_starred() });
            }
        }
    }

    fn fetch_live(&mut self, force: bool) {
        let key = "live";
        if self.live.loading || (!force && !self.live.needs_fetch(key)) {
            return;
        }
        self.live.start(key);
        self.live_attempted = Some(Instant::now());
        let history = Arc::clone(&self.history);
        self.spawn(move || Msg::Live(fetch_live(&history)));
    }

    /// Switches Explore to the live mention feed.
    pub fn show_live(&mut self) {
        self.explore_live = true;
        self.fetch(Section::Explore, false);
    }

    /// Background upkeep, called every frame: records star counts hourly and keeps the live
    /// feed fresh while it is on screen. `local_slugs` lists the user's own clones, which are
    /// always tracked.
    pub fn tick(&mut self, visible: Option<Section>, local_slugs: impl FnOnce() -> Vec<String>) {
        // Previews render fixed sample data; they must not go fetching.
        if self.login().is_none() || cfg!(test) {
            return;
        }
        let since_tracked = self.last_tracked.map(|t| t.elapsed());
        if !self.tracking && since_tracked.is_none_or(|age| age >= TRACK_EVERY) {
            self.tracking = true;
            let mut extra = local_slugs();
            extra.extend(self.starred.items.iter().map(|r| r.full_name.clone()));
            let history = Arc::clone(&self.history);
            self.spawn(move || {
                track(&history, extra);
                Msg::Tracked { first_recorded: history.lock().ok().and_then(|h| h.first_recorded()) }
            });
        }
        let mut next = TRACK_EVERY.saturating_sub(since_tracked.unwrap_or_default());
        if visible == Some(Section::Explore) && self.explore_live {
            let since_live = self.live_attempted.map_or(LIVE_REFRESH, |t| t.elapsed());
            if since_live >= LIVE_REFRESH {
                self.fetch_live(true);
            }
            next = next.min(LIVE_REFRESH.saturating_sub(since_live));
        }
        self.ctx.request_repaint_after(next.max(Duration::from_secs(1)));
    }

    pub fn set_involvement(&mut self, section: Section, involvement: Involvement) {
        match section {
            Section::PullRequests => self.pull_filter = involvement,
            Section::Issues => self.issue_filter = involvement,
            _ => return,
        }
        self.fetch(section, false);
    }

    pub fn set_explore(&mut self, query: ExploreQuery) {
        self.explore_live = false;
        self.explore_query = query;
        self.fetch(Section::Explore, false);
    }

    pub fn ensure_readme(&mut self, slug: &str) {
        if self.readmes.contains_key(slug) {
            return;
        }
        self.readmes.insert(slug.to_string(), ReadmeState::Loading);
        let slug = slug.to_string();
        self.spawn(move || {
            let result = fetch_readme(&slug);
            Msg::Readme { slug, result }
        });
    }

    pub fn set_star(&mut self, slug: &str, star: bool) {
        // Optimistic: flip it now, undo if GitHub refuses.
        self.mark_starred(slug, star);
        let slug = slug.to_string();
        self.spawn(move || {
            let path = format!("user/starred/{slug}");
            let res =
                github::run_gh(&["api", "-X", if star { "PUT" } else { "DELETE" }, &path], &std::env::temp_dir(), 30);
            let result = if res.succeeded() { Ok(()) } else { Err(github_error(&res.combined_output())) };
            Msg::Starred { slug, starred: star, result }
        });
    }

    fn mark_starred(&mut self, slug: &str, starred: bool) {
        let mut changed = false;
        let feeds = self.starred.items.iter_mut().chain(self.explore.items.iter_mut());
        for repo in feeds.chain(self.live_repos.values_mut()) {
            if repo.full_name.eq_ignore_ascii_case(slug) && repo.starred != starred {
                repo.starred = starred;
                changed = true;
            }
        }
        if changed && let Some(counts) = &mut self.counts {
            counts.starred = if starred { counts.starred + 1 } else { counts.starred.saturating_sub(1) };
        }
    }

    pub fn open_clone(&mut self, slug: &str, parent: String) {
        let name = slug.split('/').nth(1).unwrap_or(slug).to_string();
        self.clone = Some(CloneDialog { slug: slug.to_string(), parent, name, stage: CloneStage::Choosing });
    }

    pub fn start_clone(&mut self) {
        let Some(dialog) = &mut self.clone else { return };
        let dest = dialog.destination();
        if github::local_path_problem(&dest).is_some() {
            return;
        }
        dialog.stage = CloneStage::Working;
        let slug = dialog.slug.clone();
        self.spawn(move || {
            let parent = Path::new(&dest).parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
            let res = github::run_gh(&["repo", "clone", &slug, &dest], &parent, 900);
            let result = if res.succeeded() { Ok(()) } else { Err(github_error(&res.combined_output())) };
            if result.is_err() {
                let _ = crate::platform::delete_permanently(&dest);
            }
            let placeholder = GitRepo::new(slug.clone(), dest.clone());
            let summary = match &result {
                Ok(()) => format!("Cloned to {}", crate::util::abbreviate_home(&dest)),
                Err(message) => message.clone(),
            };
            let output = format!("[$ {}]\n{}", res.command_string, res.combined_output());
            let log = ActivityLogEntry::new(
                &placeholder,
                "Clone",
                res.command_string.clone(),
                result.is_ok(),
                summary,
                output,
            );
            Msg::Cloned { path: dest, log, result }
        });
    }

    pub fn checkout_pr(&mut self, item: &Item, repo: &GitRepo) {
        if self.checking_out.contains(&item.url) {
            return;
        }
        self.checking_out.push(item.url.clone());
        let (number, url, repo) = (item.number, item.url.clone(), repo.clone());
        self.spawn(move || {
            let res = github::run_gh(&["pr", "checkout", &url], Path::new(&repo.path), 120);
            let summary = if res.succeeded() {
                format!("Checked out pull request #{number}")
            } else {
                github_error(&res.combined_output())
            };
            let output = format!("[$ {}]\n{}", res.command_string, res.combined_output());
            let log = ActivityLogEntry::new(
                &repo,
                "Check Out PR",
                res.command_string.clone(),
                res.succeeded(),
                summary,
                output,
            );
            Msg::CheckedOut { url, path: repo.path.clone(), log }
        });
    }

    /// Applies background results. Returns events for the repository store.
    pub fn poll(&mut self) -> Vec<HubEvent> {
        let mut events = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Status(status) => {
                    self.connection = match status {
                        Ok(CliStatus::Ready { login, .. }) => Connection::Ready { login },
                        Ok(CliStatus::NotInstalled) => Connection::NeedsCli,
                        Ok(CliStatus::SignedOut) => Connection::NeedsSignIn,
                        Err(message) => Connection::Failed(message),
                    };
                    if self.login().is_some() {
                        self.refresh_counts();
                        if let Some(section) = self.wanted.take() {
                            self.fetch(section, false);
                        }
                    }
                }
                Msg::Counts(Ok(counts)) => self.counts = Some(counts),
                Msg::Counts(Err(_)) => {}
                Msg::Items { section, key, result } => {
                    let feed = match section {
                        Section::PullRequests => &mut self.pulls,
                        Section::Issues => &mut self.issues,
                        _ => &mut self.reviews,
                    };
                    apply(feed, key, result);
                }
                Msg::Repos { section, key, result } => {
                    let feed = if section == Section::Starred { &mut self.starred } else { &mut self.explore };
                    apply(feed, key, result);
                    if section == Section::Explore {
                        self.history_since = self.history.lock().ok().and_then(|h| h.first_recorded());
                    }
                }
                Msg::Live(result) => {
                    let result = result.map(|(mentions, repos)| {
                        self.live_repos = repos;
                        (mentions.len() as u64, mentions)
                    });
                    apply(&mut self.live, "live".into(), result);
                }
                Msg::Tracked { first_recorded } => {
                    self.tracking = false;
                    self.last_tracked = Some(Instant::now());
                    self.history_since = first_recorded;
                }
                Msg::Readme { slug, result } => {
                    let state = match result {
                        Ok(Some(text)) => ReadmeState::Ready(text),
                        Ok(None) => ReadmeState::Missing,
                        Err(message) => ReadmeState::Failed(message),
                    };
                    self.readmes.insert(slug, state);
                }
                Msg::Starred { slug, starred, result } => {
                    if let Err(message) = result {
                        self.mark_starred(&slug, !starred);
                        events.push(HubEvent::Notice(format!("Could not update the star on {slug}: {message}")));
                    }
                }
                Msg::Cloned { path, log, result } => {
                    if let Some(dialog) = &mut self.clone {
                        dialog.stage = match &result {
                            Ok(()) => CloneStage::Done { path: path.clone() },
                            Err(message) => CloneStage::Failed { message: message.clone() },
                        };
                    }
                    if result.is_ok() {
                        events.push(HubEvent::Cloned { path, log });
                    } else {
                        events.push(HubEvent::Notice(log.summary.clone()));
                    }
                }
                Msg::CheckedOut { url, path, log } => {
                    self.checking_out.retain(|pending| *pending != url);
                    events.push(HubEvent::CheckedOut { path, log });
                }
            }
        }
        events
    }
}

fn apply<T>(feed: &mut Feed<T>, key: String, result: Result<(u64, Vec<T>), String>) {
    if feed.pending.as_deref() != Some(key.as_str()) {
        // A newer query replaced this one while it was in flight.
        return;
    }
    feed.pending = None;
    feed.loading = false;
    feed.key = key;
    match result {
        Ok((total, items)) => {
            feed.total = total;
            feed.items = items;
            feed.error = None;
            feed.fetched_at = Some(Instant::now());
        }
        Err(message) => feed.error = Some(message),
    }
}

fn github_error(output: &str) -> String {
    let line = first_line(output).unwrap_or_else(|| "GitHub CLI failed.".into());
    if line.contains("HTTP 401") || line.to_lowercase().contains("auth login") {
        "GitHub sign-in expired. Sign in again with the GitHub CLI.".into()
    } else if line.contains("rate limit") {
        "GitHub's rate limit was reached. Try again in a minute.".into()
    } else {
        line
    }
}

// MARK: - Queries

const ITEM_FIELDS: &str = "
  __typename
  ... on PullRequest {
    number title url isDraft createdAt updatedAt body
    author { login } repository { nameWithOwner } comments { totalCount }
    labels(first: 6) { nodes { name color } }
    reviewDecision headRefName baseRefName
    commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
  }
  ... on Issue {
    number title url createdAt updatedAt body
    author { login } repository { nameWithOwner } comments { totalCount }
    labels(first: 6) { nodes { name color } }
  }";

const REPO_FIELDS: &str = "
  nameWithOwner description url stargazerCount forkCount pushedAt createdAt isFork viewerHasStarred openGraphImageUrl
  primaryLanguage { name color }
  repositoryTopics(first: 6) { nodes { topic { name } } }";

fn graphql(query: &str, variables: &[(&str, &str)]) -> Result<Value, String> {
    let query_arg = format!("query={query}");
    let mut args: Vec<String> = vec!["api".into(), "graphql".into(), "-f".into(), query_arg];
    for (name, value) in variables {
        args.extend(["-f".into(), format!("{name}={value}")]);
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let res = github::run_gh(&arg_refs, &std::env::temp_dir(), 45);
    if !res.succeeded() && res.stdout.trim().is_empty() {
        return Err(github_error(&res.combined_output()));
    }
    let json: Value = serde_json::from_str(&res.stdout).map_err(|e| format!("Unexpected reply from GitHub: {e}"))?;
    if let Some(message) = json["errors"][0]["message"].as_str() {
        return Err(message.to_string());
    }
    Ok(json["data"].clone())
}

fn fetch_counts() -> Result<Counts, String> {
    let query = "query($p: String!, $i: String!, $r: String!) {
        viewer { starredRepositories { totalCount } }
        pulls: search(query: $p, type: ISSUE, first: 1) { issueCount }
        issues: search(query: $i, type: ISSUE, first: 1) { issueCount }
        reviews: search(query: $r, type: ISSUE, first: 1) { issueCount }
    }";
    let data = graphql(
        query,
        &[
            ("p", "is:open is:pr author:@me archived:false"),
            ("i", "is:open is:issue assignee:@me archived:false"),
            ("r", "is:open is:pr review-requested:@me archived:false"),
        ],
    )?;
    Ok(Counts {
        pulls: data["pulls"]["issueCount"].as_u64().unwrap_or(0),
        issues: data["issues"]["issueCount"].as_u64().unwrap_or(0),
        reviews: data["reviews"]["issueCount"].as_u64().unwrap_or(0),
        starred: data["viewer"]["starredRepositories"]["totalCount"].as_u64().unwrap_or(0),
    })
}

fn fetch_items(search: &str) -> Result<(u64, Vec<Item>), String> {
    let query = format!(
        "query($q: String!) {{ search(query: $q, type: ISSUE, first: {PAGE_SIZE}) {{ issueCount nodes {{ {ITEM_FIELDS} }} }} }}"
    );
    let data = graphql(&query, &[("q", search)])?;
    let items = data["search"]["nodes"]
        .as_array()
        .map(|nodes| nodes.iter().filter_map(parse_item).collect())
        .unwrap_or_default();
    Ok((data["search"]["issueCount"].as_u64().unwrap_or(0), items))
}

fn fetch_starred() -> Result<(u64, Vec<Repo>), String> {
    let query = format!(
        "query {{ viewer {{ starredRepositories(first: 100, orderBy: {{field: STARRED_AT, direction: DESC}}) {{ totalCount nodes {{ {REPO_FIELDS} }} }} }} }}"
    );
    let data = graphql(&query, &[])?;
    let starred = &data["viewer"]["starredRepositories"];
    let repos =
        starred["nodes"].as_array().map(|nodes| nodes.iter().filter_map(parse_repo).collect()).unwrap_or_default();
    Ok((starred["totalCount"].as_u64().unwrap_or(0), repos))
}

fn fetch_trending(search: &str) -> Result<(u64, Vec<Repo>), String> {
    search_repos(search, PAGE_SIZE)
}

fn search_repos(search: &str, first: u32) -> Result<(u64, Vec<Repo>), String> {
    let query = format!(
        "query($q: String!) {{ search(query: $q, type: REPOSITORY, first: {first}) {{ repositoryCount nodes {{ ... on Repository {{ {REPO_FIELDS} }} }} }} }}"
    );
    let data = graphql(&query, &[("q", search)])?;
    let repos = data["search"]["nodes"]
        .as_array()
        .map(|nodes| nodes.iter().filter_map(parse_repo).collect())
        .unwrap_or_default();
    Ok((data["search"]["repositoryCount"].as_u64().unwrap_or(0), repos))
}

/// Looks up many repositories by `owner/name`, `per_query` at a time. Renamed, deleted, and
/// private repositories are skipped instead of failing the batch.
fn lookup_repos(slugs: &[String], fields: &str, per_query: usize) -> Vec<Repo> {
    let valid: Vec<(&str, &str)> = slugs
        .iter()
        .filter_map(|slug| {
            trending::repo_slug(&format!("https://github.com/{slug}")).filter(|s| s == slug).map(|_| slug)
        })
        .filter_map(|slug| slug.split_once('/'))
        .collect();
    // Each batch is its own gh call; run them side by side.
    thread::scope(|scope| {
        let batches: Vec<_> = valid
            .chunks(per_query)
            .map(|batch| {
                scope.spawn(move || {
                    let body: String = batch
                        .iter()
                        .enumerate()
                        .map(|(i, (owner, name))| {
                            format!("r{i}: repository(owner: \"{owner}\", name: \"{name}\") {{ {fields} }}\n")
                        })
                        .collect();
                    let Ok(data) = graphql_tolerant(&format!("query {{ {body} }}")) else { return Vec::new() };
                    (0..batch.len()).filter_map(|i| parse_repo(&data[format!("r{i}")])).collect::<Vec<_>>()
                })
            })
            .collect();
        batches.into_iter().flat_map(|batch| batch.join().unwrap_or_default()).collect()
    })
}

/// Like `graphql`, but keeps whatever data came back alongside errors (missing repositories).
fn graphql_tolerant(query: &str) -> Result<Value, String> {
    let query_arg = format!("query={query}");
    let res = github::run_gh(&["api", "graphql", "-f", &query_arg], &std::env::temp_dir(), 60);
    let json: Value = serde_json::from_str(&res.stdout).map_err(|_| github_error(&res.combined_output()))?;
    match json.get("data") {
        Some(data) if !data.is_null() => Ok(data.clone()),
        _ => Err(json["errors"][0]["message"]
            .as_str()
            .map_or_else(|| github_error(&res.combined_output()), String::from)),
    }
}

/// Just the numbers the tracker records.
const COUNT_FIELDS: &str = "nameWithOwner stargazerCount forkCount createdAt primaryLanguage { name color }";

/// Explore's ranking for one period: repositories created in it (all their stars count), older
/// ones GitDaddy has recorded counts for, and ones posted to Hacker News in the period.
fn fetch_explore(query: &ExploreQuery, history: &Mutex<History>) -> Result<(u64, Vec<Repo>), String> {
    let now = Utc::now();
    let window = query.window(now);
    let until = (!window.current).then_some(window.end);
    // The search and both mention sources are independent; fetch them at the same time.
    let (search, mut mentions) = thread::scope(|scope| {
        let search = scope.spawn(|| fetch_trending(&query.search_string(now.date_naive())));
        let hacker_news = scope.spawn(|| trending::fetch_hacker_news(window.start, until, 1000).unwrap_or_default());
        // Mastodon's tag timelines only reach back hours, so they only add to current periods.
        let mastodon =
            scope.spawn(|| if window.current { trending::fetch_mastodon().unwrap_or_default() } else { Vec::new() });
        let mut mentions = hacker_news.join().unwrap_or_default();
        mentions.extend(mastodon.join().unwrap_or_default());
        (search.join().unwrap_or_else(|_| Err("Search failed.".into())), mentions)
    });
    let (_, mut repos) = search?;
    let mut seen: HashSet<String> = repos.iter().map(|r| r.full_name.to_lowercase()).collect();
    let risers = history.lock().map(|h| h.risers(&window, query.language.as_deref(), 60)).unwrap_or_default();
    let mut mentioned: HashMap<String, Vec<Platform>> = HashMap::new();
    for mention in &mentions {
        let platforms = mentioned.entry(mention.slug.to_lowercase()).or_default();
        if !platforms.contains(&mention.platform) {
            platforms.push(mention.platform);
        }
    }
    mentions.sort_by_key(|m| std::cmp::Reverse(m.points));
    let wanted: Vec<String> = risers
        .into_iter()
        .chain(mentions.iter().map(|m| m.slug.clone()).take(40))
        .filter(|slug| seen.insert(slug.to_lowercase()))
        .collect();
    let mut extra = lookup_repos(&wanted, REPO_FIELDS, 25);
    if let Some(language) = &query.language {
        extra.retain(|r| r.language.as_ref().is_some_and(|(name, _)| name.eq_ignore_ascii_case(language)));
    }
    repos.extend(extra);

    let mut history = history.lock().map_err(|_| "Star history is unavailable.".to_string())?;
    if window.current {
        let stamp = now.timestamp();
        for repo in &repos {
            history.record(&repo.counts(), stamp);
        }
    }
    let mut ranked: Vec<Repo> = repos
        .into_iter()
        .filter_map(|mut repo| {
            let mut momentum = trending::momentum(&repo.counts(), &history, &window)?;
            momentum.mentioned_on = mentioned.get(&repo.full_name.to_lowercase()).cloned().unwrap_or_default();
            momentum.mentioned_on.sort();
            (momentum.score > 0).then(|| {
                repo.momentum = Some(momentum);
                repo
            })
        })
        .collect();
    if window.current {
        history.save();
    }
    drop(history);
    let score = |r: &Repo| r.momentum.as_ref().map_or(0, |m| m.score);
    ranked.sort_by(|a, b| score(b).cmp(&score(a)).then_with(|| b.stars.cmp(&a.stars)));
    let total = ranked.len() as u64;
    ranked.truncate(PAGE_SIZE as usize);
    Ok((total, ranked))
}

/// Recent posts linking GitHub repositories, and those repositories.
fn fetch_live(history: &Mutex<History>) -> Result<(Vec<Mention>, HashMap<String, Repo>), String> {
    let now = trending::unix_now();
    let (hacker_news, mastodon) = thread::scope(|scope| {
        let hacker_news = scope.spawn(|| trending::fetch_hacker_news(now - LIVE_HN_SECS, None, 100));
        let mastodon = scope.spawn(trending::fetch_mastodon);
        let failed = || Err("The request thread stopped.".to_string());
        (hacker_news.join().unwrap_or_else(|_| failed()), mastodon.join().unwrap_or_else(|_| failed()))
    });
    if let (Err(err), Err(_)) = (&hacker_news, &mastodon) {
        return Err(format!("Couldn't reach Hacker News or Mastodon: {err}"));
    }
    let all: Vec<Mention> = hacker_news.unwrap_or_default().into_iter().chain(mastodon.unwrap_or_default()).collect();
    let mut mentions = trending::merge_mentions(all, 120);
    let mut slugs: Vec<String> = Vec::new();
    for mention in &mentions {
        if !slugs.iter().any(|s| s.eq_ignore_ascii_case(&mention.slug)) {
            slugs.push(mention.slug.clone());
        }
    }
    slugs.truncate(80);
    let repos: HashMap<String, Repo> =
        lookup_repos(&slugs, REPO_FIELDS, 25).into_iter().map(|r| (r.full_name.to_lowercase(), r)).collect();
    if !repos.is_empty() {
        // Drop posts about repositories GitHub doesn't know (deleted, private, or mistyped).
        mentions.retain(|m| repos.contains_key(&m.slug.to_lowercase()));
        if let Ok(mut history) = history.lock() {
            for repo in repos.values() {
                history.record(&repo.counts(), now);
            }
            history.save();
        }
    }
    Ok((mentions, repos))
}

/// Records star counts for every tracked repository, plus a daily batch of likely risers:
/// active popular repositories and the month's most-starred new ones.
fn track(history: &Mutex<History>, extra: Vec<String>) {
    let now = trending::unix_now();
    let mut found: Vec<trending::Counts> = Vec::new();
    let seed_due = history.lock().is_ok_and(|h| now - h.last_seeded > SEED_EVERY_SECS);
    if seed_due {
        let today = Utc::now().date_naive();
        for search in [
            format!("stars:>1000 pushed:>={} sort:stars", today - Days::new(7)),
            format!("created:>={} sort:stars", today - Days::new(30)),
        ] {
            if let Ok((_, repos)) = search_repos(&search, 100) {
                found.extend(repos.iter().map(Repo::counts));
            }
        }
    }
    let mut seen: HashSet<String> = found.iter().map(|c| c.slug.to_lowercase()).collect();
    let tracked = history.lock().map(|h| h.tracked(600)).unwrap_or_default();
    let wanted: Vec<String> =
        tracked.into_iter().chain(extra).filter(|slug| seen.insert(slug.to_lowercase())).collect();
    found.extend(lookup_repos(&wanted, COUNT_FIELDS, 60).iter().map(Repo::counts));
    if let Ok(mut history) = history.lock() {
        for counts in &found {
            history.record(counts, now);
        }
        if seed_due {
            history.last_seeded = now;
        }
        history.compact(now);
        history.save();
    }
}

/// The README as raw markdown, or `None` when the repository has none.
fn fetch_readme(slug: &str) -> Result<Option<String>, String> {
    let path = format!("repos/{slug}/readme");
    let res = github::run_gh(&["api", "-H", "Accept: application/vnd.github.raw", &path], &std::env::temp_dir(), 30);
    if res.succeeded() {
        let mut text = res.stdout;
        if text.len() > 48 * 1024 {
            let cut = (0..=48 * 1024).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
            text.truncate(cut);
        }
        return Ok(Some(text));
    }
    if res.combined_output().contains("404") {
        return Ok(None);
    }
    Err(github_error(&res.combined_output()))
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}

fn parse_item(node: &Value) -> Option<Item> {
    let is_pr = match node["__typename"].as_str()? {
        "PullRequest" => true,
        "Issue" => false,
        _ => return None,
    };
    let labels = node["labels"]["nodes"]
        .as_array()
        .map(|labels| labels.iter().map(|l| Label { name: text(&l["name"]), color: text(&l["color"]) }).collect())
        .unwrap_or_default();
    Some(Item {
        is_pr,
        number: node["number"].as_u64()?,
        title: text(&node["title"]),
        url: text(&node["url"]),
        repo: text(&node["repository"]["nameWithOwner"]),
        author: node["author"]["login"].as_str().unwrap_or("ghost").to_string(),
        created_at: text(&node["createdAt"]),
        updated_at: text(&node["updatedAt"]),
        is_draft: node["isDraft"].as_bool().unwrap_or(false),
        comments: node["comments"]["totalCount"].as_u64().unwrap_or(0),
        labels,
        review_decision: node["reviewDecision"].as_str().map(String::from),
        checks: node["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"].as_str().map(String::from),
        head_ref: node["headRefName"].as_str().map(String::from),
        base_ref: node["baseRefName"].as_str().map(String::from),
        body: text(&node["body"]),
    })
}

fn parse_repo(node: &Value) -> Option<Repo> {
    let full_name = node["nameWithOwner"].as_str()?.to_string();
    let language = node["primaryLanguage"]["name"].as_str().map(|name| {
        (name.to_string(), node["primaryLanguage"]["color"].as_str().unwrap_or("").trim_start_matches('#').to_string())
    });
    let topics = node["repositoryTopics"]["nodes"]
        .as_array()
        .map(|t| t.iter().filter_map(|n| n["topic"]["name"].as_str().map(String::from)).collect())
        .unwrap_or_default();
    Some(Repo {
        full_name,
        description: text(&node["description"]),
        url: text(&node["url"]),
        stars: node["stargazerCount"].as_u64().unwrap_or(0),
        forks: node["forkCount"].as_u64().unwrap_or(0),
        language,
        topics,
        pushed_at: text(&node["pushedAt"]),
        created_at: text(&node["createdAt"]),
        is_fork: node["isFork"].as_bool().unwrap_or(false),
        starred: node["viewerHasStarred"].as_bool().unwrap_or(false),
        image_url: node["openGraphImageUrl"].as_str().filter(|u| u.starts_with("https://")).map(String::from),
        momentum: None,
    })
}

#[cfg(test)]
impl Hub {
    /// Fills Explore with this week's real ranking (no recorded history), for rendering against
    /// live data.
    pub fn live_fill_explore(&mut self) {
        let (total, items) = fetch_explore(&ExploreQuery::default(), &self.history).expect("trending");
        self.connection = Connection::Ready { login: "you".into() };
        self.explore = Feed { items, total, fetched_at: Some(Instant::now()), ..Feed::default() };
    }

    /// Fills the live feed with real mentions, for rendering against live data.
    pub fn live_fill_mentions(&mut self) {
        let (mentions, repos) = fetch_live(&self.history).expect("mentions");
        self.connection = Connection::Ready { login: "you".into() };
        self.live_repos = repos;
        self.live =
            Feed { total: mentions.len() as u64, items: mentions, fetched_at: Some(Instant::now()), ..Feed::default() };
        self.explore_live = true;
    }

    /// Fills every view with fictional sample data, for rendering previews.
    pub fn demo_fill(&mut self) {
        let now = chrono::Utc::now();
        let ago = |hours: i64| (now - chrono::Duration::hours(hours)).to_rfc3339();
        let label = |name: &str, color: &str| Label { name: name.into(), color: color.into() };
        let item = |number: u64, title: &str, repo: &str, author: &str, hours: i64| Item {
            is_pr: true,
            number,
            title: title.into(),
            url: format!("https://github.com/{repo}/pull/{number}"),
            repo: repo.into(),
            author: author.into(),
            created_at: ago(hours * 3),
            updated_at: ago(hours),
            is_draft: false,
            comments: 0,
            labels: Vec::new(),
            review_decision: None,
            checks: None,
            head_ref: Some("feature".into()),
            base_ref: Some("main".into()),
            body: String::new(),
        };
        let mut pulls = vec![
            Item {
                comments: 4,
                labels: vec![label("enhancement", "a2eeef"), label("ui", "7057ff")],
                review_decision: Some("APPROVED".into()),
                checks: Some("SUCCESS".into()),
                head_ref: Some("feat/offline-queue".into()),
                body: "## Summary\n\nPersist mutations locally and replay them when the connection returns.\n\n- Adds `OfflineQueue` with a SQLite store\n- Retries with exponential backoff\n- Covers the reconnect path with tests\n\n| Case | Before | After |\n| --- | --- | --- |\n| Flaky Wi-Fi | Lost edits | Replayed |".into(),
                ..item(482, "Replay queued mutations after reconnect", "acme-corp/desktop-client", "jordan-dev", 2)
            },
            Item {
                is_draft: true,
                checks: Some("PENDING".into()),
                ..item(77, "Draft: JSON reporter for `ct report`", "jordan-dev/cli-toolkit", "jordan-dev", 20)
            },
            Item {
                comments: 9,
                review_decision: Some("CHANGES_REQUESTED".into()),
                checks: Some("FAILURE".into()),
                labels: vec![label("bug", "d73a4a")],
                ..item(1203, "Fix webhook retries dropping the signature header", "opensource-guild/docs-portal", "jordan-dev", 30)
            },
        ];
        pulls[1].comments = 1;
        let reviews = vec![
            Item {
                review_decision: Some("REVIEW_REQUIRED".into()),
                checks: Some("SUCCESS".into()),
                comments: 2,
                ..item(519, "Support partial refunds in invoice export", "acme-corp/billing-worker", "ana-souza", 5)
            },
            Item {
                checks: Some("SUCCESS".into()),
                ..item(88, "Bump wgpu to 27", "acme-corp/desktop-client", "mia-chen", 9)
            },
        ];
        let issues = vec![
            Item {
                is_pr: false,
                url: "https://github.com/acme-corp/api-gateway/issues/311".into(),
                labels: vec![label("bug", "d73a4a"), label("p1", "b60205")],
                comments: 6,
                head_ref: None,
                base_ref: None,
                ..item(
                    311,
                    "Gateway returns 502 when upstream closes keep-alive early",
                    "acme-corp/api-gateway",
                    "ops-bot",
                    3,
                )
            },
            Item {
                is_pr: false,
                url: "https://github.com/opensource-guild/docs-portal/issues/96".into(),
                head_ref: None,
                base_ref: None,
                ..item(96, "Dark mode code blocks have low contrast", "opensource-guild/docs-portal", "lena-k", 50)
            },
        ];
        let repo = |name: &str, description: &str, stars: u64, language: (&str, &str), hours: i64| Repo {
            full_name: name.into(),
            description: description.into(),
            url: format!("https://github.com/{name}"),
            stars,
            forks: stars / 12,
            language: Some((language.0.into(), language.1.into())),
            topics: vec!["developer-tools".into(), "open-source".into()],
            pushed_at: ago(hours),
            created_at: ago(hours * 2),
            is_fork: false,
            starred: false,
            image_url: None,
            momentum: None,
        };
        let year = now.year();
        let gained = |stars: u64, forks: u64, basis: trending::Basis, on: &[Platform], new: bool| {
            Some(Momentum {
                stars_gained: stars,
                forks_gained: forks,
                score: trending::score(stars, forks),
                basis,
                mentioned_on: on.to_vec(),
                new_in: new.then_some(year),
            })
        };
        let recorded = trending::Basis::Recorded { since: now.timestamp() - 9 * 86_400, complete: true };
        let partial = trending::Basis::Recorded { since: now.timestamp() - 2 * 86_400, complete: false };
        let fresh = trending::Basis::NewInPeriod;
        let (hn, masto) = (Platform::HackerNews, Platform::Mastodon);
        let explore = vec![
            Repo {
                starred: true,
                momentum: gained(12_383, 835, fresh, &[hn, masto], true),
                ..repo(
                    "pixelforge/photocraft",
                    "An open-source photo editor written from scratch in Rust.",
                    67_240,
                    ("Rust", "dea584"),
                    3,
                )
            },
            Repo {
                momentum: gained(4_218, 143, recorded, &[hn], false),
                ..repo(
                    "orbital-labs/laya",
                    "A typed decision engine for agents with scored, explainable choices.",
                    33_356,
                    ("Python", "3572A5"),
                    500,
                )
            },
            Repo {
                momentum: gained(3_704, 261, fresh, &[], true),
                ..repo(
                    "streamline/anydoc",
                    "Convert Word, PowerPoint, Excel, EPUB, and PDF to clean Markdown.",
                    22_938,
                    ("TypeScript", "3178c6"),
                    7,
                )
            },
            Repo {
                momentum: gained(2_063, 47, partial, &[masto], false),
                ..repo(
                    "tinygrid/strata",
                    "Run large models on consumer hardware with a one-click installer.",
                    27_353,
                    ("C++", "f34b7d"),
                    900,
                )
            },
            Repo {
                momentum: gained(1_696, 56, fresh, &[hn], true),
                ..repo(
                    "quietkeys/handwired",
                    "Turn ordinary JavaScript into a trainable neural net.",
                    1_715,
                    ("JavaScript", "f1e05a"),
                    30,
                )
            },
            Repo {
                momentum: gained(702, 32, recorded, &[], false),
                ..repo(
                    "netmerge/net-merge",
                    "Use every network adapter at once, with per-connection balancing.",
                    1_058,
                    ("Go", "00ADD8"),
                    4_000,
                )
            },
        ];
        let mention =
            |platform: Platform, id: &str, slug: &str, title: &str, body: &str, author: &str, minutes: i64| Mention {
                platform,
                id: id.into(),
                slug: slug.into(),
                title: title.into(),
                text: body.into(),
                author: author.into(),
                link: format!("https://example.com/{id}"),
                posted_at: (now - chrono::Duration::minutes(minutes)).to_rfc3339(),
                points: (minutes as u64 * 7) % 90 + 3,
                comments: (minutes as u64 * 3) % 40,
            };
        let mentions = vec![
            mention(
                hn,
                "hn:1",
                "pixelforge/photocraft",
                "Show HN: PhotoCraft – a photo editor written from scratch in Rust",
                "",
                "mira_k",
                4,
            ),
            mention(
                masto,
                "mastodon:2",
                "tinygrid/strata",
                "Ran a 70B model on my gaming laptop with tinygrid/strata.",
                "Ran a 70B model on my gaming laptop with tinygrid/strata. One installer, no Python setup. Wild.\n\n#github #llm",
                "@devon@fosstodon.org",
                18,
            ),
            mention(
                hn,
                "hn:3",
                "quietkeys/handwired",
                "Handwired: Turn ordinary JavaScript into a trainable neural net",
                "",
                "lowlevel",
                52,
            ),
            mention(
                masto,
                "mastodon:4",
                "orbital-labs/laya",
                "Laya scores every choice an agent makes and explains why.",
                "Laya scores every choice an agent makes and explains why. Finally something I can debug.\n\n#github #ai",
                "@sam",
                95,
            ),
            mention(
                hn,
                "hn:5",
                "streamline/anydoc",
                "Anydoc converts Office files and PDFs to clean Markdown",
                "",
                "tpk",
                180,
            ),
        ];
        self.live_repos =
            explore.iter().map(|r| (r.full_name.to_lowercase(), Repo { momentum: None, ..r.clone() })).collect();
        self.live =
            Feed { total: mentions.len() as u64, items: mentions, fetched_at: Some(Instant::now()), ..Feed::default() };
        self.history_since = Some(now.timestamp() - 9 * 86_400);
        let starred = vec![
            Repo {
                starred: true,
                ..repo(
                    "acme-corp/design-system",
                    "Tokens, components, and guidelines for Acme apps.",
                    412,
                    ("TypeScript", "3178c6"),
                    20,
                )
            },
            Repo {
                starred: true,
                ..repo(
                    "pixelforge/photocraft",
                    "An open-source photo editor written from scratch in Rust.",
                    67_240,
                    ("Rust", "dea584"),
                    3,
                )
            },
        ];
        let fill = |feed: &mut Feed<_>, items, total| {
            feed.items = items;
            feed.total = total;
            feed.fetched_at = Some(Instant::now());
        };
        fill(&mut self.pulls, pulls, 3);
        fill(&mut self.reviews, reviews, 2);
        fill(&mut self.issues, issues, 2);
        self.explore = Feed { items: explore, total: 6, fetched_at: Some(Instant::now()), ..Feed::default() };
        self.starred = Feed { items: starred, total: 2, fetched_at: Some(Instant::now()), ..Feed::default() };
        self.counts = Some(Counts { pulls: 3, issues: 2, reviews: 2, starred: 2 });
        self.connection = Connection::Ready { login: "jordan-dev".into() };
        self.readmes.insert(
            "pixelforge/photocraft".into(),
            ReadmeState::Ready("# PhotoCraft\n\nA fast, non-destructive photo editor.\n\n![screenshot](docs/shot.png)\n\n## Features\n\n- Layers and masks\n- RAW support for 600+ cameras\n- GPU-accelerated filters\n\n```sh\ncargo install photocraft\n```".into()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn explore_periods() {
        // A Sunday: Trendshift's week for it started on Monday, October 12.
        let today = day(2026, 10, 18);
        let week = ExploreQuery { span: Span::Week, offset: 0, language: None };
        assert_eq!(week.range(today), (day(2026, 10, 12), today));
        assert_eq!(week.period_label(today), "This week");
        let last_week = ExploreQuery { offset: 1, ..week.clone() };
        assert_eq!(last_week.range(today), (day(2026, 10, 5), day(2026, 10, 11)));
        assert_eq!(last_week.period_label(today), "Last week");
        let earlier = ExploreQuery { offset: 2, ..week.clone() };
        assert_eq!(earlier.period_label(today), "Sep 28 – Oct 4, 2026");
        // On a Monday the week has just begun.
        assert_eq!(week.range(day(2026, 10, 19)), (day(2026, 10, 19), day(2026, 10, 19)));

        let month = ExploreQuery { span: Span::Month, offset: 0, language: Some("Rust".into()) };
        assert_eq!(month.range(today), (day(2026, 10, 1), today));
        assert_eq!(month.search_string(today), "created:2026-10-01..2026-10-18 sort:stars language:\"Rust\"");
        let march = ExploreQuery { span: Span::Month, offset: 7, language: None };
        assert_eq!(march.range(today), (day(2026, 3, 1), day(2026, 3, 31)));
        assert_eq!(march.period_label(today), "March 2026");

        let year = ExploreQuery { span: Span::Year, offset: 0, language: None };
        assert_eq!(year.range(today), (day(2026, 1, 1), today));
        let last_year = ExploreQuery { offset: 1, ..year.clone() };
        assert_eq!(last_year.range(today), (day(2025, 1, 1), day(2025, 12, 31)));
        assert_eq!(last_year.period_label(today), "2025");
        assert!(ExploreQuery { offset: 17, ..year.clone() }.can_go_back(today));
        assert!(!ExploreQuery { offset: 18, ..year.clone() }.can_go_back(today));

        let yesterday = ExploreQuery { span: Span::Day, offset: 1, language: None };
        assert_eq!(yesterday.range(today), (day(2026, 10, 17), day(2026, 10, 17)));
        assert!(!ExploreQuery { span: Span::Month, offset: 222, language: None }.can_go_back(today));
    }

    #[test]
    fn explore_windows_are_utc() {
        let now = DateTime::parse_from_rfc3339("2026-10-18T19:00:00Z").unwrap().with_timezone(&Utc);
        let midnight = |iso: &str| DateTime::parse_from_rfc3339(iso).unwrap().timestamp();
        let today = ExploreQuery { span: Span::Day, offset: 0, language: None }.window(now);
        assert_eq!(
            today,
            Window { start: midnight("2026-10-18T00:00:00Z"), end: now.timestamp(), current: true, year: 2026 }
        );
        let last_week = ExploreQuery { span: Span::Week, offset: 1, language: None }.window(now);
        assert_eq!(
            last_week,
            Window {
                start: midnight("2026-10-05T00:00:00Z"),
                end: midnight("2026-10-12T00:00:00Z"),
                current: false,
                year: 2026
            }
        );
    }

    #[test]
    fn parses_items_and_repos() {
        let pr: Value = serde_json::from_str(
            r#"{"__typename":"PullRequest","number":7,"title":"Add fork","url":"https://github.com/a/b/pull/7","isDraft":true,
                "createdAt":"2026-10-01T00:00:00Z","updatedAt":"2026-10-02T00:00:00Z","body":"Hi","author":{"login":"me"},
                "repository":{"nameWithOwner":"a/b"},"comments":{"totalCount":3},"labels":{"nodes":[{"name":"bug","color":"d73a4a"}]},
                "reviewDecision":"APPROVED","headRefName":"feat","baseRefName":"main",
                "commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"SUCCESS"}}}]}}"#,
        )
        .unwrap();
        let item = parse_item(&pr).unwrap();
        assert!(item.is_pr && item.is_draft);
        assert_eq!((item.number, item.repo.as_str(), item.comments), (7, "a/b", 3));
        assert_eq!(item.checks.as_deref(), Some("SUCCESS"));
        assert_eq!(item.labels[0].name, "bug");

        let repo: Value = serde_json::from_str(
            r##"{"nameWithOwner":"o/r","description":null,"url":"https://github.com/o/r","stargazerCount":12,"forkCount":1,
                "pushedAt":"","createdAt":"","isFork":false,"viewerHasStarred":true,
                "primaryLanguage":{"name":"Rust","color":"#dea584"},"repositoryTopics":{"nodes":[{"topic":{"name":"cli"}}]}}"##,
        )
        .unwrap();
        let repo = parse_repo(&repo).unwrap();
        assert_eq!(repo.language, Some(("Rust".into(), "dea584".into())));
        assert!(repo.starred);
        assert_eq!(repo.topics, ["cli"]);
    }

    #[test]
    #[ignore = "reads your GitHub data through gh (read-only)"]
    fn live_queries() {
        eprintln!("counts: {:?}", fetch_counts());
        let (total, items) = fetch_items("is:open is:pr author:@me archived:false").expect("prs");
        eprintln!("my open PRs: {total} ({} loaded)", items.len());
        let (total, repos) = fetch_starred().expect("starred");
        eprintln!("starred: {total}: {:?}", repos.iter().map(|r| &r.full_name).collect::<Vec<_>>());
        let history = Mutex::new(History::default());
        for span in Span::ALL {
            let query = ExploreQuery { span, offset: 0, language: None };
            let (total, repos) = fetch_explore(&query, &history).expect("ranking");
            let top: Vec<_> = repos
                .iter()
                .take(3)
                .map(|r| (&r.full_name, r.momentum.as_ref().map(|m| (m.stars_gained, m.score, m.mentioned_on.clone()))))
                .collect();
            eprintln!("{span:?}: {total} ranked, top {top:?}");
        }
        eprintln!("readme bytes: {:?}", fetch_readme("Duyfrom/GitDaddy").map(|r| r.map(|t| t.len())));
    }
}
