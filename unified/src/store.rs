use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};

use crate::git;
use crate::model::{ActivityLogEntry, GitRepo, PullStrategy, RemoteOwner, RepoFilter, RepoSortOrder, SyncState};
use crate::platform;
use crate::scanner::{self, DiscoveredRepo};
use crate::util::{folder_name, natural_cmp, plural};

const MAX_LOG_ENTRIES: usize = 250;
const NO_FOLDERS_BANNER: &str = "Select a workspace folder to start scanning Git repositories.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub scan_folders: Vec<String>,
    pub scan_nested_repos: bool,
    pub pull_strategy: PullStrategy,
    pub auto_stash_on_pull: bool,
    pub show_inspector: bool,
    pub theme: ThemeChoice,
    pub check_updates_automatically: bool,
    /// List or card view for Starred and Explore.
    pub repo_layout: crate::hub::RepoLayout,
    /// Unix time of the last successful update check.
    pub last_update_check: Option<i64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            scan_folders: Vec::new(),
            scan_nested_repos: false,
            pull_strategy: PullStrategy::Merge,
            auto_stash_on_pull: true,
            show_inspector: true,
            theme: ThemeChoice::System,
            check_updates_automatically: true,
            repo_layout: crate::hub::RepoLayout::List,
            last_update_check: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JobKind {
    Inspect { then_check_remote: bool },
    Check,
    Pull,
    Push,
    Commit,
    AddRemote,
    Delete { to_recycle_bin: bool },
}

impl JobKind {
    /// Background refreshes of local state do not block the batch buttons.
    fn counts_as_batch(self) -> bool {
        !matches!(self, JobKind::Inspect { .. })
    }
}

struct Job {
    kind: JobKind,
    total: usize,
    completed: usize,
    succeeded: usize,
    failed: usize,
    pending: HashSet<String>,
    last_log: Option<ActivityLogEntry>,
}

enum Outcome {
    Updated(Box<GitRepo>),
    Removed(String),
}

enum WorkerMsg {
    ScanFinished { discovered: Vec<DiscoveredRepo>, check_remote: bool },
    RepoDone { job: u64, path: String, outcome: Outcome, log: Option<ActivityLogEntry> },
    JobFinished { job: u64 },
}

type Operation = Arc<dyn Fn(&GitRepo) -> (Outcome, Option<ActivityLogEntry>) + Send + Sync>;

#[derive(Default, Clone, Copy)]
pub struct Counts {
    pub all: usize,
    pub outdated: usize,
    pub up_to_date: usize,
    pub dirty: usize,
    pub ahead_or_diverged: usize,
    pub unpushed: usize,
    pub issues: usize,
}

/// Which repositories the sidebar narrows the table, header counts, and batch buttons to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    All,
    /// A watched folder, by path.
    Folder(String),
    /// A remote owner, by `RemoteOwner::key`.
    Owner(String),
    NoRemote,
}

impl Scope {
    pub fn contains(&self, repo: &GitRepo) -> bool {
        match self {
            Scope::All => true,
            Scope::Folder(folder) => path_within(&repo.path, folder),
            Scope::Owner(key) => repo.remote_owner().is_some_and(|o| o.key() == *key),
            Scope::NoRemote => !repo.has_remote(),
        }
    }
}

/// True when `path` is `folder` or inside it (case-insensitive, either slash).
pub fn path_within(path: &str, folder: &str) -> bool {
    let normalize = |p: &str| p.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    let (path, folder) = (normalize(path), normalize(folder));
    path == folder || path.strip_prefix(&folder).is_some_and(|rest| rest.starts_with('\\'))
}

impl Counts {
    pub fn for_filter(&self, filter: RepoFilter) -> usize {
        match filter {
            RepoFilter::All => self.all,
            RepoFilter::NeedsUpdate => self.outdated,
            RepoFilter::UpToDate => self.up_to_date,
            RepoFilter::LocalChanges => self.dirty,
            RepoFilter::AheadOrDiverged => self.ahead_or_diverged,
            RepoFilter::Errors => self.issues,
        }
    }
}

