"""Qualify generated Dart packages on the VM and JavaScript in headless Chrome."""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

from sdk_test_support import ROOT, run, weather
from sdk_conformance_model import qualification_model
from sdk_cru_packages import copy_package, rename_shapes, replace_once, write_package
from dart_http_fixture import fixture

DART = os.environ.get("DART", "dart")

VOID = """service Service {
    version "1"
    protocol json
    operations [Ping]
}

readonly operation Ping {
    http GET "/ping"
}
"""


def dart(*args, **kwargs):
    run(DART, *args, **kwargs)


def qualify(output):
    package = output / "sdk"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "dart",
        str(qualification_model(output)), "--out", str(package), "--package-name", "weather_client")
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
        str(ROOT / "tests/models/empty-service"), "--out", str(empty), "--package-name", "empty_client")
    dart("pub", "get", cwd=empty)
    dart("analyze", cwd=empty)
    renamed_path = copy_package("weather", output / "renamed-model")
    members = ["class", "object", "context", "core", "wire", "json", "typed", "stamp", "errors", "describe"]
    replace_once(renamed_path, "location?: Location\n", "location?: Location\n" + "".join(f"        {member}?: string\n" for member in members))
    rename_shapes(renamed_path, {"CitySummary": "String", "CitySummaries": "List", "Location": "Object", "Sky": "Future"})
    void_path = write_package(output / "void-model", "example.void", **{"void.cru": VOID})
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
    typedValue:'typed', stampValue:'stamp', errorsValue:'errors', describeValue:'describe');
  final context = codec.context('example.weather#CreateCityInput');
  final encoded = wireObject(codec.encode(input, context));
  for (final field in ['object', 'context', 'core', 'wire', 'json', 'typed', 'stamp', 'errors', 'describe']) {
    if (encoded[field] != field) throw StateError('Shadowed member: $field');
  }
  final copy = codec.decode(encoded, context) as sdk.CreateCityInput;
  if (copy.object != 'object' || copy.context != 'context') throw StateError('Decode failed');
}
""", encoding="utf-8")
            dart("run", "main.dart", cwd=generated)
    for name, fixture_name in [("extended", "extended-types"), ("wire", "error-payloads"), ("describe", "client-docs-vault")]:
        generated = output / name
        package_name = {"extended": "types_client", "wire": "wire_client", "describe": "vault_client"}[name]
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "dart",
            str(ROOT / "tests/models" / fixture_name),
            "--out", str(generated), "--package-name", package_name)
        manifest = json.loads((generated / "pubspec.yaml").read_text())
        manifest["dev_dependencies"] = {"test": "^1.25.15"}
        (generated / "pubspec.yaml").write_text(json.dumps(manifest), encoding="utf-8")
        tests = generated / "test"
        tests.mkdir(exist_ok=True)
        shutil.copyfile(ROOT / "crates/crucible-codegen-dart/tests" / f"{name}_test.dart.template", tests / f"{name}_test.dart")
        dart("pub", "get", cwd=generated)
        dart("analyze", cwd=generated)
        dart("test", cwd=generated)
        dart("test", "--platform", "chrome", cwd=generated)
        if name == "describe":
            # dartdoc must accept every generated and bundled doc comment.
            docs = subprocess.run([DART, "doc", "."], cwd=generated, check=True, capture_output=True, text=True)
            if re.search(r"\b(warning|error)\b", docs.stdout + docs.stderr, re.I) and "no issues found" not in docs.stdout:
                raise RuntimeError("dart doc reported problems:\n" + docs.stdout + docs.stderr)
    qualify_payload_bindings(output)
    qualify_permissions(output)
    print("Dart fresh consumer, native and JS compilation, empty and renamed packages passed")


def qualify_permissions(output):
    """The typed permissions library, from a separate package, as a console would use it."""
    sdk = output / "leyline"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "dart",
        "tests/fixtures/permissions", "--out", str(sdk), "--package-name", "leyline")
    dart("pub", "get", cwd=sdk)
    dart("analyze", cwd=sdk)
    consumer = output / "permissions_consumer"
    consumer.mkdir(exist_ok=True)
    (consumer / "pubspec.yaml").write_text(json.dumps({"name": "permissions_consumer", "version": "0.0.0",
        "publish_to": "none", "environment": {"sdk": ">=3.6.2 <4.0.0"},
        "dependencies": {"leyline": {"path": sdk.as_posix()}}}), encoding="utf-8")
    (consumer / "main.dart").write_text('''import 'dart:convert';
import 'package:leyline/permissions.dart';

void check(bool ok, String what) {
  if (!ok) throw StateError(what);
}

void main() {
  check(GraphPermission.read.value == 'graph:read', 'a known constant');
  for (final value in ['graph:export', 'organization:read']) {
    final permission = GraphPermission(jsonDecode(jsonEncode(value)) as String);
    check(permission.value == value, 'decoded $value');
    check(jsonEncode(permission) == jsonEncode(value), '$value round-trips');
    check(permission.canonical() == permission, 'an unknown value is its own canonical');
  }
  // ignore: deprecated_member_use
  check(GraphPermission.view.canonical() == GraphPermission.read, 'an alias maps to its primary');
  check(GraphPermission.write.canonical() == GraphPermission.write, 'a primary is its own canonical');
  check(HistoryPermission.restore.canonical() == HistoryPermission.restore, 'no aliases');
  check(permissions.map((r) => r.resourceType).join(',') == 'graph,history,org', 'resource types');
  final known = {for (final r in permissions) for (final p in r.permissions) p.value: p};
  final admin = known['organization:admin']!;
  check(!admin.grantable && admin.tier == 'notGrantable', 'organization:admin is not grantable');
  final view = known['graph:view']!;
  check(view.aliasOf == 'graph:read' && view.deprecated, 'graph:view is a deprecated alias');
  check(known['graph:read']!.aliasOf == null, 'graph:read is a primary');
  check(!known.containsKey('organization:read'), 'an internal permission never appears');
  // A grant picker's filter.
  final picker = [
    for (final r in permissions)
      for (final p in r.permissions)
        if (p.grantable && !p.deprecated) p.value
  ];
  check(picker.join(',') == 'graph:audit,graph:read,graph:write,history:read,history:restore', 'picker $picker');
  check(jsonEncode(view) == '{"value":"graph:view","description":"","tier":"public","grantable":true,"deprecated":true,"aliasOf":"graph:read"}', jsonEncode(view));
  check(jsonEncode(admin) == '{"value":"organization:admin","description":"Everything in the organization.","tier":"notGrantable","grantable":false,"deprecated":false}', jsonEncode(admin));
}
''', encoding="utf-8")
    dart("pub", "get", cwd=consumer)
    dart("analyze", "--fatal-infos", cwd=consumer)
    dart("run", "main.dart", cwd=consumer)
    dart("compile", "js", "main.dart", "-o", "consumer.js", cwd=consumer)


def qualify_payload_bindings(output):
    """Payload Content-Type precedence under both JSON protocols."""
    path = ROOT / "tests/models/payload-bindings"
    for name in ("payload_bindings",):
        generated = output / name
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "dart", str(path),
            "--out", str(generated), "--package-name", "payload_bindings_client")
        manifest = json.loads((generated / "pubspec.yaml").read_text())
        manifest["dev_dependencies"] = {"test": "^1.25.15"}
        (generated / "pubspec.yaml").write_text(json.dumps(manifest), encoding="utf-8")
        (generated / "test").mkdir(exist_ok=True)
        shutil.copyfile(ROOT / "crates/crucible-codegen-dart/tests/payload_bindings_test.dart.template",
                        generated / "test/payload_bindings_test.dart")
        dart("pub", "get", cwd=generated)
        dart("analyze", cwd=generated)
        dart("test", cwd=generated)
        dart("test", "--platform", "chrome", cwd=generated)


if __name__ == "__main__":
    explicit = os.environ.get("DART_TEST_OUTPUT")
    if explicit:
        output = Path(explicit).resolve()
        output.mkdir(parents=True, exist_ok=True)
        qualify(output)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-dart-") as directory:
            qualify(Path(directory))
