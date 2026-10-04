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
import random
import time
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from email.utils import format_datetime, parsedate_to_datetime
from enum import StrEnum
from typing import Any, Callable, Self
from urllib.parse import quote, urlencode, urlsplit

import httpx


class Unset:
    """An omitted member, distinct from an explicit JSON null."""
    def __repr__(self) -> str:
        return "UNSET"

    def __copy__(self) -> Unset:
        return self

    def __deepcopy__(self, memo: Any) -> Unset:
        return self


UNSET = Unset()


class OpenEnum(StrEnum):
    @classmethod
    def _missing_(cls, value: Any) -> OpenEnum | None:
        if not isinstance(value, str):
            return None
        unknown = str.__new__(cls, value)
        unknown._name_ = "UNKNOWN"
        unknown._value_ = value
        return unknown


@dataclass(frozen=True)
class ResponseMetadata:
    status: int
    request_id: str | None
    headers: httpx.Headers


class ServiceError(Exception):
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


class NetworkError(Exception):
    """Transport failed before a complete response could be read."""


def _has_body(op: dict[str, Any]) -> bool:
    """Whether a successful response carries a body to read."""
    response = op["response"]
    return bool(response["body"]) or response.get("payload") is not None


def _check_credentials(request: httpx.Request) -> None:
    if "authorization" not in request.headers or request.url.scheme == "https":
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
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
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


