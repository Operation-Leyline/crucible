# GetGadget

<p>Reads a <a href="../index.md">Gadget</a>.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: null.

## Request syntax

```http
GET /gadgets/{gadgetId} HTTP/1.1
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>gadgetId</code> | <code>path: gadgetId</code> | <code>string</code> | Yes | — | — |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "label": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>label</code> | <code>body: label</code> | <code>string</code> | Yes | — | — |  |

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [WidgetError](../shapes/WidgetError.md) | 404 | client | No | <p>Raised by <a href="../operations/API_GetWidget.md">GetWidget</a> when <a href="../operations/API_GetWidget.md">GetWidget.widgetId</a> names nothing.</p>  |

## Idempotency and retries

Readonly: true. Idempotent: false.
