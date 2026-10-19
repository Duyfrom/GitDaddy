# GitDaddy — Unified Cross-Platform Codebase (`unified/`)

This folder contains the **single codebase** that builds both the **macOS** (`GitDaddy.dmg` / `GitDaddy.app`) and **Windows** (`GitDaddy-setup.exe`) versions of GitDaddy.

Your two original versions (`Sources/GitDaddy/` for macOS SwiftUI and `windows/` for Windows) remain untouched in the repository root as backups.

---

## 1. Run on Your Mac (One Command)

From the root of the repository, run:

```bash
./unified/run_mac.sh
```

- To also install the app into `/Applications/GitDaddy.app` and launch it:
  ```bash
  ./unified/run_mac.sh --install
  ```
- To build `unified/dist/GitDaddy.app` without opening the window:
  ```bash
  ./unified/run_mac.sh --no-open
  ```

---

## 2. How to Publish a New Release (Windows `.exe` + Universal macOS `.dmg`)

1. Open [`unified/Cargo.toml`](Cargo.toml) and change the `version` line at the top (for example, from `"1.1.0"` to `"1.2.0"`).
2. Run a quick check inside `unified/` so `Cargo.lock` updates to the new version:
   ```bash
   cd unified && cargo check && cd ..
   ```
3. Commit and push the version bump, then push a matching `v*` tag:
   ```bash
   git add unified/Cargo.toml unified/Cargo.lock
   git commit -m "Bump version to 1.2.0"
   git push origin main
   git tag v1.2.0
   git push origin v1.2.0
   ```

When you push the `v1.2.0` tag, GitHub Actions automatically runs two jobs in parallel:
- **[`.github/workflows/macos.yml`](../.github/workflows/macos.yml)** builds a **Universal macOS app** (works on both Apple Silicon M1/M2/M3/M4 and Intel Macs), packages `GitDaddy-1.2.0-macOS-universal.dmg`, `GitDaddy-macOS.zip`, and `GitDaddy-1.2.0-macOS-SHA256SUMS.txt`, and attaches them to the GitHub Release.
- **[`.github/workflows/windows.yml`](../.github/workflows/windows.yml)** builds the **Windows x64 app**, packages `GitDaddy-1.2.0-windows-x64-setup.exe`, `GitDaddy-1.2.0-windows-x64-portable.zip`, and `GitDaddy-1.2.0-windows-x64-SHA256SUMS.txt`, and attaches them to the same GitHub Release.

---

## 3. Where the macOS-Only and Windows-Only Parts Live

Almost 99% of [`unified/src/`](src/) is identical across macOS and Windows. The small OS-specific parts are isolated in four clearly labeled places:

| File | What OS-Specific Work It Does |
| :--- | :--- |
| **[`src/platform.rs`](src/platform.rs)** | Opens **Finder** vs. **File Explorer**; opens **Terminal.app / iTerm** vs. **Windows Terminal / PowerShell**; sets OS labels (`⌘` vs. `Ctrl+`, `Move to Trash` vs. `Move to Recycle Bin`, `On this Mac` vs. `On this PC`). |
| **[`src/update.rs`](src/update.rs)** | Downloads and verifies `GitDaddy-<version>-macOS-universal.dmg` on Mac vs. `GitDaddy-<version>-windows-x64-setup.exe` on Windows, and runs the platform installer. |
| **[`src/git.rs`](src/git.rs) & [`src/github.rs`](src/github.rs)** | Searches `/opt/homebrew/bin` and `/usr/local/bin` for `git` and `gh` on Mac, and `C:\Program Files\Git` / `GitHub CLI` on Windows. |
| **[`src/theme.rs`](src/theme.rs)** | Loads macOS system fallback fonts (`Apple Symbols.ttf`, `Arial Unicode.ttf`) on Mac, and Windows fonts (`Segoe UI`, `Microsoft YaHei`, `Malgun Gothic`) on Windows. |

---

## 4. Where User Data & Settings Are Stored

Nothing is hardcoded; the app uses each operating system's standard folders:

- **macOS:**
  - Settings & Star History: `~/Library/Application Support/GitDaddy/`
  - Preview Image Cache: `~/Library/Caches/GitDaddy/previews/`
- **Windows:**
  - Settings: `%APPDATA%\GitDaddy\data\`
  - Star History & Preview Image Cache: `%LOCALAPPDATA%\GitDaddy\`

---

## 5. Code Signing in Plain Language (Free vs. Paid)

Right now, both builds use **free (unsigned / ad-hoc signed)** packaging so you pay **$0**:

### What happens when you skip paid code signing (current setup):
- **On macOS (Gatekeeper):** When someone downloads `GitDaddy.dmg` from a browser on a different Mac, macOS shows *"GitDaddy Not Opened — Apple could not verify GitDaddy is free of malware."*
  - **Free workaround for users:** Right-click `GitDaddy.app` → **Open**, or run `xattr -cr /Applications/GitDaddy.app` once in Terminal. (Note: when *you* build locally with `./unified/run_mac.sh` or upgrade via the in-app updater, there is **no warning** on your Mac).
- **On Windows (SmartScreen):** When someone downloads `GitDaddy-setup.exe`, Windows shows *"Windows protected your PC"*.
  - **Free workaround for users:** Click **More info** → **Run anyway**.

### What paid code signing costs & how to add it later:
- **macOS (Apple Developer Program — $99/year):**
  1. Enroll at [developer.apple.com](https://developer.apple.com/programs/).
  2. Create a **Developer ID Application** certificate and export it as a `.p12` file.
  3. Add your certificate and Apple ID password to your GitHub repository's **Settings → Secrets → Actions**, and replace `codesign --sign -` in `unified/packaging/package_mac_dmg.sh` with your Developer ID identity and `xcrun notarytool submit`.
- **Windows (Azure Trusted Signing — ~$9.99/month, or SignPath.io — free for open-source projects):**
  1. Apply for [SignPath.io Free Open Source tier](https://about.signpath.io/product/open-source) ($0) or Microsoft Azure Trusted Signing ($9.99/mo).
  2. Add a signing step to `.github/workflows/windows.yml` after `package.ps1` produces `GitDaddy-setup.exe`.
