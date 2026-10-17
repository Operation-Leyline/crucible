# Upgrading from v1 to v2

Moving a client of the Shop API from release <code>v1</code> to release <code>v2</code>. A client names its release in the <code>Crucible-Version</code> header, and a generated client sends the release it was generated for, so a client moves by being generated, or its SDK installed, for <code>v2</code>. Each section below is one migration, oldest first: what it changed, and what a client of the release before it sees instead.

## Sunset

Release <code>v1</code> is supported until <code>2027&#45;01&#45;01</code>; after that the service may refuse its requests.

<p>v1 is served until the end of 2026.</p>

## Release v2

Follows [v1](../releases/v1.md).

<p>v2 names an item's title, measures its size as an area, and pages
<a href="../operations/API_ListItems.md">ListItems</a> by <code>limit</code>.</p>

### Renamed <a href="../operations/API_GetItem.md"><code>GetItemOutput.title</code></a>

<p>An item's name, <code>name</code> in v1, is its <a href="../operations/API_GetItem.md">GetItem.title</a>.</p>

Clients of <code>v1</code> call it <code>name</code>.

### Renamed <a href="../operations/API_ListItems.md"><code>ListItemsInput.limit</code></a>

<p>The page size is <code>limit</code>.</p>

Clients of <code>v1</code> call it <code>max</code>.

### Required <a href="../operations/API_ListItems.md"><code>ListItemsInput.limit</code></a>

<p>An older request without a page size gets 20.</p>

Clients of <code>v1</code> may leave it out: the service uses <code>20</code>.

### Added <code>HUGE</code> to <a href="../shapes/Kind.md"><code>Kind</code></a>

<p>Older clients see a huge item as large.</p>

Clients of <code>v1</code> receive <code>LARGE</code> instead.

### Reshaped <a href="../shapes/Size.md"><code>Size</code></a>

<p>A size is its area.</p>

<code>width</code>, <code>height</code>, <code>depth</code> became <code>area</code>. Clients of <code>v1</code> use the older definition:

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>width</code> | <code>width</code> | <code>integer</code> | Yes | — | — |  |
| <code>height</code> | <code>height</code> | <code>integer</code> | Yes | — | — |  |
| <code>depth</code> | <code>depth</code> | <code>integer</code> | No | — | — |  <strong>Internal</strong> |

### Retyped <a href="../operations/API_PutItem.md"><code>PutItemInput.count</code></a> to <code>string</code>

<p>The count is a string.</p>

Clients of <code>v1</code> use <code>integer</code>, converted by the service's hook.

### Retyped <a href="../operations/API_PutItem.md"><code>PutItemInput.priority</code></a> to <code>string</code>

<p>The priority is a word, and public.</p>

Clients of <code>v1</code> use <code>integer</code>, converted by the service's hook.

### Changed how <a href="../operations/API_ListItems.md"><code>ListItems</code></a> behaves: <code>ordering</code>

<p>ListItems sorts by name.</p>

### Renamed <a href="../operations/API_GetItem.md"><code>GetItemOutput.secret</code></a>

<p>The service's own code is its secret.</p>

Clients of <code>v1</code> call it <code>secretCode</code>.

### Renamed <a href="../operations/API_GetItem.md"><code>GetItemOutput.note</code></a>

<p>The service's hint is a public note.</p>

Clients of <code>v1</code> call it <code>hint</code>.

### Retired operations

<code>v2</code> dropped them, and no release serves them any more: <code>PingItems</code>.

## New since v1

Nothing was added after <code>v1</code>, up to <code>v2</code>.
