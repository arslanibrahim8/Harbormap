# Security Policy

Harbormap is designed from the ground up to be **local-first, private, and safe to run on sensitive repositories**.

## Security Architecture

- **Loopback-only binding:** `harbormap learn` binds exclusively to `127.0.0.1` on an ephemeral port — it is never exposed to your local network or the public internet.
- **Per-run random authentication token:** Every `harbormap learn` session generates a fresh 128-bit random token (32 hex characters via OS CSPRNG). Visiting `/?t=<token>` sets an `HttpOnly; SameSite=Strict` port-scoped cookie and immediately redirects to `/` so the token does not linger in the address bar. All API and asset requests require either that cookie or a matching `Authorization: Bearer` header, verified with constant-time comparison.
- **DNS rebinding protection:** Every HTTP request validates the `Host` header against `127.0.0.1:<port>` or `localhost:<port>`, returning `403 Forbidden` for foreign hosts.
- **Worktree root containment:** File reading endpoints (such as `/api/file`) canonicalize paths and refuse to serve anything outside the opened project's worktree root.
- **Zero telemetry & no outbound network calls:** Harbormap does not phone home, collect analytics, or send your source code or runtime inventory to external servers.
- **Memory & terminal safety:** The Rust workspace enforces `#![forbid(unsafe_code)]`, checks dependencies with `cargo-deny` against RustSec advisories in CI, and strips control characters from untrusted process/container metadata before printing to the terminal.

---

## Supported Versions

During the alpha phase, security fixes are applied to the `main` branch and included in the latest tagged prerelease.

| Version | Supported |
|---|---|
| `main` / latest `v0.1.x-alpha.*` | Yes |
| Older snapshots | No |

---

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, report vulnerabilities privately via **GitHub Private Vulnerability Reporting** on this repository:

- Navigate to **Security → Advisories → Report a vulnerability** at [https://github.com/arslanibrahim8/Harbormap/security/advisories/new](https://github.com/arslanibrahim8/Harbormap/security/advisories/new).

Please include as much of the following as you can:

- A clear description of the vulnerability and its potential impact.
- Step-by-step reproduction instructions or a minimal proof of concept.
- Your OS, Rust version, and the Harbormap version or commit SHA tested.
- Any suggested mitigation or patch, if you have one.

### What to Expect

- We aim to acknowledge receipt of private reports within **72 hours**.
- We will work with you to validate the issue, coordinate a fix, and agree on disclosure timing before publishing an advisory.
- Credit will be given to reporters in the security advisory unless you prefer to remain anonymous.
