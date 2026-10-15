"""Qualify native Ruby gems against shared fixtures and real HTTP."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from sdk_test_support import ROOT, run, weather
from sdk_conformance_model import qualification_model
from sdk_cru_packages import copy_package, rename_shapes, replace_once, write_package
from sdk_http_fixture import fixture

RUBY = os.environ.get("RUBY", "ruby")


def void_package(output):
    """A service whose one operation has no input or output."""
    return write_package(output / "void", "example.void", **{"void.cru": (
        "service Service {\n    version \"1\"\n    protocol json\n    operations [Ping]\n}\n\n"
        "readonly operation Ping {\n    http GET \"/ping\"\n}\n")})


def add_input_members(package, *names):
    """Optional string members on CreateCity's input."""
    replace_once(package, "        clientToken?: string\n",
                 "        clientToken?: string\n" + "".join(f"        {name}?: string\n" for name in names))


def gem(*args, **kwargs):
    run(RUBY, "-rrubygems/gem_runner", "-e", "Gem::GemRunner.new.run(ARGV)", "--", *args, **kwargs)


def qualify(output):
    package = output / "sdk"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "ruby",
        str(qualification_model(output)), "--out", str(package), "--package-name", "weather_client",
        "--module-name", "Weather")
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
    # String representations must hide sensitive members from every inspection path.
    vault = output / "vault"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "ruby",
        str(ROOT / "tests/models/client-docs-vault"), "--out", str(vault),
        "--package-name", "vault_client", "--module-name", "Vault")
    for source in sorted(vault.rglob("*.rb")):
        run(RUBY, "-c", str(source), stdout=subprocess.DEVNULL)
    run(RUBY, "-I", str(vault / "lib"), str(ROOT / "crates/crucible-codegen-ruby/tests/describe.rb"))
    gem("build", "weather_client.gemspec", cwd=package)
    consumer = output / "consumer"
    default_gems = subprocess.check_output([RUBY, "-r", "rubygems", "-e", "puts Gem.default_dir"], text=True).strip()
    consumer_env = dict(os.environ, GEM_HOME=str(consumer), GEM_PATH=os.pathsep.join([str(consumer), default_gems]))
    gem("install", "--local", "--no-document",
        str(package / "weather_client-0.0.0.gem"), env=consumer_env)
    run(RUBY, "-r", "weather_client", "-e", "raise unless Weather::CreateCityInput.new(name: 'consumer').name == 'consumer'; Weather::WeatherClient.new(Weather::Runtime::ClientConfig.new(endpoint: 'https://example.invalid')).close",
        env=consumer_env)
    print("Ruby native gem consumer passed")
    void_model = void_package(output)
    renamed_model = copy_package("weather", output / "renamed")
    rename_shapes(renamed_model, {"CitySummary": "String", "Location": "Array", "Sky": "Time",
                                  "Coordinates": "ArgumentError", "PhotoSummary": "IOError"})
    add_input_members(renamed_model, "self", "context", "object", "metadata", "class", "message", "toWire", "describe")
    for name, model in [("empty", ROOT / "tests/models/empty-service"), ("void", void_model), ("renamed", renamed_model)]:
        generated = output / name
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "ruby", str(model),
            "--out", str(generated), "--package-name", "weather_client", "--module-name", "Weather")
        client_name = {"empty":"EmptyClient", "void":"ServiceClient", "renamed":"WeatherClient"}[name]
        script = f"Weather::{client_name}.new(Weather::Runtime::ClientConfig.new(endpoint: 'http://127.0.0.1')).close"
        if name == "renamed":
            script += "; input = Weather::CreateCityInput.new(class_value: 'x', to_wire_value: 'y'); raise unless Weather.codec.encode(input, Weather.codec.context('example.weather#CreateCityInput')) == {'class'=>'x','toWire'=>'y'}"
            script += "; begin; Weather::Runtime::WireCodec.string(7); raise 'Expected native argument error'; rescue ::ArgumentError; end"
        run(RUBY, "-I", str(generated / "lib"), "-r", "weather_client", "-e", script)
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "ruby",
        str(ROOT / "tests/models/extended-types"), "--out", str(output / "extended"),
        "--package-name", "types_client", "--module-name", "Types")
    run(RUBY, "-I", str(output / "extended/lib"), str(ROOT / "crates/crucible-codegen-ruby/tests/extensions.rb"))
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "ruby",
        str(ROOT / "tests/models/error-payloads"), "--out", str(output / "wire"),
        "--package-name", "wire_client", "--module-name", "WireSdk")
    run(RUBY, "-I", str(output / "wire/lib"), str(ROOT / "crates/crucible-codegen-ruby/tests/wire.rb"))
    # Payload Content-Type precedence.
    payload = output / "payload"
    generate(ROOT / "tests/models/payload-bindings", payload, "payload_client", "PayloadSdk")
    run(RUBY, "-I", str(payload / "lib"), str(ROOT / "crates/crucible-codegen-ruby/tests/payload_bindings.rb"))
    # Typed permissions: the open permission classes and the registry, from the shared .cru fixture.
    generate(ROOT / "tests/fixtures/permissions", output / "permissions", "leyline_client", "Leyline")
    run(RUBY, "-I", str(output / "permissions/lib"), str(ROOT / "crates/crucible-codegen-ruby/tests/permissions.rb"))
    print("Ruby empty, void, renamed and extended-type packages passed")


def generate(model, out, package, module, *extra):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "ruby", str(model), *extra,
        "--out", str(out), "--package-name", package, "--module-name", module)
    for source in sorted(out.rglob("*.rb")):
        run(RUBY, "-c", str(source), stdout=subprocess.DEVNULL)


if __name__ == "__main__":
    explicit = os.environ.get("RUBY_TEST_OUTPUT")
    if explicit:
        directory = Path(explicit).resolve()
        directory.mkdir(parents=True, exist_ok=True)
        qualify(directory)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-ruby-") as directory:
            qualify(Path(directory))
