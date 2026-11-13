# PutRoleBindingInput


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>organizationId</code> | <code>organizationId</code> | [OrganizationId](../shapes/OrganizationId.md) | Yes | — | length: <code>{&quot;max&quot;:40,&quot;min&quot;:3}</code> |  |
| <code>principal</code> | <code>principal</code> | [Name](../shapes/Name.md) | Yes | — | length: <code>{&quot;max&quot;:64,&quot;min&quot;:1}</code> |  |
| <code>role</code> | <code>role</code> | [Role](../shapes/Role.md) | Yes | — | — |  |
| <code>scope</code> | <code>scope</code> | [RoleScopeType](../shapes/RoleScopeType.md) | Yes | — | — |  |
