//! Sample repositories used to render README screenshots headlessly.

use std::time::SystemTime;

use crate::model::{CommitInfo, FileChange, GitRepo, SyncState};

fn commit(hash: &str, subject: &str, author: &str, when: &str) -> CommitInfo {
    CommitInfo { hash: hash.into(), subject: subject.into(), author: author.into(), relative_date: when.into() }
}

fn change(status: &str, path: &str) -> FileChange {
    FileChange { status: status.into(), path: path.into() }
}

fn repo(name: &str, state: SyncState, last: (&str, &str, &str, i64)) -> GitRepo {
    let home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_else(|| r"C:\Users\you".into());
    let mut r = GitRepo::new(name.into(), format!(r"{home}\Projects\{name}"));
    r.branch = "main".into();
    r.upstream_branch = Some("origin/main".into());
    r.primary_remote_name = Some("origin".into());
    r.remote_url = Some(format!("git@github.com:acme-corp/{name}.git"));
    r.sync_state = state;
    r.last_commit_hash = last.0.into();
    r.last_commit_message = last.1.into();
    r.last_commit_relative_date = last.2.into();
    r.last_commit_timestamp = Some(1_760_000_000 - last.3);
    r.last_checked_at = Some(SystemTime::now());
    r.last_command_output = "[$ git fetch --all --prune --quiet]\nFetch completed successfully.".into();
    r
}

const DESKTOP_README: &str = r#"# Desktop Client

Native desktop client with offline-first synchronization and secure, OS-backed credential storage.

## Highlights

- **Offline Queue** — Persists mutations locally and replays them when connectivity returns.
- **Zero-Downtime Auth** — Automatic OAuth2 token rotation with Windows Hello support.
- **Modular Architecture** — Split into `core-network`, `sync-engine`, and `design-kit` crates.

## Getting Started

```powershell
git clone git@github.com:acme-corp/desktop-client.git
cd desktop-client
cargo run --release
```
"#;

pub fn repositories() -> Vec<GitRepo> {
    let mut desktop = repo(
        "desktop-client",
        SyncState::Behind(3),
        ("2a7f910", "refactor: split networking layer", "1 day ago", 86_400),
    );
    desktop.incoming_commits = vec![
        commit("e41b0c2", "feat(sync): resume uploads after reconnect", "Mia Chen", "2 hours ago"),
        commit("9d03a7f", "fix(auth): refresh tokens before expiry", "Jonas Berg", "5 hours ago"),
        commit("51c8e2d", "chore: bump wgpu to 27", "Mia Chen", "1 day ago"),
    ];
    desktop.repo_description =
        Some("Native desktop client with offline-first sync engine and background refresh.".into());
    desktop.readme_file_name = Some("README.md".into());
    desktop.readme_content = Some(DESKTOP_README.into());

    let mut cli =
        repo("cli-toolkit", SyncState::Ahead(2), ("b7e2c41", "feat(output): add JSON reporter", "12 minutes ago", 720));
    cli.unpushed_commits = vec![
        commit("b7e2c41", "feat(output): add JSON reporter", "You", "12 minutes ago"),
        commit("4f19ad0", "test: cover table renderer edge cases", "You", "40 minutes ago"),
    ];
    cli.local_changes = vec![change("M", r"src\commands\report.rs"), change("??", r"docs\json-output.md")];
    cli.repo_description = Some("Composable command-line utilities for release automation.".into());
    cli.readme_file_name = Some("README.md".into());
    cli.readme_content = Some("# cli-toolkit\n\nComposable command-line utilities for release automation.\n\n- `ct report` — summarize a release\n- `ct notes` — draft changelog entries".into());
    cli.last_command_output = "[$ git commit -m \"feat(output): add JSON reporter\"]\n[main b7e2c41] feat(output): add JSON reporter\n 3 files changed, 148 insertions(+), 12 deletions(-)".into();

    let mut auth = repo(
        "auth-service",
        SyncState::UnpublishedBranch { branch: "feat/passkeys".into(), remote: "origin".into() },
        ("c03d5be", "feat: passkey registration flow", "30 minutes ago", 1_800),
    );
    auth.branch = "feat/passkeys".into();
    auth.upstream_branch = None;
    auth.unpushed_commits = vec![commit("c03d5be", "feat: passkey registration flow", "You", "30 minutes ago")];

    let analytics = repo(
        "analytics-engine",
        SyncState::UpToDate,
        ("77aa0e1", "chore: update ClickHouse schema", "2 days ago", 172_800),
    );
    let gateway = repo(
        "api-gateway",
        SyncState::UpToDate,
        ("0b1c9f4", "perf(proxy): reuse upstream connections", "3 hours ago", 10_800),
    );
    let mut design = repo(
        "design-system",
        SyncState::UpToDate,
        ("e5d2a90", "feat(tokens): refine elevation scale", "1 day ago", 90_000),
    );
    design.local_changes = vec![change("M", r"tokens\color.json")];
    let mut docs =
        repo("docs-portal", SyncState::UpToDate, ("3c8f1d2", "docs(api): add webhook guide", "4 days ago", 345_600));

    let mut billing = repo(
        "billing-worker",
        SyncState::Diverged { ahead: 1, behind: 2 },
        ("8e0d4c7", "fix: retry failed invoice exports", "6 hours ago", 21_600),
    );
    billing.incoming_commits = vec![
        commit("a19e33b", "feat: support partial refunds", "Ana Souza", "3 hours ago"),
        commit("72cc5e0", "chore: rotate webhook secret", "Ana Souza", "5 hours ago"),
    ];
    billing.unpushed_commits = vec![commit("8e0d4c7", "fix: retry failed invoice exports", "You", "6 hours ago")];

    let mut sandbox = repo(
        "sandbox-prototype",
        SyncState::NoRemoteConfigured,
        ("19f0a3e", "wip: try a new layout engine", "5 days ago", 432_000),
    );
    sandbox.remote_url = None;
    sandbox.primary_remote_name = None;
    sandbox.upstream_branch = None;
    sandbox.last_command_output.clear();

    // Mix owners and folders so the sidebar has something to show: work repos, a personal
    // repo, and open-source clones (one on GitLab) in a second watched folder.
    cli.remote_url = Some("git@github.com:jordan-dev/cli-toolkit.git".into());
    move_to(&mut docs, "OpenSource", "https://github.com/opensource-guild/docs-portal.git");
    move_to(&mut design, "OpenSource", "git@gitlab.com:design-guild/design-system.git");

    vec![desktop, cli, auth, analytics, gateway, design, docs, billing, sandbox]
}

/// Watched folders the sample repositories live in.
pub fn folders() -> Vec<String> {
    let home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_else(|| r"C:\Users\you".into());
    vec![format!(r"{home}\Projects"), format!(r"{home}\OpenSource")]
}

fn move_to(repo: &mut GitRepo, folder: &str, remote: &str) {
    let home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_else(|| r"C:\Users\you".into());
    repo.path = format!(r"{home}\{folder}\{}", repo.name);
    repo.remote_url = Some(remote.into());
}
