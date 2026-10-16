# Links

<p>Widgets, linked. Make one with <a href="operations/API_CreateWidget.md">CreateWidget</a> and read it back with
<a href="operations/API_GetWidget.md">the read call</a>; a <a href="shapes/Widget.md">Widget</a> has a <a href="shapes/Widget.md">Widget.name</a> and a
<a href="shapes/Color.md">Color</a>, such as <a href="shapes/Color.md">Color.RED</a>. <a href="index.md">Gadget</a> is a resource, <a href="index.md">Links</a> is this
service and String a prelude type. Internal: Reveal and Secret.</p>
<p><code>[NotALink]</code> in code, and <a href="https://example.com">an ordinary link</a>.</p>

Service version: <code>1</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

## Authentication

- <code>crucible.api#httpBearerAuth</code>: <code>{&quot;kind&quot;:&quot;bearer&quot;}</code>

## Service errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [WidgetError](shapes/WidgetError.md) | 404 | client | No | <p>Raised by <a href="operations/API_GetWidget.md">GetWidget</a> when <a href="operations/API_GetWidget.md">GetWidget.widgetId</a> names nothing.</p>  |

## Operations

### Gadget


| Lifecycle | Operation |
|---|---|
| read | [GetGadget](operations/API_GetGadget.md) |

- [GetGadget](operations/API_GetGadget.md)

### Service operations

- [CreateWidget](operations/API_CreateWidget.md)
- [GetWidget](operations/API_GetWidget.md)
