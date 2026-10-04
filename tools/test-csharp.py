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
        "--package-version", "0.0.0-test", "--content-sha256")


def qualify(output):
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
    for label, model in [("Empty", ROOT / "tests/conformance/empty-service.json"), ("Renamed", renamed_path)]:
        generated = output / label
        generate(model, generated, "Example." + label, "Example." + label)
        run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", cwd=generated)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="crucible-csharp-") as directory:
        qualify(Path(directory))
