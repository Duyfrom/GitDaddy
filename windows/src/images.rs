//! Image loading off the UI thread, shrunk to a sensible size: README images from disk, and
//! repository card previews from GitHub.
//!
//! README screenshots are often several thousand pixels wide. Uploading them at full size
//! wastes GPU memory and can exceed the GPU's texture limit, so they are downscaled first.

use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use egui::load::{ImageLoadResult, ImageLoader, ImagePoll, LoadError, SizeHint};
use egui::{ColorImage, Context};
use sha2::{Digest, Sha256};

/// Longest side kept after decoding. The inspector is at most 640pt wide, so this stays
/// sharp at 2x display scaling.
const MAX_SIDE: u32 = 1600;

enum Entry {
    Pending,
    Ready(Arc<ColorImage>),
    Failed(String),
}

#[derive(Default)]
pub struct LocalImageLoader {
    cache: Arc<Mutex<HashMap<String, Entry>>>,
}

pub fn install(ctx: &Context) {
    ctx.add_image_loader(Arc::new(LocalImageLoader::default()));
    ctx.add_image_loader(Arc::new(RemoteImageLoader::new(ctx)));
}

/// `file:///D:/repo/docs/shot%20one.png` → `D:/repo/docs/shot one.png`.
fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    Some(PathBuf::from(rest.replace("%20", " ")))
}

fn decode(path: &PathBuf) -> Result<ColorImage, String> {
    let image = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(to_color_image(image, MAX_SIDE))
}

fn to_color_image(image: image::DynamicImage, max_side: u32) -> ColorImage {
    let image = if image.width() > max_side || image.height() > max_side {
        image.resize(max_side, max_side, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let rgba = image.to_rgba8();
    ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())
}

impl ImageLoader for LocalImageLoader {
    fn id(&self) -> &str {
        "gitdaddy::LocalImageLoader"
    }

    fn load(&self, ctx: &Context, uri: &str, _size_hint: SizeHint) -> ImageLoadResult {
        let Some(path) = file_uri_to_path(uri) else {
            return Err(LoadError::NotSupported);
        };
        let mut cache = self.cache.lock().map_err(|_| LoadError::Loading("image cache poisoned".into()))?;
        match cache.get(uri) {
            Some(Entry::Ready(image)) => return Ok(ImagePoll::Ready { image: Arc::clone(image) }),
            Some(Entry::Pending) => return Ok(ImagePoll::Pending { size: None }),
            Some(Entry::Failed(err)) => return Err(LoadError::Loading(err.clone())),
            None => {}
        }
        cache.insert(uri.to_owned(), Entry::Pending);
        drop(cache);

        let cache = Arc::clone(&self.cache);
        let ctx = ctx.clone();
        let uri = uri.to_owned();
        thread::spawn(move || {
            let entry = match decode(&path) {
                Ok(image) => Entry::Ready(Arc::new(image)),
                Err(err) => Entry::Failed(err),
            };
            if let Ok(mut cache) = cache.lock() {
                cache.insert(uri, entry);
            }
            ctx.request_repaint();
        });
        Ok(ImagePoll::Pending { size: None })
    }

    fn forget(&self, uri: &str) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(uri);
        }
    }

    fn forget_all(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    fn byte_size(&self) -> usize {
        self.cache.lock().map_or(0, |cache| {
            cache
                .values()
                .map(|entry| match entry {
                    Entry::Ready(image) => image.pixels.len() * 4,
                    _ => 0,
                })
                .sum()
        })
    }

    fn has_pending(&self) -> bool {
        self.cache.lock().is_ok_and(|cache| cache.values().any(|e| matches!(e, Entry::Pending)))
    }
}

// MARK: - Remote images (repository cards)

