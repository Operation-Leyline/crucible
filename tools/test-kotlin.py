"""Install a Kotlin SDK jar, then compile and exercise a separate coroutine consumer."""
import os
from pathlib import Path
import shutil
import tempfile
import xml.etree.ElementTree as ET
from sdk_test_support import ROOT, describe_sdk, run, snippets, weather
from sdk_conformance_model import qualification_model
from sdk_cru_packages import add_operations, copy_package, rename_shapes, replace_once, write_package

MAVEN = os.environ.get("MAVEN", "mvn")
REPO = "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo")


BUILDER_DEFAULTS = """structure BuilderDefaults {
    builder: string = "builder-value"
    block: string = "block-value"
    flag: boolean = true
    small: byte = -7
    medium: short = 300
    count: integer = 42
    large: long = -9223372036854775808
    ratio: float = 1.5
    score: double = 2.5
    label: string = "default $value\\\\path"
    sky: Sky = CLEAR
    tags: Tags = []
    forecast: Forecast = {  }
    document: document = {"count": 9223372036854775807}
    blob: blob = "AQI="
    requiredValue: string
    sensitive secretToken?: string
}

readonly operation ReadBuilderDefaults {
    http GET "/builder-defaults"
    output {
        defaults: BuilderDefaults
    }
}

// The shared payload-bindings upload, named apart from the weather shapes,
// for the Content-Type precedence test.
idempotent operation TypedUpload {
    http PUT "/typed-upload" -> 200
    input {
        @payload
        data?: blob
        @header("CoNtEnT-TyPe")
        contentType?: string
    }
}
"""


def generate(model, output, package="example.weather"):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "kotlin",
        str(model), "--out", str(output), "--group-id", "example", "--artifact-id", "weather-kotlin",
        "--package-name", package, "--package-version", "0.0.0-test")


def large_package(output):
    """100 records of 20 required strings, each an optional member of one output."""
    members = "".join(f"        record{index}?: Record{index}\n" for index in range(100))
    records = "".join(
        f"structure Record{index} {{\n" + "".join(f"    field{field}: string\n" for field in range(20)) + "}\n\n"
        for index in range(100))
    return write_package(output / "large", "example.large", **{"large.cru": (
        "service Large {\n    version \"1\"\n    protocol json\n    operations [Read]\n}\n\n"
        "readonly operation Read {\n    http GET \"/\"\n    output {\n" + members + "    }\n}\n\n" + records)})


def error_documentation(output):
    package = output / "error-docs-sdk"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "kotlin",
        str(ROOT / "tests/models/error-docs"), "--out", str(package),
        "--group-id", "example", "--artifact-id", "error-docs", "--package-name", "example.error_docs",
        "--package-version", "0.0.0-test")
    run(MAVEN, "-q", REPO, "install", cwd=package)
    consumer = output / "error-docs-consumer"
    source = consumer / "src/main/kotlin/example/error_docs"
    source.mkdir(parents=True)
    shutil.copyfile(ROOT / "crates/crucible-codegen-kotlin/tests/ErrorDocumentation.kt", source / "ErrorDocumentation.kt")
    ns = "{http://maven.apache.org/POM/4.0.0}"
    ET.register_namespace("", ns[1:-1])
    pom = ET.parse(package / "pom.xml")
    pom.getroot().find(ns + "artifactId").text = "error-docs-consumer"
    configuration = pom.getroot().find(".//" + ns + "configuration")
    configuration.find(ns + "jvmTarget").text = "17"
    args = configuration.find(ns + "args")
    args.remove(args.find(ns + "arg"))
    pom.getroot().find(ns + "build").find(ns + "sourceDirectory").text = "src/main/kotlin"
    dependency = ET.SubElement(pom.getroot().find(ns + "dependencies"), ns + "dependency")
    for name, value in [("groupId", "example"), ("artifactId", "error-docs"), ("version", "0.0.0-test")]:
        ET.SubElement(dependency, ns + name).text = value
    pom.write(consumer / "pom.xml", encoding="utf-8", xml_declaration=True)
    run(MAVEN, "-q", REPO, "package", "dependency:copy-dependencies", cwd=consumer)
    jar = next((consumer / "target").glob("error-docs-consumer-*.jar"))
    classpath = os.pathsep.join([str(jar), str(consumer / "target/dependency/*")])
    run("java", "-cp", classpath, "example.error_docs.ErrorDocumentationKt")


