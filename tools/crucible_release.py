"""Prepare reviewed workspace version bumps and verify manual GitHub releases."""

import argparse
from pathlib import Path
import re
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
# Core/nested lockfiles. Standalone consumer workspaces are discovered below;
# their locks hold only the subset of workspace crates that each consumer uses.
LOCKFILES = (
    "Cargo.lock",
    "e2e/Cargo.lock",
    "tests/consumers/leyline/Cargo.lock",
    "tests/consumers/leyline/leyline-sdk-consumer/Cargo.lock",
)
VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")
# Docs whose ```toml install snippets pin the exact workspace version.
VERSIONED_DOCS = ("crates/crucible-api/README.md", "docs/explorer.md", "docs/framework.md")
TOML_FENCE = re.compile(r"```toml\n(.*?)```", re.DOTALL)


def lockfiles(root, packages):
    # This bounded glob includes each standalone consumer without descending
    # into target directories or generated SDKs. The nested Leyline SDK is above.
    consumers = []
    for path in root.glob("tests/consumers/*/Cargo.lock"):
        entries = tomllib.loads(path.read_text(encoding="utf-8")).get("package", [])
        if any(entry["name"] in packages and "source" not in entry for entry in entries):
            consumers.append(path.relative_to(root).as_posix())
    return sorted(set(LOCKFILES).union(consumers))


def version_tuple(version):
    if not VERSION.fullmatch(version):
        raise ValueError("Use a stable semantic version such as 0.1.1")
    return tuple(map(int, version.split(".")))


def workspace(root):
    manifest = root / "Cargo.toml"
    content = manifest.read_text(encoding="utf-8")
    workspace_config = tomllib.loads(content)["workspace"]
    version = workspace_config["package"]["version"]
    packages = set()
    manifests = {
        path
        for member in workspace_config["members"]
        for path in root.glob(f"{member}/Cargo.toml")
    }
    for path in sorted(manifests):
        package = tomllib.loads(path.read_text(encoding="utf-8"))["package"]
        if package["version"] != {"workspace": True}:
            raise ValueError(f"{path} does not inherit the workspace version")
        packages.add(package["name"])
    if not packages:
        raise ValueError("No workspace packages found")
    return manifest, content, version, packages


def lock_content(path, packages, expected_version, replacement=None, require_all=False):
    content = path.read_text(encoding="utf-8")
    seen = set()
    blocks = re.split(r"(?=^\[\[package\]\]\s*$)", content, flags=re.MULTILINE)
    for index, block in enumerate(blocks):
        if not block.startswith("[[package]]"):
            continue
        package = tomllib.loads(block)["package"][0]
        if package["name"] in packages and "source" not in package:
            seen.add(package["name"])
            if package["version"] != expected_version:
                raise ValueError(f"{path}: {package['name']} is {package['version']}, expected {expected_version}")
            if replacement is not None:
                blocks[index], count = re.subn(r'^version = "[^"]+"$',
                                              f'version = "{replacement}"', block, count=1, flags=re.MULTILINE)
                if count != 1:
                    raise ValueError(f"Cannot update the version of {package['name']} in {path}")
    if not seen:
        raise ValueError(f"{path} has no workspace package entries")
    if require_all and seen != packages:
        raise ValueError(f"{path} is missing workspace packages: {sorted(packages - seen)}")
    return "".join(blocks)


def changelog_entry(root, version):
    """The CHANGELOG section for a version, or ValueError if there is none.

    A release without notes is how a breaking change reaches consumers
    unannounced, so an absent or empty section blocks both prepare and check.
    """
    path = root / "CHANGELOG.md"
    if not path.is_file():
        raise ValueError(f"{path} does not exist; add a '## [{version}]' entry")
    text = path.read_text(encoding="utf-8")
    sections = re.split(r"^## ", text, flags=re.MULTILINE)[1:]
    for section in sections:
        heading, _, body = section.partition("\n")
        if re.match(rf"\[{re.escape(version)}\](?: - \d{{4}}-\d{{2}}-\d{{2}})?\s*\Z", heading):
            body = re.sub(r"(?m)^### .*$", "", re.sub(r"(?s)<!--.*?-->", "", body)).strip()
            if not body:
                raise ValueError(f"CHANGELOG.md entry for {version} is empty")
            # The stability policy: a patch release never needs consumers to act.
            if version_tuple(version)[2] > 0 and re.search(r"(?m)^### Migration", section):
                raise ValueError(
                    f"CHANGELOG.md entry for patch release {version} has a Migration section; "
                    "a patch release must not break the stable surface (docs/releases.md)")
            return section
    raise ValueError(f"CHANGELOG.md has no '## [{version}]' entry (docs/releases.md)")