/// Card previews only ever come from GitHub's image hosts.
const REMOTE_HOSTS: [&str; 4] = [
    "opengraph.githubassets.com",
    "repository-images.githubusercontent.com",
    "avatars.githubusercontent.com",
    "user-images.githubusercontent.com",
];
const REMOTE_MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Cards are at most ~370pt wide, so 640px stays sharp at 1.75x display scaling.
const REMOTE_MAX_SIDE: u32 = 640;
/// Decoded images kept in memory; evicted ones come back from the disk cache.
const REMOTE_CACHE_LIMIT: usize = 60;
const REMOTE_WORKERS: usize = 2;
/// Downloaded previews kept on disk. Preview URLs contain a content hash, so a cached file
/// never goes stale; the oldest files are deleted past this count.
const DISK_CACHE_LIMIT: usize = 400;
/// Wait before asking again for one image GitHub answered 429 for while budget remained.
const BUSY_RETRY: Duration = Duration::from_secs(20);
/// GitHub allows 100 preview images per 15 minutes per IP; never pause longer than that.
const MAX_PAUSE: Duration = Duration::from_secs(15 * 60);

enum RemoteEntry {
    Queued,
    Ready(Arc<ColorImage>),
    /// Rate limited: ask again after this instant.
    Waiting(Instant),
    Failed(String),
}

#[derive(Default)]
struct RemoteState {
    entries: HashMap<String, RemoteEntry>,
    /// Oldest first, for evicting.
    order: VecDeque<String>,
    /// GitHub's image budget is spent until this instant; nothing is requested before then.
    paused_until: Option<Instant>,
}

impl RemoteState {
    fn insert(&mut self, uri: String, entry: RemoteEntry) {
        if !self.entries.contains_key(&uri) {
            self.order.push_back(uri.clone());
        }
        self.entries.insert(uri, entry);
        while self.order.len() > REMOTE_CACHE_LIMIT {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}

/// Downloads `https://` images from GitHub's hosts on background workers, through a disk cache.
pub struct RemoteImageLoader {
    state: Arc<Mutex<RemoteState>>,
    jobs: Mutex<Sender<String>>,
}

impl RemoteImageLoader {
    fn new(ctx: &Context) -> Self {
        let (tx, rx) = mpsc::channel::<String>();
        let rx = Arc::new(Mutex::new(rx));
        let state: Arc<Mutex<RemoteState>> = Arc::default();
        for _ in 0..REMOTE_WORKERS {
            let (rx, state, ctx) = (Arc::clone(&rx), Arc::clone(&state), ctx.clone());
            thread::spawn(move || {
                loop {
                    // The lock is only held while waiting for a job, not while downloading.
                    let next = rx.lock().ok().and_then(|rx| rx.recv().ok());
                    let Some(uri) = next else { return };
                    let entry = load_remote(&uri, &state);
                    if let RemoteEntry::Waiting(until) = &entry {
                        // Repaint when the wait is over so visible cards ask again.
                        ctx.request_repaint_after(until.saturating_duration_since(Instant::now()));
                    }
                    if let Ok(mut state) = state.lock() {
                        state.insert(uri, entry);
                    }
                    ctx.request_repaint();
                }
            });
        }
        thread::spawn(prune_disk_cache);
        Self { state, jobs: Mutex::new(tx) }
    }
}

fn is_allowed_remote(uri: &str) -> bool {
    let Some(rest) = uri.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    REMOTE_HOSTS.contains(&host)
}

fn disk_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("GitDaddy").join("previews"))
}

fn disk_cache_path(uri: &str) -> Option<PathBuf> {
    let digest: String = Sha256::digest(uri.as_bytes()).iter().take(16).map(|b| format!("{b:02x}")).collect();
    disk_cache_dir().map(|dir| dir.join(format!("{digest}.img")))
}

fn prune_disk_cache() {
    let Some(entries) = disk_cache_dir().and_then(|dir| std::fs::read_dir(dir).ok()) else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> =
        entries.flatten().filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path()))).collect();
    if files.len() <= DISK_CACHE_LIMIT {
        return;
    }
    files.sort();
    for (_, path) in &files[..files.len() - DISK_CACHE_LIMIT] {
        let _ = std::fs::remove_file(path);
    }
}

