# PutCityPhotoInput


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>cityId</code> | <code>cityId</code> | [CityId](../shapes/CityId.md) | Yes | — | pattern: <code>&quot;^c-[a-z0-9]{8}$&quot;</code> |  |
| <code>caption</code> | <code>caption</code> | <code>string</code> | No | — | — |  |
| <code>photo</code> | <code>photo</code> | [Photo](../shapes/Photo.md) | Yes | — | length: <code>{&quot;max&quot;:4096,&quot;min&quot;:1}</code> |  |
