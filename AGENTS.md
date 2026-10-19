# Notes for agents

This file is for coding agents picking up work on GitDaddy. The [README](README.md) explains the features and lists every source file. This file covers what you'd otherwise have to rediscover: how to build and test, where the app keeps state, why Explore works the way it does, and what's still open. Facts with a date were true on that date, so check them before you rely on them.

## Working with the maintainer

- Write commit messages, docs, and UI text in plain English: short sentences, no marketing words. The README is the reference for tone.
- Don't launch GitDaddy or bring windows to the front on the maintainer's PC. They may be in a fullscreen app. Check UI changes with the headless screenshot tests below, then look at the PNGs.
- Prefer free APIs that need no account. Ask before adding anything that needs a key or costs money.
- Explore shows every repository. Don't add NSFW or other content filtering. The maintainer had it removed on purpose.
- Commit and push only when asked. One request covers the work it was given for, not later work.

## Repository

Two apps share this repository and aim for the same features.

- **macOS:** SwiftUI. `Package.swift`, `Sources/GitDaddy/`, `build_app.sh`, `install.sh`.
- **Windows:** Rust and egui 0.36. Everything is under `windows/`.

State on 2026-10-19:

- `main` is at `d4265c8`: the Windows app plus the macOS port of its features.
- `windows-app` adds the Trendshift-style Explore, a fix for the test that broke CI, and this file. No pull request is open.
- The only tag is `v1.0.0`, a macOS-only release. No Windows release exists yet. The Windows app's version is `1.1.0` in `windows/Cargo.toml`.

## Windows app

### Build

CI (`.github/workflows/windows.yml`) builds with the MSVC toolchain on `windows-latest`. It runs `cargo test --locked`, then `packaging/package.ps1`, and uploads the installer, portable zip, and checksums as a build artifact. On a `v*` tag it also attaches them to the GitHub release. It runs on pushes to `main`, `v*` tags, pull requests that touch `windows/`, and manual dispatch. Pushes to other branches don't run it.

The maintainer's PC has no Visual Studio Build Tools, so it builds with the GNU toolchain:

- Rust from rustup's `stable-x86_64-pc-windows-gnu`, in `~/.cargo/bin`. That folder isn't on PATH by default.
- MinGW from WinLibs (winget package `BrechtSanders.WinLibs.POSIX.MSVCRT`), for `dlltool`, `as`, and `windres`.
- Inno Setup 6, installed per-user at `%LOCALAPPDATA%\Programs\Inno Setup 6`.

In Git Bash, set PATH before running cargo:

```sh
export PATH="$HOME/.cargo/bin:$(ls -d ~/AppData/Local/Microsoft/WinGet/Packages/BrechtSanders.WinLibs.POSIX.MSVCRT*/mingw64/bin | head -1):$PATH"
```

Before committing, run these in `windows/`. All three must pass with no warnings:

```sh
cargo fmt --check            # rustfmt.toml sets max_width = 120
cargo clippy --all-targets
cargo test                   # 2026-10-19: 42 pass, 11 ignored
```

To package, run `cargo build --release --locked`, then `powershell -File packaging/package.ps1 -SkipBuild`. The files land in `windows/dist/`.

To install a local build, run the setup with `/SILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /FORCECLOSEAPPLICATIONS /RELAUNCH=1`. It installs per-user to `%LOCALAPPDATA%\Programs\GitDaddy`. Don't wait on it with PowerShell's `Start-Process -Wait`: with `/RELAUNCH=1` that blocks until the relaunched app exits.

### Test

UI tests use `egui_kittest`. It renders scenes offscreen with wgpu and saves PNGs, so no window opens.

- `cargo test readme_screenshots -- --ignored` and `cargo test dialog_screenshots -- --ignored` render every scene to `windows/target/screenshots/`, or to `GITDADDY_SCREENSHOT_DIR` if set.
- Scenes use fictional data from `demo.rs` and `Hub::demo_fill`. The README's `Assets/screenshot-windows-*.png` files are copies of these renders.
- When you add a UI state, add a scene for it at the bottom of `app.rs` (`render("name", theme, with_repos, |app| ...)`) and look at the result.

These ignored tests use the network or real data:

