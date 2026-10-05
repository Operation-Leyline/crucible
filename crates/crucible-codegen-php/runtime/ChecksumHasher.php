<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** An incremental checksum, so streamed bodies are verified without buffering them. */
final class ChecksumHasher
{
    /** @var list<int>|null */
    private static ?array $crc64 = null;
    private ?\HashContext $context = null;
    private int $state = -1;

    public function __construct(public readonly string $algorithm)
    {
        $name = match ($algorithm) {
            // PHP's crc32b and crc32c hashes are big-endian; hash() needs no extension.
            'CRC32' => 'crc32b', 'CRC32C' => 'crc32c', 'CRC64NVME' => null,
            'MD5', 'SHA1', 'SHA256', 'SHA512' => strtolower($algorithm),
            default => throw new \UnexpectedValueException('Unsupported checksum algorithm: ' . $algorithm),
        };
        if ($name !== null) $this->context = hash_init($name);
        elseif (self::$crc64 === null) {
            $polynomial = unpack('J', "\x9a\x6c\x93\x29\xac\x4b\xc9\xb5")[1];
            $table = [];
            for ($byte = 0; $byte < 256; $byte++) {
                $value = $byte;
                for ($bit = 0; $bit < 8; $bit++) $value = (($value >> 1) & PHP_INT_MAX) ^ (($value & 1) !== 0 ? $polynomial : 0);
                $table[] = $value;
            }
            self::$crc64 = $table;
        }
    }

    public function update(string $data): void
    {
        if ($this->context !== null) { hash_update($this->context, $data); return; }
        // Reflected CRC-64/NVME on signed 64-bit integers; the shift masks the sign bit.
        $table = self::$crc64; $value = $this->state; $length = strlen($data);
        for ($i = 0; $i < $length; $i++) $value = $table[($value ^ ord($data[$i])) & 255] ^ (($value >> 8) & 0x00FFFFFFFFFFFFFF);
        $this->state = $value;
    }

    /** Base64 of the big-endian digest; the hasher cannot be updated afterwards. */
    public function base64(): string
    {
        return base64_encode($this->context !== null ? hash_final($this->context, true) : pack('J', ~$this->state));
    }
}
