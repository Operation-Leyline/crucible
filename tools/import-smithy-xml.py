"""Refresh pinned official XML protocol fixtures using Smithy's compiler (JDK required).

This is an explicit networked maintenance command, not part of the test gate.
The checked-in fixtures retain excluded operation shapes and vectors; metadata
records why their service references are removed from the supported projection.
"""
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import subprocess
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
PIN = "bd0ed9e054b29874a5bec389e16d8c4db9dc9b94"
VERSION = "1.74.0"
CACHE = ROOT / "target/official-smithy"
COMPILER = ROOT / "target/smithy-compiler"
PREFIX = "smithy-aws-protocol-tests/model/"
SHARED = [PREFIX + "shared-types.smithy", PREFIX + "aws-config.smithy",
          "smithy-protocol-test-traits/src/main/resources/META-INF/smithy/smithy.test.smithy"]


def download(url, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        with urllib.request.urlopen(url, timeout=60) as response:
            path.write_bytes(response.read())
    return path


def main():
    for name in ("LICENSE", "NOTICE"):
        source = download(f"https://raw.githubusercontent.com/smithy-lang/smithy/{PIN}/{name}", CACHE / name)
        destination = ROOT / "crates/crucible-smithy/tests/fixtures" / ("SMITHY-" + name + ".txt")
        destination.write_bytes(source.read_bytes())
    tree_path = download(f"https://api.github.com/repos/smithy-lang/smithy/git/trees/{PIN}?recursive=1",
                         CACHE / (PIN + "-tree.json"))
    tree = json.loads(tree_path.read_text(encoding="utf-8"))
    if tree.get("truncated"):
        raise RuntimeError("Smithy source tree is truncated")
    groups = {"rest-xml": ("restXml", "restXmlWithNamespace"),
              "aws-query": ("awsQuery",), "ec2-query": ("ec2Query",)}
    paths = sorted({entry["path"] for entry in tree["tree"]
                    if entry["type"] == "blob" and entry["path"].endswith(".smithy")
                    and (entry["path"] in SHARED or any(entry["path"].startswith(PREFIX + name + "/")
                         for names in groups.values() for name in names))})
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        list(pool.map(lambda path: download(f"https://raw.githubusercontent.com/smithy-lang/smithy/{PIN}/{path}",
                                             CACHE / path), paths))
    for artifact in ("smithy-model", "smithy-utils", "smithy-aws-traits", "smithy-jmespath"):
        download(f"https://repo.maven.apache.org/maven2/software/amazon/smithy/{artifact}/{VERSION}/{artifact}-{VERSION}.jar",
                 COMPILER / (artifact + ".jar"))
    classpath = str(COMPILER / "*")
    subprocess.run(["javac", "-cp", classpath, "-d", str(COMPILER),
                    str(ROOT / "tools/AssembleProtocolTests.java")], check=True)
    for label, names in groups.items():
        sources = [path for path in paths if path in SHARED or
                   any(path.startswith(PREFIX + name + "/") for name in names)]
        assembled = COMPILER / (label + ".json")
        subprocess.run(["java", "-cp", os.pathsep.join([str(COMPILER), classpath]),
                        "AssembleProtocolTests", str(assembled), *[str(CACHE / path) for path in sources]], check=True)
        model = json.loads(assembled.read_text(encoding="utf-8"))
        exclusions = []
        for service_id, service in model["shapes"].items():
            if service["type"] != "service":
                continue
            supported = []
            for ref in service.get("operations", []):
                operation = model["shapes"][ref["target"]]
                if "smithy.api#requestCompression" in operation.get("traits", {}):
                    tests = operation["traits"].get("smithy.test#httpRequestTests", [])
                    exclusions.append({"service": service_id, "operation": ref["target"],
                                       "reason": "@requestCompression is diagnosed as unsupported",
                                       "tests": [test["id"] for test in tests]})
                else:
                    supported.append(ref)
            service["operations"] = supported
        model.setdefault("metadata", {})["crucibleQualification"] = {
            "repository": "https://github.com/smithy-lang/smithy", "revision": PIN, "compilerVersion": VERSION,
            "transform": "flattenAndRemoveMixins", "excludedOperations": exclusions,
            "sources": [{"path": path, "sha256": hashlib.sha256((CACHE / path).read_bytes()).hexdigest()}
                        for path in sources]}
        destination = ROOT / "crates/crucible-smithy/tests/fixtures" / ("protocol-" + label + ".json")
        destination.write_text(json.dumps(model, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"Imported {destination.name}: {len(sources)} sources, {len(exclusions)} excluded operations")


if __name__ == "__main__":
    main()
