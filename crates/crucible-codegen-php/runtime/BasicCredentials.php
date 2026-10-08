<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** HTTP Basic credentials. The password never appears in dumps. */
final readonly class BasicCredentials
{
    /**
     * @param string $username the user name; it may not contain a colon
     * @param string $password the password, sent only in the Authorization header over HTTPS or loopback HTTP
     * @throws \InvalidArgumentException when the user name contains a colon
     */
    public function __construct(public string $username, public string $password)
    {
        if (str_contains($username, ':')) throw new \InvalidArgumentException('Basic username cannot contain a colon');
    }
    public function __debugInfo(): array { return ['username'=>$this->username, 'password'=>'[redacted]']; }
}
