#![cfg(feature = "all-clients")]
#[path = "common/clients.rs"]
mod clients;
mod common;

fn client(name: &str) -> crucible_codegen::Client {
    clients::clients()
        .into_iter()
        .find(|(n, _)| *n == name)
        .unwrap()
        .1
}
// Call the public emitters directly: a check in Client must not mask a missing target check.
fn emit(name: &str, service: &crucible_ir::ir::Service) -> Result<usize, crucible_ir::Diagnostics> {
    use crucible_codegen::Client;
    match client(name) {
        Client::Rust(options) => {
            crucible_codegen_rust::emit_client_package(service, &options).map(|files| files.len())
        }
        Client::Ts(options) => {
            crucible_codegen_ts::emit(service, &options).map(|files| files.len())
        }
        Client::Python(options) => {
            crucible_codegen_python::emit(service, &options).map(|files| files.len())
        }
        Client::Go(options) => {
            crucible_codegen_go::emit(service, &options).map(|files| files.len())
        }
        Client::Java(options) => {
            crucible_codegen_java::emit(service, &options).map(|files| files.len())
        }
        Client::Csharp(options) => {
            crucible_codegen_csharp::emit(service, &options).map(|files| files.len())
        }
        Client::Kotlin(options) => {
            crucible_codegen_kotlin::emit(service, &options).map(|files| files.len())
        }
        Client::Swift(options) => {
            crucible_codegen_swift::emit(service, &options).map(|files| files.len())
        }
        Client::Dart(options) => {
            crucible_codegen_dart::emit(service, &options).map(|files| files.len())
        }
        Client::Php(options) => {
            crucible_codegen_php::emit(service, &options).map(|files| files.len())
        }
        Client::Ruby(options) => {
            crucible_codegen_ruby::emit(service, &options).map(|files| files.len())
        }
        Client::Cpp(options) => {
            crucible_codegen_cpp::emit(service, &options).map(|files| files.len())
        }
        Client::C(options) => crucible_codegen_c::emit(service, &options).map(|files| files.len()),
        Client::R(options) => crucible_codegen_r::emit(service, &options).map(|files| files.len()),
    }
}
fn refuse(name: &str, policy: bool) {
    let mut service = common::load("version-header-default");
    let feature = if policy {
        service.unversioned = crucible_ir::ir::UnversionedPolicy::Required;
        "unversioned-required"
    } else {
        service.version_header = Some("Example-Version".into());
        "version-header"
    };
    let errors = emit(name, &service).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|d| d.shape == service.id && d.message.contains(feature))
    );
}
fn accept(name: &str) {
    let service = common::load("version-header-default");
    assert!(emit(name, &service).unwrap() > 0);
}

#[test]
fn rust_accepts_header() {
    let mut service = common::load("version-header-default");
    service.version_header = Some("Example-Version".into());
    assert!(emit("rust", &service).unwrap() > 0);
}

#[test]
fn rust_accepts_required() {
    let mut service = common::load("version-header-default");
    service.unversioned = crucible_ir::ir::UnversionedPolicy::Required;
    assert!(emit("rust", &service).unwrap() > 0);
}

#[test]
fn rust_accepts_default() {
    accept("rust");
}

#[test]
fn ts_refuses_header() {
    refuse("ts", false);
}

#[test]
fn ts_refuses_required() {
    refuse("ts", true);
}

#[test]
fn ts_accepts_default() {
    accept("ts");
}

#[test]
fn python_refuses_header() {
    refuse("python", false);
}

#[test]
fn python_refuses_required() {
    refuse("python", true);
}

#[test]
fn python_accepts_default() {
    accept("python");
}

#[test]
fn go_refuses_header() {
    refuse("go", false);
}

#[test]
fn go_refuses_required() {
    refuse("go", true);
}

#[test]
fn go_accepts_default() {
    accept("go");
}

#[test]
fn java_refuses_header() {
    refuse("java", false);
}

#[test]
fn java_refuses_required() {
    refuse("java", true);
}

#[test]
fn java_accepts_default() {
    accept("java");
}

#[test]
fn csharp_refuses_header() {
    refuse("csharp", false);
}

