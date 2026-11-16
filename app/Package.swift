// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Tilde",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(url: "https://github.com/migueldeicaza/SwiftTerm", from: "1.99.0"),
    ],
    targets: [
        .target(name: "TildeCore", path: "Sources/TildeCore"),
        .executableTarget(name: "Tilde", dependencies: ["TildeCore", "SwiftTerm"], path: "Sources/Tilde"),
        .testTarget(name: "TildeCoreTests", dependencies: ["TildeCore"], path: "Tests/TildeCoreTests"),
    ]
)
