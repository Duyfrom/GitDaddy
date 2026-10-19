use std::cmp::Ordering;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::util::{abbreviate_home, natural_cmp};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncState {
    Unknown,
    Checking,
    Pulling,
    Pushing,
    Deleting,
    UpToDate,
    Behind(usize),
    Ahead(usize),
    Diverged { ahead: usize, behind: usize },
    UnpublishedBranch { branch: String, remote: String },
    NoRemoteConfigured,
    EmptyRepo { has_remote: bool },
    DetachedHead(String),
    Error(String),
}

impl SyncState {
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Checking | Self::Pulling | Self::Pushing | Self::Deleting)
    }

    pub fn needs_update(&self) -> bool {
        self.behind_count() > 0
    }

    pub fn can_push(&self) -> bool {
        self.ahead_count() > 0 || self.is_unpublished_branch()
    }

    pub fn is_unpublished_branch(&self) -> bool {
        matches!(self, Self::UnpublishedBranch { .. })
    }

    pub fn is_diverged(&self) -> bool {
        matches!(self, Self::Diverged { .. })
    }

    pub fn can_pull(&self) -> bool {
        match self {
            Self::NoRemoteConfigured
            | Self::EmptyRepo { .. }
            | Self::DetachedHead(_)
            | Self::UnpublishedBranch { .. } => false,
            _ => !self.is_busy(),
        }
    }

    pub fn is_issue_or_no_upstream(&self) -> bool {
        matches!(
            self,
            Self::UnpublishedBranch { .. }
                | Self::NoRemoteConfigured
                | Self::EmptyRepo { .. }
                | Self::DetachedHead(_)
                | Self::Error(_)
        )
    }

    pub fn is_ahead_or_diverged(&self) -> bool {
        matches!(self, Self::Ahead(_) | Self::Diverged { .. } | Self::UnpublishedBranch { .. })
    }

    pub fn behind_count(&self) -> usize {
        match self {
            Self::Behind(count) => *count,
            Self::Diverged { behind, .. } => *behind,
            _ => 0,
        }
    }

    pub fn ahead_count(&self) -> usize {
        match self {
            Self::Ahead(count) => *count,
            Self::Diverged { ahead, .. } => *ahead,
            _ => 0,
        }
    }

    pub fn sort_priority(&self) -> u8 {
        match self {
            Self::Behind(_) => 0,
            Self::Diverged { .. } => 1,
            Self::Ahead(_) => 2,
            Self::UnpublishedBranch { .. } => 3,
            Self::Error(_) => 4,
            Self::Pulling | Self::Pushing | Self::Checking | Self::Deleting => 5,
            Self::Unknown => 6,
            Self::UpToDate => 7,
            Self::DetachedHead(_) => 8,
            Self::EmptyRepo { .. } => 9,
            Self::NoRemoteConfigured => 10,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Unknown => "Not Checked".into(),
            Self::Checking => "Checking Remote…".into(),
            Self::Pulling => "Pulling Updates…".into(),
            Self::Pushing => "Pushing Commits…".into(),
            Self::Deleting => "Deleting…".into(),
            Self::UpToDate => "Up to Date".into(),
            Self::Behind(count) => {
                format!("{count} {} Available", if *count == 1 { "Update" } else { "Updates" })
            }
            Self::Ahead(count) => format!("{count} Unpushed"),
            Self::Diverged { ahead, behind } => format!("Diverged (↑{ahead} ↓{behind})"),
            Self::UnpublishedBranch { .. } => "Unpublished Branch".into(),
            Self::NoRemoteConfigured => "Local Only (No Remote)".into(),
            Self::EmptyRepo { .. } => "Empty Repo (0 Commits)".into(),
            Self::DetachedHead(commit) => format!("Detached HEAD (@{commit})"),
            Self::Error(_) => "Sync Error".into(),
        }
    }

    pub fn row_explanation(&self) -> Option<String> {
        match self {
            Self::UnpublishedBranch { branch, remote } => {
                Some(format!("Branch '{branch}' not pushed to '{remote}' yet"))
            }
            Self::NoRemoteConfigured => Some("Local-only repository — no remote URL configured".into()),
            Self::EmptyRepo { has_remote } => Some(if *has_remote {
                "Remote URL configured, 0 commits locally".into()
            } else {
                "Initialized repository with 0 commits and no remote".into()
            }),
            Self::DetachedHead(commit) => Some(format!("Detached HEAD at {commit}")),
            Self::Error(msg) => Some(msg.clone()),
            _ => None,
        }
    }

    pub fn inspector_explanation(&self) -> String {
        let plural = |n: usize| if n == 1 { "commit" } else { "commits" };
        match self {
            Self::Unknown => "Click 'Check' to fetch from the remote repository and compare commit histories.".into(),
            Self::Checking => "Fetching latest branch references from the remote server…".into(),
            Self::Pulling => "Pulling incoming commits from the remote tracking branch…".into(),
            Self::Pushing => "Pushing local commits to the remote repository…".into(),
            Self::Deleting => "Removing the local repository folder…".into(),
            Self::UpToDate => "Your local branch and remote tracking branch have the exact same HEAD commit.".into(),
            Self::Behind(count) => format!(
                "The remote branch has {count} new {} that you haven't pulled locally yet.",
                plural(*count)
            ),
            Self::Ahead(count) => format!(
                "You have {count} local {} not yet pushed to the remote branch.",
                plural(*count)
            ),
            Self::Diverged { ahead, behind } => format!(
                "Your local branch has {ahead} unpushed {} and the remote has {behind} incoming {}. Pull before pushing.",
                plural(*ahead),
                plural(*behind)
            ),
            Self::UnpublishedBranch { branch, remote } => format!(
                "Connected to remote '{remote}', but current branch '{branch}' only exists on this PC. Click 'Publish' to run 'git push -u {remote} {branch}'."
            ),
            Self::NoRemoteConfigured => "Initialized with Git locally, but has no remote server URL ('origin'). Add a remote URL below to connect it.".into(),
            Self::EmptyRepo { has_remote } => if *has_remote {
                "Remote URL is configured, but no commits exist yet (unborn HEAD). Commit a file to create your first commit.".into()
            } else {
                "This repository has no commits and no remote URL configured yet.".into()
            },
            Self::DetachedHead(commit) => format!(
                "Git is in 'Detached HEAD' mode at commit {commit} rather than on a named branch. Switch to a branch to pull or push."
            ),
            Self::Error(msg) => msg.clone(),
        }
    }

    pub fn status_kind(&self) -> StatusKind {
        match self {
            Self::Unknown => StatusKind::Draft,
            Self::Checking | Self::Pulling | Self::Pushing | Self::Deleting => StatusKind::InProgress,
            Self::UpToDate => StatusKind::Success,
            Self::Behind(_) => StatusKind::NeedsUpdate,
            Self::Diverged { .. } => StatusKind::Diverged,
            Self::Ahead(_) => StatusKind::Unpushed,
            Self::UnpublishedBranch { .. } => StatusKind::Unpublished,
            Self::NoRemoteConfigured | Self::EmptyRepo { .. } | Self::DetachedHead(_) => StatusKind::Disabled,
            Self::Error(_) => StatusKind::Danger,
        }
    }
}

