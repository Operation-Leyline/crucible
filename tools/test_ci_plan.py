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

CONSUMERS = {"consumers"}
from ci_plan import (ALL_HOSTS, CORE_GROUPS, GROUPS, MODEL_GROUPS, SDK_GROUPS, TOOL_GROUPS, affected_groups,
                     changed_paths, onboarding_hosts, select_groups,
)


ROOT = Path(__file__).resolve().parents[1]
EVENT = {"pull_request": {"base": {"sha": "a" * 40}, "head": {"sha": "b" * 40}}}


class PlanTests(unittest.TestCase):
    def test_server_runtime_rename_keeps_old_and_new_paths_qualified(self):
        self.assertEqual(affected_groups(["crates/crucible/src/lib.rs"]), set(CORE_GROUPS))
        self.assertEqual(affected_groups(["crates/crucible-server-runtime-rust/src/lib.rs"]), CORE_GROUPS | CONSUMERS)

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

    def test_server_changes_keep_core_http_gates(self):
        for path in (
            "crates/crucible-policy/src/lib.rs", "e2e/weather-service/src/main.rs",
        ):
            with self.subTest(path=path):
                self.assertEqual(affected_groups([path]), CORE_GROUPS)
        for path in (
            "crates/crucible-server-runtime-rust/src/lib.rs",
            "crates/crucible-wire/src/lib.rs",
            "crates/crucible-codegen-rust/src/lib.rs",
            "crates/crucible-auth/src/lib.rs",
            ".github/workflows/ci.yml", ".github/workflows/release.yml",
        ):
            with self.subTest(path=path):
                self.assertEqual(affected_groups([path]), CORE_GROUPS | CONSUMERS)
        self.assertEqual(affected_groups(["crates/crucible-cli/src/main.rs"]), CORE_GROUPS | {"service_cli"})

    def test_openapi_export_keeps_its_rust_qualification(self):
        self.assertEqual(affected_groups(["crates/crucible-codegen-openapi/src/lib.rs"]), {"rust"})
        self.assertEqual(affected_groups(["crates/crucible-codegen-docs/src/lib.rs"]), {"rust", "docs_site"})
        self.assertEqual(affected_groups(["crates/crucible-codegen-docs/runtime/explorer.js"]), {"rust", "docs_site", "e2e"})
        self.assertEqual(affected_groups(["tools/test-docs-site.sh"]), {"rust", "docs_site"})
        self.assertEqual(affected_groups(["crates/crucible-codegen-docs/tests/golden/weather-external/describe.json"]), {"rust", "docs_site"})
        self.assertEqual(affected_groups(["crates/crucible-codegen-openapi/tests/golden/weather.openapi.json"]), {"rust"})

    def test_explorer_test_support_selects_http_gate(self):
        self.assertEqual(affected_groups(["crates/crucible-codegen-docs/tests/site/fake-dom.mjs"]), {"rust", "docs_site", "e2e"})

    def test_language_front_end_and_server_have_their_own_gates(self):
        self.assertEqual(affected_groups(["crates/crucible-lang/src/lower.rs"]), SDK_GROUPS)
        self.assertEqual(affected_groups(["crates/crucible-lsp/src/server.rs"]), {"rust"})
        self.assertEqual(affected_groups(["tools/package_lsp.py"]), {"rust", "e2e"})

    def test_onboarding_and_editor_integration_paths_keep_their_gates(self):
        self.assertEqual(affected_groups(["smithy/errors.smithy"]), CORE_GROUPS | CONSUMERS)
        self.assertEqual(affected_groups(["vendor/smithy-tooling/src/runtime.rs"]), CORE_GROUPS)
        self.assertEqual(affected_groups(["ide/smithy-cargo/src/main/resources/META-INF/plugin.xml"]), {"rust", "editor"})
        self.assertEqual(affected_groups(["crates/crucible-build/Cargo.toml"]), SDK_GROUPS | CONSUMERS | {"editor"})
        self.assertEqual(affected_groups(["crates/crucible-cli/src/starter/hello.smithy"]), CORE_GROUPS | {"editor", "service_cli"})

    def test_shared_model_changes_keep_representative_native_consumers(self):
        for path in (
            "crates/crucible-smithy/src/ir.rs", "crates/crucible-codegen-common/src/lib.rs",
            "tests/conformance/http.json", "Cargo.toml", "Cargo.lock",
            "tools/sdk_test_support.py",
        ):
            with self.subTest(path=path):
                self.assertEqual(affected_groups([path]), MODEL_GROUPS | ({"editor"} if path == "Cargo.toml" else set()))

    def test_shared_output_engine_keeps_native_output_consumers(self):
        for path in (
            "crates/crucible-codegen/src/client.rs",
            "crates/crucible-codegen/src/output.rs",
            "crates/crucible-codegen/src/generator.rs",
            "crates/crucible-codegen/Cargo.toml",
            "crates/crucible-codegen/tests/adapters.rs",
            "crates/crucible-codegen/tests/weather.json",
        ):
            with self.subTest(path=path):
                self.assertEqual(affected_groups([path]), SDK_GROUPS)
        self.assertEqual(affected_groups([
            "crates/crucible-codegen/src/client.rs",
            "crates/crucible-codegen-kotlin/src/lib.rs",
        ]), SDK_GROUPS | {"kotlin"})

    def test_shared_build_helpers_keep_sdk_infrastructure_and_cli_consumers(self):
        for path in (
            "crates/crucible-build/src/lib.rs",
            "crates/crucible-build/src/sdk_builder.rs",
            "crates/crucible-build/src/cdk_sdk.rs",
            "crates/crucible-build/src/terraform_sdk.rs",
            "crates/crucible-build/src/cli_sdk.rs",
            "crates/crucible-build/src/openapi_sdk.rs",
            "crates/crucible-build/src/rust_sdk.rs",
            "crates/crucible-build/tests/outputs.rs",
            "crates/crucible-build/tests/bindings.json",
        ):
            with self.subTest(path=path):
                # A test model is not part of the surface consumers build against.
                self.assertEqual(affected_groups([path]), SDK_GROUPS if path.endswith(".json") else SDK_GROUPS | CONSUMERS)
                self.assertTrue({"cdk", "terraform", "service_cli"}.issubset(affected_groups([path])))

    def test_rust_gate_exercises_sdk_features_and_minimal_build_helpers(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        jobs = dict(re.findall(r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)",
                              workflow.split("jobs:\n", 1)[1], re.MULTILINE | re.DOTALL))
        # all-features includes both Java build facade variants and all-clients
        # emitter parity. Minimal builds catch accidental compiler/native deps.
        self.assertIn("cargo test --all-features --locked", jobs["rust"])
        self.assertIn("cargo clippy --all-targets --all-features --locked", jobs["rust"])
        for package in ("crucible-build", "crucible-codegen"):
            self.assertIn(f"cargo check -p {package} --no-default-features --locked", jobs["rust"])

    def test_consumers_follow_the_stable_surface_and_always_run_at_release(self):
        for path in (
            "crates/crucible-build/src/lib.rs", "crates/crucible-server-runtime-rust/src/host.rs",
            "crates/crucible-wire/src/error.rs", "crates/crucible-codegen-rust/src/lib.rs",
            "crates/crucible-auth/src/principal.rs", "smithy/crucible.smithy",
            # The Leyline SDK consumer pins this runtime through `runtime_path`.
            "crates/crucible-client-runtime-rust/src/lib.rs",
            "tests/consumers/leyline/leyline-service/src/lib.rs", "tools/test-consumers.py",
        ):
            with self.subTest(path=path):
                self.assertIn("consumers", affected_groups([path]))
        # Unrelated generators, docs and the TypeScript runtime stay out.
        for path in (
            "crates/crucible-codegen-python/src/lib.rs", "crates/crucible-codegen-ts/src/lib.rs",
            "crates/crucible-smithy/src/ir.rs",
            "docs/releases.md", "CHANGELOG.md", "e2e/weather-service/src/main.rs",
        ):
            with self.subTest(path=path):
                self.assertNotIn("consumers", affected_groups([path]))
        for event in ("workflow_call", "workflow_dispatch", "schedule"):
            self.assertIn("consumers", select_groups(event, {}, Mock()))
        self.assertNotIn("consumers", select_groups("push", {}, Mock()))
        # A fixture-only edit needs the fixture, not every HTTP gate.
        self.assertEqual(affected_groups(["tests/consumers/leyline/Cargo.toml"]), {"rust", "consumers"})

    def test_release_qualification_runs_and_requires_the_consumers_job(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        jobs = dict(re.findall(r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)",
                              workflow.split("jobs:\n", 1)[1], re.MULTILINE | re.DOTALL))
        self.assertIn("python3 tools/test-consumers.py", jobs["consumers"])
        self.assertIn("runs-on: ${{ vars.LINUX_RUNNER || 'ubuntu-latest' }}", jobs["consumers"])
        self.assertNotIn("matrix", jobs["consumers"])
        # The aggregate cannot pass a release (full) run when consumers was skipped or failed.
        self.assertIn('results["plan"]["outputs"]["full"] == "true" and results["consumers"]["result"] != "success"', jobs["ci"])
        release = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn("needs: [preflight, qualify]", release)
        self.assertIn("uses: ./.github/workflows/ci.yml", release)

    def test_unknown_paths_keep_all_linux_consumers(self):
        for path in ("new-generator/runtime.js", "crates/crucible-new/src/lib.rs"):
            with self.subTest(path=path):
                self.assertEqual(affected_groups([path]), GROUPS)

    def test_cross_language_models_keep_representative_consumers(self):
        for path in (
            "crates/crucible-codegen-python/tests/auth.json",
            "crates/crucible-codegen-java/tests/waiters.json",
            "crates/crucible-codegen-java/tests/streaming.json",
            "crates/crucible-codegen-kotlin/tests/fixtures/streaming.json",
            "crates/crucible-codegen-python/tests/new-shared-model.json",
        ):
            with self.subTest(path=path):
                owner = path.split("/")[1].removeprefix("crucible-codegen-")
                self.assertEqual(affected_groups([path]), MODEL_GROUPS | {owner})

    def test_shared_changes_do_not_hide_explicit_language_changes(self):
        paths = ["Cargo.lock", "crates/crucible-codegen-kotlin/src/lib.rs"]
        for ordered in (paths, paths[::-1]):
            self.assertEqual(affected_groups(ordered), MODEL_GROUPS | {"kotlin"})

    def test_planner_only_edits_keep_contract_and_http_gates(self):
        self.assertEqual(affected_groups(["tools/ci_plan.py", "tools/test_ci_plan.py"]),
                         {"rust", "e2e"})

    def test_rust_client_still_qualifies_generated_cli(self):
        self.assertEqual(affected_groups(["crates/crucible-client-runtime-rust/src/transport.rs"]),
                         {"rust", "e2e", "service_cli", "consumers"})
        self.assertEqual(affected_groups([
            "crates/crucible-client/src/transport.rs",
            "crates/crucible-client-runtime-rust/src/transport.rs",
        ]), {"rust", "e2e", "service_cli", "consumers"})
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
        for name, expected in (("push", {"rust", "e2e"}), ("workflow_call", GROUPS),
                               ("workflow_dispatch", GROUPS), ("unknown", GROUPS)):
            self.assertEqual(select_groups(name, {}, diff), expected)
        for event in ({}, None, {"pull_request": {"base": {"sha": "--bad"}, "head": {"sha": "b" * 40}}}):
            self.assertEqual(select_groups("pull_request", event, diff), GROUPS)
        for error in (OSError("missing git"), subprocess.CalledProcessError(1, "git")):
            self.assertEqual(select_groups("pull_request", EVENT, Mock(side_effect=error)), GROUPS)

    def test_onboarding_qualifies_every_host_only_when_provisioning_changes(self):
        def hosts(*paths):
            return onboarding_hosts("pull_request", EVENT, Mock(return_value=list(paths)))
        self.assertEqual(hosts("crates/crucible-server-runtime-rust/src/lib.rs"), ["ubuntu-latest"])
        self.assertEqual(hosts("e2e/weather-service/src/main.rs", "smithy/errors.smithy"), ["ubuntu-latest"])
        for path in ("tools/test-onboarding.py", "tools/package_starter.py",
                     "crates/crucible-cli/src/starter/hello.smithy", "vendor/smithy-tooling/src/runtime.rs"):
            with self.subTest(path=path):
                self.assertEqual(hosts("README.md", path), ALL_HOSTS)
        self.assertEqual(onboarding_hosts("pull_request", {}, Mock()), ALL_HOSTS)
        self.assertEqual(onboarding_hosts("pull_request", EVENT, Mock(side_effect=OSError())), ALL_HOSTS)
        self.assertEqual(onboarding_hosts("push", {}, Mock()), ["ubuntu-latest"])
        for name in ("workflow_dispatch", "workflow_call", "schedule"):
            self.assertEqual(onboarding_hosts(name, {}, Mock()), ALL_HOSTS)

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
            env = dict(os.environ, GITHUB_EVENT_NAME="workflow_dispatch", GITHUB_EVENT_PATH=str(event),
                       GITHUB_OUTPUT=str(output), GITHUB_STEP_SUMMARY=str(Path(directory) / "summary"))
            subprocess.run([sys.executable, str(ROOT / "tools/ci_plan.py")],
                           env=env, check=True, capture_output=True)
            self.assertEqual(dict(line.split("=") for line in output.read_text().splitlines()),
                             {**{group: "true" for group in GROUPS}, "full": "true", "tools": "true",
                              "onboarding_hosts": json.dumps(ALL_HOSTS, separators=(",", ":"))})
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
            if job in {"rust", "qualification-tools"}:
                self.assertIn("needs: plan", body)
            else:
                self.assertRegex(body, r"needs: \[plan, rust(?:, qualification-tools)?\]")
            self.assertIn(job, gate)
            seen.add(group)
        self.assertEqual(seen, GROUPS)

    def test_pr_platform_matrices_are_bounded_and_release_retains_every_platform(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        jobs = dict(re.findall(r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)",
                              workflow.split("jobs:\n", 1)[1], re.MULTILINE | re.DOTALL))

        def selected_jobs(groups, full):
            selected = ["plan", "ci"]
            for name, body in jobs.items():
                if name in {"plan", "ci"}:
                    continue
                condition = re.search(r"^    if: (.+)$", body, re.MULTILINE).group(1)
                group = re.search(r"outputs\.(\w+) == 'true'", condition).group(1)
                if group not in groups or ("outputs.full == 'true'" in condition and not full):
                    continue
                # The shared tools build runs when a job that reuses it does.
                if "outputs.tools == 'true'" in condition and not (full or groups & TOOL_GROUPS):
                    continue
                matrix = re.search(r"full == 'true' && '([^']+)' \|\| '([^']+)'", body)
                if "outputs.onboarding_hosts" in body:
                    # Unrelated PRs; provisioning PRs select every host.
                    variants = ALL_HOSTS if full else ALL_HOSTS[:1]
                else:
                    variants = json.loads(matrix.group(1 if full else 2)) if matrix else [None]
                selected.extend((name, variant) for variant in variants)
            return selected

        self.assertEqual(len(selected_jobs(CORE_GROUPS, False)), 6)
        # Python and Go reuse one tools build instead of compiling twice.
        self.assertEqual(len(selected_jobs(MODEL_GROUPS, False)), 9)
        self.assertEqual(len(selected_jobs({"rust", "kotlin"}, False)), 6)
        # Preserve the 37 test variants (including consumers) plus planner/aggregate, one tools build
        # and the release-only language server build.
        self.assertEqual(len(selected_jobs(GROUPS, True)), 41)
        for name, variant in selected_jobs(GROUPS, False)[2:]:
            self.assertNotIn(name, {"c-windows", "cpp-windows", "swift-apple", "kotlin-android"})
            if isinstance(variant, dict):
                self.assertEqual(variant, {"os": "ubuntu-latest", "flags": ""})
        for name, variant in selected_jobs(GROUPS, False)[2:]:
            if name == "onboarding":
                self.assertEqual(variant, "ubuntu-latest")
        release = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertNotIn("schedule:", workflow)
        self.assertIn("workflow_call:", workflow)
        self.assertIn("workflow_dispatch:", workflow)
        self.assertIn("workflow_dispatch:", release)
        self.assertIn("uses: ./.github/workflows/ci.yml", release)
        self.assertIn("needs: [preflight, qualify]", release)
        self.assertIn('-f "sha=$GITHUB_SHA"', release)
        self.assertIn("--verify-tag", release)

    def test_qualification_artifacts_are_same_run_and_built_when_reused(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        jobs = dict(re.findall(r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)",
                              workflow.split("jobs:\n", 1)[1], re.MULTILINE | re.DOTALL))
        self.assertIn("needs.plan.outputs.tools == 'true'", jobs["qualification-tools"])
        self.assertIn("runs-on: ${{ vars.LINUX_RUNNER_2204 || 'ubuntu-22.04' }}", jobs["qualification-tools"])
        self.assertIn("qualification-tools-linux-${{ github.sha }}", jobs["qualification-tools"])
        # Re-running all jobs keeps the run and SHA, so replace its old artifact.
        upload = jobs["qualification-tools"].split("uses: actions/upload-artifact@", 1)[1]
        self.assertRegex(upload, r"(?m)^          overwrite: true$")
        action = (ROOT / ".github/actions/qualification-tools/action.yml").read_text()
        self.assertIn("qualification-tools-linux-${{ github.sha }}", action)
        self.assertNotIn("run-id:", action)
        self.assertNotIn("github-token:", action)
        self.assertIn("chmod +x", action)
        for name, body in jobs.items():
            if "uses: ./.github/actions/qualification-tools" in body:
                with self.subTest(job=name):
                    self.assertIn("needs: [plan, rust, qualification-tools]", body)
                    self.assertIn("always() && !cancelled() && needs.rust.result == 'success'", body)
                    self.assertIn("needs.qualification-tools.result == 'success' || needs.plan.outputs.tools != 'true'", body)
                    self.assertIn("if: runner.os == 'Linux' && needs.plan.outputs.tools == 'true'", body)
                    # Every job that reuses the tools makes the plan build them.
                    groups = set(re.findall(r"needs\.plan\.outputs\.(\w+) == 'true'", body)) - {"full", "tools"}
                    self.assertTrue(groups & (TOOL_GROUPS | {"typescript", "cdk"}), groups)
        self.assertIn('results["plan"]["outputs"]["tools"] == "true"', jobs["ci"])
        release = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn("uses: ./.github/actions/qualification-tools", release)
        self.assertNotIn("cargo build", release)

    def test_runs_are_bounded_and_prs_do_not_evict_main_caches(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        jobs = dict(re.findall(r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)",
                              workflow.split("jobs:\n", 1)[1], re.MULTILINE | re.DOTALL))
        self.assertIn("types: [opened, synchronize, reopened, ready_for_review]", workflow)
        for name in ("plan", "ci"):
            self.assertIn("!github.event.pull_request.draft", jobs[name])
        # Drafts skip plan, so every matrix built from its outputs needs a
        # literal fallback; fromJSON('') fails the whole run.
        for name, body in jobs.items():
            for expression in re.findall(r"fromJSON\((.*)\) \}\}", body):
                with self.subTest(job=name):
                    self.assertRegex(expression, r"\|\| '\[")
        # PR-ref caches are invisible to other PRs; saving them only evicts main's.
        save_on_main = "save-if: \"${{ github.ref == 'refs/heads/main' }}\""
        for name, body in jobs.items():
            with self.subTest(job=name):
                self.assertRegex(body, r"(?m)^    timeout-minutes: \d+$")
                self.assertEqual(len(re.findall(r"uses: Swatinem/rust-cache@", body)), body.count(save_on_main))
                # Every other cache restores on PRs and saves from main only.
                self.assertNotRegex(body, r"uses: actions/cache@")
                for save in re.findall(r"uses: actions/cache/save@.*\n\s+if: (.*)", body):
                    self.assertIn("github.ref == 'refs/heads/main'", save)
        self.assertIn("cache-read-only: ${{ github.ref != 'refs/heads/main' }}", jobs["kotlin-multiplatform"])


