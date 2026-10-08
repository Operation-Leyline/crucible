fn main() {
    let model = crucible_build::model("smithy");
    model.publish();
    // The SDK pins the client runtime to this checkout, the way a released SDK
    // pins it to a Crucible tag.
    model
        .rust_client("sdk")
        .package("leyline-sdk")
        .version("0.1.0")
        .runtime_path("../../../../crates/crucible-client-runtime-rust")
        .generate_into("../sdk");
}
