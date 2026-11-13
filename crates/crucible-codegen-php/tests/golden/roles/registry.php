<?php
declare(strict_types=1);
namespace Example\Weather\Roles;

/**
 * Every predefined role, in the service's catalogue order: what a role picker offers. The service also accepts custom roles, which are not listed.
 *
 * @var list<\Example\Weather\Runtime\RoleDescription>
 */
const ROLES = [
    new \Example\Weather\Runtime\RoleDescription(
        value: 'owner',
        description: 'Runs the workspace.',
        scopes: ['workspace'],
        holders: ['user'],
        assignedBy: ['owner'],
        permissions: [new \Example\Weather\Runtime\RolePermission(resourceType: 'note', permission: 'note:read'), new \Example\Weather\Runtime\RolePermission(resourceType: 'note', permission: 'note:write'), new \Example\Weather\Runtime\RolePermission(resourceType: 'note', permission: 'note:create'), new \Example\Weather\Runtime\RolePermission(resourceType: 'workspace', permission: 'workspace:read'), new \Example\Weather\Runtime\RolePermission(resourceType: 'workspace', permission: 'workspace:admin')],
    ),
    new \Example\Weather\Runtime\RoleDescription(
        value: 'viewer',
        description: 'Reads notes.',
        scopes: ['workspace', 'allNotes', 'note'],
        holders: [],
        assignedBy: [],
        permissions: [new \Example\Weather\Runtime\RolePermission(resourceType: 'note', permission: 'note:read')],
    ),
    new \Example\Weather\Runtime\RoleDescription(
        value: 'editor',
        description: 'Reads, changes and writes notes.',
        scopes: ['allNotes', 'note'],
        holders: [],
        assignedBy: [],
        permissions: [new \Example\Weather\Runtime\RolePermission(resourceType: 'note', permission: 'note:read'), new \Example\Weather\Runtime\RolePermission(resourceType: 'note', permission: 'note:write'), new \Example\Weather\Runtime\RolePermission(resourceType: 'note', permission: 'note:create')],
    ),
    new \Example\Weather\Runtime\RoleDescription(
        value: 'archivist',
        description: 'Edits notes and reads the archives they came from.',
        scopes: ['allNotes'],
        holders: [],
        assignedBy: [],
        permissions: [new \Example\Weather\Runtime\RolePermission(resourceType: 'note', permission: 'note:read'), new \Example\Weather\Runtime\RolePermission(resourceType: 'note', permission: 'note:write'), new \Example\Weather\Runtime\RolePermission(resourceType: 'note', permission: 'note:create')],
    ),
];
