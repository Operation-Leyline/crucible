<?php
declare(strict_types=1);
spl_autoload_register(static function(string $class): void {
    if (str_starts_with($class, 'WireSdk\\')) require getenv('PHP_WIRE_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, 8)) . '.php';
});
function check(bool $condition): void { if (!$condition) throw new RuntimeException('Wire check failed'); }
final class Fake implements WireSdk\Runtime\HttpTransport {
    public function __construct(private bool $failure) {}
    public function send(WireSdk\Runtime\HttpRequest $request, bool $readBody, WireSdk\Runtime\CallScope $scope): WireSdk\Runtime\HttpResponse {
        check($readBody);
        if (str_ends_with($request->url, '/document')) {
            check($request->headers['content-type'] === 'application/json' && json_decode($request->body, true) === ['name'=>'record']);
            return new WireSdk\Runtime\HttpResponse(200, body:$request->body);
        }
        check($request->headers['content-type'] === 'text/plain' && $request->body === 'héllo');
        check($request->headers['x-meta'] === 'eyJhIjoxfQ==, aGVsbG8=' && $request->headers['x-extra-trace'] === 'aGVsbG8=');
        return $this->failure ? new WireSdk\Runtime\HttpResponse(422,['x-crucible-error'=>'TextError'],'invalid text') : new WireSdk\Runtime\HttpResponse(200,['x-meta'=>'eyJhIjoxfQ==, aGVsbG8=','x-extra-trace'=>'aGVsbG8='],$request->body);
    }
    public function close(): void {}
}
$input = new WireSdk\TextInput(text:'héllo',meta:['{"a":1}','hello'],extras:['trace'=>'hello']);
$client = new WireSdk\WireClient(new WireSdk\Runtime\ClientConfig(endpoint:'http://127.0.0.1',transport:new Fake(false)));
$output = $client->text($input);
check($output->text === $input->text && $output->meta === $input->meta && $output->extras === $input->extras);
check($client->document(new WireSdk\DocumentInput(document:new WireSdk\Record(name:'record')))->document->name === 'record');
$client = new WireSdk\WireClient(new WireSdk\Runtime\ClientConfig(endpoint:'http://127.0.0.1',transport:new Fake(true)));
try { $client->text($input); throw new RuntimeException('Expected TextError'); }
catch (WireSdk\TextError $error) { check($error->details === 'invalid text' && $error->metadata->status === 422); }
echo "PHP payloads and media headers passed\n";
