import pathlib
import tempfile
import unittest
import zipfile

import build_traits_jar


class TraitsJar(unittest.TestCase):
    def test_the_jar_lists_and_carries_every_model_file(self):
        with tempfile.TemporaryDirectory() as out:
            jar = build_traits_jar.build(pathlib.Path(out))
            self.assertEqual(jar.name, f"crucible-traits-{build_traits_jar.version()}.jar")
            with zipfile.ZipFile(jar) as archive:
                listed = archive.read("META-INF/smithy/manifest").decode().split()
                self.assertEqual(listed, build_traits_jar.MODELS)
                model = archive.read("META-INF/smithy/crucible.smithy").decode()
                self.assertIn("namespace crucible.protocols", model)
                self.assertIn("structure json {}", model)
                self.assertIn("structure strict {}", model)


if __name__ == "__main__":
    unittest.main()
