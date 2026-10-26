"""Build a Java SDK jar, then exercise that jar with a separate consumer."""
import os
import sys
from pathlib import Path
import tempfile
from sdk_cru_packages import copy_package, rename_shapes, write_package
from sdk_test_support import ROOT, describe_sdk, run, snippets, weather

MAVEN = os.environ.get("MAVEN", "mvn")


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
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "tests/models/error-docs"), "--out", str(package),
        "--group-id", "example", "--artifact-id", "error-docs", "--package-name", "example.error_docs")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"),
        "package", "dependency:copy-dependencies", cwd=package)
    jar = next((package / "target").glob("error-docs-*.jar"))
    classpath = os.pathsep.join([str(jar), str(package / "target/dependency/*")])
    classes = output / "error-docs-consumer"
    run("javac", "--release", "17", "-d", str(classes), "-cp", classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/ErrorDocumentationTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), classpath]), "example.error_docs.ErrorDocumentationTest")


def qualify(output):
    error_documentation(output)
    package = output / "sdk"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        "tests/models/weather", "--out", str(package),
        "--group-id", "example", "--artifact-id", "weather-client", "--package-name", "example.weather",
        "--endpoint", "https://weather.example")
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
    check_snippets(output, classpath)
    renamed_path = copy_package("weather", output / "renamed")
    rename_shapes(renamed_path, {"CitySummary": "String", "CitySummaries": "OptionalValue",
                                 "ConflictError": "ServiceError", "Location": "UnknownUnion"})
    large_path = large_package(output)
    for label, model in [("empty", ROOT / "tests/models/empty-service"), ("renamed", renamed_path), ("large", large_path)]:
        generated = output / label
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
            str(model), "--out", str(generated), "--group-id", "example", "--artifact-id", label,
            "--package-name", "example." + label)
        run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", cwd=generated)
    types = output / "types"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "tests/models/extended-types"), "--out", str(types),
        "--group-id", "example", "--artifact-id", "types", "--package-name", "example.types")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=types)
    types_classpath = os.pathsep.join([str(types / "target/classes"), str(types / "target/dependency/*")])
    run("javac", "--release", "17", "-d", str(classes), "-cp", types_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/TypesTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), types_classpath]), "example.types.TypesTest")
    payloads = output / "payloads"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "tests/models/error-payloads"), "--out", str(payloads),
        "--group-id", "example", "--artifact-id", "payloads", "--package-name", "example.wire")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=payloads)
    payload_classpath = os.pathsep.join([str(payloads / "target/classes"), str(payloads / "target/dependency/*")])
    run("javac", "--release", "17", "-d", str(classes), "-cp", payload_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/PayloadTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), payload_classpath]), "example.wire.PayloadTest")
    # Content-Type precedence on payload uploads.
    for protocol in ("crucible.protocols#json",):
        bindings = output / ("payload-bindings-" + protocol.split("#")[1])
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
            str(ROOT / "tests/models/payload-bindings"), "--out", str(bindings),
            "--group-id", "example", "--artifact-id", "payload-bindings",
            "--package-name", "example.payload")
        run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=bindings)
        bindings_classpath = os.pathsep.join([str(bindings / "target/classes"), str(bindings / "target/dependency/*")])
        bindings_classes = bindings / "test-classes"
        run("javac", "--release", "17", "-d", str(bindings_classes), "-cp", bindings_classpath,
            str(ROOT / "crates/crucible-codegen-java/tests/PayloadContentTypeTest.java"))
        run("java", "-cp", os.pathsep.join([str(bindings_classes), bindings_classpath]), "example.payload.PayloadContentTypeTest")
    docs = output / "docs"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "tests/models/client-docs-jvm"), "--out", str(docs),
        "--group-id", "example", "--artifact-id", "docs", "--package-name", "example.docs")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=docs)
    docs_classpath = os.pathsep.join([str(docs / "target/classes"), str(docs / "target/dependency/*")])
    # Generated and bundled Javadoc must be warning-free under full doclint.
    for sources, label in [(docs, "docs"), (package, "weather")]:
        run("javadoc", "-Xdoclint:all", "-Werror", "-quiet", "-d", str(output / (label + "-javadoc")), "-cp", docs_classpath if label == "docs" else classpath,
            *map(str, sorted((sources / "src/main/java").rglob("*.java"))))
    run("javac", "--release", "17", "-d", str(classes), "-cp", docs_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/DisplayTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), docs_classpath]), "example.docs.DisplayTest")
    permissions(output, classes)
    defaults = output / "defaults"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "tests/models/defaults"), "--out", str(defaults),
        "--group-id", "example", "--artifact-id", "defaults", "--package-name", "example.defaults")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"),
        "package", "dependency:copy-dependencies", cwd=defaults)
    defaults_classpath = os.pathsep.join([str(defaults / "target/classes"), str(defaults / "target/dependency/*")])
    run("javac", "-J-Xmx256m", "--release", "17", "-d", str(classes), "-cp", defaults_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/DefaultsTest.java"))
    run("java", "-Xmx256m", "-cp", os.pathsep.join([str(classes), defaults_classpath]), "example.defaults.DefaultsTest")


def check_snippets(output, weather_classpath):
    """The crucible.sdks/1 snippets (docs/docs.md, "Snippets") compile against
    the packages they call: weather's jar above, and samples generated with
    the options it is described with. Each snippet is its own class."""
    samples = output / "samples"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        "tests/models/samples", "--out", str(samples),
        "--group-id", "example", "--artifact-id", "samples", "--package-name", "example.samples")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"),
        "package", "dependency:copy-dependencies", cwd=samples)
    samples_classpath = os.pathsep.join([str(samples / "target/classes"), str(samples / "target/dependency/*")])
    for model, table, classpath in (
        ("weather", '[sdk.java]\nout = "generated/java"\npackage = "example.weather"\ngroup_id = "example"\n'
         'artifact_id = "weather-client"\nendpoint = "https://weather.example"\n', weather_classpath),
        ("samples", '[sdk.java]\nout = "generated/java"\npackage = "example.samples"\ngroup_id = "example"\n'
         'artifact_id = "samples"\n', samples_classpath),
    ):
        directory = output / "snippets" / model
        directory.mkdir(parents=True)
        files = []
        for _, operation, variant, text in snippets(describe_sdk(table, ROOT / "tests/models" / model)):
            path = directory / f"{operation}{'Pages' if variant == 'pages' else ''}Example.java"
            path.write_text(text, encoding="utf-8")
            files.append(str(path))
        if not files:
            raise RuntimeError(f"no Java snippets described for {model}")
        run("javac", "-J-Xmx256m", "--release", "17", "-d", str(directory / "classes"), "-cp", classpath, *files)


def permissions(output, classes):
    """The typed permissions fixture: open permission types and the registry, read by a consumer."""
    package = output / "permissions"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "tests/fixtures/permissions"), "--out", str(package),
        "--group-id", "example", "--artifact-id", "leyline", "--package-name", "example.leyline")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=package)
    classpath = os.pathsep.join([str(package / "target/classes"), str(package / "target/dependency/*")])
    # Hostile permission docs must still be warning-free Javadoc.
    run("javadoc", "-Xdoclint:all", "-Werror", "-quiet", "-d", str(output / "permissions-javadoc"), "-cp", classpath,
        *map(str, sorted((package / "src/main/java/example/leyline/permissions").glob("*.java"))))
    run("javac", "--release", "17", "-Xlint:all", "-Werror", "-d", str(classes), "-cp", classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/PermissionsTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), classpath]), "example.leyline.PermissionsTest")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="crucible-java-") as directory:
        qualify(Path(directory))
