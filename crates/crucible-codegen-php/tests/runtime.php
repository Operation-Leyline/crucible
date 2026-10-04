<?php
declare(strict_types=1);
require getenv('PHP_PACKAGE') . '/vendor/autoload.php';
use Example\Weather\Client;
use Example\Weather\Models\{CreateCityInput,GetCityInput,GetCityPhotoInput,PutCityPhotoInput,ListCitiesInput,ConflictError};
use Example\Weather\Runtime\{ClientConfig, HttpTransport,HttpRequest,HttpResponse,CallScope,Field,ServiceError,CancelledException,TimeoutException,CancellationToken,Timestamp};
error_reporting(E_ALL);
set_error_handler(static function(int $severity,string $message,string $file,int $line): never { throw new ErrorException($message,0,$severity,$file,$line); });
function check(bool $value,string $message): void { if (!$value) throw new RuntimeException($message); }
function throws(string $class, callable $work): Throwable {
    try { $work(); } catch (Throwable $error) { if ($error instanceof $class) return $error; throw $error; }
    throw new RuntimeException('Expected ' . $class);
}
final class Fake implements HttpTransport {
    public array $requests = [];
    public bool $closed = false;
    public function __construct(private Closure $handler) {}
    public function send(HttpRequest $request,bool $readBody,CallScope $scope): HttpResponse {
        $scope->check(); $this->requests[] = $request;
        return ($this->handler)(count($this->requests),$request,$readBody);
    }
    public function close(): void { $this->closed = true; }
}
function sdk(Fake $fake, int $maxAttempts = 3, ?Closure $token = null,array $middleware = [],float $timeout = 5): Client {
    return new Client(new ClientConfig(endpoint:'http://127.0.0.1',transport:$fake,maxAttempts:$maxAttempts,baseDelay:0,token:$token,middleware:$middleware,timeout:$timeout));
}
$cases = json_decode(file_get_contents(getenv('RETRY_CASES')),true,flags:JSON_THROW_ON_ERROR);
foreach ($cases as $case) {
    $fake = new Fake(function($attempt) use ($case): HttpResponse {
        $body = isset($case['error']) ? ['__type'=>$case['error']] : [];
        return new HttpResponse($attempt <= $case['failures'] ? $case['status'] : 200,['x-amzn-errortype'=>'Throttled'],json_encode((object)$body,JSON_THROW_ON_ERROR));
    });
    $client = sdk($fake,$case['maxAttempts']);
    $success = false;
    try { $case['safe'] ? $client->retrySafe() : $client->retryUnsafe(); $success = true; } catch (ServiceError) {}
    check($success === $case['success'] && count($fake->requests) === $case['attempts'],'Retry ' . $case['name']);
    $client->close(); check(!$fake->closed,'Caller transport ownership');
}
$fake = new Fake(function($attempt,HttpRequest $request): HttpResponse {
    check(str_contains($request->url,'a%2Fb%20%2A%3F%23%C3%A9'),'Path escaping');
    check(str_contains($request->url,'at=2026-10-01T00%3A00%3A00Z'),'Timestamp query binding');
    return new HttpResponse(200,['x-request-id'=>'bound'],'{"city":{"id":"x","name":"n"}}');
});
$result = sdk($fake)->getCity(new GetCityInput(cityId:Field::value('a/b *?#é'),at:Field::value(Timestamp::parse('2026-10-01T00:00:00Z'))));
check($result->requestId->value === 'bound','Response header binding');
$fake = new Fake(function($attempt,HttpRequest $request): HttpResponse {
    check($request->body === null && !isset($request->headers['content-type']) && !isset($request->headers['x-amz-content-sha256']),'Empty payload request');
    return new HttpResponse(200,body:'{"photo":{"size":0,"thumbnail":""}}');
});
sdk($fake)->putCityPhoto(new PutCityPhotoInput(cityId:Field::value('x'),photo:Field::value('')));
$fake = new Fake(fn()=>new HttpResponse(200));
check(sdk($fake)->getCityPhoto(new GetCityPhotoInput(cityId:Field::value('x')))->photo->value === '', 'Empty payload response');
$token = 0;
$fake = new Fake(function($attempt,HttpRequest $request): HttpResponse {
    check($request->headers['authorization'] === 'Bearer ' . $attempt,'Refreshed credential');
    check($request->headers['x-amz-content-sha256'] === hash('sha256',$request->body),'Final body digest');
    check($request->headers['x-expected-revision'] === '7','Header binding');
    return new HttpResponse($attempt === 1 ? 503 : 201,body:$attempt === 1 ? '{"__type":"Future"}' : '{"city":{"id":"x","name":"n"}}');
});
$client = sdk($fake,token:function() use (&$token) { return (string)++$token; },middleware:[fn(HttpRequest $r) => $r->withBody($r->body . ' ')]);
$input = new CreateCityInput(name:Field::value('n'),expectedRevision:Field::value(7));
$client->createCity($input);
check(count($fake->requests) === 2 && $fake->requests[0]->body === $fake->requests[1]->body,'Stable retry bytes');
check(!$input->clientToken->isSet && json_decode($fake->requests[0]->body)->clientToken !== '', 'Generated token ownership');
$fake = new Fake(fn() => new HttpResponse(429,['x-request-id'=>'header'],'{"__type":"ThrottlingError","message":42}'));
$error = throws(ServiceError::class,fn() => sdk($fake,2)->ping());
check($error->metadata->status === 429 && $error->metadata->requestId === 'header' && $error->metadata->throttling,'Malformed error metadata');
check(count($fake->requests) === 2,'Malformed error retry');
$fake = new Fake(fn() => new HttpResponse(200,body:'{bad'));
throws(UnexpectedValueException::class,fn() => sdk($fake)->getCity(new GetCityInput(cityId:Field::value('x'))));
check(count($fake->requests) === 1,'Malformed success must not retry');
$fake = new Fake(fn() => new HttpResponse(200,body:'{"cities":[],"nextToken":"same"}'));
$pages = sdk($fake)->listCitiesPages(new ListCitiesInput());
check($fake->requests === [],'Lazy pagination');
$pages->rewind(); check($pages->valid(),'First page');
$pages->next(); check($pages->valid(),'Second page');
throws(UnexpectedValueException::class,fn() => $pages->next());
check(count($fake->requests) === 2,'Token cycle');
$fake = new Fake(fn() => new HttpResponse(503,['retry-after'=>'2147484']));
throws(TimeoutException::class,fn() => sdk($fake,timeout:0.05)->retrySafe());
check(count($fake->requests) === 1,'Retry deadline');
$cancel = new CancellationToken(); $cancel->cancel();
$fake = new Fake(fn() => new HttpResponse(200));
throws(CancelledException::class,fn() => sdk($fake)->ping($cancel));
check($fake->requests === [],'Cancellation before transport');
$client = new Client(new ClientConfig(endpoint:'http://example.invalid',token:fn()=>'secret',transport:$fake));
throws(InvalidArgumentException::class,fn()=>$client->ping());
check($fake->requests === [],'Credential security');
$client = new Client(new ClientConfig(endpoint:getenv('WEATHER_URL'),token:fn()=>'weather-token'));
$city = $client->createCity(new CreateCityInput(name:Field::value('PHP city')))->city;
check($city->name === 'PHP city','Real create');
check($client->getCity(new GetCityInput(cityId:Field::value($city->id)))->city->id === $city->id,'Real read');
$photo = "\x00\x80\xff";
$client->putCityPhoto(new PutCityPhotoInput(cityId:Field::value($city->id),photo:Field::value($photo)));
check($client->getCityPhoto(new GetCityPhotoInput(cityId:Field::value($city->id)))->photo->value === $photo,'Raw photo roundtrip');
$pages = iterator_to_array($client->listCitiesPages(new ListCitiesInput(maxResults:Field::value(1))));
check(count($pages) > 0,'Real pagination');
$client->ping();
throws(ConflictError::class,fn()=>$client->createCity(new CreateCityInput(name:Field::value('PHP city'))));
$client->close();
$fixture = getenv('FIXTURE_URL') . '/native/';
$client = new Client(new ClientConfig(endpoint:$fixture . 'partial',baseDelay:0));
check($client->getCityPhoto(new GetCityPhotoInput(cityId:Field::value('x')))->photo->value === 'complete','Truncated payload retries');
$client->close();
$client = new Client(new ClientConfig(endpoint:$fixture . 'empty-header',headers:['x-empty'=>'']));
$client->ping(); $client->close();
$client = new Client(new ClientConfig(endpoint:$fixture . 'stall',timeout:0.15));
$start = microtime(true);
throws(TimeoutException::class,fn()=>$client->getCity(new GetCityInput(cityId:Field::value('x'))));
check(microtime(true) - $start < 1.5,'Stalled body deadline');
$client->close();
$client = new Client(new ClientConfig(endpoint:$fixture . 'stall',timeout:5));
$start = microtime(true);
$token = new CancellationToken(fn()=>microtime(true) - $start >= 0.1);
throws(CancelledException::class,fn()=>$client->getCity(new GetCityInput(cityId:Field::value('x')),$token));
check(microtime(true) - $start < 1.5,'Stalled body cancellation');
$client->close();
$client = new Client(new ClientConfig(endpoint:$fixture . 'discard',timeout:0.5));
$client->ping(); $client->close();
$client = new Client(new ClientConfig(endpoint:$fixture . 'redirect',maxAttempts:1));
$error = throws(ServiceError::class,fn()=>$client->ping());
check($error->metadata->status === 302,'Redirect refusal'); $client->close();
$token = 0;
$client = new Client(new ClientConfig(endpoint:$fixture . 'echo',token:function() use (&$token) { return (string)++$token; },baseDelay:0));
$client->createCity(new CreateCityInput(name:Field::value('x')));
check($token === 2,'Real refreshed retry credential'); $client->close();
$token = 0;
$client = new Client(new ClientConfig(endpoint:$fixture . 'cache',token:function() use (&$token) { return (string)++$token; }));
check($client->getCity(new GetCityInput(cityId:Field::value('c')))->city->name === 'Bearer 1','First credential');
check($client->getCity(new GetCityInput(cityId:Field::value('c')))->city->name === 'Bearer 2','No cached credential response');
$client->close();
echo "PHP shared retry and real weather HTTP checks passed\n";
