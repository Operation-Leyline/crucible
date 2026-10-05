<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** Buffered HTTP checksums over the final transmitted or received bytes. */
final class Checksums
{
    public const SUPPORTED = ['CRC32', 'CRC32C', 'CRC64NVME', 'MD5', 'SHA1', 'SHA256', 'SHA512'];
    public const WHEN_SUPPORTED = 'WHEN_SUPPORTED';
    public const WHEN_REQUIRED = 'WHEN_REQUIRED';

    /** Base64 of the big-endian digest. */
    public static function digest(string $algorithm, string $data): string
    {
        $hasher = new ChecksumHasher($algorithm);
        $hasher->update($data);
        return $hasher->base64();
    }

    /** Hashes a replayable stream in chunks, without holding its bytes. */
    public static function stream(string $algorithm, ByteStream $stream): string
    {
        $hasher = new ChecksumHasher($algorithm);
        $body = $stream->open();
        try { while (($chunk = $body->read(65536)) !== '') $hasher->update($chunk); }
        finally { $body->close(); }
        return $hasher->base64();
    }

    /**
     * Resolves the request header and response algorithms; defaults are written into
     * the call's wire values only, never into the caller's input.
     * @param array<string|int, mixed> $values
     * @return array{algorithm: ?string, header: ?string, response: list<string>}
     */
    public static function prepare(?JsonObject $description, array &$values, string $requestPolicy, string $responsePolicy): array
    {
        $settings = ['algorithm' => null, 'header' => null, 'response' => []];
        if ($description === null) return $settings;
        if ($description->get('kind') === 'md5') return ['algorithm' => 'MD5', 'header' => 'content-md5', 'response' => []];
        $member = $description->get('requestAlgorithmMember');
        if (is_string($member)) {
            $algorithm = $values[$member] ?? null;
            if ($algorithm === null && ($description->get('requestRequired') === true || $requestPolicy === self::WHEN_SUPPORTED)) $algorithm = $values[$member] = 'CRC32';
            if ($algorithm !== null) {
                if (!in_array($algorithm, self::SUPPORTED, true)) throw new \UnexpectedValueException('Unsupported checksum algorithm: ' . WireCodec::string($algorithm));
                $settings['algorithm'] = $algorithm;
                $settings['header'] = 'x-amz-checksum-' . strtolower($algorithm);
            }
        } elseif ($description->get('requestRequired') === true) {
            $settings['algorithm'] = 'MD5';
            $settings['header'] = 'content-md5';
        }
        $mode = $description->get('validationModeMember');
        if (is_string($mode)) {
            $choice = $values[$mode] ?? null;
            if ($choice === null && $responsePolicy === self::WHEN_SUPPORTED) $choice = $values[$mode] = 'ENABLED';
            if ($choice === 'ENABLED') {
                $settings['response'] = array_values(array_filter(WireCodec::items($description->get('responseAlgorithms') ?? []), static fn($name) => in_array($name, self::SUPPORTED, true)));
            }
        }
        return $settings;
    }

    /**
     * A caller-supplied checksum header wins over the computed one.
     * @param array{algorithm: ?string, header: ?string, response: list<string>} $settings
     * @param array<string, string> $headers
     * @return array<string, string>
     */
    public static function request(array $settings, array $headers, ?string $body, ?ByteStream $stream = null): array
    {
        $name = $settings['header'];
        if ($name === null || isset($headers[$name])) return $headers;
        if ($stream === null) $headers[$name] = self::digest($settings['algorithm'], $body ?? '');
        // A one-shot stream would have to be buffered to precede it with its checksum.
        elseif (!$stream->replayable) throw new \InvalidArgumentException('A one-shot stream needs a caller-supplied ' . $name . ' header, or checksum calculation WHEN_REQUIRED');
        else $headers[$name] = self::stream($settings['algorithm'], $stream);
        return $headers;
    }

    /**
     * Validates the first modeled checksum header present, in modeled order.
     * @param array{algorithm: ?string, header: ?string, response: list<string>} $settings
     * @param array<string, string> $headers lowercase names
     */
    public static function response(array $settings, array $headers, string $body): ?string
    {
        foreach ($settings['response'] as $algorithm) {
            $expected = $headers['x-amz-checksum-' . strtolower($algorithm)] ?? null;
            if ($expected === null) continue;
            if (!hash_equals(self::digest($algorithm, $body), $expected)) throw new ChecksumException($algorithm);
            return $algorithm;
        }
        return null;
    }
}
