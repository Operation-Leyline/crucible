"""Run lowered Smithy HTTP vectors through a native SDK adapter.

Adapters read one vector on stdin and return a JSON snapshot on stdout. Request
snapshots contain method, uri, host, headers, body and queryParams. Response
snapshots contain params and error. An adapter must explicitly report unsupported
features; those do not count as passing tests.
"""
import argparse
from collections import Counter
from decimal import Decimal
import json
from pathlib import Path
import subprocess
import sys
from urllib.parse import parse_qsl, urlsplit
import xml.etree.ElementTree as ET


def read_json(text):
    def reject_constant(value):
        raise ValueError(f"Invalid JSON number: {value}")
    return json.loads(text, parse_float=Decimal, parse_constant=reject_constant)


def require(condition, message):
    # These are qualification checks, so Python's -O must not disable them.
    if not condition:
        raise AssertionError(message)


def write_json(value):
    if isinstance(value, Decimal):
        if not value.is_finite():
            raise ValueError("JSON numbers must be finite")
        return str(value)
    if isinstance(value, dict):
        return "{" + ",".join(json.dumps(key) + ":" + write_json(item) for key, item in value.items()) + "}"
    if isinstance(value, list):
        return "[" + ",".join(write_json(item) for item in value) + "]"
    return json.dumps(value, allow_nan=False)


def equal(left, right):
    if isinstance(left, dict) and isinstance(right, dict):
        return left.keys() == right.keys() and all(equal(value, right[key]) for key, value in left.items())
    if isinstance(left, list) and isinstance(right, list):
        return len(left) == len(right) and all(equal(a, b) for a, b in zip(left, right))
    if isinstance(left, (int, Decimal)) and not isinstance(left, bool):
        return isinstance(right, (int, Decimal)) and not isinstance(right, bool) and left == right
    return type(left) is type(right) and left == right


def xml_value(element):
    children = tuple(xml_value(child) for child in element)
    element_only = children and not (element.text or "").strip() and all(
        not (child.tail or "").strip() for child in element)
    if element_only:
        # Structure members have no order; stable sorting retains repeated list-item order.
        children = tuple(sorted(children, key=lambda child: child[0]))
    return (element.tag, tuple(sorted(element.attrib.items())),
            element.text if not children else (element.text or "").strip(), children,
            (element.tail or "").strip())


def compare_body(expected, actual, media):
    if expected is None:
        return
    if expected == "":
        require(actual == "", "body differs")
        return
    if media and (media.split(";", 1)[0].endswith("json") or "+json" in media):
        require(equal(read_json(expected), read_json(actual)), "JSON body differs")
    elif media and media.split(";", 1)[0].strip() == "application/x-www-form-urlencoded":
        # Form parameters are compared as a multiset: their order carries no meaning.
        require(Counter(parse_qsl(expected, keep_blank_values=True)) == Counter(parse_qsl(actual, keep_blank_values=True)),
                "form body differs")
    elif media and (media.split(";", 1)[0].endswith("xml") or "+xml" in media):
        require(xml_value(ET.fromstring(expected)) == xml_value(ET.fromstring(actual)), "XML body differs")
    else:
        require(actual == expected, "body differs")


def compare(vector, snapshot):
    case = vector["case"]
    if case["kind"] == "response":
        require(snapshot.get("error", False) == vector["error"], "modeled error outcome differs")
        require(equal(snapshot.get("params"), vector["params"]), "decoded model members differ")
        return
    require(snapshot["method"] == case["method"], "HTTP method differs")
    require(snapshot["uri"] == case["uri"], "URI path differs")
    if case.get("resolvedHost") or case.get("host"):
        endpoint = case.get("resolvedHost") or case["host"]
        require(snapshot["host"] == urlsplit("//" + endpoint).hostname, "host differs")
    actual_query = snapshot.get("queryParams", [])
    def pairs(items):
        return Counter(pair for item in items for pair in parse_qsl(item, keep_blank_values=True))
    actual_pairs = pairs(actual_query)
    expected_pairs = pairs(case.get("queryParams", []))
    require(not expected_pairs - actual_pairs, "expected query parameter/value missing")
    keys = {key for key, _ in actual_pairs}
    require(keys.isdisjoint(case.get("forbidQueryParams", [])), "forbidden query parameter present")
    require(set(case.get("requireQueryParams", [])) <= keys, "required query parameter missing")
    message = case["message"]
    headers = {key.lower(): value for key, value in snapshot.get("headers", {}).items()}
    for key, value in message.get("headers", {}).items():
        require(headers.get(key.lower()) == value, f"header {key} differs")
    require(not {key.lower() for key in message.get("forbidHeaders", [])} & headers.keys(), "forbidden header present")
    require({key.lower() for key in message.get("requireHeaders", [])} <= headers.keys(), "required header missing")
    compare_body(message.get("body"), snapshot.get("body", ""), message.get("bodyMediaType"))


def run(vectors, command, protocol, timeout=30, allow_unsupported=()):
    results = []
    for vector in vectors["tests"]:
        item = {"id": vector["id"], "operation": vector["operation"]}
        if vector.get("appliesTo") == "server" or vector["protocol"] != protocol:
            results.append({**item, "status": "not-applicable"})
            continue
        try:
            result = subprocess.run(command, input=write_json(vector), text=True,
                                    encoding="utf-8", capture_output=True, timeout=timeout, check=True)
            snapshot = read_json(result.stdout)
            if "unsupported" in snapshot:
                results.append({**item, "status": "unsupported", "reason": snapshot["unsupported"]})
                continue
            # Vendor parameters must be acknowledged by the native adapter.
            if vector.get("vendorParams") is not None and not snapshot.get("vendorParamsApplied"):
                results.append({**item, "status": "unsupported", "reason": "vendor parameters not applied"})
                continue
            compare(vector, snapshot)
            results.append({**item, "status": "passed"})
        except (AssertionError, ValueError, KeyError, OSError, subprocess.SubprocessError, ET.ParseError) as error:
            results.append({**item, "status": "failed", "reason": str(error)})
    counts = Counter(item["status"] for item in results)
    report = {"schemaVersion": 1, "service": vectors["service"], "protocol": protocol,
              "complete": counts["passed"] > 0 and counts["failed"] == 0 and counts["unsupported"] == 0,
              "counts": dict(counts), "results": results}
    if allow_unsupported:
        unsupported = {item["id"] for item in results if item["status"] == "unsupported"}
        report["qualifiedSubset"] = (counts["passed"] > 0 and counts["failed"] == 0
                                     and unsupported == set(allow_unsupported))
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("vectors", type=Path)
    parser.add_argument("--protocol", default="crucible.protocols#json")
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=30)
    parser.add_argument("--allow-unsupported", action="append", default=[], metavar="ID",
                        help="explicitly allow this unsupported test ID when qualifying a subset (repeatable)")
    argv = sys.argv[1:]
    split = argv.index("--") if "--" in argv else len(argv)
    args = parser.parse_args(argv[:split])
    command = argv[split + 1:]
    if not command:
        parser.error("supply a native adapter command after --")
    report = run(read_json(args.vectors.read_text(encoding="utf-8")), command, args.protocol,
                 args.timeout, args.allow_unsupported)
    args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["counts"], sort_keys=True))
    return 0 if report.get("qualifiedSubset", report["complete"]) else 1


if __name__ == "__main__":
    sys.exit(main())
