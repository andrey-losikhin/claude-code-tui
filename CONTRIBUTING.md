# Contributing

Small, focused changes with a clear user benefit are welcome. The project is
pre-1.0; discuss substantial architecture or dependency changes in an issue first.

## Getting started

Fork the repository, clone your fork, and create a topic branch. Requirements:
Linux, Rust 1.88+, Cargo, Python 3, Bash, and `/usr/bin/nvim` for PTY tests.
Claude credentials are not required for the automated test suite.

```sh
./scripts/check.sh
git diff --check
```

See [docs/development.md](docs/development.md). Add synthetic tests for behavior
changes and update both README languages when changing user-facing controls.
Do not change user history or terminal/editor configuration in tests.

## Pull requests

Explain the problem, approach and user-visible behavior; include actual check
results and limitations. UI screenshots must use synthetic conversations. Keep
unrelated refactoring out of the PR. Never include tokens, chat history, notes,
real project paths or private screenshots. Use the PR and issue templates.

Use Conventional Commit-style subjects where practical (`feat:`, `fix:`,
`docs:`, `test:`, `ci:`, `chore:`). Versioned changes follow Semantic Versioning;
pre-1.0 minor releases may contain breaking changes. Update the Unreleased
section of CHANGELOG.md for user-visible changes. Maintainers review and squash
PRs after the latest head passes stable/MSRV CI and both CodeQL jobs; local working plans belong in ignored `.docs/`.

By submitting a contribution, you confirm you have the right to contribute it
and agree to license it under this repository's Apache-2.0 license. Respect
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). No CLA is currently required.

## Documentation and maintenance

Keep README.md and README.ru.md consistent for features, controls and limitations.
Update architecture/security docs for changes to subprocesses, hooks, local data,
search or copying. Evidence and private plans stay in ignored `.docs/`. Do not
claim certifications or test coverage the project does not actually have.

See [docs/maintenance.md](docs/maintenance.md) for merge/release gates and current
distribution policy. Retain Apache-2.0 LICENSE/NOTICE and required third-party
attribution when adding material.

## Security

Do not publicly report vulnerabilities. Follow [SECURITY.md](SECURITY.md).
