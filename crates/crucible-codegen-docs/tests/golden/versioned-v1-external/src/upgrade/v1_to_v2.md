# Upgrading from v1 to v2

Moving a client of the Shop API from release <code>v1</code> to release <code>v2</code>. A client names its release in the <code>Crucible-Version</code> header, and a generated client sends the release it was generated for, so a client moves by being generated, or its SDK installed, for <code>v2</code>. Each section below is one migration, oldest first: what it changed, and what a client of the release before it sees instead.

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

## New since v1

Nothing was added after <code>v1</code>, up to <code>v2</code>.
