import AppKit
import Foundation
import SwiftUI

@MainActor
final class RepoStore: ObservableObject {
    private static let scanFoldersKey = "GitDaddy.scanFolders"
    private static let scanNestedKey = "GitDaddy.scanNestedRepos"
    private static let pullStrategyKey = "GitDaddy.pullStrategy"
    private static let autoStashKey = "GitDaddy.autoStashOnPull"

    @Published var scanFolders: [String] = [] {
        didSet {
            UserDefaults.standard.set(scanFolders, forKey: Self.scanFoldersKey)
        }
    }

    @Published var scanNestedRepos: Bool = false {
        didSet {
            UserDefaults.standard.set(scanNestedRepos, forKey: Self.scanNestedKey)
        }
    }

    @Published var pullStrategy: PullStrategy = .merge {
        didSet {
            UserDefaults.standard.set(pullStrategy.rawValue, forKey: Self.pullStrategyKey)
        }
    }

    @Published var autoStashOnPull: Bool = true {
        didSet {
            UserDefaults.standard.set(autoStashOnPull, forKey: Self.autoStashKey)
        }
    }

    @Published var repos: [GitRepo] = []
    @Published var selectedRepoIDs: Set<String> = []
    @Published var focusedRepoID: String? = nil

    @Published var filter: RepoFilter = .all
    @Published var sortOrder: RepoSortOrder = .statusPriority
    @Published var searchText: String = ""

    @Published var isScanning: Bool = false
    @Published var isBatchRunning: Bool = false
    @Published var batchCompletedCount: Int = 0
    @Published var batchTotalCount: Int = 0
    @Published var statusBanner: String = "Ready"

    @Published var activityLogs: [ActivityLogEntry] = []

    init() {
        let savedFolders = UserDefaults.standard.stringArray(forKey: Self.scanFoldersKey)
        if let savedFolders, !savedFolders.isEmpty {
            self.scanFolders = savedFolders
        } else {
            self.scanFolders = []
            self.statusBanner = "Select a workspace folder to start scanning Git repositories."
        }

        self.scanNestedRepos = UserDefaults.standard.bool(forKey: Self.scanNestedKey)

        if let rawStrategy = UserDefaults.standard.string(forKey: Self.pullStrategyKey),
           let strategy = PullStrategy(rawValue: rawStrategy) {
            self.pullStrategy = strategy
        }

        if UserDefaults.standard.object(forKey: Self.autoStashKey) != nil {
            self.autoStashOnPull = UserDefaults.standard.bool(forKey: Self.autoStashKey)
        } else {
            self.autoStashOnPull = true
        }
    }

    // MARK: - Computed Properties

    var filteredRepos: [GitRepo] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()

        let matching = repos.filter { repo in
            let matchesFilter: Bool
            switch filter {
            case .all:
                matchesFilter = true
            case .needsUpdate:
                matchesFilter = repo.syncState.needsUpdate
            case .upToDate:
                matchesFilter = repo.syncState == .upToDate
            case .localChanges:
                matchesFilter = repo.isDirty
            case .aheadOrDiverged:
                switch repo.syncState {
                case .ahead, .diverged, .unpublishedBranch:
                    matchesFilter = true
                default:
                    matchesFilter = false
                }
            case .errors:
                matchesFilter = repo.syncState.isIssueOrNoUpstream
            }

            guard matchesFilter else { return false }
            guard !query.isEmpty else { return true }

            return repo.name.lowercased().contains(query)
                || repo.path.lowercased().contains(query)
                || repo.branch.lowercased().contains(query)
                || (repo.remoteURL?.lowercased().contains(query) ?? false)
        }

