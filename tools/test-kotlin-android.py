"""Compile a separate Android consumer and exercise it on an attached emulator."""
import argparse
import os
from pathlib import Path
import shutil
import tempfile
from urllib.parse import urlparse
from sdk_test_support import ROOT, run, weather


def qualify(output, build_only):
    sdk = output / "sdk"
    repo = ROOT / "target/maven-repo"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "kotlin",
        str(ROOT / "tests/models/weather"), "--out", str(sdk),
        "--group-id", "example", "--artifact-id", "weather-kotlin-android", "--package-name", "example.weather",
        "--package-version", "0.0.0-test")
    run(os.environ.get("MAVEN", "mvn"), "-q", "-Dmaven.repo.local=" + str(repo), "install", cwd=sdk)
    consumer = output / "consumer"
    shutil.copytree(ROOT / "crates/crucible-codegen-kotlin/tests/android", consumer, dirs_exist_ok=True)
    gradle = [os.environ.get("GRADLE", "gradle"), "--no-daemon", "--console=plain", "-PsdkRepository=" + repo.as_uri()]
    run(*gradle, "lintDebug", "assembleDebug", "assembleDebugAndroidTest", cwd=consumer)
    if build_only:
        print("Kotlin Android consumer APK and lint passed; device tests not run")
        return
    adb = os.environ.get("ADB", "adb")
    with weather() as endpoint:
        port = urlparse(endpoint).port
        run(adb, "reverse", f"tcp:{port}", f"tcp:{port}")
        try:
            run(*gradle, "connectedDebugAndroidTest", "-Pandroid.testInstrumentationRunnerArguments.endpoint=" + endpoint,
                cwd=consumer)
        finally:
            run(adb, "reverse", "--remove", f"tcp:{port}")
    print("Kotlin Android native package and weather HTTP passed")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--build-only", action="store_true")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.output:
        args.output.resolve().mkdir(parents=True, exist_ok=True)
        qualify(args.output.resolve(), args.build_only)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-kotlin-android-") as directory:
            qualify(Path(directory), args.build_only)
