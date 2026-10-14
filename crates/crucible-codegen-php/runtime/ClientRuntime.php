<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final class ClientRuntime
{
    private readonly string $endpoint;
    private readonly HttpTransport $transport;
    /** The last HTTP Digest challenge, answered preemptively with increasing nonce counts. */
    private readonly DigestAuthentication $digestSession;
    private bool $closed = false;
    public function __construct(private readonly ClientConfig $config, private readonly WireCodec $codec,
        private readonly \Closure $errors, string $endpoint, private readonly bool $digest)
    {
        $this->endpoint = rtrim($config->endpoint ?? $endpoint, '/');
        // Modeled endpoint rules may resolve every endpoint, so a fixed one is optional there.
        if ($this->endpoint !== '' || !$codec->schema->get('endpointRules') instanceof JsonObject) {
            $url = self::url($this->endpoint);
            if (isset($url['query']) || isset($url['fragment'])) throw new \InvalidArgumentException('Endpoint must not have a query or fragment');
        }
        $this->transport = $config->transport ?? new CurlTransport();
        $this->digestSession = new DigestAuthentication();
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
    /** @param null|\Closure(string): void|resource $responseTarget where a streamed response body goes */
    public function call(string $name, WireModel $input, ?CancellationToken $cancellation = null, mixed $responseTarget = null): mixed
    {
        $scope = new CallScope($this->config->timeout, $cancellation);
        $scope->check();
        $op = $this->operation($name);
        return $this->invoke($name,$op,$this->values($op,$input),$scope,$responseTarget);
    }
    private function invoke(string $name, JsonObject $op, JsonObject $values, CallScope $scope, mixed $responseTarget = null): mixed
    {
        [$op,$prepared] = $this->prepare($name,$op,$values);
        $result = $this->send($op,$prepared,$scope,$responseTarget);
        $output = $op->get('output') === null ? new Unit() : $this->codec->decode($result,$this->codec->context($op->get('output')));
        $scope->check();
        return $output;
    }
    public function pages(string $name, WireModel $input, ?CancellationToken $cancellation = null): \Generator
    {
        $op = $this->operation($name);
        // Capture before returning the generator; later mutations cannot alter a page input.
        $initial = $this->values($op,$input);
        return $this->iterate($name,$op,$initial,$cancellation);
    }
    private function iterate(string $name, JsonObject $op, JsonObject $initial, ?CancellationToken $cancellation): \Generator
    {
        $values = $initial->members;
        $pagination = WireCodec::object($op->get('pagination'));
        $inputToken = WireCodec::string($pagination->get('input'));
        $seen = [];
        if (isset($values[$inputToken]) && $values[$inputToken] !== '') $seen[$values[$inputToken]] = true;
        while (true) {
            $scope = new CallScope($this->config->timeout,$cancellation);
            $scope->check();
            [$prepared,$request] = $this->prepare($name,$op,new JsonObject($values));
            $result = $this->send($prepared,$request,$scope);
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
    /**
     * Polls an operation until a modeled acceptor reaches a terminal state or the
     * total deadline passes. Each attempt is bounded by the remaining waiter time.
     */
    public function wait(string $operation, string $name, WireModel $input, float $timeout, ?float $minDelay = null,
        ?float $maxDelay = null, ?CancellationToken $cancellation = null, ?\Closure $random = null): mixed
    {
        $op = $this->operation($operation);
        $waiter = new Waiter($name, WireCodec::object(WireCodec::object($op->get('waiters'))->get($name)), $timeout, $minDelay, $maxDelay, $random);
        $values = $this->values($op,$input);
        $query = $this->codec->queryValue($op->get('input'),$values);
        $start = hrtime(true);
        $remaining = static fn(): float => $timeout - (hrtime(true) - $start) / 1e9;
        $attempts = 0; $output = null; $error = null;
        while (true) {
            $cancellation?->check();
            if ($remaining() <= 0) throw new WaiterTimeoutException($name,$attempts,$output,$error);
            $attempts++;
            try {
                $output = $this->invoke($operation,$op,$values,new CallScope(min($remaining(),$this->config->timeout),$cancellation));
                $error = null;
            } catch (CancelledException $cancelled) {
                throw $cancelled;
            } catch (\Exception $caught) {
                $output = null; $error = $caught;
            }
            if ($remaining() <= 0) throw new WaiterTimeoutException($name,$attempts,$output,$error);
            $observed = $output === null || $op->get('output') === null ? null : $this->codec->queryValue($op->get('output'),$this->codec->encode($output,$this->codec->context($op->get('output'))));
            $state = $waiter->state($query,$observed,$error);
            if ($state === 'success') return $output;
            if ($state === 'failure') throw new WaiterException($name,$attempts,$output,$error,$waiter->message);
            $until = hrtime(true) + (int)($waiter->delay($attempts,$remaining()) * 1e9);
            while (($left = ($until - hrtime(true)) / 1e9) > 0) {
                $cancellation?->check();
                usleep((int)max(1, min(0.05, $left) * 1e6));
            }
        }
    }
    /** @return array{JsonObject, HttpRequest} the operation with its resolved call settings, and the request */
    private function prepare(string $name, JsonObject $op, JsonObject $input): array
    {
        $values = $input->members;
        // Checksum defaults go into this call's wire values only.
        $checksum = Checksums::prepare($op->get('checksum'),$values,$this->config->requestChecksumCalculation,$this->config->responseChecksumValidation);
        $token = $op->get('token');
        if (is_string($token) && !isset($values[$token])) $values[$token] = self::uuid();
        $endpoint = $this->endpoint;
        $resolved = null;
        $rules = $this->codec->schema->get('endpointRules');
        $prefix = $this->config->disableHostPrefix ? null : $op->get('endpoint');
        if ($rules instanceof JsonObject || $prefix instanceof JsonObject) {
            $query = $this->codec->queryValue($op->get('input'),$input);
            if ($rules instanceof JsonObject) {
                $builtins = $this->config->endpointBuiltIns;
                $builtins['AWS::Region'] ??= $this->config->region;
                $builtins['SDK::Endpoint'] ??= $this->endpoint === '' ? null : $this->endpoint;
                $resolved = Endpoints::resolve($rules,$op->get('endpointParameters'),$query,$this->config->endpointParameters,$builtins);
                $endpoint = rtrim($resolved['url'],'/');
            }
            if ($prefix instanceof JsonObject) $endpoint = rtrim(Endpoints::hostPrefix($endpoint,$prefix,$query),'/');
        }
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
        if (is_string($member = $bindings->get('queryParams')) && isset($values[$member])) {
            $reserved = array_column($bindings->get('query'), 1);
            if (str_contains($path, '?')) foreach (explode('&', explode('?', $path, 2)[1]) as $pair) $reserved[] = rawurldecode(explode('=', $pair, 2)[0]);
            $map = WireCodec::object($values[$member])->members; ksort($map, SORT_STRING);
            foreach ($map as $key => $value) {
                $key = (string)$key;
                if (in_array($key, $reserved, true)) continue;
                foreach (is_array($value) ? $value : [$value] as $item) if ($item !== null) $query[] = rawurlencode($key) . '=' . rawurlencode($this->codec->bound($item));
            }
        }
        if ($query !== []) $path .= (str_contains($path,'?') ? '&' : '?') . implode('&',$query);
        $headers = self::headers($this->config->headers);
        foreach ($resolved['headers'] ?? [] as $header => $items) $headers[strtolower((string)$header)] = implode(', ', $items);
        // A Content-Type bound to an input member outranks the payload's media
        // type, which outranks endpoint and client header defaults.
        $modelledContentType = false;
        foreach (WireCodec::items($bindings->get('headers')) as [$member,$header]) {
            if (!isset($values[$member])) continue;
            $headers[strtolower($header)] = $this->codec->bound(self::media($bound($member),$bindings,$member,true));
            if (strtolower($header) === 'content-type') $modelledContentType = true;
        }
        $body = null;
        $explicit = array_map('strtolower', array_column($bindings->get('headers'), 1));
        foreach ($bindings->get('prefixHeaders') ?? [] as [$member, $prefix]) {
            if (!isset($values[$member])) continue;
            $map = WireCodec::object($values[$member])->members; ksort($map, SORT_STRING);
            foreach ($map as $key => $value) {
                $header = strtolower($prefix . $key);
                if (!in_array($header, $explicit, true)) $headers[$header] = $this->codec->bound(self::media($value,$bindings,$member,true));
            }
        }
        $bodyMembers = WireCodec::items($bindings->get('body'));
        if ($bodyMembers !== []) {
            $object = [];
            foreach ($bodyMembers as $member) if (array_key_exists($member,$values)) $object[$member] = $values[$member];
            $body = ExactJson::encode($this->codec->document($op->get('input'), new JsonObject($object), true));
            $headers['content-type'] = 'application/json';
        }
        $payload = $bindings->get('payload');
        $stream = null;
        if (is_array($payload) && $bindings->get('streaming') === true && isset($values[$payload[0]])) {
            $stream = $values[$payload[0]];
            if (!$stream instanceof ByteStream) throw new \UnexpectedValueException('Expected ByteStream');
            $bounds = WireCodec::object($members->get($payload[0]))->get('length');
            $min = $bounds instanceof JsonObject && $bounds->get('min') !== null ? $bounds->get('min')->integer() : null;
            $max = $bounds instanceof JsonObject && $bounds->get('max') !== null ? $bounds->get('max')->integer() : null;
            // Known lengths are checked before sending; every length is checked again as bytes flow.
            if ($stream->length === null && $bindings->get('requiresLength') === true) throw new \InvalidArgumentException('Stream ' . $payload[0] . ' requires a known length');
            if ($stream->length !== null && (($min !== null && $stream->length < $min) || ($max !== null && $stream->length > $max))) throw new \InvalidArgumentException('Stream ' . $payload[0] . ' length is outside its modeled bounds');
            $stream = $stream->withBounds($min, $max);
            if (!$modelledContentType) $headers['content-type'] = $payload[1];
        } elseif (is_array($payload) && isset($values[$payload[0]])) {
            $value = $values[$payload[0]];
            $kind = $bindings->get('payloadKind') ?? 'blob';
            if ($kind === 'string') $body = WireCodec::string($value);
            elseif ($kind === 'document') $body = ExactJson::encode($this->codec->document($this->payloadTarget($op->get('input'),$payload),$value,true));
            else { $body = base64_decode(WireCodec::string($value),true); if ($body === false) throw new \UnexpectedValueException('Invalid payload base64'); }
            // An empty raw payload is no body at all, so it carries no content type.
            if ($body === '') $body = null;
            elseif (!$modelledContentType) $headers['content-type'] = $payload[1];
        }
        $protocol = $this->codec->schema->get('protocol');
        if ($protocol === 'awsJson1_0' || $protocol === 'awsJson1_1') {
            $body ??= '{}';
            $headers['content-type'] = 'application/x-amz-json-' . ($protocol === 'awsJson1_0' ? '1.0' : '1.1');
            $service = WireCodec::string($this->codec->schema->get('service'));
            $headers['x-amz-target'] = substr($service, strrpos($service, '#') + 1) . '.' . $name;
        }
        // The release this client was generated for (docs/api-versions-clients.md).
        $apiVersion = $this->codec->schema->get('apiVersion');
        if (is_string($apiVersion)) $headers['crucible-version'] = $apiVersion;
        if (($rpc = self::rpc($protocol)) !== null) {
            // RPC v2: the lowered URI is /service/{Service}/operation/{Operation}; Unit input sends no body.
            [$cbor, $media, $marker] = $rpc;
            $headers['smithy-protocol'] = $marker;
            $headers['accept'] = $media;
            unset($headers['content-type']);
            $body = null;
            if (is_string($input = $op->get('input'))) {
                $object = [];
                foreach ($bodyMembers as $member) if (array_key_exists($member,$values)) $object[$member] = $values[$member];
                $document = $this->codec->document($input, new JsonObject($object), true);
                $body = $cbor ? Cbor::encode(Cbor::fromWire($this->codec, $input, $document)) : ExactJson::encode($document);
                $headers['content-type'] = $media;
            }
        }
        $op = new JsonObject([...$op->members, 'name' => $name, 'resolvedEndpoint' => $resolved, 'resolvedChecksum' => $checksum]);
        return [$op, new HttpRequest($endpoint . $path,WireCodec::string($op->get('method')),$headers,$body,$stream)];
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
    private function send(JsonObject $op, HttpRequest $prepared, CallScope $scope, mixed $responseTarget = null): JsonObject
    {
        if ($this->closed) throw new \LogicException('Client is closed');
        $safe = $op->get('safe') === true;
        $checksum = $op->get('resolvedChecksum') ?? ['algorithm'=>null,'header'=>null,'response'=>[]];
        $bindings = WireCodec::object($op->get('response'));
        $streamed = $bindings->get('streaming') === true ? new ResponseStream($responseTarget, $checksum['response']) : null;
        // A one-shot body cannot be reopened, so its call gets exactly one attempt.
        $attempts = $prepared->stream !== null && !$prepared->stream->replayable ? 1 : $this->config->maxAttempts;
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
            $stream = $request->stream;
            if ($stream !== null) {
                unset($headers['content-length'], $headers['transfer-encoding']);
                if ($stream->length !== null) $headers['content-length'] = (string)$stream->length;
                else $headers['transfer-encoding'] = 'chunked';
            }
            // Checksums and digests cover the final bytes, after middleware.
            $headers = Checksums::request($checksum,$headers,$request->body,$stream);
            if ($this->digest) {
                unset($headers['x-amz-content-sha256']);
                if ($stream !== null) {
                    // A one-shot stream is never buffered to hash it; its digest must be declared.
                    if ($stream->sha256 === null && !$stream->replayable) throw new \InvalidArgumentException('A one-shot stream needs withSha256() when content digests are enabled');
                    $stream = $stream->sha256 !== null ? $stream : $stream->withSha256(bin2hex(base64_decode(Checksums::stream('SHA256',$stream))));
                    $request = $request->withStream($stream);
                    $headers['x-amz-content-sha256'] = $stream->sha256;
                } elseif ($request->body !== null || in_array($request->method,['POST','PUT','PATCH'],true)) $headers['x-amz-content-sha256'] = hash('sha256',$request->body ?? '');
            }
            $unsigned = $request->withHeaders($headers);
            $digest = $this->config->digestAuth !== null && !isset($headers['authorization']) && in_array('smithy.api#httpDigestAuth', $op->get('auth') ?? [], true);
            for ($challenged = false; ; $challenged = true) {
                $request = Authentication::apply($unsigned,$op,$this->codec->schema,$this->config,$scope,$this->digestSession);
                $request = $request->withHeaders(self::headers($request->headers));
                $readBody = $bindings->get('body') !== [] || $bindings->get('payload') !== null || $checksum['response'] !== [];
                try { $response = $streamed === null ? $this->transport->send($request,$readBody,$scope) : $this->transport->send($request,$readBody,$scope,$streamed->sink($scope)); }
                catch (NetworkException $error) {
                    $scope->check();
                    // Response bytes the caller already received cannot be taken back.
                    if (!$safe || $attempt === $attempts || ($streamed !== null && !$streamed->retryable())) throw $error;
                    $scope->sleep($this->retryDelay($attempt,null));
                    continue 2;
                }
                $scope->check();
                // A Digest 401 is answered once per attempt: when no credentials went out, or the nonce was stale.
                if ($digest && !$challenged && $response->status === 401 && ($request->stream === null || $request->stream->replayable)
                    && ($challenge = DigestAuthentication::select($response->headers['www-authenticate'] ?? '')) !== null
                    && (!str_starts_with(strtolower($request->headers['authorization'] ?? ''), 'digest ') || strtolower($challenge['stale'] ?? '') === 'true')) {
                    $this->digestSession->accept($challenge);
                    continue;
                }
                break;
            }
            if ($response->status >= 200 && $response->status < 300) {
                $body = null;
                if ($streamed !== null) [$body,$algorithm] = $streamed->finish($response);
                else $algorithm = Checksums::response($checksum,$response->headers,$response->body);
                if ($algorithm !== null && $this->config->checksumValidated !== null) ($this->config->checksumValidated)(WireCodec::string($op->get('name')),$algorithm);
                return $this->success($op,$response,$body);
            }
            $error = $this->failure($op,$response);
            $retry = $error->metadata->throttling || ($safe && ($error->metadata->retryable || in_array($response->status,[500,502,503,504],true)));
            if (!$retry || $attempt === $attempts || ($streamed !== null && !$streamed->retryable())) throw $error;
            $scope->sleep($this->retryDelay($attempt,$response->headers['retry-after'] ?? null));
        }
        throw new \LogicException('No request attempt');
    }
    /** @return ?array{bool, string, string} whether RPC v2 uses CBOR, its media type, and its Smithy-Protocol value */
    private static function rpc(mixed $protocol): ?array
    {
        return match ($protocol) {
            'rpcv2Cbor' => [true, 'application/cbor', 'rpc-v2-cbor'],
            'rpcv2Json' => [false, 'application/json', 'rpc-v2-json'],
            default => null,
        };
    }
    /** Decodes an RPC v2 body to a wire object; null when the response is malformed for the protocol. */
    private function rpcBody(array $rpc, HttpResponse $response): ?JsonObject
    {
        // A response without the request's Smithy-Protocol value is malformed: only its status is usable.
        if (($response->headers['smithy-protocol'] ?? null) !== $rpc[2]) return null;
        if ($response->body === '') return new JsonObject();
        $value = $rpc[0] ? Cbor::decode($response->body) : ExactJson::decode($response->body);
        return $value instanceof JsonObject ? $value : null;
    }
    private function success(JsonObject $op,HttpResponse $response,?ByteStream $stream = null): JsonObject
    {
        if (($rpc = self::rpc($this->codec->schema->get('protocol'))) !== null) {
            if (($response->headers['smithy-protocol'] ?? null) !== $rpc[2]) throw new \UnexpectedValueException('Malformed RPC v2 response: missing or different Smithy-Protocol header');
            $output = $op->get('output');
            if ($output === null) return new JsonObject();
            $body = $this->rpcBody($rpc,$response) ?? throw new \UnexpectedValueException('Malformed RPC v2 response body');
            return WireCodec::object($this->codec->document($output, $rpc[0] ? Cbor::toWire($this->codec,$output,$body) : $body, false));
        }
        $bindings = WireCodec::object($op->get('response'));
        $payload = $bindings->get('payload');
        if (is_array($payload) && $stream !== null) $body = new JsonObject([$payload[0]=>$stream]);
        elseif (is_array($payload)) $body = new JsonObject([$payload[0]=>$this->payloadValue($op->get('output'),$bindings,$response->body)]);
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
        $rpc = self::rpc($this->codec->schema->get('protocol'));
        $type = null;
        if ($rpc !== null) {
            try { $body = $this->rpcBody($rpc,$response) ?? new JsonObject(); }
            catch (\UnexpectedValueException) { /* Malformed: only the status is used. */ }
            // __type is the absolute shape ID; X-Amzn-ErrorType and code are never consulted.
            $type = is_string($body->get('__type')) ? $body->get('__type') : null;
        } else {
            try { $body = WireCodec::object(ExactJson::decode($response->body)); }
            catch (\UnexpectedValueException) { /* Wire status and headers remain usable. */ }
        }
        $aws = in_array($this->codec->schema->get('protocol'), ['awsRestJson1','awsJson1_0','awsJson1_1'], true);
        if ($rpc !== null) {
            $name = $type === null ? null : substr($type, (int)strrpos('#' . $type, '#'));
        } elseif ($aws) {
            // AWS JSON discriminators may carry a namespace and a trailing ":detail".
            $name = $response->headers['x-amzn-errortype'] ?? (is_string($body->get('code')) ? $body->get('code') : (is_string($body->get('__type')) ? $body->get('__type') : null));
            if ($name !== null) { $name = explode(':', $name, 2)[0]; $name = substr($name, (int)strrpos('#' . $name, '#')); }
        } else {
            $name = $response->headers['x-crucible-error'] ?? (is_string($body->get('__type')) ? $body->get('__type') : null);
        }
        $matches = [];
        foreach (WireCodec::items($op->get('errors')) as $id) {
            $shape = $this->codec->shape($id);
            $traits = WireCodec::object($shape->get('error'));
            if ($rpc !== null) {
                // RPC v2 identifies errors only by __type: the absolute shape ID, or a bare name.
                if ($type !== null && (str_contains($type, '#') ? $type === $id : $shape->get('name') === $type)) $matches[] = $id;
            } elseif ($name !== null ? ($traits->get('wireCode') ?? $shape->get('name')) === $name : $traits->get('status')->integer() === $response->status) $matches[] = $id;
        }
        $target = count($matches) === 1 ? $matches[0] : null;
        $shape = $target === null ? null : $this->codec->shape($target);
        $traits = $shape?->get('error');
        $message = $body->get('message') ?? ($aws || $rpc !== null ? $body->get('Message') : null);
        $requestId = $response->headers['x-request-id'] ?? (is_string($body->get('requestId')) ? $body->get('requestId') : null);
        if ($aws) $requestId = $response->headers['x-amzn-requestid'] ?? $response->headers['x-amz-request-id'] ?? $requestId;
        $metadata = new ErrorMetadata(name:$name ?? ($traits?->get('wireCode') ?? $shape?->get('name')) ?? 'UnknownError',
            message:is_string($message) ? $message : 'HTTP ' . $response->status,
            status:$response->status,requestId:$requestId,
            headers:$response->headers,retryable:$traits?->get('retryable') === true,throttling:$traits?->get('throttling') === true);
        if ($target !== null) {
            try {
                $payload = $traits->get('payload');
                if ($rpc !== null && $rpc[0]) $body = Cbor::toWire($this->codec,$target,$body);
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
