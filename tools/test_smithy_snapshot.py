import json
from pathlib import Path
import shutil
import tempfile
import unittest

from sync_smithy_tooling import DESTINATION, RECORD, sync, verify


class SnapshotTests(unittest.TestCase):
    def test_shipped_compiler_snapshot_is_complete(self):
        verify()

    def test_changed_source_or_extra_file_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "snapshot"
            shutil.copytree(DESTINATION, root)
            source = root / "src/compiler.rs"
            original = source.read_bytes()
            source.write_bytes(b"unreviewed compiler")
            with self.assertRaisesRegex(ValueError, "differs: src/compiler.rs"):
                verify(root)
            source.write_bytes(original)
            (root / "unexpected").write_text("unexpected")
            with self.assertRaisesRegex(ValueError, "unexpected files"):
                verify(root)

    def test_manifest_and_embedded_compiler_versions_must_match(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "snapshot"
            shutil.copytree(DESTINATION, root)
            record = json.loads((root / RECORD).read_text())
            record["smithy_version"] = "0.0.0"
            (root / RECORD).write_text(json.dumps(record))
            with self.assertRaisesRegex(ValueError, "versions differ"):
                verify(root)

    def test_import_requires_an_immutable_commit_before_running_git(self):
        with self.assertRaisesRegex(ValueError, "immutable commit"):
            sync(Path("does-not-exist"), "main")
