use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::model::{ActivityLogEntry, CommitInfo, FileChange, GitRepo, PullStrategy, SyncState};
use crate::util::{first_line, last_line};

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub command_string: String,
}

impl CommandResult {
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }

    pub fn out(&self) -> &str {
        self.stdout.trim()
    }

    pub fn combined_output(&self) -> String {
        let out = self.stdout.trim();
        let err = self.stderr.trim();
        match (out.is_empty(), err.is_empty()) {
            (true, _) => err.to_string(),
            (_, true) => out.to_string(),
            _ => format!("{out}\n{err}"),
        }
    }

    /// `[$ git …]` header followed by the output, or `fallback` when git printed nothing.
    fn transcript(&self, fallback: &str) -> String {
        let output = self.combined_output();
        let body = if output.is_empty() { fallback } else { &output };
        format!("[$ {}]\n{body}", self.command_string)
    }

    fn short_error(&self, fallback: &str) -> String {
        first_line(&self.stderr).or_else(|| first_line(&self.stdout)).unwrap_or_else(|| fallback.to_string())
    }
}

struct GitEnvironment {
    executable: Option<PathBuf>,
    ssh_command: Option<&'static str>,
}

const NON_INTERACTIVE_SSH: &str = "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10";

fn environment() -> &'static GitEnvironment {
    static ENV: OnceLock<GitEnvironment> = OnceLock::new();
    ENV.get_or_init(|| {
        let executable = locate_git();
        // Respect any SSH setup the user already has (for example core.sshCommand pointing at
        // Windows OpenSSH so the ssh-agent service is used); only force batch mode otherwise.
        let user_configured_ssh = std::env::var_os("GIT_SSH_COMMAND").is_some()
            || std::env::var_os("GIT_SSH").is_some()
            || executable.as_ref().is_some_and(|exe| {
                let mut cmd = Command::new(exe);
                cmd.args(["config", "--get", "core.sshCommand"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null());
                hide_console(&mut cmd);
                cmd.output().is_ok_and(|o| o.status.success() && !o.stdout.trim_ascii().is_empty())
            });
        GitEnvironment { executable, ssh_command: (!user_configured_ssh).then_some(NON_INTERACTIVE_SSH) }
    })
}

fn locate_git() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "git.exe" } else { "git" };
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(exe_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let mut fallbacks: Vec<PathBuf> = Vec::new();
    #[cfg(not(windows))]
    {
        for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin", "/usr/bin"] {
            fallbacks.push(PathBuf::from(dir).join(exe_name));
        }
    }
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(var) {
            fallbacks.push(PathBuf::from(dir).join("Git").join("cmd").join(exe_name));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        fallbacks.push(PathBuf::from(local).join("Programs").join("Git").join("cmd").join(exe_name));
    }
    if let Some(home) = dirs::home_dir() {
        fallbacks.push(home.join("scoop").join("apps").join("git").join("current").join("cmd").join(exe_name));
    }
    fallbacks.into_iter().find(|p| p.is_file())
}

pub fn git_executable() -> Option<&'static Path> {
    environment().executable.as_deref()
}

#[cfg(windows)]
pub fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console(_cmd: &mut Command) {}

/// Runs a git command inside `repo_path` with a non-interactive environment and a timeout.
pub fn run_git<S: AsRef<str>>(args: &[S], repo_path: &str, timeout_secs: u64) -> CommandResult {
    let command_string = format!("git {}", args.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(" "));
    let failure = |message: String| CommandResult {
        exit_code: -1,
        stdout: String::new(),
        stderr: message,
        command_string: command_string.clone(),
    };

    let env = environment();
    let Some(exe) = env.executable.as_ref() else {
        #[cfg(target_os = "macos")]
        let msg = "Git was not found. Install Xcode Command Line Tools (xcode-select --install) or Homebrew git and restart GitDaddy.";
        #[cfg(not(target_os = "macos"))]
        let msg = "Git was not found. Install Git for Windows (https://git-scm.com/download/win) and restart GitDaddy.";
        return failure(msg.into());
    };

    let mut cmd = Command::new(exe);
    cmd.args(["-c", "core.quotepath=false", "-c", "color.ui=false"])
        .args(args.iter().map(AsRef::as_ref))
        .current_dir(repo_path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8");
    if let Some(ssh) = env.ssh_command {
        cmd.env("GIT_SSH_COMMAND", ssh);
    }
    run_command(cmd, command_string, timeout_secs)
}

/// Runs a prepared command without a console window, capturing output, and stops it after
/// `timeout_secs`. `command_string` is what logs and the Git Output pane show.
pub fn run_command(mut cmd: Command, command_string: String, timeout_secs: u64) -> CommandResult {
    let program = command_string.split_whitespace().next().unwrap_or("command").to_string();
    let failure = |message: String| CommandResult {
        exit_code: -1,
        stdout: String::new(),
        stderr: message,
        command_string: command_string.clone(),
    };
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    hide_console(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => return failure(format!("Failed to launch {program}: {err}")),
    };

    let read_pipe = |pipe: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        if let Some(mut pipe) = pipe {
            thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                let _ = tx.send(buf);
            });
        }
        rx
    };
    let stdout_rx = read_pipe(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr_rx = read_pipe(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));

    // Pipes close when the process exits, so waiting on the readers doubles as waiting on it.
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let remaining = || deadline.saturating_duration_since(Instant::now());
    let stdout = stdout_rx.recv_timeout(remaining());
    let stderr = stderr_rx.recv_timeout(remaining());
    let timed_out = stdout.is_err() || stderr.is_err();
    if timed_out {
        let _ = child.kill();
    }
    let exit_code = child.wait().ok().and_then(|status| status.code()).unwrap_or(-1);

    let decode =
        |bytes: Result<Vec<u8>, _>| bytes.map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let mut stderr = decode(stderr);
    if timed_out {
        if !stderr.trim().is_empty() {
            stderr.push('\n');
        }
        stderr.push_str(&format!("Timed out after {timeout_secs}s and was stopped."));
    }

    CommandResult { exit_code: if timed_out { -1 } else { exit_code }, stdout: decode(stdout), stderr, command_string }
}

