# Upgrading from v3 to v4

Moving a client of the Shop API from release <code>v3</code> to release <code>v4</code>, the current release. A client names its release in the <code>Crucible-Version</code> header, and a generated client sends the release it was generated for, so a client moves by being generated, or its SDK installed, for <code>v4</code>. Each section below is one migration, oldest first: what it changed, and what a client of the release before it sees instead.

## Release v4

Follows [v3](../releases/v3.md).

### Retyped <a href="../shapes/Label.md"><code>Label.size</code></a> to <code>string</code>

Clients of <code>v3</code> use <code>integer</code>, converted by the service's hook.

## New since v3

What was added after <code>v3</code>, up to <code>v4</code>. An older release is served as the current model sees it, so an addition reaches it too, except where an entry names a release that does not see it. One added in place of what an older release's own view had (under another name, or as internal API there) may still differ there: its release page says what changed.

- Operation <a href="../operations/API_GetLabel.md"><code>GetLabel</code></a>: added in [v4](../releases/v4.md).
- Operation <a href="../operations/API_PutLabel.md"><code>PutLabel</code></a>: added in [v4](../releases/v4.md).
- Member <a href="../shapes/Label.md"><code>Label.color</code></a>: added in [v4](../releases/v4.md).
