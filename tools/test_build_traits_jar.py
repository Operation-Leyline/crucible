import pathlib
import tempfile
import unittest
import zipfile
import tomllib

import build_traits_jar


class TraitsJar(unittest.TestCase):
    def test_jar_and_cargo_package_use_the_workspace_version(self):
        manifest = tomllib.loads((build_traits_jar.ROOT / "Cargo.toml").read_text())
        package = tomllib.loads((build_traits_jar.ROOT / "smithy/Cargo.toml").read_text())
        self.assertEqual(package["package"]["version"], {"workspace": True})
        self.assertEqual(build_traits_jar.version(), manifest["workspace"]["package"]["version"])
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "Cargo.toml").write_text('[workspace.package]\nversion = "9.8.7"\n')
            self.assertEqual(build_traits_jar.version(root), "9.8.7")

    def test_the_jar_lists_and_carries_every_model_file(self):
        with tempfile.TemporaryDirectory() as out:
            jar = build_traits_jar.build(pathlib.Path(out))
            self.assertEqual(jar.name, f"crucible-traits-{build_traits_jar.version()}.jar")
            with zipfile.ZipFile(jar) as archive:
                listed = archive.read("META-INF/smithy/manifest").decode().split()
                self.assertEqual(listed, build_traits_jar.MODELS)
                model = archive.read("META-INF/smithy/crucible.smithy").decode()
                self.assertIn("namespace crucible.protocols", model)
                self.assertIn("structure json {}", model)
                self.assertIn("structure strict {}", model)
                errors = archive.read("META-INF/smithy/errors.smithy").decode()
                for name in ["ValidationException", "InternalServerException", "UnknownOperationException"]:
                    self.assertIn(f"structure {name}", errors)
                auth = archive.read("META-INF/smithy/auth.smithy").decode()
                self.assertIn("namespace crucible.auth", auth)
                self.assertIn("list scopes", auth)


if __name__ == "__main__":
    unittest.main()
