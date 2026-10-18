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
        case .explore: return "flame"
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

enum TrendingSpan: String, CaseIterable, Identifiable, Sendable {
    case today = "Today"
    case thisWeek = "This week"
    case thisMonth = "This month"

    var id: String { rawValue }
    var label: String { rawValue }

    var days: Int64 {
        switch self {
        case .today: return 2
        case .thisWeek: return 7
        case .thisMonth: return 30
        }
    }

    /// Title for the active window at `offset` steps back (`0` = current window).
    func windowTitle(offset: Int) -> String {
        let range = dateRange(offset: offset)
        if offset == 0 {
            return "\(label) (\(range.start) .. \(range.end))"
        }
        return "\(range.start) .. \(range.end)"
    }

    func dateRange(offset: Int) -> (start: String, end: String) {
        let nowSecs = Int64(Date().timeIntervalSince1970)
        let spanSecs = days * 86_400
        let endSecs = nowSecs - Int64(max(0, offset)) * spanSecs
        let startSecs = endSecs - spanSecs
        return (Self.formatUTCDate(startSecs), Self.formatUTCDate(endSecs))
    }

    func canAdvance(offset: Int) -> Bool {
        let range = dateRange(offset: offset + 1)
        return range.end.asString >= "2008-02-01"
    }

    func query(language: String?, offset: Int) -> String {
        let range = dateRange(offset: offset)
        var q = "created:\(range.start)..\(range.end) sort:stars"
        if let lang = language?.trimmingCharacters(in: .whitespacesAndNewlines), !lang.isEmpty {
            if lang.contains(" ") {
                q += " language:\"\(lang)\""
            } else {
                q += " language:\(lang)"
            }
        }
        return q
    }

    static func formatUTCDate(_ epochSeconds: Int64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(max(0, epochSeconds)))
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter.string(from: date)
    }

    static let trendingLanguages: [String] = [
        "Rust",
        "TypeScript",
        "Python",
        "Go",
        "JavaScript",
        "C",
        "C++",
        "Zig",
        "Swift",
        "Kotlin",
        "Java",
        "C#",
        "Ruby",
        "PHP",
        "Shell",
        "HTML"
    ]
}

private extension String {
    var asString: String { self }
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
    let isFork: Bool
    let isArchived: Bool
    let isPrivate: Bool
    var viewerHasStarred: Bool
    let defaultBranch: String
    let primaryTopics: [String]
    let openGraphImageURL: String?

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

    static func fetchTrending(search: String) async -> Result<[HubRepo], Error> {
        let res = await runGh(
            ["api", "graphql", "-f", "query=\(Self.trendingQuery)", "-f", "q=\(search)"],
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
        let nodes = (((root["data"] as? [String: Any])?["search"] as? [String: Any])?["nodes"] as? [[String: Any]]) ?? []
        let repos = nodes.compactMap(parseRepo)
        return .success(repos)
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
        let name = (v["name"] as? String) ?? ""
        let owner = ((v["owner"] as? [String: Any])?["login"] as? String) ?? ""
        let description = (v["description"] as? String) ?? ""
        let url = (v["url"] as? String) ?? ""
        let stars = (v["stargazerCount"] as? Int) ?? 0
        let forks = (v["forkCount"] as? Int) ?? 0
        let langObj = v["primaryLanguage"] as? [String: Any]
        let language = langObj?["name"] as? String
        let languageColorHex = langObj?["color"] as? String
        let updatedISO = (v["pushedAt"] as? String) ?? (v["updatedAt"] as? String) ?? ""
        let (updatedEpoch, updatedRelative) = parseISORelative(updatedISO)
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
            nameWithOwner
            name
            owner { login }
            description
            url
            stargazerCount
            forkCount
            pushedAt
            isFork
            isArchived
            isPrivate
            viewerHasStarred
            openGraphImageUrl
            defaultBranchRef { name }
            primaryLanguage { name color }
            repositoryTopics(first: 8) { nodes { topic { name } } }
          }
        }
      }
    }
    """

    static let trendingQuery = """
    query($q: String!) {
      search(query: $q, type: REPOSITORY, first: 50) {
        nodes {
          ... on Repository {
            nameWithOwner
            name
            owner { login }
            description
            url
            stargazerCount
            forkCount
            pushedAt
            isFork
            isArchived
            isPrivate
            viewerHasStarred
            openGraphImageUrl
            defaultBranchRef { name }
            primaryLanguage { name color }
            repositoryTopics(first: 8) { nodes { topic { name } } }
          }
        }
      }
    }
    """
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
