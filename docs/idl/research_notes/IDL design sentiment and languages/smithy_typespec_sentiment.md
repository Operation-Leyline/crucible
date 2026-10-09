# Community and practitioner sentiment: Smithy (AWS) and TypeSpec (Microsoft)

Research date: 2026-10-08. Repository statistics were read live from the GitHub API on that date. Reddit could not be fetched or searched from this environment (see Gaps throughout), so community sentiment below is drawn from Hacker News (via the Algolia API), GitHub issues, and blog posts.

Summary of the shape of the evidence:
- Smithy's only large public discussion is one 2021 HN thread (243 points, 73 comments); later submissions got single-digit points. Non-AWS adoption is documented mainly through the ecosystem list (smithy4s at Disney Streaming, a handful of community generators) and a 2023 "Ask HN" that drew one answer.
- TypeSpec has three large HN threads (2024: 275/114, 118/72, 107/81 points/comments) with a recurring pattern: strong praise for "better than writing OpenAPI by hand", recurring skepticism about "yet another DSL", Node dependency, and Microsoft-centricity. Named non-Microsoft adopters are few (OpenMeter, Pinterest as a collaborator, Sportradar as a would-be adopter blocked on AsyncAPI).
- Several commenters explicitly compare the two and cite Smithy's JVM/Gradle/Kotlin requirement as the main reason to prefer TypeSpec; Smithy is cited as "more battle-tested" and "generates much more".

---

## Smithy: adoption outside AWS

