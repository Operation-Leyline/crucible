"""Qualify an installed Cargo SDK against native Rust HTTP and shared SSE profiles."""
import re
from pathlib import Path
import shutil
import tempfile
from sdk_test_support import run

ROOT = Path(__file__).resolve().parents[1]


def qualify(output):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "rust",
        "tests/consumers/rust-stream-client/model", "--client", "--package-name", "streams-sdk",
        "--runtime-path", str(ROOT / "crates/crucible-client-runtime-rust"), "--out", str(output / "sdk"))
    no_errors_model = output / "no-errors-model"
    shutil.copytree(ROOT / "tests/consumers/rust-stream-client/model", no_errors_model)
    model = no_errors_model / "model.cru"
    original = model.read_text(encoding="utf-8")
    no_errors = re.sub(r"(?m)^    errors \[[^\n]*\]\n", "", original)
    assert no_errors != original, "the empty-error fixture must remove its service errors"
    model.write_text(no_errors, encoding="utf-8")
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "rust",
        str(no_errors_model), "--client", "--package-name", "empty-streams-sdk",
        "--runtime-path", str(ROOT / "crates/crucible-client-runtime-rust"), "--out", str(output / "empty-sdk"))
    source = ROOT / "tests/consumers/rust-stream-client"
    consumer = output / "consumer"
    shutil.copytree(source / "src", consumer / "src")
    shutil.copytree(source / "tests", consumer / "tests")
    (consumer / "Cargo.toml").write_text((source / "Cargo.toml.template").read_text().replace("@ROOT@", ROOT.as_posix()), encoding="utf-8")
    shutil.copyfile(ROOT / "tests/conformance/response-stream.json", consumer / "response-stream.json")
    # Compile the generated guide's complete Rust call samples as consumer binaries.
    for index, code in enumerate(re.findall(r"```rust\n(.*?)```", (output / "sdk/README.md").read_text(encoding="utf-8"), re.S)):
        if "#[tokio::main]" in code:
            bins = consumer / "src/bin"
            bins.mkdir(exist_ok=True)
            (bins / f"guide_{index}.rs").write_text(code, encoding="utf-8")
    # Pin the complete installed-consumer graph, including both generated SDKs.
    shutil.copyfile(source / "Cargo.lock", consumer / "Cargo.lock")
    run("cargo", "test", "--locked", "--manifest-path", str(consumer / "Cargo.toml"), "--", "--nocapture")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="crucible-rust-stream-") as directory:
        qualify(Path(directory))
