// swift-tools-version: 6.0
import Foundation
import PackageDescription

let rustLibraryDirectory = ProcessInfo.processInfo.environment["AUVEMIR_IOS_LIB_DIR"]
    ?? URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .appendingPathComponent("target/aarch64-apple-ios/release", isDirectory: true)
        .path

let package = Package(
    name: "Auvemir",
    platforms: [.iOS(.v17)],
    products: [.library(name: "Auvemir", targets: ["Auvemir"])],
    targets: [
        .target(
            name: "Auvemir",
            linkerSettings: [
                .unsafeFlags(["-L\(rustLibraryDirectory)"]),
                .linkedLibrary("auvemir"),
                .linkedLibrary("c++"),
                .linkedLibrary("sqlite3"),
                .linkedFramework("UIKit"),
                .linkedFramework("AVFoundation"),
                .linkedFramework("AVKit"),
                .linkedFramework("MediaPlayer"),
                .linkedFramework("CarPlay"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("CoreGraphics"),
                .linkedFramework("CoreText"),
                .linkedFramework("Foundation"),
                .linkedFramework("Security"),
                .linkedFramework("Metal"),
                .linkedFramework("MetalKit"),
                .linkedFramework("CoreVideo"),
                .linkedFramework("OpenGLES"),
                .linkedFramework("QuartzCore"),
                .linkedFramework("SystemConfiguration"),
                .linkedFramework("UniformTypeIdentifiers"),
            ]
        ),
    ],
    swiftLanguageModes: [.v5]
)