class _Codec:
    def __init__(self, schema: dict[str, Any], classes: dict[str, Any]) -> None:
        self.schema = schema
        self.shapes = schema["shapes"]
        self.classes = classes

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
        if kind == "timestamp":
            return _timestamp(value, fmt or shape["format"], decode)
        if kind == "blob":
            return _blob(value, decode)
        if kind == "enum":
            return self.classes[target](value) if decode else str(value)
        if kind == "list":
            member = shape["member"]
            return [self.convert(member["target"], v, decode=decode, fmt=member["format"]) for v in value]
        if kind == "map":
            member = shape["value"]
            return {self.convert(shape["key"]["target"], k, decode=decode): self.convert(member["target"], v, decode=decode, fmt=member["format"]) for k, v in value.items()}
        if kind in ("structure", "union"):
            if decode and not isinstance(value, dict):
                raise ValueError(f"expected an object for {shape['name']}")
            out: dict[str, Any] = {}
            for wire, m in shape["members"].items():
                native = m["attribute"]
                item = value.get(wire, UNSET) if isinstance(value, dict) else getattr(value, native, UNSET)
                if isinstance(item, Unset) and decode and m["default"] is not None:
                    item = copy.deepcopy(m["default"])
                if isinstance(item, Unset):
                    continue
                out[native if decode else wire] = self.convert(m["target"], item, decode=decode, fmt=m["format"])
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
                        out[unknown[0]] = unknown[1]
                    if len(out) != 1:
                        raise ValueError("a union must contain exactly one member")
            return self.classes[target](**out) if decode else out
        if kind in ("byte", "short", "integer", "long"):
            bits = {"byte": 8, "short": 16, "integer": 32, "long": 64}[kind]
            if isinstance(value, bool) or not isinstance(value, int) or not -(2 ** (bits - 1)) <= value < 2 ** (bits - 1):
                raise ValueError(f"{kind} requires a signed {bits}-bit integer")
        return value

    def bound(self, member: dict[str, Any], value: Any, *, header: bool = False) -> str:
        shape = self.shapes[member["target"]]
        if shape["kind"] == "list":
            items = [self.bound(shape["member"], item, header=True) for item in value]
            return ", ".join(json.dumps(x) if any(c in x for c in ',"\\') or x != x.strip() else x for x in items)
        if shape["kind"] == "timestamp":
            value = _timestamp(value, member["format"], False)
        if isinstance(value, bool):
            return "true" if value else "false"
        return str(value)

    def read_header(self, m: dict[str, Any], text: str) -> Any:
        shape = self.shapes[m["target"]]
        kind = shape["kind"]
        if kind == "list":
            if not text.strip():
                return []
            # JSON quoting rules are also used for commas and quotes in header lists.
            parts: list[str] = []
            start = 0
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
            return [self.read_header(shape["member"], json.loads(p) if p.startswith('"') else p) for p in parts]
        if kind == "timestamp":
            return _timestamp(text, m["format"], True)
        if kind in ("byte", "short", "integer", "long"):
            return self.convert(m["target"], int(text), decode=True)
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
                 max_attempts: int = 3, base_delay: float = 0.1, max_delay: float = 20,
                 timeout: float = 30, content_sha256: bool = False,
                 middleware: list[Callable[..., Any]] | None = None) -> None:
        if endpoint is None or urlsplit(endpoint).scheme not in ("http", "https") or not urlsplit(endpoint).netloc:
            raise ValueError("an absolute HTTP(S) endpoint is required")
        if urlsplit(endpoint).query or urlsplit(endpoint).fragment:
            raise ValueError("the endpoint must not contain a query or fragment")
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
        self._digest = content_sha256
        self._middleware = middleware or []

    def _prepare(self, name: str, input: Any) -> tuple[dict[str, Any], str, bytes | None, dict[str, str]]:
        op = self._codec.schema["operations"][name]
        values = self._codec.convert(op["input"], input) or {}
        token = op["token"]
        if token is not None and values.get(token) is None:
            values[token] = str(uuid.uuid4())
        shape = self._codec.shapes.get(op["input"], {})
        members = shape.get("members", {})
        # Binding timestamps must use the model values, not already encoded JSON timestamps.
        def original(key: str) -> Any:
            m = members[key]
            return input.get(key, UNSET) if isinstance(input, dict) else getattr(input, m["attribute"], UNSET)
        uri = op["uri"]
        for key in op["request"]["labels"]:
            value = original(key)
            if value is None or isinstance(value, Unset) or str(value) == "":
                raise ValueError(f"{key} is required")
            uri = uri.replace("{" + key + "}", quote(self._codec.bound(members[key], value), safe="-._~"))
        query: list[tuple[str, str]] = []
        for key, wire in op["request"]["query"]:
            value = original(key)
            if key == token:
                value = values.get(key, UNSET)
            if value is None or isinstance(value, Unset):
                continue
            member = members[key]
            shape = self._codec.shapes[member["target"]]
            if shape["kind"] == "list":
                query.extend((wire, self._codec.bound(shape["member"], item)) for item in value)
            else:
                query.append((wire, self._codec.bound(member, value)))
        if query:
            uri += ("&" if "?" in uri else "?") + urlencode(query, quote_via=quote, safe="-._~")
        headers = dict(self._headers)
        for key, wire in op["request"]["headers"]:
            value = values.get(key, UNSET) if key == token else original(key)
            if value is not None and not isinstance(value, Unset):
                headers[wire] = self._codec.bound(members[key], value, header=True)
        body = None
        if op["request"]["body"]:
            body = json.dumps({k: values[k] for k in op["request"]["body"] if k in values}, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode()
            headers["content-type"] = "application/json"
        payload = op["request"].get("payload")
        if payload is not None:
            # The bytes go as they are: a body digest covers exactly these.
            value = original(payload[0])
            if value is not None and not isinstance(value, Unset):
                if not isinstance(value, (bytes, bytearray, memoryview)):
                    raise TypeError(f"{payload[0]} must be bytes")
                body = bytes(value)
                headers["content-type"] = payload[1]
        return op, self._endpoint + uri, body, headers

    def _decode(self, op: dict[str, Any], response: httpx.Response, body: bytes) -> Any:
        if response.is_success:
            target = op["output"]
            if target is None:
                return None
            payload = op["response"].get("payload")
            # A payload is the body as received, whatever it holds.
            values: dict[str, Any] = {payload[0]: body} if payload is not None else json.loads(body) if body else {}
            for member, header in op["response"]["headers"]:
                if header in response.headers:
                    m = self._codec.shapes[target]["members"][member]
                    parsed = self._codec.read_header(m, response.headers[header])
                    values[member] = self._codec.convert(m["target"], parsed, fmt=m["format"])
            return self._codec.convert(target, values, decode=True)
        try:
            fields = json.loads(body)
        except (ValueError, UnicodeDecodeError):
            fields = {}
        if not isinstance(fields, dict):
            fields = {}
        name = fields.get("__type")
        if not isinstance(name, str):
            name = None
        declared = {self._codec.shapes[t]["name"]: t for t in op["errors"]}
        if name is None:
            matches = [n for n, t in declared.items() if self._codec.shapes[t]["error"]["status"] == response.status_code]
            if len(matches) == 1:
                name = matches[0]
        target = declared.get(name)
        metadata = ResponseMetadata(response.status_code, response.headers.get("x-request-id") or fields.get("requestId"), response.headers)
        converted: dict[str, Any] = {}
        error: dict[str, Any] = {}
        cls = ServiceError
        if target is not None:
            shape = self._codec.shapes[target]
            error = shape["error"]
            cls = self._codec.classes[target]
            for key, m in shape["members"].items():
                converted[m["attribute"]] = self._codec.convert(m["target"], fields.get(key, UNSET), decode=True, fmt=m["format"])
            for member, header in error["headers"]:
                if header in response.headers:
                    m = shape["members"][member]
                    converted[m["attribute"]] = self._codec.read_header(m, response.headers[header])
        raise cls(name=name or "UnknownError", message=str(fields.get("message", f"HTTP {response.status_code}")),
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
        return random.uniform(0, min(self._max_delay, self._base_delay * 2 ** min(attempt - 1, 30)))

    def _can_retry(self, op: dict[str, Any], error: ServiceError) -> bool:
        return error.throttling or op["safe"] and (error.retryable or error.metadata.status in (500, 502, 503, 504))

    def _next_page(self, name: str, page: Any, seen: set[str]) -> Any:
        op = self._codec.schema["operations"][name]
        value = self._codec.convert(op["output"], page)
        for part in op["pagination"]["output"]:
            value = value.get(part) if isinstance(value, dict) else None
        if value is None or value == "":
            return None
        if value in seen:
            raise ValueError("pagination token did not advance")
        seen.add(value)
        return value


class SyncClient(_Base):
    def __init__(self, *, http: httpx.Client | None = None, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._http = http or httpx.Client()
        self._owns_http = http is None

    def close(self) -> None:
        if self._owns_http:
            self._http.close()

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *args: Any) -> None:
        self.close()

    def _call(self, name: str, input: Any = None, *, timeout: float | None = None) -> Any:
        op, url, body, headers = self._prepare(name, input)
        deadline = time.monotonic() + (self._timeout if timeout is None else timeout)
        for attempt in range(1, self._attempts + 1):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("request deadline exceeded")
            request = self._http.build_request(op["method"], url, content=body, headers=headers, timeout=remaining)
            token = self._token() if callable(self._token) else self._token
            if inspect.isawaitable(token):
                raise TypeError("use AsyncClient with an async token provider")
            if token is not None and self._codec.schema["bearer"]:
                request.headers["authorization"] = f"Bearer {token}"
            for middleware in self._middleware:
                middleware(request)
            _check_credentials(request)
            if self._digest and request.content:
                request.headers["x-amz-content-sha256"] = hashlib.sha256(request.content).hexdigest()
            response = None
            try:
                response = self._http.send(request, stream=True)
                try:
                    data = b"" if response.is_success and not _has_body(op) else response.read()
                finally:
                    response.close()
            except httpx.TransportError as error:
                if not op["safe"] or attempt == self._attempts:
                    raise NetworkError(str(error)) from error
            else:
                try:
                    return self._decode(op, response, data)
                except ServiceError as error:
                    if not self._can_retry(op, error) or attempt == self._attempts:
                        raise
            delay = self._delay(attempt, response)
            if delay >= deadline - time.monotonic():
                raise TimeoutError("retry delay exceeds request deadline")
            time.sleep(delay)
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
    def __init__(self, *, http: httpx.AsyncClient | None = None, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._http = http or httpx.AsyncClient()
        self._owns_http = http is None

    async def aclose(self) -> None:
        if self._owns_http:
            await self._http.aclose()

    async def __aenter__(self) -> Self:
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self.aclose()

    async def _call(self, name: str, input: Any = None, *, timeout: float | None = None) -> Any:
        async with asyncio.timeout(self._timeout if timeout is None else timeout):
            return await self._send(name, input, timeout)

    async def _send(self, name: str, input: Any, timeout: float | None) -> Any:
        op, url, body, headers = self._prepare(name, input)
        for attempt in range(1, self._attempts + 1):
            request = self._http.build_request(op["method"], url, content=body, headers=headers,
                                               timeout=self._timeout if timeout is None else timeout)
            token = self._token() if callable(self._token) else self._token
            if inspect.isawaitable(token):
                token = await token
            if token is not None and self._codec.schema["bearer"]:
                request.headers["authorization"] = f"Bearer {token}"
            for middleware in self._middleware:
                result = middleware(request)
                if inspect.isawaitable(result):
                    await result
            _check_credentials(request)
            if self._digest and request.content:
                request.headers["x-amz-content-sha256"] = hashlib.sha256(request.content).hexdigest()
            response = None
            try:
                response = await self._http.send(request, stream=True)
                try:
                    data = b"" if response.is_success and not _has_body(op) else await response.aread()
                finally:
                    await response.aclose()
            except httpx.TransportError as error:
                if not op["safe"] or attempt == self._attempts:
                    raise NetworkError(str(error)) from error
            else:
                try:
                    return self._decode(op, response, data)
                except ServiceError as error:
                    if not self._can_retry(op, error) or attempt == self._attempts:
                        raise
            await asyncio.sleep(self._delay(attempt, response))
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
