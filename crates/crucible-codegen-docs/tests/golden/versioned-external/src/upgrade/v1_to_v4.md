# Upgrading from v1 to v4

Moving a client of the Shop API from release <code>v1</code> to release <code>v4</code>, the current release. A client names its release in the <code>Crucible-Version</code> header, and a generated client sends the release it was generated for, so a client moves by being generated, or its SDK installed, for <code>v4</code>. Below, each release's section is the migration that made it, oldest first: what it changed, and what a client of the release before it sees instead. Any sunset on the way comes first, and what was added since comes last.

## Sunset

Release <code>v1</code> is supported until <code>2027&#45;01&#45;01</code>; after that the service may refuse its requests.

## Release v2

Follows [v1](../releases/v1.md).

### Renamed <a href="../operations/API_GetItem.md"><code>GetItemOutput.title</code></a>

Clients of <code>v1</code> call it <code>name</code>.

### Renamed <a href="../operations/API_ListItems.md"><code>ListItemsInput.limit</code></a>

Clients of <code>v1</code> call it <code>max</code>.

### Required <a href="../operations/API_ListItems.md"><code>ListItemsInput.sort</code></a>

Clients of <code>v1</code> may leave it out: the service fills it in.

## Release v3

Follows [v2](../releases/v2.md).

### Moved <a href="../operations/API_GetItem.md"><code>GetItem</code></a> to <code>GET /things/{id}</code>

Clients of <code>v2</code> call <code>GET /items/{id}</code>.

### Reshaped <a href="../shapes/GetItemOutput.md"><code>GetItemOutput</code></a>

<code>title</code>, <code>size</code> became <code>label</code>. Clients of <code>v2</code> use the older definition:

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>title</code> | <code>title</code> | <code>string</code> | Yes | — | — |  Changed in [v2](../releases/v2.md). |
| <code>size</code> | <code>size</code> | <code>integer</code> | No | — | — |  |
| <code>kind</code> | <code>kind</code> | [Kind](../shapes/Kind.md) | Yes | — | — |  |

### Required <a href="../operations/API_ListItems.md"><code>ListItemsInput.limit</code></a>

Clients of <code>v2</code> may leave it out: the service uses <code>20</code>.

### Added <code>HUGE</code> to <a href="../shapes/Kind.md"><code>Kind</code></a>

Clients of <code>v2</code> receive <code>LARGE</code> instead.

### <a href="../operations/API_GetItem.md"><code>GetItem</code></a> can return <a href="../shapes/ItemGoneException.md"><code>ItemGoneException</code></a>

Clients of <code>v2</code> receive <a href="../shapes/ItemNotFoundException.md"><code>ItemNotFoundException</code></a> instead, with <code>message</code> from <code>message</code>.

### Retyped <a href="../operations/API_DeleteItem.md"><code>DeleteItemInput.force</code></a> to <code>string</code>

Clients of <code>v2</code> use <code>boolean</code>, converted by the service's hook.

### Retyped <a href="../operations/API_DeleteItem.md"><code>DeleteItemOutput.deleted</code></a> to <code>string</code>

Clients of <code>v2</code> use <code>boolean</code>, converted by the service's hook.

### Removed <code>CountItems</code>

Clients of <code>v2</code> can still call it: <code>GET /count</code>.

## Release v4

Follows [v3](../releases/v3.md).

### Retyped <a href="../shapes/Label.md"><code>Label.size</code></a> to <code>string</code>

Clients of <code>v3</code> use <code>integer</code>, converted by the service's hook.

## New since v1

What was added after <code>v1</code>, up to <code>v4</code>. Older releases are served as the current model sees them, so an addition generally reaches them too; an entry names a release that does not see it. What an older release had in another form, under another name or not in this audience's view, may be listed here as well.

- Operation <a href="../operations/API_ListLabels.md"><code>ListLabels</code></a>: added in [v3](../releases/v3.md).
- Operation <a href="../operations/API_RecordPayment.md"><code>RecordPayment</code></a>, which Payments calls, not a client: added in [v3](../releases/v3.md).
- Data type <a href="../shapes/Label.md"><code>Label</code></a>: added in [v3](../releases/v3.md).
- Data type <a href="../shapes/Labels.md"><code>Labels</code></a>: added in [v3](../releases/v3.md).
- Operation <a href="../operations/API_GetLabel.md"><code>GetLabel</code></a>: added in [v4](../releases/v4.md).
- Operation <a href="../operations/API_PutLabel.md"><code>PutLabel</code></a>: added in [v4](../releases/v4.md).
- Member <a href="../shapes/Label.md"><code>Label.color</code></a>: added in [v4](../releases/v4.md).
