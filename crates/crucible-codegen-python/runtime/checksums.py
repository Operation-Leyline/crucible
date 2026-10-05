"""Buffered HTTP checksums; the caller supplies the final transmitted bytes."""
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


def digest(algorithm: str, data: bytes) -> str:
    if algorithm == "CRC32":
        raw = binascii.crc32(data).to_bytes(4, "big")
    elif algorithm in ("CRC32C", "CRC64NVME"):
        size = 4 if algorithm == "CRC32C" else 8
        table = _CRC32C if size == 4 else _CRC64NVME
        mask = (1 << (size * 8)) - 1
        value = mask
        for byte in data:
            value = table[(value ^ byte) & 255] ^ (value >> 8)
        raw = (value ^ mask).to_bytes(size, "big")
    elif algorithm in SUPPORTED:
        raw = hashlib.new(algorithm.lower(), data, usedforsecurity=False).digest()
    else:
        raise ValueError("unsupported checksum algorithm: " + algorithm)
    return base64.b64encode(raw).decode("ascii")


def prepare(config: dict[str, Any] | None, values: dict[str, Any], names: dict[str, str],
            request_policy: str, response_policy: str) -> tuple[dict[str, Any], dict[str, Any]]:
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
        if algorithm is None and (config.get("requestRequired") or request_policy == "WHEN_SUPPORTED"):
            algorithm = "CRC32"
            values[names[member]] = algorithm
            injected[member] = algorithm
        if algorithm is not None and algorithm not in SUPPORTED:
            raise ValueError("unsupported checksum algorithm: " + str(algorithm))
        header = "x-amz-checksum-" + algorithm.lower() if algorithm is not None else None
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
    name = settings.get("header")
    if name and name not in headers:
        headers[name] = digest(settings["algorithm"], data)


def response(settings: dict[str, Any], headers: Any, data: bytes) -> str | None:
    for algorithm in settings.get("response", []):
        expected = headers.get("x-amz-checksum-" + algorithm.lower())
        if expected is not None:
            actual = digest(algorithm, data)
            if not hmac.compare_digest(expected.encode("utf-8"), actual.encode("ascii")):
                raise ChecksumError("response " + algorithm + " checksum mismatch")
            return str(algorithm)
    return None
