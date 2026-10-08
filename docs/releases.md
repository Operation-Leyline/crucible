# Releasing Crucible

Crucible versions are released manually as GitHub tags (`vX.Y.Z`) and releases.
The framework and generators share `[workspace.package].version`. Package
registry publication is separate; this workflow publishes no registry packages.

Every release has an entry in the [CHANGELOG](../CHANGELOG.md). Upgrading
consumers start at [Migrating between releases](migrating.md).

## Stability policy for 0.x

Crucible is `0.x`: the minor number plays the role of a major version.

| Release | May change the stable surface? |
|---|---|
| Patch (`0.4.0` → `0.4.1`) | Never. Bug fixes, additive API, diagnostics for input that was already invalid, regenerated output that keeps the same API. |
| Minor (`0.4.x` → `0.5.0`) | Yes, but only with a CHANGELOG **Migration** entry that says what to change, and a `#[deprecated]` shim for the old item kept for one minor release (removed no earlier than the next minor). |

The **stable surface** is what a service or client author writes against:

- the public API of `crucible-server-runtime-rust`, `crucible-wire`,
  `crucible-client-runtime-rust`, `crucible-auth` and `crucible-build`;
- the Rust API that generated server and client bindings expose (type, trait
  and method names and signatures; not the bodies);
- the `crucible new` starter and the `crucible` CLI's documented commands;
- the Smithy traits shipped by `crucible-traits` (`smithy/`), and the
  documented behavior of the framework (phase order, error shapes, wire
  semantics).

Exempt from the policy, and free to change in any release:

- everything under a `__generated` or `__private` module or path: these are
  `pub` only so generated code and macros can reach them;
- the per-language generator `Options` structs (`crucible-codegen-*`), which
  follow their target ecosystem's conventions and change with the generator;
- generated text itself (golden files, formatting, private names), as long as
  the stable Rust API above is preserved;
- crates not listed above (`crucible-smithy`'s IR, `crucible-codegen*`
  internals, `crucible-policy`) and anything documented as experimental.

A change that moves the stable surface needs, in the same PR: the `#[deprecated(since = "…", note = "use … instead")]`
shim, a CHANGELOG entry under *Deprecated* (and *Migration* when consumers must
act), and a section in [docs/migrating.md](migrating.md). The shim is removed
in the following minor release, with a *Removed* entry. Removing a public item
without a shim is allowed only in a minor release whose Migration entry names
it, and should be the exception.

This policy should be cross-linked from `docs/stability.md` once the runtime ABI
boundary (`__generated`, `check_abi!`) lands.

## What qualifies a change

PRs keep all Rust generator unit/golden tests, formatting, lint, and tooling
tests. Framework/server changes also run TypeScript and the real HTTP harness.
Shared model changes add Python and Go as representative native consumers.
Changes to an individual generator run its affected Linux consumer, using one
runtime version. Main pushes keep Rust and HTTP integration checks.

### Consumer compatibility

The `consumers` job builds three things the way a downstream service does, one
OS, one shared Cargo cache (`tools/test-consumers.py`):

1. the starter produced by `crucible new`, built against this checkout;
2. `e2e/weather-service` (model, service, Rust client and the generated
   `smithy.test` conformance suite);
3. a Leyline-shaped fixture (`tests/consumers/leyline`): a separate model crate;
   `AuthenticatorChain` and scope checks from `crucible-auth`; a custom async
   `Middleware` and `OperationAwareMiddleware`; an `Interceptor`; a modelled
   `ValidationException` with typed reasons; `host::lambda_event`; and a
   generated Rust SDK package built by a separate crate.

It exists to fail on a change that compiles inside this workspace but breaks
what real services use. The CI planner selects it on PRs that touch
`crates/crucible-build`, `crates/crucible-server-runtime-rust`,
`crates/crucible-wire`, `crates/crucible-codegen-rust`, `crates/crucible-auth`
or `smithy/`, and always when the manual release qualifies a commit. When a
change breaks the fixture, fix the fixture only if the break is allowed by the
policy above and has a Migration entry; otherwise fix the change.

