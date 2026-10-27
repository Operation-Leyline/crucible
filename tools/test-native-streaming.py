"""Installed Python/Go stream consumers over raw TCP and the Rust framework."""
import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from sdk_test_support import generator_command

ROOT = Path(__file__).resolve().parents[1]


def run(*args, cwd=ROOT, **kwargs):
    subprocess.run(generator_command(args), cwd=cwd, check=True, **kwargs)


def qualify(language, output):
    fixture = os.environ.get("CRUCIBLE_STREAM_FIXTURE")
    if fixture:
        # The consumers launch it from their own directories.
        fixture = str(Path(fixture).resolve())
    else:
        run("cargo", "build", "--locked", "--manifest-path", "tests/consumers/native-stream-http/Cargo.toml")
        # Cargo resolves a relative CARGO_TARGET_DIR against its cwd, ROOT.
        target = ROOT / os.environ.get("CARGO_TARGET_DIR", "tests/consumers/native-stream-http/target")
        fixture = str(target / "debug" / ("native-stream-http.exe" if os.name == "nt" else "native-stream-http"))
    env = dict(os.environ, CRUCIBLE_STREAM_FIXTURE=fixture,
               CRUCIBLE_STREAM_VECTORS=str(ROOT / "tests/conformance/response-stream.json"))
    sdk = output / "sdk"
    options = ("--package-name", "streams-client", "--module-name", "streams_client") if language == "python" else ("--module-path", "example.com/streams", "--package-name", "streams")
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", language,
        "tests/consumers/rust-stream-client/model", "--out", str(sdk), *options)
    # Also qualify the existing runtime without errorDocs; #328's additive
    # runtime specialization must not make streaming imports depend on it.
    no_docs = output / "no-docs-model"
    shutil.copytree(ROOT / "tests/consumers/rust-stream-client/model", no_docs)
    model = no_docs / "model.cru"
    model.write_text(re.sub(r'(?m)^    errorDocs [^\n]*\n', '', model.read_text(encoding="utf-8")), encoding="utf-8")
    no_docs_sdk = output / "no-docs-sdk"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", language,
        str(no_docs), "--out", str(no_docs_sdk), *options)
    if language == "python":
        run(sys.executable, "-m", "mypy", str(sdk / "src"))
        run(sys.executable, "-m", "mypy", str(no_docs_sdk / "src"))
        samples = output / "samples"; samples.mkdir()
        for index, code in enumerate(re.findall(r"```python\n(.*?)```", (sdk / "README.md").read_text(encoding="utf-8"), re.S)):
            if "import streams_client" in code:
                (samples / f"guide_{index}.py").write_text(code, encoding="utf-8")
        # A guide regression that drops every sample must not pass vacuously.
        if not any(samples.iterdir()):
            raise SystemExit("no Python README samples were extracted for type checking")
        run(sys.executable, "-m", "mypy", str(samples), env=dict(env, MYPYPATH=str(sdk / "src")))
        run(sys.executable, "-m", "build", "--wheel", str(sdk))
        installed = output / "installed"
        run(sys.executable, "-m", "pip", "install", "--no-deps", "--target", str(installed), str(next((sdk / "dist").glob("*.whl"))))
        # The process imports exclusively the installed wheel, from a directory
        # with no generated source or repository packages on sys.path.
        consumer = output / "consumer"; consumer.mkdir()
        shutil.copyfile(ROOT / "crates/crucible-codegen-python/tests/streaming_consumer.py", consumer / "consumer.py")
        run(sys.executable, "consumer.py", "-v", cwd=consumer, env=dict(env, PYTHONPATH=str(installed)))
        run(sys.executable, "-m", "build", "--wheel", str(no_docs_sdk))
        no_docs_installed = output / "no-docs-installed"
        run(sys.executable, "-m", "pip", "install", "--no-deps", "--target", str(no_docs_installed), str(next((no_docs_sdk / "dist").glob("*.whl"))))
        # Run the same public consumers so legacy/no-base runtime branches are
        # exercised over native HTTP rather than only importing successfully.
        run(sys.executable, "consumer.py", "-v", cwd=consumer, env=dict(env, PYTHONPATH=str(no_docs_installed)))
    else:
        go = os.environ.get("GO", "go")
        for package in (sdk, no_docs_sdk):
            run(go, "fmt", "./...", cwd=package)
            run(go, "vet", "./...", cwd=package)
            consumer = output / ("consumer-" + package.name); consumer.mkdir()
            (consumer / "go.mod").write_text(f'module example.com/stream-consumer\n\ngo 1.24.0\nrequire example.com/streams v0.0.0\nreplace example.com/streams => ../{package.name}\n', encoding="utf-8")
            shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/streaming_test.go", consumer / "streaming_test.go")
            extracted = 0
            for index, code in enumerate(re.findall(r"```go\n(.*?)```", (package / "README.md").read_text(encoding="utf-8"), re.S)):
                if code.startswith("package main"):
                    sample = consumer / "cmd" / f"guide{index}"; sample.mkdir(parents=True)
                    (sample / "main.go").write_text(code, encoding="utf-8")
                    extracted += 1
            # A guide regression that drops every sample must not pass vacuously.
            if not extracted:
                raise SystemExit(f"no Go README samples were extracted from {package.name}")
            run(go, "fmt", "./...", cwd=consumer)
            run(go, "vet", "./...", cwd=consumer)
            run(go, "test", "-count=1", "-v", "./...", cwd=consumer, env=env)


if __name__ == "__main__":
    parser=argparse.ArgumentParser();parser.add_argument("language",choices=["python","go"])
    parser.add_argument("--out",type=Path);args=parser.parse_args()
    if args.out:
        args.out.mkdir(parents=True)
        qualify(args.language,args.out.resolve())
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-native-stream-") as directory:
            qualify(args.language,Path(directory))
