<?php
declare(strict_types=1);
// A payload's Content-Type is the modelled member if set, then the payload's
// media type, then a client-wide default: exactly one value either way.
spl_autoload_register(static function(string $class): void {
    if (str_starts_with($class, 'PayloadSdk\\')) require getenv('PHP_PAYLOAD_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, 11)) . '.php';
});
use PayloadSdk\Runtime\CallScope;
use PayloadSdk\Runtime\ClientConfig;
use PayloadSdk\Runtime\HttpRequest;
use PayloadSdk\Runtime\HttpResponse;
use PayloadSdk\Runtime\HttpTransport;
function check(bool $condition, string $message): void { if (!$condition) throw new RuntimeException('Payload bindings check failed: ' . $message); }
final class Fake implements HttpTransport {
    /** @var list<HttpRequest> */
    public array $requests = [];
    public function send(HttpRequest $request, bool $readBody, CallScope $scope): HttpResponse
    {
        $this->requests[] = $request;
        return new HttpResponse(200);
    }
    public function close(): void {}
}
foreach ([null, 'image/png'] as $contentType) {
    foreach ([null, 'image/jpeg'] as $configured) {
        $label = ($contentType ?? '-') . '/' . ($configured ?? '-');
        $fake = new Fake();
        $client = new PayloadSdk\PayloadBindingsClient(new ClientConfig(endpoint:'https://example.invalid', transport:$fake,
            headers:$configured === null ? [] : ['Content-Type' => $configured]));
        $fields = $contentType === null ? [] : ['contentType' => $contentType];
        $client->upload(new PayloadSdk\UploadInput(...['data' => 'PNG', ...$fields]));
        check(count($fake->requests) === 1 && $fake->requests[0]->body === 'PNG', $label . ' body');
        $values = [];
        foreach ($fake->requests[0]->headers as $name => $value) if (strtolower((string)$name) === 'content-type') $values[] = $value;
        check($values === [$contentType ?? 'application/octet-stream'], $label . ' content type ' . json_encode($values));
    }
}
echo "PHP payload Content-Type precedence passed\n";
