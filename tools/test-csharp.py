"""Pack a NuGet SDK, install it in a separate consumer, and exercise HTTP behavior."""
import json
import os
from pathlib import Path
import shutil
import tempfile
from sdk_test_support import ROOT, run, weather


def generate(model, output, namespace="Example.Weather", package="Example.Weather", digest=True):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "csharp",
        str(model), "--out", str(output), "--namespace", namespace, "--package-id", package,
        "--package-version", "0.0.0-test", *(["--content-sha256"] if digest else []))


def streaming(output):
    """Streaming blobs over loopback HTTP, with a hard GC heap limit below one buffered 256 MiB body."""
    model = json.loads((ROOT / "crates/crucible-codegen-java/tests/streaming.json").read_text())
    # SigV4 is optional per operation, so one package exercises unsigned and signed streams.
    service = model["shapes"]["example.stream#Streaming"]["traits"]
    service["aws.auth#sigv4"] = {"name": "service"}
    service["smithy.api#auth"] = ["aws.auth#sigv4"]
    for shape in model["shapes"].values():
        if shape["type"] == "operation":
            shape["traits"]["smithy.api#optionalAuth"] = {}
    path = output / "streaming.json"
    path.write_text(json.dumps(model), encoding="utf-8")
    package = output / "stream"
    generate(path, package, "Example.Stream", "Example.Stream", digest=False)
    feed = output / "stream-feed"
    run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(feed), cwd=package)
    consumer = output / "stream-consumer"
    consumer.mkdir()
    (consumer / "Consumer.csproj").write_text('''<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
      <ItemGroup><PackageReference Include="Example.Stream" Version="0.0.0-test"/></ItemGroup>
      <ItemGroup><RuntimeHostConfigurationOption Include="System.GC.HeapHardLimit" Value="134217728"/></ItemGroup>
    </Project>''', encoding="utf-8")
    shutil.copyfile(ROOT / "crates/crucible-codegen-csharp/tests/Streaming.cs", consumer / "Program.cs")
    run("dotnet", "restore", "--source", str(feed), "--packages", str(output / "stream-installed"), cwd=consumer)
    run("dotnet", "build", "--no-restore", "--nologo", "-v", "quiet", "-warnaserror", cwd=consumer)
    run("dotnet", str(consumer / "bin/Debug/net8.0/Consumer.dll"), cwd=consumer)


def policy(output):
    """Retry modes, endpoint discovery and telemetry against the shared runtime-policy vectors."""
    package = output / "policy"
    generate(ROOT / "crates/crucible-smithy/tests/fixtures/runtime-policy.json", package, "Example.Policy", "Example.Policy", digest=False)
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
    generate(model, package, namespace, namespace, digest=False)
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


def vault(output):
    """Digest and SigV4a: published vectors, client behavior, and a loopback server that verifies both itself."""
    consume(output, ROOT / "crates/crucible-codegen-csharp/tests/vault.json", "vault", "Example.Vault", "VaultAuth.cs")


def permissions(output):
    """Typed permissions: the open permission records and the registry; pack's -warnaserror checks the hostile docs."""
    consume(output, ROOT / "tests/fixtures/permissions", "permissions", "Example.Leyline", "Permissions.cs")


