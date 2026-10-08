# Development and checks

## Supported environment

Linux; Rust/Cargo 1.88+; Bash; Python 3; Neovim at `/usr/bin/nvim` for PTY tests.
The lockfile is committed. Dependency downloads are allowed on the first build;
use `CARGO_NET_OFFLINE=true` only after the required crates are cached.

```sh
./scripts/check.sh
```

The script runs formatting, Clippy with warnings denied, unit tests, a debug
build, shell syntax checks and Python PTY smoke tests. For Rust/shell checks only:

```sh
RUN_PTY_TESTS=0 ./scripts/check.sh
cargo build --release --locked
```

## Integration tests

`tests/support.py` forks the actual TUI with a 40x150 PTY and disposable HOME/XDG.
Synthetic executables stand in for Claude; Neovim wrappers use `--clean`.
Clipboard helpers in the copying test are stubs, so the system clipboard is not
changed. `tests/screen.py` reads the current rendered screen rather than matching
old accumulated text.

The suite covers notes and unsaved buffers, project browsing, session open/close/
rename, Russian global shortcuts, CLI key/paste protocols, mouse coordinates,
chat-only selection, clipboard fallback and Neovim output snapshots. The workspace
scenario also invokes the real hook adapter with synthetic events, verifies unread
markers and opt-in notification content, fuzzy/global modals, saved layout dragging,
message search excluding tools, and selection append preserving a dirty editor.
The panel-digits scenario verifies classic Alt, CSI-u/repeats from active Claude
and Neovim insert mode, modals/viewers, hidden/maximized panels and launcher argv
with stub terminals. It does
not verify real Claude API/auth behavior, GUI clipboard permissions or every
terminal's key bindings. Do manual checks with non-sensitive conversations.

## CI and changes

GitHub Actions runs the complete suite on Ubuntu with stable Rust and checks
Rust 1.88 separately. CodeQL scans Rust and workflow code. Dependabot proposes
weekly Cargo and Actions updates; no automatic merge is enabled. Actions use
full commit SHA pins and least-privilege token permissions.

Use a topic branch, update tests/docs, run checks, and open a focused PR.
Do not include `.docs/`, private local planning files, `target/`, real transcripts,
account configuration or notes. Read [CONTRIBUTING.md](../CONTRIBUTING.md).

Merge and release gates are in [maintenance.md](maintenance.md). CI checks apply
to the exact PR head; document real-account/GUI verification limits separately.
