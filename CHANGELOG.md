# Changelog

All notable changes to Research Core are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Project README for the public release.
- GitHub community files: `LICENSE` (MIT), `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`,
  `SECURITY.md`, issue templates, and pull-request template.
- `docs/COMMIT_RULES.md` documenting the commit-message and PR conventions.

### Changed
- App header restructured to match the OpenDesign design (no brand block or
  dividers; Ajustes tab moved to the right edge).
- Ajustes screen rebuilt to the design's two-pane layout (`settings-side` nav +
  `settings-main` cards), reusing the existing design-system CSS.

## [0.1.0] - 2026-09-01

### Added
- Native macOS app built with Tauri v2 + Rust + SQLite + MCP.
- Seven project surfaces: Inicio (dashboard), Refs, AI Review, Asistente,
  Acciones, Status y config, and Ajustes.
- Local-first SQLite persistence for projects, references, reviews, actions,
  chats, agents, and settings.
- MCP client with stdio/http transports and bundled servers: Zotero Connector,
  arXiv Search, Semantic Scholar, Filesystem.
- CLI review-agent loop for citation-aware literature reviews.
- First-run setup wizard: data folder, first project, vault lock policy, LLM
  configuration, and an interactive tutorial.
- Welcome, login, zero-projects, lock, and MCP-init splash screens ported from
  the OpenDesign prototype.
- Ajustes danger zone: wipe and recreate the database from scratch.
- Native macOS menu (File, Edit, Window, Debug) with keyboard shortcuts and
  Developer Tools toggle.
