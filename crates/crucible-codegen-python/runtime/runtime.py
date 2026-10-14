"""Crucible JSON protocol runtime, bundled with its generated client."""
from __future__ import annotations

import asyncio
import base64
import binascii
import copy
import hashlib
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
from typing import Any, Callable, Iterator, AsyncIterator, Self
from urllib.parse import parse_qsl, quote, urlencode, urlsplit

import httpx

from .auth import apply_auth, preset_auth
from . import checksums
from .waiters import Waiter, WaiterError, WaiterTimeout
from .endpoints import resolve as resolve_endpoint, host_prefix, parse_url
from .streams import ByteStream, StreamLengthError, Upload
from .policy import RetryStrategy, SystemClock
from .discovery import DiscoveredAddress, EndpointCache, EndpointDiscoveryError, cache_key
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

    Omitted members are skipped, null is shown as None, cycles as <recursive>,
    and streams are never consumed. Unknown union payloads are redacted because
    an older client cannot know their sensitivity.
    """
    active: set[int] = set()

    def render(target: str, value: Any, sensitive: bool = False) -> str:
        shape = schema["shapes"][target]
        if sensitive or shape.get("sensitive"):
            return "*** Sensitive Data Redacted ***"
        if value is None:
            return "None"
        if shape.get("streaming"):
            return "<stream>"
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
                 fields: dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.name = name
        self.message = message
        self.metadata = metadata
        self.retryable = retryable
        self.throttling = throttling
        self.fields = fields or {}

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
    return bool(response["body"]) or response.get("payload") is not None or bool(op.get("resolvedChecksum", {}).get("response"))


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
        self.settings = schema.get("protocolSettings", {})

    def convert(self, target: str | None, value: Any, *, decode: bool = False,
                fmt: str | None = None) -> Any:
        if target is None:
            return None
        shape = self.shapes[target]
        if shape["kind"] == "unit" and not isinstance(value, Unset):
            return None if decode else {}
        if value is None or isinstance(value, Unset):
            return value
        kind = shape["kind"]
        if shape.get("streaming"):
            return value  # A ByteStream, or bytes for one; never base64.
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
            converted = [self.convert(member["target"], v, decode=decode, fmt=member["format"]) for v in value]
            if shape.get("set", False) and any(item in converted[:index] for index, item in enumerate(converted)):
                raise ValueError("duplicate element in set")
            return converted
        if kind == "map":
            member = shape["value"]
            if not shape.get("sparse", False) and any(v is None for v in value.values()):
                raise ValueError("Null value in non-sparse map")
            return {self.convert(shape["key"]["target"], k, decode=decode): self.convert(member["target"], v, decode=decode, fmt=member["format"]) for k, v in value.items()}
        if kind in ("structure", "union"):
            if decode and not isinstance(value, dict):
                raise ValueError(f"expected an object for {shape['name']}")
            if (decode and kind == "union" and self.settings.get("ignoreUnknownUnionType")
                    and "__type" not in shape["members"]):
                value = {key: item for key, item in value.items() if key != "__type"}
            out: dict[str, Any] = {}
            wire_names = {m.get("jsonName", key) for key, m in shape["members"].items()}
            for key, m in shape["members"].items():
                wire = m.get("jsonName", key)
                native = m["attribute"]
                item = value.get(wire if decode else key, UNSET) if isinstance(value, dict) else getattr(value, native, UNSET)
                if (kind == "structure" and item is None and self.settings.get("omitNullStructureMembers")
                        and self.shapes[m["target"]]["kind"] != "document"):
                    item = UNSET
                if isinstance(item, Unset) and decode and m["default"] is not None:
                    item = copy.deepcopy(m["default"])
                if isinstance(item, Unset):
                    if kind == "structure" and m.get("optional") is False:
                        raise ValueError(f"missing required member: {wire}")
                    continue
                if (kind == "structure" and item is None and m.get("optional") is False
                        and self.shapes[m["target"]]["kind"] != "document"):
                    raise ValueError(f"null for nonoptional member: {wire}")
                out[native if decode else wire] = self.convert(m["target"], item, decode=decode, fmt=m["format"])
            if kind == "union":
                if decode:
                    if len(value) != 1:
                        raise ValueError("a union must contain exactly one member")
                    wire = next(iter(value))
                    if wire not in wire_names:
                        out["_unknown"] = (wire, value[wire])
                else:
                    unknown = getattr(value, "_unknown", UNSET)
                    if not isinstance(unknown, Unset):
                        if unknown[0] in wire_names:
                            raise ValueError("unknown union member collides with a known member")
                        out[unknown[0]] = unknown[1]
                    if len(out) != 1:
                        raise ValueError("a union must contain exactly one member")
            return self.classes[target](**out) if decode else out
        if kind in ("byte", "short", "integer", "long"):
            bits = {"byte": 8, "short": 16, "integer": 32, "long": 64}[kind]
            if isinstance(value, bool) or not isinstance(value, int) or not -(2 ** (bits - 1)) <= value < 2 ** (bits - 1):
                raise ValueError(f"{kind} requires a signed {bits}-bit integer")
        if kind in ("float", "double"):
            if self.settings.get("nonFiniteFloatStrings"):
                if decode and isinstance(value, str):
                    if value not in ("NaN", "Infinity", "-Infinity"):
                        raise ValueError("invalid floating point string")
                    return float(value)
                if not decode and isinstance(value, (float, Decimal)) and not math.isfinite(value):
                    return "NaN" if math.isnan(value) else "Infinity" if value > 0 else "-Infinity"
            if isinstance(value, Decimal):
                return float(value)
        return value

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
    def __init__(self, *, endpoint: str | None, schema: dict[str, Any], classes: dict[str, Any],
                 token: Any = None, headers: dict[str, str] | None = None,
                 basic_auth: Any = None, api_key: Any = None, credentials: Any = None,
                 region: str | None = None,
                 endpoint_parameters: dict[str, Any] | None = None,
                 endpoint_builtins: dict[str, Any] | None = None, disable_host_prefix: bool = False,
                 max_attempts: int = 3, base_delay: float = 0.1, max_delay: float = 20,
                 timeout: float = 30, content_sha256: bool = False,
                 request_checksum_calculation: str = "WHEN_SUPPORTED", response_checksum_validation: str = "WHEN_SUPPORTED",
                 checksum_validated: Callable[[str, str], None] | None = None,
                 middleware: list[Callable[..., Any]] | None = None,
                 retry_mode: str = "legacy", endpoint_discovery: bool | None = None,
                 telemetry: TelemetryHook | None = None, clock: Any = None) -> None:
        """Configure without sending a request.

        endpoint: Absolute HTTP(S) base URL; optional with endpoint rules.
        http: Caller-owned HTTPX client; sync/async must match this client.
        token: Bearer token or per-attempt provider; async clients await providers.
        headers: Headers sent on every request.
        basic_auth: BasicCredentials or per-attempt identity provider.
        api_key: Modeled API key string or per-attempt provider.
        credentials: AwsCredentials or per-attempt SigV4 provider.
        region: Region for endpoint rules and SigV4 signing.
        endpoint_parameters: Overrides for modeled endpoint parameters.
        endpoint_builtins: Overrides for built-in endpoint values.
        disable_host_prefix: Disable the modeled host prefix when True.
        max_attempts: Total attempt limit, including first; default 3.
        base_delay: Initial full-jitter ceiling in seconds; default 0.1.
        max_delay: Maximum backoff ceiling in seconds; default 20.
        timeout: Total call budget in seconds; default 30. Per-call timeout
            overrides it; streaming reads retain the deadline.
        content_sha256: Send SHA-256 headers for request bodies when enabled.
        request_checksum_calculation: WHEN_SUPPORTED (default) or WHEN_REQUIRED.
        response_checksum_validation: WHEN_SUPPORTED (default) or WHEN_REQUIRED.
        checksum_validated: Callback receiving operation and verified algorithm.
        middleware: Request callbacks run for every attempt before transport.
        retry_mode: legacy (default), standard retry quota, or adaptive rate limit.
        endpoint_discovery: None follows modeled policy; True enables optional
            discovery; False disables optional discovery. Required discovery stays.
        telemetry: Optional hook; exceptions are logged without failing calls.
        clock: Monotonic clock/sleep implementation for tests.

        Close owned transport with close/aclose or context management. Supplied
        HTTPX transports remain caller-owned. Share only when all callbacks and
        transport support concurrency; async clients belong to one event loop.
        Configuration must remain immutable after construction.
        """
        if endpoint is None and "endpointRules" not in schema or endpoint is not None and parse_url(endpoint) is None:
            raise ValueError("an absolute HTTP(S) endpoint is required")
        if max_attempts < 1 or timeout <= 0 or not math.isfinite(timeout):
            raise ValueError("max_attempts must be positive and timeout finite and positive")
        if any(not math.isfinite(d) or d < 0 for d in (base_delay, max_delay)):
            raise ValueError("retry delays must be finite and nonnegative")
        self._endpoint = endpoint.rstrip("/") if endpoint is not None else ""
        self._codec = _Codec(schema, classes)
        self._token = token
        self._identities = {"bearer": token, "basic": basic_auth, "apiKey": api_key, "sigV4": credentials}
        self._region = region
        self._endpoint_parameters = dict(endpoint_parameters or {})
        self._endpoint_builtins = {"AWS::Region": region, "SDK::Endpoint": endpoint, **(endpoint_builtins or {})}
        self._disable_host_prefix = disable_host_prefix
        self._headers = headers or {}
        self._attempts = max_attempts
        self._base_delay = base_delay
        self._max_delay = max_delay
        self._timeout = timeout
        self._digest = content_sha256
        self._middleware = middleware or []
        if request_checksum_calculation not in ("WHEN_SUPPORTED", "WHEN_REQUIRED") or response_checksum_validation not in ("WHEN_SUPPORTED", "WHEN_REQUIRED"):
            raise ValueError("checksum policies must be WHEN_SUPPORTED or WHEN_REQUIRED")
        self._request_checksum_calculation = request_checksum_calculation
        self._response_checksum_validation = response_checksum_validation
        self._checksum_validated = checksum_validated
        if endpoint_discovery not in (None, True, False):
            raise ValueError("endpoint_discovery must be None, True or False")
        self._clock = clock if clock is not None else SystemClock()
        # One quota and rate limiter for every call this client makes.
        self._retry = RetryStrategy(retry_mode, self._clock, base_delay=base_delay, max_delay=max_delay)
        self._telemetry = telemetry
        self._endpoint_discovery = endpoint_discovery
        self._discovery = EndpointCache(self._clock) if "endpointDiscovery" in schema else None
        self._service = str(schema.get("service", ""))
        credential_headers: set[str] = set()
        credential_query: set[str] = set()
        for scheme in schema.get("authSchemes", {}).values():
            if scheme.get("kind") == "apiKey":
                (credential_query if scheme.get("location") == "query" else credential_headers).add(scheme["name"].lower())
        self._credential_headers = frozenset(credential_headers)
        self._credential_query = frozenset(credential_query)

    def _query_value(self, target: str | None, value: Any) -> Any:
        if target is None or isinstance(value, Unset) or value is None:
            return None
        shape = self._codec.shapes[target]
        kind = shape["kind"]
        if kind in ("structure", "union"):
            result = {}
            for name, member in shape["members"].items():
                item = value.get(name, UNSET) if isinstance(value, dict) else getattr(value, member["attribute"], UNSET)
                if not isinstance(item, Unset):
                    result[name] = self._query_value(member["target"], item)
            unknown = getattr(value, "_unknown", UNSET)
            if not isinstance(unknown, Unset):
                result[unknown[0]] = unknown[1]
            return result
        if kind == "list":
            return [self._query_value(shape["member"]["target"], item) for item in value]
        if kind == "map":
            return {str(key): self._query_value(shape["value"]["target"], item) for key, item in value.items()}
        if kind == "timestamp":
            delta = value.astimezone(timezone.utc) - datetime(1970, 1, 1, tzinfo=timezone.utc)
            return Decimal(delta.days * 86400 + delta.seconds) + Decimal(delta.microseconds) / 1_000_000
        if kind == "blob":
            return None if shape.get("streaming") else base64.b64encode(value).decode("ascii")
        if kind == "enum":
            return str(value)
        if kind == "intEnum":
            return int(value)
        return value

    def _auth_options(self, op: dict[str, Any]) -> Any:
        schemes = self._codec.schema.get("authSchemes", {})
        selected = op.get("auth", ["smithy.api#httpBearerAuth"] if op["bearer"] else [])
        for name in selected:
            scheme = schemes.get(name, {"kind": "bearer"} if name == "smithy.api#httpBearerAuth" else None)
            if scheme is None:
                raise ValueError(f"unknown authentication scheme {name}")
            endpoint_auth = op.get("resolvedEndpoint", {}).get("properties", {}).get("authSchemes")
            if endpoint_auth is not None:
                match = next((item for item in endpoint_auth if item["name"].lower() == scheme["kind"].lower()), None)
                if match is None:
                    continue
                scheme = {**scheme, **{key: value for key, value in match.items() if key != "name"}}
            provider = self._identities.get(scheme["kind"])
            yield scheme, provider

    def _auth_required(self, op: dict[str, Any]) -> None:
        # Preserve existing bearer-token configuration, while modeled additional
        # schemes require an identity unless anonymous access is explicit.
        if any(name != "smithy.api#httpBearerAuth" for name in op.get("auth", [])) and not op.get("optionalAuth", False):
            raise ValueError("no credentials available for the operation's authentication schemes")

    def _prepare(self, name: str, input: Any) -> tuple[dict[str, Any], str, bytes | ByteStream | None, dict[str, str]]:
        op = self._codec.schema["operations"][name]
        endpoint: dict[str, Any] = {"url": self._endpoint, "headers": {}, "properties": {}}
        if "endpointRules" in self._codec.schema:
            endpoint = resolve_endpoint(self._codec.schema["endpointRules"], op.get("endpointParameters", {}),
                self._query_value(op["input"], input), self._endpoint_parameters, self._endpoint_builtins)
        endpoint_url = str(endpoint["url"])
        if op.get("endpoint") is not None and not self._disable_host_prefix:
            endpoint_url = host_prefix(endpoint_url, op["endpoint"], self._query_value(op["input"], input))
        op = {**op, "resolvedEndpoint": endpoint, "endpointUrl": endpoint_url.rstrip("/"),
              "endpointSource": "rules" if "endpointRules" in self._codec.schema else "configured"}
        # Telemetry shows where sensitive members went without their values.
        sensitive_headers: set[str] = set()
        sensitive_query: set[str] = set()
        values = self._codec.convert(op["input"], input) or {}
        token = op["token"]
        shape = self._codec.shapes.get(op["input"], {})
        members = shape.get("members", {})
        formats = op["request"].get("timestampFormats", {})
        def json_name(key: str) -> str:
            return str(members[key].get("jsonName", key))
        payload = op["request"].get("payload")
        stream: ByteStream | None = None
        if op["request"].get("streaming") and payload is not None:
            value = values.get(json_name(payload[0]))
            if isinstance(value, (bytes, bytearray, memoryview)):
                value = ByteStream.from_bytes(value)
            if value is not None and not isinstance(value, ByteStream):
                raise TypeError(f"{payload[0]} must be a ByteStream or bytes")
            stream = value
            if stream is not None and not stream.replayable and stream._opened:
                raise RuntimeError("a one-shot stream was already opened")
        checksum, injected = checksums.prepare(op.get("checksum"), values,
            {key: json_name(key) for key in members}, self._request_checksum_calculation, self._response_checksum_validation,
            streaming=stream is not None and stream._data is None)
        op["resolvedChecksum"] = checksum
        if token is not None and values.get(json_name(token)) is None:
            values[json_name(token)] = str(uuid.uuid4())
        # Binding timestamps must use the model values, not already encoded JSON timestamps.
        def original(key: str) -> Any:
            if key in injected:
                return injected[key]
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
                value = values.get(json_name(key), UNSET)
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
        query_member = op["request"].get("queryParams")
        if query_member is not None:
            value = original(query_member)
            if value is not None and not isinstance(value, Unset):
                reserved = {wire for _, wire in op["request"]["query"]}
                reserved.update(key for key, _ in parse_qsl(urlsplit(uri).query, keep_blank_values=True))
                item = self._codec.shapes[members[query_member]["target"]]["value"]
                collection = self._codec.shapes[item["target"]]
                for wire, entry in value.items():
                    if wire in reserved:
                        continue
                    if members[query_member].get("sensitive"):
                        sensitive_query.add(wire.lower())
                    if collection["kind"] == "list":
                        query.extend((wire, self._codec.bound(collection["member"], element)) for element in entry)
                    else:
                        query.append((wire, self._codec.bound(item, entry)))
        if query:
            uri += ("&" if "?" in uri else "?") + urlencode(query, quote_via=quote, safe="-._~")
        headers = {key.lower(): value for key, value in self._headers.items()}
        for key, entries in endpoint["headers"].items():
            if not all(isinstance(value, str) for value in entries):
                raise ValueError("endpoint header values must be strings")
            headers[key.lower()] = ", ".join(entries)
        reserved_headers = {wire.lower() for _, wire in op["request"]["headers"]}
        for key, prefix in op["request"].get("prefixHeaders", []):
            value = original(key)
            if value is not None and not isinstance(value, Unset):
                item = self._codec.shapes[members[key]["target"]]["value"]
                for suffix, entry in value.items():
                    wire = (prefix + suffix).lower()
                    if members[key].get("sensitive"):
                        sensitive_headers.add(wire)
                    if wire not in reserved_headers:
                        headers[wire] = self._codec.bound(item, entry, header=True,
                            encoding=op["request"].get("headerEncodings", {}).get(key))
        # A Content-Type bound to an input member outranks the payload's media
        # type, which outranks endpoint and client header defaults.
        modelled_content_type = False
        for key, wire in op["request"]["headers"]:
            if members[key].get("sensitive"):
                sensitive_headers.add(wire.lower())
            value = values.get(json_name(key), UNSET) if key == token else original(key)
            if value is not None and not isinstance(value, Unset):
                headers[wire.lower()] = self._codec.bound(members[key], value, header=True,
                    encoding=op["request"].get("headerEncodings", {}).get(key), fmt=formats.get(key))
                modelled_content_type = modelled_content_type or wire.lower() == "content-type"
        body: bytes | ByteStream | None = None
        if op["request"]["body"]:
            body = _json({json_name(k): values[json_name(k)] for k in op["request"]["body"] if json_name(k) in values}).encode()
            headers["content-type"] = "application/json"
        if stream is not None:
            bounds = members[payload[0]].get("length") or {}
            minimum, maximum = bounds.get("min"), bounds.get("max")
            if stream.length is None and op["request"].get("requiresLength"):
                raise ValueError(f"{payload[0]} requires a stream with a known length")
            if stream.length is not None and (minimum is not None and stream.length < minimum
                                              or maximum is not None and stream.length > maximum):
                raise StreamLengthError(f"{payload[0]} length {stream.length} is outside the modeled bounds")
            op["streamLimits"] = (minimum, maximum)
            # In-memory bytes are an ordinary buffered body; anything else streams.
            body = stream._data if stream._data is not None else stream
            if stream._data is None and stream.length is not None:
                headers["content-length"] = str(stream.length)
            if not modelled_content_type:
                headers["content-type"] = payload[1]
        elif payload is not None:
            # The bytes go as they are: a body digest covers exactly these.
            value = original(payload[0])
            # The codec has already applied the protocol's null-member policy.
            kind = op["request"].get("payloadKind", "blob")
            if json_name(payload[0]) in values and (value is not None or kind == "document"):
                if kind == "string":
                    if not isinstance(value, str):
                        raise TypeError(f"{payload[0]} must be a string")
                    body = value.encode("utf-8")
                elif kind == "document":
                    body = _json(values[json_name(payload[0])]).encode("utf-8")
                else:
                    if not isinstance(value, (bytes, bytearray, memoryview)):
                        raise TypeError(f"{payload[0]} must be bytes")
                    body = bytes(value)
                if not modelled_content_type:
                    headers["content-type"] = payload[1]
        protocol = self._codec.schema.get("protocol", "crucibleJson")
        if protocol in ("awsJson1_0", "awsJson1_1"):
            body = body if body is not None else b"{}"
            headers["content-type"] = "application/x-amz-json-" + ("1.0" if protocol == "awsJson1_0" else "1.1")
            headers["x-amz-target"] = self._codec.schema["service"].split("#")[-1] + "." + name
        # The release this client was generated for (docs/api-versions-clients.md).
        if api_version := self._codec.schema.get("apiVersion"):
            headers["crucible-version"] = api_version
        op["target"] = uri
        op["displayPath"] = display.split("?", 1)[0]
        op["sensitiveHeaders"] = frozenset(sensitive_headers) | self._credential_headers
        op["sensitiveQuery"] = frozenset(sensitive_query) | self._credential_query
        op["discovery"] = self._discovery_request(name, op, original)
        return op, endpoint_url.rstrip("/") + uri, body, headers

    def _discovery_request(self, name: str, op: dict[str, Any], original: Callable[[str], Any]) -> dict[str, Any] | None:
        """What this call sends to the discovery operation, or None when it uses no discovery."""
        discovered = op.get("discoveredEndpoint")
        config = self._codec.schema.get("endpointDiscovery")
        if discovered is None or config is None:
            return None
        if self._endpoint_discovery is False:
            if discovered["required"]:
                raise EndpointDiscoveryError(f"{name} requires endpoint discovery, which is disabled")
            return None
        if not discovered["required"] and not self._endpoint_discovery:
            return None
        operation = name if config.get("operationMember") else None
        identifiers = {key: str(original(key)) for key in discovered["identifiers"]} if config.get("identifiersMember") else {}
        input: dict[str, Any] = {}
        if operation is not None:
            input[config["operationMember"]] = operation
        if identifiers:
            input[config["identifiersMember"]] = identifiers
        return {"key": cache_key(operation, identifiers), "input": input, "required": discovered["required"],
                "operation": config["operation"].rsplit("#", 1)[-1]}

    def _discovered(self, output: Any) -> DiscoveredAddress:
        """The first usable endpoint of a discovery response, cached for its period."""
        config = self._codec.schema["endpointDiscovery"]
        operation = self._codec.schema["operations"][config["operation"].rsplit("#", 1)[-1]]
        values = self._query_value(operation["output"], output) or {}
        scheme = urlsplit(self._endpoint).scheme or "https"
        for endpoint in values.get(config["endpointsMember"]) or []:
            address = endpoint.get(config["addressMember"])
            minutes = endpoint.get(config["cachePeriodMember"])
            if not isinstance(address, str) or not address or isinstance(minutes, bool) or not isinstance(minutes, int):
                continue
            # Discovered addresses are usually bare hosts; they keep the configured scheme.
            url = address if "://" in address else f"{scheme}://{address}"
            if parse_url(url) is None:
                continue
            return DiscoveredAddress(url.rstrip("/"), self._clock.now() + max(minutes, 0) * 60)
        raise EndpointDiscoveryError("endpoint discovery returned no usable endpoint")

    def _invalidates(self, error: BaseException) -> bool:
        if not isinstance(error, ServiceError):
            return False
        target = (self._codec.schema.get("endpointDiscovery") or {}).get("error")
        code = None
        if target is not None:
            shape = self._codec.shapes[target]
            code = shape["error"].get("wireCode", shape["name"])
        return error.metadata.status == 421 or (code is not None and error.name == code)

    def _classify(self, op: dict[str, Any], error: BaseException, discovered: DiscoveredAddress | None) -> str | None:
        """The retry reason for a failed attempt, or None when the failure is not retryable."""
        if isinstance(error, httpx.TransportError):
            if not op["safe"]:
                return None
            return "timeout" if isinstance(error, httpx.TimeoutException) else "network"
        if not isinstance(error, ServiceError):
            return None
        # A misdirected request was not processed, so even a write may be resent.
        if discovered is not None and self._invalidates(error):
            return "endpoint"
        if error.throttling:
            return "throttling"
        if op["safe"] and (error.retryable or error.metadata.status in (500, 502, 503, 504)):
            return "transient"
        return None

    def _after_failure(self, op: dict[str, Any], error: BaseException, response: httpx.Response | None,
                       attempt: int, replayable: bool, discovered: DiscoveredAddress | None) -> tuple[str, float, int | None] | None:
        """Records a failed attempt with the retry policy and decides whether, and when, to retry."""
        self._retry.after_attempt(isinstance(error, ServiceError) and error.throttling)
        if discovered is not None and self._invalidates(error):
            assert self._discovery is not None and op["discovery"] is not None
            self._discovery.invalidate(op["discovery"]["key"], discovered)
        reason = self._classify(op, error, discovered)
        if reason is None or attempt >= self._attempts or not replayable:
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

    def _attempt_url(self, op: dict[str, Any], url: str, discovered: DiscoveredAddress | None) -> tuple[str, str, str]:
        """The attempt's URL, its endpoint, and where that endpoint came from."""
        if discovered is not None:
            return discovered.url + op["target"], discovered.url, "discovered"
        return url, op["endpointUrl"], op["endpointSource"]

    def _response_bindings(self, target: str, bindings: dict[str, Any], response: httpx.Response,
                           values: dict[str, Any]) -> None:
        members = self._codec.shapes[target]["members"]
        for member, prefix in bindings.get("prefixHeaders", []):
            m = members[member]
            item = self._codec.shapes[m["target"]]["value"]
            values[m.get("jsonName", member)] = {
                key[len(prefix):]: self._codec.convert(item["target"], self._codec.read_header(item, value,
                    bindings.get("headerEncodings", {}).get(member)), fmt=item["format"])
                for key, value in response.headers.items()
                if key.lower().startswith(prefix.lower())
            }
        code = bindings.get("responseCode")
        if code is not None:
            values[members[code].get("jsonName", code)] = response.status_code
        for member, header in bindings["headers"]:
            if header in response.headers:
                m = members[member]
                parsed = self._codec.read_header(m, response.headers[header], bindings.get("headerEncodings", {}).get(member),
                                                bindings.get("timestampFormats", {}).get(member))
                values[m.get("jsonName", member)] = self._codec.convert(m["target"], parsed, fmt=m["format"])

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
            values: dict[str, Any] = ({self._codec.shapes[target]["members"][payload[0]].get("jsonName", payload[0]): raw_payload}
                                      if payload is not None else json.loads(body, parse_float=Decimal) if body else {})
            self._response_bindings(target, op["response"], response, values)
            return self._codec.convert(target, values, decode=True)
        try:
            fields = json.loads(body, parse_float=Decimal)
        except (ValueError, UnicodeDecodeError):
            fields = {}
        if not isinstance(fields, dict):
            fields = {}
        name = response.headers.get("x-crucible-error", fields.get("__type"))
        aws_json = self._codec.schema.get("protocol") in ("awsRestJson1", "awsJson1_0", "awsJson1_1")
        if aws_json:
            name = response.headers.get("x-amzn-errortype", fields.get("code", fields.get("__type")))
            if isinstance(name, str):
                name = name.split(":", 1)[0].rsplit("#", 1)[-1]
        if not isinstance(name, str):
            name = None
        declared = {self._codec.shapes[t]["error"].get("wireCode", self._codec.shapes[t]["name"]): t for t in op["errors"]}
        if name is None:
            matches = [n for n, t in declared.items() if self._codec.shapes[t]["error"]["status"] == response.status_code]
            if len(matches) == 1:
                name = matches[0]
        target = declared.get(name)
        request_id = response.headers.get("x-request-id", fields.get("requestId"))
        if aws_json:
            request_id = response.headers.get("x-amzn-requestid", response.headers.get("x-amz-request-id", request_id))
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
                    fields = {shape["members"][payload[0]].get("jsonName", payload[0]): self._payload(error, body)}
                self._response_bindings(target, error, response, fields)
                for key, m in shape["members"].items():
                    converted[m["attribute"]] = self._codec.convert(m["target"], fields.get(m.get("jsonName", key), UNSET), decode=True, fmt=m["format"])
                cls = self._codec.classes[target]
            except (ValueError, TypeError, OverflowError, AttributeError):
                # A malformed modeled member must not hide the HTTP failure or its retry policy.
                converted = {}
        message = fields.get("message", fields.get("Message") if aws_json else None)
        raise cls(name=name if name is not None else "UnknownError",
                  message=message if isinstance(message, str) else f"HTTP {response.status_code}",
                  metadata=metadata, retryable=error.get("retryable", False),
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

    def _body_checks(self, op: dict[str, Any], request: httpx.Request, body: bytes | ByteStream | None) -> None:
        """Checksums and the content digest over the final body, after middleware.
        A streamed body is never read to compute them: they must be declared."""
        if not isinstance(body, ByteStream):
            checksums.request(op["resolvedChecksum"], request.headers, request.content)
            if self._digest and (request.content or request.method in ("POST", "PUT", "PATCH")):
                request.headers["x-amz-content-sha256"] = hashlib.sha256(request.content).hexdigest()
            return
        checksums.require_header(op["resolvedChecksum"], request.headers)
        if body.sha256 is not None:
            request.extensions["crucible.sha256"] = body.sha256
        if self._digest:
            if body.sha256 is not None:
                request.headers["x-amz-content-sha256"] = body.sha256
            elif "x-amz-content-sha256" not in request.headers:
                raise ValueError("a streaming body needs a precomputed SHA-256 for the content digest; use ByteStream.with_sha256")

    def _streamed_output(self, op: dict[str, Any], response: httpx.Response, body: ByteStream) -> Any:
        """Decode a successful response at its headers; the payload is read later."""
        target = op["output"]
        payload = op["response"]["payload"]
        values: dict[str, Any] = {self._codec.shapes[target]["members"][payload[0]].get("jsonName", payload[0]): body}
        self._response_bindings(target, op["response"], response, values)
        return self._codec.convert(target, values, decode=True)

    @staticmethod
    def _response_length(response: httpx.Response) -> int | None:
        value = response.headers.get("content-length")
        if value is None or "content-encoding" in response.headers or not value.isdigit():
            return None
        return int(value)

    def _output_stream(self, op: dict[str, Any], output: Any) -> ByteStream | None:
        if output is None or not op["response"].get("streaming") or op["response"].get("payload") is None:
            return None
        member = self._codec.shapes[op["output"]]["members"][op["response"]["payload"][0]]
        value = getattr(output, member["attribute"], None)
        return value if isinstance(value, ByteStream) else None

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

    def attempt_started(self, op: dict[str, Any], request: httpx.Request, endpoint: str, source: str) -> None:
        self.attempts += 1
        if self._hook is None:
            return
        self._attempt_start = self._client._clock.now()
        parts = urlsplit(endpoint)
        # Scheme, host, port and base path; never user information.
        shown = f"{parts.scheme}://{parts.netloc.rsplit('@', 1)[-1]}{parts.path}"
        query = redact_query(parse_qsl(request.url.query.decode("ascii"), keep_blank_values=True), op["sensitiveQuery"])
        target = op["displayPath"] + ("?" + urlencode(query, quote_via=quote, safe="-._~*") if query else "")
        length = request.headers.get("content-length")
        emit(self._hook, "attempt_started", AttemptStarted(
            self._id, self._client._service, self._operation, self.attempts, request.method, shown, source, target,
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

    def _discover(self, op: dict[str, Any], deadline: float) -> DiscoveredAddress | None:
        """This attempt's discovered endpoint; None means the configured endpoint."""
        request = op["discovery"]
        if request is None:
            return None
        assert self._discovery is not None
        if not request["required"] and self._discovery.failed_recently(request["key"]):
            return None

        def fetch() -> DiscoveredAddress:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("request deadline exceeded before endpoint discovery")
            return self._discovered(self._call(request["operation"], request["input"],
                                               timeout=min(remaining, self._timeout)))
        try:
            return self._discovery.resolve(request["key"], fetch, deadline - time.monotonic())
        except Exception:
            if request["required"]:
                raise
            self._discovery.record_failure(request["key"])
            return None

    def _send(self, name: str, input: Any, timeout: float | None, trace: _Trace) -> Any:
        op, url, body, headers = self._prepare(name, input)
        deadline = time.monotonic() + (self._timeout if timeout is None else timeout)
        # A one-shot stream is sent at most once, whatever the retry policy.
        replayable = not isinstance(body, ByteStream) or body.replayable
        last_cost: int | None = None
        for attempt in range(1, self._attempts + 1):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("request deadline exceeded")
            discovered = self._discover(op, deadline)
            attempt_url, endpoint, source = self._attempt_url(op, url, discovered)
            wait = self._retry.send_delay()
            if wait > 0:
                if wait >= deadline - time.monotonic():
                    self._retry.cancel_send()
                    raise TimeoutError("client-side rate limit delay exceeds request deadline")
                self._clock.sleep(wait)
            remaining = deadline - time.monotonic()
            upload = Upload(body, *op["streamLimits"]) if isinstance(body, ByteStream) else None
            request = self._http.build_request(op["method"], attempt_url, content=upload.sync() if upload else body,
                                               headers=headers, timeout=remaining)
            authentication = None
            preset = False
            for scheme, provider in self._auth_options(op):
                if preset_auth(request, scheme):
                    preset = True
                    break
                identity = provider() if callable(provider) else provider
                if inspect.isawaitable(identity):
                    if inspect.iscoroutine(identity):
                        identity.close()
                    raise TypeError("use AsyncClient with an async credential provider")
                if identity is not None:
                    authentication = scheme, identity
                    break
            if authentication is None and not preset:
                self._auth_required(op)
            for middleware in self._middleware:
                middleware(request)
            self._body_checks(op, request, body)
            if authentication is not None:
                apply_auth(request, *authentication, region=self._region, unsigned_payload=op.get("unsignedPayload", False))
            _check_credentials(request)
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("request deadline exceeded during preparation")
            request.extensions["timeout"] = {key: remaining for key in ("connect", "read", "write", "pool")}
            trace.attempt_started(op, request, endpoint, source)
            response = None
            failure: BaseException | None = None
            data = b""
            try:
                try:
                    response = self._http.send(request, stream=True)
                    if response.is_success and op["response"].get("streaming"):
                        try:
                            output = self._streamed_output(op, response, self._sync_body(name, op, response, deadline))
                        except BaseException:
                            response.close()
                            raise
                        self._succeeded(last_cost)
                        trace.attempt_finished(response, None, self._response_length(response))
                        return output
                    try:
                        data = b"" if response.is_success and not _has_body(op) else response.read()
                    finally:
                        response.close()
                except httpx.TransportError as error:
                    failure = error
                else:
                    if time.monotonic() >= deadline:
                        raise TimeoutError("request deadline exceeded while reading the response")
                    if response.is_success:
                        algorithm = checksums.response(op["resolvedChecksum"], response.headers, data)
                        if algorithm is not None and self._checksum_validated is not None:
                            self._checksum_validated(name, algorithm)
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
            finally:
                if upload is not None:
                    upload.close()
            assert failure is not None
            retry = self._after_failure(op, failure, response, attempt, replayable, discovered)
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

    def _sync_body(self, name: str, op: dict[str, Any], response: httpx.Response, deadline: float) -> ByteStream:
        """The response payload, pulled a received chunk at a time within the call's deadline."""
        found = checksums.expected(op["resolvedChecksum"], response.headers)
        hasher = checksums.Hasher(found[0]) if found is not None else None

        def chunks() -> Iterator[bytes]:
            try:
                iterator = response.iter_bytes()
                while True:
                    if time.monotonic() >= deadline:
                        raise TimeoutError("request deadline exceeded while reading the response body")
                    try:
                        chunk = next(iterator, None)
                    except httpx.TimeoutException as error:
                        raise TimeoutError("request deadline exceeded while reading the response body") from error
                    except httpx.TransportError as error:
                        raise NetworkError(str(error)) from error
                    if chunk is None:
                        break
                    if hasher is not None:
                        hasher.update(chunk)
                    yield chunk
                if found is not None and hasher is not None:
                    checksums.verify(found[0], found[1], hasher.base64())
                    if self._checksum_validated is not None:
                        self._checksum_validated(name, found[0])
            finally:
                response.close()
        return ByteStream(chunks, length=self._response_length(response), replayable=False, closer=response.close)

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

    def _wait(self, operation: str, name: str, input: Any, *, timeout: float,
              min_delay: float | None = None, max_delay: float | None = None) -> Any:
        op = self._codec.schema["operations"][operation]
        waiter = Waiter(name, op["waiters"][name], timeout, min_delay, max_delay)
        values = copy.deepcopy(input)
        query_input = self._query_value(op["input"], values)
        deadline = time.monotonic() + timeout
        attempts = 0
        output: Any = None
        error: Exception | None = None
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise WaiterTimeout(name, attempts, response=output, error=error)
            attempts += 1
            try:
                output = self._call(operation, values, timeout=min(remaining, self._timeout))
                error = None
            except Exception as caught:
                output, error = None, caught
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                stream = self._output_stream(op, output)
                if stream is not None:
                    stream.close()
                raise WaiterTimeout(name, attempts, response=output, error=error)
            state = waiter.state(query_input, self._query_value(op["output"], output), error)
            if state == "success":
                return output
            # A response the waiter does not return releases its connection.
            stream = self._output_stream(op, output)
            if stream is not None:
                stream.close()
            if state == "failure":
                raise WaiterError(name, attempts, response=output, error=error, message=waiter.message) from error
            time.sleep(waiter.delay(attempts, remaining))


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
        # The same deadline bounds a streamed response body read after the call returns.
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

    async def _discover(self, op: dict[str, Any], deadline: float) -> DiscoveredAddress | None:
        """This attempt's discovered endpoint; None means the configured endpoint."""
        request = op["discovery"]
        if request is None:
            return None
        assert self._discovery is not None
        if not request["required"] and self._discovery.failed_recently(request["key"]):
            return None
        loop = asyncio.get_running_loop()

        async def fetch() -> DiscoveredAddress:
            remaining = deadline - loop.time()
            if remaining <= 0:
                raise TimeoutError("request deadline exceeded before endpoint discovery")
            return self._discovered(await self._call(request["operation"], request["input"],
                                                     timeout=min(remaining, self._timeout)))
        try:
            return await self._discovery.aresolve(request["key"], fetch)
        except Exception:
            if request["required"]:
                raise
            self._discovery.record_failure(request["key"])
            return None


    async def _wait(self, operation: str, name: str, input: Any, *, timeout: float,
                    min_delay: float | None = None, max_delay: float | None = None) -> Any:
        op = self._codec.schema["operations"][operation]
        waiter = Waiter(name, op["waiters"][name], timeout, min_delay, max_delay)
        values = copy.deepcopy(input)
        query_input = self._query_value(op["input"], values)
        attempts = 0
        output: Any = None
        error: Exception | None = None
        deadline = time.monotonic() + timeout
        try:
            async with asyncio.timeout(timeout):
                while True:
                    remaining = deadline - time.monotonic()
                    attempts += 1
                    try:
                        output = await self._call(operation, values, timeout=min(remaining, self._timeout))
                        error = None
                    except Exception as caught:
                        output, error = None, caught
                    state = "timeout" if time.monotonic() >= deadline else waiter.state(
                        query_input, self._query_value(op["output"], output), error)
                    if state == "success":
                        return output
                    stream = self._output_stream(op, output)
                    if stream is not None:
                        await stream.aclose()
                    if state == "timeout":
                        raise WaiterTimeout(name, attempts, response=output, error=error)
                    if state == "failure":
                        raise WaiterError(name, attempts, response=output, error=error, message=waiter.message) from error
                    await asyncio.sleep(waiter.delay(attempts, deadline - time.monotonic()))
        except TimeoutError as caught:
            raise WaiterTimeout(name, attempts, response=output, error=error) from caught

    async def _send(self, name: str, input: Any, timeout: float | None, deadline: float, trace: _Trace) -> Any:
        op, url, body, headers = self._prepare(name, input)
        replayable = not isinstance(body, ByteStream) or body.replayable
        last_cost: int | None = None
        for attempt in range(1, self._attempts + 1):
            discovered = await self._discover(op, deadline)
            attempt_url, endpoint, source = self._attempt_url(op, url, discovered)
            wait = self._retry.send_delay()
            if wait > 0:
                try:
                    await self._clock.asleep(wait)
                except BaseException:
                    self._retry.cancel_send()
                    raise
            upload = Upload(body, *op["streamLimits"]) if isinstance(body, ByteStream) else None
            request = self._http.build_request(op["method"], attempt_url, content=upload.asynchronous() if upload else body,
                                               headers=headers, timeout=self._timeout if timeout is None else timeout)
            authentication = None
            preset = False
            for scheme, provider in self._auth_options(op):
                if preset_auth(request, scheme):
                    preset = True
                    break
                identity = provider() if callable(provider) else provider
                if inspect.isawaitable(identity):
                    identity = await identity
                if identity is not None:
                    authentication = scheme, identity
                    break
            if authentication is None and not preset:
                self._auth_required(op)
            for middleware in self._middleware:
                result = middleware(request)
                if inspect.isawaitable(result):
                    await result
            self._body_checks(op, request, body)
            if authentication is not None:
                apply_auth(request, *authentication, region=self._region, unsigned_payload=op.get("unsignedPayload", False))
            _check_credentials(request)
            trace.attempt_started(op, request, endpoint, source)
            response = None
            failure: BaseException | None = None
            data = b""
            try:
                try:
                    response = await self._http.send(request, stream=True)
                    if response.is_success and op["response"].get("streaming"):
                        try:
                            output = self._streamed_output(op, response, self._async_body(name, op, response, deadline))
                        except BaseException:
                            await response.aclose()
                            raise
                        self._succeeded(last_cost)
                        trace.attempt_finished(response, None, self._response_length(response))
                        return output
                    try:
                        data = b"" if response.is_success and not _has_body(op) else await response.aread()
                    finally:
                        await response.aclose()
                except httpx.TransportError as error:
                    failure = error
                else:
                    if response.is_success:
                        algorithm = checksums.response(op["resolvedChecksum"], response.headers, data)
                        if algorithm is not None and self._checksum_validated is not None:
                            self._checksum_validated(name, algorithm)
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
            finally:
                # Also runs on cancellation, so the caller's source is closed.
                if upload is not None:
                    await upload.aclose()
            assert failure is not None
            retry = self._after_failure(op, failure, response, attempt, replayable, discovered)
            trace.attempt_finished(response, failure, len(data) if response is not None else None, retry)
            if retry is None:
                if isinstance(failure, httpx.TransportError):
                    raise NetworkError(str(failure)) from failure
                raise failure
            last_cost = retry[2]
            await self._clock.asleep(retry[1])
        raise AssertionError("unreachable")

    def _async_body(self, name: str, op: dict[str, Any], response: httpx.Response, deadline: float) -> ByteStream:
        """The response payload, read on demand; the call's deadline still bounds every read."""
        found = checksums.expected(op["resolvedChecksum"], response.headers)
        hasher = checksums.Hasher(found[0]) if found is not None else None

        async def chunks() -> AsyncIterator[bytes]:
            try:
                iterator = response.aiter_bytes()
                while True:
                    try:
                        async with asyncio.timeout_at(deadline):
                            chunk = await anext(iterator, None)
                    except httpx.TimeoutException as error:
                        raise TimeoutError("request deadline exceeded while reading the response body") from error
                    except httpx.TransportError as error:
                        raise NetworkError(str(error)) from error
                    if chunk is None:
                        break
                    if hasher is not None:
                        hasher.update(chunk)
                    yield chunk
                if found is not None and hasher is not None:
                    checksums.verify(found[0], found[1], hasher.base64())
                    if self._checksum_validated is not None:
                        self._checksum_validated(name, found[0])
            finally:
                await response.aclose()
        return ByteStream(chunks, length=self._response_length(response), replayable=False, closer=response.aclose)

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
