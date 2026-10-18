<p align="center">
  <img src="Assets/logo.png" width="200" alt="GitDaddy Logo" />
</p>

<h1 align="center">GitDaddy</h1>

<p align="center">
  <strong>A fast, native macOS desktop app to monitor, pull, push, and manage all of your local Git repositories in one place.</strong>
</p>

<p align="center">
  <img src="Assets/screenshot-dashboard.png" width="100%" alt="GitDaddy Main Workspace Dashboard" />
</p>

---

## Overview

When you work across dozens of repositories on your Mac, it is easy to lose track of which folders are behind `origin`, which ones have unpushed commits or uncommitted changes, and which ones are local-only prototypes.

**GitDaddy** scans the workspace folders you choose, compares your local branches against their remotes (`git fetch`), and gives you a single dashboard to check, pull, push, inspect `README.md` files, and clean up repositories individually or in batch.

---

## Feature Tour

### 1. First-Launch Workspace Onboarding
Choose any parent folder on your Mac when you first open GitDaddy. No paths are hardcoded—GitDaddy automatically discovers every Git repository inside your selected workspace folders.

<p align="center">
  <img src="Assets/screenshot-onboarding.png" width="100%" alt="GitDaddy Workspace Onboarding Screen" />
</p>

### 2. Traffic-Light Status Dashboard & Collapsible README Inspector
See at a glance which repositories are **Up to Date** (green), **Need Attention** (yellow: updates available `↓`, unpushed commits `↑`, unpublished branch `new`, or uncommitted changes `M`), or have **Issues** (red: diverged branches, sync errors, or local-only repos). Select any repository to view its description and `README.md` right below the sync controls.

<p align="center">
  <img src="Assets/screenshot-dashboard.png" width="100%" alt="GitDaddy Dashboard and Collapsible README Viewer" />
</p>

### 3. Batch Selection, Push Unpushed Commits & Publish Branches
Select multiple repositories with checkboxes to batch-check, batch-pull, batch-push, or clean up local clones—and inspect exact unpushed commits and branch status in the right-hand inspector.

<p align="center">
  <img src="Assets/screenshot-inspector.png" width="100%" alt="GitDaddy Unpushed Commits and Batch Selection" />
</p>

---

## Key Features

- **Automatic Workspace Discovery** — Select one or more parent folders during onboarding and GitDaddy finds every Git repository inside automatically (with optional nested repository scanning).
- **Clear Traffic-Light Status System**:
  - **Green (`Up to Date`)** — Your local branch matches the remote tracking branch.
  - **Yellow (`Needs Attention`)** — Incoming updates are available to pull (`↓`), local commits are waiting to be pushed (`↑`), a local branch has not been published yet (`new`), or you have uncommitted file edits (`M`).
  - **Red (`Issue`)** — Local and remote branches have diverged, a sync error occurred, or no remote URL is configured.
- **Single, Multi-Select, or Batch Actions** — Pull or push a single repository, select multiple repositories with checkboxes, or update every outdated repository with one click.
- **Right-Hand Inspector & Collapsible README Viewer** — Click any repository to view its `README.md` and description, inspect incoming and unpushed commits, review modified files, and read raw Git command output.
- **Quick Commit & Push** — Stage all modified files, enter a commit message, and push directly from the inspector.
- **Safe Local Repository Cleanup** — Remove old local repositories from your Mac (**Move to Trash** or **Delete Permanently**) with automatic warnings if the folder has unpushed commits or uncommitted changes.

---

## Requirements

- **macOS 14.0 (Sonoma)** or later
- **Git** (`/usr/bin/git` or Homebrew Git)
- **Xcode** or **Xcode Command Line Tools** (`xcode-select --install`) when building from source

---

## How to Clone and Use as a Desktop App

### Option 1: One-Command Desktop App Install (Recommended)

Cloning the repository and running `./install.sh` compiles the native release app, bundles the custom `GitDaddy` icon, installs **`GitDaddy.app`** into `/Applications` (so you can launch it from **Spotlight**, **Launchpad**, or the **Dock** like any standard Mac app), and opens it immediately:

```bash
git clone https://github.com/Duyfrom/GitDaddy.git
cd GitDaddy
./install.sh
```

### Option 2: Build Locally in the Repo Folder

If you prefer to build `GitDaddy.app` inside the cloned folder without copying it to `/Applications`:

```bash
git clone https://github.com/Duyfrom/GitDaddy.git
cd GitDaddy
./build_app.sh
open GitDaddy.app
```

### Option 3: Download Pre-Built Release

