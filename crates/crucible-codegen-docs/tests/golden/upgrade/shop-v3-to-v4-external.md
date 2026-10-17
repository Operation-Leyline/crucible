# Upgrading from v3 to v4

Moving a client of the Shop API from release <code>v3</code> to release <code>v4</code>, the current release. A client names its release in the <code>Crucible-Version</code> header, and a generated client sends the release it was generated for, so a client moves by being generated, or its SDK installed, for <code>v4</code>. Below, each release's section is the migration that made it, oldest first: what it changed, and what a client of the release before it sees instead. Any sunset on the way comes first, and what was added since comes last.

## Release v4

Follows v3.

### Retyped <code>Label.size</code> to <code>string</code>

Clients of <code>v3</code> use <code>integer</code>, converted by the service's hook.

## New since v3

What was added after <code>v3</code>, up to <code>v4</code>. Older releases are served as the current model sees them, so an addition generally reaches them too; an entry names a release that does not see it. What an older release had in another form, under another name or not in this audience's view, may be listed here as well.

- Operation <code>GetLabel</code>: added in v4.
- Operation <code>PutLabel</code>: added in v4.
- Member <code>Label.color</code>: added in v4.
