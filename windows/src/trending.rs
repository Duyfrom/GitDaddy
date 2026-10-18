//! Explore's trending data, rebuilt on free sources the way Trendshift does it.
//!
//! GitHub no longer lists who starred a repository or when, and its public event stream stopped
//! carrying stars, so no API answers "stars gained today". Trendshift records star counts over
//! time and subtracts; GitDaddy does the same for the repositories it sees, in a small local
//! history. Repositories created within a period need no history: every star they have was
//! gained in it. Rankings use Trendshift's score, 8 × stars gained + 2 × forks gained.
//!
//! Live mentions come from Hacker News (Algolia's search API) and Mastodon's public #github
//! timelines. Both are free and need no key.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Trendshift weighs a star four times a fork.
pub fn score(stars: u64, forks: u64) -> u64 {
    8 * stars + 2 * forks
}

pub fn unix_now() -> i64 {
    Utc::now().timestamp()
}

/// Seconds since the epoch for an RFC 3339 time, as GitHub and the mention sources write them.
pub fn parse_time(iso: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(iso).ok().map(|t| t.timestamp())
}

/// `owner/name` from a github.com repository URL; `None` for gists, profiles, and site pages.
pub fn repo_slug(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let path = rest.strip_prefix("github.com/")?;
    let mut parts = path.split(['/', '?', '#']);
    let owner = parts.next()?;
    let name = parts.next()?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    const RESERVED: [&str; 18] = [
        "about",
        "apps",
        "collections",
        "enterprise",
        "explore",
        "features",
        "login",
        "marketplace",
        "notifications",
        "orgs",
        "pricing",
        "settings",
        "site",
        "sponsors",
        "topics",
        "trending",
        "users",
        "search",
    ];
    let valid = |s: &str| {
        !s.is_empty() && s.len() <= 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    if !valid(owner) || !valid(name) || name.chars().all(|c| c == '.') {
        return None;
    }
    if RESERVED.contains(&owner.to_ascii_lowercase().as_str()) {
        return None;
    }
    Some(format!("{owner}/{name}"))
}

// MARK: - Periods

/// The span of time a ranking covers, in Unix seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub start: i64,
    /// Exclusive. For the current period this is now.
    pub end: i64,
    /// Whether the period is still going: gains then run up to the live counts.
    pub current: bool,
    /// Calendar year the period ends in, for the "New" badge.
    pub year: i32,
}