def readme_requirements(path, text, packages):
    """(crate, version) for each workspace crate a ```toml example depends on,
    whatever its form: inline, aliased with `package`, or a dependency table."""
    found = []
    for fence in TOML_FENCE.findall(text):
        try:
            document = tomllib.loads(fence)
        except tomllib.TOMLDecodeError as error:
            raise ValueError(f"{path}: invalid TOML example: {error}") from error
        # A snippet may also be a bare dependency line, without a table header.
        tables = [document]
        for section in (document, *document.get("target", {}).values()):
            tables.extend(section.get(kind, {}) for kind in
                          ("dependencies", "build-dependencies", "dev-dependencies"))
        for table in tables:
            for alias, dependency in table.items():
                name = dependency.get("package", alias) if isinstance(dependency, dict) else alias
                if name in packages:
                    version = dependency.get("version") if isinstance(dependency, dict) else dependency
                    found.append((name, version))
    return found


def bump_fence(fence, packages, current, replacement):
    """Rewrite `"=current"` only in statements that declare a workspace crate."""
    lines = fence.splitlines(keepends=True)
    out, table, index = [], None, 0
    while index < len(lines):
        stripped = lines[index].strip()
        if stripped.startswith("["):
            header = re.fullmatch(r"\[(?:.*\.)?(?:build-|dev-)?dependencies\.([^\]\s]+)\]", stripped)
            table = header.group(1).strip('"') if header else None
            if table is not None:
                # `[dependencies.alias]` may name its crate with `package =`.
                body = index + 1
                while body < len(lines) and not lines[body].strip().startswith("["):
                    body += 1
                try:
                    table = tomllib.loads("".join(lines[index + 1:body])).get("package", table)
                except tomllib.TOMLDecodeError:
                    pass
            out.append(lines[index])
            index += 1
            continue
        # A statement may span lines, such as an inline table whose features
        # array is split over several; take the shortest run that parses.
        end, entry = index + 1, {}
        for candidate in range(index + 1, len(lines) + 1):
            try:
                entry = tomllib.loads("".join(lines[index:candidate]))
            except tomllib.TOMLDecodeError:
                continue
            end = candidate
            break
        statement = "".join(lines[index:end])
        declares = (table in packages and re.match(r"version\s*=", stripped) is not None) or any(
            (dependency.get("package", alias) if isinstance(dependency, dict) else alias) in packages
            for alias, dependency in entry.items())
        if declares:
            # TOML allows either quote; keep the snippet's own.
            statement = statement.replace(f'"={current}"', f'"={replacement}"').replace(
                f"'={current}'", f"'={replacement}'")
        out.append(statement)
        index = end
    return "".join(out)


def versioned_docs(root, packages, current, replacement=None):
    """Check/update the `=X.Y.Z` install requirements in published READMEs."""
    edits = {}
    for relative in VERSIONED_DOCS:
        path = root / relative
        if not path.is_file():
            raise ValueError(f"{path} is missing; it is a published README with a pinned install requirement")
        content = path.read_text(encoding="utf-8")
        requirements = readme_requirements(path, content, packages)
        if not requirements:
            raise ValueError(f"{path} has no workspace crate requirement")
        for name, version in requirements:
            if version != f"={current}":
                raise ValueError(f"{path}: {name} must require ={current}")
        if replacement is not None:
            content = TOML_FENCE.sub(
                lambda match: match.group(0).replace(
                    match.group(1), bump_fence(match.group(1), packages, current, replacement)),
                content)
            if any(version != f"={replacement}" for _, version in readme_requirements(path, content, packages)):
                raise ValueError(f"Cannot update every install requirement in {path}")
        edits[path] = content
    return edits


