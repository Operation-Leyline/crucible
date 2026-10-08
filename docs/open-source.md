# Open-source and crates.io publication checklist

Work through this list in order. Nothing here is done by merging the PR that
added it: each box is an owner decision or a follow-up change. The audit that
produced it is in that PR's description.

Check items off in a tracking issue, not in this file.

## 1. Decide

- [ ] **Licence.** Pick the SPDX expression (`Apache-2.0` recommended;
      `MIT OR Apache-2.0` is the usual alternative). Add `LICENSE` (and
      `LICENSE-MIT` if dual) at the repository root.
- [ ] **Copyright holder** for the licence header and `NOTICE`.
- [ ] **Leyline and Pigeon references.** Decide keep, generalise or remove for
      each item in the audit's reference table.
- [ ] **History.** Publish the existing history, or start a fresh public
      repository from a squashed tree. Branches, tags, `refs/pull/*` and
      closed pull requests are all public once the repository is.
- [ ] **Names and trademark.** Confirm the crate names below and whether the
      name "Crucible" is acceptable (Atlassian ships an unrelated product of
      that name; the crates.io names `crucible`, `crucible-core` and
      `crucible-cli` are taken).

## 2. Prepare the repository

- [ ] Add `LICENSE`, `NOTICE` (Smithy attribution, see below), `SECURITY.md`,
      `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`.
- [ ] Replace links to private repositories (`smithy-tooling`,
      `crucible-service-template`, `crucible-actions`) or make those public.
- [ ] Remove or generalise `examples/infra/leyline.bindings.json` and the
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
- [ ] Add `license`, `readme`, `keywords` (at most five) and `categories` to
      `[workspace.package]` and inherit them in every crate.
- [ ] Give every workspace dependency in `[workspace.dependencies]` a
      `version` next to its `path` (`crucible-wire = { version = "0.4.0",
      path = "crates/crucible-wire" }`).
- [ ] Resolve `smithy-tooling`: it lives in `vendor/`, is `publish = false`,
      and `crucible-build` depends on it. Publish it as its own crate (the
      bundled jars are about 7 MB) or replace it with a download-at-build
      step; `crucible-build` and the CLI cannot be published until then.
- [ ] Rename `crucible-cli` to `crucible-api` and update every dependent
      (see the rename plan in the PR). Keep the installed binary name
      `crucible` unless you decide otherwise.
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

## 5. Generated SDK namespaces

The generators take the package name from the user's model, so Crucible does
not own any registry name for generated clients. Reserve the names for the
SDKs Crucible itself ships (runtime packages, the CDK construct library, the
weather example) before announcing:

- [ ] npm scope, PyPI project, NuGet prefix, RubyGems, Packagist, pub.dev,
      Maven Central `groupId` (verify domain/GitHub-namespace ownership,
      for example `io.github.<owner>`).

## 6. Announce

- [ ] Make the repository public only after sections 1 to 3 are complete.
- [ ] Publish release notes that state the licence, the supported Smithy
      subset and the compatibility promise.
