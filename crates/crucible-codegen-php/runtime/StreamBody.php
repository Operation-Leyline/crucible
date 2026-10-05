<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/**
 * One opened pass over a ByteStream. It counts the bytes it hands out and fails,
 * rather than truncating, when the source breaks its declared or modeled length.
 */
final class StreamBody
{
    private string $buffer = '';
    private int $offset = 0;
    private int $count = 0;
    private bool $finished = false;
    /**
     * @param \Closure(int): ?string $next the next chunk of at most the given size, or null at the end
     * @param ?\Closure(): void $closer releases what this pass opened
     */
    private function __construct(private readonly \Closure $next, private ?\Closure $closer = null,
        private ?int $length = null, private ?int $min = null, private ?int $max = null) {}

    /** @param resource $handle closed at the end only when $owned */
    public static function fromResource(mixed $handle, bool $owned): self
    {
        if (!is_resource($handle)) throw new \InvalidArgumentException('Expected a readable stream resource');
        $next = static function (int $size) use ($handle): ?string {
            if (feof($handle)) return null;
            $chunk = fread($handle, $size);
            if ($chunk === false) throw new StreamException('Stream read failed');
            return $chunk;
        };
        return new self($next, $owned ? static function () use ($handle): void { if (is_resource($handle)) fclose($handle); } : null);
    }
    /** @param iterable<string> $chunks */
    public static function fromChunks(iterable $chunks): self
    {
        $iterator = (static fn() => yield from $chunks)();
        $started = false;
        $next = static function () use ($iterator, &$started): ?string {
            if ($started) $iterator->next();
            $started = true;
            if (!$iterator->valid()) return null;
            $chunk = $iterator->current();
            if (!is_string($chunk)) throw new StreamException('Stream chunks must be strings');
            return $chunk;
        };
        return new self($next);
    }
    /** A PSR-7 StreamInterface, or any object with read() and eof(); no PSR package is required. */
    public static function fromObject(object $stream): self
    {
        if (!method_exists($stream, 'read') || !method_exists($stream, 'eof')) throw new \InvalidArgumentException('A stream object needs read() and eof()');
        $next = static function (int $size) use ($stream): ?string {
            if ($stream->eof()) return null;
            $chunk = $stream->read($size);
            if (!is_string($chunk)) throw new StreamException('Stream read failed');
            return $chunk;
        };
        return new self($next, method_exists($stream, 'close') ? static fn() => $stream->close() : null);
    }
    /** Opens what a replay factory returned; resources and objects it returns are closed after the pass. */
    public static function from(mixed $source): self
    {
        if ($source instanceof self) return $source;
        if ($source instanceof ByteStream) return $source->open();
        if (is_string($source)) return self::fromChunks([$source]);
        if (is_resource($source)) return self::fromResource($source, true);
        if (is_iterable($source)) return self::fromChunks($source);
        if (is_object($source)) return self::fromObject($source);
        throw new \InvalidArgumentException('A stream factory must return a string, resource, iterable, or stream object');
    }

    /** Applies the exact declared length and modeled @length bounds to this pass. */
    public function limit(?int $length, ?int $min, ?int $max): self
    {
        $this->length = $length; $this->min = $min; $this->max = $max;
        return $this;
    }

    /** At most $size bytes; the empty string only at the end. */
    public function read(int $size = 65536): string
    {
        if ($this->finished || $size < 1) return '';
        try {
            while ($this->offset >= strlen($this->buffer)) {
                $chunk = ($this->next)($size);
                if ($chunk === null) { $this->finish(); return ''; }
                $this->buffer = $chunk; $this->offset = 0;
            }
            $out = substr($this->buffer, $this->offset, $size);
            $this->offset += strlen($out);
            $this->count += strlen($out);
            if ($this->length !== null && $this->count > $this->length) throw new StreamException('Stream produced more than its declared ' . $this->length . ' bytes');
            if ($this->max !== null && $this->count > $this->max) throw new StreamException('Stream exceeds the modeled maximum of ' . $this->max . ' bytes');
            return $out;
        } catch (\Throwable $error) {
            $this->close();
            throw $error instanceof StreamException || $error instanceof CancelledException || $error instanceof TimeoutException
                ? $error : new StreamException('Stream source failed: ' . $error->getMessage(), 0, $error);
        }
    }
    private function finish(): void
    {
        $this->finished = true;
        $this->close();
        if ($this->length !== null && $this->count !== $this->length) throw new StreamException('Stream produced ' . $this->count . ' of its declared ' . $this->length . ' bytes');
        if ($this->min !== null && $this->count < $this->min) throw new StreamException('Stream is shorter than the modeled minimum of ' . $this->min . ' bytes');
    }
    public function bytes(): int { return $this->count; }
    public function close(): void
    {
        $this->finished = true;
        $closer = $this->closer;
        $this->closer = null;
        $this->buffer = '';
        if ($closer !== null) $closer();
    }
}
