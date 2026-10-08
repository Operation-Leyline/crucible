"""Endpoint discovery: a per-client cache of discovered endpoints with single-flight refresh."""
from __future__ import annotations

import asyncio
import json
import threading
from dataclasses import dataclass
from typing import Any, Awaitable, Callable

# How long an optional operation keeps using the configured endpoint after discovery failed.
FAILURE_TTL = 60.0


class EndpointDiscoveryError(Exception):
    """Discovery is required for the operation but produced no usable endpoint."""


@dataclass(frozen=True)
class DiscoveredAddress:
    """A discovered endpoint URL and the clock reading at which it expires."""
    url: str
    expires: float


def cache_key(operation: str | None, identifiers: dict[str, str]) -> str:
    """What is sent to the discovery operation is exactly what keys the cache."""
    return json.dumps([operation, sorted(identifiers.items())], separators=(",", ":"))


class _Flight:
    def __init__(self) -> None:
        self.done = threading.Event()
        self.result: DiscoveredAddress | None = None
        self.error: BaseException | None = None


class EndpointCache:
    """Coalesce concurrent endpoint discovery and cache addresses until their expiry."""
    def __init__(self, clock: Any) -> None:
        self._clock = clock
        self._entries: dict[str, DiscoveredAddress] = {}
        self._lock = threading.Lock()
        self._flights: dict[str, _Flight] = {}
        self._tasks: dict[str, asyncio.Task[DiscoveredAddress]] = {}
        self._failures: dict[str, float] = {}

    def _valid(self, key: str) -> DiscoveredAddress | None:
        entry = self._entries.get(key)
        if entry is not None and self._clock.now() < entry.expires:
            return entry
        return None

    def _store(self, key: str, entry: DiscoveredAddress) -> DiscoveredAddress:
        with self._lock:
            if self._clock.now() < entry.expires:
                self._entries[key] = entry
            else:
                self._entries.pop(key, None)
        return entry

    def failed_recently(self, key: str) -> bool:
        """Whether an optional operation should keep using the configured endpoint."""
        with self._lock:
            expires = self._failures.get(key)
            if expires is not None and self._clock.now() >= expires:
                del self._failures[key]
                expires = None
            return expires is not None

    def record_failure(self, key: str) -> None:
        """Record a discovery failure to suppress optional discovery during the cooldown."""
        with self._lock:
            self._failures[key] = self._clock.now() + FAILURE_TTL

    def invalidate(self, key: str, entry: DiscoveredAddress) -> None:
        """Evict an entry a response rejected, unless a newer one already replaced it."""
        with self._lock:
            if self._entries.get(key) is entry:
                del self._entries[key]

    def resolve(self, key: str, fetch: Callable[[], DiscoveredAddress], wait: float) -> DiscoveredAddress:
        """A cached entry, or one discovery shared by every thread asking for the same key."""
        with self._lock:
            entry = self._valid(key)
            if entry is not None:
                return entry
            flight = self._flights.get(key)
            leader = flight is None
            if flight is None:
                flight = self._flights[key] = _Flight()
        if leader:
            try:
                flight.result = self._store(key, fetch())
                return flight.result
            except BaseException as error:
                flight.error = error
                raise
            finally:
                with self._lock:
                    self._flights.pop(key, None)
                flight.done.set()
        if not flight.done.wait(max(wait, 0)):
            raise TimeoutError("request deadline exceeded waiting for endpoint discovery")
        if flight.error is not None:
            raise flight.error
        assert flight.result is not None
        return flight.result

    async def aresolve(self, key: str, fetch: Callable[[], Awaitable[DiscoveredAddress]]) -> DiscoveredAddress:
        """The async form: one discovery task per key, which a cancelled caller does not cancel."""
        with self._lock:
            entry = self._valid(key)
            if entry is not None:
                return entry
            task = self._tasks.get(key)
            if task is None or task.done():
                async def run() -> DiscoveredAddress:
                    try:
                        return self._store(key, await fetch())
                    finally:
                        with self._lock:
                            if self._tasks.get(key) is task:
                                del self._tasks[key]
                task = asyncio.ensure_future(run())
                # Every caller may have been cancelled; the outcome is still consumed.
                task.add_done_callback(lambda done: done.cancelled() or done.exception())
                self._tasks[key] = task
        return await asyncio.shield(task)
