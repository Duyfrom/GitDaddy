import AppKit
import Foundation
import SwiftUI

// MARK: - GitHub CLI Status & Forking

enum GhCliStatus: Equatable, Sendable {
    case checking
    case ready(user: String, orgs: [String])
    case notAuthenticated(String)
    case notInstalled
}

struct ForkRemotePlan: Equatable, Sendable {
    let forkRemote: String
    let originalRemote: String
    let renamesOrigin: Bool
}

// MARK: - GitHub Hub Sections & Models

enum HubSection: String, CaseIterable, Identifiable, Hashable, Sendable {
    case pullRequests = "Pull Requests"
    case issues = "Issues"
    case reviewRequests = "Review Requests"
    case starred = "Starred"
    case explore = "Explore"

    var id: String { rawValue }

    var label: String { rawValue }

    var iconName: String {
        switch self {
        case .pullRequests: return "arrow.triangle.pull"
        case .issues: return "smallcircle.filled.circle"
        case .reviewRequests: return "eye"
        case .starred: return "star"
        case .explore: return "safari"
        }
    }

    var webURL: String {
        switch self {
        case .pullRequests: return "https://github.com/pulls"
        case .issues: return "https://github.com/issues"
        case .reviewRequests: return "https://github.com/pulls/review-requested"
        case .starred: return "https://github.com/stars"
        case .explore: return "https://github.com/trending"
        }
    }

    var usesInvolvement: Bool {
        switch self {
        case .pullRequests, .issues: return true
        default: return false
        }
    }

    var isRepoList: Bool {
        switch self {
        case .starred, .explore: return true
        default: return false
        }
    }
}

enum Involvement: String, CaseIterable, Identifiable, Sendable {
    case created = "Created"
    case assigned = "Assigned"
    case mentioned = "Mentioned"

    var id: String { rawValue }
    var label: String { rawValue }

    var qualifier: String {
        switch self {
        case .created: return "author:@me"
        case .assigned: return "assignee:@me"
        case .mentioned: return "mentions:@me"
        }
    }
}

// MARK: - Explore Periods & Momentum Models

struct ExploreWindow: Equatable, Sendable {
    let start: Int64
    let end: Int64
    let current: Bool
    let year: Int
    let startDateString: String
    let endDateString: String
}

enum TrendingSpan: String, CaseIterable, Identifiable, Sendable {
    case day = "Daily"
    case week = "Weekly"
    case month = "Monthly"
    case year = "Yearly"

    var id: String { rawValue }
    var label: String { rawValue }

    private static var utcCalendar: Calendar {
        var cal = Calendar(identifier: .iso8601)
        cal.timeZone = TimeZone(secondsFromGMT: 0)!
        cal.firstWeekday = 2 // Monday
        return cal
    }

    /// Inclusive UTC date range for `self` stepped back `offset` periods (`0` = current period).
    func utcDateRange(offset: Int, now: Date = Date()) -> (start: Date, end: Date) {
        let cal = Self.utcCalendar
        let today = cal.startOfDay(for: now)
        let current = offset <= 0
        let step = max(0, offset)

        switch self {
        case .day:
            let day = cal.date(byAdding: .day, value: -step, to: today) ?? today
            return (day, day)

        case .week:
            let weekday = cal.component(.weekday, from: today)
            let daysFromMonday = (weekday + 5) % 7
            let thisMonday = cal.date(byAdding: .day, value: -daysFromMonday, to: today) ?? today
            let start = cal.date(byAdding: .day, value: -7 * step, to: thisMonday) ?? thisMonday
            let end = current ? today : (cal.date(byAdding: .day, value: 6, to: start) ?? start)
            return (start, end)

        case .month:
            let comps = cal.dateComponents([.year, .month], from: today)
            let thisMonthStart = cal.date(from: comps) ?? today
            let start = cal.date(byAdding: .month, value: -step, to: thisMonthStart) ?? thisMonthStart
            let end: Date
            if current {
                end = today
            } else {
                let nextMonth = cal.date(byAdding: .month, value: 1, to: start) ?? start
                end = cal.date(byAdding: .day, value: -1, to: nextMonth) ?? start
            }
            return (start, end)

        case .year:
            let thisYear = cal.component(.year, from: today)
            let targetYear = thisYear - step
            let start = cal.date(from: DateComponents(year: targetYear, month: 1, day: 1)) ?? today
            let end = current
                ? today
                : (cal.date(from: DateComponents(year: targetYear, month: 12, day: 31)) ?? today)
            return (start, end)
        }
    }

    func window(offset: Int, now: Date = Date()) -> ExploreWindow {
        let cal = Self.utcCalendar
        let (start, end) = utcDateRange(offset: offset, now: now)
        let current = offset <= 0
        let startEpoch = Int64(start.timeIntervalSince1970)
        let endEpoch: Int64
        if current {
            endEpoch = Int64(now.timeIntervalSince1970)
        } else {
            let nextDay = cal.date(byAdding: .day, value: 1, to: end) ?? end
            endEpoch = Int64(nextDay.timeIntervalSince1970)
        }
        let year = cal.component(.year, from: end)
        return ExploreWindow(
            start: startEpoch,
            end: endEpoch,
            current: current,
            year: year,
            startDateString: Self.formatUTCDate(start),
            endDateString: Self.formatUTCDate(end)
        )
    }

    func canAdvance(offset: Int, now: Date = Date()) -> Bool {
        let (start, _) = utcDateRange(offset: offset, now: now)
        // GitHub launch date: 2008-04-10
        return Self.formatUTCDate(start) > "2008-04-10"
    }

    func periodLabel(offset: Int, now: Date = Date()) -> String {
        let cal = Self.utcCalendar
        let (start, end) = utcDateRange(offset: offset, now: now)
        let step = max(0, offset)

        switch (self, step) {
        case (.day, 0): return "Today"
        case (.day, 1): return "Yesterday"
        case (.day, _): return Self.formatUTCDisplay(start, format: "MMM d, yyyy")
        case (.week, 0): return "This week"
        case (.week, 1): return "Last week"
        case (.week, _):
            let startYear = cal.component(.year, from: start)
            let endYear = cal.component(.year, from: end)
            if startYear == endYear {
                return "\(Self.formatUTCDisplay(start, format: "MMM d")) – \(Self.formatUTCDisplay(end, format: "MMM d, yyyy"))"
            } else {
                return "\(Self.formatUTCDisplay(start, format: "MMM d, yyyy")) – \(Self.formatUTCDisplay(end, format: "MMM d, yyyy"))"
            }
        case (.month, 0): return "This month"
        case (.month, _): return Self.formatUTCDisplay(start, format: "MMMM yyyy")
        case (.year, 0): return "This year"
        case (.year, _): return Self.formatUTCDisplay(start, format: "yyyy")
        }
    }

    func captionRangeText(offset: Int, now: Date = Date()) -> String {
        let (start, end) = utcDateRange(offset: offset, now: now)
        if start == end {
            return "on \(Self.formatUTCDisplay(start, format: "MMM d, yyyy"))"
        }
        return "from \(Self.formatUTCDisplay(start, format: "MMM d")) to \(Self.formatUTCDisplay(end, format: "MMM d, yyyy"))"
    }

    func query(language: String?, offset: Int, now: Date = Date()) -> String {
        let win = window(offset: offset, now: now)
        var q = "created:\(win.startDateString)..\(win.endDateString) sort:stars"
        if let lang = language?.trimmingCharacters(in: .whitespacesAndNewlines), !lang.isEmpty {
            q += " language:\"\(lang)\""
        }
        return q
    }

    static func formatUTCDate(_ date: Date) -> String {
        formatUTCDisplay(date, format: "yyyy-MM-dd")
    }

    static func formatUTCDisplay(_ date: Date, format: String) -> String {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = format
        return formatter.string(from: date)
    }

    static let trendingLanguages: [String] = [
        "Rust",
        "TypeScript",
        "Python",
        "Go",
        "JavaScript",
        "Java",
        "C++",
        "C#",
        "C",
        "Swift",
        "Kotlin",
        "Zig",
        "Ruby",
        "PHP",
        "Shell",
        "Dart"
    ]
}

enum MentionPlatform: String, CaseIterable, Identifiable, Comparable, Sendable {
    case hackerNews = "Hacker News"
    case mastodon = "Mastodon"

    var id: String { rawValue }
    var label: String { rawValue }

    var brandColor: Color {
        switch self {
        case .hackerNews:
            return Color(red: 1.0, green: 0.4, blue: 0.0)
        case .mastodon:
            return Color(red: 99.0 / 255.0, green: 100.0 / 255.0, blue: 1.0)
        }
    }

