"""Add documentation and sensitivity examples only to native qualification models."""
import json
from sdk_test_support import ROOT
from sdk_conformance_model import qualification_model as base_model


def qualification_model(output):
    path = base_model(output)
    model = json.loads(path.read_text(encoding="utf-8"))
    model["shapes"].update(json.loads((ROOT / "tests/conformance/native-docs.json").read_text(encoding="utf-8")))
    model["shapes"]["example.weather#CreateCityInput"]["members"]["debugModel"] = {"target": "example.weather#DebugModel"}
    path.write_text(json.dumps(model), encoding="utf-8")
    return path
