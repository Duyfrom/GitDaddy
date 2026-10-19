import AppKit
import Foundation
import SwiftUI

// MARK: - Fork Dialog State

enum ForkMode: String, CaseIterable, Identifiable {
    case github = "On GitHub"
    case local = "On this Mac"

    var id: String { rawValue }
}

enum ForkOwnerChoice: Equatable, Hashable {
    case personal
    case org(String)
}

struct ForkDialogTarget: Identifiable, Equatable {
    var id: String { repo.id }
    let repo: GitRepo
    var mode: ForkMode
    var ownerChoice: ForkOwnerChoice
    var remotePlan: ForkRemotePlan
    var localDestination: String
    var isRunning: Bool = false
    var errorMessage: String? = nil
}

// MARK: - Clone Dialog State

struct CloneDialogTarget: Identifiable, Equatable {
    var id: String { slug }
    let slug: String
    var destination: String
}

// MARK: - GitHub Hub Store

@MainActor
final class GitHubHubStore: ObservableObject {
    @Published var cliStatus: GhCliStatus = .checking
    @Published var counts: HubCounts = HubCounts()

    @Published var involvement: Involvement = .created
    @Published var trendingSpan: TrendingSpan = .week
    @Published var trendingOffset: Int = 0
    @Published var trendingLanguage: String? = nil
    @Published var exploreLive: Bool = false
    @Published var livePlatformFilter: MentionPlatform? = nil
    @Published var repoLayout: RepoLayout = .list
    @Published var searchText: String = ""

    @Published var items: [HubItem] = []
    private var loadedKey: (HubSection, Involvement)? = nil
    @Published var starred: [HubRepo] = []
    private var starredLoaded: Bool = false
    @Published var trending: [HubRepo] = []
    private var trendingKey: (TrendingSpan, Int, String?)? = nil

    @Published var liveMentions: [RepoMention] = []
    @Published var liveRepos: [String: HubRepo] = [:]
    private var liveFetchedAt: Date? = nil
    @Published var historySince: Int64? = nil

    @Published var loadingSection: HubSection? = nil
    private var countsLoading: Bool = false
    private var isTrackingStars: Bool = false
    private var lastTrackedAt: Date? = nil
    private var backgroundStarted: Bool = false
    @Published var lastError: String? = nil

    @Published var selectedItemURL: String? = nil
    @Published var selectedRepoSlug: String? = nil
    @Published var selectedMentionID: String? = nil

    @Published var readmes: [String: String?] = [:] // value nil = loading, value Some(str) = loaded
    @Published var starringSlug: String? = nil
    @Published var busyAction: String? = nil

    @Published var cloneDialog: CloneDialogTarget? = nil
    @Published var forkDialog: ForkDialogTarget? = nil

    // MARK: - Counts & Section Selection

    func count(for section: HubSection) -> Int {
        switch section {
        case .pullRequests: return counts.pulls
        case .issues: return counts.issues
        case .reviewRequests: return counts.reviews
        case .starred: return counts.starred
        case .explore: return exploreLive ? liveMentions.count : trending.count
        }
    }

    func ensureCountsLoaded() {
        Task {
            if self.historySince == nil {
                self.historySince = await StarHistoryStore.shared.firstRecorded()
            }
        }
        guard !countsLoading else { return }
        if counts.pulls == 0 && counts.issues == 0 && counts.reviews == 0 && counts.starred == 0 {
            refreshCounts()
        }
    }

