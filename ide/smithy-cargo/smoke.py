#!/usr/bin/env python3
"""Run real Smithy navigation and annotation checks in an isolated headless IDE."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile

from build import ROOT, build


def smoke(ide, smithy, output, license_file=None):
    repo = ROOT.parents[1]
    output.mkdir(parents=True, exist_ok=True)
    build(ide, output, smoke=True)
    directory = Path(tempfile.mkdtemp(prefix="smithy-cargo-ide-"))
    # Preserve the isolated profile on failure for diagnostic logs.
    project = directory / "service"
    (project / "smithy").mkdir(parents=True)
    dependency = json.dumps((repo / "crates/crucible-build").as_posix())
    (project / "Cargo.toml").write_text('[package]\nname="smithy-editor-test"\nversion="0.0.0"\nedition="2024"\n'
                                       '[package.metadata.smithy]\nsources=["smithy"]\n'
                                       f'[build-dependencies]\ncrucible-build={{path={dependency}}}\n', encoding="utf-8")
    (project / "src").mkdir()
    (project / "src/lib.rs").write_text("", encoding="utf-8")
    shutil.copyfile(repo / "crates/crucible-cli/src/starter/hello.smithy", project / "smithy/hello.smithy")
    subprocess.run(["cargo", "metadata", "--format-version", "1"], cwd=project, check=True, stdout=subprocess.DEVNULL)
    settings = project / ".idea"
    settings.mkdir()
    (settings / "modules.xml").write_text('<project version="4"><component name="ProjectModuleManager"><modules><module fileurl="file://$PROJECT_DIR$/.idea/service.iml" filepath="$PROJECT_DIR$/.idea/service.iml"/></modules></component></project>')
    (settings / "service.iml").write_text('<module type="EMPTY_MODULE" version="4"><component name="NewModuleRootManager"><content url="file://$MODULE_DIR$/.."/><orderEntry type="sourceFolder" forTests="false"/></component></module>')
    plugins = directory / "plugins"
    plugins.mkdir()
    shutil.copyfile(smithy, plugins / smithy.name)
    shutil.copyfile(output / "smithy-cargo-smoke.jar", plugins / "smithy-cargo-smoke.jar")
    with zipfile.ZipFile(output / "smithy-cargo-0.1.0.zip") as archive:
        archive.extractall(plugins)
    copied_license = None
    if license_file:
        config = directory / "config"
        config.mkdir()
        copied_license = config / "rustrover.key"
        shutil.copyfile(license_file, copied_license)
    info = json.loads((ide / "product-info.json").read_text())
    launch = next(item for item in info["launch"] if item["os"] == ("Windows" if os.name == "nt" else "Linux"))
    boot = os.pathsep.join(str(ide / "lib" / name) for name in launch["bootClassPathJarNames"])
    arguments = ["-Xmx2048m", "-Djava.awt.headless=true", "-Didea.platform.prefix=RustRover",
                 "-Didea.is.internal=true", "-Didea.trust.all.projects=true", "-Didea.no.splash=true",
                 "-Didea.home.path=" + str(ide), "-Didea.config.path=" + str(directory / "config"),
                 "-Didea.system.path=" + str(directory / "system"), "-Didea.log.path=" + str(directory / "log"),
                 "-Didea.plugins.path=" + str(plugins), "-Dide.show.tips.on.startup.default.value=false",
                 *launch.get("additionalJvmArguments", []), "-classpath", boot,
                 "com.intellij.idea.Main", "smithyCargoSmoke", str(project)]
    arguments = [value.replace("%IDE_HOME%", str(ide)).replace("$IDE_HOME$", str(ide)).replace("$IDE_HOME", str(ide)) for value in arguments]
    args_file = directory / "ide.args"
    args_file.write_text("\n".join(json.dumps(value.replace("\\", "/")) for value in arguments), encoding="utf-8")
    java = ide / "jbr/bin" / ("java.exe" if os.name == "nt" else "java")
    print("Isolated headless profile: " + str(directory), flush=True)
    log = directory / "console.log"
    try:
        with log.open("w", encoding="utf-8") as stream:
            result = subprocess.run([str(java), "@" + str(args_file)], cwd=ide, stdout=stream, stderr=subprocess.STDOUT, timeout=180)
    finally:
        if copied_license:
            copied_license.unlink(missing_ok=True)
    text = log.read_text(encoding="utf-8")
    if result.returncode != 0 or "SMITHY_CARGO_SMOKE_OK" not in text:
        raise AssertionError(f"headless editor smoke failed; inspect {directory}:\n{text[-12000:]}")
    print("Real Smithy reference resolution and annotation smoke passed")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ide-home", required=True, type=Path)
    parser.add_argument("--smithy-plugin", required=True, type=Path)
    parser.add_argument("--out", type=Path, default=ROOT / "dist")
    parser.add_argument("--license-file", type=Path, help="existing local RustRover license; copied only for this test and removed afterward")
    arguments = parser.parse_args()
    smoke(arguments.ide_home.resolve(), arguments.smithy_plugin.resolve(), arguments.out.resolve(), arguments.license_file)
