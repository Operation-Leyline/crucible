# Crucible language reference

Working reference for the internal modelling language. It describes what
`crucible-lang` parses today; [decisions.md](decisions.md) says why. The
language is internal, so this changes as implementation teaches.

## Files and packages

A package is a directory with a `crucible.toml` whose `[model]` table names
the namespace, the language version and the source directories. Every
`.cru` file under those directories belongs to the package's single
namespace. File and declaration order never matter.

## Lexical structure

- **Whitespace and newlines** separate tokens and carry no meaning.
- **Comments.** `//` runs to the end of the line and is ignored. `///` is
  documentation: consecutive `///` lines are Markdown attached to the next
  declaration, member, enum value, rule, step or example.
- **Identifiers.** A letter or `_`, then letters, digits, `_`, or `-`
  between two letters or digits (`date-time`). The name of a declaration,
  member or URI label cannot contain `-`. Keywords are contextual:
  `operation`, `input` and the rest are ordinary identifiers outside the
  position where they introduce something.
- **Strings.** Double-quoted with `\"`, `\\`, `\n`, `\t`, `\r` and
  `\u{XXXX}` escapes (hex digits only).
- **Byte-order mark.** A leading BOM is whitespace.
- **Numbers.** Optional `-`, digits, optional fraction and exponent. `1..5`
  is two integers around `..`, never a float.
- **Punctuation.** `{ } ( ) [ ] , ; : ? = . .. -> <- @ $ *` and the
  comparison operators `== != < <= > >=`.

## Grammar

EBNF. `{ x }` is zero or more, `[ x ]` optional, `|` alternation. `;` and
`,` are accepted between any two list elements and between body entries
but are never required.

```ebnf
file        = { item } ;
item        = [ doc ] { annotation } { modifier } declaration ;
modifier    = "readonly" | "idempotent" | "public" | "strict" | "internal" | "sensitive" ;
                                                           (* each position checks its own *)

declaration = service | resource | operation | structure | union | enum
            | error | scalar | list | map | metadata | rule | template
            | except | migration | protocol | conversion ;

annotation  = "@" ident [ "(" [ arg { "," arg } [ "," ] ] ")" ] ;
arg         = [ ( ident ":" ) | ( string "=" ) ] value ;
value       = string | number | ident | "[" [ value { "," value } ] "]"
            | "{" [ field { "," field } ] "}" ;
field       = ( ident | string ) ":" value ;

service     = "service" ident "{" { clause } "}" ;
protocol    = "protocol" ident "{" { clause } "}" ;
clause      = ident ( "(" args ")" | "{" { clause } "}" | value ) ;

resource    = "resource" ident [ "in" ident ] "{" { resource_entry } "}" ;
resource_entry = [ doc ] { annotation } { entry_modifier } ( "id" | "property" )
                 ident [ "?" ] ":" type_ref [ "as" ident ] ;
                 (* `?` on a property, `as` on an identifier: its name in views *)
entry_modifier = "readonly" | "immutable" | "writeonly" ;
                 (* properties only; `immutable writeonly` is the one combination *)

operation   = "operation" ident [ role ident [ "collection" ] ] "{" { op_entry } "}" ;
role        = ident ;          (* creates | puts | reads | updates | deletes | lists | on;
                                  lowering reports any other word; collection follows on *)
op_entry    = http | clause_call | body | errors | example ;
http        = "http" ident string [ "->" number ] ;
clause_call = ident "(" args ")" ;                         (* paginated(..) *)
body        = [ doc ] { modifier } ( "input" | "output" ) "{" { member } "}" ;
errors      = "errors" "[" ident { "," ident } "]" ;
example     = [ doc ] "example" string "{" { example_part } "}" ;
example_part = ( "input" | "output" ) value | "error" ident value ;

structure   = "structure" ident [ "for" ident ] "{" { member } "}" ;
union       = "union" ident "{" { member } "}" ;
error       = "error" ident "{" { error_entry } "}" ;
error_entry = "status" number | "retryable" [ "throttling" ] | member ;

member      = [ doc ] { annotation } { modifier } ( include | all | elided | field_member ) ;
                                                           (* modifiers: internal, sensitive *)
include     = "include" ident ;
all         = "$" "*" ;          (* in a `for` view only *)
elided      = "$" ident [ "as" ident ] [ "?" ] ;
field_member = ident [ "?" ] [ ":" type_ref ] [ "=" value ] ;   (* no type: unit union arm *)

type_ref    = ident [ constraints ] ;
constraints = "{" { constraint } "}" ;
constraint  = ident [ range | value ] ;   (* length 1..100, range 0.., pattern "..", unique,
                                           format date-time, mediaType "image/png" *)
range       = [ number ] ".." [ number ] ;

scalar      = scalar_kind ident [ constraints ] ;
scalar_kind = "string" | "boolean" | "byte" | "short" | "integer" | "long"
            | "float" | "double" | "bigInteger" | "bigDecimal"
            | "timestamp" | "blob" | "document" ;
list        = "list" ident ":" ident [ constraints ] ;   (* constraints are the list's *)
map         = "map" ident ":" ident "->" ident [ constraints ] ;
enum        = "enum" ident [ ":" ident ] "{" { enum_value } "}" ;
enum_value  = [ doc ] { annotation } { modifier } ident [ "=" value ] ;
                         (* modifier: internal; value: a string, or a number in an integer enum *)

metadata    = "metadata" ident "on" target { "," target } [ ":" meta_type ]
              [ "conflicts" ident { "," ident } ]
              [ "retention" ( "source" | "runtime" ) ] ;   (* default runtime *)
target      = ident [ ident ] ;                            (* operation | enum value *)
meta_type   = "enum" "{" { enum_value } "}" | type_ref ;

template    = "template" ident "{" { op_entry } "}" ;

rule        = "rule" ident { ident } "{" { rule_clause } "}" ;   (* strict, exceptable, advice *)
rule_clause = "every" kind [ "where" expr ] { "must" expr }
            | "no" kind [ "where" expr ] ;
kind        = ident { ident } ;                            (* input member *)
expr        = and_expr { "or" and_expr } ;
and_expr    = not_expr { "and" not_expr } ;
not_expr    = "not" not_expr | atom ;
atom        = "(" expr ")"
            | "has" "one" "of" ref { "," ref }
            | "has" ref
            | "addresses" ident
            | path [ "is" ident { ident } | operator value ] ; (* bare path: a flag;
                                                              is inline primitive *)
operator    = "==" | "!=" | "<" | "<=" | ">" | ">=" | "in"
            | "startsWith" | "endsWith" | "matches" | "contains" ;
ref         = "@" ident | ident ;
path        = ref { "." ident } ;

except      = "except" ident "for" ident "because" string ;

migration   = "migration" string "from" string "{" { step } "}" ;
step        = [ doc ] ( rename | require | add | move | reshape | remove
                      | behavior | sunset | retype ) ;
rename      = "rename" dotted "->" ident ;
require     = "require" dotted ( "default" value | hook ) ;
add         = "add" dotted [ ident ] "downgrade" ident [ "{" { ident "<-" ident } "}" ] ;
move        = "move" ident "from" ident string ;
reshape     = "reshape" dotted [ "{" ident { "," ident } "}" ] "->" ident hook ;
remove      = "remove" "operation" ident ;
behavior    = "behavior" dotted ;
sunset      = "sunset" string ;
retype      = "retype" dotted "->" ident [ hook ] ;
hook        = "by" [ "async" ] "hook" ;
dotted      = ident { "." ident } ;

conversion  = "conversion" ident "->" ident hook ;
```

