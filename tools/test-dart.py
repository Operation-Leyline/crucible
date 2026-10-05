"""Qualify generated Dart packages on the VM and JavaScript in headless Chrome."""
import json
import os
from pathlib import Path
import shutil
import tempfile

from sdk_test_support import ROOT, run, weather
from sdk_conformance_model import qualification_model
from dart_http_fixture import fixture

DART = os.environ.get("DART", "dart")


def dart(*args, **kwargs):
    run(DART, *args, **kwargs)


def qualify(output):
    package = output / "sdk"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "dart",
        str(qualification_model(output)), "--out", str(package), "--package-name", "weather_client", "--content-sha256")
    manifest = json.loads((package / "pubspec.yaml").read_text())
    manifest["dev_dependencies"] = {"test": "^1.25.15"}
    (package / "pubspec.yaml").write_text(json.dumps(manifest), encoding="utf-8")
    tests = package / "test"
    tests.mkdir()
    source = ROOT / "crates/crucible-codegen-dart/tests"
    for name in ["runtime", "native", "http", "serialization"]:
        shutil.copyfile(source / f"{name}_test.dart.template", tests / f"{name}_test.dart")
    for name in ["json", "timestamp", "codec"]:
        checks = (source / f"{name}_test.dart").read_text().replace(
            "../runtime/", "package:weather_client/src/runtime/").replace("json_test.dart", "json_checks.dart")
        (tests / f"{name}_checks.dart").write_text(checks, encoding="utf-8")
    (tests / "exact_test.dart").write_text("""import 'package:test/test.dart';
import 'json_checks.dart' as json;
import 'timestamp_checks.dart' as timestamp;
import 'codec_checks.dart' as codec;
void main() {
  test('exact JSON and signed long boundaries', json.main);
  test('timestamp precision and calendar boundaries', timestamp.main);
  test('codec nulls, defaults, binding and range checks', codec.main);
}
""", encoding="utf-8")
    dart("pub", "get", cwd=package)
    with weather() as endpoint, fixture() as fixture_url:
        fixtures = {"weatherUrl": endpoint, "fixtureUrl": fixture_url,
            "retryCases": (ROOT / "tests/conformance/retry.json").read_text(),
            "headerCases": (ROOT / "tests/conformance/header-lists.json").read_text(),
            "serializationCases": (ROOT / "tests/conformance/serialization.json").read_text()}
        (tests / "fixtures.dart").write_text("\n".join(
            f"const {name} = {json.dumps(value, ensure_ascii=True).replace('$', chr(92) + '$')};"
            for name, value in fixtures.items()), encoding="utf-8")
        dart("analyze", cwd=package)
        dart("test", "--reporter", "expanded", cwd=package)
        dart("test", "--platform", "chrome", "--reporter", "expanded", "test/runtime_test.dart", "test/http_test.dart", "test/exact_test.dart", "test/serialization_test.dart", cwd=package)
    qualify_packages(output, package)
    print("Dart VM, Chrome conformance, and real weather HTTP passed")


def qualify_packages(output, package):
    # Install as a path dependency from a separate package, then compile its
    # consumer with both native and JavaScript compilers.
    consumer = output / "consumer"
    consumer.mkdir(exist_ok=True)
    (consumer / "pubspec.yaml").write_text(json.dumps({"name": "weather_consumer", "version": "0.0.0", "publish_to": "none",
        "environment": {"sdk": ">=3.6.2 <4.0.0"},
        "dependencies": {"weather_client": {"path": package.as_posix()}}}), encoding="utf-8")
    (consumer / "main.dart").write_text('''
import 'package:weather_client/weather_client.dart';
final class Fake implements HttpTransport {
  @override
  Future<HttpResponse> send(HttpRequest request, {required bool readBody, required CancellationToken cancellation}) async => HttpResponse(status:200);
  @override
  void close() { throw StateError('Caller-owned transport was closed'); }
}
Future<void> main() async {
  final client = WeatherClient(ClientConfig(endpoint:'http://127.0.0.1', transport:Fake()));
  await client.ping();
  client.close();
}
''', encoding="utf-8")
    dart("pub", "get", cwd=consumer)
    dart("analyze", cwd=consumer)
    executable = consumer / ("consumer.exe" if os.name == "nt" else "consumer")
    dart("compile", "exe", "main.dart", "-o", str(executable), cwd=consumer)
    run(str(executable))
    dart("compile", "js", "main.dart", "-o", "consumer.js", cwd=consumer)
    empty = output / "empty"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "dart",
        str(ROOT / "tests/conformance/empty-service.json"), "--out", str(empty), "--package-name", "empty_client")
    dart("pub", "get", cwd=empty)
    dart("analyze", cwd=empty)
    renamed = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/weather.json").read_text())
    renamed["shapes"]["example.weather#Weather"]["rename"] = {
        "example.weather#CitySummary": "String", "example.weather#CitySummaries": "List",
        "example.weather#Location": "Object", "example.weather#Sky": "Future",
    }
    for member in ["class", "object", "context", "core", "wire", "json", "typed", "stamp", "errors"]:
        renamed["shapes"]["example.weather#CreateCityInput"]["members"][member] = {"target": "smithy.api#String"}
    renamed_path = output / "renamed.json"
    renamed_path.write_text(json.dumps(renamed))
    void_path = output / "void.json"
    void_path.write_text(json.dumps({"smithy": "2.0", "shapes": {
        "example.void#Service": {"type": "service", "version": "1",
            "traits": {"crucible.protocols#json": {}}, "operations": [{"target": "example.void#Ping"}]},
        "example.void#Ping": {"type": "operation",
            "traits": {"smithy.api#http": {"method": "GET", "uri": "/ping"}}},
    }}))
    for name, model in [("renamed", renamed_path), ("void", void_path)]:
        generated = output / name
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "dart",
            str(model), "--out", str(generated), "--package-name", name + "_client")
        dart("pub", "get", cwd=generated)
        dart("analyze", cwd=generated)
        if name == "renamed":
            (generated / "main.dart").write_text("""import 'package:renamed_client/renamed_client.dart' as sdk;
import 'package:renamed_client/src/models.dart' show decodeModel;
import 'package:renamed_client/src/runtime/codec.dart';
import 'package:renamed_client/src/runtime/json.dart';
import 'package:renamed_client/src/schema.dart';
void main() {
  final codec = WireCodec(wireObject(exactJsonDecode(schemaSource)), decodeModel);
  const input = sdk.CreateCityInput(object:'object', context:'context',
    coreValue:'core', wireValue:'wire', jsonValue:'json',
    typedValue:'typed', stampValue:'stamp', errorsValue:'errors');
  final context = codec.context('example.weather#CreateCityInput');
  final encoded = wireObject(codec.encode(input, context));
  for (final field in ['object', 'context', 'core', 'wire', 'json', 'typed', 'stamp', 'errors']) {
    if (encoded[field] != field) throw StateError('Shadowed member: $field');
  }
  final copy = codec.decode(encoded, context) as sdk.CreateCityInput;
  if (copy.object != 'object' || copy.context != 'context') throw StateError('Decode failed');
}
""", encoding="utf-8")
            dart("run", "main.dart", cwd=generated)
    print("Dart fresh consumer, native and JS compilation, empty and renamed packages passed")


if __name__ == "__main__":
    explicit = os.environ.get("DART_TEST_OUTPUT")
    if explicit:
        output = Path(explicit).resolve()
        output.mkdir(parents=True, exist_ok=True)
        qualify(output)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-dart-") as directory:
            qualify(Path(directory))
