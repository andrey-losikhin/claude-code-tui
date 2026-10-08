# Changelog

Notable changes are recorded here using the [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
format. Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Continuous PTY output has a bounded per-frame parsing budget; active chats
  render first and background queues are serviced fairly without dropping data.
- Adjacent terminal cells with identical styling share text spans to reduce
  allocation and rendering overhead.

- Install/run scripts detect a broken system compiler and use an installed
  rustup stable toolchain, without changing the shell PATH or system packages.

- History scanning runs off the rendering thread; live CLI frames are refreshed
  at a 16 ms poll interval without waiting for filesystem scans.

- New chats enter their project group in activity order immediately.
- Native renaming before the first prompt updates open chats and survives restart
  using the saved session-to-directory mapping, even when history has no cwd yet.
- Project metadata is recognized after large startup records.

### Changed

- Public docs cover hook/search/notification boundaries, terminal overrides,
  troubleshooting and maintainer merge/release gates.
- English and Russian feature/control documentation is synchronized.

### Added

- Global Alt+1…4 panel selection before CLI/editor/modal input, with scoped
  Ghostty/Kitty launcher overrides preventing terminal tab-key interception.

- Observational CLI hooks for working/permission/input/ready/error statuses,
  unread markers and opt-in generic desktop/sound notifications.
- Global fuzzy switcher, previous-chat navigation, command palette, shortcut
  popup and collapse-all-projects action, including Cyrillic equivalents.
- Cancellable bounded message/Markdown search, note catalog and adding mouse
  selections to notes without discarding existing Neovim buffers.
- Persistent panel proportions with mouse dragging, sidebar toggle and chat
  maximization.

- Initial public pre-1.0 source distribution (Cargo package version 0.1.0).
- Project groups, search, pins, live rename and multiple Claude CLI sessions.
- Directory picker, per-chat Markdown/Neovim notes and keyboard/mouse navigation.
- Chat-only mouse selection and clipboard copying; active-output viewing in Neovim.
- Ghostty/Kitty desktop installation and terminal palette support.
- Linux unit/PTY CI, CodeQL, Dependabot, and community/security documentation.

[Unreleased]: https://github.com/andrey-losikhin/claude-code-tui/commits/main
