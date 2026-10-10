"""Every client runtime sends the release it was generated for
(docs/api-versions-clients.md).

The release is a service-level value in each client's description: the
bundled runtimes read `apiVersion` from it, TypeScript from each operation's
spec, and the Rust client from its operation marker. The Rust client and
Python are exercised over the wire (e2e/weather-service/tests/api_versions.rs,
crates/crucible-codegen-python/tests/test_runtime.py); this pins that no
runtime is left out, since most run only in their own CI job.
"""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
RUNTIMES = {
    "python": "crates/crucible-codegen-python/runtime/runtime.py",
    "go": "crates/crucible-codegen-go/runtime/runtime.go",
    "java": "crates/crucible-codegen-java/runtime/ClientRuntime.java",
    "csharp": "crates/crucible-codegen-csharp/runtime/ClientRuntime.cs",
    "kotlin": "crates/crucible-codegen-kotlin/runtime/ClientRuntime.kt",
    "swift": "crates/crucible-codegen-swift/runtime/ClientRuntime.swift",
    "dart": "crates/crucible-codegen-dart/runtime/client_runtime.dart",
    "php": "crates/crucible-codegen-php/runtime/ClientRuntime.php",
    "ruby": "crates/crucible-codegen-ruby/runtime/client_runtime.rb",
    "cpp": "crates/crucible-codegen-cpp/runtime/client_runtime.cpp",
    "c": "crates/crucible-codegen-c/runtime/bindings.c",
    "r": "crates/crucible-codegen-r/runtime/runtime.R",
    "typescript": "crates/crucible-codegen-ts/runtime/runtime.ts",
}


class ApiVersionClients(unittest.TestCase):
    def test_every_runtime_sends_the_release(self):
        for language, path in RUNTIMES.items():
            with self.subTest(language=language):
                source = (ROOT / path).read_text(encoding="utf-8")
                self.assertIn("apiVersion", source)
                self.assertIn("crucible-version", source.lower())

    def test_typescript_sets_the_header(self):
        runtime = (ROOT / RUNTIMES["typescript"]).read_text(encoding="utf-8")
        self.assertIn('headers.set("crucible-version", operation.apiVersion)', runtime)

    def test_the_description_and_the_rust_client_carry_it(self):
        schema = (ROOT / "crates/crucible-codegen-common/src/lib.rs").read_text(encoding="utf-8")
        self.assertIn('value["apiVersion"]', schema)
        transport = (ROOT / "crates/crucible-client-runtime-rust/src/transport.rs").read_text(encoding="utf-8")
        self.assertIn("O::API_VERSION", transport)


if __name__ == "__main__":
    unittest.main()