// MARK: - Momentum

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Basis {
    /// Created during the (current) period: every star counts.
    NewInPeriod,
    /// Created during a past period. Its stars since then are all GitDaddy can know.
    SinceCreation,
    /// From star counts GitDaddy recorded. `complete` is false when the first recording falls
    /// after the period began, so the gain only covers the time since `since`.
    Recorded { since: i64, complete: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Momentum {
    pub stars_gained: u64,
    pub forks_gained: u64,
    pub score: u64,
    pub basis: Basis,
    pub mentioned_on: Vec<Platform>,
    /// Set when the repository was created in the period's year (Trendshift's "New 2026").
    pub new_in: Option<i32>,
}

/// What a repository's numbers look like to the ranking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counts {
    pub slug: String,
    pub stars: u64,
    pub forks: u64,
    pub created_at: String,
    pub language: Option<String>,
}

/// Works out how much a repository gained in `window`, or `None` if GitDaddy can't tell.
pub fn momentum(counts: &Counts, history: &History, window: &Window) -> Option<Momentum> {
    let created = parse_time(&counts.created_at);
    let new_in =
        created.and_then(|t| Utc.timestamp_opt(t, 0).single()).filter(|t| t.year() == window.year).map(|t| t.year());
    let (stars_gained, forks_gained, basis) = match created {
        Some(created) if created >= window.start && created < window.end.max(window.start + 1) => {
            let basis = if window.current { Basis::NewInPeriod } else { Basis::SinceCreation };
            (counts.stars, counts.forks, basis)
        }
        _ => {
            let live = window.current.then_some((counts.stars, counts.forks));
            let gain = history.gain(&counts.slug, window.start, window.end, live)?;
            (gain.stars, gain.forks, Basis::Recorded { since: gain.since, complete: gain.complete })
        }
    };
    Some(Momentum {
        stars_gained,
        forks_gained,
        score: score(stars_gained, forks_gained),
        basis,
        mentioned_on: Vec::new(),
        new_in,
    })
}

// MARK: - Star history

/// Points closer together than this are not worth keeping; the tracker runs hourly.
const MIN_GAP_SECS: i64 = 50 * 60;
/// Recent points are kept as recorded; older ones thin to the first and last of each UTC day,
/// and after two months to the first of each month (enough for monthly and yearly bases).
const KEEP_ALL_SECS: i64 = 3 * 86_400;
const KEEP_DAILY_SECS: i64 = 62 * 86_400;
/// Repositories nobody has looked at for this long are dropped.
const FORGET_AFTER_SECS: i64 = 45 * 86_400;
const MAX_TRACKED: usize = 800;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Track {
    name: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    created_at: String,
    /// `[unix seconds, stars, forks]`, oldest first.
    points: Vec<(i64, u64, u64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gain {
    pub stars: u64,
    pub forks: u64,
    /// Time of the count the gain is measured from.
    pub since: i64,
    pub complete: bool,
}

/// Star and fork counts GitDaddy has seen, by lowercase `owner/name`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct History {
    repos: HashMap<String, Track>,
    /// When the tracker last added a fresh batch of likely risers.
    #[serde(default)]
    pub last_seeded: i64,
    /// Only a history loaded from disk is written back, so tests and previews never touch it.
    #[serde(skip)]
    persistent: bool,
}

impl History {
    fn path() -> Option<PathBuf> {
        dirs::data_local_dir().map(|dir| dir.join("GitDaddy").join("star-history.json"))
    }

    pub fn load() -> Self {
        let history: Option<Self> = Self::path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        Self { persistent: true, ..history.unwrap_or_default() }
    }

    pub fn save(&self) {
        if !self.persistent {
            return;
        }
        let Some(path) = Self::path() else { return };
        let Ok(bytes) = serde_json::to_vec(self) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Write then rename, so a crash mid-write never leaves half a file.
        let temp = path.with_extension("json.tmp");
        if std::fs::write(&temp, bytes).is_ok() {
            let _ = std::fs::rename(&temp, &path);
        }
    }

    /// Time of the oldest count still kept.
    pub fn first_recorded(&self) -> Option<i64> {
        self.repos.values().filter_map(|t| t.points.first().map(|p| p.0)).min()
    }

    pub fn record(&mut self, counts: &Counts, now: i64) {
        let track = self.repos.entry(counts.slug.to_lowercase()).or_default();
        track.name.clone_from(&counts.slug);
        if counts.language.is_some() {
            track.language.clone_from(&counts.language);
        }
        if !counts.created_at.is_empty() {
            track.created_at.clone_from(&counts.created_at);
        }
        // Keep the earlier point of two close ones: it sits nearer a period's start.
        if track.points.last().is_some_and(|last| now - last.0 < MIN_GAP_SECS) {
            return;
        }
        track.points.push((now, counts.stars, counts.forks));
    }

    /// Slugs worth refreshing, most recently seen first.
    pub fn tracked(&self, limit: usize) -> Vec<String> {
        let mut tracks: Vec<&Track> = self.repos.values().filter(|t| !t.points.is_empty()).collect();
        tracks.sort_by_key(|t| std::cmp::Reverse(t.points.last().map_or(0, |p| p.0)));
        tracks.into_iter().take(limit).map(|t| t.name.clone()).collect()
    }

    /// Gain between the period's start and its end (or the live counts, for a current period).
    pub fn gain(&self, slug: &str, start: i64, end: i64, live: Option<(u64, u64)>) -> Option<Gain> {
        let points = &self.repos.get(&slug.to_lowercase())?.points;
        let (base, complete) = match points.iter().rev().find(|p| p.0 <= start) {
            Some(point) => (*point, true),
            None => (*points.iter().find(|p| p.0 > start && p.0 < end)?, false),
        };
        let (stars, forks) = match live {
            Some(counts) => counts,
            None => {
                let last = points.iter().rev().find(|p| p.0 <= end && p.0 > base.0)?;
                (last.1, last.2)
            }
        };
        Some(Gain { stars: stars.saturating_sub(base.1), forks: forks.saturating_sub(base.2), since: base.0, complete })
    }

    /// Recorded repositories that gained the most in `window`, optionally in one language.
    pub fn risers(&self, window: &Window, language: Option<&str>, limit: usize) -> Vec<String> {
        let mut gains: Vec<(u64, &str)> = self
            .repos
            .values()
            .filter(|t| language.is_none_or(|l| t.language.as_deref().is_some_and(|tl| tl.eq_ignore_ascii_case(l))))
            .filter_map(|t| {
                let last = t.points.last()?;
                let live = window.current.then_some((last.1, last.2));
                let gain = self.gain(&t.name, window.start, window.end, live)?;
                Some((score(gain.stars, gain.forks), t.name.as_str()))
            })
            .filter(|(score, _)| *score > 0)
            .collect();
        gains.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        gains.into_iter().take(limit).map(|(_, name)| name.to_string()).collect()
    }

    /// Thins old points and forgets repositories nobody has seen for a while.
    pub fn compact(&mut self, now: i64) {
        let day = |t: i64| t.div_euclid(86_400);
        let month = |t: i64| Utc.timestamp_opt(t, 0).single().map_or(0, |d| d.year() * 12 + d.month() as i32);
        for track in self.repos.values_mut() {
            let mut kept: Vec<(i64, u64, u64)> = Vec::with_capacity(track.points.len());
            let points = &track.points;
            for (i, point) in points.iter().enumerate() {
                let age = now - point.0;
                let keep = if age <= KEEP_ALL_SECS {
                    true
                } else if age <= KEEP_DAILY_SECS {
                    let first_of_day = i == 0 || day(points[i - 1].0) != day(point.0);
                    let last_of_day = i + 1 == points.len() || day(points[i + 1].0) != day(point.0);
                    first_of_day || last_of_day
                } else {
                    i == 0 || month(points[i - 1].0) != month(point.0)
                };
                if keep {
                    kept.push(*point);
                }
            }
            track.points = kept;
        }
        self.repos.retain(|_, t| t.points.last().is_some_and(|p| now - p.0 <= FORGET_AFTER_SECS));
        if self.repos.len() > MAX_TRACKED {
            let mut by_recency: Vec<(i64, String)> =
                self.repos.iter().map(|(k, t)| (t.points.last().map_or(0, |p| p.0), k.clone())).collect();
            by_recency.sort();
            let excess = self.repos.len() - MAX_TRACKED;
            for (_, key) in by_recency.into_iter().take(excess) {
                self.repos.remove(&key);
            }
        }
    }
}

// MARK: - Mentions

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Platform {
    HackerNews,
    Mastodon,
}

impl Platform {
    pub fn label(self) -> &'static str {
        match self {
            Self::HackerNews => "Hacker News",
            Self::Mastodon => "Mastodon",
        }
    }
}

/// A post that links a GitHub repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    pub platform: Platform,
    /// Unique within the feed.
    pub id: String,
    /// `owner/name` as linked.
    pub slug: String,
    pub title: String,
    /// Plain text of the post (Mastodon); empty for Hacker News link posts.
    pub text: String,
    pub author: String,
    /// The post itself.
    pub link: String,
    pub posted_at: String,
    /// Hacker News points or Mastodon favourites.
    pub points: u64,
    pub comments: u64,
}

