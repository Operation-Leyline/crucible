"""Qualify generated Composer packages with exact codecs and real HTTP."""
import json
import os
from pathlib import Path
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
        "example.weather#CitySummary": "DateTime", "example.weather#Location": "Field", "example.weather#Sky": "NativeSky",
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
