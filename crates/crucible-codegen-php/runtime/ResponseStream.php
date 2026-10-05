<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/**
 * Delivers a streamed success body to its target as it arrives, verifying the
 * response checksum incrementally. The transport calls the sink synchronously,
 * so a slow target slows the transfer instead of growing a buffer.
 */
final class ResponseStream
{
    private int $delivered = 0;
    private bool $started = false;
    private ?ChecksumHasher $hasher = null;
    private ?string $expected = null;
    private mixed $buffer = null;
    /**
     * @param null|\Closure(string): void|resource $target null buffers to a temporary stream that spills to disk
     * @param list<string> $algorithms modeled response checksum algorithms, in order
     */
    public function __construct(private readonly mixed $target, private readonly array $algorithms)
    {
        if ($target === null) {
            $this->buffer = fopen('php://temp/maxmemory:1048576', 'w+b');
        } elseif (!$target instanceof \Closure && !(is_resource($target) && get_resource_type($target) === 'stream')) {
            throw new \InvalidArgumentException('A response target must be a Closure or a writable stream resource');
        }
    }
    /** @param resource $handle */
    public static function write(mixed $handle, string $chunk): void
    {
        for ($offset = 0, $length = strlen($chunk); $offset < $length; $offset += $written) {
            $written = fwrite($handle, $offset === 0 ? $chunk : substr($chunk, $offset));
            if ($written === false || $written === 0) throw new StreamException('Response target write failed');
        }
    }
    /** @return \Closure(string, array<string, string>): void for HttpTransport::send */
    public function sink(CallScope $scope): \Closure
    {
        return function (string $chunk, array $headers) use ($scope): void {
            $this->deliver($chunk, $headers);
            // Cancellation and the deadline are observed between chunks.
            $scope->check();
        };
    }
    /** @param array<string, string> $headers */
    private function deliver(string $chunk, array $headers): void
    {
        if (!$this->started) {
            $this->started = true;
            foreach ($this->algorithms as $algorithm) {
                $expected = $headers['x-amz-checksum-' . strtolower($algorithm)] ?? null;
                if ($expected !== null) { $this->hasher = new ChecksumHasher($algorithm); $this->expected = $expected; break; }
            }
        }
        if ($chunk === '') return;
        $this->hasher?->update($chunk);
        $this->delivered += strlen($chunk);
        if ($this->buffer !== null) self::write($this->buffer, $chunk);
        elseif ($this->target instanceof \Closure) ($this->target)($chunk);
        else self::write($this->target, $chunk);
    }
    /** Only bytes held in the private buffer can be discarded for a retry; caller-visible bytes cannot. */
    public function retryable(): bool
    {
        if ($this->delivered === 0 || $this->buffer !== null) {
            if ($this->buffer !== null) { ftruncate($this->buffer, 0); rewind($this->buffer); }
            $this->delivered = 0; $this->started = false; $this->hasher = null; $this->expected = null;
            return true;
        }
        return false;
    }
    /** Completes the body after a success; a transport without a sink returned it buffered. */
    public function finish(HttpResponse $response): array
    {
        $this->deliver($response->body, $response->headers);
        $algorithm = null;
        if ($this->hasher !== null) {
            if (!hash_equals($this->hasher->base64(), $this->expected)) throw new ChecksumException($this->hasher->algorithm);
            $algorithm = $this->hasher->algorithm;
        }
        if ($this->buffer !== null) { rewind($this->buffer); $body = ByteStream::fromResource($this->buffer); }
        elseif ($this->target instanceof \Closure) $body = ByteStream::fromString('');
        else $body = ByteStream::fromResource($this->target);
        return [$body, $algorithm];
    }
}