def check(root, version):
    version_tuple(version)
    _, _, current, packages = workspace(root)
    if version != current:
        raise ValueError(f"Requested {version}, but the merged workspace version is {current}")
    changelog_entry(root, version)
    dependency_manifests(root, packages, current)
    versioned_docs(root, packages, current)
    for lock in lockfiles(root, packages):
        lock_content(root / lock, packages, current, require_all=lock == "Cargo.lock")


def dependency_manifests(root, packages, current, replacement=None):
    """Check/update only versioned paths into this release's workspace crates."""
    edits = {}
    members = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["members"]
    paths = [root / "Cargo.toml", *sorted({
        path for member in members for path in root.glob(f"{member}/Cargo.toml")
    })]
    for path in paths:
        content = path.read_text(encoding="utf-8")
        document = tomllib.loads(content)
        tables = [document.get("workspace", {}).get("dependencies", {})]
        for section in (document, *document.get("target", {}).values()):
            tables.extend(section.get(kind, {}) for kind in
                          ("dependencies", "build-dependencies", "dev-dependencies"))
        for table in tables:
            for alias, dependency in table.items():
                if not isinstance(dependency, dict) or "path" not in dependency:
                    continue
                name = dependency.get("package", alias)
                if name not in packages or "version" not in dependency:
                    continue
                if dependency["version"] != "=" + current:
                    raise ValueError(f"{path}: {alias} must require ={current}")
                if replacement is not None:
                    pattern = rf'(?m)^({re.escape(alias)}\s*=\s*\{{[^\n]*?\bversion\s*=\s*)"={re.escape(current)}"'
                    content, count = re.subn(pattern, rf'\g<1>"={replacement}"', content)
                    if count != 1:
                        raise ValueError(f"Cannot update dependency {alias} in {path}")
        edits[path] = content
    return edits


def prepare(root, version):
    manifest, content, current, packages = workspace(root)
    if version_tuple(version) <= version_tuple(current):
        raise ValueError(f"New version must be greater than {current}")
    changelog_entry(root, version)
    # Validate every file before writing, so a stale lockfile leaves the
    # checkout intact. Registry dependencies and example-service versions stay put.
    edits = dependency_manifests(root, packages, current, version)
    edits.update(versioned_docs(root, packages, current, version))
    for lock in lockfiles(root, packages):
        edits[root / lock] = lock_content(root / lock, packages, current, version, require_all=lock == "Cargo.lock")
    start = content.index("[workspace.package]")
    end = content.find("\n[", start + 1)
    if end < 0:
        end = len(content)
    updated = edits[manifest]
    section, count = re.subn(r'^version = "[^"]+"$', f'version = "{version}"',
                             updated[start:end], count=1, flags=re.MULTILINE)
    if count != 1:
        raise ValueError("Cannot update [workspace.package].version")
    edits[manifest] = updated[:start] + section + updated[end:]
    changed = []
    for path, text in edits.items():
        if path.read_text(encoding="utf-8") != text:
            changed.append(path.relative_to(root).as_posix())
        path.write_text(text, encoding="utf-8", newline="\n")
    check(root, version)
    return sorted(changed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("prepare", "check"))
    parser.add_argument("version")
    args = parser.parse_args()
    try:
        changed = globals()[args.action](ROOT, args.version)
    except (ValueError, OSError, KeyError) as error:
        parser.exit(1, f"Release {args.action} failed: {error}\n")
    # `prepare` lists every file it changed, one per line, to stage as is.
    # LF on every platform: Windows text mode would end each redirected line
    # with CR, which git then reads as part of the path.
    sys.stdout.reconfigure(newline="\n")
    for path in changed or ():
        print(path)
    print(f"Crucible {args.version}: {args.action} passed", file=sys.stderr if changed else sys.stdout)


if __name__ == "__main__":
    main()
