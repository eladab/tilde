// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Weft",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(url: "https://github.com/migueldeicaza/SwiftTerm", from: "1.99.0"),
    ],
    targets: [
        .target(name: "WeftCore", path: "Sources/WeftCore"),
        .executableTarget(name: "Weft", dependencies: ["WeftCore", "SwiftTerm"], path: "Sources/Weft"),
        .testTarget(name: "WeftCoreTests", dependencies: ["WeftCore"], path: "Tests/WeftCoreTests"),
    ]
)
