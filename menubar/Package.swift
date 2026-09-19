// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "SailorDot",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(name: "SailorDot", path: "Sources/SailorDot")
    ]
)
