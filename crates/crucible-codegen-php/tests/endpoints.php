<?php
declare(strict_types=1);
spl_autoload_register(static function(string $class): void {
    if (str_starts_with($class, 'BehaviorSdk\\')) require getenv('PHP_BEHAVIOR_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, 12)) . '.php';
});
use BehaviorSdk\{Input, ServiceClient};
use BehaviorSdk\Runtime\{AwsCredentials, CallScope, ClientConfig, EndpointException, Endpoints, ExactJson, HttpRequest, HttpResponse, HttpTransport, JsonNumber, JsonObject};
error_reporting(E_ALL);
set_error_handler(static function(int $severity, string $message, string $file, int $line): never { throw new ErrorException($message, 0, $severity, $file, $line); });
function check(bool $condition, string $message): void { if (!$condition) throw new RuntimeException($message); }
function fails(string $pattern, callable $work): void
{
    try { $work(); } catch (EndpointException $error) { check(str_contains($error->getMessage(), $pattern), 'unexpected endpoint failure: ' . $error->getMessage()); return; }
    throw new RuntimeException('Expected endpoint failure: ' . $pattern);
}
final class Capture implements HttpTransport {
    /** @var list<HttpRequest> */
    public array $requests = [];
    public function send(HttpRequest $request, bool $readBody, CallScope $scope): HttpResponse
    {
        $this->requests[] = $request;
        return new HttpResponse(200, [], '{"wire-status":"READY"}');
    }
    public function close(): void {}
}
$capture = new Capture();
$identity = new AwsCredentials('key', 'secret');
$client = static fn(array $options = []) => new ServiceClient(new ClientConfig(...['transport' => $capture, 'region' => 'local', 'credentials' => static fn() => $identity, ...$options]));

// Rules resolve the URL, headers, and SigV4 properties; the host prefix comes from the input.
check($client()->read(new Input(account: 'customer'))->status === 'READY', 'resolved call');
$sent = $capture->requests[0];
check(parse_url($sent->url, PHP_URL_HOST) === 'customer.local.example.test' && parse_url($sent->url, PHP_URL_PATH) === '/resource', 'endpoint and host prefix');
check($sent->headers['x-region'] === 'local', 'endpoint header');
check(str_contains($sent->headers['authorization'], '/local/example/aws4_request') && $sent->headers['x-amz-content-sha256'] === 'UNSIGNED-PAYLOAD', 'endpoint signing properties');
check(str_contains($sent->headers['authorization'], 'SignedHeaders=') && str_contains($sent->headers['authorization'], 'x-region'), 'endpoint header is signed');
foreach (['a.b', '-bad', 'x/y', 'x@evil', '', str_repeat('x', 64)] as $label) fails('host label', static fn() => $client()->read(new Input(account: $label)));
check(count($capture->requests) === 1, 'invalid host labels never dispatch');
$client(['disableHostPrefix' => true])->read(new Input(account: 'ignored'));
check(parse_url(end($capture->requests)->url, PHP_URL_HOST) === 'local.example.test', 'host prefix can be disabled');
fails('Unknown region missing', static fn() => $client(['region' => 'missing'])->read(new Input(account: 'customer')));
fails('Region is missing', static fn() => $client(['region' => null])->read(new Input(account: 'customer')));
$client(['region' => 'missing', 'endpointBuiltIns' => ['AWS::Region' => 'local']])->read(new Input(account: 'builtin'));
check(parse_url(end($capture->requests)->url, PHP_URL_HOST) === 'builtin.local.example.test', 'built-in values override the region');
$client()->publicOperation();
check(parse_url(end($capture->requests)->url, PHP_URL_HOST) === 'local.example.test' && !isset(end($capture->requests)->headers['authorization']), 'public operation resolves without credentials');
try { new ClientConfig(endpointParameters: ['bad' => 1]); throw new RuntimeException('Accepted an integer endpoint parameter'); } catch (InvalidArgumentException) {}

// Parameter sources in order: input, client context, then defaults; types are checked.
$ref = static fn(string $name) => new JsonObject(['kind' => 'reference', 'name' => $name]);
$text = static fn(string $value) => new JsonObject(['kind' => 'string', 'parts' => [new JsonObject(['kind' => 'literal', 'value' => $value])]]);
$rules = new JsonObject(['parameters' => new JsonObject(['Name' => new JsonObject(['kind' => 'string', 'required' => true]), 'Flag' => new JsonObject(['kind' => 'boolean', 'required' => true])]),
    'rules' => [new JsonObject(['conditions' => [new JsonObject(['function' => 'isSet', 'arguments' => [$ref('Flag')], 'assign' => null])],
        'result' => new JsonObject(['kind' => 'endpoint', 'url' => $text('https://example.test/base'), 'headers' => new JsonObject(['name' => [$ref('Name')]]), 'properties' => new JsonObject(['flag' => $ref('Flag')])])])]]);
$sources = new JsonObject(['Name' => [new JsonObject(['kind' => 'inputMember', 'member' => 'name']), new JsonObject(['kind' => 'client', 'name' => 'name']), new JsonObject(['kind' => 'default', 'value' => 'default'])],
    'Flag' => [new JsonObject(['kind' => 'builtIn', 'name' => 'Flag'])]]);
$result = Endpoints::resolve($rules, $sources, new JsonObject(['name' => 'input']), ['name' => 'client'], ['Flag' => false]);
check($result['url'] === 'https://example.test/base' && $result['headers']['name'] === ['input'] && $result['properties']['flag'] === false, 'input source and false flag');
check(Endpoints::resolve($rules, $sources, new JsonObject(), ['name' => 'client'], ['Flag' => false])['headers']['name'] === ['client'], 'client source');
check(Endpoints::resolve($rules, $sources, new JsonObject(), [], ['Flag' => false])['headers']['name'] === ['default'], 'default source');
fails('missing', static fn() => Endpoints::resolve($rules, $sources, new JsonObject(), [], []));
fails('wrong type', static fn() => Endpoints::resolve($rules, $sources, new JsonObject(), [], ['Flag' => 'yes']));

// Standard library functions and URL validation.
check(Endpoints::parseUrl('https://example.test:443/a')->get('authority') === 'example.test', 'default port elided');
check(Endpoints::parseUrl('https://[::1]:8443')->get('authority') === '[::1]:8443' && Endpoints::parseUrl('https://[::1]')->get('isIp') === true, 'IPv6 authority');
check(Endpoints::parseUrl('http://127.0.0.1/a/b')->get('normalizedPath') === '/a/b/' && Endpoints::parseUrl('https://example.test')->get('path') === '/', 'paths');
foreach (['file:///path', 'https://user:password@example.test', 'https://example.test?q=x', 'https://example.test#x', 'https://example.test:99999', "https://evil\n.example", 'https://bad_host.test', 'ftp://example.test'] as $url) {
    check(Endpoints::parseUrl($url) === null, 'rejected URL ' . json_encode($url));
}
check(Endpoints::call('substring', ['abcdef', new JsonNumber('1'), new JsonNumber('3'), true]) === 'de', 'reverse substring');
check(Endpoints::call('substring', ['abcdef', new JsonNumber('1'), new JsonNumber('3'), false]) === 'bc', 'substring');
check(Endpoints::call('substring', ["\u{E9}", new JsonNumber('0'), new JsonNumber('1'), false]) === null && Endpoints::call('substring', ['a', new JsonNumber('0'), new JsonNumber('2'), false]) === null, 'substring bounds');
check(Endpoints::call('uriEncode', ["a/b +\u{E9}"]) === 'a%2Fb%20%2B%C3%A9', 'URI encoding');
check(Endpoints::call('getAttr', [ExactJson::decode('{"items":["a","b"]}'), 'items[1]']) === 'b' && Endpoints::call('getAttr', [ExactJson::decode('{"items":["a"]}'), 'items[3]']) === null, 'getAttr');
check(Endpoints::call('isSet', [false]) === true && Endpoints::call('isSet', [null]) === false, 'isSet');
check(Endpoints::call('isValidHostLabel', ['x.y', false]) === false && Endpoints::call('isValidHostLabel', ['x.y', true]) === true, 'host labels');
check(Endpoints::call('booleanEquals', [true, true]) === true && Endpoints::call('stringEquals', ['a', true]) === false, 'equality');
fails('boolean', static fn() => Endpoints::call('not', ['x']));
echo "PHP endpoint rules, host prefixes, and endpoint functions passed\n";
