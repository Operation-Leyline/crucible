<?php
declare(strict_types=1);
spl_autoload_register(static function(string $class): void {
    if (str_starts_with($class,'AuthSdk\\')) require getenv('PHP_AUTH_PACKAGE') . '/src/' . str_replace('\\','/',substr($class,8)) . '.php';
});
use AuthSdk\Runtime\{Authentication,AwsCredentials,BasicCredentials,CachedCredentials,ClientConfig,Credentials,HttpRequest,HttpResponse,HttpTransport,CallScope};
function check(bool $condition): void { if (!$condition) throw new RuntimeException('Auth check failed'); }
final class Capture implements HttpTransport {
    public ?HttpRequest $request = null;
    public function send(HttpRequest $request,bool $readBody,CallScope $scope): HttpResponse { $this->request = $request; return new HttpResponse(204); }
    public function close(): void {}
}
$capture = new Capture();
$client = new AuthSdk\AuthClient(new ClientConfig(endpoint:'http://127.0.0.1',transport:$capture,basicAuth:static fn()=>new BasicCredentials('user','páss'),apiKey:static fn()=>throw new RuntimeException('Wrong provider')));
$client->read(); check($capture->request->headers['authorization'] === 'Basic ' . base64_encode('user:páss'));
$client->publicOperation(); check(!isset($capture->request->headers['authorization']));
$client = new AuthSdk\AuthClient(new ClientConfig(endpoint:'http://127.0.0.1',transport:$capture,apiKey:static fn()=>'secret'));
$client->read(); check($capture->request->headers['x-api-key'] === 'ApiKey secret');
$client = new AuthSdk\AuthClient(new ClientConfig(endpoint:'http://127.0.0.1',transport:$capture)); $client->optional();
try { $client->read(); throw new RuntimeException('Expected missing credential failure'); } catch (UnexpectedValueException) {}
$client = new AuthSdk\AuthClient(new ClientConfig(endpoint:'http://127.0.0.1',transport:$capture,headers:['authorization'=>'Basic preset'],basicAuth:static fn()=>throw new RuntimeException('Provider invoked despite explicit auth'))); $client->read();
$request = new HttpRequest('https://examplebucket.s3.amazonaws.com/test.txt','GET',['range'=>'bytes=0-9']);
$identity = new AwsCredentials('AKIAIOSFODNN7EXAMPLE','wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY');
$signed = Authentication::sign($request,$identity,'s3','us-east-1',new DateTimeImmutable('2013-05-24T00:00:00Z'));
check(str_ends_with($signed->headers['authorization'],'Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41'));
$client = new AuthSdk\AuthClient(new ClientConfig(endpoint:'https://example.invalid',transport:$capture,credentials:static fn()=>$identity,region:'us-east-1',middleware:[static fn($r)=>$r->withHeaders(['x-custom'=>'signed'])]));
$client->read(); check(str_contains($capture->request->headers['authorization'],'x-custom') && str_contains($capture->request->headers['authorization'],'/example/aws4_request'));
$now = 1000.0; $calls = 0;
$cache = new CachedCredentials(function() use (&$calls,&$now) { $calls++; return new AwsCredentials('access','secret',expiresAt:new DateTimeImmutable('@' . ($now + 10))); }, clock:static function() use (&$now) { return $now; });
$cache(); $cache(); check($calls === 1); $now += 6; $cache(); check($calls === 2);
$path = tempnam(sys_get_temp_dir(),'crucible-profile-'); file_put_contents($path,"[test]\naws_access_key_id=access\naws_secret_access_key=secret\ncredential_process=must-never-run\n");
try { check(Credentials::profile('test',$path)?->accessKeyId === 'access'); } finally { unlink($path); }
$chain = Credentials::chain(static fn()=>null,static fn()=>$identity); check($chain() === $identity);
echo "PHP authentication, public SigV4 oracle, and credential providers passed\n";