    func startBackgroundTasks(store: RepoStore) {
        guard !backgroundStarted else { return }
        backgroundStarted = true

        // Hourly star-count tracker + 2-minute live feed refresh while Explore -> Live is on screen.
        Task { [weak self, weak store] in
            while !Task.isCancelled {
                guard let self, let store else { break }
                if case .ready = self.cliStatus {
                    let shouldTrack = self.lastTrackedAt.map { Date().timeIntervalSince($0) >= 3600 } ?? true
                    if !self.isTrackingStars && shouldTrack {
                        self.isTrackingStars = true
                        let localSlugs = store.repos.compactMap(\.githubSlug)
                        let starredSlugs = self.starred.map(\.nameWithOwner)
                        let extra = localSlugs + starredSlugs
                        let firstRec = await GitHubService.trackStarHistory(extraSlugs: extra)
                        self.historySince = firstRec
                        self.lastTrackedAt = Date()
                        self.isTrackingStars = false
                    }

                    if store.centerMode == .github(.explore) && self.exploreLive {
                        let liveAge = self.liveFetchedAt.map { Date().timeIntervalSince($0) } ?? 999
                        if liveAge >= 120 && self.loadingSection == nil {
                            self.fetchSection(.explore)
                        }
                    }
                }
                try? await Task.sleep(nanoseconds: 30_000_000_000) // check every 30s
            }
        }
    }

    func refreshCounts() {
        guard !countsLoading else { return }
        countsLoading = true
        Task {
            let status = await GitHubService.cliStatus()
            self.cliStatus = status
            if case .ready = status {
                if case .success(let fetched) = await GitHubService.fetchCounts() {
                    self.counts = fetched
                }
            }
            self.countsLoading = false
        }
    }

    func selectSection(_ section: HubSection, force: Bool = false) {
        lastError = nil
        let needsFetch: Bool
        switch section {
        case .pullRequests, .issues, .reviewRequests:
            let key = (section, effectiveInvolvement(for: section))
            needsFetch = force || loadedKey?.0 != key.0 || loadedKey?.1 != key.1
        case .starred:
            needsFetch = force || !starredLoaded
        case .explore:
            if exploreLive {
                let stale = liveFetchedAt.map { Date().timeIntervalSince($0) >= 120 } ?? true
                needsFetch = force || stale || liveMentions.isEmpty
            } else {
                let key = (trendingSpan, trendingOffset, trendingLanguage)
                needsFetch = force || trendingKey?.0 != key.0 || trendingKey?.1 != key.1 || trendingKey?.2 != key.2
            }
        }

        if needsFetch {
            fetchSection(section)
        } else {
            ensureValidSelection(in: section)
        }
    }

    func showExploreLive() {
        if !exploreLive {
            exploreLive = true
            selectSection(.explore, force: false)
        }
    }

    func selectExploreSpan(_ span: TrendingSpan) {
        if exploreLive || trendingSpan != span || trendingOffset != 0 {
            exploreLive = false
            trendingSpan = span
            trendingOffset = 0
            selectSection(.explore, force: false)
        }
    }

    func effectiveInvolvement(for section: HubSection) -> Involvement {
        section.usesInvolvement ? involvement : .created
    }

    func fetchSection(_ section: HubSection) {
        loadingSection = section
        lastError = nil
        let inv = effectiveInvolvement(for: section)
        let span = trendingSpan
        let offset = trendingOffset
        let lang = trendingLanguage
        let isLive = exploreLive

        Task {
            let status = await GitHubService.cliStatus()
            self.cliStatus = status
            guard case .ready = status else {
                self.loadingSection = nil
                return
            }

            switch section {
            case .pullRequests, .issues, .reviewRequests:
                let q = GitHubService.searchQuery(for: section, involvement: inv)
                switch await GitHubService.fetchItems(search: q) {
                case .success(let (total, list)):
                    if section == .pullRequests && inv == .created {
                        self.counts.pulls = total
                    } else if section == .issues && inv == .assigned {
                        self.counts.issues = total
                    } else if section == .reviewRequests {
                        self.counts.reviews = total
                    }
                    self.items = list
                    self.loadedKey = (section, inv)
                    self.ensureValidSelection(in: section)
                case .failure(let err):
                    self.lastError = err.localizedDescription
                }

            case .starred:
                switch await GitHubService.fetchStarred() {
                case .success(let (total, repos)):
                    self.counts.starred = total
                    self.starred = repos
                    self.starredLoaded = true
                    self.ensureValidSelection(in: section)
                case .failure(let err):
                    self.lastError = err.localizedDescription
                }

            case .explore:
                if isLive {
                    switch await GitHubService.fetchLive() {
                    case .success(let (mentions, repoMap)):
                        self.liveMentions = mentions
                        self.liveRepos = repoMap
                        self.liveFetchedAt = Date()
                        self.historySince = await StarHistoryStore.shared.firstRecorded()
                        self.ensureValidSelection(in: section)
                    case .failure(let err):
                        self.lastError = err.localizedDescription
                    }
                } else {
                    switch await GitHubService.fetchExplore(span: span, offset: offset, language: lang) {
                    case .success(let (repos, firstRec)):
                        self.trending = repos
                        self.trendingKey = (span, offset, lang)
                        self.historySince = firstRec
                        self.ensureValidSelection(in: section)
                    case .failure(let err):
                        self.lastError = err.localizedDescription
                    }
                }
            }

            self.loadingSection = nil
        }
    }

