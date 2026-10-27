import hashlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import install_cargo_dist as tool


class InstallTests(unittest.TestCase):
    def test_untrusted_bytes_fail_before_writing_an_executable(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "tool"
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                tool.install(destination, "Windows", "AMD64", lambda _: b"not the pinned release")
            self.assertFalse(destination.exists())

    def test_only_named_executable_is_copied_and_paths_cannot_escape(self):
        payload = io.BytesIO()
        with zipfile.ZipFile(payload, "w") as archive:
            archive.writestr("../../dist.exe", b"the tool")
            archive.writestr("../../outside", b"must not be extracted")
        data = payload.getvalue()
        pinned = {("Windows", "x86_64"): ("tool.zip", hashlib.sha256(data).hexdigest())}
        with tempfile.TemporaryDirectory() as directory, patch.object(tool, "ARCHIVES", pinned):
            root = Path(directory)
            # Install two levels down, so `../../outside` resolved from the
            # install directory would still land inside `root`, where rglob sees it.
            installed = tool.install(root / "a/b/tool", "Windows", "AMD64", lambda _: data)
            self.assertEqual(installed, root / "a/b/tool/dist.exe")
            self.assertEqual(installed.read_bytes(), b"the tool")
            self.assertFalse((root / "a/outside").exists())
            self.assertFalse((root / "a/dist.exe").exists())
            self.assertEqual(sorted(p.relative_to(root).as_posix() for p in root.rglob("*")),
                             ["a", "a/b", "a/b/tool", "a/b/tool/dist.exe"])

    def test_unsupported_host_does_not_download(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "not pinned"):
                tool.install(Path(directory), "Plan9", "mips", lambda _: self.fail("downloaded"))


if __name__ == "__main__":
    unittest.main()
