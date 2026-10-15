"""Pack a NuGet SDK, install it in a separate consumer, and exercise HTTP behavior."""
import json
import os
from pathlib import Path
import shutil
import tempfile
from sdk_test_support import ROOT, run, weather


def generate(model, output, namespace="Example.Weather", package="Example.Weather"):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "csharp",
        str(model), "--out", str(output), "--namespace", namespace, "--package-id", package,
        "--package-version", "0.0.0-test")


def policy(output):
    """Retry modes and telemetry against the shared runtime-policy vectors."""
    package = output / "policy"
    generate(ROOT / "crates/crucible-smithy/tests/fixtures/runtime-policy.json", package, "Example.Policy", "Example.Policy")
    feed = output / "policy-feed"
    run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(feed), cwd=package)
    consumer = output / "policy-consumer"
    consumer.mkdir()
    (consumer / "Consumer.csproj").write_text('''<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
      <ItemGroup><PackageReference Include="Example.Policy" Version="0.0.0-test"/></ItemGroup>
    </Project>''', encoding="utf-8")
    shutil.copyfile(ROOT / "crates/crucible-codegen-csharp/tests/Policy.cs", consumer / "Program.cs")
    run("dotnet", "restore", "--source", str(feed), "--packages", str(output / "policy-installed"), cwd=consumer)
    run("dotnet", "build", "--no-restore", "--nologo", "-v", "quiet", "-warnaserror", cwd=consumer)
    run("dotnet", str(consumer / "bin/Debug/net8.0/Consumer.dll"), str(ROOT / "tests/conformance"), cwd=consumer)


def consume(output, model, name, namespace, program, args=()):
    """Generate a package, pack it, install it in a fresh consumer, and run one test program against it."""
    package = output / name
    generate(model, package, namespace, namespace)
    feed = output / (name + "-feed")
    run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(feed), cwd=package)
    consumer = output / (name + "-consumer")
    consumer.mkdir()
    (consumer / "Consumer.csproj").write_text(f'''<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
      <ItemGroup><PackageReference Include="{namespace}" Version="0.0.0-test"/></ItemGroup>
    </Project>''', encoding="utf-8")
    shutil.copyfile(ROOT / "crates/crucible-codegen-csharp/tests" / program, consumer / "Program.cs")
    run("dotnet", "restore", "--source", str(feed), "--packages", str(output / (name + "-installed")), cwd=consumer)
    run("dotnet", "build", "--no-restore", "--nologo", "-v", "quiet", "-warnaserror", cwd=consumer)
    run("dotnet", str(consumer / "bin/Debug/net8.0/Consumer.dll"), *args, cwd=consumer)


def permissions(output):
    """Typed permissions: the open permission records and the registry; pack's -warnaserror checks the hostile docs."""
    consume(output, ROOT / "tests/fixtures/permissions", "permissions", "Example.Leyline", "Permissions.cs")


