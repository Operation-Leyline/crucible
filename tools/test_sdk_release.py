import json
from pathlib import Path
import tempfile
import unittest

import sdk_release as release


class SdkReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "model.json").write_text('{"smithy":"2.0","shapes":{}}')
        self.config = self.root / "sdk.json"
        self.output = self.root / "out"
        self.value = {"schemaVersion": 1, "service": "example#Service", "model": "model.json",
                      "generator": ["crucible"], "targets": {"python": {
                          "arguments": ["--package-name", "example-client", "--module-name", "example_client"],
                          "build": [["build-package"]], "test": [["test-consumer"]],
                          "artifacts": ["dist/*.whl"], "publish": [["upload", "{artifact}"]]}}}
        self.save()
        self.calls = []

    def save(self):
        self.config.write_text(json.dumps(self.value))

    def runner(self, argv, cwd, variables):
        self.calls.append(release.expand(argv, variables))
        if argv[0] == "build-package":
            (cwd / "dist").mkdir()
            (cwd / "dist/example.whl").write_bytes(b"native-package-fixture")

    def build(self):
        return release.build(self.config, "1.2.3", self.output, self.runner)

    def test_build_verifies_and_publishes_only_explicitly(self):
        manifest = self.build()
        self.assertEqual(manifest, release.verify(self.output))
        self.assertEqual([call[0] for call in self.calls], ["crucible", "build-package", "test-consumer"])
        planned = release.publish(self.output, None, runner=self.runner)
        self.assertEqual(len(planned), 1)
        self.assertEqual(len(self.calls), 3)
        release.publish(self.output, None, True, self.runner)
        self.assertEqual(len(self.calls), 4)
        release.publish(self.output, None, True, self.runner)
        self.assertEqual(len(self.calls), 4)

    def test_every_language_has_a_versioned_generation_plan(self):
        template = self.value["targets"]["python"]
        self.value["targets"] = {name: template for name in release.LANGUAGES}
        self.save()
        plan = release.plan(self.config, "2.0.0-rc.1", self.output)
        self.assertEqual(set(plan["targets"]), release.LANGUAGES)
        for language, target in plan["targets"].items():
            self.assertIn(language, target["generate"])
            self.assertEqual("--package-version" in target["generate"], language not in ("rust", "go"))

    def test_mutated_artifact_or_plan_prevents_publication(self):
        self.build()
        path = self.output / "python/dist/example.whl"
        path.write_bytes(b"modified")
        with self.assertRaisesRegex(ValueError, "changed"):
            release.publish(self.output, None, True, self.runner)
        self.assertEqual(len(self.calls), 3)

    def test_changed_plan_is_rejected(self):
        self.build()
        with (self.output / "plan.json").open("a") as stream:
            stream.write(" ")
        with self.assertRaisesRegex(ValueError, "plan changed"):
            release.verify(self.output)

    def test_missing_artifact_does_not_produce_release_manifest(self):
        self.value["targets"]["python"]["artifacts"] = ["missing/*.whl"]
        self.save()
        with self.assertRaisesRegex(ValueError, "No python artifact"):
            self.build()
        self.assertFalse((self.output / "release.json").exists())

    def test_path_escape_and_output_override_rejected(self):
        self.value["targets"]["python"]["arguments"] = ["--out=elsewhere"]
        self.save()
        with self.assertRaisesRegex(ValueError, "release owns"):
            self.build()
        self.value["targets"]["python"]["arguments"] = []
        self.value["targets"]["python"]["artifacts"] = ["../secret"]
        self.save()
        with self.assertRaisesRegex(ValueError, "escapes"):
            self.build()

    def test_failed_publication_resumes_after_successful_steps(self):
        self.value["targets"]["python"]["publish"] = [["upload", "{artifact}"], ["index"]]
        self.save()
        self.build()
        def fail_index(argv, cwd, variables):
            if argv[0] == "index":
                raise RuntimeError("temporary registry outage")
            self.runner(argv, cwd, variables)
        with self.assertRaises(RuntimeError):
            release.publish(self.output, None, True, fail_index)
        release.publish(self.output, None, True, self.runner)
        self.assertEqual([c[0] for c in self.calls].count("upload"), 1)
        self.assertEqual([c[0] for c in self.calls].count("index"), 1)

    def test_output_is_never_overwritten(self):
        self.build()
        with self.assertRaisesRegex(ValueError, "fresh release directory"):
            self.build()

    def test_native_code_and_environment_variables_are_not_interpolated(self):
        text = "print({'key': '${TOKEN}', 'version': '{version}'})"
        self.assertEqual(release.expand([text], {"version": "1.2.3"}),
                         ["print({'key': '${TOKEN}', 'version': '1.2.3'})"])


if __name__ == "__main__":
    unittest.main()
