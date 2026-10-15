"""Add documentation and sensitivity examples only to native qualification models."""
import shutil

from sdk_cru_packages import replace_once
from sdk_conformance_model import qualification_model as base_model
from sdk_test_support import ROOT


def qualification_model(output):
    package = base_model(output)
    shutil.copyfile(ROOT / "tests/conformance/native-docs.cru", package / "native-docs.cru")
    replace_once(package, "expectedRevision?: long\n", "expectedRevision?: long\n        debugModel?: DebugModel\n")
    return package
