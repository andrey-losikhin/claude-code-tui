# Claude Code TUI

[![CI](https://github.com/andrey-losikhin/claude-code-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/andrey-losikhin/claude-code-tui/actions/workflows/ci.yml)
[![CodeQL](https://github.com/andrey-losikhin/claude-code-tui/actions/workflows/codeql.yml/badge.svg)](https://github.com/andrey-losikhin/claude-code-tui/actions/workflows/codeql.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A local terminal workspace for the **Claude Code CLI**: organize chats by project,
switch between running sessions, and edit per-chat Markdown notes in Neovim.

[Русский README](README.ru.md) · [Architecture](docs/architecture.md) ·
[Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) ·
[Changelog](CHANGELOG.md) · [Maintenance](docs/maintenance.md)

> **Early development:** Linux is the supported platform and the interface is
> currently in Russian. The application launches your installed `claude` CLI;
> it does not replace its authentication, permission prompts or subscription.
> This is an independent project, not an Anthropic product.

## Features

- Collapsible project groups, search, pinned chats and local renaming.
- Live titles from Claude's `/rename`, including already open sessions.
- Multiple independent Claude PTYs; switching keeps other sessions running.
- New-chat directory picker with a filesystem browser starting at `$HOME`.
- Neovim notes below the chat, stored as ordinary Markdown in a knowledge base.
- Chat-only mouse selection: release the button to copy the selected text.
- View the active chat's screen and retained scrollback in Neovim.
- Terminal palette, native CLI colors, mouse navigation and Cyrillic shortcuts.
- CLI event statuses and unread background replies; optional desktop/sound alerts.
- Fuzzy chat switching, command palette, full-text message/Markdown search.
- Knowledge-base catalog and selected chat fragments added to Neovim notes.
- Drag-to-resize panels, saved proportions, sidebar toggle and maximized chat.
- Desktop-menu installation for Ghostty or Kitty.

The TUI does not call Anthropic's API or store account tokens. Account limits and
model state shown by Claude itself remain inside its CLI; there is no separate
quota panel because a reliable public source has not been integrated.

## Requirements

- Linux, a UTF-8 terminal, and an installed/authenticated
  [Claude Code CLI](https://code.claude.com/docs/en/overview) on `PATH`.
- Rust **1.88+** and Cargo when building from source.
- Neovim on `PATH` for notes and output viewing.
- Bash and Python 3 for the launcher/desktop installer.
- Optional Ghostty or Kitty for a desktop-menu entry; a Nerd Font improves icons.
- Optional `wl-copy` (Wayland), `xclip` or `xsel` (X11) for copying.
  Without them, copying uses OSC 52 and requires terminal clipboard permission.

## Build and run

```sh
git clone https://github.com/andrey-losikhin/claude-code-tui.git
cd claude-code-tui
cargo build --release --locked
./target/release/claude-code-tui
```

Alternatively, `./run.sh --terminal here` builds and runs in the current terminal.
Use `./run.sh --terminal ghostty` or `--terminal kitty` for a new terminal window.
The first build downloads Rust dependencies. To build with a populated cache
without network access, set `CARGO_NET_OFFLINE=true`.

## Install in the application menu

```sh
./install.sh                         # chooses Ghostty, then Kitty
./install.sh --terminal kitty        # choose explicitly
./install.sh --no-build              # install an existing release build
```

The installer writes a binary/launcher to `~/.local/bin` and a **Claude Code TUI**
desktop entry to `${XDG_DATA_HOME:-~/.local/share}/applications`. Opening that
entry runs the installed binary without Cargo. Re-run the installer after
updating sources; close and reopen existing TUI windows to use the new build.
Claude authentication and terminal/Neovim configuration are not modified.

## Keyboard and mouse

Global Alt shortcuts work from an active chat and use the equivalent Russian
keyboard keys. The Ghostty/Kitty launchers override Alt+1…4 tab bindings for
this application window so the keys reach the TUI in either keyboard layout. For `--terminal here`,
existing terminal/window-manager bindings can still intercept them.

| Key | Action |
| --- | --- |
| `Alt+1 / 2 / 3 / 4` | Projects / dialogue / open sessions / note (creates or shows it) |
| `Alt+←/→/↑/↓` | Move between panels according to their position |
| `Alt+N` / `Alt+Т` | New chat: select a project directory |
| `Alt+S` / `Alt+Ы` | Fuzzy switcher: open and recent chats |
| `Alt+J` / `Alt+О` | Return to the previous chat |
| `Alt+K` / `Alt+Л` | Searchable command palette |
| `Alt+H` / `Alt+Р` | Popup listing all TUI shortcuts |
| `Alt+Z` / `Alt+Я` | Collapse all projects |
| `Alt+F` / `Alt+А` | Search message text and Markdown notes |
| `Alt+L` / `Alt+Д` | Knowledge-base note catalog |
| `Alt+E` / `Alt+У` | Append the last Claude-pane selection to the active note |
| `Alt+U` / `Alt+Г` | Hide/show the sidebar |
| `Alt+D` / `Alt+В` | Maximize/restore the chat |
| `Alt+T` / `Alt+Е` | Toggle desktop notifications (off by default) |
| `Alt+A` / `Alt+Ф` | Toggle sound notifications (off by default) |
| `Alt+X` / `Alt+Ч` | Stop only the active Claude session and return to projects |
| `Alt+M` / `Alt+Ь` | Create/show/hide the active chat's Neovim note |
| `Alt+B` / `Alt+И` | View only the active chat's output in Neovim |
| `Alt+V` / `Alt+М` | Alias for output viewing if the terminal passes it through |
| `Alt+C` / `Alt+С` | Toggle TUI selection versus forwarding drags to CLI/Neovim |
| `Alt+Q` / `Alt+Й` | Exit; Neovim asks about unsaved notes |
| `↑/↓`, `←/→`, `Enter` | Select chats, collapse/expand projects, open a chat |
| `/` or `f`, `r`, `p`, `m` | Search, rename, pin, set a model override in the sidebar |
| `r` in open sessions | Rename the selected running chat |
| `?` in the sidebar | Show help |

Popups use typing to filter, `↑/↓` to select, `Enter` or a mouse click to open,
`Esc` to cancel. Global keys remain available inside them. Drag the vertical
sidebar border or the horizontal chat/note border to resize; proportions persist.
The palette uses the same action registry as the shortcuts popup.

Status comes from additive command hooks supplied to each CLI via `--settings`.
The TUI never emits permission decisions or writes your Claude settings. If hooks
are disabled by policy, the status stays unknown. `◆` marks a background reply or
request for attention and clears when the chat is shown in the dialogue panel.
Desktop alerts require `notify-send`; both alert options are opt-in and saved.
Notifications contain no chat text. The private local hook socket is removed on
normal exit; hook payload text is neither persisted nor forwarded to it.

Search runs in a cancellable worker over top-level user/assistant text and saved
Markdown. Tool payloads and subagent records are excluded. Limits: 200 results,
5 seconds, 32 MiB per file, 1 MiB per record; skipped/limited work is shown in the
popup. Results open the associated chat and a read-only Neovim view positioned
at the match. A note result opens the saved file; unsaved editor text is not searched.
The note catalog opens associated notes in their existing editor; notes without
history are available in a read-only viewer. Selections are cleared when switching chats. `Alt+E` appends a Markdown quote to
the current note, preserving an existing dirty buffer; use `:w` to save it.

Drag with the left mouse button **inside the chat** to select across lines. The
selection stays within the chat viewport, including when the pointer crosses a
panel boundary. Releasing the button copies the text automatically. Sidebar
clicks still select projects/chats; the wheel scrolls the focused viewport.

Use `:w`, `:wq` and other normal Neovim commands in notes. In output viewing,
`:q` or `Alt+B` returns to the workspace. The output file is a temporary plain
text snapshot of the Claude PTY (current screen plus up to 2,000 retained
scrollback rows), **not** the full conversation archive or the outer terminal.
If Kitty maps `Alt+V` to its own scrollback plugin, use `Alt+B`.

`Ctrl+C` belongs to Claude and may only interrupt a request; use `Alt+X` to close
its session. Modified CLI keys, including `Shift+Enter`, are preserved when the
outer terminal reports the modifiers. Function keys are forwarded to the CLI.

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| Installed window still uses old controls | Re-run `./install.sh`, close the old TUI and reopen **Claude Code TUI**. |
| Alt+1…4 switches terminal tabs | Use the desktop launcher or `./run.sh --terminal ghostty` / `kitty`; these supply window-specific overrides. Direct launches inherit terminal bindings. Check window-manager shortcuts too. |
| Chat status remains unknown | Status needs Claude command hooks; managed policy, disabled hooks or CLI compatibility can prevent events. The TUI does not override that policy. |
| A search result is missing | Search reads saved text/Markdown, excludes tools/subagents and unsaved buffers, and reports skipped files and bounds in the popup. |
| System rustc fails before compilation | run/install try an already installed rustup stable. A working toolchain is required; scripts do not repair system libraries or silently download a fallback. |

## Support

Use the [bug report](https://github.com/andrey-losikhin/claude-code-tui/issues/new?template=bug_report.yml)
or [feature request](https://github.com/andrey-losikhin/claude-code-tui/issues/new?template=feature_request.yml)
template. Include the commit, terminal/OS and a minimal reproduction with
sanitized output. Report vulnerabilities via [SECURITY.md](SECURITY.md) and follow
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). See [CHANGELOG.md](CHANGELOG.md) for
implemented changes; open proposals do not commit a release date.

## Local data

| Data | Location |
| --- | --- |
| Claude session history (read by TUI) | `~/.claude/projects/**/*.jsonl` |
| TUI projects, pins, overrides and collapsed groups | `${XDG_CONFIG_HOME:-~/.config}/claude-code-tui/config.json` |
| Per-chat notes | `~/knowledge-base/claude-code-tui/notes/<session-id>.md` |
| Output-viewer snapshots | System temporary directory; mode `0600`, removed on close |

Legacy notes from the XDG data directory are copied on first open without
replacing existing documents. Closing a chat never deletes its history/notes.
A forcibly terminated TUI may leave a temporary snapshot; treat local history,
notes, snapshots and copied chat text as private data. See [SECURITY.md](SECURITY.md).

## Development

```sh
./scripts/check.sh
```

The checks cover formatting, Clippy, unit tests and Linux PTY integration tests
using a fake Claude executable and isolated HOME/XDG directories. Real Neovim is
used with `--clean`; tests do not log in to Claude or access user conversations.
See [development instructions](docs/development.md) for dependencies and details.

## Community and license

Use the [issue templates](https://github.com/andrey-losikhin/claude-code-tui/issues/new/choose)
for bugs and feature requests. Read [CONTRIBUTING.md](CONTRIBUTING.md),
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), and [CHANGELOG.md](CHANGELOG.md).
Report suspected vulnerabilities **privately** using [SECURITY.md](SECURITY.md).

Licensed under [Apache-2.0](LICENSE). See [NOTICE](NOTICE) for attribution.
Claude Code and third-party dependencies retain their own licenses and terms.
