# Reveal

> **Internal API.**

<p>Reads a <a href="../shapes/Secret.md">Secret</a>; internal, like <a href="../shapes/Secret.md">Secret.value</a>.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: null.

## Request syntax

```http
GET /secrets HTTP/1.1
```

No modeled body.

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "secret": "<structure>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>secret</code> | <code>body: secret</code> | [Secret](../shapes/Secret.md) | No | — | — |  |

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
GET /secrets HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "secret": {
    "value": "value"
  }
}
```
