"""Generate, type-check, and exercise a Python SDK with this interpreter."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = ROOT / "target/weather-python"


def run(*args, **kwargs):
    subprocess.run(args, cwd=ROOT, check=True, **kwargs)


def main():
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
        "crates/crucible-smithy/tests/fixtures/weather.json", "--out", str(PACKAGE),
        "--package-name", "weather-client", "--module-name", "weather_client",
        "--endpoint", "https://weather.example", "--content-sha256")
    env = dict(os.environ, PYTHONPATH=str(PACKAGE / "src"))
    run(sys.executable, "-m", "mypy", str(PACKAGE / "src"))
    run("cargo", "build", "--locked", "-q", "--manifest-path", "e2e/Cargo.toml", "-p", "weather-service")
    target = Path(os.environ.get("CARGO_TARGET_DIR", "e2e/target"))
    if not target.is_absolute():
        target = ROOT / target
    binary = target / "debug" / ("weather-service.exe" if os.name == "nt" else "weather-service")
    server = subprocess.Popen([str(binary)], cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              text=True, creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    try:
        line = server.stdout.readline().strip()
        if not line.startswith("listening on "):
            raise RuntimeError("weather service failed to start: " + line)
        env["WEATHER_URL"] = "http://" + line.removeprefix("listening on ")
        run(sys.executable, "-m", "unittest", "discover", "-s", "crates/crucible-codegen-python/tests",
            "-p", "test_*.py", "-v", env=env)
    finally:
        server.terminate()
        server.wait(timeout=10)
    run(sys.executable, "-m", "build", "--wheel", str(PACKAGE))
    with tempfile.TemporaryDirectory() as installed:
        wheel = next((PACKAGE / "dist").glob("*.whl"))
        run(sys.executable, "-m", "pip", "install", "--no-deps", "--target", installed, str(wheel))
        run(sys.executable, "-c", "from weather_client import Client, CreateCityInput; from importlib.resources import files; assert files('weather_client').joinpath('py.typed').is_file(); assert files('weather_client').joinpath('crucible.json').is_file()",
            env=dict(os.environ, PYTHONPATH=installed))


if __name__ == "__main__":
    main()
