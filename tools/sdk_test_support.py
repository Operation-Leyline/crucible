"""Shared native SDK qualification helpers; all services stay on loopback."""
from contextlib import contextmanager
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[1]


def generator_command(args):
    """Allow concurrent native gates to use one explicitly frozen generator build."""
    binary = os.environ.get("CRUCIBLE_BIN")
    if binary and args[:2] == ("cargo", "run") and "crucible-cli" in args and "--" in args:
        path = Path(binary).resolve()
        if not path.is_file():
            raise RuntimeError(f"CRUCIBLE_BIN is not an executable file: {path}")
        return (str(path), *args[args.index("--") + 1:])
    return args


def run(*args, **kwargs):
    args = generator_command(args)
    timeout = kwargs.pop("timeout", 600 if Path(args[0]).stem in {"java", "dotnet"} else 1200)
    try:
        subprocess.run(args, cwd=kwargs.pop("cwd", ROOT), check=True, timeout=timeout, **kwargs)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"Qualification step timed out after {timeout}s: {args!r}") from error


def describe_sdk(sdk, model=ROOT / "tests/models/weather"):
    """The `crucible.sdks/1` document (docs/docs.md) for one SDK over `model`.

    `sdk` is the TOML of a `[sdk.<name>]` table; describe it with the options
    the caller generates its package with, so every name in its snippets is
    one that package declares.
    """
    with tempfile.TemporaryDirectory() as directory:
        directory = Path(directory)
        shutil.copytree(model, directory / "model")
        (directory / "crucible-sdk.toml").write_text(
            f'schema_version = 1\nmodel = "model"\n\n{sdk}', encoding="utf-8")
        out = directory / "sdks.json"
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "sdk", "describe",
            "--file", str(directory / "crucible-sdk.toml"), "--out", str(out))
        return json.loads(out.read_text(encoding="utf-8"))


def snippets(document):
    """Every snippet in a `crucible.sdks/1` document, as (SDK, operation name,
    variant, text): `call` for an operation's snippet, `pages` for its
    paginated one."""
    for name, sdk in sorted(document["sdks"].items()):
        for operation, entry in sorted(sdk["operations"].items()):
            short = operation.split("#", 1)[1]
            yield name, short, "call", entry["snippet"]
            if entry.get("paginatedSnippet"):
                yield name, short, "pages", entry["paginatedSnippet"]


@contextmanager
def weather():
    if not os.environ.get("WEATHER_BIN"):
        run("cargo", "build", "--locked", "-q", "--manifest-path", "e2e/Cargo.toml", "-p", "weather-service")
    target = Path(os.environ.get("CARGO_TARGET_DIR", "e2e/target"))
    if not target.is_absolute():
        target = ROOT / target
    binary = Path(os.environ["WEATHER_BIN"]) if os.environ.get("WEATHER_BIN") else target / "debug" / ("weather-service.exe" if os.name == "nt" else "weather-service")
    server = subprocess.Popen([str(binary)], cwd=ROOT, stdout=subprocess.PIPE, text=True,
                              creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    try:
        started = queue.Queue()
        def drain():
            started.put(server.stdout.readline().strip())
            for _ in server.stdout:
                pass
        threading.Thread(target=drain, daemon=True).start()
        line = started.get(timeout=30)
        if not line.startswith("listening on "):
            raise RuntimeError("weather service failed to start: " + line)
        yield "http://" + line.removeprefix("listening on ")
    finally:
        server.terminate()
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait(timeout=10)
