"""Qualify generated Composer packages with exact codecs and real HTTP."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
from sdk_test_support import ROOT, run, weather
from sdk_conformance_model import qualification_model
from sdk_cru_packages import copy_package, rename_shapes, replace_once, write_package
from sdk_http_fixture import fixture

PHP = os.environ.get("PHP", "php")
COMPOSER = os.environ.get("COMPOSER_PHAR")

def void_package(output):
    """A service whose one operation has no input or output."""
    return write_package(output / "void", "example.void", **{"void.cru": (
        "service Service {\n    version \"1\"\n    protocol json\n    operations [Ping]\n}\n\n"
        "readonly operation Ping {\n    http GET \"/ping\"\n}\n")})


def add_input_members(package, *names):
    """Optional string members on CreateCity's input."""
    replace_once(package, "        clientToken?: string\n",
                 "        clientToken?: string\n" + "".join(f"        {name}?: string\n" for name in names))


def composer(*args, **kwargs):
    command = [PHP, COMPOSER] if COMPOSER else ["composer"]
    run(*command, "--no-interaction", "--no-plugins", "--no-scripts", *args, **kwargs)

def qualify(output):
    package = output / "sdk"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
        str(qualification_model(output)), "--out", str(package), "--package-name", "example/weather-client",
        "--namespace", "Example\\Weather")
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
    void_path = void_package(output)
    renamed_path = copy_package("weather", output / "renamed")
    rename_shapes(renamed_path, {"DateTime": "Instant", "CitySummary": "DateTime", "Location": "Field", "Sky": "NativeSky"})
    add_input_members(renamed_path, "this", "context", "object", "metadata", "code", "message")
    for name, model in [("empty", ROOT / "tests/models/empty-service"), ("void", void_path), ("renamed", renamed_path)]:
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
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
        str(ROOT / "tests/models/extended-types"), "--out", str(output / "extended"),
        "--package-name", "example/extended", "--namespace", "Types")
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/extensions.php"), env=dict(os.environ,
        PHP_EXTENDED_PACKAGE=str(output / "extended")))
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
        str(ROOT / "tests/models/error-payloads"), "--out", str(output / "wire"),
        "--package-name", "example/wire", "--namespace", "WireSdk")
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/wire.php"), env=dict(os.environ, PHP_WIRE_PACKAGE=str(output / "wire")))
    # Payload Content-Type precedence.
    payload_package = output / "payload"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
        str(ROOT / "tests/models/payload-bindings"),
        "--out", str(payload_package), "--package-name", "example/payload", "--namespace", "PayloadSdk")
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/payload_bindings.php"),
        env=dict(os.environ, PHP_PAYLOAD_PACKAGE=str(payload_package)))
    # String representations must hide sensitive members from every inspection path.
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
        str(ROOT / "tests/models/client-docs-vault"), "--out", str(output / "describe"),
        "--package-name", "example/vault", "--namespace", "VaultSdk")
    for path in sorted((output / "describe").rglob("*.php")):
        run(PHP, "-l", str(path), stdout=subprocess.DEVNULL)
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/describe.php"), env=dict(os.environ, PHP_VAULT_PACKAGE=str(output / "describe")))
    # Typed permissions: the open permission classes and the registry, from the shared .cru fixture,
    # through Composer, which loads the registry's namespace constant.
    permissions = output / "permissions"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "php",
        str(ROOT / "tests/fixtures/permissions"), "--out", str(permissions),
        "--package-name", "example/leyline", "--namespace", "Leyline")
    for path in sorted(permissions.rglob("*.php")):
        run(PHP, "-l", str(path), stdout=subprocess.DEVNULL)
    composer("validate", "--no-check-publish", cwd=permissions)
    composer("install", cwd=permissions)
    run(PHP, str(ROOT / "crates/crucible-codegen-php/tests/permissions.php"),
        env=dict(os.environ, PHP_PERMISSIONS_PACKAGE=str(permissions)))

    print("PHP generated models, shared retry, native HTTP, Composer archive, empty and renamed packages passed")

if __name__ == "__main__":
    explicit = os.environ.get("PHP_TEST_OUTPUT")
    if explicit:
        directory = Path(explicit).resolve()
        directory.mkdir(parents=True, exist_ok=True)
        qualify(directory)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-php-") as directory:
            qualify(Path(directory))
