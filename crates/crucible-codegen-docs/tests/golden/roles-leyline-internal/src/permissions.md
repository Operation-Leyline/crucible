# Permissions

What a grant names: a resource type and a permission value. Values are compared case-sensitively; a deprecated alias means the same as the value it names.

## <code>graph</code>

Identifiers: <code>organizationId</code>, <code>graphId</code>.

### Internal

The server's own vocabulary: never in a client, a public reference or a refusal.

| Permission | Description |
|---|---|
| <code>graph:create</code> |  |
| <code>graph:manage</code> |  |
| <code>graph:read</code> |  |
| <code>graph:write</code> |  |

## <code>graphHistory</code>

Identifiers: <code>organizationId</code>, <code>historyId</code>.

### Internal

The server's own vocabulary: never in a client, a public reference or a refusal.

| Permission | Description |
|---|---|
| <code>history:read</code> |  |
| <code>history:restore</code> |  |

## <code>organization</code>

Identifiers: <code>organizationId</code>.

### Internal

The server's own vocabulary: never in a client, a public reference or a refusal.

| Permission | Description |
|---|---|
| <code>apiKey:manage</code> |  |
| <code>apiKey:read</code> |  |
| <code>audit:manage</code> |  |
| <code>audit:read</code> |  |
| <code>backup:manage</code> |  |
| <code>backup:read</code> |  |
| <code>graphGroup:manage</code> |  |
| <code>graphGroup:read</code> |  |
| <code>member:manage</code> |  |
| <code>member:read</code> |  |
| <code>organization:read</code> |  |
| <code>roleBinding:manage</code> |  |
| <code>roleBinding:read</code> |  |
| <code>settings:manage</code> |  |
| <code>settings:read</code> |  |
| <code>usage:read</code> |  |
