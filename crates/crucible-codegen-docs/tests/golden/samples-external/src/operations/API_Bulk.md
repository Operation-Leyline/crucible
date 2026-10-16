# Bulk

<p>Values a sample cannot show in full, or can only show exactly.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[]</code>. Optional authentication: null.

## Request syntax

```http
POST /bulk HTTP/1.1
Content-Type: application/json
X-Bell: {headerBell}
X-Blank: {headerBlank}
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>headerBell</code> | <code>headers: X-Bell</code> | [Bell](../shapes/Bell.md) | No | — | pattern: <code>&quot;^[\\x01]$&quot;</code> |  |
| <code>headerBlank</code> | <code>headers: X-Blank</code> | [Blank](../shapes/Blank.md) | No | — | pattern: <code>&quot;^\\s+$&quot;</code> |  |

### Request body

```json
{
  "labels": "<list>",
  "limits": "<map>",
  "huge": "<bigInteger>",
  "tight": "<bigDecimal>",
  "essay": "<string>",
  "archive": "<blob>",
  "blanks": "<list>",
  "bell": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>labels</code> | <code>body: labels</code> | [ManyLabels](../shapes/ManyLabels.md) | No | — | length: <code>{&quot;max&quot;:null,&quot;min&quot;:150}</code> |  |
| <code>limits</code> | <code>body: limits</code> | [ManyLimits](../shapes/ManyLimits.md) | No | — | length: <code>{&quot;max&quot;:null,&quot;min&quot;:150}</code> |  |
| <code>huge</code> | <code>body: huge</code> | [Huge](../shapes/Huge.md) | No | — | range: <code>{&quot;max&quot;:null,&quot;min&quot;:9223372036854775808}</code> |  |
| <code>tight</code> | <code>body: tight</code> | [Tight](../shapes/Tight.md) | No | — | range: <code>{&quot;max&quot;:0.29999999999999999999,&quot;min&quot;:null}</code> |  |
| <code>essay</code> | <code>body: essay</code> | [Essay](../shapes/Essay.md) | No | — | length: <code>{&quot;max&quot;:null,&quot;min&quot;:2000}</code> |  |
| <code>archive</code> | <code>body: archive</code> | [Archive](../shapes/Archive.md) | No | — | length: <code>{&quot;max&quot;:null,&quot;min&quot;:2000}</code> |  |
| <code>blanks</code> | <code>body: blanks</code> | [Blanks](../shapes/Blanks.md) | No | — | length: <code>{&quot;max&quot;:3,&quot;min&quot;:2}</code>; Unique items |  |
| <code>bell</code> | <code>body: bell</code> | [Bell](../shapes/Bell.md) | No | — | pattern: <code>&quot;^[\\x01]$&quot;</code> |  |

## Response syntax

```http
HTTP/1.1 200
```

No modeled body.

## Errors

No modeled errors.

## Idempotency and retries

Readonly: false. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Placeholders that do not meet their constraints: <code>input.archive</code>, <code>input.essay</code>, <code>input.headerBell</code>, <code>input.headerBlank</code>, <code>input.labels</code>, <code>input.limits</code>.

Request:

```http
POST /bulk HTTP/1.1
Content-Type: application/json
X-Bell: headerBell
X-Blank: headerBlank

{
  "archive": "YXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcmNoaXZlYXJjaGl2ZWFyY2hpdmVhcg==",
  "bell": "\u0001",
  "blanks": [
    "      ",
    "       "
  ],
  "essay": "essayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessayessa",
  "huge": 9223372036854775808,
  "labels": [
    "labels",
    "labels1",
    "labels2",
    "labels3",
    "labels4",
    "labels5",
    "labels6",
    "labels7",
    "labels8",
    "labels9",
    "labels10",
    "labels11",
    "labels12",
    "labels13",
    "labels14",
    "labels15",
    "labels16",
    "labels17",
    "labels18",
    "labels19",
    "labels20",
    "labels21",
    "labels22",
    "labels23",
    "labels24",
    "labels25",
    "labels26",
    "labels27",
    "labels28",
    "labels29",
    "labels30",
    "labels31",
    "labels32",
    "labels33",
    "labels34",
    "labels35",
    "labels36",
    "labels37",
    "labels38",
    "labels39",
    "labels40",
    "labels41",
    "labels42",
    "labels43",
    "labels44",
    "labels45",
    "labels46",
    "labels47",
    "labels48",
    "labels49",
    "labels50",
    "labels51",
    "labels52",
    "labels53",
    "labels54",
    "labels55",
    "labels56",
    "labels57",
    "labels58",
    "labels59",
    "labels60",
    "labels61",
    "labels62",
    "labels63",
    "labels64",
    "labels65",
    "labels66",
    "labels67",
    "labels68",
    "labels69",
    "labels70",
    "labels71",
    "labels72",
    "labels73",
    "labels74",
    "labels75",
    "labels76",
    "labels77",
    "labels78",
    "labels79",
    "labels80",
    "labels81",
    "labels82",
    "labels83",
    "labels84",
    "labels85",
    "labels86",
    "labels87",
    "labels88",
    "labels89",
    "labels90",
    "labels91",
    "labels92",
    "labels93",
    "labels94",
    "labels95",
    "labels96",
    "labels97",
    "labels98",
    "labels99"
  ],
  "limits": {
    "key": 1024,
    "key1": 1025,
    "key10": 1034,
    "key11": 1035,
    "key12": 1036,
    "key13": 1037,
    "key14": 1038,
    "key15": 1039,
    "key16": 1040,
    "key17": 1041,
    "key18": 1042,
    "key19": 1043,
    "key2": 1026,
    "key20": 1044,
    "key21": 1045,
    "key22": 1046,
    "key23": 1047,
    "key24": 1048,
    "key25": 1049,
    "key26": 1050,
    "key27": 1051,
    "key28": 1052,
    "key29": 1053,
    "key3": 1027,
    "key30": 1054,
    "key31": 1055,
    "key32": 1056,
    "key33": 1057,
    "key34": 1058,
    "key35": 1059,
    "key36": 1060,
    "key37": 1061,
    "key38": 1062,
    "key39": 1063,
    "key4": 1028,
    "key40": 1064,
    "key41": 1065,
    "key42": 1066,
    "key43": 1067,
    "key44": 1068,
    "key45": 1069,
    "key46": 1070,
    "key47": 1071,
    "key48": 1072,
    "key49": 1073,
    "key5": 1029,
    "key50": 1074,
    "key51": 1075,
    "key52": 1076,
    "key53": 1077,
    "key54": 1078,
    "key55": 1079,
    "key56": 1080,
    "key57": 1081,
    "key58": 1082,
    "key59": 1083,
    "key6": 1030,
    "key60": 1084,
    "key61": 1085,
    "key62": 1086,
    "key63": 1087,
    "key64": 1088,
    "key65": 1089,
    "key66": 1090,
    "key67": 1091,
    "key68": 1092,
    "key69": 1093,
    "key7": 1031,
    "key70": 1094,
    "key71": 1095,
    "key72": 1096,
    "key73": 1097,
    "key74": 1098,
    "key75": 1099,
    "key76": 1100,
    "key77": 1101,
    "key78": 1102,
    "key79": 1103,
    "key8": 1032,
    "key80": 1104,
    "key81": 1105,
    "key82": 1106,
    "key83": 1107,
    "key84": 1108,
    "key85": 1109,
    "key86": 1110,
    "key87": 1111,
    "key88": 1112,
    "key89": 1113,
    "key9": 1033,
    "key90": 1114,
    "key91": 1115,
    "key92": 1116,
    "key93": 1117,
    "key94": 1118,
    "key95": 1119,
    "key96": 1120,
    "key97": 1121,
    "key98": 1122,
    "key99": 1123
  },
  "tight": 0.29999999999999999999
}
```

Response:

```http
HTTP/1.1 200
```