    static func < (lhs: MentionPlatform, rhs: MentionPlatform) -> Bool {
        if lhs == rhs { return false }
        return lhs == .hackerNews
    }
}

enum MomentumBasis: Equatable, Sendable {
    case newInPeriod
    case sinceCreation
    case recorded(since: Int64, complete: Bool)

    var descriptionText: String {
        switch self {
        case .newInPeriod:
            return "New repository: all its stars count"
        case .sinceCreation:
            return "Stars since it was created"
        case .recorded(let since, let complete):
            let date = Date(timeIntervalSince1970: TimeInterval(since))
            let formatted = TrendingSpan.formatUTCDisplay(date, format: "MMM d HH:mm 'UTC'")
            return complete ? "Recorded since \(formatted)" : "Partial: since \(formatted)"
        }
    }

    var shortCardLabel: String {
        switch self {
        case .newInPeriod, .recorded(_, complete: true):
            return ""
        case .sinceCreation:
            return "Stars since created"
        case .recorded(_, complete: false):
            return "Partial period"
        }
    }
}

struct RepoMomentum: Equatable, Sendable {
    let starsGained: Int
    let forksGained: Int
    let score: Int
    let basis: MomentumBasis
    var mentionedOn: [MentionPlatform]
    let newInYear: Int?

    static func computeScore(stars: Int, forks: Int) -> Int {
        8 * max(0, stars) + 2 * max(0, forks)
    }

    func tooltip(totalStars: Int) -> String {
        var lines: [String] = [
            "+\(GitHubService.formatThousands(starsGained)) stars, +\(GitHubService.formatThousands(forksGained)) forks this period · score \(GitHubService.formatThousands(score))",
            "\(GitHubService.formatThousands(totalStars)) stars in total",
            basis.descriptionText
        ]
        if !mentionedOn.isEmpty {
            let names = mentionedOn.map(\.label).joined(separator: " and ")
            lines.append("Mentioned on \(names)")
        }
        return lines.joined(separator: "\n")
    }
}

struct RepoMention: Identifiable, Equatable, Sendable {
    let platform: MentionPlatform
    let id: String
    let slug: String
    let title: String
    let text: String
    let author: String
    let link: String
    let postedAt: String
    let postedRelative: String
    let postedEpoch: Int64
    let points: Int
    let comments: Int

    var statsLabel: String {
        let (ptWord, cmtWord) = platform == .hackerNews ? ("point", "comment") : ("favourite", "reply")
        func plural(_ n: Int, _ word: String) -> String {
            if n == 1 { return "1 \(word)" }
            if word == "reply" { return "\(n) replies" }
            return "\(n) \(word)s"
        }
        return "\(plural(points, ptWord)) · \(plural(comments, cmtWord))"
    }
}

enum RepoLayout: String, CaseIterable, Identifiable, Sendable {
    case list = "List"
    case cards = "Cards"

    var id: String { rawValue }
}

enum CiState: String, Equatable, Sendable {
    case success
    case pending
    case failure
}

enum ReviewDecision: String, Equatable, Sendable {
    case approved
    case changesRequested
    case reviewRequired

    var label: String {
        switch self {
        case .approved: return "Approved"
        case .changesRequested: return "Changes requested"
        case .reviewRequired: return "Review required"
        }
    }
}

struct HubLabel: Identifiable, Equatable, Sendable {
    var id: String { name }
    let name: String
    let colorHex: String

    var color: Color {
        GitHubService.colorFromHex(colorHex) ?? Theme.textSecondary
    }
}

struct HubItem: Identifiable, Equatable, Sendable {
    var id: String { url }
    let number: Int
    let title: String
    let url: String
    let repoSlug: String
    let author: String
    let updatedRelative: String
    let updatedEpoch: Int64
    let comments: Int
    let isPR: Bool
    let isDraft: Bool
    let headRef: String
    let baseRef: String
    let additions: Int
    let deletions: Int
    let reviewDecision: ReviewDecision?
    let ciState: CiState?
    let labels: [HubLabel]
    let body: String
}

struct HubRepo: Identifiable, Equatable, Sendable {
    var id: String { nameWithOwner }
    let nameWithOwner: String
    let name: String
    let owner: String
    let description: String
    let url: String
    let stars: Int
    let forks: Int
    let language: String?
    let languageColorHex: String?
    let updatedRelative: String
    let updatedEpoch: Int64
    let createdAt: String
    let createdEpoch: Int64
    let isFork: Bool
    let isArchived: Bool
    let isPrivate: Bool
    var viewerHasStarred: Bool
    let defaultBranch: String
    let primaryTopics: [String]
    let openGraphImageURL: String?
    var momentum: RepoMomentum? = nil

    var languageColor: Color? {
        guard let hex = languageColorHex else { return nil }
        return GitHubService.colorFromHex(hex)
    }
}

struct HubCounts: Equatable, Sendable {
    var pulls: Int = 0
    var issues: Int = 0
    var reviews: Int = 0
    var starred: Int = 0
}

// MARK: - Star History Tracker (Trendshift-style Local Star History)

struct StarHistoryPoint: Codable, Equatable, Sendable {
    let at: Int64
    let stars: Int
    let forks: Int

    init(at: Int64, stars: Int, forks: Int) {
        self.at = at
        self.stars = stars
        self.forks = forks
    }

    init(from decoder: Decoder) throws {
        var container = try decoder.unkeyedContainer()
        self.at = try container.decode(Int64.self)
        self.stars = try container.decode(Int.self)
        self.forks = try container.decode(Int.self)
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.unkeyedContainer()
        try container.encode(at)
        try container.encode(stars)
        try container.encode(forks)
    }
}

struct StarHistoryTrack: Codable, Equatable, Sendable {
    var name: String = ""
    var language: String? = nil
    var createdAt: String = ""
    var points: [StarHistoryPoint] = []

    enum CodingKeys: String, CodingKey {
        case name
        case language
        case createdAt = "created_at"
        case points
    }
}

struct StarHistoryFile: Codable, Sendable {
    var lastSeeded: Int64 = 0
    var repos: [String: StarHistoryTrack] = [:]

    enum CodingKeys: String, CodingKey {
        case lastSeeded = "last_seeded"
        case repos
    }
}

