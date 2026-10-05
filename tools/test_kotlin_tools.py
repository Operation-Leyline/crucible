import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

TOOLS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))
SPEC = importlib.util.spec_from_file_location("kotlin_multiplatform", TOOLS / "test-kotlin-multiplatform.py")
KMP = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(KMP)


class GradleExecutableTests(unittest.TestCase):
    def script(self, directory, name="gradle", text="configured-gradle"):
        executable = directory / (name + ".bat" if os.name == "nt" else name)
        source = f"@echo off\necho {text}\n" if os.name == "nt" else f"#!/bin/sh\nprintf '%s\\n' '{text}'\n"
        executable.write_text(source, encoding="ascii")
        executable.chmod(0o755)
        return executable

    def test_explicit_command_name_and_path_are_used(self):
        with tempfile.TemporaryDirectory(prefix="kotlin gradle ") as temporary:
            root = Path(temporary)
            executable = self.script(root, "custom-gradle")
            for override in ["custom-gradle", str(executable)]:
                with self.subTest(override=override), patch.dict(os.environ, {
                    "GRADLE": override, "PATH": str(root) + os.pathsep + os.environ.get("PATH", ""),
                }):
                    resolved = KMP.gradle_executable()
                    self.assertEqual(resolved, str(executable.resolve()))
                    self.assertEqual(subprocess.check_output([resolved], text=True).strip(), "configured-gradle")

    def test_relative_override_survives_build_directory_change(self):
        with tempfile.TemporaryDirectory(prefix="kotlin gradle ") as temporary:
            root = Path(temporary)
            executable = self.script(root)
            build = root / "consumer"
            build.mkdir()
            original_directory = Path.cwd()
            try:
                os.chdir(root)
                with patch.dict(os.environ, {"GRADLE": os.path.join(".", executable.name)}):
                    resolved = KMP.gradle_executable()
            finally:
                os.chdir(original_directory)
            self.assertTrue(Path(resolved).is_absolute())
            self.assertEqual(subprocess.check_output([resolved], cwd=build, text=True).strip(), "configured-gradle")

    def test_missing_override_fails_before_generation(self):
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ, {
            "GRADLE": str(Path(temporary) / "missing-gradle"),
        }), patch.object(KMP, "run") as run:
            with self.assertRaisesRegex(RuntimeError, "Gradle executable not found"):
                KMP.qualify(Path(temporary))
            run.assert_not_called()

    @unittest.skipUnless(os.name == "nt", "Windows executable-extension precedence")
    def test_pinned_batch_precedes_later_executable_shim(self):
        with tempfile.TemporaryDirectory(prefix="kotlin gradle ") as temporary:
            root = Path(temporary)
            pinned, installed = root / "pinned 8.13", root / "installed 9.8"
            pinned.mkdir()
            installed.mkdir()
            executable = self.script(pinned, text="pinned-8.13")
            shutil.copyfile(os.environ["COMSPEC"], installed / "gradle.exe")
            environment = {
                "PATH": os.pathsep.join([str(pinned), str(installed), os.environ.get("PATH", "")]),
                "PATHEXT": ".COM;.EXE;.BAT;.CMD",
            }
            with patch.dict(os.environ, environment):
                os.environ.pop("GRADLE", None)
                arguments = ["/d", "/c", "echo", "unpinned-9.8"]
                # Reproduce the old call choosing .exe despite the earlier pinned .bat.
                self.assertEqual(subprocess.check_output(["gradle", *arguments], cwd=root, text=True).strip(), "unpinned-9.8")
                resolved = KMP.gradle_executable()
                self.assertEqual(resolved, str(executable.resolve()))
                self.assertEqual(subprocess.check_output([resolved, *arguments], cwd=root, text=True).strip(), "pinned-8.13")


if __name__ == "__main__":
    unittest.main()
