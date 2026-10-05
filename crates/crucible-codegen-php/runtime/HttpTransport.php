<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

interface HttpTransport
{
    /**
     * Throw NetworkException for transport failures; honor the call deadline and cancellation.
     * A request with a $stream sends the bytes of $request->stream->open(). A transport may
     * declare a fourth parameter, ?\Closure $sink: when the runtime passes one, the transport
     * calls $sink($chunk, $headers) for each chunk of a 2xx body instead of buffering it, with
     * lowercase response headers. A transport without it returns the body buffered.
     */
    public function send(HttpRequest $request, bool $readBody, CallScope $scope): HttpResponse;
    public function close(): void;
}