actor StarHistoryStore {
    static let shared = StarHistoryStore()

    private static let minRecordGapSecs: Int64 = 30 * 60
    private static let seedEverySecs: Int64 = 20 * 60 * 60
    private static let maxTrackedRepos: Int = 800

    private var data: StarHistoryFile
    private var loaded: Bool = false

    private init() {
        self.data = StarHistoryFile()
    }

    private static var historyFileURL: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory())
        let dir = base.appendingPathComponent("GitDaddy", isDirectory: true)
        return dir.appendingPathComponent("star-history.json")
    }

    private func ensureLoaded() {
        guard !loaded else { return }
        loaded = true
        let url = Self.historyFileURL
        guard let raw = try? Data(contentsOf: url),
              let decoded = try? JSONDecoder().decode(StarHistoryFile.self, from: raw) else {
            return
        }
        self.data = decoded
    }

    func save() {
        ensureLoaded()
        let url = Self.historyFileURL
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        guard let encoded = try? JSONEncoder().encode(data) else { return }
        try? encoded.write(to: url, options: .atomic)
    }

    func firstRecorded() -> Int64? {
        ensureLoaded()
        return data.repos.values.compactMap { $0.points.first?.at }.min()
    }

    func needsSeeding(now: Int64) -> Bool {
        ensureLoaded()
        return now - data.lastSeeded > Self.seedEverySecs
    }

    func markSeeded(now: Int64) {
        ensureLoaded()
        data.lastSeeded = now
    }

    func record(repo: HubRepo, at: Int64) {
        ensureLoaded()
        let key = repo.nameWithOwner.lowercased()
        var track = data.repos[key] ?? StarHistoryTrack()
        track.name = repo.nameWithOwner
        if let lang = repo.language {
            track.language = lang
        }
        if !repo.createdAt.isEmpty {
            track.createdAt = repo.createdAt
        }
        if let last = track.points.last, at - last.at < Self.minRecordGapSecs {
            track.points[track.points.count - 1] = StarHistoryPoint(at: at, stars: repo.stars, forks: repo.forks)
        } else {
            track.points.append(StarHistoryPoint(at: at, stars: repo.stars, forks: repo.forks))
        }
        data.repos[key] = track
    }

    func recordBatch(repos: [HubRepo], at: Int64) {
        ensureLoaded()
        for repo in repos {
            record(repo: repo, at: at)
        }
    }

    func trackedSlugs(limit: Int) -> [String] {
        ensureLoaded()
        var list: [(String, Int, Int64)] = data.repos.values.compactMap { track in
            guard let last = track.points.last else { return nil }
            let slug = track.name.isEmpty ? nil : track.name
            guard let slug else { return nil }
            return (slug, last.stars, last.at)
        }
        list.sort { a, b in
            if a.1 != b.1 { return a.1 > b.1 }
            return a.2 > b.2
        }
        return Array(list.prefix(limit).map(\.0))
    }

    func risers(window: ExploreWindow, language: String?, limit: Int) -> [String] {
        ensureLoaded()
        var scored: [(String, Int)] = []
        for (key, track) in data.repos {
            if let lang = language,
               track.language?.caseInsensitiveCompare(lang) != .orderedSame {
                continue
            }
            guard let (_, latest) = Self.windowBounds(points: track.points, window: window) else {
                continue
            }
            let slug = track.name.isEmpty ? key : track.name
            let dummy = HubRepo(
                nameWithOwner: slug,
                name: slug.split(separator: "/").last.map(String.init) ?? slug,
                owner: slug.split(separator: "/").first.map(String.init) ?? "",
                description: "",
                url: "https://github.com/\(slug)",
                stars: latest.stars,
                forks: latest.forks,
                language: track.language,
                languageColorHex: nil,
                updatedRelative: "",
                updatedEpoch: 0,
                createdAt: track.createdAt,
                createdEpoch: GitHubService.parseISORelative(track.createdAt).0,
                isFork: false,
                isArchived: false,
                isPrivate: false,
                viewerHasStarred: false,
                defaultBranch: "main",
                primaryTopics: [],
                openGraphImageURL: nil
            )
            if let m = computeMomentum(for: dummy, window: window), m.score > 0 {
                scored.append((slug, m.score))
            }
        }
        scored.sort { $0.1 > $1.1 }
        return Array(scored.prefix(limit).map(\.0))
    }

    func computeMomenta(
        for repos: [HubRepo],
        window: ExploreWindow,
        mentioned: [String: [MentionPlatform]],
        recordNow: Int64?
    ) -> ([HubRepo], Int64?) {
        ensureLoaded()
        if let stamp = recordNow {
            for repo in repos {
                record(repo: repo, at: stamp)
            }
        }
        var ranked: [HubRepo] = []
        for var repo in repos {
            guard var m = computeMomentum(for: repo, window: window) else { continue }
            var platforms = mentioned[repo.nameWithOwner.lowercased()] ?? []
            platforms.sort()
            m.mentionedOn = platforms
            if m.score > 0 {
                repo.momentum = m
                ranked.append(repo)
            }
        }
        if recordNow != nil {
            save()
        }
        ranked.sort { a, b in
            let sa = a.momentum?.score ?? 0
            let sb = b.momentum?.score ?? 0
            if sa != sb { return sa > sb }
            return a.stars > b.stars
        }
        return (ranked, firstRecorded())
    }

    func computeMomentum(for repo: HubRepo, window: ExploreWindow) -> RepoMomentum? {
        ensureLoaded()
        let created = repo.createdEpoch > 0 ? repo.createdEpoch : nil
        if let c = created, c >= window.end {
            return nil
        }
        let createdYear: Int? = created.map { epoch in
            var cal = Calendar(identifier: .gregorian)
            cal.timeZone = TimeZone(secondsFromGMT: 0)!
            return cal.component(.year, from: Date(timeIntervalSince1970: TimeInterval(epoch)))
        }
        let newInYear: Int? = (createdYear == window.year) ? window.year : nil

        let track = data.repos[repo.nameWithOwner.lowercased()]
        let recordedBounds: (StarHistoryPoint, StarHistoryPoint)? = track.flatMap { tr in
            guard let (base, endPt) = Self.windowBounds(points: tr.points, window: window) else {
                return nil
            }
            if window.current {
                return (base, StarHistoryPoint(at: window.end, stars: repo.stars, forks: repo.forks))
            } else {
                return (base, endPt)
            }
        }

        if let (baseline, endPt) = recordedBounds, baseline.at <= window.start {
            let starsGained = max(0, endPt.stars - baseline.stars)
            let forksGained = max(0, endPt.forks - baseline.forks)
            return RepoMomentum(
                starsGained: starsGained,
                forksGained: forksGained,
                score: RepoMomentum.computeScore(stars: starsGained, forks: forksGained),
                basis: .recorded(since: baseline.at, complete: true),
                mentionedOn: [],
                newInYear: newInYear
            )
        }

        if let c = created, c >= window.start {
            let (starsGained, forksGained): (Int, Int)
            if let (_, endPt) = recordedBounds, !window.current {
                starsGained = endPt.stars
                forksGained = endPt.forks
            } else {
                starsGained = repo.stars
                forksGained = repo.forks
            }
            let basis: MomentumBasis = window.current ? .newInPeriod : .sinceCreation
            return RepoMomentum(
                starsGained: starsGained,
                forksGained: forksGained,
                score: RepoMomentum.computeScore(stars: starsGained, forks: forksGained),
                basis: basis,
                mentionedOn: [],
                newInYear: newInYear
            )
        }

        if let (baseline, endPt) = recordedBounds, endPt.at > baseline.at {
            let starsGained = max(0, endPt.stars - baseline.stars)
            let forksGained = max(0, endPt.forks - baseline.forks)
            return RepoMomentum(
                starsGained: starsGained,
                forksGained: forksGained,
                score: RepoMomentum.computeScore(stars: starsGained, forks: forksGained),
                basis: .recorded(since: baseline.at, complete: false),
                mentionedOn: [],
                newInYear: newInYear
            )
        }

        return nil
    }

    private static func windowBounds(points: [StarHistoryPoint], window: ExploreWindow) -> (StarHistoryPoint, StarHistoryPoint)? {
        guard let endPt = points.last(where: { $0.at <= window.end }),
              endPt.at >= window.start else {
            return nil
        }
        let baseline = points.last(where: { $0.at <= window.start })
            ?? points.first(where: { $0.at >= window.start })
        guard let baseline else { return nil }
        return (baseline, endPt)
    }

    func compact(now: Int64) {
        ensureLoaded()
        let cutoff = now - 45 * 86_400
        data.repos = data.repos.filter { _, track in
            track.points.last.map { $0.at >= cutoff } ?? false
        }
        for key in Array(data.repos.keys) {
            if var track = data.repos[key] {
                track.points = Self.compactPoints(track.points, now: now)
                data.repos[key] = track
            }
        }
        if data.repos.count > Self.maxTrackedRepos {
            let keep = Set(trackedSlugs(limit: Self.maxTrackedRepos).map { $0.lowercased() })
            data.repos = data.repos.filter { keep.contains($0.key) }
        }
    }

    private static func compactPoints(_ points: [StarHistoryPoint], now: Int64) -> [StarHistoryPoint] {
        guard points.count > 2 else { return points }
        let hourlyCutoff = now - 3 * 86_400
        let dailyCutoff = now - 62 * 86_400
        var out: [StarHistoryPoint] = []
        for (i, point) in points.enumerated() {
            let prev = i > 0 ? points[i - 1] : nil
            let next = i + 1 < points.count ? points[i + 1] : nil
            let keep: Bool
            if point.at >= hourlyCutoff {
                keep = true
            } else if point.at >= dailyCutoff {
                let day = point.at / 86_400
                keep = prev.map { $0.at / 86_400 != day } ?? true
                    || next.map { $0.at / 86_400 != day } ?? true
            } else {
                let month = point.at / (30 * 86_400)
                keep = prev.map { $0.at / (30 * 86_400) != month } ?? true
            }
            if keep {
                out.append(point)
            }
        }
        return out
    }
}

// MARK: - GitHub CLI Service

enum GitHubService {
    static let ghExecutablePath: String? = {
        let candidates = [
            "/opt/homebrew/bin/gh",
            "/usr/local/bin/gh",
            "/usr/bin/gh",
            "/run/current-system/sw/bin/gh"
        ]
        for path in candidates where FileManager.default.isExecutableFile(atPath: path) {
            return path
        }
        if let pathEnv = ProcessInfo.processInfo.environment["PATH"] {
            for dir in pathEnv.split(separator: ":") {
                let candidate = String(dir) + "/gh"
                if FileManager.default.isExecutableFile(atPath: candidate) {
                    return candidate
                }
            }
        }
        return nil
    }()