    func refreshActive(_ section: HubSection) {
        refreshCounts()
        selectSection(section, force: true)
    }

    // MARK: - Explore Caption

    func exploreCaptionText() -> String {
        if exploreLive {
            return "Posts that link a GitHub repository, from Hacker News (past two days) and Mastodon's #github, newest first. Refreshes every two minutes."
        }
        let period = trendingSpan.captionRangeText(offset: trendingOffset)
        let historyText: String
        if let since = historySince {
            let date = Date(timeIntervalSince1970: TimeInterval(since))
            let formatted = TrendingSpan.formatUTCDisplay(date, format: "MMM d, yyyy")
            historyText = "Older repositories are measured from the star counts GitDaddy has recorded since \(formatted)."
        } else {
            historyText = "New repositories count every star. Older ones join as GitDaddy records star counts, hourly while it runs."
        }
        return "Ranked by stars and forks gained \(period) (UTC), scored 8 × stars + 2 × forks as on Trendshift. \(historyText)"
    }

    // MARK: - Filtering & Selection

    func filteredItems() -> [HubItem] {
        let q = searchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !q.isEmpty else { return items }
        return items.filter { item in
            item.title.lowercased().contains(q)
                || item.repoSlug.lowercased().contains(q)
                || item.author.lowercased().contains(q)
                || item.headRef.lowercased().contains(q)
                || "#\(item.number)".contains(q)
                || item.labels.contains { $0.name.lowercased().contains(q) }
        }
    }

    func filteredRepos(for section: HubSection) -> [HubRepo] {
        let source = section == .explore ? trending : starred
        let q = searchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !q.isEmpty else { return source }
        return source.filter { repo in
            repo.nameWithOwner.lowercased().contains(q)
                || repo.description.lowercased().contains(q)
                || (repo.language?.lowercased().contains(q) ?? false)
                || repo.primaryTopics.contains { $0.lowercased().contains(q) }
        }
    }

    func filteredMentions() -> [RepoMention] {
        let q = searchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return liveMentions.filter { m in
            if let pf = livePlatformFilter, m.platform != pf {
                return false
            }
            guard !q.isEmpty else { return true }
            return m.slug.lowercased().contains(q)
                || m.title.lowercased().contains(q)
                || m.author.lowercased().contains(q)
                || m.text.lowercased().contains(q)
        }
    }

    var selectedItem: HubItem? {
        guard let url = selectedItemURL else { return nil }
        return items.first { $0.url == url }
    }

    var selectedMention: RepoMention? {
        guard let id = selectedMentionID else { return nil }
        return liveMentions.first { $0.id == id }
    }

    var selectedMentionRepo: HubRepo? {
        guard let mention = selectedMention else { return nil }
        return liveRepos[mention.slug.lowercased()]
    }

    func selectMention(_ mention: RepoMention) {
        selectedMentionID = mention.id
        if let repo = liveRepos[mention.slug.lowercased()] {
            ensureReadmeLoaded(for: repo.nameWithOwner)
        } else {
            ensureReadmeLoaded(for: mention.slug)
        }
    }