pub struct Store {
    pub settings: Settings,
    pub repos: Vec<GitRepo>,
    pub selected: HashSet<String>,
    pub focused: Option<String>,

    pub is_scanning: bool,
    pub status_banner: String,
    pub activity_logs: VecDeque<ActivityLogEntry>,

    jobs: HashMap<u64, Job>,
    next_job_id: u64,
    queued_scan: Option<bool>,
    /// Repository to select once the next scan finds it (a fresh local fork).
    pending_focus: Option<String>,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
    ctx: egui::Context,
}

impl Store {
    pub fn new(ctx: egui::Context, settings: Settings) -> Self {
        let (tx, rx) = mpsc::channel();
        let status_banner =
            if settings.scan_folders.is_empty() { NO_FOLDERS_BANNER.to_string() } else { "Ready".to_string() };
        Self {
            settings,
            repos: Vec::new(),
            selected: HashSet::new(),
            focused: None,
            is_scanning: false,
            status_banner,
            activity_logs: VecDeque::new(),
            jobs: HashMap::new(),
            next_job_id: 1,
            queued_scan: None,
            pending_focus: None,
            tx,
            rx,
            ctx,
        }
    }

    #[cfg(test)]
    pub fn egui_ctx(&self) -> egui::Context {
        self.ctx.clone()
    }

    // MARK: Derived state

    pub fn counts(&self) -> Counts {
        self.counts_in(&Scope::All)
    }

    /// Status counts for the repositories inside `scope`.
    pub fn counts_in(&self, scope: &Scope) -> Counts {
        let mut c = Counts::default();
        for repo in self.repos.iter().filter(|r| scope.contains(r)) {
            c.all += 1;
            let s = &repo.sync_state;
            c.outdated += usize::from(s.needs_update());
            c.up_to_date += usize::from(*s == SyncState::UpToDate);
            c.dirty += usize::from(repo.is_dirty());
            c.ahead_or_diverged += usize::from(s.is_ahead_or_diverged());
            c.unpushed += usize::from(s.can_push());
            c.issues += usize::from(s.is_issue_or_no_upstream());
        }
        c
    }

    /// Indices into `repos` that pass the filter and search, in display order.
    pub fn filtered_indices(&self, scope: &Scope, filter: RepoFilter, sort: RepoSortOrder, search: &str) -> Vec<usize> {
        let query = search.trim().to_lowercase();
        let mut matching: Vec<usize> = (0..self.repos.len())
            .filter(|&i| {
                let repo = &self.repos[i];
                scope.contains(repo)
                    && filter.matches(repo)
                    && (query.is_empty()
                        || repo.name.to_lowercase().contains(&query)
                        || repo.path.to_lowercase().contains(&query)
                        || repo.branch.to_lowercase().contains(&query)
                        || repo.remote_url.as_deref().is_some_and(|u| u.to_lowercase().contains(&query)))
            })
            .collect();
        matching.sort_by(|&a, &b| sort.compare(&self.repos[a], &self.repos[b]));
        matching
    }

    /// (repositories, repositories with updates to pull) inside `scope`.
    pub fn scope_summary(&self, scope: &Scope) -> (usize, usize) {
        self.repos
            .iter()
            .filter(|r| scope.contains(r))
            .fold((0, 0), |(n, behind), r| (n + 1, behind + usize::from(r.sync_state.needs_update())))
    }

