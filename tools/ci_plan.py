"""Select native qualification gates without guessing at shared dependencies."""

import json
import os
from pathlib import Path
import re
import subprocess


GROUPS = frozenset({
    "rust", "e2e", "service_cli", "cdk", "terraform", "c", "cpp", "r",
    "ruby", "php", "dart", "swift", "kotlin", "csharp", "java", "go",
    "python", "typescript",
})
GENERATORS = {
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
    **{f"tools/test-{name}.py": groups for name, groups in GENERATORS.items()},
    "tools/test-ts.sh": {"typescript", "e2e"},
    "tools/test-e2e.sh": {"e2e"},
    "tools/test-kotlin-android.py": {"kotlin"},
    "tools/test-kotlin-multiplatform.py": {"kotlin"},
    "tools/test_kotlin_tools.py": {"kotlin"},
    "tools/install-yyjson.py": {"c"},
    # Planner-only edits exercise selection contracts and the live skip/aggregate
    # path without rebuilding native consumers whose sources did not change.
    "tools/ci_plan.py": {"e2e"},
    "tools/test_ci_plan.py": {"e2e"},
}


def affected_groups(paths):
    selected = {"rust"}
    for path in paths:
        parts = Path(path).parts
        if len(parts) >= 2 and parts[0] == "crates":
            # Models live in several generator test directories but are reused
            # by other languages (for example Python auth and Java waiters).
            if "/tests/" in path and path.endswith(".json") and "/tests/golden/" not in path:
                return set(GROUPS)
            crate = parts[1]
            if crate == "crucible-client":
                selected.update({"e2e", "service_cli"})
            elif crate == "crucible-cli-runtime":
                selected.add("service_cli")
            elif crate.startswith("crucible-codegen-"):
                language = crate.removeprefix("crucible-codegen-")
                # Rust server generation and common wire descriptions can
                # change the Weather service or every bundled SDK runtime.
                if language not in GENERATORS:
                    return set(GROUPS)
                selected.update(GENERATORS[language])
            else:
                # Framework, wire, build, CLI and Smithy changes affect the
                # generator/server used by independent native consumers.
                return set(GROUPS)
        elif path in TOOLS:
            selected.update(TOOLS[path])
        elif path.startswith("tools/cpp-vcpkg/"):
            selected.update({"c", "cpp"})
        elif path.startswith("docs/") and path.endswith(".md"):
            continue
        elif path in {"README.md", "AGENTS.md", "CLAUDE.md", "LICENSE"}:
            continue
        else:
            # New files and shared fixtures must not silently lose coverage.
            return set(GROUPS)
    return selected


def select_groups(event_name, event, diff):
    if event_name in {"schedule", "workflow_dispatch"}:
        return set(GROUPS)
    if event_name == "push":
        # PRs qualify affected native consumers; nightly catches interactions
        # on main. Keep immediate merged-code Rust and HTTP integration proof.
        return {"rust", "e2e"}
    if event_name != "pull_request":
        return set(GROUPS)
    try:
        pr = event["pull_request"]
        base, head = pr["base"]["sha"], pr["head"]["sha"]
        if not all(isinstance(sha, str) and re.fullmatch(r"[0-9a-f]{40}", sha)
                   for sha in (base, head)):
            return set(GROUPS)
        # Three-dot excludes unrelated changes that landed on the base branch.
        # No rename detection: select both the old and new path of a move.
        paths = diff(base, head)
    except (KeyError, TypeError, OSError, subprocess.CalledProcessError):
        return set(GROUPS)
    return affected_groups(paths)


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
    groups = select_groups(os.environ.get("GITHUB_EVENT_NAME", ""), event, changed_paths)
    outputs = "".join(f"{group}={str(group in groups).lower()}\n" for group in sorted(GROUPS))
    print(outputs, end="")
    with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
        output.write(outputs)
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with Path(summary).open("a", encoding="utf-8") as output:
            output.write("Selected CI groups: " + ", ".join(sorted(groups)) + "\n")


if __name__ == "__main__":
    main()
