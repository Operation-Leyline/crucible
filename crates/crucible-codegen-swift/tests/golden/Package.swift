// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "WeatherClient",
    platforms: [.macOS(.v12), .iOS(.v15)],
    products: [.library(name: "WeatherClient", targets: ["WeatherClient"])],
    dependencies: [.package(url: "https://github.com/apple/swift-crypto.git", exact: "3.10.0")],
    targets: [.target(name: "WeatherClient", dependencies: [.product(name: "Crypto", package: "swift-crypto")], resources: [.process("Resources")])]
)
