//! Forking through the GitHub CLI (`gh`), which already handles GitHub sign-in.
//!
//! After `gh repo fork --remote` the clone has the fork as `origin` and the original as
//! `upstream`. GitDaddy then sets `remote.pushDefault`, so pulls keep coming from the
//! original while pushes go to the fork.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::git::{self, CommandResult};
use crate::model::{self, ActivityLogEntry, GitRepo};
use crate::util::first_line;

pub const CLI_DOWNLOAD_URL: &str = "https://cli.github.com";
/// Points at a different `gh` executable (used by tests).
const GH_OVERRIDE_ENV: &str = "GITDADDY_GH";

fn locate_gh() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(GH_OVERRIDE_ENV) {
        return Some(PathBuf::from(path));
    }
    let exe = if cfg!(windows) { "gh.exe" } else { "gh" };
    if let Some(path_var) = std::env::var_os("PATH")
        && let Some(found) = std::env::split_paths(&path_var).map(|d| d.join(exe)).find(|p| p.is_file())
    {
        return Some(found);
    }
    let mut candidates = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(var) {
            candidates.push(PathBuf::from(dir).join("GitHub CLI").join(exe));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        candidates.push(local.join("Programs").join("GitHub CLI").join(exe));
        candidates.push(local.join("Microsoft").join("WinGet").join("Links").join(exe));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join("scoop").join("shims").join(exe));
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn run_gh(args: &[&str], cwd: &Path, timeout_secs: u64) -> CommandResult {
    let command_string = format!("gh {}", args.join(" "));
    let Some(exe) = locate_gh() else {
        return CommandResult {
            exit_code: -1,
            stdout: String::new(),
            stderr: "GitHub CLI (gh) is not installed.".into(),
            command_string,
        };
    };
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .current_dir(cwd)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("NO_COLOR", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    git::run_command(cmd, command_string, timeout_secs)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliStatus {
    NotInstalled,
    SignedOut,
    Ready { login: String, orgs: Vec<String> },
}

/// Checks that `gh` is installed and signed in, and lists where a fork can go.
pub fn cli_status() -> Result<CliStatus, String> {
    if locate_gh().is_none() {
        return Ok(CliStatus::NotInstalled);
    }
    let cwd = std::env::temp_dir();
    let auth = run_gh(&["auth", "status", "--hostname", "github.com"], &cwd, 20);
    if !auth.succeeded() {
        let output = auth.combined_output().to_lowercase();
        if output.contains("not logged") || output.contains("gh auth login") {
            return Ok(CliStatus::SignedOut);
        }
        return Err(
            first_line(&auth.combined_output()).unwrap_or_else(|| "GitHub CLI could not check your sign-in.".into())
        );
    }
    let user = run_gh(&["api", "user", "--jq", ".login"], &cwd, 20);
    let login = user.out().to_string();
    if !user.succeeded() || login.is_empty() {
        return Err(first_line(&user.combined_output()).unwrap_or_else(|| "Could not reach GitHub.".into()));
    }
    let orgs = run_gh(&["api", "user/orgs", "--jq", ".[].login"], &cwd, 20);
    let orgs = if orgs.succeeded() {
        orgs.stdout.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
    } else {
        Vec::new()
    };
    Ok(CliStatus::Ready { login, orgs })
}

/// Opens a terminal running GitHub's browser sign-in, left open so its result stays visible.
pub fn open_sign_in_terminal() -> Result<(), String> {
    let gh = locate_gh().ok_or("GitHub CLI (gh) is not installed.")?;
    let script = format!("& '{}' auth login --hostname github.com --web", gh.display().to_string().replace('\'', "''"));
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoExit", "-NoProfile", "-Command", &script]);
    crate::platform::new_console(&mut cmd);
    cmd.spawn().map(|_| ()).map_err(|e| format!("Could not open a terminal: {e}"))
}

/// Remote names the clone will use after forking.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForkPlan {
    /// Remote that will point at the fork (pushes go here).
    pub fork_remote: String,
    /// Remote that keeps pointing at the original (pulls come from here).
    pub original_remote: String,
}

/// gh renames `origin` to `upstream` and makes the fork `origin`, unless `upstream` is taken.
pub fn plan_remotes(repo: &GitRepo) -> ForkPlan {
    let remotes = git::run_git(&["remote"], &repo.path, 5);
    let names: Vec<&str> = remotes.stdout.lines().map(str::trim).collect();
    let original = repo.primary_remote_name.clone().unwrap_or_else(|| "origin".into());
    if original == "origin" && !names.contains(&"upstream") {
        ForkPlan { fork_remote: "origin".into(), original_remote: "upstream".into() }
    } else {
        ForkPlan { fork_remote: "fork".into(), original_remote: original }
    }
}

pub struct ForkOutcome {
    pub repo: GitRepo,
    pub log: ActivityLogEntry,
    /// `owner/name` of the fork, or what went wrong.
    pub result: Result<String, String>,
}

/// Creates the fork on GitHub (under `org`, or the signed-in account) and points the clone at it.
pub fn fork_repository(repo: &GitRepo, org: Option<&str>, plan: &ForkPlan) -> ForkOutcome {
    let mut args = vec!["repo", "fork", "--remote", "--remote-name", plan.fork_remote.as_str()];
    if let Some(org) = org {
        args.extend(["--org", org]);
    }
    let res = run_gh(&args, Path::new(&repo.path), 120);
    let transcript = format!("[$ {}]\n{}", res.command_string, res.combined_output());
    if !res.succeeded() {
        let message = first_line(&res.stderr)
            .or_else(|| first_line(&res.stdout))
            .unwrap_or_else(|| "GitHub CLI could not create the fork.".into());
        let mut failed = repo.clone();
        failed.last_command_output = transcript.clone();
        let log = ActivityLogEntry::new(repo, "Fork", res.command_string.clone(), false, message.clone(), transcript);
        return ForkOutcome { repo: failed, log, result: Err(message) };
    }
    finish_fork_setup(repo, plan, transcript)
}

/// Once the fork remote exists: push to it by default, then fetch and re-read the clone.
pub fn finish_fork_setup(repo: &GitRepo, plan: &ForkPlan, transcript: String) -> ForkOutcome {
    let path = repo.path.as_str();
    let config = git::run_git(&["config", "remote.pushDefault", plan.fork_remote.as_str()], path, 5);
    let fork_url = git::run_git(&["remote", "get-url", plan.fork_remote.as_str()], path, 5).out().to_string();
    let fork_name = model::parse_remote(&fork_url).map(|(_, path)| path).unwrap_or_else(|| fork_url.clone());
    let (mut checked, _) = git::check_remote_status(repo);
    let output = format!("{transcript}\n\n[$ {}]\n\n{}", config.command_string, checked.last_command_output);
    checked.last_command_output = output.clone();
    let succeeded = config.succeeded();
    let summary = if succeeded {
        format!("Forked to {fork_name}")
    } else {
        format!("Forked to {fork_name}, but could not set the push remote")
    };
    let log = ActivityLogEntry::new(repo, "Fork", "gh repo fork --remote", succeeded, summary, output);
    ForkOutcome { repo: checked, log, result: Ok(fork_name) }
}

// MARK: - Dialog state, driven by the UI

/// Where the fork goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForkTarget {
    GitHub,
    /// An independent copy in a new folder on this PC.
    Local,
}

