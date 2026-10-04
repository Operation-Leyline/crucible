<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

interface HttpTransport
{
    /** Throw NetworkException for transport failures; honor the call deadline and cancellation. */
    public function send(HttpRequest $request, bool $readBody, CallScope $scope): HttpResponse;
    public function close(): void;
}
