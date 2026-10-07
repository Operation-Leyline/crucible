fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    // The complete parser and model/policy visitors exceed the MSVC default
    // main-thread stack in debug builds. Reserve space for the CLI's full path.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo::rustc-link-arg-bin=crucible=/STACK:8388608");
    }
}
