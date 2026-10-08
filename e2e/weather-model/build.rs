//! Publishes the weather model: the fixture the other packages share, plus the
//! `smithy.test` vectors in `weather-vectors.json`. The vectors are an overlay
//! of traits by shape id, merged here the way an `apply` statement in a
//! separate Smithy file would add them, so the shared fixture (and every
//! client generated from it) stays free of test data.
use std::path::{Path, PathBuf};

use serde_json::Value;

fn main() {
    let fixtures = Path::new("../../crates/crucible-smithy/tests/fixtures");
    let model = fixtures.join("weather.json");
    let vectors = fixtures.join("weather-vectors.json");
    println!("cargo::rerun-if-changed={}", model.display());
    println!("cargo::rerun-if-changed={}", vectors.display());

    let read = |path: &Path| -> Value {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("{} is not JSON: {error}", path.display()))
    };
    let mut merged = read(&model);
    let overlay = read(&vectors);
    for (shape, traits) in overlay.as_object().expect("an object of shapes") {
        let target = merged["shapes"][shape]["traits"]
            .as_object_mut()
            .unwrap_or_else(|| panic!("{shape} has no traits to add vectors to"));
        for (name, value) in traits.as_object().expect("an object of traits") {
            assert!(
                target.insert(name.clone(), value.clone()).is_none(),
                "{shape} already has {name}"
            );
        }
    }

    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("weather.json");
    std::fs::write(&out, serde_json::to_string_pretty(&merged).unwrap())
        .expect("cannot write the merged model");
    crucible_build::model(out).publish();
}