#[test]
fn csharp_refuses_required() {
    refuse("csharp", true);
}

#[test]
fn csharp_accepts_default() {
    accept("csharp");
}

#[test]
fn kotlin_refuses_header() {
    refuse("kotlin", false);
}

#[test]
fn kotlin_refuses_required() {
    refuse("kotlin", true);
}

#[test]
fn kotlin_accepts_default() {
    accept("kotlin");
}

#[test]
fn swift_refuses_header() {
    refuse("swift", false);
}

#[test]
fn swift_refuses_required() {
    refuse("swift", true);
}

#[test]
fn swift_accepts_default() {
    accept("swift");
}

#[test]
fn dart_refuses_header() {
    refuse("dart", false);
}

#[test]
fn dart_refuses_required() {
    refuse("dart", true);
}

#[test]
fn dart_accepts_default() {
    accept("dart");
}

#[test]
fn php_refuses_header() {
    refuse("php", false);
}

#[test]
fn php_refuses_required() {
    refuse("php", true);
}

#[test]
fn php_accepts_default() {
    accept("php");
}

#[test]
fn ruby_refuses_header() {
    refuse("ruby", false);
}

#[test]
fn ruby_refuses_required() {
    refuse("ruby", true);
}

#[test]
fn ruby_accepts_default() {
    accept("ruby");
}

#[test]
fn cpp_refuses_header() {
    refuse("cpp", false);
}

#[test]
fn cpp_refuses_required() {
    refuse("cpp", true);
}

#[test]
fn cpp_accepts_default() {
    accept("cpp");
}

#[test]
fn c_refuses_header() {
    refuse("c", false);
}

#[test]
fn c_refuses_required() {
    refuse("c", true);
}

#[test]
fn c_accepts_default() {
    accept("c");
}

#[test]
fn r_refuses_header() {
    refuse("r", false);
}

#[test]
fn r_refuses_required() {
    refuse("r", true);
}

#[test]
fn r_accepts_default() {
    accept("r");
}

#[cfg(feature = "rust")]
#[test]
fn server_accepts_named_header() {
    let mut service = common::load("version-header-default");
    service.version_header = Some("Example-Version".into());
    assert!(
        !crucible_codegen_rust::generate(&service, &Default::default())
            .unwrap()
            .is_empty()
    );
}
#[cfg(feature = "rust")]
#[test]
fn server_accepts_required_policy() {
    let mut service = common::load("version-header-default");
    service.unversioned = crucible_ir::ir::UnversionedPolicy::Required;
    assert!(
        !crucible_codegen_rust::generate(&service, &Default::default())
            .unwrap()
            .is_empty()
    );
}
#[cfg(feature = "rust")]
#[test]
fn server_accepts_default() {
    assert!(
        !crucible_codegen_rust::generate(
            &common::load("version-header-default"),
            &Default::default()
        )
        .unwrap()
        .is_empty()
    );
}
#[cfg(feature = "cli")]
fn cli_options() -> crucible_codegen_cli::Options {
    crucible_codegen_cli::Options {
        bin_name: "example".into(),
        package_version: "1.0.0".into(),
        endpoint: None,
        runtime_path: None,
    }
}
#[cfg(feature = "cli")]
#[test]
fn cli_accepts_named_header() {
    let mut service = common::load("version-header-default");
    service.version_header = Some("Example-Version".into());
    assert!(
        !crucible_codegen_cli::emit(&service, &cli_options())
            .unwrap()
            .is_empty()
    );
}
#[cfg(feature = "cli")]
#[test]
fn cli_accepts_required_policy() {
    let mut service = common::load("version-header-default");
    service.unversioned = crucible_ir::ir::UnversionedPolicy::Required;
    assert!(
        !crucible_codegen_cli::emit(&service, &cli_options())
            .unwrap()
            .is_empty()
    );
}
#[cfg(feature = "cli")]
#[test]
fn cli_accepts_default() {
    assert!(
        !crucible_codegen_cli::emit(&common::load("version-header-default"), &cli_options())
            .unwrap()
            .is_empty()
    );
}
