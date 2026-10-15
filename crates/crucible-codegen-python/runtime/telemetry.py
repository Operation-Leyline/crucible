"""Client telemetry: a dependency-free hook interface with redacted events.

Events never carry bodies, credentials, or the values of sensitive members.
docs/sdk-runtime-policy.md is the cross-language contract.
"""
from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Any, Mapping

REDACTED = "***"

# Always redacted, whatever the model says.
CREDENTIAL_HEADERS = frozenset({
    "authorization", "proxy-authorization", "cookie", "set-cookie",
})

_log = logging.getLogger(__name__)


@dataclass(frozen=True)
class OperationStarted:
    """Call-start event with invocation identifier, service, operation and monotonic time."""
    invocation_id: str
    service: str
    operation: str
    time: float


@dataclass(frozen=True)
class AttemptStarted:
    """Request-attempt event with credential/sensitive headers and target values redacted."""
    invocation_id: str
    service: str
    operation: str
    attempt: int
    method: str
    endpoint: str
    """Scheme, host and port only."""
    target: str
    """Path and query, with sensitive and credential values redacted."""
    headers: Mapping[str, str]
    """Request headers as sent, with sensitive and credential values redacted."""
    request_bytes: int | None
    time: float


@dataclass(frozen=True)
class AttemptFinished:
    """Attempt-result event with safe error classification, latency and retry decision."""
    invocation_id: str
    service: str
    operation: str
    attempt: int
    status: int | None
    error: str | None
    """A modeled error's name or the exception class name; never a message."""
    request_id: str | None
    response_bytes: int | None
    latency: float
    retry_reason: str | None
    """Set when another attempt follows: throttling, transient, timeout or network."""
    retry_delay: float | None
    time: float


@dataclass(frozen=True)
class OperationFinished:
    """Final call event with total attempts, status, safe error classification and latency."""
    invocation_id: str
    service: str
    operation: str
    attempts: int
    status: int | None
    error: str | None
    latency: float
    time: float


class TelemetryHook:
    """Override any of these. A hook that raises is logged and never changes the call's outcome."""

    def operation_started(self, event: OperationStarted) -> None:
        """Observe a logical call starting. Hook failures are isolated from the call."""
        pass

    def attempt_started(self, event: AttemptStarted) -> None:
        """Observe a redacted outgoing attempt. Hook failures are isolated from the call."""
        pass

    def attempt_finished(self, event: AttemptFinished) -> None:
        """Observe an attempt result and retry decision. Hook failures are isolated from the call."""
        pass

    def operation_finished(self, event: OperationFinished) -> None:
        """Observe logical call completion. Hook failures are isolated from the call."""
        pass


def emit(hook: TelemetryHook | None, method: str, event: Any) -> None:
    """Invoke a telemetry hook; log hook failures without changing call results."""
    if hook is None:
        return
    try:
        getattr(hook, method)(event)
    except Exception:
        _log.exception("telemetry hook %s failed", method)


def redact_headers(headers: Any, sensitive: frozenset[str]) -> dict[str, str]:
    """Copy headers and replace sensitive or credential values with the redaction marker."""
    return {name: REDACTED if name in CREDENTIAL_HEADERS or name in sensitive else value
            for name, value in ((key.lower(), value) for key, value in headers.items())}


def redact_query(pairs: list[tuple[str, str]], sensitive: frozenset[str]) -> list[tuple[str, str]]:
    """Names compare case-insensitively: redacting too much is the safe failure."""
    return [(name, REDACTED if name.lower() in sensitive else value)
            for name, value in pairs]


class OpenTelemetryHook(TelemetryHook):
    """One span per operation on any OpenTelemetry-compatible tracer, attempts as span events.

    The tracer is duck-typed (``start_span(name, attributes=...)`` returning a span
    with ``set_attribute``, ``add_event`` and ``end``), so the OpenTelemetry API is
    not a dependency of the generated package.
    """

    def __init__(self, tracer: Any) -> None:
        self._tracer = tracer
        self._spans: dict[str, Any] = {}

    def operation_started(self, event: OperationStarted) -> None:
        """Observe a logical call starting. Hook failures are isolated from the call."""
        span = self._tracer.start_span(f"{event.service.rsplit('#', 1)[-1]}.{event.operation}", attributes={
            "rpc.system": "smithy", "rpc.service": event.service, "rpc.method": event.operation,
        })
        self._spans[event.invocation_id] = span

    def attempt_started(self, event: AttemptStarted) -> None:
        """Observe a redacted outgoing attempt. Hook failures are isolated from the call."""
        span = self._spans.get(event.invocation_id)
        if span is None:
            return
        span.set_attribute("server.address", event.endpoint)
        span.set_attribute("http.request.method", event.method)
        if event.attempt > 1:
            span.set_attribute("http.request.resend_count", event.attempt - 1)

    def attempt_finished(self, event: AttemptFinished) -> None:
        """Observe an attempt result and retry decision. Hook failures are isolated from the call."""
        span = self._spans.get(event.invocation_id)
        if span is None:
            return
        attributes: dict[str, Any] = {"crucible.attempt": event.attempt, "crucible.latency": event.latency}
        for key, value in (("http.response.status_code", event.status), ("error.type", event.error),
                           ("crucible.retry_reason", event.retry_reason), ("crucible.request_id", event.request_id)):
            if value is not None:
                attributes[key] = value
        span.add_event("crucible.attempt", attributes=attributes)

    def operation_finished(self, event: OperationFinished) -> None:
        """Observe logical call completion. Hook failures are isolated from the call."""
        span = self._spans.pop(event.invocation_id, None)
        if span is None:
            return
        span.set_attribute("crucible.attempts", event.attempts)
        if event.status is not None:
            span.set_attribute("http.response.status_code", event.status)
        if event.error is not None:
            span.set_attribute("error.type", event.error)
            try:
                from opentelemetry.trace import Status, StatusCode  # type: ignore[import-not-found, unused-ignore]
            except ImportError:
                pass
            else:
                span.set_status(Status(StatusCode.ERROR))
        span.end()
