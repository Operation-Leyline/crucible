"""Install a Kotlin SDK jar, then compile and exercise a separate coroutine consumer."""
import json
import os
from pathlib import Path
import shutil
import tempfile
import xml.etree.ElementTree as ET
from sdk_test_support import ROOT, run, weather
from sdk_conformance_model import qualification_model

MAVEN = os.environ.get("MAVEN", "mvn")
REPO = "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo")


def generate(model, output, package="example.weather"):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "kotlin",
        str(model), "--out", str(output), "--group-id", "example", "--artifact-id", "weather-kotlin",
        "--package-name", package, "--package-version", "0.0.0-test", "--content-sha256")


def qualify(output):
    package = output / "sdk"
    model_path = qualification_model(output)
    model = json.loads(model_path.read_text())
    defaults = {
        "builder": ("smithy.api#String", "builder-value"),
        "block": ("smithy.api#String", "block-value"),
        "flag": ("smithy.api#Boolean", True),
        "small": ("smithy.api#Byte", -7),
        "medium": ("smithy.api#Short", 300),
        "count": ("smithy.api#Integer", 42),
        "decimalCount": ("smithy.api#Integer", 17.0),
        "large": ("smithy.api#Long", -9223372036854775808),
        "ratio": ("smithy.api#Float", 1.5),
        "score": ("smithy.api#Double", 2.5),
        "label": ("smithy.api#String", 'default $value\\path'),
        "sky": ("example.weather#Sky", "sunny"),
        "tags": ("example.weather#Tags", []),
        "forecast": ("example.weather#Forecast", {}),
        "document": ("smithy.api#Document", {"count": 9223372036854775807}),
        "blob": ("smithy.api#Blob", "AQI="),
        "date": ("smithy.api#Timestamp", -0.25),
        "epoch": ("smithy.api#Timestamp", -1.5),
    }
    members = {name: {"target": target, "traits": {"smithy.api#default": value}}
               for name, (target, value) in defaults.items()}
    members["requiredValue"] = {"target": "smithy.api#String", "traits": {"smithy.api#required": {}}}
    members["secretToken"] = {"target": "smithy.api#String", "traits": {"smithy.api#sensitive": {}}}
    members["epoch"]["traits"]["smithy.api#timestampFormat"] = "epoch-seconds"
    members["optionalDefault"] = {"target": "smithy.api#Integer", "traits": {"smithy.api#default": 19, "smithy.api#clientOptional": {}}}
    model["shapes"]["example.weather#BuilderDefaults"] = {"type": "structure", "members": members}
    model["shapes"]["example.weather#ReadBuilderDefaults"] = {
        "type": "operation", "output": {"target": "example.weather#BuilderDefaults"},
        "traits": {"smithy.api#http": {"method": "GET", "uri": "/builder-defaults"}, "smithy.api#readonly": {}}}
    model["shapes"]["example.weather#Weather"]["operations"].append({"target": "example.weather#ReadBuilderDefaults"})
    streaming = json.loads((ROOT / "crates/crucible-codegen-kotlin/tests/fixtures/streaming.json").read_text())
    model["shapes"]["example.weather#Weather"]["operations"].extend(streaming["shapes"].pop("example.stream#Streaming")["operations"])
    model["shapes"].update(streaming["shapes"])
    # The shared payload-bindings uploads, renamed apart from the streaming
    # fixture's shapes, for the Content-Type precedence test.
    bindings = json.loads((ROOT / "crates/crucible-codegen-python/tests/payload-bindings.json").read_text()
                          .replace("example.payload#", "example.payload#Typed"))["shapes"]
    for operation in ("Upload", "UploadStream"):
        model["shapes"]["example.weather#Weather"]["operations"].append({"target": "example.payload#Typed" + operation})
        bindings["example.payload#Typed" + operation]["traits"]["smithy.api#http"]["uri"] = "/typed-" + operation.lower()
    model["shapes"].update({key: value for key, value in bindings.items() if key.split("#Typed")[1] in (
        "Upload", "UploadInput", "UploadStream", "StreamInput", "Stream")})
    model_path.write_text(json.dumps(model), encoding="utf-8")
    generate(model_path, package)
    run(MAVEN, "-q", REPO, "install", "dependency:copy-dependencies", cwd=package)
    consumer = output / "consumer"
    source = consumer / "src/main/kotlin/example/weather"
    source.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "crates/crucible-codegen-kotlin/tests/Main.kt", source / "Main.kt")
    ns = "{http://maven.apache.org/POM/4.0.0}"
    ET.register_namespace("", ns[1:-1])
    pom = ET.parse(package / "pom.xml")
    pom.getroot().find(ns + "artifactId").text = "kotlin-consumer"
    # The test HTTP server uses JDK APIs; the distributable itself targets Java 8 APIs.
    configuration = pom.getroot().find(".//" + ns + "configuration")
    configuration.find(ns + "jvmTarget").text = "17"
    configuration.find(ns + "args").remove(configuration.find(ns + "args").find(ns + "arg"))
    pom.getroot().find(ns + "build").find(ns + "sourceDirectory").text = "src/main/kotlin"
    dependency = ET.SubElement(pom.getroot().find(ns + "dependencies"), ns + "dependency")
    for name, value in [("groupId", "example"), ("artifactId", "weather-kotlin"), ("version", "0.0.0-test")]:
        ET.SubElement(dependency, ns + name).text = value
    pom.write(consumer / "pom.xml", encoding="utf-8", xml_declaration=True)
    run(MAVEN, "-q", REPO, "package", "dependency:copy-dependencies", cwd=consumer)
    jar = next((consumer / "target").glob("kotlin-consumer-*.jar"))
    classpath = os.pathsep.join([str(jar), str(consumer / "target/dependency/*")])
    java_source = consumer / "JavaEnumConsumer.java"
    java_source.write_text("""import example.weather.model.Sky;
import example.weather.model.ValidationReason;
public final class JavaEnumConsumer {
    public static void main(String[] args) {
        if (Sky.CLEAR.getValue().isEmpty() || ValidationReason.OTHER.getValue().isEmpty())
            throw new AssertionError("Generated enum static fields");
    }
}
""", encoding="utf-8")
    java_classes = consumer / "java-classes"
    java_classes.mkdir()
    run("javac", "--release", "17", "-Xlint:all", "-Werror", "-cp", classpath,
        "-d", str(java_classes), str(java_source))
    run("java", "-cp", os.pathsep.join([str(java_classes), classpath]), "JavaEnumConsumer")
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
    permissions(output)
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


