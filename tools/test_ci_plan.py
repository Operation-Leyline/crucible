"""Coverage selection is a correctness gate, not only a cost optimization."""

import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock

from ci_plan import GROUPS, affected_groups, changed_paths, select_groups


ROOT = Path(__file__).resolve().parents[1]
EVENT = {"pull_request": {"base": {"sha": "a" * 40}, "head": {"sha": "b" * 40}}}


class PlanTests(unittest.TestCase):
    def test_language_changes_keep_native_consumers_and_dependencies(self):
        for language, expected in {
            "python": {"rust", "python"},
            "java": {"rust", "java"},
            "cpp": {"rust", "cpp"},
            "kotlin": {"rust", "kotlin"},
            "go": {"rust", "go", "terraform"},
            "ts": {"rust", "typescript", "e2e"},
            "infra": {"rust", "cdk", "terraform"},
        }.items():
            with self.subTest(language=language):
                self.assertEqual(affected_groups([f"crates/crucible-codegen-{language}/src/lib.rs"]), expected)

    def test_shared_changes_select_every_consumer(self):
        for path in (
            "crates/crucible-smithy/src/ir.rs", "crates/crucible/src/lib.rs",
            "crates/crucible-wire/src/lib.rs", "crates/crucible-build/src/lib.rs",
            "crates/crucible-codegen-common/src/lib.rs", "crates/crucible-codegen-rust/src/lib.rs",
            "crates/crucible-cli/src/main.rs", "e2e/weather-service/src/main.rs",
            "tests/conformance/http.json", "Cargo.toml", "Cargo.lock",
            "tools/sdk_test_support.py", ".github/workflows/ci.yml",
            "tools/ci_plan.py", "new-generator/runtime.js",
        ):
            with self.subTest(path=path):
                self.assertEqual(affected_groups([path]), GROUPS)

    def test_rust_client_still_qualifies_generated_cli(self):
        self.assertEqual(affected_groups(["crates/crucible-client/src/transport.rs"]),
                         {"rust", "e2e", "service_cli"})
        self.assertEqual(affected_groups(["crates/crucible-cli-runtime/src/output.rs"]),
                         {"rust", "service_cli"})

    def test_tools_and_moves_select_all_affected_groups(self):
        self.assertEqual(affected_groups(["tools/test-kotlin-multiplatform.py"]), {"rust", "kotlin"})
        self.assertEqual(affected_groups(["tools/cpp-vcpkg/vcpkg.json"]), {"rust", "c", "cpp"})
        self.assertEqual(affected_groups([
            "crates/crucible-codegen-python/src/old.rs",
            "crates/crucible-codegen-go/src/new.rs",
        ]), {"rust", "python", "go", "terraform"})
        # Golden package READMEs are generated output, not repository docs.
        self.assertEqual(affected_groups(["crates/crucible-codegen-java/tests/golden/README.md"]),
                         {"rust", "java"})

    def test_docs_only_keep_the_rust_gate(self):
        self.assertEqual(affected_groups(["README.md", "docs/framework.md"]), {"rust"})

    def test_event_policies_and_diff_failures(self):
        diff = Mock(return_value=["crates/crucible-codegen-java/src/lib.rs"])
        self.assertEqual(select_groups("pull_request", EVENT, diff), {"rust", "java"})
        diff.assert_called_once_with("a" * 40, "b" * 40)
        for name, expected in (("push", {"rust", "e2e"}), ("schedule", GROUPS),
                               ("workflow_dispatch", GROUPS), ("unknown", GROUPS)):
            self.assertEqual(select_groups(name, {}, diff), expected)
        for event in ({}, None, {"pull_request": {"base": {"sha": "--bad"}, "head": {"sha": "b" * 40}}}):
            self.assertEqual(select_groups("pull_request", event, diff), GROUPS)
        for error in (OSError("missing git"), subprocess.CalledProcessError(1, "git")):
            self.assertEqual(select_groups("pull_request", EVENT, Mock(side_effect=error)), GROUPS)

    def test_three_dot_diff_excludes_new_base_changes_and_keeps_deleted_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=directory).decode().strip()
            git("init", "-q")
            git("config", "user.name", "CI test")
            git("config", "user.email", "ci@example.invalid")
            source = Path(directory) / "old.py"
            source.write_text("base\n")
            git("add", ".")
            git("commit", "-qm", "base")
            fork = git("rev-parse", "HEAD")
            source.rename(Path(directory) / "new.py")
            git("add", "-A")
            git("commit", "-qm", "move")
            head = git("rev-parse", "HEAD")
            git("checkout", "-q", "--detach", fork)
            (Path(directory) / "unrelated.rs").write_text("base branch only\n")
            git("add", ".")
            git("commit", "-qm", "unrelated")
            base = git("rev-parse", "HEAD")
            previous = os.getcwd()
            try:
                os.chdir(directory)
                self.assertEqual(set(changed_paths(base, head)), {"old.py", "new.py"})
            finally:
                os.chdir(previous)

    def test_outputs_have_every_group_and_match_workflow_gates(self):
        with tempfile.TemporaryDirectory() as directory:
            event = Path(directory) / "event.json"
            event.write_text(json.dumps({}))
            output = Path(directory) / "output"
            env = dict(os.environ, GITHUB_EVENT_NAME="schedule", GITHUB_EVENT_PATH=str(event),
                       GITHUB_OUTPUT=str(output), GITHUB_STEP_SUMMARY=str(Path(directory) / "summary"))
            subprocess.run([sys.executable, str(ROOT / "tools/ci_plan.py")],
                           env=env, check=True, capture_output=True)
            self.assertEqual(dict(line.split("=") for line in output.read_text().splitlines()),
                             {group: "true" for group in GROUPS})
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        jobs = re.findall(r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)", workflow.split("jobs:\n", 1)[1],
                          re.MULTILINE | re.DOTALL)
        gate = dict(jobs)["ci"]
        seen = set()
        for job, body in jobs:
            if job in {"plan", "ci"}:
                continue
            group = re.search(r"needs\.plan\.outputs\.(\w+) == 'true'", body).group(1)
            self.assertIn(group, GROUPS)
            self.assertIn("needs: plan" if job == "rust" else "needs: [plan, rust]", body)
            self.assertIn(job, gate)
            seen.add(group)
        self.assertEqual(seen, GROUPS)


if __name__ == "__main__":
    unittest.main()
