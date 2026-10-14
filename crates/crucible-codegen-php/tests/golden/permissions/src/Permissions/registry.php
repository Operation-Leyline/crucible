<?php
declare(strict_types=1);
namespace Example\Weather\Permissions;

/**
 * Every permission this client knows, by resource type, deprecated aliases included. A grant picker offers only the `grantable` ones.
 *
 * @var list<\Example\Weather\Runtime\ResourcePermissions>
 */
const PERMISSIONS = [
    new \Example\Weather\Runtime\ResourcePermissions(resourceType: 'graph', permissions: [
        new \Example\Weather\Runtime\PermissionDescription(value: 'graph:audit', description: '', tier: 'permissionOnly', grantable: true, deprecated: false),
        new \Example\Weather\Runtime\PermissionDescription(value: 'graph:read', description: 'Read a graph and its catalog.', tier: 'public', grantable: true, deprecated: false),
        new \Example\Weather\Runtime\PermissionDescription(value: 'graph:view', description: '', tier: 'public', grantable: true, deprecated: true, aliasOf: 'graph:read'),
        new \Example\Weather\Runtime\PermissionDescription(value: 'graph:write', description: "Change a graph: */ ends no comment and [bare] links nowhere.\n@param is not a tag.", tier: 'public', grantable: true, deprecated: false),
    ]),
    new \Example\Weather\Runtime\ResourcePermissions(resourceType: 'history', permissions: [
        new \Example\Weather\Runtime\PermissionDescription(value: 'history:read', description: '', tier: 'public', grantable: true, deprecated: false),
        new \Example\Weather\Runtime\PermissionDescription(value: 'history:restore', description: '', tier: 'permissionOnly', grantable: true, deprecated: false),
    ]),
    new \Example\Weather\Runtime\ResourcePermissions(resourceType: 'org', permissions: [
        new \Example\Weather\Runtime\PermissionDescription(value: 'organization:admin', description: 'Everything in the organization.', tier: 'notGrantable', grantable: false, deprecated: false),
    ]),
];