/// Reads local Git state (branch, remote, working tree, cached ahead/behind) without a network fetch.
pub fn inspect_local_repository(repo: &GitRepo) -> GitRepo {
    let mut updated = repo.clone();
    let path = repo.path.as_str();

    // 1. Current branch & HEAD commit.
    let branch_res = run_git(&["symbolic-ref", "--short", "-q", "HEAD"], path, 5);
    let rev_res = run_git(&["rev-parse", "--short", "HEAD"], path, 5);
    let short_rev = rev_res.out().to_string();
    let has_commits = rev_res.succeeded() && !short_rev.is_empty();

    let mut is_detached = false;
    if branch_res.succeeded() {
        let branch = branch_res.out();
        updated.branch = if branch.is_empty() { "HEAD".into() } else { branch.into() };
    } else if has_commits {
        is_detached = true;
        updated.branch = format!("detached@{short_rev}");
    } else {
        updated.branch = "main (unborn)".into();
    }

    // 2. Remote URL (prefer origin, fall back to the first remote).
    let origin_res = run_git(&["remote", "get-url", "origin"], path, 5);
    let mut primary_remote = None;
    if origin_res.succeeded() {
        primary_remote = Some("origin".to_string());
        updated.remote_url = Some(origin_res.out().to_string());
    } else {
        let remotes_res = run_git(&["remote"], path, 5);
        let first_remote = remotes_res.stdout.lines().map(str::trim).find(|l| !l.is_empty());
        if let Some(first_remote) = first_remote {
            let url_res = run_git(&["remote", "get-url", first_remote], path, 5);
            updated.remote_url = Some(url_res.out().to_string());
            primary_remote = Some(first_remote.to_string());
        } else {
            updated.remote_url = None;
        }
    }
    updated.primary_remote_name = primary_remote.clone();

    // 2b. Forks keep the original project as `upstream`.
    updated.fork_parent_url = None;
    if primary_remote.as_deref() != Some("upstream") {
        let upstream_url = run_git(&["remote", "get-url", "upstream"], path, 5);
        let url = upstream_url.out();
        if upstream_url.succeeded() && !url.is_empty() && Some(url) != updated.remote_url.as_deref() {
            updated.fork_parent_url = Some(url.to_string());
        }
    }

    // 3. Working tree status.
    let status_res = run_git(&["status", "--porcelain"], path, 8);
    if status_res.succeeded() {
        updated.local_changes = parse_porcelain_status(&status_res.stdout);
    }

    // 4. Last commit on HEAD.
    updated.last_commit_hash.clear();
    updated.last_commit_message.clear();
    updated.last_commit_relative_date.clear();
    updated.last_commit_timestamp = None;
    if has_commits {
        let log_res = run_git(&["log", "-1", "--format=%h%x1f%s%x1f%cr%x1f%ct"], path, 5);
        if log_res.succeeded() {
            let parts: Vec<&str> = log_res.out().split('\u{1f}').collect();
            if parts.len() >= 4 {
                updated.last_commit_hash = parts[0].to_string();
                updated.last_commit_message = parts[1].to_string();
                updated.last_commit_relative_date = parts[2].to_string();
                updated.last_commit_timestamp = parts[3].trim().parse().ok();
            }
        }
    }

    // 4b. README & description.
    let readme = load_readme_and_description(Path::new(path));
    updated.readme_file_name = readme.file_name;
    updated.readme_content = readme.content;
    updated.repo_description = readme.description;

    // 5. Special states before comparing with the upstream.
    let reset_tracking = |repo: &mut GitRepo, state: SyncState| {
        repo.upstream_branch = None;
        repo.incoming_commits.clear();
        repo.unpushed_commits.clear();
        repo.sync_state = state;
    };
    if !has_commits {
        reset_tracking(&mut updated, SyncState::EmptyRepo { has_remote: primary_remote.is_some() });
        return updated;
    }
    let Some(remote_name) = primary_remote else {
        reset_tracking(&mut updated, SyncState::NoRemoteConfigured);
        return updated;
    };
    if is_detached {
        reset_tracking(&mut updated, SyncState::DetachedHead(short_rev));
        return updated;
    }

    // 6. Upstream branch & ahead/behind counts.
    let upstream_res = run_git(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"], path, 5);
    let mut upstream_ref = None;
    if upstream_res.succeeded() && !upstream_res.out().is_empty() {
        upstream_ref = Some(upstream_res.out().to_string());
    } else {
        let fallback = format!("{remote_name}/{}", updated.branch);
        let verify_res = run_git(&["rev-parse", "--verify", "--quiet", &format!("refs/remotes/{fallback}")], path, 5);
        if verify_res.succeeded() {
            upstream_ref = Some(fallback);
        }
    }
    updated.upstream_branch = upstream_ref.clone();

    let Some(upstream) = upstream_ref else {
        updated.incoming_commits.clear();
        updated.unpushed_commits = fetch_commits(&["HEAD".to_string()], path, 10);
        updated.sync_state = SyncState::UnpublishedBranch { branch: updated.branch.clone(), remote: remote_name };
        return updated;
    };

    // A fork pulls from the original (`upstream`) but pushes to the fork (remote.pushDefault).
    // Then "unpushed" means "not in my fork yet", so compare against the push target instead.
    // (`@{push}` cannot be used: with push.default=simple git refuses to resolve it here.)
    updated.push_remote = None;
    let tracking_remote = upstream.split_once('/').map(|(remote, _)| remote.to_string());
    let config = |key: String| {
        let res = run_git(&["config", "--get", &key], path, 5);
        (res.succeeded() && !res.out().is_empty()).then(|| res.out().to_string())
    };
    let push_remote = if upstream_res.succeeded() {
        config(format!("branch.{}.pushRemote", updated.branch)).or_else(|| config("remote.pushDefault".into()))
    } else {
        None
    };
    let push_target = push_remote
        .filter(|remote| Some(remote) != tracking_remote.as_ref())
        .map(|remote| format!("{remote}/{}", updated.branch));
    let ahead_base = match &push_target {
        Some(target) => {
            updated.push_remote = target.split_once('/').map(|(remote, _)| remote.to_string());
            // Until the branch exists on the fork, everything not in the original still needs pushing.
            let exists = run_git(&["rev-parse", "--verify", "--quiet", &format!("refs/remotes/{target}")], path, 5);
            if exists.succeeded() { target.clone() } else { upstream.clone() }
        }
        None => upstream.clone(),
    };

    let count = |range: String| run_git(&["rev-list", "--count", &range], path, 8).out().parse::<usize>().ok();
    if let (Some(ahead), Some(behind)) = (count(format!("{ahead_base}..HEAD")), count(format!("HEAD..{upstream}"))) {
        updated.sync_state = match (ahead, behind) {
            (0, 0) => SyncState::UpToDate,
            (0, behind) => SyncState::Behind(behind),
            (ahead, 0) => SyncState::Ahead(ahead),
            (ahead, behind) => SyncState::Diverged { ahead, behind },
        };
        updated.incoming_commits =
            if behind > 0 { fetch_commits(&[format!("HEAD..{upstream}")], path, 20) } else { Vec::new() };
        updated.unpushed_commits =
            if ahead > 0 { fetch_commits(&[format!("{ahead_base}..HEAD")], path, 20) } else { Vec::new() };
    }

    updated
}

fn parse_porcelain_status(stdout: &str) -> Vec<FileChange> {
    stdout
        .lines()
        .filter(|line| line.len() >= 3 && line.is_char_boundary(3))
        .map(|line| {
            let status = line[..2].trim();
            FileChange {
                status: if status.is_empty() { "M".into() } else { status.into() },
                path: line[3..].to_string(),
            }
        })
        .collect()
}

/// Fetches from the remote (`git fetch --all --prune`) and compares local vs remote branches.
pub fn check_remote_status(repo: &GitRepo) -> (GitRepo, ActivityLogEntry) {
    let path = repo.path.as_str();

    let remotes_res = run_git(&["remote"], path, 5);
    if remotes_res.out().is_empty() {
        let mut inspected = inspect_local_repository(repo);
        inspected.last_checked_at = Some(SystemTime::now());
        let log = ActivityLogEntry::new(
            repo,
            "Check Remote",
            "git remote",
            true,
            inspected.sync_state.label(),
            "Local-only repository: no remote server URL (e.g. 'origin') is configured.",
        );
        return (inspected, log);
    }

    let fetch_res = run_git(&["fetch", "--all", "--prune", "--quiet"], path, 30);
    let mut inspected = inspect_local_repository(repo);
    inspected.last_checked_at = Some(SystemTime::now());
    inspected.last_command_output = fetch_res.transcript("Fetch completed successfully.");

    if !fetch_res.succeeded() {
        let short_err = fetch_res.short_error("Failed to fetch remote");
        inspected.sync_state = SyncState::Error(short_err.clone());
        let log = ActivityLogEntry::new(
            repo,
            "Check Remote",
            fetch_res.command_string.clone(),
            false,
            short_err,
            fetch_res.combined_output(),
        );
        return (inspected, log);
    }

    let log = ActivityLogEntry::new(
        repo,
        "Check Remote",
        fetch_res.command_string.clone(),
        true,
        inspected.sync_state.label(),
        inspected.last_command_output.clone(),
    );
    (inspected, log)
}

fn has_explicit_upstream(path: &str) -> bool {
    run_git(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"], path, 5).succeeded()
}

/// Runs `git pull` with the chosen strategy and optional auto-stash, then refreshes state.
pub fn pull_repository(repo: &GitRepo, strategy: PullStrategy, auto_stash: bool) -> (GitRepo, ActivityLogEntry) {
    let path = repo.path.as_str();
    let mut args = strategy.git_arguments();
    if auto_stash {
        args.push("--autostash".into());
    }
    // Without a configured tracking branch, name the remote and branch explicitly.
    if !has_explicit_upstream(path)
        && let Some((remote, branch)) = repo.upstream_branch.as_deref().and_then(|u| u.split_once('/'))
    {
        args.push(remote.into());
        args.push(branch.into());
    }

    let pull_res = run_git(&args, path, 45);
    let mut inspected = inspect_local_repository(repo);
    inspected.last_checked_at = Some(SystemTime::now());
    let full_output = pull_res.transcript("Already up to date.");
    inspected.last_command_output = full_output.clone();

    if pull_res.succeeded() {
        let summary = last_line(&pull_res.stdout).unwrap_or_else(|| "Pulled successfully".into());
        let log = ActivityLogEntry::new(repo, "Git Pull", pull_res.command_string.clone(), true, summary, full_output);
        (inspected, log)
    } else {
        let short_err = pull_res.short_error("Git pull failed");
        inspected.sync_state = SyncState::Error(short_err.clone());
        let log =
            ActivityLogEntry::new(repo, "Git Pull", pull_res.command_string.clone(), false, short_err, full_output);
        (inspected, log)
    }
}

/// Runs `git push` (setting the upstream with `-u <remote> <branch>` when needed), then refreshes state.
pub fn push_repository(repo: &GitRepo) -> (GitRepo, ActivityLogEntry) {
    let path = repo.path.as_str();
    let mut args: Vec<String> = vec!["push".into()];

    if let Some(push_remote) = &repo.push_remote {
        // Fork: the branch tracks the original, so name the fork explicitly. This works
        // whatever push.default is set to.
        args.extend([push_remote.clone(), "HEAD".into()]);
    } else if !has_explicit_upstream(path) {
        if let Some((remote, branch)) = repo.upstream_branch.as_deref().and_then(|u| u.split_once('/')) {
            args.extend(["-u".into(), remote.into(), branch.into()]);
        } else if is_named_branch(&repo.branch) {
            let remote = repo.primary_remote_name.clone().unwrap_or_else(|| "origin".into());
            args.extend(["-u".into(), remote, repo.branch.clone()]);
        }
    }

    let push_res = run_git(&args, path, 45);
    let mut inspected = inspect_local_repository(repo);
    inspected.last_checked_at = Some(SystemTime::now());
    let full_output = push_res.transcript("Everything up-to-date.");
    inspected.last_command_output = full_output.clone();

    if push_res.succeeded() {
        let summary = last_line(&push_res.combined_output()).unwrap_or_else(|| "Pushed commits to remote".into());
        let log = ActivityLogEntry::new(repo, "Git Push", push_res.command_string.clone(), true, summary, full_output);
        (inspected, log)
    } else {
        let short_err = push_res.short_error("Git push failed");
        inspected.sync_state = SyncState::Error(short_err.clone());
        let log =
            ActivityLogEntry::new(repo, "Git Push", push_res.command_string.clone(), false, short_err, full_output);
        (inspected, log)
    }
}

fn is_named_branch(branch: &str) -> bool {
    !branch.is_empty() && branch != "—" && !branch.starts_with("detached@") && !branch.ends_with("(unborn)")
}

/// Stages everything (`git add -A`), commits with `message`, and pushes when a remote exists.
pub fn commit_and_push_repository(repo: &GitRepo, message: &str) -> (GitRepo, ActivityLogEntry) {
    let path = repo.path.as_str();

    let add_res = run_git(&["add", "-A"], path, 15);
    if !add_res.succeeded() {
        let mut inspected = inspect_local_repository(repo);
        inspected.sync_state = SyncState::Error(format!("git add failed: {}", add_res.stderr.trim()));
        inspected.last_command_output = add_res.transcript("");
        let log = ActivityLogEntry::new(
            repo,
            "Commit & Push",
            add_res.command_string.clone(),
            false,
            "Failed to stage files",
            inspected.last_command_output.clone(),
        );
        return (inspected, log);
    }

    let commit_res = run_git(&["commit", "-m", message], path, 20);
    if !commit_res.succeeded() {
        let mut inspected = inspect_local_repository(repo);
        let short_err = first_line(&commit_res.combined_output()).unwrap_or_else(|| "git commit failed".into());
        inspected.sync_state = SyncState::Error(short_err.clone());
        inspected.last_command_output = commit_res.transcript("");
        let log = ActivityLogEntry::new(
            repo,
            "Commit & Push",
            commit_res.command_string.clone(),
            false,
            short_err,
            inspected.last_command_output.clone(),
        );
        return (inspected, log);
    }

    if !repo.has_remote() {
        let mut inspected = inspect_local_repository(repo);
        inspected.last_command_output = commit_res.transcript("");
        let log = ActivityLogEntry::new(
            repo,
            "Git Commit",
            commit_res.command_string.clone(),
            true,
            format!("Committed locally: {message}"),
            inspected.last_command_output.clone(),
        );
        return (inspected, log);
    }

    let (mut pushed, push_log) = push_repository(repo);
    let combined = format!("{}\n\n{}", commit_res.transcript(""), pushed.last_command_output);
    pushed.last_command_output = combined.clone();
    let summary = if push_log.succeeded { format!("Committed & pushed: {message}") } else { push_log.summary.clone() };
    let log = ActivityLogEntry::new(
        repo,
        "Commit & Push",
        "git add -A && git commit && git push",
        push_log.succeeded,
        summary,
        combined,
    );
    (pushed, log)
}

/// Copies `repo` into the new folder `dest` as an independent repository: every local branch
/// and tag, the same remotes and remote-tracking refs, and the same upstream settings, so its
/// status matches the original straight away. Uncommitted changes and stashes are not copied,
/// and the copy keeps no link to the source folder. Returns the steps' transcript either way.
pub fn fork_locally(repo: &GitRepo, dest: &str) -> (Result<(), String>, String) {
    let src = repo.path.as_str();
    let mut transcript = Vec::new();
    let mut step = |res: CommandResult, what: &str| -> Result<CommandResult, String> {
        transcript.push(res.transcript("Done."));
        if res.succeeded() { Ok(res) } else { Err(format!("{what}: {}", res.short_error("git failed"))) }
    };
    let parent = Path::new(dest).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| ".".into());

    let result = (|| {
        // A local clone hard-links the object files, so even large repositories copy quickly.
        step(run_git(&["clone", "--quiet", src, dest], &parent, 900), "Could not copy the repository")?;
        // Drop the clone's link back to the source folder, then take the source's refs as-is.
        step(run_git(&["remote", "remove", "origin"], dest, 15), "Could not detach the copy")?;
        step(
            run_git(
                &[
                    "fetch",
                    "--quiet",
                    "--update-head-ok",
                    src,
                    "+refs/heads/*:refs/heads/*",
                    "+refs/tags/*:refs/tags/*",
                    "+refs/remotes/*:refs/remotes/*",
                ],
                dest,
                300,
            ),
            "Could not copy branches and tags",
        )?;

        let remotes = run_git(&["remote"], src, 5);
        for name in remotes.stdout.lines().map(str::trim).filter(|n| !n.is_empty()) {
            let url = run_git(&["remote", "get-url", name], src, 5);
            step(run_git(&["remote", "add", name, url.out()], dest, 5), "Could not copy the remotes")?;
            let push_url = run_git(&["config", "--get", &format!("remote.{name}.pushurl")], src, 5);
            if push_url.succeeded() && !push_url.out().is_empty() {
                step(
                    run_git(&["remote", "set-url", "--push", name, push_url.out()], dest, 5),
                    "Could not copy the remotes",
                )?;
            }
        }

        // Upstream tracking (branch.*.remote / merge / pushRemote) and a fork's push default.
        let branch_config = run_git(&["config", "--get-regexp", r"^branch\."], src, 5);
        let push_default = run_git(&["config", "--get", "remote.pushDefault"], src, 5);
        let settings = branch_config
            .stdout
            .lines()
            .filter_map(|line| line.split_once(' '))
            .chain(push_default.succeeded().then(|| ("remote.pushDefault", push_default.out())));
        for (key, value) in settings {
            step(run_git(&["config", key, value], dest, 5), "Could not copy branch settings")?;
        }

        if repo.branch.starts_with("detached@") {
            let head = run_git(&["rev-parse", "HEAD"], src, 5);
            step(run_git(&["checkout", "--quiet", "--detach", head.out()], dest, 120), "Could not check out HEAD")?;
        }
        Ok(())
    })();

    (result, transcript.join("\n\n"))
}

/// Adds an `origin` remote to a local-only repository and checks it.
pub fn add_remote_origin(remote_url: &str, repo: &GitRepo) -> (GitRepo, ActivityLogEntry) {
    let res = run_git(&["remote", "add", "origin", remote_url], &repo.path, 10);
    if !res.succeeded() {
        let mut inspected = inspect_local_repository(repo);
        let err = res.combined_output();
        inspected.sync_state = SyncState::Error(first_line(&err).unwrap_or_else(|| "git remote add failed".into()));
        inspected.last_command_output = format!("[$ {}]\n{err}", res.command_string);
        let log = ActivityLogEntry::new(
            repo,
            "Add Remote",
            res.command_string.clone(),
            false,
            err,
            inspected.last_command_output.clone(),
        );
        return (inspected, log);
    }
    check_remote_status(repo)
}

fn fetch_commits(range: &[String], path: &str, max_count: usize) -> Vec<CommitInfo> {
    let mut args = vec!["log".to_string()];
    args.extend(range.iter().cloned());
    args.extend(["-n".into(), max_count.to_string(), "--format=%h%x1f%s%x1f%an%x1f%cr".into()]);
    let res = run_git(&args, path, 6);
    if !res.succeeded() {
        return Vec::new();
    }
    res.stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\u{1f}').collect();
            (parts.len() >= 4).then(|| CommitInfo {
                hash: parts[0].to_string(),
                subject: parts[1].to_string(),
                author: parts[2].to_string(),
                relative_date: parts[3].to_string(),
            })
        })
        .collect()
}

