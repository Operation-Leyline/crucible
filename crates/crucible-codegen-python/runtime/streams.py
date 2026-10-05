"""Streaming blobs: request body sources and response bodies, never buffered whole."""
from __future__ import annotations

import asyncio
import inspect
import os
import re
from typing import Any, AsyncIterable, AsyncIterator, BinaryIO, Callable, Iterable, Iterator, Self

CHUNK = 256 * 1024
_SHA256 = re.compile(r"[0-9a-fA-F]{64}")


class StreamLengthError(ValueError):
    """A stream produced a length other than the one declared or modeled."""


def _close(source: Any) -> None:
    close = getattr(source, "close", None)
    if callable(close):
        close()


async def _aclose(source: Any) -> None:
    close = getattr(source, "aclose", None)
    if callable(close):
        await close()
    else:
        _close(source)


def _chunk(value: Any) -> bytes:
    if isinstance(value, bytes):
        return value
    if isinstance(value, (bytearray, memoryview)):
        return bytes(value)
    raise TypeError("a byte stream must produce bytes, not " + type(value).__name__)


def _sync_chunks(source: Any) -> Iterator[bytes]:
    """Read any synchronous source a chunk at a time."""
    if isinstance(source, (bytes, bytearray, memoryview)):
        if source:
            yield bytes(source)
        return
    read = getattr(source, "read", None)
    if callable(read):
        if inspect.iscoroutinefunction(read):
            raise TypeError("an asynchronous reader needs the async client")
        while chunk := _chunk(read(CHUNK)):
            yield chunk
        return
    if isinstance(source, AsyncIterable) and not isinstance(source, Iterable):
        raise TypeError("an asynchronous iterable needs the async client")
    for item in source:
        yield _chunk(item)


async def _async_chunks(source: Any) -> AsyncIterator[bytes]:
    """Read any source a chunk at a time without blocking the event loop."""
    if isinstance(source, (bytes, bytearray, memoryview)):
        if source:
            yield bytes(source)
        return
    read = getattr(source, "read", None)
    if callable(read):
        while True:
            # A blocking reader, such as a file, runs off the event loop.
            data = await read(CHUNK) if inspect.iscoroutinefunction(read) else await asyncio.to_thread(read, CHUNK)
            chunk = _chunk(data)
            if not chunk:
                return
            yield chunk
    if isinstance(source, AsyncIterable):
        async for item in source:
            yield _chunk(item)
        return
    if isinstance(source, (list, tuple)):
        for item in source:
            yield _chunk(item)
        return
    iterator = iter(source)
    done = object()
    while (item := await asyncio.to_thread(next, iterator, done)) is not done:
        yield _chunk(item)


