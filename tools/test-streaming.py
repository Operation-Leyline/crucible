#!/usr/bin/env python3
"""Run the compiled TypeScript streaming consumer against the Rust HTTP host.

tools/test-ts.sh generates and compiles the SDK first. Keep temporary output
outside the checkout and always stop the child server, including on failure.
"""
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
    env = {**os.environ, "CARGO_TARGET_DIR": str(target)}
    subprocess.run(["cargo", "build", "--locked", "--manifest-path",
                    str(ROOT / "tests/consumers/streaming/Cargo.toml")], cwd=ROOT, env=env, check=True)
    binary = target / "debug" / ("streaming-consumer.exe" if os.name == "nt" else "streaming-consumer")
    with tempfile.TemporaryDirectory(prefix="crucible-streaming-") as directory:
        log = Path(directory) / "server.log"
        with log.open("w", encoding="utf-8") as output:
            process = subprocess.Popen([str(binary)], cwd=ROOT,
                                       env={**env, "CRUCIBLE_ADDR": "127.0.0.1:0"},
                                       stdout=output, stderr=subprocess.STDOUT)
            try:
                deadline = time.monotonic() + 30
                address = None
                while time.monotonic() < deadline:
                    for line in log.read_text(encoding="utf-8").splitlines():
                        if line.startswith("Listening on "):
                            address = line.removeprefix("Listening on ")
                    if address or process.poll() is not None:
                        break
                    time.sleep(0.05)
                if not address:
                    raise RuntimeError(f"streaming server did not start:\n{log.read_text(encoding='utf-8')}")
                subprocess.run(["node", "--test", "crates/crucible-codegen-ts/tests/ts/dist/streaming-live.test.js"],
                               cwd=ROOT, env={**env, "CRUCIBLE_STREAMING_URL": address}, check=True, timeout=60)
            finally:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)
    print("streaming: TypeScript client and Rust server passed over HTTP", flush=True)


if __name__ == "__main__":
    main()