def qualify(output):
    error_documentation(output)
    package = output / "sdk"
    model_path = qualification_model(output)
    (model_path / "builder-defaults.cru").write_text(BUILDER_DEFAULTS, encoding="utf-8", newline="\n")
    add_operations(model_path, "ReadBuilderDefaults", "TypedUpload")
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
    check_snippets(output, package, model_path)
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
    renamed_path = copy_package("weather", output / "renamed")
    rename_shapes(renamed_path, {"CitySummary": "String", "CitySummaries": "OptionalValue",
                                 "ConflictError": "ServiceError", "Location": "Request"})
    replace_once(renamed_path, "        clientToken?: string\n", "        clientToken?: string\n        class?: string\n")
    for name, model in [("empty", ROOT / "tests/models/empty-service"), ("renamed", renamed_path)]:
        generated = output / name
        generate(model, generated, "example." + name)
        run(MAVEN, "-q", REPO, "package", cwd=generated)
    permissions(output)
    # A single giant decoder compiles for weather but exceeds the JVM method-size
    # limit for real services. Keep a public, synthetic full-size package in CI.
    large = large_package(output)
    generated = output / "large"
    generate(large, generated, "example.large")
    run(MAVEN, "-q", REPO, "package", cwd=generated)


def check_snippets(output, package, model_path):
    """The crucible.sdks/1 snippets (docs/docs.md, "Snippets") compile against
    the packages they call: weather as installed above (from `model_path`), and
    samples, generated and installed with the options it is described with.
    Every snippet declares `main`, so each is compiled in a package of its own,
    which a pasted snippet does not need."""
    samples = output / "samples"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "kotlin",
        "tests/models/samples", "--out", str(samples), "--group-id", "example",
        "--artifact-id", "samples-kotlin", "--package-name", "example.samples",
        "--package-version", "0.0.0-test")
    run(MAVEN, "-q", REPO, "install", cwd=samples)
    project = output / "snippets"
    count = 0
    for model, path, table in (
        ("weather", model_path,
         '[sdk.kotlin]\nout = "generated/kotlin"\npackage = "example.weather"\ngroup_id = "example"\n'
         'artifact_id = "weather-kotlin"\n'),
        ("samples", ROOT / "tests/models/samples",
         '[sdk.kotlin]\nout = "generated/kotlin"\npackage = "example.samples"\ngroup_id = "example"\n'
         'artifact_id = "samples-kotlin"\n'),
    ):
        for _, operation, variant, text in snippets(describe_sdk(table, path)):
            name = f"{operation}{'Pages' if variant == 'pages' else ''}".lower()
            source = project / "src/main/kotlin/snippets" / model / f"{name}.kt"
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_text(f"package snippets.{model}.{name}\n\n{text}", encoding="utf-8")
            count += 1
    if count == 0:
        raise RuntimeError("no Kotlin snippets described")
    ns = "{http://maven.apache.org/POM/4.0.0}"
    pom = ET.parse(package / "pom.xml")
    pom.getroot().find(ns + "artifactId").text = "kotlin-snippets"
    configuration = pom.getroot().find(".//" + ns + "configuration")
    configuration.find(ns + "jvmTarget").text = "17"
    configuration.find(ns + "args").remove(configuration.find(ns + "args").find(ns + "arg"))
    pom.getroot().find(ns + "build").find(ns + "sourceDirectory").text = "src/main/kotlin"
    for artifact in ("weather-kotlin", "samples-kotlin"):
        dependency = ET.SubElement(pom.getroot().find(ns + "dependencies"), ns + "dependency")
        for name, value in [("groupId", "example"), ("artifactId", artifact), ("version", "0.0.0-test")]:
            ET.SubElement(dependency, ns + name).text = value
    pom.write(project / "pom.xml", encoding="utf-8", xml_declaration=True)
    run(MAVEN, "-q", REPO, "compile", cwd=project)


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
