# RewriteGraph

<p>Hostile documentation: a backtick in a <code>when</code> literal, and handler
clause documentation that must keep its case, its paragraphs and its
code spans.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Authorization

<p>Requires <code>graph:read</code> on the graph, when <code>mode == "a`b"</code>; <code>graph:write</code> on the graph, checked by the service (HTTP rewrites only, when <code>mode</code> is <code>a`b</code>, and [ops] approve); <code>history:read</code> on the history, checked by the service (see below).</p>

<p><code>history:read</code> on the history, checked by the service:</p>
<p>GraphQL plans are checked twice:</p>
<ul>
<li>once when parsed, against */ text;</li>
<li>once when run, see [bare] and <a href="https://example.com/guide">the guide</a>.</li>
</ul>
<p>@see the runbook; @param is no tag either.</p>

| Mode | Permission | Resource type | Bindings | When | Notes |
|---|---|---|---|---|---|
| check | <code>graph:read</code> | <code>graph</code> | <code>organizationId</code> = <code>organizationId</code>, <code>graphId</code> = <code>graphId</code> | <code>mode == &quot;a`b&quot;</code> |  |
| handler | <code>graph:write</code> | <code>graph</code> |  |  | <p>HTTP rewrites only, when <code>mode</code> is <code>a`b</code>, and [ops] approve.</p> |
| handler | <code>history:read</code> | <code>history</code> |  |  | <p>GraphQL plans are checked twice:</p> <ul> <li>once when parsed, against */ text;</li> <li>once when run, see [bare] and <a href="https://example.com/guide">the guide</a>.</li> </ul> <p>@see the runbook; @param is no tag either.</p> |

## Request syntax

```http
POST /orgs/{organizationId}/graphs/{graphId}/rewrite HTTP/1.1
Content-Type: application/json
Authorization: Bearer {token}
Crucible-Version: 2026-10-01
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>organizationId</code> | <code>path: organizationId</code> | [OrganizationId](../shapes/OrganizationId.md) | Yes | — | pattern: <code>&quot;^org_[a-z0-9]+$&quot;</code> |  |
| <code>graphId</code> | <code>path: graphId</code> | [GraphId](../shapes/GraphId.md) | Yes | — | pattern: <code>&quot;^g_[a-z0-9]+$&quot;</code> |  |

### Request body

```json
{
  "mode": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>mode</code> | <code>body: mode</code> | <code>string</code> | No | — | — |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "rewritten": "<boolean>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>rewritten</code> | <code>body: rewritten</code> | <code>boolean</code> | Yes | — | — |  |

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [NotFoundException](../shapes/NotFoundException.md) | <code>NotFoundException</code> | 404 | client | No | <p>The resource does not exist, or the caller may not know that it does.</p>  |
| [ValidationException](../shapes/ValidationException.md) | <code>ValidationException</code> | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](../shapes/InternalServerException.md) | <code>InternalServerException</code> | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: false. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
POST /orgs/org_organizationid/graphs/g_graphid/rewrite HTTP/1.1
Content-Type: application/json

{
  "mode": "mode"
}
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "rewritten": false
}
```
