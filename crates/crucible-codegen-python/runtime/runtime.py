"""Crucible JSON protocol runtime, bundled with its generated client."""
from __future__ import annotations

import asyncio
import base64
import binascii
import copy
import inspect
import ipaddress
import json
import math
import re
import time
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from decimal import Decimal
from email.utils import format_datetime, parsedate_to_datetime
from enum import IntEnum, StrEnum
from typing import Any, Callable, Self
from urllib.parse import parse_qsl, quote, urlencode, urlsplit

import httpx

from .auth import apply_auth, preset_auth
from .policy import RetryStrategy, SystemClock
from .telemetry import (REDACTED, AttemptFinished, AttemptStarted, OperationFinished, OperationStarted,
                        TelemetryHook, emit, redact_headers, redact_query)


class Unset:
    """An omitted member, distinct from an explicit JSON null."""
    def __repr__(self) -> str:
        return "UNSET"

    def __copy__(self) -> Unset:
        return self

    def __deepcopy__(self, memo: Any) -> Unset:
        return self


UNSET = Unset()


def model_repr(schema: dict[str, Any], target: str, value: Any) -> str:
    """Format modeled values recursively without reading sensitive members.

    Omitted members are skipped, null is shown as None and cycles as <recursive>.
    Unknown union payloads are redacted because
    an older client cannot know their sensitivity.
    """
    active: set[int] = set()

    def render(target: str, value: Any, sensitive: bool = False) -> str:
        shape = schema["shapes"][target]
        if sensitive or shape.get("sensitive"):
            return "*** Sensitive Data Redacted ***"
        if value is None:
            return "None"
        kind = shape["kind"]
        if kind not in ("structure", "union", "list", "map"):
            return repr(value)
        if id(value) in active:
            return "<recursive>"
        active.add(id(value))
        try:
            if kind in ("structure", "union"):
                parts = []
                for name, member in shape["members"].items():
                    native = member["attribute"]
                    # Presence may be checked, but sensitive values are never formatted.
                    item = value.get(native, UNSET) if isinstance(value, dict) else getattr(value, native, UNSET)
                    if not isinstance(item, Unset):
                        parts.append(native + "=" + render(member["target"], item, member.get("sensitive", False)))
                if kind == "union" and not isinstance(getattr(value, "_unknown", UNSET), Unset):
                    parts.append("_unknown=*** Sensitive Data Redacted ***")
                return shape["name"] + "(" + ", ".join(parts) + ")"
            if kind == "list":
                member = shape["member"]
                return "[" + ", ".join(render(member["target"], item, member.get("sensitive", False)) for item in value) + "]"
            key, member = shape["key"], shape["value"]
            return "{" + ", ".join(render(key["target"], k, key.get("sensitive", False)) + ": " + render(member["target"], v, member.get("sensitive", False)) for k, v in value.items()) + "}"
        finally:
            active.remove(id(value))

    return render(target, value)


class OpenEnum(StrEnum):
    """String enum that preserves unknown server values as UNKNOWN members."""
    @classmethod
    def _missing_(cls, value: Any) -> OpenEnum | None:
        if not isinstance(value, str):
            return None
        unknown = str.__new__(cls, value)
        unknown._name_ = "UNKNOWN"
        unknown._value_ = value
        return unknown


class OpenIntEnum(IntEnum):
    """Signed 32-bit enum that preserves unknown server values."""
    @classmethod
    def _missing_(cls, value: Any) -> OpenIntEnum | None:
        if isinstance(value, bool) or not isinstance(value, int) or not -(2 ** 31) <= value < 2 ** 31:
            return None
        unknown = int.__new__(cls, value)
        unknown._name_ = "UNKNOWN"
        unknown._value_ = value
        return unknown


@dataclass(frozen=True, kw_only=True)
class PermissionDescription:
    """One permission in a client's permission registry (`permissions.PERMISSIONS`)."""
    value: str
    """What grants store."""
    description: str
    """The model's documentation for it, empty when it has none."""
    tier: str
    """The tier: "public", "permissionOnly" or "notGrantable"; a plain string, since a newer server may add tiers."""
    grantable: bool
    """False for a bundle the service confers whole: never offer it in a picker."""
    deprecated: bool
    alias_of: str | None = None
    """For a deprecated alias, the value it stands for."""


@dataclass(frozen=True, kw_only=True)
class ResourcePermissions:
    """One resource type's permissions, sorted by value."""
    resource_type: str
    permissions: tuple[PermissionDescription, ...]


def _json(value: Any) -> str:
    """Write Decimal as a JSON number without passing through binary floats."""
    if isinstance(value, Decimal):
        if not value.is_finite():
            raise ValueError("non-finite decimal is not a JSON number")
        return str(value)
    if isinstance(value, dict):
        if any(not isinstance(key, str) for key in value):
            raise TypeError("JSON object keys must be strings")
        return "{" + ",".join(json.dumps(key, ensure_ascii=False) + ":" + _json(item) for key, item in value.items()) + "}"
    if isinstance(value, (list, tuple)):
        return "[" + ",".join(_json(item) for item in value) + "]"
    return json.dumps(value, ensure_ascii=False, allow_nan=False, separators=(",", ":"))


