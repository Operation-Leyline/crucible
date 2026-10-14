"""Generate, test over HTTP, and consume a Go SDK from a separate module."""
import os
import json
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading
from sdk_test_support import generator_command

ROOT = Path(__file__).resolve().parents[1]
GO = os.environ.get("GO", "go")


def run(*args, **kwargs):
    subprocess.run(generator_command(args), cwd=kwargs.pop("cwd", ROOT), check=True, **kwargs)


def qualify(output):
    for protocol, name in (("crucible.protocols#json", "payloadbindings"),
                           ("aws.protocols#restJson1", "restpayloadbindings")):
        package = output / name
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
            "crates/crucible-codegen-python/tests/payload-bindings.json", "--protocol", protocol,
            "--out", str(package), "--module-path", "example.com/" + name, "--package-name", "payloadbindings")
        shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/payload_bindings_test.go", package / "payload_bindings_test.go")
        run(GO, "fmt", "./...", cwd=package)
        run(GO, "vet", "./...", cwd=package)
        run(GO, "test", "-count=1", "-v", "./...", cwd=package)
    docs = output / "docs"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "tests/fixtures/client-docs.json", "--out", str(docs),
        "--module-path", "example.com/docs", "--package-name", "docs")
    shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/docs_test.go", docs / "docs_test.go")
    run(GO, "fmt", "./...", cwd=docs)
    run(GO, "vet", "./...", cwd=docs)
    run(GO, "test", "-count=1", "-v", "./...", cwd=docs)
    run(GO, "doc", "-all", ".", cwd=docs, stdout=subprocess.DEVNULL)
    checksum = output / "checksum"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "crates/crucible-smithy/tests/fixtures/checksums.json", "--out", str(checksum),
        "--module-path", "example.com/checksum", "--package-name", "checksum")
    shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/checksums_test.go", checksum / "checksums_test.go")
    shutil.copyfile(ROOT / "tests/conformance/checksums.json", checksum / "checksums.json")
    run(GO, "fmt", "./...", cwd=checksum)
    run(GO, "test", "-count=1", "-v", "./...", cwd=checksum)
    protocol_model = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/protocol-json.json").read_text())
    protocol_model["shapes"]["example.json#JsonService"]["rename"] = {"example.json#Problem": "ProblemFault"}
    protocol_path = output / "protocol.json"
    protocol_path.write_text(json.dumps(protocol_model), encoding="utf-8")
    for protocol in ("restJson1", "awsJson1_0", "awsJson1_1"):
        package = output / protocol
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
            str(protocol_path), "--protocol", "aws.protocols#" + protocol,
            "--out", str(package), "--module-path", "example.com/protocol", "--package-name", "protocol")
        shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/protocols_test.go", package / "protocols_test.go")
        run(GO, "fmt", "./...", cwd=package)
        run(GO, "test", "-count=1", "-v", "./...", cwd=package)
    behavior = output / "behavior"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "crates/crucible-smithy/tests/fixtures/client-behaviors.json", "--out", str(behavior),
        "--module-path", "example.com/behavior", "--package-name", "behavior")
    shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/endpoints_test.go", behavior / "endpoints_test.go")
    run(GO, "fmt", "./...", cwd=behavior)
    run(GO, "test", "-count=1", "-v", "./...", cwd=behavior)
    jobs = output / "jobs"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "crates/crucible-codegen-java/tests/waiters.json", "--out", str(jobs),
        "--module-path", "example.com/jobs", "--package-name", "jobs")
    shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/waiters_test.go", jobs / "waiters_test.go")
    run(GO, "fmt", "./...", cwd=jobs)
    run(GO, "test", "-count=1", "-v", "./...", cwd=jobs)
    streaming = output / "streaming"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "crates/crucible-codegen-java/tests/streaming.json", "--out", str(streaming),
        "--module-path", "example.com/stream", "--package-name", "stream")
    shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/streaming_test.go", streaming / "streaming_test.go")
    shutil.copyfile(ROOT / "tests/conformance/checksums.json", streaming / "checksums.json")
    run(GO, "fmt", "./...", cwd=streaming)
    run(GO, "vet", "./...", cwd=streaming)
    run(GO, "test", "-count=1", "-v", "./...", cwd=streaming)
    auth = output / "auth"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "crates/crucible-codegen-python/tests/auth.json", "--out", str(auth),
        "--module-path", "example.com/auth", "--package-name", "auth")
    shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/auth_test.go", auth / "auth_test.go")
    run(GO, "fmt", "./...", cwd=auth)
    run(GO, "test", "-count=1", "-v", "./...", cwd=auth)
    wire = output / "wire"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "crates/crucible-smithy/tests/fixtures/error-payloads.json", "--out", str(wire),
        "--module-path", "example.com/wire", "--package-name", "wire")
    shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/payloads_test.go", wire / "payloads_test.go")
    run(GO, "fmt", "./...", cwd=wire)
    run(GO, "test", "-count=1", "-v", "./...", cwd=wire)
    extended = output / "extended"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "crates/crucible-smithy/tests/fixtures/extended-types.json", "--out", str(extended),
        "--module-path", "example.com/types", "--package-name", "types")
    shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/types_test.go", extended / "types_test.go")
    run(GO, "fmt", "./...", cwd=extended)
    run(GO, "test", "-count=1", "-v", "./...", cwd=extended)
    dynamic = output / "dynamic"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "crates/crucible-smithy/tests/fixtures/dynamic-bindings.json", "--out", str(dynamic),
        "--module-path", "example.com/dynamic", "--package-name", "dynamic")
    shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/dynamic_test.go", dynamic / "dynamic_test.go")
    run(GO, "fmt", "./...", cwd=dynamic)
    run(GO, "test", "-count=1", "-v", "./...", cwd=dynamic)
    policy = output / "policy"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "crates/crucible-smithy/tests/fixtures/runtime-policy.json", "--out", str(policy),
        "--module-path", "example.com/policy", "--package-name", "policy")
    shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/policy_test.go", policy / "policy_test.go")
    for vector in ("retry-quota", "adaptive-retry", "endpoint-discovery", "telemetry"):
        shutil.copyfile(ROOT / "tests/conformance" / f"{vector}.json", policy / f"{vector}.json")
    run(GO, "fmt", "./...", cwd=policy)
    run(GO, "vet", "./...", cwd=policy)
    run(GO, "test", "-count=1", "-v", "./...", cwd=policy)
    # The race detector needs cgo and a C compiler, so it is opt-in.
    if os.environ.get("GO_RACE"):
        run(GO, "test", "-race", "-count=3", "./...", cwd=policy)
    package = output / "sdk"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "crates/crucible-smithy/tests/fixtures/weather.json", "--out", str(package),
        "--module-path", "example.com/weather", "--package-name", "weather",
        "--endpoint", "https://weather.example", "--content-sha256")
    shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/runtime_test.go", package / "runtime_test.go")
    shutil.copyfile(ROOT / "crates/crucible-codegen-go/tests/serialization_test.go", package / "serialization_test.go")
    shutil.copyfile(ROOT / "tests/conformance/serialization.json", package / "serialization.json")
    shutil.copyfile(ROOT / "tests/conformance/retry.json", package / "retry.json")
    shutil.copyfile(ROOT / "tests/conformance/header-lists.json", package / "header-lists.json")
    shutil.copyfile(ROOT / "tests/conformance/malformed-errors.json", package / "malformed-errors.json")
    run(GO, "fmt", "./...", cwd=package)
    run(GO, "vet", "./...", cwd=package)
    if not os.environ.get("WEATHER_BIN"):
        run("cargo", "build", "--locked", "-q", "--manifest-path", "e2e/Cargo.toml", "-p", "weather-service")
    target = Path(os.environ.get("CARGO_TARGET_DIR", "e2e/target"))
    if not target.is_absolute():
        target = ROOT / target
    binary = Path(os.environ["WEATHER_BIN"]) if os.environ.get("WEATHER_BIN") else target / "debug" / ("weather-service.exe" if os.name == "nt" else "weather-service")
    server = subprocess.Popen([str(binary)], cwd=ROOT, stdout=subprocess.PIPE, text=True,
                              creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    try:
        started = queue.Queue()
        def drain():
            started.put(server.stdout.readline().strip())
            for _ in server.stdout:
                pass
        threading.Thread(target=drain, daemon=True).start()
        line = started.get(timeout=30)
        if not line.startswith("listening on "):
            raise RuntimeError("weather service failed to start: " + line)
        env = dict(os.environ, WEATHER_URL="http://" + line.removeprefix("listening on "))
        run(GO, "test", "-count=1", "-v", "./...", cwd=package, env=env)
    finally:
        server.terminate()
        server.wait(timeout=10)
    consumer = output / "consumer"
    consumer.mkdir()
    (consumer / "go.mod").write_text(
        'module example.com/consumer\n\ngo 1.24.0\nrequire example.com/weather v0.0.0\n'
        'replace example.com/weather => ../sdk\n', encoding="utf-8")
    (consumer / "main.go").write_text('''package main
import (
    "context"
    "io"
    "net/http"
    "net/http/httptest"
    "example.com/weather"
)
func main() {
    requests := 0
    server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        requests++
        if r.Method == "POST" {
            io.WriteString(w, `{"city":{"id":"c","name":"consumer","population":0}}`)
        } else {
            if r.URL.Query().Get("maxResults") != "50" { panic("native limit was not bound") }
            io.WriteString(w, `{"cities":[{"id":"c","name":"consumer","population":0}]}`)
        }
    }))
    defer server.Close()
    client, err := weather.NewWeatherClient(weather.Config{Endpoint:server.URL})
    if err != nil { panic(err) }
    input := &weather.CreateCityInput{}
    input.SetName("consumer")
    if input.GetName() != "consumer" { panic("native getter") }
    created, err := client.CreateCity(context.Background(), input)
    if err != nil { panic(err) }
    if created.City.Name != "consumer" { panic("native response") }
    list := &weather.ListCitiesInput{}
    list.SetMaxResults(50)
    list.SetTags([]string{"capital"})
    pages, err := client.ListCitiesPages(list)
    if err != nil { panic(err) }
    if requests != 1 { panic("pagination must be lazy") }
    for pages.HasMorePages() {
        page, err := pages.NextPage(context.Background())
        if err != nil { panic(err) }
        if len(page.Cities) != 1 || page.Cities[0].Name != "consumer" || page.GetNextToken() != "" { panic("native page access") }
    }
    if requests != 2 { panic("unexpected page count") }
}
''', encoding="utf-8")
    run(GO, "run", ".", cwd=consumer)
    empty = output / "empty"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "tests/conformance/empty-service.json", "--out", str(empty),
        "--module-path", "example.com/empty", "--package-name", "empty")
    run(GO, "test", "./...", cwd=empty)
    permissions(output)