    static func runGh(
        _ args: [String],
        in cwd: String? = nil,
        timeoutSeconds: TimeInterval = 30
    ) async -> CommandResult {
        await Task.detached(priority: .userInitiated) {
            let cmdDisplay = "gh " + args.joined(separator: " ")
            guard let ghPath = ghExecutablePath else {
                return CommandResult(
                    exitCode: -1,
                    stdout: "",
                    stderr: "gh: command not found",
                    commandString: cmdDisplay
                )
            }

            let process = Process()
            process.executableURL = URL(fileURLWithPath: ghPath)
            process.arguments = args
            if let cwd {
                process.currentDirectoryURL = URL(fileURLWithPath: cwd)
            }

            var env = ProcessInfo.processInfo.environment
            env["GH_PROMPT_DISABLED"] = "1"
            env["NO_COLOR"] = "1"
            env["LC_ALL"] = "en_US.UTF-8"
            let currentPath = env["PATH"] ?? "/usr/bin:/bin:/usr/sbin:/sbin"
            env["PATH"] = "/opt/homebrew/bin:/usr/local/bin:" + currentPath
            process.environment = env

            let stdoutPipe = Pipe()
            let stderrPipe = Pipe()
            process.standardOutput = stdoutPipe
            process.standardError = stderrPipe

            do {
                try process.run()
            } catch {
                return CommandResult(
                    exitCode: -1,
                    stdout: "",
                    stderr: "Failed to launch gh: \(error.localizedDescription)",
                    commandString: cmdDisplay
                )
            }

            let timeoutItem = DispatchWorkItem {
                if process.isRunning {
                    process.terminate()
                }
            }
            DispatchQueue.global().asyncAfter(deadline: .now() + timeoutSeconds, execute: timeoutItem)

            let outData = stdoutPipe.fileHandleForReading.readDataToEndOfFile()
            let errData = stderrPipe.fileHandleForReading.readDataToEndOfFile()

            process.waitUntilExit()
            timeoutItem.cancel()

            let stdout = String(data: outData, encoding: .utf8) ?? ""
            let stderr = String(data: errData, encoding: .utf8) ?? ""

            return CommandResult(
                exitCode: process.terminationStatus,
                stdout: stdout,
                stderr: stderr,
                commandString: cmdDisplay
            )
        }.value
    }

    // MARK: - CLI Auth & Forking

    static func cliStatus() async -> GhCliStatus {
        guard ghExecutablePath != nil else {
            return .notInstalled
        }

        let versionRes = await runGh(["--version"], timeoutSeconds: 8)
        guard versionRes.succeeded else {
            return .notInstalled
        }

        let authRes = await runGh(["auth", "status", "--hostname", "github.com"], timeoutSeconds: 10)
        if !authRes.succeeded {
            let msg = authRes.combinedOutput
                .split(whereSeparator: \.isNewline)
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .first { !$0.isEmpty }
                ?? "Not signed in to github.com"
            return .notAuthenticated(msg)
        }

        let userRes = await runGh(["api", "user", "--jq", ".login"], timeoutSeconds: 10)
        var user = userRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
        if !userRes.succeeded || user.isEmpty {
            user = parseUserFromAuthOutput(authRes.combinedOutput) ?? "your account"
        }

        let orgsRes = await runGh(["api", "user/orgs", "--jq", ".[].login"], timeoutSeconds: 10)
        let orgs: [String]
        if orgsRes.succeeded {
            orgs = orgsRes.stdout
                .split(whereSeparator: \.isNewline)
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
        } else {
            orgs = []
        }

        return .ready(user: user, orgs: orgs)
    }

    private static func parseUserFromAuthOutput(_ text: String) -> String? {
        for line in text.split(whereSeparator: \.isNewline) {
            if let range = line.range(of: "account ") {
                let after = line[range.upperBound...]
                let token = after.split { $0.isWhitespace || $0 == "(" }.first
                if let token, !token.isEmpty {
                    return String(token)
                }
            }
        }
        return nil
    }

    static func planRemotes(repoPath: String, primaryRemote: String?) async -> ForkRemotePlan {
        let primary = primaryRemote ?? "origin"
        let hasUpstream = (await GitService.runGit(["remote", "get-url", "upstream"], in: repoPath, timeoutSeconds: 5)).succeeded
        let hasFork = (await GitService.runGit(["remote", "get-url", "fork"], in: repoPath, timeoutSeconds: 5)).succeeded

        if primary == "origin" && !hasUpstream {
            return ForkRemotePlan(
                forkRemote: "origin",
                originalRemote: "upstream",
                renamesOrigin: true
            )
        } else {
            let forkRemote = hasFork ? "myfork" : "fork"
            return ForkRemotePlan(
                forkRemote: forkRemote,
                originalRemote: primary,
                renamesOrigin: false
            )
        }
    }

    static func forkRepository(
        _ repo: GitRepo,
        owner: String?
    ) async -> (repo: GitRepo, logEntry: ActivityLogEntry) {
        let path = repo.path
        let plan = await planRemotes(repoPath: path, primaryRemote: repo.primaryRemoteName)

        var args = [
            "repo",
            "fork",
            "--remote",
            "--remote-name",
            plan.forkRemote
        ]
        if let org = owner?.trimmingCharacters(in: .whitespacesAndNewlines), !org.isEmpty {
            args.append(contentsOf: ["--org", org])
        }

        let forkRes = await runGh(args, in: path, timeoutSeconds: 60)
        var transcript = "[$ \(forkRes.commandString)]\n\(forkRes.combinedOutput)"

        if !forkRes.succeeded {
            var inspected = await GitService.inspectLocalRepository(repo, markUnknownIfCached: false)
            inspected.lastCommandOutput = transcript
            let errText = forkRes.combinedOutput.trimmingCharacters(in: .whitespacesAndNewlines)
            let shortErr = errText.split(whereSeparator: \.isNewline).first.map(String.init) ?? "gh repo fork failed"
            inspected.syncState = .error(shortErr)
            let log = ActivityLogEntry(
                timestamp: Date(),
                repoName: repo.name,
                repoPath: path,
                action: "Fork Repository",
                command: forkRes.commandString,
                succeeded: false,
                summary: shortErr,
                output: transcript
            )
            return (inspected, log)
        }

        let cfgRes = await GitService.runGit(["config", "remote.pushDefault", plan.forkRemote], in: path, timeoutSeconds: 5)
        if !cfgRes.combinedOutput.isEmpty {
            transcript += "\n[$ \(cfgRes.commandString)]\n\(cfgRes.combinedOutput)"
        } else {
            transcript += "\n[$ \(cfgRes.commandString)]"
        }

        let (checkedRepo, checkLog) = await GitService.checkRemoteStatus(for: repo)
        var finalRepo = checkedRepo
        finalRepo.lastCommandOutput = "\(transcript)\n\n\(checkLog.output)"

        let forkURLDisplay = finalRepo.formattedRemoteHost ?? plan.forkRemote
        let summary = plan.renamesOrigin
            ? "Forked to \(forkURLDisplay) (origin → fork, upstream → original)"
            : "Forked to \(forkURLDisplay) (remote '\(plan.forkRemote)')"

        let log = ActivityLogEntry(
            timestamp: Date(),
            repoName: repo.name,
            repoPath: path,
            action: "Fork Repository",
            command: forkRes.commandString,
            succeeded: true,
            summary: summary,
            output: finalRepo.lastCommandOutput
        )
        return (finalRepo, log)
    }

    static func openSignInTerminal() {
        let script = """
        tell application "Terminal"
            activate
            do script "gh auth login --web -h github.com"
        end tell
        """
        if let appleScript = NSAppleScript(source: script) {
            var errorDict: NSDictionary? = nil
            appleScript.executeAndReturnError(&errorDict)
        }
    }

    // MARK: - GitHub Hub GraphQL & REST Operations

    static func searchQuery(for section: HubSection, involvement: Involvement) -> String {
        switch section {
        case .pullRequests:
            return "is:open is:pr \(involvement.qualifier) archived:false sort:updated-desc"
        case .issues:
            return "is:open is:issue \(involvement.qualifier) archived:false sort:updated-desc"
        case .reviewRequests:
            return "is:open is:pr review-requested:@me archived:false sort:updated-desc"
        case .starred, .explore:
            return ""
        }
    }

