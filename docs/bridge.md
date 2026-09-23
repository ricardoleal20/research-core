# The Bridge — One Channel for Every Surface

The bridge (v0.2.0, Story 6.14, FR-21.1, NFR-13) is the **one pluggable
channel** every remote surface — the mobile-web companion today, iOS later —
reaches the home machine through. There is never a second network path, and
never a second writer.

## Security model (the single-writer invariant)

**Off by default.** Nothing listens beyond the existing localhost view
(`server.rs`, AD-7) unless the owner explicitly enables a channel:

- `RC_BRIDGE=off | tunnel | chopflow` at startup (default `off`), or
- the `enable_bridge` / `disable_bridge` commands from Ajustes → Puente.

Failures to start are logged honestly, never fatal — the desktop app does
not depend on the bridge.

**One channel at a time (AD-7).** A second start while a channel is active
is refused with `bridge_already_active:`. Disabling stops the tunnel
listener (or the ChopFlow poll loop) immediately.

**Pairing is required.** Every `/api` request through the bridge carries the
`X-RC-Pairing` header; its sha-256 must match a paired device's row in
`bridge_tokens`. Unpaired requests are refused with `unpaired:` — no open
endpoints, no unauthenticated requests. The `/m` shell and static assets are
exempt (they carry no data until paired). Pairing is a user action
(`pair a device` in Ajustes → Puente): it mints a **one-time token** (shown
once, never stored — only its sha-256 is), appends an auditable
`bridge.device_paired` event, and the paired-devices list is the fold of
that ledger. Re-pairing a device rotates its token; unpairing revokes it
immediately.

**Who can read.** A paired surface reads the **same read-only API slice**
the localhost server serves (`server::read_api_router`) — missions,
dashboard, digest, trust, proposals, receipts. One read contract, folded
over the shared core by the same process.

**Who can approve/reject/capture.** A paired surface decides through the
**same typed core commands** the desktop review surface calls —
`proposals::approve` / `proposals::reject` (and the quick-capture command,
Story 6.15) — executed **in-process, on the home machine, by the home
writer**. Remote decisions are evented with `actor=user` and surface
attribution (`mobile`) visible in receipts (NFR-13).

**What is refused, and why.** There is **no remote append endpoint** — the
bridge process never appends to the store directly, so the single-writer
rule (AD-14) holds by construction, not by convention. The remote
vocabulary is closed (`approve` | `reject` | `capture` — PRD §10: remote
approvals are push + one-tap, nothing more); anything else is refused with
`invalid_command:`. A stale basis is refused with `basis_stale:` unless the
owner explicitly force-confirms — a forced merge records the marker; never a
blind merge (AD-13). A decided proposal is refused with `not_pending:`. The
localhost server (port 4761) keeps its v1 contract: reads only, every
mutation refused — the mobile mutations live exclusively on the paired
bridge channel.

## The adapters

### Tunnel (the default when the bridge is on)

Binds a configurable listen address (default `0.0.0.0:4762`;
`RC_BRIDGE_ADDR` / `RC_BRIDGE_PORT` override) and serves the mobile
companion at `/m` plus the read and remote-command slice. Reachability is
the owner's own (VPN / Tailscale / SSH forwarding). Push delivery is by
reachability: the paired surface polls `/api/notifications` while the
machine is reachable; the companion renders the honest offline state when it
is not (FR-9.1 spirit).

### ChopFlow (first-class, optional — off by default)

The home machine connects **outbound** to the user's ChopFlow deployment.
Config: `RC_CHOPFLOW_URL` (base URL) + `RC_CHOPFLOW_TOKEN` (a pairing token
minted by the same pair-a-device flow). The documented HTTP slice, all
authorized with the token as a bearer:

```
GET  {base}/v1/devices/research-core/commands           → pending remote commands
POST {base}/v1/devices/research-core/commands/{id}/result → the typed command's outcome
POST {base}/v1/devices/research-core/notifications        → verdict-summary push
```

The adapter polls every 30 s, executes each remote command through the same
typed in-process path (single writer), and posts the result. Push payloads
carry **verdict summaries only** — never research content beyond the
summary (NFR-13 extending NFR-1). A failed push is recorded as a visible
`bridge.push_failed` event (actor=system:bridge) — never a silent miss. An
unconfigured or unreachable ChopFlow is an honest error
(`chopflow_unconfigured:`), never a crash, and no flow requires it
(FR-22.3 spirit).

Removing or disabling the ChopFlow adapter changes nothing else — the tunnel
and the desktop app never depend on it.
