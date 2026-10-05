"""Native Python adapter for tools/protocol_conformance.py (generated module argument)."""
import importlib
import sys

import httpx
from protocol_conformance import read_json, write_json


class Captured(Exception):
    pass


def main(module_name):
    model = importlib.import_module(module_name + ".models")
    runtime = importlib.import_module(module_name + ".runtime")
    codec = runtime._Codec(model._SCHEMA, model._CLASSES)
    vector = read_json(sys.stdin.read())
    if vector.get("vendorParams") is not None or vector.get("authScheme") is not None:
        return {"unsupported": "Python adapter needs vendor/auth fixture configuration"}

    def native(target, node):
        if node is None or target is None:
            return node
        shape = codec.shapes[target]
        kind = shape["kind"]
        if kind in ("structure", "union"):
            if kind == "union" and any(key not in shape["members"] for key in node):
                return codec.classes[target](_unknown=next(iter(node.items())))
            return codec.classes[target](**{shape["members"][key]["attribute"]:
                native(shape["members"][key]["target"], value) for key, value in node.items()})
        if kind == "list":
            return [native(shape["member"]["target"], value) for value in node]
        if kind == "map":
            return {key: native(shape["value"]["target"], value) for key, value in node.items()}
        return codec.convert(target, node, decode=True, fmt="epoch-seconds" if kind == "timestamp" else None)

    def params(target, value):
        if value is None or target is None:
            return value
        shape = codec.shapes[target]
        kind = shape["kind"]
        if kind in ("structure", "union"):
            result = {}
            for key, member in shape["members"].items():
                item = getattr(value, member["attribute"], runtime.UNSET)
                if not isinstance(item, runtime.Unset):
                    result[key] = params(member["target"], item)
            unknown = getattr(value, "_unknown", runtime.UNSET)
            if not isinstance(unknown, runtime.Unset):
                result[unknown[0]] = unknown[1]
            return result
        if kind == "list":
            return [params(shape["member"]["target"], item) for item in value]
        if kind == "map":
            return {key: params(shape["value"]["target"], item) for key, item in value.items()}
        return codec.convert(target, value, fmt="epoch-seconds" if kind == "timestamp" else None)

    operation = vector["operation"].split("#", 1)[1]
    case = vector["case"]
    if case["kind"] == "request":
        snapshot = {}
        def capture(request):
            raw = request.url.raw_path.decode("ascii")
            path, _, query = raw.partition("?")
            snapshot.update(method=request.method, uri=path, host=request.url.host,
                            queryParams=query.split("&") if query else [], headers=dict(request.headers),
                            body=request.content.decode("utf-8"))
            raise Captured()
        with httpx.Client(transport=httpx.MockTransport(capture)) as http:
            client = runtime.SyncClient(endpoint="https://" + (case.get("host") or "example.test"),
                                        schema=model._SCHEMA, classes=model._CLASSES, http=http)
            try:
                client._call(operation, native(vector["shape"], vector["params"]))
            except Captured:
                return snapshot
        raise AssertionError("native client did not send a request")
    message = case["message"]
    response = httpx.Response(case["code"], headers=message["headers"], content=(message.get("body") or "").encode())
    client = runtime._Base(endpoint="https://example.test", schema=model._SCHEMA, classes=model._CLASSES)
    try:
        value = client._decode(model._SCHEMA["operations"][operation], response, response.content)
        return {"params": params(vector["shape"], value), "error": False}
    except runtime.ServiceError as error:
        return {"params": params(vector["shape"], error), "error": True}


if __name__ == "__main__":
    print(write_json(main(sys.argv[1])))
