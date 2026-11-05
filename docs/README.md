# Crucible documentation

Use this index to find the guide for your task. Start with the
[repository overview](../README.md) if you are new to Crucible.

## Getting started

- [Your first Rust service](quickstart.md): install the CLI, create a service, and change its model.
- [The Crucible framework](framework.md): service layout, handlers, middleware, testing, and hosting.
- [Client setup and conformance](clients.md): generate clients and configure their shared behavior.
- [Building named model outputs](build-outputs.md): produce several SDKs, documents, or infrastructure packages from one model.

## Modeling an API

- [Language reference](idl/reference.md): `.cru` syntax, types, traits, operations, and resources.
- [API policy and compatibility](api-policy.md): configure conventions and check contract changes.
- [Serving older API versions](api-versions.md): freeze releases, define migrations, and serve versioned APIs.
- [API versions and clients](api-versions-clients.md): generate clients and references for supported releases.

## Building and operating services

- [Authentication](auth.md): configure bearer tokens, JWTs, and API keys.
- [Typed permissions](permissions.md): model and enforce operation permissions.
- [Idempotency record policies](idempotency-receipts.md): replay and receipt behavior.
- [Response streams](streaming.md): model, produce, and consume typed response sequences.
- [Serving MCP](mcp.md): expose modeled operations as MCP tools.
- [Crucible Explorer](explorer.md): enable the local API explorer.

## Generating clients and other outputs

- [Client guides](clients/): language-specific setup, APIs, and migration notes.
- [Service CLI](clients/cli.md): generate a standalone command-line client.
- [OpenAPI generation](openapi.md): export an OpenAPI 3.1 document.
- [JSON Schema export](json-schema.md): export JSON Schema 2020-12 definitions.
- [Service API reference documentation](docs.md): generate Markdown, HTML, and agent-readable references.
- [Infrastructure packages](infrastructure.md): generate CDK constructs and Terraform/OpenTofu providers.

## Packaging and releasing

- [SDK releases](sdk-releases.md): declare, generate, and release SDK packages.
- [SDK packaging and releases design](sdk-packaging.md): packaging architecture, rollout phases, and design decisions.
- [SDK runtime policy](sdk-runtime-policy.md): shared retry and telemetry behavior.
- [SDK feature completion](sdk-completion.md): current cross-language feature status.
- [Releasing Crucible](releases.md): release and stability procedures for this repository.
- [Publishing Crucible](open-source.md): repository publication policy.

## Upgrading and understanding internals

- [Migrating between releases](migrating.md): upgrade instructions for Crucible releases.
- [Rust runtime stability](stability.md): supported service, client, and generated-code boundaries.
- [Design and wire protocol](design.md): the model-to-IR pipeline and Crucible JSON rules.
- [Language design decisions](idl/decisions.md): the rationale behind language and IR behavior.
- [Language construct inventory](idl/inventory.md): implementation coverage for language constructs.
- [Language research](idl/research_notes/): background research and source notes.
