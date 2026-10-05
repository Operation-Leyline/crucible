"""Qualify packed jsii constructs through independent native CDK consumers.

Default: TypeScript, JavaScript, Python. Select additional native toolchains with
--language java/dotnet/go, or --language all. Every run generates all four jsii
bindings; selected consumers must build and produce the same template. Missing
toolchains fail, rather than silently reducing coverage.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "crates/crucible-codegen-cdk/tests/fixtures"
CONSUMERS = ROOT / "tests/cdk"


def run(*args, cwd=ROOT, capture=False):
    print("+", " ".join(str(arg) for arg in args), flush=True)
    result = subprocess.run([str(arg) for arg in args], cwd=cwd, check=True,
                            text=True, capture_output=capture, timeout=1200)
    return result.stdout if capture else None


def tool(name, fallback):
    value = os.environ.get(name, fallback)
    found = shutil.which(value)
    if found:
        return str(Path(found).resolve())
    path = Path(value)
    if path.is_file():
        return str(path.resolve())
    raise RuntimeError(f"Required tool {value!r} not found; set {name} to its executable")


def copy(name, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(CONSUMERS / name, destination)


def prepare(work, npm, node):
    package = work / "package"
    cli = os.environ.get("CRUCIBLE_BIN")
    command = [str(Path(cli).resolve())] if cli else ["cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--"]
    run(*command, "generate", "cdk", FIXTURES / "model.json", "--bindings", FIXTURES / "bindings.json",
        "--config", FIXTURES / "options.json", "--out", package)
    run(npm, "install", "--ignore-scripts", "--no-audit", "--no-fund", cwd=package)
    run(npm, "run", "build", cwd=package)
    packed = json.loads(run(npm, "pack", "--ignore-scripts", "--json", capture=True, cwd=package))
    tarball = package / packed[0]["filename"]
    # A version is deliberately reused by fixtures. A content-addressed path
    # prevents npm's local tarball cache from qualifying an earlier package.
    artifact = work / (hashlib.sha256(tarball.read_bytes()).hexdigest() + ".tgz")
    shutil.copyfile(tarball, artifact)
    assembly = json.loads((package / ".jsii").read_text(encoding="utf-8"))
    assert set(assembly["targets"]) >= {"python", "java", "dotnet", "go"}
    run(node, package / "node_modules/jsii-pacmak/bin/jsii-pacmak", "--code-only", "--force",
        "--validate-assemblies", "--outdir", work / "bindings", cwd=package)
    for language in ["python", "java", "dotnet", "go"]:
        assert (work / "bindings" / language).is_dir(), f"Missing {language} package"
    return artifact


def typescript(work, tarball, npm, node):
    consumer = work / "typescript"
    consumer.mkdir(exist_ok=True)
    package = {"name": "independent-cdk-consumer", "version": "0.0.0", "private": True,
               "dependencies": {"@example/weather-cdk": "file:" + tarball.as_posix(),
                                "aws-cdk-lib": "2.272.0", "constructs": "10.8.1"},
               "devDependencies": {"typescript": "~5.9.0", "@types/node": "^22.0.0"}}
    (consumer / "package.json").write_text(json.dumps(package), encoding="utf-8")
    copy("consumer.ts", consumer / "consumer.ts")
    copy("consumer.js", consumer / "javascript.cjs")
    run(npm, "install", "--ignore-scripts", "--no-audit", "--no-fund", cwd=consumer)
    run(node, consumer / "node_modules/typescript/bin/tsc", "--strict", "--skipLibCheck", "--target", "ES2020",
        "--module", "commonjs", "consumer.ts", cwd=consumer)
    run(node, "consumer.js", cwd=consumer)
    run(node, "javascript.cjs", cwd=consumer)
    return json.loads((consumer / "template.json").read_text(encoding="utf-8"))


def python(work):
    consumer = work / "python"
    consumer.mkdir(exist_ok=True)
    venv = consumer / "venv"
    run(sys.executable, "-m", "venv", venv)
    executable = venv / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    run(executable, "-m", "pip", "install", "--disable-pip-version-check",
        work / "bindings/python", "aws-cdk-lib==2.272.0", "constructs==10.8.1")
    copy("consumer.py", consumer / "consumer.py")
    run(executable, "consumer.py", cwd=consumer)
    return consumer


def java(work):
    mvn = tool("MVN", "mvn.cmd" if os.name == "nt" else "mvn")
    generated = work / "bindings/java"
    run(mvn, "-q", "-DskipTests", "install", cwd=generated)
    consumer = work / "java"
    copy("Consumer.java", consumer / "src/main/java/Consumer.java")
    (consumer / "pom.xml").write_text('''<project xmlns="http://maven.apache.org/POM/4.0.0">
<modelVersion>4.0.0</modelVersion><groupId>test</groupId><artifactId>consumer</artifactId><version>1</version>
<properties><maven.compiler.release>17</maven.compiler.release></properties>
<dependencies><dependency><groupId>com.example</groupId><artifactId>weather-cdk</artifactId><version>0.0.0</version></dependency>
<dependency><groupId>software.amazon.awscdk</groupId><artifactId>aws-cdk-lib</artifactId><version>2.272.0</version></dependency></dependencies>
<build><plugins><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-compiler-plugin</artifactId><version>3.14.0</version></plugin>
<plugin><groupId>org.codehaus.mojo</groupId><artifactId>exec-maven-plugin</artifactId><version>3.5.0</version></plugin></plugins></build></project>''', encoding="utf-8")
    run(mvn, "-q", "compile", "exec:java", "-Dexec.mainClass=Consumer", cwd=consumer)
    return consumer


def dotnet(work):
    executable = tool("DOTNET", "dotnet")
    projects = list((work / "bindings/dotnet").rglob("*.csproj"))
    assert len(projects) == 1, projects
    packages = work / "nuget"
    run(executable, "build", projects[0], "--configuration", "Release")
    run(executable, "pack", projects[0], "--no-build", "--output", packages, "--configuration", "Release")
    archive = packages / "Example.Weather.0.0.0.nupkg"
    cache = work / ("nuget-cache-" + hashlib.sha256(archive.read_bytes()).hexdigest()[:16])
    consumer = work / "dotnet"
    consumer.mkdir(exist_ok=True)
    framework = os.environ.get("CDK_DOTNET_FRAMEWORK", "net8.0")
    (consumer / "Consumer.csproj").write_text(f'''<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup>
<OutputType>Exe</OutputType><TargetFramework>{escape(framework)}</TargetFramework><RestoreAdditionalProjectSources>{escape(packages.as_posix())}</RestoreAdditionalProjectSources><RestorePackagesPath>{escape(cache.as_posix())}</RestorePackagesPath>
</PropertyGroup><ItemGroup><PackageReference Include="Example.Weather" Version="0.0.0" />
<PackageReference Include="Amazon.CDK.Lib" Version="2.272.0" /></ItemGroup></Project>''', encoding="utf-8")
    copy("Consumer.cs", consumer / "Consumer.cs")
    run(executable, "run", "--project", consumer / "Consumer.csproj", cwd=consumer)
    return consumer


def go(work):
    executable = tool("GO", "go")
    modules = list((work / "bindings/go").rglob("go.mod"))
    assert len(modules) == 1, modules
    generated = modules[0].parent
    consumer = work / "go"
    consumer.mkdir(exist_ok=True)
    copy("main.go", consumer / "main.go")
    (consumer / "go.mod").write_text(f'''module qualification

go 1.23.0

require example.com/weather-cdk/weathercdk v0.0.0

replace example.com/weather-cdk/weathercdk => {json.dumps(generated.as_posix())}
''', encoding="utf-8")
    run(executable, "mod", "tidy", cwd=consumer)
    run(executable, "run", ".", cwd=consumer)
    return consumer


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--language", action="append", choices=["typescript", "python", "java", "dotnet", "go", "all"])
    parser.add_argument("--work-dir", type=Path, default=ROOT / "target/cdk-consumer")
    args = parser.parse_args()
    languages = args.language or ["typescript", "python"]
    if "all" in languages:
        languages = ["typescript", "python", "java", "dotnet", "go"]
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    npm = tool("NPM", "npm.cmd" if os.name == "nt" else "npm")
    node = tool("NODE", "node")
    os.environ["PATH"] = str(Path(node).parent) + os.pathsep + os.environ.get("PATH", "")
    os.environ.setdefault("CARGO_TARGET_DIR", str(ROOT / "target/cdk-consumer-rust"))
    os.environ.setdefault("CARGO_BUILD_JOBS", "2")
    tarball = prepare(work, npm, node)
    # TypeScript is the synthesis oracle, even for a one-language matrix job.
    expected = typescript(work, tarball, npm, node)
    for language in languages:
        if language == "typescript":
            continue
        consumer = globals()[language](work)
        actual = json.loads((consumer / "template.json").read_text(encoding="utf-8-sig"))
        assert actual == expected, f"{language} synthesized a different resource contract: {actual!r}"
        print(f"{language}: native consumer matches TypeScript resource contract", flush=True)
    print("CDK qualification passed: jsii assembly, all binding sources, TypeScript/JavaScript and " + ", ".join(languages), flush=True)


if __name__ == "__main__":
    main()
