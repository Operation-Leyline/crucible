<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** One role in the client's role registry (`Roles\ROLES`). */
final readonly class RoleDescription
{
    /**
     * @param string $value what bindings store
     * @param string $description the model's documentation for it, empty when it has none
     * @param list<string> $scopes the scope values it may be bound at, in catalogue order
     * @param list<string> $holders the kinds of principal that may hold it; empty: any
     * @param list<string> $assignedBy the roles whose holders alone may assign it; empty: whoever may write bindings
     * @param list<RolePermission> $permissions what it grants: the permissions this client knows
     */
    public function __construct(
        public string $value,
        public string $description,
        public array $scopes,
        public array $holders,
        public array $assignedBy,
        public array $permissions,
    ) {}
}
