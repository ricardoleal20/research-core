# Contributing to Research Core

Thanks for your interest in contributing to Research Core! This is a local-first,
AI-powered research workspace, and community contributions are welcome — bug
reports, fixes, new MCP integrations, translations, and docs all help.

## Before you start

- Check [open issues](../../issues) to see if your topic is already being discussed.
- For anything beyond a small fix, please **open an issue first** to discuss the
  direction before investing time in code. This avoids wasted work on changes
  that may not align with the project's roadmap.

## Development setup

Prerequisites: **Rust (1.77+)**, **Node.js 18+**, and the
[Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/) for macOS.

```bash
git clone https://github.com/ricardoleal20/research-core.git
cd research-core
npm install
npm run tauri dev
```

The frontend is plain TypeScript + Vite (no framework); the backend is Rust under
`src-tauri/`. See the [README](README.md) for the full architecture map.

## Making a change

1. **Branch** from `main` using the convention `ricardo/{issue-if-exists}-{what-we-solve}`
   (see [`docs/COMMIT_RULES.md`](docs/COMMIT_RULES.md)).
2. **Code** — match the style and idioms of the surrounding code. Keep
   dependencies widely-adopted and community-supported.
3. **Test** — at minimum, run:
   ```bash
   npx tsc --noEmit
   npx vite build
   cargo check --manifest-path src-tauri/Cargo.toml
   npm run tauri build
   ```
   Verify the app launches cleanly and the relevant screen works.
4. **Commit** using the gitmoji + Action format described in
   [`docs/COMMIT_RULES.md`](docs/COMMIT_RULES.md). Do **not** add
   `Co-Authored-By: Claude` or any AI-attribution trailer.
5. **Open a PR** against `main` with the four-section description
   (`Summary`, `Changes`, `Test plan`, `Refs`). Assign yourself and add a label.

## Code style

- Frontend: TypeScript, no runtime framework. One module per view in `src/views/`.
  Reuse the design-system CSS variables in `src/styles.css`.
- Backend: Rust, idiomatic, modules under `src-tauri/src/`. Keep commands thin;
  put logic in the relevant module.
- Prefer clean, readable, maintainable code over clever or terse solutions.
- Never inline secrets. Use a local `.env` (gitignored) read via the environment.

## Reporting bugs & requesting features

Use the issue templates (Bug report / Feature request). Include repro steps,
expected vs. actual behavior, and your macOS version / arch. For crashes, attach
the relevant lines from `~/Library/Application Support/com.researchcore.app/logs/app.log`.

## Security

Found a security issue? **Do not open a public issue.** See
[`SECURITY.md`](SECURITY.md) for how to report it privately.

## Code of Conduct

By participating you agree to uphold the [Code of Conduct](CODE_OF_CONDUCT.md).
Be kind, assume good intent, and keep discussions focused on the work.
