"""Generate, test over HTTP, and consume a Go SDK from a separate module."""
import os
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[1]
GO = os.environ.get("GO", "go")


def run(*args, **kwargs):
    subprocess.run(args, cwd=kwargs.pop("cwd", ROOT), check=True, **kwargs)


def qualify(output):
    package = output / "sdk"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "crates/crucible-smithy/tests/fixtures/weather.json", "--out", str(package),
        "--module-path", "example.com/weather", "--package-name", "weather",
        "--endpoint", "https://weather.example", "--content-sha256")
    shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/runtime_test.go", package / "runtime_test.go")
    shutil.copyfile(ROOT / "tests/conformance/retry.json", package / "retry.json")
    shutil.copyfile(ROOT / "tests/conformance/header-lists.json", package / "header-lists.json")
    shutil.copyfile(ROOT / "tests/conformance/malformed-errors.json", package / "malformed-errors.json")
    run(GO, "fmt", "./...", cwd=package)
    run(GO, "vet", "./...", cwd=package)
    run("cargo", "build", "--locked", "-q", "--manifest-path", "e2e/Cargo.toml", "-p", "weather-service")
    target = Path(os.environ.get("CARGO_TARGET_DIR", "e2e/target"))
    if not target.is_absolute():
        target = ROOT / target
    binary = target / "debug" / ("weather-service.exe" if os.name == "nt" else "weather-service")
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
        env = dict(os.environ, WEATHER_URL="http://" + line.removeprefix("listening on "))
        run(GO, "test", "-count=1", "-v", "./...", cwd=package, env=env)
    finally:
        server.terminate()
        server.wait(timeout=10)
    consumer = output / "consumer"
    consumer.mkdir()
    (consumer / "go.mod").write_text(
        'module example.com/consumer\n\ngo 1.24.0\nrequire example.com/weather v0.0.0\n'
        'replace example.com/weather => ../sdk\n', encoding="utf-8")
    (consumer / "main.go").write_text(
        'package main\nimport "example.com/weather"\n'
        'func main(){ _,err:=weather.NewClient(weather.Config{}); if err!=nil {panic(err)}; '
        '_ = weather.CreateCityInput{Name:weather.Some[weather.CityName]("consumer")} }\n', encoding="utf-8")
    run(GO, "run", ".", cwd=consumer)
    empty = output / "empty"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "tests/conformance/empty-service.json", "--out", str(empty),
        "--module-path", "example.com/empty", "--package-name", "empty")
    run(GO, "test", "./...", cwd=empty)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="crucible-go-") as directory:
        qualify(Path(directory))
