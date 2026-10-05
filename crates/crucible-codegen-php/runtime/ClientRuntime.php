<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final class ClientRuntime
{
    private readonly string $endpoint;
    private readonly HttpTransport $transport;
    private bool $closed = false;
    public function __construct(private readonly ClientConfig $config, private readonly WireCodec $codec,
        private readonly \Closure $errors, string $endpoint, private readonly bool $digest)
    {
        $this->endpoint = rtrim($config->endpoint ?? $endpoint, '/');
        $url = self::url($this->endpoint);
        if (isset($url['query']) || isset($url['fragment'])) throw new \InvalidArgumentException('Endpoint must not have a query or fragment');
        $this->transport = $config->transport ?? new CurlTransport();
    }
    public function close(): void
    {
        if (!$this->closed && $this->config->transport === null) $this->transport->close();
        $this->closed = true;
    }
    private function operation(string $name): JsonObject
    {
        return WireCodec::object(WireCodec::object($this->codec->schema->get('operations'))->get($name));
    }
    public function call(string $name, WireModel $input, ?CancellationToken $cancellation = null): mixed
    {
        $scope = new CallScope($this->config->timeout, $cancellation);
        $scope->check();
        $op = $this->operation($name);
        $values = $op->get('input') === null ? new JsonObject() : WireCodec::object($this->codec->encode($input,$this->codec->context($op->get('input'))));
        $prepared = $this->prepare($op,$values);
        $result = $this->send($op,$prepared,$scope);
        $output = $op->get('output') === null ? new Unit() : $this->codec->decode($result,$this->codec->context($op->get('output')));
        $scope->check();
        return $output;
    }
    public function pages(string $name, WireModel $input, ?CancellationToken $cancellation = null): \Generator
    {
        $op = $this->operation($name);
        // Capture before returning the generator; later mutations cannot alter a page input.
        $initial = WireCodec::object($this->codec->encode($input,$this->codec->context($op->get('input'))));
        return $this->iterate($op,$initial,$cancellation);
    }
    private function iterate(JsonObject $op, JsonObject $initial, ?CancellationToken $cancellation): \Generator
    {
        $values = $initial->members;
        $pagination = WireCodec::object($op->get('pagination'));
        $inputToken = WireCodec::string($pagination->get('input'));
        $seen = [];
        if (isset($values[$inputToken]) && $values[$inputToken] !== '') $seen[$values[$inputToken]] = true;
        while (true) {
            $scope = new CallScope($this->config->timeout,$cancellation);
            $scope->check();
            $result = $this->send($op,$this->prepare($op,new JsonObject($values)),$scope);
            $page = $this->codec->decode($result,$this->codec->context($op->get('output')));
            $scope->check();
            yield $page;
            $token = $result;
            foreach (WireCodec::items($pagination->get('output')) as $part) $token = $token instanceof JsonObject ? $token->get($part) : null;
            if ($token === null || $token === '') return;
            $token = WireCodec::string($token);
            if (isset($seen[$token])) throw new \UnexpectedValueException('Pagination token cycle');
            $seen[$token] = true;
            $values[$inputToken] = $token;
        }
    }
    private function prepare(JsonObject $op, JsonObject $input): HttpRequest
    {
        $values = $input->members;
        $token = $op->get('token');
        if (is_string($token) && !isset($values[$token])) $values[$token] = self::uuid();
        $bindings = WireCodec::object($op->get('request'));
        $path = WireCodec::string($op->get('uri'));
        foreach (WireCodec::items($bindings->get('labels')) as $name) {
            if (!isset($values[$name])) throw new \UnexpectedValueException('Missing label: ' . $name);
            $text = $this->codec->bound($values[$name]);
            if ($text === '') throw new \UnexpectedValueException('Empty label: ' . $name);
            $greedy = in_array($name, $op->get('greedyLabels') ?? [], true);
            $parts = $greedy ? explode('/', $text) : [$text];
            if (in_array('.', $parts, true) || in_array('..', $parts, true)) throw new \UnexpectedValueException('Dot-segment HTTP label: ' . $name);
            $path = str_replace('{' . $name . ($greedy ? '+' : '') . '}', implode('/', array_map('rawurlencode', $parts)), $path);
        }
        $query = [];
        foreach (WireCodec::items($bindings->get('query')) as [$member,$name]) {
            if (!isset($values[$member])) continue;
            foreach (is_array($values[$member]) ? $values[$member] : [$values[$member]] as $value) $query[] = rawurlencode($name) . '=' . rawurlencode($this->codec->bound($value));
        }
        if (is_string($member = $bindings->get('queryParams')) && isset($values[$member])) {
            $reserved = array_column($bindings->get('query'), 1);
            if (str_contains($path, '?')) foreach (explode('&', explode('?', $path, 2)[1]) as $pair) $reserved[] = rawurldecode(explode('=', $pair, 2)[0]);
            $map = WireCodec::object($values[$member])->members; ksort($map, SORT_STRING);
            foreach ($map as $name => $value) {
                $name = (string)$name;
                if (in_array($name, $reserved, true)) continue;
                foreach (is_array($value) ? $value : [$value] as $item) if ($item !== null) $query[] = rawurlencode($name) . '=' . rawurlencode($this->codec->bound($item));
            }
        }
        if ($query !== []) $path .= (str_contains($path,'?') ? '&' : '?') . implode('&',$query);
        $headers = self::headers($this->config->headers);
        foreach (WireCodec::items($bindings->get('headers')) as [$member,$name]) {
            if (isset($values[$member])) $headers[strtolower($name)] = $this->codec->bound(self::media($values[$member],$bindings,$member,true));
        }
        $body = null;
        $explicit = array_map('strtolower', array_column($bindings->get('headers'), 1));
        foreach ($bindings->get('prefixHeaders') ?? [] as [$member, $prefix]) {
            if (!isset($values[$member])) continue;
            $map = WireCodec::object($values[$member])->members; ksort($map, SORT_STRING);
            foreach ($map as $key => $value) {
                $name = strtolower($prefix . $key);
                if (!in_array($name, $explicit, true)) $headers[$name] = $this->codec->bound(self::media($value,$bindings,$member,true));
            }
        }
        $members = WireCodec::items($bindings->get('body'));
        if ($members !== []) {
            $object = [];
            foreach ($members as $name) if (array_key_exists($name,$values)) $object[$name] = $values[$name];
            $body = ExactJson::encode($this->codec->document($op->get('input'), new JsonObject($object), true));
            $headers['content-type'] = 'application/json';
        }
        $payload = $bindings->get('payload');
        if (is_array($payload) && isset($values[$payload[0]])) {
            $value = $values[$payload[0]];
            $kind = $bindings->get('payloadKind') ?? 'blob';
            if ($kind === 'string') $body = WireCodec::string($value);
            elseif ($kind === 'document') $body = ExactJson::encode($this->codec->document($this->payloadTarget($op->get('input'),$payload),$value,true));
            else { $body = base64_decode(WireCodec::string($value),true); if ($body === false) throw new \UnexpectedValueException('Invalid payload base64'); }
            $headers['content-type'] = $payload[1];
        }
        return new HttpRequest($this->endpoint . $path,WireCodec::string($op->get('method')),$headers,$body);
    }
    private function send(JsonObject $op, HttpRequest $prepared, CallScope $scope): JsonObject
    {
        if ($this->closed) throw new \LogicException('Client is closed');
        $safe = $op->get('safe') === true;
        for ($attempt = 1; $attempt <= $this->config->maxAttempts; $attempt++) {
            $scope->check();
            $request = $prepared;
            foreach ($this->config->middleware as $middleware) {
                $scope->check();
                $request = $middleware($request);
                if (!$request instanceof HttpRequest) throw new \UnexpectedValueException('Middleware must return HttpRequest');
            }
            $scope->check();
            $headers = self::headers($request->headers);
            $url = self::url($request->url);
            if (isset($headers['authorization']) && strtolower($url['scheme']) !== 'https' && !in_array(strtolower(trim($url['host'],'[]')),['localhost','127.0.0.1','::1'],true)) {
                throw new \InvalidArgumentException('Credentials require HTTPS or loopback HTTP');
            }
            if ($this->digest) {
                unset($headers['x-amz-content-sha256']);
                if ($request->body !== null || in_array($request->method,['POST','PUT','PATCH'],true)) $headers['x-amz-content-sha256'] = hash('sha256',$request->body ?? '');
            }
            $request = Authentication::apply($request->withHeaders($headers),$op,$this->codec->schema,$this->config,$scope);
            $request = $request->withHeaders(self::headers($request->headers));
            $bindings = WireCodec::object($op->get('response'));
            try { $response = $this->transport->send($request,$bindings->get('body') !== [] || $bindings->get('payload') !== null,$scope); }
            catch (NetworkException $error) {
                $scope->check();
                if (!$safe || $attempt === $this->config->maxAttempts) throw $error;
                $scope->sleep($this->retryDelay($attempt,null));
                continue;
            }
            $scope->check();
            if ($response->status >= 200 && $response->status < 300) return $this->success($op,$response);
            $error = $this->failure($op,$response);
            $retry = $error->metadata->throttling || ($safe && ($error->metadata->retryable || in_array($response->status,[500,502,503,504],true)));
            if (!$retry || $attempt === $this->config->maxAttempts) throw $error;
            $scope->sleep($this->retryDelay($attempt,$response->headers['retry-after'] ?? null));
        }
        throw new \LogicException('No request attempt');
    }
    private function success(JsonObject $op,HttpResponse $response): JsonObject
    {
        $bindings = WireCodec::object($op->get('response'));
        $payload = $bindings->get('payload');
        if (is_array($payload)) $body = new JsonObject([$payload[0]=>$this->payloadValue($op->get('output'),$bindings,$response->body)]);
        elseif ($bindings->get('body') === [] || $response->body === '') $body = new JsonObject();
        else $body = WireCodec::object($this->codec->document($op->get('output'), ExactJson::decode($response->body), false));
        return $op->get('output') === null ? $body : $this->applyHeaders($op->get('output'),$bindings,$response,$body);
    }
    private function applyHeaders(string $target,JsonObject $bindings,HttpResponse $response,JsonObject $body): JsonObject
    {
        $values = $body->members;
        $members = WireCodec::object($this->codec->shape($target)->get('members'));
        foreach ($bindings->get('headers') as [$member,$name]) {
            if (isset($response->headers[strtolower($name)])) $values[$member] = self::media($this->codec->header(WireCodec::object($members->get($member)),$response->headers[strtolower($name)]),$bindings,$member,false);
        }
        $explicit = array_map('strtolower', array_column($bindings->get('headers'), 1));
        foreach ($bindings->get('prefixHeaders') ?? [] as [$member, $prefix]) {
            $prefix = strtolower($prefix); $map = [];
            foreach ($response->headers as $name => $text) if (str_starts_with($name, $prefix) && !in_array($name, $explicit, true)) $map[substr($name, strlen($prefix))] = self::media($text,$bindings,$member,false);
            $values[$member] = new JsonObject($map);
        }
        if (is_string($member = $bindings->get('responseCode'))) $values[$member] = new JsonNumber((string)$response->status);
        return new JsonObject($values);
    }
    private function failure(JsonObject $op,HttpResponse $response): ServiceError
    {
        $body = new JsonObject();
        try { $body = WireCodec::object(ExactJson::decode($response->body)); }
        catch (\UnexpectedValueException) { /* Wire status and headers remain usable. */ }
        $name = $response->headers['x-crucible-error'] ?? (is_string($body->get('__type')) ? $body->get('__type') : null);
        $matches = [];
        foreach (WireCodec::items($op->get('errors')) as $id) {
            $shape = $this->codec->shape($id);
            $traits = WireCodec::object($shape->get('error'));
            if ($name !== null ? ($traits->get('wireCode') ?? $shape->get('name')) === $name : $traits->get('status')->integer() === $response->status) $matches[] = $id;
        }
        $target = count($matches) === 1 ? $matches[0] : null;
        $shape = $target === null ? null : $this->codec->shape($target);
        $traits = $shape?->get('error');
        $metadata = new ErrorMetadata(name:$name ?? $shape?->get('name') ?? 'UnknownError',
            message:is_string($body->get('message')) ? $body->get('message') : 'HTTP ' . $response->status,
            status:$response->status,requestId:$response->headers['x-request-id'] ?? (is_string($body->get('requestId')) ? $body->get('requestId') : null),
            headers:$response->headers,retryable:$traits?->get('retryable') === true,throttling:$traits?->get('throttling') === true);
        if ($target !== null) {
            try {
                $payload = $traits->get('payload');
                $body = is_array($payload) ? new JsonObject([$payload[0]=>$this->payloadValue($target,$traits,$response->body)]) : $this->codec->document($target,$body,false);
                $body = $this->applyHeaders($target,$traits,$response,$body);
                return ($this->errors)($target,$body,$this->codec->context($target),$metadata);
            } catch (\UnexpectedValueException|\TypeError) { /* Keep retry metadata when modeled members are malformed. */ }
        }
        return new UnknownServiceError($metadata);
    }
    private function payloadTarget(string $target,array $payload): string
    {
        return WireCodec::object(WireCodec::object($this->codec->shape($target)->get('members'))->get($payload[0]))->get('target');
    }
    private function payloadValue(string $target,JsonObject $bindings,string $body): mixed
    {
        return match ($bindings->get('payloadKind') ?? 'blob') {
            'string' => WireCodec::string($body),
            'document' => $body === '' ? null : $this->codec->document($this->payloadTarget($target,$bindings->get('payload')),ExactJson::decode($body),false),
            default => base64_encode($body),
        };
    }
    private static function media(mixed $value,JsonObject $bindings,string $member,bool $encode): mixed
    {
        if ($bindings->get('headerEncodings')?->get($member) !== 'base64') return $value;
        if (is_array($value)) return array_map(fn($item)=>self::media($item,$bindings,$member,$encode),$value);
        $text = WireCodec::string($value);
        if ($encode) return base64_encode($text);
        $decoded = base64_decode($text,true);
        if ($decoded === false || base64_encode($decoded) !== $text) throw new \UnexpectedValueException('Invalid media-type header base64');
        return WireCodec::string($decoded);
    }
    private function retryDelay(int $attempt,?string $header): float
    {
        if ($header !== null) {
            if (is_numeric($header) && is_finite((float)$header) && (float)$header >= 0) return (float)$header;
            try { return max(0,Timestamp::fromHttpDate($header)->seconds - microtime(true)); }
            catch (\UnexpectedValueException) { /* Fall back to bounded jitter. */ }
        }
        $cap = min($this->config->maxDelay,$this->config->baseDelay * 2 ** min($attempt - 1,30));
        return $cap * (random_int(0,PHP_INT_MAX) / PHP_INT_MAX);
    }
    private static function url(string $text): array
    {
        $url = parse_url($text);
        if ($url === false || !isset($url['scheme'],$url['host']) || $url['host'] === '' || !in_array(strtolower($url['scheme']),['https','http'],true) || isset($url['user']) || isset($url['pass']) || preg_match('/[\x00-\x20\x7f]/',$text)) {
            throw new \InvalidArgumentException('Absolute HTTP(S) URL without credentials required');
        }
        return $url;
    }
    private static function headers(array $headers): array
    {
        ksort($headers,SORT_STRING);
        $result = [];
        foreach ($headers as $name=>$value) $result[strtolower($name)] = WireCodec::string($value);
        return $result;
    }
    private static function uuid(): string
    {
        $bytes = random_bytes(16);
        $bytes[6] = chr(ord($bytes[6]) & 15 | 64);
        $bytes[8] = chr(ord($bytes[8]) & 63 | 128);
        $hex = bin2hex($bytes);
        return substr($hex,0,8) . '-' . substr($hex,8,4) . '-' . substr($hex,12,4) . '-' . substr($hex,16,4) . '-' . substr($hex,20);
    }
}