/// Whether GitHub forking is ready to go (only checked when the GitHub tab is used).
pub enum ForkPhase {
    Checking,
    NeedsCli,
    NeedsSignIn,
    Ready { login: String, orgs: Vec<String>, plan: ForkPlan },
    Unavailable { message: String },
}

pub enum ForkDone {
    GitHub { fork: String },
    Local { path: String },
}

pub enum ForkStage {
    Choosing,
    Working { message: String },
    Done(ForkDone),
    Failed { message: String },
}

enum Msg {
    Status(Result<CliStatus, String>, ForkPlan),
    Forked(Box<ForkOutcome>),
    Copied { path: String, log: ActivityLogEntry, result: Result<(), String> },
}

/// What the app should do once a fork finishes.
pub enum ForkEvent {
    /// The GitHub fork rewired the existing clone.
    Rewired(Box<ForkOutcome>),
    /// A new local copy exists and should join the list.
    Copied {
        path: String,
        log: ActivityLogEntry,
    },
    Failed {
        log: ActivityLogEntry,
    },
}

pub struct ForkFlow {
    pub repo_path: String,
    pub target: ForkTarget,
    pub phase: Option<ForkPhase>,
    pub stage: ForkStage,
    /// Account or organization a GitHub fork goes to; starts as the signed-in account.
    pub destination: String,
    /// Folder for a local fork.
    pub local_path: String,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    ctx: egui::Context,
}