@dataclass(frozen=True)
class ResponseMetadata:
    """HTTP status, request identifier, headers and buffered body retained from a response."""
    status: int
    request_id: str | None
    headers: httpx.Headers
    body: bytes = b""


class ServiceError(Exception):
    """Error response with name, message, metadata, retry classification and decoded modeled fields."""
    def __init__(self, *, name: str, message: str, metadata: ResponseMetadata,
                 retryable: bool = False, throttling: bool = False,
# BEGIN ERROR DOCUMENTATION
                 doc_url: str | None = None,
# END ERROR DOCUMENTATION
                 fields: dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.name = name
        self.message = message
        self.metadata = metadata
        self.retryable = retryable
        self.throttling = throttling
        self.fields = fields or {}
# BEGIN ERROR DOCUMENTATION
        # Selected from generated metadata, never from the response's fields.
        self.doc_url = doc_url
# END ERROR DOCUMENTATION

    def __getattr__(self, name: str) -> Any:
        try:
            return self.fields[name]
        except KeyError:
            raise AttributeError(name) from None

    def __repr__(self) -> str:
        """Redact unknown response fields; generated errors provide modeled reprs."""
        return type(self).__name__ + "(message=*** Sensitive Data Redacted ***)"

    def __str__(self) -> str:
        """Use the same redaction as repr for error text."""
        return repr(self)


class ServiceException(ServiceError):
    """Base for every error response from this service."""


class NetworkError(Exception):
    """Transport failed before a complete response could be read."""


def _has_body(op: dict[str, Any]) -> bool:
    """Whether a successful response carries a body to read."""
    response = op["response"]
    return bool(response["body"]) or response.get("payload") is not None


def _valid_endpoint(value: Any) -> bool:
    """Whether the value is an absolute HTTP(S) base URL with a host and no credentials, query or fragment."""
    if not isinstance(value, str) or any(ord(char) <= 32 or ord(char) == 127 for char in value):
        return False
    try:
        url = urlsplit(value)
        url.port
    except ValueError:
        return False
    return (url.scheme in ("http", "https") and bool(url.hostname) and url.username is None
            and url.password is None and "?" not in value and "#" not in value)


def _check_credentials(request: httpx.Request) -> None:
    if ("authorization" not in request.headers and not request.extensions.get("crucible.credentials")) or request.url.scheme == "https":
        return
    host = request.url.host
    try:
        loopback = ipaddress.ip_address(host).is_loopback
    except ValueError:
        loopback = host == "localhost"
    if not loopback:
        raise ValueError("refusing to send credentials over plaintext HTTP outside loopback")


def _timestamp(value: Any, fmt: str, decode: bool) -> Any:
    if decode:
        if fmt == "epoch-seconds":
            return datetime.fromtimestamp(float(value), timezone.utc)
        if fmt == "http-date":
            return parsedate_to_datetime(value).astimezone(timezone.utc)
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
        # An offset-less date-time is UTC, and must stay aware to be sent back.
        return parsed.replace(tzinfo=timezone.utc) if parsed.tzinfo is None else parsed
    if not isinstance(value, datetime) or value.tzinfo is None:
        raise TypeError("timestamps must be timezone-aware datetime values")
    value = value.astimezone(timezone.utc)
    if fmt == "epoch-seconds":
        return value.timestamp()
    if fmt == "http-date":
        return format_datetime(value, usegmt=True)
    return value.isoformat().replace("+00:00", "Z")


def _blob(value: Any, decode: bool) -> Any:
    """Bytes as standard, padded base64 in JSON. A payload is already bytes."""
    if decode:
        if isinstance(value, bytes):
            return value
        try:
            return base64.b64decode(value, validate=True)
        except (binascii.Error, TypeError, ValueError) as error:
            raise ValueError("a blob must be base64") from error
    if not isinstance(value, (bytes, bytearray, memoryview)):
        raise TypeError("blobs must be bytes")
    return base64.b64encode(value).decode("ascii")


_HTTP_DATE_SEPARATOR = re.compile(r",\s*(?=(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun),)")


class _Codec:
    def __init__(self, schema: dict[str, Any], classes: dict[str, Any]) -> None:
        self.schema = schema
        self.shapes = schema["shapes"]
        self.classes = classes

    def convert(self, target: str | None, value: Any, *, decode: bool = False,
                fmt: str | None = None, strict: bool = False) -> Any:
        if target is None:
            return None
        shape = self.shapes[target]
        if shape["kind"] == "unit" and not isinstance(value, Unset):
            if strict and value is not None and (not isinstance(value, dict) or value):
                raise ValueError("unit requires an empty object")
            return None if decode else {}
        if value is None or isinstance(value, Unset):
            return value
        kind = shape["kind"]
        if strict:
            if kind == "string" and not isinstance(value, str):
                raise ValueError("string requires text")
            if kind == "boolean" and not isinstance(value, bool):
                raise ValueError("boolean requires bool")
            if kind in ("float", "double") and (isinstance(value, bool) or not (
                    isinstance(value, (int, float, Decimal))
                    or isinstance(value, str) and value in ("NaN", "Infinity", "-Infinity"))):
                raise ValueError("float requires a number or a modeled nonfinite literal")
            if kind == "list" and not isinstance(value, list):
                raise ValueError("list requires an array")
            if kind == "map" and not isinstance(value, dict):
                raise ValueError("map requires an object")
        if kind == "timestamp":
            return _timestamp(value, fmt or shape["format"], decode)
        if kind == "blob":
            return _blob(value, decode)
        if kind == "enum":
            return self.classes[target](value) if decode else str(value)
        if kind == "intEnum":
            if isinstance(value, bool) or not isinstance(value, int) or not -(2 ** 31) <= value < 2 ** 31:
                raise ValueError("integer enum requires a signed 32-bit integer")
            return self.classes[target](value) if decode else int(value)
        if kind == "bigInteger":
            if isinstance(value, bool) or not isinstance(value, int):
                raise ValueError("bigInteger requires an integer")
            return value
        if kind == "bigDecimal":
            if isinstance(value, bool) or not isinstance(value, (Decimal, int)):
                raise ValueError("bigDecimal requires Decimal or an exact integer")
            decimal = Decimal(value)
            if not decimal.is_finite():
                raise ValueError("non-finite decimal")
            return decimal
        if kind == "list":
            member = shape["member"]
            if not shape.get("sparse", False) and any(v is None for v in value):
                raise ValueError("Null element in non-sparse list")
            converted = [self.convert(member["target"], v, decode=decode, fmt=member["format"], strict=strict) for v in value]
            if shape.get("set", False) and any(item in converted[:index] for index, item in enumerate(converted)):
                raise ValueError("duplicate element in set")
            return converted
        if kind == "map":
            member = shape["value"]
            if not shape.get("sparse", False) and any(v is None for v in value.values()):
                raise ValueError("Null value in non-sparse map")
            return {self.convert(shape["key"]["target"], k, decode=decode, strict=strict): self.convert(member["target"], v, decode=decode, fmt=member["format"], strict=strict) for k, v in value.items()}
        if kind in ("structure", "union"):
            out = self.members(shape, value, decode=decode, strict=strict)
            return self.classes[target](**out) if decode else out
        if kind in ("byte", "short", "integer", "long"):
            bits = {"byte": 8, "short": 16, "integer": 32, "long": 64}[kind]
            if isinstance(value, bool) or not isinstance(value, int) or not -(2 ** (bits - 1)) <= value < 2 ** (bits - 1):
                raise ValueError(f"{kind} requires a signed {bits}-bit integer")
        if kind in ("float", "double"):
            if isinstance(value, Decimal):
                return float(value)
        return value

    def members(self, shape: dict[str, Any], value: Any, *, decode: bool,
                strict: bool = False) -> dict[str, Any]:
        """Decode the shared structure/union members before constructing its model."""
        kind = shape["kind"]
        if decode and not isinstance(value, dict):
            raise ValueError(f"expected an object for {shape['name']}")
        out: dict[str, Any] = {}
        for key, m in shape["members"].items():
            native = m["attribute"]
            item = value.get(key, UNSET) if isinstance(value, dict) else getattr(value, native, UNSET)
            if isinstance(item, Unset) and decode and m["default"] is not None:
                item = copy.deepcopy(m["default"])
            if isinstance(item, Unset):
                if kind == "structure" and m.get("optional") is False:
                    raise ValueError(f"missing required member: {key}")
                continue
            if (kind == "structure" and item is None and m.get("optional") is False
                    and self.shapes[m["target"]]["kind"] != "document"):
                raise ValueError(f"null for nonoptional member: {key}")
            out[native if decode else key] = self.convert(m["target"], item, decode=decode, fmt=m["format"], strict=strict)
        if kind == "union":
            if decode:
                if len(value) != 1:
                    raise ValueError("a union must contain exactly one member")
                wire = next(iter(value))
                if wire not in shape["members"]:
                    out["_unknown"] = (wire, value[wire])
            else:
                unknown = getattr(value, "_unknown", UNSET)
                if not isinstance(unknown, Unset):
                    if unknown[0] in shape["members"]:
                        raise ValueError("unknown union member collides with a known member")
                    out[unknown[0]] = unknown[1]
                if len(out) != 1:
                    raise ValueError("a union must contain exactly one member")
        return out

    def bound(self, member: dict[str, Any], value: Any, *, header: bool = False, encoding: str | None = None, fmt: str | None = None) -> str:
        shape = self.shapes[member["target"]]
        if shape["kind"] == "list":
            items = [self.bound(shape["member"], item, header=True, encoding=encoding, fmt=fmt) for item in value]
            return ", ".join(json.dumps(x) if not x or any(c in x for c in ',"\\') or x != x.strip() else x for x in items)
        if encoding == "base64":
            return base64.b64encode(value.encode("utf-8")).decode("ascii")
        if shape["kind"] == "timestamp":
            value = _timestamp(value, fmt or member["format"], False)
        if shape["kind"] in ("float", "double") and isinstance(value, (float, Decimal)) and not math.isfinite(value):
            return "NaN" if math.isnan(value) else "Infinity" if value > 0 else "-Infinity"
        if isinstance(value, bool):
            return "true" if value else "false"
        return str(value)

    def read_header(self, m: dict[str, Any], text: str, encoding: str | None = None, fmt: str | None = None) -> Any:
        shape = self.shapes[m["target"]]
        kind = shape["kind"]
        if kind == "list":
            if not text.strip():
                return []
            parts: list[str] = []
            start = 0
            member = shape["member"]
            if self.shapes[member["target"]]["kind"] == "timestamp" and (fmt or member.get("format")) == "http-date" and not text.lstrip().startswith('"'):
                # Every IMF-fixdate holds a comma and the protocols write these lists unquoted, so only a comma
                # that is followed by the next date's weekday separates two values.
                for found in _HTTP_DATE_SEPARATOR.finditer(text):
                    parts.append(text[start:found.start()].strip())
                    start = found.start() + 1
            else:
                # JSON quoting rules are also used for commas and quotes in header lists.
                quoted = escaped = False
                for i, c in enumerate(text):
                    if escaped:
                        escaped = False
                    elif c == "\\" and quoted:
                        escaped = True
                    elif c == '"':
                        quoted = not quoted
                    elif c == "," and not quoted:
                        parts.append(text[start:i].strip())
                        start = i + 1
                if quoted:
                    raise ValueError("unterminated quoted header value")
            parts.append(text[start:].strip())
            return [self.read_header(shape["member"], json.loads(p) if p.startswith('"') else p, encoding, fmt) for p in parts]
        if encoding == "base64":
            text = base64.b64decode(text, validate=True).decode("utf-8")
        if kind == "timestamp":
            return _timestamp(text, fmt or m["format"], True)
        # Header text is a str, but these kinds are validated as int/Decimal.
        if kind in ("byte", "short", "integer", "long", "intEnum", "bigInteger"):
            return self.convert(m["target"], int(text), decode=True)
        if kind == "bigDecimal":
            return self.convert(m["target"], Decimal(text), decode=True)
        if kind in ("float", "double"):
            return float(text)
        if kind == "boolean":
            if text not in ("true", "false"):
                raise ValueError("invalid boolean header")
            return text == "true"
        return self.convert(m["target"], text, decode=True)


class _Base:
    def __init__(self, *, endpoint: str, schema: dict[str, Any], classes: dict[str, Any],
                 token: Any = None, headers: dict[str, str] | None = None,
                 max_attempts: int = 3, base_delay: float = 0.1, max_delay: float = 20,
                 timeout: float = 30, middleware: list[Callable[..., Any]] | None = None,
                 retry_mode: str = "legacy", telemetry: TelemetryHook | None = None,
                 clock: Any = None) -> None:
        """Configure without sending a request.

        endpoint: Absolute HTTP(S) base URL.
        http: Caller-owned HTTPX client; sync/async must match this client.
        token: Bearer token or per-attempt provider; async clients await providers.
        headers: Headers sent on every request.
        max_attempts: Total attempt limit, including first; default 3.
        base_delay: Initial full-jitter ceiling in seconds; default 0.1.
        max_delay: Maximum backoff ceiling in seconds; default 20.
        timeout: Total call budget in seconds; default 30. Per-call timeout
            overrides it.
        middleware: Request callbacks run for every attempt before transport.
        retry_mode: legacy (default), standard retry quota, or adaptive rate limit.
        telemetry: Optional hook; exceptions are logged without failing calls.
        clock: Monotonic clock/sleep implementation for tests.

        Close owned transport with close/aclose or context management. Supplied
        HTTPX transports remain caller-owned. Share only when all callbacks and
        transport support concurrency; async clients belong to one event loop.
        Configuration must remain immutable after construction.
        """
        if not _valid_endpoint(endpoint):
            raise ValueError("an absolute HTTP(S) endpoint is required")
        if max_attempts < 1 or timeout <= 0 or not math.isfinite(timeout):
            raise ValueError("max_attempts must be positive and timeout finite and positive")
        if any(not math.isfinite(d) or d < 0 for d in (base_delay, max_delay)):
            raise ValueError("retry delays must be finite and nonnegative")
        self._endpoint = endpoint.rstrip("/")
        self._codec = _Codec(schema, classes)
        self._token = token
        self._headers = headers or {}
        self._attempts = max_attempts
        self._base_delay = base_delay
        self._max_delay = max_delay
        self._timeout = timeout
        self._middleware = middleware or []
        self._clock = clock if clock is not None else SystemClock()
        # One quota and rate limiter for every call this client makes.
        self._retry = RetryStrategy(retry_mode, self._clock, base_delay=base_delay, max_delay=max_delay)
        self._telemetry = telemetry
        self._service = str(schema.get("service", ""))

    def _prepare(self, name: str, input: Any) -> tuple[dict[str, Any], str, bytes | None, dict[str, str]]:
        op = dict(self._codec.schema["operations"][name])
        # Telemetry shows where sensitive members went without their values.
        sensitive_headers: set[str] = set()
        sensitive_query: set[str] = set()
        values = self._codec.convert(op["input"], input) or {}
        token = op["token"]
        shape = self._codec.shapes.get(op["input"], {})
        members = shape.get("members", {})
        formats = op["request"].get("timestampFormats", {})
        payload = op["request"].get("payload")
        if token is not None and values.get(token) is None:
            values[token] = str(uuid.uuid4())
        # Binding timestamps must use the model values, not already encoded JSON timestamps.
        def original(key: str) -> Any:
            m = members[key]
            return input.get(key, UNSET) if isinstance(input, dict) else getattr(input, m["attribute"], UNSET)
        uri = op["uri"]
        display = uri
        for key in op["request"]["labels"]:
            value = original(key)
            if value is None or isinstance(value, Unset) or str(value) == "":
                raise ValueError(f"{key} is required")
            label = self._codec.bound(members[key], value, fmt=formats.get(key))
            greedy = key in op.get("greedyLabels", [])
            if any(part in (".", "..") for part in (label.split("/") if greedy else [label])):
                raise ValueError(f"{key} cannot be a dot segment")
            placeholder = "{" + key + ("+" if greedy else "") + "}"
            encoded = quote(label, safe="/-._~" if greedy else "-._~")
            uri = uri.replace(placeholder, encoded)
            display = display.replace(placeholder, REDACTED if members[key].get("sensitive") else encoded)
        query: list[tuple[str, str]] = []
        for key, wire in op["request"]["query"]:
            value = original(key)
            if key == token:
                value = values.get(key, UNSET)
            if value is None or isinstance(value, Unset):
                continue
            member = members[key]
            if member.get("sensitive"):
                sensitive_query.add(wire.lower())
            shape = self._codec.shapes[member["target"]]
            if shape["kind"] == "list":
                query.extend((wire, self._codec.bound(shape["member"], item, fmt=formats.get(key))) for item in value)
            else:
                query.append((wire, self._codec.bound(member, value, fmt=formats.get(key))))
        if query:
            uri += ("&" if "?" in uri else "?") + urlencode(query, quote_via=quote, safe="-._~")
        headers = {key.lower(): value for key, value in self._headers.items()}
        # A Content-Type bound to an input member outranks the payload's media
        # type, which outranks client header defaults.
        modelled_content_type = False
        for key, wire in op["request"]["headers"]:
            if members[key].get("sensitive"):
                sensitive_headers.add(wire.lower())
            value = values.get(key, UNSET) if key == token else original(key)
            if value is not None and not isinstance(value, Unset):
                headers[wire.lower()] = self._codec.bound(members[key], value, header=True,
                    encoding=op["request"].get("headerEncodings", {}).get(key), fmt=formats.get(key))
                modelled_content_type = modelled_content_type or wire.lower() == "content-type"
        body: bytes | None = None
        if op["request"]["body"]:
            body = _json({k: values[k] for k in op["request"]["body"] if k in values}).encode()
            headers["content-type"] = "application/json"
        elif payload is not None:
            value = original(payload[0])
            kind = op["request"].get("payloadKind", "blob")
            if payload[0] in values and (value is not None or kind == "document"):
                if kind == "string":
                    if not isinstance(value, str):
                        raise TypeError(f"{payload[0]} must be a string")
                    body = value.encode("utf-8")
                elif kind == "document":
                    body = _json(values[payload[0]]).encode("utf-8")
                else:
                    if not isinstance(value, (bytes, bytearray, memoryview)):
                        raise TypeError(f"{payload[0]} must be bytes")
                    body = bytes(value)
                if not modelled_content_type:
                    headers["content-type"] = payload[1]
        # The release this client was generated for (docs/api-versions-clients.md).
        if api_version := self._codec.schema.get("apiVersion"):
            headers["crucible-version"] = api_version
        op["target"] = uri
        op["displayPath"] = display.split("?", 1)[0]
        op["sensitiveHeaders"] = frozenset(sensitive_headers)
        op["sensitiveQuery"] = frozenset(sensitive_query)
        return op, self._endpoint + uri, body, headers

    def _bearer(self, op: dict[str, Any], request: httpx.Request) -> Any:
        """The token to send, or None when the operation is anonymous, the request already has one, or none is set."""
        if not op["bearer"] or preset_auth(request):
            return None
        return self._token() if callable(self._token) else self._token

    def _classify(self, op: dict[str, Any], error: BaseException) -> str | None:
        """The retry reason for a failed attempt, or None when the failure is not retryable."""
        if isinstance(error, httpx.TransportError):
            if not op["safe"]:
                return None
            return "timeout" if isinstance(error, httpx.TimeoutException) else "network"
        if not isinstance(error, ServiceError):
            return None
        if error.throttling:
            return "throttling"
        if op["safe"] and (error.retryable or error.metadata.status in (500, 502, 503, 504)):
            return "transient"
        return None

    def _after_failure(self, op: dict[str, Any], error: BaseException, response: httpx.Response | None,
                       attempt: int) -> tuple[str, float, int | None] | None:
        """Records a failed attempt with the retry policy and decides whether, and when, to retry."""
        self._retry.after_attempt(isinstance(error, ServiceError) and error.throttling)
        reason = self._classify(op, error)
        if reason is None or attempt >= self._attempts:
            return None
        granted, cost = self._retry.acquire_retry(reason == "timeout")
        if not granted:
            return None
        return reason, self._delay(attempt, response), cost

    def _succeeded(self, last_cost: int | None) -> None:
        self._retry.after_attempt(False)
        self._retry.succeeded(last_cost)

    def _interrupted(self, response: httpx.Response | None) -> None:
        """An attempt ended by an error that is never retried still measured a response."""
        if response is not None:
            self._retry.after_attempt(False)

    def _response_bindings(self, target: str, bindings: dict[str, Any], response: httpx.Response,
                           values: dict[str, Any]) -> None:
        members = self._codec.shapes[target]["members"]
        for member, header in bindings["headers"]:
            if header in response.headers:
                m = members[member]
                parsed = self._codec.read_header(m, response.headers[header], bindings.get("headerEncodings", {}).get(member),
                                                bindings.get("timestampFormats", {}).get(member))
                values[member] = self._codec.convert(m["target"], parsed, fmt=m["format"])

    @staticmethod
    def _payload(bindings: dict[str, Any], body: bytes) -> Any:
        kind = bindings.get("payloadKind", "blob")
        if kind == "string":
            return body.decode("utf-8")
        if kind == "document":
            return json.loads(body, parse_float=Decimal) if body else None
        return body

    def _decode(self, op: dict[str, Any], response: httpx.Response, body: bytes) -> Any:
        if response.is_success:
            target = op["output"]
            if target is None:
                return None
            payload = op["response"].get("payload")
            # A payload is the body as received, whatever it holds.
            raw_payload = self._payload(op["response"], body)
            values: dict[str, Any] = ({payload[0]: raw_payload}
                                      if payload is not None else json.loads(body, parse_float=Decimal) if body else {})
            self._response_bindings(target, op["response"], response, values)
            return self._codec.convert(target, values, decode=True)
# BEGIN ERROR DOCUMENTATION
        documentation_body_valid = not body
# END ERROR DOCUMENTATION
        try:
            fields = json.loads(body, parse_float=Decimal)
# BEGIN ERROR DOCUMENTATION
            documentation_body_valid = isinstance(fields, dict)
# END ERROR DOCUMENTATION
        except (ValueError, UnicodeDecodeError):
            fields = {}
        if not isinstance(fields, dict):
            fields = {}
        name = response.headers.get("x-crucible-error", fields.get("__type"))
        if not isinstance(name, str):
            name = None
        declared = {self._codec.shapes[t]["error"].get("wireCode", self._codec.shapes[t]["name"]): t for t in op["errors"]}
        if name is None:
            matches = [n for n, t in declared.items() if self._codec.shapes[t]["error"]["status"] == response.status_code]
            if len(matches) == 1:
                name = matches[0]
        target = declared.get(name)
        request_id = response.headers.get("x-request-id", fields.get("requestId"))
        metadata = ResponseMetadata(response.status_code, request_id if isinstance(request_id, str) else None,
                                    httpx.Headers(response.headers), body)
        converted: dict[str, Any] = {}
        error: dict[str, Any] = {}
        cls = ServiceException
        if target is not None:
            shape = self._codec.shapes[target]
            error = shape["error"]
            try:
                payload = error.get("payload")
                if payload is not None:
                    fields = {payload[0]: self._payload(error, body)}
                self._response_bindings(target, error, response, fields)
# BEGIN ERROR DOCUMENTATION
                if payload is None and not documentation_body_valid:
                    raise ValueError("modeled error body requires an object")
                converted = self._codec.members(shape, fields, decode=True, strict=True)
# END ERROR DOCUMENTATION
# BEGIN LEGACY ERROR MEMBERS
                for key, m in shape["members"].items():
                    converted[m["attribute"]] = self._codec.convert(m["target"], fields.get(key, UNSET), decode=True, fmt=m["format"])
# END LEGACY ERROR MEMBERS
                cls = self._codec.classes[target]
            except (ValueError, TypeError, OverflowError, AttributeError):
                # A malformed modeled member must not hide the HTTP failure or its retry policy.
                converted = {}
# BEGIN ERROR DOCUMENTATION
        doc_url = None
        documentation = error.get("documentation")
        if target is not None and documentation is not None:
            doc_url = documentation["url"]
            member = documentation["reasonMember"]
            if member is not None:
                attribute = self._codec.shapes[target]["members"][member]["attribute"]
                reason = converted.get(attribute, UNSET)
                if isinstance(reason, str):
                    doc_url = documentation["reasonUrls"].get(reason, doc_url)
# END ERROR DOCUMENTATION
        message = fields.get("message")
        raise cls(name=name if name is not None else "UnknownError",
                  message=message if isinstance(message, str) else f"HTTP {response.status_code}",
                  metadata=metadata, retryable=error.get("retryable", False),
# BEGIN ERROR DOCUMENTATION
                  doc_url=doc_url,
# END ERROR DOCUMENTATION
                  throttling=error.get("throttling", False), fields=converted)

    def _delay(self, attempt: int, response: httpx.Response | None) -> float:
        if response is not None:
            value = response.headers.get("retry-after")
            if value is not None:
                try:
                    delay = float(value)
                except ValueError:
                    try:
                        delay = (parsedate_to_datetime(value) - datetime.now(timezone.utc)).total_seconds()
                    except (ValueError, TypeError, OverflowError):
                        delay = -1
                if math.isfinite(delay) and delay >= 0:
                    return delay
        return self._retry.backoff(attempt)

    def _next_page(self, name: str, page: Any, seen: set[str]) -> Any:
        op = self._codec.schema["operations"][name]
        value = page
        target = op["output"]
        for part in op["pagination"]["output"]:
            member = self._codec.shapes[target]["members"][part]
            value = value.get(part) if isinstance(value, dict) else getattr(value, member["attribute"], None)
            target = member["target"]
            if isinstance(value, Unset):
                return None
        if value is None or value == "":
            return None
        if value in seen:
            raise ValueError("pagination token did not advance")
        seen.add(value)
        return value


def _error_name(error: BaseException | None) -> str | None:
    if error is None:
        return None
    return error.name if isinstance(error, ServiceError) else type(error).__name__


class _Trace:
    """One call's telemetry. Without a hook it only counts attempts."""

    def __init__(self, client: _Base, operation: str) -> None:
        self._client = client
        self._hook = client._telemetry
        self._operation = operation
        self.attempts = 0
        self._status: int | None = None
        self._id = ""
        self._start = self._attempt_start = 0.0
        if self._hook is not None:
            self._id = uuid.uuid4().hex
            self._start = client._clock.now()
            emit(self._hook, "operation_started", OperationStarted(self._id, client._service, operation, self._start))

    def attempt_started(self, op: dict[str, Any], request: httpx.Request) -> None:
        self.attempts += 1
        if self._hook is None:
            return
        self._attempt_start = self._client._clock.now()
        parts = urlsplit(self._client._endpoint)
        # Scheme, host, port and base path; never user information.
        shown = f"{parts.scheme}://{parts.netloc.rsplit('@', 1)[-1]}{parts.path}"
        query = redact_query(parse_qsl(request.url.query.decode("ascii"), keep_blank_values=True), op["sensitiveQuery"])
        target = op["displayPath"] + ("?" + urlencode(query, quote_via=quote, safe="-._~*") if query else "")
        length = request.headers.get("content-length")
        emit(self._hook, "attempt_started", AttemptStarted(
            self._id, self._client._service, self._operation, self.attempts, request.method, shown, target,
            redact_headers(request.headers, op["sensitiveHeaders"]),
            int(length) if length is not None and length.isdigit() else None, self._attempt_start))

    def attempt_finished(self, response: httpx.Response | None, error: BaseException | None, size: int | None,
                         retry: tuple[str, float, int | None] | None = None) -> None:
        self._status = response.status_code if response is not None else None
        if self._hook is None:
            return
        now = self._client._clock.now()
        request_id = None
        if isinstance(error, ServiceError):
            request_id = error.metadata.request_id
        elif response is not None:
            request_id = response.headers.get("x-request-id", response.headers.get("x-amzn-requestid"))
        emit(self._hook, "attempt_finished", AttemptFinished(
            self._id, self._client._service, self._operation, self.attempts, self._status, _error_name(error),
            request_id, size, now - self._attempt_start, retry[0] if retry else None, retry[1] if retry else None, now))

    def finished(self, error: BaseException | None) -> None:
        if self._hook is None:
            return
        now = self._client._clock.now()
        emit(self._hook, "operation_finished", OperationFinished(
            self._id, self._client._service, self._operation, self.attempts, self._status, _error_name(error),
            now - self._start, now))


class SyncClient(_Base):
    """Synchronous HTTPX transport runtime; close or context management closes only its owned transport."""
    def __init__(self, *, http: httpx.Client | None = None, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._http = http or httpx.Client()
        self._owns_http = http is None
    __init__.__doc__ = _Base.__init__.__doc__

    def close(self) -> None:
        """Close the client-owned synchronous HTTPX transport; a supplied transport remains open."""
        if self._owns_http:
            self._http.close()

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *args: Any) -> None:
        self.close()

    def _call(self, name: str, input: Any = None, *, timeout: float | None = None) -> Any:
        trace = _Trace(self, name)
        try:
            output = self._send(name, input, timeout, trace)
        except BaseException as error:
            trace.finished(error)
            raise
        trace.finished(None)
        return output

    def _send(self, name: str, input: Any, timeout: float | None, trace: _Trace) -> Any:
        op, url, body, headers = self._prepare(name, input)
        deadline = time.monotonic() + (self._timeout if timeout is None else timeout)
        last_cost: int | None = None
        for attempt in range(1, self._attempts + 1):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("request deadline exceeded")
            wait = self._retry.send_delay()
            if wait > 0:
                if wait >= deadline - time.monotonic():
                    self._retry.cancel_send()
                    raise TimeoutError("client-side rate limit delay exceeds request deadline")
                self._clock.sleep(wait)
            remaining = deadline - time.monotonic()
            request = self._http.build_request(op["method"], url, content=body, headers=headers, timeout=remaining)
            token = self._bearer(op, request)
            if inspect.isawaitable(token):
                if inspect.iscoroutine(token):
                    token.close()
                raise TypeError("use AsyncClient with an async token provider")
            for middleware in self._middleware:
                middleware(request)
            if token is not None:
                apply_auth(request, token)
            _check_credentials(request)
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("request deadline exceeded during preparation")
            request.extensions["timeout"] = {key: remaining for key in ("connect", "read", "write", "pool")}
            trace.attempt_started(op, request)
            response = None
            failure: BaseException | None = None
            data = b""
            try:
                try:
                    response = self._http.send(request, stream=True)
                    try:
                        data = b"" if response.is_success and not _has_body(op) else response.read()
                    finally:
                        response.close()
                except httpx.TransportError as error:
                    failure = error
                else:
                    if time.monotonic() >= deadline:
                        raise TimeoutError("request deadline exceeded while reading the response")
                    try:
                        output = self._decode(op, response, data)
                    except ServiceError as error:
                        failure = error
                    else:
                        self._succeeded(last_cost)
                        trace.attempt_finished(response, None, len(data))
                        return output
            except BaseException as error:
                self._interrupted(response)
                trace.attempt_finished(response, error, len(data) if response is not None else None)
                raise
            assert failure is not None
            retry = self._after_failure(op, failure, response, attempt)
            trace.attempt_finished(response, failure, len(data) if response is not None else None, retry)
            if retry is None:
                if isinstance(failure, httpx.TransportError):
                    raise NetworkError(str(failure)) from failure
                raise failure
            delay, last_cost = retry[1], retry[2]
            if delay >= deadline - time.monotonic():
                raise TimeoutError("retry delay exceeds request deadline")
            self._clock.sleep(delay)
        raise AssertionError("unreachable")

    def _pages(self, name: str, input: Any = None, **options: Any) -> Any:
        op = self._codec.schema["operations"][name]
        values = copy.deepcopy(input) if input is not None else {}
        member = op["pagination"]["input"]
        attribute = self._codec.shapes[op["input"]]["members"][member]["attribute"]
        initial = values.get(member) if isinstance(values, dict) else getattr(values, attribute, None)
        seen = {initial} if isinstance(initial, str) and initial else set()
        while True:
            page = self._call(name, values, **options)
            yield page
            token = self._next_page(name, page, seen)
            if token is None:
                return
            if isinstance(values, dict):
                values[member] = token
            else:
                setattr(values, attribute, token)


class AsyncClientBase(_Base):
    """Async HTTPX transport runtime; aclose or async context management closes only its owned transport."""
    def __init__(self, *, http: httpx.AsyncClient | None = None, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._http = http or httpx.AsyncClient()
        self._owns_http = http is None
    __init__.__doc__ = _Base.__init__.__doc__

    async def aclose(self) -> None:
        """Close owned asynchronous transport or response resources; callers retain ownership of supplied HTTPX clients."""
        if self._owns_http:
            await self._http.aclose()

    async def __aenter__(self) -> Self:
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self.aclose()

    async def _call(self, name: str, input: Any = None, *, timeout: float | None = None) -> Any:
        deadline = asyncio.get_running_loop().time() + (self._timeout if timeout is None else timeout)
        trace = _Trace(self, name)
        try:
            async with asyncio.timeout_at(deadline):
                output = await self._send(name, input, timeout, deadline, trace)
        except BaseException as error:
            trace.finished(error)
            raise
        trace.finished(None)
        return output

    async def _send(self, name: str, input: Any, timeout: float | None, deadline: float, trace: _Trace) -> Any:
        op, url, body, headers = self._prepare(name, input)
        last_cost: int | None = None
        for attempt in range(1, self._attempts + 1):
            wait = self._retry.send_delay()
            if wait > 0:
                try:
                    await self._clock.asleep(wait)
                except BaseException:
                    self._retry.cancel_send()
                    raise
            request = self._http.build_request(op["method"], url, content=body, headers=headers,
                                               timeout=self._timeout if timeout is None else timeout)
            token = self._bearer(op, request)
            if inspect.isawaitable(token):
                token = await token
            for middleware in self._middleware:
                result = middleware(request)
                if inspect.isawaitable(result):
                    await result
            if token is not None:
                apply_auth(request, token)
            _check_credentials(request)
            trace.attempt_started(op, request)
            response = None
            failure: BaseException | None = None
            data = b""
            try:
                try:
                    response = await self._http.send(request, stream=True)
                    try:
                        data = b"" if response.is_success and not _has_body(op) else await response.aread()
                    finally:
                        await response.aclose()
                except httpx.TransportError as error:
                    failure = error
                else:
                    try:
                        output = self._decode(op, response, data)
                    except ServiceError as error:
                        failure = error
                    else:
                        self._succeeded(last_cost)
                        trace.attempt_finished(response, None, len(data))
                        return output
            except BaseException as error:
                self._interrupted(response)
                trace.attempt_finished(response, error, len(data) if response is not None else None)
                raise
            assert failure is not None
            retry = self._after_failure(op, failure, response, attempt)
            trace.attempt_finished(response, failure, len(data) if response is not None else None, retry)
            if retry is None:
                if isinstance(failure, httpx.TransportError):
                    raise NetworkError(str(failure)) from failure
                raise failure
            last_cost = retry[2]
            await self._clock.asleep(retry[1])
        raise AssertionError("unreachable")

    async def _pages(self, name: str, input: Any = None, **options: Any) -> Any:
        op = self._codec.schema["operations"][name]
        values = copy.deepcopy(input) if input is not None else {}
        member = op["pagination"]["input"]
        attribute = self._codec.shapes[op["input"]]["members"][member]["attribute"]
        initial = values.get(member) if isinstance(values, dict) else getattr(values, attribute, None)
        seen = {initial} if isinstance(initial, str) and initial else set()
        while True:
            page = await self._call(name, values, **options)
            yield page
            token = self._next_page(name, page, seen)
            if token is None:
                return
            if isinstance(values, dict):
                values[member] = token
            else:
                setattr(values, attribute, token)
