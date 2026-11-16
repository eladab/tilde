// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Tilde",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(url: "https://github.com/migueldeicaza/SwiftTerm", from: "1.99.0"),
        .package(url: "https://github.com/sparkle-project/Sparkle", from: "2.6.0"),
    ],
    targets: [
        .target(name: "TildeCore", path: "Sources/TildeCore"),
        .executableTarget(name: "Tilde", dependencies: ["TildeCore", "SwiftTerm", .product(name: "Sparkle", package: "Sparkle")],
            path: "Sources/Tilde",
            // Where Tilde.app keeps Sparkle.framework (see scripts/bundle.sh).
            linkerSettings: [.unsafeFlags(["-Xlinker", "-rpath", "-Xlinker", "@executable_path/../Frameworks"])]
        ),
        .testTarget(name: "TildeCoreTests", dependencies: ["TildeCore"], path: "Tests/TildeCoreTests"),
    ]
)