const HN_SEARCH: &str = "https://hn.algolia.com/api/v1/search_by_date";
/// Public #github tag timelines; the second instance is where many developers post.
const MASTODON_TIMELINES: [&str; 2] = [
    "https://mastodon.social/api/v1/timelines/tag/github?limit=40",
    "https://fosstodon.org/api/v1/timelines/tag/github?limit=40",
];

fn get_json(url: &str) -> Result<Value, String> {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    let agent = AGENT.get_or_init(|| crate::update::http_agent(Some(Duration::from_secs(20))));
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    let body = response.body_mut().with_config().limit(8 * 1024 * 1024).read_to_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

/// Hacker News stories linking a GitHub repository, posted in `[since, until)`, newest first.
pub fn fetch_hacker_news(since: i64, until: Option<i64>, limit: u32) -> Result<Vec<Mention>, String> {
    let mut filter = format!("created_at_i>={since}");
    if let Some(until) = until {
        filter.push_str(&format!(",created_at_i<{until}"));
    }
    let filter = filter.replace('>', "%3E").replace('<', "%3C").replace('=', "%3D").replace(',', "%2C");
    let url = format!(
        "{HN_SEARCH}?query=github.com&restrictSearchableAttributes=url&tags=story&numericFilters={filter}&hitsPerPage={limit}"
    );
    Ok(parse_hacker_news(&get_json(&url)?))
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}

pub fn parse_hacker_news(json: &Value) -> Vec<Mention> {
    let Some(hits) = json["hits"].as_array() else { return Vec::new() };
    hits.iter()
        .filter_map(|hit| {
            let slug = repo_slug(hit["url"].as_str()?)?;
            let id = hit["objectID"].as_str()?.to_string();
            Some(Mention {
                platform: Platform::HackerNews,
                link: format!("https://news.ycombinator.com/item?id={id}"),
                id: format!("hn:{id}"),
                slug,
                title: text(&hit["title"]),
                text: String::new(),
                author: text(&hit["author"]),
                posted_at: text(&hit["created_at"]),
                points: hit["points"].as_u64().unwrap_or(0),
                comments: hit["num_comments"].as_u64().unwrap_or(0),
            })
        })
        .collect()
}

/// Latest #github posts that link a repository, from every timeline that answers.
pub fn fetch_mastodon() -> Result<Vec<Mention>, String> {
    let mut mentions = Vec::new();
    let mut last_error = None;
    for url in MASTODON_TIMELINES {
        match get_json(url) {
            Ok(json) => mentions.extend(parse_mastodon(&json)),
            Err(err) => last_error = Some(err),
        }
    }
    match last_error {
        Some(err) if mentions.is_empty() => Err(err),
        _ => Ok(mentions),
    }
}

pub fn parse_mastodon(json: &Value) -> Vec<Mention> {
    let Some(statuses) = json.as_array() else { return Vec::new() };
    statuses
        .iter()
        .filter_map(|status| {
            let content = status["content"].as_str()?;
            let slug = links(content).find_map(|href| repo_slug(&href))?;
            let link = text(&status["url"]);
            let body = html_to_text(content);
            let title = body.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim().to_string();
            Some(Mention {
                platform: Platform::Mastodon,
                id: format!("mastodon:{link}"),
                slug,
                title,
                text: body,
                author: format!("@{}", text(&status["account"]["acct"])),
                link,
                posted_at: text(&status["created_at"]),
                points: status["favourites_count"].as_u64().unwrap_or(0),
                comments: status["replies_count"].as_u64().unwrap_or(0),
            })
        })
        .collect()
}

/// `href` values in an HTML fragment.
fn links(html: &str) -> impl Iterator<Item = String> + '_ {
    html.split("href=\"").skip(1).filter_map(|rest| rest.split('"').next().map(decode_entities))
}