class ByteStream:
    """A `@streaming` blob: a request body source, or a response body read once.

    Sources made by `from_bytes`, `from_file` and `from_factory` are replayable
    and may be retried; `from_iterable` and `from_reader` are one-shot, opened at
    most once, so their call is never retried.
    """

    def __init__(self, opener: Callable[[], Any], *, length: int | None = None, replayable: bool,
                 data: bytes | None = None, closer: Callable[[], Any] | None = None) -> None:
        if length is not None and (isinstance(length, bool) or not isinstance(length, int) or length < 0):
            raise ValueError("a stream length must be a nonnegative integer")
        self._opener = opener
        self._length = length
        self._replayable = replayable
        self._data = data
        self._closer = closer
        self._sha256: str | None = None
        self._opened = False
        self._closed = False
        self._pending = bytearray()
        self._sync: Iterator[bytes] | None = None
        self._async: AsyncIterator[bytes] | None = None

    @classmethod
    def from_bytes(cls, data: bytes | bytearray | memoryview) -> ByteStream:
        """In-memory bytes: replayable, sized, and hashed like any buffered body."""
        if not isinstance(data, (bytes, bytearray, memoryview)):
            raise TypeError("from_bytes requires bytes")
        value = bytes(data)
        return cls(lambda: value, length=len(value), replayable=True, data=value)

    @classmethod
    def from_file(cls, path: str | os.PathLike[str]) -> ByteStream:
        """A file reopened for each attempt; its current size is sent as Content-Length."""
        location = os.fspath(path)
        return cls(lambda: open(location, "rb"), length=os.path.getsize(location), replayable=True)

    @classmethod
    def from_factory(cls, factory: Callable[[], Iterable[bytes] | AsyncIterable[bytes] | BinaryIO | bytes],
                     length: int | None = None) -> ByteStream:
        """Called once per attempt; each call must produce the same bytes."""
        if not callable(factory):
            raise TypeError("from_factory requires a callable")
        return cls(factory, length=length, replayable=True)

    @classmethod
    def from_iterable(cls, chunks: Iterable[bytes] | AsyncIterable[bytes], length: int | None = None) -> ByteStream:
        """A one-shot iterable or async iterable of byte chunks."""
        if isinstance(chunks, (bytes, bytearray, memoryview, str)):
            raise TypeError("from_iterable requires an iterable of byte chunks; use from_bytes for bytes")
        return cls(lambda: chunks, length=length, replayable=False)

    @classmethod
    def from_reader(cls, reader: Any, length: int | None = None) -> ByteStream:
        """A one-shot reader with `read(size)`, such as an open binary file or socket."""
        if not callable(getattr(reader, "read", None)):
            raise TypeError("from_reader requires an object with read(size)")
        return cls(lambda: reader, length=length, replayable=False)

    def with_sha256(self, hex_digest: str) -> Self:
        """Declare the body's SHA-256 so a content digest or SigV4 never reads the stream."""
        if not isinstance(hex_digest, str) or not _SHA256.fullmatch(hex_digest):
            raise ValueError("a SHA-256 digest must be 64 hexadecimal characters")
        self._sha256 = hex_digest.lower()
        return self

    @property
    def length(self) -> int | None:
        return self._length

    @property
    def replayable(self) -> bool:
        return self._replayable

    @property
    def sha256(self) -> str | None:
        return self._sha256

    def __repr__(self) -> str:
        kind = "replayable" if self._replayable else "one-shot"
        return f"ByteStream({kind}, length={self._length})"

    def __copy__(self) -> ByteStream:
        return self

    def __deepcopy__(self, memo: Any) -> ByteStream:
        return self

    def _open(self) -> Any:
        """Open the source once per attempt; a one-shot source only ever once."""
        if self._closed:
            raise RuntimeError("the stream is closed")
        if not self._replayable:
            if self._opened:
                raise RuntimeError("a one-shot stream was already opened")
        self._opened = True
        return self._opener()

    # Reading: a response body, or a source read directly by the caller.

    def _iterator(self) -> Iterator[bytes]:
        if self._async is not None:
            raise RuntimeError("the stream is being read asynchronously")
        if self._sync is None:
            self._sync = self._source_chunks(self._open())
        return self._sync

    @staticmethod
    def _source_chunks(source: Any) -> Iterator[bytes]:
        try:
            yield from _sync_chunks(source)
        finally:
            _close(source)

    def __iter__(self) -> Iterator[bytes]:
        iterator = self._iterator()
        if self._pending:
            data = bytes(self._pending)
            self._pending.clear()
            yield data
        yield from iterator

    def iter_chunks(self) -> Iterator[bytes]:
        return iter(self)

    def read(self, size: int = -1) -> bytes:
        """Read up to `size` bytes, or everything that remains when `size` is negative."""
        iterator = self._iterator()
        if size < 0:
            for chunk in iterator:
                self._pending += chunk
        else:
            while len(self._pending) < size:
                item = next(iterator, None)
                if item is None:
                    break
                self._pending += item
        end = len(self._pending) if size < 0 else size
        data = bytes(self._pending[:end])
        del self._pending[:end]
        return data

    def write_to(self, target: Any) -> int:
        """Copy the remaining bytes to a binary writer; returns the count."""
        total = 0
        for chunk in self:
            target.write(chunk)
            total += len(chunk)
        return total

    def close(self) -> None:
        """Release the body; an unread or partly read response closes its connection."""
        self._closed = True
        iterator, self._sync = self._sync, None
        if isinstance(iterator, Iterator) and hasattr(iterator, "close"):
            try:
                iterator.close()
            except ValueError:
                pass  # Closed from another thread while it reads; the closer below ends that read.
        if self._closer is not None:
            result = self._closer()
            if inspect.iscoroutine(result):
                result.close()
                raise RuntimeError("an asynchronous response body must be closed with aclose()")

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *args: Any) -> None:
        self.close()

    def _aiterator(self) -> AsyncIterator[bytes]:
        if self._sync is not None:
            raise RuntimeError("the stream is being read synchronously")
        if self._async is None:
            self._async = self._async_source_chunks(self._open())
        return self._async

    @staticmethod
    async def _async_source_chunks(source: Any) -> AsyncIterator[bytes]:
        try:
            async for chunk in _async_chunks(source):
                yield chunk
        finally:
            await _aclose(source)

    async def __aiter__(self) -> AsyncIterator[bytes]:
        iterator = self._aiterator()
        if self._pending:
            data = bytes(self._pending)
            self._pending.clear()
            yield data
        async for chunk in iterator:
            yield chunk

    async def aread(self, size: int = -1) -> bytes:
        iterator = self._aiterator()
        if size < 0:
            async for chunk in iterator:
                self._pending += chunk
        else:
            while len(self._pending) < size:
                try:
                    chunk = await anext(iterator)
                except StopAsyncIteration:
                    break
                self._pending += chunk
        end = len(self._pending) if size < 0 else size
        data = bytes(self._pending[:end])
        del self._pending[:end]
        return data

    async def aclose(self) -> None:
        self._closed = True
        iterator, self._async = self._async, None
        if iterator is not None and hasattr(iterator, "aclose"):
            try:
                await iterator.aclose()
            except RuntimeError:
                pass  # Already running in another task, which the closer below ends.
        if self._closer is not None:
            result = self._closer()
            if inspect.isawaitable(result):
                await result

    async def __aenter__(self) -> Self:
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self.aclose()


