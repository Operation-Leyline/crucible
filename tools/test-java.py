"""Build a Java SDK jar, then exercise that jar with a separate consumer."""
import os
import json
import sys
from pathlib import Path
import tempfile
from sdk_test_support import ROOT, run, weather
from xml_qualification import PACKAGES, REGRESSIONS, conformance_options, write_exclusions

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
    shapes = {
        "example.large#Large": {"type": "service", "version": "1", "traits": {"crucible.protocols#json": {}}, "operations": [{"target": "example.large#Read"}]},
        "example.large#Read": {"type": "operation", "output": {"target": "example.large#Output"}, "traits": {"smithy.api#http": {"method": "GET", "uri": "/"}}},
        "example.large#Output": {"type": "structure", "members": {}},
    }
    for index in range(100):
        target = f"example.large#Record{index}"
        shapes[target] = {"type": "structure", "members": {f"field{field}": {"target": "smithy.api#String", "traits": {"smithy.api#required": {}}} for field in range(20)}}
        shapes["example.large#Output"]["members"][f"record{index}"] = {"target": target}
    large_path = output / "large.json"
    large_path.write_text(json.dumps({"smithy": "2.0", "shapes": shapes}), encoding="utf-8")
    for label, model in [("empty", ROOT / "tests/conformance/empty-service.json"), ("renamed", renamed_path), ("large", large_path)]:
        generated = output / label
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
            str(model), "--out", str(generated), "--group-id", "example", "--artifact-id", label,
            "--package-name", "example." + label)
        run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", cwd=generated)
    dynamic = output / "dynamic"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "crates/crucible-smithy/tests/fixtures/dynamic-bindings.json"), "--out", str(dynamic),
        "--group-id", "example", "--artifact-id", "dynamic", "--package-name", "example.dynamic")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=dynamic)
    dynamic_classpath = os.pathsep.join([str(dynamic / "target/classes"), str(dynamic / "target/dependency/*")])
    run("javac", "--release", "17", "-d", str(classes), "-cp", dynamic_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/DynamicTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), dynamic_classpath]), "example.dynamic.DynamicTest")
    types = output / "types"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "crates/crucible-smithy/tests/fixtures/extended-types.json"), "--out", str(types),
        "--group-id", "example", "--artifact-id", "types", "--package-name", "example.types")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=types)
    types_classpath = os.pathsep.join([str(types / "target/classes"), str(types / "target/dependency/*")])
    run("javac", "--release", "17", "-d", str(classes), "-cp", types_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/TypesTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), types_classpath]), "example.types.TypesTest")
    payloads = output / "payloads"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "crates/crucible-smithy/tests/fixtures/error-payloads.json"), "--out", str(payloads),
        "--group-id", "example", "--artifact-id", "payloads", "--package-name", "example.wire")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=payloads)
    payload_classpath = os.pathsep.join([str(payloads / "target/classes"), str(payloads / "target/dependency/*")])
    run("javac", "--release", "17", "-d", str(classes), "-cp", payload_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/PayloadTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), payload_classpath]), "example.wire.PayloadTest")
    streaming = output / "streaming"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "crates/crucible-codegen-java/tests/streaming.json"), "--out", str(streaming),
        "--group-id", "example", "--artifact-id", "streaming", "--package-name", "example.stream")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=streaming)
    streaming_classpath = os.pathsep.join([str(streaming / "target/classes"), str(streaming / "target/dependency/*")])
    run("javac", "--release", "17", "-d", str(classes), "-cp", streaming_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/StreamingTest.java"))
    # 256 MiB each way under a 64 MiB heap: any buffered transfer fails.
    run("java", "-Xmx64m", "-cp", os.pathsep.join([str(classes), streaming_classpath]), "example.stream.StreamingTest")
    auth = output / "auth"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "crates/crucible-codegen-python/tests/auth.json"), "--out", str(auth),
        "--group-id", "example", "--artifact-id", "auth", "--package-name", "example.auth")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=auth)
    auth_classpath = os.pathsep.join([str(auth / "target/classes"), str(auth / "target/dependency/*")])
    run("javac", "--release", "17", "-d", str(classes), "-cp", auth_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/AuthTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), auth_classpath]), "example.auth.AuthTest")
    waiters = output / "waiters"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "crates/crucible-codegen-java/tests/waiters.json"), "--out", str(waiters),
        "--group-id", "example", "--artifact-id", "waiters", "--package-name", "example.wait")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=waiters)
    waiter_classpath = os.pathsep.join([str(waiters / "target/classes"), str(waiters / "target/dependency/*")])
    run("javac", "--release", "17", "-d", str(classes), "-cp", waiter_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/WaiterTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), waiter_classpath]), "example.wait.WaiterTest")
    waiter_identity(output, classes)
    endpoints = output / "endpoints"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "crates/crucible-codegen-java/tests/endpoints.json"), "--out", str(endpoints),
        "--group-id", "example", "--artifact-id", "endpoints", "--package-name", "example.endpoint")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=endpoints)
    endpoint_classpath = os.pathsep.join([str(endpoints / "target/classes"), str(endpoints / "target/dependency/*")])
    run("javac", "--release", "17", "-d", str(classes), "-cp", endpoint_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/EndpointTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), endpoint_classpath]), "example.endpoint.EndpointTest")
    checksums = output / "checksums"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "crates/crucible-smithy/tests/fixtures/checksums.json"), "--out", str(checksums),
        "--group-id", "example", "--artifact-id", "checksums", "--package-name", "example.checksums")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=checksums)
    checksum_classpath = os.pathsep.join([str(checksums / "target/classes"), str(checksums / "target/dependency/*")])
    run("javac", "--release", "17", "-d", str(classes), "-cp", checksum_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/ChecksumTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), checksum_classpath]), "example.checksums.ChecksumTest")
    for protocol in ["restJson1", "awsJson1_0", "awsJson1_1"]:
        generated = output / protocol
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
            str(ROOT / "crates/crucible-smithy/tests/fixtures/protocol-json.json"), "--protocol", "aws.protocols#" + protocol,
            "--out", str(generated), "--group-id", "example", "--artifact-id", "protocol", "--package-name", "example.json")
        run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=generated)
        protocol_classpath = os.pathsep.join([str(generated / "target/classes"), str(generated / "target/dependency/*")])
        run("javac", "--release", "17", "-d", str(classes), "-cp", protocol_classpath,
            str(ROOT / "crates/crucible-codegen-java/tests/ProtocolTest.java"))
        run("java", "-cp", os.pathsep.join([str(classes), protocol_classpath]), "example.json.ProtocolTest", protocol)
    docs = output / "docs"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "crates/crucible-smithy/tests/fixtures/client-docs.json"), "--out", str(docs),
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
    xml_protocols(output, classes)
    defaults = output / "defaults"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
        str(ROOT / "crates/crucible-codegen-java/tests/defaults.json"), "--out", str(defaults),
        "--group-id", "example", "--artifact-id", "defaults", "--package-name", "example.defaults")
    run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"),
        "package", "dependency:copy-dependencies", cwd=defaults)
    defaults_classpath = os.pathsep.join([str(defaults / "target/classes"), str(defaults / "target/dependency/*")])
    run("javac", "-J-Xmx256m", "--release", "17", "-d", str(classes), "-cp", defaults_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/DefaultsTest.java"))
    run("java", "-Xmx256m", "-cp", os.pathsep.join([str(classes), defaults_classpath]), "example.defaults.DefaultsTest")


def waiter_identity(output, classes):
    model = ROOT / "tests/conformance/waiter-errors.json"
    packages = []
    for label, service in [("query", "QueryIdentity"), ("json", "JsonIdentity")]:
        package = output / ("waiter-identity-" + label)
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java",
            str(model), "--service", "example.waiter#" + service, "--out", str(package),
            "--group-id", "example", "--artifact-id", "waiter-identity-" + label,
            "--package-name", "example.waiter" + label)
        run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"),
            "package", "dependency:copy-dependencies", cwd=package)
        packages.append(package)
    classpath = os.pathsep.join([str(package / "target/classes") for package in packages]
                               + [str(packages[0] / "target/dependency/*")])
    run("javac", "--release", "17", "-d", str(classes), "-cp", classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/WaiterIdentityTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), classpath]), "example.waiterquery.WaiterIdentityTest")


