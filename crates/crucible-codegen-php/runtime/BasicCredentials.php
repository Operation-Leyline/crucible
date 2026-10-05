<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final readonly class BasicCredentials
{
    public function __construct(public string $username, public string $password)
    {
        if (str_contains($username, ':')) throw new \InvalidArgumentException('Basic username cannot contain a colon');
    }
    public function __debugInfo(): array { return ['username'=>$this->username, 'password'=>'[redacted]']; }
}
