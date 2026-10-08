# Maintainer workflow

## Project conventions

- Apache-2.0: retain LICENSE and NOTICE; review attribution for third-party material.
- Semantic Versioning for release numbers; pre-1.0 minor versions may break compatibility.
- Keep a Changelog sections for user-visible changes, kept under Unreleased until release.
- Conventional Commit-style PR titles and squash subjects (`feat:`, `fix:`, `docs:`, etc.).
- GitHub issue/PR templates, CODEOWNERS, security reporting and conduct policy.

These are project conventions, not a claim of ISO certification, formal security
assurance or compliance with every open-source standard.

## Pull request and merge gates

1. Create a topic branch from current main and include a focused diff.
2. Update synthetic tests, both README languages, CHANGELOG and affected design/
   security documents. Exclude `.docs/`, target files, account data and private notes.
3. Run `./scripts/check.sh` and `git diff --check`. Review staged paths and content.
4. Open a PR using the template. State actual test results, behavior, local-data
   changes and what was not checked with a real Claude account or GUI terminal.
5. Review findings and ensure the latest PR head passes stable CI, Rust 1.88 CI,
   CodeQL Rust and CodeQL Actions. Fix failures in the branch; do not bypass them
   by disabling checks or force-pushing main. Repository rules remain authoritative.
6. Squash-merge to main with a descriptive conventional subject. Verify the merged
   commit and main checks, then update the local checkout with a fast-forward.

Do not weaken branch protection or enable new account-level permissions as part
of a routine merge. Workflow changes must be explained in the PR.

## Release preparation

There is currently no automatic binary publication workflow; the supported
installation path builds from source or installs an existing local release build.
Merging a PR does not create a numbered release or Git tag.

For a separately authorized release:

1. Choose a SemVer version, update Cargo.toml and the lockfile's package entry.
2. Move the intended Unreleased entries to a version/date section (YYYY-MM-DD),
   add comparison links and describe any breaking change or migration.
3. Run the full suite and locked release build on a clean checkout; require the
   matching main CI and CodeQL results. Verify the advertised MSRV and platform.
4. Create a `vX.Y.Z` tag at the reviewed commit and publish release notes derived
   from the changelog. Never publish tokens, transcripts, personal paths or notes.
5. If binaries are distributed later, document their target/toolchain/build source,
   include LICENSE/NOTICE and publish SHA-256 checksums. Add reproducibility and
   provenance automation before claiming either property.

## Dependency and security upkeep

Dependabot proposes Cargo/Actions updates. Review compatibility, lockfile changes,
MSRV and security notes before merging. Keep Actions pinned by full commit SHA and
workflow permissions minimal. Follow SECURITY.md for private vulnerability handling;
public CI scans do not establish that a build is defect-free.
