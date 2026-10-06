<?php
declare(strict_types=1);
// The loopback oracle reads bodies without buffering and validates their framing.
function readExactly($client, int $count, ?HashContext $hash): int
{
    $received = 0;
    while ($received < $count) {
        $chunk = fread($client, min(65536, $count - $received));
        if ($chunk === false || $chunk === '') break;
        if ($hash !== null) hash_update($hash, $chunk);
        $received += strlen($chunk);
    }
    return $received;
}
function readChunked($client, HashContext $hash): int
{
    $received = 0;
    while (true) {
        $line = fgets($client);
        if ($line === false) throw new RuntimeException('Missing request chunk size');
        $encoded = trim(explode(';', $line, 2)[0]);
        if (!preg_match('/^[0-9a-f]+$/iD', $encoded)) throw new RuntimeException('Invalid request chunk size');
        $length = (int)hexdec($encoded);
        if ($length === 0) {
            while (($trailer = fgets($client)) !== false && $trailer !== "\r\n") {}
            if ($trailer === false) throw new RuntimeException('Incomplete request trailers');
            return $received;
        }
        $count = readExactly($client, $length, $hash);
        if ($count !== $length) throw new RuntimeException('Incomplete request chunk body');
        $received += $count;
        // A socket can return only CR; consuming one short read would leave
        // LF to be mistaken for an empty (zero-sized) next chunk.
        $delimiter = '';
        while (strlen($delimiter) < 2) {
            $part = fread($client, 2 - strlen($delimiter));
            if ($part === false || $part === '') throw new RuntimeException('Incomplete request chunk delimiter');
            $delimiter .= $part;
        }
        if ($delimiter !== "\r\n") throw new RuntimeException('Invalid request chunk delimiter');
    }
}