fn decode_entities(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// Mastodon post HTML as plain text: paragraphs and line breaks become newlines.
pub fn html_to_text(html: &str) -> String {
    let html = html.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n").replace("</p>", "\n\n");
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(out.trim())
}

/// Newest first, one entry per post, at most `limit`.
pub fn merge_mentions(mut mentions: Vec<Mention>, limit: usize) -> Vec<Mention> {
    let mut seen = HashSet::new();
    mentions.retain(|m| seen.insert(m.id.clone()));
    mentions.sort_by_key(|m| std::cmp::Reverse(parse_time(&m.posted_at).unwrap_or(0)));
    mentions.truncate(limit);
    mentions
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    fn counts(slug: &str, stars: u64, forks: u64, created_at: &str) -> Counts {
        Counts { slug: slug.into(), stars, forks, created_at: created_at.into(), language: Some("Rust".into()) }
    }

    #[test]
    fn slugs_come_only_from_repository_links() {
        assert_eq!(repo_slug("https://github.com/aixisstudio/Snitch").as_deref(), Some("aixisstudio/Snitch"));
        assert_eq!(repo_slug("https://www.github.com/a/b.git").as_deref(), Some("a/b"));
        assert_eq!(
            repo_slug("https://github.com/TheWariend/MuscleMemory/tree/main/benchmarks").as_deref(),
            Some("TheWariend/MuscleMemory")
        );
        assert_eq!(repo_slug("http://github.com/x/y?tab=readme").as_deref(), Some("x/y"));
        assert_eq!(repo_slug("https://gist.github.com/rcalixte/d2de5c816f"), None);
        assert_eq!(repo_slug("https://github.com/torvalds"), None);
        assert_eq!(repo_slug("https://github.com/orgs/acme/projects"), None);
        assert_eq!(repo_slug("https://github.com/features/copilot"), None);
        assert_eq!(repo_slug("https://example.com/github.com/a/b"), None);
        assert_eq!(repo_slug("https://github.com/a/b\"onerror"), None);
    }

    #[test]
    fn trendshift_score() {
        // Matches Trendshift's daily list: google-research/harness2 scored 29,918.
        assert_eq!(score(3704, 143), 29_918);
        assert_eq!(score(702, 32), 5_680);
    }

    #[test]
    fn gains_come_from_recorded_counts() {
        let mut history = History::default();
        let start = 1_000 * DAY;
        history.record(&counts("a/old", 100, 10, "2020-01-01T00:00:00Z"), start - 3_600);
        history.record(&counts("a/old", 160, 12, ""), start + 6 * 3_600);
        history.record(&counts("a/old", 190, 13, ""), start + 12 * 3_600);
        // A point within the minimum gap is skipped.
        history.record(&counts("a/old", 999, 99, ""), start + 12 * 3_600 + 60);

        let full = history.gain("A/OLD", start, start + DAY, None).unwrap();
        assert_eq!(full, Gain { stars: 90, forks: 3, since: start - 3_600, complete: true });
        let live = history.gain("a/old", start, start + DAY, Some((250, 20))).unwrap();
        assert_eq!((live.stars, live.forks, live.complete), (150, 10, true));
        // First recorded after the period began: a partial gain.
        let partial = history.gain("a/old", start - 2 * 3_600 - DAY, start - 1_800, Some((130, 11))).unwrap();
        assert_eq!((partial.stars, partial.complete, partial.since), (30, false, start - 3_600));
        assert_eq!(history.gain("a/unknown", start, start + DAY, None), None);
    }

    #[test]
    fn new_repositories_gain_every_star() {
        let history = History::default();
        let start = parse_time("2026-10-12T00:00:00Z").unwrap();
        let window = Window { start, end: start + 7 * DAY, current: true, year: 2026 };
        let fresh = momentum(&counts("n/new", 4_780, 261, "2026-10-13T08:00:00Z"), &history, &window).unwrap();
        assert_eq!(fresh.stars_gained, 4_780);
        assert_eq!(fresh.basis, Basis::NewInPeriod);
        assert_eq!(fresh.new_in, Some(2026));
        // Older and never recorded: GitDaddy can't tell, so it isn't ranked.
        assert_eq!(momentum(&counts("o/old", 9_000, 10, "2025-01-01T00:00:00Z"), &history, &window), None);
        let past = Window { current: false, ..window };
        let created_then = momentum(&counts("n/new", 4_780, 261, "2026-10-13T08:00:00Z"), &history, &past).unwrap();
        assert_eq!(created_then.basis, Basis::SinceCreation);
    }

    #[test]
    fn risers_rank_by_recorded_score() {
        let mut history = History::default();
        let start = 2_000 * DAY;
        for (slug, before, after) in [("a/slow", 10, 15), ("b/fast", 10, 400), ("c/flat", 10, 10)] {
            history.record(&counts(slug, before, 0, "2019-01-01T00:00:00Z"), start - 60);
            history.record(&counts(slug, after, 0, ""), start + 3_600);
        }
        let window = Window { start, end: start + DAY, current: true, year: 2026 };
        assert_eq!(history.risers(&window, None, 10), vec!["b/fast".to_string(), "a/slow".to_string()]);
        assert_eq!(history.risers(&window, Some("go"), 10), Vec::<String>::new());
    }

    #[test]
    fn compaction_thins_old_points() {
        let mut history = History::default();
        let now = 3_000 * DAY;
        // Hourly points for 100 days.
        for hour in 0..(100 * 24) {
            let t = now - 100 * DAY + hour * 3_600;
            history.record(&counts("a/b", hour as u64, 0, ""), t);
        }
        history.record(&counts("gone/away", 1, 0, ""), now - 60 * DAY);
        history.compact(now);
        let points = &history.repos["a/b"].points;
        assert!(points.len() < 72 + 2 * 60 + 4, "kept {} points", points.len());
        assert!(points.len() > 72, "kept {} points", points.len());
        assert!(!history.repos.contains_key("gone/away"));
        // The month's first point survives, so monthly and yearly bases still resolve.
        assert_eq!(points[0].0, now - 100 * DAY);
        let json = serde_json::to_string(&history).unwrap();
        let back: History = serde_json::from_str(&json).unwrap();
        assert_eq!(back.repos["a/b"].points, *points);
    }

    #[test]
    fn parses_hacker_news_hits() {
        let json = serde_json::json!({"hits": [
            {"objectID": "50144098", "title": "Show HN: Snitch – see every connection your computer makes",
             "url": "https://github.com/aixisstudio/Snitch", "author": "aixis", "points": 41,
             "num_comments": 9, "created_at": "2026-10-18T18:52:27Z"},
            {"objectID": "1", "title": "A gist", "url": "https://gist.github.com/x/y", "created_at": "2026-10-18T18:00:00Z"},
            {"objectID": "2", "title": "Ask HN", "url": null, "created_at": "2026-10-18T18:00:00Z"}
        ]});
        let mentions = parse_hacker_news(&json);
        assert_eq!(mentions.len(), 1);
        let m = &mentions[0];
        assert_eq!((m.slug.as_str(), m.points, m.comments), ("aixisstudio/Snitch", 41, 9));
        assert_eq!(m.link, "https://news.ycombinator.com/item?id=50144098");
        assert_eq!(m.platform, Platform::HackerNews);
    }

    #[test]
    fn parses_mastodon_posts() {
        let json = serde_json::json!([
            {"url": "https://mastodon.social/@kellylford/117462709697032870", "created_at": "2026-10-18T15:51:49.979Z",
             "account": {"acct": "kellylford"}, "favourites_count": 4, "replies_count": 1,
             "content": "<p>New tool for managing repos &amp; issues:<br>GHManage</p><p><a href=\"https://mastodon.social/tags/github\" class=\"hashtag\">#<span>github</span></a> <a href=\"https://github.com/TheIdeaPlace/GHManage\" rel=\"nofollow\"><span>github.com/TheIdeaPlace/GHManage</span></a></p>"},
            {"url": "https://mastodon.social/@x/1", "created_at": "2026-10-18T15:00:00Z", "account": {"acct": "x"},
             "content": "<p>No repository here <a href=\"https://github.com/features\">features</a></p>"}
        ]);
        let mentions = parse_mastodon(&json);
        assert_eq!(mentions.len(), 1);
        let m = &mentions[0];
        assert_eq!(m.slug, "TheIdeaPlace/GHManage");
        assert_eq!(m.author, "@kellylford");
        assert_eq!(m.title, "New tool for managing repos & issues:");
        assert!(m.text.contains("GHManage\n\n#github"));
        assert_eq!(m.points, 4);
    }

    #[test]
    fn mentions_merge_newest_first() {
        let m = |id: &str, at: &str| Mention {
            platform: Platform::HackerNews,
            id: id.into(),
            slug: "a/b".into(),
            title: String::new(),
            text: String::new(),
            author: String::new(),
            link: String::new(),
            posted_at: at.into(),
            points: 0,
            comments: 0,
        };
        let merged = merge_mentions(
            vec![m("1", "2026-10-18T10:00:00Z"), m("2", "2026-10-18T12:00:00Z"), m("1", "2026-10-18T10:00:00Z")],
            10,
        );
        assert_eq!(merged.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["2", "1"]);
    }

    #[test]
    #[ignore = "calls Hacker News and Mastodon; run with `cargo test live_mentions -- --ignored --nocapture`"]
    fn live_mentions() {
        let hn = fetch_hacker_news(unix_now() - DAY, None, 100).expect("hacker news");
        eprintln!("hacker news: {} repo stories, newest {:?}", hn.len(), hn.first().map(|m| (&m.slug, &m.title)));
        let mastodon = fetch_mastodon().expect("mastodon");
        eprintln!(
            "mastodon: {} repo posts, newest {:?}",
            mastodon.len(),
            mastodon.first().map(|m| (&m.slug, &m.author))
        );
        assert!(!hn.is_empty());
    }
}
