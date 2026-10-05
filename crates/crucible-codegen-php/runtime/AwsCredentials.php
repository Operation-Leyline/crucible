<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final readonly class AwsCredentials
{
    public function __construct(public string $accessKeyId, public string $secretAccessKey,
        public ?string $sessionToken = null, public ?\DateTimeImmutable $expiresAt = null)
    {
        if ($accessKeyId === '' || $secretAccessKey === '') throw new \InvalidArgumentException('AWS credentials must be nonempty');
    }
    public function __debugInfo(): array { return ['credentials'=>'[redacted]', 'expiresAt'=>$this->expiresAt]; }
}
