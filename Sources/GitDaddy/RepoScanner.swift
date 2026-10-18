import Foundation

struct RepoScanner {
    static let ignoredDirectoryNames: Set<String> = [
        ".git",
        "node_modules",
        ".build",
        "DerivedData",
        "Pods",
        "Carthage",
        "venv",
        ".venv",
        "env",
        "__pycache__",
        ".pytest_cache",
        ".mypy_cache",
        "target",
        "dist",
        "build",
        "out",
        ".next",
        ".nuxt",
        ".svelte-kit",
        ".turbo",
        ".gradle",
        ".idea",
        ".vscode",
        "vendor",
        "bower_components",
        ".Trash",
        "Library"
    ]

    /// Scans the provided root folder paths and returns discovered Git repository paths.
    static func discoverRepositories(
        in rootFolders: [String],
        maxDepth: Int = 5,
        scanNestedRepos: Bool = false
    ) async -> [(repoPath: String, rootFolder: String)] {
        await Task.detached(priority: .userInitiated) {
            var discovered: [(repoPath: String, rootFolder: String)] = []
            var seenPaths = Set<String>()
            let fm = FileManager.default

            for rawRoot in rootFolders {
                let rootURL = URL(fileURLWithPath: rawRoot).standardizedFileURL
                let rootPath = rootURL.path

                var isDir: ObjCBool = false
                guard fm.fileExists(atPath: rootPath, isDirectory: &isDir), isDir.boolValue else {
                    continue
                }

                // Check if the root folder itself is a Git repository
                if isGitRepository(at: rootURL, fileManager: fm) {
                    if seenPaths.insert(rootPath).inserted {
                        discovered.append((repoPath: rootPath, rootFolder: rootPath))
                    }
                    if !scanNestedRepos {
                        continue
                    }
                }

                let resourceKeys: Set<URLResourceKey> = [.isDirectoryKey, .isSymbolicLinkKey, .nameKey]
                guard let enumerator = fm.enumerator(
                    at: rootURL,
                    includingPropertiesForKeys: Array(resourceKeys),
                    options: [.skipsPackageDescendants]
                ) else {
                    continue
                }

                while let fileURL = enumerator.nextObject() as? URL {
                    let depth = enumerator.level
                    if depth > maxDepth {
                        enumerator.skipDescendants()
                        continue
                    }

                    guard let values = try? fileURL.resourceValues(forKeys: resourceKeys) else {
                        continue
                    }

                    if values.isSymbolicLink == true {
                        continue
                    }

                    guard values.isDirectory == true else {
                        continue
                    }

                    let folderName = values.name ?? fileURL.lastPathComponent

                    // Skip hidden directories and heavy build/dependency directories
                    if folderName.hasPrefix(".") || ignoredDirectoryNames.contains(folderName) {
                        enumerator.skipDescendants()
                        continue
                    }

                    if isGitRepository(at: fileURL, fileManager: fm) {
                        let standardized = fileURL.standardizedFileURL.path
                        if seenPaths.insert(standardized).inserted {
                            discovered.append((repoPath: standardized, rootFolder: rootPath))
                        }
                        if !scanNestedRepos {
                            enumerator.skipDescendants()
                        }
                    }
                }
            }

            return discovered.sorted {
                $0.repoPath.localizedStandardCompare($1.repoPath) == .orderedAscending
            }
        }.value
    }

    private static func isGitRepository(at directoryURL: URL, fileManager: FileManager) -> Bool {
        let gitPath = directoryURL.appendingPathComponent(".git").path
        return fileManager.fileExists(atPath: gitPath)
    }
}