    func selectedRepo(for section: HubSection) -> HubRepo? {
        guard let slug = selectedRepoSlug else { return nil }
        let primary = section == .explore ? trending : starred
        let secondary = section == .explore ? starred : trending
        return primary.first { $0.nameWithOwner == slug }
            ?? secondary.first { $0.nameWithOwner == slug }
    }

    func selectRepo(_ slug: String) {
        selectedRepoSlug = slug
        ensureReadmeLoaded(for: slug)
    }

    func ensureReadmeLoaded(for slug: String) {
        if readmes.keys.contains(slug) { return }
        readmes[slug] = nil // mark loading
        Task {
            switch await GitHubService.fetchReadme(slug: slug) {
            case .success(let md):
                self.readmes[slug] = .some(md)
            case .failure:
                self.readmes[slug] = .some("")
            }
        }
    }

    private func ensureValidSelection(in section: HubSection) {
        if section == .explore && exploreLive {
            let list = filteredMentions()
            if selectedMentionID == nil || !list.contains(where: { $0.id == selectedMentionID }) {
                selectedMentionID = list.first?.id
            }
            if let mention = selectedMention {
                ensureReadmeLoaded(for: liveRepos[mention.slug.lowercased()]?.nameWithOwner ?? mention.slug)
            }
        } else if section.isRepoList {
            let list = section == .explore ? trending : starred
            if selectedRepoSlug == nil || !list.contains(where: { $0.nameWithOwner == selectedRepoSlug }) {
                selectedRepoSlug = list.first?.nameWithOwner
            }
            if let slug = selectedRepoSlug {
                ensureReadmeLoaded(for: slug)
            }
        } else {
            if selectedItemURL == nil || !items.contains(where: { $0.url == selectedItemURL }) {
                selectedItemURL = items.first?.url
            }
        }
    }

    // MARK: - Star / Unstar

    func toggleStar(for repo: HubRepo, store: RepoStore) {
        guard starringSlug == nil else { return }
        let slug = repo.nameWithOwner
        let targetStar = !repo.viewerHasStarred
        starringSlug = slug
        store.statusBanner = targetStar ? "Starring \(slug)…" : "Removing star from \(slug)…"

        Task {
            switch await GitHubService.setStar(slug: slug, starred: targetStar) {
            case .success:
                for idx in self.starred.indices where self.starred[idx].nameWithOwner.caseInsensitiveCompare(slug) == .orderedSame {
                    self.starred[idx].viewerHasStarred = targetStar
                }
                for idx in self.trending.indices where self.trending[idx].nameWithOwner.caseInsensitiveCompare(slug) == .orderedSame {
                    self.trending[idx].viewerHasStarred = targetStar
                }
                let lowerKey = slug.lowercased()
                if var liveRepo = self.liveRepos[lowerKey] {
                    liveRepo.viewerHasStarred = targetStar
                    self.liveRepos[lowerKey] = liveRepo
                }
                if targetStar {
                    if !self.starred.contains(where: { $0.nameWithOwner.caseInsensitiveCompare(slug) == .orderedSame }) {
                        var copy = repo
                        copy.viewerHasStarred = true
                        copy.momentum = nil
                        self.starred.insert(copy, at: 0)
                        self.counts.starred += 1
                    }
                } else {
                    let beforeCount = self.starred.count
                    self.starred.removeAll { $0.nameWithOwner.caseInsensitiveCompare(slug) == .orderedSame }
                    if self.starred.count < beforeCount && self.counts.starred > 0 {
                        self.counts.starred -= 1
                    }
                }
                store.statusBanner = targetStar ? "Starred \(slug)." : "Unstarred \(slug)."
            case .failure(let err):
                store.statusBanner = "Star failed: \(err.localizedDescription)"
            }
            self.starringSlug = nil
        }
    }

    // MARK: - Clone Dialog & PR Checkout

