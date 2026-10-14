<?php
declare(strict_types=1);
// A payload's Content-Type is the modelled member if set, then the payload's
// media type, then a client-wide default: exactly one value either way.
spl_autoload_register(static function(string $class): void {
    if (str_starts_with($class, 'PayloadSdk\\')) require getenv('PHP_PAYLOAD_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, 11)) . '.php';
});
use PayloadSdk\Runtime\ByteStream;
use PayloadSdk\Runtime\CallScope;
use PayloadSdk\Runtime\ClientConfig;
use PayloadSdk\Runtime\HttpRequest;
use PayloadSdk\Runtime\HttpResponse;
use PayloadSdk\Runtime\HttpTransport;
function check(bool $condition, string $message): void { if (!$condition) throw new RuntimeException('Payload bindings check failed: ' . $message); }
final class Fake implements HttpTransport {
    /** @var list<HttpRequest> */
    public array $requests = [];
    /** @var list<?string> */
    public array $bodies = [];
    public function send(HttpRequest $request, bool $readBody, CallScope $scope, ?Closure $sink = null): HttpResponse
    {
        $this->requests[] = $request;
        if ($request->stream !== null) {
            $body = $request->stream->open(); $data = '';
            try { while (($chunk = $body->read(2)) !== '') $data .= $chunk; } finally { $body->close(); }
            $this->bodies[] = $data;
        } else $this->bodies[] = $request->body;
        return new HttpResponse(200);
    }
    public function close(): void {}
}
foreach ([false, true] as $streaming) {
    foreach ([null, 'image/png'] as $contentType) {
        foreach ([null, 'image/jpeg'] as $configured) {
            $label = ($streaming ? 'streaming' : 'buffered') . '/' . ($contentType ?? '-') . '/' . ($configured ?? '-');
            $fake = new Fake();
            $client = new PayloadSdk\PayloadBindingsClient(new ClientConfig(endpoint:'https://example.invalid', transport:$fake,
                headers:$configured === null ? [] : ['Content-Type' => $configured]));
            $fields = $contentType === null ? [] : ['contentType' => $contentType];
            if ($streaming) $client->uploadStream(new PayloadSdk\StreamInput(...['data' => ByteStream::fromIterable(['P', 'NG']), ...$fields]));
            else $client->upload(new PayloadSdk\UploadInput(...['data' => 'PNG', ...$fields]));
            check(count($fake->requests) === 1 && $fake->bodies[0] === 'PNG', $label . ' body');
            $values = [];
            foreach ($fake->requests[0]->headers as $name => $value) if (strtolower((string)$name) === 'content-type') $values[] = $value;
            check($values === [$contentType ?? 'application/octet-stream'], $label . ' content type ' . json_encode($values));
        }
    }
}
echo "PHP payload Content-Type precedence passed\n";
