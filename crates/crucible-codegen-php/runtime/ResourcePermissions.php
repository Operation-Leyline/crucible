<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** One resource type's permissions, sorted by value. */
final readonly class ResourcePermissions
{
    /**
     * @param string $resourceType the authorization name grants carry
     * @param list<PermissionDescription> $permissions
     */
    public function __construct(public string $resourceType, public array $permissions) {}
}