def qualify(output):
    # Doc comments and ToString of the generated models; pack runs with -warnaserror, so a malformed comment fails.
    consume(output, ROOT / "crates/crucible-smithy/tests/fixtures/client-docs-jvm.json", "display", "Example.Docs", "Display.cs")
    permissions(output)
    vault(output)
    policy(output)
    streaming(output)
    checksum = output / "checksum"
    generate(ROOT / "crates/crucible-smithy/tests/fixtures/checksums.json", checksum, "Example.Checksum", "Example.Checksum")
    checksum_feed = output / "checksum-feed"
    run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(checksum_feed), cwd=checksum)
    checksum_consumer = output / "checksum-consumer"
    checksum_consumer.mkdir()
    (checksum_consumer / "Consumer.csproj").write_text('''<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
      <ItemGroup><PackageReference Include="Example.Checksum" Version="0.0.0-test"/></ItemGroup>
    </Project>''', encoding="utf-8")
    shutil.copyfile(ROOT / "crates/crucible-codegen-csharp/tests/Checksums.cs", checksum_consumer / "Program.cs")
    shutil.copyfile(ROOT / "tests/conformance/checksums.json", checksum_consumer / "checksums.json")
    run("dotnet", "restore", "--source", str(checksum_feed), "--packages", str(output / "checksum-installed"), cwd=checksum_consumer)
    run("dotnet", "run", "--no-restore", "--project", str(checksum_consumer), cwd=checksum_consumer)
    behavior = output / "behavior"
    generate(ROOT / "crates/crucible-smithy/tests/fixtures/client-behaviors.json", behavior, "Example.Behavior", "Example.Behavior")
    behavior_feed = output / "behavior-feed"
    run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(behavior_feed), cwd=behavior)
    behavior_consumer = output / "behavior-consumer"
    behavior_consumer.mkdir()
    (behavior_consumer / "Consumer.csproj").write_text('''<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
      <ItemGroup><PackageReference Include="Example.Behavior" Version="0.0.0-test"/></ItemGroup>
    </Project>''', encoding="utf-8")
    shutil.copyfile(ROOT / "crates/crucible-codegen-csharp/tests/Endpoints.cs", behavior_consumer / "Program.cs")
    run("dotnet", "restore", "--source", str(behavior_feed), "--packages", str(output / "behavior-installed"), cwd=behavior_consumer)
    run("dotnet", "run", "--no-restore", "--project", str(behavior_consumer), cwd=behavior_consumer)
    jobs = output / "jobs"
    generate(ROOT / "crates/crucible-codegen-java/tests/waiters.json", jobs, "Example.Jobs", "Example.Jobs")
    jobs_feed = output / "jobs-feed"
    run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(jobs_feed), cwd=jobs)
    jobs_consumer = output / "jobs-consumer"
    jobs_consumer.mkdir()
    (jobs_consumer / "Consumer.csproj").write_text('''<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
      <ItemGroup><PackageReference Include="Example.Jobs" Version="0.0.0-test"/></ItemGroup>
    </Project>''', encoding="utf-8")
    shutil.copyfile(ROOT / "crates/crucible-codegen-csharp/tests/Waiters.cs", jobs_consumer / "Program.cs")
    run("dotnet", "restore", "--source", str(jobs_feed), "--packages", str(output / "jobs-installed"), cwd=jobs_consumer)
    run("dotnet", "run", "--no-restore", "--project", str(jobs_consumer), cwd=jobs_consumer)
    auth = output / "auth"
    generate(ROOT / "crates/crucible-codegen-python/tests/auth.json", auth, "Example.Auth", "Example.Auth")
    auth_feed = output / "auth-feed"
    run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(auth_feed), cwd=auth)
    auth_consumer = output / "auth-consumer"
    auth_consumer.mkdir()
    (auth_consumer / "Consumer.csproj").write_text('''<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
      <ItemGroup><PackageReference Include="Example.Auth" Version="0.0.0-test"/></ItemGroup>
    </Project>''', encoding="utf-8")
    shutil.copyfile(ROOT / "crates/crucible-codegen-csharp/tests/Auth.cs", auth_consumer / "Program.cs")
    run("dotnet", "restore", "--source", str(auth_feed), "--packages", str(output / "auth-installed"), cwd=auth_consumer)
    run("dotnet", "run", "--no-restore", "--project", str(auth_consumer), cwd=auth_consumer)
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
                 "Example.PayloadBindings", "Example.PayloadBindings", protocol=protocol, digest=False)
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
    dynamic = output / "dynamic"
    generate(ROOT / "crates/crucible-smithy/tests/fixtures/dynamic-bindings.json", dynamic, "Example.Dynamic", "Example.Dynamic")
    dynamic_feed = output / "dynamic-feed"
    run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(dynamic_feed), cwd=dynamic)
    dynamic_consumer = output / "dynamic-consumer"
    dynamic_consumer.mkdir()
    (dynamic_consumer / "Consumer.csproj").write_text('''<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
      <ItemGroup><PackageReference Include="Example.Dynamic" Version="0.0.0-test"/></ItemGroup>
    </Project>''', encoding="utf-8")
    shutil.copyfile(ROOT / "crates/crucible-codegen-csharp/tests/Dynamic.cs", dynamic_consumer / "Program.cs")
    run("dotnet", "restore", "--source", str(dynamic_feed), "--packages", str(output / "dynamic-installed"), cwd=dynamic_consumer)
    run("dotnet", "run", "--no-restore", "--project", str(dynamic_consumer), cwd=dynamic_consumer)
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
        if os.environ.get("CSHARP_ONLY") == "streaming":
            streaming(Path(directory))
        elif os.environ.get("CSHARP_ONLY") == "policy":
            policy(Path(directory))
        elif os.environ.get("CSHARP_ONLY") == "vault":
            vault(Path(directory))
        elif os.environ.get("CSHARP_ONLY") == "permissions":
            permissions(Path(directory))
        else:
            qualify(Path(directory))
