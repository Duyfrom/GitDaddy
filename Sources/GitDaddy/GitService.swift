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

        // 2b. Check if an `upstream` remote points at a fork parent repository
        if primaryRemote != "upstream" {
            let upstreamURLRes = await runGit(["remote", "get-url", "upstream"], in: path, timeoutSeconds: 5)
            let upstreamURL = upstreamURLRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
            if upstreamURLRes.succeeded && !upstreamURL.isEmpty && upstreamURL != (updated.remoteURL ?? "") {
                updated.forkParentURL = upstreamURL
            } else {
                updated.forkParentURL = nil
            }
        } else {
            updated.forkParentURL = nil
        }

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
            updated.pushRemote = nil
            updated.isTrackingExplicitlySet = false
            updated.incomingCommits = []
            updated.unpushedCommits = []
            updated.syncState = .emptyRepo(hasRemote: primaryRemote != nil)
            return updated
        }

        guard let remoteName = primaryRemote else {
            updated.upstreamBranch = nil
            updated.pushRemote = nil
            updated.isTrackingExplicitlySet = false
            updated.incomingCommits = []
            updated.unpushedCommits = []
            updated.syncState = .noRemoteConfigured
            return updated
        }

        if isDetached {
            updated.upstreamBranch = nil
            updated.pushRemote = nil
            updated.isTrackingExplicitlySet = false
            updated.incomingCommits = []
            updated.unpushedCommits = []
            updated.syncState = .detachedHead(commit: shortRev)
            return updated
        }

        // 6. Upstream branch, pushRemote, & ahead/behind counts
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

        // Check pushRemote (`branch.<branch>.pushRemote` or `remote.pushDefault`)
        let branchPushRes = await runGit(["config", "--get", "branch.\(updated.branch).pushRemote"], in: path, timeoutSeconds: 5)
        let defaultPushRes = await runGit(["config", "--get", "remote.pushDefault"], in: path, timeoutSeconds: 5)
        let configuredPush: String? = {
            let bp = branchPushRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
            if branchPushRes.succeeded && !bp.isEmpty { return bp }
            let dp = defaultPushRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
            if defaultPushRes.succeeded && !dp.isEmpty { return dp }
            return nil
        }()
        let upstreamRemotePart = upstreamRef?.split(separator: "/").first.map(String.init)
        if let push = configuredPush, upstreamRemotePart != push {
            updated.pushRemote = push
        } else {
            updated.pushRemote = nil
        }

        guard let targetUpstream = upstreamRef else {
            updated.incomingCommits = []
            updated.unpushedCommits = await fetchCommitsList(range: "HEAD", in: path, maxCount: 10)
            let publishRemote = updated.pushRemote ?? remoteName
            updated.syncState = .unpublishedBranch(branch: updated.branch, remote: publishRemote)
            return updated
        }

        // On a fork (`upstream` for pull, `origin` for push), `behind` is measured
        // against the tracking upstream while `ahead` is measured against the push remote's branch.
        var pushTarget = targetUpstream
        if let pushRemote = updated.pushRemote {
            let candidate = "\(pushRemote)/\(updated.branch)"
            let verifyPush = await runGit(
                ["rev-parse", "--verify", "--quiet", "refs/remotes/\(candidate)"],
                in: path,
                timeoutSeconds: 5
            )
            if verifyPush.succeeded {
                pushTarget = candidate
            }
        }

        var aheadCount = 0
        var behindCount = 0
        var countsSucceeded = false

        if pushTarget == targetUpstream {
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
                   let a = Int(tokens[0]),
                   let b = Int(tokens[1]) {
                    aheadCount = a
                    behindCount = b
                    countsSucceeded = true
                }
            }
        } else {
            let behindRes = await runGit(["rev-list", "--count", "HEAD..\(targetUpstream)"], in: path, timeoutSeconds: 8)
            let aheadRes = await runGit(["rev-list", "--count", "\(pushTarget)..HEAD"], in: path, timeoutSeconds: 8)
            if behindRes.succeeded && aheadRes.succeeded {
                behindCount = Int(behindRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines)) ?? 0
                aheadCount = Int(aheadRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines)) ?? 0
                countsSucceeded = true
            }
        }

        if countsSucceeded {
            if aheadCount > 0 && behindCount > 0 {
                updated.syncState = .diverged(ahead: aheadCount, behind: behindCount)
            } else if behindCount > 0 {
                updated.syncState = .behind(count: behindCount)
            } else if aheadCount > 0 {
                updated.syncState = .ahead(count: aheadCount)
            } else {
                updated.syncState = .upToDate
            }

            if behindCount > 0 {
                updated.incomingCommits = await fetchCommitsList(
                    range: "HEAD..\(targetUpstream)",
                    in: path
                )
            } else {
                updated.incomingCommits = []
            }

            if aheadCount > 0 {
                updated.unpushedCommits = await fetchCommitsList(
                    range: "\(pushTarget)..HEAD",
                    in: path
                )
            } else {
                updated.unpushedCommits = []
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

    /// Runs `git push` (using `pushRemote` on a fork, or setting upstream `-u origin <branch>` if needed) and refreshes repository state.
    static func pushRepository(_ repo: GitRepo) async -> (repo: GitRepo, logEntry: ActivityLogEntry) {
        let path = repo.path
        var args = ["push"]

        if let pushRemote = repo.pushRemote {
            args.append(contentsOf: [pushRemote, "HEAD"])
        } else {
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
                    let remote = repo.primaryRemoteName ?? "origin"
                    args.append(contentsOf: ["-u", remote, repo.branch])
                }
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

    // MARK: - Local Fork (Independent Copy on Disk)

    /// Suggests `<repo>-fork` (or `-fork-2`, `-fork-3`, …) next to `sourcePath`.
    static func defaultLocalForkDestination(for sourcePath: String) -> String {
        let fm = FileManager.default
        let sourceURL = URL(fileURLWithPath: sourcePath)
        let parentURL = sourceURL.deletingLastPathComponent()
        let baseName = sourceURL.lastPathComponent

        let firstCandidate = parentURL.appendingPathComponent("\(baseName)-fork").path
        if !fm.fileExists(atPath: firstCandidate) {
            return firstCandidate
        }
        for n in 2...99 {
            let candidate = parentURL.appendingPathComponent("\(baseName)-fork-\(n)").path
            if !fm.fileExists(atPath: candidate) {
                return candidate
            }
        }
        return firstCandidate
    }

    /// Creates an independent local copy of `source` at `destinationPath`.
    /// Clones via hardlinks, removes the temporary `origin` pointing at `source`,
    /// copies all local branches, tags, remote-tracking refs, remotes, and branch tracking config.
    static func forkLocally(
        source: GitRepo,
        destinationPath: String
    ) async -> (result: Result<GitRepo, Error>, logEntry: ActivityLogEntry) {
        let src = source.path
        let dest = (destinationPath as NSString).expandingTildeInPath
        let destURL = URL(fileURLWithPath: dest).standardizedFileURL
        let destPath = destURL.path
        let newName = destURL.lastPathComponent.isEmpty ? "\(source.name)-fork" : destURL.lastPathComponent
        let parentPath = destURL.deletingLastPathComponent().path

        let fm = FileManager.default
        if fm.fileExists(atPath: destPath) {
            let contents = (try? fm.contentsOfDirectory(atPath: destPath)) ?? ["non-empty"]
            if !contents.isEmpty {
                let msg = "Destination '\(destPath)' already exists and is not empty."
                let log = ActivityLogEntry(
                    timestamp: Date(),
                    repoName: newName,
                    repoPath: destPath,
                    action: "Fork Locally",
                    command: "git clone \(src) \(destPath)",
                    succeeded: false,
                    summary: msg,
                    output: msg
                )
                return (.failure(NSError(domain: "GitDaddy", code: 1, userInfo: [NSLocalizedDescriptionKey: msg])), log)
            }
        } else {
            do {
                try fm.createDirectory(atPath: parentPath, withIntermediateDirectories: true)
            } catch {
                let msg = "Could not create parent folder: \(error.localizedDescription)"
                let log = ActivityLogEntry(
                    timestamp: Date(),
                    repoName: newName,
                    repoPath: destPath,
                    action: "Fork Locally",
                    command: "mkdir -p \(parentPath)",
                    succeeded: false,
                    summary: msg,
                    output: msg
                )
                return (.failure(error), log)
            }
        }

        var transcript: [String] = []

        // 1. Clone local repository (uses hardlinks for objects on the same filesystem)
        let cloneRes = await runGit(["clone", "--quiet", src, destPath], in: parentPath, timeoutSeconds: 60)
        transcript.append("[$ \(cloneRes.commandString)]")
        if !cloneRes.combinedOutput.isEmpty {
            transcript.append(cloneRes.combinedOutput)
        }
        if !cloneRes.succeeded {
            let err = cloneRes.stderr.trimmingCharacters(in: .whitespacesAndNewlines)
            let shortErr = err.split(whereSeparator: \.isNewline).first.map(String.init) ?? "git clone failed"
            let log = ActivityLogEntry(
                timestamp: Date(),
                repoName: newName,
                repoPath: destPath,
                action: "Fork Locally",
                command: cloneRes.commandString,
                succeeded: false,
                summary: shortErr,
                output: transcript.joined(separator: "\n")
            )
            return (.failure(NSError(domain: "GitDaddy", code: 2, userInfo: [NSLocalizedDescriptionKey: shortErr])), log)
        }

        // 2. Drop the temporary `origin` pointing at the source folder
        _ = await runGit(["remote", "remove", "origin"], in: destPath, timeoutSeconds: 5)

        // 3. Copy all branches, tags, and remote-tracking refs from the source
        let fetchRefs = await runGit(
            [
                "fetch",
                "--quiet",
                "--update-head-ok",
                src,
                "+refs/heads/*:refs/heads/*",
                "+refs/tags/*:refs/tags/*",
                "+refs/remotes/*:refs/remotes/*"
            ],
            in: destPath,
            timeoutSeconds: 30
        )
        transcript.append("[$ \(fetchRefs.commandString)]")

        // 4. Re-create the source's remotes (`origin`, `upstream`, etc.) on the copy
        var copiedRemotes: [String] = []
        let remotesRes = await runGit(["remote"], in: src, timeoutSeconds: 5)
        for line in remotesRes.stdout.split(whereSeparator: \.isNewline) {
            let name = line.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !name.isEmpty else { continue }
            let urlRes = await runGit(["remote", "get-url", name], in: src, timeoutSeconds: 5)
            let url = urlRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
            guard urlRes.succeeded && !url.isEmpty else { continue }
            let addRes = await runGit(["remote", "add", name, url], in: destPath, timeoutSeconds: 5)
            if addRes.succeeded {
                copiedRemotes.append(name)
            }
            let pushURLRes = await runGit(["config", "--get", "remote.\(name).pushurl"], in: src, timeoutSeconds: 5)
            let pushURL = pushURLRes.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
            if pushURLRes.succeeded && !pushURL.isEmpty && pushURL != url {
                _ = await runGit(["remote", "set-url", "--push", name, pushURL], in: destPath, timeoutSeconds: 5)
            }
        }

        // 5. Copy branch tracking config and pushDefault so ahead/behind works immediately
        let branchCfg = await runGit(["config", "--get-regexp", #"^branch\."#], in: src, timeoutSeconds: 5)
        if branchCfg.succeeded {
            for line in branchCfg.stdout.split(whereSeparator: \.isNewline) {
                let parts = line.split(separator: " ", maxSplits: 1).map(String.init)
                if parts.count == 2 {
                    _ = await runGit(["config", parts[0], parts[1]], in: destPath, timeoutSeconds: 5)
                }
            }
        }
        let pushDefault = await runGit(["config", "--get", "remote.pushDefault"], in: src, timeoutSeconds: 5)
        let pdVal = pushDefault.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
        if pushDefault.succeeded && !pdVal.isEmpty {
            _ = await runGit(["config", "remote.pushDefault", pdVal], in: destPath, timeoutSeconds: 5)
        }

        let summary = copiedRemotes.isEmpty
            ? "Created local copy at \(destPath)"
            : "Created local copy at \(destPath) (remotes: \(copiedRemotes.joined(separator: ", ")))"
        transcript.append(summary)
        let fullOutput = transcript.joined(separator: "\n")

        var newRepo = GitRepo(name: newName, path: destPath, rootScanFolder: parentPath)
        newRepo = await inspectLocalRepository(newRepo, markUnknownIfCached: false)
        newRepo.lastCheckedAt = Date()
        newRepo.lastCommandOutput = fullOutput

        let log = ActivityLogEntry(
            timestamp: Date(),
            repoName: newName,
            repoPath: destPath,
            action: "Fork Locally",
            command: "git clone \(src) \(destPath)",
            succeeded: true,
            summary: summary,
            output: fullOutput
        )
        return (.success(newRepo), log)
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

    /// Reads the repository README file and/or description (.git/description, package.json, Cargo.toml, pyproject.toml, or README summary).
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

        // 2b. Check Cargo.toml / pyproject.toml for `description = "..."`
        if explicitDescription == nil {
            explicitDescription = tomlDescription(at: rootURL.appendingPathComponent("Cargo.toml"))
                ?? tomlDescription(at: rootURL.appendingPathComponent("pyproject.toml"))
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
                let data = (try? handle.read(upToCount: 49_152)) ?? Data()
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
                if line.hasPrefix("#") || line.hasPrefix("!") || line.hasPrefix("[!") || line.hasPrefix("<") || line.hasPrefix("```") || line.hasPrefix("---") || line.hasPrefix("|") || line.hasPrefix(">") {
                    continue
                }
                explicitDescription = line
                break
            }
        }

        return (matchedFileName, readmeText, explicitDescription)
    }

    private static func tomlDescription(at url: URL) -> String? {
        guard let text = try? String(contentsOf: url, encoding: .utf8) else { return nil }
        for rawLine in text.components(separatedBy: .newlines).prefix(80) {
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            guard line.hasPrefix("description") else { continue }
            let rest = line.dropFirst("description".count).trimmingCharacters(in: .whitespaces)
            guard rest.hasPrefix("=") else { continue }
            let val = rest.dropFirst().trimmingCharacters(in: .whitespaces)
            if val.count >= 2 && ((val.hasPrefix("\"") && val.hasSuffix("\"")) || (val.hasPrefix("'") && val.hasSuffix("'"))) {
                let inner = String(val.dropFirst().dropLast()).trimmingCharacters(in: .whitespacesAndNewlines)
                if !inner.isEmpty {
                    return inner
                }
            }
        }
        return nil
    }
}

