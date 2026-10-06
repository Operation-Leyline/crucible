# Releasing Crucible

Crucible versions are released manually as GitHub tags (`vX.Y.Z`) and releases.
The framework and generators share `[workspace.package].version`. Package
registry publication is separate; this workflow publishes no registry packages.

The [0.2.0 release](https://github.com/Operation-Leyline/crucible/releases/tag/v0.2.0)
introduces the middleware/interceptor pipeline and stricter wire validation.
See [Migrating to Crucible 0.2](migrating-0.2.md) before updating consumers.

PRs keep all Rust generator unit/golden tests, formatting, lint, and tooling
tests. Framework/server changes also run TypeScript and the real HTTP harness.
Shared model changes add Python and Go as representative native consumers.
Changes to an individual generator run its affected Linux consumer, using one
runtime version. Main pushes keep Rust and HTTP integration checks.

The manual release runs every language, supported runtime version, Windows,
Apple/native targets, and the Android emulator. There is no scheduled full run.
Failures found only by those checks can block a release after PRs have merged.
A failed qualification never publishes a version tag or release.

## Preparing a version

Start with a clean, current main checkout. Choose a stable version greater than
the current workspace version; for example, from `0.2.0` to `0.2.1`:

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
git add Cargo.toml Cargo.lock e2e/Cargo.lock crates/*/tests/golden
git commit -m "chore: prepare Crucible 0.2.1"
git push -u origin claude/release-0.2.1
gh pr create --base main --title "Release Crucible 0.2.1" --body "Bump the shared workspace version, lock entries, and generator provenance metadata to 0.2.1."
```

Review and merge the PR through the normal process before starting publication.

## Publishing the merged version

Run **Release Crucible** from the GitHub Actions page, select `main`, and enter
the merged version. The equivalent CLI command is:

```sh
gh workflow run release.yml --ref main -f version=0.2.1
gh run list --workflow release.yml --limit 1
```

Preflight rejects other branches, a mismatched manifest/lockfile version, and
an existing tag. The full matrix tests the exact commit selected when the run
starts. After it passes, publication creates that commit's tag atomically and
adds a GitHub release with generated notes and source archives. Later merges
do not change the selected commit. A run starting or completing qualification
does not prove publication; check the publish job and GitHub release.

If creating the GitHub release fails after its tag was created, inspect the
tag's commit and the successful qualification before completing the existing
release manually. Do not move or reuse a published version tag.
