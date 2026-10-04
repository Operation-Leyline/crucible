"""Install a Kotlin SDK jar, then compile and exercise a separate coroutine consumer."""
import json
import os
from pathlib import Path
import shutil
import tempfile
import xml.etree.ElementTree as ET
from sdk_test_support import ROOT, run, weather

MAVEN = os.environ.get("MAVEN", "mvn")
REPO = "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo")


def generate(model, output, package="example.weather"):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "kotlin",
        str(model), "--out", str(output), "--group-id", "example", "--artifact-id", "weather-kotlin",
        "--package-name", package, "--package-version", "0.0.0-test", "--content-sha256")


def qualification_model(output):
    model = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/weather.json").read_text())
    shapes = model["shapes"]
    service = shapes["example.weather#Weather"]
    global_errors = service.pop("errors")
    for shape in shapes.values():
        if shape["type"] == "operation":
            shape.setdefault("errors", []).extend(global_errors)
    for name in ["Ordinary", "Transient", "Throttled"]:
        traits = {"smithy.api#error": "client", "smithy.api#httpError": 429 if name == "Throttled" else 400}
        if name != "Ordinary":
            traits["smithy.api#retryable"] = {"throttling": name == "Throttled"}
        shapes["example.weather#" + name] = {"type": "structure", "traits": traits,
            "members": {"message": {"target": "smithy.api#String"}}}
    for name in ["RetrySafe", "RetryUnsafe"]:
        traits = {"smithy.api#http": {"method": "POST", "uri": "/retry"}}
        if name == "RetrySafe":
            traits["smithy.api#idempotent"] = {}
        shapes["example.weather#" + name] = {"type": "operation", "traits": traits,
            "input": {"target": "smithy.api#Unit"}, "output": {"target": "smithy.api#Unit"},
            "errors": [{"target": "example.weather#" + error} for error in ["Ordinary", "Transient", "Throttled"]]}
        service["operations"].append({"target": "example.weather#" + name})
    path = output / "model.json"
    path.write_text(json.dumps(model), encoding="utf-8")
    return path


def qualify(output):
    package = output / "sdk"
    generate(qualification_model(output), package)
    run(MAVEN, "-q", REPO, "install", "dependency:copy-dependencies", cwd=package)
    consumer = output / "consumer"
    source = consumer / "src/main/kotlin/example/weather"
    source.mkdir(parents=True)
    shutil.copyfile(ROOT / "crates/crucible-codegen-kotlin/tests/Main.kt", source / "Main.kt")
    ns = "{http://maven.apache.org/POM/4.0.0}"
    ET.register_namespace("", ns[1:-1])
    pom = ET.parse(package / "pom.xml")
    pom.getroot().find(ns + "artifactId").text = "kotlin-consumer"
    # The test HTTP server uses JDK APIs; the distributable itself targets Java 8 APIs.
    configuration = pom.getroot().find(".//" + ns + "configuration")
    configuration.find(ns + "jvmTarget").text = "17"
    configuration.remove(configuration.find(ns + "args"))
    dependency = ET.SubElement(pom.getroot().find(ns + "dependencies"), ns + "dependency")
    for name, value in [("groupId", "example"), ("artifactId", "weather-kotlin"), ("version", "0.0.0-test")]:
        ET.SubElement(dependency, ns + name).text = value
    pom.write(consumer / "pom.xml", encoding="utf-8", xml_declaration=True)
    run(MAVEN, "-q", REPO, "package", "dependency:copy-dependencies", cwd=consumer)
    jar = next((consumer / "target").glob("kotlin-consumer-*.jar"))
    classpath = os.pathsep.join([str(jar), str(consumer / "target/dependency/*")])
    with weather() as endpoint:
        run("java", "-Xmx256m", "-cp", classpath, "example.weather.MainKt",
            str(ROOT / "tests/conformance/retry.json"), str(ROOT / "tests/conformance/header-lists.json"),
            env=dict(os.environ, WEATHER_URL=endpoint))
    renamed = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/weather.json").read_text())
    renamed["shapes"]["example.weather#Weather"]["rename"] = {
        "example.weather#CitySummary": "String", "example.weather#CitySummaries": "OptionalValue",
        "example.weather#ConflictError": "ServiceError", "example.weather#Location": "Request",
    }
    renamed["shapes"]["example.weather#CreateCityInput"]["members"]["class"] = {"target": "smithy.api#String"}
    renamed_path = output / "renamed.json"
    renamed_path.write_text(json.dumps(renamed), encoding="utf-8")
    for name, model in [("empty", ROOT / "tests/conformance/empty-service.json"), ("renamed", renamed_path)]:
        generated = output / name
        generate(model, generated, "example." + name)
        run(MAVEN, "-q", REPO, "package", cwd=generated)
    # A single giant decoder compiles for weather but exceeds the JVM method-size
    # limit for real services. Keep a public, synthetic full-size package in CI.
    shapes = {
        "example.large#Large": {"type": "service", "version": "1", "traits": {"crucible.protocols#json": {}},
            "operations": [{"target": "example.large#Read"}]},
        "example.large#Read": {"type": "operation", "output": {"target": "example.large#Output"},
            "traits": {"smithy.api#readonly": {}, "smithy.api#http": {"method": "GET", "uri": "/"}}},
        "example.large#Output": {"type": "structure", "members": {}},
    }
    for index in range(100):
        target = f"example.large#Record{index}"
        shapes[target] = {"type": "structure", "members": {
            f"field{field}": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}}}
            for field in range(20)
        }}
        shapes["example.large#Output"]["members"][f"record{index}"] = {"target": target}
    large = output / "large.json"
    large.write_text(json.dumps({"smithy": "2.0", "shapes": shapes}), encoding="utf-8")
    generated = output / "large"
    generate(large, generated, "example.large")
    run(MAVEN, "-q", REPO, "package", cwd=generated)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="crucible-kotlin-") as directory:
        qualify(Path(directory))
