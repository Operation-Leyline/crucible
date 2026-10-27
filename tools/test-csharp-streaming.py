"""Pack and install native .NET 8 streaming SDKs; qualify TCP and Rust HTTP."""
import os
from pathlib import Path
import re
import shutil
import tempfile
from sdk_test_support import ROOT, run


def qualify(output):
    fixture = os.environ.get("CRUCIBLE_STREAM_FIXTURE")
    if fixture:
        # The consumers launch it from their own directories.
        fixture = str(Path(fixture).resolve())
    else:
        run("cargo", "build", "--locked", "--manifest-path", "tests/consumers/native-stream-http/Cargo.toml")
        # Cargo resolves a relative CARGO_TARGET_DIR against its cwd, ROOT.
        target = ROOT / os.environ.get("CARGO_TARGET_DIR", "tests/consumers/native-stream-http/target")
        fixture = str(target / "debug" / ("native-stream-http.exe" if os.name == "nt" else "native-stream-http"))
    env = dict(os.environ, CRUCIBLE_STREAM_FIXTURE=fixture,
               CRUCIBLE_STREAM_VECTORS=str(ROOT / "tests/conformance/response-stream.json"))
    for docs in (True, False):
        name = "docs" if docs else "no-docs"
        model = output / (name + "-model")
        shutil.copytree(ROOT / "tests/consumers/rust-stream-client/model", model)
        source = (model / "model.cru").read_text(encoding="utf-8")
        source = source.replace("errors [ValidationError,", "errors [BlobFailure, TimeFailure, ValidationError,")
        source += '''
error BlobFailure {
    status 451
    @payload
    value: blob
}
error TimeFailure {
    status 452
    @header("X-Time")
    value: timestamp
}
'''
        if not docs:
            source = re.sub(r'(?m)^    errorDocs [^\n]*\n', '', source)
        (model / "model.cru").write_text(source, encoding="utf-8")
        sdk = output / (name + "-sdk")
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "csharp",
            str(model), "--out", str(sdk), "--namespace", "Example.Streams", "--package-id", "Example.Streams",
            "--package-version", "0.0.0-test")
        feed = output / (name + "-feed")
        run("dotnet", "pack", "--nologo", "-v", "quiet", "-warnaserror", "-o", str(feed), cwd=sdk)
        consumer = output / (name + "-consumer")
        consumer.mkdir()
        (consumer / "Consumer.csproj").write_text('''<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup>
  <ItemGroup><PackageReference Include="Example.Streams" Version="0.0.0-test"/></ItemGroup>
</Project>''', encoding="utf-8")
        shutil.copyfile(ROOT / "crates/crucible-codegen-csharp/tests/Streaming.cs", consumer / "Program.cs")
        run("dotnet", "restore", "--source", str(feed), "--packages", str(output / (name + "-installed")), cwd=consumer)
        run("dotnet", "build", "--no-restore", "--nologo", "-v", "quiet", "-warnaserror", cwd=consumer)
        run("dotnet", str(consumer / "bin/Debug/net8.0/Consumer.dll"), cwd=consumer, env=env)
        # Compile each generated README snippet against the separately installed package.
        compiled = 0
        for index, code in enumerate(re.findall(r"```csharp\n(.*?)```", (sdk / "README.md").read_text(encoding="utf-8"), re.S)):
            if "using Example.Streams;" not in code:
                continue
            sample = output / f"{name}-snippet-{index}"
            sample.mkdir()
            shutil.copyfile(consumer / "Consumer.csproj", sample / "Snippet.csproj")
            (sample / "Program.cs").write_text(code, encoding="utf-8")
            run("dotnet", "restore", "--source", str(feed), "--packages", str(output / (name + "-installed")), cwd=sample)
            run("dotnet", "build", "--no-restore", "--nologo", "-v", "quiet", "-warnaserror", cwd=sample)
            compiled += 1
        # A guide regression that drops every sample must not pass vacuously.
        if not compiled:
            raise SystemExit(f"no C# README samples were extracted from {sdk.name}")


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    if args.out:
        args.out.mkdir(parents=True)
        qualify(args.out.resolve())
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-csharp-stream-") as directory:
            qualify(Path(directory))
