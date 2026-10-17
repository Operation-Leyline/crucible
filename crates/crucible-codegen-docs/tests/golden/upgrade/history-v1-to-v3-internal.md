# Upgrading from v1 to v3

Moving a client of the Shop API from release <code>v1</code> to release <code>v3</code>, the current release. A client names its release in the <code>Crucible-Version</code> header, and a generated client sends the release it was generated for, so a client moves by being generated, or its SDK installed, for <code>v3</code>. Each section below is one migration, oldest first: what it changed, and what a client of the release before it sees instead.

## Sunset

Release <code>v1</code> is supported until <code>2027&#45;01&#45;01</code>; after that the service may refuse its requests.

<p>v1 is served until the end of 2026.</p>

## Release v2

Follows v1.

<p>v2 names an item's title, measures its size as an area, and pages
ListItems by <code>limit</code>.</p>

### Renamed <code>GetItemOutput.title</code>

<p>An item's name, <code>name</code> in v1, is its GetItem.title.</p>

Clients of <code>v1</code> call it <code>name</code>.

### Renamed <code>ListItemsInput.limit</code>

<p>The page size is <code>limit</code>.</p>

Clients of <code>v1</code> call it <code>max</code>.

### Required <code>ListItemsInput.limit</code>

<p>An older request without a page size gets 20.</p>

Clients of <code>v1</code> may leave it out: the service uses <code>20</code>.

### Added <code>HUGE</code> to <code>Kind</code>

<p>Older clients see a huge item as large.</p>

Clients of <code>v1</code> receive <code>LARGE</code> instead.

### Reshaped <code>Size</code>

<p>A size is its area.</p>

<code>width</code>, <code>height</code>, <code>depth</code> became <code>area</code>. Clients of <code>v1</code> use the older definition:

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>width</code> | <code>width</code> | <code>integer</code> | Yes | — | — |  |
| <code>height</code> | <code>height</code> | <code>integer</code> | Yes | — | — |  |
| <code>depth</code> | <code>depth</code> | <code>integer</code> | No | — | — |  <strong>Internal</strong> |

### Retyped <code>PutItemInput.count</code> to <code>string</code>

<p>The count is a string.</p>

Clients of <code>v1</code> use <code>integer</code>, converted by the service's hook.

### Retyped <code>PutItemInput.priority</code> to <code>string</code>

<p>The priority is a word, and public.</p>

Clients of <code>v1</code> use <code>integer</code>, converted by the service's hook.

### Changed how <code>ListItems</code> behaves: <code>ordering</code>

<p>ListItems sorts by name.</p>

### Renamed <code>GetItemOutput.secret</code>

<p>The service's own code is its secret.</p>

Clients of <code>v1</code> call it <code>secretCode</code>.

### Renamed <code>GetItemOutput.note</code>

<p>The service's hint is a public note.</p>

Clients of <code>v1</code> call it <code>hint</code>.

### Retired operations

<code>v2</code> dropped them, and no release serves them any more: <code>PingItems</code>.

## Release v3

Follows v2.

<p>v3 moves GetItem to <code>/things</code>, reports a deleted item as gone, and
retires CountItems: count the names ListItems returns.</p>

### Moved <code>GetItem</code> to <code>GET /things/{id}</code>

<p>Items are things now.</p>

Clients of <code>v2</code> call <code>GET /items/{id}</code>.

### <code>GetItem</code> can return <code>ItemGone</code>

<p>Older clients see a deleted item as not found.</p>

Clients of <code>v2</code> receive <code>ItemNotFound</code> instead, with <code>message</code> from <code>message</code>.

### <code>GetItem</code> can return <code>ItemLocked</code>

<p>Older clients see a locked item as not found, its detail saying
who holds it.</p>

Clients of <code>v2</code> receive <code>ItemNotFound</code> instead, with <code>message</code> from <code>message</code>, <code>detail</code> from <code>holder</code>.

### <code>GetItem</code> can return <code>Conflict</code>

<p>Older clients see a race as not found.</p>

Clients of <code>v2</code> receive <code>ItemNotFound</code> instead, with <code>message</code> from <code>message</code>.

### Added <code>SECRET</code> to <code>Kind</code>

<p>Older clients see the service's own kind as large.</p>

Clients of <code>v2</code> receive <code>LARGE</code> instead.

### Required <code>ListItemsInput.sort</code>

<p>An older request's sort order is the account's.</p>

Clients of <code>v2</code> may leave it out: the service fills it in.

### Retyped <code>PutItemOutput.total</code> to <code>string</code>

<p>The total is a string, by a declared conversion.</p>

Clients of <code>v2</code> use <code>integer</code>, converted by a declared conversion.

### Removed <code>CountItems</code>

<p>CountItems is gone: count what ListItems returns.</p>

Clients of <code>v2</code> can still call it: <code>GET /count</code>.

### Changed how <code>Audit</code> behaves: <code>retention</code>

<p>Audit entries are kept for a year.</p>

## New since v1

What was added after <code>v1</code>, up to <code>v3</code>. An older release is served as the current model sees it, so an addition reaches it too, except where an entry names a release that does not see it. One added in place of what an older release's own view had (under another name, or as internal API there) may still differ there: its release page says what changed.

- Operation <code>DeleteItem</code>: added in v3.
- Member <code>Size.unit</code>: added in v3. Clients of v1 do not see it.
