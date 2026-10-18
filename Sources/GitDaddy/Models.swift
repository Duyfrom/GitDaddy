import Foundation
import SwiftUI

enum SyncState: Equatable, Sendable {
    case unknown
    case checking
    case pulling
    case pushing
    case upToDate
    case behind(count: Int)
    case ahead(count: Int)
    case diverged(ahead: Int, behind: Int)
    case unpublishedBranch(branch: String, remote: String)
    case noRemoteConfigured
    case emptyRepo(hasRemote: Bool)
    case detachedHead(commit: String)
    case error(String)

    var isBusy: Bool {
        switch self {
        case .checking, .pulling, .pushing:
            return true
        default:
            return false
        }
    }

    var needsUpdate: Bool {
        switch self {
        case .behind(let count):
            return count > 0
        case .diverged(_, let behind):
            return behind > 0
        default:
            return false
        }
    }

    var canPush: Bool {
        if aheadCount > 0 { return true }
        if case .unpublishedBranch = self { return true }
        return false
    }

    var isUnpublishedBranch: Bool {
        if case .unpublishedBranch = self { return true }
        return false
    }

    var canPull: Bool {
        switch self {
        case .noRemoteConfigured, .emptyRepo, .detachedHead, .unpublishedBranch:
            return false
        default:
            return !isBusy
        }
    }

    var isIssueOrNoUpstream: Bool {
        switch self {
        case .unpublishedBranch, .noRemoteConfigured, .emptyRepo, .detachedHead, .error:
            return true
        default:
            return false
        }
    }

    var behindCount: Int {
        switch self {
        case .behind(let count):
            return count
        case .diverged(_, let behind):
            return behind
        default:
            return 0
        }
    }

    var aheadCount: Int {
        switch self {
        case .ahead(let count):
            return count
        case .diverged(let ahead, _):
            return ahead
        default:
            return 0
        }
    }

    var sortPriority: Int {
        switch self {
        case .behind: return 0
        case .diverged: return 1
        case .ahead: return 2
        case .unpublishedBranch: return 3
        case .error: return 4
        case .pulling, .pushing, .checking: return 5
        case .unknown: return 6
        case .upToDate: return 7
        case .detachedHead: return 8
        case .emptyRepo: return 9
        case .noRemoteConfigured: return 10
        }
    }

    var label: String {
        switch self {
        case .unknown:
            return "Not Checked"
        case .checking:
            return "Checking Remote…"
        case .pulling:
            return "Pulling Updates…"
        case .pushing:
            return "Pushing Commits…"
        case .upToDate:
            return "Up to Date"
        case .behind(let count):
            return "\(count) \(count == 1 ? "Update" : "Updates") Available"
        case .ahead(let count):
            return "\(count) Unpushed"
        case .diverged(let ahead, let behind):
            return "Diverged (↑\(ahead) ↓\(behind))"
        case .unpublishedBranch:
            return "Unpublished Branch"
        case .noRemoteConfigured:
            return "Local Only (No Remote)"
        case .emptyRepo:
            return "Empty Repo (0 Commits)"
        case .detachedHead(let commit):
            return "Detached HEAD (@\(commit))"
        case .error:
            return "Sync Error"
        }
    }

    var rowExplanation: String? {
        switch self {
        case .unpublishedBranch(let branch, let remote):
            return "Branch '\(branch)' not pushed to '\(remote)' yet"
        case .noRemoteConfigured:
            return "Local-only repository — no remote URL configured"
        case .emptyRepo(let hasRemote):
            return hasRemote
                ? "Remote URL configured, 0 commits locally"
                : "Initialized repository with 0 commits and no remote"
        case .detachedHead(let commit):
            return "Detached HEAD at \(commit)"
        case .error(let msg):
            return msg
        default:
            return nil
        }
    }

    var inspectorExplanation: String {
        switch self {
        case .unknown:
            return "Click 'Check' to fetch from the remote repository and compare commit histories."
        case .checking:
            return "Fetching latest branch references from the remote server…"
        case .pulling:
            return "Pulling incoming commits from the remote tracking branch…"
        case .pushing:
            return "Pushing local commits to the remote repository…"
        case .upToDate:
            return "Your local branch and remote tracking branch have the exact same HEAD commit."
        case .behind(let count):
            return "The remote branch has \(count) new \(count == 1 ? "commit" : "commits") that you haven't pulled locally yet."
        case .ahead(let count):
            return "You have \(count) local \(count == 1 ? "commit" : "commits") not yet pushed to the remote branch."
        case .diverged(let ahead, let behind):
            return "Your local branch has \(ahead) unpushed \(ahead == 1 ? "commit" : "commits") and the remote has \(behind) incoming \(behind == 1 ? "commit" : "commits"). Pull before pushing."
        case .unpublishedBranch(let branch, let remote):
            return "Connected to remote '\(remote)', but current branch '\(branch)' only exists on your Mac. Click 'Publish Branch' to run 'git push -u \(remote) \(branch)'."
        case .noRemoteConfigured:
            return "Initialized with Git locally, but has no remote server URL ('origin'). Add a remote URL below to connect it."
        case .emptyRepo(let hasRemote):
            return hasRemote
                ? "Remote URL is configured, but no commits exist yet (Unborn HEAD). Stage and commit a file below to create your first commit."
                : "This repository has no commits and no remote URL configured yet."
        case .detachedHead(let commit):
            return "Git is in 'Detached HEAD' mode at commit \(commit) rather than on a named branch. Switch to a branch to pull or push."
        case .error(let msg):
            return msg
        }
    }
}