    /// Remote owners with their repository count and how many need updates, largest first.
    pub fn owners(&self) -> Vec<(RemoteOwner, usize, usize)> {
        let mut by_owner: HashMap<RemoteOwner, (usize, usize)> = HashMap::new();
        for repo in &self.repos {
            if let Some(owner) = repo.remote_owner() {
                let entry = by_owner.entry(owner).or_default();
                entry.0 += 1;
                entry.1 += usize::from(repo.sync_state.needs_update());
            }
        }
        let mut owners: Vec<_> = by_owner.into_iter().map(|(o, (n, behind))| (o, n, behind)).collect();
        owners.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| natural_cmp(&a.0.name, &b.0.name)));
        owners
    }

    pub fn repo(&self, path: &str) -> Option<&GitRepo> {
        self.repos.iter().find(|r| r.path == path)
    }

    pub fn focused_repo(&self) -> Option<&GitRepo> {
        self.focused.as_deref().and_then(|p| self.repo(p))
    }

    pub fn is_batch_running(&self) -> bool {
        self.jobs.values().any(|j| j.kind.counts_as_batch())
    }

    /// (completed, total) across running batch jobs.
    pub fn batch_progress(&self) -> (usize, usize) {
        self.jobs
            .values()
            .filter(|j| j.kind.counts_as_batch())
            .fold((0, 0), |(done, total), j| (done + j.completed, total + j.total))
    }

    pub fn busy(&self) -> bool {
        self.is_scanning || self.is_batch_running()
    }

    // MARK: Folder management

    pub fn prompt_to_add_folder(&mut self) {
        let Some(folders) = rfd::FileDialog::new().set_title("Add Folder or Git Repository to Scan").pick_folders()
        else {
            return;
        };
        let mut added_any = false;
        for folder in folders {
            let path = folder.to_string_lossy().into_owned();
            if !self.settings.scan_folders.iter().any(|f| f.eq_ignore_ascii_case(&path)) {
                self.settings.scan_folders.push(path);
                added_any = true;
            }
        }
        if added_any {
            self.scan_directories(true);
        }
    }

    pub fn remove_scan_folder(&mut self, folder: &str) {
        self.settings.scan_folders.retain(|f| f != folder);
        self.scan_directories(false);
    }

    // MARK: Selection

    pub fn toggle_selection(&mut self, path: &str) {
        if !self.selected.remove(path) {
            self.selected.insert(path.to_string());
        }
    }

    pub fn select_paths(&mut self, paths: impl IntoIterator<Item = String>) {
        self.selected.extend(paths);
    }

    pub fn select_all_outdated(&mut self, scope: &Scope) {
        self.selected = self.paths_where(|r| scope.contains(r) && r.sync_state.needs_update());
    }

    pub fn select_all_unpushed(&mut self, scope: &Scope) {
        self.selected = self.paths_where(|r| scope.contains(r) && r.sync_state.can_push());
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
    }

    fn paths_where(&self, predicate: impl Fn(&GitRepo) -> bool) -> HashSet<String> {
        self.repos.iter().filter(|r| predicate(r)).map(|r| r.path.clone()).collect()
    }

    pub fn selected_paths(&self) -> Vec<String> {
        self.selected.iter().cloned().collect()
    }

    // MARK: Scanning

    pub fn scan_directories(&mut self, check_remote_after_scan: bool) {
        if self.is_scanning {
            // Remember the request and run it once the current scan lands.
            self.queued_scan = Some(self.queued_scan.unwrap_or(false) || check_remote_after_scan);
            return;
        }
        if self.settings.scan_folders.is_empty() {
            self.repos.clear();
            self.selected.clear();
            self.focused = None;
            self.status_banner = NO_FOLDERS_BANNER.into();
            return;
        }
        self.is_scanning = true;
        self.status_banner = "Scanning folders for Git repositories…".into();

        let folders = self.settings.scan_folders.clone();
        let nested = self.settings.scan_nested_repos;
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let discovered = scanner::discover_repositories(&folders, scanner::MAX_DEPTH, nested);
            let _ = tx.send(WorkerMsg::ScanFinished { discovered, check_remote: check_remote_after_scan });
            ctx.request_repaint();
        });
    }

    fn finish_scan(&mut self, discovered: Vec<DiscoveredRepo>, check_remote: bool) {
        let mut existing: HashMap<String, GitRepo> = self.repos.drain(..).map(|r| (r.path.clone(), r)).collect();
        self.repos = discovered
            .into_iter()
            .map(|d| {
                existing.remove(&d.repo_path).unwrap_or_else(|| GitRepo::new(folder_name(&d.repo_path), d.repo_path))
            })
            .collect();

        let known: HashSet<&str> = self.repos.iter().map(|r| r.path.as_str()).collect();
        self.selected.retain(|p| known.contains(p.as_str()));
        if self.focused.as_deref().is_none_or(|p| !known.contains(p)) {
            self.focused = self.repos.first().map(|r| r.path.clone());
        }
        if let Some(path) = self.pending_focus.take_if(|p| known.contains(p.as_str())) {
            self.focused = Some(path);
        }
        self.is_scanning = false;

        let n = self.repos.len();
        self.status_banner =
            format!("Detected {n} Git {}. Reading local state…", plural(n, "repository", "repositories"));

        if let Some(queued_check) = self.queued_scan.take() {
            self.scan_directories(queued_check || check_remote);
            return;
        }

        let targets = self.repos.clone();
        if targets.is_empty() {
            self.status_banner = "Detected 0 Git repositories.".into();
            return;
        }
        self.spawn_job(
            JobKind::Inspect { then_check_remote: check_remote },
            targets,
            8,
            Arc::new(|repo| (Outcome::Updated(Box::new(git::inspect_local_repository(repo))), None)),
        );
    }

    // MARK: Git operations

    fn targets(&self, paths: &[String], extra: impl Fn(&GitRepo) -> bool) -> Vec<GitRepo> {
        self.repos.iter().filter(|r| paths.contains(&r.path) && !r.sync_state.is_busy() && extra(r)).cloned().collect()
    }

    fn mark(&mut self, targets: &[GitRepo], state: SyncState) {
        let paths: HashSet<&str> = targets.iter().map(|t| t.path.as_str()).collect();
        for repo in self.repos.iter_mut().filter(|r| paths.contains(r.path.as_str())) {
            repo.sync_state = state.clone();
        }
    }

    pub fn check_repos(&mut self, paths: &[String]) {
        let targets = self.targets(paths, |_| true);
        if targets.is_empty() {
            return;
        }
        self.mark(&targets, SyncState::Checking);
        self.status_banner = format!(
            "Checking remote updates for {} {}…",
            targets.len(),
            plural(targets.len(), "repository", "repositories")
        );
        self.spawn_job(
            JobKind::Check,
            targets,
            6,
            Arc::new(|repo| {
                let (updated, log) = git::check_remote_status(repo);
                (Outcome::Updated(Box::new(updated)), Some(log))
            }),
        );
    }

    pub fn check_all(&mut self) {
        self.check_scope(&Scope::All);
    }

    pub fn check_scope(&mut self, scope: &Scope) {
        let paths: Vec<String> = self.paths_where(|r| scope.contains(r)).into_iter().collect();
        self.check_repos(&paths);
    }

    pub fn pull_repos(&mut self, paths: &[String]) {
        let targets = self.targets(paths, |_| true);
        if targets.is_empty() {
            return;
        }
        let strategy = self.settings.pull_strategy;
        let auto_stash = self.settings.auto_stash_on_pull;
        self.mark(&targets, SyncState::Pulling);
        self.status_banner =
            format!("Pulling updates for {} {}…", targets.len(), plural(targets.len(), "repository", "repositories"));
        self.spawn_job(
            JobKind::Pull,
            targets,
            4,
            Arc::new(move |repo| {
                let (updated, log) = git::pull_repository(repo, strategy, auto_stash);
                (Outcome::Updated(Box::new(updated)), Some(log))
            }),
        );
    }

    pub fn pull_all_outdated(&mut self, scope: &Scope) {
        let paths: Vec<String> =
            self.paths_where(|r| scope.contains(r) && r.sync_state.needs_update()).into_iter().collect();
        self.pull_repos(&paths);
    }

    pub fn pull_all(&mut self, scope: &Scope) {
        let paths: Vec<String> =
            self.paths_where(|r| scope.contains(r) && r.sync_state.can_pull()).into_iter().collect();
        self.pull_repos(&paths);
    }

    pub fn push_repos(&mut self, paths: &[String]) {
        let targets = self.targets(paths, GitRepo::has_remote);
        if targets.is_empty() {
            return;
        }
        self.mark(&targets, SyncState::Pushing);
        self.status_banner =
            format!("Pushing commits for {} {}…", targets.len(), plural(targets.len(), "repository", "repositories"));
        self.spawn_job(
            JobKind::Push,
            targets,
            4,
            Arc::new(|repo| {
                let (updated, log) = git::push_repository(repo);
                (Outcome::Updated(Box::new(updated)), Some(log))
            }),
        );
    }

    pub fn push_all_unpushed(&mut self, scope: &Scope) {
        let paths: Vec<String> =
            self.paths_where(|r| scope.contains(r) && r.sync_state.can_push()).into_iter().collect();
        self.push_repos(&paths);
    }

    pub fn commit_and_push(&mut self, path: &str, message: &str) {
        let message = message.trim().to_string();
        let targets = self.targets(&[path.to_string()], |_| true);
        let Some(target) = targets.first() else { return };
        if message.is_empty() {
            return;
        }
        self.status_banner = format!("Committing changes in {}…", target.name);
        self.mark(&targets, SyncState::Pushing);
        self.spawn_job(
            JobKind::Commit,
            targets,
            1,
            Arc::new(move |repo| {
                let (updated, log) = git::commit_and_push_repository(repo, &message);
                (Outcome::Updated(Box::new(updated)), Some(log))
            }),
        );
    }

    pub fn add_remote_origin(&mut self, path: &str, url: &str) {
        let url = url.trim().to_string();
        let targets = self.targets(&[path.to_string()], |_| true);
        let Some(target) = targets.first() else { return };
        if url.is_empty() {
            return;
        }
        self.status_banner = format!("Adding remote origin to {}…", target.name);
        self.mark(&targets, SyncState::Checking);
        self.spawn_job(
            JobKind::AddRemote,
            targets,
            1,
            Arc::new(move |repo| {
                let (updated, log) = git::add_remote_origin(&url, repo);
                (Outcome::Updated(Box::new(updated)), Some(log))
            }),
        );
    }

    pub fn delete_repositories(&mut self, paths: &[String], to_recycle_bin: bool) {
        let targets = self.targets(paths, |_| true);
        if targets.is_empty() {
            return;
        }
        self.mark(&targets, SyncState::Deleting);
        self.status_banner =
            format!("Deleting {} local {}…", targets.len(), plural(targets.len(), "repository", "repositories"));
        self.spawn_job(
            JobKind::Delete { to_recycle_bin },
            targets,
            2,
            Arc::new(move |repo| {
                let display = repo.display_path();
                let result = if to_recycle_bin {
                    platform::move_to_recycle_bin(&repo.path)
                } else {
                    platform::delete_permanently(&repo.path)
                };
                match result {
                    Ok(()) => {
                        let log = ActivityLogEntry::new(
                            repo,
                            if to_recycle_bin { platform::TRASH_BUTTON_LABEL } else { "Delete Permanently" },
                            if to_recycle_bin {
                                format!("{} {display}", platform::TRASH_BUTTON_LABEL)
                            } else {
                                format!("Remove {display}")
                            },
                            true,
                            if to_recycle_bin {
                                format!("Moved local repository to {}", platform::TRASH_NOUN)
                            } else {
                                "Permanently deleted local repository".into()
                            },
                            format!("Removed local directory: {}", repo.path),
                        );
                        (Outcome::Removed(repo.path.clone()), Some(log))
                    }
                    Err(err) => {
                        let log = ActivityLogEntry::new(
                            repo,
                            "Delete Repo",
                            format!("Remove {display}"),
                            false,
                            err.clone(),
                            format!("Failed to delete {}: {err}", repo.path),
                        );
                        // Re-read the folder so the row leaves the "Deleting…" state.
                        (Outcome::Updated(Box::new(git::inspect_local_repository(repo))), Some(log))
                    }
                }
            }),
        );
    }

    /// Applies a result produced outside the job queue (forking), with its log entry.
    pub fn apply_repo_update(&mut self, repo: GitRepo, log: ActivityLogEntry) {
        self.status_banner = if log.succeeded {
            format!("{} • {}", log.repo_name, log.summary)
        } else {
            format!("Fork failed for {}: {}", log.repo_name, log.summary)
        };
        if let Some(slot) = self.repos.iter_mut().find(|r| r.path == repo.path) {
            *slot = repo;
        }
        self.activity_logs.push_front(log);
        self.activity_logs.truncate(MAX_LOG_ENTRIES);
    }

    /// Brings a freshly created local fork into the list and selects it.
    pub fn adopt_repository(&mut self, path: &str, log: ActivityLogEntry) {
        self.status_banner = format!("{} • {}", log.repo_name, log.summary);
        self.activity_logs.push_front(log);
        self.activity_logs.truncate(MAX_LOG_ENTRIES);
        if !self.settings.scan_folders.iter().any(|folder| path_within(path, folder)) {
            self.settings.scan_folders.push(path.to_string());
        }
        self.pending_focus = Some(path.to_string());
        self.scan_directories(false);
    }

    /// Logs work done outside the job queue on `path` (a pull request checkout) and re-reads it.
    pub fn log_and_refresh(&mut self, path: &str, log: ActivityLogEntry) {
        self.status_banner = if log.succeeded {
            format!("{} • {}", log.repo_name, log.summary)
        } else {
            format!("{} failed for {}: {}", log.action, log.repo_name, log.summary)
        };
        self.activity_logs.push_front(log);
        self.activity_logs.truncate(MAX_LOG_ENTRIES);
        self.check_repos(&[path.to_string()]);
    }

    pub fn record_log(&mut self, log: ActivityLogEntry) {
        self.status_banner = format!("{} failed for {}: {}", log.action, log.repo_name, log.summary);
        self.activity_logs.push_front(log);
        self.activity_logs.truncate(MAX_LOG_ENTRIES);
    }

    pub fn clear_activity_log(&mut self) {
        self.activity_logs.clear();
    }

    // MARK: Job engine

    fn spawn_job(&mut self, kind: JobKind, targets: Vec<GitRepo>, concurrency: usize, op: Operation) {
        let id = self.next_job_id;
        self.next_job_id += 1;
        self.jobs.insert(
            id,
            Job {
                kind,
                total: targets.len(),
                completed: 0,
                succeeded: 0,
                failed: 0,
                pending: targets.iter().map(|t| t.path.clone()).collect(),
                last_log: None,
            },
        );

        let workers = concurrency.clamp(1, targets.len().max(1));
        let queue = Arc::new(Mutex::new(VecDeque::from(targets)));
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    let queue = Arc::clone(&queue);
                    let op = Arc::clone(&op);
                    let tx = tx.clone();
                    let ctx = ctx.clone();
                    thread::spawn(move || {
                        loop {
                            let next = queue.lock().ok().and_then(|mut q| q.pop_front());
                            let Some(repo) = next else { break };
                            let (outcome, log) = op(&repo);
                            let _ = tx.send(WorkerMsg::RepoDone { job: id, path: repo.path.clone(), outcome, log });
                            ctx.request_repaint();
                        }
                    })
                })
                .collect();
            for handle in handles {
                let _ = handle.join();
            }
            let _ = tx.send(WorkerMsg::JobFinished { job: id });
            ctx.request_repaint();
        });
    }

    /// Applies results from background workers. Call once per frame.
    pub fn process_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                WorkerMsg::ScanFinished { discovered, check_remote } => self.finish_scan(discovered, check_remote),
                WorkerMsg::RepoDone { job, path, outcome, log } => self.apply_result(job, &path, outcome, log),
                WorkerMsg::JobFinished { job } => self.finish_job(job),
            }
        }
    }

    fn apply_result(&mut self, job_id: u64, path: &str, outcome: Outcome, log: Option<ActivityLogEntry>) {
        match outcome {
            Outcome::Updated(updated) => {
                if let Some(slot) = self.repos.iter_mut().find(|r| r.path == updated.path) {
                    *slot = *updated;
                }
            }
            Outcome::Removed(removed) => {
                self.repos.retain(|r| r.path != removed);
                self.selected.remove(&removed);
                self.settings.scan_folders.retain(|f| !f.eq_ignore_ascii_case(&removed));
                if self.focused.as_deref() == Some(removed.as_str()) {
                    self.focused = self.repos.first().map(|r| r.path.clone());
                }
            }
        }

        if let Some(job) = self.jobs.get_mut(&job_id) {
            job.pending.remove(path);
            job.completed += 1;
            match &log {
                Some(entry) if !entry.succeeded => job.failed += 1,
                _ => job.succeeded += 1,
            }
            job.last_log = log.clone();
        }
        if let Some(entry) = log {
            self.activity_logs.push_front(entry);
            self.activity_logs.truncate(MAX_LOG_ENTRIES);
        }
    }

    fn finish_job(&mut self, job_id: u64) {
        let Some(job) = self.jobs.remove(&job_id) else { return };

        // A worker that died mid-operation leaves its repo marked busy; surface that instead.
        for path in &job.pending {
            if let Some(repo) = self.repos.iter_mut().find(|r| &r.path == path)
                && repo.sync_state.is_busy()
            {
                repo.sync_state = SyncState::Error("The operation stopped unexpectedly.".into());
            }
        }

        let (ok, failed) = (job.succeeded, job.failed);
        let repos_word = |n: usize| plural(n, "repository", "repositories");
        match job.kind {
            JobKind::Inspect { then_check_remote } => {
                if then_check_remote && !self.repos.is_empty() {
                    self.check_all();
                } else {
                    let n = self.repos.len();
                    self.status_banner = format!("Detected {n} Git {}.", repos_word(n));
                }
            }
            JobKind::Check => {
                let outdated = self.counts().outdated;
                self.status_banner = if outdated > 0 {
                    format!(
                        "Check complete • {outdated} {} updates available.",
                        plural(outdated, "repository has", "repositories have")
                    )
                } else {
                    "Check complete • All repositories are up to date with their remotes.".into()
                };
            }
            JobKind::Pull | JobKind::Push => {
                let verb = if job.kind == JobKind::Pull { "pulled" } else { "pushed" };
                self.status_banner = if failed == 0 {
                    format!("Successfully {verb} {ok} {}.", repos_word(ok))
                } else {
                    let verb = if job.kind == JobKind::Pull { "Pulled" } else { "Pushed" };
                    format!("{verb} {ok} succeeded, {failed} failed. Check the inspector or activity log for details.")
                };
            }
            JobKind::Commit => {
                if let Some(log) = job.last_log {
                    self.status_banner = if log.succeeded {
                        log.summary
                    } else {
                        format!("Commit failed for {}: {}", log.repo_name, log.summary)
                    };
                }
            }
            JobKind::AddRemote => {
                if let Some(log) = job.last_log {
                    self.status_banner = if log.succeeded {
                        format!("Added remote origin and checked {}.", log.repo_name)
                    } else {
                        format!("Failed to add remote to {}: {}", log.repo_name, log.summary)
                    };
                }
            }
            JobKind::Delete { to_recycle_bin } => {
                self.status_banner = if failed == 0 && to_recycle_bin {
                    format!("Moved {ok} local {} to {}.", repos_word(ok), platform::TRASH_NOUN)
                } else if failed == 0 {
                    format!("Permanently deleted {ok} local {} from disk.", repos_word(ok))
                } else {
                    format!("Deleted {ok}, failed to delete {failed}. Check the activity log.")
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn folder_matching_and_scopes() {
        assert!(path_within(r"D:\Dev\app", r"D:\Dev"));
        assert!(path_within(r"d:\dev\app\", "D:/Dev/"));
        assert!(path_within(r"D:\Dev", r"D:\Dev"));
        assert!(!path_within(r"D:\Developments\app", r"D:\Dev"));

        let mut mine = GitRepo::new("a".into(), r"D:\Dev\a".into());
        mine.remote_url = Some("git@github.com:me/a.git".into());
        let local = GitRepo::new("b".into(), r"E:\Other\b".into());
        assert!(Scope::Folder(r"D:\Dev".into()).contains(&mine));
        assert!(!Scope::Folder(r"D:\Dev".into()).contains(&local));
        assert!(Scope::Owner("github.com/me".into()).contains(&mine));
        assert!(Scope::NoRemote.contains(&local) && !Scope::NoRemote.contains(&mine));
        assert!(Scope::All.contains(&local));
    }

    fn wait_until_idle(store: &mut Store) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            store.process_messages();
            if !store.is_scanning && store.jobs.is_empty() {
                return;
            }
            assert!(Instant::now() < deadline, "store did not settle: {}", store.status_banner);
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn scan_inspect_and_check_through_worker_threads() {
        if git::git_executable().is_none() {
            return;
        }
        let root = std::env::temp_dir().join(format!("gitdaddy-store-{}", std::process::id()));
        let _ = platform::delete_permanently(&root.to_string_lossy());
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_string_lossy().into_owned();
        let run = |args: &[&str], cwd: &str| assert!(git::run_git(args, cwd, 30).succeeded(), "{args:?}");

        let remote = root.join("remote.git").to_string_lossy().into_owned();
        run(&["-c", "init.defaultBranch=main", "init", "--bare", &remote], &root_str);
        let work = root.join("projects");
        std::fs::create_dir_all(&work).unwrap();
        let work_str = work.to_string_lossy().into_owned();
        run(&["clone", &remote, "tracked"], &work_str);
        let tracked = work.join("tracked").to_string_lossy().into_owned();
        run(&["-c", "init.defaultBranch=main", "init", "local-only"], &work_str);
        for repo in [&tracked, &work.join("local-only").to_string_lossy().into_owned()] {
            run(&["config", "user.name", "GitDaddy Test"], repo);
            run(&["config", "user.email", "test@example.com"], repo);
            run(&["config", "commit.gpgsign", "false"], repo);
            std::fs::write(Path::new(repo).join("f.txt"), "x").unwrap();
            run(&["add", "-A"], repo);
            run(&["commit", "-m", "init"], repo);
        }
        run(&["push", "-u", "origin", "HEAD"], &tracked);

        let settings = Settings { scan_folders: vec![work_str.clone()], ..Settings::default() };
        let mut store = Store::new(egui::Context::default(), settings);
        store.scan_directories(true);
        wait_until_idle(&mut store);

        let names: Vec<&str> = store.repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["local-only", "tracked"]);
        assert_eq!(store.repos[0].sync_state, SyncState::NoRemoteConfigured);
        assert_eq!(store.repos[1].sync_state, SyncState::UpToDate);
        assert!(store.repos[1].last_checked_at.is_some(), "scan should be followed by a remote check");
        assert!(store.status_banner.starts_with("Check complete"), "{}", store.status_banner);
        assert_eq!(store.counts().issues, 1);

        // Recycle-bin deletes are not exercised here; permanent delete removes the row.
        let local = store.repos[0].path.clone();
        store.delete_repositories(std::slice::from_ref(&local), false);
        wait_until_idle(&mut store);
        assert!(store.repo(&local).is_none());
        assert!(!Path::new(&local).exists());
        assert!(store.activity_logs.iter().any(|l| l.action == "Delete Permanently" && l.succeeded));

        drop(store);
        let _ = platform::delete_permanently(&root_str);
    }
}