## Checks

Beyond the grammar, the compiler refuses a model that breaks the rules in
[decisions.md](decisions.md); one run reports every problem. The HTTP
rules of decision 35c (labels, query and header targets, header names,
bodiless responses, one payload, unique routes) and the checks on
pagination members, examples and name collisions run on the IR in
`crucible_smithy::validate`, shared with the Smithy front end. The
language's own checks include:

- a binding appears only where its part of the message exists (`@query`
  in an input; `@header` and `@payload` in an input, output or error), and
  a member has one binding;
- `@token` marks at most one input member, an optional string;
- map keys are strings or string enums;
- each constraint applies to its type and is given once, and a `pattern`
  must be runnable;
- defaults fit the member's type and constraints; an enum default names a
  value's symbol;
- enum value names, and integer enum values, are unique;
- a metadata declaration is checked once (targets, conflicts, retention,
  value type, no built-in annotation's name) and each use's value against
  its type;
- resources: the parent is a resource with no cycle, identifier names are
  unique across the chain, a property's name differs from every identifier
  the resource has (inherited ones too) and from its other properties,
  identifiers are strings or string enums and take no keyword or `?`, only
  `readonly`, `immutable`, `writeonly` and `immutable writeonly` apply to a
  property, and an identifier's `as` spelling is unique among the names the
  resource's own views show;
- `$name` names one of the resource's own identifiers or properties, or an
  inherited identifier; an ancestor's properties are not inherited;
- `$*` appears once, only in a `for` view, and an entry it includes is
  named again only as `$name?` after it, which loosens it for that view;
- property mutability (decision 34a): an `updates` input does not bind a
  `readonly` or `immutable` property, a `creates` or `puts` input does not
  bind a `readonly` one, some `creates` or `puts` input binds every
  `immutable` property, a view or output never shows a `writeonly` one,
  and `@mutability` on a member that binds a property agrees with its
  keyword. Bindings under `@query` and the inputs of `reads`, `lists`,
  `deletes` and `on` operations are not writes;
- roles: each lifecycle role is held once per resource, instance roles
  bind every identifier and collection roles only the parent's, and a
  role's implied keyword is not contradicted;
- service, operation, error and example clauses, list entries, object
  keys and annotations are given once; a service declares its version.

Warnings never withhold the IR. The compiler warns about a plain member
whose name is a resource identifier or property ("did you mean
`$name`?", decision 33), a property that is neither `readonly` nor
`immutable` and that no `updates`, `puts` or `on` input binds (declare it
`immutable`, or `readonly` when nothing writes it; decision 34a), a
resource with `reads`, `lists` or `creates` operations whose outputs show
none of its properties, a structure without `for` that restates a
resource's identifier and a property (decision 34h), an operation attached
to a resource the service does not reach, a member constraint that loosens its type's (it replaces
it, so `length ..40` on a `1..100` type drops the minimum), a `[Name]`
doc link that names nothing (decision 41), and a `///` comment that
documents nothing.

## Semantics, briefly

The meaning of each construct is in [decisions.md](decisions.md): members
are required unless marked `?` (20), inputs and outputs are anonymous (25),
resources, roles and representations (31-34i), protocols and families
(35-35d), metadata (43), rules, templates and exceptions (45-47f),
migrations (51-59).

## In the editor

`crucible-lsp` explains each keyword, built-in type, constraint and
built-in annotation on hover from one table, `KEYWORDS` in
`crates/crucible-lsp/src/hover.rs`, written from this reference and
[decisions.md](decisions.md). Change it with them: its test parses every
example in it and hovers the word it explains.
