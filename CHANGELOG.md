# Changelog

All notable changes to Research Core are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Local models as a first-class provider (Story 6.1, FR-24): an Ollama (or
  equivalent) adapter in the provider registry — native `/api/chat` +
  `/api/tags`, no API key, base URL in settings (never the keychain), and a
  hard localhost-only guard (zero egress, unit-tested). A reachable local
  endpoint is a REAL provider — it satisfies the assistant's real-only rule;
  a stopped runtime is the honest unconfigured state / typed
  `local_unreachable:` error, never a simulated fallback. Local calls record
  honest token counts at $0 (`note: "local"`), local models flow into every
  model picker (assistant per-conversation, skills, mission roles via
  `RoleConfig`), and the critic ≠ drafter rule holds across mixed
  local/remote configurations. Ajustes → IA gains a "Local / Local" row with
  base URL, live detection, model refresh, and test connection (bilingual
  EN/ES + pt/fr).
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
