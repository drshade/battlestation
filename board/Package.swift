// swift-tools-version:5.10
// The Battlestation menu-bar app (macOS): the Team Board, the Deck and the
// Switchboard over `bsctl status --stream`. Built and bundled by bin/build;
// see board/README.md.
import PackageDescription

let package = Package(
    name: "Battlestation",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(name: "Battlestation", path: "Sources/Battlestation")
    ]
)
