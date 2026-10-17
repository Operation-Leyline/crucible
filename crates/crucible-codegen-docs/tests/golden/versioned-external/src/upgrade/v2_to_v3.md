# Upgrading from v2 to v3

Moving a client of the Shop API from release <code>v2</code> to release <code>v3</code>. A client names its release in the <code>Crucible-Version</code> header, and a generated client sends the release it was generated for, so a client moves by being generated, or its SDK installed, for <code>v3</code>. Below, each release's section is the migration that made it, oldest first: what it changed, and what a client of the release before it sees instead. Any sunset on the way comes first, and what was added since comes last.

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

## New since v2

What was added after <code>v2</code>, up to <code>v3</code>. An older release is served as the current model sees it, so an addition generally reaches it too; an entry names a release that does not see it. What an older release had in another form, under another name or not in this audience's view, may be listed here as well.

- Operation <a href="../operations/API_ListLabels.md"><code>ListLabels</code></a>: added in [v3](../releases/v3.md).
- Operation <a href="../operations/API_RecordPayment.md"><code>RecordPayment</code></a>, which Payments calls, not a client: added in [v3](../releases/v3.md).
- Data type <a href="../shapes/Label.md"><code>Label</code></a>: added in [v3](../releases/v3.md).
- Data type <a href="../shapes/Labels.md"><code>Labels</code></a>: added in [v3](../releases/v3.md).
