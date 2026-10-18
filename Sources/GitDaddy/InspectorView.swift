import SwiftUI

struct InspectorView: View {
    @ObservedObject var store: RepoStore
    let repo: GitRepo

    @State private var commitMessage: String = ""
    @State private var newRemoteURL: String = ""
    @State private var showDeleteConfirm: Bool = false
    @State private var isReadmeExpanded: Bool = true

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Fixed Header & Sync Controls (never scroll away)
            headerSection
            Divider().overlay(Theme.border)
            syncStatusSection
            Divider().overlay(Theme.border)

            // Scrollable data region for README/description, commits, dirty files, and console output
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.space6) {
                    readmeAndDescriptionSection

                    if !repo.incomingCommits.isEmpty {
                        commitsSection(
                            title: "Incoming Updates",
                            count: repo.incomingCommits.count,
                            commits: repo.incomingCommits
                        )
                    }

                    if !repo.unpushedCommits.isEmpty {
                        commitsSection(
                            title: repo.syncState.isUnpublishedBranch ? "Local Branch Commits" : "Unpushed Commits",
                            count: repo.unpushedCommits.count,
                            commits: repo.unpushedCommits
                        )
                    }

                    if !repo.localChanges.isEmpty {
                        localChangesSection
                    }

                    if !repo.lastCommandOutput.isEmpty {
                        consoleOutputSection
                    }
                }
                .padding(Theme.space4)
            }
        }
        .background(Theme.surface1)
        .confirmationDialog(
            "Delete Local Repository \"\(repo.name)\"?",
            isPresented: $showDeleteConfirm,
            titleVisibility: .visible
        ) {
            Button("Move to Trash") {
                store.deleteRepositories(ids: [repo.id], moveToTrash: true)
            }
            Button("Delete Permanently", role: .destructive) {
                store.deleteRepositories(ids: [repo.id], moveToTrash: false)
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(deleteWarningMessage)
        }
    }

    private var deleteWarningMessage: String {
        var msg = "Folder: \(repo.displayPath)\nThis removes the repository from your Mac (the remote repository is not affected)."
        if repo.syncState.aheadCount > 0 || repo.syncState.isUnpublishedBranch || repo.isDirty {
            var warnings: [String] = []
            if repo.syncState.aheadCount > 0 {
                warnings.append("\(repo.syncState.aheadCount) unpushed commit(s)")
            } else if repo.syncState.isUnpublishedBranch {
                warnings.append("an unpublished local branch (\(repo.branch))")
            }
            if repo.isDirty {
                warnings.append("\(repo.localChanges.count) uncommitted file change(s)")
            }
            msg += "\n\nWarning: This repository has " + warnings.joined(separator: " and ") + " that have not been pushed to the remote."
        }
        return msg
    }

    // MARK: - Fixed Top Header

    private var headerSection: some View {
        VStack(alignment: .leading, spacing: Theme.space3) {
            HStack(alignment: .center, spacing: Theme.space2) {
                VStack(alignment: .leading, spacing: Theme.space1) {
                    Text(repo.name)
                        .font(Theme.fontTitle(weight: .medium))
                        .tracking(Theme.trackingTitle)
                        .foregroundStyle(Theme.textPrimary)
                        .lineLimit(1)

                    Text(repo.displayPath)
                        .font(Theme.fontMeta(monospaced: true))
                        .foregroundStyle(Theme.textTertiary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                }

                Spacer(minLength: Theme.space2)

                // Ghost icon actions (28px square, radius 6px, hover surface2)
                HStack(spacing: Theme.space1) {
                    RestyleIconButton(
                        icon: "folder",
                        tooltip: "Reveal in Finder"
                    ) {
                        store.openInFinder(repo.path)
                    }

                    RestyleIconButton(
                        icon: "terminal",
                        tooltip: "Open in Terminal"
                    ) {
                        store.openInTerminal(repo.path)
                    }

                    RestyleIconButton(
                        icon: "doc.on.doc",
                        tooltip: "Copy Repository Path"
                    ) {
                        store.copyPathToClipboard(repo.path)
                    }

                    RestyleIconButton(
                        icon: "trash",
                        tooltip: "Delete Local Repository",
                        isDisabled: repo.syncState.isBusy
                    ) {
                        showDeleteConfirm = true
                    }
                }
            }

            // Compact metadata rows (28px height)
            VStack(spacing: 0) {
                metaRow(
                    label: "Branch",
                    value: repo.upstreamBranch != nil
                        ? "\(repo.branch)  →  \(repo.upstreamBranch!)"
                        : repo.branch,
                    monospaced: true
                )

                Divider().overlay(Theme.border)

                metaRow(
                    label: "Remote",
                    value: repo.formattedRemoteHost ?? "None (local only)",
                    monospaced: repo.formattedRemoteHost != nil
                )

                Divider().overlay(Theme.border)

                metaRow(
                    label: "HEAD",
                    value: repo.lastCommitHash.isEmpty
                        ? "No commits yet"
                        : "\(repo.lastCommitHash)  \(repo.lastCommitMessage)",
                    trailingMeta: repo.lastCommitRelativeDate,
                    monospaced: false
                )
            }
            .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radiusContainer)
                    .strokeBorder(Theme.border, lineWidth: 1)
            )
        }
        .padding(Theme.space4)
    }

    private func metaRow(
        label: String,
        value: String,
        trailingMeta: String = "",
        monospaced: Bool
    ) -> some View {
        HStack(spacing: Theme.space2) {
            Text(label)
                .font(Theme.fontMeta(weight: .regular))
                .tracking(Theme.trackingBody)
                .foregroundStyle(Theme.textTertiary)
                .frame(width: 56, alignment: .leading)

            Text(value)
                .font(monospaced ? Theme.fontMeta(weight: .regular, monospaced: true) : Theme.fontMeta(weight: .regular))
                .tracking(Theme.trackingBody)
                .foregroundStyle(Theme.textPrimary)
                .lineLimit(1)
                .truncationMode(.middle)

            Spacer(minLength: Theme.space1)

            if !trailingMeta.isEmpty {
                Text(trailingMeta)
                    .font(Theme.fontMeta(weight: .regular))
                    .foregroundStyle(Theme.textTertiary)
                    .lineLimit(1)
            }
        }
        .padding(.horizontal, Theme.space3)
        .frame(height: Theme.rowCompact)
    }

    // MARK: - Sync Status & Actions Section

    private var syncStatusSection: some View {
        VStack(alignment: .leading, spacing: Theme.space3) {
            HStack(spacing: Theme.space2) {
                StatusIcon(state: repo.syncState, compact: false, showLabel: true, mutedNormal: false)

                Spacer()

                if let checked = repo.lastCheckedAt {
                    Text(checked.formatted(date: .omitted, time: .shortened))
                        .font(Theme.fontMeta())
                        .monospacedDigit()
                        .foregroundStyle(Theme.textTertiary)
                }
            }

            Text(repo.syncState.inspectorExplanation)
                .font(Theme.fontMeta())
                .tracking(Theme.trackingBody)
                .foregroundStyle(Theme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)

            if repo.remoteURL == nil {
                HStack(spacing: Theme.space2) {
                    TextField("https://github.com/user/repo.git", text: $newRemoteURL)
                        .textFieldStyle(.plain)
                        .font(Theme.fontMeta(monospaced: true))
                        .foregroundStyle(Theme.textPrimary)
                        .padding(.horizontal, Theme.space3)
                        .frame(height: Theme.rowHeight)
                        .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
                        .overlay(
                            RoundedRectangle(cornerRadius: Theme.radiusControl)
                                .strokeBorder(Theme.borderStrong, lineWidth: 1)
                        )

                    RestyleButton(
                        label: "Add Remote",
                        icon: "link",
                        variant: .primary,
                        isDisabled: newRemoteURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || repo.syncState.isBusy
                    ) {
                        let url = newRemoteURL.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !url.isEmpty else { return }
                        newRemoteURL = ""
                        Task {
                            await store.addRemoteOrigin(to: repo.id, url: url)
                        }
                    }
                }
            }

            HStack(spacing: Theme.space2) {
                RestyleButton(
                    label: "Check",
                    icon: "arrow.triangle.2.circlepath",
                    variant: .secondary,
                    isDisabled: repo.syncState.isBusy || repo.remoteURL == nil
                ) {
                    Task {
                        await store.checkSingleRepo(repo.id)
                    }
                }

                RestyleButton(
                    label: repo.syncState.behindCount > 0 ? "Pull (\(repo.syncState.behindCount))" : "Pull",
                    icon: "arrow.down",
                    variant: repo.syncState.needsUpdate ? .primary : .secondary,
                    isDisabled: !repo.syncState.canPull
                ) {
                    Task {
                        await store.pullSingleRepo(repo.id)
                    }
                }

                RestyleButton(
                    label: repo.syncState.isUnpublishedBranch
                        ? "Publish"
                        : (repo.syncState.aheadCount > 0 ? "Push (\(repo.syncState.aheadCount))" : "Push"),
                    icon: "arrow.up",
                    variant: (!repo.syncState.needsUpdate && repo.syncState.canPush) ? .primary : .secondary,
                    isDisabled: repo.syncState.isBusy || repo.remoteURL == nil || repo.lastCommitHash.isEmpty
                ) {
                    Task {
                        await store.pushSingleRepo(repo.id)
                    }
                }
            }
        }
        .padding(Theme.space4)
    }

    // MARK: - Commits Section (Flat Rows separated by 1px borders)

    private func commitsSection(
        title: String,
        count: Int,
        commits: [CommitInfo]
    ) -> some View {
        VStack(alignment: .leading, spacing: Theme.space2) {
            HStack(spacing: Theme.space2) {
                Text(title)
                    .font(Theme.fontMeta(weight: .medium))
                    .tracking(Theme.trackingBody)
                    .foregroundStyle(Theme.textSecondary)

                Text("\(count)")
                    .font(Theme.fontMeta(weight: .regular))
                    .monospacedDigit()
                    .foregroundStyle(Theme.textTertiary)

                Spacer()
            }

            VStack(spacing: 0) {
                ForEach(Array(commits.enumerated()), id: \.element.id) { index, commit in
                    HStack(spacing: Theme.space2) {
                        Text(commit.hash)
                            .font(Theme.fontMeta(weight: .regular, monospaced: true))
                            .foregroundStyle(Theme.textSecondary)
                            .frame(width: 56, alignment: .leading)

                        Text(commit.subject)
                            .font(Theme.fontBody(weight: .regular))
                            .tracking(Theme.trackingBody)
                            .foregroundStyle(Theme.textPrimary)
                            .lineLimit(1)
                            .truncationMode(.tail)

                        Spacer(minLength: Theme.space2)

                        Text(commit.relativeDate)
                            .font(Theme.fontMeta())
                            .foregroundStyle(Theme.textTertiary)
                            .lineLimit(1)
                    }
                    .padding(.horizontal, Theme.space3)
                    .frame(height: Theme.rowCompact)

                    if index < commits.count - 1 {
                        Divider().overlay(Theme.border)
                    }
                }
            }
            .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radiusContainer)
                    .strokeBorder(Theme.border, lineWidth: 1)
            )
        }
    }

    // MARK: - Uncommitted Changes Section

    private var localChangesSection: some View {
        VStack(alignment: .leading, spacing: Theme.space2) {
            HStack(spacing: Theme.space2) {
                Text("Uncommitted Changes")
                    .font(Theme.fontMeta(weight: .medium))
                    .tracking(Theme.trackingBody)
                    .foregroundStyle(Theme.textSecondary)

                Text("\(repo.localChanges.count)")
                    .font(Theme.fontMeta(weight: .regular))
                    .monospacedDigit()
                    .foregroundStyle(Theme.textTertiary)

                Spacer()

                if store.autoStashOnPull {
                    Text("Auto-stash active")
                        .font(Theme.fontMeta())
                        .foregroundStyle(Theme.textTertiary)
                }
            }

            HStack(spacing: Theme.space2) {
                TextField(
                    repo.remoteURL != nil ? "Commit message to stage & push…" : "Commit message…",
                    text: $commitMessage
                )
                .textFieldStyle(.plain)
                .font(Theme.fontBody())
                .foregroundStyle(Theme.textPrimary)
                .padding(.horizontal, Theme.space3)
                .frame(height: Theme.rowHeight)
                .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.radiusControl)
                        .strokeBorder(Theme.borderStrong, lineWidth: 1)
                )

                RestyleButton(
                    label: repo.remoteURL != nil ? "Commit & Push" : "Commit",
                    variant: .secondary,
                    isDisabled: commitMessage.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || repo.syncState.isBusy
                ) {
                    let msg = commitMessage.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !msg.isEmpty else { return }
                    commitMessage = ""
                    Task {
                        await store.commitAndPushSingleRepo(repo.id, message: msg)
                    }
                }
            }

            VStack(spacing: 0) {
                let visibleChanges = Array(repo.localChanges.prefix(30))
                ForEach(Array(visibleChanges.enumerated()), id: \.element.id) { index, change in
                    HStack(spacing: Theme.space2) {
                        Text(change.status)
                            .font(Theme.fontMeta(weight: .medium, monospaced: true))
                            .foregroundStyle(Theme.textSecondary)
                            .frame(width: 24, alignment: .leading)

                        Text(change.path)
                            .font(Theme.fontMeta(monospaced: true))
                            .foregroundStyle(Theme.textPrimary)
                            .lineLimit(1)
                            .truncationMode(.middle)

                        Spacer()
                    }
                    .padding(.horizontal, Theme.space3)
                    .frame(height: Theme.rowCompact)

                    if index < visibleChanges.count - 1 {
                        Divider().overlay(Theme.border)
                    }
                }
            }
            .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radiusContainer)
                    .strokeBorder(Theme.border, lineWidth: 1)
            )
        }
    }

    // MARK: - Collapsible README & Repository Description Section

    private var readmeAndDescriptionSection: some View {
        VStack(alignment: .leading, spacing: Theme.space2) {
            Button {
                isReadmeExpanded.toggle()
            } label: {
                HStack(spacing: Theme.space2) {
                    Image(systemName: isReadmeExpanded ? "chevron.down" : "chevron.right")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(Theme.textSecondary)
                        .frame(width: 12)

                    Image(systemName: "doc.text")
                        .font(.system(size: 12, weight: .regular))
                        .foregroundStyle(Theme.textSecondary)

                    Text(repo.readmeFileName ?? "README & Description")
                        .font(Theme.fontMeta(weight: .medium))
                        .tracking(Theme.trackingBody)
                        .foregroundStyle(Theme.textSecondary)

                    Spacer()

                    Text(isReadmeExpanded ? "Hide" : "Show")
                        .font(Theme.fontMeta())
                        .foregroundStyle(Theme.textTertiary)
                }
                .padding(.horizontal, Theme.space2)
                .frame(height: Theme.rowCompact)
                .background(Theme.surface2.opacity(0.6), in: RoundedRectangle(cornerRadius: Theme.radiusControl))
                .contentShape(RoundedRectangle(cornerRadius: Theme.radiusControl))
            }
            .buttonStyle(.plain)

            if isReadmeExpanded {
                VStack(alignment: .leading, spacing: Theme.space3) {
                    if let desc = repo.repoDescription, !desc.isEmpty {
                        VStack(alignment: .leading, spacing: Theme.space1) {
                            Text("Description")
                                .font(Theme.fontMeta(weight: .medium))
                                .foregroundStyle(Theme.textTertiary)

                            Text(desc)
                                .font(Theme.fontBody())
                                .tracking(Theme.trackingBody)
                                .foregroundStyle(Theme.textPrimary)
                                .fixedSize(horizontal: false, vertical: true)
                                .textSelection(.enabled)
                        }
                    }

                    if let readme = repo.readmeContent, !readme.isEmpty {
                        if repo.repoDescription != nil {
                            Divider().overlay(Theme.border)
                        }
                        ReadmeMarkdownView(markdown: readme)
                    } else if repo.repoDescription == nil {
                        Text("No README file or description found in this repository.")
                            .font(Theme.fontMeta())
                            .foregroundStyle(Theme.textTertiary)
                    }
                }
                .padding(Theme.space3)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.radiusContainer)
                        .strokeBorder(Theme.border, lineWidth: 1)
                )
            } else if let desc = repo.repoDescription, !desc.isEmpty {
                Text(desc)
                    .font(Theme.fontMeta())
                    .tracking(Theme.trackingBody)
                    .foregroundStyle(Theme.textSecondary)
                    .lineLimit(2)
                    .padding(.horizontal, Theme.space2)
            }
        }
    }

    // MARK: - Console Output Section

    private var consoleOutputSection: some View {
        VStack(alignment: .leading, spacing: Theme.space2) {
            Text("Git Output")
                .font(Theme.fontMeta(weight: .medium))
                .tracking(Theme.trackingBody)
                .foregroundStyle(Theme.textSecondary)

            Text(repo.lastCommandOutput)
                .font(Theme.fontMeta(monospaced: true))
                .foregroundStyle(Theme.textSecondary)
                .textSelection(.enabled)
                .padding(Theme.space3)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.radiusContainer)
                        .strokeBorder(Theme.border, lineWidth: 1)
                )
        }
    }
}