def permissions(output):
    """The typed permissions sub-package, from a separate module, as a console would use it."""
    sdk = output / "leyline"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "go",
        "tests/fixtures/permissions", "--out", str(sdk),
        "--module-path", "example.com/leyline", "--package-name", "leyline")
    run(GO, "vet", "./...", cwd=sdk)
    consumer = output / "permissions-consumer"
    consumer.mkdir()
    (consumer / "go.mod").write_text(
        'module example.com/consumer\n\ngo 1.24.0\nrequire example.com/leyline v0.0.0\n'
        'replace example.com/leyline => ../leyline\n', encoding="utf-8")
    (consumer / "main.go").write_text('''package main

import (
	"encoding/json"
	"fmt"
	"reflect"

	"example.com/leyline/permissions"
)

func check(ok bool, what string) {
	if !ok {
		panic(what)
	}
}

// roundTrip decodes a JSON string into a permission and encodes it back.
func roundTrip(text string) string {
	var value permissions.GraphPermission
	if err := json.Unmarshal([]byte(fmt.Sprintf("%q", text)), &value); err != nil {
		panic(err)
	}
	check(string(value) == text, "decoded "+text)
	encoded, err := json.Marshal(value)
	if err != nil {
		panic(err)
	}
	return string(encoded)
}

func main() {
	check(permissions.GraphPermissionRead == "graph:read", "a known constant's value")
	check(roundTrip("graph:export") == `"graph:export"`, "an unknown value round-trips")
	check(roundTrip("organization:read") == `"organization:read"`, "an internal value round-trips")
	check(permissions.GraphPermissionView.Canonical() == permissions.GraphPermissionRead, "an alias's canonical")
	check(permissions.GraphPermissionWrite.Canonical() == permissions.GraphPermissionWrite, "a primary's canonical")
	check(permissions.GraphPermission("graph:export").Canonical() == "graph:export", "an unknown value's canonical")
	check(permissions.HistoryPermissionRestore.Canonical() == permissions.HistoryPermissionRestore, "no aliases")
	var types []string
	var grantable []string
	found := map[string]permissions.PermissionDescription{}
	for _, resource := range permissions.Permissions {
		types = append(types, resource.ResourceType)
		for _, permission := range resource.Permissions {
			found[permission.Value] = permission
			// A grant picker's filter.
			if permission.Grantable && !permission.Deprecated {
				grantable = append(grantable, permission.Value)
			}
		}
	}
	check(reflect.DeepEqual(types, []string{"graph", "history", "org"}), fmt.Sprint("resource types ", types))
	check(reflect.DeepEqual(grantable, []string{"graph:audit", "graph:read", "graph:write", "history:read", "history:restore"}), fmt.Sprint("grantable ", grantable))
	admin := found["organization:admin"]
	check(!admin.Grantable && admin.Tier == "notGrantable", "organization:admin is not grantable")
	view := found["graph:view"]
	check(view.AliasOf == "graph:read" && view.Deprecated, "graph:view is a deprecated alias")
	check(found["graph:read"].AliasOf == "" && found["graph:read"].Tier == "public", "graph:read is a public primary")
	_, internal := found["organization:read"]
	check(!internal, "an internal permission never appears")
	encoded, err := json.Marshal(view)
	if err != nil {
		panic(err)
	}
	check(string(encoded) == `{"value":"graph:view","description":"","tier":"public","grantable":true,"deprecated":true,"aliasOf":"graph:read"}`, string(encoded))
	encoded, err = json.Marshal(admin)
	if err != nil {
		panic(err)
	}
	check(string(encoded) == `{"value":"organization:admin","description":"Everything in the organization.","tier":"notGrantable","grantable":false,"deprecated":false}`, string(encoded))
}
''', encoding="utf-8")
    run(GO, "vet", ".", cwd=consumer)
    run(GO, "run", ".", cwd=consumer)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="crucible-go-") as directory:
        qualify(Path(directory))
