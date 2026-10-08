"""Credential providers and HTTP authentication for generated clients."""
from __future__ import annotations

import asyncio
import base64
import configparser
import hashlib
import hmac
import inspect
import os
import posixpath
import threading
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Callable
from urllib.parse import quote, unquote

import httpx


@dataclass(frozen=True)
class BasicCredentials:
    """HTTP Basic username and password. The password is omitted from repr."""
    username: str
    password: str = field(repr=False)

    def __post_init__(self) -> None:
        if ":" in self.username:
            raise ValueError("Basic authentication username cannot contain a colon")


@dataclass(frozen=True)
class AwsCredentials:
    """AWS signing credentials with optional timezone-aware expiry; secrets are omitted from repr."""
    access_key_id: str = field(repr=False)
    secret_access_key: str = field(repr=False)
    session_token: str | None = field(default=None, repr=False)
    expires_at: datetime | None = None

    def __post_init__(self) -> None:
        if not self.access_key_id or not self.secret_access_key:
            raise ValueError("AWS access key and secret key must be nonempty")
        if self.expires_at is not None and self.expires_at.tzinfo is None:
            raise ValueError("credential expiry must be timezone aware")


def credential_chain(*providers: Callable[[], Any]) -> Callable[[], Any]:
    """Try providers in order; None means unavailable, errors remain errors."""
    def resolve() -> Any:
        for provider in providers:
            value = provider()
            if inspect.isawaitable(value):
                if inspect.iscoroutine(value):
                    value.close()
                raise TypeError("use async_credential_chain with async providers")
            if value is not None:
                return value
        return None
    return resolve


def async_credential_chain(*providers: Callable[[], Any]) -> Callable[[], Any]:
    """Try asynchronous or synchronous providers in order; None means unavailable, errors propagate."""
    async def resolve() -> Any:
        for provider in providers:
            value = provider()
            if inspect.isawaitable(value):
                value = await value
            if value is not None:
                return value
        return None
    return resolve


def environment_credentials() -> AwsCredentials | None:
    """Read static AWS environment credentials; partial identities raise ValueError."""
    access = os.environ.get("AWS_ACCESS_KEY_ID")
    secret = os.environ.get("AWS_SECRET_ACCESS_KEY")
    if not access and not secret:
        return None
    if not access or not secret:
        raise ValueError("AWS environment credentials are incomplete")
    return AwsCredentials(access, secret, os.environ.get("AWS_SESSION_TOKEN"))


def profile_credentials(profile: str | None = None, path: str | Path | None = None) -> AwsCredentials | None:
    """Read static credentials; never execute credential_process from a file."""
    source = Path(path or os.environ.get("AWS_SHARED_CREDENTIALS_FILE") or Path.home() / ".aws/credentials")
    config = configparser.RawConfigParser()
    if not source.exists():
        return None
    with source.open(encoding="utf-8") as stream:
        config.read_file(stream)
    name = profile or os.environ.get("AWS_PROFILE", "default")
    if not config.has_section(name):
        return None
    values = config[name]
    access, secret = values.get("aws_access_key_id"), values.get("aws_secret_access_key")
    if not access and not secret:
        return None
    if not access or not secret:
        raise ValueError("AWS profile credentials are incomplete")
    return AwsCredentials(access, secret, values.get("aws_session_token"))