struct ReadmeInfo {
    file_name: Option<String>,
    content: Option<String>,
    description: Option<String>,
}

const README_READ_LIMIT: u64 = 24 * 1024;

/// Reads the README and a short description (.git/description, package manifests, or the README itself).
fn load_readme_and_description(root: &Path) -> ReadmeInfo {
    let mut description = std::fs::read_to_string(root.join(".git").join("description"))
        .ok()
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty() && !d.to_lowercase().starts_with("unnamed repository"));

    if description.is_none() {
        description = std::fs::read(root.join("package.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|json| json.get("description")?.as_str().map(|d| d.trim().to_string()))
            .filter(|d| !d.is_empty());
    }
    if description.is_none() {
        description =
            ["Cargo.toml", "pyproject.toml"].iter().find_map(|manifest| manifest_description(&root.join(manifest)));
    }

    const PREFERRED: [&str; 7] =
        ["README.md", "readme.md", "Readme.md", "README.markdown", "README.txt", "README.rst", "README"];
    let file_name = PREFERRED.iter().map(|n| n.to_string()).find(|n| root.join(n).is_file()).or_else(|| {
        let mut entries: Vec<String> = std::fs::read_dir(root)
            .ok()?
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.to_lowercase().starts_with("readme"))
            .collect();
        entries.sort();
        entries.into_iter().next()
    });

    let content = file_name.as_ref().and_then(|name| {
        let file = std::fs::File::open(root.join(name)).ok()?;
        let mut bytes = Vec::new();
        file.take(README_READ_LIMIT).read_to_end(&mut bytes).ok()?;
        let text = String::from_utf8_lossy(&bytes);
        let text = text.trim_start_matches('\u{feff}').trim_end_matches('\u{fffd}').trim();
        (!text.is_empty()).then(|| text.to_string())
    });

    if description.is_none()
        && let Some(text) = &content
    {
        description = text
            .lines()
            .map(str::trim)
            .find(|line| {
                !line.is_empty()
                    && !["#", "!", "[!", "<", "```", "---", "==", "[![", "|"].iter().any(|p| line.starts_with(p))
            })
            .map(str::to_string);
    }

    ReadmeInfo { file_name, content, description }
}