def qualify(output):
    # Doc comments and ToString of the generated models; pack runs with -warnaserror, so a malformed comment fails.
    consume(output, ROOT / "crates/crucible-smithy/tests/fixtures/client-docs-jvm.json", "display", "Example.Docs", "Display.cs")
    permissions(output)
    policy(output)
    wire = output / "wire"
    generate(ROOT / "crates/crucible-smithy/tests/fixtures/error-payloads.json", wire, "Example.Wire", "Example.Wire")
    wire_feed = output / "wire-feed"
    run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(wire_feed), cwd=wire)
    wire_consumer = output / "wire-consumer"
    wire_consumer.mkdir()
    (wire_consumer / "Consumer.csproj").write_text('''<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
      <ItemGroup><PackageReference Include="Example.Wire" Version="0.0.0-test"/></ItemGroup>
    </Project>''', encoding="utf-8")
    shutil.copyfile(ROOT / "crates/crucible-codegen-csharp/tests/Payloads.cs", wire_consumer / "Program.cs")
    run("dotnet", "restore", "--source", str(wire_feed), "--packages", str(output / "wire-installed"), cwd=wire_consumer)
    run("dotnet", "run", "--no-restore", "--project", str(wire_consumer), cwd=wire_consumer)
    for protocol, label in (("crucible.protocols#json", "payload-bindings"),):
        bindings = output / label
        generate(ROOT / "crates/crucible-codegen-python/tests/payload-bindings.json", bindings,
                 "Example.PayloadBindings", "Example.PayloadBindings")
        bindings_feed = output / (label + "-feed")
        run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(bindings_feed), cwd=bindings)
        bindings_consumer = output / (label + "-consumer")
        bindings_consumer.mkdir()
        (bindings_consumer / "Consumer.csproj").write_text('''<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
      <ItemGroup><PackageReference Include="Example.PayloadBindings" Version="0.0.0-test"/></ItemGroup>
    </Project>''', encoding="utf-8")
        shutil.copyfile(ROOT / "crates/crucible-codegen-csharp/tests/ContentType.cs", bindings_consumer / "Program.cs")
        run("dotnet", "restore", "--source", str(bindings_feed), "--packages", str(output / (label + "-installed")), cwd=bindings_consumer)
        run("dotnet", "run", "--no-restore", "--project", str(bindings_consumer), cwd=bindings_consumer)
    extended = output / "types"
    generate(ROOT / "crates/crucible-smithy/tests/fixtures/extended-types.json", extended, "Example.Types", "Example.Types")
    types_feed = output / "types-feed"
    run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(types_feed), cwd=extended)
    types_consumer = output / "types-consumer"
    types_consumer.mkdir()
    (types_consumer / "Consumer.csproj").write_text('''<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
      <ItemGroup><PackageReference Include="Example.Types" Version="0.0.0-test"/></ItemGroup>
    </Project>''', encoding="utf-8")
    shutil.copyfile(ROOT / "crates/crucible-codegen-csharp/tests/Types.cs", types_consumer / "Program.cs")
    run("dotnet", "restore", "--source", str(types_feed), "--packages", str(output / "types-installed"), cwd=types_consumer)
    run("dotnet", "run", "--no-restore", "--project", str(types_consumer), cwd=types_consumer)
    package = output / "sdk"
    feed = output / "feed"
    generate(ROOT / "crates/crucible-smithy/tests/fixtures/weather.json", package)
    run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(feed), cwd=package)
    consumer = output / "consumer"
    consumer.mkdir()
    (consumer / "Consumer.csproj").write_text('''<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
      <ItemGroup><PackageReference Include="Example.Weather" Version="0.0.0-test"/></ItemGroup>
    </Project>''', encoding="utf-8")
    shutil.copyfile(ROOT / "crates/crucible-codegen-csharp/tests/Program.cs", consumer / "Program.cs")
    # A fresh package cache proves this run consumes the package just generated.
    run("dotnet", "restore", "--source", str(feed), "--packages", str(output / "installed"), cwd=consumer)
    run("dotnet", "build", "--no-restore", "--nologo", "-v", "quiet", "-warnaserror", cwd=consumer)
    with weather() as endpoint:
        run("dotnet", str(consumer / "bin/Debug/net8.0/Consumer.dll"),
            str(ROOT / "tests/conformance/retry.json"), str(ROOT / "tests/conformance/header-lists.json"),
            env=dict(os.environ, WEATHER_URL=endpoint))
    renamed = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/weather.json").read_text())
    renamed["shapes"]["example.weather#Weather"]["rename"] = {
        "example.weather#CitySummary": "String", "example.weather#CitySummaries": "Optional",
        "example.weather#ConflictError": "ServiceError", "example.weather#Location": "UnknownUnion",
    }
    for name in ["toString", "getType", "equals", "getHashCode"]:
        renamed["shapes"]["example.weather#CreateCityInput"]["members"][name] = {"target": "smithy.api#String"}
    for name in ["getBaseException", "getObjectData"]:
        renamed["shapes"]["example.weather#ConflictError"]["members"][name] = {"target": "smithy.api#String"}
    renamed_path = output / "renamed.json"
    renamed_path.write_text(json.dumps(renamed), encoding="utf-8")
    recursive = json.loads(json.dumps(renamed))
    recursive["shapes"].update({
        "example.weather#RecursiveNode": {"type": "structure", "members": {"children": {"target": "example.weather#RecursiveList"}, "mapping": {"target": "example.weather#RecursiveMap"}}},
        "example.weather#RecursiveList": {"type": "list", "member": {"target": "example.weather#RecursiveNode"}},
        "example.weather#RecursiveMap": {"type": "map", "key": {"target": "smithy.api#String"}, "value": {"target": "example.weather#RecursiveListMap"}},
        "example.weather#RecursiveListMap": {"type": "list", "member": {"target": "example.weather#RecursiveNode"}},
    })
    for name in ["RecursiveList", "RecursiveMap"]:
        recursive["shapes"]["example.weather#CreateCityInput"]["members"][name] = {"target": "example.weather#" + name}
    recursive_path = output / "recursive.json"
    recursive_path.write_text(json.dumps(recursive), encoding="utf-8")
    for label, model in [("Empty", ROOT / "tests/conformance/empty-service.json"), ("Renamed", renamed_path), ("Recursive", recursive_path)]:
        generated = output / label
        generate(model, generated, "Example." + label, "Example." + label)
        run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", cwd=generated)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="crucible-csharp-") as directory:
        if os.environ.get("CSHARP_ONLY") == "policy":
            policy(Path(directory))
        elif os.environ.get("CSHARP_ONLY") == "permissions":
            permissions(Path(directory))
        else:
            qualify(Path(directory))
