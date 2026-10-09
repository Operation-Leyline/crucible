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
  between two letters or digits (`date-time`). Keywords are contextual:
  `operation`, `input` and the rest are ordinary identifiers outside the
  position where they introduce something.
- **Strings.** Double-quoted with `\"`, `\\`, `\n`, `\t`, `\r` and
  `\u{XXXX}` escapes.
- **Numbers.** Optional `-`, digits, optional fraction and exponent. `1..5`
  is two integers around `..`, never a float.
- **Punctuation.** `{ } ( ) [ ] , ; : ? = . .. -> <- @ $` and the
  comparison operators `== != < <= > >=`.

## Grammar

EBNF. `{ x }` is zero or more, `[ x ]` optional, `|` alternation. `;` and
`,` are accepted between any two list elements and between body entries
but are never required.

```ebnf
file        = { item } ;
item        = [ doc ] { annotation } { modifier } declaration ;
modifier    = "readonly" | "idempotent" | "public" | "strict" | "internal" | "sensitive" ;

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
resource_entry = [ doc ] { annotation } ( "id" | "property" ) ident ":" type_ref ;

operation   = "operation" ident [ role ident [ "collection" ] ] "{" { op_entry } "}" ;
role        = "creates" | "puts" | "reads" | "updates" | "deletes" | "lists" | "on" ;
op_entry    = http | clause_call | body | errors | example ;
http        = "http" ident string [ "->" number ] ;
clause_call = ident "(" args ")" ;                         (* paginated(..) *)
body        = { modifier } ( "input" | "output" ) "{" { member } "}" ;
errors      = "errors" "[" ident { "," ident } "]" ;
example     = [ doc ] "example" string "{" { example_part } "}" ;
example_part = ( "input" | "output" ) value | "error" ident value ;

structure   = "structure" ident [ "for" ident ] "{" { member } "}" ;
union       = "union" ident "{" { member } "}" ;
error       = "error" ident "{" { error_entry } "}" ;
error_entry = "status" number | "retryable" [ "throttling" ] | member ;

member      = [ doc ] { annotation } ( include | elided | field_member ) ;
include     = "include" ident ;
elided      = "$" ident [ "as" ident ] [ "?" ] ;
field_member = ident [ "?" ] [ ":" type_ref ] [ "=" value ] ;   (* no type: unit union arm *)

type_ref    = ident [ constraints ] ;
constraints = "{" { constraint } "}" ;
constraint  = ident [ range | value ] ;                   (* length 1..100, pattern "..", unique *)
range       = [ number ] ".." [ number ] ;

scalar      = scalar_kind ident [ constraints ] ;
scalar_kind = "string" | "boolean" | "byte" | "short" | "integer" | "long"
            | "float" | "double" | "bigInteger" | "bigDecimal"
            | "timestamp" | "blob" | "document" ;
list        = "list" ident ":" ident [ constraints ] ;   (* constraints are the list's *)
map         = "map" ident ":" ident "->" ident [ constraints ] ;
enum        = "enum" ident [ ":" ident ] "{" { enum_value } "}" ;
enum_value  = [ doc ] { annotation } ident [ "=" ( string | number ) ] ;

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
            | path [ operator operand ] ;                  (* bare path: a flag *)
operator    = "==" | "!=" | "<" | "<=" | ">" | ">=" | "in" | "is"
            | "startsWith" | "endsWith" | "matches" | "contains" ;
operand     = value | ident { ident } ;                    (* is inline primitive *)
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

## Semantics, briefly

The meaning of each construct is in [decisions.md](decisions.md): members
are required unless marked `?` (20), inputs and outputs are anonymous (25),
resources and roles (31-34a), protocols and families (35-35d), metadata (43),
rules, templates and exceptions (45-47f), migrations (51-59).
