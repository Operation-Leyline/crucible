"""Modeled waiter acceptors and timing shared by sync and async clients."""
import math
import random
from typing import Any

from .query import Query


class WaiterError(Exception):
    def __init__(self, name: str, attempts: int, *, response: Any = None, error: Exception | None = None, message: str | list[str] | None = None) -> None:
        super().__init__(f"waiter {name} failed after {attempts} attempts" + (f": {message}" if message else ""))
        self.name, self.attempts, self.response, self.error = name, attempts, response, error
        self.message = message


class WaiterTimeout(WaiterError, TimeoutError):
    pass


class Waiter:
    def __init__(self, name: str, description: dict[str, Any], timeout: float,
                 min_delay: float | None, max_delay: float | None) -> None:
        self.name, self.timeout = name, timeout
        self.minimum = description["minDelay"] if min_delay is None else min_delay
        self.maximum = description["maxDelay"] if max_delay is None else max_delay
        if not math.isfinite(timeout) or timeout <= 0:
            raise ValueError("waiter timeout must be finite and positive")
        if not math.isfinite(self.minimum) or not math.isfinite(self.maximum) or self.minimum < 0 or self.maximum < self.minimum:
            raise ValueError("waiter delays must be finite, nonnegative, and ordered")
        self.acceptors = [(item, Query(item["matcher"]["path"]["expression"]) if "path" in item["matcher"] else None) for item in description["acceptors"]]
        self.message: str | list[str] | None = None

    def state(self, input: Any, output: Any, error: Exception | None) -> str:
        self.message = None
        for acceptor, query in self.acceptors:
            matcher = acceptor["matcher"]
            kind = matcher["kind"]
            match = False
            if kind == "success":
                match = (error is None) == matcher["expected"]
            elif kind == "errorType":
                name = getattr(error, "name", None)
                match = error is not None and name in (matcher["name"], matcher["name"].split("#")[-1])
            elif error is None and query is not None:
                result = query.search({"input": input, "output": output} if kind == "inputOutput" else output)
                comparator, expected = matcher["comparator"], matcher["expected"]
                if comparator == "stringEquals":
                    match = isinstance(result, str) and result == expected
                elif comparator == "booleanEquals":
                    match = isinstance(result, bool) and result == (expected == "true")
                elif comparator == "allStringEquals":
                    match = isinstance(result, list) and bool(result) and all(isinstance(item, str) and item == expected for item in result)
                elif comparator == "anyStringEquals":
                    match = isinstance(result, list) and any(isinstance(item, str) and item == expected for item in result)
            if match:
                if acceptor.get("message") is not None:
                    message = Query(acceptor["message"]["expression"]).search(output)
                    if isinstance(message, str) or isinstance(message, list) and all(isinstance(item, str) for item in message):
                        self.message = message
                return str(acceptor["state"])
        return "failure" if error is not None else "retry"

    def delay(self, attempt: int, remaining: float) -> float:
        ceiling = min(self.maximum, self.minimum * 2 ** min(attempt - 1, 62))
        delay = random.uniform(self.minimum, ceiling)
        if remaining <= self.minimum:
            return remaining
        return remaining - self.minimum if remaining - delay <= self.minimum else delay
