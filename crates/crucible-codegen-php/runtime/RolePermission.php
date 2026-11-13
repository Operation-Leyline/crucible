<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** One permission a role grants. */
final readonly class RolePermission
{
    /**
     * @param string $resourceType the authorization name grants carry
     * @param string $permission what grants store
     */
    public function __construct(public string $resourceType, public string $permission) {}
}
