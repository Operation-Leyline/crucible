"""Qualify generated Composer packages with exact codecs and real HTTP."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
from sdk_test_support import ROOT, run, weather
from sdk_conformance_model import qualification_model
from sdk_http_fixture import fixture

PHP = os.environ.get("PHP", "php")
COMPOSER = os.environ.get("COMPOSER_PHAR")

def composer(*args, **kwargs):
    command = [PHP, COMPOSER] if COMPOSER else ["composer"]
    run(*command, "--no-interaction", "--no-plugins", "--no-scripts", *args, **kwargs)

def qualify(output):
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/loopback_framing.php"))
    package = output / "sdk"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
        str(qualification_model(output)), "--out", str(package), "--package-name", "example/weather-client",
        "--namespace", "Example\\Weather", "--content-sha256")
    for path in sorted(package.rglob("*.php")):
        run(PHP, "-l", str(path))
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/codec.php"))
    composer("validate", "--no-check-publish", cwd=package)
    composer("install", cwd=package)
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/models.php"), env=dict(os.environ, PHP_PACKAGE=str(package)))
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/serialization.php"), env=dict(os.environ, PHP_PACKAGE=str(package)))
    with weather() as endpoint, fixture() as fixture_url:
        run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/runtime.php"),
            env=dict(os.environ, PHP_PACKAGE=str(package), WEATHER_URL=endpoint, FIXTURE_URL=fixture_url, RETRY_CASES=str(ROOT / "tests/conformance/retry.json")))
        run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/proxy.php"),
            env=dict(os.environ, PHP_PACKAGE=str(package), HTTPS_PROXY=fixture_url, https_proxy=fixture_url, NO_PROXY="", no_proxy=""))
    archives = output / "archives"
    archives.mkdir(exist_ok=True)
    composer("archive", "--format=zip", "--dir=" + str(archives), cwd=package)
    consumer = output / "consumer"
    consumer.mkdir(exist_ok=True)
    (consumer / "composer.json").write_text(json.dumps({
        "name": "example/consumer", "license": "proprietary",
        "repositories": [{"type": "artifact", "url": archives.as_posix()}],
        "require": {"example/weather-client": "0.0.0"},
    }), encoding="utf-8")
    composer("install", cwd=consumer)
    (consumer / "main.php").write_text("""<?php
require __DIR__ . '/vendor/autoload.php';
$input = new Example\\Weather\\CreateCityInput(name:'consumer');
if ($input->name !== 'consumer') throw new RuntimeException('Consumer failed');
$client = new Example\\Weather\\WeatherClient(new Example\\Weather\\Runtime\\ClientConfig(endpoint:'https://example.invalid'));
$client->close();
echo "PHP archive consumer passed\\n";
""", encoding="utf-8")
    run(PHP, str(consumer / "main.php"))
    void_path = output / "void.json"
    void_path.write_text(json.dumps({"smithy": "2.0", "shapes": {
        "example.void#Service": {"type": "service", "version": "1", "traits": {"crucible.protocols#json": {}}, "operations": [{"target": "example.void#Ping"}]},
        "example.void#Ping": {"type": "operation", "traits": {"smithy.api#http": {"method": "GET", "uri": "/ping"}}},
    }}))
    renamed = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/weather.json").read_text())
    renamed["shapes"]["example.weather#Weather"]["rename"] = {
        "example.weather#DateTime": "Instant", "example.weather#CitySummary": "DateTime", "example.weather#Location": "Field", "example.weather#Sky": "NativeSky",
    }
    for member in ["this", "context", "object", "metadata", "code", "message"]:
        renamed["shapes"]["example.weather#CreateCityInput"]["members"][member] = {"target": "smithy.api#String"}
    renamed_path = output / "renamed.json"
    renamed_path.write_text(json.dumps(renamed))
    for name, model in [("empty", ROOT / "tests/conformance/empty-service.json"), ("void", void_path), ("renamed", renamed_path)]:
        generated = output / name
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php", str(model),
            "--out", str(generated), "--package-name", "example/" + name, "--namespace", "Example\\Weather")
        composer("install", cwd=generated)
        (generated / "check.php").write_text("""<?php
