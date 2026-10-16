# CreateWidgetInput


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>name</code> | <code>name</code> | <code>string</code> | Yes | — | — | <p>What <a href="../shapes/Widget.md">Widget.name</a> becomes.</p>  |
| <code>color</code> | <code>color</code> | [Color](../shapes/Color.md) | No | — | — |  |
| <code>choice</code> | <code>choice</code> | [Choice](../shapes/Choice.md) | No | — | — | <p>A <a href="../shapes/Choice.md">Choice</a>: <a href="../shapes/Choice.md">Choice.widget</a> or <a href="../shapes/Choice.md">Choice.label</a>, one of two.</p>  |
| <code>label</code> | <code>label</code> | [Label](../shapes/Label.md) | No | — | length: <code>{&quot;max&quot;:10,&quot;min&quot;:1}</code> | <p>A <a href="../shapes/Label.md">Label</a>, plain text where a language has no type for it.</p>  |
| <code>code</code> | <code>code</code> | [Code](../shapes/Code.md) | No | — | — | <p>A <a href="../shapes/Code.md">Code</a>, such as <a href="../shapes/Code.md">Code.ONE</a>.</p>  |
| <code>widgets</code> | <code>widgets</code> | [Widgets](../shapes/Widgets.md) | No | — | — | <p><a href="../shapes/Widgets.md">Widgets</a>, a list.</p>  |
| <code>byName</code> | <code>byName</code> | [WidgetsByName](../shapes/WidgetsByName.md) | No | — | — | <p><a href="../shapes/WidgetsByName.md">WidgetsByName</a>, a map.</p>  |
| <code>note</code> | <code>note</code> | <code>string</code> | No | — | — | <p>Why a call failed, as <a href="../shapes/WidgetError.md">WidgetError.message</a> says.</p>  |