The manual release runs every language, supported runtime version, Windows,
Apple/native targets, and the Android emulator. There is no scheduled full run.
Failures found only by those checks can block a release after PRs have merged.
A failed qualification never publishes a version tag or release.

## Preparing a version

Start with a clean, current main checkout. Choose a stable version greater than
the current workspace version; for example, from `0.2.0` to `0.2.1`.

First give the version a [CHANGELOG](../CHANGELOG.md) entry: a
`## [0.2.1] - YYYY-MM-DD` heading with the Keep-a-Changelog groups that apply
(*Added*, *Changed*, *Deprecated*, *Removed*, *Migration*). Per the stability
policy, a minor release that moves the stable surface must have a *Migration*
entry, and a patch release must not. `prepare` and `check` refuse a version
without an entry, so a release cannot be tagged without notes.

```sh
git switch -c claude/release-0.2.1
python tools/crucible_release.py prepare 0.2.1
git diff -- Cargo.toml Cargo.lock e2e/Cargo.lock
python tools/crucible_release.py check 0.2.1
```

The preparation helper updates the workspace version and both lockfiles. It
preserves registry dependency versions and example-service package versions.
It does not update checked-in generator golden files. Generated package
metadata also records the Crucible version: refresh those goldens before
committing a version bump, and review the diff to confirm that only intended
output changed:

```sh
CRUCIBLE_BLESS=1 cargo test --locked
git diff -- crates
```

Commit the version bump and reviewed golden changes together, then open a PR:

```sh
git add CHANGELOG.md Cargo.toml Cargo.lock e2e/Cargo.lock crates/*/tests/golden
git commit -m "chore: prepare Crucible 0.2.1"
git push -u origin claude/release-0.2.1
gh pr create --base main --title "Release Crucible 0.2.1" --body "Bump the shared workspace version, lock entries, and generator provenance metadata to 0.2.1."
```

Review and merge the PR through the normal process before starting publication.

### Checklist: stable-surface diff

Nothing automated compares the public API with the previous tag: a
`cargo public-api` diff needs a nightly toolchain for rustdoc JSON, a pinned
tool install and an allowlist for the exempt `__generated`/`__private` paths,
and it cannot tell a `#[deprecated]` shim from a removal without parsing
attributes, which is more upkeep than a PR gate justifies. Until that is
cheap, do this by hand when preparing a minor release (and skim it for a
patch, which must show no removals):

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-public-api --locked
for crate in crucible-server-runtime-rust crucible-wire crucible-client-runtime-rust crucible-auth crucible-build; do
  cargo public-api --package "$crate" diff v0.4.0..HEAD
done
```

Replace `v0.4.0` with the previous tag. Every *removed* or *changed* line must
be exempt (`__generated`, `__private`), carry a `#[deprecated]` shim from the
previous release, or appear under *Migration* in the CHANGELOG. Record the
result in the release PR.

## Publishing the merged version

Run **Release Crucible** from the GitHub Actions page, select `main`, and enter
the merged version. The equivalent CLI command is:

```sh
gh workflow run release.yml --ref main -f version=0.2.1
gh run list --workflow release.yml --limit 1
```

Preflight rejects other branches, a mismatched manifest/lockfile version, a
version without a CHANGELOG entry, and an existing tag. The full matrix tests
the exact commit selected when the run starts, including the `consumers` job
(see [Consumer compatibility](#consumer-compatibility)); the `ci` aggregate and
the publish job both require it, so a tag is never created for a commit whose
starter, example service or Leyline-shaped fixture fails to build. After it passes, publication creates that commit's tag atomically and
adds a GitHub release with generated notes and source archives. Later merges
do not change the selected commit. A run starting or completing qualification
does not prove publication; check the publish job and GitHub release.

If creating the GitHub release fails after its tag was created, inspect the
tag's commit and the successful qualification before completing the existing
release manually. Do not move or reuse a published version tag.
