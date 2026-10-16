# GetAccountOutput


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>accountId</code> | <code>accountId</code> | [AccountId](../shapes/AccountId.md) | Yes | — | pattern: <code>&quot;^a-[0-9a-z]{8}$&quot;</code> |  |
| <code>credit</code> | <code>credit</code> | <code>long</code> | Yes | — | — | <p>What only payment events change.</p>  |
