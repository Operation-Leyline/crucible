"""Build versioned SDK artifacts once, verify their provenance, then publish them.

The checked-in release configuration owns native build/test/publish commands.
Commands are argument arrays, never shell programs. Credentials stay in the
process environment and are neither expanded into plans nor saved in manifests.
"""
import argparse
from datetime import datetime, timezone
import glob
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

LANGUAGES = frozenset("rust ts python go java kotlin csharp swift dart php ruby cpp c r".split())
RESERVED = frozenset(("--out", "--service", "--package-version"))
VERSION = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?\Z")


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def within(root, value):
    path = (root / value).resolve()
    if not path.is_relative_to(root.resolve()):
        raise ValueError(f"Path escapes release directory: {value}")
    return path


def command(value):
    if not isinstance(value, list) or not value or not all(isinstance(v, str) and v for v in value):
        raise ValueError("Commands must be nonempty arrays of string arguments")
    return value


def load_config(path):
    config = read_json(path)
    if config.get("schemaVersion") != 1:
        raise ValueError("Unsupported SDK release configuration version")
    if not isinstance(config.get("service"), str) or "#" not in config["service"]:
        raise ValueError("A Smithy service shape ID is required")
    targets = config.get("targets")
    if not isinstance(targets, dict) or not targets or not set(targets) <= LANGUAGES:
        raise ValueError("Specify one or more supported SDK targets")
    command(config.get("generator", ["crucible"]))
    for language, target in targets.items():
        args = target.get("arguments", [])
        if not isinstance(args, list) or not all(isinstance(a, str) for a in args):
            raise ValueError(f"Invalid generator arguments for {language}")
        if any(a.split("=", 1)[0] in RESERVED for a in args):
            raise ValueError("The release owns output, service and version arguments")
        for phase in ("prepare", "build", "test", "publish"):
            steps = target.get(phase, [])
            if not isinstance(steps, list):
                raise ValueError(f"{language}.{phase} must be an array of commands")
            for step in steps:
                command(step)
        if not target.get("build") or not target.get("test"):
            raise ValueError(f"{language} requires native build and consumer test commands")
        patterns = target.get("artifacts")
        if not isinstance(patterns, list) or not patterns or not all(isinstance(p, str) for p in patterns):
            raise ValueError(f"{language} requires artifact patterns")
        for pattern in patterns:
            if Path(pattern).is_absolute() or ".." in Path(pattern).parts:
                raise ValueError(f"Artifact pattern escapes package: {pattern}")
    return config


def execute(argv, cwd, variables):
    expanded = expand(command(argv), variables)
    expanded[0] = shutil.which(expanded[0]) or expanded[0]
    subprocess.run(expanded, cwd=cwd, check=True, shell=False)


def expand(argv, variables):
    # Keep braces in native code arguments untouched; only named release
    # placeholders are expanded. In particular, environment variables are not.
    return [re.sub(r"\{(version|package|release|source|python|artifact)\}",
                   lambda match: variables[match[1]], part) for part in argv]


def plan(config_path, version, output):
    if not VERSION.fullmatch(version):
        raise ValueError("Release version must be an explicit semantic version")
    config = load_config(config_path)
    source = config_path.parent.resolve()
    model = (source / config["model"]).resolve()
    if not model.is_file():
        raise ValueError(f"Model does not exist: {model}")
    result = {"schemaVersion": 1, "version": version, "service": config["service"],
              "modelSha256": digest(model), "configurationSha256": digest(config_path),
              "generator": config.get("generator", ["crucible"]), "targets": {}}
    for language, target in sorted(config["targets"].items()):
        package = output.resolve() / language
        destination = package / "src/lib.rs" if language == "rust" else package
        generate = [*result["generator"], "generate", language, str(model),
                    "--service", config["service"], "--out", str(destination), *target.get("arguments", [])]
        # Rust's package recipe owns Cargo's version; Go versions belong to module tags.
        if language not in ("rust", "go"):
            generate += ["--package-version", version]
        result["targets"][language] = {"generate": generate, **{
            phase: target.get(phase, []) for phase in ("prepare", "build", "test", "publish")},
            "artifacts": target["artifacts"]}
    return result


