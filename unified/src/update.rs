//! Finds newer Windows builds on GitHub Releases, downloads the installer, verifies it
//! against the release's SHA-256 list, and hands over to it.

use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const RELEASES_PAGE: &str = "https://github.com/Duyfrom/GitDaddy/releases";
const RELEASES_API: &str = "https://api.github.com/repos/Duyfrom/GitDaddy/releases?per_page=30";
#[cfg(test)]
const INSTALLER_SUFFIX: &str = "-windows-x64-setup.exe";
#[cfg(test)]
const CHECKSUMS_SUFFIX: &str = "-windows-x64-SHA256SUMS.txt";

#[cfg(target_os = "macos")]
const INSTALLER_SUFFIXES: &[&str] = &["-macOS-universal.dmg", "-macOS.dmg", "-windows-x64-setup.exe"];
#[cfg(not(target_os = "macos"))]
const INSTALLER_SUFFIXES: &[&str] = &["-windows-x64-setup.exe"];

#[cfg(target_os = "macos")]
const CHECKSUMS_SUFFIXES: &[&str] = &["-macOS-SHA256SUMS.txt", "-windows-x64-SHA256SUMS.txt"];
#[cfg(not(target_os = "macos"))]
const CHECKSUMS_SUFFIXES: &[&str] = &["-windows-x64-SHA256SUMS.txt"];

/// Points the updater at another releases endpoint (used by tests with a local server).
const API_OVERRIDE_ENV: &str = "GITDADDY_UPDATE_URL";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u64, u64, u64);

impl Version {
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("crate version is semver")
    }

    /// Parses `1.2.3`, `v1.2`, or `1.2.3-beta` (pre-release suffixes are ignored).
    pub fn parse(text: &str) -> Option<Self> {
        let core = text.trim().trim_start_matches(['v', 'V']);
        let core = core.split(['-', '+']).next()?;
        let mut parts = core.split('.').map(|p| p.parse::<u64>());
        let major = parts.next()?.ok()?;
        let minor = parts.next().unwrap_or(Ok(0)).ok()?;
        let patch = parts.next().unwrap_or(Ok(0)).ok()?;
        Some(Self(major, minor, patch))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Clone, Debug)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: Version,
    pub tag: String,
    pub notes: String,
    pub page_url: String,
    pub published: String,
    pub installer: Asset,
    pub checksums: Option<Asset>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

fn releases_url() -> String {
    std::env::var(API_OVERRIDE_ENV).unwrap_or_else(|_| RELEASES_API.to_string())
}

/// HTTP client using the OS's own TLS and certificate store, identified as GitDaddy.
/// `timeout` bounds a whole request; downloads pass `None`.
pub fn http_agent(timeout: Option<Duration>) -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
    let mut config = ureq::Agent::config_builder()
        .tls_config(tls)
        .user_agent(format!("GitDaddy/{} (+https://github.com/Duyfrom/GitDaddy)", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_global(timeout);
    if releases_url().starts_with("http://127.0.0.1") {
        // Local test server: never route through a system proxy.
        config = config.proxy(None);
    }
    config.build().new_agent()
}

fn agent(for_download: bool) -> ureq::Agent {
    http_agent((!for_download).then_some(Duration::from_secs(30)))
}

fn http_error(err: ureq::Error) -> String {
    match err {
        ureq::Error::StatusCode(403) | ureq::Error::StatusCode(429) => {
            "GitHub's rate limit was reached. Try again in an hour.".into()
        }
        ureq::Error::StatusCode(code) => format!("GitHub returned HTTP {code}."),
        other => format!("Could not reach GitHub: {other}"),
    }
}

/// Returns the newest published release that ships an installer for this OS, if any.
pub fn fetch_latest_release() -> Result<Option<Release>, String> {
    let mut response =
        agent(false).get(&releases_url()).header("Accept", "application/vnd.github+json").call().map_err(http_error)?;
    let body = response.body_mut().read_to_string().map_err(|e| format!("Could not read GitHub's reply: {e}"))?;
    let releases: Vec<ApiRelease> =
        serde_json::from_str(&body).map_err(|e| format!("Unexpected reply from GitHub: {e}"))?;
    Ok(newest_platform_release(releases))
}

fn newest_platform_release(releases: Vec<ApiRelease>) -> Option<Release> {
    releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let version = Version::parse(&r.tag_name)?;
            let find = |suffixes: &[&str]| {
                suffixes.iter().find_map(|suffix| {
                    r.assets.iter().find(|a| a.name.ends_with(suffix)).map(|a| Asset {
                        name: a.name.clone(),
                        url: a.browser_download_url.clone(),
                        size: a.size,
                    })
                })
            };
            let installer = find(INSTALLER_SUFFIXES)?;
            Some(Release {
                version,
                tag: r.tag_name.clone(),
                notes: r.body.clone().unwrap_or_default(),
                page_url: r.html_url.clone(),
                published: r.published_at.as_deref().and_then(|d| d.get(..10)).unwrap_or("").to_string(),
                installer,
                checksums: find(CHECKSUMS_SUFFIXES),
            })
        })
        .max_by_key(|r| r.version)
}