/// `D:\Dev\app` → `D:\Dev\app-fork`, or `app-fork-2`, `app-fork-3`, … if taken.
pub fn default_local_fork_path(repo_path: &str) -> String {
    let source = Path::new(repo_path);
    let parent = source.parent().unwrap_or(source);
    let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into());
    (1..)
        .map(|n| if n == 1 { format!("{name}-fork") } else { format!("{name}-fork-{n}") })
        .map(|candidate| parent.join(candidate))
        .find(|candidate| !candidate.exists())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Why a local fork cannot go to `path`, if it cannot.
pub fn local_path_problem(path: &str) -> Option<&'static str> {
    let path = Path::new(path.trim());
    if path.as_os_str().is_empty() {
        return Some("Choose a folder for the copy.");
    }
    if !path.is_absolute() {
        return Some("Use a full path, such as D:\\Projects\\my-fork.");
    }
    if path.exists() && std::fs::read_dir(path).map_or(true, |mut entries| entries.next().is_some()) {
        return Some("That folder already exists. Pick a new name.");
    }
    if !path.parent().is_some_and(Path::is_dir) {
        return Some("The parent folder does not exist.");
    }
    None
}

impl ForkFlow {
    pub fn start(repo: &GitRepo, ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let target = if repo.can_fork() { ForkTarget::GitHub } else { ForkTarget::Local };
        let mut flow = Self {
            repo_path: repo.path.clone(),
            target,
            phase: None,
            stage: ForkStage::Choosing,
            destination: String::new(),
            local_path: default_local_fork_path(&repo.path),
            tx,
            rx,
            ctx,
        };
        if target == ForkTarget::GitHub {
            flow.check(repo);
        }
        flow
    }