        return matching.sorted { lhs, rhs in
            switch sortOrder {
            case .statusPriority:
                if lhs.syncState.sortPriority != rhs.syncState.sortPriority {
                    return lhs.syncState.sortPriority < rhs.syncState.sortPriority
                }
                if lhs.syncState.behindCount != rhs.syncState.behindCount {
                    return lhs.syncState.behindCount > rhs.syncState.behindCount
                }
                return lhs.name.localizedStandardCompare(rhs.name) == .orderedAscending
            case .name:
                return lhs.name.localizedStandardCompare(rhs.name) == .orderedAscending
            case .lastUpdated:
                let lDate = lhs.lastCommitTimestamp ?? .distantPast
                let rDate = rhs.lastCommitTimestamp ?? .distantPast
                if lDate != rDate {
                    return lDate > rDate
                }
                return lhs.name.localizedStandardCompare(rhs.name) == .orderedAscending
            case .path:
                return lhs.path.localizedStandardCompare(rhs.path) == .orderedAscending
            }
        }
    }

    var focusedRepo: GitRepo? {
        guard let focusedRepoID else { return nil }
        return repos.first(where: { $0.id == focusedRepoID })
    }

    var outdatedReposCount: Int {
        repos.filter { $0.syncState.needsUpdate }.count
    }

    var upToDateReposCount: Int {
        repos.filter { $0.syncState == .upToDate }.count
    }

    var dirtyReposCount: Int {
        repos.filter { $0.isDirty }.count
    }

    var aheadOrDivergedCount: Int {
        repos.filter {
            switch $0.syncState {
            case .ahead, .diverged, .unpublishedBranch:
                return true
            default:
                return false
            }
        }.count
    }

    var unpushedReposCount: Int {
        repos.filter { $0.syncState.canPush }.count
    }

    var errorOrNoRemoteCount: Int {
        repos.filter { $0.syncState.isIssueOrNoUpstream }.count
    }

    // MARK: - Folder Management

    func promptToAddFolder() {
        let panel = NSOpenPanel()
        panel.title = "Add Folder or Git Repository to Scan"
        panel.message = "Choose a workspace directory (to detect all git folders inside) or an individual Git repository."
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = true
        panel.canCreateDirectories = false

        if panel.runModal() == .OK {
            var addedAny = false
            for url in panel.urls {
                let path = url.standardizedFileURL.path
                if !scanFolders.contains(path) {
                    scanFolders.append(path)
                    addedAny = true
                }
            }
            if addedAny {
                Task {
                    await scanDirectories(checkRemoteAfterScan: true)
                }
            }
        }
    }

    func removeScanFolder(_ folder: String) {
        scanFolders.removeAll { $0 == folder }
        Task {
            await scanDirectories(checkRemoteAfterScan: false)
        }
    }

    // MARK: - Selection Helpers

    func toggleSelection(for repoID: String) {
        if selectedRepoIDs.contains(repoID) {
            selectedRepoIDs.remove(repoID)
        } else {
            selectedRepoIDs.insert(repoID)
        }
    }

    func selectAllVisible() {
        for repo in filteredRepos {
            selectedRepoIDs.insert(repo.id)
        }
    }

    func selectAllOutdated() {
        selectedRepoIDs = Set(repos.filter { $0.syncState.needsUpdate }.map(\.id))
    }

    func selectAllUnpushed() {
        selectedRepoIDs = Set(repos.filter { $0.syncState.canPush }.map(\.id))
    }

    func clearSelection() {
        selectedRepoIDs.removeAll()
    }

    // MARK: - Scanning & Git Operations

    func scanDirectories(checkRemoteAfterScan: Bool = true) async {
        guard !isScanning else { return }
        if scanFolders.isEmpty {
            repos = []
            selectedRepoIDs.removeAll()
            focusedRepoID = nil
            statusBanner = "Select a workspace folder to start scanning Git repositories."
            return
        }
        isScanning = true
        statusBanner = "Scanning folders for Git repositories…"

        let existingMap = Dictionary(uniqueKeysWithValues: repos.map { ($0.path, $0) })
        let discovered = await RepoScanner.discoverRepositories(
            in: scanFolders,
            maxDepth: 5,
            scanNestedRepos: scanNestedRepos
        )

        var initialRepos: [GitRepo] = []
        for item in discovered {
            if let existing = existingMap[item.repoPath] {
                initialRepos.append(existing)
            } else {
                let name = URL(fileURLWithPath: item.repoPath).lastPathComponent
                initialRepos.append(
                    GitRepo(
                        name: name,
                        path: item.repoPath,
                        rootScanFolder: item.rootFolder
                    )
                )
            }
        }

        repos = initialRepos
        selectedRepoIDs.formIntersection(Set(initialRepos.map(\.id)))
        if focusedRepoID == nil || !repos.contains(where: { $0.id == focusedRepoID }) {
            focusedRepoID = repos.first?.id
        }

        isScanning = false
        statusBanner = "Detected \(repos.count) Git \(repos.count == 1 ? "repository" : "repositories"). Reading local state…"

        // Fast local status pass
        await inspectAllLocalStates()

        if checkRemoteAfterScan && !repos.isEmpty {
            await checkRemoteForRepos(ids: repos.map(\.id))
        } else {
            statusBanner = "Detected \(repos.count) Git \(repos.count == 1 ? "repository" : "repositories")."
        }
    }

    private func inspectAllLocalStates() async {
        let snapshot = repos
        await withTaskGroup(of: GitRepo.self) { group in
            var iterator = snapshot.makeIterator()
            let maxConcurrency = 8
            for _ in 0..<maxConcurrency {
                if let next = iterator.next() {
                    group.addTask {
                        let keepCachedState = next.lastCheckedAt != nil
                        return await GitService.inspectLocalRepository(next, markUnknownIfCached: !keepCachedState)
                    }
                }
            }

            while let updated = await group.next() {
                updateRepoInPlace(updated)
                if let next = iterator.next() {
                    group.addTask {
                        let keepCachedState = next.lastCheckedAt != nil
                        return await GitService.inspectLocalRepository(next, markUnknownIfCached: !keepCachedState)
                    }
                }
            }
        }
    }

    func checkSingleRepo(_ repoID: String) async {
        await checkRemoteForRepos(ids: [repoID])
    }

    func checkSelectedRepos() async {
        let targets = Array(selectedRepoIDs)
        guard !targets.isEmpty else { return }
        await checkRemoteForRepos(ids: targets)
    }

    func checkAllRepos() async {
        let targets = repos.map(\.id)
        guard !targets.isEmpty else { return }
        await checkRemoteForRepos(ids: targets)
    }

    func checkRemoteForRepos(ids: [String]) async {
        let targetRepos = repos.filter { ids.contains($0.id) && !$0.syncState.isBusy }
        guard !targetRepos.isEmpty else { return }

        isBatchRunning = true
        batchCompletedCount = 0
        batchTotalCount = targetRepos.count

        for r in targetRepos {
            setSyncState(.checking, for: r.id)
        }
        statusBanner = "Checking remote updates for \(targetRepos.count) \(targetRepos.count == 1 ? "repository" : "repositories")…"

        let maxConcurrency = 6
        await withTaskGroup(of: (GitRepo, ActivityLogEntry).self) { group in
            var iterator = targetRepos.makeIterator()
            for _ in 0..<maxConcurrency {
                if let next = iterator.next() {
                    group.addTask {
                        await GitService.checkRemoteStatus(for: next)
                    }
                }
            }

            while let (updatedRepo, logEntry) = await group.next() {
                updateRepoInPlace(updatedRepo)
                prependLog(logEntry)
                batchCompletedCount += 1

                if let next = iterator.next() {
                    group.addTask {
                        await GitService.checkRemoteStatus(for: next)
                    }
                }
            }
        }

        isBatchRunning = false
        let behindTotal = outdatedReposCount
        if behindTotal > 0 {
            statusBanner = "Check complete • \(behindTotal) \(behindTotal == 1 ? "repository has" : "repositories have") updates available."
        } else {
            statusBanner = "Check complete • All repositories are up to date with their remotes."
        }
    }

    // MARK: - Pull Operations

    func pullSingleRepo(_ repoID: String) async {
        await pullRepos(ids: [repoID])
    }

    func pullSelectedRepos() async {
        let targets = Array(selectedRepoIDs)
        guard !targets.isEmpty else { return }
        await pullRepos(ids: targets)
    }

    func pullAllOutdatedRepos() async {
        let targets = repos.filter { $0.syncState.needsUpdate }.map(\.id)
        guard !targets.isEmpty else { return }
        await pullRepos(ids: targets)
    }

    func pullAllRepos() async {
        let targets = repos.filter { $0.syncState.canPull }.map(\.id)
        guard !targets.isEmpty else { return }
        await pullRepos(ids: targets)
    }

    func pullRepos(ids: [String]) async {
        let targetRepos = repos.filter { ids.contains($0.id) && !$0.syncState.isBusy }
        guard !targetRepos.isEmpty else { return }

        let currentStrategy = pullStrategy
        let currentAutoStash = autoStashOnPull

        isBatchRunning = true
        batchCompletedCount = 0
        batchTotalCount = targetRepos.count

        for r in targetRepos {
            setSyncState(.pulling, for: r.id)
        }
        statusBanner = "Pulling updates for \(targetRepos.count) \(targetRepos.count == 1 ? "repository" : "repositories")…"

        var successCount = 0
        var failCount = 0
        let maxConcurrency = 4

        await withTaskGroup(of: (GitRepo, ActivityLogEntry).self) { group in
            var iterator = targetRepos.makeIterator()
            for _ in 0..<maxConcurrency {
                if let next = iterator.next() {
                    group.addTask {
                        await GitService.pullRepository(
                            next,
                            strategy: currentStrategy,
                            autoStash: currentAutoStash
                        )
                    }
                }
            }

            while let (updatedRepo, logEntry) = await group.next() {
                updateRepoInPlace(updatedRepo)
                prependLog(logEntry)
                batchCompletedCount += 1
                if logEntry.succeeded {
                    successCount += 1
                } else {
                    failCount += 1
                }

                if let next = iterator.next() {
                    group.addTask {
                        await GitService.pullRepository(
                            next,
                            strategy: currentStrategy,
                            autoStash: currentAutoStash
                        )
                    }
                }
            }
        }

        isBatchRunning = false
        if failCount == 0 {
            statusBanner = "Successfully pulled \(successCount) \(successCount == 1 ? "repository" : "repositories")."
        } else {
            statusBanner = "Pulled \(successCount) succeeded, \(failCount) failed. Check inspector or activity log for details."
        }
    }

    // MARK: - Push Operations

    func pushSingleRepo(_ repoID: String) async {
        await pushRepos(ids: [repoID])
    }

    func pushSelectedRepos() async {
        let targets = Array(selectedRepoIDs)
        guard !targets.isEmpty else { return }
        await pushRepos(ids: targets)
    }

    func pushAllUnpushedRepos() async {
        let targets = repos.filter { $0.syncState.canPush }.map(\.id)
        guard !targets.isEmpty else { return }
        await pushRepos(ids: targets)
    }

    func pushRepos(ids: [String]) async {
        let targetRepos = repos.filter { ids.contains($0.id) && !$0.syncState.isBusy && $0.remoteURL != nil }
        guard !targetRepos.isEmpty else { return }

        isBatchRunning = true
        batchCompletedCount = 0
        batchTotalCount = targetRepos.count

        for r in targetRepos {
            setSyncState(.pushing, for: r.id)
        }
        statusBanner = "Pushing commits for \(targetRepos.count) \(targetRepos.count == 1 ? "repository" : "repositories")…"

        var successCount = 0
        var failCount = 0
        let maxConcurrency = 4

        await withTaskGroup(of: (GitRepo, ActivityLogEntry).self) { group in
            var iterator = targetRepos.makeIterator()
            for _ in 0..<maxConcurrency {
                if let next = iterator.next() {
                    group.addTask {
                        await GitService.pushRepository(next)
                    }
                }
            }

            while let (updatedRepo, logEntry) = await group.next() {
                updateRepoInPlace(updatedRepo)
                prependLog(logEntry)
                batchCompletedCount += 1
                if logEntry.succeeded {
                    successCount += 1
                } else {
                    failCount += 1
                }

                if let next = iterator.next() {
                    group.addTask {
                        await GitService.pushRepository(next)
                    }
                }
            }
        }

        isBatchRunning = false
        if failCount == 0 {
            statusBanner = "Successfully pushed \(successCount) \(successCount == 1 ? "repository" : "repositories")."
        } else {
            statusBanner = "Pushed \(successCount) succeeded, \(failCount) failed. Check inspector or activity log for details."
        }
    }

    func commitAndPushSingleRepo(_ repoID: String, message: String) async {
        let trimmed = message.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty,
              let target = repos.first(where: { $0.id == repoID }),
              !target.syncState.isBusy else {
            return
        }

        setSyncState(.pushing, for: repoID)
        statusBanner = "Committing changes in \(target.name)…"

        let (updatedRepo, logEntry) = await GitService.commitAndPushRepository(target, message: trimmed)
        updateRepoInPlace(updatedRepo)
        prependLog(logEntry)

        if logEntry.succeeded {
            statusBanner = logEntry.summary
        } else {
            statusBanner = "Commit failed for \(target.name): \(logEntry.summary)"
        }
    }

    func addRemoteOrigin(to repoID: String, url: String) async {
        let trimmed = url.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty,
              let target = repos.first(where: { $0.id == repoID }),
              !target.syncState.isBusy else {
            return
        }

        setSyncState(.checking, for: repoID)
        statusBanner = "Adding remote origin to \(target.name)…"

        let (updatedRepo, logEntry) = await GitService.addRemoteOrigin(trimmed, to: target)
        updateRepoInPlace(updatedRepo)
        prependLog(logEntry)

        if logEntry.succeeded {
            statusBanner = "Added remote origin and checked \(target.name)."
        } else {
            statusBanner = "Failed to add remote to \(target.name): \(logEntry.summary)"
        }
    }

    // MARK: - Delete Local Repository

    func deleteRepositories(ids: [String], moveToTrash: Bool = true) {
        let targets = repos.filter { ids.contains($0.id) }
        guard !targets.isEmpty else { return }

        let fm = FileManager.default
        var deletedCount = 0
        var failedCount = 0

        for repo in targets {
            let folderURL = URL(fileURLWithPath: repo.path)
            do {
                if moveToTrash {
                    try fm.trashItem(at: folderURL, resultingItemURL: nil)
                } else {
                    try fm.removeItem(at: folderURL)
                }
                deletedCount += 1

                repos.removeAll { $0.id == repo.id }
                selectedRepoIDs.remove(repo.id)
                scanFolders.removeAll { $0 == repo.path }
                if focusedRepoID == repo.id {
                    focusedRepoID = repos.first?.id
                }

                let actionName = moveToTrash ? "Move to Trash" : "Delete Permanently"
                prependLog(
                    ActivityLogEntry(
                        timestamp: Date(),
                        repoName: repo.name,
                        repoPath: repo.path,
                        action: actionName,
                        command: moveToTrash ? "Trash \(repo.displayPath)" : "rm -rf \(repo.displayPath)",
                        succeeded: true,
                        summary: moveToTrash ? "Moved local repository to Trash" : "Permanently deleted local repository",
                        output: "Removed local directory: \(repo.path)"
                    )
                )
            } catch {
                failedCount += 1
                prependLog(
                    ActivityLogEntry(
                        timestamp: Date(),
                        repoName: repo.name,
                        repoPath: repo.path,
                        action: "Delete Repo",
                        command: "Remove \(repo.displayPath)",
                        succeeded: false,
                        summary: error.localizedDescription,
                        output: "Failed to delete \(repo.path): \(error.localizedDescription)"
                    )
                )
            }
        }

        if failedCount == 0 {
            let verb = moveToTrash ? "Moved" : "Permanently deleted"
            let suffix = moveToTrash ? " to Trash." : " from disk."
            statusBanner = "\(verb) \(deletedCount) local \(deletedCount == 1 ? "repository" : "repositories")\(suffix)"
        } else {
            statusBanner = "Deleted \(deletedCount), failed to delete \(failedCount). Check activity log."
        }
    }

    // MARK: - Helpers & External Actions

    private func setSyncState(_ state: SyncState, for repoID: String) {
        if let idx = repos.firstIndex(where: { $0.id == repoID }) {
            repos[idx].syncState = state
        }
    }

    private func updateRepoInPlace(_ updated: GitRepo) {
        if let idx = repos.firstIndex(where: { $0.id == updated.id }) {
            repos[idx] = updated
        }
    }

    private func prependLog(_ entry: ActivityLogEntry) {
        activityLogs.insert(entry, at: 0)
        if activityLogs.count > 250 {
            activityLogs.removeLast(activityLogs.count - 250)
        }
    }

    func openInFinder(_ path: String) {
        NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: path)
    }

    func openInTerminal(_ path: String) {
        let terminalApps = [
            "/Applications/iTerm.app",
            "/Applications/Ghostty.app",
            "/Applications/Warp.app",
            "/System/Applications/Utilities/Terminal.app"
        ]
        for appPath in terminalApps where FileManager.default.fileExists(atPath: appPath) {
            let appURL = URL(fileURLWithPath: appPath)
            let folderURL = URL(fileURLWithPath: path)
            let config = NSWorkspace.OpenConfiguration()
            NSWorkspace.shared.open([folderURL], withApplicationAt: appURL, configuration: config)
            return
        }
    }

    func copyPathToClipboard(_ path: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(path, forType: .string)
    }
}
