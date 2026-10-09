# IDL-first API/schema modelling languages: Smithy, TypeSpec, Fern, Thrift, Cap'n Proto, FlatBuffers, Avro IDL, AsyncAPI

Research date: 2026-10-08. Sources are primary (official specs, docs sites, repo source files) unless noted. Where a doc site page could not be fetched cleanly, the repo source of that page was read instead and is cited as such. Versions: Smithy IDL 2.0 (spec at smithy.io/2.0; 1.0 contrasted where it differs), TypeSpec 1.x (1.0 GA May 2025), Fern Definition (current docs, format now "not recommended for new customers"), Thrift 0.26 IDL page, Cap'n Proto current language page, FlatBuffers current flatbuffers.dev, Avro 1.12.0 IDL, AsyncAPI 3.0.0 (with notes from the 3.1.0 draft in the spec repo).

## Nullability: required/optional defaults, marker syntax, default values

### Takeaway
The eight languages split into two camps on defaults: *required-by-default with an optional marker* (TypeSpec `?`, Fern `optional<T>`, Avro union-with-null / `T?`, AsyncAPI JSON Schema `required` list is the exception: optional by default) versus *optional-by-default with a required marker* (Smithy `@required`, Thrift `required`, FlatBuffers `(required)`). Cap'n Proto refuses the concept entirely, on the record, as a lesson from Protobuf's `required`. Smithy 2.0 is the most elaborate: optionality is computed from `@required`, `@default`, `@clientOptional`, `@input` and `@addedDefault`, and differs for "authoritative" (server) vs "non-authoritative" (client) consumers.

### Cited Findings

