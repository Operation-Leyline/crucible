"""Generate a provider, compile it, and exercise protocol-six schema and HTTP CRUD.

GO overrides the Go executable. TERRAFORM optionally runs an installed Terraform
or OpenTofu binary against the compiled provider through a development override.
All generated output and dependencies stay in the caller's selected target.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path(__file__).resolve().parents[1]
OUT = Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target" / "terraform"))).resolve() / "terraform-provider"
GO = os.environ.get("GO", "go")


def run(*args, cwd=ROOT, env=None):
    subprocess.run(args, cwd=cwd, env=env, check=True)


def qualify_cli(terraform, consumer, env):
    """A real CLI catches plan/state contract mistakes unit CRUD cannot."""
    objects = {}
    calls = {"POST": 0, "PUT": 0, "DELETE": 0}

    class API(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def dispatch(self):
            if self.command in calls:
                calls[self.command] += 1
            identifier = self.path.rsplit("/", 1)[-1]
            if self.command in ("POST", "PUT"):
                data = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
                if self.command == "POST":
                    identifier = "w-" + str(calls["POST"])
                objects[identifier] = dict(data, id=identifier)
                objects[identifier].setdefault("slug", "server-default")
            found = identifier in objects
            body = {"widget": objects[identifier]} if found else {"__type": "NotFound"}
            if self.command == "DELETE" and found:
                del objects[identifier]
                body = {}
            self.send_response(200 if found else 404)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(body).encode())

        do_GET = do_POST = do_PUT = do_DELETE = dispatch

    server = ThreadingHTTPServer(("127.0.0.1", 0), API)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    config = {"terraform": {"required_providers": {"inventory": {"source": "registry.terraform.io/example/inventory"}}},
              "provider": {"inventory": {"endpoint": f"http://127.0.0.1:{server.server_port}"}},
              "resource": {"inventory_widget": {"sample": {"name": "first", "capacity": 3, "enabled": True}}}}

    def write():
        (consumer / "main.tf.json").write_text(json.dumps(config), encoding="utf-8")

    def tf(*args):
        result = subprocess.run([terraform, *args, "-no-color"], cwd=consumer, env=env, capture_output=True, text=True)
        if result.returncode:
            raise RuntimeError(result.stdout + result.stderr)
        return result

    try:
        write()
        tf("apply", "-auto-approve")
        assert calls["POST"] == 1 and len(objects) == 1
        tf("plan", "-detailed-exitcode")
        config["resource"]["inventory_widget"]["sample"]["name"] = "changed"
        write()
        tf("apply", "-auto-approve")
        assert calls["PUT"] == 1 and objects["w-1"]["name"] == "changed"
        tf("plan", "-detailed-exitcode")
        config["resource"]["inventory_widget"]["sample"]["slug"] = "immutable"
        write()
        tf("apply", "-auto-approve")
        assert calls["POST"] == 2 and calls["DELETE"] == 1 and len(objects) == 1
        tf("plan", "-detailed-exitcode")
        tf("destroy", "-auto-approve")
        assert not objects
        print("Terraform CLI create/update/replacement/destroy and no-op plans passed")
    finally:
        server.shutdown()
        server.server_close()
        worker.join()


def main():
    env = dict(os.environ, CRUCIBLE_TERRAFORM_OUT=str(OUT))
    run("cargo", "test", "--locked", "-p", "crucible-codegen-terraform", "--test", "generate", env=env)
    shutil.copyfile(ROOT / "crates/crucible-codegen-terraform/tests/terraform_test.go", OUT / "internal/provider/terraform_test.go")
    run(GO, "mod", "tidy", cwd=OUT)
    run(GO, "fmt", "./...", cwd=OUT)
    run(GO, "vet", "./...", cwd=OUT)
    run(GO, "test", "-count=1", "-v", "./...", cwd=OUT)
    binary_dir = OUT / "bin"
    binary_dir.mkdir(exist_ok=True)
    binary = binary_dir / ("terraform-provider-inventory.exe" if os.name == "nt" else "terraform-provider-inventory")
    run(GO, "build", "-buildvcs=false", "-o", str(binary), ".", cwd=OUT)
    if terraform := os.environ.get("TERRAFORM"):
        config = OUT / "terraform.tfrc"
        config.write_text('provider_installation {\n dev_overrides {\n "registry.terraform.io/example/inventory" = ' + json.dumps(binary_dir.as_posix()) + '\n }\n direct {}\n}\n', encoding="utf-8")
        consumer = OUT / "consumer"
        consumer.mkdir(exist_ok=True)
        (consumer / "main.tf.json").write_text(json.dumps({"terraform": {"required_providers": {"inventory": {"source": "registry.terraform.io/example/inventory"}}}}), encoding="utf-8")
        tfenv = dict(os.environ, TF_CLI_CONFIG_FILE=str(config), TF_IN_AUTOMATION="1", CRUCIBLE_TOKEN="fixture-token")
        schema = subprocess.run([terraform, "providers", "schema", "-json"], cwd=consumer, env=tfenv, check=True, capture_output=True, text=True)
        assert "inventory_widget" in json.loads(schema.stdout)["provider_schemas"]["registry.terraform.io/example/inventory"]["resource_schemas"]
        run(terraform, "validate", cwd=consumer, env=tfenv)
        qualify_cli(terraform, consumer, tfenv)


if __name__ == "__main__":
    main()