1. Download **`GitDaddy-macOS.zip`** from the [Releases](https://github.com/Duyfrom/GitDaddy/releases) page.
2. Unzip and drag **`GitDaddy.app`** into your `/Applications` folder.
3. If macOS Gatekeeper blocks the first launch (because the app is ad-hoc signed), run:
   ```bash
   xattr -cr /Applications/GitDaddy.app
   open /Applications/GitDaddy.app
   ```

---

## How to Use GitDaddy

### 1. Select Your Workspace Folder (First Launch)
1. When you first open GitDaddy, click **Choose Workspace Folder…** on the welcome screen (or click **Add Folder…** / press `⌘O` in the left sidebar) and select the folder where you keep your Git projects.
2. GitDaddy remembers your selected workspace folders automatically for future launches. You can add or remove watched folders in the left sidebar at any time.
3. Toggle **Scan nested repos** in the sidebar if you keep Git repositories inside deeper sub-projects.

### 2. Check for Remote Updates
- Click **Check All** in the top bar (or press `⌘R`) to run `git fetch --all --prune` across all repositories in parallel.
- To check a single repository, click the refresh icon (`↻`) on its row.

### 3. Pull Incoming Updates
- **Single repository**: Click the down-arrow (`↓`) on the repository row, or select the row and click **Pull** in the right inspector.
- **Multiple repositories**: Check the boxes next to the repositories you want to update, then click **Pull (N)** in the bottom bar.
- **All outdated repositories**: Click **Update Outdated (N)** in the top-right bar to pull every repository that has incoming commits.
- **Pull Mode**: Under **Pull Mode** in the left sidebar, choose your preferred strategy:
  - *Merge (Default)* — Standard `git pull`
  - *Rebase (`--rebase`)* — Replays your local commits on top of incoming changes
  - *Fast-Forward (`--ff-only`)* — Updates only if the branch can fast-forward cleanly
  - *Auto-stash local edits* — Automatically runs `--autostash` so uncommitted local edits are stashed before pulling and re-applied afterward.

### 4. Push Local Commits or Publish a Branch
- **Push commits**: When a repository shows `↑N`, click the up-arrow (`↑`) on its row or click **Push Unpushed (N)** in the top bar.
- **Publish a new branch**: When a repository shows **Unpublished Branch** (`new`), click **Publish** to run `git push -u origin <branch>`.
- **Commit & Push**: Select a repository with uncommitted changes (`M`), enter a commit message in the right inspector under **Uncommitted Changes**, and click **Commit & Push**.

### 5. Inspect README, Commits, and Logs
- Click any repository row to open the right-hand inspector:
  - **README & Description** — View the repository's description and formatted `README.md` right below the sync status controls (click **Hide** / **Show** to collapse or expand).
  - **Incoming / Unpushed Commits** — Inspect commit hashes, messages, and relative timestamps.
  - **Git Output** — View the exact console output from the latest Git command.
- Click the **Activity Log** icon in the top-right bar to view a timestamped log of all Git operations in the current session.

### 6. Delete a Local Repository
- Click the trash icon in the right inspector (or open the `•••` menu on any row and select **Delete Local Repository…**).
- Choose **Move to Trash** or **Delete Permanently**.
- GitDaddy warns you beforehand if the repository contains unpushed commits or uncommitted changes. Only the folder on your Mac is removed; remote repositories on GitHub/GitLab are never affected.

---

## Understanding Repository Statuses

| Status | Color | What It Means in Simple Terms |
| :--- | :--- | :--- |
| **Up to Date** | Green | Your local branch and the remote branch are on the exact same commit. |
| **Updates Available (`↓N`)** | Yellow | The remote branch has `N` new commits ready to pull. |
| **Unpushed (`↑N`)** | Yellow | Your local branch has `N` commits that have not been pushed to the remote yet. |
| **Unpublished Branch (`new`)** | Yellow | The repository has a remote configured, but your current branch only exists on your Mac. Click **Publish** to push it. |
| **Dirty (`NM`)** | Yellow | You have `N` modified or untracked files in your working tree that are not committed yet. |
| **Diverged (`↑A ↓B`)** | Red | Both your local branch and the remote branch have new commits. Pull before pushing. |
| **Local Only (No Remote)** | Red | The folder was initialized with `git init` locally, but has no remote URL (`origin`). You can add a remote URL directly in the inspector. |
| **Sync Error** | Red | Git could not reach or update from the remote server. Check the inspector for the exact Git error message. |

---

## Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `⌘O` | Add Folder to Scan |
| `⌘R` | Check All Remotes (`git fetch`) |
| `⇧⌘R` | Rescan Watched Folders |
| `⌘P` | Pull Selected Repositories |
| `⇧⌘P` | Update All Outdated Repositories |
| `⌘U` | Push Selected Repositories |
| `⇧⌘U` | Push All Unpushed Repositories |
| `⇧⌘A` | Select All Visible Repositories |

---

## Project Structure

```text
GitDaddy/
├── Assets/
│   ├── AppIcon.png               # 1024x1024 macOS app icon source
│   ├── logo.png                  # GitDaddy project logo
│   ├── screenshot-dashboard.png  # Main dashboard & README inspector screenshot
│   ├── screenshot-inspector.png  # Multi-select & unpushed commits screenshot
│   └── screenshot-onboarding.png # First-launch workspace onboarding screenshot
├── Package.swift                 # Swift Package Manager manifest (macOS 14+)
├── build_app.sh                  # Builds release binary & packages GitDaddy.app
├── install.sh                    # Builds & installs GitDaddy.app into /Applications
└── Sources/GitDaddy/
    ├── GitDaddyApp.swift         # App entry point, window setup & menu shortcuts
    ├── Models.swift              # Repository, sync state, and filter models
    ├── RepoScanner.swift         # Directory scanner for discovering .git folders
    ├── GitService.swift          # Async non-interactive git CLI runner
    ├── RepoStore.swift           # MainActor state management & batch coordinator
    ├── ContentView.swift         # Main 3-pane workspace UI
    ├── InspectorView.swift       # Right inspector & collapsible README viewer
    └── Theme.swift               # Color tokens, typography & status indicators
```

---

## License

Released under the [MIT License](LICENSE).
