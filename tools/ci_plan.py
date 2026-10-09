"""Select native qualification gates without guessing at shared dependencies."""

import functools
import json
import os
from pathlib import Path
import re
import subprocess


GROUPS = frozenset({
    "rust", "e2e", "service_cli", "cdk", "terraform", "c", "cpp", "r",
    "ruby", "php", "dart", "swift", "kotlin", "csharp", "java", "go",
    "python", "typescript", "onboarding", "editor", "consumers", "docs_site",
})
# Rust checks include every generator's unit/golden tests. Real HTTP and the
# dependency-free TS runtime cover the server path on every shared-code PR.
CORE_GROUPS = frozenset({"rust", "e2e", "typescript", "onboarding"})
# Model/wire-description changes also compile representative native SDKs.
# The complete language/platform matrix runs on the scheduled/manual tier.
MODEL_GROUPS = CORE_GROUPS | {"python", "go"}
# The shared engine and Cargo facade own clients, schemas, provisioning and CLI
# packages. Keep representative SDKs and each additional native output consumer.
SDK_GROUPS = MODEL_GROUPS | {"java", "cdk", "terraform", "service_cli"}
# The stable surface downstream services build against: compile the starter, the
# example service and a Leyline-shaped fixture. Selected by the crates and files
# whose public API or generated shape those consumers use, and always at release.
CONSUMER_CRATES = frozenset({
    "crucible-build", "crucible-server-runtime-rust", "crucible-wire",
    "crucible-codegen-rust", "crucible-auth", "crucible-client-runtime-rust",
})
GENERATORS = {
    # The OpenAPI emitter is qualified by Rust unit/golden and CLI tests.
    "openapi": set(),
    # The static reference renderer is plain JS run by Node; Rust golden tests
    # cover the files the generator copies.
    "docs": {"docs_site"},
    "cli": {"service_cli"},
    "infra": {"cdk", "terraform"},
    "cdk": {"cdk"},
    "terraform": {"terraform"},
    # The provider embeds the generated Go client's wire runtime.
    "go": {"go", "terraform"},
    "ts": {"typescript", "e2e"},
    **{name: {name} for name in (
        "c", "cpp", "r", "ruby", "php", "dart", "swift", "kotlin",
        "csharp", "java", "python",
    )},
}
TOOLS = {
    "tools/test-api-policy.py": CORE_GROUPS,
    "tools/test-onboarding.py": {"onboarding"},
    "tools/package_starter.py": {"onboarding"},
    "tools/sync_smithy_tooling.py": {"onboarding"},
    "tools/test_smithy_snapshot.py": {"onboarding"},
    "tools/build_traits_jar.py": CORE_GROUPS,
    **{f"tools/test-{name}.py": groups for name, groups in GENERATORS.items()},
    "tools/test-ts.sh": {"typescript", "e2e"},
    "tools/test-docs-site.sh": {"docs_site"},
    "tools/test-e2e.sh": {"e2e"},
    "tools/test-kotlin-android.py": {"kotlin"},
    "tools/test-kotlin-multiplatform.py": {"kotlin"},
    "tools/test_kotlin_tools.py": {"kotlin"},
    "tools/install-yyjson.py": {"c"},
    # Planner-only edits exercise selection contracts and the live skip/aggregate
    # path without rebuilding native consumers whose sources did not change.
    "tools/ci_plan.py": {"e2e"},
    "tools/test_ci_plan.py": {"e2e"},
    ".github/workflows/ci.yml": CORE_GROUPS | {"consumers"},
    ".github/workflows/release.yml": CORE_GROUPS | {"consumers"},
    "tools/crucible_release.py": {"e2e"},
    "tools/package_lsp.py": {"e2e"},
    "tools/test_crucible_release.py": {"e2e"},
    "tools/test-consumers.py": {"consumers"},
}


# Starter provisioning differs per host, so changes to it qualify Windows and
# macOS on the PR. Everything else that reaches onboarding runs Linux-only on
# PRs; release (full) runs keep every host.
ONBOARDING_HOST_PATHS = (
    "tools/test-onboarding.py", "tools/package_starter.py",
    "tools/sync_smithy_tooling.py", "tools/test_smithy_snapshot.py",
    "crates/crucible-cli/src/starter", "vendor/smithy-tooling/",
)
ALL_HOSTS = ["ubuntu-latest", "windows-latest", "macos-latest"]