    func openCloneDialog(for slug: String, scanFolders: [String]) {
        let name = slug.split(separator: "/").last.map(String.init) ?? slug
        let baseFolder = scanFolders.first ?? FileManager.default.homeDirectoryForCurrentUser.path
        let dest = URL(fileURLWithPath: baseFolder).appendingPathComponent(name).path
        cloneDialog = CloneDialogTarget(slug: slug, destination: dest)
    }

    func executeClone(slug: String, destination: String, store: RepoStore) {
        let dest = (destination.trimmingCharacters(in: .whitespacesAndNewlines) as NSString).expandingTildeInPath
        guard !dest.isEmpty else { return }
        cloneDialog = nil
        busyAction = "Cloning \(slug)…"
        store.statusBanner = "Cloning \(slug) into \(dest)…"

        Task {
            let parentPath = URL(fileURLWithPath: dest).deletingLastPathComponent().path
            try? FileManager.default.createDirectory(atPath: parentPath, withIntermediateDirectories: true)

            let res = await GitHubService.runGh(["repo", "clone", slug, dest], timeoutSeconds: 180)
            let repoName = URL(fileURLWithPath: dest).lastPathComponent

            if res.succeeded {
                var newRepo = GitRepo(name: repoName, path: dest, rootScanFolder: parentPath)
                newRepo = await GitService.inspectLocalRepository(newRepo, markUnknownIfCached: false)
                newRepo.lastCheckedAt = Date()
                newRepo.lastCommandOutput = "[$ \(res.commandString)]\n\(res.combinedOutput)"

                let log = ActivityLogEntry(
                    timestamp: Date(),
                    repoName: repoName,
                    repoPath: dest,
                    action: "Clone",
                    command: res.commandString,
                    succeeded: true,
                    summary: "Cloned \(slug)",
                    output: res.combinedOutput
                )
                store.adoptClonedOrForkedRepo(newRepo, logEntry: log, switchToReposView: false)
                store.statusBanner = "Cloned \(slug) → \(newRepo.displayPath)"
            } else {
                let errLine = res.combinedOutput
                    .split(whereSeparator: \.isNewline)
                    .first.map(String.init) ?? "gh repo clone failed"
                let log = ActivityLogEntry(
                    timestamp: Date(),
                    repoName: repoName,
                    repoPath: dest,
                    action: "Clone",
                    command: res.commandString,
                    succeeded: false,
                    summary: errLine,
                    output: res.combinedOutput
                )
                store.recordLog(log)
                store.statusBanner = "Clone failed: \(errLine)"
            }
            self.busyAction = nil
        }
    }

    func checkoutPR(localRepo: GitRepo, item: HubItem, store: RepoStore) {
        busyAction = "Checking out #\(item.number)…"
        store.statusBanner = "Checking out PR #\(item.number) (\(item.headRef)) in \(localRepo.name)…"

        Task {
            let res = await GitHubService.runGh(["pr", "checkout", item.url], in: localRepo.path, timeoutSeconds: 60)
            var updated = await GitService.inspectLocalRepository(localRepo, markUnknownIfCached: false)
            updated.lastCheckedAt = Date()
            updated.lastCommandOutput = "[$ \(res.commandString)]\n\(res.combinedOutput)"

            let summary = res.succeeded
                ? "Checked out PR #\(item.number) (\(updated.branch))"
                : (res.combinedOutput.split(whereSeparator: \.isNewline).first.map(String.init) ?? "gh pr checkout failed")

            let log = ActivityLogEntry(
                timestamp: Date(),
                repoName: localRepo.name,
                repoPath: localRepo.path,
                action: "Checkout PR",
                command: res.commandString,
                succeeded: res.succeeded,
                summary: summary,
                output: res.combinedOutput
            )
            store.applyUpdatedRepo(updated, logEntry: log)
            store.statusBanner = res.succeeded
                ? "\(localRepo.name): \(summary)"
                : "Checkout failed: \(summary)"
            self.busyAction = nil
        }
    }

    // MARK: - Fork Dialog (GitHub & Local)

