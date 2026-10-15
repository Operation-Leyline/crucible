#![cfg(any(
    feature = "ts",
    feature = "python",
    feature = "go",
    feature = "csharp",
    feature = "kotlin",
    feature = "swift",
    feature = "dart",
    feature = "php",
    feature = "ruby",
    feature = "cpp",
    feature = "c",
    feature = "r"
))]

use std::fs;
use std::process::Command;

use crucible_build::Client;

#[test]
fn fluent_probe() {
    if std::env::var_os("CRUCIBLE_FLUENT_PROBE").is_none() {
        return;
    }
    let package = crucible_build::model("model.json").compile();
    macro_rules! equivalent {
        ($name:literal, $fluent:expr, $native:expr) => {{
            let fluent = $fluent
                .try_generate_into(concat!("exports/", $name))
                .unwrap();
            let generic = package.sdk(concat!("raw-", $name), $native).generate();
            assert_eq!(fluent.target, generic.target);
            assert_eq!(fluent.files, generic.files);
            for path in &fluent.files {
                assert_eq!(
                    fs::read(fluent.path.join(path)).unwrap(),
                    fs::read(generic.path.join(path)).unwrap(),
                    "{}: {}",
                    $name,
                    path
                );
            }
        }};
    }
    #[cfg(feature = "ts")]
    {
        package.ts_client("defaults-ts").try_generate().unwrap();
        equivalent!(
            "ts",
            package
                .ts_client("ts")
                .package("weather-sdk")
                .version("1.2.3")
                .client_name("CustomWeatherClient")
                .endpoint("https://sdk.example")
                .package_json(serde_json::json!({"bugs": "https://sdk.example/issues"})),
            Client::Ts(crucible_build::TsOptions {
                package_name: "weather-sdk".into(),
                package_version: "1.2.3".into(),
                client_name: Some("CustomWeatherClient".into()),
                endpoint: Some("https://sdk.example".into()),
                package_json: Some(serde_json::json!({"bugs": "https://sdk.example/issues"})),
                metadata: Default::default(),
            })
        );
    }
    #[cfg(feature = "python")]
    {
        package
            .python_client("defaults-python")
            .try_generate()
            .unwrap();
        equivalent!(
            "python",
            package
                .python_client("python")
                .package("weather-sdk")
                .version("1.2.3")
                .endpoint("https://sdk.example")
                .module_name("weather_client"),
            Client::Python(crucible_build::PythonOptions {
                package_name: "weather-sdk".into(),
                package_version: "1.2.3".into(),
                endpoint: Some("https://sdk.example".into()),
                module_name: "weather_client".into()
            })
        );
    }
    #[cfg(feature = "go")]
    {
        package.go_client("defaults-go").try_generate().unwrap();
        equivalent!(
            "go",
            package
                .go_client("go")
                .module_path("example.com/weather-sdk")
                .package("weather_sdk")
                .endpoint("https://sdk.example"),
            Client::Go(crucible_build::GoOptions {
                module_path: "example.com/weather-sdk".into(),
                package_name: "weather_sdk".into(),
                endpoint: Some("https://sdk.example".into()),
            })
        );
    }
    #[cfg(feature = "csharp")]
    {
        package
            .csharp_client("defaults-csharp")
            .try_generate()
            .unwrap();
        equivalent!(
            "csharp",
            package
                .csharp_client("csharp")
                .package("Weather.SDK")
                .namespace("Example.Weather")
                .version("1.2.3")
                .endpoint("https://sdk.example"),
            Client::Csharp(crucible_build::CsharpOptions {
                namespace: "Example.Weather".into(),
                package_id: "Weather.SDK".into(),
                package_version: "1.2.3".into(),
                endpoint: Some("https://sdk.example".into()),
            })
        );
    }
    #[cfg(feature = "kotlin")]
    {
        package
            .kotlin_client("defaults-kotlin")
            .try_generate()
            .unwrap();
        equivalent!(
            "kotlin",
            package
                .kotlin_client("kotlin")
                .group_id("example.weather")
                .artifact_id("weather-sdk")
                .package("example.weather")
                .version("1.2.3")
                .endpoint("https://sdk.example"),
            Client::Kotlin(crucible_build::KotlinOptions {
                group_id: "example.weather".into(),
                artifact_id: "weather-sdk".into(),
                package_name: "example.weather".into(),
                package_version: "1.2.3".into(),
                endpoint: Some("https://sdk.example".into()),
            })
        );
    }
    #[cfg(feature = "swift")]
    {
        package
            .swift_client("defaults-swift")
            .try_generate()
            .unwrap();
        let simple = package
            .swift_client("swift-simple")
            .package("MyServiceSDK")
            .generate();
        assert!(simple.path.join("Sources/MyServiceSDK").is_dir());
        let manifest = fs::read_to_string(simple.path.join("Package.swift")).unwrap();
        assert!(manifest.contains("name: \"MyServiceSDK\""));
        let explicit = package
            .swift_client("swift-module")
            .module_name("SharedAPI")
            .package("DistributionSDK")
            .try_generate()
            .unwrap();
        assert!(explicit.path.join("Sources/SharedAPI").is_dir());
        equivalent!(
            "swift",
            package
                .swift_client("swift")
                .package("WeatherSDK")
                .version("1.2.3")
                .endpoint("https://sdk.example")
                .module_name("WeatherClientSDK"),
            Client::Swift(crucible_build::SwiftOptions {
                package_name: "WeatherSDK".into(),
                package_version: "1.2.3".into(),
                endpoint: Some("https://sdk.example".into()),
                module_name: "WeatherClientSDK".into()
            })
        );
    }
    #[cfg(feature = "dart")]
    {
        package.dart_client("defaults-dart").try_generate().unwrap();
        equivalent!(
            "dart",
            package
                .dart_client("dart")
                .package("weather_sdk")
                .version("1.2.3")
                .endpoint("https://sdk.example"),
            Client::Dart(crucible_build::DartOptions {
                package_name: "weather_sdk".into(),
                package_version: "1.2.3".into(),
                endpoint: Some("https://sdk.example".into()),
            })
        );
    }
    #[cfg(feature = "php")]
    {
        package.php_client("defaults-php").try_generate().unwrap();
        equivalent!(
            "php",
            package
                .php_client("php")
                .package("example/weather-sdk")
                .version("1.2.3")
                .endpoint("https://sdk.example")
                .namespace("ExampleWeather"),
            Client::Php(crucible_build::PhpOptions {
                package_name: "example/weather-sdk".into(),
                package_version: "1.2.3".into(),
                endpoint: Some("https://sdk.example".into()),
                namespace: "ExampleWeather".into()
            })
        );
    }
    #[cfg(feature = "ruby")]
    {
        package.ruby_client("defaults-ruby").try_generate().unwrap();
        equivalent!(
            "ruby",
            package
                .ruby_client("ruby")
                .package("weather_sdk")
                .version("1.2.3")
                .endpoint("https://sdk.example")
                .module_name("WeatherClientSDK"),
            Client::Ruby(crucible_build::RubyOptions {
                package_name: "weather_sdk".into(),
                package_version: "1.2.3".into(),
                endpoint: Some("https://sdk.example".into()),
                module_name: "WeatherClientSDK".into()
            })
        );
    }
    #[cfg(feature = "cpp")]
    {
        package.cpp_client("defaults-cpp").try_generate().unwrap();
        equivalent!(
            "cpp",
            package
                .cpp_client("cpp")
                .package("weather_sdk")
                .version("1.2.3")
                .endpoint("https://sdk.example")
                .namespace("ExampleWeather"),
            Client::Cpp(crucible_build::CppOptions {
                package_name: "weather_sdk".into(),
                package_version: "1.2.3".into(),
                endpoint: Some("https://sdk.example".into()),
                namespace: "ExampleWeather".into()
            })
        );
    }
    #[cfg(feature = "c")]
    {
        package.c_client("defaults-c").try_generate().unwrap();
        equivalent!(
            "c",
            package
                .c_client("c")
                .package("weather_sdk")
                .version("1.2.3")
                .endpoint("https://sdk.example")
                .prefix("weather"),
            Client::C(crucible_build::COptions {
                package_name: "weather_sdk".into(),
                package_version: "1.2.3".into(),
                endpoint: Some("https://sdk.example".into()),
                prefix: "weather".into()
            })
        );
    }
    #[cfg(feature = "r")]
    {
        package.r_client("defaults-r").try_generate().unwrap();
        equivalent!(
            "r",
            package
                .r_client("r")
                .package("weather.sdk")
                .version("1.2.3")
                .endpoint("https://sdk.example"),
            Client::R(crucible_build::ROptions {
                package_name: "weather.sdk".into(),
                package_version: "1.2.3".into(),
                endpoint: Some("https://sdk.example".into()),
            })
        );
    }
}

#[test]
fn every_enabled_fluent_target_emits_the_native_options_and_exports_a_package() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("model.json"),
        include_str!("../../crucible-smithy/tests/fixtures/weather.json"),
    )
    .unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fluent_probe", "--nocapture"])
        .env("CRUCIBLE_FLUENT_PROBE", "1")
        .env("CARGO_MANIFEST_DIR", root.path())
        .env("OUT_DIR", root.path().join("build-output"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let index: serde_json::Value = serde_json::from_slice(
        &fs::read(root.path().join("build-output/crucible-artifacts.json")).unwrap(),
    )
    .unwrap();
    assert!(!index["artifacts"].as_array().unwrap().is_empty());
    assert!(root.path().join("exports").is_dir());
}