require __DIR__ . '/vendor/autoload.php';
foreach (new RecursiveIteratorIterator(new RecursiveDirectoryIterator(__DIR__ . '/src')) as $file) {
    if ($file->isFile() && $file->getExtension() === 'php') require_once $file->getPathname();
}
$class = Example\\Weather\\PLACEHOLDER_CLIENT::class;
$client = new $class(new Example\\Weather\\Runtime\\ClientConfig(endpoint:'http://127.0.0.1'));
$client->close();
""".replace("PLACEHOLDER_CLIENT", {"empty":"EmptyClient", "void":"ServiceClient", "renamed":"WeatherClient"}[name]), encoding="utf-8")
        run(PHP, str(generated / "check.php"))
    for name, fixture_name, namespace in [("dynamic", "dynamic-bindings", "Dynamic"), ("extended", "extended-types", "Types")]:
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
            str(ROOT / f"crates/crucible-smithy/tests/fixtures/{fixture_name}.json"), "--out", str(output / name),
            "--package-name", "example/" + name, "--namespace", namespace)
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/extensions.php"), env=dict(os.environ,
        PHP_DYNAMIC_PACKAGE=str(output / "dynamic"), PHP_EXTENDED_PACKAGE=str(output / "extended")))
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
        str(ROOT / "crates/crucible-smithy/tests/fixtures/error-payloads.json"), "--out", str(output / "wire"),
        "--package-name", "example/wire", "--namespace", "WireSdk")
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/wire.php"), env=dict(os.environ, PHP_WIRE_PACKAGE=str(output / "wire")))
    # Payload Content-Type precedence under both payload-capable JSON protocols.
    for protocol in ("crucible.protocols#json", "aws.protocols#restJson1"):
        payload_package = output / ("payload-" + protocol.split("#")[1])
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
            str(ROOT / "crates/crucible-codegen-python/tests/payload-bindings.json"), "--protocol", protocol,
            "--out", str(payload_package), "--package-name", "example/payload", "--namespace", "PayloadSdk")
        run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/payload_bindings.php"),
            env=dict(os.environ, PHP_PAYLOAD_PACKAGE=str(payload_package)))
    # String representations must hide sensitive members from every inspection path.
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
        str(ROOT / "crates/crucible-smithy/tests/fixtures/client-docs.json"), "--out", str(output / "describe"),
        "--package-name", "example/vault", "--namespace", "VaultSdk")
    for path in sorted((output / "describe").rglob("*.php")):
        run(PHP, "-l", str(path), stdout=subprocess.DEVNULL)
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/describe.php"), env=dict(os.environ, PHP_VAULT_PACKAGE=str(output / "describe")))
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
        str(ROOT / "crates/crucible-codegen-python/tests/auth.json"), "--out", str(output / "auth"),
        "--package-name", "example/auth", "--namespace", "AuthSdk")
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/auth.php"), env=dict(os.environ, PHP_AUTH_PACKAGE=str(output / "auth")))
    for name, model, namespace, test, variable, extra in [
        ("jobs", "crates/crucible-codegen-java/tests/waiters.json", "JobsSdk", "waiters.php", "PHP_JOBS_PACKAGE", {}),
        ("behavior", "crates/crucible-smithy/tests/fixtures/client-behaviors.json", "BehaviorSdk", "endpoints.php", "PHP_BEHAVIOR_PACKAGE", {}),
        ("checksum", "crates/crucible-smithy/tests/fixtures/checksums.json", "ChecksumSdk", "checksums.php", "PHP_CHECKSUM_PACKAGE",
         {"CHECKSUM_VECTORS": str(ROOT / "tests/conformance/checksums.json")}),
    ]:
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php", str(ROOT / model),
            "--out", str(output / name), "--package-name", "example/" + name, "--namespace", namespace)
        run(PHP, str(ROOT / "crates/crucible-codegen-php/tests" / test), env=dict(os.environ, **{variable: str(output / name)}, **extra))
    protocol_model = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/protocol-json.json").read_text())
    protocol_model["shapes"]["example.json#JsonService"]["rename"] = {"example.json#Problem": "ProblemFault"}
    protocol_path = output / "protocol.json"
    protocol_path.write_text(json.dumps(protocol_model), encoding="utf-8")
    for protocol in ("restJson1", "awsJson1_0", "awsJson1_1"):
        package = output / protocol
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php", str(protocol_path),
            "--protocol", "aws.protocols#" + protocol, "--out", str(package), "--package-name", "example/protocol",
            "--namespace", "ProtocolSdk")
        for path in sorted(package.rglob("*.php")):
            run(PHP, "-l", str(path), stdout=subprocess.DEVNULL)
        run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/protocols.php"),
            env=dict(os.environ, PHP_PROTOCOL_PACKAGE=str(package), PROTOCOL=protocol))
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
        str(ROOT / "crates/crucible-codegen-java/tests/streaming.json"), "--out", str(output / "stream"),
        "--package-name", "example/stream", "--namespace", "StreamSdk")
    for path in sorted((output / "stream").rglob("*.php")):
        run(PHP, "-l", str(path), stdout=subprocess.DEVNULL)
    streams = dict(os.environ, PHP_STREAM_PACKAGE=str(output / "stream"), PHP_JOBS_PACKAGE=str(output / "jobs"),
                   PHP_CHECKSUM_PACKAGE=str(output / "checksum"), CHECKSUM_VECTORS=str(ROOT / "tests/conformance/checksums.json"))
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/streaming.php"), env=streams)
    # Real cURL against a loopback server: 256 MiB each way under a 64 MiB PHP memory limit.
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/loopback.php"), env=streams)
    # Smithy RPC v2: the official protocol tests (lowered into each package's schema), then
    # published CBOR/Digest/SigV4a vectors and the cURL transport against a verifying server.
    for protocol, vectors in (("rpcv2Cbor", "rpcv2-cbor"), ("rpcv2Json", "rpcv2-json")):
        package = output / protocol
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
            str(ROOT / f"crates/crucible-smithy/tests/fixtures/{vectors}.json"), "--out", str(package),
            "--package-name", "example/rpc", "--namespace", "RpcSdk")
        for path in sorted(package.rglob("*.php")):
            run(PHP, "-l", str(path), stdout=subprocess.DEVNULL)
        run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/rpcv2.php"),
            env=dict(os.environ, PHP_RPC_PACKAGE=str(package), PROTOCOL=protocol))
    vault = output / "vault"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
        str(ROOT / "crates/crucible-smithy/tests/fixtures/rpc-auth.json"), "--out", str(vault),
        "--package-name", "example/vault", "--namespace", "VaultSdk")
    for path in sorted(vault.rglob("*.php")):
        run(PHP, "-l", str(path), stdout=subprocess.DEVNULL)
    composer("validate", "--no-check-publish", cwd=vault)
    vault_env = dict(os.environ, PHP_VAULT_PACKAGE=str(vault))
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/rpcauth.php"), env=vault_env)
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/rpc_loopback.php"), env=vault_env)
    print("PHP generated models, shared retry, native HTTP, waiters, endpoint rules, checksums, AWS JSON and Smithy RPC v2 protocols, Digest and SigV4a authentication, streaming blobs, Composer archive, empty and renamed packages passed")

if __name__ == "__main__":
    explicit = os.environ.get("PHP_TEST_OUTPUT")
    if explicit:
        directory = Path(explicit).resolve()
        directory.mkdir(parents=True, exist_ok=True)
        qualify(directory)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-php-") as directory:
            qualify(Path(directory))
