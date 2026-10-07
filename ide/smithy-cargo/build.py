#!/usr/bin/env python3
"""Compile and package Smithy Cargo against an installed RustRover SDK."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent


def build(ide, destination, smoke=False):
    info = json.loads((ide / "product-info.json").read_text(encoding="utf-8"))
    if info["productCode"] != "RR" or not info["buildNumber"].startswith("262."):
        raise ValueError("this integration is qualified against RustRover 2026.2 (build 262)")
    java = ide / "jbr/bin" / ("java.exe" if os.name == "nt" else "java")
    javac = java.with_name("javac.exe" if os.name == "nt" else "javac")
    if not javac.is_file():
        raise ValueError("the SDK needs a full JDK with javac")
    jars = sorted((ide / "lib").glob("*.jar"))
    classpath = os.pathsep.join(str(path) for path in jars)
    with tempfile.TemporaryDirectory(prefix="smithy-cargo-build-") as temporary:
        directory = Path(temporary)
        classes = directory / "classes"
        classes.mkdir()
        sources = sorted((ROOT / "src/main/java").rglob("*.java")) + sorted((ROOT / "src/test/java").rglob("*.java"))
        arguments = ["--release", "21", "-encoding", "UTF-8", "-classpath", classpath, "-d", str(classes), *map(str, sources)]
        argument_file = directory / "javac.args"
        # javac @files use their own quoting; forward slashes avoid Windows escapes.
        argument_file.write_text("\n".join(json.dumps(value.replace("\\", "/")) for value in arguments), encoding="utf-8")
        subprocess.run([str(javac), "@" + str(argument_file)], check=True)
        runtime_args = directory / "java.args"
        runtime_args.write_text("\n".join(json.dumps(value.replace("\\", "/")) for value in
                                         ["-ea", "-classpath", str(classes) + os.pathsep + classpath,
                                          "com.leyline.smithy.cargo.CargoMetadataTest"]), encoding="utf-8")
        subprocess.run([str(java), "@" + str(runtime_args)], check=True)
        jar = directory / "smithy-cargo.jar"
        with zipfile.ZipFile(jar, "w", zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(classes.rglob("*.class")):
                if "Test" not in path.name and "Smoke" not in path.name:
                    archive.write(path, path.relative_to(classes).as_posix())
            archive.write(ROOT / "src/main/resources/META-INF/plugin.xml", "META-INF/plugin.xml")
        destination.mkdir(parents=True, exist_ok=True)
        output = destination / "smithy-cargo-0.1.0.zip"
        with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as archive:
            archive.write(jar, "smithy-cargo/lib/smithy-cargo.jar")
        if smoke:
            with zipfile.ZipFile(destination / "smithy-cargo-smoke.jar", "w", zipfile.ZIP_DEFLATED) as archive:
                for path in sorted(classes.rglob("CargoSmoke*.class")):
                    archive.write(path, path.relative_to(classes).as_posix())
                archive.writestr("META-INF/plugin.xml", '<idea-plugin><id>com.leyline.smithy.cargo.smoke</id><name>Smithy Cargo Smoke</name><version>0.1.0</version><idea-version since-build="262"/><depends>com.intellij.modules.platform</depends><depends>com.leyline.smithy.cargo</depends><depends>software.amazon.smithy.intellij</depends><extensions defaultExtensionNs="com.intellij"><appStarter id="smithyCargoSmoke" isInternal="true" implementation="com.leyline.smithy.cargo.CargoSmoke"/></extensions></idea-plugin>')
        print(output)
        return output


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ide-home", required=True, type=Path)
    parser.add_argument("--out", type=Path, default=ROOT / "dist")
    parser.add_argument("--smoke", action="store_true")
    arguments = parser.parse_args()
    build(arguments.ide_home.resolve(), arguments.out.resolve(), arguments.smoke)
