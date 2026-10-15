<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/**
 * Sends HTTP requests for a client. Implement it to plug in a different HTTP
 * stack or a test double through ClientConfig::$transport.
 */
interface HttpTransport
{
    /** Throw NetworkException for transport failures; honor the call deadline and cancellation. */
    public function send(HttpRequest $request, bool $readBody, CallScope $scope): HttpResponse;
    /** Releases the transport's resources. */
    public function close(): void;
}
