"""Report what each workspace crate still lacks before it can be published to crates.io.

Read-only: it never edits a manifest. Exits 1 when any crate that is meant to be
published has a problem, so it can gate a release once the manifests are fixed.

    python tools/check_publish_metadata.py            # table of problems, exit 1 if any
    python tools/check_publish_metadata.py --order    # also print the dependency-ordered publish list
    python tools/check_publish_metadata.py --json     # machine-readable
"""

import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
# Shipped as prebuilt binaries on GitHub Releases, never to crates.io.
BINARY_ONLY = {"crucible-cli"}
MAX_KEYWORDS = 5
MAX_CATEGORIES = 5


def workspace_packages(root):
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"],
        cwd=root, check=True, capture_output=True, text=True,
    ).stdout
    metadata = json.loads(out)
    members = set(metadata["workspace_members"])
    return [p for p in metadata["packages"] if p["id"] in members and p["name"] not in BINARY_ONLY]


def publishable(package):
    # `publish = false` is `[]` in cargo metadata; null means "any registry".
    return package["publish"] is None or bool(package["publish"])


def problems(package, members):
    """List the reasons `package` cannot be published as-is."""
    found = []
    if not publishable(package):
        found.append("publish = false (workspace or crate manifest)")
    if not package.get("description"):
        found.append("missing description")
    if not (package.get("license") or package.get("license_file")):
        found.append("missing license (SPDX expression) or license-file")
    if not package.get("repository"):
        found.append("missing repository")
    readme = package.get("readme")
    if not readme:
        found.append("missing readme")
    elif not (Path(package["manifest_path"]).parent / readme).is_file():
        found.append(f"readme file not found: {readme}")
    keywords = package.get("keywords") or []
    if not keywords:
        found.append("missing keywords")
    elif len(keywords) > MAX_KEYWORDS:
        found.append(f"more than {MAX_KEYWORDS} keywords")
    categories = package.get("categories") or []
    if not categories:
        found.append("missing categories")
    elif len(categories) > MAX_CATEGORIES:
        found.append(f"more than {MAX_CATEGORIES} categories")
    for dep in package["dependencies"]:
        # Cargo drops a dev-dependency that has a path but no version when packaging.
        if dep.get("path") is None or dep.get("kind") == "dev":
            continue
        # A path dependency is published as a registry dependency, which needs a version.
        if dep["req"] == "*":
            found.append(f"path dependency {dep['name']} has no version requirement")
        if dep["name"] not in members:
            found.append(f"path dependency {dep['name']} is outside the workspace")
        elif not members[dep["name"]]:
            found.append(f"depends on unpublishable workspace crate {dep['name']}")
    return found


def publish_order(packages):
    """Dependencies first; ties broken by name so the order is deterministic.

    Dev-dependency cycles do not constrain publishing (cargo strips the version
    from a dev-dependency that has a path), so only normal and build edges count.
    """
    names = {p["name"] for p in packages}
    deps = {
        p["name"]: {
            d["name"] for d in p["dependencies"]
            if d["name"] in names and d.get("kind") != "dev"
        }
        for p in packages
    }
    order, done = [], set()
    while len(done) < len(deps):
        ready = sorted(n for n, d in deps.items() if n not in done and d <= done)
        if not ready:
            cycle = sorted(set(deps) - done)
            raise ValueError(f"dependency cycle among: {', '.join(cycle)}")
        order.extend(ready)
        done.update(ready)
    return order


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--order", action="store_true", help="print the dependency-ordered publish list")
    parser.add_argument("--json", action="store_true", help="emit JSON instead of text")
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args(argv)

    packages = workspace_packages(args.root)
    members = {p["name"]: publishable(p) for p in packages}
    report = {p["name"]: problems(p, members) for p in sorted(packages, key=lambda p: p["name"])}
    try:
        order = publish_order(packages)
    except ValueError as error:
        order = None
        report["<workspace>"] = [str(error)]

    if args.json:
        print(json.dumps({"problems": report, "order": order}, indent=2))
    else:
        bad = {name: items for name, items in report.items() if items}
        for name, items in bad.items():
            print(f"{name}")
            for item in items:
                print(f"  - {item}")
        print(f"\n{len(bad)} of {len(report)} crates need changes before publishing")
        if args.order and order:
            print("\nPublish order:")
            for index, name in enumerate(order, 1):
                print(f"{index:3}. {name}")
    return 1 if any(report.values()) else 0


if __name__ == "__main__":
    sys.exit(main())
