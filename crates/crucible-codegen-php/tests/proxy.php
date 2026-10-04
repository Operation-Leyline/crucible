<?php
declare(strict_types=1);
require getenv('PHP_PACKAGE') . '/vendor/autoload.php';

use Example\Weather\Client;
use Example\Weather\Runtime\ClientConfig;
use Example\Weather\Runtime\NetworkException;

$client = new Client(new ClientConfig(endpoint:'https://example.invalid',timeout:2,maxAttempts:1));
try {
    $client->ping();
    throw new RuntimeException('Proxy CONNECT was mistaken for an API success');
} catch (NetworkException) {
    echo "PHP proxy CONNECT isolation passed\n";
} finally {
    $client->close();
}
