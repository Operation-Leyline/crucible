"""Shared native SDK qualification helpers; all services stay on loopback."""
from contextlib import contextmanager
import os
from pathlib import Path
import queue
import subprocess
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