| Test | Needs |
| :--- | :--- |
| `live_queries` (hub.rs) | `gh` signed in. Runs every Explore ranking against GitHub. |
| `live_explore_cards`, `live_mentions_feed` (app.rs) | Network and `gh`. Renders real Explore cards and the real Live feed. |
| `live_mentions` (trending.rs) | Network. Calls Hacker News and Mastodon. |
| `live_gh_status` (github.rs) | `gh` signed in. |
| `live_github_check` (update.rs) | Network. Reads the GitHub releases API. |
| `real_installer_upgrade` (update.rs) | `GITDADDY_TEST_RELEASE_DIR` set to a folder with a newer setup and its SHA256SUMS. Installs it for real. |
| `real_repo_inspector` (app.rs), `real_repository_copy_matches_original` (github.rs) | `GITDADDY_DEBUG_REPO` set to a repository path. |

Two more overrides exist: `GITDADDY_GH` points at a specific `gh.exe`, and `GITDADDY_UPDATE_URL` points the updater at another releases endpoint.

Two guards keep tests away from the maintainer's real data. Keep both if you change that code.

- `trending::History` saves only when it came from `History::load()`. Tests and previews use `History::default()`, and `save()` does nothing for those.
- `Hub::tick` returns early under `cfg!(test)`, so tests never start the hourly star tracker.

Live render tests also spend GitHub's preview-image allowance (see the Explore section), so run them sparingly.

A test that commits must set `user.name` and `user.email` in the repository it commits in, because CI runners have no global git identity. Missing identity is what failed CI on `main` on 2026-10-18 (run 39157448316).

### Where the app keeps state