/// Finds `name`'s hash in a `sha256sum`-style list.
fn checksum_for<'a>(list: &'a str, name: &str) -> Option<&'a str> {
    list.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        (file == name && hash.len() == 64).then_some(hash)
    })
}

fn sha256_hex(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Removes installers left over from an earlier update (the one still running is locked and stays).
pub fn remove_old_downloads() {
    let _ = fs::remove_dir_all(download_dir());
}

pub fn download_dir() -> PathBuf {
    std::env::temp_dir().join("GitDaddy-update")
}

/// Downloads and verifies the installer. `progress` receives the bytes written so far.
pub fn download_installer(release: &Release, progress: &AtomicU64, cancel: &AtomicBool) -> Result<PathBuf, String> {
    let checksums = release
        .checksums
        .as_ref()
        .ok_or("This release has no checksum file, so GitDaddy will not install it automatically.")?;
    let agent = agent(true);
    let list = agent
        .get(&checksums.url)
        .call()
        .map_err(http_error)?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Could not read the checksum file: {e}"))?;
    let expected = checksum_for(&list, &release.installer.name)
        .ok_or("The checksum file does not list this installer.")?
        .to_lowercase();

    let dir = download_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let target = dir.join(&release.installer.name);
    let partial = dir.join(format!("{}.part", release.installer.name));

    let result = (|| {
        let response = agent.get(&release.installer.url).call().map_err(http_error)?;
        let mut reader = response.into_body().into_reader();
        let mut file = File::create(&partial).map_err(|e| format!("Could not write the download: {e}"))?;
        let mut buf = vec![0u8; 64 * 1024];
        let mut written = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Download cancelled.".to_string());
            }
            let n = reader.read(&mut buf).map_err(|e| format!("Download interrupted: {e}"))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| format!("Could not write the download: {e}"))?;
            written += n as u64;
            progress.store(written, Ordering::Relaxed);
        }
        file.flush().map_err(|e| e.to_string())?;
        drop(file);
        if release.installer.size > 0 && written != release.installer.size {
            return Err(format!("Download incomplete ({written} of {} bytes).", release.installer.size));
        }
        let actual = sha256_hex(&partial)?;
        if actual != expected {
            return Err("The downloaded installer does not match its published checksum.".to_string());
        }
        let _ = fs::remove_file(&target);
        fs::rename(&partial, &target).map_err(|e| e.to_string())?;
        Ok(target.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

/// Starts the installer.
/// - On Windows: launches the Inno Setup installer in silent mode with `/RELAUNCH=1`.
/// - On macOS: mounts the verified `.dmg`, replaces `/Applications/GitDaddy.app` if writable
///   (or opens the `.dmg` in Finder), and relaunches `GitDaddy.app`.
pub fn launch_installer(installer: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let dmg = installer.display().to_string().replace('\'', "'\\''");
        let script = format!(
            "sleep 1; \
             MOUNT_DIR=$(hdiutil attach -nobrowse -readonly '{dmg}' | awk -F'\\t' '/\\/Volumes\\// {{print $NF; exit}}'); \
             if [ -n \"$MOUNT_DIR\" ] && [ -d \"$MOUNT_DIR/GitDaddy.app\" ] && [ -w \"/Applications\" ]; then \
               rm -rf \"/Applications/GitDaddy.app\" && \
               cp -R \"$MOUNT_DIR/GitDaddy.app\" \"/Applications/GitDaddy.app\" && \
               xattr -cr \"/Applications/GitDaddy.app\" 2>/dev/null || true; \
               hdiutil detach \"$MOUNT_DIR\" -quiet 2>/dev/null || true; \
               open \"/Applications/GitDaddy.app\"; \
             else \
               open '{dmg}'; \
             fi"
        );
        Command::new("sh")
            .args(["-c", &script])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not start the macOS updater: {e}"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Command::new(installer)
            .args([
                "/SILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/CLOSEAPPLICATIONS",
                "/FORCECLOSEAPPLICATIONS",
                "/RELAUNCH=1",
            ])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not start the installer: {e}"))
    }
}

/// True when running from an installed copy:
/// - On macOS: inside a `.app` bundle (`.app/Contents/MacOS/`).
/// - On Windows: next to an Inno Setup uninstaller (`unins*.exe`).
pub fn is_installed_copy() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::env::current_exe().ok().is_some_and(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS/"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .and_then(|dir| fs::read_dir(dir).ok())
            .is_some_and(|entries| {
                entries.flatten().any(|e| {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    name.starts_with("unins") && name.ends_with(".exe")
                })
            })
    }
}

