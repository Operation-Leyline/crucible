# Upgrading from v1 to v4

Moving a client of the Shop API from release <code>v1</code> to release <code>v4</code>, the current release. A client names its release in the <code>Crucible-Version</code> header, and a generated client sends the release it was generated for, so a client moves by being generated, or its SDK installed, for <code>v4</code>. Each section below is one migration, oldest first: what it changed, and what a client of the release before it sees instead.

## Sunset

Release <code>v1</code> is supported until <code>2027&#45;01&#45;01</code>; after that the service may refuse its requests.

## Release v2

Follows v1.

### Renamed <code>GetItemOutput.title</code>

Clients of <code>v1</code> call it <code>name</code>.

### Renamed <code>ListItemsInput.limit</code>

Clients of <code>v1</code> call it <code>max</code>.

### Required <code>ListItemsInput.sort</code>

Clients of <code>v1</code> may leave it out: the service fills it in.

## Release v3

Follows v2.

### Moved <code>GetItem</code> to <code>GET /things/{id}</code>

Clients of <code>v2</code> call <code>GET /items/{id}</code>.

### Reshaped <code>GetItemOutput</code>

<code>title</code>, <code>size</code> became <code>label</code>. Clients of <code>v2</code> use the older definition:

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>title</code> | <code>title</code> | <code>string</code> | Yes | — | — |  Changed in v2. |
| <code>size</code> | <code>size</code> | <code>integer</code> | No | — | — |  |
| <code>kind</code> | <code>kind</code> | Kind | Yes | — | — |  |

### Required <code>ListItemsInput.limit</code>

Clients of <code>v2</code> may leave it out: the service uses <code>20</code>.

### Added <code>HUGE</code> to <code>Kind</code>

Clients of <code>v2</code> receive <code>LARGE</code> instead.

### <code>GetItem</code> can return <code>ItemGoneException</code>

Clients of <code>v2</code> receive <code>ItemNotFoundException</code> instead, with <code>message</code> from <code>message</code>.

### Retyped <code>DeleteItemInput.force</code> to <code>string</code>

Clients of <code>v2</code> use <code>boolean</code>, converted by the service's hook.

### Retyped <code>DeleteItemOutput.deleted</code> to <code>string</code>

Clients of <code>v2</code> use <code>boolean</code>, converted by the service's hook.

### Removed <code>CountItems</code>

Clients of <code>v2</code> can still call it: <code>GET /count</code>.

## Release v4

Follows v3.

### Retyped <code>Label.size</code> to <code>string</code>

Clients of <code>v3</code> use <code>integer</code>, converted by the service's hook.

## New since v1

What was added after <code>v1</code>, up to <code>v4</code>. An older release is served as the current model sees it, so an addition reaches it too, except where an entry names a release that does not see it. One added in place of what an older release's own view had (under another name, or as internal API there) may still differ there: its release page says what changed.

- Operation <code>ListLabels</code>: added in v3.
- Operation <code>RecordPayment</code>: added in v3.
- Data type <code>Label</code>: added in v3.
- Data type <code>Labels</code>: added in v3.
- Operation <code>GetLabel</code>: added in v4.
- Operation <code>PutLabel</code>: added in v4.