| What | Where |
| :--- | :--- |
| Settings (eframe persistence, key `gitdaddy.settings`) | `%APPDATA%\GitDaddy\data\app.ron` |
| Star history for Explore | `%LOCALAPPDATA%\GitDaddy\star-history.json` |
| Cached GitHub preview images, at most 400 | `%LOCALAPPDATA%\GitDaddy\previews\` |

### Code notes

The README's project tree describes each module. What it doesn't say:

- `hub.rs` runs GitHub queries on background threads. Results come back as `Msg` values, and `Hub::poll` applies them each frame. `hub_ui.rs` draws the GitHub views and returns `HubAction`s, which `app.rs` handles.
- `app.rs` ends with every screenshot scene and the UI tests.
- `trending.rs` has no UI. It holds the star history, the score and momentum math, and the Hacker News and Mastodon fetchers and parsers.
- `update.rs::http_agent` is the shared HTTP client. Its User-Agent is `GitDaddy/<version> (+https://github.com/Duyfrom/GitDaddy)`. GitHub's preview-image host rejects requests without one.
- `images.rs` loads README images and GitHub preview images. It fetches only from GitHub hosts and caches previews on disk.
- `widgets.rs::segmented` takes its ID from `ui.next_auto_id()`. Two segmented controls in one row clashed before that change.
- `theme.rs` adds Windows CJK fonts as fallbacks (Microsoft YaHei or Yu Gothic, plus Malgun Gothic), so Chinese, Japanese, and Korean text renders. They cost about 33 MB of memory.
- The status tabs in the header show only with the repository list. When a GitHub view is open (`view.section` is `Some`), the header shows the section's icon and name instead.

### Explore, and why it works this way

Explore copies [Trendshift](https://trendshift.io). Trendshift was studied on 2026-10-18 with REA (`npx rea-agents`, version 6.7.0) against a headless Chrome. REA's `analyze-web-bundle` ran out of memory on trendshift.io, so the findings came from the Next.js data embedded in its pages (`self.__next_f.push`).

What Trendshift does:

- Daily, Weekly, Monthly, and Yearly are UTC calendar periods. Weeks start on Monday.
- Its base score is 8 × stars gained + 2 × forks gained. Many entries match it exactly: harness2 scores 29,918, for example. Some entries score lower, so Trendshift discounts some stars. How it picks them is unknown.
- Its Live mentions come from X, Reddit, and Hacker News.
- Its API needs an account and billing.

What a free client can get (checked 2026-10-18):

- GitHub no longer exposes who starred a repository or when. REST returns 404 for stargazer lists, and GraphQL's `stargazers.totalCount` returns 0. No API answers "stars gained this week".
- OSS Insight's trending API, the one free star-trend source, has been unavailable since 2026-03-01.
- Reddit's unauthenticated RSS and JSON return 403 or 429. Bluesky's public search returns 403. X is paid.
- Hacker News through Algolia works: `https://hn.algolia.com/api/v1/search_by_date?query=github.com&restrictSearchableAttributes=url&tags=story&numericFilters=created_at_i>=<unix time>`.
- Mastodon works: `api/v1/timelines/tag/github?limit=40` on mastodon.social and fosstodon.org.
- GitHub's preview images (`opengraph.githubassets.com`) allow about 100 requests per 15 minutes per IP. After that they return 429 with `X-RateLimit-Remaining: 0` and `Retry-After: 900`, and `images.rs` pauses until then. Their URLs are content-hashed, so cached files never go stale.

So GitDaddy does what Trendshift does internally: it records star counts and subtracts them. A ranking is built like this (`fetch_explore` in hub.rs):

1. Search GitHub for repositories created in the period. At the same time, fetch up to 1,000 Hacker News stories and, for the current period only, the Mastodon timelines.
2. Add the 60 biggest risers from the star history and the 40 most-mentioned repositories. Read their counts with batched GraphQL aliases (`lookup_repos`).
3. Score each repository with `trending::momentum`. A repository created in the period counts all its stars. Any other repository needs recorded counts, and `History::gain` subtracts them. The gain is complete if a count exists from before the period began, and partial otherwise. The inspector's Momentum box says which.
4. Show the top 50 with a score above 0.

The star history (`trending::History`) is kept like this:

- While signed in to GitHub, `Hub::tick` runs `track` every hour. Once a day it first adds repositories from two searches of 100 results each: popular repositories pushed in the past week, and the most-starred repositories created in the past 30 days.
- `track` then records counts for up to 600 tracked repositories plus the user's clones and stars.
- `record` skips a repository whose last count is less than 50 minutes old.
- `compact` keeps every count for 3 days. Up to 62 days it keeps the first and last count of each UTC day, and after that the first count of each month. It drops repositories unseen for 45 days and keeps at most 800.

What this means for users:

- After a fresh install, Daily lists mostly repositories created that day. Older repositories join with partial gains once they have two hourly counts, and their gains are complete the next UTC day.
- Monthly and Yearly undercount older repositories until the history reaches back past the start of the period. Until then, their gains cover only the time since GitDaddy first recorded them.
- The rankings still line up with Trendshift's. On 2026-10-18, this month's top three (anatomy, openai/math, wordcraft) were on Trendshift's monthly list. This year's top three (morluto/rea, mattpocock/skills, affaan-m/ECC) were on its yearly list.

Live (`fetch_live` in hub.rs) merges Hacker News stories from the past 2 days with both Mastodon timelines. It removes duplicates, sorts newest first, and keeps 120. Posts about repositories GitHub doesn't return are dropped. The feed refreshes every 2 minutes while it's open.

Not copied from Trendshift: the star discount, X and Reddit mentions, and its likes and bookmarks.

## macOS app

- Nobody has compiled the Swift code on the maintainer's Windows PC. `7cf9f23` (sidebar folders and owners) was written there without a compiler. Whether `d4265c8` was compiled is unknown. Build on a Mac before any macOS release.
- macOS Explore still uses the old model: the most-starred repositories created today, this week, or this month (`TrendingSpan` in `GitHubService.swift`). It has no momentum score, no Yearly, and no Live. Porting `trending.rs` is the main gap between the two apps.
- `build_app.sh` builds and ad-hoc signs `GitDaddy.app`. It doesn't make the `GitDaddy-macOS.zip` the README links to.

## Releases

- The Windows updater (`update.rs`) reads the 30 newest releases. It picks the newest one that has both `GitDaddy-<version>-windows-x64-setup.exe` and `GitDaddy-<version>-windows-x64-SHA256SUMS.txt`. It skips releases without them, such as the macOS-only `v1.0.0`.
- To release Windows:
  1. Bump `version` in `windows/Cargo.toml`.
  2. Run `cargo build` so `Cargo.lock` picks up the new version.
  3. Merge to `main`.
  4. Push a `v<version>` tag. CI creates the release if it doesn't exist and uploads the files.

## Open items (2026-10-19)

1. Merge `windows-app` into `main`. CI on `main` stays red until the test fix lands there.
2. Publish the first Windows release, `v1.1.0`.
3. Port Explore's momentum rankings and Live to macOS.
4. Decide whether to hide the status tabs in folder and owner views too. They're hidden only in GitHub views now. The maintainer was asked and hasn't answered.
