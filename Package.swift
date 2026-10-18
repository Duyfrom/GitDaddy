// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "GitDaddy",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .executable(name: "GitDaddy", targets: ["GitDaddy"])
    ],
    targets: [
        .executableTarget(
            name: "GitDaddy",
            path: "Sources/GitDaddy"
        )
    ]
)