class CachedCredentials:
    """Share a provider across threads and refresh expiring identities once."""
    def __init__(self, provider: Callable[[], Any], *, refresh_before: float = 60,
                 clock: Callable[[], datetime] = lambda: datetime.now(timezone.utc)) -> None:
        if refresh_before < 0:
            raise ValueError("refresh_before cannot be negative")
        self._provider, self._clock = provider, clock
        self._skew = timedelta(seconds=refresh_before)
        self._value: AwsCredentials | None = None
        self._refresh_at: datetime | None = None
        self._lock = threading.Lock()

    def _fresh(self) -> bool:
        return self._value is not None and (self._refresh_at is None or self._refresh_at > self._clock())

    def _store(self, value: Any) -> AwsCredentials | None:
        if value is not None and not isinstance(value, AwsCredentials):
            raise TypeError("credential provider must return AwsCredentials or None")
        if value is not None and value.expires_at is not None and value.expires_at <= self._clock():
            raise ValueError("credential provider returned expired credentials")
        self._refresh_at = None
        if value is not None and value.expires_at is not None:
            remaining = value.expires_at - self._clock()
            # Short-lived credentials must remain reusable instead of making
            # every waiter on the refresh lock invoke the provider again.
            self._refresh_at = value.expires_at - min(self._skew, remaining / 2)
        self._value = value
        return value

    def __call__(self) -> AwsCredentials | None:
        with self._lock:
            if self._fresh():
                return self._value
            value = self._provider()
            if inspect.isawaitable(value):
                if inspect.iscoroutine(value):
                    value.close()
                raise TypeError("use AsyncCachedCredentials with async providers")
            return self._store(value)


class AsyncCachedCredentials(CachedCredentials):
    """One refresh in flight per event loop; cancellation releases the lock."""
    def __init__(self, provider: Callable[[], Any], **options: Any) -> None:
        super().__init__(provider, **options)
        self._async_lock = asyncio.Lock()

    async def __call__(self) -> AwsCredentials | None:  # type: ignore[override]
        async with self._async_lock:
            if self._fresh():
                return self._value
            value = self._provider()
            if inspect.isawaitable(value):
                value = await value
            return self._store(value)


def sign_v4(request: httpx.Request, credentials: AwsCredentials, *, service: str,
            region: str, now: datetime | None = None, unsigned_payload: bool = False,
            disable_double_encoding: bool | None = None, disable_normalize_path: bool | None = None) -> None:
    """Sign the final HTTP request using AWS Signature Version 4."""
    now = now or datetime.now(timezone.utc)
    if now.tzinfo is None:
        raise ValueError("signing time must be timezone aware")
    now = now.astimezone(timezone.utc)
    if credentials.expires_at is not None and credentials.expires_at <= now:
        raise ValueError("AWS credentials have expired")
    if not service or not region or any(c in service + region for c in "/\r\n"):
        raise ValueError("SigV4 service and region must be nonempty scope components")
    request.headers.pop("authorization", None)
    request.headers["x-amz-date"] = now.strftime("%Y%m%dT%H%M%SZ")
    if credentials.session_token is not None:
        request.headers["x-amz-security-token"] = credentials.session_token
    else:
        request.headers.pop("x-amz-security-token", None)
    try:
        content: bytes | None = request.content
    except httpx.RequestNotRead:
        content = None  # A streamed body, which is never read here to hash it.
    if unsigned_payload:
        digest = "UNSIGNED-PAYLOAD"
    elif content is not None:
        digest = hashlib.sha256(content).hexdigest()
    else:
        declared = request.extensions.get("crucible.sha256")
        if not isinstance(declared, str):
            raise ValueError("SigV4 needs the SHA-256 of a streaming body: declare it with "
                             "ByteStream.with_sha256, or model the operation with an unsigned payload")
        digest = declared
    request.headers["x-amz-content-sha256"] = digest
    # S3 preserves repeated slashes and already escaped path bytes. Other AWS
    # services normalize the path and URI-encode its percent escapes again.
    path = request.url.raw_path.split(b"?", 1)[0].decode("ascii") or "/"
    if not (service == "s3" if disable_normalize_path is None else disable_normalize_path):
        trailing = path.endswith("/")
        path = posixpath.normpath(path)
        path = "/" + path.lstrip("/")
        if trailing and not path.endswith("/"):
            path += "/"
    if not (service == "s3" if disable_double_encoding is None else disable_double_encoding):
        path = quote(path, safe="/-._~")
    query = []
    for item in request.url.query.decode("ascii").split("&"):
        if not item:
            continue
        key, _, value = item.partition("=")
        query.append((quote(unquote(key), safe="-._~"), quote(unquote(value), safe="-._~")))
    canonical_query = "&".join(f"{key}={value}" for key, value in sorted(query))
    excluded = {"authorization", "connection", "expect", "transfer-encoding", "user-agent", "x-amzn-trace-id", "accept-encoding", "content-length"}
    headers: dict[str, list[str]] = {}
    for key, value in request.headers.multi_items():
        if key.lower() not in excluded:
            headers.setdefault(key.lower(), []).append(" ".join(value.split()))
    names = sorted(headers)
    signed = ";".join(names)
    canonical_headers = "".join(key + ":" + ",".join(headers[key]) + "\n" for key in names)
    canonical = "\n".join((request.method, path, canonical_query, canonical_headers, signed, digest))
    date = now.strftime("%Y%m%d")
    scope = f"{date}/{region}/{service}/aws4_request"
    string_to_sign = f"AWS4-HMAC-SHA256\n{request.headers['x-amz-date']}\n{scope}\n{hashlib.sha256(canonical.encode()).hexdigest()}"
    key_bytes = ("AWS4" + credentials.secret_access_key).encode()
    for component in (date, region, service, "aws4_request"):
        key_bytes = hmac.new(key_bytes, component.encode(), hashlib.sha256).digest()
    signature = hmac.new(key_bytes, string_to_sign.encode(), hashlib.sha256).hexdigest()
    request.headers["authorization"] = f"AWS4-HMAC-SHA256 Credential={credentials.access_key_id}/{scope}, SignedHeaders={signed}, Signature={signature}"