struct CommitInfo: Identifiable, Equatable, Sendable {
    var id: String { hash }
    let hash: String
    let subject: String
    let author: String
    let relativeDate: String
}

struct FileChange: Identifiable, Equatable, Sendable {
    var id: String { "\(status):\(path)" }
    let status: String
    let path: String
}

enum PullStrategy: String, CaseIterable, Identifiable, Sendable {
    case merge = "Merge (Default)"
    case rebase = "Rebase (--rebase)"
    case fastForwardOnly = "Fast-Forward (--ff-only)"

    var id: String { rawValue }

    var gitArguments: [String] {
        switch self {
        case .merge:
            return ["pull"]
        case .rebase:
            return ["pull", "--rebase"]
        case .fastForwardOnly:
            return ["pull", "--ff-only"]
        }
    }
}

enum RepoFilter: String, CaseIterable, Identifiable {
    case all = "All Repos"
    case needsUpdate = "Needs Update"
    case upToDate = "Up to Date"
    case localChanges = "Dirty / Uncommitted"
    case aheadOrDiverged = "Unpushed / Unpublished"
    case errors = "Local Only / Issues"

    var id: String { rawValue }

    var statusPreview: CanonicalStatusKind {
        switch self {
        case .all: return .allRepos
        case .needsUpdate: return .needsUpdate
        case .upToDate: return .success
        case .localChanges: return .dirty
        case .aheadOrDiverged: return .unpushed
        case .errors: return .danger
        }
    }
}

enum RepoSortOrder: String, CaseIterable, Identifiable {
    case statusPriority = "Update Status"
    case name = "Name (A–Z)"
    case lastUpdated = "Recent Commit"
    case path = "Folder Path"

    var id: String { rawValue }
}

struct ActivityLogEntry: Identifiable, Equatable, Sendable {
    let id = UUID()
    let timestamp: Date
    let repoName: String
    let repoPath: String
    let action: String
    let command: String
    let succeeded: Bool
    let summary: String
    let output: String
}

struct GitRepo: Identifiable, Equatable, Sendable {
    var id: String { path }
    let name: String
    let path: String
    let rootScanFolder: String

    var branch: String = "—"
    var upstreamBranch: String? = nil
    var isTrackingExplicitlySet: Bool = false
    var primaryRemoteName: String? = nil
    var remoteURL: String? = nil
    var syncState: SyncState = .unknown

    var incomingCommits: [CommitInfo] = []
    var unpushedCommits: [CommitInfo] = []
    var localChanges: [FileChange] = []

    var lastCommitHash: String = ""
    var lastCommitMessage: String = ""
    var lastCommitRelativeDate: String = ""
    var lastCommitTimestamp: Date? = nil

    var lastCheckedAt: Date? = nil
    var lastCommandOutput: String = ""
    var lastPullMessage: String? = nil

    var repoDescription: String? = nil
    var readmeFileName: String? = nil
    var readmeContent: String? = nil

    var isDirty: Bool {
        !localChanges.isEmpty
    }

    var modifiedCount: Int {
        localChanges.filter { $0.status != "??" }.count
    }

    var untrackedCount: Int {
        localChanges.filter { $0.status == "??" }.count
    }

    var displayPath: String {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        if path.hasPrefix(home) {
            return "~" + path.dropFirst(home.count)
        }
        return path
    }

    var parentDisplayPath: String {
        let parent = (path as NSString).deletingLastPathComponent
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        if parent.hasPrefix(home) {
            return "~" + parent.dropFirst(home.count)
        }
        return parent
    }

    var formattedRemoteHost: String? {
        guard let url = remoteURL, !url.isEmpty else { return nil }
        if url.hasPrefix("git@") {
            let stripped = url.dropFirst(4)
                .replacingOccurrences(of: ":", with: "/")
                .replacingOccurrences(of: ".git", with: "")
            return String(stripped)
        }
        if let parsed = URL(string: url), let host = parsed.host {
            let cleanPath = parsed.path.replacingOccurrences(of: ".git", with: "")
            return host + cleanPath
        }
        return url
    }
}
