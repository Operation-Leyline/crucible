"""Qualify native Ruby gems against shared fixtures and real HTTP."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
from sdk_test_support import ROOT, run, weather
from sdk_conformance_model import qualification_model
from sdk_http_fixture import fixture

RUBY = os.environ.get("RUBY", "ruby")


def gem(*args, **kwargs):
    run(RUBY, "-rrubygems/gem_runner", "-e", "Gem::GemRunner.new.run(ARGV)", "--", *args, **kwargs)


def qualify(output):
    package = output / "sdk"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "ruby",
        str(qualification_model(output)), "--out", str(package), "--package-name", "weather_client",
        "--module-name", "Weather", "--content-sha256")
    for source in sorted(package.rglob("*.rb")):
        run(RUBY, "-c", str(source))
    run(RUBY, "-I", str(package / "lib"), str(ROOT / "crates/crucible-codegen-ruby/tests/serialization.rb"))
    with weather() as endpoint, fixture() as fixture_url:
        run(RUBY, "-I", str(package / "lib"), str(ROOT / "crates/crucible-codegen-ruby/tests/runtime.rb"),
            env=dict(os.environ, WEATHER_URL=endpoint, FIXTURE_URL=fixture_url,
                     RETRY_CASES=str(ROOT / "tests/conformance/retry.json"),
                     HEADER_CASES=str(ROOT / "tests/conformance/header-lists.json")))
        run(RUBY, "-I", str(package / "lib"), str(ROOT / "crates/crucible-codegen-ruby/tests/proxy.rb"),
            env=dict(os.environ, http_proxy=fixture_url, https_proxy=fixture_url, HTTP_PROXY=fixture_url,
                     HTTPS_PROXY=fixture_url, NO_PROXY="", no_proxy=""))
    gem("build", "weather_client.gemspec", cwd=package)
    consumer = output / "consumer"
    default_gems = subprocess.check_output([RUBY, "-r", "rubygems", "-e", "puts Gem.default_dir"], text=True).strip()
    consumer_env = dict(os.environ, GEM_HOME=str(consumer), GEM_PATH=os.pathsep.join([str(consumer), default_gems]))
    gem("install", "--local", "--no-document",
        str(package / "weather_client-0.0.0.gem"), env=consumer_env)
    run(RUBY, "-r", "weather_client", "-e", "raise unless Weather::CreateCityInput.new(name: 'consumer').name == 'consumer'; Weather::WeatherClient.new(Weather::Runtime::ClientConfig.new(endpoint: 'https://example.invalid')).close",
        env=consumer_env)
    print("Ruby native gem consumer passed")
    void_model = output / "void.json"
    void_model.write_text(json.dumps({"smithy": "2.0", "shapes": {
        "example.void#Service": {"type": "service", "version": "1", "traits": {"crucible.protocols#json": {}}, "operations": [{"target": "example.void#Ping"}]},
        "example.void#Ping": {"type": "operation", "traits": {"smithy.api#http": {"method": "GET", "uri": "/ping"}}},
    }}), encoding="utf-8")
    renamed = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/weather.json").read_text())
    renamed["shapes"]["example.weather#Weather"]["rename"] = {
        "example.weather#CitySummary": "String", "example.weather#Location": "Array", "example.weather#Sky": "Time",
        "example.weather#Coordinates": "ArgumentError", "example.weather#PhotoSummary": "IOError",
    }
    for member in ["self", "context", "object", "metadata", "class", "message", "toWire"]:
        renamed["shapes"]["example.weather#CreateCityInput"]["members"][member] = {"target": "smithy.api#String"}
    renamed_model = output / "renamed.json"
    renamed_model.write_text(json.dumps(renamed), encoding="utf-8")
    for name, model in [("empty", ROOT / "tests/conformance/empty-service.json"), ("void", void_model), ("renamed", renamed_model)]:
        generated = output / name
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "ruby", str(model),
            "--out", str(generated), "--package-name", "weather_client", "--module-name", "Weather")
        client_name = {"empty":"EmptyClient", "void":"ServiceClient", "renamed":"WeatherClient"}[name]
        script = f"Weather::{client_name}.new(Weather::Runtime::ClientConfig.new(endpoint: 'http://127.0.0.1')).close"
        if name == "renamed":
            script += "; input = Weather::CreateCityInput.new(class_value: 'x', to_wire_value: 'y'); raise unless Weather.codec.encode(input, Weather.codec.context('example.weather#CreateCityInput')) == {'class'=>'x','toWire'=>'y'}"
            script += "; begin; Weather::Runtime::WireCodec.string(7); raise 'Expected native argument error'; rescue ::ArgumentError; end"
        run(RUBY, "-I", str(generated / "lib"), "-r", "weather_client", "-e", script)
    for name, fixture_name, package_name, module in [
        ("dynamic", "dynamic-bindings", "dynamic_client", "Dynamic"),
        ("extended", "extended-types", "types_client", "Types"),
    ]:
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "ruby",
            str(ROOT / f"crates/crucible-smithy/tests/fixtures/{fixture_name}.json"), "--out", str(output / name),
            "--package-name", package_name, "--module-name", module)
    run(RUBY, "-I", str(output / "dynamic/lib"), "-I", str(output / "extended/lib"),
        str(ROOT / "crates/crucible-codegen-ruby/tests/extensions.rb"))
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "ruby",
        str(ROOT / "crates/crucible-smithy/tests/fixtures/error-payloads.json"), "--out", str(output / "wire"),
        "--package-name", "wire_client", "--module-name", "WireSdk")
    run(RUBY, "-I", str(output / "wire/lib"), str(ROOT / "crates/crucible-codegen-ruby/tests/wire.rb"))
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "ruby",
        str(ROOT / "crates/crucible-codegen-python/tests/auth.json"), "--out", str(output / "auth"),
        "--package-name", "auth_client", "--module-name", "AuthSdk")
    run(RUBY, "-I", str(output / "auth/lib"), str(ROOT / "crates/crucible-codegen-ruby/tests/auth.rb"))
    print("Ruby empty, void, renamed, dynamic-binding and extended-type packages passed")


if __name__ == "__main__":
    explicit = os.environ.get("RUBY_TEST_OUTPUT")
    if explicit:
        directory = Path(explicit).resolve()
        directory.mkdir(parents=True, exist_ok=True)
        qualify(directory)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-ruby-") as directory:
            qualify(Path(directory))
