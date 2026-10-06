# Repository guidance

- This is a Linux Rust TUI wrapping the real Claude Code CLI; preserve CLI authentication and permission prompts.
- Read README.md, docs/architecture.md, docs/development.md and the current diff before changes.
- Use small patches, avoid unrelated dependencies/refactors, preserve existing user changes.
- Run `./scripts/check.sh`; offline: `CARGO_NET_OFFLINE=true ./scripts/check.sh` after dependencies are cached.
- PTY integration tests require `/usr/bin/nvim` and use fake Claude, isolated HOME/XDG and synthetic data.
- Never run tests against a real Claude account, history, knowledge base or terminal/editor configuration.
- Keep English/Russian README controls consistent. Update CHANGELOG.md for user-visible changes.
- Keep private paths, credentials, notes, transcripts, build products and `.docs/` out of Git.
- Do not commit/push or create releases unless explicitly authorized by the user.
