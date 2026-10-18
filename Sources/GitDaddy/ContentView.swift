import SwiftUI

struct ContentView: View {
    @ObservedObject var store: RepoStore
    @State private var showActivityLogSheet: Bool = false
    @State private var showInspector: Bool = true
    @State private var pendingDeleteRepoIDs: [String] = []
    @State private var showDeleteConfirmDialog: Bool = false

    var body: some View {
        VStack(spacing: 0) {
            topHeaderBar
            Rectangle().fill(Theme.border).frame(height: 1)

            HStack(spacing: 0) {
                sidebarPane
                    .frame(width: 224)

                Rectangle().fill(Theme.border).frame(width: 1)

                centerTablePane
                    .frame(minWidth: 520, maxWidth: .infinity, maxHeight: .infinity)

                if showInspector {
                    Rectangle().fill(Theme.border).frame(width: 1)

                    if let focused = store.focusedRepo {
                        InspectorView(store: store, repo: focused)
                            .frame(width: 360)
                    } else {
                        emptyInspectorPlaceholder
                            .frame(width: 360)
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)

            Rectangle().fill(Theme.border).frame(height: 1)
            bottomStatusBar
        }
        .background(Theme.canvas)
        .sheet(isPresented: $showActivityLogSheet) {
            ActivityLogSheet(store: store)
        }
        .confirmationDialog(
            deleteDialogTitle,
            isPresented: $showDeleteConfirmDialog,
            titleVisibility: .visible
        ) {
            Button("Move to Trash") {
                store.deleteRepositories(ids: pendingDeleteRepoIDs, moveToTrash: true)
                pendingDeleteRepoIDs = []
            }
            Button("Delete Permanently", role: .destructive) {
                store.deleteRepositories(ids: pendingDeleteRepoIDs, moveToTrash: false)
                pendingDeleteRepoIDs = []
            }
            Button("Cancel", role: .cancel) {
                pendingDeleteRepoIDs = []
            }
        } message: {
            Text(deleteDialogMessage)
        }
    }

    // MARK: - Top Header Bar (Fixed, Never Scrolls)

    private var topHeaderBar: some View {
        HStack(spacing: Theme.space3) {
            HStack(spacing: Theme.space2) {
                if let logo = AppDelegate.appLogoImage {
                    Image(nsImage: logo)
                        .resizable()
                        .interpolation(.high)
                        .frame(width: 22, height: 22)
                }

                Text("GitDaddy")
                    .font(Theme.fontTitle(weight: .medium))
                    .tracking(Theme.trackingTitle)
                    .foregroundStyle(Theme.textPrimary)
            }

            Rectangle()
                .fill(Theme.border)
                .frame(width: 1, height: 16)

            // Status summary counts: Green = Good, Yellow = Notice, Red = Bad
            HStack(spacing: Theme.space1) {
                FilterCountTab(
                    label: "All",
                    statusKind: .allRepos,
                    count: store.repos.count,
                    isSelected: store.filter == .all
                ) {
                    store.filter = .all
                }

                FilterCountTab(
                    label: "Up to Date",
                    statusKind: .success,
                    count: store.upToDateReposCount,
                    isSelected: store.filter == .upToDate
                ) {
                    store.filter = .upToDate
                }

                FilterCountTab(
                    label: "Updates",
                    statusKind: .needsUpdate,
                    count: store.outdatedReposCount,
                    isSelected: store.filter == .needsUpdate
                ) {
                    store.filter = .needsUpdate
                }

                FilterCountTab(
                    label: "Dirty",
                    statusKind: .dirty,
                    count: store.dirtyReposCount,
                    isSelected: store.filter == .localChanges
                ) {
                    store.filter = .localChanges
                }

                FilterCountTab(
                    label: "Unpushed",
                    statusKind: .unpushed,
                    count: store.aheadOrDivergedCount,
                    isSelected: store.filter == .aheadOrDiverged
                ) {
                    store.filter = .aheadOrDiverged
                }

                FilterCountTab(
                    label: "Issues",
                    statusKind: .danger,
                    count: store.errorOrNoRemoteCount,
                    isSelected: store.filter == .errors
                ) {
                    store.filter = .errors
                }
            }

            Spacer()

            HStack(spacing: Theme.space2) {
                RestyleButton(
                    label: "Check All",
                    icon: "arrow.triangle.2.circlepath",
                    variant: .secondary,
                    compact: true,
                    isDisabled: store.isScanning || store.isBatchRunning || store.repos.isEmpty
                ) {
                    Task {
                        await store.checkAllRepos()
                    }
                }

                if store.unpushedReposCount > 0 {
                    RestyleButton(
                        label: "Push Unpushed (\(store.unpushedReposCount))",
                        icon: "arrow.up",
                        variant: .secondary,
                        compact: true,
                        isDisabled: store.isScanning || store.isBatchRunning
                    ) {
                        Task {
                            await store.pushAllUnpushedRepos()
                        }
                    }
                }

                RestyleButton(
                    label: store.outdatedReposCount > 0
                        ? "Update Outdated (\(store.outdatedReposCount))"
                        : "Pull All",
                    icon: "arrow.down",
                    variant: .primary,
                    compact: true,
                    isDisabled: store.isScanning || store.isBatchRunning || store.repos.isEmpty
                ) {
                    Task {
                        if store.outdatedReposCount > 0 {
                            await store.pullAllOutdatedRepos()
                        } else {
                            await store.pullAllRepos()
                        }
                    }
                }

                RestyleIconButton(
                    icon: "list.bullet.rectangle",
                    tooltip: "Activity Log"
                ) {
                    showActivityLogSheet = true
                }

                RestyleIconButton(
                    icon: "sidebar.right",
                    tooltip: "Toggle Inspector"
                ) {
                    showInspector.toggle()
                }
            }
        }
        .padding(.horizontal, Theme.space4)
        .frame(height: 44)
        .background(Theme.surface1)
    }

    // MARK: - Left Sidebar Pane (Flat Surface1)

    private var sidebarPane: some View {
        VStack(alignment: .leading, spacing: Theme.space6) {
            // Views Section
            VStack(alignment: .leading, spacing: Theme.space1) {
                sectionHeader("Views")

                ForEach(RepoFilter.allCases) { item in
                    SidebarNavRow(
                        title: item.rawValue,
                        statusKind: item.statusPreview,
                        count: badgeCount(for: item),
                        isSelected: store.filter == item
                    ) {
                        store.filter = item
                    }
                }
            }

            // Watched Folders Section
            VStack(alignment: .leading, spacing: Theme.space1) {
                HStack {
                    sectionHeader("Watched Folders")
                    Spacer()
                    RestyleIconButton(
                        icon: "arrow.clockwise",
                        tooltip: "Rescan Watched Folders",
                        isDisabled: store.isScanning
                    ) {
                        Task {
                            await store.scanDirectories(checkRemoteAfterScan: true)
                        }
                    }
                }

                ForEach(store.scanFolders, id: \.self) { folder in
                    WatchedFolderRow(
                        name: URL(fileURLWithPath: folder).lastPathComponent,
                        path: abbreviateHome(folder)
                    ) {
                        store.removeScanFolder(folder)
                    }
                }

                RestyleButton(
                    label: "Add Folder…",
                    icon: "plus",
                    variant: .secondary,
                    compact: true
                ) {
                    store.promptToAddFolder()
                }
                .padding(.top, Theme.space1)

                Toggle(isOn: $store.scanNestedRepos) {
                    Text("Scan nested repos")
                        .font(Theme.fontMeta())
                        .tracking(Theme.trackingBody)
                        .foregroundStyle(Theme.textSecondary)
                }
                .toggleStyle(.checkbox)
                .padding(.top, Theme.space1)
                .onChange(of: store.scanNestedRepos) { _, _ in
                    Task {
                        await store.scanDirectories(checkRemoteAfterScan: false)
                    }
                }
            }

            // Pull Strategy Section
            VStack(alignment: .leading, spacing: Theme.space2) {
                sectionHeader("Pull Mode")

                Picker("", selection: $store.pullStrategy) {
                    ForEach(PullStrategy.allCases) { strategy in
                        Text(strategy.rawValue).tag(strategy)
                    }
                }
                .labelsHidden()
                .pickerStyle(.menu)
                .controlSize(.small)

                Toggle(isOn: $store.autoStashOnPull) {
                    Text("Auto-stash local edits")
                        .font(Theme.fontMeta())
                        .tracking(Theme.trackingBody)
                        .foregroundStyle(Theme.textSecondary)
                }
                .toggleStyle(.checkbox)
            }

            Spacer()
        }
        .padding(Theme.space3)
        .background(Theme.surface1)
    }

    private func sectionHeader(_ text: String) -> some View {
        Text(text)
            .font(Theme.fontMeta(weight: .medium))
            .tracking(Theme.trackingBody)
            .foregroundStyle(Theme.textTertiary)
            .padding(.horizontal, Theme.space2)
            .padding(.bottom, Theme.space1)
    }

    // MARK: - Center Table Pane

    private var centerTablePane: some View {
        VStack(spacing: 0) {
            filterAndSelectionBar
            Rectangle().fill(Theme.border).frame(height: 1)
            tableColumnHeaders
            Rectangle().fill(Theme.border).frame(height: 1)

            if store.isScanning && store.repos.isEmpty {
                VStack(spacing: Theme.space2) {
                    StatusIcon(state: .checking, compact: false, showLabel: false)
                    Text("Scanning directories for Git repositories…")
                        .font(Theme.fontBody(weight: .medium))
                        .tracking(Theme.trackingBody)
                        .foregroundStyle(Theme.textPrimary)
                    Text(store.scanFolders.map(abbreviateHome).joined(separator: ", "))
                        .font(Theme.fontMeta(monospaced: true))
                        .foregroundStyle(Theme.textTertiary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if store.filteredRepos.isEmpty {
                emptyReposView
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(store.filteredRepos) { repo in
                            RepoTableRowView(
                                repo: repo,
                                isSelected: store.selectedRepoIDs.contains(repo.id),
                                isFocused: store.focusedRepoID == repo.id,
                                onToggleSelect: {
                                    store.toggleSelection(for: repo.id)
                                },
                                onFocus: {
                                    store.focusedRepoID = repo.id
                                },
                                onCheck: {
                                    Task {
                                        await store.checkSingleRepo(repo.id)
                                    }
                                },
                                onPull: {
                                    Task {
                                        await store.pullSingleRepo(repo.id)
                                    }
                                },
                                onPush: {
                                    Task {
                                        await store.pushSingleRepo(repo.id)
                                    }
                                },
                                onDelete: {
                                    requestDelete(for: [repo.id])
                                },
                                onOpenFinder: {
                                    store.openInFinder(repo.path)
                                },
                                onOpenTerminal: {
                                    store.openInTerminal(repo.path)
                                },
                                onCopyPath: {
                                    store.copyPathToClipboard(repo.path)
                                }
                            )
                            Rectangle().fill(Theme.border).frame(height: 1)
                        }
                    }
                }
            }
        }
        .background(Theme.canvas)
    }

    // MARK: - Search, Sort, and Selection Bar

    private var filterAndSelectionBar: some View {
        HStack(spacing: Theme.space2) {
            HStack(spacing: Theme.space2) {
                Image(systemName: "magnifyingglass")
                    .font(.system(size: Theme.iconCompact - 2, weight: .regular))
                    .foregroundStyle(Theme.textTertiary)

                TextField("Filter repositories, branches, paths…", text: $store.searchText)
                    .textFieldStyle(.plain)
                    .font(Theme.fontBody())
                    .foregroundStyle(Theme.textPrimary)

                if !store.searchText.isEmpty {
                    Button {
                        store.searchText = ""
                    } label: {
                        Image(systemName: "xmark")
                            .font(.system(size: 11, weight: .regular))
                            .foregroundStyle(Theme.textTertiary)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, Theme.space3)
            .frame(maxWidth: 280)
            .frame(height: Theme.rowCompact)
            .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radiusControl)
                    .strokeBorder(Theme.borderStrong, lineWidth: 1)
            )

            Picker("Sort", selection: $store.sortOrder) {
                ForEach(RepoSortOrder.allCases) { order in
                    Text(order.rawValue).tag(order)
                }
            }
            .labelsHidden()
            .pickerStyle(.menu)
            .controlSize(.small)
            .frame(width: 140)

            Spacer()

            HStack(spacing: Theme.space2) {
                RestyleButton(
                    label: "Select All (\(store.filteredRepos.count))",
                    variant: .ghost,
                    compact: true,
                    isDisabled: store.filteredRepos.isEmpty
                ) {
                    store.selectAllVisible()
                }

                if store.outdatedReposCount > 0 {
                    RestyleButton(
                        label: "Select Outdated (\(store.outdatedReposCount))",
                        variant: .ghost,
                        compact: true
                    ) {
                        store.selectAllOutdated()
                    }
                }

                if store.unpushedReposCount > 0 {
                    RestyleButton(
                        label: "Select Unpushed (\(store.unpushedReposCount))",
                        variant: .ghost,
                        compact: true
                    ) {
                        store.selectAllUnpushed()
                    }
                }

                if !store.selectedRepoIDs.isEmpty {
                    RestyleButton(
                        label: "Clear (\(store.selectedRepoIDs.count))",
                        variant: .ghost,
                        compact: true
                    ) {
                        store.clearSelection()
                    }
                }
            }
        }
        .padding(.horizontal, Theme.space3)
        .frame(height: 40)
        .background(Theme.surface1)
    }

    // MARK: - Table Column Headers (28px Compact Row)

    private var tableColumnHeaders: some View {
        HStack(spacing: Theme.space2) {
            Text("")
                .frame(width: 20)

            Text("St")
                .font(Theme.fontMeta(weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .frame(width: 24, alignment: .center)

            Text("Repository")
                .font(Theme.fontMeta(weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .frame(width: 180, alignment: .leading)

            Text("Branch")
                .font(Theme.fontMeta(weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .frame(width: 140, alignment: .leading)

            Text("Status / Latest Commit")
                .font(Theme.fontMeta(weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .frame(maxWidth: .infinity, alignment: .leading)

            Text("Delta")
                .font(Theme.fontMeta(weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .frame(width: 72, alignment: .trailing)

            Text("Actions")
                .font(Theme.fontMeta(weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .frame(width: 116, alignment: .trailing)
        }
        .padding(.horizontal, Theme.space3)
        .frame(height: Theme.rowCompact)
        .background(Theme.surface1)
    }

    // MARK: - Bottom Status Bar (32px)

    private var bottomStatusBar: some View {
        HStack(spacing: Theme.space2) {
            StatusIcon(
                state: store.isBatchRunning
                    ? .checking
                    : (store.outdatedReposCount > 0 ? .behind(count: store.outdatedReposCount) : .upToDate),
                compact: true,
                showLabel: false,
                mutedNormal: true
            )

            if store.isBatchRunning {
                Text("\(store.batchCompletedCount)/\(store.batchTotalCount)")
                    .font(Theme.fontMeta(weight: .medium, monospaced: true))
                    .monospacedDigit()
                    .foregroundStyle(Theme.textPrimary)
            }

            Text(store.statusBanner)
                .font(Theme.fontMeta())
                .tracking(Theme.trackingBody)
                .foregroundStyle(Theme.textSecondary)
                .lineLimit(1)

            Spacer()

            if !store.selectedRepoIDs.isEmpty {
                RestyleButton(
                    label: "Delete (\(store.selectedRepoIDs.count))",
                    icon: "trash",
                    variant: .secondary,
                    compact: true,
                    isDisabled: store.isBatchRunning
                ) {
                    requestDelete(for: Array(store.selectedRepoIDs))
                }

                RestyleButton(
                    label: "Check (\(store.selectedRepoIDs.count))",
                    icon: "arrow.triangle.2.circlepath",
                    variant: .secondary,
                    compact: true,
                    isDisabled: store.isBatchRunning
                ) {
                    Task {
                        await store.checkSelectedRepos()
                    }
                }

                RestyleButton(
                    label: "Push (\(store.selectedRepoIDs.count))",
                    icon: "arrow.up",
                    variant: .secondary,
                    compact: true,
                    isDisabled: store.isBatchRunning
                ) {
                    Task {
                        await store.pushSelectedRepos()
                    }
                }

                RestyleButton(
                    label: "Pull (\(store.selectedRepoIDs.count))",
                    icon: "arrow.down",
                    variant: .primary,
                    compact: true,
                    isDisabled: store.isBatchRunning
                ) {
                    Task {
                        await store.pullSelectedRepos()
                    }
                }
            }
        }
        .padding(.horizontal, Theme.space3)
        .frame(height: Theme.rowHeight)
        .background(Theme.surface1)
    }

    // MARK: - Empty States

    private var emptyReposView: some View {
        VStack(spacing: Theme.space3) {
            Text(store.repos.isEmpty ? "No Git repositories found" : "No matching repositories")
                .font(Theme.fontTitle(weight: .medium))
                .tracking(Theme.trackingTitle)
                .foregroundStyle(Theme.textPrimary)

            Text(
                store.repos.isEmpty
                    ? "Add a workspace folder to scan for local Git repositories."
                    : "Clear the current filter or search query to view all repositories."
            )
            .font(Theme.fontBody())
            .tracking(Theme.trackingBody)
            .foregroundStyle(Theme.textSecondary)

            if store.repos.isEmpty {
                RestyleButton(label: "Add Folder…", icon: "plus", variant: .primary) {
                    store.promptToAddFolder()
                }
            } else {
                RestyleButton(label: "Reset Filters", variant: .secondary) {
                    store.filter = .all
                    store.searchText = ""
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var emptyInspectorPlaceholder: some View {
        VStack(spacing: Theme.space2) {
            Text("No repository selected")
                .font(Theme.fontBody(weight: .medium))
                .tracking(Theme.trackingBody)
                .foregroundStyle(Theme.textSecondary)
            Text("Select a row to inspect commits, working tree changes, and git output.")
                .font(Theme.fontMeta())
                .tracking(Theme.trackingBody)
                .foregroundStyle(Theme.textTertiary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, Theme.space6)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.surface1)
    }

    // MARK: - Delete Dialog Helpers

    private var deleteDialogTitle: String {
        let targets = store.repos.filter { pendingDeleteRepoIDs.contains($0.id) }
        if targets.count == 1, let first = targets.first {
            return "Delete Local Repository \"\(first.name)\"?"
        }
        return "Delete \(targets.count) Local Repositories?"
    }

    private var deleteDialogMessage: String {
        let targets = store.repos.filter { pendingDeleteRepoIDs.contains($0.id) }
        guard !targets.isEmpty else { return "" }

        let names = targets.prefix(5).map { "- \($0.name) (\($0.displayPath))" }.joined(separator: "\n")
        let extra = targets.count > 5 ? "\n…and \(targets.count - 5) more." : ""

        let unpushedRepos = targets.filter { $0.syncState.aheadCount > 0 || $0.syncState.isUnpublishedBranch }
        let dirtyRepos = targets.filter { $0.isDirty }

        var warning = ""
        if !unpushedRepos.isEmpty || !dirtyRepos.isEmpty {
            warning = "\n\nWarning: "
            var parts: [String] = []
            if !unpushedRepos.isEmpty {
                parts.append("\(unpushedRepos.count) repo(s) have unpushed or unpublished commits")
            }
            if !dirtyRepos.isEmpty {
                parts.append("\(dirtyRepos.count) repo(s) have uncommitted local changes")
            }
            warning += parts.joined(separator: " and ") + "."
        }

        return "\(names)\(extra)\n\nThis removes the folder(s) from your Mac. Remote repositories are not affected.\(warning)"
    }

    private func requestDelete(for ids: [String]) {
        guard !ids.isEmpty else { return }
        pendingDeleteRepoIDs = ids
        showDeleteConfirmDialog = true
    }

    private func badgeCount(for filter: RepoFilter) -> Int {
        switch filter {
        case .all: return store.repos.count
        case .needsUpdate: return store.outdatedReposCount
        case .upToDate: return store.upToDateReposCount
        case .localChanges: return store.dirtyReposCount
        case .aheadOrDiverged: return store.aheadOrDivergedCount
        case .errors: return store.errorOrNoRemoteCount
        }
    }

    private func abbreviateHome(_ path: String) -> String {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        if path.hasPrefix(home) {
            let suffix = path.dropFirst(home.count)
            return suffix.isEmpty ? "~" : "~" + suffix
        }
        return path
    }
}

// MARK: - Top Filter Count Tab (Status Icon + Tabular Number)

struct FilterCountTab: View {
    let label: String
    let statusKind: CanonicalStatusKind?
    let count: Int
    let isSelected: Bool
    let action: () -> Void

    @State private var isHovered: Bool = false

    private var semanticColor: Color {
        guard let statusKind, statusKind != .allRepos else {
            return Theme.accent
        }
        return statusKind.strokeColor()
    }

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if let statusKind {
                    Image(systemName: statusKind.symbolName)
                        .symbolRenderingMode(.monochrome)
                        .font(.system(size: Theme.iconCompact - 2, weight: .medium))
                        .foregroundColor(statusKind.strokeColor())
                        .foregroundStyle(statusKind.strokeColor())
                }
                Text(label)
                    .font(Theme.fontMeta(weight: isSelected ? .medium : .regular))
                    .tracking(Theme.trackingBody)
                    .foregroundStyle(isSelected ? Theme.textPrimary : Theme.textSecondary)
                Text("\(count)")
                    .font(Theme.fontMeta(weight: isSelected || count > 0 ? .medium : .regular))
                    .monospacedDigit()
                    .foregroundStyle(count > 0 ? Theme.textPrimary : Theme.textTertiary)
            }
            .padding(.horizontal, Theme.space2)
            .frame(height: Theme.rowCompact)
            .background(
                isSelected
                    ? semanticColor.opacity(0.12)
                    : (isHovered ? Theme.surface2 : Color.clear),
                in: RoundedRectangle(cornerRadius: Theme.radiusControl)
            )
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radiusControl)
                    .strokeBorder(
                        isSelected ? semanticColor.opacity(0.35) : Color.clear,
                        lineWidth: 1
                    )
            )
        }
        .buttonStyle(.plain)
        .onHover { hovering in
            isHovered = hovering
        }
    }
}

// MARK: - Sidebar Navigation Row (28px Compact)

struct SidebarNavRow: View {
    let title: String
    let statusKind: CanonicalStatusKind?
    let count: Int
    let isSelected: Bool
    let action: () -> Void

    @State private var isHovered: Bool = false

    private var indicatorColor: Color {
        guard let statusKind, statusKind != .allRepos else {
            return Theme.accent
        }
        return statusKind.strokeColor()
    }

    var body: some View {
        Button(action: action) {
            HStack(spacing: Theme.space2) {
                if let statusKind {
                    Image(systemName: statusKind.symbolName)
                        .symbolRenderingMode(.monochrome)
                        .font(.system(size: Theme.iconCompact - 2, weight: .medium))
                        .foregroundColor(statusKind.strokeColor())
                        .foregroundStyle(statusKind.strokeColor())
                        .frame(width: Theme.iconCompact)
                } else {
                    Image(systemName: "tray")
                        .font(.system(size: Theme.iconCompact - 2, weight: .regular))
                        .foregroundStyle(isSelected ? Theme.textPrimary : Theme.textSecondary)
                        .frame(width: Theme.iconCompact)
                }

                Text(title)
                    .font(Theme.fontBody(weight: isSelected ? .medium : .regular))
                    .tracking(Theme.trackingBody)
                    .foregroundStyle(isSelected ? Theme.textPrimary : Theme.textSecondary)
                    .lineLimit(1)

                Spacer()

                if count > 0 {
                    Text("\(count)")
                        .font(Theme.fontMeta(weight: .medium))
                        .monospacedDigit()
                        .foregroundStyle(isSelected ? Theme.textPrimary : Theme.textSecondary)
                }
            }
            .padding(.horizontal, Theme.space2)
            .frame(height: Theme.rowCompact)
            .background(
                (isSelected || isHovered) ? Theme.surface2 : Color.clear,
                in: RoundedRectangle(cornerRadius: Theme.radiusControl)
            )
            .overlay(alignment: .leading) {
                if isSelected {
                    RoundedRectangle(cornerRadius: 1)
                        .fill(indicatorColor)
                        .frame(width: 2, height: 16)
                }
            }
            .contentShape(RoundedRectangle(cornerRadius: Theme.radiusControl))
        }
        .buttonStyle(.plain)
        .onHover { hovering in
            isHovered = hovering
        }
    }
}

// MARK: - Watched Folder Row (28px Compact)

struct WatchedFolderRow: View {
    let name: String
    let path: String
    let onRemove: () -> Void

    @State private var isHovered: Bool = false

    var body: some View {
        HStack(spacing: Theme.space2) {
            Image(systemName: "folder")
                .font(.system(size: Theme.iconCompact - 2, weight: .regular))
                .foregroundStyle(Theme.textSecondary)
                .frame(width: Theme.iconCompact)

            Text(name)
                .font(Theme.fontBody(weight: .regular))
                .tracking(Theme.trackingBody)
                .foregroundStyle(Theme.textPrimary)
                .lineLimit(1)

            Text(path)
                .font(Theme.fontMeta(monospaced: true))
                .foregroundStyle(Theme.textTertiary)
                .lineLimit(1)
                .truncationMode(.middle)

            Spacer(minLength: 0)

            if isHovered {
                Button(action: onRemove) {
                    Image(systemName: "xmark")
                        .font(.system(size: 10, weight: .regular))
                        .foregroundStyle(Theme.textSecondary)
                }
                .buttonStyle(.plain)
                .help("Remove watched folder")
            }
        }
        .padding(.horizontal, Theme.space2)
        .frame(height: Theme.rowCompact)
        .background(
            isHovered ? Theme.surface2 : Color.clear,
            in: RoundedRectangle(cornerRadius: Theme.radiusControl)
        )
        .onHover { hovering in
            isHovered = hovering
        }
    }
}

// MARK: - Dense 32px Full-Bleed Repository Table Row

struct RepoTableRowView: View {
    let repo: GitRepo
    let isSelected: Bool
    let isFocused: Bool
    let onToggleSelect: () -> Void
    let onFocus: () -> Void
    let onCheck: () -> Void
    let onPull: () -> Void
    let onPush: () -> Void
    let onDelete: () -> Void
    let onOpenFinder: () -> Void
    let onOpenTerminal: () -> Void
    let onCopyPath: () -> Void

    @State private var isHovered: Bool = false

    private var isDiverged: Bool {
        if case .diverged = repo.syncState { return true }
        return false
    }

    var body: some View {
        HStack(spacing: Theme.space2) {
            // 1. Multi-select Checkbox (20px)
            Button(action: onToggleSelect) {
                Image(systemName: isSelected ? "checkmark.square" : "square")
                    .font(.system(size: Theme.iconSize - 2, weight: .regular))
                    .foregroundStyle(isSelected ? Theme.accent : Theme.textSecondary)
                    .frame(width: 20, height: Theme.rowCompact)
            }
            .buttonStyle(.plain)
            .accessibilityLabel(isSelected ? "Deselect \(repo.name)" : "Select \(repo.name)")

            // 2. Status Icon Column (24px) — Green = Good, Yellow = Notice, Red = Bad
            StatusIcon(
                state: repo.syncState,
                compact: false,
                showLabel: false
            )
            .frame(width: 24, alignment: .center)

            // 3. Repository Name (180px)
            HStack(spacing: 6) {
                Text(repo.name)
                    .font(Theme.fontBody(weight: .medium))
                    .tracking(Theme.trackingBody)
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
                    .truncationMode(.tail)

                Spacer(minLength: 0)
            }
            .frame(width: 180, alignment: .leading)

            // 4. Branch (140px)
            HStack(spacing: 4) {
                Image(systemName: "arrow.triangle.branch")
                    .font(.system(size: 11, weight: .regular))
                    .foregroundStyle(Theme.textTertiary)
                Text(repo.branch)
                    .font(Theme.fontMeta(weight: .regular, monospaced: true))
                    .foregroundStyle(Theme.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
            .frame(width: 140, alignment: .leading)

            // 5. Status / Explanation / Latest Commit (Single-line truncated)
            HStack(spacing: Theme.space2) {
                Text(statusOrCommitSummary)
                    .font(Theme.fontBody(weight: .regular))
                    .tracking(Theme.trackingBody)
                    .foregroundStyle(repo.syncState.needsUpdate || repo.syncState.isIssueOrNoUpstream ? Theme.textPrimary : Theme.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.tail)

                if !repo.lastCommitRelativeDate.isEmpty {
                    Text(repo.lastCommitRelativeDate)
                        .font(Theme.fontMeta())
                        .foregroundStyle(Theme.textTertiary)
                        .lineLimit(1)
                }

                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            // 6. Right-aligned Tabular Delta Counts (72px)
            HStack(spacing: 6) {
                Spacer(minLength: 0)

                if repo.syncState.behindCount > 0 {
                    Text("↓\(repo.syncState.behindCount)")
                        .font(Theme.fontMeta(weight: .medium, monospaced: true))
                        .monospacedDigit()
                        .foregroundStyle(isDiverged ? Theme.statusBad : Theme.statusNotice)
                }

                if repo.syncState.aheadCount > 0 {
                    Text("↑\(repo.syncState.aheadCount)")
                        .font(Theme.fontMeta(weight: .medium, monospaced: true))
                        .monospacedDigit()
                        .foregroundStyle(isDiverged ? Theme.statusBad : Theme.statusNotice)
                } else if repo.syncState.isUnpublishedBranch {
                    Text("new")
                        .font(Theme.fontMeta(weight: .medium, monospaced: true))
                        .foregroundStyle(Theme.statusNotice)
                }

                if repo.isDirty {
                    Text("\(repo.localChanges.count)M")
                        .font(Theme.fontMeta(weight: .medium, monospaced: true))
                        .monospacedDigit()
                        .foregroundStyle(Theme.statusNotice)
                        .help("\(repo.localChanges.count) uncommitted file change(s)")
                }
            }
            .frame(width: 72, alignment: .trailing)

            // 7. Ghost Icon Row Actions (28px square, radius 6px, hover surface2) + More Menu
            HStack(spacing: 2) {
                RestyleIconButton(
                    icon: "arrow.triangle.2.circlepath",
                    tooltip: repo.remoteURL == nil ? "No remote URL configured" : "Check Remote (git fetch)",
                    isDisabled: repo.syncState.isBusy || repo.remoteURL == nil,
                    action: onCheck
                )

                RestyleIconButton(
                    icon: "arrow.down",
                    tooltip: repo.syncState.behindCount > 0
                        ? "Pull \(repo.syncState.behindCount) Update(s) (git pull)"
                        : "Pull from Remote (git pull)",
                    isDisabled: !repo.syncState.canPull,
                    action: onPull
                )

                RestyleIconButton(
                    icon: "arrow.up",
                    tooltip: repo.syncState.isUnpublishedBranch
                        ? "Publish Branch (git push -u)"
                        : (repo.syncState.aheadCount > 0 ? "Push \(repo.syncState.aheadCount) Commit(s)" : "Push to Remote"),
                    isDisabled: repo.syncState.isBusy || repo.remoteURL == nil || repo.lastCommitHash.isEmpty,
                    action: onPush
                )

                Menu {
                    Button("Check Remote (git fetch)", action: onCheck)
                        .disabled(repo.remoteURL == nil)
                    Button("Pull Updates (git pull)", action: onPull)
                        .disabled(!repo.syncState.canPull)
                    Button(
                        repo.syncState.isUnpublishedBranch ? "Publish Branch (git push -u)" : "Push Commits (git push)",
                        action: onPush
                    )
                    .disabled(repo.remoteURL == nil || repo.lastCommitHash.isEmpty)

                    Divider()

                    Button("Reveal in Finder", action: onOpenFinder)
                    Button("Open in Terminal", action: onOpenTerminal)
                    Button("Copy Repository Path", action: onCopyPath)

                    Divider()

                    Button("Delete Local Repository…", role: .destructive, action: onDelete)
                } label: {
                    Image(systemName: "ellipsis")
                        .font(.system(size: Theme.iconSize - 2, weight: .regular))
                        .foregroundStyle(Theme.textSecondary)
                        .frame(width: Theme.rowCompact, height: Theme.rowCompact)
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .frame(width: Theme.rowCompact, height: Theme.rowCompact)
                .help("More Actions")
            }
            .frame(width: 116, alignment: .trailing)
        }
        .padding(.horizontal, Theme.space3)
        .frame(height: Theme.rowHeight)
        .background(rowBackground)
        .overlay(alignment: .leading) {
            if isFocused || isSelected {
                Rectangle()
                    .fill(Theme.accent)
                    .frame(width: 2)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {
            onFocus()
        }
        .onHover { hovering in
            isHovered = hovering
        }
        .contextMenu {
            Button("Check Remote (git fetch)", action: onCheck)
                .disabled(repo.remoteURL == nil)
            Button("Pull Updates (git pull)", action: onPull)
                .disabled(!repo.syncState.canPull)
            Button(
                repo.syncState.isUnpublishedBranch ? "Publish Branch (git push -u)" : "Push Commits (git push)",
                action: onPush
            )
            .disabled(repo.remoteURL == nil || repo.lastCommitHash.isEmpty)

            Divider()

            Button(isSelected ? "Deselect Repository" : "Select Repository", action: onToggleSelect)

            Divider()

            Button("Reveal in Finder", action: onOpenFinder)
            Button("Open in Terminal", action: onOpenTerminal)
            Button("Copy Repository Path", action: onCopyPath)

            Divider()

            Button("Delete Local Repository…", role: .destructive, action: onDelete)
        }
    }

    private var rowBackground: Color {
        if isFocused || isSelected || isHovered {
            return Theme.surface2
        }
        return Theme.surface1
    }

    private var statusOrCommitSummary: String {
        if let incoming = repo.incomingCommits.first {
            return "\(repo.syncState.label) — \(incoming.subject)"
        }
        if let explanation = repo.syncState.rowExplanation {
            return explanation
        }
        if !repo.lastCommitMessage.isEmpty {
            return repo.lastCommitMessage
        }
        return repo.displayPath
    }
}

// MARK: - Activity Log Sheet (Flat Restyle Dialog)

struct ActivityLogSheet: View {
    @ObservedObject var store: RepoStore
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Activity Log")
                    .font(Theme.fontTitle(weight: .medium))
                    .tracking(Theme.trackingTitle)
                    .foregroundStyle(Theme.textPrimary)

                Spacer()

                if !store.activityLogs.isEmpty {
                    RestyleButton(label: "Clear", variant: .secondary, compact: true) {
                        store.activityLogs.removeAll()
                    }
                }

                RestyleButton(label: "Close", variant: .primary, compact: true) {
                    dismiss()
                }
            }
            .padding(.horizontal, Theme.space4)
            .frame(height: 44)
            .background(Theme.surface1)

            Rectangle().fill(Theme.border).frame(height: 1)

            if store.activityLogs.isEmpty {
                VStack(spacing: Theme.space2) {
                    Text("No git operations recorded yet.")
                        .font(Theme.fontBody())
                        .foregroundStyle(Theme.textSecondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Theme.canvas)
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(store.activityLogs) { entry in
                            VStack(alignment: .leading, spacing: Theme.space1) {
                                HStack(spacing: Theme.space2) {
                                    Image(systemName: entry.succeeded ? "checkmark.circle" : "xmark.circle")
                                        .font(.system(size: Theme.iconCompact - 2, weight: .regular))
                                        .foregroundStyle(entry.succeeded ? Theme.statusSuccess : Theme.statusDanger)

                                    Text(entry.repoName)
                                        .font(Theme.fontBody(weight: .medium))
                                        .foregroundStyle(Theme.textPrimary)

                                    Text(entry.action)
                                        .font(Theme.fontMeta())
                                        .foregroundStyle(Theme.textSecondary)

                                    Text(entry.summary)
                                        .font(Theme.fontMeta())
                                        .foregroundStyle(Theme.textTertiary)
                                        .lineLimit(1)

                                    Spacer()

                                    Text(entry.timestamp.formatted(date: .omitted, time: .standard))
                                        .font(Theme.fontMeta(monospaced: true))
                                        .monospacedDigit()
                                        .foregroundStyle(Theme.textTertiary)
                                }
                                .frame(height: Theme.rowCompact)

                                if !entry.output.isEmpty {
                                    Text(entry.output)
                                        .font(Theme.fontMeta(monospaced: true))
                                        .foregroundStyle(Theme.textSecondary)
                                        .textSelection(.enabled)
                                        .padding(Theme.space2)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                        .background(Theme.surface1, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
                                        .overlay(
                                            RoundedRectangle(cornerRadius: Theme.radiusControl)
                                                .strokeBorder(Theme.border, lineWidth: 1)
                                        )
                                }
                            }
                            .padding(.horizontal, Theme.space4)
                            .padding(.vertical, Theme.space2)

                            Rectangle().fill(Theme.border).frame(height: 1)
                        }
                    }
                }
                .background(Theme.canvas)
            }
        }
        .frame(width: 680, height: 480)
    }
}