**Smithy (IDL 2.0)**
- "Structure members in Smithy are automatically considered optional." — [Smithy type refinement traits](https://smithy.io/2.0/spec/type-refinement-traits.html)
- `@required`: "Marks a structure member as required, meaning a value for the member MUST be present." "It applies to all shapes, including inputs of operations, outputs of operations, and errors." — [Smithy type refinement traits](https://smithy.io/2.0/spec/type-refinement-traits.html)
- Default sugar: `normative: Boolean = true` is equivalent to `@default(true)` on the member ("Syntactic sugar can be used to apply the default trait to a structure member."). — [Smithy IDL spec](https://smithy.io/2.0/spec/idl.html)
- Structure example from the spec: `structure MyStructure { foo: String  @required baz: Integer  greeting: String = "Hello" }` — [Smithy aggregate types](https://smithy.io/2.0/spec/aggregate-types.html)
- `@default` restrictions: "list: can only be set to an empty list", "map: can only be set to an empty map", "structure: no default value", "union: no default value". `@default(null)` "can be set to `null` to explicitly indicate that the member has no default value." Rationale against defaults in patch APIs: "it is impossible for a server to differentiate between whether a property was set to its default value or if a property was omitted." — [Smithy type refinement traits](https://smithy.io/2.0/spec/type-refinement-traits.html)
- `@clientOptional`: "Requires that non-authoritative generators like clients treat a structure member as optional"; rationale: "For cases when a service is unsure if a member will be required forever". `@addedDefault`: "Indicates that the default trait was added to a structure member after initially publishing the member", so "tooling [can] decide whether to ignore the `@default` trait if it will break backward compatibility in the tool." — [Smithy type refinement traits](https://smithy.io/2.0/spec/type-refinement-traits.html)
- `@input`: "Required members of a structure marked with the `@input` trait are implicitly considered clientOptional." — [Smithy type refinement traits](https://smithy.io/2.0/spec/type-refinement-traits.html)
- Optionality table: `@required`/`@default` members are "Present unless also `@clientOptional` or part of an `@input` structure" for non-authoritative consumers; for `@input` structures "All members are optional regardless of the `@required` or `@default` trait" (non-authoritative); "Authoritative model consumers like servers MAY choose to determine optionality using more restrictive rules" (ignoring `@input` and `@clientOptional`). — [Smithy aggregate types](https://smithy.io/2.0/spec/aggregate-types.html)
- Compatibility rules: "New members MUST NOT be marked with the required trait."; "The addedDefault trait SHOULD be used any time a default trait is added to a previously published member."; "Replacing both the `required` and `clientOptional` trait with the `default` trait is _not_ a backward compatible change"; for `@input` structures "Transitioning top-level members from `required` to optional is allowed". — [Smithy aggregate types](https://smithy.io/2.0/spec/aggregate-types.html)
- `@sparse`: "Indicates that lists and maps MAY contain `null` values." — [Smithy type refinement traits](https://smithy.io/2.0/spec/type-refinement-traits.html)
- **1.0 contrast**: in 1.0 nullability was driven by `@box`: "Boolean, byte, short, integer, long, float, and double shapes are only considered boxed if they are marked with the box trait." "All other shapes are always considered boxed." `@box` on a member means "the member may or may not contain a value, and the member has no default value." — [Smithy 1.0 type refinement traits](https://smithy.io/1.0/spec/core/type-refinement-traits.html)
- Migration 1→2: `@box` is removed; primitive root shapes that were unboxed need `@default(false)`/`@default(0)` and members targeting them repeat it (`= false`, `= 0`); a formerly boxed member becomes `= null` (`@default(null)`); a non-required member targeting a `@streaming` blob needs `= ""`. — [Smithy 1.0→2.0 migration guide (RST source)](https://smithy.io/2.0/_sources/guides/model-translations/migrating-idl-1-to-2.rst.txt)
- Stated rationale (AWS announcement, Aug 2022): IDL 2.0 "improves developer productivity by reducing the amount of nullable properties in generated code", and adds "custom default values". — [AWS What's New: Smithy IDL 2.0 GA](https://aws.amazon.com/about-aws/whats-new/2022/08/smithy-interface-definition-language-idl-2-0-generally-available/)

**TypeSpec**
- "Properties can be designated as optional by using the `?` symbol." Example: `model Dog { address?: string; }`. — [TypeSpec Models (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/language-basics/models.md)
- "Properties can be assigned a default value using the `=` operator." Example: `address?: string = "wild"; age: uint8 = 0;` — defaults shown on both optional and required properties. — [TypeSpec Models (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/language-basics/models.md)
- The REST library's update templates carry `@patch(#{ implicitOptionality: true }) // for legacy behavior`, i.e. PATCH bodies are no longer implicitly optional in current TypeSpec unless opted in. — [TypeSpec packages/rest/lib/resource.tsp](https://raw.githubusercontent.com/microsoft/typespec/main/packages/rest/lib/resource.tsp)

**Fern**
- Type table: "Optional value, e.g., `optional<string>`"; example `line2: optional<string>`. — [Fern Definition: Types](https://buildwithfern.com/learn/api-definitions/ferndef/types)
- Query parameter example: `limit: optional<integer>`. — [Fern Definition: Endpoints](https://buildwithfern.com/learn/api-definitions/ferndef/endpoints)
- `nullable<T>` vs `optional<T>` (Java generator changelog, 2025-08-25): with the `use-nullable-annotation` flag, "`nullable<T>` generates as the raw type T with a `@Nullable` annotation, while `optional<T>` remains as `Optional<T>`", described as "proper API semantics for fields that can be null vs those that may be absent from requests." — [Fern Java generator changelog](https://buildwithfern.com/learn/sdks/generators/java/changelog/2025/8/25)

**Apache Thrift**
- Grammar: `Field ::= FieldID? FieldReq? FieldType Identifier ('=' ConstValue)? XsdFieldOptions ListSeparator?`, `FieldReq ::= 'required' | 'optional'`. Three-state requiredness: default requiredness applies "implicitly if neither _required_ nor _optional_ are given." "Required fields are always written and are expected to be set." "Optional fields are only written when they are set". On defaults: "the current status quo is more or less that default fields are typically set at initialization time." — [Thrift IDL](https://thrift.apache.org/docs/idl)

**Cap'n Proto**
- FAQ "How do I make a field 'required'?": "You don't. You may find this surprising, but the 'required' keyword in Protocol Buffers turned out to be a horrible mistake." "A field declared required, unfortunately, is required everywhere." "The right answer is for applications to do validation as-needed in application-level code." "Cap'n Proto doesn't have any parsing step during which to check for required fields." — [Cap'n Proto FAQ](https://capnproto.org/faq.html)
- Optional: "Cap'n Proto has no notion of 'optional' fields." "A better approach may be to give the field a bogus default value and interpret that value to mean 'not present'." — [Cap'n Proto FAQ](https://capnproto.org/faq.html)
- Defaults: `foo @0 :Int32 = 123;`, `grault @5 :Data = 0x"a1 40 33";`; unions default to lowest-numbered member and a `Void` field named `unset` is the idiom to avoid a real default. — [Cap'n Proto language](https://capnproto.org/language.html)

**FlatBuffers**
- Table field form `name:type [= scalar] (attributes);`. "If the default value is not specified in the schema, it will be `0` for scalar types, or `null` for other types." Only scalars take explicit defaults. "You generally do not want to change default values after they're initially defined." — [FlatBuffers schema](https://flatbuffers.dev/schema/)
- Optional scalars: `hp:short = null;` "For optional scalars, just set the field default value to `null`." Rationale: defaults are not written to the buffer, so present-equal-to-default and absent are indistinguishable; "`null` is a value that's outside of all types, so we will always write if `add_field` is called." — [FlatBuffers schema](https://flatbuffers.dev/schema/)
- Required: `hp:short (required)` makes the verifier reject buffers missing it; "You cannot have `required` set with an explicit default value, it will result in a compiler error." Structs: "Structs may only contain scalars or other structs"; struct fields are required with no defaults. — [FlatBuffers schema](https://flatbuffers.dev/schema/)

**Avro IDL (1.12.0)**
- Fields are required unless typed as a union with null. Explicit: `union { null, string } optionalString1 = null;` Shorthand: `string? optionalString2 = null;` The null position in the union follows the default value, and the default must match the first union branch. — [Avro IDL 1.12.0](https://avro.apache.org/docs/1.12.0/idl-language/)
- Defaults: `=` after the field name with a JSON-style value, e.g. `boolean active = true;`, `int result_code = -1;`. — [Avro IDL 1.12.0](https://avro.apache.org/docs/1.12.0/idl-language/)
- The `?` shorthand originated as AVRO-3257 ("add Kotlin style optional types like this: `string? optionalName = null;`"); a packaging request citing the upstream 1.11.1 release notes lists "add IDL syntax to create optional fields" (secondary source). — [AVRO-3257](https://issues.apache.org/jira/browse/AVRO-3257); [openSUSE packaging request 993435](https://build.openbuildservice.org/request/show/993435)

**AsyncAPI 3.0**
- Payloads are Schema Objects, "a superset of the JSON Schema Specification Draft 07"; required fields are listed in a `required` array (`required: [name]`), so properties are optional unless listed. `default` differs from JSON Schema: "the value MUST conform to the defined type for the Schema Object defined at the same level." — [AsyncAPI 3.0.0 spec](https://www.asyncapi.com/docs/reference/specification/v3.0.0); [AsyncAPI spec source](https://github.com/asyncapi/spec/blob/master/spec/asyncapi.md)

### Inferences
- Smithy's "authoritative vs non-authoritative" split is unique among the eight: the same member can be non-null in generated server types and nullable in generated client types. This is the design response to the "@required is forever" problem Cap'n Proto solves by having no required at all.
- FlatBuffers' `= null` and Avro's `T?` both reuse the default-value slot to express optionality, which keeps the grammar small but overloads one syntax.
- Thrift's three-state (default/required/optional) requiredness is the only one with an unnamed middle state; its own docs describe default-requiredness behaviour as a "status quo", not a specification.

### Gaps
- Could not retrieve the Fern types page text on `nullable<T>` itself (only the Java generator changelog documents the distinction); the exact definition page for `nullable<T>` in the Fern Definition docs was not found.
- TypeSpec docs fetched do not state "required by default" in those words; it is implied by `?` being the optional marker.
- Thrift's "default" requiredness has no normative wire semantics in the IDL page; the page itself says it is "more or less" a status quo.

## Operation inputs/outputs: named messages vs anonymous parameter lists, and generated names

### Takeaway
Smithy and Fern require (or strongly steer toward) a dedicated named structure per operation input/output, with the name derived from the operation (`GetUserInput` via the `$operationInputSuffix` control) or declared in the YAML (`request: name: CreateUserRequest`). TypeSpec, Thrift, Cap'n Proto, Avro and FlatBuffers RPC take parameter lists; TypeSpec treats the list as an anonymous model whose naming is left to emitters (and Azure's linter flags unnamed types). AsyncAPI has no operations with inputs/outputs in the RPC sense: an operation is `send`/`receive` of named messages on a channel.

### Cited Findings

**Smithy**
- Inline syntax: `input := { userId: String }`, `output := { ... }`. "A structure defined using inline syntax is automatically marked with the input trait for inputs" (and output trait for outputs). "For inputs, the generated name is the name of the operation shape with the suffix `Input` added." (`GetUser` → `GetUserInput`, outputs get `Output`). Control statements `$operationInputSuffix: "Request"` and `$operationOutputSuffix: "Response"` change the suffix. — [Smithy IDL spec](https://smithy.io/2.0/spec/idl.html)
- "`input` and `output` must target structures. Each should be a dedicated structure marked `@input` or `@output`." Omitted input or output defaults to `smithy.api#Unit`; the spec warns this prevents adding members later. `@input`: "Specializes a structure for use only as the input of a single operation"; `@output`: "Specializes a structure for use only as the output of a single operation." — [Smithy service types](https://smithy.io/2.0/spec/service-types.html); [Smithy type refinement traits](https://smithy.io/2.0/spec/type-refinement-traits.html)
- Inline input with mixins: `input := with [UserIdentifiers] {}`; the migration guide says inline I/O "improves readability and reduces the amount of boilerplate." — [Smithy 1.0→2.0 migration guide](https://smithy.io/2.0/_sources/guides/model-translations/migrating-idl-1-to-2.rst.txt)

**TypeSpec**
- "You can declare operations using the `op` keyword." `op ping(): void;` "The parameters of an operation represent a model." Spread into parameters: `op feedDog(...CommonParams, name: string): void;` Reuse: "You can reuse operation signatures with the `is` keyword." `op deletePet is Delete;` Templates: `op ReadResource<T>(id: string): T;` then `op readPet is ReadResource<Pet>;` — [TypeSpec Operations (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/language-basics/operations.md)
- Interfaces group operations: `interface SampleInterface { foo(): int32; bar(): string; }`, `interface C extends A, B { c(): string; }`, `interface ReadWrite<T> { read(): T; write(t: T): void; }` — [TypeSpec Interfaces (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/language-basics/interfaces.md)
- Naming of anonymous parameter/response models is an emitter concern: the compiler's `getEffectiveModelType` helper exists so an emitter can "find a better name for a set of properties after filtering". Azure's client-generator linter rule `no-unnamed-types` says anonymous types "receive a generated name in emitted SDKs" and recommends declaring a named model; the Java emitter's `empty-name` diagnostic likewise asks for an explicit model name for anonymous request/response shapes. — [getEffectiveModelType](https://typespec.io/docs/standard-library/reference/js-api/functions/geteffectivemodeltype/); [TCGC no-unnamed-types](https://azure.github.io/typespec-azure/docs/libraries/typespec-client-generator-core/rules/no-unnamed-types/); [typespec-java empty-name](https://azure.github.io/typespec-azure/docs/emitters/clients/typespec-java/reference/diagnostics/empty-name/)
- `@friendlyName("{name}List", T)` on `model List<Item>` names template instances. — [TypeSpec built-in decorators (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/standard-library/built-in-decorators.md)

**Fern**
- Request as a type reference: `request: CreateMovieRequest`, `response: MovieId`; or inline with a required name: `request:` / `# this name is required for idiomatic SDKs` / `name: CreateUserRequest` / `body:` / `properties:` / `userName: string`. "If the request body is an object, you can **inline the type declaration**." Query parameters and headers also sit under `request:` with the same `name:`. `response: list<User>` or `response: type: User / status-code: 201`. "An endpoint has exactly one success response." — [Fern Definition: Endpoints](https://buildwithfern.com/learn/api-definitions/ferndef/endpoints); [Fern Definition: Overview](https://buildwithfern.com/learn/api-definitions/ferndef/overview)

**Thrift**
- `Function ::= 'oneway'? FunctionType Identifier '(' Field* ')' Throws? ListSeparator?`, `FunctionType ::= FieldType | 'void'`. Parameters are a field list (with field ids), i.e. an anonymous struct. — [Thrift IDL](https://thrift.apache.org/docs/idl)

**Cap'n Proto**
- Methods take named params and return named results: `create @1 (name :Text) -> (file :File);` with parameter defaults `read @1 (startAt :UInt64 = 0, ...)`. Interfaces support multiple inheritance via `extends(...)` and pass by reference ("a 'capability-based' RPC system"). Generic methods: `newAssignable @0 [T] (initialValue :T)`. — [Cap'n Proto language](https://capnproto.org/language.html)

**FlatBuffers**
- `rpc_service MonsterStorage { Store(Monster):StoreResponse; Retrieve(MonsterId):Monster; }` — request and response must be table types; gRPC support is "preliminary". — [FlatBuffers schema](https://flatbuffers.dev/schema/)

**Avro IDL**
- Messages (protocol files only): `int add(int foo, int bar = 0);`, `void logMessage(string message);`, `void goKaboom() throws Kaboom;`, `void fireAndForget(string message) oneway;` — [Avro IDL 1.12.0](https://avro.apache.org/docs/1.12.0/idl-language/)

**AsyncAPI 3.0**
- Operations reference a channel and declare direction: `operations: onUserSignedUp: action: receive  channel: $ref: '#/channels/userSignedUp'`. Channels hold `address` and a `messages` map. v3 split operations out of channels; v2 put `publish`/`subscribe` under each channel. v3 `action: send`/`receive` "describes what your application does, not what others do." — [AsyncAPI 3.0.0 spec](https://www.asyncapi.com/docs/reference/specification/v3.0.0); [Migrating to v3](https://www.asyncapi.com/docs/migration/migrating-to-v3)

### Inferences
- Smithy is the only language where the input/output *shape* is a first-class, separately-addressable, trait-bearing structure by rule (`@input`/`@output`), which is what makes its client-optional and HTTP-binding rules expressible on members.
- TypeSpec's "parameters are a model" plus spread gives Smithy-like reuse without naming, at the cost of pushing naming into emitters; the Azure linter's existence is evidence that generated names were a pain point.
- Fern's comment "this name is required for idiomatic SDKs" shows the same pressure resolved by making the author supply the name in-line.

### Gaps
- No official TypeSpec document was found that specifies the generated-name algorithm for anonymous parameter models (e.g. a `Request` suffix); only linter rules and emitter diagnostics were found.
- The Smithy IDL page does not state the generated name when `$operationInputSuffix` is combined with mixins/`for` resource binding beyond the plain suffix rule.

## Annotations: decorators/traits/attributes, keywords vs annotations, custom typed metadata

### Takeaway
Smithy (traits, `@foo(...)`), TypeSpec (decorators, `@foo(...)`, plus `@@foo(Target, ...)` for retroactive use) and Cap'n Proto (`$foo(...)` with `annotation foo(targets) :Type;`) all have *declared, typed, target-restricted* custom metadata. Thrift `(key = "value")`, FlatBuffers `attribute "name";` and AsyncAPI `x-` extensions are untyped string/any bags. Avro's `@name(json)` annotations become custom schema properties. Fern has no user-extensible annotation mechanism in the definition format (only fixed YAML keys).

### Cited Findings

**Smithy**
- Traits precede the shape: `@length(min: 1, max: 100)` / `@documentation("Contains a string")` / `string MyString`. Retroactive: `apply MyString @documentation("This is my string!")`, block form `apply MyString { @documentation(...) @length(min: 1, max: 10) }`, members `apply MyStructure$foo @documentation("...")`; applying outside the definition is "treated exactly the same as applying the trait inside of a shape definition." — [Smithy IDL spec](https://smithy.io/2.0/spec/idl.html)
- Defining traits: "Traits are defined by applying smithy.api#trait to a shape." Example `@trait(selector: "string", conflicts: [beta])`; `selector` is "A valid selector that defines where the trait can be applied."; `conflicts` "Defines the shape IDs of traits that MUST NOT be applied to the same shape"; `structurallyExclusive: "member"` means "only a single member of a structure can be marked with the trait." "The value provided for a trait MUST be compatible with the shape of the trait." "Only a single instance of a trait can be applied to a shape." — [Smithy model spec](https://smithy.io/2.0/spec/model.html)
- Trait definition using `@idRef`: `@trait @idRef(failWhenMissing: true, selector: "integer") string integerRef`. — [Smithy constraint traits](https://smithy.io/2.0/spec/constraint-traits.html)
- Prelude: "built-in traits defined in the Smithy prelude namespace, `smithy.api`, are automatically available". — [Smithy model spec](https://smithy.io/2.0/spec/model.html)

**TypeSpec**
- "Decorators are referenced using the `@` prefix and must be placed before the entity they are decorating." `@tag("Sample")`; "If no arguments are provided, the parentheses can be omitted," e.g. `@mark`. Augment decorators: `@@tag(Dog, "Sample");`, `@@visibility(Dog.name, Lifecycle.Read);` "As the augment decorator is a statement, it must end with a semicolon (`;`)." "Notably, aliases cannot be decorated, as they do not create new Types"; same for union `|` expressions and anonymous models. Rationale: "Decorators form the core of TypeSpec's extensibility, providing the flexibility to describe a wide variety of APIs and associated metadata." Auto decorator (no JS): `auto dec label(target: Model, value: valueof string);` — [TypeSpec Decorators (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/language-basics/decorators.md)
- Declaring: `extern dec logType(target: unknown, name: string);`, `extern dec track(target: Model | Enum);`, `extern dec tag(target: unknown, value: valueof string);`, optional `name?: valueof string`, rest `...names: valueof string[]`. JS: `export function $logType(context: DecoratorContext, target: Type, name: string)`; exporting via `$decorators` is "Recommended". Reason for the TypeSpec-side signature: "It enables type checking for the parameters", "It provides IDE IntelliSense". — [TypeSpec Create decorators (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/extending-typespec/create-decorators.md)
- Decorator values use `#{...}` object-value literals, e.g. `@service(#{title: "Pet store"})`, `@example(#{name: "Fluffy", age: 2})`. — [TypeSpec built-in decorators (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/standard-library/built-in-decorators.md)

**Fern**
- All metadata is fixed YAML keys (`docs:`, `validation:`, `availability:`, `http: statusCode:`); no user-defined annotation syntax appears in the types/endpoints/errors pages. — [Fern Definition: Types](https://buildwithfern.com/learn/api-definitions/ferndef/types); [Fern Definition: Endpoints](https://buildwithfern.com/learn/api-definitions/ferndef/endpoints)

**Thrift**
- Parenthesised key/value annotations after the annotated element (from the compiler's own test file): typedef `typedef list<i32> ( cpp.template = "std::list" ) int_linked_list`; field `1: i32 bar ( presence = "required" );`; struct `} (cpp.customostream)`; function `void Foo( ) ( deprecated = "This method has neither 'x' nor \"y\"" )`, bare key `void Deprecated() ( deprecated )`; enum value `SUNDAY ( weekend = "yes" ),`; service `} (a.b="c")`. — [apache/thrift test/AnnotationTest.thrift](https://github.com/apache/thrift/blob/master/test/AnnotationTest.thrift)
- The IDL grammar page does not define the annotation production; `xsd_all`/`xsd_optional`/`xsd_nillable` are noted as having no current purpose and "strongly discouraged". — [Thrift IDL](https://thrift.apache.org/docs/idl)

**Cap'n Proto**
- Declared as `annotation foo(struct, enum) :Text;` and applied as `$foo("bar")`. Targets include `file`, `struct`, `field`, `union`, `group`, `enum`, `method`, and `*`. Struct-typed and `Void` annotations are supported. — [Cap'n Proto language](https://capnproto.org/language.html)

**FlatBuffers**
- User-defined: `attribute "priority";` declares one, queryable at runtime; applied as `friendly:bool = false (deprecated, priority: 1);`. Built-ins: `id`, `deprecated`, `required`, `force_align`, `bit_flags`, `nested_flatbuffer`, `flexbuffer`, `key`, `hash`, `original_order`, `native_*`. — [FlatBuffers schema](https://flatbuffers.dev/schema/)

**Avro IDL**
- `@order("ascending")`, `@aliases([...])`, `@logicalType("timestamp-micros") long finishTime;`, `@namespace("...")`, `@java-class("java.util.ArrayList") array<string> myStrings;`. "Other annotations become custom properties." — [Avro IDL 1.12.0](https://avro.apache.org/docs/1.12.0/idl-language/)

**AsyncAPI**
- Specification Extensions: pattern `^x-[\w\d\.\x2d_]+$`, type `Any`; "The field name MUST begin with `x-`, for example, `x-internal-id`." "The value can be `null`, a primitive, an array or an object." Many objects "MAY be extended with Specification Extensions." — [AsyncAPI spec source](https://github.com/asyncapi/spec/blob/master/spec/asyncapi.md); [AsyncAPI 3.0.0 spec](https://www.asyncapi.com/docs/reference/specification/v3.0.0)

### Inferences
- Smithy's traits are the most self-describing: a trait *is* a shape in the model, so trait values are validated by the same type system as data, and `selector` constrains targets in a query language. TypeSpec achieves equivalent target-typing via `extern dec` signatures but the implementation lives in JS.
- Only Smithy (`apply`) and TypeSpec (`@@`) support retroactive annotation of shapes defined elsewhere, which is what lets protocol/HTTP or language-specific metadata live in separate files.

### Gaps
- Thrift: no normative spec of the annotation grammar was found on thrift.apache.org; the evidence is the compiler's test corpus.
- Avro: the IDL page does not document user-defined typed annotations beyond "custom properties".

## Constraints: length, range, pattern, uniqueness

### Takeaway
Smithy (`@length`, `@range`, `@pattern`, `@uniqueItems`), TypeSpec (`@minLength`/`@maxLength`, `@minValue`/`@maxValue`, `@pattern`, `@minItems`/`@maxItems`, `@format`), Fern (`validation:` block) and AsyncAPI (JSON Schema keywords) carry constraints in the model; Thrift, Cap'n Proto, FlatBuffers and Avro have none (Cap'n Proto by explicit policy: validation belongs in application code).

### Cited Findings
- Smithy: `@length(min: 1, max: 10) string MyString` (targets "Any list, map, string, or blob; or a member that targets one of these shapes"); `@range(min: 1, max: 10) integer OneToTen` ("A number or a member that targets a number"); `@pattern("^[A-Za-z]+$") string Alphabetic`; `@uniqueItems list MyList { member: String }` ("A list that does not transitively contain floats, doubles, or documents"). The `@enum` trait: "This trait is deprecated. An enum shape should be used instead." — [Smithy constraint traits](https://smithy.io/2.0/spec/constraint-traits.html)
- Smithy 1→2: `set` is replaced by `list` + `@uniqueItems`. — [Smithy migration guide](https://smithy.io/2.0/_sources/guides/model-translations/migrating-idl-1-to-2.rst.txt)
- TypeSpec: `@minLength(value: valueof integer)` / `@maxLength` target `string | ModelProperty` (`@minLength(2) scalar Username extends string;`); `@minValue`/`@maxValue(value: valueof numeric | utcDateTime | offsetDateTime | plainDate | plainTime | duration)`; `@pattern(pattern: valueof string, validationMessage?: valueof string)` targets `string | bytes | ModelProperty`; `@minItems`/`@maxItems` target `unknown[] | ModelProperty` (`@minItems(1) model Endpoints is string[];`); `@format(format: valueof string)` (`@format("uuid") scalar uuid extends string;`). — [TypeSpec built-in decorators (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/standard-library/built-in-decorators.md)
- Fern: `validation: minLength: 2 / maxLength: 100 / pattern: "^[A-Za-z ]+$"` on a string, `validation: min: 0 / max: 150` on an integer; "exist in your API definition and are enforced by the server, but the generated client SDKs don't include validation logic." Also `set<T>`: "An unordered collection with unique elements". — [Fern Definition: Types](https://buildwithfern.com/learn/api-definitions/ferndef/types)
- Thrift: the IDL grammar has no constraint production. — [Thrift IDL](https://thrift.apache.org/docs/idl)
- Cap'n Proto: "Low-level infrastructure that doesn't care about message content should not validate it at all." — [Cap'n Proto FAQ](https://capnproto.org/faq.html)
- FlatBuffers: attributes listed are structural (`id`, `required`, `key`, `force_align`, ...); no value constraints. — [FlatBuffers schema](https://flatbuffers.dev/schema/)
- Avro IDL: no constraint syntax on the IDL page; logical types (`decimal(9,2)`, `date`, `uuid`) refine representation, not validation. — [Avro IDL 1.12.0](https://avro.apache.org/docs/1.12.0/idl-language/)
- AsyncAPI: Schema Object inherits JSON Schema Draft 07 validation keywords; `format` adds AsyncAPI-specific predefined formats. — [AsyncAPI spec source](https://github.com/asyncapi/spec/blob/master/spec/asyncapi.md)

### Inferences
- The constraint-bearing languages are exactly the HTTP/API-description ones; the serialization-format IDLs treat validation as out of scope.
- Smithy's single `@length` across list/map/string/blob contrasts with TypeSpec's per-kind decorators (`@minLength` vs `@minItems`), mirroring JSON Schema's vocabulary.

### Gaps
- No Thrift/FlatBuffers/Avro rationale for omitting constraints was found beyond Cap'n Proto's FAQ.

## Errors: modelled errors with status codes

### Takeaway
Smithy, TypeSpec, Fern and Avro/Thrift all name errors as types; HTTP status attaches via `@httpError(404)` (Smithy), `@error` + `@statusCode`/union variants (TypeSpec), `http: statusCode: 404` (Fern). Thrift (`exception` + `throws`), Avro (`error` + `throws`) and Cap'n Proto/FlatBuffers model errors at the RPC level or not at all. AsyncAPI has no error concept.

### Cited Findings
- Smithy: `@error("client") @httpError(404) structure MyError {}`; "The provided value SHOULD be between 400 and 499 for 'client' errors" or 500–599 for server; without `@httpError` the defaults are 400 or 500. `errors` on an operation lists shapes that "must be marked with `@error`"; service-level `errors` are "common errors that all bound operations can return." `@error`: "Indicates that a structure shape represents an error." — [Smithy HTTP bindings](https://smithy.io/2.0/spec/http-bindings.html); [Smithy service types](https://smithy.io/2.0/spec/service-types.html); [Smithy type refinement traits](https://smithy.io/2.0/spec/type-refinement-traits.html)
- TypeSpec: `@error` targets `Model`, no parameters (`@error model PetStoreError { ... }`). HTTP: "Use the `@statusCode` decorator on a property to declare a status code for a response"; defaults `200`, and `4xx,5xx` for `@error` responses; "use number literal types to create a discriminated union of response types" (e.g. `op read(): {@statusCode _: 200, ...Pet}` or union of `statusCode: 200` and `statusCode: 404` variants); built-ins `OkResponse`, `NotFoundResponse`, `NoContentResponse`, `Body<T>`; error variants in a return union map to 4xx/5xx. — [TypeSpec built-in decorators (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/standard-library/built-in-decorators.md); [TypeSpec HTTP operations (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/libraries/http/operations.md); [TypeSpec HTTP cheat sheet (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/libraries/http/cheat-sheet.md)
- Fern: `errors: UserNotFoundError: status-code: 404 / type: UserNotFoundErrorBody`; overview example `errors: NotFoundError: http: statusCode: 404 / type: properties: id: MovieId`; endpoints list `errors: - NotFoundError`. Error discrimination in `api.yml`: status-code discrimination requires each endpoint's listed errors to have distinct codes; name discrimination assumes "every error response has an extra property denoting the error name" (recommended for server-side generation). Note: the two Fern pages show two spellings (`status-code:` vs `http: statusCode:`). — [Fern Definition: Errors](https://buildwithfern.com/learn/api-definitions/ferndef/errors); [Fern Definition: Overview](https://buildwithfern.com/learn/api-definitions/ferndef/overview); [Fern api.yml errors](https://buildwithfern.com/learn/api-definitions/ferndef/api-yml/errors)
- Thrift: `Exception ::= 'exception' Identifier '{' Field* '}'`, `Throws ::= 'throws' '(' Field* ')'`. — [Thrift IDL](https://thrift.apache.org/docs/idl)
- Avro IDL: `error Kaboom { string explanation; int result_code = -1; }` ("`error` uses the same body syntax as `record`"), `void goKaboom() throws Kaboom;`. — [Avro IDL 1.12.0](https://avro.apache.org/docs/1.12.0/idl-language/)
- Cap'n Proto language page: no error/exception declaration in the schema language. — [Cap'n Proto language](https://capnproto.org/language.html)
- FlatBuffers: `rpc_service` has only request/response tables. — [FlatBuffers schema](https://flatbuffers.dev/schema/)
- AsyncAPI 3.0: no error object; `reply` exists on operations for request/reply (not detailed in fetched text). — [AsyncAPI 3.0.0 spec](https://www.asyncapi.com/docs/reference/specification/v3.0.0)

### Inferences
- TypeSpec folds errors into the return-type union with `@statusCode` literals; Smithy keeps a separate `errors` list and derives the status from the error shape. Fern's `api.yml` discrimination strategy makes explicit the runtime problem both of the others leave to generators (how a client tells which error arrived).

### Gaps
- The Fern docs inconsistency (`status-code` vs `http: statusCode`) could not be resolved to one canonical form; both appear in current pages.
- AsyncAPI's request/reply `reply` field details were not in the fetched spec text.

## HTTP bindings: path, query, header, payload

### Takeaway
Smithy binds HTTP per member with traits on the dedicated input structure (`@http`, `@httpLabel`, `@httpQuery`, `@httpHeader`, `@httpPayload`, `@httpPrefixHeaders`, `@httpQueryParams`, `@httpResponseCode`) and requires `@httpLabel` members to be `@required`. TypeSpec binds via `@route`/verb decorators plus `@path`/`@query`/`@header`/`@body`/`@bodyRoot`/`@statusCode` with an *implicit body* rule for unannotated parameters. Fern uses fixed YAML keys (`path`, `path-parameters`, `query-parameters`, `headers`, `request.body`). AsyncAPI uses per-protocol `bindings` objects. Thrift, Cap'n Proto, FlatBuffers and Avro IDL have no HTTP bindings.

### Cited Findings
- Smithy: `@http(method: "PUT", uri: "/{bucketName}/{key}", code: 200)` (`code` optional, default 200; uri "MUST start with '/'"). `@required @httpLabel foo: String` with `@http(method: "GET", uri: "/{foo}")`; "Every label MUST have a corresponding input member, the input member MUST be marked as" required; labels are ignored on output/errors. `@httpQuery("color") color: String` (names "MUST be case-sensitively unique"). `@httpQueryParams myParams: MapOfStrings`. `@httpHeader("X-Foo") foo: String` (case-insensitively unique; `Authorization`, `Host`, `Content-Length`, `User-Agent` restricted). `@httpPrefixHeaders("X-Foo-") headers: StringMap`. `@httpPayload content: Blob` ("Multiple members of the same structure MUST NOT be bound to" httpPayload; with a payload, other input members must be label/header/prefixHeaders/queryParams/query). `@httpResponseCode statusCode: Integer` ("SHOULD be between 200 and 299"). `@httpChecksumRequired operation PutSomething`. — [Smithy HTTP bindings](https://smithy.io/2.0/spec/http-bindings.html)
- TypeSpec cheat sheet: `@get op read(): void`, `@route("/pets/{petId}") op getPet(petId: string): Pet`, `op read(@header traceparent: string): Pet`, `op read(): {@header eTag: string, ...Pet}`, `op list(@query filter: string): Pet[]`, `op add(@body pet: Pet): void`, `op add(...Pet): void` ("Implicit body parameter"), `op read(): {@statusCode _: 200, ...Pet}`; `@server("https://{region}.example.com", "Region scoped endpoint", {region: string})`; `@useAuth(ApiKeyAuth<"header", "Api-Key">)`. — [TypeSpec HTTP cheat sheet (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/libraries/http/cheat-sheet.md)
- TypeSpec rules: `@route` on a namespace/interface prefixes "all the operations, interfaces and namespaces underneath"; `@path` on a brace-named parameter is optional; `@header` name "is inferred from the property or parameter name" if omitted. Implicit body: "The set of parameters that are not marked @header, @query, or @path form the request body."; "The set of properties of the return model that are not marked @header or @statusCode form the response body."; "If the return type is not a model, then it defines the response body." Default verb is `@get` unless a body is present (then POST). `@body`: "As soon as `@body` is reached the content is exactly the body."; `@bodyRoot` lets HTTP-metadata properties inside the model stay metadata. Inapplicable metadata (e.g. `@query` on a response) "becomes part of the payload instead" unless `@includeInapplicableMetadataInPayload` is `false`. — [TypeSpec HTTP operations (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/libraries/http/operations.md)
- Fern: `service: base-path: /users / auth: false / endpoints:`; `path: /{userId} / path-parameters: userId: string / method: GET`; `request: name: GetAllUsersRequest / query-parameters: limit: optional<integer> / headers: X-Endpoint-Header: string`; service-level headers "cascade to the service's endpoints"; `pagination: offset: $request.page / step: $request.page_size / results: $response.data / has-next-page: $response.has_more`; `idempotent: true`. — [Fern Definition: Endpoints](https://buildwithfern.com/learn/api-definitions/ferndef/endpoints)
- AsyncAPI: bindings "hold protocol-specific details at the server, channel, operation, and message levels", keyed by protocol (`http`, `kafka`, ...); v3 moves the channel path into `address: "test/path"` with an arbitrary channel key. — [AsyncAPI 3.0.0 spec](https://www.asyncapi.com/docs/reference/specification/v3.0.0); [Migrating to v3](https://www.asyncapi.com/docs/migration/migrating-to-v3)
- Thrift, Cap'n Proto, FlatBuffers, Avro IDL pages contain no HTTP binding constructs. — [Thrift IDL](https://thrift.apache.org/docs/idl); [Cap'n Proto language](https://capnproto.org/language.html); [FlatBuffers schema](https://flatbuffers.dev/schema/); [Avro IDL 1.12.0](https://avro.apache.org/docs/1.12.0/idl-language/)

### Inferences
- Smithy is "explicit everything" (every member bound, payload exclusivity rules, uniqueness rules); TypeSpec is "convention with escape hatches" (implicit body, inferred header names, inherited routes). Smithy's requirement that labels be `@required` is the direct consequence of its optional-by-default members.

### Gaps
- TypeSpec `@path` options (`allowReserved`) and `@query` `explode` were not in the fetched operations page.

## Resources and lifecycle

### Takeaway
Smithy has a first-class `resource` shape with identifiers, properties and six lifecycle slots (`create`, `put`, `read`, `update`, `delete`, `list`) with mandated behaviour traits (`@readonly`, `@idempotent`). TypeSpec's core language has no resource; `@typespec/rest` adds `@resource`/`@parentResource`/`@key`/`@autoRoute`/`@segment` and interface templates (`ResourceRead<R, E>` ... `ResourceOperations<R, E>`) whose operations are tagged `@readsResource(R)` etc. None of the other six has a resource concept.

### Cited Findings
- Smithy: "Smithy defines a _resource_ as an entity with an identity that has a set of operations." Members: `identifiers` (string shapes), `properties`, lifecycle `create`/`put`/`read`/`update`/`delete`/`list`, `operations`, `collectionOperations`, `resources` (no cycles; a resource bound once per closure). Lifecycle rules: `put` idempotent instance op where "Client supplies the identifier" (`@noReplace` prevents replacement); `create` collection op, "Service generates the identifier"; `read` readonly instance; `update` instance; `delete` idempotent instance; `list` readonly collection, should be paginated. Child resources repeat all parent identifiers; `@resourceIdentifier` binds explicitly; properties must appear in some instance operation I/O (`@property(name:)`, `@nestedProperties`, `@notProperty`). — [Smithy service types](https://smithy.io/2.0/spec/service-types.html)
- Smithy quickstart example: `service Weather { version: "2006-03-01"  resources: [City]  operations: [GetCurrentTime] }`, `resource City { identifiers: { cityId: CityId }  read: GetCity  list: ListCities  resources: [Forecast] }`, `@readonly operation GetCity`. — [Smithy quickstart](https://smithy.io/2.0/quickstart.html)
- TypeSpec REST decorators: `@TypeSpec.Rest.resource(collectionName: valueof string)` on `Model` ("Mark this model as a resource type with a name."); `@parentResource(parent: Model)`; `@segment(name: valueof string)` ("Defines the preceding path segment for a `@path` parameter in auto-generated routes."); `@autoRoute` on `Interface | Operation`; `@readsResource`/`@createsResource`/`@updatesResource`/`@deletesResource`/`@listsResource(resourceType: Model)` on `Operation`; `@action(name?)` ("Scoped to a resource item /pets/{petId}/my-action"), `@collectionAction(resourceType, name?)`. Example: `@autoRoute interface Pets { get(@segment("pets") @path id: string): void; //-> route: /pets/{id} }`. — [TypeSpec REST decorators (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/libraries/rest/reference/decorators.md)
- TypeSpec resource templates (library source): `interface ResourceRead<Resource extends {}, Error> { @readsResource(Resource) get(...ResourceParameters<Resource>): Resource | Error; }`; `ResourceCreate` with `@bodyRoot resource: ResourceCreateModel<Resource>` returning `Resource | ResourceCreatedResponse<Resource> | Error`; `ResourceCreateOrReplace`, `ResourceCreateOrUpdate` (`@patch(#{ implicitOptionality: true })`), `ResourceUpdate`, `ResourceDelete` (`ResourceDeletedResponse | Error`), `ResourceList` (`CollectionWithNextLink<Resource> | Error`); `ResourceOperations<R,E> extends ResourceInstanceOperations<R,E>, ResourceCollectionOperations<R,E>`, guarded by `@Private.validateHasKey(Resource)` and `@Private.validateIsError(Error)`; `ExtensionResource*` variants take `<Extension, Resource, Error>`; `@friendlyName("{name}Key", Resource) model KeysOf<Resource> {}`. — [TypeSpec packages/rest/lib/resource.tsp](https://raw.githubusercontent.com/microsoft/typespec/main/packages/rest/lib/resource.tsp)
- TypeSpec `@key(altName?: valueof string)` on `ModelProperty` (`@key id: string;`). — [TypeSpec built-in decorators (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/standard-library/built-in-decorators.md)
- TypeSpec `@autoRoute` on CommonParameters with `@path @segment("tenants") tenantId: string` yields "/tenants/{tenantId}/users/{userName}"; "If `@autoRoute` is applied to an interface, it is not 'portable'." — [TypeSpec resource routing (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/libraries/rest/resource-routing.md)
- Fern, Thrift, Cap'n Proto, FlatBuffers, Avro, AsyncAPI: no resource construct in the fetched specs. (AsyncAPI's nearest analogue is channels.) — [Fern Definition: Endpoints](https://buildwithfern.com/learn/api-definitions/ferndef/endpoints); [AsyncAPI 3.0.0 spec](https://www.asyncapi.com/docs/reference/specification/v3.0.0)

### Inferences
- Smithy encodes lifecycle semantics as *validated model structure* (a `read` must be `@readonly`, a `delete` must be `@idempotent`), whereas TypeSpec encodes them as *library templates plus marker decorators*; the TypeSpec approach is user-replaceable, the Smithy one is normative.

### Gaps
- Azure ARM resource templates (`typespec-azure-resource-manager`) were out of scope of fetched pages.
- No designer rationale text was found for why Smithy made resources first-class beyond the definition sentence.

## Reuse: mixins, spread, extends, templates, interfaces, retroactive annotation

### Takeaway
TypeSpec has the richest reuse toolkit (spread `...`, `extends`, `is`, templates on models/ops/interfaces, interface `extends`, `@@` augment). Smithy 2.0 added mixins (`with [Mixin]`), target elision (`$name` from a bound resource or mixin), and has always had `apply`. Fern has `extends` (multiple). Thrift has service `extends` only. Cap'n Proto has generics and interface multiple inheritance. FlatBuffers and Avro have neither inheritance nor templates. AsyncAPI reuses via `$ref` and `components`.

### Cited Findings
- Smithy: `structure UserDetails with [BaseUser] {` where `BaseUser` is `@mixin`; `structure UserSummary for User { $name ... }` ("Structures may only reference one resource shape in this way."); `$` elision resolves from the bound resource identifier first, then a mixin member of the same name. `apply` statements (above). Mixins let you share members "without having to copy and paste them"; elision requires opting in per member, "which allows partial views of a resource". — [Smithy IDL spec](https://smithy.io/2.0/spec/idl.html); [Smithy migration guide](https://smithy.io/2.0/_sources/guides/model-translations/migrating-idl-1-to-2.rst.txt)
- TypeSpec: "The spread operator (`...`) copies the members of a source model into a target model." `model Dog { ...Animal; ...Pet; }`; `model Dog extends Animal {}`; `model StringThing is Thing<string>;`; `model Page<Item> { size: int32; item: Item[]; }` and `model DogPage { ...Page<Dog>; }`. — [TypeSpec Models (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/language-basics/models.md)
- TypeSpec ops/interfaces: `op deletePet is Delete;`, `op readPet is ReadResource<Pet>;`, `interface C extends A, B`, `interface ReadWrite<T>`. — [TypeSpec Operations (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/language-basics/operations.md); [TypeSpec Interfaces (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/language-basics/interfaces.md)
- Fern: `Dog: extends: Pet / properties: breed: string`; "You can extend multiple objects:" `extends: - Dog - Pet`. Aliases: `UserId: string`. — [Fern Definition: Types](https://buildwithfern.com/learn/api-definitions/ferndef/types)
- Thrift: `Service ::= 'service' Identifier ( 'extends' Identifier )? '{' Function* '}'`; `Typedef ::= 'typedef' DefinitionType Identifier`. No struct inheritance. — [Thrift IDL](https://thrift.apache.org/docs/idl)
- Cap'n Proto: `struct Map(Key, Value)` generics ("Only pointer types can be parameters"; omitted parameters default to `AnyPointer`); interface `extends(...)` multiple inheritance; `using` aliases; retroactive unionisation of an existing field "without changing its layout". — [Cap'n Proto language](https://capnproto.org/language.html)
- FlatBuffers: `include`, `namespace`, unions; no inheritance or generics on the schema page. — [FlatBuffers schema](https://flatbuffers.dev/schema/)
- Avro IDL: `import idl/protocol/schema`, `@aliases` for renames; no inheritance. — [Avro IDL 1.12.0](https://avro.apache.org/docs/1.12.0/idl-language/)
- AsyncAPI: `$ref: '#/components/schemas/Address'`; "`$ref` follows the Reference Object rules, not the JSON Schema meaning"; v3 "requires explicit `$ref` pointers everywhere" (e.g. `servers: - $ref: "#/servers/production"`). — [AsyncAPI 3.0.0 spec](https://www.asyncapi.com/docs/reference/specification/v3.0.0); [Migrating to v3](https://www.asyncapi.com/docs/migration/migrating-to-v3)

### Inferences
- Smithy's mixin design deliberately excludes subtyping: a mixin copies members and a `for` binding copies identifier targets; there is no "is-a" relation, consistent with its generated-code-stays-flat goal. TypeSpec offers both copy (`...`) and subtype (`extends`).

### Gaps
- Smithy 2.0 spec text on mixin restrictions (e.g. traits on mixins, `@mixin(localTraits:)`) was not in the fetched IDL page.

## Documentation and examples

### Takeaway
Doc-comment syntax: Smithy `///` (must precede traits), TypeSpec `/** */` (equivalent to `@doc`, supports `@param`/`@returns`/`@template`), Thrift `/** */` (`///` explicitly does not work), FlatBuffers `///`, Avro `/** */`, Cap'n Proto `#` comments placed *after* the declaration, Fern `docs:` keys, AsyncAPI `description` (CommonMark). Structured examples exist in Smithy (`@examples` with input/output/error), TypeSpec (`@example`, `@opExample`), Fern (`examples:` on endpoints/types) and AsyncAPI (message `examples`).

### Cited Findings
- Smithy: "A documentation comment is formed when three forward slashes" begin a line; they "MUST appear" before any traits on the shape. `@documentation` "Adds documentation to a shape or member using the CommonMark format." `@examples` members: `title`, `documentation`, `input`, `output`, `error`, `allowConstraintErrors`; "A value for `output` or `error` SHOULD be provided. However, both MUST NOT be defined for the same example." Also `@externalDocumentation` (map of name→URL), `@deprecated(message, since)`, `@since`, `@title`, `@sensitive` ("MUST NOT be exposed in things like exception messages or log output"), `@unstable`, `@internal`. Members without docs inherit the target shape's documentation. — [Smithy IDL spec](https://smithy.io/2.0/spec/idl.html); [Smithy documentation traits](https://smithy.io/2.0/spec/documentation-traits.html)
- TypeSpec: "Doc comments start with `/**` and continue until the closing `*/` is encountered." "These comments are treated as if they were attached using the `@doc` decorator." Tags: `@param`, `@returns`, `@template`, `@example` "(unofficial)". "TypeSpec Documentation support CommonMark markdown formatting." `@doc(doc: valueof string, formatArgs?: {})`, `@summary`, `@example(#{name: "Fluffy", age: 2})` on models/enums/scalars/unions/properties, `@opExample(#{parameters: #{...}, returnType: #{...}})` on operations. — [TypeSpec Documentation (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/language-basics/documentation.md); [TypeSpec built-in decorators (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/standard-library/built-in-decorators.md)
- Fern: `docs:` at type, property, endpoint and service level (`Person: docs: A person represents a human being`); endpoint `examples:` with `path-parameters`, `query-parameters`, `headers`, `response: body:` and failure examples via `response: error: UserNotFoundError`. — [Fern Definition: Types](https://buildwithfern.com/learn/api-definitions/ferndef/types); [Fern Definition: Endpoints](https://buildwithfern.com/learn/api-definitions/ferndef/endpoints)
- Thrift: `/** ... */` before typedefs, structs, fields ("doct text goes before a comma"), enums/values, services, functions and parameters; "This style of Doxy-comment doesn't work." for `///`; a file-level "Program doctext". — [apache/thrift test/DocTest.thrift](https://github.com/apache/thrift/blob/master/test/DocTest.thrift)
- Cap'n Proto: "Comments meant as documentation should appear after the declaration." — [Cap'n Proto language](https://capnproto.org/language.html)
- FlatBuffers: `///` on its own line documents the next declaration and "is output into generated C++". — [FlatBuffers schema](https://flatbuffers.dev/schema/)
- Avro IDL: `/** ... */` directly before a type or field becomes its documentation string; `//` and `/* */` are ordinary comments. — [Avro IDL 1.12.0](https://avro.apache.org/docs/1.12.0/idl-language/)
- AsyncAPI: `description` uses CommonMark; message `examples: - name: SimpleSignup / payload: ...` (with `summary`, `headers`); `tags`, `externalDocs` at several levels; Schema `deprecated: true` "SHOULD be transitioned out of usage". — [AsyncAPI 3.0.0 spec](https://www.asyncapi.com/docs/reference/specification/v3.0.0); [AsyncAPI spec source](https://github.com/asyncapi/spec/blob/master/spec/asyncapi.md)

### Inferences
- Smithy's `@examples` is the only one that models the *error* case of an operation example and can relax constraint validation (`allowConstraintErrors`), reflecting that examples are validated against the model.

### Gaps
- No language's docs describe cross-linking shapes from doc comments (Smithy's page explicitly does not; TypeSpec's page has no cross-reference section).

## Namespaces, packages, files, imports, standard libraries

### Takeaway
Smithy: `namespace` per file, `use` for short names, `metadata`, control statements (`$version`), a merged "semantic model" from many files, and a `smithy.api` prelude. TypeSpec: `import "./file.tsp"`, directories resolve to `main.tsp`, npm packages by name, `@typespec/http`/`rest`/`openapi` libraries. Fern: `api.yml` + per-service YAML files with `imports:`. Thrift: `include`, per-language `namespace`. Cap'n Proto: `using import "x.capnp"`, 64-bit file ids. FlatBuffers: `include`, `namespace`, `root_type`, `file_identifier`. Avro: `protocol`/`schema` files with `import idl|protocol|schema`. AsyncAPI: single document with `components` and `$ref`.

### Cited Findings
- Smithy: `$version: "2"`, `namespace example.weather`; "Shapes can only be defined after a namespace is declared."; `use` statements import shapes under shorter names; `metadata foo = "bar"`; "Implementations MUST ignore unknown control statements."; shape IDs `namespace#ShapeName$memberName`; "All Smithy models automatically include a prelude."; "One or more model files can be assembled (or merged) together to form a semantic model." Build: `smithy-build.json` with `"version": "1.0"`, `"sources": ["model"]`, then `smithy build`. 1.0 and 2.0 files can be loaded into one model for incremental migration. — [Smithy IDL spec](https://smithy.io/2.0/spec/idl.html); [Smithy model spec](https://smithy.io/2.0/spec/model.html); [Smithy quickstart](https://smithy.io/2.0/quickstart.html); [Smithy migration guide](https://smithy.io/2.0/_sources/guides/model-translations/migrating-idl-1-to-2.rst.txt)
- TypeSpec: `import "./models/foo.tsp"; import "./decorators.js";` ("must either start with `./` or `../`, or be an absolute path"); `import "./models";` → `./models/main.tsp`; `import "@typespec/rest";` → `./node_modules/@typespec/rest/lib/main.tsp`. — [TypeSpec Imports (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/language-basics/imports.md)
- TypeSpec 1.0 stable set: `@typespec/compiler`, `@typespec/http`, `@typespec/openapi`, `@typespec/openapi3`, `@typespec/json-schema`, VS Code extension; client/server codegen for .NET/JS/Java/Python in preview. — [TypeSpec 1.0 GA](https://typespec.io/blog/typespec-1-0-GA-release/); [TypeSpec 1.0.0 release notes](https://typespec.io/release-notes/typespec-1-0-0/)
- Fern: per-service file (`imdb.yml`) with `service:`/`types:`/`errors:` sections; `api.yml` holds API-level config such as error discrimination. "Fern Definition isn't recommended for new customers and Fern isn't accepting feature requests for this format." — [Fern Definition: Overview](https://buildwithfern.com/learn/api-definitions/ferndef/overview); [Fern api.yml errors](https://buildwithfern.com/learn/api-definitions/ferndef/api-yml/errors)
- Thrift: `Include ::= 'include' Literal`, `CppInclude ::= 'cpp_include' Literal`, `Namespace ::= ( 'namespace' ( NamespaceScope Identifier ) )` with scope `*` for all languages. — [Thrift IDL](https://thrift.apache.org/docs/idl)
- Cap'n Proto: "A Cap'n Proto file must have a unique 64-bit ID" (`@0xdbb9ad1f14bf0b36;`); `using Bar = import "bar.capnp";`, `using import "bar.capnp".Baz;`; absolute paths searched in `-I` dirs; type IDs default to "the first 8 bytes of the MD5 hash" of parent id + name. — [Cap'n Proto language](https://capnproto.org/language.html)
- FlatBuffers: `namespace MyGame;` (dots nest); `include "mydefinitions.fbs";` ("Only the current file's definitions are generated"); `root_type Monster;`; `file_identifier "MYFI";` (exactly 4 chars at bytes 4–7); `file_extension "ext";`. — [FlatBuffers schema](https://flatbuffers.dev/schema/)
- Avro IDL: a file is either `protocol MyProtocol { }` (optionally `@namespace("...")`) or a schema file with exactly one `schema ...;` plus named types under `namespace`; `import idl "foo.avdl";`, `import protocol "foo.avpr";`, `import schema "foo.avsc";` (schema files import only named types; protocols also import messages); `.avdl` compiles to `.avpr`/`.avsc`. — [Avro IDL 1.12.0](https://avro.apache.org/docs/1.12.0/idl-language/)
- AsyncAPI: root `asyncapi` (REQUIRED), `info` (`title`, `version` required), `servers`, `channels`, `operations`, `components`; `schemaFormat` moved under `payload` in v3; multi-format schemas accept JSON Schema draft-07, Avro 1.9.0, OpenAPI 3.0.0, RAML 1.0, Protobuf 2/3 media types. — [AsyncAPI 3.0.0 spec](https://www.asyncapi.com/docs/reference/specification/v3.0.0); [Migrating to v3](https://www.asyncapi.com/docs/migration/migrating-to-v3); [AsyncAPI spec source](https://github.com/asyncapi/spec/blob/master/spec/asyncapi.md)

### Inferences
- Smithy and Cap'n Proto are the two that give shapes globally unique, stable identities independent of file layout (shape IDs; 64-bit ids), which is what makes Smithy's `apply`-from-anywhere and Cap'n Proto's evolution rules workable.

### Gaps
- TypeSpec `namespace`/`using` statement syntax was not in the fetched imports page.
- The spec-repo `master` of AsyncAPI already references "AsyncAPI 3.1.0" in the multi-format table; 3.1.0's release status was not verified.

## Compiler implementation language and runtime dependency

### Takeaway
JVM: Smithy (Java; CLI distributed as platform binaries), Avro IDL (`avro-tools.jar`). Node: TypeSpec (`@typespec/compiler`), Fern (`fern-api`, Node 22+), AsyncAPI tooling (`@asyncapi/parser`, CLI, generator). C++: Thrift compiler, Cap'n Proto `capnp`, FlatBuffers `flatc`.

### Cited Findings
- Smithy CLI: installed via `brew install smithy-cli` / `scoop install smithy-lang/smithy-cli` / zip installer; supported on macOS (x86, ARM), Linux (x86, ARM), Windows (x64); the install page does not mention Java. — [Smithy CLI installation](https://smithy.io/2.0/guides/smithy-cli/cli_installation.html)
- Smithy Gradle path: "make sure you have gradle installed", `gradle build`. — [Smithy quickstart](https://smithy.io/2.0/quickstart.html)
- TypeSpec: `@typespec/compiler` is an npm package; decorators are implemented as exported JS functions (`export function $logType(context: DecoratorContext, ...)`). — [TypeSpec 1.0 GA](https://typespec.io/blog/typespec-1-0-GA-release/); [TypeSpec Create decorators (doc source)](https://raw.githubusercontent.com/microsoft/typespec/main/website/src/content/docs/docs/extending-typespec/create-decorators.md)
- Fern: "Requires Node.js 22 or higher, which ships with npm 10 or higher."; `npm install -g fern-api`. — [Fern CLI overview](https://buildwithfern.com/learn/cli-api-reference/cli-reference/overview)
- Thrift: "Contains the Thrift compiler, implemented in C++." Building needs Flex (`compiler/cpp/thriftl.cc`). — [apache/thrift README](https://github.com/apache/thrift/blob/master/README.md); [Thrift building from source](https://thrift.apache.org/docs/BuildingFromSource)
- Cap'n Proto: the tools "are written in C++"; "Cap'n Proto makes extensive use of C++14 language features"; minimum GCC 7.0 / Clang 6.0 / Visual C++ 2019; other-language codegen via plugins (`capnpc-c++.exe`, `capnpc-capnp.exe`). — [Cap'n Proto install](https://capnproto.org/install.html)
- FlatBuffers: "The distribution main build system is configured by `cmake`"; language guides for C, C++, C#, Dart, Go, Java, JavaScript, Kotlin, Lobster, Lua, PHP, Python, Rust, Swift, TypeScript. — [FlatBuffers building](https://flatbuffers.dev/building/)
- Avro IDL: `java -jar avro-tools.jar idl ... .avdl ... .avsc`. — [Avro IDL 1.12.0](https://avro.apache.org/docs/1.12.0/idl-language/)
- AsyncAPI: `@asyncapi/parser` validates and parses documents "in your Node.js or browser application", "Validation is powered by Spectral"; generator via `npm install @asyncapi/generator` and the AsyncAPI CLI. — [asyncapi/parser-js](https://github.com/asyncapi/parser-js); [AsyncAPI generator installation](https://www.asyncapi.com/docs/tools/generator/installation-guide)

### Inferences
- The Smithy CLI's platform-specific binaries with no Java prerequisite on the install page suggest a bundled runtime, but the page does not say so; treat "no separate JDK needed" as unconfirmed.

### Gaps
- Could not confirm from a primary page whether the Smithy CLI bundles a JRE or requires one; the install page is silent and the quickstart only mentions Gradle for the Gradle path.
- FlatBuffers docs fetched do not state the C++ standard `flatc` requires.