// MARK: - Lightweight Markdown Block Renderer for README Content

private struct ReadmeMarkdownView: View {
    let markdown: String

    private enum Block: Identifiable {
        case heading(id: Int, level: Int, text: String)
        case codeBlock(id: Int, code: String)
        case bullet(id: Int, text: String)
        case paragraph(id: Int, text: String)

        var id: Int {
            switch self {
            case .heading(let id, _, _),
                 .codeBlock(let id, _),
                 .bullet(let id, _),
                 .paragraph(let id, _):
                return id
            }
        }
    }

    private var blocks: [Block] {
        var result: [Block] = []
        let lines = markdown.components(separatedBy: .newlines)
        var inCodeFence = false
        var codeAccumulator: [String] = []
        var paragraphAccumulator: [String] = []
        var nextID = 0

        func flushParagraph() {
            guard !paragraphAccumulator.isEmpty else { return }
            let joined = paragraphAccumulator.joined(separator: " ")
            result.append(.paragraph(id: nextID, text: joined))
            nextID += 1
            paragraphAccumulator.removeAll()
        }

        for rawLine in lines.prefix(180) {
            let trimmed = rawLine.trimmingCharacters(in: .whitespaces)

            if trimmed.hasPrefix("```") {
                flushParagraph()
                if inCodeFence {
                    result.append(.codeBlock(id: nextID, code: codeAccumulator.joined(separator: "\n")))
                    nextID += 1
                    codeAccumulator.removeAll()
                    inCodeFence = false
                } else {
                    inCodeFence = true
                }
                continue
            }

            if inCodeFence {
                codeAccumulator.append(rawLine)
                continue
            }

            if trimmed.isEmpty {
                flushParagraph()
                continue
            }

            if trimmed.hasPrefix("#") {
                flushParagraph()
                let hashes = trimmed.prefix { $0 == "#" }
                let title = trimmed.dropFirst(hashes.count).trimmingCharacters(in: .whitespaces)
                if !title.isEmpty {
                    result.append(.heading(id: nextID, level: min(hashes.count, 3), text: title))
                    nextID += 1
                }
                continue
            }

            if trimmed.hasPrefix("- ") || trimmed.hasPrefix("* ") {
                flushParagraph()
                let item = String(trimmed.dropFirst(2)).trimmingCharacters(in: .whitespaces)
                result.append(.bullet(id: nextID, text: item))
                nextID += 1
                continue
            }

            paragraphAccumulator.append(trimmed)
        }

        flushParagraph()
        if !codeAccumulator.isEmpty {
            result.append(.codeBlock(id: nextID, code: codeAccumulator.joined(separator: "\n")))
        }
        return result
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.space2) {
            ForEach(blocks) { block in
                switch block {
                case .heading(_, let level, let text):
                    Text(inlineMarkdown(text))
                        .font(level == 1 ? Theme.fontTitle(weight: .medium) : Theme.fontBody(weight: .medium))
                        .tracking(level == 1 ? Theme.trackingTitle : Theme.trackingBody)
                        .foregroundStyle(Theme.textPrimary)
                        .padding(.top, level == 1 ? 2 : 4)

                case .codeBlock(_, let code):
                    Text(code)
                        .font(Theme.fontMeta(monospaced: true))
                        .foregroundStyle(Theme.textSecondary)
                        .padding(Theme.space2)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Theme.surface1, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
                        .overlay(
                            RoundedRectangle(cornerRadius: Theme.radiusControl)
                                .strokeBorder(Theme.border, lineWidth: 1)
                        )

                case .bullet(_, let text):
                    HStack(alignment: .top, spacing: 6) {
                        Text("•")
                            .font(Theme.fontMeta(weight: .medium))
                            .foregroundStyle(Theme.textTertiary)
                        Text(inlineMarkdown(text))
                            .font(Theme.fontMeta())
                            .tracking(Theme.trackingBody)
                            .foregroundStyle(Theme.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }

                case .paragraph(_, let text):
                    Text(inlineMarkdown(text))
                        .font(Theme.fontMeta())
                        .tracking(Theme.trackingBody)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .textSelection(.enabled)
    }

    private func inlineMarkdown(_ raw: String) -> AttributedString {
        if let attr = try? AttributedString(
            markdown: raw,
            options: AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        ) {
            return attr
        }
        return AttributedString(raw)
    }
}