// MARK: - State machine driven by the UI

pub enum Phase {
    Idle,
    Checking,
    UpToDate(Option<Release>),
    NoRelease,
    Available(Release),
    Downloading { release: Release, progress: Arc<AtomicU64>, cancel: Arc<AtomicBool> },
    Installing(Release),
    Failed { message: String, release: Option<Release> },
}

enum Msg {
    Checked { result: Result<Option<Release>, String>, manual: bool },
    Downloaded { release: Release, result: Result<PathBuf, String> },
}

pub struct Updater {
    pub phase: Phase,
    pub show_dialog: bool,
    /// Set once an installer has been launched; the app should close.
    pub quit_requested: bool,
    /// Installed with the setup (can self-update) rather than run from the portable zip.
    pub installed: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    ctx: egui::Context,
}

impl Updater {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            phase: Phase::Idle,
            show_dialog: false,
            quit_requested: false,
            installed: is_installed_copy(),
            tx,
            rx,
            ctx,
        }
    }

    pub fn available_release(&self) -> Option<&Release> {
        match &self.phase {
            Phase::Available(r) => Some(r),
            _ => None,
        }
    }

    pub fn is_working(&self) -> bool {
        matches!(self.phase, Phase::Checking | Phase::Downloading { .. } | Phase::Installing(_))
    }

    /// `manual` checks open the dialog and report every outcome; background checks only
    /// surface an available update.
    pub fn check(&mut self, manual: bool) {
        if manual {
            self.show_dialog = true;
        }
        if self.is_working() {
            return;
        }
        if !manual && matches!(self.phase, Phase::Available(_)) {
            return;
        }
        self.phase = Phase::Checking;
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let result = fetch_latest_release();
            let _ = tx.send(Msg::Checked { result, manual });
            ctx.request_repaint();
        });
    }

    pub fn start_download(&mut self) {
        let Phase::Available(release) = &self.phase else { return };
        let release = release.clone();
        let progress = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        self.phase = Phase::Downloading {
            release: release.clone(),
            progress: Arc::clone(&progress),
            cancel: Arc::clone(&cancel),
        };
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            // Repaint while bytes arrive so the progress bar moves.
            let ticker = {
                let ctx = ctx.clone();
                let cancel = Arc::clone(&cancel);
                let done = Arc::new(AtomicBool::new(false));
                let done_flag = Arc::clone(&done);
                thread::spawn(move || {
                    while !done_flag.load(Ordering::Relaxed) && !cancel.load(Ordering::Relaxed) {
                        ctx.request_repaint();
                        thread::sleep(Duration::from_millis(150));
                    }
                });
                done
            };
            let result = download_installer(&release, &progress, &cancel);
            ticker.store(true, Ordering::Relaxed);
            let _ = tx.send(Msg::Downloaded { release, result });
            ctx.request_repaint();
        });
    }

    /// Retries a failed download, or the check that failed.
    pub fn retry(&mut self) {
        if let Phase::Failed { release: Some(release), .. } = &self.phase {
            self.phase = Phase::Available(release.clone());
            self.start_download();
        } else {
            self.phase = Phase::Idle;
            self.check(true);
        }
    }

    pub fn cancel_download(&mut self) {
        if let Phase::Downloading { cancel, .. } = &self.phase {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn dismiss(&mut self) {
        self.show_dialog = false;
        if matches!(self.phase, Phase::UpToDate(_) | Phase::NoRelease | Phase::Failed { .. }) {
            self.phase = Phase::Idle;
        }
    }

    /// Applies background results. Returns true when a check finished successfully.
    pub fn poll(&mut self) -> bool {
        let mut checked = false;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Checked { result, manual } => {
                    checked |= result.is_ok();
                    self.phase = match result {
                        Ok(Some(release)) if release.version > Version::current() => Phase::Available(release),
                        Ok(Some(release)) if manual => Phase::UpToDate(Some(release)),
                        Ok(None) if manual => Phase::NoRelease,
                        Err(message) if manual => Phase::Failed { message, release: None },
                        _ => Phase::Idle,
                    };
                }
                Msg::Downloaded { release, result } => {
                    self.phase = match result.and_then(|path| launch_installer(&path).map(|()| path)) {
                        Ok(_) => {
                            self.quit_requested = true;
                            Phase::Installing(release)
                        }
                        Err(message) if message == "Download cancelled." => Phase::Available(release),
                        Err(message) => Phase::Failed { message, release: Some(release) },
                    };
                }
            }
        }
        checked
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn version_parsing_and_order() {
        assert_eq!(Version::parse("v1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(Version::parse("1.10"), Some(Version(1, 10, 0)));
        assert_eq!(Version::parse("2.0.0-beta.1"), Some(Version(2, 0, 0)));
        assert_eq!(Version::parse("macos-1"), None);
        assert!(Version::parse("1.10.0") > Version::parse("1.9.9"));
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn checksum_lookup() {
        let list = format!("{}  GitDaddy-1.2.0-windows-x64-setup.exe\n{} *other.zip\n", "a".repeat(64), "b".repeat(64));
        assert_eq!(checksum_for(&list, "GitDaddy-1.2.0-windows-x64-setup.exe"), Some("a".repeat(64).as_str()));
        assert_eq!(checksum_for(&list, "other.zip"), Some("b".repeat(64).as_str()));
        assert_eq!(checksum_for(&list, "missing.exe"), None);
    }

    fn api_json(base: &str) -> String {
        format!(
            r#"[
              {{"tag_name":"v9.0.0","html_url":"x","draft":false,"prerelease":true,"assets":[
                {{"name":"GitDaddy-9.0.0-windows-x64-setup.exe","browser_download_url":"{base}/nope","size":1}}]}},
              {{"tag_name":"v3.0.0","html_url":"x","draft":false,"prerelease":false,"assets":[
                {{"name":"GitDaddy-macOS.zip","browser_download_url":"{base}/mac","size":1}}]}},
              {{"tag_name":"v2.1.0","html_url":"{base}/page","body":"Notes","published_at":"2026-11-02T10:00:00Z","draft":false,"prerelease":false,"assets":[
                {{"name":"GitDaddy-2.1.0-windows-x64-setup.exe","browser_download_url":"{base}/setup","size":{size}}},
                {{"name":"GitDaddy-2.1.0-windows-x64-SHA256SUMS.txt","browser_download_url":"{base}/sums","size":100}}]}},
              {{"tag_name":"v2.0.0","html_url":"x","draft":false,"prerelease":false,"assets":[
                {{"name":"GitDaddy-2.0.0-windows-x64-setup.exe","browser_download_url":"{base}/old","size":1}}]}}
            ]"#,
            size = PAYLOAD.len()
        )
    }

    const PAYLOAD: &[u8] = b"pretend this is an installer";

    type Routes = Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>;

    /// Minimal HTTP/1.1 server on 127.0.0.1; routes can be added after it starts.
    struct TestServer {
        base: String,
        routes: Routes,
    }

    impl TestServer {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let routes: Routes = Arc::default();
            let shared = Arc::clone(&routes);
            thread::spawn(move || {
                for mut stream in listener.incoming().flatten() {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request_line = String::new();
                    let _ = reader.read_line(&mut request_line);
                    let mut header = String::new();
                    while reader.read_line(&mut header).unwrap_or(0) > 2 {
                        header.clear();
                    }
                    let path = request_line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let body = shared.lock().unwrap().iter().find(|(p, _)| *p == path).map(|(_, b)| b.clone());
                    let (status, body) = match body {
                        Some(body) => ("200 OK", body),
                        None => ("404 Not Found", Vec::new()),
                    };
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(&body);
                }
            });
            Self { base, routes }
        }

        fn route(&self, path: &str, body: impl Into<Vec<u8>>) {
            self.routes.lock().unwrap().push((path.to_string(), body.into()));
        }
    }

    fn sums_line(hash: &str) -> String {
        format!("{hash}  GitDaddy-2.1.0-windows-x64-setup.exe\n")
    }

    #[test]
    fn finds_release_downloads_and_verifies() {
        let server = TestServer::start();
        let real_hash: String = Sha256::digest(PAYLOAD).iter().map(|b| format!("{b:02x}")).collect();
        server.route("/releases", api_json(&server.base));
        server.route("/setup", PAYLOAD);
        server.route("/sums", sums_line(&real_hash));
        server.route("/bad-sums", sums_line(&"0".repeat(64)));
        // Only this test touches the override, so parallel tests cannot race on it.
        unsafe { std::env::set_var(API_OVERRIDE_ENV, format!("{}/releases", server.base)) };

        // Skips the prerelease, the macOS-only release, and older Windows releases.
        let release = fetch_latest_release().unwrap().expect("a windows release");
        assert_eq!(release.version, Version(2, 1, 0));
        assert_eq!(release.notes, "Notes");
        assert_eq!(release.published, "2026-11-02");

        let progress = AtomicU64::new(0);
        let cancel = AtomicBool::new(false);
        let path = download_installer(&release, &progress, &cancel).unwrap();
        assert_eq!(fs::read(&path).unwrap(), PAYLOAD);
        assert_eq!(progress.load(Ordering::Relaxed), PAYLOAD.len() as u64);
        let _ = fs::remove_file(&path);

        // A checksum mismatch is rejected and leaves no installer behind.
        let mut tampered = release.clone();
        tampered.checksums.as_mut().unwrap().url = format!("{}/bad-sums", server.base);
        let err = download_installer(&tampered, &progress, &cancel).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert!(!download_dir().join(&tampered.installer.name).exists());

        // Without a checksum file the updater refuses to install.
        let mut unverifiable = release.clone();
        unverifiable.checksums = None;
        assert!(download_installer(&unverifiable, &progress, &cancel).is_err());

        // Cancelling stops the download.
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(download_installer(&release, &progress, &cancel).unwrap_err(), "Download cancelled.");

        unsafe { std::env::remove_var(API_OVERRIDE_ENV) };
    }

    #[test]
    #[ignore = "talks to api.github.com; run with `cargo test live_github -- --ignored`"]
    fn live_github_check() {
        let latest = fetch_latest_release().expect("GitHub releases API reachable over native TLS");
        eprintln!("newest Windows release on GitHub: {:?}", latest.map(|r| r.tag));
    }

    fn wait_until(what: &str, timeout_secs: u64, mut done: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(500));
        }
    }

    fn command_output(program: &str, args: &[&str]) -> String {
        Command::new(program)
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    }

    /// Full update path against a real installer: serve a newer build locally, then let the
    /// updater download, verify, and launch it. The installed copy is upgraded and reopened.
    #[test]
    #[ignore = "installs a real build; set GITDADDY_TEST_RELEASE_DIR to a folder with a newer setup + SHA256SUMS"]
    fn real_installer_upgrade() {
        let dir = PathBuf::from(std::env::var("GITDADDY_TEST_RELEASE_DIR").expect("GITDADDY_TEST_RELEASE_DIR"));
        let find = |suffix: &str| {
            fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .find(|p| p.to_string_lossy().ends_with(suffix))
                .unwrap()
        };
        let (setup, sums) = (find(INSTALLER_SUFFIX), find(CHECKSUMS_SUFFIX));
        let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
        let version = name(&setup).trim_start_matches("GitDaddy-").trim_end_matches(INSTALLER_SUFFIX).to_string();

        let server = TestServer::start();
        let base = server.base.clone();
        server.route("/setup", fs::read(&setup).unwrap());
        server.route("/sums", fs::read(&sums).unwrap());
        server.route(
            "/releases",
            format!(
                r#"[{{"tag_name":"v{version}","html_url":"{base}","draft":false,"prerelease":false,"assets":[
                    {{"name":"{}","browser_download_url":"{base}/setup","size":{}}},
                    {{"name":"{}","browser_download_url":"{base}/sums","size":{}}}]}}]"#,
                name(&setup),
                fs::metadata(&setup).unwrap().len(),
                name(&sums),
                fs::metadata(&sums).unwrap().len()
            ),
        );
        unsafe { std::env::set_var(API_OVERRIDE_ENV, format!("{base}/releases")) };

        let release = fetch_latest_release().unwrap().expect("local release");
        assert!(release.version > Version::current(), "{} should be newer than the test binary", release.version);
        let path = download_installer(&release, &AtomicU64::new(0), &AtomicBool::new(false)).unwrap();
        let started = std::time::Instant::now();
        launch_installer(&path).unwrap();
        // In the app, the updater closes the window right after starting Setup; do the same here.
        command_output(
            "powershell",
            &["-NoProfile", "-Command", "Get-Process GitDaddy -EA 0 | % { $_.CloseMainWindow() }"],
        );

        let key =
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{9CB12CE5-520C-45E5-ACBA-63AE0523926B}_is1";
        wait_until("the installed version to change", 180, || {
            command_output("reg", &["query", key, "/v", "DisplayVersion"]).contains(&version)
        });
        wait_until("GitDaddy to reopen", 60, || {
            command_output("tasklist", &["/FI", "IMAGENAME eq GitDaddy.exe", "/NH"]).contains("GitDaddy.exe")
        });
        eprintln!("updated to {version} and reopened in {:.1}s", started.elapsed().as_secs_f32());
        unsafe { std::env::remove_var(API_OVERRIDE_ENV) };
    }
}
