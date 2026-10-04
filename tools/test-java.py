"""Build a Java SDK jar, then exercise that jar with a separate consumer."""
import os
import json
from pathlib import Path
import tempfile
from sdk_test_support import ROOT, run, weather

MAVEN = os.environ.get("MAVEN", "mvn")


def qualify(output):
    package = output / "sdk"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        "crates/crucible-smithy/tests/fixtures/weather.json", "--out", str(package),
        "--group-id", "example", "--artifact-id", "weather-client", "--package-name", "example.weather",
        "--endpoint", "https://weather.example", "--content-sha256")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"),
        "package", "dependency:copy-dependencies", cwd=package)
    jar = next((package / "target").glob("weather-client-*.jar"))
    classpath = os.pathsep.join([str(jar), str(package / "target/dependency/*")])
    classes = output / "test-classes"
    run("javac", "-J-Xmx256m", "--release", "17", "-d", str(classes), "-cp", classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/ClientTest.java"))
    with weather() as endpoint:
        run("java", "-Xmx256m", "-cp", os.pathsep.join([str(classes), classpath]), "example.weather.ClientTest",
            str(ROOT / "tests/conformance/retry.json"), str(ROOT / "tests/conformance/header-lists.json"),
            env=dict(os.environ, WEATHER_URL=endpoint))
    renamed = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/weather.json").read_text())
    renamed["shapes"]["example.weather#Weather"]["rename"] = {
        "example.weather#CitySummary": "String",
        "example.weather#CitySummaries": "OptionalValue",
        "example.weather#ConflictError": "ServiceError",
        "example.weather#Location": "UnknownUnion",
    }
    renamed_path = output / "renamed.json"
    renamed_path.write_text(json.dumps(renamed), encoding="utf-8")
    for label, model in [("empty", ROOT / "tests/conformance/empty-service.json"), ("renamed", renamed_path)]:
        generated = output / label
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
            str(model), "--out", str(generated), "--group-id", "example", "--artifact-id", label,
            "--package-name", "example." + label)
        run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", cwd=generated)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="crucible-java-") as directory:
        qualify(Path(directory))
