# Permissions

What a grant names: a resource type and a permission value. Values are compared case-sensitively; a deprecated alias means the same as the value it names.

## <code>graph</code>

Identifiers: <code>organizationId</code>, <code>graphId</code>.

### Permissions

| Permission | Description |
|---|---|
| <code>graph:read</code> | <p>Read a graph and its catalog.</p> |
| <code>graph:view</code> | <strong>Deprecated</strong> alias of <code>graph:read</code>. |
| <code>graph:write</code> |  |

### Permission-only

Required by no operation: the service's policy checks them.

| Permission | Description |
|---|---|
| <code>graph:audit</code> |  |

## <code>history</code>

Identifiers: <code>organizationId</code>, <code>graphId</code>, <code>historyId</code>.

### Permissions

| Permission | Description |
|---|---|
| <code>history:read</code> |  |

### Permission-only

Required by no operation: the service's policy checks them.

| Permission | Description |
|---|---|
| <code>history:restore</code> |  |

## <code>org</code>

Identifiers: <code>organizationId</code>.

### Bundles

Not grantable on their own: the service's policy confers them whole.

| Permission | Description |
|---|---|
| <code>organization:admin</code> | <p>Everything in the organization.</p> |
