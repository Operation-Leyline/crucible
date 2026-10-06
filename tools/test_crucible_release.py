from pathlib import Path
import tempfile
import unittest

from crucible_release import check, prepare, version_tuple


class ReleaseTests(unittest.TestCase):
    def fixture(self, root):
        (root / "crates/crucible").mkdir(parents=True)
        (root / "crates/crucible/Cargo.toml").write_text('[package]\nname = "crucible"\nversion.workspace = true\n')
        (root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.1.0"\n\n[workspace.dependencies]\nserde = "1"\n')
        (root / "e2e").mkdir()
        lock = '''version = 4

[[package]]
name = "crucible"
version = "0.1.0"

[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "keep-this"

[[package]]
name = "weather-service"
version = "0.1.0"
'''
        for path in (root / "Cargo.lock", root / "e2e/Cargo.lock"):
            path.write_text(lock)

    def test_prepare_updates_both_lockfiles_without_upgrading_dependencies_or_examples(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            check(root, "0.1.0")
            prepare(root, "0.2.0")
            check(root, "0.2.0")
            for path in (root / "Cargo.lock", root / "e2e/Cargo.lock"):
                text = path.read_text()
                self.assertIn('name = "crucible"\nversion = "0.2.0"', text)
                self.assertIn('name = "weather-service"\nversion = "0.1.0"', text)
                self.assertIn('name = "serde"\nversion = "1.0.0"', text)
                self.assertIn('checksum = "keep-this"', text)

    def test_stale_lockfile_does_not_partially_bump_version(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            stale = root / "e2e/Cargo.lock"
            stale.write_text(stale.read_text().replace('name = "crucible"\nversion = "0.1.0"', 'name = "crucible"\nversion = "0.0.9"'))
            before = {path: path.read_bytes() for path in (root / "Cargo.toml", root / "Cargo.lock", stale)}
            with self.assertRaisesRegex(ValueError, "expected 0.1.0"):
                prepare(root, "0.2.0")
            self.assertEqual(before, {path: path.read_bytes() for path in before})

    def test_mismatched_release_and_non_increasing_versions_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            with self.assertRaisesRegex(ValueError, "merged workspace version"):
                check(root, "0.2.0")
            for version in ("0.1.0", "0.0.9"):
                with self.assertRaisesRegex(ValueError, "greater than"):
                    prepare(root, version)

    def test_stable_semantic_versions_only(self):
        self.assertEqual(version_tuple("1.12.3"), (1, 12, 3))
        for version in ("v1.2.3", "01.2.3", "1.2", "1.2.3-rc.1", "1.2.3\n", "--bad"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                version_tuple(version)


if __name__ == "__main__":
    unittest.main()
