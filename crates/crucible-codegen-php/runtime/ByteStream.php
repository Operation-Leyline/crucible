<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/**
 * A @streaming blob: the whole HTTP body, sent or received without buffering it.
 * A replayable source is reopened for each attempt; a one-shot source opens once,
 * so its call is never retried and a second use fails.
 */
final class ByteStream
{
    private ?StreamBody $reader = null;
    /** @param \Closure(): StreamBody $opener */
    private function __construct(private readonly \Closure $opener, public readonly ?int $length, public readonly bool $replayable,
        public readonly ?string $sha256 = null, private readonly mixed $handle = null, private readonly ?int $min = null, private readonly ?int $max = null)
    {
        if ($length !== null && $length < 0) throw new \InvalidArgumentException('Stream length must be nonnegative');
    }

    public static function fromString(string $data): self
    {
        return new self(static fn() => StreamBody::fromChunks([$data]), strlen($data), true);
    }
    /** Reopened for every attempt; the length is the file size when the call starts. */
    public static function fromFile(string $path): self
    {
        clearstatcache(true, $path);
        $size = is_file($path) && is_readable($path) ? filesize($path) : false;
        if ($size === false) throw new \InvalidArgumentException('Unreadable stream file: ' . $path);
        return new self(static function () use ($path): StreamBody {
            $handle = @fopen($path, 'rb');
            if ($handle === false) throw new StreamException('Cannot open stream file: ' . $path);
            return StreamBody::fromResource($handle, true);
        }, $size, true);
    }
    /**
     * A caller-owned resource, never closed by the client. A seekable resource replays from
     * its current position; any other is one-shot. A plain file's remaining size is its length.
     * @param resource $resource
     */
    public static function fromResource(mixed $resource, ?int $length = null): self
    {
        if (!is_resource($resource)) throw new \InvalidArgumentException('Expected a readable stream resource');
        $meta = stream_get_meta_data($resource);
        $start = $meta['seekable'] ? ftell($resource) : false;
        if ($start === false) return new self(self::once(static fn() => StreamBody::fromResource($resource, false)), $length, false, null, $resource);
        if ($length === null && in_array($meta['wrapper_type'] ?? '', ['plainfile', 'PHP'], true)) {
            $stat = fstat($resource);
            if ($stat !== false && $stat['size'] >= $start) $length = $stat['size'] - $start;
        }
        return new self(static function () use ($resource, $start): StreamBody {
            if (fseek($resource, $start) !== 0) throw new StreamException('Cannot rewind the stream');
            return StreamBody::fromResource($resource, false);
        }, $length, true, null, $resource);
    }
    /** An array replays; a generator or other iterator is one-shot. @param iterable<string> $chunks */
    public static function fromIterable(iterable $chunks, ?int $length = null): self
    {
        if (is_array($chunks)) return new self(static fn() => StreamBody::fromChunks($chunks), $length ?? array_sum(array_map('strlen', $chunks)), true);
        return new self(self::once(static fn() => StreamBody::fromChunks($chunks)), $length, false);
    }
    /**
     * A PSR-7 StreamInterface (or any object with read() and eof()). Seekable streams
     * replay from their current position; getSize() supplies the length when known.
     */
    public static function fromObject(object $stream, ?int $length = null): self
    {
        $start = method_exists($stream, 'isSeekable') && method_exists($stream, 'seek') && method_exists($stream, 'tell') && $stream->isSeekable() ? $stream->tell() : null;
        if ($length === null && method_exists($stream, 'getSize') && is_int($size = $stream->getSize())) $length = $start === null ? $size : $size - $start;
        if ($start === null) return new self(self::once(static fn() => StreamBody::fromObject($stream)), $length, false);
        return new self(static function () use ($stream, $start): StreamBody {
            $stream->seek($start);
            return StreamBody::fromObject($stream);
        }, $length, true);
    }
    /**
     * Calls the factory once per attempt; it must produce the same bytes every time, as a
     * string, resource, iterable, or stream object. What it returns is closed after the attempt.
     * @param \Closure(): mixed $factory
     */
    public static function fromFactory(\Closure $factory, ?int $length = null): self
    {
        return new self(static fn() => StreamBody::from($factory()), $length, true);
    }

    /** Declares the hex SHA-256 of the bytes, for SigV4 and content digests, so the body is never read to hash it. */
    public function withSha256(string $hex): self
    {
        if (!preg_match('/^[0-9a-fA-F]{64}$/D', $hex)) throw new \InvalidArgumentException('SHA-256 must be 64 hex digits');
        return new self($this->opener, $this->length, $this->replayable, strtolower($hex), $this->handle, $this->min, $this->max);
    }
    /** Applies modeled @length bounds to every pass; used by the runtime. */
    public function withBounds(?int $min, ?int $max): self
    {
        return new self($this->opener, $this->length, $this->replayable, $this->sha256, $this->handle, $min, $max);
    }
    /** Opens one pass; a one-shot stream can be opened once. */
    public function open(): StreamBody
    {
        return ($this->opener)()->limit($this->length, $this->min, $this->max);
    }
    private static function once(\Closure $open): \Closure
    {
        $used = false;
        return static function () use (&$used, $open): StreamBody {
            if ($used) throw new \LogicException('A one-shot stream was already consumed');
            $used = true;
            return $open();
        };
    }

    /** Reads a received body (or an unsent source) sequentially; the empty string marks the end. */
    public function read(int $size = 65536): string
    {
        $this->reader ??= $this->open();
        return $this->reader->read($size);
    }
    public function getContents(): string
    {
        $data = '';
        while (($chunk = $this->read()) !== '') $data .= $chunk;
        return $data;
    }
    /** Copies the remaining bytes to a writable resource without holding them. @param resource $target */
    public function copyTo(mixed $target): int
    {
        $total = 0;
        while (($chunk = $this->read()) !== '') { ResponseStream::write($target, $chunk); $total += strlen($chunk); }
        return $total;
    }
    /** The underlying resource: a response target, the response buffer, or a wrapped source. @return resource|null */
    public function resource(): mixed { return $this->handle; }
    public function close(): void { $this->reader?->close(); }
}
