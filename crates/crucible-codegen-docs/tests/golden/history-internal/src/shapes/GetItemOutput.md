# GetItemOutput


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>title</code> | <code>title</code> | <code>string</code> | Yes | — | — | <p>The item's title.</p>  Changed in <code>v2</code>. |
| <code>kind</code> | <code>kind</code> | [Kind](../shapes/Kind.md) | Yes | — | — |  |
| <code>size</code> | <code>size</code> | [Size](../shapes/Size.md) | Yes | — | — |  |
| <code>secret</code> | <code>secret</code> | <code>string</code> | No | — | — | <p>A code only the service reads.</p>  <strong>Internal</strong> Changed in <code>v2</code>. |