class Prose(unittest.TestCase):
    def test_prose_workflows_and_python_tests_do_not_select_every_job(self):
        # The changelog accompanies most changes; it must not select everything.
        self.assertEqual(affected_groups(["CHANGELOG.md"]), {"rust"})
        self.assertEqual(affected_groups(["docs/idl/examples/weather/model/weather.cru"]), {"rust"})
        self.assertEqual(affected_groups([".github/workflows/sdk-release.yml"]), {"rust"})
        self.assertEqual(affected_groups(["tools/test_crucible_release_notes.py"]), {"rust"})
        # Workflows the PR runs still select their own gates.
        self.assertIn("consumers", affected_groups([".github/workflows/ci.yml"]))
        # Unknown paths, and the composite actions every job uses, still
        # select everything.
        self.assertEqual(affected_groups(["somewhere/new.txt"]), set(GROUPS))
        self.assertEqual(
            affected_groups([".github/actions/qualification-tools/action.yml"]), set(GROUPS)
        )

    def test_the_shared_tools_build_runs_only_when_a_job_reuses_it(self):
        def tools(*paths):
            with tempfile.TemporaryDirectory() as directory:
                event = Path(directory) / "event.json"
                event.write_text(json.dumps(EVENT))
                output = Path(directory) / "output"
                env = dict(os.environ, GITHUB_EVENT_NAME="pull_request", GITHUB_EVENT_PATH=str(event),
                           GITHUB_OUTPUT=str(output), GITHUB_STEP_SUMMARY=str(Path(directory) / "summary"))
                script = (
                    "import sys, ci_plan\n"
                    f"ci_plan.changed_paths = lambda base, head: {list(paths)!r}\n"
                    "ci_plan.main()\n"
                )
                subprocess.run([sys.executable, "-c", script], env=env, check=True,
                               capture_output=True, cwd=ROOT / "tools")
                return dict(line.split("=", 1) for line in output.read_text().splitlines())["tools"]
        self.assertEqual(tools("crates/crucible-codegen-python/src/lib.rs"), "true")
        self.assertEqual(tools("crates/crucible-codegen-ts/src/lib.rs"), "false")
        self.assertEqual(tools("crates/crucible-codegen-cdk/src/lib.rs"), "false")
        self.assertEqual(tools("CHANGELOG.md"), "false")


if __name__ == "__main__":
    unittest.main()
