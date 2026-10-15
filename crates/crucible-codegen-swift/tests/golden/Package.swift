// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "WeatherClient",
    platforms: [.macOS(.v12), .iOS(.v15)],
    products: [.library(name: "WeatherClient", targets: ["WeatherClient"])],
    targets: [.target(name: "WeatherClient", resources: [.process("Resources")])]
)