    func openForkDialog(for repo: GitRepo) {
        let defaultMode: ForkMode = repo.canFork ? .github : .local
        let defaultDest = GitService.defaultLocalForkDestination(for: repo.path)
        let placeholderPlan = ForkRemotePlan(forkRemote: "origin", originalRemote: "upstream", renamesOrigin: true)

        forkDialog = ForkDialogTarget(
            repo: repo,
            mode: defaultMode,
            ownerChoice: .personal,
            remotePlan: placeholderPlan,
            localDestination: defaultDest,
            isRunning: false,
            errorMessage: nil
        )

        Task {
            let plan = await GitHubService.planRemotes(repoPath: repo.path, primaryRemote: repo.primaryRemoteName)
            let status = await GitHubService.cliStatus()
            self.cliStatus = status
            if self.forkDialog?.repo.id == repo.id {
                self.forkDialog?.remotePlan = plan
            }
        }
    }

    func executeFork(store: RepoStore) {
        guard var current = forkDialog, !current.isRunning else { return }
        current.isRunning = true
        current.errorMessage = nil
        forkDialog = current

        switch current.mode {
        case .github:
            let ownerArg: String? = {
                switch current.ownerChoice {
                case .personal: return nil
                case .org(let org): return org
                }
            }()
            store.statusBanner = "Forking \(current.repo.name) on GitHub…"
            Task {
                let (updatedRepo, logEntry) = await GitHubService.forkRepository(current.repo, owner: ownerArg)
                store.applyUpdatedRepo(updatedRepo, logEntry: logEntry)
                if logEntry.succeeded {
                    store.statusBanner = logEntry.summary
                    self.forkDialog = nil
                } else {
                    store.statusBanner = "Fork failed for \(current.repo.name): \(logEntry.summary)"
                    self.forkDialog?.isRunning = false
                    self.forkDialog?.errorMessage = logEntry.summary
                }
            }

        case .local:
            let dest = current.localDestination.trimmingCharacters(in: .whitespacesAndNewlines)
            store.statusBanner = "Creating local copy of \(current.repo.name)…"
            Task {
                let (result, logEntry) = await GitService.forkLocally(source: current.repo, destinationPath: dest)
                switch result {
                case .success(let newRepo):
                    store.adoptClonedOrForkedRepo(newRepo, logEntry: logEntry, switchToReposView: true)
                    self.forkDialog = nil
                case .failure(let err):
                    store.recordLog(logEntry)
                    store.statusBanner = "Local fork failed: \(err.localizedDescription)"
                    self.forkDialog?.isRunning = false
                    self.forkDialog?.errorMessage = err.localizedDescription
                }
            }
        }
    }
}

// MARK: - In-App Software Updater Store

struct ReleaseInfo: Equatable, Sendable {
    let version: String
    let tag: String
    let title: String
    let notes: String
    let htmlURL: String
    let downloadURL: String?
    let assetName: String?
    let publishedAt: String
}

enum UpdateStatus: Equatable, Sendable {
    case idle
    case checking
    case upToDate
    case available(ReleaseInfo)
    case failed(String)
}

@MainActor
final class UpdaterStore: ObservableObject {
    static let appVersion: String = {
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0.1.0"
    }()
    static let releasesPageURL = "https://github.com/Duyfrom/GitDaddy/releases"
    private static let latestAPIURL = "https://api.github.com/repos/Duyfrom/GitDaddy/releases/latest"

    @Published var status: UpdateStatus = .idle
    @Published var showModal: Bool = false

    var availableRelease: ReleaseInfo? {
        if case .available(let info) = status {
            return info
        }
        return nil
    }

    func startAutoCheck() {
        Task {
            try? await Task.sleep(nanoseconds: 3_000_000_000)
            if self.status == .idle {
                self.check(openModal: false)
            }
        }
    }

    func check(openModal: Bool) {
        if openModal {
            showModal = true
        }
        guard status != .checking else { return }
        status = .checking

        Task {
            let result = await Self.fetchLatestRelease()
            switch result {
            case .success(let info):
                if Self.isNewer(latest: info.version, current: Self.appVersion) {
                    self.status = .available(info)
                } else {
                    self.status = .upToDate
                }
            case .failure(let err):
                self.status = .failed(err.localizedDescription)
            }
        }
    }

