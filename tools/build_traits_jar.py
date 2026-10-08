"""Packages smithy/crucible.smithy as a Smithy model jar.

A Smithy build finds trait definitions in jars on its classpath: each lists
its model files in META-INF/smithy/manifest. This is that jar, so a model
takes crucible's protocol as a dependency rather than a copied file:

    python tools/build_traits_jar.py [out_dir]

writes out_dir/crucible-traits-<version>.jar (default out_dir: dist), the
version read from [workspace.package].version in Cargo.toml.
"""

import pathlib
import sys
import zipfile
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
MODELS = ["crucible.smithy", "errors.smithy", "auth.smithy"]


def version(root: pathlib.Path = ROOT) -> str:
    return tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]


def build(out_dir: pathlib.Path) -> pathlib.Path:
    out_dir.mkdir(parents=True, exist_ok=True)
    jar = out_dir / f"crucible-traits-{version()}.jar"
    with zipfile.ZipFile(jar, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\n")
        archive.writestr("META-INF/smithy/manifest", "".join(f"{name}\n" for name in MODELS))
        for name in MODELS:
            text = (ROOT / "smithy" / name).read_text(encoding="utf-8")
            archive.writestr(f"META-INF/smithy/{name}", text)
    return jar


if __name__ == "__main__":
    print(build(pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "dist"))
