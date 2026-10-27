from pathlib import Path
import tempfile
import unittest

import check_publish_metadata as check


def package(name, deps=(), **overrides):
    base = {
        "name": name,
        "publish": None,
        "description": "d",
        "license": "Apache-2.0",
        "license_file": None,
        "repository": "https://example.invalid/r",
        "readme": None,
        "keywords": ["a"],
        "categories": ["development-tools"],
        "manifest_path": "/nonexistent/Cargo.toml",
        "dependencies": list(deps),
    }
    base.update(overrides)
    return base


def path_dep(name, req="^0.4.0", kind=None):
    return {"name": name, "path": f"../{name}", "req": req, "kind": kind}


class ProblemsTest(unittest.TestCase):
    def test_complete_crate_with_readme_missing_only_reports_readme(self):
        self.assertEqual(check.problems(package("a"), {"a": True}), ["missing readme"])

    def test_publish_false_license_and_metadata_are_reported(self):
        found = check.problems(
            package("a", publish=[], license=None, description=None, keywords=[], categories=[]),
            {"a": False},
        )
        self.assertIn("publish = false (workspace or crate manifest)", found)
        self.assertIn("missing description", found)
        self.assertIn("missing license (SPDX expression) or license-file", found)
        self.assertIn("missing keywords", found)
        self.assertIn("missing categories", found)

    def test_registry_limits_on_keywords(self):
        found = check.problems(package("a", keywords=list("abcdef")), {"a": True})
        self.assertIn("more than 5 keywords", found)

    def test_path_dependency_needs_version_and_a_publishable_target(self):
        crate = package("a", deps=[path_dep("b", req="*"), path_dep("c"), path_dep("ext")])
        found = check.problems(crate, {"a": True, "b": True, "c": False})
        self.assertIn("path dependency b has no version requirement", found)
        self.assertIn("depends on unpublishable workspace crate c", found)
        self.assertIn("path dependency ext is outside the workspace", found)

    def test_packaged_readme_links_must_be_absolute(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "README.md").write_text(
                "[a](docs/a.md) [b](https://example.invalid/b) [c](#c) ![d](img/d.png) [a](docs/a.md)\n"
                "[ref]: docs/ref.md\n[ok]: https://example.invalid/ok\n"
                "[angled-ok]: <https://example.invalid/angled>\n[angled]: <docs/angled.md>\n"
                '<img src="img/logo.svg"> <a href="https://example.invalid/x">x</a> <a href=\'#top\'>t</a>\n',
                encoding="utf-8")
            crate = package("a", readme="README.md", manifest_path=str(root / "Cargo.toml"))
            self.assertEqual(check.problems(crate, {"a": True}), [
                "readme has a relative link: docs/a.md",
                "readme has a relative link: docs/angled.md",
                "readme has a relative link: docs/ref.md",
                "readme has a relative link: img/d.png",
                "readme has a relative link: img/logo.svg",
            ])

    def test_dev_dependencies_do_not_block_publishing(self):
        crate = package("a", deps=[path_dep("c", req="*", kind="dev")])
        self.assertEqual(check.problems(crate, {"a": True, "c": False}), ["missing readme"])


class OrderTest(unittest.TestCase):
    def test_dependencies_come_first_and_ties_sort_by_name(self):
        packages = [
            package("top", deps=[path_dep("mid"), path_dep("leaf-b")]),
            package("mid", deps=[path_dep("leaf-a")]),
            package("leaf-b"),
            package("leaf-a"),
        ]
        self.assertEqual(check.publish_order(packages), ["leaf-a", "leaf-b", "mid", "top"])

    def test_dev_dependency_cycles_are_allowed(self):
        packages = [
            package("a", deps=[path_dep("b", kind="dev")]),
            package("b", deps=[path_dep("a")]),
        ]
        self.assertEqual(check.publish_order(packages), ["a", "b"])

    def test_normal_cycles_are_rejected(self):
        packages = [package("a", deps=[path_dep("b")]), package("b", deps=[path_dep("a")])]
        with self.assertRaises(ValueError):
            check.publish_order(packages)


if __name__ == "__main__":
    unittest.main()
