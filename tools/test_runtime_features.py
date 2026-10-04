import pathlib
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]

# Linked into every service and client a consumer builds. Cargo unifies
# features across that build, so a feature turned on here changes the
# consumer's own code: `preserve_order` would make every `serde_json::Map`
# in the service keep insertion order, and a service that hashes or
# compares JSON maps relies on their sorted order.
RUNTIME = ["crucible", "crucible-wire", "crucible-client"]


def serde_json_features(manifest):
    dependency = manifest.get("dependencies", {}).get("serde_json", {})
    features = set(dependency.get("features", [])) if isinstance(dependency, dict) else set()
    if isinstance(dependency, dict) and dependency.get("workspace"):
        root = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        shared = root["workspace"]["dependencies"]["serde_json"]
        features |= set(shared.get("features", [])) if isinstance(shared, dict) else set()
    return features


class RuntimeFeatures(unittest.TestCase):
    def test_runtime_crates_leave_json_map_order_alone(self):
        for crate in RUNTIME:
            manifest = tomllib.loads(
                (ROOT / "crates" / crate / "Cargo.toml").read_text(encoding="utf-8")
            )
            self.assertNotIn("preserve_order", serde_json_features(manifest), crate)


if __name__ == "__main__":
    unittest.main()