    private static func fetchLatestRelease() async -> Result<ReleaseInfo, Error> {
        guard let url = URL(string: latestAPIURL) else {
            return .failure(NSError(domain: "GitDaddy.Update", code: 1, userInfo: [NSLocalizedDescriptionKey: "Invalid update URL"]))
        }
        var request = URLRequest(url: url, timeoutInterval: 15)
        request.setValue("GitDaddy/\(appVersion)", forHTTPHeaderField: "User-Agent")
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")

        do {
            let (data, response) = try await URLSession.shared.data(for: request)
            guard let http = response as? HTTPURLResponse else {
                return .failure(NSError(domain: "GitDaddy.Update", code: 2, userInfo: [NSLocalizedDescriptionKey: "No HTTP response"]))
            }
            if http.statusCode == 404 {
                return .success(ReleaseInfo(
                    version: appVersion,
                    tag: "v\(appVersion)",
                    title: "GitDaddy v\(appVersion)",
                    notes: "",
                    htmlURL: releasesPageURL,
                    downloadURL: nil,
                    assetName: nil,
                    publishedAt: ""
                ))
            }
            guard (200..<300).contains(http.statusCode) else {
                return .failure(NSError(domain: "GitDaddy.Update", code: http.statusCode, userInfo: [NSLocalizedDescriptionKey: "GitHub API returned HTTP \(http.statusCode)"]))
            }
            guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let rawTag = json["tag_name"] as? String else {
                return .failure(NSError(domain: "GitDaddy.Update", code: 3, userInfo: [NSLocalizedDescriptionKey: "Malformed release JSON"]))
            }
            let version = normalizeVersion(rawTag)
            let title = (json["name"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? rawTag
            let notes = (json["body"] as? String) ?? ""
            let htmlURL = (json["html_url"] as? String) ?? releasesPageURL
            let publishedRaw = (json["published_at"] as? String) ?? ""
            let publishedAt = String(publishedRaw.prefix(10))

            var downloadURL: String? = nil
            var assetName: String? = nil
            if let assets = json["assets"] as? [[String: Any]] {
                for asset in assets {
                    guard let name = asset["name"] as? String,
                          let browserURL = asset["browser_download_url"] as? String else { continue }
                    let lower = name.lowercased()
                    let isMacAsset = lower.hasSuffix(".dmg")
                        || lower.hasSuffix(".pkg")
                        || ((lower.contains("mac") || lower.contains("darwin") || lower.contains("osx")) && lower.hasSuffix(".zip"))
                    if isMacAsset {
                        downloadURL = browserURL
                        assetName = name
                        if lower.hasSuffix(".dmg") {
                            break
                        }
                    }
                }
            }

            return .success(ReleaseInfo(
                version: version,
                tag: rawTag,
                title: title,
                notes: notes,
                htmlURL: htmlURL,
                downloadURL: downloadURL,
                assetName: assetName,
                publishedAt: publishedAt
            ))
        } catch {
            return .failure(error)
        }
    }

    static func normalizeVersion(_ raw: String) -> String {
        var s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if s.hasPrefix("v") || s.hasPrefix("V") {
            s.removeFirst()
        }
        return s.split(whereSeparator: { $0 == "-" || $0 == "+" }).first.map(String.init) ?? s
    }

    static func isNewer(latest: String, current: String) -> Bool {
        let parse: (String) -> [UInt64] = { s in
            normalizeVersion(s).split(separator: ".").map { UInt64($0) ?? 0 }
        }
        let l = parse(latest)
        let c = parse(current)
        let count = max(l.count, c.count, 3)
        for i in 0..<count {
            let lv = i < l.count ? l[i] : 0
            let cv = i < c.count ? c[i] : 0
            if lv != cv { return lv > cv }
        }
        return false
    }
}
