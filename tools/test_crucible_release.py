from pathlib import Path
import tempfile
import unittest

from crucible_release import LOCKFILES, changelog_entry, check, prepare, version_tuple


CHANGELOG = """# Changelog

## [Unreleased]

## [0.2.1] - 2026-02-01

### Fixed

- A bug.

## [0.2.0] - 2026-01-02

### Added

- A feature.

### Migration

- Rename the thing.

## [0.1.0] - 2026-01-01

### Added

- The start.
"""


class ReleaseTests(unittest.TestCase):
    def fixture(self, root):
        (root / "CHANGELOG.md").write_text(CHANGELOG)
        (root / "crates/crucible-server-runtime-rust").mkdir(parents=True)
        (root / "crates/crucible-server-runtime-rust/Cargo.toml").write_text('[package]\nname = "crucible-server-runtime-rust"\nversion.workspace = true\n')
        (root / "Cargo.toml").write_text('[workspace]\nmembers = ["crates/*", "tools/extra"]\n\n[workspace.package]\nversion = "0.1.0"\n\n[workspace.dependencies]\nserde = "1"\n')
        (root / "e2e").mkdir()
        (root / "tests/consumers/leyline/leyline-sdk-consumer").mkdir(parents=True)
        lock = '''version = 4

[[package]]
name = "crucible-server-runtime-rust"
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
        for path in (root / lock for lock in LOCKFILES):
            path.write_text(lock)

    def test_prepare_updates_both_lockfiles_without_upgrading_dependencies_or_examples(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            check(root, "0.1.0")
            prepare(root, "0.2.0")
            check(root, "0.2.0")
            for path in (root / lock for lock in LOCKFILES):
                text = path.read_text()
                self.assertIn('name = "crucible-server-runtime-rust"\nversion = "0.2.0"', text)
                self.assertIn('name = "weather-service"\nversion = "0.1.0"', text)
                self.assertIn('name = "serde"\nversion = "1.0.0"', text)
                self.assertIn('checksum = "keep-this"', text)

    def test_stale_lockfile_does_not_partially_bump_version(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            stale = root / "e2e/Cargo.lock"
            stale.write_text(stale.read_text().replace('name = "crucible-server-runtime-rust"\nversion = "0.1.0"', 'name = "crucible-server-runtime-rust"\nversion = "0.0.9"'))
            before = {path: path.read_bytes() for path in (root / "Cargo.toml", root / "Cargo.lock", stale)}
            with self.assertRaisesRegex(ValueError, "expected 0.1.0"):
                prepare(root, "0.2.0")
            self.assertEqual(before, {path: path.read_bytes() for path in before})

    def test_member_outside_crates_is_checked_and_bumped(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            (root / "tools/extra").mkdir(parents=True)
            (root / "tools/extra/Cargo.toml").write_text(
                '[package]\nname = "crucible-extra"\nversion.workspace = true\n'
            )
            with self.assertRaisesRegex(ValueError, "missing workspace packages.*crucible-extra"):
                check(root, "0.1.0")
            for path in (root / lock for lock in LOCKFILES):
                with path.open("a") as lock:
                    lock.write('\n[[package]]\nname = "crucible-extra"\nversion = "0.1.0"\n')
            prepare(root, "0.2.0")
            check(root, "0.2.0")
            for path in (root / lock for lock in LOCKFILES):
                self.assertIn('name = "crucible-extra"\nversion = "0.2.0"', path.read_text())

    def test_mismatched_release_and_non_increasing_versions_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            with self.assertRaisesRegex(ValueError, "merged workspace version"):
                check(root, "0.2.0")
            for version in ("0.1.0", "0.0.9"):
                with self.assertRaisesRegex(ValueError, "greater than"):
                    prepare(root, version)

    def test_release_without_a_changelog_entry_is_refused_and_leaves_files_alone(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            before = {path: path.read_bytes() for path in (root / "Cargo.toml", *(root / lock for lock in LOCKFILES))}
            with self.assertRaisesRegex(ValueError, r"no '## \[0.3.0\]' entry"):
                prepare(root, "0.3.0")
            self.assertEqual(before, {path: path.read_bytes() for path in before})
            # [Unreleased] holds no version, so it can never satisfy the gate.
            with self.assertRaisesRegex(ValueError, r"no '## \[0.1.5\]' entry"):
                changelog_entry(root, "0.1.5")
            (root / "CHANGELOG.md").unlink()
            with self.assertRaisesRegex(ValueError, "does not exist"):
                check(root, "0.1.0")

    def test_check_requires_the_entry_for_the_merged_version(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            check(root, "0.1.0")
            (root / "CHANGELOG.md").write_text(CHANGELOG.replace("## [0.1.0] - 2026-01-01", "## [0.0.9] - 2026-01-01"))
            with self.assertRaisesRegex(ValueError, r"no '## \[0.1.0\]' entry"):
                check(root, "0.1.0")

    def test_empty_entry_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            (root / "CHANGELOG.md").write_text("""## [0.2.0] - 2026-01-02

### Added



## [0.1.0]

- Start
""")
            with self.assertRaisesRegex(ValueError, "entry for 0.2.0 is empty"):
                prepare(root, "0.2.0")
            # The heading may omit its date, and the version must match exactly.
            changelog_entry(root, "0.1.0")
            with self.assertRaises(ValueError):
                changelog_entry(root, "0.1")

    def test_patch_release_may_not_carry_a_migration_entry(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            changelog_entry(root, "0.2.0")  # a minor may migrate
            (root / "CHANGELOG.md").write_text(CHANGELOG.replace("### Fixed", "### Migration"))
            with self.assertRaisesRegex(ValueError, "patch release 0.2.1 has a Migration"):
                changelog_entry(root, "0.2.1")

    def test_stable_semantic_versions_only(self):
        self.assertEqual(version_tuple("1.12.3"), (1, 12, 3))
        for version in ("v1.2.3", "01.2.3", "1.2", "1.2.3-rc.1", "1.2.3\n", "--bad"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                version_tuple(version)


if __name__ == "__main__":
    unittest.main()
