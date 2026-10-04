"""Weather plus isolated operations for shared retry qualification."""
import json
from sdk_test_support import ROOT


def qualification_model(output):
    model = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/weather.json").read_text())
    shapes = model["shapes"]
    service = shapes["example.weather#Weather"]
    global_errors = service.pop("errors")
    for shape in shapes.values():
        if shape["type"] == "operation":
            shape.setdefault("errors", []).extend(global_errors)
    for name in ["Ordinary", "Transient", "Throttled"]:
        traits = {"smithy.api#error": "client", "smithy.api#httpError": 429 if name == "Throttled" else 400}
        if name != "Ordinary":
            traits["smithy.api#retryable"] = {"throttling": name == "Throttled"}
        shapes["example.weather#" + name] = {"type": "structure", "traits": traits,
            "members": {"message": {"target": "smithy.api#String"}}}
    for name in ["RetrySafe", "RetryUnsafe"]:
        traits = {"smithy.api#http": {"method": "POST", "uri": "/retry"}}
        if name == "RetrySafe":
            traits["smithy.api#idempotent"] = {}
        shapes["example.weather#" + name] = {"type": "operation", "traits": traits,
            "input": {"target": "smithy.api#Unit"}, "output": {"target": "smithy.api#Unit"},
            "errors": [{"target": "example.weather#" + error} for error in ["Ordinary", "Transient", "Throttled"]]}
        service["operations"].append({"target": "example.weather#" + name})
    path = output / "model.json"
    path.write_text(json.dumps(model), encoding="utf-8")
    return path
