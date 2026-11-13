# Permissions

What a grant names: a resource type and a permission value. Values are compared case-sensitively; a deprecated alias means the same as the value it names.

## <code>archive</code>

Identifiers: <code>workspaceId</code>, <code>archiveId</code>.

### Permissions

| Permission | Description |
|---|---|
| <code>archive:read</code> | <p>Read an archive.</p> |

## <code>note</code>

Identifiers: <code>workspaceId</code>, <code>noteId</code>.

### Permissions

| Permission | Description |
|---|---|
| <code>note:create</code> | <p>Write new notes.</p> |
| <code>note:read</code> | <p>Read a note.</p> |
| <code>note:write</code> | <p>Change a note.</p> |

### Internal

The server's own vocabulary: never in a client, a public reference or a refusal.

| Permission | Description |
|---|---|
| <code>note:purge</code> | <p>Remove a note for good.</p> |

## <code>workspace</code>

Identifiers: <code>workspaceId</code>.

### Permissions

| Permission | Description |
|---|---|
| <code>workspace:read</code> | <p>Read the workspace's settings.</p> |

### Bundles

Not grantable on their own: the service's policy confers them whole.

| Permission | Description |
|---|---|
| <code>workspace:admin</code> | <p>Everything in the workspace, conferred whole.</p> |

### Internal

The server's own vocabulary: never in a client, a public reference or a refusal.

| Permission | Description |
|---|---|
| <code>workspace:audit</code> | <p>Count what the workspace holds: the service's own jobs.</p> |