### Takeaway
Documented non-AWS adoption is thin and concentrated in the Scala world (Disney Streaming's smithy4s, plus Alloy/smithy-translate from the same team); a 2023 "Ask HN: Does anyone use AWS Smithy outside of AWS?" got exactly one reply, which pointed at smithy4s and smithy-dafny. Community generators exist for Scala, Erlang/Elixir/Gleam, C#, OCaml, Rust and Common Lisp, but none shows evidence of large organisational adoption.

### Cited Findings
- Smithy's own docs state the motivation was AWS-internal: existing IDLs "were not extensible enough to meet our needs and not compatible with our existing services." — [smithy.io 2.0 index](https://smithy.io/2.0/index.html)
- "Ask HN: Does anyone use AWS Smithy outside of AWS?" (2023-12-06, 2 points, 1 comment). Poster thesurlydev: "I'm impressed with Smithy ... I've seen the code generation code and it looks daunting." The only reply (salkeldr) named smithy-dafny and noted "smithy-scala was developed by Disney rather than Amazon." — [HN 38546950](https://news.ycombinator.com/item?id=38546950)
- The awesome-smithy list (read 2026-10-08) names these non-AWS projects: smithy4s (Disney Streaming, Scala clients and servers), smithy-beam (Erlang/Elixir/Gleam), smithy-dotnet (C#), smaws (OCaml), Atelier (Rust implementation, marked work-in-progress), smithy-lisp, Mill and SBT plugins, Smithy Cargo (Rust build.rs integration), smithy-translate (OpenAPI/JSON Schema to Smithy and Smithy to Protobuf, Disney Streaming), Alloy (Disney Streaming trait library), smithy4s-course and Smithy Playground (kubukoz), tree-sitter-smithy, zed-smithy, and the Build Server Protocol which uses Smithy as its definition language. — [awesome-smithy README](https://github.com/smithy-lang/awesome-smithy)
- GitHub stars on 2026-10-08: smithy-lang/smithy 2,373 (94 open issues, created 2019-03); disneystreaming/smithy4s 402 (115 open issues, last push 2026-10-06); smithy-rs 684 (417 open issues); smithy-typescript 328; smithy-java 71; smithy-language-server 45. For comparison microsoft/typespec 5,880 stars, 1,053 open issues. — GitHub API, read live ([smithy](https://github.com/smithy-lang/smithy), [smithy4s](https://github.com/disneystreaming/smithy4s), [typespec](https://github.com/microsoft/typespec))
- In the 2021 HN thread, Smithy's creator mtdowling said "Smithy can be used for anything and isn't specific to AWS," and an AWS principal (donavanm) said "composable models, projections, and transforms are super common" internally. — [HN 27081084](https://news.ycombinator.com/item?id=27081084) (2021-05-07, 243 points, 73 comments)
- A 2022 "Ask HN: Which API IDLs would you use?" poster (Existenceblinks) wrote: "I'm personally inclined to Smithy which seems to be the most battle-tested with real and large use case" and that it could be sold to a boss "because it's from AWS." — [HN 34072823](https://news.ycombinator.com/item?id=34072823) (2022-12-20)
- In the March 2024 TypeSpec thread, hocuspocus: "I would pick Smithy if I had to go spec-first" (they use Tapir code-first); lijok said their team "was moving toward Smithy to avoid OpenAPI." — [HN 39833321](https://news.ycombinator.com/item?id=39833321) (2024-03-26)
- AWS published all AWS service models in Smithy JSON AST form (June 2025, aws/api-models-aws, daily to Maven Central). InfoQ quoted developer David Wells: "This is an incredible 1st-party repo containing all AWS service models." InfoQ named no non-AWS Smithy users and framed third-party SDK generation as a possibility, not an existing deployment. — [InfoQ 2025-06-19](https://www.infoq.com/news/2025/06/aws-smithy-api-models-opensource/); [AWS News Blog](https://aws.amazon.com/blogs/aws/introducing-aws-api-models-and-publicly-available-resources-for-aws-api-definitions/)
- The HN submission of the AWS models release got 1 point and 0 comments (2025-06-06). — [HN 44202937](https://news.ycombinator.com/item?id=44202937)
- The smithy4s docs home page names no adopters other than crediting Disney Streaming; it positions Smithy as "protocol-agnostic" with "a standard library of protocol-related traits (http/json/xml)" and gives no comparison to OpenAPI/protobuf/Tapir. — [smithy4s docs](https://disneystreaming.github.io/smithy4s/)
- kubukoz (smithy4s contributor) has a Polish-language talk deck titled "Uwolnij swoje API od bólu z smithy4s" ("Free your API from pain with smithy4s"). — [Speaker Deck](https://speakerdeck.com/kubukoz/uwolnij-swoje-api-od-bolu-z-smithy4s-c06de564-4646-422e-befd-dabd4579e5e1)
- Bump.sh publishes a guide "Using Smithy to define your APIs" framing it for API-first design (date not shown). — [Bump.sh guide](https://docs.bump.sh/guides/api-basics/using-smithy-to-define-your-apis/)

### Inferences
- Outside AWS, Smithy's living ecosystem is essentially the Scala/Typelevel community around Disney Streaming (smithy4s, Alloy, smithy-translate, kubukoz's tooling). That community produced most of the non-AWS tooling and most of the non-AWS bug reports in the core repo (e.g. kubukoz, Baccata on issue #1660).
- The "sell it to the boss because it's AWS" sentiment and the 2021 thread's enthusiasm did not translate into visible adoption stories; there are no engineering-blog "we chose Smithy" posts in the search results.

### Gaps
- No Reddit threads could be retrieved (reddit.com blocked; site: search returned nothing).
- I found no English-language conference talk or engineering blog from Disney Streaming explaining why they chose Smithy; the smithy4s docs do not state it.
- GitHub Discussions on smithy-lang/smithy could not be listed (PowerShell quoting failure on the GraphQL call).
- No independent adoption numbers (downloads, user counts) for Smithy exist in the sources found.

---

## Smithy: the JVM/Gradle requirement, IDE and tooling quality

### Takeaway
The JVM/Gradle/Kotlin dependency is the single most-cited reason people give for preferring TypeSpec; AWS has partially answered it with a standalone CLI (2023) but refused to publish it to npm (2025), and its own IDE tooling (language server, IntelliJ plugin) is small and the creator admitted VS Code tooling gaps in 2024.

### Cited Findings
- jen20 (HN, 2024-04-30): TypeSpec's "lack of Kotlin and Gradle is a significant advantage over Smithy," and "since smithy is already out there and used in a similar domain, it would be useful to have a comparison." — [HN 40206124](https://news.ycombinator.com/item?id=40206124)
- johnathon023 (HN, 2025-12-27, on the Alloy codegen framework that powers TypeSpec): "Looks like a great DSL, similar to https://smithy.io/, but more extensible, not tied to Java, and built with Typescript in mind." — [HN 46399072](https://news.ycombinator.com/item?id=46399072)
- In the 2021 HN thread, scrubs: "I know JAVA from previous jobs, but not Gradle," and found it unclear what generated artifacts should be composed with; stevenhuang could not find codegen docs. mtdowling and catlifeonmars discussed Smithy's VS Code tooling gaps in the 2024 TypeSpec thread; catlifeonmars "found no easy parser hook for a language server." — [HN 27081084](https://news.ycombinator.com/item?id=27081084); [HN 40206124](https://news.ycombinator.com/item?id=40206124)
- Issue #2569 "[feature] publish smithy cli to npm" (2025-03-23, closed): opener cogwirrel: "It's super cool to be able to use Smithy via a CLI without the need to install Java and Gradle!" and asked for an npm package. AWS's JordonPhillips (2025-11-14): "This seems like a strange way to use npm, and it probably violates npm's terms of use ... ultimately what we're distributing is a minimal JRE alongside a fat jar." A later commenter (crowecawcaw, 2025-12-28) pushed back; the opener's resolution (2026-08-06) was an indirection via `npx -y mise exec smithy -- smithy build`. — [smithy #2569](https://github.com/smithy-lang/smithy/issues/2569)
- Issue #1001 "Use Wasm for plugin mechanism" (2021-12, 8 comments, closed) shows the plugin model is JVM-bound. — [smithy #1001](https://github.com/smithy-lang/smithy/issues/1001)
- Issue #824 "Please invest time in better examples and documentation" (2021-06-09): "the documentation makes getting started incredibly difficult ... I'm basically poking around in the dark." mtdowling replied the docs were "still a work in progress" and that generators would be "spread over this year and next year"; the issue was closed in 2023-07 pointing to `smithy init` and the smithy-examples repo. — [smithy #824](https://github.com/smithy-lang/smithy/issues/824)
- Issue #1660 "Version compatibility guarantees" (2023-03-06, kubukoz): a minor-version bump (1.27.2 to 1.28.0) broke binary compatibility (`NoSuchMethodError`) between smithy-model and smithy-openapi when pulled transitively via Disney's smithytranslate and alloy. Baccata (smithy4s author): "the expectation from downstream users of libraries that use the semver versioning pattern is that minor versions are binary backward compatible." mtdowling: "Sorry about the break ... we will make every reasonable attempt to maintain binary compatibility." — [smithy #1660](https://github.com/smithy-lang/smithy/issues/1660)
- Other Gradle/CLI friction issues (all closed): #1297 "Smithy gradle plugin dependency resolution can result in projects that won't build" (2022-07); #1859 "Smithy models with subdirectories no-longer resolve correctly" (2023-07); #2370 SSL handshake failures with private Maven repos via the CLI (2024-08); #2493 `smithy init` fails outside a git repo (2024-12, 10 comments); #2029 CLI behind an authenticating proxy (2023-11). — [#1297](https://github.com/smithy-lang/smithy/issues/1297), [#1859](https://github.com/smithy-lang/smithy/issues/1859), [#2370](https://github.com/smithy-lang/smithy/issues/2370), [#2493](https://github.com/smithy-lang/smithy/issues/2493), [#2029](https://github.com/smithy-lang/smithy/issues/2029)
- IDE: the official VS Code extension (smithy-lang/smithy-vscode) wraps the Smithy Language Server (distributed via Maven; Neovim configs launch it with `coursier launch software.amazon.smithy:smithy-language-server`). The official awslabs/smithy-intellij plugin's README says it is "in development" and its last push was March 2022; the community iancaffey/smithy-intellij-plugin is more recent (push April 2024). JetBrains' LSP API is only available in paid IDEs, so LSP-based Smithy plugins do not work in IntelliJ Community. — [awslabs/smithy-intellij](https://github.com/awslabs/smithy-intellij); [iancaffey plugin](https://github.com/iancaffey/smithy-intellij-plugin); [JetBrains LSP docs](https://plugins.jetbrains.com/docs/intellij/language-server-protocol.html); [nvim-lspconfig smithy_ls](https://git.sudomsg.com/mirror/nvim-lspconfig/tree/lsp/smithy_ls.lua)
- smithy-language-server: 45 stars, 19 open issues, last push 2026-10-01 (live). — GitHub API
- kubukoz maintains a separate "Smithy Playground" language server + VS Code client, marked work-in-progress on the awesome list, plus a web "Smithy Selector Playground". — [awesome-smithy](https://github.com/smithy-lang/awesome-smithy)
- The Smithy CLI was introduced 2023-04 (HN submission: 8 points, 0 comments). — [AWS blog](https://aws.amazon.com/blogs/developer/introducing-the-smithy-cli/); [HN 35732164](https://news.ycombinator.com/item?id=35732164)

### Inferences
- AWS treats the JVM as a non-negotiable implementation choice (the npm refusal, the Wasm plugin issue closed) rather than a problem to solve; non-JVM teams must wrap the CLI.
- The existence of a community-built language server (kubukoz's Smithy Playground) and a community IntelliJ plugin that is more active than the official one suggests the official IDE story is under-resourced relative to TypeSpec's (whose VS Code extension is part of the 1.0 stable set).

### Gaps
- I found no quantitative data (marketplace installs) for the Smithy VS Code or IntelliJ extensions.
- No HN comment-level search hits for "smithy gradle" (Algolia returned zero comment hits for that query), so the JVM complaint is attested in story-level threads only.

---

## Smithy: trait system, resources (@notProperty/@nestedProperties), 2.0 nullability, mixins/apply

### Takeaway
There is no visible "trait soup" discourse by that name; the documented friction is specific: the `@input` implies `@clientOptional` rule confuses users (an open 2026 issue led the creator to say the guidance should change), `@nestedProperties` plus member elision produced an unclear validation error (2024), mixin/`apply` AST output confused a generator author (open since 2025), and the 2.0 nullability model was still "a work in progress" in generators when announced and forced breaking SDK changes in 2023.

### Cited Findings
- Smithy IDL 2.0 (announced 2022-08-11, Kevin Stich): pre-2.0 generators "essentially treated every structure member as nullable"; 2.0 added `@default`, required removing `@required` only when a default is supplied, mixins ("a kind of model-time copy-paste"), resource `properties` with the new `@notProperty` trait, and `enum`/`intEnum` shapes replacing the deprecated `@enum` trait. The post admits "many teams get confused by how these semantics work" and that implementing nullability "in generators and AWS SDKs is still a work in progress." It does not say what breaks on migration; it links a migration guide. — [AWS blog: Introducing Smithy IDL 2.0](https://aws.amazon.com/blogs/developer/introducing-smithy-idl-2-0)
- The HN submission of Smithy 2.0 got 4 points and 1 comment (2022-08-11). — [HN 32420575](https://news.ycombinator.com/item?id=32420575)
- Issue #3203 "Question about input trait that implies clientOptional on required fields" (2026-07-02, open, 9 comments): timocov found that `@input` making `@required` members implicitly `@clientOptional` meant `smithy diff` did not flag marking an existing member required as breaking. mtdowling (2026-07-03): "we should update the guidance to instead say that the member remains required when validation is performed but is 'optional' in generated types regardless of client or server. This is works better for sharing types across client, server, types, etc." — [smithy #3203](https://github.com/smithy-lang/smithy/issues/3203)
- Issue #2294 "Validation issue with `nestedProperties` and member elision" (2024-05-21, closed, 8 comments): `@nestedProperties` on a resource-bound structure produced "Member tenantId does not target a property or identifier for resource TenantResource"; adding `@required` did not help, adding `@resourceIdentifier` did; an AWS maintainer said "The error messages returned for elided identifiers are currently a bit unclear." — [smithy #2294](https://github.com/smithy-lang/smithy/issues/2294)
- Issue #2535 "Improve mixin documentation + possibly mixin bug" (2025-02-21, open, 8 comments): mullermp (writing a generator) found the `smithy ast` output emits both an `apply` shape and the trait on the member for mixin-redefined members and asked "Why is there both ..."; JordonPhillips explained it is intentional and recommended `--flatten` for AST consumers. — [smithy #2535](https://github.com/smithy-lang/smithy/issues/2535)
- The most-commented issue in the repo is #1002 "How to specify a constant HTTP header value in a Smithy model?" (2021-12, still open, 32 comments); mtdowling: "You can't right now, but this is a reasonable feature request." — [smithy #1002](https://github.com/smithy-lang/smithy/issues/1002)
- Other design-friction issues: #1160 "HttpHeaderTraitValidator is overly constraining" (2022-03, 10 comments); #1029 "URI conflict validation is too constraining" (2021-12, 8 comments); #1386 "Non-2xx HTTP response code emits a DANGER validation error" (2022-09, 8 comments); #2270 "Representing a list at the root of the output" (2024-05, 8 comments); #2143 "Add support for untagged union types" (2024-02); #1222 "How should a server reject unknown union variants?" (2022-05, 9 comments). — [#1160](https://github.com/smithy-lang/smithy/issues/1160), [#1029](https://github.com/smithy-lang/smithy/issues/1029), [#1386](https://github.com/smithy-lang/smithy/issues/1386), [#2270](https://github.com/smithy-lang/smithy/issues/2270), [#2143](https://github.com/smithy-lang/smithy/issues/2143), [#1222](https://github.com/smithy-lang/smithy/issues/1222)
- smithy-rs applied 2.0 nullability in November 2023: "generated struct fields modeled as required stop being Option-wrapped wherever the model allows it" (smithy-rs#2916, upgrade guidance in smithy-rs discussion #2929), with a `nullabilityCheckMode: "CLIENT_ZERO_VALUE_V1"` escape hatch; builders now fail if a required member is unset. — [aws-sdk-rust release 2023-11-02](https://github.com/awslabs/aws-sdk-rust/releases/tag/release-2023-11-02)
- Smithy 1.72.0 adjusted the `AddedRequiredMember` diff check so new `@clientOptional` members are no longer flagged, "the sanctioned way to add a required member without breaking generated client code." — [smithy 1.72.0 release notes](https://newreleases.io/project/github/smithy-lang/smithy/release/1.72.0)
- Positive design sentiment from 2021 HN: ellisv "It's elegant. It doesn't overreach."; martypitt "The low signal-to-noise ratio and boilerplate in OpenAPI and RAML is a killer IMO."; phjordon (AWS) "Smithy is designed to be auth agnostic so new auth traits can be added by anyone." — [HN 27081084](https://news.ycombinator.com/item?id=27081084)
- In the April 2024 TypeSpec thread, gjmveloso: "It's a lot of language for a DSL to define services. Smithy sounds more balanced and extendable"; nicce said Smithy "can already generate much more than TypeSpec based on the docs and awesome list." — [HN 41601728](https://news.ycombinator.com/item?id=41601728); [HN 40206124](https://news.ycombinator.com/item?id=40206124)

### Inferences
- The 2.0 nullability design is subtle enough that even in 2026 a user and the creator disagreed on what the docs should say; a team that must implement the `@input`/`@clientOptional`/`@default`/`@required` matrix in its own generator inherits that subtlety.
- Resource properties/`@nestedProperties`/member elision are a source of unclear diagnostics rather than of principled complaints; the mechanism works once `@resourceIdentifier` or `for` binding is spelled out.

### Gaps
- No source uses the phrase "trait soup" or complains about selectors specifically; selector friction is only implied by the existence of kubukoz's "Smithy Selector Playground".
- I could not retrieve the body of smithy-rs discussion #2929 (the nullability upgrade guide) or the IDL 1.0-to-2.0 migration guide text.

---

## Smithy: generators (smithy-rs, smithy-typescript, smithy-java), OpenAPI conversion, protocol tests

### Takeaway
Most official generators are still marked "under construction" on AWS's own list (TypeScript client and server, Go, Rust client and server, Ruby, Swift, Python, Dafny, Java server), smithy-rs carries a README warning that all interfaces are unstable, and the OpenAPI converter's limits surface as feature-request issues (bearer auth customisation, discriminated unions, constant headers). I found no independent write-up of running smithy-rs or smithy-typescript server generators outside AWS.

### Cited Findings
- awesome-smithy (read 2026-10-08) marks these official generators with the construction icon: TypeScript client and server, Java server, Go client, Rust client and server, Ruby, Swift, Dafny, Python. Only the Java client and Kotlin client are unmarked. — [awesome-smithy](https://github.com/smithy-lang/awesome-smithy)
- smithy-rs README: "all internal and external interfaces are considered unstable and subject to change without notice." A 2026 advisory for `aws-smithy-http-server` describes an unauthenticated Slowloris denial-of-service in the default `serve()` path. — [smithy-rs](https://github.com/smithy-lang/smithy-rs); [GitLab advisories](https://advisories.gitlab.com/pkg/cargo/aws-smithy-http-server/)
- smithy-typescript server generator docs warn: "Due to incompatibilities between the way that Smithy HTTP bindings and API Gateway HTTP APIs match requests to operations, use of HTTP APIs with the Smithy Server Generator for TypeScript is not recommended." — [smithy.io ts-ssdk supported endpoints](https://smithy.io/ts-ssdk/supported-endpoints.html)
- smithy-typescript most-commented issues: #492 "Unable to build generated typescript code" (2022-01, 20 comments); #503 "Members on models are always type or `undefined`" (2022-02); #1366 "Navigating to input and output types for a command in TypeScript is too tedious" (2024-08); #1675 "Non generic types Buffer & Uint8Array" (2025-09, 16 comments). — [smithy-typescript issues](https://github.com/smithy-lang/smithy-typescript/issues)
- In 2021, the creator confirmed "No Python codegen yet"; Python was announced 2023-08 (HN: 2 points). An HN commenter in 2021 noted the Go implementation was "buried within the V2 SDK." — [HN 27081084](https://news.ycombinator.com/item?id=27081084); [HN 37001386](https://news.ycombinator.com/item?id=37001386)
- OpenAPI conversion friction issues: #872 "Allow customizing httpBearerAuth scheme" (2021-07, 11 comments); #825 "How to specify JWT bearer Type in openapi generation" (2021-06); #1082 "New `union` json strategy to generate discriminated unions" (2022-02); #1722 "Is it possible to generate the Swagger definitions using ... aws-models?" (2023-04); #782 "Converting Smithy to GraphQL?" (2021-04); #1170 "Provide Smithy models for default API Gateway errors" (open). mtdowling's 2021 reply to a feature request listed "How would it convert to OpenAPI?" as a design consideration, showing the converter constrains the IDL's evolution. — [#872](https://github.com/smithy-lang/smithy/issues/872), [#825](https://github.com/smithy-lang/smithy/issues/825), [#1082](https://github.com/smithy-lang/smithy/issues/1082), [#1722](https://github.com/smithy-lang/smithy/issues/1722), [#782](https://github.com/smithy-lang/smithy/issues/782), [#1002](https://github.com/smithy-lang/smithy/issues/1002)
- Disney Streaming built smithy-translate for "best-effort" OpenAPI/JSON Schema to Smithy conversion, implying the official direction (Smithy to OpenAPI) was insufficient for brownfield adoption. — [awesome-smithy](https://github.com/smithy-lang/awesome-smithy)
- In 2021 HN, mtdowling on constraint traits: "today it's treated as server-side validation only and not used in client codegen," and on streaming: "We haven't built a WebSockets based protocol yet." — [HN 27081084](https://news.ycombinator.com/item?id=27081084)
- The 2021 thread's example-request issue #102 "Example for Codegen Workflow" (2019-06, 15 comments) is the second most-commented issue in the repo. — [smithy #102](https://github.com/smithy-lang/smithy/issues/102)

### Inferences
- AWS's generator investment tracks AWS SDK needs (clients first, servers later and still marked incomplete), which matches the "AWS adjacent" characterisation in the 2023 Ask HN reply.
- Teams outside AWS that need a server generator have historically built their own (smithy4s, smithy-beam, smithy-dotnet) rather than relying on smithy-rs/smithy-typescript server support.

### Gaps
- No independent experience reports (blog, talk, HN thread) on smithy-rs or smithy-typescript server generation outside AWS were found.
- No community commentary on Smithy's protocol-test (`smithy.test`) suite was found; the only reference is the existence of the official protocol test models.
- smithy-java (71 stars) is too new/small to have community sentiment in the sources found.

---

## Smithy: why teams chose or rejected it

### Takeaway
The recorded reasons for choosing Smithy are "battle-tested at AWS scale", "from AWS so sellable internally", protocol-agnosticism and "elegant, doesn't overreach"; the recorded reasons for rejecting or hesitating are the JVM/Gradle/Kotlin toolchain, daunting codegen internals, thin docs/examples (2021-2023), and the suspicion that it is AWS-shaped.

### Cited Findings
- For: Existenceblinks (HN 2022): "most battle-tested with real and large use case"; could sell it "because it's from AWS." — [HN 34072823](https://news.ycombinator.com/item?id=34072823)
- For: hocuspocus (HN 2024): "I would pick Smithy if I had to go spec-first." — [HN 39833321](https://news.ycombinator.com/item?id=39833321)
- For: RideAndWave (HN 2021, runs a competing project): "Good amazon opened up their stuff, there should be more competition on this front." — [HN 27081084](https://news.ycombinator.com/item?id=27081084)
- Against: thesurlydev (HN 2023): "I've seen the code generation code and it looks daunting," preferring Smithy to OpenAPI to Swagger tooling. — [HN 38546950](https://news.ycombinator.com/item?id=38546950)
- Against: jen20 (HN 2024) on Kotlin/Gradle; johnathon023 (HN 2025) "not tied to Java." — [HN 40206124](https://news.ycombinator.com/item?id=40206124); [HN 46399072](https://news.ycombinator.com/item?id=46399072)
- Against: jabalsad (issue #824, 2021): "the documentation makes getting started incredibly difficult"; garretcharp (2022): "Docs are still very lacking on any real examples." — [smithy #824](https://github.com/smithy-lang/smithy/issues/824)
- Official positioning (i-programmer review, 2023): Smithy "has now become the default choice at Amazon for modeling services." — [i-programmer](https://i-programmer.info/news/90-tools/16651-model-your-apis-with-aws-smithy.html)

### Inferences
- The decision calculus visible in public is "AWS pedigree and model richness vs. JVM toolchain and AWS-centric generator maturity"; teams on the JVM (Scala especially) lean in, others lean to TypeSpec or stay with OpenAPI.

### Gaps
- No engineering blog titled or framed as "why we chose/left Smithy" was found from any company.

---

## TypeSpec: adoption outside Azure

### Takeaway
Named non-Microsoft adopters are few and small: OpenMeter (2024 blog), Pinterest (quoted on the 1.0 GA page and collaborating on a GraphQL emitter), and anonymous HN commenters; Sportradar said it would adopt if AsyncAPI were supported. Microsoft's own claims are about internal breadth (Azure, Microsoft Graph, Microsoft 365 Copilot), not external users.

### Cited Findings
- 1.0 GA announcement (May 2025): no user counts or named adopters except an unattributed "Pinterest Engineering Team" quote ("been searching for something like TypeSpec for years") and Pinterest as collaborator on a GraphQL emitter; refers to a "growing community" without numbers. Stable: `@typespec/compiler`, `@typespec/http`, `@typespec/openapi`, `@typespec/openapi3`, `@typespec/json-schema`, the VS Code extension. Preview: `@typespec/protobuf`, `events`, `rest`, `sse`, `streams`, `versioning`, `xml`, and the C#/JS/Java/Python client and server emitters. — [TypeSpec 1.0 GA blog](https://typespec.io/blog/typespec-1-0-GA-release/); [release notes 2025-05-06](https://typespec.io/docs/release-notes/release-2025-05-06)
- InfoQ (2024-05-15): "actively used within Microsoft, especially by the Azure services and Microsoft Graph teams"; no external adoption figures; Brian Terlson: "eager to get more feedback." — [InfoQ 2024-05](https://www.infoq.com/news/2024/05/typespec/)
- InfoWorld (Simon Bisson, 2024-05-16): "TypeSpec is in wide use inside Microsoft"; Azure-specific docs are "aimed primarily at Microsoft's internal users"; "interesting to see how a wider TypeSpec community evolves." — [InfoWorld](https://www.infoworld.com/article/2337528/designing-and-developing-apis-with-typespec.html)
- OpenMeter (Peter Marton, 2024-11-13) adopted TypeSpec as source of truth with the OpenAPI 3 emitter feeding oapi-codegen, openapi-typescript and Fumadocs; OpenAPI output ~600k characters vs ~200k of TypeSpec; "TypeSpec feels less like writing configuration and more like writing code." Caveat: TypeSpec's own SDK/server emitters "still in progress," so they still rely on OpenAPI-based generators. — [OpenMeter blog](https://openmeter.io/blog/better-openapi-with-typespec)
- Issue #2463 "POC TypeSpec to AsyncAPI" (2023-09, open, 33 comments, labelled Backlog): kennethaasan (2024-05-31): "At Sportradar we use both OpenAPI and AsyncAPI with JSON Schema and would consider TypeSpec if AsyncAPI was supported." — [typespec #2463](https://github.com/microsoft/typespec/issues/2463)
- HN production mentions: lknuth (2024-09): "We facilitate this by having an api-specs repo with TSP and using Pull Requests to discuss"; BillyTheKing (2024-04) used it for a recent API, "a great tool"; dajonker (2024-03): "It's so much better than writing OpenAPI by hand, and I never ever will do that again after working with TypeSpec"; a commenter said their team "built a custom TypeSpec emitter to output an SDK and client libraries." — [HN 41601728](https://news.ycombinator.com/item?id=41601728); [HN 40206124](https://news.ycombinator.com/item?id=40206124); [HN 39833321](https://news.ycombinator.com/item?id=39833321)
- Microsoft 365 Copilot declarative agents use "TypeSpec for Microsoft 365 Copilot," GA 2025-11-25 (internal Microsoft product use). — [Microsoft 365 dev blog](https://devblogs.microsoft.com/microsoft365dev/introducing-typespec-for-microsoft-365-copilot/)
- AutoRest's repo says it is no longer under active development, TypeSpec is the successor, with a retirement date of 2026-07-01. — [Azure/AutoRest](https://github.com/Azure/AutoRest)
- Repo stats 2026-10-08: 5,880 stars, 403 forks, 1,053 open issues, created 2021-06. — GitHub API

### Inferences
- The 1.0 GA page's inability to name adopters beyond Pinterest, a year after a 275-point HN launch, suggests external adoption is real but modest and mostly in small teams that then feed OpenAPI into existing generator pipelines.
- The AsyncAPI gap (open since 2023, "Backlog") is a concrete blocker for event-driven shops.

### Gaps
- No adoption numbers (npm downloads over time, VS Code installs) were gathered.
- Reddit was unreachable.
- GitHub Discussions for microsoft/typespec could not be listed.

---

## TypeSpec: Node/npm requirement, decorator design, OpenAPI-emitter-first positioning

### Takeaway
The Node dependency and `@`-decorator style draw predictable complaints ("you still need node", "Java EE/JAX-RS flashbacks", "hidden languages smuggled in string types"), while Microsoft explicitly positions TypeSpec as an API-first design language that interoperates with rather than replaces OpenAPI; independent writers (Nordic APIs, Speakeasy, apichangelog) mostly accept that framing, and one technical writer (passo.uno) argues it recentralises API design on developers.

### Cited Findings
- Official: bterlson (HN 2024-04): "TypeSpec is designed primarily as an API first tool as opposed to being an output" and "We're not trying to replace OpenAPI, OpenAPI is great in many ways"; on why a new language rather than a TypeScript dialect: "HTTP needs a lot of metadata that isn't found in TypeScript"; "We first tried a TypeScript DSL and I really wish it would have worked for our use cases." On OpenAPI 3.0 targeting: "the ecosystem doesn't support 3.1 broadly yet." — [HN 40206124](https://news.ycombinator.com/item?id=40206124); [HN 39833321](https://news.ycombinator.com/item?id=39833321); [HN 41601728](https://news.ycombinator.com/item?id=41601728)
- Official (bterlson's own blog, 2024-03-25): "Humans don't find OpenAPI particularly pleasant to author and review"; "The code generation from OpenAPI is often not stellar"; Azure uses "dozens of shared templates" (e.g. long-running operations); admitted gaps: callbacks unsupported, examples and XML in progress, pagination needs work, object unions map to `anyOf` which "draws criticism from OpenAPI purists." — [blog.trl.sn](https://blog.trl.sn/blog/typespec-for-openapi/)
- Node: kkukshtel (HN 2024-04) complained TypeSpec, CUE, Pkl and Dhall depend on Node or other runtimes and "wished for a C core with language bindings"; crowdyriver (HN 2024-09): "you still need node to run this thing, so you don't avoid the node dependency." Microsoft (InfoQ): "We also use JavaScript for our extensibility and packaging story." — [HN 40206124](https://news.ycombinator.com/item?id=40206124); [HN 41601728](https://news.ycombinator.com/item?id=41601728); [InfoQ](https://www.infoq.com/news/2024/05/typespec/)
- Decorators/syntax: activitypea (HN 2024-09): "The syntax leaves a bad first impression. I'm getting flashbacks to Java EE/JAX-RS days."; mdaniel: "I despise 'hidden' languages smuggled in string types."; bananaquant: "Design by committee?"; one commenter said `@` annotations are "moving backwards from code-first frameworks like NestJS." — [HN 41601728](https://news.ycombinator.com/item?id=41601728); [HN 40206124](https://news.ycombinator.com/item?id=40206124)
- "Yet another DSL": verdverm: bespoke languages are "a hard sell"; Alupis: "it's difficult for the team to justify yet another new DSL"; paxys: "do we really need another standard? Just use OpenAPI directly folks."; otabdeveloper4: "these bespoke solutions that add nothing but DSL syntactic sugar are dead on arrival."; Etheryte: "OpenAPI is battle tested and has all the tooling I could ever dream of"; hoppersftw: "NIH for Microsoft"; gslin linked XKCD 927. — [HN 40206124](https://news.ycombinator.com/item?id=40206124); [HN 41601728](https://news.ycombinator.com/item?id=41601728)
- Independent critique (passo.uno, technical writer, 2024-09-02): the claim that OpenAPI files should be "automatically generated artifacts and nothing more" is "stupefying"; documentation decorators are "incredibly limited compared to OpenAPI"; TypeSpec looks like "an invitation for developers to define APIs somewhere else that isn't YAML" and "wrestling back control of API development from other collectives"; concedes "more accurate data types" and IDE support. — [passo.uno](https://passo.uno/typespec-openapi-api-design/)
- Nordic APIs (Chris Wood, Ozone API, 2024-05-08): TypeSpec "does not" aim to replace OpenAPI; it bridges design-first tools and the OpenAPI toolchain; quotes Gareth Jones (Microsoft) on "hundreds of thousands to ... millions of lines of consolidated OAS ... I just can't work with that sort of information." — [Nordic APIs](https://nordicapis.com/tis-but-a-scratch-does-typespec-reignite-the-specification-wars/)
- Nordic APIs (Kristopher Sandoval, 2025-08-12) comparison table: TypeSpec "No" for quick handwritten specs, complex integrations, and non-technical stakeholders editing; cons: "smaller ecosystem," gRPC emitter "early," "heavily Microsoft-centric, with limited third-party community resources"; verdict: "an addendum to OpenAPI, not a replacement." — [Nordic APIs](https://nordicapis.com/openapi-vs-typespec-which-to-use/)
- Speakeasy (undated): "writing OpenAPI documents by hand is complex, tedious, and error-prone"; TypeSpec "does not yet support webhooks or callbacks"; it is for designing APIs that don't exist yet, less suited to documenting existing ones. — [Speakeasy](https://www.speakeasy.com/openapi/frameworks/typespec)
- apichangelog (Bruno Pedro, 2025-06-13): "TypeSpec feels like a glove" to developers; "TypeSpec is not perfect" but shortcomings "bearable" (none named). — [apichangelog](https://apichangelog.substack.com/p/can-typespec-be-an-alternative-to-openapi)
- Open design issue: #826 "Invalid OA3 for unions" (2022-08, bterlson, still open, 19 comments) on `oneOf`/`allOf` emission. — [typespec #826](https://github.com/microsoft/typespec/issues/826)

### Inferences
- The "OpenAPI-first" positioning is a deliberate adoption strategy (keep existing generators) and it works for teams like OpenMeter, but it also means TypeSpec's value is capped by what OpenAPI 3.0 can express (unions, callbacks, pagination), which is exactly what bterlson lists as pain points.

### Gaps
- No Reddit sentiment.
- No source quantifies how often TypeSpec users drop it after the design phase vs keep it as source of truth (Speakeasy mentions both patterns).

---

## TypeSpec: versioning support, playground and LSP quality, 1.0 reception

### Takeaway
Versioning has been a recurring confusion point since the Cadl era (`@versionedDependency` "confusing" per Azure's own review board, 2022; explicit API-version parameter still open since 2024; projections deprecated for mutators in 0.65, Feb 2025) and `@typespec/versioning` remained preview at 1.0; the VS Code extension and playground are praised and part of the stable set; the 1.0 release itself drew almost no HN attention (RC thread: 3 points, 2 comments).

### Cited Findings
- Issue #857 "versionedDependency decorator is confusing" (2022-08, closed, 22 comments): from Azure's ARB review; the tuple syntax existed because "we can't have enum member as model keys." — [typespec #857](https://github.com/microsoft/typespec/issues/857)
- Issue #244 "[Versioning] OptionalProperties/UpdatableProperties causes versioning metadata to be ignored" (2022-02, 22 comments). — [typespec #244](https://github.com/microsoft/typespec/issues/244)
- Issue #2819 "Have an explicit way of defining the api version parameter" (2024-01, open, 17 comments): emitters assume a parameter named `api-version`; p-bakker (2025-05): with URL-path versioning "typespec emits a yaml file per version and between the different yamls there are duplicate routes"; adminy (2025-06): "Any updates on this?" — [typespec #2819](https://github.com/microsoft/typespec/issues/2819)
- Issue #1953 (2023): splitting a versioned service across files raised `using-versioned-library` with "the complete lack of source information on the error, making it extremely hard to track down." — [typespec #1953](https://github.com/microsoft/typespec/issues/1953)
- 0.65 (Feb 2025): compiler projections deprecated (removal planned 0.66); "versioning projection was deprecated in favor of the mutator approach" (`getVersioningMutators` + `unsafe_mutateSubgraphWithNamespace`). The published `@typespec/versioning` 0.84.0 README still documented `buildVersionProjections`. — [TypeSpec 0.65 release notes](https://typespec.io/release-notes/typespec-0-65/); [versioning README 0.84.0](https://unpkg.com/@typespec/versioning@0.84.0/README.md)
- 1.0 GA: `@typespec/versioning` listed as preview, not stable. — [1.0 GA blog](https://typespec.io/blog/typespec-1-0-GA-release/)
- 1.0 breaking change: `@patch` no longer applies "implicit optionality" by default. — [release notes 2025-05-06](https://typespec.io/docs/release-notes/release-2025-05-06)
- IDE/LSP: Nordic APIs notes "an NPM CLI, Visual Studio and VS Code plugins"; OpenMeter cites "IDE support, linters, and compilation"; issue #401 (2022) asked for emitters to be opt-out of running in the language server; #5902 (2025-02, 19 comments) "Need to be able to run the standalone typespec version from the IDE"; #5385 (2024-12) VS Code server-emitter floating peer-dependency versions. — [Nordic APIs](https://nordicapis.com/tis-but-a-scratch-does-typespec-reignite-the-specification-wars/); [typespec #5902](https://github.com/microsoft/typespec/issues/5902); [typespec #401](https://github.com/microsoft/typespec/issues/401)
- 1.0 reception on HN: "TypeSpec 1.0-RC" (2025-04-03) 3 points, 2 comments. No HN thread for GA was found. — [HN 43573149](https://news.ycombinator.com/item?id=43573149)
- Other open/large issues at 1.0: #4903 ".NET Client Patterns" (2024-10, 20 comments, open); #8455 "[TCGC] Support Discriminated Unions in all client emitters" (2025-09, 20 comments, open); #2378 SSE support (closed 2023); #2970 Simple XML support (closed). — [typespec issues](https://github.com/microsoft/typespec/issues)

### Inferences
- Versioning is the one core concern where TypeSpec's design is still visibly in flux five years in; a team relying on it would be on a preview library whose API changed in 2025.
- The 1,053 open issues (vs Smithy's 94) reflect both a larger surface (many emitters, Azure-driven client generators) and Microsoft's use of the public tracker for internal backlog.

### Gaps
- No independent review of the playground (typespec.io/playground) quality was found; only official and adopter mentions of IDE support.
- Whether projections were actually removed in 0.66 was not confirmed.

---

## TypeSpec: HN/Reddit/GitHub praise and complaints (consolidated)

### Takeaway
Praise clusters on conciseness and TypeScript familiarity ("so much better than writing OpenAPI by hand"); complaints cluster on new-DSL fatigue, Node, decorator syntax, Microsoft trust/dogfooding, missing AsyncAPI/GraphQL/streaming, and OpenAPI purists' objections to `anyOf` unions.

### Cited Findings
- Praise: dajonker "never ever will do that again"; mnahkies: adding TypeSpec input to their OpenAPI generator was "pretty painless"; domoritz: "definitely beats writing JSON schema by hand"; aleksiy123 likes validation, closest alternative CUE; mooreds has a ">12k lines" spec and wants a better format; tholm: "Having to hand cut OpenAPI specs is gross"; mmcclure: maintaining YAML OpenAPI is "extremely annoying and tedious"; jitl: "our OpenAPI YAML needs to be about 1.2MB." — [HN 39833321](https://news.ycombinator.com/item?id=39833321); [HN 40206124](https://news.ycombinator.com/item?id=40206124); [HN 41601728](https://news.ycombinator.com/item?id=41601728)
- Complaints: DanielHB "Codegen is annoying and error prone" and TypeScript is "too powerful" to map onto OpenAPI; darylteo: spec-first "creates drift and CI/CD nightmares"; cjonas: new languages lose because LLMs are trained on existing ones; mbrock: the launch post read like "GPT-4 output"; pjmlp doubted Microsoft's dogfooding claims; schnable asked about async/event-driven APIs with no answer; drewda: GraphQL output "didn't actually get around to" being added (bterlson, Sept 2024: "It is being worked on."). — [HN 40206124](https://news.ycombinator.com/item?id=40206124); [HN 41601728](https://news.ycombinator.com/item?id=41601728)
- Protobuf comparison in favour of TypeSpec: codeflo "Unions, generics, required fields, default values, enums that don't pollute the global namespace"; frankrobert: protobuf3's removal of required fields is a dealbreaker for typed frontends. — [HN 41601728](https://news.ycombinator.com/item?id=41601728); [HN 40206124](https://news.ycombinator.com/item?id=40206124)
- Process views: ivan_gammel: spec-first "works fine in the long run" but needs process maturity; seer: "Schema first allows for great communication between teams"; Petermarcu: gRPC users are "100% fine with schema first." — [HN 40206124](https://news.ycombinator.com/item?id=40206124); [HN 39833321](https://news.ycombinator.com/item?id=39833321)
- Cadl-era issue #1457 "cadl compile failed when package.json doesn't include all emitters defined in cadl-project.yaml" (2022-12, 32 comments) shows early npm-packaging friction. — [typespec #1457](https://github.com/microsoft/typespec/issues/1457)

### Inferences
- The TypeSpec debate on HN is really a debate about design-first vs code-first and about OpenAPI YAML ergonomics; TypeSpec wins among people already committed to design-first.

### Gaps
- Reddit unreachable; no r/programming, r/webdev, r/rust or r/java threads were retrieved.

---

## Direct comparisons: TypeSpec vs Smithy, and both vs OpenAPI/Protobuf

### Takeaway
No substantive, independent head-to-head article comparing Smithy and TypeSpec was found; the comparisons that exist are HN comments (toolchain language, extensibility, breadth of generators), a Nordic APIs session framing both as "next generation" DSLs, and a Korean GeekNews thread asking the question without an answer. Vendor comparisons (Speakeasy, Fern, Stainless) are about SDK generators and mostly ignore both languages.

### Cited Findings
- Petermarcu (HN 2024-03-28): "TypeSpec and Smithy live in a similar space here. You can generate OpenAPI specs from them, or you can generate assets directly from them." — [HN 39833321](https://news.ycombinator.com/item?id=39833321)
- joelwilsson (HN 2024-04-30): "Looks like a competitor/alternative to Smithy"; nicce: Smithy "can already generate much more than TypeSpec"; jen20: TypeSpec's "lack of Kotlin and Gradle is a significant advantage over Smithy"; gjmveloso (2024-09): "Smithy sounds more balanced and extendable." — [HN 40206124](https://news.ycombinator.com/item?id=40206124); [HN 41601728](https://news.ycombinator.com/item?id=41601728)
- johnathon023 (HN 2025-12-27): TypeSpec/Alloy is "similar to smithy.io, but more extensible, not tied to Java, and built with Typescript in mind." — [HN 46399072](https://news.ycombinator.com/item?id=46399072)
- Gareth Jones (Microsoft) at Nordic APIs Austin API Summit, 2024-03-13: "Didn't the API description wars end in 2017 when we all agreed that OAS was the way forward?"; names "Smithy from Amazon and TypeSpec, open sourced by Microsoft" as DSLs that "move us back to a more intentional approach to design." — [Nordic APIs session](https://nordicapis.com/sessions/reigniting-the-api-description-wars-with-typespec-and-the-next-generation-of-api-languages/)
- GeekNews (Korean HN mirror, 2024-05-02) carries a TypeSpec-vs-Smithy question with one comment and no substantive answer. — [news.hada.io](https://news.hada.io/topic?id=14607)
- StackShare has a "OpenAPI Specification vs Smithy" page with no substantive content beyond descriptions. — [StackShare](https://stackshare.io/stackups/openapi-specification-vs-smithy)
- Speakeasy's vendor comparisons describe Fern as "built on top of a DSL" (its own Fern Definition) and Stainless as relying on "a custom DSL known as the Stainless config," versus Speakeasy being "OpenAPI-native"; Fern says it accepts OpenAPI, AsyncAPI, OpenRPC, gRPC/protobuf, GraphQL or its own format and can `fern export` back to OpenAPI. A Fern page claims Anthropic acquired Stainless in May 2026 and is winding down the hosted generator (unverified). — [Speakeasy vs Fern](https://speakeasy.com/blog/speakeasy-vs-fern); [Speakeasy vs Stainless](https://speakeasy.com/post/speakeasy-vs-stainless); [Fern: leave Stainless](https://buildwithfern.com/lp/leave-stainless)
- 2021 HN Smithy thread comparisons: aszen: Smithy "is resource based rather than message based" (vs gRPC); mtdowling: "Smithy is protocol agnostic (like gRPC actually)"; dexwiz (2024): "I'd rather have generated xml than generate yaml" (WSDL); paulddraper: TypeSpec "is closer to WSDL+SOAP and will rise or fall with the JSON and protobuf formats it describes." — [HN 27081084](https://news.ycombinator.com/item?id=27081084); [HN 40206124](https://news.ycombinator.com/item?id=40206124)
- Nordic APIs published a Smithy overview on 2024-06-13 (listing only; article text not retrieved). — [Nordic APIs design category](https://nordicapis.com/category/design/page/13)

### Inferences
- The absence of a serious comparison article after two years of both being public suggests the audiences barely overlap: Smithy's public users are JVM/Scala and AWS-adjacent; TypeSpec's are TypeScript/OpenAPI-pipeline teams.
- SDK vendors (Fern, Stainless) chose to define their own DSLs rather than adopt Smithy or TypeSpec, which is itself a data point on the "build your own" question.

### Gaps
- No article from Stripe, Shopify, Twilio, or API consultancies directly comparing Smithy, TypeSpec, OpenAPI and Protobuf was found.
- The Nordic APIs Smithy overview (2024-06-13) text was not retrieved.

---

## Precedents: companies that built their own API modelling language

### Takeaway
Palantir's Conjure (2018) is the clearest precedent: a deliberately constrained YAML IDL with multi-language generators that the authors kept because "the constraints it imposes on API design ... produce focus and clarity"; its HN reception was "a step backwards from Thrift or Protobufs?" and it remains a Palantir-only ecosystem (492 stars, still pushed in 2026). Stripe uses an internal Ruby DSL that emits OpenAPI, Google uses protobuf plus AIPs, Twilio publishes OpenAPI, and Microsoft's own Cadl replaced AutoRest's Swagger-first pipeline.

### Cited Findings
- Palantir Conjure (blog 2018): built "to drive consistency across our breadth of HTTP/JSON APIs and to make it easier to define, discuss, review and collaborate on those APIs"; "from a declarative API definition written in YAML, Conjure generates client and server bindings ... Java, Typescript and Python"; they first planned it "as a stepping-stone to gRPC" but kept it because of "the constraints it imposes on API design, since they produce focus and clarity"; "defines hundreds of APIs across Palantir's products." — [Palantir blog (summary via search)](https://blog.palantir.com/introducing-conjure-palantirs-toolchain-for-http-json-apis-2175ec172d32); [Conjure concepts](https://github.com/palantir/conjure/blob/master/docs/concepts.md)
- HN on Conjure (2018-11-20, 61 points, 10 comments): 0l0rin: "At a first glance it seems like a step backwards from already established solutions like Thrift or Protobufs?" and "What exactly is the problem that Conjure is solving better than its existing alternatives?" Palantir's markelliot: "When we started on this tooling there really weren't great gRPC options for the browser"; they rejected Swagger/OpenAPI because "it focused more on the full coverage of any kind of HTTP/JSON API"; "JSON is obviously not as compact as Protobuf or Thrift" but serialization is rarely the bottleneck and a common format "helps operability." — [HN 18494628](https://news.ycombinator.com/item?id=18494628)
- Conjure repo 2026-10-08: 492 stars, 77 forks, last push 2026-10-08. A 2024 resubmission to HN got 1 point. — GitHub API; [HN 41604642](https://news.ycombinator.com/item?id=41604642)
- Stripe (brandur, 2021-04-25): source of truth is "a DSL built in Ruby code around the API server's implementation"; a script "walks the DSL and dumps its structure to OpenAPI" in three variants (internal, Dashboard, public); "A check in CI makes sure that OpenAPI specs on mainline are accurate"; "A JavaScript program reads OpenAPI and uses it to codegen API library code in all support languages"; before this "one person kept seven language libraries current ... This was never sustainable"; off-the-shelf tools were rejected to "maintain compatibility with the existing versions"; the author "doesn't yet know what someone should do today to build something similar at lower cost." — [brandur.org](https://brandur.org/fragments/stripe-codegen)
- Stripe's public spec repo carries an expanded internal variant with "special annotations, deprecated endpoints, and pre-release features intended to support generating Stripe API libraries" (e.g. `x-expandableFields`); as of 2026-01-28 the specs cover v1 and v2 namespaces in one file. — [Stripe changelog 2026-01-28](https://docs.stripe.com/changelog/clover/2026-01-28/openapi-with-v2-apis)
- Google: "Nearly all Google APIs are described with the Protocol Buffers language"; AIPs are Google-wide conventions so teams can "build lots of tools that 'just work' across AIP-compliant APIs"; AIP-8 requires guidance to benefit clients such as SDKs and IaC tooling. — [google.aip.dev](https://google.aip.dev/); [AIP-8](https://google.aip.dev/8); [Nordic APIs on AIPs](https://nordicapis.com/api-improvement-proposals-googles-take-on-the-api-style-guide/)
- Twilio: OpenAPI spec beta April 2021, GA March 2023, public repo twilio-oai; the sources do not describe the internal source it is generated from. — [Twilio GA post](https://www.twilio.com/en-us/blog/products/launches/introducing-twilios-openapi-specification-ga); [twilio-oai](https://github.com/twilio/twilio-oai)
- Microsoft: Cadl ("Concise API Design Language") built "on concepts from both TypeScript and Bicep," renamed TypeSpec in the 2023-03-13 release; AutoRest (the Swagger-first SDK generator) is deprecated with retirement 2026-07-01 and TypeSpec named as successor; Azure SDK packages are migrated when a `tsp-location.yaml` replaces `src/autorest.md`. — [InfoWorld](https://www.infoworld.com/article/2337528/designing-and-developing-apis-with-typespec.html); [Cadl to TypeSpec migration notes](https://typespec.io/docs/release-notes/cadl-typespec-migration); [Azure/AutoRest](https://github.com/Azure/AutoRest)
- Smithy itself is an own-IDL precedent: mtdowling (2021) described it as "basically the open source v2" of Amazon's earlier internal IDLs "but with a public spec and tooling"; jonhohle: "Amazon has done this internally across several different iterations and it's fantastic." — [HN 27081084](https://news.ycombinator.com/item?id=27081084)
- Fern and Stainless (SDK vendors) each built their own definition DSLs while importing OpenAPI; Figma moved off its custom Skew language to TypeScript (a cautionary data point on bespoke languages, though not an API IDL). — [Speakeasy vs Fern](https://speakeasy.com/blog/speakeasy-vs-fern); [Simon Willison on Figma](https://feeds.simonwillison.net/2024/May/4/figmas-journey-to-typescript)
- HN commenter verdverm (2024) on bespoke languages: "a hard sell" because people won't learn them and "the ecosystem tooling is costly to build"; cjonas: new languages are disadvantaged because LLMs are trained on existing ones. — [HN 40206124](https://news.ycombinator.com/item?id=40206124)

### Inferences
- Every documented own-IDL (Conjure, Stripe's Ruby DSL, Smithy, Cadl) was justified by the same two things: control over a constrained, consistent API style, and a generator pipeline the company already owned. None gained a community outside its home company except Smithy (marginally, via Scala) and TypeSpec (modestly).
- The consistent external cost is the one Conjure's HN thread and brandur's post both name: outsiders ask "why not Thrift/protobuf/OpenAPI," and the pipeline "isn't reusable elsewhere."
- All of them kept OpenAPI as an export target for interoperability (Stripe, Twilio, TypeSpec, Smithy, Fern), which suggests any new language should treat OpenAPI emission as table stakes rather than a differentiator.

### Gaps
- The Palantir blog post itself returned HTTP 403 and the Conjure README 404'd on both `master` and `develop`; findings rely on the search summary, the HN thread, and the concepts page.
- No Shopify source was found (Shopify's public APIs are GraphQL-first; no IDL-choice write-up surfaced).
- No Twilio source describes its internal definition format.
- No retrospective from Palantir on Conjure's long-term costs was found.
