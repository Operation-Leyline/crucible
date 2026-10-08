<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** AWS credentials for Signature Version 4. Secrets never appear in dumps. */
final readonly class AwsCredentials
{
    /**
     * @param string $accessKeyId the access key ID
     * @param string $secretAccessKey the secret access key
     * @param ?string $sessionToken the session token of temporary credentials
     * @param ?\DateTimeImmutable $expiresAt when temporary credentials expire; signing with expired ones fails
     * @throws \InvalidArgumentException when the key ID or secret is empty
     */
    public function __construct(public string $accessKeyId, public string $secretAccessKey,
        public ?string $sessionToken = null, public ?\DateTimeImmutable $expiresAt = null)
    {
        if ($accessKeyId === '' || $secretAccessKey === '') throw new \InvalidArgumentException('AWS credentials must be nonempty');
    }
    public function __debugInfo(): array { return ['credentials'=>'[redacted]', 'expiresAt'=>$this->expiresAt]; }
}
