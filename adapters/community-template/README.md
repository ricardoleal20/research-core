# research-core-adapter-template

The reference implementation of ResearchCore's versioned compute-target
adapter contract (Story 6.5, FR-22.4, NFR-15): a complete, working
`sleep-template` adapter — ~150 lines using only the public contract,
with its own process tracking, proving the contract is self-sufficient.

See `docs/adapters/contract.md` (the contract) and `docs/adapters/README.md`
(registration). The adapter:

- re-validates the spec at `submit` (structured specs only),
- spawns argv-direct (never a shell string),
- reports reasoned terminals and a typed `unknown_job:` for foreign
  handles,
- registers through `adapter_contract::register_community`, which
  rejects malformed kinds, first-party kinds, duplicates, and wrong
  contract versions with a specific reason.

## Try it

```sh
cargo test            # the contract tests: registration + lifecycle + refusals
```

## Make it yours

1. Copy the crate, rename it, implement `submit`/`monitor`/`fetch` for
   your platform.
2. Keep the four rules (`docs/adapters/contract.md` §4) — they bind
   first-party and community adapters equally.
3. Call `install()` once at startup. The kind then appears in target
   selection and settings exactly as a first-party kind does.
4. Publish your crate (Apache-2.0) — distribution is yours, not a
   ResearchCore service.