def permissions(output):
    """The typed permissions package on the JVM: Java reads the constants and the registry as static fields.
    tools/test-kotlin-multiplatform.py runs the full consumer test on every target."""
    generated = output / "permissions"
    generate(ROOT / "tests/fixtures/permissions", generated, "example.leyline")
    run(MAVEN, "-q", REPO, "package", "dependency:copy-dependencies", cwd=generated)
    classpath = os.pathsep.join([str(generated / "target/classes"), str(generated / "target/dependency/*")])
    source = output / "permissions-java" / "JavaPermissionsConsumer.java"
    source.parent.mkdir()
    source.write_text("""import example.leyline.permissions.GraphPermission;
import example.leyline.permissions.Permissions;
public final class JavaPermissionsConsumer {
    public static void main(String[] args) {
        if (!GraphPermission.READ.getValue().equals("graph:read")
                || !new GraphPermission("graph:view").canonical().equals(GraphPermission.READ)
                || !new GraphPermission("graph:export").canonical().getValue().equals("graph:export")
                || !Permissions.PERMISSIONS.get(2).getResourceType().equals("org")
                || Permissions.PERMISSIONS.get(2).getPermissions().get(0).getGrantable())
            throw new AssertionError("Generated permission static fields");
    }
}
""", encoding="utf-8")
    classes = output / "permissions-java-classes"
    classes.mkdir()
    run("javac", "--release", "17", "-Xlint:all", "-Werror", "-cp", classpath, "-d", str(classes), str(source))
    run("java", "-cp", os.pathsep.join([str(classes), classpath]), "JavaPermissionsConsumer")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="crucible-kotlin-") as directory:
        qualify(Path(directory))
