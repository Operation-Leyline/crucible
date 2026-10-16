# GetWidget

<p>Reads the <a href="../shapes/Widget.md">Widget</a> that <a href="../operations/API_CreateWidget.md">CreateWidget</a> made, by <a href="../operations/API_GetWidget.md">GetWidget.widgetId</a>,
and returns it as <a href="../operations/API_GetWidget.md">GetWidget.widget</a>. Fails with <a href="../shapes/WidgetError.md">WidgetError</a>.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;crucible.api#httpBearerAuth&quot;]</code>. Optional authentication: null.

## Request syntax

```http
GET /widgets/{widgetId} HTTP/1.1
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>widgetId</code> | <code>path: widgetId</code> | <code>string</code> | Yes | — | — |  |

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

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /widgets/widgetId HTTP/1.1
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
