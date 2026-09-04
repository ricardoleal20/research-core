# Security Policy

## Supported versions

Research Core is early-stage (v0.1.x). Security fixes are applied to the latest
`main` and the most recent release only.

| Version | Supported |
| ------- | --------- |
| latest `main` | ✅ |
| latest release | ✅ |
| older releases | ❌ |

## Reporting a vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Instead, please report them privately:

1. Go to the repo's **Security** tab → **Report a vulnerability** (GitHub's
   private vulnerability reporting), **or**
2. Email the maintainer directly if a contact address is listed in the repo
   profile.

Please include:

- A description of the issue and its potential impact
- Steps to reproduce (proof of concept if possible)
- Affected version / commit
- Any suggested remediation

We will acknowledge receipt within **72 hours** and aim to send an initial
assessment within **7 days**. Please give us reasonable time to investigate and
publish a fix before disclosing the issue publicly.

## Scope

Research Core is a **local-first** desktop app. By design, all project data
lives on your machine in a SQLite database under
`~/Library/Application Support/com.researchcore.app/`. The app only makes
outbound network calls you explicitly configure:

- To the **LLM provider** you select in Ajustes (API key is stored locally and
  sent only to that provider).
- To the **MCP servers** you connect (Zotero, arXiv, Semantic Scholar,
  Filesystem, or any you add).

Local data is not encrypted at rest beyond what macOS provides; the optional
**vault lock** gates app access behind a passphrase but is an access control,
not full-disk encryption. Treat the physical security of your Mac as part of
your threat model.

## Disclosure policy

Once a fix is released, we will publish a GitHub Security Advisory crediting the
reporter (unless they prefer to remain anonymous).
