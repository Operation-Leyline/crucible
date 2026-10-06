<?php
declare(strict_types=1);
require __DIR__ . '/loopback_body.php';
// Socket reads may return fewer bytes than requested, even for a two-byte CRLF.
final class FragmentedInput
{
    public mixed $context;
    public static string $data;
    private int $offset = 0;
    public function stream_open(string $path, string $mode, int $options, ?string &$openedPath): bool { return true; }
    public function stream_read(int $count): string { return substr(self::$data, $this->offset++, 1); }
    public function stream_eof(): bool { return $this->offset >= strlen(self::$data); }
    public function stream_stat(): array { return []; }
}
stream_wrapper_register('fragmented', FragmentedInput::class);
FragmentedInput::$data = "4\r\nwarm\r\n4\r\nmore\r\n0\r\n\r\n";
$stream = fopen('fragmented://upload', 'r');
$hash = hash_init('crc32b');
$received = readChunked($stream, $hash);
fclose($stream);
if ($received !== 8 || hash_final($hash) !== hash('crc32b', 'warmmore')) {
    throw new RuntimeException('Fragmented chunk delimiter truncated or corrupted upload: ' . $received . ' bytes');
}
foreach (["4\r\nwar", "4\r\nwarm\r", "4\r\nwarmXY0\r\n\r\n", "\r\n"] as $invalid) {
    FragmentedInput::$data = $invalid;
    $stream = fopen('fragmented://upload', 'r');
    try {
        readChunked($stream, hash_init('crc32b'));
        throw new LogicException('Invalid framing was accepted');
    } catch (RuntimeException) {
        // An incomplete or malformed upload must fail, never report success.
    } finally { fclose($stream); }
}
echo "PHP fragmented chunk delimiter and malformed framing passed\n";
