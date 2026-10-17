# Upgrading from v2 to v3

Moving a client of the Shop API from release <code>v2</code> to release <code>v3</code>, the current release. A client names its release in the <code>Crucible-Version</code> header, and a generated client sends the release it was generated for, so a client moves by being generated, or its SDK installed, for <code>v3</code>. Below, each release's section is the migration that made it, oldest first: what it changed, and what a client of the release before it sees instead. Any sunset on the way comes first, and what was added since comes last.

## Release v3

Follows [v2](../releases/v2.md).

<p>v3 moves <a href="../operations/API_GetItem.md">GetItem</a> to <code>/things</code>, reports a deleted item as gone, and
retires CountItems: count the names <a href="../operations/API_ListItems.md">ListItems</a> returns.</p>

### Moved <a href="../operations/API_GetItem.md"><code>GetItem</code></a> to <code>GET /things/{id}</code>

<p>Items are things now.</p>

Clients of <code>v2</code> call <code>GET /items/{id}</code>.

### <a href="../operations/API_GetItem.md"><code>GetItem</code></a> can return <a href="../shapes/ItemGone.md"><code>ItemGone</code></a>

<p>Older clients see a deleted item as not found.</p>

Clients of <code>v2</code> receive <a href="../shapes/ItemNotFound.md"><code>ItemNotFound</code></a> instead, with <code>message</code> from <code>message</code>.

### <a href="../operations/API_GetItem.md"><code>GetItem</code></a> can return <a href="../shapes/ItemLocked.md"><code>ItemLocked</code></a>

<p>Older clients see a locked item as not found, its detail saying
who holds it.</p>

Clients of <code>v2</code> receive <a href="../shapes/ItemNotFound.md"><code>ItemNotFound</code></a> instead, with <code>message</code> from <code>message</code>, <code>detail</code> from <code>holder</code>.

### <a href="../operations/API_GetItem.md"><code>GetItem</code></a> can return <a href="../shapes/Conflict.md"><code>Conflict</code></a>

<p>Older clients see a race as not found.</p>

Clients of <code>v2</code> receive <a href="../shapes/ItemNotFound.md"><code>ItemNotFound</code></a> instead, with <code>message</code> from <code>message</code>.

### Added <code>SECRET</code> to <a href="../shapes/Kind.md"><code>Kind</code></a>

<p>Older clients see the service's own kind as large.</p>

Clients of <code>v2</code> receive <code>LARGE</code> instead.

### Required <a href="../operations/API_ListItems.md"><code>ListItemsInput.sort</code></a>

<p>An older request's sort order is the account's.</p>

Clients of <code>v2</code> may leave it out: the service fills it in.

### Retyped <a href="../operations/API_PutItem.md"><code>PutItemOutput.total</code></a> to <code>string</code>

<p>The total is a string, by a declared conversion.</p>

Clients of <code>v2</code> use <code>integer</code>, converted by a declared conversion.

### Removed <code>CountItems</code>

<p>CountItems is gone: count what <a href="../operations/API_ListItems.md">ListItems</a> returns.</p>

Clients of <code>v2</code> can still call it: <code>GET /count</code>.

### Changed how <a href="../operations/API_Audit.md"><code>Audit</code></a> behaves: <code>retention</code>

<p>Audit entries are kept for a year.</p>

## New since v2

What was added after <code>v2</code>, up to <code>v3</code>. An older release is served as the current model sees it, so an addition generally reaches it too; an entry names a release that does not see it. What an older release had in another form, under another name or not in this audience's view, may be listed here as well.

- Operation <a href="../operations/API_DeleteItem.md"><code>DeleteItem</code></a>: added in [v3](../releases/v3.md).
- Member <a href="../shapes/Size.md"><code>Size.unit</code></a>: added in [v3](../releases/v3.md).
