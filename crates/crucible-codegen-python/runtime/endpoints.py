"""Evaluate the shared endpoint rules IR without reading Smithy traits."""
import ipaddress
import re
from typing import Any
from urllib.parse import quote, urlsplit, urlunsplit

from .query import Query


def host_label(value: Any, subdomains: bool = False) -> bool:
    if not isinstance(value, str) or not value:
        return False
    labels = value.split(".") if subdomains else [value]
    return all(re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?", label) is not None for label in labels)


def parse_url(value: Any) -> dict[str, Any] | None:
    if not isinstance(value, str) or any(ord(char) <= 32 or ord(char) == 127 for char in value):
        return None
    try:
        url = urlsplit(value)
        if url.scheme not in ("http", "https") or not url.hostname or url.username is not None or url.password is not None or "?" in value or "#" in value:
            return None
        port = url.port
        host = url.hostname
        try:
            ipaddress.ip_address(host)
            is_ip = True
        except ValueError:
            is_ip = False
            if not host_label(host.rstrip("."), True):
                return None
        authority = f"[{host}]" if ":" in host else host
        if port is not None and port != (443 if url.scheme == "https" else 80):
            authority += f":{port}"
        path = url.path or "/"
        return {"scheme": url.scheme, "authority": authority, "path": path,
                "normalizedPath": path if path.endswith("/") else path + "/", "isIp": is_ip}
    except ValueError:
        return None


def attribute(value: Any, path: Any) -> Any:
    if isinstance(path, str):
        steps: list[dict[str, Any]] = []
        for part in path.split("."):
            key, marker, index = part.partition("[")
            if key:
                steps.append({"kind": "member", "name": key})
            if marker:
                steps.append({"kind": "index", "index": int(index.removesuffix("]"))})
    else:
        steps = path
    for step in steps:
        if step["kind"] == "member":
            value = value.get(step["name"]) if isinstance(value, dict) else None
        else:
            position = step["index"]
            value = value[position] if isinstance(value, list) and -len(value) <= position < len(value) else None
        if value is None:
            break
    return value


def function(name: str, args: list[Any]) -> Any:
    if name == "isSet":
        return args[0] is not None
    if name == "not":
        if not isinstance(args[0], bool):
            raise ValueError("endpoint not expects a boolean")
        return not args[0]
    if name in ("booleanEquals", "stringEquals"):
        expected = bool if name == "booleanEquals" else str
        return all(isinstance(value, expected) for value in args) and args[0] == args[1]
    if name == "getAttr":
        return attribute(*args)
    if name == "substring":
        text, start, end, reverse = args
        if not isinstance(text, str) or not text.isascii() or not 0 <= start <= end <= len(text):
            return None
        return text[len(text) - end:len(text) - start] if reverse else text[start:end]
    if name == "isValidHostLabel":
        return host_label(*args)
    if name == "parseUrl":
        return parse_url(args[0])
    if name == "uriEncode":
        return quote(args[0], safe="-._~")
    raise ValueError(f"unsupported compiled endpoint function {name}")


def expression(node: dict[str, Any], values: dict[str, Any]) -> Any:
    kind = node["kind"]
    if kind == "reference":
        return values.get(node["name"])
    if kind in ("boolean", "integer"):
        return node["value"]
    if kind == "array":
        return [expression(item, values) for item in node["values"]]
    if kind == "object":
        return {key: expression(item, values) for key, item in node["values"].items()}
    if kind == "call":
        return function(node["function"], [expression(item, values) for item in node["arguments"]])
    if kind == "string":
        parts = []
        for part in node["parts"]:
            value = part["value"] if part["kind"] == "literal" else attribute(values.get(part["name"]), part["path"])
            if value is None or not isinstance(value, (str, bool, int)):
                raise ValueError("endpoint template references a missing or non-scalar parameter")
            parts.append(str(value).lower() if isinstance(value, bool) else str(value))
        return "".join(parts)
    raise ValueError(f"unsupported compiled endpoint expression {kind}")


def rules(nodes: list[dict[str, Any]], parameters: dict[str, Any]) -> dict[str, Any] | None:
    for rule in nodes:
        scope = dict(parameters)
        for condition in rule["conditions"]:
            value = function(condition["function"], [expression(item, scope) for item in condition["arguments"]])
            if value is None or value is False:
                break
            if condition["assign"] is not None:
                scope[condition["assign"]] = value
        else:
            result = rule["result"]
            if result["kind"] == "tree":
                nested = rules(result["rules"], scope)
                if nested is not None:
                    return nested
            elif result["kind"] == "error":
                raise ValueError(expression(result["message"], scope))
            else:
                url = expression(result["url"], scope)
                if parse_url(url) is None:
                    raise ValueError("endpoint rule returned an invalid HTTP(S) URL")
                return {"url": url, "headers": {key: [expression(item, scope) for item in entries] for key, entries in result["headers"].items()},
                        "properties": {key: expression(item, scope) for key, item in result["properties"].items()}}
    return None


def resolve(rule_set: dict[str, Any], sources: dict[str, Any], input: Any,
            client: dict[str, Any], builtins: dict[str, Any]) -> dict[str, Any]:
    parameters = {}
    for name, parameter in rule_set["parameters"].items():
        value = None
        for source in sources.get(name, []):
            kind = source["kind"]
            if kind in ("static", "default"):
                value = source["value"]
            elif kind == "inputMember":
                value = input.get(source["member"]) if isinstance(input, dict) else None
            elif kind == "inputPath":
                value = Query(source["path"]["expression"]).search(input)
            elif kind == "client":
                value = client.get(source["name"])
            elif kind == "builtIn":
                value = builtins.get(source["name"])
            if value is not None:
                break
        if value is None and parameter["required"]:
            raise ValueError(f"required endpoint parameter {name} is missing")
        if value is not None:
            kind = parameter["kind"]
            valid = isinstance(value, str) if kind == "string" else isinstance(value, bool) if kind == "boolean" else isinstance(value, list) and all(isinstance(item, str) for item in value)
            if not valid:
                raise ValueError(f"endpoint parameter {name} has the wrong type")
        parameters[name] = value
    result = rules(rule_set["rules"], parameters)
    if result is None:
        raise ValueError("no endpoint rule matched")
    return result


def host_prefix(url: str, description: dict[str, Any], input: Any) -> str:
    prefix = ""
    for part in description["parts"]:
        if part["kind"] == "literal":
            prefix += part["value"]
        else:
            value = input.get(part["member"]) if isinstance(input, dict) else None
            if not isinstance(value, str) or not host_label(value):
                raise ValueError(f"invalid endpoint host label {part['member']}")
            prefix += value
    parsed = urlsplit(url)
    host = prefix + (parsed.hostname or "")
    if len(host) > 253 or not host_label(host, True):
        raise ValueError("invalid endpoint hostname after prefix expansion")
    authority = host + (f":{parsed.port}" if parsed.port is not None else "")
    return urlunsplit((parsed.scheme, authority, parsed.path, "", ""))
