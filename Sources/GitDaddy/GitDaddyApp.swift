import AppKit
import SwiftUI

final class AppDelegate: NSObject, NSApplicationDelegate {
    static let appLogoImage: NSImage? = {
        if let resURL = Bundle.main.resourceURL {
            let pngURL = resURL.appendingPathComponent("AppIcon.png")
            if let img = NSImage(contentsOf: pngURL) {
                return img
            }
            let icnsURL = resURL.appendingPathComponent("AppIcon.icns")
            if let img = NSImage(contentsOf: icnsURL) {
                return img
            }
        }
        let fallbackURL = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
            .appendingPathComponent("Assets/AppIcon.png")
        if let img = NSImage(contentsOf: fallbackURL) {
            return img
        }
        return nil
    }()

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApplication.shared.setActivationPolicy(.regular)
        if let logo = Self.appLogoImage {
            NSApplication.shared.applicationIconImage = logo
        }
        NSApplication.shared.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }
}

@main
struct GitDaddyApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @StateObject private var store = RepoStore()
    @StateObject private var hubStore = GitHubHubStore()
    @StateObject private var updater = UpdaterStore()

    var body: some Scene {
        WindowGroup("GitDaddy — Git Repository Update Manager") {
            ContentView(store: store, hubStore: hubStore, updater: updater)
                .frame(minWidth: 980, idealWidth: 1180, minHeight: 620, idealHeight: 760)
                .task {
                    await store.scanDirectories(checkRemoteAfterScan: true)
                }
        }
        .windowToolbarStyle(.unified(showsTitle: true))
        .commands {
            CommandGroup(after: .appInfo) {
                Button("Check for Updates…") {
                    updater.check(openModal: true)
                }
            }

            CommandMenu("Repositories") {
                Button("Add Folder to Scan…") {
                    store.promptToAddFolder()
                }
                .keyboardShortcut("o", modifiers: [.command])

                Button("Rescan Watched Folders") {
                    Task {
                        await store.scanDirectories(checkRemoteAfterScan: false)
                    }
                }
                .keyboardShortcut("r", modifiers: [.command, .shift])

                Button("Filter Repositories…") {
                    store.centerMode = .repositories
                    NotificationCenter.default.post(name: .focusSearchField, object: nil)
                }
                .keyboardShortcut("f", modifiers: [.command])

                Divider()

                Button("Check All Remotes (git fetch)") {
                    Task {
                        await store.checkAllRepos()
                    }
                }
                .keyboardShortcut("r", modifiers: [.command])

                Button("Pull Selected Repositories") {
                    Task {
                        await store.pullSelectedRepos()
                    }
                }
                .keyboardShortcut("p", modifiers: [.command])
                .disabled(store.selectedRepoIDs.isEmpty)

                Button("Update All Outdated Repositories") {
                    Task {
                        await store.pullAllOutdatedRepos()
                    }
                }
                .keyboardShortcut("p", modifiers: [.command, .shift])
                .disabled(store.outdatedReposCount == 0)

                Divider()

                Button("Push Selected Repositories") {
                    Task {
                        await store.pushSelectedRepos()
                    }
                }
                .keyboardShortcut("u", modifiers: [.command])
                .disabled(store.selectedRepoIDs.isEmpty)

                Button("Push All Unpushed Repositories") {
                    Task {
                        await store.pushAllUnpushedRepos()
                    }
                }
                .keyboardShortcut("u", modifiers: [.command, .shift])
                .disabled(store.unpushedReposCount == 0)

                Divider()

                Button("Fork Focused Repository…") {
                    if let focused = store.focusedRepo {
                        hubStore.openForkDialog(for: focused)
                    }
                }
                .disabled(store.focusedRepo == nil)

                Divider()

                Button("Select All Visible") {
                    store.selectAllVisible()
                }
                .keyboardShortcut("a", modifiers: [.command, .shift])

                Button("Select Outdated Only") {
                    store.selectAllOutdated()
                }

                Button("Select Unpushed Only") {
                    store.selectAllUnpushed()
                }

                Button("Clear Selection") {
                    store.clearSelection()
                }
            }

            CommandMenu("GitHub") {
                ForEach(HubSection.allCases) { section in
                    Button(section.label) {
                        store.centerMode = .github(section)
                        hubStore.selectSection(section, force: false)
                    }
                }

                Divider()

                Button("Refresh GitHub Data") {
                    hubStore.refreshCounts()
                    if case .github(let sec) = store.centerMode {
                        hubStore.selectSection(sec, force: true)
                    }
                }
            }
        }
    }
}

