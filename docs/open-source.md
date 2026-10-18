# Open-source and crates.io publication checklist

Work through this list in order. Nothing here is done by merging the PR that
added it: each box is an owner decision or a follow-up change. The audit that
produced it is in that PR's description.

Check items off in a tracking issue, not in this file.

## 1. Decide

- [x] **Licence.** Pick the SPDX expression (`Apache-2.0` recommended;
      `MIT OR Apache-2.0` is the usual alternative). Add `LICENSE` (and
      `LICENSE-MIT` if dual) at the repository root.
- [ ] **Copyright holder** for the licence header and `NOTICE`.
- [x] **Leyline and Pigeon references.** Decide keep, generalise or remove for
      each item in the audit's reference table.
- [x] **History.** Publish the existing history, or start a fresh public
      repository from a squashed tree. Branches, tags, `refs/pull/*` and
      closed pull requests are all public once the repository is.
- [x] **Names and trademark.** Confirm the crate names and whether the name
      "Crucible" is acceptable (Atlassian ships an unrelated product of that
      name; the crates.io names `crucible`, `crucible-core` and `crucible-cli`
      are taken by unrelated projects). Decided: the executable is `crucible`,
      shipped as prebuilt binaries; `crucible-cli` stays unpublished
      (`publish = false`); `crucible-api` is a new thin library crate.

## 2. Prepare the repository

- [ ] Add `LICENSE`, `NOTICE`, `SECURITY.md`,
      `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`.
- [x] Replace links to private repositories (`crucible-service-template`, `crucible-actions`) or make those public.
- [x] Remove or generalise `examples/infra/leyline.bindings.json` and the
      Leyline/Pigeon prose in `README.md`, `AGENTS.md`, `docs/`.
- [ ] Remove `.claude`-style or agent scratch files if any appear (none are
      committed today), and decide whether `AGENTS.md`/`CLAUDE.md` stay.
- [ ] Enable branch protection on `main`, require review, and restrict who
      may run the `crates-io` environment.
- [ ] Settings → Actions → "Require approval for all outside collaborators"
      (fork pull request workflows), default workflow token permission
      "read repository contents", and "Allow only selected actions".
- [ ] Turn on secret scanning with push protection, Dependabot alerts and
      private vulnerability reporting.
- [ ] Pin every third-party action by commit SHA (`.github/dependabot.yml`
      keeps the pins current), and move `sdk-release.yml` to trusted
      publishing for npm/PyPI.

## 3. Fix the crate manifests

Run the check; it prints what each crate still lacks and the publish order:

```sh
python tools/check_publish_metadata.py --order
```

- [ ] Set `publish` back to the default (remove `publish = false` from
      `[workspace.package]`); keep it on `e2e/` and `vendor/` crates.
- [ ] Add `license`, `readme`, `keywords` and `categories` (at most five each) to
      `[workspace.package]` and inherit them in every crate.
- [ ] Give every workspace dependency in `[workspace.dependencies]` a
      `version` next to its `path` (`crucible-wire = { version = "0.4.0",
      path = "crates/crucible-wire" }`).
- [ ] Keep `crucible-cli` as is and set `publish = false` in its own manifest
      (`tools/check_publish_metadata.py` and the publish workflow skip it).
- [ ] Add the new `crucible-api` library crate: it re-exports the runtime
      crates behind features (`server` → `crucible-server-runtime-rust`,
      `client` → `crucible-client-runtime-rust`, `auth` → the client auth
      modules and the server auth work once merged), has no logic of its
      own, and is published after the crates it re-exports. `crucible-build`
      stays a separate published crate for `build-dependencies`.
- [ ] Check package contents and size for each crate with
      `cargo package --list -p <crate>`; crates.io rejects uploads above
      10 MiB. Exclude `tests/golden` and large fixtures where tests do not
      need to ship.
- [ ] `cargo deny check` passes (`deny.toml`).

## 4. First publication

crates.io Trusted Publishing cannot be configured for a crate that does not
exist yet, and a new account may publish only a few new crates before the
rate limit applies (a burst, then one more every ten minutes at the time of
writing). Plan for both:

- [ ] Ask the crates.io team (help@crates.io) to raise the new-crate rate
      limit before publishing about thirty crates.
- [ ] Create a scoped, short-lived API token limited to `publish-new` and
      store it as the `CARGO_REGISTRY_TOKEN` secret of the `crates-io`
      environment (only reviewers' approval may use it).
- [ ] Create the `crates-io` environment with required reviewers.
- [ ] Once a second maintainer exists, enable **Prevent self-review** on the
      `crates-io` environment so whoever starts a publish cannot approve it.
- [ ] Run **Publish crates** with `dry_run` on (the default). Fix everything
      it reports; the dry run packages and compiles each crate against the
      others in dependency order.
- [ ] Tag the release (`release.yml` creates `v<version>`), then run
      **Publish crates** from that tag with `dry_run` off and
      `auth = bootstrap-token`.
- [ ] For every published crate, open its crates.io settings → Trusted
      Publishing and add: owner, repository, workflow `publish-crates.yml`,
      environment `crates-io`.
- [ ] Delete the `CARGO_REGISTRY_TOKEN` secret. Later releases use
      `auth = trusted-publishing`.
- [ ] Add every crate owner/team on crates.io so no single account is a
      point of failure.

If a run stops part-way, crates published so far stay published. Re-run only
after checking what `cargo publish --workspace` will do with already-published
versions on the current Cargo; if unsure, bump the patch version.

## 5. The `crucible` executable

`crucible-cli` builds the `crucible` binary and is not published to
crates.io; do not run `cargo install crucible-cli` (that name belongs to an
unrelated crate on crates.io). Ship it as prebuilt binaries on GitHub Releases:

- [ ] Adopt [cargo-dist](https://opensource.axo.dev/cargo-dist/) (`dist init`):
      targets Linux (x86_64/aarch64 gnu or musl), macOS (x86_64/aarch64) and
      Windows (x86_64), shell and PowerShell installers, checksums.
      cargo-dist ignores `publish = false` packages unless the package sets
      `dist = true` under `[package.metadata.dist]`; confirm that on the
      version you install.
- [ ] Decide how it joins `release.yml`: that workflow already tags the
      qualified commit and attaches the starter zips and the IDE plugin. Either
      call cargo-dist's generated workflow from the tag, or have `release.yml`
      create the release and cargo-dist upload to it. The generated workflow
      is regenerated by `dist generate`; keep local edits minimal.
- [ ] Optional `cargo binstall crucible-cli` support with
      `[package.metadata.binstall]` (`pkg-url` pointing at the release
      archive); note binstall resolves the crate name, so it must name the
      repository explicitly or the unrelated `crucible-cli` on crates.io wins.
- [ ] Sign or attest the binaries (GitHub artifact attestations) and publish
      their checksums.

## 6. Generated SDK namespaces

The generators take the package name from the user's model, so Crucible does
not own any registry name for generated clients. Reserve the names for the
SDKs Crucible itself ships (runtime packages, the CDK construct library, the
weather example) before announcing:

- [ ] npm scope, PyPI project, NuGet prefix, RubyGems, Packagist, pub.dev,
      Maven Central `groupId` (verify domain/GitHub-namespace ownership,
      for example `io.github.<owner>`).

## 7. Announce

- [ ] Make the repository public only after sections 1 to 3 are complete.
- [ ] Publish release notes that state the licence, the `.cru` language
      version and the compatibility promise.
