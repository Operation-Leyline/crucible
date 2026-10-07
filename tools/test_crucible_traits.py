"""Prove the packaged Cargo dependency works without the repository beside it."""

import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import tomllib
import unittest
import zipfile

import build_traits_jar


ROOT = Path(__file__).resolve().parents[1]


class CargoTraits(unittest.TestCase):
    def cargo(self, arguments, cwd, environment):
        result = subprocess.run(
            ["cargo", *arguments], cwd=cwd, env=environment,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_installed_consumer_writes_the_same_model_as_the_jar(self):
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        with tempfile.TemporaryDirectory(prefix="crucible-traits-") as temporary:
            directory = Path(temporary)
            environment = {**os.environ, "CARGO_TARGET_DIR": str(directory / "target"), "CARGO_BUILD_JOBS": "2"}
            self.cargo(
                ["package", "-p", "crucible-traits", "--allow-dirty", "--no-verify", "--offline", "--locked"],
                ROOT, environment,
            )
            package = directory / "target/package" / f"crucible-traits-{version}.crate"
            with tarfile.open(package) as archive:
                archive.extractall(directory / "installed", filter="data")
            installed = directory / "installed" / f"crucible-traits-{version}"
            model = (installed / "crucible.smithy").read_text(encoding="utf-8")
            with zipfile.ZipFile(build_traits_jar.build(directory / "jar")) as archive:
                self.assertEqual(model, archive.read("META-INF/smithy/crucible.smithy").decode())
                self.assertEqual((installed / "errors.smithy").read_text(encoding="utf-8"), archive.read("META-INF/smithy/errors.smithy").decode())

            consumer = directory / "consumer"
            (consumer / "src").mkdir(parents=True)
            (consumer / "Cargo.toml").write_text(
                '[package]\nname = "traits-consumer"\nversion = "0.0.0"\nedition = "2024"\n\n'
                '[build-dependencies]\ncrucible-traits = { path = "' + installed.as_posix() + '" }\n',
                encoding="utf-8",
            )
            (consumer / "build.rs").write_text('''
fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    assert_eq!(crucible_traits::VERSION, "''' + version + '''");
    assert!(crucible_traits::MODEL.contains("namespace crucible.protocols"));
    assert!(crucible_traits::MODEL.contains("structure json {}"));
    assert!(crucible_traits::MODEL.contains("structure strict {}"));
    for name in ["ValidationException", "InternalServerException", "UnknownOperationException"] {
        assert!(crucible_traits::ERRORS.contains(&format!("structure {name}")));
    }
    crucible_traits::write_models("smithy-imports")?;
    assert_eq!(std::fs::read("smithy-imports/errors.smithy")?, crucible_traits::ERRORS.as_bytes());
    let path = std::path::Path::new("src/smithy-ide/crucible.smithy");
    crucible_traits::write_model(path)?;
    assert_eq!(std::fs::read(path)?, crucible_traits::MODEL.as_bytes());
    let modified = std::fs::metadata(path)?.modified()?;
    crucible_traits::write_model(path)?;
    assert_eq!(std::fs::metadata(path)?.modified()?, modified);
    std::fs::write(path, "stale model")?;
    crucible_traits::write_model(path)?;
    assert_eq!(std::fs::read(path)?, crucible_traits::MODEL.as_bytes());
    crucible_traits::write_model("bare.smithy")?;
    assert_eq!(std::fs::read("bare.smithy")?, crucible_traits::MODEL.as_bytes());
    std::fs::write("blocked", "file")?;
    assert!(crucible_traits::write_model("blocked/model.smithy").is_err());
    assert!(crucible_traits::write_model("src/smithy-ide").is_err());
    Ok(())
}
''', encoding="utf-8")
            (consumer / "src/main.rs").write_text('''
fn main() {
    let model = include_str!("smithy-ide/crucible.smithy");
    assert!(model.contains("namespace crucible.protocols"));
    assert!(model.contains("structure json {}"));
    assert!(model.contains("structure strict {}"));
}
''', encoding="utf-8")
            self.cargo(["run", "--offline", "--quiet"], consumer, environment)


if __name__ == "__main__":
    unittest.main()
