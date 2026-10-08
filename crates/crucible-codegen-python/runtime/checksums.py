"""HTTP checksums over buffered bytes, or incrementally over a streamed body."""
from __future__ import annotations

import base64
import binascii
import hashlib
import hmac
from typing import Any


class ChecksumError(ValueError):
    """A response checksum differs from the received payload."""


SUPPORTED = ("CRC32", "CRC32C", "CRC64NVME", "MD5", "SHA1", "SHA256", "SHA512")


def _table(polynomial: int) -> tuple[int, ...]:
    result = []
    for byte in range(256):
        value = byte
        for _ in range(8):
            value = (value >> 1) ^ (polynomial if value & 1 else 0)
        result.append(value)
    return tuple(result)


_CRC32C = _table(0x82F63B78)
_CRC64NVME = _table(0x9A6C9329AC4BC9B5)


class Hasher:
    """One checksum computed over chunks as they are sent or received."""
    def __init__(self, algorithm: str) -> None:
        if algorithm not in SUPPORTED:
            raise ValueError("unsupported checksum algorithm: " + algorithm)
        self.algorithm = algorithm
        self._crc = 0
        if algorithm in ("CRC32C", "CRC64NVME"):
            self._size = 4 if algorithm == "CRC32C" else 8
            self._table = _CRC32C if self._size == 4 else _CRC64NVME
            self._mask = (1 << (self._size * 8)) - 1
            self._crc = self._mask
        self._hash = None if algorithm.startswith("CRC") else hashlib.new(algorithm.lower(), usedforsecurity=False)

    def update(self, data: bytes) -> None:
        if self._hash is not None:
            self._hash.update(data)
        elif self.algorithm == "CRC32":
            self._crc = binascii.crc32(data, self._crc)
        else:
            value, table = self._crc, self._table
            for byte in data:
                value = table[(value ^ byte) & 255] ^ (value >> 8)
            self._crc = value

    def base64(self) -> str:
        if self._hash is not None:
            raw = self._hash.digest()
        elif self.algorithm == "CRC32":
            raw = self._crc.to_bytes(4, "big")
        else:
            raw = (self._crc ^ self._mask).to_bytes(self._size, "big")
        return base64.b64encode(raw).decode("ascii")


def digest(algorithm: str, data: bytes) -> str:
    """Compute the selected checksum from buffered bytes."""
    hasher = Hasher(algorithm)
    hasher.update(data)
    return hasher.base64()


def prepare(config: dict[str, Any] | None, values: dict[str, Any], names: dict[str, str],
            request_policy: str, response_policy: str, streaming: bool = False) -> tuple[dict[str, Any], dict[str, Any]]:
    """Settle the algorithms for one call. A streamed body is never read twice to
    compute a checksum, so it gets no implicit algorithm; a required or chosen one
    must arrive as a caller-supplied header (see `require_header`)."""
    if config is None:
        return {}, {}
    if config["kind"] == "md5":
        return {"algorithm": "MD5", "header": "content-md5"}, {}
    injected: dict[str, Any] = {}
    member = config.get("requestAlgorithmMember")
    algorithm = None
    header = None
    if member:
        algorithm = values.get(names[member])
        if algorithm is None and not streaming and (config.get("requestRequired") or request_policy == "WHEN_SUPPORTED"):
            algorithm = "CRC32"
            values[names[member]] = algorithm
            injected[member] = algorithm
        if algorithm is not None and algorithm not in SUPPORTED:
            raise ValueError("unsupported checksum algorithm: " + str(algorithm))
        header = "x-amz-checksum-" + algorithm.lower() if algorithm is not None else None
        if algorithm is None and streaming and config.get("requestRequired"):
            header = "x-amz-checksum-*"
    elif config.get("requestRequired"):
        algorithm, header = "MD5", "content-md5"
    mode = config.get("validationModeMember")
    validate = False
    if mode:
        choice = values.get(names[mode])
        if choice is None and response_policy == "WHEN_SUPPORTED":
            choice = "ENABLED"
            values[names[mode]] = choice
            injected[mode] = choice
        validate = choice == "ENABLED"
    return {"algorithm": algorithm, "header": header,
            "response": [name for name in config.get("responseAlgorithms", []) if name in SUPPORTED] if validate else []}, injected


def request(settings: dict[str, Any], headers: Any, data: bytes) -> None:
    """Choose and calculate a modeled request checksum according to the configured policy."""
    name = settings.get("header")
    if name and name not in headers:
        headers[name] = digest(settings["algorithm"], data)


def require_header(settings: dict[str, Any], headers: Any) -> None:
    """A streamed request body carries only a checksum the caller supplied."""
    name = settings.get("header")
    if not name:
        return
    present = any(key.lower().startswith("x-amz-checksum-") for key in headers) if name == "x-amz-checksum-*" else name in headers
    if not present:
        raise ValueError("a streaming request needs its precomputed " + (settings.get("algorithm") or "checksum")
                         + " header; the stream is not buffered to compute one")


def expected(settings: dict[str, Any], headers: Any) -> tuple[str, str] | None:
    """The first modeled response algorithm whose header is present."""
    for algorithm in settings.get("response", []):
        value = headers.get("x-amz-checksum-" + algorithm.lower())
        if value is not None:
            return str(algorithm), value
    return None


def verify(algorithm: str, expected_value: str, actual: str) -> None:
    """Verify an encoded response checksum; malformed or mismatching checksums raise ChecksumError."""
    if not hmac.compare_digest(expected_value.encode("utf-8"), actual.encode("ascii")):
        raise ChecksumError("response " + algorithm + " checksum mismatch")


def response(settings: dict[str, Any], headers: Any, data: bytes) -> str | None:
    """Select and verify a modeled response checksum according to the configured policy."""
    found = expected(settings, headers)
    if found is None:
        return None
    verify(found[0], found[1], digest(found[0], data))
    return found[0]
