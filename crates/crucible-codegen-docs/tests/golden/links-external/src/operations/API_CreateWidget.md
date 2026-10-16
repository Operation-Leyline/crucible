# CreateWidget

<p>Makes a <a href="../shapes/Widget.md">Widget</a>; read it with <a href="../operations/API_GetWidget.md">GetWidget</a>.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Request syntax

```http
POST /widgets HTTP/1.1
Content-Type: application/json
Authorization: Bearer {token}
Crucible-Version: 1
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request body

```json
{
  "name": "<string>",
  "color": "<enum>",
  "choice": "<union>",
  "label": "<string>",
  "code": "<intEnum>",
  "widgets": "<list>",
  "byName": "<map>",
  "note": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>name</code> | <code>body: name</code> | <code>string</code> | Yes | — | — | <p>What <a href="../shapes/Widget.md">Widget.name</a> becomes.</p>  |
| <code>color</code> | <code>body: color</code> | [Color](../shapes/Color.md) | No | — | — |  |
| <code>choice</code> | <code>body: choice</code> | [Choice](../shapes/Choice.md) | No | — | — | <p>A <a href="../shapes/Choice.md">Choice</a>: <a href="../shapes/Choice.md">Choice.widget</a> or <a href="../shapes/Choice.md">Choice.label</a>, one of two.</p>  |
| <code>label</code> | <code>body: label</code> | [Label](../shapes/Label.md) | No | — | length: <code>{&quot;max&quot;:10,&quot;min&quot;:1}</code> | <p>A <a href="../shapes/Label.md">Label</a>, plain text where a language has no type for it.</p>  |
| <code>code</code> | <code>body: code</code> | [Code](../shapes/Code.md) | No | — | — | <p>A <a href="../shapes/Code.md">Code</a>, such as <a href="../shapes/Code.md">Code.ONE</a>.</p>  |
| <code>widgets</code> | <code>body: widgets</code> | [Widgets](../shapes/Widgets.md) | No | — | — | <p><a href="../shapes/Widgets.md">Widgets</a>, a list.</p>  |
| <code>byName</code> | <code>body: byName</code> | [WidgetsByName](../shapes/WidgetsByName.md) | No | — | — | <p><a href="../shapes/WidgetsByName.md">WidgetsByName</a>, a map.</p>  |
| <code>note</code> | <code>body: note</code> | <code>string</code> | No | — | — | <p>Why a call failed, as <a href="../shapes/WidgetError.md">WidgetError.message</a> says.</p>  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "widget": "<structure>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>widget</code> | <code>body: widget</code> | [Widget](../shapes/Widget.md) | Yes | — | — |  |

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [WidgetError](../shapes/WidgetError.md) | <code>WidgetError</code> | 404 | client | No | <p>Raised by <a href="../operations/API_GetWidget.md">GetWidget</a> when <a href="../operations/API_GetWidget.md">GetWidget.widgetId</a> names nothing.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: false. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
POST /widgets HTTP/1.1
Content-Type: application/json

{
  "byName": {
    "key": {
      "color": "RED",
      "name": "name",
      "object": "object"
    }
  },
  "choice": {
    "widget": {
      "color": "RED",
      "name": "name",
      "object": "object"
    }
  },
  "code": 1,
  "color": "RED",
  "label": "label",
  "name": "name",
  "note": "note",
  "widgets": [
    {
      "color": "RED",
      "name": "name",
      "object": "object"
    }
  ]
}
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "widget": {
    "color": "RED",
    "name": "name",
    "object": "object"
  }
}
```
