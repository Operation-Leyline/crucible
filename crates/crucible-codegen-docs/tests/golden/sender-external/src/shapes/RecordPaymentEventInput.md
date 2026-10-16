# RecordPaymentEventInput


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>signature</code> | <code>signature</code> | [PaymentSignature](../shapes/PaymentSignature.md) | Yes | — | length: <code>{&quot;max&quot;:1024,&quot;min&quot;:1}</code>; pattern: <code>&quot;^[ -~]+$&quot;</code> |  <strong>Sensitive</strong> |
| <code>event</code> | <code>event</code> | [PaymentEventBody](../shapes/PaymentEventBody.md) | Yes | — | — |  |