    static func fetchCounts() async -> Result<HubCounts, Error> {
        let res = await runGh(["api", "graphql", "-f", "query=\(Self.countsQuery)"], timeoutSeconds: 20)
        guard res.succeeded else {
            return .failure(makeError(res.combinedOutput))
        }
        guard let data = res.stdout.data(using: .utf8),
              let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let d = root["data"] as? [String: Any] else {
            return .failure(makeError("Invalid GraphQL response for counts"))
        }

        let pulls = ((d["pulls"] as? [String: Any])?["issueCount"] as? Int) ?? 0
        let issues = ((d["issues"] as? [String: Any])?["issueCount"] as? Int) ?? 0
        let reviews = ((d["reviews"] as? [String: Any])?["issueCount"] as? Int) ?? 0
        let starred = (((d["viewer"] as? [String: Any])?["starredRepositories"] as? [String: Any])?["totalCount"] as? Int) ?? 0

        return .success(HubCounts(pulls: pulls, issues: issues, reviews: reviews, starred: starred))
    }

    static func fetchItems(search: String) async -> Result<(Int, [HubItem]), Error> {
        let res = await runGh(
            ["api", "graphql", "-f", "query=\(Self.searchItemsQuery)", "-f", "q=\(search)"],
            timeoutSeconds: 25
        )
        guard res.succeeded else {
            return .failure(makeError(res.combinedOutput))
        }
        guard let data = res.stdout.data(using: .utf8),
              let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
            return .failure(makeError("Invalid JSON response"))
        }
        if let errMsg = graphQLError(from: root) {
            return .failure(makeError(errMsg))
        }
        guard let searchObj = (root["data"] as? [String: Any])?["search"] as? [String: Any] else {
            return .failure(makeError("Missing data.search in response"))
        }
        let total = (searchObj["issueCount"] as? Int) ?? 0
        let nodes = (searchObj["nodes"] as? [[String: Any]]) ?? []
        let items = nodes.compactMap(parseItem)
        return .success((total, items))
    }

    static func fetchStarred() async -> Result<(Int, [HubRepo]), Error> {
        let res = await runGh(["api", "graphql", "-f", "query=\(Self.starredQuery)"], timeoutSeconds: 25)
        guard res.succeeded else {
            return .failure(makeError(res.combinedOutput))
        }
        guard let data = res.stdout.data(using: .utf8),
              let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
            return .failure(makeError("Invalid JSON response"))
        }
        if let errMsg = graphQLError(from: root) {
            return .failure(makeError(errMsg))
        }
        guard let starredObj = ((root["data"] as? [String: Any])?["viewer"] as? [String: Any])?["starredRepositories"] as? [String: Any] else {
            return .failure(makeError("Missing viewer.starredRepositories in response"))
        }
        let total = (starredObj["totalCount"] as? Int) ?? 0
        let nodes = (starredObj["nodes"] as? [[String: Any]]) ?? []
        let repos = nodes.compactMap(parseRepo)
        return .success((total, repos))
    }

    static func fetchTrending(search: String, first: Int = 50) async -> Result<[HubRepo], Error> {
        let query = Self.searchReposQuery(first: first)
        let res = await runGh(
            ["api", "graphql", "-f", "query=\(query)", "-f", "q=\(search)"],
            timeoutSeconds: 30
        )
        guard res.succeeded else {
            return .failure(makeError(res.combinedOutput))
        }
        guard let data = res.stdout.data(using: .utf8),
              let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
            return .failure(makeError("Invalid JSON response"))
        }
        if let errMsg = graphQLError(from: root) {
            return .failure(makeError(errMsg))
        }
        let nodes = (((root["data"] as? [String: Any])?["search"] as? [String: Any])?["nodes"] as? [[String: Any]]) ?? []
        let repos = nodes.compactMap(parseRepo)
        return .success(repos)
    }

    /// Looks up many repositories by `owner/name` in parallel batches.
    /// Renamed, deleted, or private repositories are skipped instead of failing the batch.
    static func lookupRepos(slugs: [String], fields: String = Self.repoGraphQLFields, perQuery: Int = 25) async -> [HubRepo] {
        let valid: [(String, String)] = slugs.compactMap { raw in
            guard let clean = repoSlug(from: "https://github.com/\(raw)"),
                  clean.caseInsensitiveCompare(raw) == .orderedSame else {
                return nil
            }
            let parts = clean.split(separator: "/", maxSplits: 1)
            guard parts.count == 2 else { return nil }
            return (String(parts[0]), String(parts[1]))
        }
        guard !valid.isEmpty else { return [] }

        var batches: [[(String, String)]] = []
        var idx = 0
        while idx < valid.count {
            let end = min(idx + perQuery, valid.count)
            batches.append(Array(valid[idx..<end]))
            idx = end
        }

        return await withTaskGroup(of: [HubRepo].self) { group in
            for batch in batches {
                group.addTask {
                    var body = ""
                    for (i, pair) in batch.enumerated() {
                        body += "r\(i): repository(owner: \"\(pair.0)\", name: \"\(pair.1)\") { \(fields) }\n"
                    }
                    let q = "query { \(body) }"
                    let res = await runGh(["api", "graphql", "-f", "query=\(q)"], timeoutSeconds: 45)
                    guard let data = res.stdout.data(using: .utf8),
                          let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
                          let dataObj = root["data"] as? [String: Any] else {
                        return []
                    }
                    var found: [HubRepo] = []
                    for i in 0..<batch.count {
                        if let node = dataObj["r\(i)"] as? [String: Any],
                           let repo = parseRepo(node) {
                            found.append(repo)
                        }
                    }
                    return found
                }
            }
            var all: [HubRepo] = []
            for await part in group {
                all.append(contentsOf: part)
            }
            return all
        }
    }

    /// Explore's ranking for one period: repositories created in it, older ones GitDaddy has recorded
    /// star counts for, and ones posted to Hacker News or Mastodon in the period.
    static func fetchExplore(
        span: TrendingSpan,
        offset: Int,
        language: String?
    ) async -> Result<([HubRepo], Int64?), Error> {
        let now = Date()
        let window = span.window(offset: offset, now: now)
        let until: Int64? = window.current ? nil : window.end
        let searchStr = span.query(language: language, offset: offset, now: now)

        async let searchRes = fetchTrending(search: searchStr, first: 50)
        async let hnMentions = fetchHackerNews(since: window.start, until: until, limit: 1000)
        async let mastoMentions: [RepoMention] = window.current ? (try? await fetchMastodon().get()) ?? [] : []

        let (searchOutcome, hnOutcome, mastoList) = await (searchRes, hnMentions, mastoMentions)
        var repos: [HubRepo]
        switch searchOutcome {
        case .success(let list):
            repos = list
        case .failure(let err):
            return .failure(err)
        }

        var mentions = (try? hnOutcome.get()) ?? []
        mentions.append(contentsOf: mastoList)

        var mentioned: [String: [MentionPlatform]] = [:]
        for mention in mentions {
            let key = mention.slug.lowercased()
            var list = mentioned[key] ?? []
            if !list.contains(mention.platform) {
                list.append(mention.platform)
            }
            mentioned[key] = list
        }

        var seen = Set(repos.map { $0.nameWithOwner.lowercased() })
        let risers = await StarHistoryStore.shared.risers(window: window, language: language, limit: 60)
        let sortedMentions = mentions.sorted { $0.points > $1.points }
        var wanted: [String] = []
        for slug in risers + sortedMentions.prefix(40).map(\.slug) {
            if seen.insert(slug.lowercased()).inserted {
                wanted.append(slug)
            }
        }

        if !wanted.isEmpty {
            var extra = await lookupRepos(slugs: wanted, fields: Self.repoGraphQLFields, perQuery: 25)
            if let lang = language?.trimmingCharacters(in: .whitespacesAndNewlines), !lang.isEmpty {
                extra.removeAll { repo in
                    guard let rLang = repo.language else { return true }
                    return rLang.caseInsensitiveCompare(lang) != .orderedSame
                }
            }
            repos.append(contentsOf: extra)
        }

        let stamp: Int64? = window.current ? Int64(now.timeIntervalSince1970) : nil
        let (ranked, firstRecorded) = await StarHistoryStore.shared.computeMomenta(
            for: repos,
            window: window,
            mentioned: mentioned,
            recordNow: stamp
        )
        return .success((Array(ranked.prefix(50)), firstRecorded))
    }

    /// Live feed: recent Hacker News and Mastodon posts linking a GitHub repository.
    static func fetchLive() async -> Result<([RepoMention], [String: HubRepo]), Error> {
        let now = Int64(Date().timeIntervalSince1970)
        let twoDaysAgo = now - 2 * 86_400

        async let hnTask = fetchHackerNews(since: twoDaysAgo, until: nil, limit: 100)
        async let mastoTask = fetchMastodon()

        let (hnRes, mastoRes) = await (hnTask, mastoTask)
        if case .failure(let hnErr) = hnRes, case .failure = mastoRes {
            return .failure(makeError("Couldn't reach Hacker News or Mastodon: \(hnErr.localizedDescription)"))
        }

        let combined = ((try? hnRes.get()) ?? []) + ((try? mastoRes.get()) ?? [])
        var mentions = mergeMentions(combined, limit: 120)

        var slugs: [String] = []
        for m in mentions {
            if !slugs.contains(where: { $0.caseInsensitiveCompare(m.slug) == .orderedSame }) {
                slugs.append(m.slug)
            }
        }
        if slugs.count > 80 {
            slugs = Array(slugs.prefix(80))
        }

        let fetchedRepos = await lookupRepos(slugs: slugs, fields: Self.repoGraphQLFields, perQuery: 25)
        var repoMap: [String: HubRepo] = [:]
        for repo in fetchedRepos {
            repoMap[repo.nameWithOwner.lowercased()] = repo
        }

        if !repoMap.isEmpty {
            mentions.removeAll { repoMap[$0.slug.lowercased()] == nil }
            await StarHistoryStore.shared.recordBatch(repos: Array(repoMap.values), at: now)
            await StarHistoryStore.shared.save()
        }

        return .success((mentions, repoMap))
    }

    /// Hourly background star tracker.
    static func trackStarHistory(extraSlugs: [String]) async -> Int64? {
        let now = Int64(Date().timeIntervalSince1970)
        var found: [HubRepo] = []
        let seedDue = await StarHistoryStore.shared.needsSeeding(now: now)
        if seedDue {
            let cal = Calendar(identifier: .gregorian)
            let today = Date()
            let weekAgo = TrendingSpan.formatUTCDate(cal.date(byAdding: .day, value: -7, to: today) ?? today)
            let monthAgo = TrendingSpan.formatUTCDate(cal.date(byAdding: .day, value: -30, to: today) ?? today)
            for query in [
                "stars:>1000 pushed:>=\(weekAgo) sort:stars",
                "created:>=\(monthAgo) sort:stars"
            ] {
                if case .success(let repos) = await fetchTrending(search: query, first: 100) {
                    found.append(contentsOf: repos)
                }
            }
        }

        var seen = Set(found.map { $0.nameWithOwner.lowercased() })
        let tracked = await StarHistoryStore.shared.trackedSlugs(limit: 600)
        var wanted: [String] = []
        for slug in tracked + extraSlugs {
            if seen.insert(slug.lowercased()).inserted {
                wanted.append(slug)
            }
        }
        if !wanted.isEmpty {
            let lookedUp = await lookupRepos(slugs: wanted, fields: Self.countGraphQLFields, perQuery: 60)
            found.append(contentsOf: lookedUp)
        }

        await StarHistoryStore.shared.recordBatch(repos: found, at: now)
        if seedDue {
            await StarHistoryStore.shared.markSeeded(now: now)
        }
        await StarHistoryStore.shared.compact(now: now)
        await StarHistoryStore.shared.save()
        return await StarHistoryStore.shared.firstRecorded()
    }

    // MARK: - Public Mention Feeds (Hacker News & Mastodon)

    private static let notOwners: Set<String> = [
        "about", "account", "ACCOUNTS", "advisories", "apps", "blog", "cli", "codespaces",
        "collections", "contact", "copilot", "customer-stories", "dashboard", "enterprise",
        "events", "explore", "features", "gist", "gists", "github-copilot", "gitignore",
        "issues", "join", "login", "logout", "marketplace", "Mine", "mobile", "new",
        "notifications", "orgs", "organizations", "pricing", "pulls", "readme", "releases",
        "search", "security", " session", "settings", "signup", "site", "sponsors", "stars",
        " status", "support", "team", "terms", "topics", "trending", "users", "watching"
    ]

    static func repoSlug(from url: String) -> String? {
        var rest = url.trimmingCharacters(in: .whitespacesAndNewlines)
        if rest.hasPrefix("https://") {
            rest.removeFirst("https://".count)
        } else if rest.hasPrefix("http://") {
            rest.removeFirst("http://".count)
        } else {
            return nil
        }
        if rest.hasPrefix("www.") {
            rest.removeFirst("www.".count)
        }
        guard rest.hasPrefix("github.com/") else { return nil }
        rest.removeFirst("github.com/".count)
        let path = rest.split(whereSeparator: { $0 == "?" || $0 == "#" || $0 == "\"" || $0 == "'" || $0 == ")" || $0 == ">" || $0.isWhitespace }).first.map(String.init) ?? ""
        let parts = path.split(separator: "/").filter { !$0.isEmpty }
        guard parts.count >= 2 else { return nil }
        let owner = String(parts[0])
        var repo = String(parts[1])
        if repo.hasSuffix(".git") {
            repo.removeLast(4)
        }
        guard isValidSlugPart(owner), isValidSlugPart(repo) else { return nil }
        let lowerOwner = owner.lowercased()
        if notOwners.contains(owner) || notOwners.contains(lowerOwner) {
            return nil
        }
        return "\(owner)/\(repo)"
    }

    private static func isValidSlugPart(_ part: String) -> Bool {
        guard !part.isEmpty, part.count <= 100, part != ".", part != ".." else { return false }
        return part.unicodeScalars.allSatisfy { scalar in
            CharacterSet.alphanumerics.contains(scalar) || scalar == "-" || scalar == "_" || scalar == "."
        }
    }

    private static func firstRepoSlug(in text: String) -> String? {
        var searchRange = text.startIndex..<text.endIndex
        while let range = text.range(of: "github.com/", options: [.caseInsensitive], range: searchRange) {
            let tail = String(text[range.lowerBound...])
            if let slug = repoSlug(from: "https://\(tail)") {
                return slug
            }
            searchRange = range.upperBound..<text.endIndex
        }
        return nil
    }

    static func fetchHackerNews(since: Int64, until: Int64?, limit: Int) async -> Result<[RepoMention], Error> {
        let perPage = min(max(1, limit), 1000)
        let filter: String
        if let until {
            filter = "created_at_i%3E%3D\(since)%2Ccreated_at_i%3C\(until)"
        } else {
            filter = "created_at_i%3E%3D\(since)"
        }
        let urlStr = "https://hn.algolia.com/api/v1/search_by_date?query=github.com&restrictSearchableAttributes=url&tags=story&numericFilters=\(filter)&hitsPerPage=\(perPage)"
        guard let url = URL(string: urlStr) else {
            return .failure(makeError("Invalid Hacker News URL"))
        }
        var req = URLRequest(url: url, timeoutInterval: 15)
        req.setValue("GitDaddy/0.1", forHTTPHeaderField: "User-Agent")
        req.setValue("application/json", forHTTPHeaderField: "Accept")

        do {
            let (data, resp) = try await URLSession.shared.data(for: req)
            guard let http = resp as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
                return .failure(makeError("Hacker News returned an error"))
            }
            guard let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let hits = root["hits"] as? [[String: Any]] else {
                return .failure(makeError("Malformed Hacker News response"))
            }
            let mentions: [RepoMention] = hits.compactMap { hit in
                guard let hitURL = hit["url"] as? String,
                      let slug = repoSlug(from: hitURL),
                      let id = hit["objectID"] as? String else {
                    return nil
                }
                let title = (hit["title"] as? String) ?? slug
                let author = (hit["author"] as? String) ?? ""
                let postedAt = (hit["created_at"] as? String) ?? ""
                let (epoch, rel) = parseISORelative(postedAt)
                let points = max(0, (hit["points"] as? Int) ?? 0)
                let comments = max(0, (hit["num_comments"] as? Int) ?? 0)
                return RepoMention(
                    platform: .hackerNews,
                    id: "hn:\(id)",
                    slug: slug,
                    title: title,
                    text: "",
                    author: author,
                    link: "https://news.ycombinator.com/item?id=\(id)",
                    postedAt: postedAt,
                    postedRelative: rel,
                    postedEpoch: epoch,
                    points: points,
                    comments: comments
                )
            }
            return .success(mentions)
        } catch {
            return .failure(error)
        }
    }

    static func fetchMastodon() async -> Result<[RepoMention], Error> {
        let instances = ["mastodon.social", "fosstodon.org"]
        var all: [RepoMention] = []
        var lastErr: Error? = nil

        for host in instances {
            let urlStr = "https://\(host)/api/v1/timelines/tag/github?limit=40"
            guard let url = URL(string: urlStr) else { continue }
            var req = URLRequest(url: url, timeoutInterval: 15)
            req.setValue("GitDaddy/0.1", forHTTPHeaderField: "User-Agent")
            req.setValue("application/json", forHTTPHeaderField: "Accept")
            do {
                let (data, resp) = try await URLSession.shared.data(for: req)
                guard let http = resp as? HTTPURLResponse, (200..<300).contains(http.statusCode),
                      let posts = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
                    continue
                }
                for post in posts {
                    if let m = parseMastodonPost(post) {
                        all.append(m)
                    }
                }
            } catch {
                lastErr = error
            }
        }
        if all.isEmpty, let err = lastErr {
            return .failure(err)
        }
        return .success(all)
    }

    private static func parseMastodonPost(_ post: [String: Any]) -> RepoMention? {
        let html = (post["content"] as? String) ?? ""
        let cardURL = ((post["card"] as? [String: Any])?["url"] as? String) ?? ""
        guard let slug = repoSlug(from: cardURL) ?? firstRepoSlug(in: html) else {
            return nil
        }
        let link = (post["url"] as? String) ?? (post["uri"] as? String) ?? ""
        guard !link.isEmpty else { return nil }
        let plain = stripHTML(html)
        let title = plain
            .split(whereSeparator: \.isNewline)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .first { !$0.isEmpty }
            ?? slug
        let acct = ((post["account"] as? [String: Any])?["acct"] as? String) ?? ""
        let author = acct.isEmpty ? "" : "@\(acct)"
        let postedAt = (post["created_at"] as? String) ?? ""
        let (epoch, rel) = parseISORelative(postedAt)
        let points = (post["favourites_count"] as? Int) ?? 0
        let comments = (post["replies_count"] as? Int) ?? 0
        return RepoMention(
            platform: .mastodon,
            id: "mastodon:\(link)",
            slug: slug,
            title: title,
            text: plain,
            author: author,
            link: link,
            postedAt: postedAt,
            postedRelative: rel,
            postedEpoch: epoch,
            points: points,
            comments: comments
        )
    }

    static func stripHTML(_ html: String) -> String {
        let blockTags = ["</p>", "<br>", "<br/>", "<br />", "</li>"]
        var text = html
        for tag in blockTags {
            text = text.replacingOccurrences(of: tag, with: "\n", options: .caseInsensitive)
        }
        var out = ""
        var inTag = false
        for ch in text {
            if ch == "<" {
                inTag = true
            } else if ch == ">" {
                inTag = false
            } else if !inTag {
                out.append(ch)
            }
        }
        return out
            .replacingOccurrences(of: "&amp;", with: "&")
            .replacingOccurrences(of: "&lt;", with: "<")
            .replacingOccurrences(of: "&gt;", with: ">")
            .replacingOccurrences(of: "&quot;", with: "\"")
            .replacingOccurrences(of: "&#39;", with: "'")
            .replacingOccurrences(of: "&apos;", with: "'")
            .replacingOccurrences(of: "&nbsp;", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    static func mergeMentions(_ items: [RepoMention], limit: Int) -> [RepoMention] {
        var seen = Set<String>()
        var out = items.filter { seen.insert($0.id).inserted }
        out.sort { $0.postedEpoch > $1.postedEpoch }
        if out.count > limit {
            out.removeSubrange(limit..<out.count)
        }
        return out
    }

    static func fetchReadme(slug: String) async -> Result<String, Error> {
        let endpoint = "repos/\(slug)/readme"
        let res = await runGh(
            ["api", "-H", "Accept: application/vnd.github.raw", endpoint],
            timeoutSeconds: 20
        )
        if res.succeeded {
            let text = String(res.stdout.prefix(49_152))
            return .success(text)
        }
        let combined = res.combinedOutput
        if combined.contains("404") || combined.contains("Not Found") {
            return .success("")
        }
        return .failure(makeError(combined))
    }

    static func setStar(slug: String, starred: Bool) async -> Result<Void, Error> {
        let method = starred ? "PUT" : "DELETE"
        let endpoint = "user/starred/\(slug)"
        let res = await runGh(["api", "-X", method, endpoint], timeoutSeconds: 15)
        if res.succeeded {
            return .success(())
        }
        return .failure(makeError(res.combinedOutput))
    }

    // MARK: - GraphQL Parsers

    private static func graphQLError(from root: [String: Any]) -> String? {
        guard let errors = root["errors"] as? [[String: Any]],
              let first = errors.first,
              let message = first["message"] as? String else {
            return nil
        }
        return message
    }

    private static func parseItem(_ v: [String: Any]) -> HubItem? {
        guard let kind = v["__typename"] as? String,
              kind == "PullRequest" || kind == "Issue" else {
            return nil
        }
        let isPR = (kind == "PullRequest")
        guard let number = v["number"] as? Int,
              let title = v["title"] as? String,
              let url = v["url"] as? String else {
            return nil
        }
        let repoSlug = ((v["repository"] as? [String: Any])?["nameWithOwner"] as? String) ?? ""
        let author = ((v["author"] as? [String: Any])?["login"] as? String) ?? "ghost"
        let updatedISO = (v["updatedAt"] as? String) ?? ""
        let (updatedEpoch, updatedRelative) = parseISORelative(updatedISO)
        let comments = ((v["comments"] as? [String: Any])?["totalCount"] as? Int) ?? 0
        let isDraft = (v["isDraft"] as? Bool) ?? false
        let headRef = (v["headRefName"] as? String) ?? ""
        let baseRef = (v["baseRefName"] as? String) ?? ""
        let additions = (v["additions"] as? Int) ?? 0
        let deletions = (v["deletions"] as? Int) ?? 0

        let reviewDecision: ReviewDecision? = {
            guard let raw = v["reviewDecision"] as? String else { return nil }
            switch raw {
            case "APPROVED": return .approved
            case "CHANGES_REQUESTED": return .changesRequested
            case "REVIEW_REQUIRED": return .reviewRequired
            default: return nil
            }
        }()

        let ciState: CiState? = {
            guard let commits = (v["commits"] as? [String: Any])?["nodes"] as? [[String: Any]],
                  let first = commits.first,
                  let commit = first["commit"] as? [String: Any],
                  let rollup = commit["statusCheckRollup"] as? [String: Any],
                  let state = rollup["state"] as? String else {
                return nil
            }
            switch state {
            case "SUCCESS": return .success
            case "PENDING", "EXPECTED": return .pending
            case "FAILURE", "ERROR": return .failure
            default: return nil
            }
        }()

        let labelNodes = ((v["labels"] as? [String: Any])?["nodes"] as? [[String: Any]]) ?? []
        let labels: [HubLabel] = labelNodes.compactMap { l in
            guard let name = l["name"] as? String, !name.isEmpty else { return nil }
            let hex = (l["color"] as? String) ?? "888888"
            return HubLabel(name: name, colorHex: hex)
        }

        let body = (v["body"] as? String) ?? ""

        return HubItem(
            number: number,
            title: title,
            url: url,
            repoSlug: repoSlug,
            author: author,
            updatedRelative: updatedRelative,
            updatedEpoch: updatedEpoch,
            comments: comments,
            isPR: isPR,
            isDraft: isDraft,
            headRef: headRef,
            baseRef: baseRef,
            additions: additions,
            deletions: deletions,
            reviewDecision: reviewDecision,
            ciState: ciState,
            labels: labels,
            body: body
        )
    }

    private static func parseRepo(_ v: [String: Any]) -> HubRepo? {
        guard let nameWithOwner = v["nameWithOwner"] as? String, !nameWithOwner.isEmpty else {
            return nil
        }
        let parts = nameWithOwner.split(separator: "/", maxSplits: 1)
        let fallbackOwner = parts.first.map(String.init) ?? ""
        let fallbackName = parts.count > 1 ? String(parts[1]) : nameWithOwner
        let name = (v["name"] as? String) ?? fallbackName
        let owner = ((v["owner"] as? [String: Any])?["login"] as? String) ?? fallbackOwner
        let description = (v["description"] as? String) ?? ""
        let url = (v["url"] as? String) ?? "https://github.com/\(nameWithOwner)"
        let stars = (v["stargazerCount"] as? Int) ?? 0
        let forks = (v["forkCount"] as? Int) ?? 0
        let langObj = v["primaryLanguage"] as? [String: Any]
        let language = langObj?["name"] as? String
        let languageColorHex = langObj?["color"] as? String
        let updatedISO = (v["pushedAt"] as? String) ?? (v["updatedAt"] as? String) ?? ""
        let (updatedEpoch, updatedRelative) = parseISORelative(updatedISO)
        let createdISO = (v["createdAt"] as? String) ?? ""
        let (createdEpoch, _) = parseISORelative(createdISO)
        let isFork = (v["isFork"] as? Bool) ?? false
        let isArchived = (v["isArchived"] as? Bool) ?? false
        let isPrivate = (v["isPrivate"] as? Bool) ?? false
        let viewerHasStarred = (v["viewerHasStarred"] as? Bool) ?? false
        let defaultBranch = ((v["defaultBranchRef"] as? [String: Any])?["name"] as? String) ?? "main"

        let topicNodes = ((v["repositoryTopics"] as? [String: Any])?["nodes"] as? [[String: Any]]) ?? []
        let topics: [String] = topicNodes.compactMap { node in
            ((node["topic"] as? [String: Any])?["name"] as? String)
        }

        let openGraphImageURL = (v["openGraphImageUrl"] as? String).flatMap {
            $0.hasPrefix("https://") ? $0 : nil
        }

        return HubRepo(
            nameWithOwner: nameWithOwner,
            name: name,
            owner: owner,
            description: description,
            url: url,
            stars: stars,
            forks: forks,
            language: language,
            languageColorHex: languageColorHex,
            updatedRelative: updatedRelative,
            updatedEpoch: updatedEpoch,
            createdAt: createdISO,
            createdEpoch: createdEpoch,
            isFork: isFork,
            isArchived: isArchived,
            isPrivate: isPrivate,
            viewerHasStarred: viewerHasStarred,
            defaultBranch: defaultBranch,
            primaryTopics: topics,
            openGraphImageURL: openGraphImageURL
        )
    }

    // MARK: - Formatting & Utilities

    static func parseISORelative(_ iso: String) -> (Int64, String) {
        guard !iso.isEmpty else { return (0, "") }
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        var date = formatter.date(from: iso)
        if date == nil {
            formatter.formatOptions = [.withInternetDateTime]
            date = formatter.date(from: iso)
        }
        guard let parsedDate = date else { return (0, "") }
        let epoch = Int64(parsedDate.timeIntervalSince1970)
        let now = Int64(Date().timeIntervalSince1970)
        let diff = max(0, now - epoch)
        let rel: String
        if diff < 60 {
            rel = "just now"
        } else if diff < 3600 {
            rel = "\(diff / 60)m ago"
        } else if diff < 86_400 {
            rel = "\(diff / 3600)h ago"
        } else if diff < 86_400 * 30 {
            rel = "\(diff / 86_400)d ago"
        } else if diff < 86_400 * 365 {
            rel = "\(diff / (86_400 * 30))mo ago"
        } else {
            rel = "\(diff / (86_400 * 365))y ago"
        }
        return (epoch, rel)
    }

    static func formatCount(_ n: Int) -> String {
        if n >= 1_000_000 {
            return String(format: "%.1fM", Double(n) / 1_000_000.0)
        } else if n >= 10_000 {
            return String(format: "%.0fk", Double(n) / 1_000.0)
        } else if n >= 1_000 {
            return String(format: "%.1fk", Double(n) / 1_000.0)
        }
        return "\(n)"
    }

    static func formatGained(_ n: Int) -> String {
        "+\(formatCount(n))"
    }

    static func formatThousands(_ n: Int) -> String {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.groupingSeparator = ","
        return formatter.string(from: NSNumber(value: n)) ?? "\(n)"
    }

    static func colorFromHex(_ raw: String) -> Color? {
        let hex = raw.trimmingCharacters(in: CharacterSet(charactersIn: "#"))
        guard hex.count == 6, let val = UInt32(hex, radix: 16) else { return nil }
        let r = Double((val >> 16) & 0xFF) / 255.0
        let g = Double((val >> 8) & 0xFF) / 255.0
        let b = Double(val & 0xFF) / 255.0
        return Color(red: r, green: g, blue: b)
    }

    private static func makeError(_ message: String) -> Error {
        let line = message
            .split(whereSeparator: \.isNewline)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .first { !$0.isEmpty }
            ?? "GitHub request failed"
        return NSError(domain: "GitDaddy.GitHub", code: 1, userInfo: [NSLocalizedDescriptionKey: line])
    }

    // MARK: - GraphQL Query Strings

    static let repoGraphQLFields = """
    nameWithOwner
    name
    owner { login }
    description
    url
    stargazerCount
    forkCount
    pushedAt
    createdAt
    isFork
    isArchived
    isPrivate
    viewerHasStarred
    openGraphImageUrl
    defaultBranchRef { name }
    primaryLanguage { name color }
    repositoryTopics(first: 8) { nodes { topic { name } } }
    """

    static let countGraphQLFields = """
    nameWithOwner
    stargazerCount
    forkCount
    createdAt
    primaryLanguage { name color }
    """

    static let countsQuery = """
    query {
      viewer {
        starredRepositories { totalCount }
      }
      pulls: search(query: "is:open is:pr author:@me archived:false", type: ISSUE, first: 1) { issueCount }
      issues: search(query: "is:open is:issue assignee:@me archived:false", type: ISSUE, first: 1) { issueCount }
      reviews: search(query: "is:open is:pr review-requested:@me archived:false", type: ISSUE, first: 1) { issueCount }
    }
    """

    static let searchItemsQuery = """
    query($q: String!) {
      search(query: $q, type: ISSUE, first: 50) {
        issueCount
        nodes {
          __typename
          ... on PullRequest {
            number
            title
            url
            isDraft
            updatedAt
            headRefName
            baseRefName
            additions
            deletions
            reviewDecision
            body
            author { login }
            repository { nameWithOwner }
            comments { totalCount }
            labels(first: 6) { nodes { name color } }
            commits(last: 1) {
              nodes {
                commit {
                  statusCheckRollup { state }
                }
              }
            }
          }
          ... on Issue {
            number
            title
            url
            updatedAt
            body
            author { login }
            repository { nameWithOwner }
            comments { totalCount }
            labels(first: 6) { nodes { name color } }
          }
        }
      }
    }
    """

    static let starredQuery = """
    query {
      viewer {
        starredRepositories(first: 100, orderBy: {field: STARRED_AT, direction: DESC}) {
          totalCount
          nodes {
            \(repoGraphQLFields)
          }
        }
      }
    }
    """

    static func searchReposQuery(first: Int) -> String {
        """
        query($q: String!) {
          search(query: $q, type: REPOSITORY, first: \(first)) {
            nodes {
              ... on Repository {
                \(repoGraphQLFields)
              }
            }
          }
        }
        """
    }
}

// MARK: - Social Preview Image Disk Cache

actor SocialPreviewCache {
    static let shared = SocialPreviewCache()

    private var memoryCache: [String: NSImage] = [:]
    private var inFlight: Set<String> = []

    private var cacheDirectory: URL {
        let base = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory())
        return base.appendingPathComponent("GitDaddy/og", isDirectory: true)
    }

    private func diskPath(for slug: String) -> URL {
        let safe = slug
            .unicodeScalars
            .map { CharacterSet.alphanumerics.contains($0) || $0 == "-" || $0 == "_" || $0 == "." ? String($0) : "__" }
            .joined()
        return cacheDirectory.appendingPathComponent("\(safe).png")
    }

    func cachedImage(for slug: String) -> NSImage? {
        if let mem = memoryCache[slug] {
            return mem
        }
        let path = diskPath(for: slug)
        if FileManager.default.fileExists(atPath: path.path),
           let img = NSImage(contentsOf: path) {
            memoryCache[slug] = img
            return img
        }
        return nil
    }

    func fetchImage(for slug: String, urlString: String) async -> NSImage? {
        if let existing = cachedImage(for: slug) {
            return existing
        }
        guard !inFlight.contains(slug),
              let url = URL(string: urlString),
              urlString.hasPrefix("https://") else {
            return nil
        }
        inFlight.insert(slug)
        defer { inFlight.remove(slug) }

        var request = URLRequest(url: url, timeoutInterval: 15)
        request.setValue("GitDaddy/0.1", forHTTPHeaderField: "User-Agent")

        do {
            let (data, response) = try await URLSession.shared.data(for: request)
            guard let http = response as? HTTPURLResponse,
                  (200..<300).contains(http.statusCode),
                  data.count <= 8 * 1024 * 1024,
                  let image = NSImage(data: data) else {
                return nil
            }
            memoryCache[slug] = image
            let dest = diskPath(for: slug)
            try? FileManager.default.createDirectory(at: cacheDirectory, withIntermediateDirectories: true)
            try? data.write(to: dest, options: .atomic)
            return image
        } catch {
            return nil
        }
    }
}
