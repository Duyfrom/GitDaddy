//! The sidebar's GitHub section: pull requests, issues, review requests, starred and trending
//! repositories. Everything goes through the GitHub CLI's GraphQL access, so it uses the same
//! sign-in as forking.

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, Months, NaiveDate};
use serde_json::Value;

use crate::github::{self, CliStatus};
use crate::model::{ActivityLogEntry, GitRepo};
use crate::util::first_line;

/// Feeds older than this are refetched when their view is opened again.
const STALE_AFTER: Duration = Duration::from_secs(5 * 60);
const PAGE_SIZE: u32 = 50;

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

/// Length of an Explore period.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Span {
    Day,
    Week,
    Month,
}

impl Span {
    pub const ALL: [Self; 3] = [Self::Day, Self::Week, Self::Month];

    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "Today",
            Self::Week => "This week",
            Self::Month => "This month",
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
    /// Inclusive date range. Days and weeks roll back from `today`; months are calendar months.
    pub fn range(&self, today: NaiveDate) -> (NaiveDate, NaiveDate) {
        match self.span {
            Span::Day => {
                let day = today - chrono::Days::new(u64::from(self.offset));
                (day, day)
            }
            Span::Week => {
                let end = today - chrono::Days::new(7 * u64::from(self.offset));
                (end - chrono::Days::new(6), end)
            }
            Span::Month => {
                let first_of_this_month = today.with_day(1).expect("day 1 exists");
                let start = first_of_this_month - Months::new(self.offset);
                let end = if self.offset == 0 { today } else { start + Months::new(1) - chrono::Days::new(1) };
                (start, end)
            }
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
            (Span::Week, 0) => "Past 7 days".into(),
            (Span::Week, _) if start.year() == end.year() => {
                format!("{} – {}", start.format("%b %-d"), end.format("%b %-d, %Y"))
            }
            (Span::Week, _) => format!("{} – {}", start.format("%b %-d, %Y"), end.format("%b %-d, %Y")),
            (Span::Month, 0) => format!("{} so far", start.format("%B %Y")),
            (Span::Month, _) => start.format("%B %Y").to_string(),
        }
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
    /// View to load once the connection check succeeds.
    wanted: Option<Section>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    ctx: egui::Context,
}

impl Hub {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
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
        let today = Local::now().date_naive();
        match section {
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
            Section::Starred | Section::Explore => {
                let key = if section == Section::Starred {
                    "starred".to_string()
                } else {
                    self.explore_query.search_string(today)
                };
                let feed = if section == Section::Starred { &mut self.starred } else { &mut self.explore };
                if !force && !feed.needs_fetch(&key) {
                    return;
                }
                feed.start(&key);
                let query = key.clone();
                self.spawn(move || {
                    let result = if section == Section::Starred { fetch_starred() } else { fetch_trending(&query) };
                    Msg::Repos { section, key, result }
                });
            }
        }
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
        for repo in self.starred.items.iter_mut().chain(self.explore.items.iter_mut()) {
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
    let query = format!(
        "query($q: String!) {{ search(query: $q, type: REPOSITORY, first: {PAGE_SIZE}) {{ repositoryCount nodes {{ ... on Repository {{ {REPO_FIELDS} }} }} }} }}"
    );
    let data = graphql(&query, &[("q", search)])?;
    let repos = data["search"]["nodes"]
        .as_array()
        .map(|nodes| nodes.iter().filter_map(parse_repo).collect())
        .unwrap_or_default();
    Ok((data["search"]["repositoryCount"].as_u64().unwrap_or(0), repos))
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
    })
}

#[cfg(test)]
impl Hub {
    /// Fills every view with fictional sample data, for rendering previews.
    /// Fills Explore with this week's real results, for rendering against live data.
    pub fn live_fill_explore(&mut self) {
        let search = ExploreQuery::default().search_string(Local::now().date_naive());
        let (total, items) = fetch_trending(&search).expect("trending");
        self.connection = Connection::Ready { login: "you".into() };
        self.explore = Feed { items, total, fetched_at: Some(Instant::now()), ..Feed::default() };
    }

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
        };
        let explore = vec![
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
            repo(
                "orbital-labs/laya",
                "A typed decision engine for agents with scored, explainable choices.",
                33_356,
                ("Python", "3572A5"),
                5,
            ),
            repo(
                "streamline/anydoc",
                "Convert Word, PowerPoint, Excel, EPUB, and PDF to clean Markdown.",
                22_938,
                ("TypeScript", "3178c6"),
                7,
            ),
            repo(
                "tinygrid/strata",
                "Run large models on consumer hardware with a one-click installer.",
                27_353,
                ("C++", "f34b7d"),
                2,
            ),
            repo(
                "quietkeys/handwired",
                "Turn ordinary JavaScript into a trainable neural net.",
                1_715,
                ("JavaScript", "f1e05a"),
                30,
            ),
            repo(
                "netmerge/net-merge",
                "Use every network adapter at once, with per-connection balancing.",
                1_058,
                ("Go", "00ADD8"),
                40,
            ),
        ];
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
        self.explore = Feed { items: explore, total: 2_031_680, fetched_at: Some(Instant::now()), ..Feed::default() };
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
        let today = day(2026, 10, 19);
        let week = ExploreQuery { span: Span::Week, offset: 0, language: None };
        assert_eq!(week.range(today), (day(2026, 10, 13), day(2026, 10, 19)));
        assert_eq!(week.period_label(today), "Past 7 days");
        let last_week = ExploreQuery { offset: 1, ..week.clone() };
        assert_eq!(last_week.range(today), (day(2026, 10, 6), day(2026, 10, 12)));
        assert_eq!(last_week.period_label(today), "Oct 6 – Oct 12, 2026");

        let month = ExploreQuery { span: Span::Month, offset: 0, language: Some("Rust".into()) };
        assert_eq!(month.range(today), (day(2026, 10, 1), today));
        assert_eq!(month.search_string(today), "created:2026-10-01..2026-10-19 sort:stars language:\"Rust\"");
        let march = ExploreQuery { span: Span::Month, offset: 7, language: None };
        assert_eq!(march.range(today), (day(2026, 3, 1), day(2026, 3, 31)));
        assert_eq!(march.period_label(today), "March 2026");

        let yesterday = ExploreQuery { span: Span::Day, offset: 1, language: None };
        assert_eq!(yesterday.range(today), (day(2026, 10, 18), day(2026, 10, 18)));
        assert!(!ExploreQuery { span: Span::Month, offset: 222, language: None }.can_go_back(today));
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
        let today = Local::now().date_naive();
        let (total, repos) = fetch_trending(&ExploreQuery::default().search_string(today)).expect("trending");
        eprintln!("trending this week: {total} matches, top: {:?}", repos.first().map(|r| (&r.full_name, r.stars)));
        eprintln!("readme bytes: {:?}", fetch_readme("Duyfrom/GitDaddy").map(|r| r.map(|t| t.len())));
    }
}
