# CreateWidget

<p>Makes a <a href="../shapes/Widget.md">Widget</a>; read it with <a href="../operations/API_GetWidget.md">GetWidget</a>.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: null.

## Request syntax

```http
POST /widgets HTTP/1.1
Content-Type: application/json
```

### Request body

```json
{
  "name": "<string>",
  "color": "<enum>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>name</code> | <code>body: name</code> | <code>string</code> | Yes | — | — | <p>What <a href="../shapes/Widget.md">Widget.name</a> becomes.</p>  |
| <code>color</code> | <code>body: color</code> | [Color](../shapes/Color.md) | No | — | — |  |

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

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [WidgetError](../shapes/WidgetError.md) | 404 | client | No | <p>Raised by <a href="../operations/API_GetWidget.md">GetWidget</a> when <a href="../operations/API_GetWidget.md">GetWidget.widgetId</a> names nothing.</p>  |

## Idempotency and retries

Readonly: false. Idempotent: false.
