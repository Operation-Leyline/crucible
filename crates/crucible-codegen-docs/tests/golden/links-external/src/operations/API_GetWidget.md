# GetWidget

<p>Reads the <a href="../shapes/Widget.md">Widget</a> that <a href="../operations/API_CreateWidget.md">CreateWidget</a> made, by <a href="../operations/API_GetWidget.md">GetWidget.widgetId</a>,
and returns it as <a href="../operations/API_GetWidget.md">GetWidget.widget</a>. Fails with <a href="../shapes/WidgetError.md">WidgetError</a>.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Request syntax

```http
GET /widgets/{widgetId} HTTP/1.1
Authorization: Bearer {token}
Crucible-Version: 1
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

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

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [WidgetError](../shapes/WidgetError.md) | <code>WidgetError</code> | 404 | client | No | <p>Raised by <a href="../operations/API_GetWidget.md">GetWidget</a> when <a href="../operations/API_GetWidget.md">GetWidget.widgetId</a> names nothing.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

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
