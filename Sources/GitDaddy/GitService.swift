import Foundation

struct CommandResult: Sendable {
    let exitCode: Int32
    let stdout: String
    let stderr: String
    let commandString: String

    var combinedOutput: String {
        let out = stdout.trimmingCharacters(in: .whitespacesAndNewlines)
        let err = stderr.trimmingCharacters(in: .whitespacesAndNewlines)
        if out.isEmpty { return err }
        if err.isEmpty { return out }
        return out + "\n" + err
    }

    var succeeded: Bool {
        exitCode == 0
    }
}

enum GitService {
    static let gitExecutablePath: String = {
        let candidates = [
            "/opt/homebrew/bin/git",
            "/usr/local/bin/git",
            "/usr/bin/git"
        ]
        for path in candidates where FileManager.default.isExecutableFile(atPath: path) {
            return path
        }
        return "/usr/bin/git"
    }()

    /// Runs a git command inside `repoPath` with non-interactive environment and a timeout.
    static func runGit(
        _ args: [String],
        in repoPath: String,
        timeoutSeconds: TimeInterval = 30
    ) async -> CommandResult {
        await Task.detached(priority: .userInitiated) {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: gitExecutablePath)
            process.arguments = args
            process.currentDirectoryURL = URL(fileURLWithPath: repoPath)

            var env = ProcessInfo.processInfo.environment
            env["GIT_TERMINAL_PROMPT"] = "0"
            env["GCM_INTERACTIVE"] = "never"
            env["LC_ALL"] = "en_US.UTF-8"
            let currentPath = env["PATH"] ?? "/usr/bin:/bin:/usr/sbin:/sbin"
            env["PATH"] = "/opt/homebrew/bin:/usr/local/bin:" + currentPath
            if env["GIT_SSH_COMMAND"] == nil {
                env["GIT_SSH_COMMAND"] = "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10"
            }
            process.environment = env

            let stdoutPipe = Pipe()
            let stderrPipe = Pipe()
            process.standardOutput = stdoutPipe
            process.standardError = stderrPipe

            let cmdDisplay = "git " + args.joined(separator: " ")

            do {
                try process.run()
            } catch {
                return CommandResult(
                    exitCode: -1,
                    stdout: "",
                    stderr: "Failed to launch git: \(error.localizedDescription)",
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

    /// Reads local Git state (branch, remote, working tree changes, cached ahead/behind) without network fetch.
    static func inspectLocalRepository(_ repo: GitRepo, markUnknownIfCached: Bool = false) async -> GitRepo {
        var updated = repo
        let path = repo.path

        // 1. Current branch & HEAD commit verification
        let branchRes = await runGit(["symbolic-ref", "--short", "-q", "HEAD"], in: path, timeoutSeconds: 5)
        let revRes = await runGit(["rev-parse", "--short", "HEAD"], in: path, timeoutSeconds: 5)
        let hasCommits = revRes.succeeded && !revRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        let shortRev = revRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines)

        var isDetached = false
        if branchRes.succeeded {
            let b = branchRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
            updated.branch = b.isEmpty ? "HEAD" : b
        } else if hasCommits {
            isDetached = true
            updated.branch = "detached@\(shortRev)"
        } else {
            updated.branch = "main (unborn)"
        }

        // 2. Remote URL (prefer origin, fallback to first remote)
        let originURLRes = await runGit(["remote", "get-url", "origin"], in: path, timeoutSeconds: 5)
        var primaryRemote: String? = nil
        if originURLRes.succeeded {
            primaryRemote = "origin"
            updated.remoteURL = originURLRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
        } else {
            let remotesRes = await runGit(["remote"], in: path, timeoutSeconds: 5)
            let firstRemote = remotesRes.stdout
                .split(whereSeparator: \.isNewline)
                .map(String.init)
                .first
            if let firstRemote, !firstRemote.isEmpty {
                primaryRemote = firstRemote
                let urlRes = await runGit(["remote", "get-url", firstRemote], in: path, timeoutSeconds: 5)
                updated.remoteURL = urlRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
            } else {
                updated.remoteURL = nil
            }
        }
        updated.primaryRemoteName = primaryRemote

        // 3. Working tree status (porcelain)
        let statusRes = await runGit(["status", "--porcelain"], in: path, timeoutSeconds: 8)
        if statusRes.succeeded {
            var changes: [FileChange] = []
            for line in statusRes.stdout.split(whereSeparator: \.isNewline) {
                guard line.count >= 3 else { continue }
                let statusPrefix = String(line.prefix(2)).trimmingCharacters(in: .whitespaces)
                let filePath = String(line.dropFirst(3))
                changes.append(FileChange(status: statusPrefix.isEmpty ? "M" : statusPrefix, path: filePath))
            }
            updated.localChanges = changes
        }

        // 4. Last commit info on HEAD (if commits exist)
        if hasCommits {
            let logRes = await runGit(
                ["log", "-1", "--format=%h%x1f%s%x1f%cr%x1f%ct"],
                in: path,
                timeoutSeconds: 5
            )
            if logRes.succeeded {
                let parts = logRes.stdout
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                    .components(separatedBy: "\u{1f}")
                if parts.count >= 4 {
                    updated.lastCommitHash = parts[0]
                    updated.lastCommitMessage = parts[1]
                    updated.lastCommitRelativeDate = parts[2]
                    if let epoch = TimeInterval(parts[3]) {
                        updated.lastCommitTimestamp = Date(timeIntervalSince1970: epoch)
                    }
                }
            }
        } else {
            updated.lastCommitHash = ""
            updated.lastCommitMessage = ""
            updated.lastCommitRelativeDate = ""
            updated.lastCommitTimestamp = nil
        }

        // 4b. Load README file & repository description
        let readmeInfo = loadReadmeAndDescription(in: path)
        updated.readmeFileName = readmeInfo.fileName
        updated.readmeContent = readmeInfo.content
        updated.repoDescription = readmeInfo.description

        // 5. Classify special states before upstream comparison
        if !hasCommits {
            updated.upstreamBranch = nil
            updated.isTrackingExplicitlySet = false
            updated.incomingCommits = []
            updated.unpushedCommits = []
            updated.syncState = .emptyRepo(hasRemote: primaryRemote != nil)
            return updated
        }

        guard let remoteName = primaryRemote else {
            updated.upstreamBranch = nil
            updated.isTrackingExplicitlySet = false
            updated.incomingCommits = []
            updated.unpushedCommits = []
            updated.syncState = .noRemoteConfigured
            return updated
        }

        if isDetached {
            updated.upstreamBranch = nil
            updated.isTrackingExplicitlySet = false
            updated.incomingCommits = []
            updated.unpushedCommits = []
            updated.syncState = .detachedHead(commit: shortRev)
            return updated
        }

        // 6. Upstream branch & ahead/behind counts
        var upstreamRef: String? = nil
        let upstreamRes = await runGit(
            ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
            in: path,
            timeoutSeconds: 5
        )
        if upstreamRes.succeeded {
            let candidate = upstreamRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
            if !candidate.isEmpty {
                upstreamRef = candidate
                updated.isTrackingExplicitlySet = true
            }
        } else {
            updated.isTrackingExplicitlySet = false
            let fallbackCandidate = "\(remoteName)/\(updated.branch)"
            let verifyRes = await runGit(
                ["rev-parse", "--verify", "--quiet", "refs/remotes/\(fallbackCandidate)"],
                in: path,
                timeoutSeconds: 5
            )
            if verifyRes.succeeded {
                upstreamRef = fallbackCandidate
            }
        }

        updated.upstreamBranch = upstreamRef

        guard let targetUpstream = upstreamRef else {
            updated.incomingCommits = []
            updated.unpushedCommits = await fetchCommitsList(range: "HEAD", in: path, maxCount: 10)
            updated.syncState = .unpublishedBranch(branch: updated.branch, remote: remoteName)
            return updated
        }

        // Compute ahead / behind counts: left is HEAD (ahead), right is upstream (behind)
        let countsRes = await runGit(
            ["rev-list", "--left-right", "--count", "HEAD...\(targetUpstream)"],
            in: path,
            timeoutSeconds: 8
        )

        if countsRes.succeeded {
            let tokens = countsRes.stdout
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .split(whereSeparator: \.isWhitespace)
            if tokens.count == 2,
               let ahead = Int(tokens[0]),
               let behind = Int(tokens[1]) {
                if ahead > 0 && behind > 0 {
                    updated.syncState = .diverged(ahead: ahead, behind: behind)
                } else if behind > 0 {
                    updated.syncState = .behind(count: behind)
                } else if ahead > 0 {
                    updated.syncState = .ahead(count: ahead)
                } else {
                    updated.syncState = .upToDate
                }

                // Fetch incoming commit details if behind > 0
                if behind > 0 {
                    updated.incomingCommits = await fetchCommitsList(
                        range: "HEAD..\(targetUpstream)",
                        in: path
                    )
                } else {
                    updated.incomingCommits = []
                }

                // Fetch unpushed commit details if ahead > 0
                if ahead > 0 {
                    updated.unpushedCommits = await fetchCommitsList(
                        range: "\(targetUpstream)..HEAD",
                        in: path
                    )
                } else {
                    updated.unpushedCommits = []
                }
            }
        }

        return updated
    }

    /// Fetches from remote (`git fetch --all --prune`) and compares local vs remote branches.
    static func checkRemoteStatus(for repo: GitRepo) async -> (repo: GitRepo, logEntry: ActivityLogEntry) {
        let path = repo.path

        // First verify if a remote exists
        let remotesRes = await runGit(["remote"], in: path, timeoutSeconds: 5)
        let hasRemote = !remotesRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty

        if !hasRemote {
            var inspected = await inspectLocalRepository(repo, markUnknownIfCached: false)
            inspected.lastCheckedAt = Date()
            let log = ActivityLogEntry(
                timestamp: Date(),
                repoName: repo.name,
                repoPath: repo.path,
                action: "Check Remote",
                command: "git remote",
                succeeded: true,
                summary: inspected.syncState.label,
                output: "Local-only repository: no remote server URL (e.g. 'origin') is configured."
            )
            return (inspected, log)
        }

        let fetchRes = await runGit(
            ["fetch", "--all", "--prune", "--quiet"],
            in: path,
            timeoutSeconds: 30
        )

        var inspected = await inspectLocalRepository(repo, markUnknownIfCached: false)
        inspected.lastCheckedAt = Date()
        inspected.lastCommandOutput = fetchRes.combinedOutput.isEmpty
            ? "[$ \(fetchRes.commandString)]\nFetch completed successfully."
            : "[$ \(fetchRes.commandString)]\n\(fetchRes.combinedOutput)"

        if !fetchRes.succeeded {
            let errText = fetchRes.stderr.trimmingCharacters(in: .whitespacesAndNewlines)
            let shortErr = errText.split(whereSeparator: \.isNewline).first.map(String.init) ?? "Failed to fetch remote"
            inspected.syncState = .error(shortErr)
            let log = ActivityLogEntry(
                timestamp: Date(),
                repoName: repo.name,
                repoPath: repo.path,
                action: "Check Remote",
                command: fetchRes.commandString,
                succeeded: false,
                summary: shortErr,
                output: fetchRes.combinedOutput
            )
            return (inspected, log)
        }

        let log = ActivityLogEntry(
            timestamp: Date(),
            repoName: repo.name,
            repoPath: repo.path,
            action: "Check Remote",
            command: fetchRes.commandString,
            succeeded: true,
            summary: inspected.syncState.label,
            output: inspected.lastCommandOutput
        )
        return (inspected, log)
    }

    /// Runs `git pull` with the chosen strategy and optional auto-stash, then refreshes repository state.
    static func pullRepository(
        _ repo: GitRepo,
        strategy: PullStrategy,
        autoStash: Bool
    ) async -> (repo: GitRepo, logEntry: ActivityLogEntry) {
        let path = repo.path
        var args = strategy.gitArguments
        if autoStash {
            args.append("--autostash")
        }

        // If no upstream tracking branch is explicitly configured on the branch, specify origin <branch>
        let upstreamCheck = await runGit(
            ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
            in: path,
            timeoutSeconds: 5
        )
        if !upstreamCheck.succeeded,
           let upstream = repo.upstreamBranch,
           upstream.contains("/") {
            let parts = upstream.split(separator: "/", maxSplits: 1).map(String.init)
            if parts.count == 2 {
                args.append(contentsOf: [parts[0], parts[1]])
            }
        }

        let pullRes = await runGit(args, in: path, timeoutSeconds: 45)
        var inspected = await inspectLocalRepository(repo, markUnknownIfCached: false)
        inspected.lastCheckedAt = Date()

        let fullOutput = pullRes.combinedOutput.isEmpty
            ? "[$ \(pullRes.commandString)]\nAlready up to date."
            : "[$ \(pullRes.commandString)]\n\(pullRes.combinedOutput)"
        inspected.lastCommandOutput = fullOutput

        if pullRes.succeeded {
            let firstLine = pullRes.stdout
                .split(whereSeparator: \.isNewline)
                .last
                .map(String.init) ?? "Pulled successfully"
            inspected.lastPullMessage = firstLine
            let log = ActivityLogEntry(
                timestamp: Date(),
                repoName: repo.name,
                repoPath: repo.path,
                action: "Git Pull",
                command: pullRes.commandString,
                succeeded: true,
                summary: firstLine,
                output: fullOutput
            )
            return (inspected, log)
        } else {
            let errText = pullRes.stderr.trimmingCharacters(in: .whitespacesAndNewlines)
            let shortErr = errText.split(whereSeparator: \.isNewline).first.map(String.init) ?? "Git pull failed"
            inspected.syncState = .error(shortErr)
            let log = ActivityLogEntry(
                timestamp: Date(),
                repoName: repo.name,
                repoPath: repo.path,
                action: "Git Pull",
                command: pullRes.commandString,
                succeeded: false,
                summary: shortErr,
                output: fullOutput
            )
            return (inspected, log)
        }
    }

    /// Runs `git push` (setting upstream `-u origin <branch>` if needed) and refreshes repository state.
    static func pushRepository(_ repo: GitRepo) async -> (repo: GitRepo, logEntry: ActivityLogEntry) {
        let path = repo.path
        var args = ["push"]

        let upstreamCheck = await runGit(
            ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
            in: path,
            timeoutSeconds: 5
        )
        if !upstreamCheck.succeeded {
            if let upstream = repo.upstreamBranch, upstream.contains("/") {
                let parts = upstream.split(separator: "/", maxSplits: 1).map(String.init)
                if parts.count == 2 {
                    args.append(contentsOf: ["-u", parts[0], parts[1]])
                }
            } else if !repo.branch.isEmpty && !repo.branch.hasPrefix("detached@") && repo.branch != "Unborn" {
                args.append(contentsOf: ["-u", "origin", repo.branch])
            }
        }

        let pushRes = await runGit(args, in: path, timeoutSeconds: 45)
        var inspected = await inspectLocalRepository(repo, markUnknownIfCached: false)
        inspected.lastCheckedAt = Date()

        let fullOutput = pushRes.combinedOutput.isEmpty
            ? "[$ \(pushRes.commandString)]\nEverything up-to-date."
            : "[$ \(pushRes.commandString)]\n\(pushRes.combinedOutput)"
        inspected.lastCommandOutput = fullOutput

        if pushRes.succeeded {
            let summaryLine = pushRes.combinedOutput
                .split(whereSeparator: \.isNewline)
                .last
                .map(String.init) ?? "Pushed commits to remote"
            let log = ActivityLogEntry(
                timestamp: Date(),
                repoName: repo.name,
                repoPath: repo.path,
                action: "Git Push",
                command: pushRes.commandString,
                succeeded: true,
                summary: summaryLine,
                output: fullOutput
            )
            return (inspected, log)
        } else {
            let errText = pushRes.stderr.trimmingCharacters(in: .whitespacesAndNewlines)
            let shortErr = errText.split(whereSeparator: \.isNewline).first.map(String.init) ?? "Git push failed"
            inspected.syncState = .error(shortErr)
            let log = ActivityLogEntry(
                timestamp: Date(),
                repoName: repo.name,
                repoPath: repo.path,
                action: "Git Push",
                command: pushRes.commandString,
                succeeded: false,
                summary: shortErr,
                output: fullOutput
            )
            return (inspected, log)
        }
    }

    /// Stages all local changes (`git add -A`), commits with `message`, and pushes to the remote.
    static func commitAndPushRepository(
        _ repo: GitRepo,
        message: String
    ) async -> (repo: GitRepo, logEntry: ActivityLogEntry) {
        let path = repo.path
        let addRes = await runGit(["add", "-A"], in: path, timeoutSeconds: 15)
        if !addRes.succeeded {
            var inspected = await inspectLocalRepository(repo, markUnknownIfCached: false)
            let err = addRes.stderr.trimmingCharacters(in: .whitespacesAndNewlines)
            inspected.syncState = .error("git add failed: \(err)")
            inspected.lastCommandOutput = "[$ \(addRes.commandString)]\n\(addRes.combinedOutput)"
            let log = ActivityLogEntry(
                timestamp: Date(),
                repoName: repo.name,
                repoPath: repo.path,
                action: "Commit & Push",
                command: addRes.commandString,
                succeeded: false,
                summary: "Failed to stage files",
                output: inspected.lastCommandOutput
            )
            return (inspected, log)
        }

        let commitRes = await runGit(["commit", "-m", message], in: path, timeoutSeconds: 20)
        if !commitRes.succeeded {
            var inspected = await inspectLocalRepository(repo, markUnknownIfCached: false)
            let err = commitRes.combinedOutput.trimmingCharacters(in: .whitespacesAndNewlines)
            let shortErr = err.split(whereSeparator: \.isNewline).first.map(String.init) ?? "git commit failed"
            inspected.syncState = .error(shortErr)
            inspected.lastCommandOutput = "[$ \(commitRes.commandString)]\n\(commitRes.combinedOutput)"
            let log = ActivityLogEntry(
                timestamp: Date(),
                repoName: repo.name,
                repoPath: repo.path,
                action: "Commit & Push",
                command: commitRes.commandString,
                succeeded: false,
                summary: shortErr,
                output: inspected.lastCommandOutput
            )
            return (inspected, log)
        }

        if repo.remoteURL == nil {
            var inspected = await inspectLocalRepository(repo, markUnknownIfCached: false)
            inspected.lastCommandOutput = "[$ \(commitRes.commandString)]\n\(commitRes.combinedOutput)"
            let log = ActivityLogEntry(
                timestamp: Date(),
                repoName: repo.name,
                repoPath: repo.path,
                action: "Git Commit",
                command: commitRes.commandString,
                succeeded: true,
                summary: "Committed locally: \(message)",
                output: inspected.lastCommandOutput
            )
            return (inspected, log)
        }

        let (pushedRepo, pushLog) = await pushRepository(repo)
        var finalRepo = pushedRepo
        let combinedOut = "[$ \(commitRes.commandString)]\n\(commitRes.combinedOutput)\n\n\(pushedRepo.lastCommandOutput)"
        finalRepo.lastCommandOutput = combinedOut

        let log = ActivityLogEntry(
            timestamp: Date(),
            repoName: repo.name,
            repoPath: repo.path,
            action: "Commit & Push",
            command: "git add -A && git commit && git push",
            succeeded: pushLog.succeeded,
            summary: pushLog.succeeded ? "Committed & pushed: \(message)" : pushLog.summary,
            output: combinedOut
        )
        return (finalRepo, log)
    }

    /// Configures `origin` remote URL on a local-only repository and fetches from it.
    static func addRemoteOrigin(
        _ remoteURL: String,
        to repo: GitRepo
    ) async -> (repo: GitRepo, logEntry: ActivityLogEntry) {
        let path = repo.path
        let addRemoteRes = await runGit(["remote", "add", "origin", remoteURL], in: path, timeoutSeconds: 10)
        if !addRemoteRes.succeeded {
            var inspected = await inspectLocalRepository(repo, markUnknownIfCached: false)
            let err = addRemoteRes.combinedOutput.trimmingCharacters(in: .whitespacesAndNewlines)
            inspected.syncState = .error(err)
            inspected.lastCommandOutput = "[$ \(addRemoteRes.commandString)]\n\(err)"
            let log = ActivityLogEntry(
                timestamp: Date(),
                repoName: repo.name,
                repoPath: repo.path,
                action: "Add Remote",
                command: addRemoteRes.commandString,
                succeeded: false,
                summary: err,
                output: inspected.lastCommandOutput
            )
            return (inspected, log)
        }

        return await checkRemoteStatus(for: repo)
    }

    private static func fetchCommitsList(
        range: String,
        in repoPath: String,
        maxCount: Int = 20
    ) async -> [CommitInfo] {
        let res = await runGit(
            ["log", range, "-n", "\(maxCount)", "--format=%h%x1f%s%x1f%an%x1f%cr"],
            in: repoPath,
            timeoutSeconds: 6
        )
        guard res.succeeded else { return [] }

        var commits: [CommitInfo] = []
        for line in res.stdout.split(whereSeparator: \.isNewline) {
            let parts = String(line).components(separatedBy: "\u{1f}")
            if parts.count >= 4 {
                commits.append(
                    CommitInfo(
                        hash: parts[0],
                        subject: parts[1],
                        author: parts[2],
                        relativeDate: parts[3]
                    )
                )
            }
        }
        return commits
    }

    /// Reads the repository README file and/or description (.git/description, package.json, or README summary).
    private static func loadReadmeAndDescription(
        in repoPath: String
    ) -> (fileName: String?, content: String?, description: String?) {
        let fm = FileManager.default
        let rootURL = URL(fileURLWithPath: repoPath)

        // 1. Check .git/description
        var explicitDescription: String? = nil
        let gitDescURL = rootURL.appendingPathComponent(".git/description")
        if let rawDesc = try? String(contentsOf: gitDescURL, encoding: .utf8) {
            let trimmed = rawDesc.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty,
               !trimmed.lowercased().hasPrefix("unnamed repository") {
                explicitDescription = trimmed
            }
        }

        // 2. Check package.json for description if not in .git/description
        if explicitDescription == nil {
            let pkgURL = rootURL.appendingPathComponent("package.json")
            if let data = try? Data(contentsOf: pkgURL),
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let desc = json["description"] as? String {
                let trimmed = desc.trimmingCharacters(in: .whitespacesAndNewlines)
                if !trimmed.isEmpty {
                    explicitDescription = trimmed
                }
            }
        }

        // 3. Locate README file in repository root
        let preferredNames = [
            "README.md",
            "readme.md",
            "Readme.md",
            "README.markdown",
            "README.txt",
            "README.rst",
            "README"
        ]

        var matchedFileName: String? = nil
        for candidate in preferredNames {
            let candidateURL = rootURL.appendingPathComponent(candidate)
            var isDir: ObjCBool = false
            if fm.fileExists(atPath: candidateURL.path, isDirectory: &isDir), !isDir.boolValue {
                matchedFileName = candidate
                break
            }
        }

        if matchedFileName == nil,
           let entries = try? fm.contentsOfDirectory(atPath: repoPath) {
            matchedFileName = entries.first { entry in
                let lower = entry.lowercased()
                guard lower.hasPrefix("readme") else { return false }
                var isDir: ObjCBool = false
                let fullPath = rootURL.appendingPathComponent(entry).path
                return fm.fileExists(atPath: fullPath, isDirectory: &isDir) && !isDir.boolValue
            }
        }

        var readmeText: String? = nil
        if let fileName = matchedFileName {
            let fileURL = rootURL.appendingPathComponent(fileName)
            if let handle = try? FileHandle(forReadingFrom: fileURL) {
                let data = (try? handle.read(upToCount: 24_576)) ?? Data()
                try? handle.close()
                if let str = String(data: data, encoding: .utf8) {
                    let trimmed = str.trimmingCharacters(in: .whitespacesAndNewlines)
                    if !trimmed.isEmpty {
                        readmeText = trimmed
                    }
                }
            }
        }

        // 4. Derive a short description from the first prose line of README if no explicit description exists
        if explicitDescription == nil, let readmeText {
            for rawLine in readmeText.components(separatedBy: .newlines) {
                let line = rawLine.trimmingCharacters(in: .whitespaces)
                if line.isEmpty { continue }
                if line.hasPrefix("#") || line.hasPrefix("!") || line.hasPrefix("[!") || line.hasPrefix("<") || line.hasPrefix("```") || line.hasPrefix("---") {
                    continue
                }
                explicitDescription = line
                break
            }
        }

        return (matchedFileName, readmeText, explicitDescription)
    }
}