    /// A flow frozen in one state, for rendering previews without calling gh.
    #[cfg(test)]
    pub fn preview(
        repo: &GitRepo,
        ctx: egui::Context,
        target: ForkTarget,
        phase: Option<ForkPhase>,
        stage: ForkStage,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            repo_path: repo.path.clone(),
            target,
            phase,
            stage,
            destination: "jordan-dev".into(),
            local_path: default_local_fork_path(&repo.path),
            tx,
            rx,
            ctx,
        }
    }

    /// Switches tabs; GitHub sign-in is checked the first time its tab is shown.
    pub fn set_target(&mut self, target: ForkTarget, repo: &GitRepo) {
        self.target = target;
        if target == ForkTarget::GitHub && self.phase.is_none() {
            self.check(repo);
        }
    }

    pub fn check(&mut self, repo: &GitRepo) {
        self.phase = Some(ForkPhase::Checking);
        self.stage = ForkStage::Choosing;
        let (tx, ctx, repo) = (self.tx.clone(), self.ctx.clone(), repo.clone());
        thread::spawn(move || {
            let status = cli_status();
            let _ = tx.send(Msg::Status(status, plan_remotes(&repo)));
            ctx.request_repaint();
        });
    }

    pub fn fork_on_github(&mut self, repo: &GitRepo) {
        let Some(ForkPhase::Ready { login, plan, .. }) = &self.phase else { return };
        let org = (!self.destination.eq_ignore_ascii_case(login)).then(|| self.destination.clone());
        let plan = plan.clone();
        let name = repo.github_slug().and_then(|s| s.split('/').nth(1).map(String::from)).unwrap_or_default();
        self.stage = ForkStage::Working { message: format!("Creating {}/{name} on GitHub…", self.destination) };
        let (tx, ctx, repo) = (self.tx.clone(), self.ctx.clone(), repo.clone());
        thread::spawn(move || {
            let outcome = fork_repository(&repo, org.as_deref(), &plan);
            let _ = tx.send(Msg::Forked(Box::new(outcome)));
            ctx.request_repaint();
        });
    }

    pub fn fork_locally(&mut self, repo: &GitRepo) {
        let path = self.local_path.trim().to_string();
        if local_path_problem(&path).is_some() {
            return;
        }
        self.stage = ForkStage::Working {
            message: format!("Copying {} to {}…", repo.name, crate::util::abbreviate_home(&path)),
        };
        let (tx, ctx, repo) = (self.tx.clone(), self.ctx.clone(), repo.clone());
        thread::spawn(move || {
            let existed = Path::new(&path).exists();
            let (result, transcript) = git::fork_locally(&repo, &path);
            if result.is_err() && !existed {
                // Leave nothing half-copied behind.
                let _ = crate::platform::delete_permanently(&path);
            }
            let summary = match &result {
                Ok(()) => format!("Copied to {}", crate::util::abbreviate_home(&path)),
                Err(message) => message.clone(),
            };
            let log = ActivityLogEntry::new(
                &repo,
                "Fork Locally",
                format!("git clone {} {path}", repo.path),
                result.is_ok(),
                summary,
                transcript,
            );
            let _ = tx.send(Msg::Copied { path, log, result });
            ctx.request_repaint();
        });
    }

    /// Applies background results; returns what the app should apply to its store, once.
    pub fn poll(&mut self) -> Option<ForkEvent> {
        let mut event = None;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Status(status, plan) => {
                    self.phase = Some(match status {
                        Ok(CliStatus::NotInstalled) => ForkPhase::NeedsCli,
                        Ok(CliStatus::SignedOut) => ForkPhase::NeedsSignIn,
                        Ok(CliStatus::Ready { login, orgs }) => {
                            self.destination = login.clone();
                            ForkPhase::Ready { login, orgs, plan }
                        }
                        Err(message) => ForkPhase::Unavailable { message },
                    });
                }
                Msg::Forked(outcome) => {
                    self.stage = match &outcome.result {
                        Ok(fork) => ForkStage::Done(ForkDone::GitHub { fork: fork.clone() }),
                        Err(message) => ForkStage::Failed { message: message.clone() },
                    };
                    event = Some(ForkEvent::Rewired(outcome));
                }
                Msg::Copied { path, log, result } => match result {
                    Ok(()) => {
                        self.stage = ForkStage::Done(ForkDone::Local { path: path.clone() });
                        event = Some(ForkEvent::Copied { path, log });
                    }
                    Err(message) => {
                        self.stage = ForkStage::Failed { message };
                        event = Some(ForkEvent::Failed { log });
                    }
                },
            }
        }
        event
    }

    pub fn is_busy(&self) -> bool {
        matches!(self.stage, ForkStage::Working { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SyncState;

    fn git_ok(args: &[&str], cwd: &str) {
        let res = git::run_git(args, cwd, 30);
        assert!(res.succeeded(), "{} failed: {}", res.command_string, res.combined_output());
    }

    fn identity(path: &str) {
        git_ok(&["config", "user.name", "GitDaddy Test"], path);
        git_ok(&["config", "user.email", "test@example.com"], path);
        git_ok(&["config", "commit.gpgsign", "false"], path);
        git_ok(&["config", "pull.rebase", "false"], path);
    }

    fn commit(path: &str, file: &str, message: &str) {
        std::fs::write(Path::new(path).join(file), message).unwrap();
        git_ok(&["add", "-A"], path);
        git_ok(&["commit", "-m", message], path);
    }

    /// Does what `gh repo fork --remote` does locally, with bare repos standing in for GitHub,
    /// then checks GitDaddy's setup and status for a fork.
    #[test]
    fn fork_setup_pulls_from_original_and_pushes_to_fork() {
        let root = std::env::temp_dir().join(format!("gitdaddy-fork-{}", std::process::id()));
        let _ = crate::platform::delete_permanently(&root.to_string_lossy());
        std::fs::create_dir_all(&root).unwrap();
        let at = |name: &str| root.join(name).to_string_lossy().into_owned();
        let root_str = at("");

        // The original project, with one commit from its maintainer.
        git_ok(&["-c", "init.defaultBranch=main", "init", "--bare", &at("original.git")], &root_str);
        git_ok(&["clone", &at("original.git"), "maintainer"], &root_str);
        identity(&at("maintainer"));
        git_ok(&["checkout", "-b", "main"], &at("maintainer"));
        commit(&at("maintainer"), "README.md", "init");
        git_ok(&["push", "-u", "origin", "main"], &at("maintainer"));

        // Our clone of it, before forking.
        git_ok(&["clone", &at("original.git"), "mine"], &root_str);
        let mine = at("mine");
        identity(&mine);
        let repo = git::inspect_local_repository(&GitRepo::new("mine".into(), mine.clone()));
        let plan = plan_remotes(&repo);
        assert_eq!(plan, ForkPlan { fork_remote: "origin".into(), original_remote: "upstream".into() });

        // What gh does: create the fork on the server, rename origin to upstream, add the fork as origin.
        git_ok(&["clone", "--bare", &at("original.git"), &at("fork.git")], &root_str);
        git_ok(&["remote", "rename", "origin", "upstream"], &mine);
        git_ok(&["remote", "add", "origin", &at("fork.git")], &mine);

        let outcome = finish_fork_setup(&repo, &plan, "[$ gh repo fork]".into());
        assert!(outcome.log.succeeded, "{}", outcome.log.output);
        let forked = outcome.repo;
        assert_eq!(forked.sync_state, SyncState::UpToDate, "{}", forked.last_command_output);
        assert_eq!(forked.remote_url.as_deref(), Some(at("fork.git").as_str()));
        assert_eq!(forked.fork_parent_url.as_deref(), Some(at("original.git").as_str()));
        assert_eq!(forked.push_remote.as_deref(), Some("origin"));
        assert_eq!(forked.upstream_branch.as_deref(), Some("upstream/main"));
        assert!(!forked.can_fork());

        // A local commit is "unpushed" until it reaches the fork, not the original.
        commit(&mine, "work.txt", "feat: my change");
        let forked = git::inspect_local_repository(&forked);
        assert_eq!(forked.sync_state, SyncState::Ahead(1));
        let (forked, log) = git::push_repository(&forked);
        assert!(log.succeeded, "{}", log.output);
        assert_eq!(forked.sync_state, SyncState::UpToDate, "{}", forked.last_command_output);
        let in_fork = git::run_git(&["log", "--oneline", "-1", "main"], &at("fork.git"), 10);
        assert!(in_fork.out().contains("feat: my change"), "{}", in_fork.out());
        let in_original = git::run_git(&["log", "--oneline", "-1", "main"], &at("original.git"), 10);
        assert!(!in_original.out().contains("feat: my change"), "push must not touch the original");

        // New work in the original shows up as an update to pull, and pulls bring it in.
        git_ok(&["pull", "--ff-only"], &at("maintainer"));
        commit(&at("maintainer"), "upstream.txt", "feat: upstream change");
        git_ok(&["push"], &at("maintainer"));
        let (forked, _) = git::check_remote_status(&forked);
        assert_eq!(forked.sync_state, SyncState::Behind(1));
        let (forked, log) = git::pull_repository(&forked, crate::model::PullStrategy::Merge, true);
        assert!(log.succeeded, "{}", log.output);
        // The merge commit and the upstream change are not in the fork yet.
        assert!(matches!(forked.sync_state, SyncState::Ahead(_)), "{:?}", forked.sync_state);

        let _ = crate::platform::delete_permanently(&root_str);
    }

    #[test]
    fn keeps_existing_upstream_remote() {
        let root = std::env::temp_dir().join(format!("gitdaddy-plan-{}", std::process::id()));
        let _ = crate::platform::delete_permanently(&root.to_string_lossy());
        std::fs::create_dir_all(&root).unwrap();
        let path = root.to_string_lossy().into_owned();
        git_ok(&["init"], &path);
        git_ok(&["remote", "add", "origin", "git@github.com:someone/proj.git"], &path);
        git_ok(&["remote", "add", "upstream", "git@github.com:their-parent/proj.git"], &path);
        let repo = git::inspect_local_repository(&GitRepo::new("proj".into(), path.clone()));
        assert_eq!(plan_remotes(&repo), ForkPlan { fork_remote: "fork".into(), original_remote: "origin".into() });
        let _ = crate::platform::delete_permanently(&path);
    }

    #[test]
    #[ignore = "asks GitHub who is signed in (read-only); run with `cargo test live_gh -- --ignored`"]
    fn live_gh_status() {
        let status = cli_status().expect("gh reachable");
        eprintln!("gh status: {status:?}");
        assert!(matches!(status, CliStatus::Ready { .. } | CliStatus::SignedOut | CliStatus::NotInstalled));
    }
}

#[cfg(test)]
mod local_fork_tests {
    use super::*;

    #[test]
    fn suggests_a_free_sibling_folder() {
        let root = std::env::temp_dir().join(format!("gitdaddy-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app")).unwrap();
        let app = root.join("app").to_string_lossy().into_owned();
        assert_eq!(default_local_fork_path(&app), root.join("app-fork").to_string_lossy());
        std::fs::create_dir_all(root.join("app-fork")).unwrap();
        std::fs::write(root.join("app-fork").join("x"), "x").unwrap();
        assert_eq!(default_local_fork_path(&app), root.join("app-fork-2").to_string_lossy());

        assert!(local_path_problem(&root.join("app-fork-2").to_string_lossy()).is_none());
        assert!(local_path_problem(&root.join("app-fork").to_string_lossy()).is_some(), "non-empty folder");
        assert!(local_path_problem(r"relative\path").is_some());
        assert!(local_path_problem(&root.join("missing").join("child").to_string_lossy()).is_some());
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod real_local_fork {
    use super::*;

    /// Copies a real repository (read-only for the source) and compares the copy's status.
    #[test]
    #[ignore = "set GITDADDY_DEBUG_REPO; copies it into the temp folder and deletes the copy"]
    fn real_repository_copy_matches_original() {
        let source = std::env::var("GITDADDY_DEBUG_REPO").expect("GITDADDY_DEBUG_REPO");
        let original = git::inspect_local_repository(&GitRepo::new(crate::util::folder_name(&source), source.clone()));
        let dest = std::env::temp_dir().join(format!("gitdaddy-real-copy-{}", std::process::id()));
        let dest = dest.to_string_lossy().into_owned();
        let started = std::time::Instant::now();
        let (result, transcript) = git::fork_locally(&original, &dest);
        let elapsed = started.elapsed();
        let copy = git::inspect_local_repository(&GitRepo::new("copy".into(), dest.clone()));
        let _ = crate::platform::delete_permanently(&dest);
        assert!(result.is_ok(), "{result:?}\n{transcript}");
        eprintln!(
            "copied in {:.1}s: branch {} -> {}, upstream {:?}, state {:?} / original {:?}",
            elapsed.as_secs_f32(),
            original.branch,
            copy.branch,
            copy.upstream_branch,
            copy.sync_state,
            original.sync_state
        );
        assert_eq!(copy.branch, original.branch);
        assert_eq!(copy.upstream_branch, original.upstream_branch);
        assert_eq!(copy.remote_url, original.remote_url);
        assert_eq!(copy.sync_state, original.sync_state);
    }
}
