# Commit & Pull Request Rules

This document defines the commit-message and pull-request conventions for the
Research Core repository. Following them keeps history readable, enables
automated tooling, and lets reviewers scan a change at a glance.

## Commit Message Format

Organization convention is **gitmoji + Action format**:

```
<gitmoji> <Action>: <summary>
```

- The **gitmoji** is a literal token (e.g. `:sparkles:`, `:bug:`, `:memo:`,
  `:wrench:`, `:white_check_mark:`, `:zap:`, `:lock:`, `:fire:`).
- The **Action** is a capitalized verb in **imperative mood**
  (`Add`, `Update`, `Fix`, `Remove`, `Refactor`, `Document`, …).
  Imperative mood is required — "Add", not "Added" or "Adding".
- The **summary** is concise. Soft target ≤ 72 characters.

The gitmoji + Action prefix is **non-optional** — every commit on this repo
carries one.

### Examples

```
:sparkles: Add: uv workspace scaffold
:bug: Fix: idempotency-key day-bucket collision
:memo: Update: AGENTS.md autonomy boundary wording
```

### Multi-line body

If the change needs explanation, write a body paragraph after the subject
(separated by a blank line). Keep the body wrapped at a reasonable width.

```
:sparkles: Add: idempotency-key cache for Linear escalation

Caches resolved escalation keys per-day so a re-triggered webhook within
the same UTC day does not produce a duplicate ticket. TTL is 24h.

Closes: INTM-52
```

## Linear Ticket Trailer

Soft default: commits **should** include a Linear trailer as the **last line**
of the commit message body.

- `Refs: INTM-<n>` — the commit references a ticket without completing it.
- `Closes: INTM-<n>` — the commit completes the ticket and should auto-close
  it on merge.

Multiple ticket references are comma-separated: `Refs: INTM-8, INTM-9`. Use
`Refs:` when a single commit advances multiple tickets but completes none;
use `Closes:` only when the commit completes the cited ticket.

## Co-Authored-By / AI Attribution

**AI-attribution trailers are NOT added to commits on this repo.** Do **not**
add `Co-Authored-By: Claude` or any AI-attribution trailer. This is a
project-level prohibition aligned with the organization's preference — the
`Claude` co-author tag is explicitly forbidden by the repository owner.

Genuine human co-authorship (real pair-programming with another named human)
MAY use `Co-Authored-By: <Name> <email>` at the author's discretion; this
document does not prescribe such a trailer.

## Signing

GPG/SSH commit signing is the host's concern. This project imposes no
project-level signing requirement. If a developer's local workflow signs
commits, the signatures are preserved through PR; CI does not verify
signatures.

## Forbidden in Commits

Never commit any of the following:

- **Secrets or credentials of any kind.** Use a local `.env` file (gitignored)
  and read values via the process environment; never inline a credential in
  code or a commit message.
- **Large generated artifacts.** Use `.gitignore`; if a file genuinely must be
  tracked, document the rationale in the commit body.
- **Merge commits on protected branches.** Use rebase or squash-merge via PR;
  `main` is fast-forward only.

## Pull Request Rules

### PR Title

```
<TICKET> :: <one-line summary>
```

- No emoji, no gitmoji, no action-verb prefix.
- Soft target ≤ 72 characters total.
- Imperative mood for the summary (`Add X`, `Fix Y`, `Update Z`).

Examples:

```
INTM-52 :: Add idempotent ticket creation
OPS-3438 :: Fix scheduler edge case in algorithm
```

Multi-ticket PRs: list both IDs — `INTM-52, OPS-3438 :: Add X`. If more than
two, omit from the title and list all IDs in `## Refs`.

### PR Assignee

Every PR **must** have an assignee. Assign yourself (your git account).

### PR Labels

Every PR **must** carry at least one label. Human-authored PRs follow the same
label convention as automated ones.

### PR Description Sections

The PR description uses this ordered set of sections:

1. **`## Summary`** — what changes and why; one paragraph.
2. **`## Changes`** — bulleted list of files or areas touched.
3. **`## Test plan`** — how the change was tested: explicit test commands
   (e.g. `cargo test`, `npm run tauri build`), manual verification steps,
   and any out-of-band checks.
4. **`## Refs`** — Linear ticket links (`INTM-<n>`), requirement IDs
   addressed, and decision IDs honored.

This four-section shape is normative for this repo and is what reviewers
expect.

### Branches

- Always work on a branch named `ricardo/{issue-if-exists}-{what-we-solve}`.
- `main` is protected and fast-forward only. Land changes via rebase or
  squash-merge through a PR.
