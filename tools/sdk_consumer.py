"""Check public exports from installed release artifacts, outside their source tree."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("language", choices=("python", "ts"))
    parser.add_argument("module")
    parser.add_argument("exports", nargs="+")
    args = parser.parse_args()
    artifact = next(Path.cwd().glob("dist/*.whl" if args.language == "python" else "*.tgz"))
    with tempfile.TemporaryDirectory(prefix="crucible-release-consumer-") as temporary:
        root = Path(temporary)
        if args.language == "python":
            subprocess.run([sys.executable, "-m", "venv", str(root / "venv")], check=True)
            python = root / "venv" / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
            subprocess.run([str(python), "-m", "pip", "install", str(artifact)], check=True)
            code = ("import importlib,sys; module=importlib.import_module(sys.argv[1]); "
                    "missing=[name for name in sys.argv[2:] if not hasattr(module,name)]; "
                    "sys.exit('Missing public exports: '+str(missing) if missing else 0)")
            subprocess.run([str(python), "-I", "-c", code, args.module, *args.exports], cwd=root, check=True)
        else:
            import shutil
            npm = shutil.which("npm")
            subprocess.run([npm, "install", "--prefix", temporary, "--ignore-scripts", "--no-audit",
                            "--no-fund", "--no-package-lock", str(artifact)], check=True)
            (root / "consumer.mjs").write_text(
                "const sdk = await import(" + json.dumps(args.module) + ");\n"
                "for (const name of " + json.dumps(args.exports) + ") {\n"
                "  if (!(name in sdk)) throw new Error(`Missing public export: ${name}`);\n}\n",
                encoding="utf-8")
            subprocess.run(["node", "consumer.mjs"], cwd=root, check=True)


if __name__ == "__main__":
    main()