/// Pulls a top-level `description = "..."` out of a TOML manifest without a full TOML parser.
fn manifest_description(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line.trim_matches(['[', ']']).trim().to_string();
            continue;
        }
        if !matches!(section.as_str(), "package" | "project" | "tool.poetry") {
            continue;
        }
        let Some(value) = line.strip_prefix("description").map(str::trim_start) else {
            continue;
        };
        let Some(value) = value.strip_prefix('=') else { continue };
        let value = value.trim().trim_matches(['"', '\'']).trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_parsing() {
        let changes = parse_porcelain_status(" M src/main.rs\n?? notes.txt\nR  old.rs -> new.rs\n");
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0].status, "M");
        assert_eq!(changes[0].path, "src/main.rs");
        assert_eq!(changes[1].status, "??");
        assert_eq!(changes[2].status, "R");
        assert_eq!(changes[2].path, "old.rs -> new.rs");
    }

    #[test]
    fn named_branches() {
        assert!(is_named_branch("main"));
        assert!(is_named_branch("feat/login"));
        assert!(!is_named_branch("detached@abc123"));
        assert!(!is_named_branch("main (unborn)"));
    }

    /// Temporary directory removed on drop (git object files are read-only, hence the helper).
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("gitdaddy-{name}-{}", std::process::id()));
            let _ = crate::platform::delete_permanently(&dir.to_string_lossy());
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self, child: &str) -> String {
            self.0.join(child).to_string_lossy().into_owned()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = crate::platform::delete_permanently(&self.0.to_string_lossy());
        }
    }

    fn git_ok(args: &[&str], cwd: &str) {
        let res = run_git(args, cwd, 30);
        assert!(res.succeeded(), "{} failed: {}", res.command_string, res.combined_output());
    }

    fn clone_with_identity(scratch: &Scratch, remote: &str, name: &str) -> GitRepo {
        git_ok(&["clone", remote, name], &scratch.path(""));
        let path = scratch.path(name);
        git_ok(&["config", "user.name", "GitDaddy Test"], &path);
        git_ok(&["config", "user.email", "test@example.com"], &path);
        git_ok(&["config", "commit.gpgsign", "false"], &path);
        GitRepo::new(name.into(), path)
    }

    fn commit_file(repo: &GitRepo, file: &str, message: &str) {
        std::fs::write(Path::new(&repo.path).join(file), message).unwrap();
        git_ok(&["add", "-A"], &repo.path);
        git_ok(&["commit", "-m", message], &repo.path);
    }

    #[test]
    fn end_to_end_sync_workflow() {
        if git_executable().is_none() {
            eprintln!("git not installed; skipping");
            return;
        }
        let scratch = Scratch::new("e2e");
        let remote = scratch.path("remote.git");
        git_ok(&["-c", "init.defaultBranch=main", "init", "--bare", &remote], &scratch.path(""));

        // Seed the remote from a first clone.
        let alice = clone_with_identity(&scratch, &remote, "alice");
        git_ok(&["checkout", "-b", "main"], &alice.path);
        commit_file(&alice, "README.md", "# Demo\n\nShared demo repository.");
        git_ok(&["push", "-u", "origin", "main"], &alice.path);

        let bob = clone_with_identity(&scratch, &remote, "bob");
        let bob = inspect_local_repository(&bob);
        assert_eq!(bob.sync_state, SyncState::UpToDate);
        assert_eq!(bob.branch, "main");
        assert_eq!(bob.upstream_branch.as_deref(), Some("origin/main"));
        assert_eq!(bob.repo_description.as_deref(), Some("Shared demo repository."));

        // Alice pushes; Bob sees the update after a check, then pulls it.
        commit_file(&alice, "a.txt", "feat: alice change");
        git_ok(&["push"], &alice.path);
        let (bob, log) = check_remote_status(&bob);
        assert!(log.succeeded, "{}", log.output);
        assert_eq!(bob.sync_state, SyncState::Behind(1));
        assert_eq!(bob.incoming_commits[0].subject, "feat: alice change");
        let (bob, log) = pull_repository(&bob, PullStrategy::FastForwardOnly, true);
        assert!(log.succeeded, "{}", log.output);
        assert_eq!(bob.sync_state, SyncState::UpToDate);

        // Bob commits locally, then pushes.
        commit_file(&bob, "b.txt", "feat: bob change");
        let bob = inspect_local_repository(&bob);
        assert_eq!(bob.sync_state, SyncState::Ahead(1));
        assert_eq!(bob.unpushed_commits.len(), 1);
        let (bob, log) = push_repository(&bob);
        assert!(log.succeeded, "{}", log.output);
        assert_eq!(bob.sync_state, SyncState::UpToDate);

        // A new branch is unpublished until GitDaddy publishes it with `push -u`.
        git_ok(&["checkout", "-b", "feat/new-thing"], &bob.path);
        commit_file(&bob, "c.txt", "feat: branch work");
        let bob = inspect_local_repository(&bob);
        assert!(bob.sync_state.is_unpublished_branch(), "{:?}", bob.sync_state);
        let (bob, log) = push_repository(&bob);
        assert!(log.succeeded, "{}", log.output);
        assert_eq!(bob.sync_state, SyncState::UpToDate);
        assert_eq!(bob.upstream_branch.as_deref(), Some("origin/feat/new-thing"));

        // Dirty working tree, then commit & push in one step.
        std::fs::write(Path::new(&bob.path).join("notes.txt"), "wip").unwrap();
        let bob = inspect_local_repository(&bob);
        assert!(bob.is_dirty());
        assert_eq!(bob.local_changes[0].status, "??");
        let (bob, log) = commit_and_push_repository(&bob, "docs: add notes");
        assert!(log.succeeded, "{}", log.output);
        assert!(!bob.is_dirty());
        assert_eq!(bob.sync_state, SyncState::UpToDate);

        // Both sides commit: diverged.
        git_ok(&["checkout", "main"], &bob.path);
        git_ok(&["pull", "--ff-only"], &alice.path);
        commit_file(&alice, "d.txt", "fix: alice again");
        git_ok(&["push"], &alice.path);
        commit_file(&bob, "e.txt", "fix: bob again");
        let (bob, _) = check_remote_status(&bob);
        assert_eq!(bob.sync_state, SyncState::Diverged { ahead: 1, behind: 1 });

        // Local-only repository: empty, then committed, then connected to a remote.
        let local_path = scratch.path("local-only");
        git_ok(&["-c", "init.defaultBranch=main", "init", &local_path], &scratch.path(""));
        git_ok(&["config", "user.name", "GitDaddy Test"], &local_path);
        git_ok(&["config", "user.email", "test@example.com"], &local_path);
        git_ok(&["config", "commit.gpgsign", "false"], &local_path);
        let local = inspect_local_repository(&GitRepo::new("local-only".into(), local_path.clone()));
        assert_eq!(local.sync_state, SyncState::EmptyRepo { has_remote: false });
        commit_file(&local, "x.txt", "init");
        let local = inspect_local_repository(&local);
        assert_eq!(local.sync_state, SyncState::NoRemoteConfigured);
        let second_remote = scratch.path("second.git");
        git_ok(&["init", "--bare", &second_remote], &scratch.path(""));
        let (local, log) = add_remote_origin(&second_remote, &local);
        assert!(log.succeeded, "{}", log.output);
        assert!(local.sync_state.is_unpublished_branch(), "{:?}", local.sync_state);
        let (local, log) = push_repository(&local);
        assert!(log.succeeded, "{}", log.output);
        assert_eq!(local.sync_state, SyncState::UpToDate);

        // Permanent delete copes with git's read-only object files.
        crate::platform::delete_permanently(&alice.path).expect("delete clone");
        assert!(!Path::new(&alice.path).exists());
    }

    #[test]
    fn local_fork_is_an_independent_copy() {
        if git_executable().is_none() {
            return;
        }
        let scratch = Scratch::new("local-fork");
        let remote = scratch.path("remote.git");
        git_ok(&["-c", "init.defaultBranch=main", "init", "--bare", &remote], &scratch.path(""));
        let source = clone_with_identity(&scratch, &remote, "source");
        git_ok(&["checkout", "-b", "main"], &source.path);
        commit_file(&source, "README.md", "# Source");
        git_ok(&["push", "-u", "origin", "main"], &source.path);
        git_ok(&["tag", "v1.0"], &source.path);
        // A local-only branch, an unpushed commit on main, and an uncommitted file.
        git_ok(&["branch", "experiment"], &source.path);
        commit_file(&source, "local.txt", "feat: not pushed yet");
        std::fs::write(Path::new(&source.path).join("scratch.txt"), "uncommitted").unwrap();
        let source = inspect_local_repository(&source);
        assert_eq!(source.sync_state, SyncState::Ahead(1));

        let dest = scratch.path("source-fork");
        let (result, transcript) = fork_locally(&source, &dest);
        assert!(result.is_ok(), "{result:?}\n{transcript}");

        // Same branches, tags, remotes, and tracking, so the status matches without a fetch.
        let copy = inspect_local_repository(&GitRepo::new("source-fork".into(), dest.clone()));
        assert_eq!(copy.branch, "main");
        assert_eq!(copy.sync_state, SyncState::Ahead(1), "{transcript}");
        assert_eq!(copy.remote_url.as_deref(), Some(remote.as_str()));
        assert_eq!(copy.upstream_branch.as_deref(), Some("origin/main"));
        let branches = run_git(&["branch", "--format=%(refname:short)"], &dest, 5);
        assert!(branches.stdout.lines().any(|b| b == "experiment"), "{}", branches.stdout);
        assert_eq!(run_git(&["tag"], &dest, 5).out(), "v1.0");

        // Uncommitted work stays in the original, and the copy has no link back to it.
        assert!(!Path::new(&dest).join("scratch.txt").exists());
        assert!(Path::new(&dest).join("local.txt").exists());
        assert_eq!(run_git(&["remote"], &dest, 5).out(), "origin");
        // The copy doesn't inherit the source's local identity, and CI runners have no global one.
        git_ok(&["config", "user.name", "GitDaddy Test"], &dest);
        git_ok(&["config", "user.email", "test@example.com"], &dest);
        commit_file(&copy, "only-in-copy.txt", "feat: copy work");
        assert!(!Path::new(&source.path).join("only-in-copy.txt").exists());
        let source_log = run_git(&["log", "--oneline", "-1"], &source.path, 5);
        assert!(!source_log.out().contains("copy work"));
    }

    #[test]
    fn unreachable_remote_reports_error() {
        if git_executable().is_none() {
            return;
        }
        let scratch = Scratch::new("unreachable");
        let path = scratch.path("repo");
        git_ok(&["-c", "init.defaultBranch=main", "init", &path], &scratch.path(""));
        git_ok(&["config", "user.name", "GitDaddy Test"], &path);
        git_ok(&["config", "user.email", "test@example.com"], &path);
        git_ok(&["config", "commit.gpgsign", "false"], &path);
        let repo = GitRepo::new("repo".into(), path.clone());
        commit_file(&repo, "x.txt", "init");
        git_ok(&["remote", "add", "origin", &scratch.path("missing.git")], &path);
        let (repo, log) = check_remote_status(&inspect_local_repository(&repo));
        assert!(!log.succeeded);
        assert!(matches!(repo.sync_state, SyncState::Error(_)), "{:?}", repo.sync_state);
    }
}
