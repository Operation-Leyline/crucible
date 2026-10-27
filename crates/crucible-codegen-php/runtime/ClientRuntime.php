<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final class ClientRuntime
{
    private readonly string $endpoint;
    private readonly HttpTransport $transport;
    private bool $closed = false;
    public function __construct(private readonly ClientConfig $config, private readonly WireCodec $codec,
        private readonly \Closure $errors, string $endpoint)
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
    private function values(JsonObject $op, WireModel $input): JsonObject
    {
        return $op->get('input') === null ? new JsonObject() : WireCodec::object($this->codec->encode($input,$this->codec->context($op->get('input'))));
    }
    public function call(string $name, WireModel $input, ?CancellationToken $cancellation = null): mixed
    {
        $scope = new CallScope($this->config->timeout, $cancellation);
        $scope->check();
        $op = $this->operation($name);
        $result = $this->send($op,$this->prepare($op,$this->values($op,$input)),$scope);
        $output = $op->get('output') === null ? new Unit() : $this->codec->decode($result,$this->codec->context($op->get('output')));
        $scope->check();
        return $output;
    }
    public function pages(string $name, WireModel $input, ?CancellationToken $cancellation = null): \Generator
    {
        $op = $this->operation($name);
        // Capture before returning the generator; later mutations cannot alter a page input.
        $initial = $this->values($op,$input);
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
        $members = $op->get('input') === null ? new JsonObject() : WireCodec::object($this->codec->shape($op->get('input'))->get('members'));
        $formats = $bindings->get('timestampFormats');
        // HTTP bindings may use a timestamp format other than the member's document format.
        $bound = fn(string $member): mixed => $formats instanceof JsonObject && is_string($format = $formats->get($member))
            ? $this->retime($values[$member],WireCodec::object($members->get($member)),$format,true) : $values[$member];
        $path = WireCodec::string($op->get('uri'));
        foreach (WireCodec::items($bindings->get('labels')) as $label) {
            if (!isset($values[$label])) throw new \UnexpectedValueException('Missing label: ' . $label);
            $text = $this->codec->bound($bound($label));
            if ($text === '') throw new \UnexpectedValueException('Empty label: ' . $label);
            $greedy = in_array($label, $op->get('greedyLabels') ?? [], true);
            $parts = $greedy ? explode('/', $text) : [$text];
            if (in_array('.', $parts, true) || in_array('..', $parts, true)) throw new \UnexpectedValueException('Dot-segment HTTP label: ' . $label);
            $path = str_replace('{' . $label . ($greedy ? '+' : '') . '}', implode('/', array_map('rawurlencode', $parts)), $path);
        }
        $query = [];
        foreach (WireCodec::items($bindings->get('query')) as [$member,$wire]) {
            if (!isset($values[$member])) continue;
            $value = $bound($member);
            foreach (is_array($value) ? $value : [$value] as $item) $query[] = rawurlencode($wire) . '=' . rawurlencode($this->codec->bound($item));
        }
        if ($query !== []) $path .= (str_contains($path,'?') ? '&' : '?') . implode('&',$query);
        $headers = self::headers($this->config->headers);
        // A Content-Type bound to an input member outranks the payload's media
        // type, which outranks client header defaults.
        $modelledContentType = false;
        foreach (WireCodec::items($bindings->get('headers')) as [$member,$header]) {
            if (!isset($values[$member])) continue;
            $headers[strtolower($header)] = $this->codec->bound(self::media($bound($member),$bindings,$member,true));
            if (strtolower($header) === 'content-type') $modelledContentType = true;
        }
        $body = null;
        $bodyMembers = WireCodec::items($bindings->get('body'));
        if ($bodyMembers !== []) {
            $object = [];
            foreach ($bodyMembers as $member) if (array_key_exists($member,$values)) $object[$member] = $values[$member];
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
            // An empty raw payload is no body at all, so it carries no content type.
            if ($body === '') $body = null;
            elseif (!$modelledContentType) $headers['content-type'] = $payload[1];
        }
        // The release this client was generated for (docs/api-versions-clients.md).
        $apiVersion = $this->codec->schema->get('apiVersion');
        if (is_string($apiVersion)) $headers['crucible-version'] = $apiVersion;
        return new HttpRequest($this->endpoint . $path,WireCodec::string($op->get('method')),$headers,$body);
    }
    /** Converts timestamps between a member's document format and an HTTP binding format. */
    private function retime(mixed $value, JsonObject $member, string $format, bool $toBinding): mixed
    {
        $target = WireCodec::string($member->get('target'));
        $shape = $this->codec->shape($target);
        if ($shape->get('kind') === 'list') {
            $element = WireCodec::object($shape->get('member'));
            return array_map(fn($item) => $item === null ? null : $this->retime($item,$element,$format,$toBinding), WireCodec::items($value));
        }
        if ($shape->get('kind') !== 'timestamp') return $value;
        [$from,$to] = $toBinding ? [$member->get('format'),$format] : [$format,$member->get('format')];
        $timestamp = $this->codec->decode($value,new WireContext($this->codec,$target,$from));
        return $this->codec->encode($timestamp,new WireContext($this->codec,$target,$to));
    }
    private function send(JsonObject $op, HttpRequest $prepared, CallScope $scope): JsonObject
    {
        if ($this->closed) throw new \LogicException('Client is closed');
        $safe = $op->get('safe') === true;
        $bindings = WireCodec::object($op->get('response'));
        $attempts = $this->config->maxAttempts;
        for ($attempt = 1; $attempt <= $attempts; $attempt++) {
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
            $request = Authentication::apply($request->withHeaders($headers),$op,$this->config,$scope);
            $request = $request->withHeaders(self::headers($request->headers));
            $readBody = $bindings->get('body') !== [] || $bindings->get('payload') !== null;
            try { $response = $this->transport->send($request,$readBody,$scope); }
            catch (NetworkException $error) {
                $scope->check();
                if (!$safe || $attempt === $attempts) throw $error;
                $scope->sleep($this->retryDelay($attempt,null));
                continue;
            }
            $scope->check();
            if ($response->status >= 200 && $response->status < 300) return $this->success($op,$response);
            $error = $this->failure($op,$response);
            $retry = $error->metadata->throttling || ($safe && ($error->metadata->retryable || in_array($response->status,[500,502,503,504],true)));
            if (!$retry || $attempt === $attempts) throw $error;
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
        $formats = $bindings->get('timestampFormats');
        foreach ($bindings->get('headers') as [$member,$name]) {
            if (!isset($response->headers[strtolower($name)])) continue;
            $definition = WireCodec::object($members->get($member));
            $format = $formats instanceof JsonObject ? $formats->get($member) : null;
            $value = self::media($this->codec->header($definition,$response->headers[strtolower($name)],$format),$bindings,$member,false);
            $values[$member] = is_string($format) ? $this->retime($value,$definition,$format,false) : $value;
        }
        return new JsonObject($values);
    }
    private function failure(JsonObject $op,HttpResponse $response): ServiceError
    {
        $body = new JsonObject();
        // BEGIN ERROR DOCUMENTATION
        $validErrorObject = strspn($response->body, " \t\r\n") === strlen($response->body);
        // END ERROR DOCUMENTATION
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
        // BEGIN ERROR DOCUMENTATION
        $documentation = $traits?->get('documentation');
        // END ERROR DOCUMENTATION
        $message = $body->get('message');
        $requestId = $response->headers['x-request-id'] ?? (is_string($body->get('requestId')) ? $body->get('requestId') : null);
        $metadata = new ErrorMetadata(name:$name ?? ($traits?->get('wireCode') ?? $shape?->get('name')) ?? 'UnknownError',
            message:is_string($message) ? $message : 'HTTP ' . $response->status,
            status:$response->status,requestId:$requestId,
            headers:$response->headers,retryable:$traits?->get('retryable') === true,throttling:$traits?->get('throttling') === true);
        if ($target !== null) {
            try {
                $payload = $traits->get('payload');
                // BEGIN ERROR DOCUMENTATION
                if (!is_array($payload) && !$validErrorObject) throw new \UnexpectedValueException('Invalid error object');
                // END ERROR DOCUMENTATION
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
    // BEGIN ERROR DOCUMENTATION
    private function documentedMetadata(ErrorMetadata $metadata, ?JsonObject $documentation, JsonObject $body): ErrorMetadata
    {
        $member = $documentation?->get('reasonMember');
        $reason = is_string($member)
            ? ($body->has($member) ? $body->get($member) : $documentation->get('reasonDefault')) : null;
        $pages = $documentation?->get('reasonUrls');
        if (!is_string($reason) || !($pages instanceof JsonObject) || !$pages->has($reason)) return $metadata;
        return new ErrorMetadata(name:$metadata->name,message:$metadata->message,status:$metadata->status,
            requestId:$metadata->requestId,headers:$metadata->headers,retryable:$metadata->retryable,
            throttling:$metadata->throttling,docUrl:$pages->get($reason));
    }
    // END ERROR DOCUMENTATION
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
