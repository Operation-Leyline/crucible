"""Build `.cru` packages for the SDK qualification gates from the shared test models."""
import re
import shutil
from pathlib import Path

from sdk_test_support import ROOT

MODELS = ROOT / "tests/models"

MANIFEST = """schema_version = 1

[model]
namespace = "{namespace}"
language = 1
sources = ["."]
imports = []

[model.naming]
input = "{{Operation}}Input"
output = "{{Operation}}Output"
"""


def copy_package(name, destination):
    """A fresh copy of `tests/models/<name>` at `destination`, free to edit."""
    destination = Path(destination)
    if destination.exists():
        shutil.rmtree(destination)
    shutil.copytree(MODELS / name, destination)
    return destination


def write_package(directory, namespace, **files):
    """A package of `files` (file name to source text) in `namespace`."""
    directory = Path(directory)
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    (directory / "crucible.toml").write_text(MANIFEST.format(namespace=namespace), encoding="utf-8", newline="\n")
    for name, text in files.items():
        (directory / name).write_text(text, encoding="utf-8", newline="\n")
    return directory


def edit_files(package, edit):
    """Rewrite every `.cru` source of `package` through `edit(path, text)`."""
    for path in sorted(Path(package).rglob("*.cru")):
        text = path.read_text(encoding="utf-8")
        edited = edit(path, text)
        if edited != text:
            path.write_text(edited, encoding="utf-8", newline="\n")


def replace_once(package, old, new):
    """Replace `old` with `new` in the one source of `package` that holds it."""
    found = []

    def edit(path, text):
        if old in text:
            found.append(path)
            return text.replace(old, new, 1)
        return text

    edit_files(package, edit)
    if not found:
        raise RuntimeError(f"{old!r} is in no source of {package}")


def add_operations(package, *names):
    """Add operations to the service's `operations [..]` list."""
    def edit(path, text):
        return re.sub(r"(\n    operations \[)([^\]]*)\]", lambda m: m.group(1) + m.group(2) + "".join(", " + n for n in names) + "]", text, count=1)

    edit_files(package, edit)


def rename_shapes(package, renames):
    """Spell shapes differently in the generated SDK: rename them whole-word in every source."""
    def edit(path, text):
        for old, new in renames.items():
            text = re.sub(r"\b" + re.escape(old) + r"\b", new, text)
        return text

    edit_files(package, edit)
