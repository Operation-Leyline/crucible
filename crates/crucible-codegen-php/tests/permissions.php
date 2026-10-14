<?php
// Run against a Composer-installed package generated from tests/fixtures/permissions
// with namespace Leyline (PHP_PERMISSIONS_PACKAGE): the open permission classes and
// the registry as an application reads them (docs/permissions.md §6.3).
declare(strict_types=1);
require getenv('PHP_PERMISSIONS_PACKAGE') . '/vendor/autoload.php';

use Leyline\Permissions\{GraphPermission, HistoryPermission, OrganizationPermission};
use Leyline\Runtime\{PermissionDescription, ResourcePermissions};
use const Leyline\Permissions\PERMISSIONS;

function check(bool $value, string $message): void { if (!$value) throw new RuntimeException($message); }

// A grant written by a newer server, or naming an internal permission this
// client never sees, is still a value of the type, kept as it is.
$stored = array_map(fn (string $value) => new GraphPermission($value), json_decode('["graph:read","graph:export","organization:read"]', true));
check($stored[0]->value === GraphPermission::READ && GraphPermission::READ === 'graph:read', 'a known value is its constant');
check($stored[0] == new GraphPermission(GraphPermission::READ), 'equal values compare equal');
check(array_map(fn (GraphPermission $p) => $p->value, $stored) === ['graph:read', 'graph:export', 'organization:read'], 'unknown values are kept');
check(json_encode(array_map(fn (GraphPermission $p) => $p->value, $stored)) === '["graph:read","graph:export","organization:read"]', 'unknown values are written back unchanged');
check($stored[1]->canonical() === $stored[1], 'an unknown value is its own canonical form');

// A deprecated alias keeps its value and canonicalizes to its primary.
check(GraphPermission::VIEW === 'graph:view', 'the alias keeps its value');
check((new GraphPermission(GraphPermission::VIEW))->canonical()->value === GraphPermission::READ, 'the alias canonicalizes to its primary');
check((new GraphPermission(GraphPermission::READ))->canonical()->value === GraphPermission::READ, 'a primary is its own canonical form');
check((new HistoryPermission(HistoryPermission::RESTORE))->canonical()->value === HistoryPermission::RESTORE, 'no alias, no change');
$view = (new ReflectionClassConstant(GraphPermission::class, 'VIEW'))->getDocComment();
check(is_string($view) && str_contains($view, '@deprecated'), 'the alias is deprecated');

// The registry offers grantable permissions and no internal one.
check(array_map(fn (ResourcePermissions $r) => $r->resourceType, PERMISSIONS) === ['graph', 'history', 'org'], 'resource types');
$values = array_merge(...array_map(fn (ResourcePermissions $r) => array_map(fn (PermissionDescription $p) => $p->value, $r->permissions), PERMISSIONS));
check(!in_array('organization:read', $values, true), 'no internal permission in the registry');
check(!in_array('organization:read', (new ReflectionClass(OrganizationPermission::class))->getConstants(), true), 'no internal constant');
$find = function (int $resource, string $value): PermissionDescription {
    foreach (PERMISSIONS[$resource]->permissions as $permission) {
        if ($permission->value === $value) return $permission;
    }
    throw new RuntimeException("missing $value");
};
$admin = $find(2, OrganizationPermission::ADMIN);
check($admin->grantable === false && $admin->tier === 'notGrantable', 'a bundle is not grantable');
$alias = $find(0, 'graph:view');
check($alias->aliasOf === 'graph:read' && $alias->deprecated === true, 'the alias names its primary and is deprecated');
check($find(0, 'graph:read')->aliasOf === null, 'a primary has no aliasOf');
check(str_contains($find(0, 'graph:write')->description, "\n@param is not a tag."), 'descriptions keep their line breaks');
$picker = [];
foreach (PERMISSIONS as $resource) {
    foreach ($resource->permissions as $permission) {
        if ($permission->grantable && !$permission->deprecated) $picker[] = $permission->value;
    }
}
check($picker === ['graph:audit', 'graph:read', 'graph:write', 'history:read', 'history:restore'], 'picker: ' . implode(', ', $picker));
echo "PHP permissions passed\n";