#[derive(Debug, PartialEq)]
enum Limit {
    /// GitHub refused this one image (often while it is still rendering it); budget remains.
    Busy,
    /// The budget is spent; nothing more is allowed for this long.
    Spent(Duration),
}

/// Reads GitHub's rate-limit answer from a response's status and headers.
fn rate_limit(
    status: u16,
    remaining: Option<u64>,
    retry_after: Option<u64>,
    reset_at: Option<u64>,
    now_unix: u64,
) -> Option<Limit> {
    let spent = remaining == Some(0);
    if status != 429 && !(status == 403 && spent) {
        return None;
    }
    if !spent {
        return Some(Limit::Busy);
    }
    let seconds = retry_after.or_else(|| reset_at.map(|reset| reset.saturating_sub(now_unix))).unwrap_or(60);
    Some(Limit::Spent(Duration::from_secs(seconds.max(1)).min(MAX_PAUSE)))
}

enum Download {
    Limited(Limit),
    Failed(String),
}

fn download(uri: &str) -> Result<Vec<u8>, Download> {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    let agent = AGENT.get_or_init(|| crate::update::http_agent(Some(Duration::from_secs(30))));
    let response = agent
        .get(uri)
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|e| Download::Failed(e.to_string()))?;
    let status = response.status().as_u16();
    let header = |name: &str| {
        response.headers().get(name).and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok())
    };
    let now_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    if let Some(limit) = rate_limit(
        status,
        header("x-ratelimit-remaining"),
        header("retry-after"),
        header("x-ratelimit-reset"),
        now_unix,
    ) {
        return Err(Download::Limited(limit));
    }
    if status != 200 {
        return Err(Download::Failed(format!("HTTP {status}")));
    }
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(REMOTE_MAX_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| Download::Failed(e.to_string()))?;
    Ok(bytes)
}

fn decode_bytes(bytes: &[u8]) -> Result<ColorImage, String> {
    let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    Ok(to_color_image(image, REMOTE_MAX_SIDE))
}

fn load_remote(uri: &str, state: &Mutex<RemoteState>) -> RemoteEntry {
    let cached = disk_cache_path(uri);
    if let Some(image) = cached.as_ref().and_then(|path| decode_bytes(&std::fs::read(path).ok()?).ok()) {
        return RemoteEntry::Ready(Arc::new(image));
    }
    let paused = state.lock().ok().and_then(|s| s.paused_until).filter(|until| *until > Instant::now());
    if let Some(until) = paused {
        return RemoteEntry::Waiting(until);
    }
    match download(uri) {
        Ok(bytes) => match decode_bytes(&bytes) {
            Ok(image) => {
                if let Some(path) = cached {
                    let _ = path.parent().map(std::fs::create_dir_all);
                    let _ = std::fs::write(path, &bytes);
                }
                RemoteEntry::Ready(Arc::new(image))
            }
            Err(err) => RemoteEntry::Failed(err),
        },
        Err(Download::Limited(Limit::Busy)) => RemoteEntry::Waiting(Instant::now() + BUSY_RETRY),
        Err(Download::Limited(Limit::Spent(pause))) => {
            let until = Instant::now() + pause;
            if let Ok(mut state) = state.lock() {
                state.paused_until = Some(until);
            }
            RemoteEntry::Waiting(until)
        }
        Err(Download::Failed(err)) => RemoteEntry::Failed(err),
    }
}

impl ImageLoader for RemoteImageLoader {
    fn id(&self) -> &str {
        "gitdaddy::RemoteImageLoader"
    }

