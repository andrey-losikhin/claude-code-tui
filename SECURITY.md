# Security policy

## Supported versions

Claude Code TUI is pre-1.0. Security fixes target the latest `main` revision.
Older revisions and modified builds are not separately supported.

## Reporting a vulnerability

Please do not open a public issue. Use [GitHub private vulnerability reporting](https://github.com/andrey-losikhin/claude-code-tui/security/advisories/new).
Include the revision, affected component, impact and reproduction with synthetic
data. Never send real Claude transcripts, Markdown notes, auth files, tokens or
private project paths. The maintainer coordinates fixes and disclosure based on
severity and availability; no response-time SLA is promised.

## Boundaries and local data

- Claude Code handles authentication, network access and tool permissions. The
  TUI launches `claude` through a PTY without an SDK or direct Anthropic API calls.
- Programs named `claude`, `nvim`, `wl-copy`, `xclip`, and `xsel` are resolved from
  PATH. Use a trusted PATH and review your terminal/Neovim configuration.
- Claude history is read from local JSONL. The TUI owns a separate configuration
  file, knowledge-base notes, and temporary plain-text output snapshots.
- Notes and copied text may contain sensitive data. Notes are not encrypted by
  the TUI. Snapshot files use mode 0600 and are removed on normal close; crashes
  or forced termination can leave them in the system temporary directory.
- Selecting text copies it to the system clipboard. Clipboard managers may keep
  it longer. The fallback sends an OSC 52 clipboard request to the outer terminal.
- Note filenames use validated session identifiers; symlink note files are
  rejected. Command arguments are passed directly, not through a shell.
- CI uses synthetic histories, fake Claude executables and disposable HOME/XDG
  directories; it must never receive a real Claude account or conversation.

This policy covers the wrapper; upstream Claude Code, terminals, Neovim and
Rust dependencies have their own security policies. CI and automated scanning
are useful checks, not a guarantee that the application is free of defects.
