fn main() {
    let model = "../../crates/crucible-smithy/tests/fixtures/rpcv2-json.json";
    println!("cargo::rerun-if-changed={model}");
    crucible_build::model(model)
        .server()
        // The only operation with response vectors and neither an input nor a
        // request vector: its request is just the RPC route and headers.
        .seed_request(
            "GreetingWithErrors",
            "POST",
            "/service/RpcV2JsonProtocol/operation/GreetingWithErrors",
            &[
                ("smithy-protocol", "rpc-v2-json"),
                ("accept", "application/json"),
            ],
            None,
        )
        .generate();
}