    fn load(&self, _ctx: &Context, uri: &str, _size_hint: SizeHint) -> ImageLoadResult {
        if !is_allowed_remote(uri) {
            return Err(LoadError::NotSupported);
        }
        let mut state = self.state.lock().map_err(|_| LoadError::Loading("image cache poisoned".into()))?;
        match state.entries.get(uri) {
            Some(RemoteEntry::Ready(image)) => return Ok(ImagePoll::Ready { image: Arc::clone(image) }),
            Some(RemoteEntry::Queued) => return Ok(ImagePoll::Pending { size: None }),
            Some(RemoteEntry::Waiting(until)) if *until > Instant::now() => {
                return Err(LoadError::Loading("GitHub is rate-limiting preview images".into()));
            }
            Some(RemoteEntry::Failed(err)) => return Err(LoadError::Loading(err.clone())),
            Some(RemoteEntry::Waiting(_)) | None => {}
        }
        state.insert(uri.to_owned(), RemoteEntry::Queued);
        drop(state);
        if let Ok(jobs) = self.jobs.lock() {
            let _ = jobs.send(uri.to_owned());
        }
        Ok(ImagePoll::Pending { size: None })
    }

    fn forget(&self, uri: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.entries.remove(uri);
            state.order.retain(|u| u != uri);
        }
    }

    fn forget_all(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.entries.clear();
            state.order.clear();
        }
    }

    fn byte_size(&self) -> usize {
        self.state.lock().map_or(0, |state| {
            state
                .entries
                .values()
                .map(|entry| match entry {
                    RemoteEntry::Ready(image) => image.pixels.len() * 4,
                    _ => 0,
                })
                .sum()
        })
    }

    fn has_pending(&self) -> bool {
        self.state.lock().is_ok_and(|state| state.entries.values().any(|e| matches!(e, RemoteEntry::Queued)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(file_uri_to_path("file:///D:/repo/a%20b.png"), Some(PathBuf::from("D:/repo/a b.png")));
        assert_eq!(file_uri_to_path("https://x/y.png"), None);
    }

    #[test]
    fn reads_githubs_rate_limit_answers() {
        assert_eq!(rate_limit(200, Some(42), None, None, 0), None);
        assert_eq!(rate_limit(404, None, None, None, 0), None);
        // Budget left: only this image is refused.
        assert_eq!(rate_limit(429, Some(37), None, None, 0), Some(Limit::Busy));
        assert_eq!(rate_limit(429, None, None, None, 0), Some(Limit::Busy));
        // Budget spent: Retry-After wins, then the reset time, capped at 15 minutes.
        assert_eq!(
            rate_limit(429, Some(0), Some(900), Some(5_000), 1_000),
            Some(Limit::Spent(Duration::from_secs(900)))
        );
        assert_eq!(rate_limit(429, Some(0), None, Some(1_120), 1_000), Some(Limit::Spent(Duration::from_secs(120))));
        assert_eq!(rate_limit(403, Some(0), Some(86_400), None, 0), Some(Limit::Spent(MAX_PAUSE)));
        assert_eq!(rate_limit(403, Some(12), None, None, 0), None);
    }

    #[test]
    fn only_github_image_hosts_are_fetched() {
        assert!(is_allowed_remote("https://opengraph.githubassets.com/abc/owner/repo"));
        assert!(is_allowed_remote("https://repository-images.githubusercontent.com/1/2"));
        assert!(!is_allowed_remote("http://opengraph.githubassets.com/abc"));
        assert!(!is_allowed_remote("https://example.com/tracker.png"));
        assert!(!is_allowed_remote("https://opengraph.githubassets.com.evil.io/x"));
    }

    #[test]
    fn large_images_are_downscaled() {
        let path = std::env::temp_dir().join(format!("gitdaddy-big-{}.png", std::process::id()));
        image::RgbaImage::from_pixel(3200, 1000, image::Rgba([10, 20, 30, 255])).save(&path).unwrap();
        let decoded = decode(&path).unwrap();
        assert_eq!(decoded.size, [1600, 500]);
        let _ = std::fs::remove_file(&path);
    }
}
