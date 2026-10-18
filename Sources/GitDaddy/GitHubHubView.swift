import AppKit
import SwiftUI

// MARK: - GitHub Hub Center Pane

struct GitHubCenterPane: View {
    @ObservedObject var store: RepoStore
    @ObservedObject var hubStore: GitHubHubStore
    let section: HubSection

    private var isLoading: Bool {
        hubStore.loadingSection == section
    }

    var body: some View {
        VStack(spacing: 0) {
            topToolbar
            Rectangle().fill(Theme.border).frame(height: 1)
            contentArea
        }
        .background(Theme.canvas)
    }

    // MARK: - Top Toolbar (40px)

    private var topToolbar: some View {
        HStack(spacing: Theme.space2) {
            if section.usesInvolvement {
                HStack(spacing: 2) {
                    ForEach(Involvement.allCases) { inv in
                        let selected = hubStore.involvement == inv
                        Button {
                            if hubStore.involvement != inv {
                                hubStore.involvement = inv
                                hubStore.selectSection(section, force: true)
                            }
                        } label: {
                            Text(inv.label)
                                .font(Theme.fontMeta(weight: selected ? .medium : .regular))
                                .foregroundStyle(selected ? Theme.textPrimary : Theme.textSecondary)
                                .padding(.horizontal, Theme.space2)
                                .frame(height: 24)
                                .background(
                                    selected ? Theme.surface2 : Color.clear,
                                    in: RoundedRectangle(cornerRadius: 4)
                                )
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(2)
                .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.radiusControl)
                        .strokeBorder(Theme.border, lineWidth: 1)
                )
            } else if section == .explore {
                exploreToolbarControls
            } else {
                HStack(spacing: 6) {
                    Image(systemName: section.iconName)
                        .font(.system(size: Theme.iconCompact - 2, weight: .regular))
                        .foregroundStyle(Theme.textSecondary)
                    Text(section.label)
                        .font(Theme.fontBody(weight: .medium))
                        .foregroundStyle(Theme.textPrimary)
                }
            }

            if section.isRepoList {
                layoutToggleSegmented
            }

            // Search Input
            HStack(spacing: Theme.space2) {
                Image(systemName: "magnifyingglass")
                    .font(.system(size: Theme.iconCompact - 2, weight: .regular))
                    .foregroundStyle(Theme.textTertiary)

                TextField(
                    section.isRepoList ? "Filter repositories, language, topics…" : "Filter by title, repo, author, label…",
                    text: $hubStore.searchText
                )
                .textFieldStyle(.plain)
                .font(Theme.fontBody())
                .foregroundStyle(Theme.textPrimary)

                if !hubStore.searchText.isEmpty {
                    Button {
                        hubStore.searchText = ""
                    } label: {
                        Image(systemName: "xmark")
                            .font(.system(size: 11, weight: .regular))
                            .foregroundStyle(Theme.textTertiary)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, Theme.space3)
            .frame(minWidth: 140, maxWidth: 230)
            .frame(height: Theme.rowCompact)
            .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radiusControl)
                    .strokeBorder(Theme.borderStrong, lineWidth: 1)
            )

            Spacer(minLength: Theme.space1)

            // Busy / Count + Refresh
            HStack(spacing: Theme.space2) {
                if let busy = hubStore.busyAction {
                    DedicatedSpinIndicator(size: 14)
                    Text(busy)
                        .font(Theme.fontMeta())
                        .foregroundStyle(Theme.textSecondary)
                        .lineLimit(1)
                } else if isLoading {
                    DedicatedSpinIndicator(size: 14)
                    Text("Loading…")
                        .font(Theme.fontMeta())
                        .foregroundStyle(Theme.textTertiary)
                } else {
                    let count = section.isRepoList
                        ? hubStore.filteredRepos(for: section).count
                        : hubStore.filteredItems().count
                    Text("\(count) shown")
                        .font(Theme.fontMeta())
                        .monospacedDigit()
                        .foregroundStyle(Theme.textTertiary)
                }

                RestyleIconButton(
                    icon: "arrow.clockwise",
                    tooltip: "Refresh from GitHub",
                    isDisabled: isLoading
                ) {
                    hubStore.refreshActive(section)
                }
            }
        }
        .padding(.horizontal, Theme.space3)
        .frame(height: 40)
        .background(Theme.surface1)
    }

    private var exploreToolbarControls: some View {
        HStack(spacing: 6) {
            // Span selector (Today / This week / This month)
            HStack(spacing: 2) {
                ForEach(TrendingSpan.allCases) { span in
                    let selected = hubStore.trendingSpan == span
                    Button {
                        if hubStore.trendingSpan != span || hubStore.trendingOffset != 0 {
                            hubStore.trendingSpan = span
                            hubStore.trendingOffset = 0
                            hubStore.selectSection(.explore, force: true)
                        }
                    } label: {
                        Text(span.label)
                            .font(Theme.fontMeta(weight: selected ? .medium : .regular))
                            .foregroundStyle(selected ? Theme.textPrimary : Theme.textSecondary)
                            .padding(.horizontal, Theme.space2)
                            .frame(height: 24)
                            .background(
                                selected ? Theme.surface2 : Color.clear,
                                in: RoundedRectangle(cornerRadius: 4)
                            )
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(2)
            .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radiusControl)
                    .strokeBorder(Theme.border, lineWidth: 1)
            )

            // Date-range stepper (< date range >)
            let range = hubStore.trendingSpan.dateRange(offset: hubStore.trendingOffset)
            let canPrev = hubStore.trendingSpan.canAdvance(offset: hubStore.trendingOffset)
            let canNext = hubStore.trendingOffset > 0
            HStack(spacing: 2) {
                Button {
                    if canPrev {
                        hubStore.trendingOffset += 1
                        hubStore.selectSection(.explore, force: true)
                    }
                } label: {
                    Image(systemName: "chevron.left")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(canPrev ? Theme.textSecondary : Theme.textTertiary.opacity(0.4))
                        .frame(width: 22, height: 24)
                }
                .buttonStyle(.plain)
                .disabled(!canPrev)
                .help("Previous \(hubStore.trendingSpan.label.lowercased()) window")

                Button {
                    if canNext {
                        hubStore.trendingOffset = 0
                        hubStore.selectSection(.explore, force: true)
                    }
                } label: {
                    Text(hubStore.trendingOffset == 0 ? range.end : "\(range.start) .. \(range.end)")
                        .font(Theme.fontMeta(monospaced: true))
                        .foregroundStyle(hubStore.trendingOffset > 0 ? Theme.textPrimary : Theme.textSecondary)
                        .padding(.horizontal, 4)
                }
                .buttonStyle(.plain)
                .help(
                    hubStore.trendingOffset > 0
                        ? "Showing \(range.start) .. \(range.end) — click to jump back to latest"
                        : "Repositories created \(range.start) .. \(range.end)"
                )

                Button {
                    if canNext {
                        hubStore.trendingOffset -= 1
                        hubStore.selectSection(.explore, force: true)
                    }
                } label: {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(canNext ? Theme.textSecondary : Theme.textTertiary.opacity(0.4))
                        .frame(width: 22, height: 24)
                }
                .buttonStyle(.plain)
                .disabled(!canNext)
                .help("Newer window")
            }
            .padding(2)
            .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radiusControl)
                    .strokeBorder(Theme.border, lineWidth: 1)
            )

            // Language filter menu
            Menu {
                Button("Any language") {
                    if hubStore.trendingLanguage != nil {
                        hubStore.trendingLanguage = nil
                        hubStore.selectSection(.explore, force: true)
                    }
                }
                Divider()
                ForEach(TrendingSpan.trendingLanguages, id: \.self) { lang in
                    Button(lang) {
                        if hubStore.trendingLanguage != lang {
                            hubStore.trendingLanguage = lang
                            hubStore.selectSection(.explore, force: true)
                        }
                    }
                }
            } label: {
                HStack(spacing: 4) {
                    Text(hubStore.trendingLanguage ?? "Any language")
                        .font(Theme.fontMeta())
                        .foregroundStyle(Theme.textPrimary)
                        .lineLimit(1)
                    Image(systemName: "chevron.down")
                        .font(.system(size: 9, weight: .medium))
                        .foregroundStyle(Theme.textTertiary)
                }
                .padding(.horizontal, Theme.space2)
                .frame(height: Theme.rowCompact)
                .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.radiusControl)
                        .strokeBorder(Theme.borderStrong, lineWidth: 1)
                )
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
        }
    }

    private var layoutToggleSegmented: some View {
        HStack(spacing: 2) {
            ForEach([(RepoLayout.list, "list.bullet", "List view"), (RepoLayout.cards, "square.grid.2x2", "Card view with social preview")], id: \.0) { mode, icon, tip in
                let selected = hubStore.repoLayout == mode
                Button {
                    hubStore.repoLayout = mode
                } label: {
                    Image(systemName: icon)
                        .font(.system(size: 11, weight: .regular))
                        .foregroundStyle(selected ? Theme.textPrimary : Theme.textSecondary)
                        .frame(width: 24, height: 24)
                        .background(
                            selected ? Theme.surface2 : Color.clear,
                            in: RoundedRectangle(cornerRadius: 4)
                        )
                }
                .buttonStyle(.plain)
                .help(tip)
            }
        }
        .padding(2)
        .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.radiusControl)
                .strokeBorder(Theme.border, lineWidth: 1)
        )
    }

    // MARK: - Main Content Area

    @ViewBuilder
    private var contentArea: some View {
        switch hubStore.cliStatus {
        case .notInstalled:
            cliPromptView(installed: false, message: "Install GitHub CLI (`gh`) to browse pull requests, issues, stars, and trending repositories.")
        case .notAuthenticated(let msg):
            cliPromptView(installed: true, message: msg)
        case .checking where hubStore.items.isEmpty && hubStore.starred.isEmpty && hubStore.trending.isEmpty:
            loadingView(text: "Connecting to GitHub…")
        default:
            if let err = hubStore.lastError {
                errorView(message: err)
            } else if isLoading && ((section.isRepoList && hubStore.filteredRepos(for: section).isEmpty) || (!section.isRepoList && hubStore.filteredItems().isEmpty)) {
                loadingView(text: "Loading \(section.label.lowercased()) from GitHub…")
            } else if section.isRepoList {
                repoCollectionView
            } else {
                itemsListView
            }
        }
    }

    private var itemsListView: some View {
        let items = hubStore.filteredItems()
        return Group {
            if items.isEmpty {
                emptySectionView
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(items) { item in
                            HubItemRowView(
                                item: item,
                                isSelected: hubStore.selectedItemURL == item.url,
                                isClonedLocally: store.localClone(for: item.repoSlug) != nil,
                                onSelect: {
                                    hubStore.selectedItemURL = item.url
                                },
                                onOpenBrowser: {
                                    store.openURL(item.url)
                                }
                            )
                            Rectangle().fill(Theme.border).frame(height: 1)
                        }
                    }
                }
            }
        }
    }

    private var repoCollectionView: some View {
        let repos = hubStore.filteredRepos(for: section)
        return Group {
            if repos.isEmpty {
                emptySectionView
            } else if hubStore.repoLayout == .cards {
                ScrollView {
                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: 270, maximum: 420), spacing: 12)],
                        spacing: 12
                    ) {
                        ForEach(repos) { repo in
                            HubRepoCardView(
                                repo: repo,
                                isSelected: hubStore.selectedRepoSlug == repo.nameWithOwner,
                                isClonedLocally: store.localClone(for: repo.nameWithOwner) != nil,
                                onSelect: {
                                    hubStore.selectRepo(repo.nameWithOwner)
                                },
                                onOpenBrowser: {
                                    store.openURL(repo.url)
                                }
                            )
                        }
                    }
                    .padding(12)
                }
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(repos) { repo in
                            HubRepoRowView(
                                repo: repo,
                                isSelected: hubStore.selectedRepoSlug == repo.nameWithOwner,
                                isClonedLocally: store.localClone(for: repo.nameWithOwner) != nil,
                                onSelect: {
                                    hubStore.selectRepo(repo.nameWithOwner)
                                },
                                onOpenBrowser: {
                                    store.openURL(repo.url)
                                }
                            )
                            Rectangle().fill(Theme.border).frame(height: 1)
                        }
                    }
                }
            }
        }
    }

    // MARK: - Empty / Loading / Error States

    private func loadingView(text: String) -> some View {
        VStack(spacing: Theme.space2) {
            DedicatedSpinIndicator(size: 18)
            Text(text)
                .font(Theme.fontBody())
                .foregroundStyle(Theme.textSecondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func errorView(message: String) -> some View {
        VStack(spacing: Theme.space3) {
            Image(systemName: "exclamationmark.triangle")
                .font(.system(size: 20))
                .foregroundStyle(Theme.statusBad)
            Text("Couldn't load \(section.label)")
                .font(Theme.fontTitle(weight: .medium))
                .foregroundStyle(Theme.textPrimary)
            Text(message)
                .font(Theme.fontMeta(monospaced: true))
                .foregroundStyle(Theme.textSecondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 440)
            RestyleButton(label: "Try Again", icon: "arrow.clockwise", variant: .primary, compact: true) {
                hubStore.selectSection(section, force: true)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var emptySectionView: some View {
        let (headline, detail): (String, String) = {
            if !hubStore.searchText.trimmingCharacters(in: .whitespaces).isEmpty {
                return ("No matching results", "Clear the filter query to see all items.")
            }
            switch section {
            case .pullRequests:
                return ("No open pull requests", "Open pull requests matching this filter will appear here.")
            case .issues:
                return ("No open issues", "Open issues matching this filter will appear here.")
            case .reviewRequests:
                return ("No review requests", "Pull requests waiting for your review will appear here.")
            case .starred:
                return ("No starred repositories", "Repositories you star on GitHub will appear here.")
            case .explore:
                return ("No repositories found", "Try another language or time span.")
            }
        }()
        return VStack(spacing: Theme.space2) {
            Text(headline)
                .font(Theme.fontTitle(weight: .medium))
                .foregroundStyle(Theme.textPrimary)
            Text(detail)
                .font(Theme.fontBody())
                .foregroundStyle(Theme.textSecondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func cliPromptView(installed: Bool, message: String) -> some View {
        VStack(spacing: Theme.space3) {
            Image(systemName: "globe")
                .font(.system(size: 26))
                .foregroundStyle(Theme.textSecondary)

            Text(installed ? "Sign in to GitHub CLI" : "GitHub CLI (`gh`) Required")
                .font(Theme.fontTitle(weight: .medium))
                .foregroundStyle(Theme.textPrimary)

            Text(message)
                .font(Theme.fontBody())
                .foregroundStyle(Theme.textSecondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 420)

            HStack(spacing: Theme.space2) {
                if installed {
                    RestyleButton(label: "Sign in with GitHub…", icon: "terminal", variant: .primary, compact: true) {
                        GitHubService.openSignInTerminal()
                    }
                    RestyleButton(label: "Check Again", icon: "arrow.clockwise", variant: .secondary, compact: true) {
                        hubStore.refreshActive(section)
                    }
                } else {
                    RestyleButton(label: "Install GitHub CLI…", icon: "globe", variant: .primary, compact: true) {
                        store.openURL("https://cli.github.com")
                    }
                    RestyleButton(label: "Check Again", icon: "arrow.clockwise", variant: .secondary, compact: true) {
                        hubStore.refreshActive(section)
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

// MARK: - PR / Issue Row View (44px)

struct HubItemRowView: View {
    let item: HubItem
    let isSelected: Bool
    let isClonedLocally: Bool
    let onSelect: () -> Void
    let onOpenBrowser: () -> Void

    @State private var isHovered: Bool = false

    private var statusColor: Color {
        if item.isDraft { return Theme.textTertiary }
        return Theme.statusGood
    }

    private var iconName: String {
        if item.isPR {
            return "arrow.triangle.pull"
        }
        return "smallcircle.filled.circle"
    }

    var body: some View {
        HStack(spacing: Theme.space2) {
            Image(systemName: iconName)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(statusColor)
                .frame(width: 20)

            // Repo#number + local badge
            HStack(spacing: 6) {
                Text("\(item.repoSlug)#\(item.number)")
                    .font(Theme.fontMeta(monospaced: true))
                    .foregroundStyle(Theme.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.middle)

                if isClonedLocally {
                    Text("cloned")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(Theme.accent)
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(Theme.accent.opacity(0.14), in: RoundedRectangle(cornerRadius: 4))
                }
            }
            .frame(width: 210, alignment: .leading)

            // Title
            Text(item.title)
                .font(Theme.fontBody(weight: .medium))
                .tracking(Theme.trackingBody)
                .foregroundStyle(Theme.textPrimary)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: .infinity, alignment: .leading)

            // Branch chip for PRs
            if item.isPR && !item.headRef.isEmpty {
                HStack(spacing: 3) {
                    Image(systemName: "arrow.triangle.branch")
                        .font(.system(size: 10))
                        .foregroundStyle(Theme.textTertiary)
                    Text(item.headRef)
                        .font(Theme.fontMeta(monospaced: true))
                        .foregroundStyle(Theme.textSecondary)
                        .lineLimit(1)
                }
                .frame(maxWidth: 130, alignment: .trailing)
            }

            // Review decision chip
            if let review = item.reviewDecision {
                let revColor: Color = {
                    switch review {
                    case .approved: return Theme.statusGood
                    case .changesRequested: return Theme.statusBad
                    case .reviewRequired: return Theme.statusNotice
                    }
                }()
                Text(review.label)
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(revColor)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(revColor.opacity(0.14), in: RoundedRectangle(cornerRadius: 4))
            }

            // Labels (up to 2)
            ForEach(item.labels.prefix(2)) { label in
                Text(label.name)
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(label.color)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(label.color.opacity(0.16), in: RoundedRectangle(cornerRadius: 4))
                    .lineLimit(1)
            }

            // CI dot
            if let ci = item.ciState {
                let (ciIcon, ciColor, ciTip): (String, Color, String) = {
                    switch ci {
                    case .success: return ("checkmark.circle", Theme.statusGood, "Checks passing")
                    case .pending: return ("clock", Theme.statusNotice, "Checks pending")
                    case .failure: return ("xmark.circle", Theme.statusBad, "Checks failing")
                    }
                }()
                Image(systemName: ciIcon)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(ciColor)
                    .help(ciTip)
            }

            // Comments count
            if item.comments > 0 {
                HStack(spacing: 3) {
                    Image(systemName: "bubble.right")
                        .font(.system(size: 10))
                    Text("\(item.comments)")
                        .font(Theme.fontMeta(monospaced: true))
                }
                .foregroundStyle(Theme.textTertiary)
            }

            // Relative updated time
            Text(item.updatedRelative)
                .font(Theme.fontMeta())
                .monospacedDigit()
                .foregroundStyle(Theme.textTertiary)
                .frame(width: 58, alignment: .trailing)
        }
        .padding(.horizontal, Theme.space3)
        .frame(height: 40)
        .background((isSelected || isHovered) ? Theme.surface2 : Theme.surface1)
        .overlay(alignment: .leading) {
            if isSelected {
                Rectangle().fill(Theme.accent).frame(width: 2)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture(count: 2) {
            onOpenBrowser()
        }
        .simultaneousGesture(
            TapGesture(count: 1).onEnded {
                onSelect()
            }
        )
        .onHover { hovering in
            isHovered = hovering
        }
    }
}

// MARK: - Starred / Explore Repository List Row (44px)

struct HubRepoRowView: View {
    let repo: HubRepo
    let isSelected: Bool
    let isClonedLocally: Bool
    let onSelect: () -> Void
    let onOpenBrowser: () -> Void

    @State private var isHovered: Bool = false

    private var iconName: String {
        if repo.isPrivate { return "lock" }
        if repo.isFork { return "arrow.triangle.branch" }
        return "book.closed"
    }

    var body: some View {
        HStack(spacing: Theme.space2) {
            Image(systemName: iconName)
                .font(.system(size: 13))
                .foregroundStyle(Theme.textSecondary)
                .frame(width: 20)

            // owner / name + badges
            HStack(spacing: 6) {
                HStack(spacing: 0) {
                    Text("\(repo.owner) / ")
                        .font(Theme.fontBody())
                        .foregroundStyle(Theme.textSecondary)
                    Text(repo.name)
                        .font(Theme.fontBody(weight: .medium))
                        .foregroundStyle(Theme.textPrimary)
                }
                .lineLimit(1)
                .truncationMode(.tail)

                if isClonedLocally {
                    Text("cloned")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(Theme.accent)
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(Theme.accent.opacity(0.14), in: RoundedRectangle(cornerRadius: 4))
                }

                if repo.isArchived {
                    Text("archived")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(Theme.statusNotice)
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(Theme.statusNotice.opacity(0.14), in: RoundedRectangle(cornerRadius: 4))
                }
            }
            .frame(width: 240, alignment: .leading)

            // Description
            Text(repo.description.isEmpty ? "No description" : repo.description)
                .font(Theme.fontBody())
                .foregroundStyle(repo.description.isEmpty ? Theme.textTertiary : Theme.textSecondary)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: .infinity, alignment: .leading)

            // Primary Language
            if let lang = repo.language {
                HStack(spacing: 5) {
                    Circle()
                        .fill(repo.languageColor ?? Theme.textTertiary)
                        .frame(width: 8, height: 8)
                    Text(lang)
                        .font(Theme.fontMeta())
                        .foregroundStyle(Theme.textSecondary)
                        .lineLimit(1)
                }
                .frame(width: 96, alignment: .trailing)
            }

            // Stars
            HStack(spacing: 3) {
                Image(systemName: repo.viewerHasStarred ? "star.fill" : "star")
                    .font(.system(size: 11))
                    .foregroundStyle(repo.viewerHasStarred ? Theme.statusNotice : Theme.textTertiary)
                Text(GitHubService.formatCount(repo.stars))
                    .font(Theme.fontMeta(monospaced: true))
                    .monospacedDigit()
                    .foregroundStyle(Theme.textSecondary)
            }
            .frame(width: 64, alignment: .trailing)

            // Forks
            if repo.forks > 0 {
                HStack(spacing: 3) {
                    Image(systemName: "arrow.triangle.branch")
                        .font(.system(size: 10))
                        .foregroundStyle(Theme.textTertiary)
                    Text(GitHubService.formatCount(repo.forks))
                        .font(Theme.fontMeta(monospaced: true))
                        .monospacedDigit()
                        .foregroundStyle(Theme.textTertiary)
                }
                .frame(width: 52, alignment: .trailing)
            }

            // Updated relative
            Text(repo.updatedRelative)
                .font(Theme.fontMeta())
                .monospacedDigit()
                .foregroundStyle(Theme.textTertiary)
                .frame(width: 58, alignment: .trailing)
        }
        .padding(.horizontal, Theme.space3)
        .frame(height: 42)
        .background((isSelected || isHovered) ? Theme.surface2 : Theme.surface1)
        .overlay(alignment: .leading) {
            if isSelected {
                Rectangle().fill(Theme.accent).frame(width: 2)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture(count: 2) {
            onOpenBrowser()
        }
        .simultaneousGesture(
            TapGesture(count: 1).onEnded {
                onSelect()
            }
        )
        .onHover { hovering in
            isHovered = hovering
        }
    }
}

// MARK: - Starred / Explore Social Preview Card View

struct HubRepoCardView: View {
    let repo: HubRepo
    let isSelected: Bool
    let isClonedLocally: Bool
    let onSelect: () -> Void
    let onOpenBrowser: () -> Void

    @State private var isHovered: Bool = false
    @State private var previewImage: NSImage? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Social Preview Banner (2:1 aspect ratio)
            GeometryReader { geo in
                ZStack(alignment: .bottom) {
                    if let img = previewImage {
                        Image(nsImage: img)
                            .resizable()
                            .scaledToFill()
                            .frame(width: geo.size.width, height: geo.size.height)
                            .clipped()
                    } else {
                        ZStack(alignment: .bottomLeading) {
                            Theme.surface2
                            VStack(alignment: .leading, spacing: 2) {
                                Text(repo.owner)
                                    .font(Theme.fontMeta())
                                    .foregroundStyle(Theme.textTertiary)
                                Text(repo.name)
                                    .font(Theme.fontTitle(weight: .medium))
                                    .foregroundStyle(Theme.textPrimary)
                                    .lineLimit(1)
                            }
                            .padding(Theme.space3)
                        }
                    }

                    if let langColor = repo.languageColor {
                        Rectangle()
                            .fill(langColor)
                            .frame(height: 3)
                    }
                }
            }
            .aspectRatio(2.0, contentMode: .fit)
            .clipped()

            Rectangle().fill(Theme.border).frame(height: 1)

            // Card Body
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 6) {
                    HStack(spacing: 0) {
                        Text("\(repo.owner) / ")
                            .font(Theme.fontMeta())
                            .foregroundStyle(Theme.textSecondary)
                        Text(repo.name)
                            .font(Theme.fontBody(weight: .medium))
                            .foregroundStyle(Theme.textPrimary)
                    }
                    .lineLimit(1)
                    .truncationMode(.tail)

                    Spacer(minLength: 4)

                    if isClonedLocally {
                        Text("cloned")
                            .font(.system(size: 10, weight: .medium))
                            .foregroundStyle(Theme.accent)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .background(Theme.accent.opacity(0.14), in: RoundedRectangle(cornerRadius: 4))
                    }
                }

                Text(repo.description.isEmpty ? "No description provided." : repo.description)
                    .font(Theme.fontMeta())
                    .foregroundStyle(repo.description.isEmpty ? Theme.textTertiary : Theme.textSecondary)
                    .lineLimit(2)
                    .frame(height: 32, alignment: .topLeading)

                // Footer: Language, Stars, Forks, Updated
                HStack(spacing: 10) {
                    if let lang = repo.language {
                        HStack(spacing: 4) {
                            Circle()
                                .fill(repo.languageColor ?? Theme.textTertiary)
                                .frame(width: 7, height: 7)
                            Text(lang)
                                .font(Theme.fontMeta())
                                .foregroundStyle(Theme.textSecondary)
                                .lineLimit(1)
                        }
                    }

                    HStack(spacing: 3) {
                        Image(systemName: repo.viewerHasStarred ? "star.fill" : "star")
                            .font(.system(size: 10))
                            .foregroundStyle(repo.viewerHasStarred ? Theme.statusNotice : Theme.textTertiary)
                        Text(GitHubService.formatCount(repo.stars))
                            .font(Theme.fontMeta(monospaced: true))
                            .foregroundStyle(Theme.textSecondary)
                    }

                    if repo.forks > 0 {
                        HStack(spacing: 3) {
                            Image(systemName: "arrow.triangle.branch")
                                .font(.system(size: 10))
                                .foregroundStyle(Theme.textTertiary)
                            Text(GitHubService.formatCount(repo.forks))
                                .font(Theme.fontMeta(monospaced: true))
                                .foregroundStyle(Theme.textTertiary)
                        }
                    }

                    Spacer(minLength: 0)

                    Text(repo.updatedRelative)
                        .font(Theme.fontMeta())
                        .foregroundStyle(Theme.textTertiary)
                }
            }
            .padding(Theme.space3)
        }
        .background((isSelected || isHovered) ? Theme.surface2 : Theme.surface1)
        .clipShape(RoundedRectangle(cornerRadius: Theme.radiusContainer))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.radiusContainer)
                .strokeBorder(
                    isSelected ? Theme.accent : (isHovered ? Theme.borderStrong : Theme.border),
                    lineWidth: isSelected ? 1.5 : 1
                )
        )
        .contentShape(RoundedRectangle(cornerRadius: Theme.radiusContainer))
        .onTapGesture(count: 2) {
            onOpenBrowser()
        }
        .simultaneousGesture(
            TapGesture(count: 1).onEnded {
                onSelect()
            }
        )
        .onHover { hovering in
            isHovered = hovering
        }
        .task(id: repo.openGraphImageURL) {
            guard let url = repo.openGraphImageURL else { return }
            if let cached = await SocialPreviewCache.shared.cachedImage(for: repo.nameWithOwner) {
                previewImage = cached
            } else if let fetched = await SocialPreviewCache.shared.fetchImage(for: repo.nameWithOwner, urlString: url) {
                previewImage = fetched
            }
        }
    }
}

// MARK: - GitHub Right-Hand Inspector

struct GitHubInspectorView: View {
    @ObservedObject var store: RepoStore
    @ObservedObject var hubStore: GitHubHubStore
    let section: HubSection

    var body: some View {
        Group {
            if section.isRepoList {
                if let repo = hubStore.selectedRepo(for: section) {
                    HubRepoInspectorView(store: store, hubStore: hubStore, repo: repo)
                } else {
                    emptyInspector(text: "Select a repository to view its README, topics, and clone options.")
                }
            } else {
                if let item = hubStore.selectedItem {
                    HubItemInspectorView(store: store, hubStore: hubStore, item: item)
                } else {
                    emptyInspector(text: "Select a pull request or issue to view its description, checks, and actions.")
                }
            }
        }
        .background(Theme.surface1)
    }

    private func emptyInspector(text: String) -> some View {
        VStack(spacing: Theme.space2) {
            Text("Nothing selected")
                .font(Theme.fontBody(weight: .medium))
                .foregroundStyle(Theme.textSecondary)
            Text(text)
                .font(Theme.fontMeta())
                .foregroundStyle(Theme.textTertiary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, Theme.space6)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct HubItemInspectorView: View {
    @ObservedObject var store: RepoStore
    @ObservedObject var hubStore: GitHubHubStore
    let item: HubItem

    var body: some View {
        let localClone = store.localClone(for: item.repoSlug)

        VStack(alignment: .leading, spacing: 0) {
            // Fixed Top Header
            VStack(alignment: .leading, spacing: Theme.space3) {
                Text("\(item.repoSlug)  #\(item.number)")
                    .font(Theme.fontMeta(monospaced: true))
                    .foregroundStyle(Theme.textTertiary)

                Text(item.title)
                    .font(Theme.fontTitle(weight: .medium))
                    .tracking(Theme.trackingTitle)
                    .foregroundStyle(Theme.textPrimary)
                    .fixedSize(horizontal: false, vertical: true)

                HStack(spacing: Theme.space2) {
                    RestyleButton(label: "Open on GitHub", icon: "globe", variant: .primary, compact: true) {
                        store.openURL(item.url)
                    }

                    if let local = localClone {
                        if item.isPR {
                            RestyleButton(
                                label: "Check Out",
                                icon: "arrow.triangle.branch",
                                variant: .secondary,
                                compact: true,
                                isDisabled: hubStore.busyAction != nil
                            ) {
                                hubStore.checkoutPR(localRepo: local, item: item, store: store)
                            }
                        }

                        RestyleButton(label: "Show Repository", icon: "folder", variant: .secondary, compact: true) {
                            store.focusLocalRepo(local.id)
                        }
                    }
                }

                // Metadata rows
                VStack(spacing: 0) {
                    inspectorMetaRow(
                        label: "Status",
                        value: item.isDraft ? "Draft" : (item.isPR ? "Open pull request" : "Open issue"),
                        trailing: item.reviewDecision?.label ?? ""
                    )
                    Divider().overlay(Theme.border)
                    inspectorMetaRow(
                        label: "Author",
                        value: "@\(item.author)",
                        trailing: item.comments > 0 ? "\(item.comments) comments" : ""
                    )
                    if item.isPR && !item.headRef.isEmpty {
                        Divider().overlay(Theme.border)
                        inspectorMetaRow(
                            label: "Branch",
                            value: "\(item.headRef)  →  \(item.baseRef)",
                            monospaced: true
                        )
                        Divider().overlay(Theme.border)
                        inspectorMetaRow(
                            label: "Diff",
                            value: "+\(item.additions)  −\(item.deletions)",
                            monospaced: true
                        )
                    }
                    if let ci = item.ciState {
                        Divider().overlay(Theme.border)
                        let ciText: String = {
                            switch ci {
                            case .success: return "Passing"
                            case .pending: return "Pending"
                            case .failure: return "Failing"
                            }
                        }()
                        inspectorMetaRow(label: "Checks", value: ciText)
                    }
                    Divider().overlay(Theme.border)
                    inspectorMetaRow(label: "Updated", value: item.updatedRelative)
                    if let local = localClone {
                        Divider().overlay(Theme.border)
                        inspectorMetaRow(label: "Local", value: local.displayPath, monospaced: true)
                    }
                }
                .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.radiusContainer)
                        .strokeBorder(Theme.border, lineWidth: 1)
                )
            }
            .padding(Theme.space4)

            Divider().overlay(Theme.border)

            // Scrollable Labels & Body
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.space4) {
                    if !item.labels.isEmpty {
                        VStack(alignment: .leading, spacing: Theme.space2) {
                            Text("Labels")
                                .font(Theme.fontMeta(weight: .medium))
                                .foregroundStyle(Theme.textSecondary)

                            FlowLayoutView(spacing: 6) {
                                ForEach(item.labels) { label in
                                    Text(label.name)
                                        .font(.system(size: 11, weight: .medium))
                                        .foregroundStyle(label.color)
                                        .padding(.horizontal, 7)
                                        .padding(.vertical, 3)
                                        .background(label.color.opacity(0.16), in: RoundedRectangle(cornerRadius: 4))
                                }
                            }
                        }
                    }

                    VStack(alignment: .leading, spacing: Theme.space2) {
                        Text("Description")
                            .font(Theme.fontMeta(weight: .medium))
                            .foregroundStyle(Theme.textSecondary)

                        VStack(alignment: .leading, spacing: 0) {
                            let bodyText = item.body.trimmingCharacters(in: .whitespacesAndNewlines)
                            if bodyText.isEmpty {
                                Text("No description provided.")
                                    .font(Theme.fontMeta())
                                    .foregroundStyle(Theme.textTertiary)
                            } else {
                                ReadmeMarkdownView(markdown: bodyText, repoPath: localClone?.path)
                            }
                        }
                        .padding(Theme.space3)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
                        .overlay(
                            RoundedRectangle(cornerRadius: Theme.radiusContainer)
                                .strokeBorder(Theme.border, lineWidth: 1)
                        )
                    }
                }
                .padding(Theme.space4)
            }
        }
    }
}

private struct HubRepoInspectorView: View {
    @ObservedObject var store: RepoStore
    @ObservedObject var hubStore: GitHubHubStore
    let repo: HubRepo

    var body: some View {
        let localClone = store.localClone(for: repo.nameWithOwner)
        let starBusy = hubStore.starringSlug == repo.nameWithOwner

        VStack(alignment: .leading, spacing: 0) {
            // Fixed Top Header
            VStack(alignment: .leading, spacing: Theme.space3) {
                HStack(spacing: 0) {
                    Text("\(repo.owner) / ")
                        .font(Theme.fontTitle(weight: .regular))
                        .foregroundStyle(Theme.textSecondary)
                    Text(repo.name)
                        .font(Theme.fontTitle(weight: .medium))
                        .foregroundStyle(Theme.textPrimary)
                }
                .lineLimit(1)

                if !repo.description.isEmpty {
                    Text(repo.description)
                        .font(Theme.fontBody())
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }

                HStack(spacing: Theme.space2) {
                    if let local = localClone {
                        RestyleButton(label: "Show Repository", icon: "folder", variant: .primary, compact: true) {
                            store.focusLocalRepo(local.id)
                        }
                    } else {
                        RestyleButton(
                            label: "Clone…",
                            icon: "arrow.down.to.line",
                            variant: .primary,
                            compact: true,
                            isDisabled: hubStore.busyAction != nil
                        ) {
                            hubStore.openCloneDialog(for: repo.nameWithOwner, scanFolders: store.scanFolders)
                        }
                    }

                    RestyleButton(
                        label: repo.viewerHasStarred ? "Starred" : "Star",
                        icon: repo.viewerHasStarred ? "star.fill" : "star",
                        variant: .secondary,
                        compact: true,
                        isDisabled: starBusy
                    ) {
                        hubStore.toggleStar(for: repo, store: store)
                    }

                    RestyleButton(label: "Open on GitHub", icon: "globe", variant: .secondary, compact: true) {
                        store.openURL(repo.url)
                    }
                }

                // Metadata rows
                VStack(spacing: 0) {
                    inspectorMetaRow(
                        label: "Stars",
                        value: GitHubService.formatCount(repo.stars),
                        trailing: "\(GitHubService.formatCount(repo.forks)) forks"
                    )
                    if let lang = repo.language {
                        Divider().overlay(Theme.border)
                        inspectorMetaRow(label: "Language", value: lang)
                    }
                    Divider().overlay(Theme.border)
                    inspectorMetaRow(label: "Branch", value: repo.defaultBranch, monospaced: true)
                    Divider().overlay(Theme.border)
                    inspectorMetaRow(
                        label: "Updated",
                        value: repo.updatedRelative.isEmpty ? "—" : repo.updatedRelative,
                        trailing: repo.isArchived ? "archived" : ""
                    )
                    Divider().overlay(Theme.border)
                    inspectorMetaRow(
                        label: "Visibility",
                        value: repo.isPrivate ? "Private" : "Public",
                        trailing: repo.isFork ? "fork" : ""
                    )
                    if let local = localClone {
                        Divider().overlay(Theme.border)
                        inspectorMetaRow(label: "Local", value: local.displayPath, monospaced: true)
                    }
                }
                .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.radiusContainer)
                        .strokeBorder(Theme.border, lineWidth: 1)
                )
            }
            .padding(Theme.space4)

            Divider().overlay(Theme.border)

            // Scrollable Topics & README
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.space4) {
                    if !repo.primaryTopics.isEmpty {
                        VStack(alignment: .leading, spacing: Theme.space2) {
                            Text("Topics")
                                .font(Theme.fontMeta(weight: .medium))
                                .foregroundStyle(Theme.textSecondary)

                            FlowLayoutView(spacing: 6) {
                                ForEach(repo.primaryTopics, id: \.self) { topic in
                                    Text(topic)
                                        .font(Theme.fontMeta())
                                        .foregroundStyle(Theme.textSecondary)
                                        .padding(.horizontal, 8)
                                        .padding(.vertical, 3)
                                        .background(Theme.surface2, in: RoundedRectangle(cornerRadius: 4))
                                }
                            }
                        }
                    }

                    VStack(alignment: .leading, spacing: Theme.space2) {
                        Text("README")
                            .font(Theme.fontMeta(weight: .medium))
                            .foregroundStyle(Theme.textSecondary)

                        VStack(alignment: .leading, spacing: 0) {
                            switch hubStore.readmes[repo.nameWithOwner] {
                            case .none, .some(.none):
                                HStack(spacing: Theme.space2) {
                                    DedicatedSpinIndicator(size: 14)
                                    Text("Loading README from GitHub…")
                                        .font(Theme.fontMeta())
                                        .foregroundStyle(Theme.textTertiary)
                                }
                            case .some(.some(let md)) where md.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty:
                                Text("No README found in this repository.")
                                    .font(Theme.fontMeta())
                                    .foregroundStyle(Theme.textTertiary)
                            case .some(.some(let md)):
                                ReadmeMarkdownView(markdown: md, repoPath: localClone?.path)
                            }
                        }
                        .padding(Theme.space3)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
                        .overlay(
                            RoundedRectangle(cornerRadius: Theme.radiusContainer)
                                .strokeBorder(Theme.border, lineWidth: 1)
                        )
                    }
                }
                .padding(Theme.space4)
            }
        }
        .onAppear {
            hubStore.ensureReadmeLoaded(for: repo.nameWithOwner)
        }
    }
}

private func inspectorMetaRow(
    label: String,
    value: String,
    trailing: String = "",
    monospaced: Bool = false
) -> some View {
    HStack(spacing: Theme.space2) {
        Text(label)
            .font(Theme.fontMeta())
            .foregroundStyle(Theme.textTertiary)
            .frame(width: 64, alignment: .leading)

        Text(value)
            .font(monospaced ? Theme.fontMeta(monospaced: true) : Theme.fontMeta())
            .foregroundStyle(Theme.textPrimary)
            .lineLimit(1)
            .truncationMode(.middle)

        Spacer(minLength: 4)

        if !trailing.isEmpty {
            Text(trailing)
                .font(Theme.fontMeta())
                .foregroundStyle(Theme.textTertiary)
                .lineLimit(1)
        }
    }
    .padding(.horizontal, Theme.space3)
    .frame(height: Theme.rowCompact)
}

// MARK: - Clone Repository Modal Sheet

struct CloneRepoSheet: View {
    @ObservedObject var store: RepoStore
    @ObservedObject var hubStore: GitHubHubStore
    let target: CloneDialogTarget

    @State private var destination: String = ""

    var body: some View {
        let repoName = target.slug.split(separator: "/").last.map(String.init) ?? target.slug

        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Clone \(target.slug)")
                    .font(Theme.fontTitle(weight: .medium))
                    .foregroundStyle(Theme.textPrimary)
                Spacer()
                RestyleIconButton(icon: "xmark", tooltip: "Close") {
                    hubStore.cloneDialog = nil
                }
            }
            .padding(.horizontal, Theme.space4)
            .frame(height: 44)
            .background(Theme.surface1)

            Rectangle().fill(Theme.border).frame(height: 1)

            VStack(alignment: .leading, spacing: Theme.space3) {
                Text("Destination folder")
                    .font(Theme.fontMeta(weight: .medium))
                    .foregroundStyle(Theme.textSecondary)

                HStack(spacing: Theme.space2) {
                    TextField("~/Development/\(repoName)", text: $destination)
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

                    RestyleButton(label: "Browse…", icon: "folder", variant: .secondary, compact: true) {
                        let panel = NSOpenPanel()
                        panel.title = "Select parent folder for \(repoName)"
                        panel.canChooseDirectories = true
                        panel.canChooseFiles = false
                        panel.allowsMultipleSelection = false
                        panel.canCreateDirectories = true
                        if panel.runModal() == .OK, let folder = panel.url {
                            destination = folder.appendingPathComponent(repoName).path
                        }
                    }
                }

                if !store.scanFolders.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Watched folders:")
                            .font(Theme.fontMeta())
                            .foregroundStyle(Theme.textTertiary)

                        FlowLayoutView(spacing: 6) {
                            ForEach(store.scanFolders, id: \.self) { folder in
                                let label = URL(fileURLWithPath: folder).lastPathComponent
                                RestyleButton(label: label, icon: "folder", variant: .secondary, compact: true) {
                                    destination = URL(fileURLWithPath: folder).appendingPathComponent(repoName).path
                                }
                            }
                        }
                    }
                }

                Spacer(minLength: Theme.space2)

                HStack {
                    Spacer()
                    RestyleButton(label: "Cancel", variant: .secondary, compact: true) {
                        hubStore.cloneDialog = nil
                    }
                    RestyleButton(
                        label: "Clone",
                        icon: "arrow.down.to.line",
                        variant: .primary,
                        compact: true,
                        isDisabled: destination.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                    ) {
                        hubStore.executeClone(slug: target.slug, destination: destination, store: store)
                    }
                }
            }
            .padding(Theme.space4)
            .background(Theme.surface1)
        }
        .frame(width: 500, height: 250)
        .onAppear {
            destination = target.destination
        }
    }
}

// MARK: - Fork Repository Modal Sheet (On GitHub & On This Mac)

struct ForkRepoSheet: View {
    @ObservedObject var store: RepoStore
    @ObservedObject var hubStore: GitHubHubStore
    let target: ForkDialogTarget

    @State private var mode: ForkMode = .github
    @State private var ownerChoice: ForkOwnerChoice = .personal
    @State private var localDestination: String = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Fork \"\(target.repo.name)\"")
                        .font(Theme.fontTitle(weight: .medium))
                        .foregroundStyle(Theme.textPrimary)
                    Text(target.repo.formattedRemoteHost ?? target.repo.displayPath)
                        .font(Theme.fontMeta(monospaced: true))
                        .foregroundStyle(Theme.textTertiary)
                        .lineLimit(1)
                }
                Spacer()
                RestyleIconButton(icon: "xmark", tooltip: "Close", isDisabled: target.isRunning) {
                    hubStore.forkDialog = nil
                }
            }
            .padding(.horizontal, Theme.space4)
            .frame(height: 52)
            .background(Theme.surface1)

            Rectangle().fill(Theme.border).frame(height: 1)

            VStack(alignment: .leading, spacing: Theme.space3) {
                // Segmented Mode Picker: On GitHub vs On this Mac
                HStack(spacing: 4) {
                    ForEach(ForkMode.allCases) { itemMode in
                        let selected = mode == itemMode
                        let disabled = (itemMode == .github && !target.repo.canFork) || target.isRunning
                        Button {
                            if !disabled {
                                mode = itemMode
                                hubStore.forkDialog?.mode = itemMode
                                hubStore.forkDialog?.errorMessage = nil
                            }
                        } label: {
                            HStack(spacing: 6) {
                                Image(systemName: itemMode == .github ? "globe" : "folder")
                                    .font(.system(size: 12))
                                Text(itemMode.rawValue)
                                    .font(Theme.fontBody(weight: selected ? .medium : .regular))
                            }
                            .foregroundStyle(selected ? Theme.textPrimary : Theme.textSecondary)
                            .frame(maxWidth: .infinity)
                            .frame(height: Theme.rowCompact)
                            .background(
                                selected ? Theme.surface2 : Color.clear,
                                in: RoundedRectangle(cornerRadius: 4)
                            )
                        }
                        .buttonStyle(.plain)
                        .disabled(disabled)
                        .opacity(disabled ? 0.45 : 1.0)
                    }
                }
                .padding(3)
                .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.radiusControl)
                        .strokeBorder(Theme.border, lineWidth: 1)
                )

                if mode == .github {
                    githubForkBody
                } else {
                    localForkBody
                }

                if let err = target.errorMessage {
                    HStack(spacing: Theme.space2) {
                        Image(systemName: "exclamationmark.triangle")
                            .foregroundStyle(Theme.statusBad)
                        Text(err)
                            .font(Theme.fontMeta())
                            .foregroundStyle(Theme.statusBad)
                            .lineLimit(2)
                    }
                    .padding(Theme.space2)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
                    .overlay(
                        RoundedRectangle(cornerRadius: Theme.radiusControl)
                            .strokeBorder(Theme.statusBad.opacity(0.45), lineWidth: 1)
                    )
                }

                Spacer(minLength: 0)

                // Footer Actions
                HStack {
                    if target.isRunning {
                        DedicatedSpinIndicator(size: 14)
                        Text(mode == .github ? "Creating fork on GitHub…" : "Creating local copy…")
                            .font(Theme.fontMeta())
                            .foregroundStyle(Theme.textSecondary)
                    }
                    Spacer()

                    RestyleButton(label: "Cancel", variant: .secondary, compact: true, isDisabled: target.isRunning) {
                        hubStore.forkDialog = nil
                    }

                    if mode == .github {
                        let isReady: Bool = {
                            if case .ready = hubStore.cliStatus { return true }
                            return false
                        }()
                        RestyleButton(
                            label: target.isRunning ? "Forking…" : "Create Fork",
                            icon: "arrow.triangle.branch",
                            variant: .primary,
                            compact: true,
                            isDisabled: target.isRunning || !isReady
                        ) {
                            hubStore.forkDialog?.mode = .github
                            hubStore.forkDialog?.ownerChoice = ownerChoice
                            hubStore.executeFork(store: store)
                        }
                    } else {
                        RestyleButton(
                            label: target.isRunning ? "Copying…" : "Create Local Copy",
                            icon: "arrow.triangle.branch",
                            variant: .primary,
                            compact: true,
                            isDisabled: target.isRunning || localDestination.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                        ) {
                            hubStore.forkDialog?.mode = .local
                            hubStore.forkDialog?.localDestination = localDestination
                            hubStore.executeFork(store: store)
                        }
                    }
                }
            }
            .padding(Theme.space4)
            .background(Theme.surface1)
        }
        .frame(width: 500, height: 390)
        .onAppear {
            mode = target.mode
            ownerChoice = target.ownerChoice
            localDestination = target.localDestination
        }
    }

    @ViewBuilder
    private var githubForkBody: some View {
        switch hubStore.cliStatus {
        case .checking:
            HStack(spacing: Theme.space2) {
                DedicatedSpinIndicator(size: 14)
                Text("Checking GitHub CLI (`gh`) authentication…")
                    .font(Theme.fontBody())
                    .foregroundStyle(Theme.textSecondary)
            }
            .padding(.vertical, Theme.space4)

        case .ready(let user, let orgs):
            let chosenOwner: String = {
                switch ownerChoice {
                case .personal: return user
                case .org(let o): return o
                }
            }()

            VStack(alignment: .leading, spacing: Theme.space2) {
                Text("Owner")
                    .font(Theme.fontMeta(weight: .medium))
                    .foregroundStyle(Theme.textSecondary)

                ScrollView {
                    VStack(spacing: 4) {
                        ownerOptionRow(
                            label: "\(user) (personal account)",
                            choice: .personal
                        )
                        ForEach(orgs, id: \.self) { org in
                            ownerOptionRow(label: org, choice: .org(org))
                        }
                    }
                }
                .frame(maxHeight: 110)

                // Remote plan preview card
                VStack(spacing: 0) {
                    inspectorMetaRow(
                        label: "Upstream",
                        value: "\(target.remotePlan.originalRemote)  →  \(target.repo.formattedRemoteHost ?? "original")",
                        trailing: target.remotePlan.renamesOrigin ? "renamed from origin" : "",
                        monospaced: true
                    )
                    Divider().overlay(Theme.border)
                    inspectorMetaRow(
                        label: "Push to",
                        value: "\(target.remotePlan.forkRemote)  →  github.com/\(chosenOwner)/\(target.repo.name)",
                        trailing: "pushDefault",
                        monospaced: true
                    )
                }
                .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.radiusContainer)
                        .strokeBorder(Theme.border, lineWidth: 1)
                )
            }

        case .notAuthenticated(let msg):
            VStack(alignment: .leading, spacing: Theme.space2) {
                Text("GitHub CLI is not signed in")
                    .font(Theme.fontBody(weight: .medium))
                    .foregroundStyle(Theme.textPrimary)
                Text(msg)
                    .font(Theme.fontMeta(monospaced: true))
                    .foregroundStyle(Theme.textSecondary)
                    .lineLimit(2)
                HStack(spacing: Theme.space2) {
                    RestyleButton(label: "Sign in with GitHub…", icon: "terminal", variant: .primary, compact: true) {
                        GitHubService.openSignInTerminal()
                    }
                    RestyleButton(label: "Check Again", icon: "arrow.clockwise", variant: .secondary, compact: true) {
                        Task {
                            hubStore.cliStatus = await GitHubService.cliStatus()
                        }
                    }
                }
            }
            .padding(Theme.space3)
            .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radiusContainer)
                    .strokeBorder(Theme.border, lineWidth: 1)
            )

        case .notInstalled:
            VStack(alignment: .leading, spacing: Theme.space2) {
                Text("GitHub CLI (`gh`) is not installed")
                    .font(Theme.fontBody(weight: .medium))
                    .foregroundStyle(Theme.textPrimary)
                Text("GitDaddy uses `gh repo fork` so your existing credentials and SSH/HTTPS preferences work out of the box.")
                    .font(Theme.fontMeta())
                    .foregroundStyle(Theme.textSecondary)
                HStack(spacing: Theme.space2) {
                    RestyleButton(label: "Install GitHub CLI…", icon: "globe", variant: .primary, compact: true) {
                        store.openURL("https://cli.github.com")
                    }
                    RestyleButton(label: "Check Again", icon: "arrow.clockwise", variant: .secondary, compact: true) {
                        Task {
                            hubStore.cliStatus = await GitHubService.cliStatus()
                        }
                    }
                }
            }
            .padding(Theme.space3)
            .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radiusContainer)
                    .strokeBorder(Theme.border, lineWidth: 1)
            )
        }
    }

    private func ownerOptionRow(label: String, choice: ForkOwnerChoice) -> some View {
        let selected = ownerChoice == choice
        return Button {
            ownerChoice = choice
            hubStore.forkDialog?.ownerChoice = choice
        } label: {
            HStack(spacing: Theme.space2) {
                Image(systemName: selected ? "largecircle.fill.circle" : "circle")
                    .font(.system(size: 12))
                    .foregroundStyle(selected ? Theme.accent : Theme.textTertiary)
                Text(label)
                    .font(Theme.fontBody(weight: selected ? .medium : .regular))
                    .foregroundStyle(Theme.textPrimary)
                Spacer()
            }
            .padding(.horizontal, Theme.space3)
            .frame(height: Theme.rowCompact)
            .background(selected ? Theme.surface2 : Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radiusControl)
                    .strokeBorder(selected ? Theme.accent.opacity(0.5) : Theme.border, lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }

    private var localForkBody: some View {
        let newFolderName = URL(fileURLWithPath: localDestination.trimmingCharacters(in: .whitespacesAndNewlines)).lastPathComponent

        return VStack(alignment: .leading, spacing: Theme.space2) {
            Text("New folder path")
                .font(Theme.fontMeta(weight: .medium))
                .foregroundStyle(Theme.textSecondary)

            HStack(spacing: Theme.space2) {
                TextField("/path/to/\(target.repo.name)-fork", text: $localDestination)
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

                RestyleButton(label: "Browse…", icon: "folder", variant: .secondary, compact: true) {
                    let panel = NSOpenPanel()
                    panel.title = "Choose parent folder for local fork"
                    panel.canChooseDirectories = true
                    panel.canChooseFiles = false
                    panel.allowsMultipleSelection = false
                    panel.canCreateDirectories = true
                    if panel.runModal() == .OK, let parent = panel.url {
                        let leaf = newFolderName.isEmpty ? "\(target.repo.name)-fork" : newFolderName
                        localDestination = parent.appendingPathComponent(leaf).path
                    }
                }
            }

            VStack(spacing: 0) {
                inspectorMetaRow(label: "Source", value: target.repo.displayPath, monospaced: true)
                Divider().overlay(Theme.border)
                inspectorMetaRow(
                    label: "Remotes",
                    value: target.repo.formattedRemoteHost ?? "None (local-only copy)",
                    trailing: target.repo.hasRemote ? "preserved, not linked to source" : ""
                )
            }
            .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radiusContainer)
                    .strokeBorder(Theme.border, lineWidth: 1)
            )

            Text("Creates a standalone repository on disk with all branches, tags, and remotes copied over. The new copy does not use the original folder as its `origin` remote.")
                .font(Theme.fontMeta())
                .foregroundStyle(Theme.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

// MARK: - Software Update Modal Sheet

struct SoftwareUpdateSheet: View {
    @ObservedObject var updater: UpdaterStore
    @ObservedObject var store: RepoStore

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Software Update")
                    .font(Theme.fontTitle(weight: .medium))
                    .foregroundStyle(Theme.textPrimary)
                Spacer()
                Text("Current: v\(UpdaterStore.appVersion)")
                    .font(Theme.fontMeta(monospaced: true))
                    .foregroundStyle(Theme.textTertiary)
                RestyleIconButton(icon: "xmark", tooltip: "Close") {
                    updater.showModal = false
                }
            }
            .padding(.horizontal, Theme.space4)
            .frame(height: 44)
            .background(Theme.surface1)

            Rectangle().fill(Theme.border).frame(height: 1)

            VStack(alignment: .leading, spacing: Theme.space3) {
                switch updater.status {
                case .idle, .checking:
                    HStack(spacing: Theme.space2) {
                        DedicatedSpinIndicator(size: 16)
                        Text("Checking GitHub Releases for updates…")
                            .font(Theme.fontBody())
                            .foregroundStyle(Theme.textSecondary)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)

                case .upToDate:
                    VStack(alignment: .leading, spacing: Theme.space3) {
                        HStack(spacing: Theme.space2) {
                            Image(systemName: "checkmark.circle")
                                .font(.system(size: 16, weight: .medium))
                                .foregroundStyle(Theme.statusGood)
                            VStack(alignment: .leading, spacing: 2) {
                                Text("GitDaddy is up to date")
                                    .font(Theme.fontBody(weight: .medium))
                                    .foregroundStyle(Theme.textPrimary)
                                Text("v\(UpdaterStore.appVersion) is the latest version available.")
                                    .font(Theme.fontMeta())
                                    .foregroundStyle(Theme.textSecondary)
                            }
                        }
                        Spacer()
                        HStack {
                            RestyleButton(label: "Check Again", icon: "arrow.clockwise", variant: .secondary, compact: true) {
                                updater.check(openModal: true)
                            }
                            RestyleButton(label: "View Releases", icon: "globe", variant: .secondary, compact: true) {
                                store.openURL(UpdaterStore.releasesPageURL)
                            }
                            Spacer()
                            RestyleButton(label: "Close", variant: .primary, compact: true) {
                                updater.showModal = false
                            }
                        }
                    }

                case .available(let info):
                    VStack(alignment: .leading, spacing: Theme.space3) {
                        HStack(spacing: Theme.space2) {
                            Image(systemName: "arrow.down.circle")
                                .font(.system(size: 16, weight: .medium))
                                .foregroundStyle(Theme.statusNotice)
                            VStack(alignment: .leading, spacing: 2) {
                                Text("GitDaddy v\(info.version) is available")
                                    .font(Theme.fontBody(weight: .medium))
                                    .foregroundStyle(Theme.textPrimary)
                                Text(info.publishedAt.isEmpty ? "You have v\(UpdaterStore.appVersion)" : "Published \(info.publishedAt) • you have v\(UpdaterStore.appVersion)")
                                    .font(Theme.fontMeta())
                                    .foregroundStyle(Theme.textSecondary)
                            }
                        }

                        if !info.notes.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                            ScrollView {
                                ReadmeMarkdownView(markdown: info.notes)
                                    .padding(Theme.space3)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                            .frame(maxHeight: 160)
                            .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.radiusContainer))
                            .overlay(
                                RoundedRectangle(cornerRadius: Theme.radiusContainer)
                                    .strokeBorder(Theme.border, lineWidth: 1)
                            )
                        }

                        Spacer()

                        HStack {
                            RestyleButton(
                                label: info.assetName.map { "Download \($0)" } ?? "Download Update",
                                icon: "arrow.down.to.line",
                                variant: .primary,
                                compact: true
                            ) {
                                store.openURL(info.downloadURL ?? info.htmlURL)
                            }
                            RestyleButton(label: "Release Page", icon: "globe", variant: .secondary, compact: true) {
                                store.openURL(info.htmlURL)
                            }
                            Spacer()
                            RestyleButton(label: "Later", variant: .secondary, compact: true) {
                                updater.showModal = false
                            }
                        }
                    }

                case .failed(let err):
                    VStack(alignment: .leading, spacing: Theme.space3) {
                        HStack(spacing: Theme.space2) {
                            Image(systemName: "exclamationmark.triangle")
                                .font(.system(size: 16, weight: .medium))
                                .foregroundStyle(Theme.statusBad)
                            VStack(alignment: .leading, spacing: 2) {
                                Text("Couldn't check for updates")
                                    .font(Theme.fontBody(weight: .medium))
                                    .foregroundStyle(Theme.textPrimary)
                                Text(err)
                                    .font(Theme.fontMeta(monospaced: true))
                                    .foregroundStyle(Theme.textSecondary)
                                    .lineLimit(2)
                            }
                        }
                        Spacer()
                        HStack {
                            RestyleButton(label: "Retry", icon: "arrow.clockwise", variant: .primary, compact: true) {
                                updater.check(openModal: true)
                            }
                            RestyleButton(label: "Open Releases", icon: "globe", variant: .secondary, compact: true) {
                                store.openURL(UpdaterStore.releasesPageURL)
                            }
                            Spacer()
                            RestyleButton(label: "Close", variant: .secondary, compact: true) {
                                updater.showModal = false
                            }
                        }
                    }
                }
            }
            .padding(Theme.space4)
            .background(Theme.surface1)
        }
        .frame(width: 480, height: 300)
    }
}

// MARK: - Simple Wrapping Flow Layout for Topics & Labels

struct FlowLayoutView<Content: View>: View {
    var spacing: CGFloat = 6
    @ViewBuilder var content: () -> Content

    var body: some View {
        _FlowLayout(spacing: spacing) {
            content()
        }
    }
}

private struct _FlowLayout: Layout {
    let spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxWidth = proposal.width ?? .infinity
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > 0 && x + size.width > maxWidth {
                x = 0
                y += rowHeight + spacing
                rowHeight = 0
            }
            rowHeight = max(rowHeight, size.height)
            x += size.width + spacing
        }
        return CGSize(width: maxWidth.isFinite ? maxWidth : x, height: y + rowHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX
        var y = bounds.minY
        var rowHeight: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > bounds.minX && x + size.width > bounds.maxX {
                x = bounds.minX
                y += rowHeight + spacing
                rowHeight = 0
            }
            subview.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            rowHeight = max(rowHeight, size.height)
            x += size.width + spacing
        }
    }
}