def build(config_path, version, output, runner=execute):
    release = plan(config_path, version, output)
    if output.exists() and any(output.iterdir()):
        raise ValueError("Use a fresh release directory; existing artifacts are never overwritten")
    output.mkdir(parents=True, exist_ok=True)
    write_json(output / "plan.json", release)
    artifacts = []
    for language, target in release["targets"].items():
        package = output.resolve() / language
        package.mkdir(parents=True)
        if language == "rust":
            (package / "src").mkdir()
        variables = {"version": version, "package": str(package),
                     "release": str(output.resolve()), "source": str(config_path.parent.resolve()),
                     "python": sys.executable}
        runner(target["generate"], config_path.parent.resolve(), variables)
        for phase in ("prepare", "build", "test"):
            for step in target[phase]:
                runner(step, package, variables)
        found = set()
        for pattern in target["artifacts"]:
            matches = sorted(glob.glob(str(package / pattern), recursive=True))
            if not matches:
                raise ValueError(f"No {language} artifact matches {pattern}")
            for match in matches:
                artifact = within(package, Path(match))
                if not artifact.is_file():
                    raise ValueError(f"Artifact is not a file: {artifact}")
                found.add(artifact)
        for artifact in sorted(found):
            artifacts.append({"target": language, "path": artifact.relative_to(output.resolve()).as_posix(),
                              "sha256": digest(artifact), "size": artifact.stat().st_size})
    manifest = {"schemaVersion": 1, "service": release["service"], "version": version,
                "modelSha256": release["modelSha256"], "configurationSha256": release["configurationSha256"],
                "planSha256": digest(output / "plan.json"), "artifacts": artifacts}
    write_json(output / "release.json", manifest)
    return manifest


def verify(output):
    manifest = read_json(output / "release.json")
    if manifest.get("schemaVersion") != 1 or not manifest.get("artifacts"):
        raise ValueError("Release has no verified artifacts")
    if digest(output / "plan.json") != manifest["planSha256"]:
        raise ValueError("Release plan changed after validation")
    seen = set()
    for item in manifest["artifacts"]:
        if item["path"] in seen:
            raise ValueError("Duplicate artifact in manifest")
        seen.add(item["path"])
        artifact = within(output, item["path"])
        if artifact.stat().st_size != item["size"] or digest(artifact) != item["sha256"]:
            raise ValueError(f"Artifact changed after validation: {item['path']}")
    return manifest


def publish(output, targets, execute_publish=False, runner=execute):
    manifest = verify(output)
    release = read_json(output / "plan.json")
    selected = sorted(targets or release["targets"])
    if not set(selected) <= set(release["targets"]):
        raise ValueError("Publication requested an unbuilt target")
    journal_path = output / "publication.json"
    journal = read_json(journal_path) if journal_path.exists() else {
        "version": manifest["version"], "planSha256": manifest["planSha256"], "completed": [], "steps": []}
    if journal.get("version") != manifest["version"] or journal.get("planSha256") != manifest["planSha256"]:
        raise ValueError("Publication journal belongs to another release")
    commands = []
    for language in selected:
        steps = release["targets"][language]["publish"]
        if not steps:
            raise ValueError(f"No registry publication recipe for {language}")
        if language in journal["completed"]:
            continue
        package = within(output, language)
        artifacts = [within(output, a["path"]) for a in manifest["artifacts"] if a["target"] == language]
        variables = {"version": manifest["version"], "package": str(package),
                     "release": str(output.resolve()), "python": sys.executable}
        for index, step in enumerate(steps):
            # An artifact placeholder repeats the command for each validated artifact.
            batches = artifacts if any("{artifact}" in part for part in step) else [None]
            for artifact in batches:
                current = {**variables, "artifact": str(artifact) if artifact else ""}
                expanded = expand(step, current)
                step_id = f"{language}:{index}:{artifact.relative_to(output) if artifact else ''}"
                if step_id in journal["steps"]:
                    continue
                commands.append({"target": language, "command": expanded})
                if execute_publish:
                    verify(output)
                    runner(step, package, current)
                    journal["steps"].append(step_id)
                    journal["updatedAt"] = datetime.now(timezone.utc).isoformat()
                    write_json(journal_path, journal)
        if execute_publish:
            journal["completed"].append(language)
            journal["updatedAt"] = datetime.now(timezone.utc).isoformat()
            write_json(journal_path, journal)
    return commands


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    for action in ("plan", "build"):
        child = commands.add_parser(action)
        child.add_argument("config", type=Path)
        child.add_argument("--version", required=True)
        child.add_argument("--out", required=True, type=Path)
    check = commands.add_parser("verify")
    check.add_argument("directory", type=Path)
    push = commands.add_parser("publish")
    push.add_argument("directory", type=Path)
    push.add_argument("--target", action="append", choices=sorted(LANGUAGES))
    push.add_argument("--execute", action="store_true", help="Execute configured registry commands; otherwise print the plan")
    args = parser.parse_args()
    try:
        if args.action in ("plan", "build"):
            result = globals()[args.action](args.config.resolve(), args.version, args.out.resolve())
        elif args.action == "verify":
            result = verify(args.directory.resolve())
        else:
            result = publish(args.directory.resolve(), args.target, args.execute)
        print(json.dumps(result, indent=2, sort_keys=True))
    except (ValueError, OSError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"SDK release failed: {error}\n")


if __name__ == "__main__":
    main()
