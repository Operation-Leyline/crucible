# ValidationException

> **An older release.** This reference documents release <code>v2</code>. The current release is <code>v4</code>: see [its reference](../../../v4/index.html).

<p>A request cannot be parsed or does not satisfy the service's input model.</p>

Type: <code>structure</code>.

## Reasons

Designated member: <code>reason</code>.

- <code>cannotParse</code>
- <code>fieldValidationFailed</code>
- <code>other</code>
- <code>unknownOperation</code>

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>message</code> | <code>message</code> | <code>string</code> | Yes | — | — |  |
| <code>reason</code> | <code>reason</code> | [ValidationExceptionReason](../shapes/ValidationExceptionReason.md) | Yes | — | — |  |
| <code>fieldList</code> | <code>fieldList</code> | [ValidationExceptionFieldList](../shapes/ValidationExceptionFieldList.md) | No | — | — |  |
