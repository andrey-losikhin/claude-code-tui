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
  PATH. Optional desktop notifications also resolve `notify-send` from PATH.
  Use a trusted PATH and review your terminal/Neovim configuration.
- Claude history is read from local JSONL. The TUI owns a separate configuration
  file, knowledge-base notes, and temporary plain-text output snapshots.
- Notes and copied text may contain sensitive data. Notes are not encrypted by
  the TUI. Snapshot files use mode 0600 and are removed on normal close; crashes
  or forced termination can leave them in the system temporary directory.
- Selecting text copies it to the system clipboard. Clipboard managers may keep
  it longer. The fallback sends an OSC 52 clipboard request to the outer terminal.
- Note filenames use validated session identifiers; symlink note files are
  rejected. PTY child arguments are passed directly. Observational hooks run
  through Claude's hook shell; executable/socket paths are quoted, and chat text
  is never interpolated into the command.
- Inline `--settings` adds observational hooks without modifying Claude files or
  emitting permission decisions. Hook stdin is bounded; only session/event/type/
  prompt identifiers reach a private socket in a directory with mode 0700.
  Prompt/transcript text is not written to this transport. A crash can leave the
  directory/socket behind; hooks are optional if policy disables them.
- Search results stay in memory; read-only previews use private snapshot files.
  Tools/subagent records are excluded. Appending a fragment to a running note
  modifies its existing editor buffer without overwriting unsaved text. Switching
  chats clears the candidate selection.
- Desktop/sound notifications are opt-in and contain generic activity text,
  not conversation contents.
- Ghostty/Kitty launchers override panel shortcuts for the launched application
  window without rewriting the user's terminal configuration.
- CI uses synthetic histories, fake Claude executables and disposable HOME/XDG
  directories; it must never receive a real Claude account or conversation.

This policy covers the wrapper; upstream Claude Code, terminals, Neovim and
Rust dependencies have their own security policies. CI and automated scanning
are useful checks, not a guarantee that the application is free of defects.
