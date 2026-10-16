# Links

<p>Widgets, linked. Make one with <a href="operations/API_CreateWidget.md">CreateWidget</a> and read it back with
<a href="operations/API_GetWidget.md">the read call</a>; a <a href="shapes/Widget.md">Widget</a> has a <a href="shapes/Widget.md">Widget.name</a> and a
<a href="shapes/Color.md">Color</a>, such as <a href="shapes/Color.md">Color.RED</a>. <a href="index.md">Gadget</a> is a resource, <a href="index.md">Links</a> is this
service and String a prelude type. Internal: Reveal and Secret.</p>
<p><code>[NotALink]</code> in code, and <a href="https://example.com">an ordinary link</a>.</p>

Service version: <code>1</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

Every request names the release it is written for in a header: <code>Crucible&#45;Version&#58; 1</code>, the current release. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

An operation that takes no credential says so on its page.

## Service errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [WidgetError](shapes/WidgetError.md) | <code>WidgetError</code> | 404 | client | No | <p>Raised by <a href="operations/API_GetWidget.md">GetWidget</a> when <a href="operations/API_GetWidget.md">GetWidget.widgetId</a> names nothing.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Operations

### Gadget


| Lifecycle | Operation |
|---|---|
| read | [GetGadget](operations/API_GetGadget.md) |

- [GetGadget](operations/API_GetGadget.md)

### Service operations

- [CreateWidget](operations/API_CreateWidget.md)
- [GetWidget](operations/API_GetWidget.md)
