<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** One permission in the client's permission registry (`Permissions\PERMISSIONS`). */
final readonly class PermissionDescription
{
    /**
     * @param string $value what grants store
     * @param string $description the model's documentation for it, empty when it has none
     * @param string $tier `public`, `permissionOnly` or `notGrantable`: a plain string, since a newer server may add tiers
     * @param bool $grantable false for a bundle the service confers whole: never offer it in a picker
     * @param bool $deprecated whether the model deprecates it; every alias is deprecated
     * @param string|null $aliasOf for a deprecated alias, the value it stands for
     */
    public function __construct(
        public string $value,
        public string $description,
        public string $tier,
        public bool $grantable,
        public bool $deprecated,
        public ?string $aliasOf = null,
    ) {}
}