def affected_groups(paths):
    selected = {"rust"}
    for path in paths:
        if path in {"crates/crucible-codegen-docs/runtime/explorer.js", "crates/crucible-codegen-docs/tests/site/fake-dom.mjs"}:
            selected.add("e2e")
        if path.startswith("ide/smithy-cargo/"):
            selected.add("editor")
            continue
        if path in {"Cargo.toml", "crates/crucible-build/Cargo.toml", "smithy/Cargo.toml", "vendor/smithy-tooling/Cargo.toml"} or path.startswith("crates/crucible-cli/src/starter"):
            selected.add("editor")
        parts = Path(path).parts
        if len(parts) >= 2 and parts[0] == "crates":
            # Models live in several generator test directories but are reused
            # by other languages (for example Python auth and Java waiters).
            if "/tests/" in path and path.endswith(".json") and "/tests/golden/" not in path:
                selected.update(SDK_GROUPS if parts[1] in {"crucible-codegen", "crucible-build"} else MODEL_GROUPS)
                # A fixture may be shared, but its owning generator's native
                # harness must also see the changed model before release.
                language = parts[1].removeprefix("crucible-codegen-")
                selected.update(GENERATORS.get(language, set()))
                continue
            crate = parts[1]
            if crate in CONSUMER_CRATES:
                selected.add("consumers")
            # A rename diff includes deleted paths under the former crate name.
            if crate in {"crucible-client", "crucible-client-runtime-rust"}:
                selected.update({"e2e", "service_cli"})
            elif crate == "crucible-cli-runtime":
                selected.add("service_cli")
            elif crate in {"crucible", "crucible-server-runtime-rust", "crucible-wire", "crucible-policy", "crucible-cli", "crucible-codegen-rust", "crucible-auth"}:
                selected.update(CORE_GROUPS)
                if crate == "crucible-cli":
                    selected.add("service_cli")
            elif crate in {"crucible-codegen", "crucible-build"}:
                selected.update(SDK_GROUPS)
            elif crate == "crucible-lang":
                # The .cru front end lowers through crucible-build into every output.
                selected.update(SDK_GROUPS)
            elif crate == "crucible-lsp":
                # Editors only: its unit and protocol tests run in the Rust job.
                selected.add("rust")
            elif crate in {"crucible-smithy", "crucible-codegen-common"}:
                selected.update(MODEL_GROUPS)
            elif crate.startswith("crucible-codegen-"):
                language = crate.removeprefix("crucible-codegen-")
                if language not in GENERATORS:
                    return set(GROUPS)
                selected.update(GENERATORS[language])
            else:
                # New crates must not silently lose their native qualification.
                return set(GROUPS)
        elif path in TOOLS:
            selected.update(TOOLS[path])
        elif path.startswith("tools/cpp-vcpkg/"):
            selected.update({"c", "cpp"})
        elif path.startswith("tests/consumers/"):
            selected.add("consumers")
        elif path.startswith(("smithy/", "vendor/smithy-tooling/")):
            selected.update(CORE_GROUPS)
            if path.startswith("smithy/"):
                selected.add("consumers")
        elif path.startswith("e2e/"):
            selected.update(CORE_GROUPS)
        elif path.startswith("tests/conformance/") or path in {"Cargo.toml", "Cargo.lock", "tools/sdk_test_support.py"}:
            selected.update(MODEL_GROUPS)
        elif path.startswith("docs/") and path.endswith(".md"):
            continue
        elif path in {"README.md", "AGENTS.md", "CLAUDE.md", "LICENSE"}:
            continue
        else:
            # New files and shared fixtures must not silently lose coverage.
            return set(GROUPS)
    return selected


def pull_request_paths(event, diff):
    """The PR's changed paths, or None when they cannot be trusted."""
    try:
        pr = event["pull_request"]
        base, head = pr["base"]["sha"], pr["head"]["sha"]
        if not all(isinstance(sha, str) and re.fullmatch(r"[0-9a-f]{40}", sha)
                   for sha in (base, head)):
            return None
        # Three-dot excludes unrelated changes that landed on the base branch.
        # No rename detection: select both the old and new path of a move.
        return diff(base, head)
    except (KeyError, TypeError, OSError, subprocess.CalledProcessError):
        return None


def select_groups(event_name, event, diff):
    if event_name in {"schedule", "workflow_dispatch"}:
        return set(GROUPS)
    if event_name == "push":
        # PRs already qualified the change; the merged commit re-proves the
        # Rust and HTTP gates and refreshes main's dependency caches.
        return {"rust", "e2e"}
    if event_name != "pull_request":
        return set(GROUPS)
    paths = pull_request_paths(event, diff)
    return set(GROUPS) if paths is None else affected_groups(paths)


def onboarding_hosts(event_name, event, diff):
    if event_name != "pull_request":
        return ALL_HOSTS if event_name != "push" else ALL_HOSTS[:1]
    paths = pull_request_paths(event, diff)
    if paths is None or any(path.startswith(ONBOARDING_HOST_PATHS) for path in paths):
        return ALL_HOSTS
    return ALL_HOSTS[:1]


def changed_paths(base, head):
    result = subprocess.run(
        ["git", "diff", "--name-only", "--no-renames", "-z", f"{base}...{head}", "--"],
        check=True, capture_output=True,
    )
    if not result.stdout:
        return []
    return result.stdout.decode("utf-8", errors="surrogateescape").rstrip("\0").split("\0")


def main():
    try:
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
    except (KeyError, OSError, ValueError):
        event = {}
    event_name = os.environ.get("GITHUB_EVENT_NAME", "")
    diff = functools.cache(changed_paths)
    groups = select_groups(event_name, event, diff)
    hosts = onboarding_hosts(event_name, event, diff)
    outputs = "".join(f"{group}={str(group in groups).lower()}\n" for group in sorted(GROUPS))
    outputs += f"onboarding_hosts={json.dumps(hosts, separators=(',', ':'))}\n"
    # Unknown paths/diff failures still select all Linux consumers on PRs, but
    # only explicit full runs spend minutes on the extra platforms/versions.
    full = event_name not in {"pull_request", "push"}
    outputs += f"full={str(full).lower()}\n"
    print(outputs, end="")
    with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
        output.write(outputs)
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with Path(summary).open("a", encoding="utf-8") as output:
            output.write("Selected CI groups: " + ", ".join(sorted(groups)) + "\n")
            output.write("Qualification tier: " + ("full" if full else "PR/core") + "\n")


if __name__ == "__main__":
    main()