class Upload:
    """One attempt's request body: opened lazily by the transport, checked as it flows."""

    def __init__(self, stream: ByteStream, minimum: int | None, maximum: int | None) -> None:
        self._stream = stream
        self._minimum = minimum
        self._maximum = maximum
        self._source: Any = None
        self.opened = False

    def _check(self, count: int, final: bool) -> None:
        declared = self._stream.length
        if declared is not None and (count > declared or final and count != declared):
            raise StreamLengthError(f"the stream produced {'more' if count > declared else 'fewer'} bytes than its declared length {declared}")
        if self._maximum is not None and count > self._maximum:
            raise StreamLengthError(f"the stream exceeds the modeled maximum length {self._maximum}")
        if final and self._minimum is not None and count < self._minimum:
            raise StreamLengthError(f"the stream is shorter than the modeled minimum length {self._minimum}")

    def sync(self) -> Iterable[bytes]:
        upload = self

        class Body:
            def __iter__(self) -> Iterator[bytes]:
                upload.opened = True
                upload._source = upload._stream._open()
                count = 0
                for chunk in _sync_chunks(upload._source):
                    count += len(chunk)
                    upload._check(count, False)
                    yield chunk
                upload._check(count, True)
                upload.close()
        return Body()

    def asynchronous(self) -> AsyncIterable[bytes]:
        upload = self

        class Body:
            async def __aiter__(self) -> AsyncIterator[bytes]:
                upload.opened = True
                upload._source = upload._stream._open()
                count = 0
                async for chunk in _async_chunks(upload._source):
                    count += len(chunk)
                    upload._check(count, False)
                    yield chunk
                upload._check(count, True)
                await upload.aclose()
        return Body()

    def close(self) -> None:
        source, self._source = self._source, None
        if source is not None:
            _close(source)

    async def aclose(self) -> None:
        source, self._source = self._source, None
        if source is not None:
            await _aclose(source)
