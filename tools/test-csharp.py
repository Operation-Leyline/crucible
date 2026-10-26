"""Pack a NuGet SDK, install it in a separate consumer, and exercise HTTP behavior."""
import os
from pathlib import Path
import shutil
import tempfile
from sdk_cru_packages import copy_package, rename_shapes, replace_once
from sdk_test_support import ROOT, describe_sdk, run, snippets, weather


RECURSIVE = """structure RecursiveNode {
    children?: RecursiveList
    mapping?: RecursiveMap
}

list RecursiveList: RecursiveNode

map RecursiveMap: string -> RecursiveListMap

list RecursiveListMap: RecursiveNode
"""


def generate(model, output, namespace="Example.Weather", package="Example.Weather"):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "csharp",
        str(model), "--out", str(output), "--namespace", namespace, "--package-id", package,
        "--package-version", "0.0.0-test")


def policy(output):
    """Retry modes and telemetry against the shared runtime-policy vectors."""
    package = output / "policy"
    generate(ROOT / "tests/models/runtime-policy", package, "Example.Policy", "Example.Policy")
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


def check_snippets(output, weather_feed):
    """The crucible.sdks/1 snippets (docs/docs.md, "Snippets") build against the
    packages they call: weather packed in `weather_feed`, and samples, generated
    with the options it is described with. Each snippet is a console project's
    top-level statements, as a caller pastes it, so each has its own project;
    one solution builds them all. Warnings stay warnings: a snippet calling a
    deprecated operation is still the call a caller makes."""
    samples = output / "samples"
    generate(ROOT / "tests/models/samples", samples, "Example.Samples", "Example.Samples")
    samples_feed = output / "samples-feed"
    run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(samples_feed), cwd=samples)
    root = output / "snippets"
    root.mkdir()
    projects = []
    for model, namespace, table in (
        ("weather", "Example.Weather",
         '[sdk.csharp]\nout = "generated/csharp"\nnamespace = "Example.Weather"\npackage_id = "Example.Weather"\n'),
        ("samples", "Example.Samples",
         '[sdk.csharp]\nout = "generated/csharp"\nnamespace = "Example.Samples"\npackage_id = "Example.Samples"\n'),
    ):
        for _, operation, variant, text in snippets(describe_sdk(table, ROOT / "tests/models" / model)):
            project = root / model / f"{operation}{'Pages' if variant == 'pages' else ''}"
            project.mkdir(parents=True)
            name = f"{model.title()}{project.name}"
            (project / f"{name}.csproj").write_text(f'''<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
      <ItemGroup><PackageReference Include="{namespace}" Version="0.0.0-test"/></ItemGroup>
    </Project>''', encoding="utf-8")
            (project / "Program.cs").write_text(text, encoding="utf-8")
            projects.append(str(project / f"{name}.csproj"))
    if not projects:
        raise RuntimeError("no C# snippets described")
    run("dotnet", "new", "sln", "--name", "Snippets", cwd=root)
    solution = next(root.glob("Snippets.sln*"))
    run("dotnet", "sln", str(solution), "add", *projects, cwd=root)
    run("dotnet", "restore", str(solution), "--source", str(weather_feed), "--source", str(samples_feed),
        "--packages", str(output / "snippets-installed"), cwd=root)
    run("dotnet", "build", str(solution), "--no-restore", "--nologo", "-v", "quiet", cwd=root)


def qualify(output):
    consume(output, ROOT / "tests/models/error-docs", "error-docs", "Example.ErrorDocs", "ErrorDocumentation.cs",
            (str(ROOT / "tests/conformance/error-docs.json"),))
    # Doc comments and ToString of the generated models; pack runs with -warnaserror, so a malformed comment fails.
    consume(output, ROOT / "tests/models/client-docs-jvm", "display", "Example.Docs", "Display.cs")
    permissions(output)
    policy(output)
    wire = output / "wire"
    generate(ROOT / "tests/models/error-payloads", wire, "Example.Wire", "Example.Wire")
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
        generate(ROOT / "tests/models/payload-bindings", bindings,
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
    generate(ROOT / "tests/models/extended-types", extended, "Example.Types", "Example.Types")
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
    generate(ROOT / "tests/models/weather", package)
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
    check_snippets(output, feed)
    renamed = copy_package("weather", output / "renamed-model")
    replace_once(renamed, "location?: Location\n", "location?: Location\n" + "".join(f"        {name}?: string\n" for name in ["toString", "getType", "equals", "getHashCode"]))
    replace_once(renamed, "    status 409\n", "    status 409\n" + "".join(f"    {name}?: string\n" for name in ["getBaseException", "getObjectData"]))
    rename_shapes(renamed, {"CitySummary": "String", "CitySummaries": "Optional", "ConflictError": "ServiceError", "Location": "UnknownUnion"})
    recursive = output / "recursive-model"
    shutil.copytree(renamed, recursive, dirs_exist_ok=True)
    (recursive / "recursive.cru").write_text(RECURSIVE, encoding="utf-8", newline="\n")
    replace_once(recursive, "        toString?: string\n", "        RecursiveList?: RecursiveList\n        RecursiveMap?: RecursiveMap\n        toString?: string\n")
    for label, model in [("Empty", ROOT / "tests/models/empty-service"), ("Renamed", renamed), ("Recursive", recursive)]:
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