def xml_protocols(output, classes):
    """restXml, awsQuery and ec2Query: Smithy vectors through a native adapter, then real HTTP."""
    fixtures = ROOT / "crates/crucible-smithy/tests/fixtures"
    write_exclusions(output)
    adapter = (ROOT / "crates/crucible-codegen-java/tests/ProtocolAdapter.java").read_text(encoding="utf-8")
    roots = []
    for label, fixture, service, protocol in PACKAGES:
        generated = output / ("xml-" + label)
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "java", str(fixtures / fixture),
            "--service", service, "--out", str(generated), "--group-id", "example", "--artifact-id", "xml-" + label,
            "--package-name", "example." + label)
        run(MAVEN, "-q", "-Dmaven.repo.local=" + str(ROOT / "target/maven-repo"), "package", "dependency:copy-dependencies", cwd=generated)
        if (label, fixture, service, protocol) in REGRESSIONS:
            roots.append(generated)
        package_classpath = os.pathsep.join([str(generated / "target/classes"), str(generated / "target/dependency/*")])
        # The adapter reaches the package-private runtime, so it joins each generated package.
        source = output / ("adapter-" + label) / "ProtocolAdapter.java"
        source.parent.mkdir(parents=True, exist_ok=True)
        source.write_text(adapter.replace("package example.xml;", f"package example.{label};", 1), encoding="utf-8")
        adapter_classes = output / ("adapter-" + label + "-classes")
        run("javac", "--release", "17", "-d", str(adapter_classes), "-cp", package_classpath, str(source))
        vectors = output / ("vectors-" + label)
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "conformance", str(fixtures / fixture),
            "--service", service, "--target", "java", "--out", str(vectors))
        run(sys.executable, str(ROOT / "tools/protocol_conformance.py"), str(vectors / "protocol-tests.json"),
            "--protocol", protocol, "--report", str(vectors / "results.json"), *conformance_options(label), "--", "java", "-cp",
            os.pathsep.join([str(adapter_classes), package_classpath]), f"example.{label}.ProtocolAdapter")
    protocol_classpath = os.pathsep.join([str(root / "target/classes") for root in roots] + [str(roots[0] / "target/dependency/*")])
    run("javac", "--release", "17", "-d", str(classes), "-cp", protocol_classpath,
        str(ROOT / "crates/crucible-codegen-java/tests/XmlProtocolTest.java"),
        str(ROOT / "crates/crucible-codegen-java/tests/QueryProtocolTest.java"))
    run("java", "-cp", os.pathsep.join([str(classes), protocol_classpath]), "example.xml.XmlProtocolTest")
    run("java", "-cp", os.pathsep.join([str(classes), protocol_classpath]), "example.query.QueryProtocolTest")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="crucible-java-") as directory:
        qualify(Path(directory))
