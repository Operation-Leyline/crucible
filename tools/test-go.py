"""Generate, test over HTTP, and consume a Go SDK from a separate module."""
import os
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[1]
GO = os.environ.get("GO", "go")


def run(*args, **kwargs):
    subprocess.run(args, cwd=kwargs.pop("cwd", ROOT), check=True, **kwargs)


def qualify(output):
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
    run("cargo", "build", "--locked", "-q", "--manifest-path", "e2e/Cargo.toml", "-p", "weather-service")
    target = Path(os.environ.get("CARGO_TARGET_DIR", "e2e/target"))
    if not target.is_absolute():
        target = ROOT / target
    binary = target / "debug" / ("weather-service.exe" if os.name == "nt" else "weather-service")
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


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="crucible-go-") as directory:
        qualify(Path(directory))
