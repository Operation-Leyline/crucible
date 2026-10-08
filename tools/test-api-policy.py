"""Prove Cargo enforces policy edits/removal and preserves cached model exports."""

import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def run(arguments, environment, success=True):
    result = subprocess.run(arguments, env=environment, text=True, capture_output=True, timeout=600)
    if (result.returncode == 0) != success:
        raise AssertionError(f"Unexpected status: {arguments}\n{result.stdout}\n{result.stderr}")
    return result.stdout + result.stderr


def main():
    with tempfile.TemporaryDirectory(prefix="crucible-cargo-policy-") as directory:
        root = Path(directory)
        consumer = root / "consumer"
        consumer.mkdir()
        (consumer / "src").mkdir()
        dependency = json.dumps(str(ROOT / "crates/crucible-build"))
        (consumer / "Cargo.toml").write_text(
            '[package]\nname = "policy-consumer"\nversion = "0.0.0"\nedition = "2024"\n'
            '[workspace]\n[build-dependencies]\n'
            f'crucible-build = {{ path = {dependency}, default-features = false, features = ["rust"] }}\n', encoding="utf-8")
        (consumer / "src/main.rs").write_text("fn main() {}\n", encoding="utf-8")
        (consumer / "build.rs").write_text('''fn main() {
    let path = std::env::var("POLICY_TEST_COUNT").unwrap();
    let count: usize = std::fs::read_to_string(&path).unwrap_or_else(|_| "0".into()).parse().unwrap();
    std::fs::write(path, (count + 1).to_string()).unwrap();
    crucible_build::model("model.json").server()
        .custom_rejections().policy("crucible.toml").generate();
}
''', encoding="utf-8")
        fixture = ROOT / "crates/crucible-policy/tests/fixtures/contract.json"
        (consumer / "model.json").write_bytes(fixture.read_bytes())
        config = consumer / "crucible.toml"
        config.write_text('schema_version = 1\n[rules.paths]\nprefix = "/v1"\n', encoding="utf-8")
        count = root / "count.txt"
        exported = root / "exported.json"
        environment = {**os.environ, "POLICY_TEST_COUNT": str(count), "CRUCIBLE_MODEL_OUT": str(exported), "CARGO_BUILD_JOBS": "2"}
        command = ["cargo", "check", "--offline", "--manifest-path", str(consumer / "Cargo.toml")]
        run(command, environment)
        assert exported.read_bytes() == fixture.read_bytes()
        assert exported.stat().st_mtime_ns == (consumer / "model.json").stat().st_mtime_ns
        assert count.read_text() == "1"
        run(command, environment)
        assert count.read_text() == "1", "Unchanged policy/model should reuse Cargo's cached build"
        config.write_text('schema_version = 1\n[rules.paths]\nprefix = "/v2"\n', encoding="utf-8")
        assert "paths.prefix" in run(command, environment, success=False)
        config.write_text('schema_version = 1\n[rules.paths]\nprefix = "/v2"\nseverity = "warning"\n', encoding="utf-8")
        assert "paths.prefix" in run(command, environment)
        config.write_text('schema_version = 1\n[rules.paths]\nprefix = "/v2"\n[[exceptions]]\nrule = "paths.prefix"\nshape = "example.api#GetItem"\nreason = "Existing API prefix"\n', encoding="utf-8")
        assert "Existing API prefix" in run(command, environment)
        exported.unlink()
        run(command, environment)
        assert exported.exists(), "Deleting an export must regenerate it on the next build"
        config.unlink()
        assert "cannot read" in run(command, environment, success=False)
        print("Cargo policy checks passed: caching, edited rules, warnings, exceptions, export regeneration, and missing config")


if __name__ == "__main__":
    main()