def preset_auth(request: httpx.Request, scheme: dict[str, Any]) -> bool:
    """Resolve the operation authentication scheme from modeled policy and configured identity sources."""
    kind = scheme["kind"]
    if kind == "apiKey":
        if scheme["location"] == "query":
            value = request.url.params.get(scheme["name"], "")
            present = bool(value)
        else:
            value = request.headers.get(scheme["name"], "")
            prefix = scheme.get("scheme")
            present = bool(value) if not prefix else value.lower().startswith(prefix.lower() + " ") and len(value) > len(prefix) + 1
    else:
        prefix = {"bearer": "Bearer", "basic": "Basic", "sigV4": "AWS4-HMAC-SHA256"}.get(kind)
        value = request.headers.get("authorization", "")
        present = prefix is not None and value.lower().startswith(prefix.lower() + " ") and len(value) > len(prefix) + 1
    if present:
        request.extensions["crucible.credentials"] = True
    return present


def apply_auth(request: httpx.Request, scheme: dict[str, Any], identity: Any, *, region: str | None,
               unsigned_payload: bool = False) -> None:
    """Apply the selected authentication to an HTTPX request before sending."""
    kind = scheme["kind"]
    if kind == "bearer":
        request.headers["authorization"] = f"Bearer {identity}"
    elif kind == "basic":
        if not isinstance(identity, BasicCredentials):
            raise TypeError("basic_auth must provide BasicCredentials")
        value = base64.b64encode(f"{identity.username}:{identity.password}".encode()).decode("ascii")
        request.headers["authorization"] = f"Basic {value}"
    elif kind == "apiKey":
        if not isinstance(identity, str) or not identity:
            raise TypeError("api_key must provide a nonempty string")
        if scheme["location"] == "query":
            request.url = request.url.copy_set_param(scheme["name"], identity)
        else:
            request.headers[scheme["name"]] = (scheme["scheme"] + " " if scheme.get("scheme") else "") + identity
    elif kind == "sigV4":
        if not isinstance(identity, AwsCredentials):
            raise TypeError("credentials must provide AwsCredentials")
        region = scheme.get("signingRegion", region)
        if region is None:
            raise ValueError("region is required for SigV4")
        sign_v4(request, identity, service=scheme["signingName"], region=region, unsigned_payload=unsigned_payload,
                disable_double_encoding=scheme.get("disableDoubleEncoding"), disable_normalize_path=scheme.get("disableNormalizePath"))
    else:
        raise ValueError("unsupported authentication scheme")
    request.extensions["crucible.credentials"] = True
