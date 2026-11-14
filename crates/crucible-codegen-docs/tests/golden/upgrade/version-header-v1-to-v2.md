# Upgrading from v1 to v2

Moving a client of the Shelves API from release <code>v1</code> to release <code>v2</code>, the current release. A client names its release in the <code>Example-Version</code> header, and a generated client sends the release it was generated for, so a client moves by being generated, or its SDK installed, for <code>v2</code>. Below, each release's section is the migration that made it, oldest first: what it changed, and what a client of the release before it sees instead. Any sunset on the way comes first, and what was added since comes last.

A request without the release header is refused with HTTP 400, except for a sender-signed operation, which is served at the current release.

## Release v2

Follows v1.

### Renamed <code>PaintShelfInput.color</code>

<p>The colour is spelled <code>color</code>.</p>

Clients of <code>v1</code> call it <code>colour</code>.

## New since v1

Nothing was added after <code>v1</code>, up to <code>v2</code>.