/// Visual status category: green = good, yellow = notice, red = bad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusKind {
    AllRepos,
    Success,
    InProgress,
    Draft,
    NeedsUpdate,
    Dirty,
    Unpushed,
    Unpublished,
    Diverged,
    Danger,
    Disabled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitInfo {
    pub hash: String,
    pub subject: String,
    pub author: String,
    pub relative_date: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub status: String,
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PullStrategy {
    #[default]
    Merge,
    Rebase,
    FastForwardOnly,
}

impl PullStrategy {
    pub const ALL: [Self; 3] = [Self::Merge, Self::Rebase, Self::FastForwardOnly];

    pub fn label(self) -> &'static str {
        match self {
            Self::Merge => "Merge (Default)",
            Self::Rebase => "Rebase (--rebase)",
            Self::FastForwardOnly => "Fast-Forward (--ff-only)",
        }
    }

    pub fn git_arguments(self) -> Vec<String> {
        let args: &[&str] = match self {
            Self::Merge => &["pull"],
            Self::Rebase => &["pull", "--rebase"],
            Self::FastForwardOnly => &["pull", "--ff-only"],
        };
        args.iter().map(|s| s.to_string()).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepoFilter {
    All,
    NeedsUpdate,
    UpToDate,
    LocalChanges,
    AheadOrDiverged,
    Errors,
}

impl RepoFilter {
    /// In header-tab order.
    pub const ALL: [Self; 6] =
        [Self::All, Self::UpToDate, Self::NeedsUpdate, Self::LocalChanges, Self::AheadOrDiverged, Self::Errors];

    /// Short label for the header tabs.
    pub fn tab_label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::NeedsUpdate => "Updates",
            Self::UpToDate => "Up to Date",
            Self::LocalChanges => "Dirty",
            Self::AheadOrDiverged => "Unpushed",
            Self::Errors => "Issues",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All Repos",
            Self::NeedsUpdate => "Needs Update",
            Self::UpToDate => "Up to Date",
            Self::LocalChanges => "Dirty / Uncommitted",
            Self::AheadOrDiverged => "Unpushed / Unpublished",
            Self::Errors => "Local Only / Issues",
        }
    }

    pub fn status_preview(self) -> StatusKind {
        match self {
            Self::All => StatusKind::AllRepos,
            Self::NeedsUpdate => StatusKind::NeedsUpdate,
            Self::UpToDate => StatusKind::Success,
            Self::LocalChanges => StatusKind::Dirty,
            Self::AheadOrDiverged => StatusKind::Unpushed,
            Self::Errors => StatusKind::Danger,
        }
    }

    pub fn matches(self, repo: &GitRepo) -> bool {
        match self {
            Self::All => true,
            Self::NeedsUpdate => repo.sync_state.needs_update(),
            Self::UpToDate => repo.sync_state == SyncState::UpToDate,
            Self::LocalChanges => repo.is_dirty(),
            Self::AheadOrDiverged => repo.sync_state.is_ahead_or_diverged(),
            Self::Errors => repo.sync_state.is_issue_or_no_upstream(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepoSortOrder {
    StatusPriority,
    Name,
    LastUpdated,
    Path,
}

impl RepoSortOrder {
    pub const ALL: [Self; 4] = [Self::StatusPriority, Self::Name, Self::LastUpdated, Self::Path];

    pub fn label(self) -> &'static str {
        match self {
            Self::StatusPriority => "Update Status",
            Self::Name => "Name (A–Z)",
            Self::LastUpdated => "Recent Commit",
            Self::Path => "Folder Path",
        }
    }

    pub fn compare(self, lhs: &GitRepo, rhs: &GitRepo) -> Ordering {
        match self {
            Self::StatusPriority => lhs
                .sync_state
                .sort_priority()
                .cmp(&rhs.sync_state.sort_priority())
                .then_with(|| rhs.sync_state.behind_count().cmp(&lhs.sync_state.behind_count()))
                .then_with(|| natural_cmp(&lhs.name, &rhs.name)),
            Self::Name => natural_cmp(&lhs.name, &rhs.name),
            Self::LastUpdated => rhs
                .last_commit_timestamp
                .cmp(&lhs.last_commit_timestamp)
                .then_with(|| natural_cmp(&lhs.name, &rhs.name)),
            Self::Path => natural_cmp(&lhs.path, &rhs.path),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ActivityLogEntry {
    pub timestamp: SystemTime,
    pub repo_name: String,
    pub action: String,
    pub command: String,
    pub succeeded: bool,
    pub summary: String,
    pub output: String,
}

impl ActivityLogEntry {
    pub fn new(
        repo: &GitRepo,
        action: &str,
        command: impl Into<String>,
        succeeded: bool,
        summary: impl Into<String>,
        output: impl Into<String>,
    ) -> Self {
        Self {
            timestamp: SystemTime::now(),
            repo_name: repo.name.clone(),
            action: action.into(),
            command: command.into(),
            succeeded,
            summary: summary.into(),
            output: output.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GitRepo {
    pub name: String,
    pub path: String,

    pub branch: String,
    pub upstream_branch: Option<String>,
    pub primary_remote_name: Option<String>,
    pub remote_url: Option<String>,
    /// URL of the original project when this clone is a fork (its `upstream` remote).
    pub fork_parent_url: Option<String>,
    /// Where `git push` goes when it differs from the tracking remote (forks).
    pub push_remote: Option<String>,
    pub sync_state: SyncState,

    pub incoming_commits: Vec<CommitInfo>,
    pub unpushed_commits: Vec<CommitInfo>,
    pub local_changes: Vec<FileChange>,

    pub last_commit_hash: String,
    pub last_commit_message: String,
    pub last_commit_relative_date: String,
    /// Committer date as a Unix timestamp.
    pub last_commit_timestamp: Option<i64>,

    pub last_checked_at: Option<SystemTime>,
    pub last_command_output: String,

    pub repo_description: Option<String>,
    pub readme_file_name: Option<String>,
    pub readme_content: Option<String>,
}

impl GitRepo {
    pub fn new(name: String, path: String) -> Self {
        Self {
            name,
            path,
            branch: "—".into(),
            upstream_branch: None,
            primary_remote_name: None,
            remote_url: None,
            fork_parent_url: None,
            push_remote: None,
            sync_state: SyncState::Unknown,
            incoming_commits: Vec::new(),
            unpushed_commits: Vec::new(),
            local_changes: Vec::new(),
            last_commit_hash: String::new(),
            last_commit_message: String::new(),
            last_commit_relative_date: String::new(),
            last_commit_timestamp: None,
            last_checked_at: None,
            last_command_output: String::new(),
            repo_description: None,
            readme_file_name: None,
            readme_content: None,
        }
    }

    pub fn is_dirty(&self) -> bool {
        !self.local_changes.is_empty()
    }

    pub fn has_remote(&self) -> bool {
        self.remote_url.is_some()
    }

    pub fn has_commits(&self) -> bool {
        !self.last_commit_hash.is_empty()
    }

    pub fn display_path(&self) -> String {
        abbreviate_home(&self.path)
    }

    pub fn formatted_remote_host(&self) -> Option<String> {
        let url = self.remote_url.as_deref().filter(|u| !u.is_empty())?;
        Some(match parse_remote(url) {
            Some((host, path)) if path.is_empty() => host,
            Some((host, path)) => format!("{host}/{path}"),
            None => url.to_string(),
        })
    }

    /// `owner/name` on GitHub, e.g. `collabs-inc/collab-public`.
    pub fn github_slug(&self) -> Option<String> {
        let (host, path) = parse_remote(self.remote_url.as_deref()?)?;
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        (host == "github.com" && segments.len() == 2).then(|| segments.join("/"))
    }

    /// GitHub repositories that are not already a fork of something can be forked.
    pub fn can_fork(&self) -> bool {
        self.fork_parent_url.is_none() && self.github_slug().is_some()
    }

    /// The original project of a fork, shown as `owner/name` (with the host when not GitHub).
    pub fn fork_parent_display(&self) -> Option<String> {
        let url = self.fork_parent_url.as_deref()?;
        Some(match parse_remote(url) {
            Some((host, path)) if host == "github.com" => path,
            Some((host, path)) => format!("{host}/{path}"),
            None => url.to_string(),
        })
    }

    /// The account or organization that owns the remote, e.g. `Duyfrom` for
    /// `git@github.com:Duyfrom/GitDaddy.git`. `None` for local-path remotes.
    pub fn remote_owner(&self) -> Option<RemoteOwner> {
        let (host, path) = parse_remote(self.remote_url.as_deref()?)?;
        let mut segments = path.split('/').filter(|s| !s.is_empty());
        let name = segments.next()?.to_string();
        segments.next()?;
        Some(RemoteOwner { host, name })
    }

    /// One-line summary for the table: incoming commit, state explanation, latest commit, or path.
    pub fn status_or_commit_summary(&self) -> String {
        if let Some(incoming) = self.incoming_commits.first() {
            return format!("{} — {}", self.sync_state.label(), incoming.subject);
        }
        if let Some(explanation) = self.sync_state.row_explanation() {
            return explanation;
        }
        if !self.last_commit_message.is_empty() {
            return self.last_commit_message.clone();
        }
        self.display_path()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RemoteOwner {
    pub host: String,
    pub name: String,
}

impl RemoteOwner {
    pub fn key(&self) -> String {
        format!("{}/{}", self.host, self.name)
    }
}

/// Splits a remote URL into `(host, path)`, dropping credentials, ports, and `.git`.
/// Handles `https://`, `ssh://`, `git://`, and scp-style `user@host:path`. Returns `None`
/// for local paths and `file://` URLs.
pub fn parse_remote(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let (authority, path) = if let Some((scheme, rest)) = url.split_once("://") {
        if scheme.eq_ignore_ascii_case("file") {
            return None;
        }
        rest.split_once('/').unwrap_or((rest, ""))
    } else {
        // scp-style `git@github.com:owner/repo`. A single letter before ':' is a drive (C:\repo).
        let (authority, path) = url.split_once(':')?;
        let host_part = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        if host_part.len() < 2 || authority.contains(['/', '\\']) {
            return None;
        }
        (authority, path)
    };
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host.split(':').next().unwrap_or(host).to_lowercase();
    if host.is_empty() {
        return None;
    }
    let path = path.trim_matches('/').trim_end_matches(".git").trim_end_matches('/').to_string();
    Some((host, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(url: &str) -> Option<(String, String)> {
        let mut repo = GitRepo::new("r".into(), "r".into());
        repo.remote_url = Some(url.into());
        repo.remote_owner().map(|o| (o.host, o.name))
    }

    #[test]
    fn remote_owners() {
        let gh = Some(("github.com".to_string(), "Duyfrom".to_string()));
        assert_eq!(owner("git@github.com:Duyfrom/GitDaddy.git"), gh);
        assert_eq!(owner("https://github.com/Duyfrom/GitDaddy"), gh);
        assert_eq!(owner("https://token:x@github.com/Duyfrom/GitDaddy.git/"), gh);
        assert_eq!(owner("ssh://git@GitHub.com:22/Duyfrom/GitDaddy.git"), gh);
        assert_eq!(owner("git@gitlab.com:group/sub/proj.git"), Some(("gitlab.com".into(), "group".into())));
        assert_eq!(owner(r"D:\repos\remote.git"), None);
        assert_eq!(owner("/srv/git/remote.git"), None);
        assert_eq!(owner("file:///srv/git/remote.git"), None);
        assert_eq!(owner("https://example.com/single"), None);
    }

    #[test]
    fn formatted_hosts() {
        let mut repo = GitRepo::new("r".into(), "r".into());
        repo.remote_url = Some("git@github.com:collabs-inc/collab-public.git".into());
        assert_eq!(repo.formatted_remote_host().as_deref(), Some("github.com/collabs-inc/collab-public"));
        repo.remote_url = Some(r"D:\repos\remote.git".into());
        assert_eq!(repo.formatted_remote_host().as_deref(), Some(r"D:\repos\remote.git"));
    }
}
