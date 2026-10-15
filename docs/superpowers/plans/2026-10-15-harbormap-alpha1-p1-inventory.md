# Harbormap alpha.1, Part 1: Inventory, `status` and `why` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `harbormap` binary that reads what runs on macOS, Linux and Windows (listening ports, processes, Docker containers), maps every listening port to its project, worktree, service and framework, and explains any port at three levels. It is read-only, makes no network calls and uses no LLM.

**Architecture:** One Cargo workspace with four library crates and the CLI. `hm-probe` reads the OS and Docker into a `RawInventory`; command lines are redacted and environments reduced to names plus a few allowed values before anything leaves the probe. `hm-project` resolves the inventory against git worktrees, manifests and signature packs into a deterministic `Snapshot`. `hm-doctor` loads the CC0 packs and renders three-level cards. The `harbormap` CLI prints `status` and `why` as text or JSON. Resolution reads the filesystem only through a `World` trait, so most tests replay recorded JSON scenarios and run the same on every OS.

**Tech Stack:** Rust 1.99.0 (edition 2024); sysinfo 0.39, listeners 0.6 (macOS and Windows), bollard 0.21 with tokio 1.53, gix 0.89, clap 4.6, serde, serde_json, toml 1.1, regex, url, blake3, dunce; tests use proptest, assert_cmd, predicates and tempfile; GitHub Actions on macos-26, ubuntu-24.04 and windows-2025.

**Spec:** `docs/superpowers/specs/2026-10-15-harbormap-v0.1-runtime-doctor-design.md` (milestone alpha.1 "See"). Principles come from `docs/superpowers/specs/2026-10-15-harbormap-product-design.md`.

## Where this plan fits

alpha.1 "See" is split into four plans. Each one ends with working, tested software.

| Plan | Delivers | Spec sections |
|---|---|---|
| **P1 (this plan)** | Workspace, OS and Docker probes, project and worktree resolution, signature packs, three-level cards, `harbormap status`, `harbormap why`, CI on three OSes, and the first exit criterion | §2 alpha.1, §3, §5.4 (`status`, `why`), §6.1–6.2, §6.4, §7.1, §8, §15 |
| P2 | Safe stop: policy, planner, identity re-check, process-tree stop per OS, SQLite audit log, `harbormap stop`, the negative safety suite (second exit criterion) | §10.1–10.2, §16.4 |
| P3 | Daemon: lifecycle, JSON-RPC over a user-only socket or pipe, HTTP and WebSocket with the security baseline, `harbormap daemon`, `harbormap doctor`, `harbormap` opening the dashboard | §5.1–5.3, §15, §16.5 |
| P4 | Dashboard: list and map views (lane layout in `hm-layout`, SVG), story panel, item drawer, Activity, light and dark themes, levels | §13 |

P2, P3 and P4 are written after P1 lands, against the real code.

**Scope decisions inside P1.** These follow the spec's milestone split; none removes a requirement.
- Expectation sources (§7.2), compose config hashes, idle detection, restart-trigger globs, WSL (Tier 2) and Windows excluded port ranges feed doctor rules, so they arrive with alpha.2. The `restart_globs` fields already exist in the pack schema.
- Agent attribution (`started_by`, env markers) arrives with the beta milestone.
- `status` text looks the same at every level; levels shape the `why` cards. The dashboard (P4) is the beginner surface. A global `--level beginner|builder|expert` flag selects the card level.
- Packs are validated in Rust on every test run; a JSON Schema file for pack authors comes with the 0.1.0 documentation.
- Packs are compiled in with `include_str!`. Crates stay `publish = false` until a release plan changes that.
- Linux reads `/proc/net/tcp` and `/proc/net/tcp6` directly. They list every socket with its owner uid, so a socket whose process is hidden from us is kept with an empty PID list and shown as "unknown owner" with the uid. This replaces the `ss -ltnp` fallback in §6.2: `ss` hides other users' PIDs too, and Harbormap avoids a subprocess.

## Before you start

- Install Rust with rustup if `cargo --version` fails: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`. The repository pins the toolchain in `rust-toolchain.toml` (Task 1); run `rustup toolchain install` once inside the repository to install exactly that version with clippy and rustfmt.
- Git 2.28 or newer (one test creates a linked worktree).
- Only for Task 17: Node 22+, Python 3.12+, the .NET 8 SDK or newer, and optionally Docker.
- Work on a branch, for example `git switch -c alpha1-p1-inventory`.
- Every commit is signed off for the DCO: `git commit -s`.
- After every task: `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` must stay clean; CI enforces both.

## Global Constraints

- Toolchain `1.99.0`, edition `2024`; workspace lints `unsafe_code = "forbid"`; clippy warnings fail CI.
- Code is `AGPL-3.0-only`; everything under `packs/` is `CC0-1.0`; every commit carries `Signed-off-by` (DCO).
- Local only: no network calls at runtime. The only outside connection is the local Docker engine.
- Secrets rule: keep environment variable names only. Read values only for keys matching `^(PORT|.*_PORT)$` (as an integer) and `^.*_(URL|URI|HOST|DSN)$` (reduced to scheme, host and port; userinfo, path, query and fragment are dropped).
- Redact command lines before they are stored or printed: `--password[= ]…`, `-p<val>` for MySQL-like CLIs, `(token|secret|key|password)=…`, `Authorization: Bearer …`, URL userinfo, the prefixes `sk_live_`, `sk_test_`, `ghp_`, `gho_`, `xox[bp]-`, `AKIA`, `AIza`, and strings of ≥24 characters with ≥3.5 bits/char entropy.
- Read-only: P1 never stops, starts or writes anything outside the repository.
- Deterministic: the same inventory gives byte-identical snapshot JSON; every output list is sorted.
- No guessing: an owner Harbormap cannot see is reported as `unknown` with a reason; an inferred framework says "probably".
- Tier 1 is macOS arm64, Linux x64 and Windows x64: all code compiles and its tests pass on all three.
- `Snapshot` JSON field names (snake_case) are a public contract; `SNAPSHOT_SCHEMA = 1`.
- Sentences users read come from `packs/` (CC0), not from code; code only frames them (titles, table headings, notes).
- A repository whose root is the home folder, `/` or a drive root is never a project.
- Dependency versions are exactly those in the root `Cargo.toml` from Task 1.

## Review Focus

The spec does not list these inputs, but a person using Harbormap will meet them. Each has a test in the task that owns the code.

1. A port split between IPv4 and IPv6 by two different programs (common on macOS: `::1` vs `127.0.0.1`): `why` shows both owners, one card each (Task 14, `two_programs_splitting_a_port_get_two_cards`).
2. A program that renames itself, as npm and Next.js do through node (`npm run dev`): on macOS the OS then reports the first environment entries, values included, as its last arguments. They are dropped, so no environment value reaches a snapshot (Task 5, `environment_entries_left_by_a_renamed_process_are_dropped`).
3. A Docker engine that accepts the connection but never answers: `status` still finishes within the timeout and says Docker is not answering (Task 8, `a_hanging_engine_times_out_with_a_note`).
4. Windows paths that differ in case or end with a backslash, and PID 4 holding port 80: the project still matches and port 80 reads "Windows HTTP service" (Task 12, `windows_paths_match_without_case_and_pid_4_is_the_http_service`).
5. A home folder that is itself a git repository (dotfiles): it never swallows every process as one project (Task 12, `a_git_repository_at_home_is_ignored`).

---

### Task 1: Workspace, licences and CI skeleton

This task creates the Cargo workspace with its five crates and their final dependency lists, pins the Rust toolchain, adds the licences and sets up CI. It ends with a `harbormap` binary that only answers `--version`. After this task, no later task edits a `Cargo.toml`.

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`
- Create: `crates/hm-core/Cargo.toml`, `crates/hm-core/src/lib.rs`
- Create: `crates/hm-probe/Cargo.toml`, `crates/hm-probe/src/lib.rs`
- Create: `crates/hm-project/Cargo.toml`, `crates/hm-project/src/lib.rs`
- Create: `crates/hm-doctor/Cargo.toml`, `crates/hm-doctor/src/lib.rs`
- Create: `crates/harbormap/Cargo.toml`, `crates/harbormap/src/main.rs`
- Create: `LICENSE`, `packs/LICENSE` (downloaded), `deny.toml`, `.github/workflows/ci.yml`
- Create: `Cargo.lock` (written by Cargo on the first build; commit it, because `harbormap` is a binary)
- Modify: `.gitignore` (add `/target`)
- Test: `crates/harbormap/tests/cli.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - Workspace members `hm-core`, `hm-probe`, `hm-project` and `hm-doctor` (libraries) and `harbormap` (the binary). Each crate depends only on crates listed in its own `Cargo.toml` below; versions come from `[workspace.dependencies]` in the root `Cargo.toml`.
  - `hm-probe` uses `listeners` only on macOS and Windows: `[target.'cfg(any(target_os = "macos", target_os = "windows"))'.dependencies]`.
  - Each library's `src/lib.rs` holds only its crate doc comment. The task that adds a module adds its `pub mod` line.
  - `harbormap --version` prints `harbormap 0.1.0-alpha.1`.
  - Workspace lints: `unsafe_code = "forbid"`; every clippy lint in `clippy::all` warns, and CI turns warnings into errors.

- [ ] **Step 1: Write the workspace manifest and pin the toolchain**

The root manifest lists the five crates (every folder under `crates/`), the package fields they share, the exact dependency versions and the lints.

Create `Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.1.0-alpha.1"
edition = "2024"
rust-version = "1.99"
license = "AGPL-3.0-only"
repository = "https://github.com/harbormap/harbormap"
publish = false

[workspace.dependencies]
hm-core = { path = "crates/hm-core" }
hm-doctor = { path = "crates/hm-doctor" }
hm-probe = { path = "crates/hm-probe" }
hm-project = { path = "crates/hm-project" }
anyhow = "1.0.104"
assert_cmd = "2.2.2"
blake3 = "1.8.7"
bollard = "0.21.1"
clap = { version = "4.6.7", features = ["derive"] }
dunce = "1.0.5"
gix = { version = "0.89.0", default-features = false, features = ["sha1"] }
listeners = "0.6.1"
predicates = "3.1.4"
proptest = "1.12.0"
regex = "1.13.1"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.154"
sysinfo = { version = "0.39.6", default-features = false, features = ["system", "user"] }
tempfile = "3.27.0"
tokio = { version = "1.53.3", features = ["rt", "time"] }
toml = "1.1.8"
url = "2.5.8"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
```

Create `rust-toolchain.toml`. It makes every `cargo` command in the repository use Rust 1.99.0 with clippy and rustfmt:

```toml
[toolchain]
channel = "1.99.0"
components = ["clippy", "rustfmt"]
```

Install that toolchain, then check that Cargo uses it.

Run: `rustup toolchain install`
Expected: rustup downloads and installs `1.99.0` with `clippy` and `rustfmt`, or says `using existing install for 1.99.0-<your platform>` when it is already there.

Run: `cargo --version`
Expected: `cargo 1.99.0 (5f94df478 2026-08-27)`

- [ ] **Step 2: Add the five crate manifests**

Each crate inherits the shared fields from the workspace and declares its dependencies by name only, so the versions stay in one place.

Create `crates/hm-core/Cargo.toml`:

```toml
[package]
name = "hm-core"
description = "Harbormap domain model, stable IDs, redaction and environment-value rules"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish.workspace = true

[dependencies]
blake3.workspace = true
regex.workspace = true
serde.workspace = true
url.workspace = true

[dev-dependencies]
proptest.workspace = true
serde_json.workspace = true

[lints]
workspace = true
```

Create `crates/hm-probe/Cargo.toml`:

```toml
[package]
name = "hm-probe"
description = "Harbormap OS adapters: processes, listening sockets and Docker engines"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish.workspace = true

[dependencies]
bollard.workspace = true
hm-core.workspace = true
serde_json.workspace = true
sysinfo.workspace = true
tokio.workspace = true

[target.'cfg(any(target_os = "macos", target_os = "windows"))'.dependencies]
listeners.workspace = true

[dev-dependencies]
dunce.workspace = true
tempfile.workspace = true

[lints]
workspace = true
```

Create `crates/hm-project/Cargo.toml`. `hm-doctor` is a dev-dependency because the resolution tests load the real signature packs:

```toml
[package]
name = "hm-project"
description = "Harbormap projects, worktrees, signature matching and snapshot resolution"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish.workspace = true

[dependencies]
dunce.workspace = true
gix.workspace = true
hm-core.workspace = true
serde.workspace = true
serde_json.workspace = true
toml.workspace = true

[dev-dependencies]
hm-doctor.workspace = true
tempfile.workspace = true

[lints]
workspace = true
```

Create `crates/hm-doctor/Cargo.toml`:

```toml
[package]
name = "hm-doctor"
description = "Harbormap packs (signatures and texts), card rendering and, from alpha.2, doctor rules"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish.workspace = true

[dependencies]
hm-core.workspace = true
serde.workspace = true
serde_json.workspace = true
toml.workspace = true

[lints]
workspace = true
```

Create `crates/harbormap/Cargo.toml`:

```toml
[package]
name = "harbormap"
description = "Know what's running, why it's broken, and keep your AI agents from breaking it."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish.workspace = true

[dependencies]
anyhow.workspace = true
clap.workspace = true
hm-core.workspace = true
hm-doctor.workspace = true
hm-probe.workspace = true
hm-project.workspace = true
serde.workspace = true
serde_json.workspace = true

[dev-dependencies]
assert_cmd.workspace = true
predicates.workspace = true

[lints]
workspace = true
```

- [ ] **Step 3: Add placeholder sources and build the workspace**

Every package needs a source file before Cargo can build the workspace. Each library starts with only its crate doc comment (a file with no content at all would fail `cargo fmt --check`), and the binary starts as an empty `main`.

Create `crates/hm-core/src/lib.rs`:

```rust
//! Shared building blocks for every Harbormap crate: the snapshot model, the raw
//! inventory that probes produce, signature-pack types, stable IDs, argv redaction
//! and the narrow rules for reading environment values.
```

Create `crates/hm-probe/src/lib.rs`:

```rust
//! OS adapters: what runs on this computer, before any interpretation.
```

Create `crates/hm-project/src/lib.rs`:

```rust
//! Turns a raw inventory into a snapshot: which project and worktree each process
//! and container belongs to, what each program is (signatures), which services
//! exist and who owns every listening port.
```

Create `crates/hm-doctor/src/lib.rs`:

```rust
//! Packs (signatures and card texts), the three-level card renderer and, from
//! alpha.2, the doctor rules.
```

Create `crates/harbormap/src/main.rs`:

```rust
fn main() {}
```

Run: `cargo build --workspace`
Expected: Cargo downloads and compiles every dependency (about 40 seconds on an Apple M-series laptop; longer on a cold machine), writes `Cargo.lock`, and ends with ``Finished `dev` profile [unoptimized + debuginfo] target(s)`` and no warnings.

- [ ] **Step 4: Write the failing test**

The first integration test runs the real binary and checks its version line. `assert_cmd` builds the binary and finds it.

Create `crates/harbormap/tests/cli.rs`:

```rust
use assert_cmd::Command;

#[test]
fn prints_its_version() {
    Command::cargo_bin("harbormap")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("harbormap {}\n", env!("CARGO_PKG_VERSION")));
}
```

- [ ] **Step 5: Run the test to see it fail**

Run: `cargo test -p harbormap --test cli`
Expected: FAIL — `test result: FAILED. 0 passed; 1 failed`. The panic message says ``Unexpected stdout`` with `stdout=""`: the empty `main` prints nothing.

- [ ] **Step 6: Write the command-line entry point**

`clap` provides `--version` (from the crate version) and `--help` (from the `about` text). Later tasks add the subcommands.

Replace the whole content of `crates/harbormap/src/main.rs` with:

```rust
//! The `harbormap` command line.

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "harbormap",
    version,
    about = "Know what's running, why it's broken, and keep your AI agents from breaking it."
)]
struct Cli {}

fn main() {
    Cli::parse();
}
```

- [ ] **Step 7: Run the test to see it pass**

Run: `cargo test -p harbormap --test cli`
Expected: PASS — `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`

You can also try it: `cargo run -q -p harbormap -- --version` prints `harbormap 0.1.0-alpha.1`.

- [ ] **Step 8: Ignore build output and add the licences**

Add `/target` as a new line at the end of `.gitignore`, so the file reads:

```gitignore
.DS_Store
/target
```

The code is AGPL-3.0-only; everything under `packs/` is CC0-1.0, so anyone can reuse the signatures and texts. Download both licence texts and check that they are the expected files:

```bash
curl -fsSL -o LICENSE https://www.gnu.org/licenses/agpl-3.0.txt
echo "0d96a4ff68ad6d4b6f1f30f713b18d5184912ba8dd389f86aa7710db079abcb0  LICENSE" | shasum -a 256 -c -
mkdir -p packs
curl -fsSL -o packs/LICENSE https://creativecommons.org/publicdomain/zero/1.0/legalcode.txt
echo "a2010f343487d3f7618affe54f789f5487602331c0a8d03f49e9a7c547cf0499  packs/LICENSE" | shasum -a 256 -c -
```

Expected: `LICENSE: OK` and `packs/LICENSE: OK`. Where `shasum` is missing (some Linux and Windows shells), use `sha256sum -c -` with the same input. If a check fails, the server returned something else (for example an error page): delete the file and download it again.

- [ ] **Step 9: Add the dependency policy and the CI workflow**

`deny.toml` configures cargo-deny, which CI runs: every RustSec advisory fails the build, and only licences compatible with distributing the binary under AGPL-3.0-only are allowed.

Create `deny.toml`:

```toml
# cargo-deny configuration for Harbormap (checked with cargo-deny 0.20.2).
# Docs: https://embarkstudios.github.io/cargo-deny/
# Run locally with: cargo deny check

[graph]
# Only resolve dependencies for the Tier-1 platforms we ship binaries for.
# Add a target here before shipping a binary for it.
targets = [
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
]

[advisories]
# Every vulnerability advisory from the RustSec database fails the check.
# Ignore one only with a reason, e.g.
# { id = "RUSTSEC-0000-0000", reason = "the affected API is not used" }.
unmaintained = "all"
unsound = "workspace"
yanked = "warn"
ignore = []

[licenses]
# The `harbormap` binary is distributed under AGPL-3.0-only. Every license
# below is compatible with distributing a combined work under AGPL-3.0-only.
# Any other license fails the check until someone reviews it and adds it here.
# Known incompatible licenses include GPL-2.0-only, EPL-1.0/EPL-2.0 (without a
# GPL secondary licence), CDDL-1.0, OpenSSL/SSLeay and BUSL-1.1.
allow = [
    "AGPL-3.0-only",                  # Harbormap's own crates
    "Apache-2.0",                     # compatible with (A)GPL version 3, not with GPL-2.0
    "Apache-2.0 WITH LLVM-exception", # the exception only adds permissions
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",                        # Boost Software License (not BUSL-1.1)
    "CC0-1.0",
    "ISC",
    "MIT",
    "MIT-0",
    "MPL-2.0",                        # AGPL-3.0 is a "Secondary License" in MPL-2.0 section 1.12
    "Unicode-3.0",
    "Unlicense",
    "Zlib",
]
# Pre-approved licenses that no current dependency uses are not an error.
unused-allowed-license = "allow"
confidence-threshold = 0.8
exceptions = []

[licenses.private]
# Check our own crates too: each must declare `license.workspace = true`.
ignore = false

[bans]
multiple-versions = "warn"
wildcards = "deny"
# Path dependencies between our unpublished workspace crates are fine.
allow-wildcard-paths = true
highlight = "all"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
allow-git = []
```

The workflow has three jobs: `fmt` checks formatting, `test` runs clippy and the tests on the three Tier-1 runners (macOS arm64, Linux x64, Windows x64), and `deny` runs cargo-deny. Task 17 adds the end-to-end job.

Create `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

# A new push to a pull request cancels the older runs of that pull request.
# Pushes to main get their own group (the run id), so they are never cancelled.
concurrency:
  group: ${{ github.workflow }}-${{ github.event.pull_request.number || github.run_id }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}

env:
  CARGO_TERM_COLOR: always

jobs:
  fmt:
    runs-on: ubuntu-24.04
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      # rust-toolchain.toml pins channel 1.99.0 with clippy and rustfmt. Given no
      # toolchain name, `rustup toolchain install` (rustup >= 1.28) installs that
      # toolchain and its components. We do not rely on implicit installation on
      # first use: see https://github.com/rust-lang/rustup/issues/4836.
      - name: Install the pinned Rust toolchain
        run: rustup toolchain install --profile minimal --no-self-update
      - run: cargo fmt --all --check

  test:
    strategy:
      fail-fast: false
      matrix:
        os: [macos-26, ubuntu-24.04, windows-2025]
    runs-on: ${{ matrix.os }}
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - name: Install the pinned Rust toolchain
        run: rustup toolchain install --profile minimal --no-self-update
      # Must come after the toolchain step: the cache key uses the rustc version.
      - uses: Swatinem/rust-cache@v2
        with:
          save-if: ${{ github.ref == 'refs/heads/main' }}
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace

  deny:
    runs-on: ubuntu-24.04
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      # Runs `cargo deny --all-features check` (advisories, bans, licenses and
      # sources) with deny.toml from the repository root.
      - uses: EmbarkStudios/cargo-deny-action@v2
```

If you have cargo-deny installed (`cargo install cargo-deny --locked`), `cargo deny check` reports `advisories ok, bans ok, licenses ok, sources ok`; two `multiple-versions` warnings (for `hashbrown` and `syn`) are expected. You don't need it locally: CI runs it on every push.

- [ ] **Step 10: Run the workspace checks**

These are the same checks CI runs.

Run: `cargo fmt --all --check`
Expected: no output, exit code 0.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: ``Finished `dev` profile [unoptimized + debuginfo] target(s)`` with no warnings.

Run: `cargo test --workspace`
Expected: PASS — one `test result: ok. 1 passed` line (from `tests/cli.rs`); every other test binary and doc-test run reports `0 passed`.

- [ ] **Step 11: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml .gitignore LICENSE packs/LICENSE deny.toml .github/workflows/ci.yml crates/hm-core/Cargo.toml crates/hm-core/src/lib.rs crates/hm-probe/Cargo.toml crates/hm-probe/src/lib.rs crates/hm-project/Cargo.toml crates/hm-project/src/lib.rs crates/hm-doctor/Cargo.toml crates/hm-doctor/src/lib.rs crates/harbormap/Cargo.toml crates/harbormap/src/main.rs crates/harbormap/tests/cli.rs
git commit -s -m "chore: set up the Cargo workspace, licences and CI"
```

### Task 2: Names and environment-value rules

This task starts the `hm-core` library with two small modules that later tasks build on. `names` turns a program path into a short name that compares the same way on macOS, Linux and Windows. `env` applies the secrets rule from spec §15: Harbormap keeps environment variable names, but reads only port numbers and the scheme, host and port of URL-like values.

**Files:**
- Modify: `crates/hm-core/src/lib.rs` (replace the placeholder from Task 1 with the crate doc comment, `pub mod env;` and `pub mod names;`)
- Create: `crates/hm-core/src/names.rs`
- Create: `crates/hm-core/src/env.rs`
- Test: `crates/hm-core/src/names.rs`, `crates/hm-core/src/env.rs` (unit tests in the same files)

**Interfaces:**
- Consumes: no code from earlier tasks. It uses the `hm-core` dependencies that Task 1 declared: `serde` and `url`, plus the dev-dependencies `proptest` and `serde_json`.
- Produces, in `hm_core::names`:
  - `pub fn basename(path: &str) -> &str`
  - `pub fn exe_stem(path: &str) -> String`
- Produces, in `hm_core::env` (every struct field is `pub`):
  - `pub struct EnvFacts { names: Vec<String>, ports: Vec<EnvPort>, urls: Vec<EnvUrl> }` (derives `Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize`; every field is `#[serde(default)]`)
  - `pub struct EnvPort { name: String, port: u16 }` (derives `Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize`)
  - `pub struct EnvUrl { name: String, scheme: String, host: String, port: Option<u16> }` (same derives as `EnvPort`)
  - `pub fn is_port_key(name: &str) -> bool`
  - `pub fn is_url_key(name: &str) -> bool`
  - `pub fn extract<I, K, V>(vars: I) -> EnvFacts where I: IntoIterator<Item = (K, V)>, K: AsRef<str>, V: AsRef<str>`
  - `pub fn extract_from_pairs<I, S>(entries: I) -> EnvFacts where I: IntoIterator<Item = S>, S: AsRef<str>`

- [ ] **Step 1: Write the failing test for `names`**

The tests describe how a program path becomes a short name: both path separators work, the result is lowercase, and Windows executable extensions are dropped.

Replace the whole content of `crates/hm-core/src/lib.rs` with:

```rust
//! Shared building blocks for every Harbormap crate: the snapshot model, the raw
//! inventory that probes produce, signature-pack types, stable IDs, argv redaction
//! and the narrow rules for reading environment values.

pub mod names;
```

Create `crates/hm-core/src/names.rs` with only its test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_both_separators() {
        assert_eq!(basename("/usr/local/bin/node"), "node");
        assert_eq!(basename(r"C:\Program Files\nodejs\node.exe"), "node.exe");
        assert_eq!(basename("vite"), "vite");
    }

    #[test]
    fn exe_stem_lowercases_and_strips_windows_extensions() {
        assert_eq!(exe_stem("/usr/bin/Python3"), "python3");
        assert_eq!(exe_stem(r"C:\Tools\Node.EXE"), "node");
        assert_eq!(exe_stem("npm.cmd"), "npm");
        assert_eq!(exe_stem("vite.js"), "vite.js");
    }
}
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p hm-core names`
Expected: FAIL — compile errors such as ``error[E0425]: cannot find function `basename` in this scope`` (and the same for `exe_stem`).

- [ ] **Step 3: Write the `names` implementation**

Add this above the `#[cfg(test)]` line in `crates/hm-core/src/names.rs`. The block ends with one empty line, so that a blank line separates it from `#[cfg(test)]`:

```rust
//! Small helpers for program names that work the same on every OS.

/// The last path component, splitting on both `/` and `\`.
pub fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Lowercase basename without a Windows executable extension:
/// `/usr/bin/Python3` → `python3`, `C:\\Tools\\Node.EXE` → `node`.
pub fn exe_stem(path: &str) -> String {
    let base = basename(path).to_ascii_lowercase();
    for ext in [".exe", ".cmd", ".bat", ".com"] {
        if let Some(stem) = base.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    base
}

```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p hm-core names`
Expected: PASS — `test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`

- [ ] **Step 5: Write the failing test for `env`**

The tests check that only names, port values and the scheme, host and port of URL-like values survive. A property test checks that generated passwords and tokens never appear in the JSON.

Add `pub mod env;` above `pub mod names;` in `crates/hm-core/src/lib.rs`, so the file reads:

```rust
//! Shared building blocks for every Harbormap crate: the snapshot model, the raw
//! inventory that probes produce, signature-pack types, stable IDs, argv redaction
//! and the narrow rules for reading environment values.

pub mod env;
pub mod names;
```

Create `crates/hm-core/src/env.rs` with only its test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn keeps_names_and_drops_other_values() {
        let facts = extract([
            ("SECRET_KEY", "hunter2"),
            ("PATH", "/usr/bin"),
            ("PATH", "/usr/bin"),
        ]);
        assert_eq!(facts.names, vec!["PATH", "SECRET_KEY"]);
        assert!(facts.ports.is_empty());
        assert!(facts.urls.is_empty());
    }

    #[test]
    fn reads_port_like_keys_as_integers() {
        let facts = extract([("PORT", "3000"), ("API_PORT", " 8000 "), ("DB_PORT", "x")]);
        assert_eq!(
            facts.ports,
            vec![
                EnvPort {
                    name: "API_PORT".into(),
                    port: 8000
                },
                EnvPort {
                    name: "PORT".into(),
                    port: 3000
                },
            ]
        );
    }

    #[test]
    fn reduces_url_like_values_to_scheme_host_and_port() {
        let facts = extract([
            (
                "DATABASE_URL",
                "postgres://app:s3cret@localhost:5432/shop?sslmode=disable",
            ),
            ("REDIS_URL", "redis://:pw@cache/0"),
            ("API_HOST", "api.internal:8080"),
            ("DB_HOST", "db"),
            ("BROKEN_URL", "not a url"),
            ("QUERY_URL", "localhost:5432/db?password=p@ss"),
            ("V6_HOST", "[::1]:5432"),
            ("BRACKET_HOST", "[hunter2 and more]"),
        ]);
        assert_eq!(
            facts.urls,
            vec![
                EnvUrl {
                    name: "API_HOST".into(),
                    scheme: String::new(),
                    host: "api.internal".into(),
                    port: Some(8080)
                },
                EnvUrl {
                    name: "DATABASE_URL".into(),
                    scheme: "postgres".into(),
                    host: "localhost".into(),
                    port: Some(5432)
                },
                EnvUrl {
                    name: "DB_HOST".into(),
                    scheme: String::new(),
                    host: "db".into(),
                    port: None
                },
                EnvUrl {
                    name: "QUERY_URL".into(),
                    scheme: String::new(),
                    host: "localhost".into(),
                    port: Some(5432)
                },
                EnvUrl {
                    name: "REDIS_URL".into(),
                    scheme: "redis".into(),
                    host: "cache".into(),
                    port: None
                },
                EnvUrl {
                    name: "V6_HOST".into(),
                    scheme: String::new(),
                    host: "::1".into(),
                    port: Some(5432)
                },
            ]
        );
        let json = serde_json::to_string(&facts).unwrap();
        assert!(!json.contains("s3cret"));
        assert!(!json.contains("pw"));
        assert!(!json.contains("sslmode"));
        assert!(!json.contains("p@ss"));
        assert!(!json.contains("hunter2"));
    }

    #[test]
    fn splits_environment_block_entries() {
        let facts = extract_from_pairs(["PORT=5173", "=C:=C:\\work", "EMPTY="]);
        assert_eq!(facts.names, vec!["EMPTY", "PORT"]);
        assert_eq!(facts.ports[0].port, 5173);
    }

    proptest! {
        #[test]
        fn no_secret_value_survives(
            user in "[a-z]{3,10}",
            // The digit keeps generated values from colliding with fixed JSON text.
            pass in "[A-Za-z]{3,10}[0-9][A-Za-z0-9]{4,12}",
            secret in "[A-Za-z]{3,10}[0-9][A-Za-z0-9]{4,20}",
        ) {
            let url = format!("postgres://{user}:{pass}@db.local:5432/app?token={secret}");
            // Without a scheme, and with an `@` in the query.
            let bare = format!("{user}:{pass}@db.local:5432/app?next=x@{secret}");
            let facts = extract([
                ("DATABASE_URL", url.as_str()),
                ("BARE_DSN", bare.as_str()),
                ("API_TOKEN", secret.as_str()),
                ("PASSWORD", pass.as_str()),
            ]);
            let json = serde_json::to_string(&facts).unwrap();
            prop_assert!(!json.contains(&pass));
            prop_assert!(!json.contains(&secret));
        }
    }
}
```

- [ ] **Step 6: Run the test to see it fail**

Run: `cargo test -p hm-core env`
Expected: FAIL — compile errors such as ``error[E0422]: cannot find struct, variant or union type `EnvPort` in this scope`` and ``error[E0425]: cannot find function `extract` in this scope``.

- [ ] **Step 7: Write the `env` implementation**

Add this above the `#[cfg(test)]` line in `crates/hm-core/src/env.rs`, again ending with one empty line. `extract` uses a let chain (`&& let Some((scheme, host, port)) = parse_target(value.trim())`), which needs edition 2024; Task 1 set that edition for the workspace.

```rust
//! The secrets rule for environment variables (v0.1 spec §15).
//!
//! Harbormap keeps variable *names*. It reads exactly two kinds of values:
//! - keys matching `^(PORT|.*_PORT)$`, parsed as an integer;
//! - keys matching `^.*_(URL|URI|HOST|DSN)$`, parsed in memory to scheme, host and port.
//!
//! Userinfo, path, query and fragment are dropped before anything leaves this module.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvFacts {
    /// Every variable name, sorted and deduplicated. Never values.
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub ports: Vec<EnvPort>,
    #[serde(default)]
    pub urls: Vec<EnvUrl>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EnvPort {
    pub name: String,
    pub port: u16,
}

/// A URL-like value reduced to where it points.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EnvUrl {
    pub name: String,
    /// Empty when the value had no scheme (for example `DB_HOST=db:5432`).
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
}

pub fn is_port_key(name: &str) -> bool {
    name == "PORT" || name.ends_with("_PORT")
}

pub fn is_url_key(name: &str) -> bool {
    ["_URL", "_URI", "_HOST", "_DSN"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

/// Reduces `NAME=value` pairs to names plus the allowed facts.
pub fn extract<I, K, V>(vars: I) -> EnvFacts
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut facts = EnvFacts::default();
    for (name, value) in vars {
        let (name, value) = (name.as_ref(), value.as_ref());
        if name.is_empty() {
            continue;
        }
        facts.names.push(name.to_string());
        if is_port_key(name) {
            if let Ok(port) = value.trim().parse::<u16>() {
                facts.ports.push(EnvPort {
                    name: name.to_string(),
                    port,
                });
            }
        } else if is_url_key(name)
            && let Some((scheme, host, port)) = parse_target(value.trim())
        {
            facts.urls.push(EnvUrl {
                name: name.to_string(),
                scheme,
                host,
                port,
            });
        }
    }
    facts.names.sort();
    facts.names.dedup();
    // Sorted by name first, then by value, so repeated names cannot change the order.
    facts.ports.sort();
    facts.ports.dedup();
    facts.urls.sort();
    facts.urls.dedup();
    facts
}

/// Splits `KEY=value` entries as found in a process environment block.
pub fn extract_from_pairs<I, S>(entries: I) -> EnvFacts
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let pairs: Vec<(String, String)> = entries
        .into_iter()
        .filter_map(|entry| {
            let entry = entry.as_ref();
            // Windows keeps hidden per-drive variables such as "=C:=C:\\". Their name is
            // empty, so `extract` skips them.
            let (name, value) = entry.split_once('=')?;
            Some((name.to_string(), value.to_string()))
        })
        .collect();
    extract(pairs)
}

fn parse_target(value: &str) -> Option<(String, String, Option<u16>)> {
    if value.is_empty() {
        return None;
    }
    if value.contains("://") {
        let url = url::Url::parse(value).ok()?;
        let host = url
            .host_str()?
            .trim_start_matches('[')
            .trim_end_matches(']');
        if host.is_empty() {
            return None;
        }
        return Some((
            url.scheme().to_string(),
            host.to_string(),
            url.port_or_known_default(),
        ));
    }
    // host, host:port or [v6]:port without a scheme. The authority ends at the first
    // `/`, `?` or `#`, so an `@` in a path or query is never taken for userinfo.
    let authority = value.split(['/', '?', '#']).next()?;
    let hostport = authority
        .rsplit_once('@')
        .map_or(authority, |(_, rest)| rest);
    if let Some(rest) = hostport.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        host.parse::<std::net::Ipv6Addr>().ok()?;
        let port = after.strip_prefix(':').and_then(|p| p.parse().ok());
        return Some((String::new(), host.to_string(), port));
    }
    match hostport.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            let port = port.parse::<u16>().ok()?;
            valid_host(host).then(|| (String::new(), host.to_string(), Some(port)))
        }
        Some(_) => None,
        None => valid_host(hostport).then(|| (String::new(), hostport.to_string(), None)),
    }
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

```

- [ ] **Step 8: Run the tests to see them pass**

Run: `cargo test -p hm-core env`
Expected: PASS — `test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out`. The 2 filtered-out tests are the `names` tests. The property test `no_secret_value_survives` counts as one test.

- [ ] **Step 9: Check formatting**

CI runs this check, so run it before you commit.

Run: `cargo fmt --all --check`
Expected: no output, exit code 0.

- [ ] **Step 10: Commit**

```bash
git add crates/hm-core/src/lib.rs crates/hm-core/src/names.rs crates/hm-core/src/env.rs
git commit -s -m "feat(core): add program-name helpers and environment-value rules"
```

### Task 3: Redaction

This task adds the redaction rules from spec §15. Every command line goes through `redact_argv` before it is stored or shown, so passwords, tokens, URL credentials, known key formats and long random-looking strings never leave the probe. Log lines will use `redact_text` from alpha.3 on.

**Files:**
- Create: `crates/hm-core/src/redact.rs`
- Modify: `crates/hm-core/src/lib.rs` (add `pub mod redact;`)
- Test: `crates/hm-core/src/redact.rs` (unit tests in the same file)

**Interfaces:**
- Consumes: `hm_core::names::exe_stem(path: &str) -> String` (Task 2). It also uses the `regex` dependency and the `proptest` dev-dependency from Task 1.
- Produces, in `hm_core::redact`: `pub const MASK: &str = "***";`, `pub fn redact_argv(argv: &[String]) -> Vec<String>`, `pub fn redact_text(text: &str) -> String`

- [ ] **Step 1: Write the failing test**

The tests check that ordinary command lines stay unchanged and that secret flags, MySQL-style `-pVALUE`, URL credentials, bearer tokens, `key=value` secrets, known key prefixes and random-looking strings are masked; five property tests generate secrets and check that none survive.

Add `pub mod redact;` after `pub mod names;` in `crates/hm-core/src/lib.rs`, so the file reads:

```rust
//! Shared building blocks for every Harbormap crate: the snapshot model, the raw
//! inventory that probes produce, signature-pack types, stable IDs, argv redaction
//! and the narrow rules for reading environment values.

pub mod env;
pub mod names;
pub mod redact;
```

Create `crates/hm-core/src/redact.rs` with only its test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    #[test]
    fn leaves_ordinary_command_lines_alone() {
        for parts in [
            &[
                "node",
                "/Users/me/shop/apps/web/node_modules/.bin/vite",
                "--port",
                "5173",
            ][..],
            &[
                "java",
                "-cp",
                "app.jar",
                "org.springframework.boot.loader.launch.JarLauncher",
            ],
            &[
                "python3", "-m", "uvicorn", "main:app", "--reload", "--port", "8000",
            ],
            &["psql", "-h", "localhost", "-p", "5432", "-U", "app"],
            &["/var/folders/zz/T/go-build3456789012/b001/exe/main"],
        ] {
            let input = argv(parts);
            assert_eq!(redact_argv(&input), input);
        }
    }

    #[test]
    fn masks_secret_flags_and_their_values() {
        assert_eq!(
            redact_argv(&argv(&["app", "--password", "hunter2", "--port", "80"])),
            argv(&["app", "--password", "***", "--port", "80"])
        );
        assert_eq!(
            redact_argv(&argv(&["app", "--api-key=abc123"])),
            argv(&["app", "--api-key=***"])
        );
    }

    #[test]
    fn masks_attached_mysql_passwords_only_for_mysql_like_tools() {
        assert_eq!(
            redact_argv(&argv(&["mysql", "-u", "root", "-phunter2", "shop"])),
            argv(&["mysql", "-u", "root", "-p***", "shop"])
        );
        assert_eq!(
            redact_argv(&argv(&["psql", "-p5432"])),
            argv(&["psql", "-p5432"])
        );
    }

    #[test]
    fn masks_secrets_inside_text() {
        assert_eq!(
            redact_text("postgres://app:s3cret@db:5432/shop"),
            "postgres://***@db:5432/shop"
        );
        assert_eq!(
            redact_text("Authorization: Bearer abc.def.ghi"),
            "Authorization: Bearer ***"
        );
        assert_eq!(
            redact_text("-Dspring.datasource.password=letmein"),
            "-Dspring.datasource.password=***"
        );
        assert_eq!(redact_text(r#"{"password":"x1"}"#), r#"{"password":***}"#);
        assert_eq!(redact_text("key ghp_aBcDeFgHiJkLmNoPqRsTuV"), "key ghp_***");
        assert_eq!(
            redact_text("--session Xk2pQ9vR7tLm3nB8cD4fG6hJ1sA5"),
            "--session ***"
        );
    }

    proptest! {
        #[test]
        fn prefixed_keys_never_survive(
            prefix in prop::sample::select(vec!["sk_live_", "sk_test_", "ghp_", "gho_", "xoxb-", "xoxp-", "AKIA", "AIza"]),
            suffix in "[A-Za-z0-9]{16,40}",
        ) {
            let out = redact_text(&format!("run --flag {prefix}{suffix} now"));
            prop_assert!(!out.contains(&suffix));
        }

        #[test]
        fn url_passwords_never_survive(user in "[a-z]{3,8}", pass in "[A-Za-z0-9]{6,20}") {
            let out = redact_text(&format!("postgres://{user}:{pass}@db:5432/app"));
            let needle = format!(":{pass}@");
            prop_assert!(!out.contains(&needle));
        }

        #[test]
        fn secret_named_values_never_survive(
            name in prop::sample::select(vec!["token", "API_KEY", "client_secret", "DB_PASSWORD", "pwd"]),
            value in "[A-Za-z0-9]{6,30}",
        ) {
            let out = redact_text(&format!("{name}={value}"));
            let needle = format!("={value}");
            prop_assert!(!out.contains(&needle));
        }

        #[test]
        fn random_strings_never_survive(secret in "[A-Za-z0-9]{24,64}") {
            prop_assume!(looks_random(&secret));
            let out = redact_text(&format!("--x {secret} --y"));
            prop_assert!(!out.contains(&secret));
        }

        #[test]
        fn secret_flags_never_leak(value in "[A-Za-z]{2,10}[0-9][!@#%^&*][A-Za-z0-9]{1,12}") {
            let spaced = redact_argv(&argv(&["app", "--password", &value]));
            let joined = redact_argv(&argv(&["app", &format!("--token={value}")]));
            let mysql = redact_argv(&argv(&["mysql", &format!("-p{value}")]));
            for out in [spaced, joined, mysql] {
                prop_assert!(!out.iter().any(|arg| arg.contains(&value)));
            }
        }
    }
}
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p hm-core redact`
Expected: FAIL — compile errors such as ``error[E0425]: cannot find function `redact_argv` in this scope`` (and the same for `redact_text` and `looks_random`).

- [ ] **Step 3: Write the implementation**

Add this above the `#[cfg(test)]` line in `crates/hm-core/src/redact.rs`, ending with one empty line:

```rust
//! Redaction of command lines before they are stored or shown (v0.1 spec §15).
//! Log lines use `redact_text` from alpha.3 on.

use std::sync::LazyLock;

use regex::Regex;

use crate::names::exe_stem;

pub const MASK: &str = "***";

/// Flags whose value is the next argument (or follows `=`).
const SECRET_FLAGS: &[&str] = &[
    "--password",
    "--passwd",
    "--pass",
    "--token",
    "--auth-token",
    "--secret",
    "--api-key",
    "--apikey",
    "--access-key",
    "--secret-key",
];

/// CLIs where `-pVALUE` (attached) is a password rather than a port.
const MYSQL_LIKE: &[&str] = &[
    "mysql",
    "mysqldump",
    "mysqladmin",
    "mysqlsh",
    "mariadb",
    "mariadb-dump",
];

static URL_USERINFO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z][A-Za-z0-9+.\-]*://)[^/@\s]+@").unwrap());

static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(bearer)\s+[A-Za-z0-9._~+/=\-]+").unwrap());

static PREFIXED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(sk_live_|sk_test_|rk_live_|rk_test_|ghp_|gho_|ghu_|ghs_|ghr_|github_pat_|glpat-|xox[abprs]-|AKIA|ASIA|AIza)[A-Za-z0-9_\-]{8,}",
    )
    .unwrap()
});

static KEY_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)([A-Za-z0-9_.\-]*(?:token|secret|password|passwd|pwd|key)[A-Za-z0-9_.\-]*"?\s*[=:]\s*)("[^"]*"|'[^']*'|[^\s&;,"']+)"#,
    )
    .unwrap()
});

static CANDIDATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9_+=\-]{24,}").unwrap());

/// Redacts a full command line. `argv[0]` decides whether MySQL-style `-pVALUE` applies.
pub fn redact_argv(argv: &[String]) -> Vec<String> {
    let mysql_like = argv
        .first()
        .is_some_and(|first| MYSQL_LIKE.contains(&exe_stem(first).as_str()));
    let mut out = Vec::with_capacity(argv.len());
    let mut mask_next = false;
    for (index, arg) in argv.iter().enumerate() {
        if mask_next {
            out.push(MASK.to_string());
            mask_next = false;
            continue;
        }
        if index > 0 {
            let lower = arg.to_ascii_lowercase();
            if SECRET_FLAGS.contains(&lower.as_str()) {
                out.push(arg.clone());
                mask_next = true;
                continue;
            }
            if let Some((flag, _)) = arg.split_once('=')
                && SECRET_FLAGS.contains(&flag.to_ascii_lowercase().as_str())
            {
                out.push(format!("{flag}={MASK}"));
                continue;
            }
            if mysql_like && arg.len() > 2 && arg.starts_with("-p") {
                out.push(format!("-p{MASK}"));
                continue;
            }
        }
        out.push(redact_text(arg));
    }
    out
}

/// Redacts secrets inside free text: URL userinfo, bearer tokens, `key=value` pairs
/// whose key names a secret, known key prefixes and long high-entropy strings.
pub fn redact_text(text: &str) -> String {
    let text = URL_USERINFO.replace_all(text, format!("${{1}}{MASK}@"));
    let text = BEARER.replace_all(&text, format!("${{1}} {MASK}"));
    let text = PREFIXED.replace_all(&text, format!("${{1}}{MASK}"));
    let text = KEY_VALUE.replace_all(&text, format!("${{1}}{MASK}"));
    CANDIDATE
        .replace_all(&text, |caps: &regex::Captures<'_>| {
            let candidate = &caps[0];
            if looks_random(candidate) {
                MASK.to_string()
            } else {
                candidate.to_string()
            }
        })
        .into_owned()
}

/// At least 24 characters (guaranteed by `CANDIDATE`), letters and digits mixed,
/// and at least 3.5 bits of Shannon entropy per character.
fn looks_random(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
        && s.chars().any(|c| c.is_ascii_alphabetic())
        && shannon_entropy(s) >= 3.5
}

fn shannon_entropy(s: &str) -> f64 {
    let mut counts = [0usize; 256];
    for byte in s.bytes() {
        counts[byte as usize] += 1;
    }
    let len = s.len() as f64;
    counts
        .iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let p = count as f64 / len;
            -p * p.log2()
        })
        .sum()
}

```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p hm-core redact`
Expected: PASS — `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out`. That is 4 example tests and 5 property tests; the 7 filtered-out tests belong to `names` and `env`.

- [ ] **Step 5: Check formatting**

Run: `cargo fmt --all --check`
Expected: no output, exit code 0.

- [ ] **Step 6: Commit**

```bash
git add crates/hm-core/src/redact.rs crates/hm-core/src/lib.rs
git commit -s -m "feat(core): redact secrets from command lines"
```

### Task 4: Domain model, IDs, raw inventory and signature types

This task adds the rest of the shared types. `model` is the snapshot that `harbormap status --json` prints, so its field names are a public contract. `ids` derives stable IDs from content (spec §3), `raw` holds what the probes report before any interpretation, and `signature` holds the types that signature packs load into.

**Files:**
- Create: `crates/hm-core/src/model.rs`
- Create: `crates/hm-core/src/ids.rs`
- Create: `crates/hm-core/src/raw.rs`
- Create: `crates/hm-core/src/signature.rs`
- Modify: `crates/hm-core/src/lib.rs` (add `pub mod ids;`, `pub mod model;`, `pub mod raw;` and `pub mod signature;`)
- Test: `crates/hm-core/src/model.rs`, `crates/hm-core/src/ids.rs` (unit tests in the same files). `raw.rs` and `signature.rs` have no tests of their own.

**Interfaces:**
- Consumes: `hm_core::env::EnvFacts` (Task 2). It also uses the `blake3` and `serde` dependencies and the `serde_json` dev-dependency from Task 1.
- Produces, in `hm_core::model` (every struct field is `pub`; every type derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`; every enum is `snake_case` in JSON):
  - `pub const SNAPSHOT_SCHEMA: u32 = 1;`
  - `pub enum Role { See, Work, Keep, Outside, Shared, Unknown }` (also `Copy, Hash, PartialOrd, Ord`)
  - `pub enum Kind { DevServer, Database, Cache, Worker, System, Ide, Terminal, AgentCli, Unknown }` (also `Copy, Hash, PartialOrd, Ord`)
  - `pub enum Proto { Tcp }` (also `Copy, Default, Hash, PartialOrd, Ord`; the default is `Tcp`)
  - `pub enum BindScope { Loopback, All, Specific }` (also `Copy, Hash, PartialOrd, Ord`) with `pub fn of(addr: IpAddr) -> Self`
  - `pub fn canonical_ip(addr: IpAddr) -> IpAddr`
  - `pub enum Section { Project, Elsewhere, System }` (also `Copy, Hash, PartialOrd, Ord`)
  - `pub enum Owner { Process { pid: u32 }, ContainerForwarder { container_id: String }, System { reason: String, pid: Option<u32> }, Unknown { reason: String } }` (also `Hash`; JSON tag `"kind"`)
  - `pub enum Confidence { Sure, Probably }` (also `Copy, Hash`)
  - `pub struct Snapshot { schema: u32, taken_at: u64, host: Host, opened: Option<String>, projects: Vec<Project>, services: Vec<Service>, listeners: Vec<Listener>, processes: Vec<Process>, containers: Vec<Container>, engines: Vec<Engine>, notes: Vec<Note> }`
  - `pub struct Host { os: String, arch: String }`
  - `pub struct Project { id: String, name: String, root: PathBuf, opened: bool, git: bool, worktrees: Vec<Worktree> }`
  - `pub struct Worktree { path: PathBuf, branch: Option<String>, head: Option<String> }`
  - `pub struct Service { id: String, name: String, section: Section, project_id: Option<String>, worktree: Option<PathBuf>, signature: Option<String>, framework: Option<String>, confidence: Confidence, role: Role, kind: Kind, source: ServiceSource, ports: Vec<u16>, urls: Vec<String>, state: String, health: Option<String> }`
  - `pub enum ServiceSource { Process { main_pid: u32, root_pid: u32, pids: Vec<u32> }, Container { container_id: String } }` (JSON tag `"type"`)
  - `pub struct Listener { proto: Proto, addr: IpAddr, port: u16, bind: BindScope, owner: Owner, section: Section, project_id: Option<String>, service_id: Option<String>, label: String }`
  - `pub struct Process { pid: u32, ppid: Option<u32>, start_time: u64, name: String, exe: Option<PathBuf>, argv: Vec<String>, cwd: Option<PathBuf>, user: Option<String>, own: bool, signature: Option<String>, kind: Kind, project_id: Option<String>, worktree: Option<PathBuf>, rss_bytes: u64, uptime_s: u64, env: EnvFacts }`
  - `pub struct Container { id: String, engine: String, name: String, image: String, state: String, health: Option<String>, exit_code: Option<i64>, oom_killed: bool, restart_count: i64, started_at: Option<String>, ports: Vec<PortBinding>, networks: Vec<String>, mounts: Vec<Mount>, compose: Option<Compose>, signature: Option<String>, section: Section, project_id: Option<String> }`
  - `pub struct PortBinding { container_port: u16, proto: String, host_ip: Option<IpAddr>, host_port: Option<u16> }` (also `Hash`)
  - `pub struct Mount { kind: String, name: Option<String>, source: Option<PathBuf>, destination: String }`
  - `pub struct Compose { project: String, service: String, working_dir: Option<PathBuf>, config_files: Vec<PathBuf>, config_hash: Option<String>, oneoff: bool, number: Option<u32> }`
  - `pub struct Engine { id: String, name: String, endpoint: String, version: Option<String>, reachable: bool, error: Option<String> }`
  - `pub struct Note { code: String, text: String }`
- Produces, in `hm_core::ids`:
  - `pub fn project_id(canonical_root: &Path) -> String` (`p_` plus 12 hex characters)
  - `pub fn service_id(project_id: Option<&str>, worktree: Option<&Path>, name: &str) -> String` (`s_` plus 12 hex characters)
  - `pub fn engine_id(key: &str) -> String` (`e_` plus 12 hex characters)
- Produces, in `hm_core::raw` (every field is `pub`; every type derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`; all except `RawListener` also derive `Default` and are `#[serde(default)]`):
  - `pub struct RawInventory { taken_at: u64, os: String, arch: String, self_pid: u32, current_uid: Option<String>, processes: Vec<RawProcess>, listeners: Vec<RawListener>, containers: Vec<RawContainer>, engines: Vec<Engine>, notes: Vec<Note> }`
  - `pub struct RawProcess { pid: u32, ppid: Option<u32>, start_time: u64, name: String, exe: Option<PathBuf>, argv: Vec<String>, cwd: Option<PathBuf>, uid: Option<String>, user: Option<String>, rss_bytes: u64, env: EnvFacts }`
  - `pub struct RawListener { proto: Proto, addr: IpAddr, port: u16, pids: Vec<u32>, uid: Option<u32> }` (`addr` and `port` are required in JSON; the other fields have defaults)
  - `pub struct RawContainer { engine: String, id: String, name: String, image: String, state: String, health: Option<String>, exit_code: Option<i64>, oom_killed: bool, restart_count: i64, started_at: Option<String>, ports: Vec<PortBinding>, networks: Vec<String>, mounts: Vec<Mount>, labels: BTreeMap<String, String> }`
- Produces, in `hm_core::signature` (every struct field is `pub` except in `SignatureSet`; `Texts`, `Signature`, `When` and `Fixture` reject unknown keys with `#[serde(deny_unknown_fields)]`):
  - `pub enum Level { Beginner, Builder, Expert }` (derives `Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize`; the default is `Builder`)
  - `pub struct Texts { beginner: String, builder: String, expert: String }` (derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`) with `pub fn get(&self, level: Level) -> &str`
  - `pub struct Signature { id: String, name: String, role: Role, kind: Kind, default_port: Option<u16>, http: bool, forwarder: bool, priority: i32, os: Option<String>, restart_globs: Vec<String>, restart_globs_verified: bool, when: Vec<When>, texts: Texts, fixture: Vec<Fixture> }` (same derives as `Texts`; `priority` defaults to 50)
  - `pub struct When { exe: Vec<String>, argv_basename: Vec<String>, argv_contains: Vec<String>, argv_prefix: Vec<String>, ancestor_exe: Vec<String>, cwd_files: Vec<String>, package: Vec<String>, image: Vec<String>, probably: bool }` (derives `Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize`) with `pub fn is_empty(&self) -> bool`
  - `pub struct Fixture { exe: Option<String>, argv: Vec<String>, ancestors: Vec<String>, cwd_files: Vec<String>, packages: Vec<String>, image: Option<String>, os: Option<String> }` (same derives as `When`)
  - `pub struct SignatureSet` (one private field; derives `Debug, Clone, Default`) with `pub fn new(signatures: Vec<Signature>) -> Self` (sorts by priority, highest first, then by ID), `pub fn iter(&self) -> impl Iterator<Item = &Signature>`, `pub fn get(&self, id: &str) -> Option<&Signature>`, `pub fn len(&self) -> usize`, `pub fn is_empty(&self) -> bool`

- [ ] **Step 1: Write the failing test for the model**

The tests check how bind addresses are classified and how the JSON looks, because the output of `harbormap status --json` is a public contract.

Add `pub mod model;` after `pub mod env;` in `crates/hm-core/src/lib.rs`. Then create `crates/hm-core/src/model.rs` with only its test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_scope_classifies_addresses() {
        assert_eq!(
            BindScope::of("127.0.0.1".parse().unwrap()),
            BindScope::Loopback
        );
        assert_eq!(BindScope::of("::1".parse().unwrap()), BindScope::Loopback);
        assert_eq!(
            BindScope::of("::ffff:127.0.0.1".parse().unwrap()),
            BindScope::Loopback
        );
        assert_eq!(BindScope::of("0.0.0.0".parse().unwrap()), BindScope::All);
        assert_eq!(BindScope::of("::".parse().unwrap()), BindScope::All);
        assert_eq!(
            BindScope::of("192.168.1.20".parse().unwrap()),
            BindScope::Specific
        );
    }

    #[test]
    fn owner_serializes_with_a_kind_tag() {
        let json = serde_json::to_value(Owner::Process { pid: 812 }).unwrap();
        assert_eq!(json, serde_json::json!({"kind": "process", "pid": 812}));
        let json = serde_json::to_value(Owner::ContainerForwarder {
            container_id: "abc".into(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "container_forwarder", "container_id": "abc"})
        );
    }

    #[test]
    fn enums_use_snake_case() {
        assert_eq!(serde_json::to_value(Kind::DevServer).unwrap(), "dev_server");
        assert_eq!(serde_json::to_value(Role::See).unwrap(), "see");
        assert_eq!(
            serde_json::to_value(Section::Elsewhere).unwrap(),
            "elsewhere"
        );
        let source = ServiceSource::Container {
            container_id: "c1".into(),
        };
        assert_eq!(
            serde_json::to_value(source).unwrap(),
            serde_json::json!({"type": "container", "container_id": "c1"})
        );
    }
}
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p hm-core model`
Expected: FAIL — compile errors such as ``error[E0433]: cannot find type `BindScope` in this scope`` (and the same for `Owner`, `Kind`, `Role`, `Section` and `ServiceSource`).

- [ ] **Step 3: Write the model**

Add this above the `#[cfg(test)]` line in `crates/hm-core/src/model.rs`, ending with one empty line:

```rust
//! The snapshot: everything Harbormap knows about the computer at one instant, after
//! projects, services and owners have been resolved. `harbormap status --json` prints
//! this structure, so its field names are a public contract.

use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::env::EnvFacts;

/// Bumped when a field is renamed or removed. Adding a field does not bump it.
pub const SNAPSHOT_SCHEMA: u32 = 1;

/// Lane on the Harbor map (Product design §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    See,
    Work,
    Keep,
    Outside,
    Shared,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    DevServer,
    Database,
    Cache,
    Worker,
    System,
    Ide,
    Terminal,
    AgentCli,
    Unknown,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Proto {
    #[default]
    Tcp,
}

/// Who can reach a listening socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindScope {
    /// 127.0.0.0/8 or ::1: only this computer.
    Loopback,
    /// 0.0.0.0 or ::: every network interface.
    All,
    /// One specific interface address.
    Specific,
}

impl BindScope {
    pub fn of(addr: IpAddr) -> Self {
        let addr = canonical_ip(addr);
        if addr.is_loopback() {
            BindScope::Loopback
        } else if addr.is_unspecified() {
            BindScope::All
        } else {
            BindScope::Specific
        }
    }
}

/// Turns IPv4-mapped IPv6 addresses (`::ffff:127.0.0.1`) into plain IPv4.
pub fn canonical_ip(addr: IpAddr) -> IpAddr {
    match addr {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        IpAddr::V4(v4) => IpAddr::V4(v4),
    }
}

/// Which part of the screen an item belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    /// The project Harbormap was opened in.
    Project,
    /// Other projects and containers on this computer. Read-only.
    Elsewhere,
    /// System programs, apps outside projects and owners Harbormap cannot see.
    System,
}

/// Who holds a listening socket.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Owner {
    Process {
        pid: u32,
    },
    ContainerForwarder {
        container_id: String,
    },
    /// The operating system or an engine helper; `pid` when a process is visible.
    System {
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pid: Option<u32>,
    },
    Unknown {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Sure,
    Probably,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema: u32,
    /// Unix seconds when the probe ran.
    pub taken_at: u64,
    pub host: Host,
    /// ID of the project Harbormap was opened in, if any.
    pub opened: Option<String>,
    pub projects: Vec<Project>,
    pub services: Vec<Service>,
    pub listeners: Vec<Listener>,
    pub processes: Vec<Process>,
    pub containers: Vec<Container>,
    pub engines: Vec<Engine>,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    pub opened: bool,
    /// False for a plain folder without a git repository.
    pub git: bool,
    pub worktrees: Vec<Worktree>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub section: Section,
    /// `None` for containers that belong to no project.
    pub project_id: Option<String>,
    pub worktree: Option<PathBuf>,
    pub signature: Option<String>,
    pub framework: Option<String>,
    pub confidence: Confidence,
    pub role: Role,
    pub kind: Kind,
    pub source: ServiceSource,
    pub ports: Vec<u16>,
    pub urls: Vec<String>,
    pub state: String,
    pub health: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServiceSource {
    /// `main_pid` holds the port, `root_pid` is the top of its wrapper chain
    /// (for example `npm run dev`), and `pids` is the root plus all descendants.
    Process {
        main_pid: u32,
        root_pid: u32,
        pids: Vec<u32>,
    },
    Container {
        container_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listener {
    pub proto: Proto,
    pub addr: IpAddr,
    pub port: u16,
    pub bind: BindScope,
    pub owner: Owner,
    pub section: Section,
    pub project_id: Option<String>,
    pub service_id: Option<String>,
    /// Short human label for the owner, e.g. "web (Vite)" or "AirPlay Receiver".
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Process {
    pub pid: u32,
    pub ppid: Option<u32>,
    /// Unix seconds. Together with `pid` this is the process identity.
    pub start_time: u64,
    pub name: String,
    pub exe: Option<PathBuf>,
    /// Redacted before it ever reaches this struct.
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub user: Option<String>,
    /// True when the current OS user owns the process.
    pub own: bool,
    pub signature: Option<String>,
    pub kind: Kind,
    pub project_id: Option<String>,
    pub worktree: Option<PathBuf>,
    pub rss_bytes: u64,
    pub uptime_s: u64,
    pub env: EnvFacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Container {
    pub id: String,
    /// ID of the engine that runs it.
    pub engine: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub health: Option<String>,
    pub exit_code: Option<i64>,
    pub oom_killed: bool,
    pub restart_count: i64,
    pub started_at: Option<String>,
    pub ports: Vec<PortBinding>,
    pub networks: Vec<String>,
    pub mounts: Vec<Mount>,
    pub compose: Option<Compose>,
    pub signature: Option<String>,
    pub section: Section,
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PortBinding {
    pub container_port: u16,
    pub proto: String,
    pub host_ip: Option<IpAddr>,
    pub host_port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mount {
    /// "volume", "bind", "tmpfs", ...
    pub kind: String,
    /// Volume name, for volumes.
    pub name: Option<String>,
    /// Host path, for bind mounts.
    pub source: Option<PathBuf>,
    pub destination: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compose {
    pub project: String,
    pub service: String,
    pub working_dir: Option<PathBuf>,
    pub config_files: Vec<PathBuf>,
    pub config_hash: Option<String>,
    pub oneoff: bool,
    pub number: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Engine {
    pub id: String,
    /// "Docker Desktop", "OrbStack", "Colima", "Rancher Desktop", "Podman" or "Docker".
    pub name: String,
    pub endpoint: String,
    pub version: Option<String>,
    pub reachable: bool,
    pub error: Option<String>,
}

/// Something the user should know about the snapshot itself, such as a gap in what
/// Harbormap could see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub code: String,
    pub text: String,
}

```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p hm-core model`
Expected: PASS — `test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 16 filtered out`

- [ ] **Step 5: Write the failing test for IDs**

The test checks that IDs are stable for the same input, differ for different inputs, and look like `p_` or `s_` plus 12 hex characters.

Add `pub mod ids;` after `pub mod env;` in `crates/hm-core/src/lib.rs`. Then create `crates/hm-core/src/ids.rs` with only its test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_distinct() {
        let a = project_id(Path::new("/work/shop"));
        assert_eq!(a, project_id(Path::new("/work/shop")));
        assert_ne!(a, project_id(Path::new("/work/blog")));
        assert!(a.starts_with("p_") && a.len() == 14);

        let web = service_id(Some(&a), Some(Path::new("/work/shop")), "web");
        assert_eq!(
            web,
            service_id(Some(&a), Some(Path::new("/work/shop")), "web")
        );
        assert_ne!(
            web,
            service_id(Some(&a), Some(Path::new("/work/shop-wt")), "web")
        );
        assert_ne!(
            web,
            service_id(Some(&a), Some(Path::new("/work/shop")), "api")
        );
        assert!(web.starts_with("s_") && web.len() == 14);
    }
}
```

- [ ] **Step 6: Run the test to see it fail**

Run: `cargo test -p hm-core ids`
Expected: FAIL — compile errors such as ``error[E0425]: cannot find function `project_id` in this scope`` and ``error[E0433]: cannot find type `Path` in this scope``. The implementation's `use std::path::Path;` brings `Path` into scope for the tests.

- [ ] **Step 7: Write the ID functions**

Add this above the `#[cfg(test)]` line in `crates/hm-core/src/ids.rs`, ending with one empty line:

```rust
//! Stable IDs derived from content, never from counters (v0.1 spec §3).

use std::path::Path;

/// `p_` + 12 hex characters of BLAKE3 over the canonical project root.
pub fn project_id(canonical_root: &Path) -> String {
    let hash = blake3::hash(canonical_root.to_string_lossy().as_bytes());
    format!("p_{}", &hash.to_hex()[..12])
}

/// `s_` + 12 hex characters of BLAKE3 over project, worktree and service name, so a
/// service keeps its ID across restarts.
pub fn service_id(project_id: Option<&str>, worktree: Option<&Path>, name: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(project_id.unwrap_or("").as_bytes());
    hasher.update(&[0]);
    if let Some(worktree) = worktree {
        hasher.update(worktree.to_string_lossy().as_bytes());
    }
    hasher.update(&[0]);
    hasher.update(name.as_bytes());
    format!("s_{}", &hasher.finalize().to_hex()[..12])
}

/// `e_` + 12 hex characters of BLAKE3 over the engine's own ID (or its endpoint when
/// the engine reports none), so two sockets that reach one daemon share an ID.
pub fn engine_id(key: &str) -> String {
    format!("e_{}", &blake3::hash(key.as_bytes()).to_hex()[..12])
}

```

- [ ] **Step 8: Run the tests to see them pass**

Run: `cargo test -p hm-core ids`
Expected: PASS — `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 19 filtered out`

- [ ] **Step 9: Write the raw inventory types**

`raw.rs` has no tests of its own. The compiler checks it in the next step. Later, Tasks 5, 6 and 8 fill these types from the probes, and Task 12 loads them from recorded JSON.

Add `pub mod raw;` after `pub mod names;` in `crates/hm-core/src/lib.rs`. Then create `crates/hm-core/src/raw.rs`:

```rust
//! What the probes report before any interpretation.
//!
//! Debug builds of the CLI can load a recorded inventory from JSON
//! (`HARBORMAP_FIXTURE`), which keeps resolution tests independent of the machine
//! they run on. Every optional field therefore has a serde default.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::env::EnvFacts;
use crate::model::{Engine, Mount, Note, PortBinding, Proto};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RawInventory {
    /// Unix seconds when the probe ran.
    pub taken_at: u64,
    /// `std::env::consts::OS` of the probing machine.
    pub os: String,
    pub arch: String,
    /// PID of the Harbormap process that ran the probe.
    pub self_pid: u32,
    /// Platform user key of the current user: the uid on Unix, the SID on Windows.
    pub current_uid: Option<String>,
    pub processes: Vec<RawProcess>,
    pub listeners: Vec<RawListener>,
    pub containers: Vec<RawContainer>,
    pub engines: Vec<Engine>,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RawProcess {
    pub pid: u32,
    pub ppid: Option<u32>,
    /// Unix seconds.
    pub start_time: u64,
    pub name: String,
    pub exe: Option<PathBuf>,
    /// Already redacted.
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Same format as `RawInventory::current_uid`.
    pub uid: Option<String>,
    pub user: Option<String>,
    pub rss_bytes: u64,
    pub env: EnvFacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawListener {
    #[serde(default)]
    pub proto: Proto,
    pub addr: IpAddr,
    pub port: u16,
    /// Every process that holds the socket. Empty when the owner is not visible.
    #[serde(default)]
    pub pids: Vec<u32>,
    /// Socket owner uid when the OS reports one without a visible process (Linux).
    #[serde(default)]
    pub uid: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RawContainer {
    /// ID of the engine that reported it.
    pub engine: String,
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub health: Option<String>,
    pub exit_code: Option<i64>,
    pub oom_killed: bool,
    pub restart_count: i64,
    pub started_at: Option<String>,
    pub ports: Vec<PortBinding>,
    pub networks: Vec<String>,
    pub mounts: Vec<Mount>,
    /// Only `com.docker.compose.*` labels are kept.
    pub labels: BTreeMap<String, String>,
}
```

- [ ] **Step 10: Build to check the raw types**

Run: `cargo build -p hm-core`
Expected: success — ``Finished `dev` profile [unoptimized + debuginfo] target(s)``, with no errors and no warnings.

- [ ] **Step 11: Write the signature-pack types**

`signature.rs` has no tests of its own either. The compiler checks it in the next step. Later, Task 9 loads the signature packs into these types, and Task 11 checks every pack fixture against them.

Add `pub mod signature;` after `pub mod redact;` in `crates/hm-core/src/lib.rs`. Then create `crates/hm-core/src/signature.rs`:

```rust
//! Types for signature packs (`packs/signatures/*.toml`). Loading and validation live
//! in `hm-doctor`; matching lives in `hm-project`.

use serde::{Deserialize, Serialize};

use crate::model::{Kind, Role};

/// The three explanation levels (Product design §3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Beginner,
    #[default]
    Builder,
    Expert,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Texts {
    pub beginner: String,
    pub builder: String,
    pub expert: String,
}

impl Texts {
    pub fn get(&self, level: Level) -> &str {
        match level {
            Level::Beginner => &self.beginner,
            Level::Builder => &self.builder,
            Level::Expert => &self.expert,
        }
    }
}

/// How to recognise one program, and what to say about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    /// Lowercase letters, digits and dashes; unique across all packs.
    pub id: String,
    /// Display name, e.g. "Vite" or "PostgreSQL".
    pub name: String,
    pub role: Role,
    pub kind: Kind,
    #[serde(default)]
    pub default_port: Option<u16>,
    /// The program serves HTTP, so a port becomes an `http://localhost:<port>` link.
    #[serde(default)]
    pub http: bool,
    /// The program forwards ports for containers (Docker Desktop, OrbStack, ...).
    #[serde(default)]
    pub forwarder: bool,
    /// Higher wins when several signatures match. Generic runtimes use 10.
    #[serde(default = "default_priority")]
    pub priority: i32,
    /// Restricts the signature to one OS ("macos", "linux" or "windows").
    #[serde(default)]
    pub os: Option<String>,
    /// Files whose change needs a restart (used by STALE-001 from alpha.2).
    #[serde(default)]
    pub restart_globs: Vec<String>,
    #[serde(default)]
    pub restart_globs_verified: bool,
    /// Alternatives: the signature matches when any `when` block matches.
    pub when: Vec<When>,
    pub texts: Texts,
    /// Examples that must match this signature; checked by tests.
    #[serde(default)]
    pub fixture: Vec<Fixture>,
}

fn default_priority() -> i32 {
    50
}

/// One way to recognise a program. Every listed condition must hold; inside one
/// condition, any listed value may match. Comparisons ignore case.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct When {
    /// Executable stem (`node`, `postgres`); a trailing `*` matches a prefix (`python*`).
    pub exe: Vec<String>,
    /// Basename of any argument (argv[0] included), ignoring `.js`, `.cjs`, `.mjs`, `.ts`, `.py`, `.exe`, `.cmd`.
    pub argv_basename: Vec<String>,
    /// Substring of any argument, case-sensitive.
    pub argv_contains: Vec<String>,
    /// Prefix of any argument, including argv[0] (`next-server`).
    pub argv_prefix: Vec<String>,
    /// Executable stem of any ancestor process.
    pub ancestor_exe: Vec<String>,
    /// File name or `*.ext` pattern present in the working directory.
    pub cwd_files: Vec<String>,
    /// Dependency declared in the nearest package.json, pyproject.toml or requirements file.
    pub package: Vec<String>,
    /// Last path segment of a container image, without tag or digest.
    pub image: Vec<String>,
    /// The match is a good guess rather than certain; the UI says "probably".
    pub probably: bool,
}

impl When {
    pub fn is_empty(&self) -> bool {
        self.exe.is_empty()
            && self.argv_basename.is_empty()
            && self.argv_contains.is_empty()
            && self.argv_prefix.is_empty()
            && self.ancestor_exe.is_empty()
            && self.cwd_files.is_empty()
            && self.package.is_empty()
            && self.image.is_empty()
    }
}

/// A process or container description that must match its signature.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Fixture {
    pub exe: Option<String>,
    pub argv: Vec<String>,
    pub ancestors: Vec<String>,
    pub cwd_files: Vec<String>,
    pub packages: Vec<String>,
    pub image: Option<String>,
    pub os: Option<String>,
}

/// Signatures in match order: highest priority first, then by ID.
#[derive(Debug, Clone, Default)]
pub struct SignatureSet {
    signatures: Vec<Signature>,
}

impl SignatureSet {
    pub fn new(mut signatures: Vec<Signature>) -> Self {
        signatures.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.id.cmp(&b.id)));
        Self { signatures }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Signature> {
        self.signatures.iter()
    }

    pub fn get(&self, id: &str) -> Option<&Signature> {
        self.signatures.iter().find(|signature| signature.id == id)
    }

    pub fn len(&self) -> usize {
        self.signatures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.signatures.is_empty()
    }
}
```

- [ ] **Step 12: Build to check the signature types**

Run: `cargo build -p hm-core`
Expected: success — ``Finished `dev` profile [unoptimized + debuginfo] target(s)``, with no errors and no warnings.

`crates/hm-core/src/lib.rs` now reads exactly:

```rust
//! Shared building blocks for every Harbormap crate: the snapshot model, the raw
//! inventory that probes produce, signature-pack types, stable IDs, argv redaction
//! and the narrow rules for reading environment values.

pub mod env;
pub mod ids;
pub mod model;
pub mod names;
pub mod raw;
pub mod redact;
pub mod signature;
```

- [ ] **Step 13: Run every `hm-core` test**

Run: `cargo test -p hm-core`
Expected: PASS — `test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out` (2 `names`, 5 `env`, 9 `redact`, 3 `model` and 1 `ids` test), followed by the doc-test run with `0 passed`.

- [ ] **Step 14: Check formatting**

Run: `cargo fmt --all --check`
Expected: no output, exit code 0.

- [ ] **Step 15: Commit**

```bash
git add crates/hm-core/src/lib.rs crates/hm-core/src/model.rs crates/hm-core/src/ids.rs crates/hm-core/src/raw.rs crates/hm-core/src/signature.rs
git commit -s -m "feat(core): add snapshot model, stable IDs, raw inventory and signature types"
```

### Task 5: Process table probe

This task starts the `hm-probe` crate with its first OS adapter: one read of the process table through `sysinfo`. Each process is reduced to what Harbormap may keep: redacted argv, environment variable *names*, and only the values the secrets rule allows (`PORT`-like integers and `*_URL`-like targets, spec §15). On macOS, a process that renamed itself (npm and Next.js do, through node) leaves environment entries, values included, at the end of its argv as the OS reports it; the probe drops that tail, so no environment value leaves the probe. A live test starts a child process with known environment variables and checks that the probe sees its working folder and those facts, but never a secret value.

**Files:**
- Create: `crates/hm-probe/src/processes.rs`
- Create: `crates/hm-probe/tests/live.rs`
- Modify: `crates/hm-probe/src/lib.rs` (replace the Task 1 placeholder with the crate doc comment and `pub mod processes;`)
- Test: `crates/hm-probe/src/processes.rs` (unit tests in the same file), `crates/hm-probe/tests/live.rs` (live test on the real machine)

**Interfaces:**
- Consumes: `hm_core::env::extract_from_pairs<I, S>(entries: I) -> EnvFacts where I: IntoIterator<Item = S>, S: AsRef<str>` (Task 2); `hm_core::env::{EnvFacts, EnvPort, EnvUrl}` (Task 2, checked by the live test); `hm_core::redact::redact_argv(argv: &[String]) -> Vec<String>` (Task 3); `hm_core::raw::RawProcess` (Task 4)
- Produces (in `hm_probe::processes`): `pub struct ProcessTable { pub processes: Vec<RawProcess>, pub self_pid: u32, pub current_uid: Option<String> }`, `pub fn read_processes() -> ProcessTable`, `pub fn clean_cwd(cwd: &Path) -> PathBuf`

- [ ] **Step 1: Write the failing test**

Set `crates/hm-probe/src/lib.rs` to exactly this (it replaces the placeholder from Task 1):

```rust
//! OS adapters: what runs on this computer, before any interpretation.

pub mod processes;
```

Create `crates/hm-probe/src/processes.rs` with only its test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleted_working_folders_keep_their_original_path() {
        assert_eq!(
            clean_cwd(Path::new("/work/shop/apps/web (deleted)")),
            PathBuf::from("/work/shop/apps/web")
        );
        assert_eq!(
            clean_cwd(Path::new("/work/shop")),
            PathBuf::from("/work/shop")
        );
    }

    #[test]
    fn environment_entries_left_by_a_renamed_process_are_dropped() {
        // What macOS returns for node after `process.title = "npm run dev"`, and for perl
        // after `$0 = "worker"`: the title, perhaps a blank, then environment entries.
        let mut node = args(&[
            "npm run dev",
            "COMMAND_MODE=unix2003",
            "HOME=/Users/me",
            "API_TOKEN=s3cr3t",
        ]);
        drop_environment_tail(&mut node);
        assert_eq!(node, args(&["npm run dev"]));

        let mut perl = args(&["worker", "        ", "HOME=/Users/me"]);
        drop_environment_tail(&mut perl);
        assert_eq!(perl, args(&["worker"]));

        // Ordinary arguments stay, including a `NAME=value` that is not last.
        let make = args(&["make", "CC=clang", "-j4"]);
        let mut kept = make.clone();
        drop_environment_tail(&mut kept);
        assert_eq!(kept, make);

        let vite = args(&["node", "vite", "--port=5173", "http://localhost/?a=b"]);
        let mut kept = vite.clone();
        drop_environment_tail(&mut kept);
        assert_eq!(kept, vite);
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }
}
```

Create `crates/hm-probe/tests/live.rs`. `child_sleeper` is an ignored test that the live test starts as a child process (the test binary runs itself again with `child_sleeper --exact --ignored`), so the probe has a process with a known working folder and environment to find. `KillOnDrop` stops the child even when an assertion fails.

```rust
//! Probes the real machine: sockets and processes this test creates must show up.

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use hm_probe::processes::read_processes;

/// Spawned by other tests as a long-lived child; does nothing when run normally.
#[test]
#[ignore = "helper process for other tests"]
fn child_sleeper() {
    if std::env::var_os("HM_CHILD").is_some() {
        std::thread::sleep(Duration::from_secs(30));
    }
}

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_child(cwd: &std::path::Path) -> KillOnDrop {
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["child_sleeper", "--exact", "--ignored", "--nocapture"])
        .current_dir(cwd)
        .env("HM_CHILD", "1")
        .env("PORT", "5999")
        .env("DATABASE_URL", "postgres://app:pw12345@db.local:5432/shop")
        .env("HM_TEST_SECRET", "topsecret123")
        .spawn()
        .unwrap();
    KillOnDrop(child)
}

fn wait_until<T>(mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < deadline, "condition not met within 10 s");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn processes_include_cwd_and_env_facts_but_no_secret_values() {
    let dir = tempfile::tempdir().unwrap();
    let child = spawn_child(dir.path());
    let pid = child.0.id();
    let process = wait_until(|| {
        read_processes()
            .processes
            .into_iter()
            .find(|process| process.pid == pid && process.cwd.is_some())
    });
    assert_eq!(
        dunce::canonicalize(process.cwd.as_ref().unwrap()).unwrap(),
        dunce::canonicalize(dir.path()).unwrap()
    );
    assert!(
        process
            .env
            .names
            .iter()
            .any(|name| name == "HM_TEST_SECRET")
    );
    assert!(
        process
            .env
            .ports
            .iter()
            .any(|p| p.name == "PORT" && p.port == 5999)
    );
    assert!(
        process.env.urls.iter().any(|url| url.name == "DATABASE_URL"
            && url.host == "db.local"
            && url.port == Some(5432))
    );
    let json = serde_json::to_string(&process).unwrap();
    assert!(!json.contains("topsecret123"));
    assert!(!json.contains("pw12345"));
}
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p hm-probe processes`
Expected: FAIL — compile errors such as `` error[E0425]: cannot find function `clean_cwd` in this scope ``, the same for `drop_environment_tail`, and `` error[E0432]: unresolved import `hm_probe::processes::read_processes` ``.

- [ ] **Step 3: Write the implementation**

Add this above the `#[cfg(test)]` line in `crates/hm-probe/src/processes.rs`, with one blank line between them:

```rust
//! The process table, read through sysinfo and reduced to what Harbormap may keep:
//! redacted argv and environment *names* plus the allowed values.

use std::path::{Path, PathBuf};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, Uid, UpdateKind, Users};

use hm_core::env;
use hm_core::raw::RawProcess;
use hm_core::redact::redact_argv;

pub struct ProcessTable {
    pub processes: Vec<RawProcess>,
    pub self_pid: u32,
    pub current_uid: Option<String>,
}

pub fn read_processes() -> ProcessTable {
    let refresh = ProcessRefreshKind::nothing()
        .with_memory()
        .with_user(UpdateKind::Always)
        .with_cwd(UpdateKind::Always)
        .with_cmd(UpdateKind::Always)
        .with_exe(UpdateKind::Always)
        .with_environ(UpdateKind::Always)
        .without_tasks();
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
    let users = Users::new_with_refreshed_list();

    let self_pid = sysinfo::get_current_pid().map_or(std::process::id(), Pid::as_u32);
    let current_uid = system
        .process(Pid::from_u32(self_pid))
        .and_then(|process| process.user_id())
        .map(uid_key);

    let mut processes: Vec<RawProcess> = system
        .processes()
        .values()
        .map(|process| {
            let name = process.name().to_string_lossy().into_owned();
            let mut argv: Vec<String> = process
                .cmd()
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            if cfg!(target_os = "macos") {
                drop_environment_tail(&mut argv);
            }
            if argv.is_empty() {
                argv.push(name.clone());
            }
            RawProcess {
                pid: process.pid().as_u32(),
                ppid: process.parent().map(Pid::as_u32),
                start_time: process.start_time(),
                name,
                exe: process.exe().map(ToOwned::to_owned),
                argv: redact_argv(&argv),
                cwd: process.cwd().map(clean_cwd),
                uid: process.user_id().map(uid_key),
                user: process
                    .user_id()
                    .and_then(|uid| users.get_user_by_id(uid))
                    .map(|user| user.name().to_string()),
                rss_bytes: process.memory(),
                env: env::extract_from_pairs(
                    process
                        .environ()
                        .iter()
                        .map(|entry| entry.to_string_lossy()),
                ),
            }
        })
        .collect();
    processes.sort_by_key(|process| process.pid);
    ProcessTable {
        processes,
        self_pid,
        current_uid,
    }
}

/// Linux reports a working folder that was removed as "/path (deleted)". The
/// process still belongs where it started, so the suffix is dropped.
pub fn clean_cwd(cwd: &Path) -> PathBuf {
    match cwd
        .to_str()
        .and_then(|text| text.strip_suffix(" (deleted)"))
    {
        Some(original) => PathBuf::from(original),
        None => cwd.to_path_buf(),
    }
}

/// On macOS, a process that renamed itself (node, which runs npm and Next.js, or perl)
/// keeps its new title in argv[0] and blanks the rest of its original arguments. The
/// argument reader skips the blanks and returns the first environment entries in their
/// place, values included. Environment values must never leave the probe (spec §15), so
/// a run of `NAME=value` or blank arguments at the end is dropped. Real argument lists
/// rarely end that way, and dropping one loses only display detail.
fn drop_environment_tail(argv: &mut Vec<String>) {
    while argv
        .last()
        .is_some_and(|arg| arg.trim().is_empty() || is_environment_entry(arg))
    {
        argv.pop();
    }
}

/// `NAME=value`, where NAME is a letter or `_` followed by letters, digits or `_`.
fn is_environment_entry(arg: &str) -> bool {
    arg.split_once('=').is_some_and(|(name, _)| {
        name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The uid on Unix, the SID string on Windows.
fn uid_key(uid: &Uid) -> String {
    (**uid).to_string()
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p hm-probe processes`
Expected: PASS — unit tests `test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`; `tests/live.rs` `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out`. The live output shows one extra `running 1 test` line: it is printed by the child process and is expected.

- [ ] **Step 5: Commit**

```bash
git add crates/hm-probe/src/lib.rs crates/hm-probe/src/processes.rs crates/hm-probe/tests/live.rs
git commit -s -m "feat(probe): read the process table with redacted argv and env facts"
```

### Task 6: Listener probes

This task adds the second OS adapter: every listening TCP socket on the computer, with the PIDs that hold it, read the way each OS allows without admin rights. `merge` then folds the raw rows so each socket appears once: IPv4-mapped addresses become plain IPv4 and the owner PIDs are unioned.

Why two sources on macOS: libproc (through the `listeners` crate) only sees sockets of processes the current user may inspect, so servers run by root or by other users would be missing. `netstat -anv -p tcp` shows every socket with its PID without root, so its rows fill that gap; rows whose PID libproc already reported are skipped. Why an empty `pids` list on Linux: `/proc/net/tcp{,6}` lists every listening socket, but the `/proc/<pid>/fd` links that tie a socket to its process cannot be read for other users' processes without root. Those sockets are kept with an empty `pids` list (and the socket's `uid`), so the port still shows up with an honest "owner not visible" instead of disappearing (spec §6.1, "Honest gaps"). Windows reports every owner PID through `GetExtendedTcpTable` without admin rights. The `listeners` crate is a dependency only on macOS and Windows (Task 1), so `macos.rs` and `windows.rs` compile only there; the `/proc` and `netstat` parsers are plain functions that compile and are unit-tested on every OS.

**Files:**
- Create: `crates/hm-probe/src/listeners/mod.rs`
- Create: `crates/hm-probe/src/listeners/linux.rs`
- Create: `crates/hm-probe/src/listeners/netstat.rs`
- Create: `crates/hm-probe/src/listeners/macos.rs`
- Create: `crates/hm-probe/src/listeners/windows.rs`
- Modify: `crates/hm-probe/src/lib.rs` (add `pub mod listeners;`)
- Modify: `crates/hm-probe/tests/live.rs` (add `listeners_include_a_socket_this_process_holds` and its imports)
- Test: `crates/hm-probe/src/listeners/{mod.rs,linux.rs,netstat.rs}` (unit tests in the same files), `crates/hm-probe/tests/live.rs` (live test)

**Interfaces:**
- Consumes: `hm_core::model::Proto` and `hm_core::model::canonical_ip(addr: IpAddr) -> IpAddr` (Task 4); `hm_core::raw::RawListener { pub proto: Proto, pub addr: IpAddr, pub port: u16, pub pids: Vec<u32>, pub uid: Option<u32> }` (Task 4); the `listeners` crate (`listeners::get_all()`, `listeners::Protocol::TCP`, `listeners::SocketState::Listen`) on macOS and Windows only (Task 1)
- Produces:
  - `hm_probe::listeners`: `pub fn read_listeners() -> Result<Vec<RawListener>, String>`, `pub fn merge(raw: Vec<RawListener>) -> Vec<RawListener>`
  - `hm_probe::listeners::linux`: `pub struct ProcSocket { pub addr: IpAddr, pub port: u16, pub uid: u32, pub inode: u64 }`, `pub fn parse_proc_net_tcp(text: &str) -> Vec<ProcSocket>`, `pub fn parse_socket_link(link: &str) -> Option<u64>`, and on Linux only `pub fn read() -> Result<Vec<hm_core::raw::RawListener>, String>`
  - `hm_probe::listeners::netstat`: `pub struct NetstatRow { pub addr: IpAddr, pub port: u16, pub pid: Option<u32>, pub process: Option<String> }`, `pub fn parse(text: &str) -> Vec<NetstatRow>`
  - private modules `macos` (macOS only) and `windows` (Windows only), each with `pub fn read() -> Result<Vec<RawListener>, String>`

- [ ] **Step 1: Write the failing test**

Set `crates/hm-probe/src/lib.rs` to:

```rust
//! OS adapters: what runs on this computer, before any interpretation.

pub mod listeners;
pub mod processes;
```

Create `crates/hm-probe/src/listeners/mod.rs` with only its test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn listener(addr: &str, port: u16, pids: &[u32]) -> RawListener {
        RawListener {
            proto: Proto::Tcp,
            addr: addr.parse().unwrap(),
            port,
            pids: pids.to_vec(),
            uid: None,
        }
    }

    #[test]
    fn merge_unions_pids_of_the_same_socket() {
        let merged = merge(vec![
            listener("127.0.0.1", 8000, &[20]),
            listener("::ffff:127.0.0.1", 8000, &[10, 20]),
            listener("::", 8000, &[30]),
        ]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].addr, "127.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(merged[0].pids, vec![10, 20]);
        assert_eq!(merged[1].pids, vec![30]);
    }
}
```

Create `crates/hm-probe/src/listeners/linux.rs` with only its test module. The fixture rows are real `/proc/net/tcp` lines; the kernel prints addresses as hex in host byte order, so the tests run on little-endian machines (every CI runner):

```rust
#[cfg(all(test, target_endian = "little"))]
mod tests {
    use super::*;

    const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41234 1 0000000000000000 100 0 0 10 0
   1: 00000000:1538 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 41300 1 0000000000000000 100 0 0 10 0
   2: 0100007F:D431 0100007F:1F90 01 00000000:00000000 00:00000000 00000000  1000        0 41400 1 0000000000000000 20 4 30 10 -1
";

    const TCP6: &str = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:0BB8 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 52000 1 0000000000000000 100 0 0 10 0
   1: 00000000000000000000000000000000:1F40 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 52001 1 0000000000000000 100 0 0 10 0
";

    #[test]
    fn parses_ipv4_listeners_and_skips_other_states() {
        let sockets = parse_proc_net_tcp(TCP);
        assert_eq!(
            sockets,
            vec![
                ProcSocket {
                    addr: "127.0.0.1".parse().unwrap(),
                    port: 8080,
                    uid: 1000,
                    inode: 41234
                },
                ProcSocket {
                    addr: "0.0.0.0".parse().unwrap(),
                    port: 5432,
                    uid: 0,
                    inode: 41300
                },
            ]
        );
    }

    #[test]
    fn parses_ipv6_listeners() {
        let sockets = parse_proc_net_tcp(TCP6);
        assert_eq!(sockets[0].addr, "::1".parse::<IpAddr>().unwrap());
        assert_eq!(sockets[0].port, 3000);
        assert_eq!(sockets[1].addr, "::".parse::<IpAddr>().unwrap());
        assert_eq!(sockets[1].port, 8000);
    }

    #[test]
    fn parses_socket_links() {
        assert_eq!(parse_socket_link("socket:[41234]"), Some(41234));
        assert_eq!(parse_socket_link("pipe:[123]"), None);
        assert_eq!(parse_socket_link("/dev/null"), None);
    }
}
```

Create `crates/hm-probe/src/listeners/netstat.rs` with only its test module. `MODERN` is recent macOS output, whose `process:pid` column can contain spaces; `LEGACY` has the older numeric `pid` column:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const MODERN: &str = "Active Internet connections (including servers)
Proto Recv-Q Send-Q  Local Address          Foreign Address        (state)          rxbytes      txbytes  rhiwat  shiwat          process:pid    state  options           gencnt    flags   flags1 usecnt rtncnt fltrs
tcp4       0      0  10.0.0.5.55810         160.79.104.10.443      ESTABLISHED         7868         9585  131072  131072           claude:31903  00102 00000000 0000000000a0e5ca 00000081 04000900      2      0 000000
tcp4       0      0  127.0.0.1.9820         *.*                    LISTEN                 0            0  131072  131072    Google Chrome:76915  00100 00000006 0000000000858d85 00000000 00000800      1      0 000000
tcp46      0      0  *.3330                 *.*                    LISTEN                 0            0  131072  131072             node:89507  00100 00000106 00000000003e6d86 00000001 00000800      1      0 000000
tcp6       0      0  ::1.631                *.*                    LISTEN                 0            0  131072  131072            cupsd:412    00100 00000106 00000000003e6d87 00000001 00000800      1      0 000000
tcp4       0      0  *.5000                 *.*                    LISTEN                 0            0  131072  131072    ControlCenter:640  00100 00000106 00000000003e6d88 00000001 00000800      1      0 000000
";

    const LEGACY: &str = "Active Internet connections (including servers)
Proto Recv-Q Send-Q  Local Address          Foreign Address        (state)     rhiwat shiwat    pid   epid  state    options
tcp4       0      0  127.0.0.1.8080         *.*                    LISTEN      131072 131072   1234      0 0x0100 0x00000006
";

    #[test]
    fn parses_modern_rows_with_spaces_in_process_names() {
        let rows = parse(MODERN);
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows[0],
            NetstatRow {
                addr: "127.0.0.1".parse().unwrap(),
                port: 9820,
                pid: Some(76915),
                process: Some("Google Chrome".into())
            }
        );
        assert_eq!(rows[1].addr, "::".parse::<IpAddr>().unwrap());
        assert_eq!(rows[1].port, 3330);
        assert_eq!(rows[2].addr, "::1".parse::<IpAddr>().unwrap());
        assert_eq!(rows[3].addr, "0.0.0.0".parse::<IpAddr>().unwrap());
        assert_eq!(rows[3].process.as_deref(), Some("ControlCenter"));
    }

    #[test]
    fn parses_legacy_numeric_pid_column() {
        assert_eq!(
            parse(LEGACY),
            vec![NetstatRow {
                addr: "127.0.0.1".parse().unwrap(),
                port: 8080,
                pid: Some(1234),
                process: None
            }]
        );
    }

    #[test]
    fn returns_nothing_without_a_header() {
        assert!(parse("garbage\nmore garbage").is_empty());
    }
}
```

`linux.rs` and `netstat.rs` are declared by the `pub mod` lines that Step 3 adds to `listeners/mod.rs`, so their tests are first compiled in Step 4.

In `crates/hm-probe/tests/live.rs`, replace the `use` lines at the top with:

```rust
use std::net::TcpListener;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use hm_probe::listeners::read_listeners;
use hm_probe::processes::read_processes;
```

Then insert this test between `wait_until` and `processes_include_cwd_and_env_facts_but_no_secret_values`, with one blank line on each side:

```rust
#[test]
fn listeners_include_a_socket_this_process_holds() {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    let listeners = read_listeners().unwrap();
    let ours = listeners
        .iter()
        .find(|listener| listener.port == port)
        .expect("our port is listed");
    assert_eq!(ours.addr, "127.0.0.1".parse::<std::net::IpAddr>().unwrap());
    assert!(
        ours.pids.contains(&std::process::id()),
        "pids: {:?}",
        ours.pids
    );
}
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p hm-probe listeners`
Expected: FAIL — compile errors such as `` error[E0425]: cannot find function `merge` in this scope `` and `` error[E0432]: unresolved import `hm_probe::listeners::read_listeners` ``.

- [ ] **Step 3: Write the implementation**

Add this above the `#[cfg(test)]` line in `crates/hm-probe/src/listeners/mod.rs`, with one blank line between them:

```rust
//! Listening TCP sockets and the processes that hold them.
//!
//! - Linux: `/proc/net/tcp{,6}` plus socket inodes under `/proc/<pid>/fd`. Sockets
//!   whose owner we cannot see are kept with an empty `pids` list.
//! - macOS: libproc (through the `listeners` crate) for our own processes, plus
//!   `netstat -anv -p tcp` for other users' processes, which it shows without root.
//! - Windows: `GetExtendedTcpTable` (through the `listeners` crate), which reports
//!   every owner PID without admin rights.

use std::collections::BTreeMap;
use std::net::IpAddr;

use hm_core::model::{Proto, canonical_ip};
use hm_core::raw::RawListener;

pub mod linux;
pub mod netstat;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub fn read_listeners() -> Result<Vec<RawListener>, String> {
    platform_read().map(merge)
}

#[cfg(target_os = "linux")]
fn platform_read() -> Result<Vec<RawListener>, String> {
    linux::read()
}

#[cfg(target_os = "macos")]
fn platform_read() -> Result<Vec<RawListener>, String> {
    macos::read()
}

#[cfg(target_os = "windows")]
fn platform_read() -> Result<Vec<RawListener>, String> {
    windows::read()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_read() -> Result<Vec<RawListener>, String> {
    Err("this operating system is not supported".into())
}

/// One entry per (protocol, address, port), with the union of owner PIDs.
pub fn merge(raw: Vec<RawListener>) -> Vec<RawListener> {
    let mut merged: BTreeMap<(Proto, IpAddr, u16), RawListener> = BTreeMap::new();
    for listener in raw {
        let addr = canonical_ip(listener.addr);
        let entry = merged
            .entry((listener.proto, addr, listener.port))
            .or_insert_with(|| RawListener {
                proto: listener.proto,
                addr,
                port: listener.port,
                pids: Vec::new(),
                uid: None,
            });
        for pid in listener.pids {
            if !entry.pids.contains(&pid) {
                entry.pids.push(pid);
            }
        }
        entry.uid = entry.uid.or(listener.uid);
    }
    merged
        .into_values()
        .map(|mut listener| {
            listener.pids.sort_unstable();
            listener
        })
        .collect()
}
```

Add this above the `#[cfg(all(test, target_endian = "little"))]` line in `crates/hm-probe/src/listeners/linux.rs`, with one blank line between them:

```rust
//! Linux: parse `/proc/net/tcp` and `/proc/net/tcp6`, then map socket inodes to
//! the processes whose file descriptors point at them.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// One listening socket from a `/proc/net/tcp*` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcSocket {
    pub addr: IpAddr,
    pub port: u16,
    pub uid: u32,
    pub inode: u64,
}

const TCP_LISTEN: &str = "0A";

/// Parses a `/proc/net/tcp` (IPv4) or `/proc/net/tcp6` (IPv6) table and keeps the
/// sockets in LISTEN state. Addresses are printed in host byte order per 32-bit word.
pub fn parse_proc_net_tcp(text: &str) -> Vec<ProcSocket> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 10 || cols[3] != TCP_LISTEN {
                return None;
            }
            let (addr_hex, port_hex) = cols[1].split_once(':')?;
            Some(ProcSocket {
                addr: parse_hex_addr(addr_hex)?,
                port: u16::from_str_radix(port_hex, 16).ok()?,
                uid: cols[7].parse().ok()?,
                inode: cols[9].parse().ok()?,
            })
        })
        .collect()
}

fn parse_hex_addr(hex: &str) -> Option<IpAddr> {
    let word = |chunk: &str| u32::from_str_radix(chunk, 16).ok().map(u32::to_ne_bytes);
    match hex.len() {
        8 => Some(IpAddr::V4(Ipv4Addr::from(word(hex)?))),
        32 => {
            let mut bytes = [0u8; 16];
            for (index, out) in bytes.chunks_mut(4).enumerate() {
                out.copy_from_slice(&word(&hex[index * 8..index * 8 + 8])?);
            }
            Some(IpAddr::V6(Ipv6Addr::from(bytes)))
        }
        _ => None,
    }
}

/// `socket:[12345]` → `12345`.
pub fn parse_socket_link(link: &str) -> Option<u64> {
    link.strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

#[cfg(target_os = "linux")]
pub fn read() -> Result<Vec<hm_core::raw::RawListener>, String> {
    use std::collections::HashMap;

    use hm_core::model::Proto;
    use hm_core::raw::RawListener;

    let mut sockets = Vec::new();
    for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
        match std::fs::read_to_string(path) {
            Ok(text) => sockets.extend(parse_proc_net_tcp(&text)),
            // tcp6 is missing when IPv6 is disabled.
            Err(_) if path.ends_with('6') => {}
            Err(error) => return Err(format!("cannot read {path}: {error}")),
        }
    }

    let mut owners: HashMap<u64, Vec<u32>> = HashMap::new();
    let proc_dir = std::fs::read_dir("/proc").map_err(|error| error.to_string())?;
    for entry in proc_dir.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        // Other users' fd directories are unreadable without root; skip them.
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd.path())
                && let Some(inode) = target.to_str().and_then(parse_socket_link)
            {
                owners.entry(inode).or_default().push(pid);
            }
        }
    }

    Ok(sockets
        .into_iter()
        .map(|socket| RawListener {
            proto: Proto::Tcp,
            addr: socket.addr,
            port: socket.port,
            pids: owners.get(&socket.inode).cloned().unwrap_or_default(),
            uid: Some(socket.uid),
        })
        .collect())
}
```

Add this above the `#[cfg(test)]` line in `crates/hm-probe/src/listeners/netstat.rs`, with one blank line between them:

```rust
//! Parser for macOS `netstat -anv -p tcp`, which reports the PID of every socket,
//! including other users' sockets, without root.
//!
//! Recent macOS prints a `process:pid` column whose process name may contain spaces
//! ("Google Chrome:76915"); older releases print a numeric `pid` column. Columns are
//! located from the header, so both layouts parse.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetstatRow {
    pub addr: IpAddr,
    pub port: u16,
    pub pid: Option<u32>,
    pub process: Option<String>,
}

pub fn parse(text: &str) -> Vec<NetstatRow> {
    let mut lines = text.lines();
    let Some(header) = lines
        .by_ref()
        .find(|line| line.trim_start().starts_with("Proto"))
    else {
        return Vec::new();
    };
    let header: Vec<&str> = header.split_whitespace().collect();
    let Some(pid_col) = header
        .iter()
        .position(|word| *word == "process:pid" || *word == "pid")
    else {
        return Vec::new();
    };
    // "Local Address" and "Foreign Address" are two header words but one row token each.
    let Some(start) = pid_col.checked_sub(2) else {
        return Vec::new();
    };
    let tail = header.len() - pid_col - 1;

    lines
        .filter_map(|line| {
            let tokens: Vec<&str> = line.split_whitespace().collect();
            if tokens.len() < 6 || !tokens[0].starts_with("tcp") || tokens[5] != "LISTEN" {
                return None;
            }
            let (addr, port) = parse_local(tokens[0], tokens[3])?;
            let end = tokens.len().checked_sub(tail)?;
            let (pid, process) = if start < end {
                parse_process(&tokens[start..end].join(" "))
            } else {
                (None, None)
            };
            Some(NetstatRow {
                addr,
                port,
                pid,
                process,
            })
        })
        .collect()
}

/// `127.0.0.1.8080`, `*.5432`, `::1.3000` or `fe80::1%lo0.631`.
fn parse_local(proto: &str, local: &str) -> Option<(IpAddr, u16)> {
    let (host, port) = local.rsplit_once('.')?;
    let port = port.parse().ok()?;
    let addr = if host == "*" {
        if proto == "tcp4" {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        }
    } else {
        let host = host.split('%').next()?;
        host.parse().ok()?
    };
    Some((addr, port))
}

/// `Google Chrome:76915` → (76915, "Google Chrome"); `812` → (812, none).
fn parse_process(field: &str) -> (Option<u32>, Option<String>) {
    match field.rsplit_once(':') {
        Some((name, pid)) => (pid.parse().ok(), Some(name.to_string())),
        None => (field.parse().ok(), None),
    }
}
```

Create `crates/hm-probe/src/listeners/macos.rs`:

```rust
//! macOS: libproc for our own processes, `netstat` for everyone else's.

use std::collections::HashSet;
use std::process::Command;

use hm_core::model::Proto;
use hm_core::raw::RawListener;

use super::netstat;

pub fn read() -> Result<Vec<RawListener>, String> {
    let own = listeners::get_all().map_err(|error| error.to_string())?;
    let mut out: Vec<RawListener> = own
        .into_iter()
        .filter(|listener| {
            listener.protocol == listeners::Protocol::TCP
                && listener.state == listeners::SocketState::Listen
        })
        .map(|listener| RawListener {
            proto: Proto::Tcp,
            addr: listener.socket.ip(),
            port: listener.socket.port(),
            pids: vec![listener.process.pid],
            uid: None,
        })
        .collect();

    // netstat fills in sockets of processes libproc may not inspect (root, other users).
    let seen: HashSet<u32> = out
        .iter()
        .flat_map(|listener| listener.pids.clone())
        .collect();
    if let Ok(output) = Command::new("/usr/sbin/netstat")
        .args(["-anv", "-p", "tcp"])
        .output()
    {
        for row in netstat::parse(&String::from_utf8_lossy(&output.stdout)) {
            if row.pid.is_some_and(|pid| seen.contains(&pid)) {
                continue;
            }
            out.push(RawListener {
                proto: Proto::Tcp,
                addr: row.addr,
                port: row.port,
                pids: row.pid.into_iter().collect(),
                uid: None,
            });
        }
    }
    Ok(out)
}
```

Create `crates/hm-probe/src/listeners/windows.rs`:

```rust
//! Windows: `GetExtendedTcpTable` through the `listeners` crate.

use hm_core::model::Proto;
use hm_core::raw::RawListener;

pub fn read() -> Result<Vec<RawListener>, String> {
    let all = listeners::get_all().map_err(|error| error.to_string())?;
    Ok(all
        .into_iter()
        .filter(|listener| {
            listener.protocol == listeners::Protocol::TCP
                && listener.state == listeners::SocketState::Listen
        })
        .map(|listener| RawListener {
            proto: Proto::Tcp,
            addr: listener.socket.ip(),
            port: listener.socket.port(),
            pids: vec![listener.process.pid],
            uid: None,
        })
        .collect())
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p hm-probe listeners`
Expected: PASS — unit tests `test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out`; `tests/live.rs` `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out`.

- [ ] **Step 5: Commit**

```bash
git add crates/hm-probe/src/lib.rs crates/hm-probe/src/listeners/mod.rs crates/hm-probe/src/listeners/linux.rs crates/hm-probe/src/listeners/netstat.rs crates/hm-probe/src/listeners/macos.rs crates/hm-probe/src/listeners/windows.rs crates/hm-probe/tests/live.rs
git commit -s -m "feat(probe): list listening TCP sockets and the processes that hold them"
```

### Task 7: Docker engine discovery

This task finds where Docker-compatible engines might be listening, in the order of spec §6.4: `DOCKER_HOST`, the current docker context, then the well-known sockets of Docker Desktop, OrbStack, Colima, Rancher Desktop and Podman, and the Windows named pipe. Discovery reads everything through a `DiscoverEnv` value, so tests can point it at a temporary home folder; connecting to the engines comes in Task 8.

**Files:**
- Create: `crates/hm-probe/src/docker/mod.rs` (first version: module doc comment, `pub mod discover;` and `on_path`; Task 8 adds the rest)
- Create: `crates/hm-probe/src/docker/discover.rs`
- Modify: `crates/hm-probe/src/lib.rs` (add `pub mod docker;`)
- Test: `crates/hm-probe/src/docker/discover.rs` (unit tests in the same file)

**Interfaces:**
- Consumes: no items from earlier tasks (it builds on the `hm-probe` crate set up in Tasks 5–6); reads docker's `config.json` and context `meta.json` with `serde_json::Value`
- Produces:
  - `hm_probe::docker`: `pub fn on_path(name: &str) -> bool`
  - `hm_probe::docker::discover`: `pub struct Candidate { pub endpoint: String, pub label: String }`; `pub struct DiscoverEnv { pub docker_host: Option<String>, pub docker_context: Option<String>, pub docker_config: Option<PathBuf>, pub home: Option<PathBuf>, pub xdg_runtime_dir: Option<PathBuf>, pub system_sockets: Vec<PathBuf>, pub windows: bool }` (with `Default`); `pub fn DiscoverEnv::from_process() -> Self`; `pub fn candidates(env: &DiscoverEnv) -> Vec<Candidate>`; `pub fn label_for(endpoint: &str) -> String`

- [ ] **Step 1: Write the failing test**

Set `crates/hm-probe/src/lib.rs` to:

```rust
//! OS adapters: what runs on this computer, before any interpretation.

pub mod docker;
pub mod listeners;
pub mod processes;
```

Create `crates/hm-probe/src/docker/mod.rs` with the module doc comment and the `discover` module declaration:

```rust
//! Docker-compatible engines: discovery, then one container listing per reachable
//! engine. Problems become notes; probing never fails as a whole.

pub mod discover;
```

Create `crates/hm-probe/src/docker/discover.rs` with only its test module. The symlink test is `#[cfg(unix)]`, because Windows engines are reached through a named pipe:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn docker_host_comes_first_then_existing_sockets() {
        let home = tempfile::tempdir().unwrap();
        touch(&home.path().join(".orbstack/run/docker.sock"));
        let env = DiscoverEnv {
            docker_host: Some("tcp://127.0.0.1:2375".into()),
            home: Some(home.path().to_path_buf()),
            ..DiscoverEnv::default()
        };
        let found = candidates(&env);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].endpoint, "tcp://127.0.0.1:2375");
        assert_eq!(found[1].label, "OrbStack");
        assert!(found[1].endpoint.ends_with(".orbstack/run/docker.sock"));
    }

    #[test]
    fn reads_the_current_context_endpoint() {
        let home = tempfile::tempdir().unwrap();
        let docker = home.path().join(".docker");
        std::fs::create_dir_all(docker.join("contexts/meta/abc123")).unwrap();
        std::fs::write(docker.join("config.json"), r#"{"currentContext":"colima"}"#).unwrap();
        std::fs::write(
            docker.join("contexts/meta/abc123/meta.json"),
            r#"{"Name":"colima","Endpoints":{"docker":{"Host":"unix:///Users/me/.colima/default/docker.sock"}}}"#,
        )
        .unwrap();
        let env = DiscoverEnv {
            home: Some(home.path().to_path_buf()),
            ..DiscoverEnv::default()
        };
        let found = candidates(&env);
        assert_eq!(
            found[0].endpoint,
            "unix:///Users/me/.colima/default/docker.sock"
        );
        assert_eq!(found[0].label, "Colima");
    }

    #[test]
    fn missing_sockets_are_skipped_and_windows_gets_the_pipe() {
        let home = tempfile::tempdir().unwrap();
        let env = DiscoverEnv {
            home: Some(home.path().to_path_buf()),
            system_sockets: vec![home.path().join("nope.sock")],
            windows: true,
            ..DiscoverEnv::default()
        };
        let found = candidates(&env);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].endpoint, "npipe:////./pipe/docker_engine");
        assert_eq!(found[0].label, "Docker Desktop");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_sockets_collapse_to_one_candidate() {
        let home = tempfile::tempdir().unwrap();
        let real = home.path().join(".docker/run/docker.sock");
        touch(&real);
        let link = home.path().join("var-run-docker.sock");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let env = DiscoverEnv {
            home: Some(home.path().to_path_buf()),
            system_sockets: vec![link],
            ..DiscoverEnv::default()
        };
        assert_eq!(candidates(&env).len(), 1);
    }
}
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p hm-probe discover`
Expected: FAIL — compile errors such as `` error[E0422]: cannot find struct, variant or union type `DiscoverEnv` in this scope `` and `` error[E0425]: cannot find function `candidates` in this scope ``.

- [ ] **Step 3: Write the implementation**

Add this above the `#[cfg(test)]` line in `crates/hm-probe/src/docker/discover.rs`, with one blank line between them:

```rust
//! Where Docker-compatible engines might be listening (v0.1 spec §6.4), in order:
//! `DOCKER_HOST`, the current docker context, then well-known sockets.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// `unix:///path`, `npipe:////./pipe/name` or `tcp://host:port`.
    pub endpoint: String,
    /// Engine name guessed from the endpoint; refined after connecting.
    pub label: String,
}

/// Everything discovery reads, so tests can point it at a temporary home.
#[derive(Debug, Clone, Default)]
pub struct DiscoverEnv {
    pub docker_host: Option<String>,
    pub docker_context: Option<String>,
    /// `DOCKER_CONFIG`, defaulting to `~/.docker`.
    pub docker_config: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub xdg_runtime_dir: Option<PathBuf>,
    /// Absolute sockets outside the home directory, such as `/var/run/docker.sock`.
    pub system_sockets: Vec<PathBuf>,
    pub windows: bool,
}

impl DiscoverEnv {
    pub fn from_process() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        let mut system_sockets = vec![PathBuf::from("/var/run/docker.sock")];
        system_sockets.extend(podman_machine_socket());
        DiscoverEnv {
            docker_host: var("DOCKER_HOST"),
            docker_context: var("DOCKER_CONTEXT"),
            docker_config: var("DOCKER_CONFIG").map(PathBuf::from),
            home: std::env::home_dir(),
            xdg_runtime_dir: var("XDG_RUNTIME_DIR").map(PathBuf::from),
            system_sockets,
            windows: cfg!(windows),
        }
    }
}

pub fn candidates(env: &DiscoverEnv) -> Vec<Candidate> {
    let mut found: Vec<Candidate> = Vec::new();
    let mut push = |endpoint: String| {
        let endpoint = canonical_endpoint(&endpoint);
        if !found.iter().any(|candidate| candidate.endpoint == endpoint) {
            found.push(Candidate {
                label: label_for(&endpoint),
                endpoint,
            });
        }
    };

    if let Some(host) = &env.docker_host {
        push(host.clone());
    }
    if let Some(host) = context_host(env) {
        push(host);
    }

    let mut sockets: Vec<PathBuf> = Vec::new();
    if let Some(home) = &env.home {
        for relative in [
            ".docker/run/docker.sock",
            ".orbstack/run/docker.sock",
            ".colima/default/docker.sock",
            ".rd/docker.sock",
        ] {
            sockets.push(home.join(relative));
        }
    }
    sockets.extend(env.system_sockets.iter().cloned());
    if let Some(runtime) = &env.xdg_runtime_dir {
        sockets.push(runtime.join("docker.sock"));
        sockets.push(runtime.join("podman/podman.sock"));
    }
    for socket in sockets {
        if socket.exists() {
            push(format!("unix://{}", socket.display()));
        }
    }
    if env.windows {
        push("npipe:////./pipe/docker_engine".to_string());
    }
    found
}

/// Reads `currentContext` from the docker config (or `DOCKER_CONTEXT`) and returns that
/// context's docker endpoint. The built-in `default` context has no metadata file.
fn context_host(env: &DiscoverEnv) -> Option<String> {
    let config_dir = env
        .docker_config
        .clone()
        .or_else(|| env.home.as_ref().map(|home| home.join(".docker")))?;
    let name = match &env.docker_context {
        Some(name) => name.clone(),
        None => {
            let config = std::fs::read_to_string(config_dir.join("config.json")).ok()?;
            let config: serde_json::Value = serde_json::from_str(&config).ok()?;
            config.get("currentContext")?.as_str()?.to_string()
        }
    };
    if name.is_empty() || name == "default" {
        return None;
    }
    // Metadata directories are named by a hash of the context name; scan them instead.
    for entry in std::fs::read_dir(config_dir.join("contexts/meta"))
        .ok()?
        .flatten()
    {
        let Ok(text) = std::fs::read_to_string(entry.path().join("meta.json")) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if meta.get("Name").and_then(|value| value.as_str()) == Some(name.as_str()) {
            return meta
                .pointer("/Endpoints/docker/Host")
                .and_then(|value| value.as_str())
                .map(str::to_string);
        }
    }
    None
}

/// Resolves symlinks in `unix://` endpoints so two names for one socket collapse.
fn canonical_endpoint(endpoint: &str) -> String {
    match endpoint.strip_prefix("unix://") {
        Some(path) => match std::fs::canonicalize(path) {
            Ok(real) => format!("unix://{}", real.display()),
            Err(_) => endpoint.to_string(),
        },
        None => endpoint.to_string(),
    }
}

pub fn label_for(endpoint: &str) -> String {
    let lower = endpoint.to_ascii_lowercase();
    let label = if lower.contains(".orbstack") {
        "OrbStack"
    } else if lower.contains(".colima") || lower.contains("lima") {
        "Colima"
    } else if lower.contains("/.rd/") {
        "Rancher Desktop"
    } else if lower.contains("podman") {
        "Podman"
    } else if lower.contains(".docker/run") || lower.contains("docker_engine") {
        "Docker Desktop"
    } else {
        "Docker"
    };
    label.to_string()
}

/// Asks `podman machine inspect` for the API socket when the podman CLI exists.
fn podman_machine_socket() -> Option<PathBuf> {
    if !super::on_path("podman") {
        return None;
    }
    let output = std::process::Command::new("podman")
        .args([
            "machine",
            "inspect",
            "--format",
            "{{.ConnectionInfo.PodmanSocket.Path}}",
        ])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !path.is_empty() && Path::new(&path).is_absolute())
        .then(|| PathBuf::from(path))
}
```

Append `on_path` to `crates/hm-probe/src/docker/mod.rs`, after `pub mod discover;` and one blank line. `discover.rs` calls it as `super::on_path` to decide whether to ask `podman machine inspect` for the Podman socket; Task 8 uses it again for the "not installed" note:

```rust
/// True when an executable with this name is on `PATH`.
pub fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        dir.join(name).is_file() || (cfg!(windows) && dir.join(format!("{name}.exe")).is_file())
    })
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p hm-probe discover`
Expected: PASS — unit tests `test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out` (on Windows 3 passed, because the `#[cfg(unix)]` symlink test does not exist there); `tests/live.rs` `test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out`.

- [ ] **Step 5: Commit**

```bash
git add crates/hm-probe/src/lib.rs crates/hm-probe/src/docker/mod.rs crates/hm-probe/src/docker/discover.rs
git commit -s -m "feat(probe): discover Docker-compatible engine endpoints"
```

### Task 8: Docker containers and the probe facade

This task connects to every discovered engine with bollard, lists all containers and converts each inspect response into a `RawContainer`, dropping environment values and every label outside `com.docker.compose.*`. Each engine gets at most `docker_timeout`; an engine that refuses the connection or never answers is listed as unreachable with the reason, and when no engine answers (or none is installed) a note says why, so the probe never fails as a whole. Finally `hm_probe::probe()` runs the Docker probe on its own thread while it reads listeners and processes, and returns one `RawInventory`.

**Files:**
- Create: `crates/hm-probe/src/docker/convert.rs`
- Create: `crates/hm-probe/tests/fixtures/inspect-compose-postgres.json`
- Modify: `crates/hm-probe/src/docker/mod.rs` (add `pub mod convert;`, the engine probing code and its tests)
- Modify: `crates/hm-probe/src/lib.rs` (add `ProbeOptions` and `probe()`)
- Modify: `crates/hm-probe/tests/live.rs` (add `probe_reports_itself_and_its_socket` and its import)
- Test: `crates/hm-probe/src/docker/{convert.rs,mod.rs}` (unit tests in the same files), `crates/hm-probe/tests/live.rs` (live test)

**Interfaces:**
- Consumes: `hm_core::ids::engine_id(key: &str) -> String` (Task 4); `hm_core::model::{Engine, Mount, Note, PortBinding}` and `hm_core::raw::{RawContainer, RawInventory}` (Task 4); `hm_probe::processes::read_processes() -> ProcessTable` (Task 5); `hm_probe::listeners::read_listeners() -> Result<Vec<RawListener>, String>` (Task 6); `hm_probe::docker::discover::{Candidate, DiscoverEnv, candidates}` and `hm_probe::docker::on_path` (Task 7); bollard's `Docker`, `API_DEFAULT_VERSION`, `query_parameters::ListContainersOptions` and `models::ContainerInspectResponse`
- Produces:
  - `hm_probe::docker::convert`: `pub fn from_inspect(engine: &str, inspect: ContainerInspectResponse) -> Option<RawContainer>`
  - `hm_probe::docker`: `pub struct DockerReport { pub engines: Vec<Engine>, pub containers: Vec<RawContainer>, pub notes: Vec<Note> }` (with `Default`); `pub fn DockerReport::failed(reason: &str) -> Self`; `pub fn probe_docker(env: &DiscoverEnv, timeout: Duration) -> DockerReport`
  - `hm_probe`: `pub struct ProbeOptions { pub docker_timeout: Duration }` (`Default` is 3 seconds); `pub fn probe(options: &ProbeOptions) -> RawInventory` (used by the CLI in Task 15)
  - Note codes in `RawInventory::notes`: `docker_not_installed`, `docker_not_running`, `docker_not_answering`, `docker_unavailable`, `listeners_unavailable`

- [ ] **Step 1: Write the failing test**

Create the fixture `crates/hm-probe/tests/fixtures/inspect-compose-postgres.json`, a trimmed `docker inspect` of a Compose Postgres container. It carries a password in `Env` and a label outside `com.docker.compose.*`, which the conversion must drop:

```json
{
  "Id": "4f1c2a9e8b7d6c5b4a39281706f5e4d3c2b1a09f8e7d6c5b4a3928170f6e5d4c",
  "Created": "2026-10-15T07:00:01.987654321Z",
  "Path": "docker-entrypoint.sh",
  "Args": ["postgres"],
  "State": {
    "Status": "running",
    "Running": true,
    "Paused": false,
    "Restarting": false,
    "OOMKilled": false,
    "Dead": false,
    "Pid": 4242,
    "ExitCode": 0,
    "Error": "",
    "StartedAt": "2026-10-15T07:00:03.123456789Z",
    "FinishedAt": "0001-01-01T00:00:00Z",
    "Health": {
      "Status": "healthy",
      "FailingStreak": 0,
      "Log": []
    }
  },
  "Image": "sha256:9d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e",
  "Name": "/shop-db-1",
  "RestartCount": 0,
  "Mounts": [
    {
      "Type": "volume",
      "Name": "shop_pgdata",
      "Source": "/var/lib/docker/volumes/shop_pgdata/_data",
      "Destination": "/var/lib/postgresql/data",
      "Driver": "local",
      "Mode": "z",
      "RW": true,
      "Propagation": ""
    }
  ],
  "Config": {
    "Hostname": "4f1c2a9e8b7d",
    "Env": [
      "POSTGRES_PASSWORD=example-secret",
      "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
    ],
    "Image": "postgres:16-alpine",
    "Labels": {
      "com.docker.compose.config-hash": "8c3e1f0a",
      "com.docker.compose.container-number": "1",
      "com.docker.compose.oneoff": "False",
      "com.docker.compose.project": "shop",
      "com.docker.compose.project.config_files": "/work/shop/compose.yaml",
      "com.docker.compose.project.working_dir": "/work/shop",
      "com.docker.compose.service": "db",
      "org.opencontainers.image.version": "16.4"
    }
  },
  "NetworkSettings": {
    "Ports": {
      "5432/tcp": [
        { "HostIp": "0.0.0.0", "HostPort": "55432" },
        { "HostIp": "::", "HostPort": "55432" }
      ]
    },
    "Networks": {
      "shop_default": {
        "Aliases": ["shop-db-1", "db"],
        "NetworkID": "a1b2c3",
        "IPAddress": "172.18.0.2"
      }
    }
  }
}
```

Create `crates/hm-probe/src/docker/convert.rs` with only its test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const POSTGRES: &str = include_str!("../../tests/fixtures/inspect-compose-postgres.json");

    #[test]
    fn converts_a_compose_postgres_container() {
        let inspect: ContainerInspectResponse = serde_json::from_str(POSTGRES).unwrap();
        let container = from_inspect("e_engine", inspect).unwrap();
        assert_eq!(container.name, "shop-db-1");
        assert_eq!(container.image, "postgres:16-alpine");
        assert_eq!(container.state, "running");
        assert_eq!(container.health.as_deref(), Some("healthy"));
        assert_eq!(container.restart_count, 0);
        assert_eq!(
            container.started_at.as_deref(),
            Some("2026-10-15T07:00:03.123456789Z")
        );
        assert_eq!(
            container.ports,
            vec![
                PortBinding {
                    container_port: 5432,
                    proto: "tcp".into(),
                    host_ip: Some("0.0.0.0".parse().unwrap()),
                    host_port: Some(55432),
                },
                PortBinding {
                    container_port: 5432,
                    proto: "tcp".into(),
                    host_ip: Some("::".parse().unwrap()),
                    host_port: Some(55432),
                },
            ]
        );
        assert_eq!(container.networks, vec!["shop_default"]);
        assert_eq!(container.mounts[0].kind, "volume");
        assert_eq!(container.mounts[0].name.as_deref(), Some("shop_pgdata"));
        assert_eq!(container.mounts[0].source, None);
        assert_eq!(
            container
                .labels
                .get("com.docker.compose.service")
                .map(String::as_str),
            Some("db")
        );
        assert!(
            !container
                .labels
                .contains_key("org.opencontainers.image.version")
        );
        let json = serde_json::to_string(&container).unwrap();
        assert!(!json.contains("POSTGRES_PASSWORD"));
        assert!(!json.contains("example-secret"));
    }
}
```

In `crates/hm-probe/src/docker/mod.rs`, add `pub mod convert;` above `pub mod discover;`, so the file starts with:

```rust
//! Docker-compatible engines: discovery, then one container listing per reachable
//! engine. Problems become notes; probing never fails as a whole.

pub mod convert;
pub mod discover;
```

Append this test module at the end of `crates/hm-probe/src/docker/mod.rs`, after `on_path` and one blank line. `a_hanging_engine_times_out_with_a_note` is `#[cfg(unix)]`: it binds a Unix socket that accepts connections and never answers:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_refined_by_the_engine_os() {
        assert_eq!(
            refine_label("Docker", Some("Docker Desktop")),
            "Docker Desktop"
        );
        assert_eq!(refine_label("Docker", Some("OrbStack")), "OrbStack");
        assert_eq!(refine_label("Colima", Some("Ubuntu 24.04 LTS")), "Colima");
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn unreachable_candidates_say_why() {
        let report = runtime().block_on(probe_all(
            vec![Candidate {
                endpoint: "tcp://127.0.0.1:1".into(),
                label: "Docker".into(),
            }],
            Duration::from_secs(2),
        ));
        assert_eq!(report.engines.len(), 1);
        assert!(!report.engines[0].reachable);
        assert!(report.engines[0].error.is_some());
        assert_eq!(report.notes[0].code, "docker_not_running");
    }

    #[cfg(unix)]
    #[test]
    fn a_hanging_engine_times_out_with_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hang.sock");
        let socket = std::os::unix::net::UnixListener::bind(&path).unwrap();
        // Accept connections and hold them open without ever answering.
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in socket.incoming() {
                held.push(stream);
            }
        });
        let started = std::time::Instant::now();
        let report = runtime().block_on(probe_all(
            vec![Candidate {
                endpoint: format!("unix://{}", path.display()),
                label: "Docker".into(),
            }],
            Duration::from_millis(500),
        ));
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(!report.engines[0].reachable);
        assert_eq!(report.notes[0].code, "docker_not_answering");
    }
}
```

In `crates/hm-probe/tests/live.rs`, replace the `use` lines at the top with:

```rust
use std::net::TcpListener;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use hm_probe::listeners::read_listeners;
use hm_probe::processes::read_processes;
use hm_probe::{ProbeOptions, probe};
```

Then append this test at the end of the file, after one blank line:

```rust
#[test]
fn probe_reports_itself_and_its_socket() {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    let inventory = probe(&ProbeOptions {
        docker_timeout: Duration::from_secs(2),
    });
    assert_eq!(inventory.self_pid, std::process::id());
    assert!(inventory.current_uid.is_some());
    assert!(
        inventory
            .processes
            .iter()
            .any(|p| p.pid == inventory.self_pid)
    );
    assert!(inventory.listeners.iter().any(|l| l.port == port));
    assert_eq!(inventory.os, std::env::consts::OS);
}
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p hm-probe`
Expected: FAIL — compile errors such as `` error[E0432]: unresolved imports `hm_probe::ProbeOptions`, `hm_probe::probe` `` and `` error[E0425]: cannot find function `from_inspect` in this scope `` (the same error appears for `refine_label` and `probe_all`).

- [ ] **Step 3: Write the implementation**

Add this above the `#[cfg(test)]` line in `crates/hm-probe/src/docker/convert.rs`, with one blank line between them:

```rust
//! Engine API inspect responses → `RawContainer`. Environment values and every label
//! outside `com.docker.compose.*` are dropped here.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::PathBuf;

use bollard::models::ContainerInspectResponse;

use hm_core::model::{Mount, PortBinding};
use hm_core::raw::RawContainer;

const COMPOSE_LABEL_PREFIX: &str = "com.docker.compose.";
const NEVER_STARTED: &str = "0001-01-01T00:00:00Z";

pub fn from_inspect(engine: &str, inspect: ContainerInspectResponse) -> Option<RawContainer> {
    let id = inspect.id?;
    let state = inspect.state.unwrap_or_default();
    let config = inspect.config.unwrap_or_default();
    let network = inspect.network_settings.unwrap_or_default();

    let mut ports: Vec<PortBinding> = Vec::new();
    for (key, bindings) in network.ports.unwrap_or_default() {
        let Some((port, proto)) = key.split_once('/') else {
            continue;
        };
        let Ok(container_port) = port.parse::<u16>() else {
            continue;
        };
        let bindings = bindings.unwrap_or_default();
        if bindings.is_empty() {
            ports.push(PortBinding {
                container_port,
                proto: proto.to_string(),
                host_ip: None,
                host_port: None,
            });
        }
        for binding in bindings {
            ports.push(PortBinding {
                container_port,
                proto: proto.to_string(),
                host_ip: binding
                    .host_ip
                    .as_deref()
                    .filter(|ip| !ip.is_empty())
                    .and_then(|ip| ip.parse::<IpAddr>().ok()),
                host_port: binding.host_port.as_deref().and_then(|p| p.parse().ok()),
            });
        }
    }
    ports.sort_by(|a, b| {
        (a.container_port, &a.proto, a.host_port, a.host_ip).cmp(&(
            b.container_port,
            &b.proto,
            b.host_port,
            b.host_ip,
        ))
    });

    let mut networks: Vec<String> = network.networks.unwrap_or_default().into_keys().collect();
    networks.sort();

    let mounts = inspect
        .mounts
        .unwrap_or_default()
        .into_iter()
        .map(|mount| {
            let kind = mount.typ.unwrap_or_default();
            let source = if kind == "bind" {
                mount.source.map(PathBuf::from)
            } else {
                None
            };
            Mount {
                kind,
                name: mount.name.filter(|name| !name.is_empty()),
                source,
                destination: mount.destination.unwrap_or_default(),
            }
        })
        .collect();

    let labels: BTreeMap<String, String> = config
        .labels
        .unwrap_or_default()
        .into_iter()
        .filter(|(key, _)| key.starts_with(COMPOSE_LABEL_PREFIX))
        .collect();

    Some(RawContainer {
        engine: engine.to_string(),
        id,
        name: inspect
            .name
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_string(),
        image: config.image.or(inspect.image).unwrap_or_default(),
        state: state
            .status
            .map(|status| status.to_string())
            .unwrap_or_else(|| "unknown".into()),
        health: state
            .health
            .and_then(|health| health.status)
            .map(|status| status.to_string())
            .filter(|status| !status.is_empty() && status != "none"),
        exit_code: state.exit_code,
        oom_killed: state.oom_killed.unwrap_or(false),
        restart_count: inspect.restart_count.unwrap_or(0),
        started_at: state
            .started_at
            .filter(|at| !at.is_empty() && at != NEVER_STARTED),
        ports,
        networks,
        mounts,
        labels,
    })
}
```

In `crates/hm-probe/src/docker/mod.rs`, insert this between `pub mod discover;` and the `` /// True when an executable with this name is on `PATH`. `` comment, with one blank line on each side:

```rust
use std::time::Duration;

use bollard::query_parameters::ListContainersOptions;
use bollard::{API_DEFAULT_VERSION, Docker};

use hm_core::ids::engine_id;
use hm_core::model::{Engine, Note};
use hm_core::raw::RawContainer;

use discover::{Candidate, DiscoverEnv};

/// Seconds bollard waits for a single API request.
const REQUEST_TIMEOUT_SECS: u64 = 5;

#[derive(Debug, Default)]
pub struct DockerReport {
    pub engines: Vec<Engine>,
    pub containers: Vec<RawContainer>,
    pub notes: Vec<Note>,
}

impl DockerReport {
    pub fn failed(reason: &str) -> Self {
        DockerReport {
            notes: vec![note(
                "docker_unavailable",
                &format!("Harbormap couldn't check Docker: {reason}."),
            )],
            ..DockerReport::default()
        }
    }
}

/// Probes every candidate engine, giving each at most `timeout`.
pub fn probe_docker(env: &DiscoverEnv, timeout: Duration) -> DockerReport {
    let candidates = discover::candidates(env);
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(probe_all(candidates, timeout)),
        Err(error) => DockerReport::failed(&error.to_string()),
    }
}

async fn probe_all(candidates: Vec<Candidate>, timeout: Duration) -> DockerReport {
    let mut report = DockerReport::default();
    if candidates.is_empty() {
        report
            .notes
            .push(if on_path("docker") || on_path("podman") {
                not_running()
            } else {
                note(
                    "docker_not_installed",
                    "Docker isn't installed, so there are no containers to show.",
                )
            });
        return report;
    }
    let mut timed_out = false;
    for candidate in candidates {
        match tokio::time::timeout(timeout, probe_one(&candidate)).await {
            Ok(Ok((engine, containers))) => {
                // Two sockets can reach the same daemon; keep the first.
                if report.engines.iter().any(|known| known.id == engine.id) {
                    continue;
                }
                report.engines.push(engine);
                report.containers.extend(containers);
            }
            Ok(Err(error)) => report.engines.push(unreachable(&candidate, error)),
            Err(_) => {
                timed_out = true;
                report.engines.push(unreachable(
                    &candidate,
                    format!("no answer within {} ms", timeout.as_millis()),
                ));
            }
        }
    }
    if !report.engines.iter().any(|engine| engine.reachable) {
        report.notes.push(if timed_out {
            note(
                "docker_not_answering",
                "Docker didn't answer in time. It may still be starting; try again in a moment.",
            )
        } else {
            not_running()
        });
    }
    report
}

async fn probe_one(candidate: &Candidate) -> Result<(Engine, Vec<RawContainer>), String> {
    let docker = connect(&candidate.endpoint)?
        .negotiate_version()
        .await
        .map_err(|error| error.to_string())?;
    let info = docker.info().await.map_err(|error| error.to_string())?;
    let key = info
        .id
        .clone()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| candidate.endpoint.clone());
    let id = engine_id(&key);
    let summaries = docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            ..ListContainersOptions::default()
        }))
        .await
        .map_err(|error| error.to_string())?;
    let mut containers = Vec::new();
    for summary in summaries {
        let Some(container_id) = summary.id else {
            continue;
        };
        // A container can disappear between listing and inspecting; skip it then.
        if let Ok(inspect) = docker.inspect_container(&container_id, None).await
            && let Some(container) = convert::from_inspect(&id, inspect)
        {
            containers.push(container);
        }
    }
    containers.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    let engine = Engine {
        id,
        name: refine_label(&candidate.label, info.operating_system.as_deref()),
        endpoint: candidate.endpoint.clone(),
        version: info.server_version,
        reachable: true,
        error: None,
    };
    Ok((engine, containers))
}

fn connect(endpoint: &str) -> Result<Docker, String> {
    let docker = if endpoint.starts_with("tcp://") || endpoint.starts_with("http://") {
        Docker::connect_with_http(endpoint, REQUEST_TIMEOUT_SECS, API_DEFAULT_VERSION)
    } else {
        connect_local(endpoint)?
    };
    docker.map_err(|error| error.to_string())
}

#[cfg(unix)]
fn connect_local(endpoint: &str) -> Result<Result<Docker, bollard::errors::Error>, String> {
    match endpoint.strip_prefix("unix://") {
        Some(path) => Ok(Docker::connect_with_unix(
            path,
            REQUEST_TIMEOUT_SECS,
            API_DEFAULT_VERSION,
        )),
        None => Err(format!("unsupported endpoint {endpoint}")),
    }
}

#[cfg(windows)]
fn connect_local(endpoint: &str) -> Result<Result<Docker, bollard::errors::Error>, String> {
    if endpoint.starts_with("npipe://") {
        Ok(Docker::connect_with_named_pipe(
            endpoint,
            REQUEST_TIMEOUT_SECS,
            API_DEFAULT_VERSION,
        ))
    } else {
        Err(format!("unsupported endpoint {endpoint}"))
    }
}

fn refine_label(label: &str, operating_system: Option<&str>) -> String {
    match operating_system {
        Some(os) if os.contains("Docker Desktop") => "Docker Desktop".into(),
        Some(os) if os.contains("OrbStack") => "OrbStack".into(),
        _ => label.to_string(),
    }
}

fn unreachable(candidate: &Candidate, error: String) -> Engine {
    Engine {
        id: engine_id(&candidate.endpoint),
        name: candidate.label.clone(),
        endpoint: candidate.endpoint.clone(),
        version: None,
        reachable: false,
        error: Some(error),
    }
}

fn not_running() -> Note {
    note(
        "docker_not_running",
        "Docker isn't running. Start Docker Desktop (or your container engine) to see containers.",
    )
}

fn note(code: &str, text: &str) -> Note {
    Note {
        code: code.into(),
        text: text.into(),
    }
}
```

Replace the whole of `crates/hm-probe/src/lib.rs` with:

```rust
//! OS adapters: what runs on this computer, before any interpretation.

pub mod docker;
pub mod listeners;
pub mod processes;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hm_core::model::Note;
use hm_core::raw::RawInventory;

use docker::DockerReport;
use docker::discover::DiscoverEnv;

#[derive(Debug, Clone)]
pub struct ProbeOptions {
    /// Upper bound for each Docker engine.
    pub docker_timeout: Duration,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        ProbeOptions {
            docker_timeout: Duration::from_secs(3),
        }
    }
}

/// Reads listeners, processes and containers. Docker is probed on its own thread
/// while the OS tables are read.
pub fn probe(options: &ProbeOptions) -> RawInventory {
    let taken_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let docker_env = DiscoverEnv::from_process();
    let (table, listeners, docker) = std::thread::scope(|scope| {
        let docker = scope.spawn(|| docker::probe_docker(&docker_env, options.docker_timeout));
        // Listeners first: an owner that starts later does not matter, one that exits
        // in between shows up as "process ended".
        let listeners = listeners::read_listeners();
        let table = processes::read_processes();
        let docker = docker
            .join()
            .unwrap_or_else(|_| DockerReport::failed("the Docker probe stopped unexpectedly"));
        (table, listeners, docker)
    });

    let mut notes = Vec::new();
    let listeners = listeners.unwrap_or_else(|error| {
        notes.push(Note {
            code: "listeners_unavailable".into(),
            text: format!("Harbormap couldn't read listening ports: {error}"),
        });
        Vec::new()
    });
    notes.extend(docker.notes);

    RawInventory {
        taken_at,
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        self_pid: table.self_pid,
        current_uid: table.current_uid,
        processes: table.processes,
        listeners,
        containers: docker.containers,
        engines: docker.engines,
        notes,
    }
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p hm-probe`
Expected: PASS — unit tests `test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out` (on Windows 15 passed, because the two `#[cfg(unix)]` tests do not exist there); `tests/live.rs` `test result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out` (the ignored test is the `child_sleeper` helper); doc-tests `test result: ok. 0 passed`. The live probe test makes no assertions about Docker, so it passes whether or not an engine is running.

- [ ] **Step 5: Commit**

```bash
git add crates/hm-probe/src/lib.rs crates/hm-probe/src/docker/mod.rs crates/hm-probe/src/docker/convert.rs crates/hm-probe/tests/fixtures/inspect-compose-postgres.json crates/hm-probe/tests/live.rs
git commit -s -m "feat(probe): read Docker containers and combine all probes in probe()"
```

### Task 9: Signature packs

Harbormap recognises programs through signature packs: TOML files under `packs/signatures/` that are compiled into the binary. This task adds the five v0.1 packs (45 signatures: front-end dev servers, back-end servers and runtimes, data stores, system owners, editors, AI agents and headless test browsers) and the `hm-doctor` loader. The loader checks each entry on its own, skips a broken entry with a warning instead of failing, and keeps only the first of two entries with the same ID.

**Files:**
- Create: `packs/signatures/web.toml`
- Create: `packs/signatures/servers.toml`
- Create: `packs/signatures/data.toml`
- Create: `packs/signatures/system.toml`
- Create: `packs/signatures/tools.toml`
- Create: `crates/hm-doctor/src/packs.rs`
- Modify: `crates/hm-doctor/src/lib.rs` (whole content: crate doc comment and `pub mod packs;`)
- Test: `crates/hm-doctor/src/packs.rs` (unit tests in the same file)

**Interfaces:**
- Consumes: `hm_core::signature::{Level, Signature, SignatureSet}`, with `SignatureSet::new(signatures: Vec<Signature>) -> SignatureSet`, `SignatureSet::iter(&self) -> impl Iterator<Item = &Signature>`, `SignatureSet::len(&self) -> usize` and `Texts::get(&self, level: Level) -> &str` (Task 4); the `toml` crate (Task 1).
- Produces: `pub const SIGNATURE_FILES: &[(&str, &str)]` (pack path and embedded text), `pub struct Packs { pub signatures: SignatureSet, pub warnings: Vec<String> }`, `pub fn load_embedded() -> Packs`, `pub fn parse_signatures(file: &str, text: &str, warnings: &mut Vec<String>) -> Vec<Signature>`, and the five pack files.

- [ ] **Step 1: Write the failing test**

Declare the module and create `packs.rs` with only its tests: the embedded packs load without warnings, a bad ID and an unknown field each skip only their own entry, and a duplicate ID keeps the first entry.

Replace the whole content of `crates/hm-doctor/src/lib.rs` with:

```rust
//! Packs (signatures and card texts), the three-level card renderer and, from
//! alpha.2, the doctor rules.

pub mod packs;
```

Create `crates/hm-doctor/src/packs.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_packs_load_without_warnings() {
        let packs = load_embedded();
        assert!(packs.warnings.is_empty(), "{:#?}", packs.warnings);
        assert!(
            packs.signatures.len() >= 30,
            "only {} signatures",
            packs.signatures.len()
        );
    }

    #[test]
    fn a_broken_entry_is_skipped_with_a_warning() {
        let text = r#"
[[signature]]
id = "good"
name = "Good"
role = "work"
kind = "dev_server"
[[signature.when]]
exe = ["good"]
[signature.texts]
beginner = "b"
builder = "b"
expert = "e"
[[signature.fixture]]
exe = "good"

[[signature]]
id = "Bad Id"
name = "Bad"
role = "work"
kind = "dev_server"
[[signature.when]]
exe = ["bad"]
[signature.texts]
beginner = "b"
builder = "b"
expert = "e"
[[signature.fixture]]
exe = "bad"

[[signature]]
id = "typo"
name = "Typo"
role = "work"
kind = "dev_server"
[[signature.when]]
exee = ["typo"]
[signature.texts]
beginner = "b"
builder = "b"
expert = "e"
"#;
        let mut warnings = Vec::new();
        let signatures = parse_signatures("test.toml", text, &mut warnings);
        assert_eq!(signatures.len(), 1);
        assert_eq!(signatures[0].id, "good");
        assert_eq!(warnings.len(), 2, "{warnings:#?}");
        assert!(warnings[0].contains("Bad Id"));
        assert!(warnings[1].contains("typo"));
    }

    #[test]
    fn duplicate_ids_keep_the_first_entry() {
        let packs = load_embedded();
        let mut signatures: Vec<Signature> = packs.signatures.iter().cloned().collect();
        signatures.push(signatures[0].clone());
        let mut warnings = Vec::new();
        let kept = drop_duplicate_ids(signatures, &mut warnings);
        assert_eq!(kept.len(), packs.signatures.len());
        assert_eq!(warnings.len(), 1);
    }
}
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p hm-doctor packs`
Expected: FAIL — compile errors such as ``error[E0425]: cannot find function `load_embedded` in this scope``.

- [ ] **Step 3: Write the implementation**

Create the five pack files with exactly this content. Each `[[signature]]` entry deserializes into `hm_core::signature::Signature` (Task 4): `when` blocks are alternatives whose conditions must all hold, the highest `priority` wins (default 50; generic runtimes use 10), and every entry carries three texts and at least one `[[signature.fixture]]`, which Task 11 runs through the matcher.

`packs/signatures/web.toml`:

```toml
# Front-end dev servers: what people see. Licensed CC0-1.0 (see packs/LICENSE).
# Schema: crates/hm-core/src/signature.rs. Every entry needs at least one fixture.

[[signature]]
id = "vite"
name = "Vite"
role = "see"
kind = "dev_server"
default_port = 5173
http = true
[[signature.when]]
argv_basename = ["vite"]
[signature.texts]
beginner = "This is the live preview of your website while you work on it. It updates as soon as you save a file."
builder = "Vite dev server: serves the front end with hot module replacement. Default port 5173; when 5173 is busy it moves to the next free port unless server.strictPort is set."
expert = "Vite dev server (native ESM, esbuild pre-bundling). Port from --port or server.port (default 5173), host from --host (default localhost)."
[[signature.fixture]]
exe = "node"
argv = ["node", "/work/shop/apps/web/node_modules/.bin/vite", "--port", "5173"]
[[signature.fixture]]
exe = "node"
argv = ["node", "C:\\work\\shop\\node_modules\\vite\\bin\\vite.js"]

[[signature]]
id = "sveltekit"
name = "SvelteKit"
role = "see"
kind = "dev_server"
default_port = 5173
http = true
priority = 60
[[signature.when]]
argv_basename = ["vite"]
package = ["@sveltejs/kit"]
[signature.texts]
beginner = "This is the live preview of your SvelteKit website. It updates as soon as you save a file."
builder = "SvelteKit dev server (vite dev with the SvelteKit plugin). Default port 5173."
expert = "SvelteKit on Vite; port from --port or server.port in vite.config (default 5173)."
[[signature.fixture]]
exe = "node"
argv = ["node", "/work/blog/node_modules/.bin/vite", "dev"]
packages = ["@sveltejs/kit", "vite"]

[[signature]]
id = "next"
name = "Next.js"
role = "see"
kind = "dev_server"
default_port = 3000
http = true
[[signature.when]]
argv_basename = ["next"]
[[signature.when]]
argv_prefix = ["next-server"]
[[signature.when]]
exe = ["node"]
package = ["next"]
probably = true
[signature.texts]
beginner = "This is the live preview of your website, built with Next.js. It updates as soon as you save a file."
builder = "Next.js dev server (next dev): pages, API routes and hot reload. Default port 3000."
expert = "Next.js server; the process renames itself to next-server (vX). Port from -p/--port or PORT (default 3000). Changes to next.config.* restart it automatically."
[[signature.fixture]]
exe = "node"
argv = ["node", "/work/shop/apps/web/node_modules/.bin/next", "dev"]
[[signature.fixture]]
exe = "node"
argv = ["next-server (v15.5.4)"]
[[signature.fixture]]
exe = "node"
argv = ["node", "server.js"]
packages = ["next", "react"]

[[signature]]
id = "webpack"
name = "webpack dev server"
role = "see"
kind = "dev_server"
default_port = 3000
http = true
[[signature.when]]
argv_contains = ["react-scripts", "webpack-dev-server"]
[[signature.when]]
argv_basename = ["webpack", "webpack-cli"]
argv_contains = ["serve"]
[signature.texts]
beginner = "This is the live preview of your website while you work on it. It reloads the page when you save a file."
builder = "webpack-dev-server, often started by Create React App (react-scripts start). Default port 3000 for Create React App, 8080 for plain webpack."
expert = "webpack-dev-server; Create React App reads PORT and HOST from the environment, and proxy settings from package.json \"proxy\" or src/setupProxy.js."
[[signature.fixture]]
exe = "node"
argv = ["node", "/work/shop/node_modules/react-scripts/scripts/start.js"]
[[signature.fixture]]
exe = "node"
argv = ["node", "/work/shop/node_modules/.bin/webpack", "serve", "--port", "8080"]

[[signature]]
id = "angular"
name = "Angular"
role = "see"
kind = "dev_server"
default_port = 4200
http = true
[[signature.when]]
argv_basename = ["ng"]
[signature.texts]
beginner = "This is the live preview of your Angular website. It updates as soon as you save a file."
builder = "Angular CLI dev server (ng serve). Default port 4200."
expert = "ng serve; port from --port or the serve options in angular.json (default 4200)."
[[signature.fixture]]
exe = "node"
argv = ["node", "/work/shop/node_modules/.bin/ng", "serve"]

[[signature]]
id = "nuxt"
name = "Nuxt"
role = "see"
kind = "dev_server"
default_port = 3000
http = true
[[signature.when]]
argv_basename = ["nuxt", "nuxi"]
[[signature.when]]
exe = ["node"]
package = ["nuxt"]
probably = true
[signature.texts]
beginner = "This is the live preview of your Nuxt website. It updates as soon as you save a file."
builder = "Nuxt dev server (nuxi dev): Vue pages, server routes and hot reload. Default port 3000."
expert = "Nuxt dev server with Nitro for server routes; port from --port, NUXT_PORT or PORT (default 3000)."
[[signature.fixture]]
exe = "node"
argv = ["node", "/work/shop/node_modules/.bin/nuxi", "dev"]

[[signature]]
id = "remix"
name = "Remix / React Router"
role = "see"
kind = "dev_server"
default_port = 5173
http = true
[[signature.when]]
argv_basename = ["remix", "react-router"]
[signature.texts]
beginner = "This is the live preview of your website, built with Remix (React Router). It updates as soon as you save a file."
builder = "Remix or React Router framework dev server (remix vite:dev or react-router dev). Default port 5173."
expert = "Remix / React Router framework mode on Vite; port from --port or server.port in vite.config."
[[signature.fixture]]
exe = "node"
argv = ["node", "/work/shop/node_modules/.bin/react-router", "dev"]
[[signature.fixture]]
exe = "node"
argv = ["node", "/work/shop/node_modules/.bin/remix", "vite:dev"]

[[signature]]
id = "astro"
name = "Astro"
role = "see"
kind = "dev_server"
default_port = 4321
http = true
[[signature.when]]
argv_basename = ["astro"]
[signature.texts]
beginner = "This is the live preview of your Astro website. It updates as soon as you save a file."
builder = "Astro dev server (astro dev). Default port 4321."
expert = "Astro dev server on Vite; port from --port or server.port in astro.config (default 4321)."
[[signature.fixture]]
exe = "node"
argv = ["node", "/work/site/node_modules/.bin/astro", "dev"]
```

`packs/signatures/servers.toml`:

```toml
# Back-end servers, workers and language runtimes: what does the work.
# Licensed CC0-1.0 (see packs/LICENSE). Generic runtimes use priority 10 so any
# framework signature wins over them.

[[signature]]
id = "nestjs"
name = "NestJS"
role = "work"
kind = "dev_server"
default_port = 3000
http = true
priority = 40
[[signature.when]]
argv_basename = ["nest"]
[[signature.when]]
exe = ["node", "bun"]
package = ["@nestjs/core"]
probably = true
[signature.texts]
beginner = "This is your app's server program, built with NestJS. It answers requests from your website or app."
builder = "NestJS server (nest start). Default port 3000."
expert = "NestJS application on Node.js; the port is set by app.listen() in main.ts, often from PORT."
[[signature.fixture]]
exe = "node"
argv = ["node", "/work/shop/apps/api/node_modules/.bin/nest", "start", "--watch"]
[[signature.fixture]]
exe = "node"
argv = ["node", "dist/main.js"]
packages = ["@nestjs/core", "express"]

[[signature]]
id = "express"
name = "Express"
role = "work"
kind = "dev_server"
default_port = 3000
http = true
priority = 30
[[signature.when]]
exe = ["node", "bun"]
package = ["express"]
probably = true
[signature.texts]
beginner = "This is your app's server program, written in JavaScript with Express. It answers requests from your website or app."
builder = "Node.js server using Express (found in the package.json dependencies)."
expert = "Node.js process; Express inferred from package.json dependencies. The port usually comes from PORT or app.listen()."
[[signature.fixture]]
exe = "node"
argv = ["node", "server.js"]
packages = ["express"]

[[signature]]
id = "node"
name = "Node.js"
role = "work"
kind = "dev_server"
priority = 10
[[signature.when]]
exe = ["node", "bun", "deno"]
[signature.texts]
beginner = "This is a program written in JavaScript."
builder = "Node.js (or Bun/Deno) process. Harbormap could not tell which framework it uses."
expert = "Generic JavaScript runtime process; no framework marker in its arguments or dependencies."
[[signature.fixture]]
exe = "node"
argv = ["node", "index.js"]

[[signature]]
id = "django"
name = "Django"
role = "work"
kind = "dev_server"
default_port = 8000
http = true
priority = 55
[[signature.when]]
argv_basename = ["manage", "django-admin"]
argv_contains = ["runserver"]
[signature.texts]
beginner = "This is your app's server program, built with Django (Python). It answers requests from your website."
builder = "Django development server (manage.py runserver). Default port 8000; it reloads when Python files change."
expert = "Django runserver: an autoreloader parent and a worker child share the socket. Address from the runserver argument (default 127.0.0.1:8000)."
[[signature.fixture]]
exe = "python3"
argv = ["python3", "manage.py", "runserver", "8000"]
cwd_files = ["manage.py"]

[[signature]]
id = "flask"
name = "Flask"
role = "work"
kind = "dev_server"
default_port = 5000
http = true
priority = 40
[[signature.when]]
argv_basename = ["flask"]
argv_contains = ["run"]
[[signature.when]]
exe = ["python*"]
package = ["flask"]
probably = true
[signature.texts]
beginner = "This is your app's server program, built with Flask (Python). It answers requests from your website."
builder = "Flask development server (flask run). Default port 5000, which AirPlay Receiver often holds on macOS."
expert = "Flask/Werkzeug development server; port from --port or FLASK_RUN_PORT (default 5000)."
[[signature.fixture]]
exe = "python3"
argv = ["python3", "-m", "flask", "run"]
[[signature.fixture]]
exe = "python3.12"
argv = ["python3.12", "app.py"]
packages = ["flask"]

[[signature]]
id = "fastapi"
name = "FastAPI"
role = "work"
kind = "dev_server"
default_port = 8000
http = true
priority = 60
[[signature.when]]
argv_basename = ["fastapi"]
[[signature.when]]
argv_basename = ["uvicorn", "hypercorn", "granian"]
package = ["fastapi"]
[signature.texts]
beginner = "This is your app's server program, built with FastAPI (Python). It answers requests from your website or app."
builder = "FastAPI app served by Uvicorn. Default port 8000; interactive API docs at /docs."
expert = "FastAPI (ASGI) under uvicorn or the fastapi CLI; with --reload the reloader parent holds the socket and a worker child serves it."
[[signature.fixture]]
exe = "python3"
argv = ["python3", "-m", "uvicorn", "main:app", "--reload"]
packages = ["fastapi", "uvicorn"]
[[signature.fixture]]
exe = "python"
argv = ["/work/api/.venv/bin/python", "/work/api/.venv/bin/fastapi", "dev", "main.py"]

[[signature]]
id = "uvicorn"
name = "Uvicorn"
role = "work"
kind = "dev_server"
default_port = 8000
http = true
[[signature.when]]
argv_basename = ["uvicorn"]
[signature.texts]
beginner = "This is a Python server program that answers requests from your website or app."
builder = "Uvicorn ASGI server (Starlette, Litestar or another ASGI app). Default port 8000."
expert = "uvicorn; port from --port (default 8000), host from --host (default 127.0.0.1)."
[[signature.fixture]]
exe = "python3"
argv = ["python3", "-m", "uvicorn", "app:app"]

[[signature]]
id = "gunicorn"
name = "Gunicorn"
role = "work"
kind = "dev_server"
default_port = 8000
http = true
[[signature.when]]
argv_basename = ["gunicorn"]
[signature.texts]
beginner = "This is a Python server program that runs your web app."
builder = "Gunicorn WSGI server: one master process and several workers. Default port 8000."
expert = "gunicorn master with forked workers sharing the listening socket; bind from -b/--bind (default 127.0.0.1:8000)."
[[signature.fixture]]
exe = "python3"
argv = ["python3", "/work/shop/.venv/bin/gunicorn", "shop.wsgi"]

[[signature]]
id = "rails"
name = "Ruby on Rails"
role = "work"
kind = "dev_server"
default_port = 3000
http = true
[[signature.when]]
argv_basename = ["rails"]
[[signature.when]]
argv_prefix = ["puma "]
[signature.texts]
beginner = "This is your app's server program, built with Ruby on Rails. It answers requests from your website."
builder = "Rails server (Puma). Default port 3000."
expert = "Puma serving a Rails app; port from -p or PORT (default 3000)."
[[signature.fixture]]
exe = "ruby"
argv = ["ruby", "bin/rails", "server"]
[[signature.fixture]]
exe = "ruby"
argv = ["puma 6.4.3 (tcp://localhost:3000) [shop]"]

[[signature]]
id = "dotnet"
name = "ASP.NET Core"
role = "work"
kind = "dev_server"
default_port = 5000
http = true
[[signature.when]]
ancestor_exe = ["dotnet"]
cwd_files = ["*.csproj", "*.fsproj"]
[[signature.when]]
exe = ["dotnet"]
argv_contains = [".dll"]
[[signature.when]]
ancestor_exe = ["dotnet"]
probably = true
[signature.texts]
beginner = "This is your app's server program, built with .NET (ASP.NET Core). It answers requests from your website or app."
builder = "ASP.NET Core app (Kestrel), usually started with dotnet run. Its URLs come from Properties/launchSettings.json or --urls."
expert = "Kestrel; URLs from --urls, ASPNETCORE_URLS / ASPNETCORE_HTTP_PORTS or launchSettings.json applicationUrl (default http://localhost:5000)."
[[signature.fixture]]
exe = "DotnetApp"
argv = ["/work/shop/api/bin/Debug/net8.0/DotnetApp"]
ancestors = ["dotnet"]
cwd_files = ["DotnetApp.csproj", "Program.cs"]
[[signature.fixture]]
exe = "dotnet"
argv = ["dotnet", "bin/Debug/net8.0/DotnetApp.dll"]

[[signature]]
id = "spring"
name = "Spring Boot"
role = "work"
kind = "dev_server"
default_port = 8080
http = true
[[signature.when]]
exe = ["java", "javaw"]
argv_contains = ["org.springframework.boot", "spring-boot"]
[signature.texts]
beginner = "This is your app's server program, built with Spring Boot (Java). It answers requests from your website or app."
builder = "Spring Boot application with an embedded web server. Default port 8080."
expert = "Spring Boot JVM; port from server.port in application.properties/yml, SERVER_PORT or --server.port (default 8080)."
[[signature.fixture]]
exe = "java"
argv = ["java", "-cp", "/work/api/target/classes:/home/me/.m2/repository/org/springframework/boot/spring-boot/3.3.4/spring-boot-3.3.4.jar", "com.example.Application"]
[[signature.fixture]]
exe = "java"
argv = ["java", "org.springframework.boot.loader.launch.JarLauncher"]

[[signature]]
id = "java"
name = "Java"
role = "work"
kind = "dev_server"
priority = 10
[[signature.when]]
exe = ["java", "javaw"]
[signature.texts]
beginner = "This is a program written in Java."
builder = "Java process. Harbormap could not tell which framework it uses."
expert = "Generic JVM process; no framework marker in its arguments."
[[signature.fixture]]
exe = "java"
argv = ["java", "-jar", "app.jar"]

[[signature]]
id = "go"
name = "Go"
role = "work"
kind = "dev_server"
[[signature.when]]
ancestor_exe = ["go", "air"]
[[signature.when]]
argv_contains = ["/go-build", "\\go-build"]
[signature.texts]
beginner = "This is a program written in Go, started from your project."
builder = "Go program, started with go run or a live-reload tool such as air."
expert = "Go binary; go run builds it into a go-build temporary directory and runs it as a child of the go tool."
[[signature.fixture]]
exe = "main"
argv = ["/var/folders/zz/T/go-build3456789012/b001/exe/main"]
ancestors = ["go"]
[[signature.fixture]]
exe = "server"
argv = ["./tmp/server"]
ancestors = ["air"]

[[signature]]
id = "celery"
name = "Celery"
role = "work"
kind = "worker"
[[signature.when]]
argv_basename = ["celery"]
argv_contains = ["worker"]
[signature.texts]
beginner = "This is a background worker. It does slow jobs, like sending emails, so your app stays fast."
builder = "Celery worker: runs background tasks taken from a broker such as Redis or RabbitMQ."
expert = "Celery worker (prefork pool by default); broker from CELERY_BROKER_URL or the app configuration."
[[signature.fixture]]
exe = "python3"
argv = ["python3", "/work/shop/.venv/bin/celery", "-A", "shop", "worker", "-l", "info"]

[[signature]]
id = "python"
name = "Python"
role = "work"
kind = "dev_server"
priority = 10
[[signature.when]]
exe = ["python*", "pythonw*"]
[signature.texts]
beginner = "This is a program written in Python."
builder = "Python process. Harbormap could not tell which framework it uses."
expert = "Generic Python interpreter process; no framework marker in its arguments or dependencies."
[[signature.fixture]]
exe = "python3.13"
argv = ["python3.13", "worker.py"]
```

`packs/signatures/data.toml`:

```toml
# Databases, caches, storage, brokers and web servers. Licensed CC0-1.0 (see packs/LICENSE).
# `image` compares the last path segment of a container image without tag or digest.

[[signature]]
id = "postgres"
name = "PostgreSQL"
role = "keep"
kind = "database"
default_port = 5432
[[signature.when]]
exe = ["postgres", "postmaster"]
[[signature.when]]
image = ["postgres", "postgresql", "postgis", "pgvector", "timescaledb", "timescaledb-ha"]
[signature.texts]
beginner = "This is your project's database. It stores your app's data, such as users and orders."
builder = "PostgreSQL database server. Default port 5432."
expert = "PostgreSQL postmaster; data lives in PGDATA, or in the /var/lib/postgresql/data volume of a container."
[[signature.fixture]]
exe = "postgres"
argv = ["/opt/homebrew/opt/postgresql@16/bin/postgres", "-D", "/opt/homebrew/var/postgresql@16"]
[[signature.fixture]]
image = "postgres:16-alpine"
[[signature.fixture]]
image = "docker.io/bitnami/postgresql:16"

[[signature]]
id = "mysql"
name = "MySQL / MariaDB"
role = "keep"
kind = "database"
default_port = 3306
[[signature.when]]
exe = ["mysqld", "mariadbd"]
[[signature.when]]
image = ["mysql", "mariadb", "percona-server"]
[signature.texts]
beginner = "This is your project's database (MySQL or MariaDB). It stores your app's data."
builder = "MySQL or MariaDB server. Default port 3306."
expert = "mysqld / mariadbd; classic protocol on 3306, X protocol on 33060 for MySQL 8."
[[signature.fixture]]
exe = "mysqld"
argv = ["/usr/sbin/mysqld"]
[[signature.fixture]]
image = "mysql:8.4"
[[signature.fixture]]
image = "mariadb:11"

[[signature]]
id = "mongo"
name = "MongoDB"
role = "keep"
kind = "database"
default_port = 27017
[[signature.when]]
exe = ["mongod", "mongos"]
[[signature.when]]
image = ["mongo", "mongodb-community-server"]
[signature.texts]
beginner = "This is your project's database (MongoDB). It stores your app's data as documents."
builder = "MongoDB server. Default port 27017."
expert = "mongod; wire protocol on 27017, data in --dbpath or the container's /data/db volume."
[[signature.fixture]]
exe = "mongod"
argv = ["mongod", "--dbpath", "/data/db"]
[[signature.fixture]]
image = "mongo:7"

[[signature]]
id = "redis"
name = "Redis"
role = "keep"
kind = "cache"
default_port = 6379
[[signature.when]]
exe = ["redis-server", "valkey-server", "keydb-server"]
[[signature.when]]
image = ["redis", "valkey", "redis-stack", "redis-stack-server", "keydb"]
[signature.texts]
beginner = "This is a fast memory store. Your app keeps short-lived data here, such as logins or a queue of jobs."
builder = "Redis-compatible in-memory store (Redis, Valkey or KeyDB). Default port 6379."
expert = "RESP protocol on 6379; data persists only when RDB or AOF persistence is configured."
[[signature.fixture]]
exe = "redis-server"
argv = ["redis-server *:6379"]
[[signature.fixture]]
image = "redis:7-alpine"
[[signature.fixture]]
image = "valkey/valkey:8"

[[signature]]
id = "search"
name = "Elasticsearch / OpenSearch"
role = "keep"
kind = "database"
default_port = 9200
http = true
[[signature.when]]
image = ["elasticsearch", "opensearch"]
[[signature.when]]
exe = ["java"]
argv_contains = ["org.elasticsearch.bootstrap", "org.opensearch.bootstrap"]
[signature.texts]
beginner = "This is a search engine for your app's data. It powers search boxes and filters."
builder = "Elasticsearch or OpenSearch. HTTP API on 9200, node-to-node transport on 9300."
expert = "JVM search node; REST on 9200, transport on 9300, heap from ES_JAVA_OPTS or OPENSEARCH_JAVA_OPTS."
[[signature.fixture]]
image = "docker.elastic.co/elasticsearch/elasticsearch:8.15.0"
[[signature.fixture]]
image = "opensearchproject/opensearch:2"
[[signature.fixture]]
exe = "java"
argv = ["java", "-Xms1g", "org.elasticsearch.bootstrap.Elasticsearch"]

[[signature]]
id = "minio"
name = "MinIO"
role = "keep"
kind = "database"
default_port = 9000
http = true
[[signature.when]]
exe = ["minio"]
[[signature.when]]
image = ["minio"]
[signature.texts]
beginner = "This stores files for your app, like an online drive."
builder = "MinIO S3-compatible object storage. API on 9000; the web console usually runs on 9001."
expert = "MinIO server; S3 API on 9000, console from --console-address (often :9001)."
[[signature.fixture]]
exe = "minio"
argv = ["minio", "server", "/data"]
[[signature.fixture]]
image = "minio/minio:latest"

[[signature]]
id = "rabbitmq"
name = "RabbitMQ"
role = "keep"
kind = "database"
default_port = 5672
[[signature.when]]
exe = ["beam.smp", "beam"]
argv_contains = ["rabbit"]
[[signature.when]]
image = ["rabbitmq"]
[signature.texts]
beginner = "This passes messages between parts of your app, so work can wait in line safely."
builder = "RabbitMQ message broker. AMQP on 5672, management web UI on 15672."
expert = "RabbitMQ on the Erlang VM (beam.smp); AMQP 5672, management 15672, epmd 4369."
[[signature.fixture]]
exe = "beam.smp"
argv = ["/usr/lib/erlang/erts-15.0/bin/beam.smp", "-W", "w", "--", "-root", "/usr/lib/erlang", "-s", "rabbit", "boot"]
[[signature.fixture]]
image = "rabbitmq:3-management"

[[signature]]
id = "nginx"
name = "nginx"
role = "see"
kind = "dev_server"
default_port = 80
http = true
[[signature.when]]
exe = ["nginx"]
[[signature.when]]
image = ["nginx", "nginx-unprivileged", "openresty"]
[signature.texts]
beginner = "This is a web server. It sends pages to the browser or passes requests on to your app."
builder = "nginx web server or reverse proxy. Default port 80 (8080 in unprivileged images)."
expert = "nginx master and worker processes; ports come from the listen directives in nginx.conf and conf.d."
[[signature.fixture]]
exe = "nginx"
argv = ["nginx: master process nginx -g daemon off;"]
[[signature.fixture]]
image = "nginx:1.27-alpine"
```

`packs/signatures/system.toml`:

```toml
# Container engine forwarders and operating-system owners of ports.
# Licensed CC0-1.0 (see packs/LICENSE). `forwarder = true` means published container
# ports appear through this program; Harbormap then names the container instead.

[[signature]]
id = "docker-forwarder"
name = "Docker port forwarder"
role = "unknown"
kind = "system"
forwarder = true
[[signature.when]]
exe = ["com.docker.backend", "com.docker.vpnkit", "vpnkit-bridge", "com.docker.proxy", "docker-proxy", "rootlessport", "rootlesskit", "pasta", "slirp4netns", "gvproxy"]
[signature.texts]
beginner = "This is Docker passing a port from a container to your computer."
builder = "Container engine port forwarder: a container's published port (docker run -p or compose ports) appears on this computer through it."
expert = "Engine port forwarder (Docker Desktop backend, docker-proxy, rootlesskit/pasta or gvproxy); Harbormap resolves the container by its published port."
[[signature.fixture]]
exe = "com.docker.backend"
argv = ["/Applications/Docker.app/Contents/MacOS/com.docker.backend", "services"]
[[signature.fixture]]
exe = "docker-proxy"
argv = ["/usr/bin/docker-proxy", "-proto", "tcp", "-host-ip", "0.0.0.0", "-host-port", "5432", "-container-ip", "172.17.0.2", "-container-port", "5432"]

[[signature]]
id = "orbstack"
name = "OrbStack"
role = "unknown"
kind = "system"
forwarder = true
os = "macos"
[[signature.when]]
exe = ["orbstack helper", "orbstack"]
[signature.texts]
beginner = "This is OrbStack passing a port from a container or Linux machine to your Mac."
builder = "OrbStack port forwarder for containers and Linux machines."
expert = "OrbStack Helper forwards published container ports and machine ports to macOS."
[[signature.fixture]]
exe = "OrbStack Helper"
argv = ["/Applications/OrbStack.app/Contents/Frameworks/OrbStack Helper.app/Contents/MacOS/OrbStack Helper", "vmgr"]
os = "macos"

[[signature]]
id = "lima"
name = "Colima / Lima"
role = "unknown"
kind = "system"
forwarder = true
[[signature.when]]
exe = ["limactl"]
[[signature.when]]
exe = ["ssh"]
argv_contains = ["/.lima/", "/.colima/"]
[signature.texts]
beginner = "This is Colima (or Lima) passing a port from a container to your computer."
builder = "Lima host agent forwarding ports from the Colima/Lima virtual machine."
expert = "limactl hostagent (or an ssh tunnel in older Lima) forwards guest ports to the host."
[[signature.fixture]]
exe = "limactl"
argv = ["limactl", "hostagent", "--pidfile", "/Users/me/.colima/_lima/colima/ha.pid", "colima"]

[[signature]]
id = "wslrelay"
name = "WSL port relay"
role = "unknown"
kind = "system"
os = "windows"
[[signature.when]]
exe = ["wslrelay"]
[signature.texts]
beginner = "This is Windows passing a port from a Linux program running inside WSL."
builder = "wslrelay forwards localhost ports from a WSL 2 distribution to Windows."
expert = "wslrelay.exe relays a WSL 2 guest listener to the Windows loopback interface."
[[signature.fixture]]
exe = "wslrelay.exe"
argv = ["C:\\Program Files\\WSL\\wslrelay.exe", "--mode", "1"]
os = "windows"

[[signature]]
id = "airplay"
name = "AirPlay Receiver"
role = "unknown"
kind = "system"
os = "macos"
[[signature.when]]
exe = ["controlcenter"]
[signature.texts]
beginner = "This is AirPlay Receiver, a macOS feature that lets other devices play music or video on this Mac. It is not part of your project. To free this port, turn off AirPlay Receiver in System Settings → General → AirDrop & Handoff."
builder = "macOS AirPlay Receiver (ControlCenter) listens on 5000 and 7000. Turn it off in System Settings → General → AirDrop & Handoff, or use another port."
expert = "ControlCenter.app (AirPlay Receiver) binds *:5000 and *:7000 since macOS 12."
[[signature.fixture]]
exe = "ControlCenter"
argv = ["/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter"]
os = "macos"

[[signature]]
id = "http-sys"
name = "Windows HTTP service"
role = "unknown"
kind = "system"
os = "windows"
[[signature.when]]
exe = ["system"]
[signature.texts]
beginner = "This port is held by Windows itself (its web service), not by a program you started."
builder = "Windows HTTP Server API (http.sys, PID 4). IIS, WinRM and URL reservations register ports through it."
expert = "http.sys kernel driver under PID 4; inspect with netsh http show servicestate."
[[signature.fixture]]
exe = "System"
os = "windows"
```

`packs/signatures/tools.toml`:

```toml
# Editors, AI coding agents and test browsers. Licensed CC0-1.0 (see packs/LICENSE).
# Editor and agent ports are never treated as project services, even when the
# program's working folder is inside a project.

[[signature]]
id = "claude-code"
name = "Claude Code"
role = "unknown"
kind = "agent_cli"
[[signature.when]]
exe = ["claude"]
[signature.texts]
beginner = "This is Claude Code, an AI coding assistant. It is not part of your project's app."
builder = "Claude Code CLI session; it may listen on localhost to talk to your editor."
expert = "Claude Code CLI; local ports are used for IDE integration."
[[signature.fixture]]
exe = "claude"
argv = ["claude"]

[[signature]]
id = "codex"
name = "Codex CLI"
role = "unknown"
kind = "agent_cli"
[[signature.when]]
exe = ["codex"]
[[signature.when]]
argv_basename = ["codex"]
[signature.texts]
beginner = "This is Codex, an AI coding assistant. It is not part of your project's app."
builder = "OpenAI Codex CLI session."
expert = "Codex CLI process (native binary or the npm launcher)."
[[signature.fixture]]
exe = "codex"
argv = ["codex"]

[[signature]]
id = "gemini-cli"
name = "Gemini CLI"
role = "unknown"
kind = "agent_cli"
[[signature.when]]
argv_basename = ["gemini"]
[signature.texts]
beginner = "This is Gemini CLI, an AI coding assistant. It is not part of your project's app."
builder = "Google Gemini CLI session."
expert = "Gemini CLI running on Node.js."
[[signature.fixture]]
exe = "node"
argv = ["node", "/usr/local/bin/gemini"]

[[signature]]
id = "vscode"
name = "Visual Studio Code"
role = "unknown"
kind = "ide"
[[signature.when]]
exe = ["code", "code helper*", "code - insiders", "code-insiders"]
[signature.texts]
beginner = "This belongs to your code editor, Visual Studio Code. It is not part of your project's app."
builder = "VS Code or one of its helper processes (extensions, language servers, debuggers)."
expert = "VS Code Electron process; extension hosts and debug adapters open localhost ports."
[[signature.fixture]]
exe = "Code Helper (Plugin)"
argv = ["/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)"]

[[signature]]
id = "cursor"
name = "Cursor"
role = "unknown"
kind = "ide"
[[signature.when]]
exe = ["cursor", "cursor helper*"]
[signature.texts]
beginner = "This belongs to your code editor, Cursor. It is not part of your project's app."
builder = "Cursor editor or one of its helper processes."
expert = "Cursor Electron process; extension hosts open localhost ports."
[[signature.fixture]]
exe = "Cursor Helper (Plugin)"

[[signature]]
id = "jetbrains"
name = "JetBrains IDE"
role = "unknown"
kind = "ide"
[[signature.when]]
exe = ["idea", "pycharm", "webstorm", "goland", "rider", "clion", "phpstorm", "rubymine", "datagrip", "rustrover", "fleet"]
[signature.texts]
beginner = "This belongs to your code editor (a JetBrains IDE). It is not part of your project's app."
builder = "JetBrains IDE (IntelliJ IDEA, PyCharm, WebStorm, ...)."
expert = "JetBrains IDE JVM; port 63342 is its built-in web server."
[[signature.fixture]]
exe = "pycharm"

[[signature]]
id = "zed"
name = "Zed"
role = "unknown"
kind = "ide"
[[signature.when]]
exe = ["zed", "zed-editor"]
[signature.texts]
beginner = "This belongs to your code editor, Zed. It is not part of your project's app."
builder = "Zed editor process."
expert = "Zed editor; language servers it starts may open local ports."
[[signature.fixture]]
exe = "zed"

[[signature]]
id = "headless-browser"
name = "Headless browser"
role = "unknown"
kind = "unknown"
[[signature.when]]
exe = ["google chrome", "chromium", "chrome", "chrome-headless-shell", "msedge", "microsoft edge"]
argv_contains = ["--headless", "--remote-debugging-port"]
[signature.texts]
beginner = "This is a web browser running without a window. Tests or an AI agent usually start it to check your app, and sometimes forget to close it."
builder = "Headless Chrome, Chromium or Edge with a remote-debugging port, typically started by Playwright, Puppeteer or an agent's browser tool."
expert = "Browser started with --headless and/or --remote-debugging-port; the port speaks the Chrome DevTools Protocol."
[[signature.fixture]]
exe = "Google Chrome"
argv = ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "--headless=new", "--remote-debugging-port=9877", "about:blank"]
```

Then add this above the `#[cfg(test)]` line in `crates/hm-doctor/src/packs.rs` (the `include_str!` paths are relative to that file and point at the repository's `packs/` folder):

```rust
//! Signature packs compiled into the binary and validated on load. A broken entry is
//! skipped with a warning; it never stops Harbormap.

use std::collections::HashSet;

use hm_core::signature::{Level, Signature, SignatureSet};

pub const SIGNATURE_FILES: &[(&str, &str)] = &[
    (
        "signatures/web.toml",
        include_str!("../../../packs/signatures/web.toml"),
    ),
    (
        "signatures/servers.toml",
        include_str!("../../../packs/signatures/servers.toml"),
    ),
    (
        "signatures/data.toml",
        include_str!("../../../packs/signatures/data.toml"),
    ),
    (
        "signatures/system.toml",
        include_str!("../../../packs/signatures/system.toml"),
    ),
    (
        "signatures/tools.toml",
        include_str!("../../../packs/signatures/tools.toml"),
    ),
];

#[derive(Debug, Clone)]
pub struct Packs {
    pub signatures: SignatureSet,
    pub warnings: Vec<String>,
}

pub fn load_embedded() -> Packs {
    let mut warnings = Vec::new();
    let mut signatures = Vec::new();
    for (file, text) in SIGNATURE_FILES {
        signatures.extend(parse_signatures(file, text, &mut warnings));
    }
    let signatures = drop_duplicate_ids(signatures, &mut warnings);
    Packs {
        signatures: SignatureSet::new(signatures),
        warnings,
    }
}

/// Parses one signature file entry by entry, so one bad entry does not hide the rest.
pub fn parse_signatures(file: &str, text: &str, warnings: &mut Vec<String>) -> Vec<Signature> {
    let table: toml::Table = match toml::from_str(text) {
        Ok(table) => table,
        Err(error) => {
            warnings.push(format!("{file}: {error}"));
            return Vec::new();
        }
    };
    let Some(entries) = table.get("signature").and_then(toml::Value::as_array) else {
        warnings.push(format!("{file}: no [[signature]] entries"));
        return Vec::new();
    };
    let mut signatures = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let label = entry
            .get("id")
            .and_then(toml::Value::as_str)
            .unwrap_or("?")
            .to_string();
        let parsed = entry
            .clone()
            .try_into::<Signature>()
            .map_err(|error| error.to_string())
            .and_then(|signature| validate(&signature).map(|()| signature));
        match parsed {
            Ok(signature) => signatures.push(signature),
            Err(problem) => warnings.push(format!(
                "{file}: signature #{} ({label}) skipped: {}",
                index + 1,
                problem.trim()
            )),
        }
    }
    signatures
}

fn validate(signature: &Signature) -> Result<(), String> {
    let id_ok = !signature.id.is_empty()
        && signature
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !id_ok {
        return Err("id must use lowercase letters, digits and dashes".into());
    }
    if signature.name.trim().is_empty() {
        return Err("name is empty".into());
    }
    if signature.when.is_empty() || signature.when.iter().any(|when| when.is_empty()) {
        return Err("every `when` block needs at least one condition".into());
    }
    if signature.fixture.is_empty() {
        return Err("at least one fixture is required".into());
    }
    for level in [Level::Beginner, Level::Builder, Level::Expert] {
        if signature.texts.get(level).trim().is_empty() {
            return Err(format!("the {level:?} text is empty"));
        }
    }
    if let Some(os) = &signature.os
        && !["macos", "linux", "windows"].contains(&os.as_str())
    {
        return Err("os must be macos, linux or windows".into());
    }
    Ok(())
}

fn drop_duplicate_ids(signatures: Vec<Signature>, warnings: &mut Vec<String>) -> Vec<Signature> {
    let mut seen = HashSet::new();
    signatures
        .into_iter()
        .filter(|signature| {
            let fresh = seen.insert(signature.id.clone());
            if !fresh {
                warnings.push(format!("duplicate signature id {} skipped", signature.id));
            }
            fresh
        })
        .collect()
}

```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p hm-doctor packs`
Expected: PASS — `test result: ok. 3 passed`

- [ ] **Step 5: Commit**

```bash
git add packs/signatures/web.toml packs/signatures/servers.toml packs/signatures/data.toml packs/signatures/system.toml packs/signatures/tools.toml crates/hm-doctor/src/lib.rs crates/hm-doctor/src/packs.rs
git commit -s -m "feat(doctor): embed and validate the signature packs"
```

### Task 10: Paths, worlds and git

Resolution has to decide which project a folder belongs to, and its tests must run on any OS without touching the real disk. This task adds path rules that work for macOS, Linux and Windows paths alike (Windows paths compare without case), the `World` trait with a JSON-recorded `FixtureWorld` (plus the `Scenario` recording format that later tests load), and `RealWorld`, which reads the filesystem and finds git repositories and their linked worktrees through gix.

**Files:**
- Create: `crates/hm-project/src/paths.rs`
- Create: `crates/hm-project/src/world.rs`
- Create: `crates/hm-project/src/git.rs`
- Modify: `crates/hm-project/src/lib.rs` (whole content: crate doc comment, `pub mod git;`, `pub mod paths;`, `pub mod world;` and `pub use world::{FixtureWorld, RepoInfo, Scenario, World};`)
- Test: `crates/hm-project/src/paths.rs`, `crates/hm-project/src/world.rs`, `crates/hm-project/src/git.rs` (unit tests in the same files)

**Interfaces:**
- Consumes: `hm_core::model::Worktree { pub path: PathBuf, pub branch: Option<String>, pub head: Option<String> }` and `hm_core::raw::RawInventory` (Task 4); the `gix`, `dunce`, `serde`, `serde_json` and (dev) `tempfile` crates (Task 1).
- Produces:
  - `hm_project::paths`: `pub fn within(child: &Path, parent: &Path, windows: bool) -> bool`, `pub fn is_boundary(dir: &Path, home: Option<&Path>, windows: bool) -> bool`
  - `hm_project::world`: `pub struct RepoInfo { pub root: PathBuf, pub worktrees: Vec<Worktree> }`; `pub trait World { fn repo_for(&self, dir: &Path) -> Option<RepoInfo>; fn read_to_string(&self, path: &Path) -> Option<String>; fn file_names(&self, dir: &Path) -> Vec<String>; fn home(&self) -> Option<PathBuf>; fn canonicalize(&self, path: &Path) -> PathBuf; fn windows(&self) -> bool; }`; `pub struct FixtureWorld { pub home: Option<PathBuf>, pub windows: bool, pub repos: Vec<RepoInfo>, pub files: BTreeMap<PathBuf, String> }` (implements `World`, `Default`, serde); `pub struct Scenario { pub launch_dir: PathBuf, pub world: FixtureWorld, pub inventory: RawInventory }` (serde)
  - `hm_project::git`: `pub struct RealWorld` with `pub fn new() -> Self` (implements `World`, `Default`), `pub fn discover(dir: &Path) -> Option<RepoInfo>`
  - crate root: `pub use world::{FixtureWorld, RepoInfo, Scenario, World};`

- [ ] **Step 1: Write the failing test**

Declare the three modules and create each file with only its tests; the git test builds a real repository with a linked worktree in a temp folder by shelling out to `git`, and returns early (passing) when `git` is not installed.

Replace the whole content of `crates/hm-project/src/lib.rs` with:

```rust
//! Turns a raw inventory into a snapshot: which project and worktree each process
//! and container belongs to, what each program is (signatures), which services
//! exist and who owns every listening port.

pub mod git;
pub mod paths;
pub mod world;
```

Create `crates/hm-project/src/paths.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn within_respects_component_boundaries() {
        assert!(within(
            Path::new("/work/shop/apps"),
            Path::new("/work/shop"),
            false
        ));
        assert!(within(
            Path::new("/work/shop"),
            Path::new("/work/shop/"),
            false
        ));
        assert!(!within(
            Path::new("/work/shopping"),
            Path::new("/work/shop"),
            false
        ));
        assert!(within(Path::new("/anything"), Path::new("/"), false));
    }

    #[test]
    fn windows_paths_ignore_case_and_separators() {
        assert!(within(
            Path::new(r"C:\Work\Shop\apps\"),
            Path::new("c:/work/shop"),
            true
        ));
        assert!(!within(
            Path::new(r"C:\Work\Shop"),
            Path::new("c:/work/shop"),
            false
        ));
    }

    #[test]
    fn home_and_its_ancestors_are_boundaries() {
        let home = Some(Path::new("/home/me"));
        assert!(is_boundary(Path::new("/"), home, false));
        assert!(is_boundary(Path::new("/home"), home, false));
        assert!(is_boundary(Path::new("/home/me"), home, false));
        assert!(!is_boundary(Path::new("/home/me/shop"), home, false));
        assert!(is_boundary(Path::new("C:/"), None, true));
    }
}
```

Create `crates/hm-project/src/world.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> FixtureWorld {
        serde_json::from_str(
            r#"{
              "home": "/home/me",
              "repos": [
                {"root": "/work/shop", "worktrees": [
                  {"path": "/work/shop", "branch": "main", "head": "a1"},
                  {"path": "/work/shop-feature", "branch": "feature", "head": "b2"}
                ]},
                {"root": "/work/shop/vendor/lib", "worktrees": [
                  {"path": "/work/shop/vendor/lib", "branch": "main", "head": "c3"}
                ]}
              ],
              "files": {
                "/work/shop/apps/web/package.json": "{\"name\": \"web\"}",
                "/work/shop/apps/web/vite.config.ts": ""
              }
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn finds_the_innermost_repository_including_linked_worktrees() {
        let world = world();
        assert_eq!(
            world
                .repo_for(Path::new("/work/shop/apps/web"))
                .unwrap()
                .root,
            Path::new("/work/shop")
        );
        assert_eq!(
            world
                .repo_for(Path::new("/work/shop-feature/api"))
                .unwrap()
                .root,
            Path::new("/work/shop")
        );
        assert_eq!(
            world
                .repo_for(Path::new("/work/shop/vendor/lib/src"))
                .unwrap()
                .root,
            Path::new("/work/shop/vendor/lib")
        );
        assert!(world.repo_for(Path::new("/tmp")).is_none());
    }

    #[test]
    fn lists_files_of_a_directory() {
        assert_eq!(
            world().file_names(Path::new("/work/shop/apps/web")),
            vec!["package.json", "vite.config.ts"]
        );
    }
}
```

Create `crates/hm-project/src/git.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    #[test]
    fn discovers_root_branch_and_linked_worktrees() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("shop");
        std::fs::create_dir_all(repo.join("apps/web")).unwrap();
        if !git(&repo, &["init", "-q", "-b", "main"]) {
            eprintln!("git is not installed; skipping");
            return;
        }
        assert!(git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]));
        assert!(git(
            &repo,
            &["worktree", "add", "-q", "-b", "feature", "../shop-feature"]
        ));

        let info = discover(&repo.join("apps/web")).unwrap();
        assert_eq!(info.root, canonical(&repo));
        assert_eq!(info.worktrees.len(), 2);
        assert_eq!(info.worktrees[0].branch.as_deref(), Some("main"));
        assert_eq!(
            info.worktrees[1].path,
            canonical(&temp.path().join("shop-feature"))
        );
        assert_eq!(info.worktrees[1].branch.as_deref(), Some("feature"));
        assert_eq!(info.worktrees[0].head.as_ref().map(String::len), Some(40));

        let from_linked = discover(&temp.path().join("shop-feature")).unwrap();
        assert_eq!(from_linked.root, info.root);
    }

    #[test]
    fn a_folder_without_git_has_no_repository() {
        let temp = tempfile::tempdir().unwrap();
        // The temp dir might sit inside a repository on a developer machine; only
        // assert when it does not.
        if gix::discover(temp.path()).is_err() {
            assert!(discover(temp.path()).is_none());
        }
    }
}
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p hm-project`
Expected: FAIL — compile errors such as ``error[E0425]: cannot find function `within` in this scope``.

- [ ] **Step 3: Write the implementation**

Add this above the `#[cfg(test)]` line in `crates/hm-project/src/paths.rs`:

```rust
//! Path comparisons that behave the same for real paths and recorded fixtures.
//! Windows paths compare case-insensitively.

use std::path::Path;

fn normalized(path: &Path, windows: bool) -> String {
    let mut text = path.to_string_lossy().replace('\\', "/");
    while text.len() > 1 && text.ends_with('/') && !text.ends_with(":/") {
        text.pop();
    }
    if windows {
        text = text.to_lowercase();
    }
    text
}

/// True when `child` is `parent` or lies inside it.
pub fn within(child: &Path, parent: &Path, windows: bool) -> bool {
    let (child, parent) = (normalized(child, windows), normalized(parent, windows));
    child == parent
        || (child.starts_with(&parent)
            && (parent.ends_with('/') || child[parent.len()..].starts_with('/')))
}

/// Directories that must never become a project: the filesystem root, a drive root,
/// the home directory and its ancestors.
pub fn is_boundary(dir: &Path, home: Option<&Path>, windows: bool) -> bool {
    if dir.parent().is_none() || normalized(dir, windows).ends_with(":/") {
        return true;
    }
    match home {
        Some(home) => within(home, dir, windows),
        None => false,
    }
}

```

Add this above the `#[cfg(test)]` line in `crates/hm-project/src/world.rs`:

```rust
//! The read-only view of the filesystem and git that resolution needs. `RealWorld`
//! (in `git.rs`) reads the machine; `FixtureWorld` replays a recorded JSON world so
//! resolution can be tested on any OS.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use hm_core::model::Worktree;
use hm_core::raw::RawInventory;

use crate::paths::within;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoInfo {
    /// Root of the main worktree; this is the project root.
    pub root: PathBuf,
    /// Every worktree of the repository, the main one first.
    pub worktrees: Vec<Worktree>,
}

pub trait World {
    /// The repository whose worktree contains `dir`, if any.
    fn repo_for(&self, dir: &Path) -> Option<RepoInfo>;
    fn read_to_string(&self, path: &Path) -> Option<String>;
    /// Names of the files directly inside `dir`.
    fn file_names(&self, dir: &Path) -> Vec<String>;
    fn home(&self) -> Option<PathBuf>;
    fn canonicalize(&self, path: &Path) -> PathBuf;
    /// True when paths compare case-insensitively (Windows).
    fn windows(&self) -> bool;
}

/// A recorded world: repositories, file contents and the home directory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FixtureWorld {
    pub home: Option<PathBuf>,
    pub windows: bool,
    pub repos: Vec<RepoInfo>,
    /// File path → contents.
    pub files: BTreeMap<PathBuf, String>,
}

impl World for FixtureWorld {
    fn repo_for(&self, dir: &Path) -> Option<RepoInfo> {
        self.repos
            .iter()
            .filter_map(|repo| {
                repo.worktrees
                    .iter()
                    .filter(|worktree| within(dir, &worktree.path, self.windows))
                    .map(|worktree| worktree.path.as_os_str().len())
                    .max()
                    .map(|depth| (depth, repo))
            })
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, repo)| repo.clone())
    }

    fn read_to_string(&self, path: &Path) -> Option<String> {
        self.files.get(path).cloned()
    }

    fn file_names(&self, dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = self
            .files
            .keys()
            .filter(|path| path.parent() == Some(dir))
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn home(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn canonicalize(&self, path: &Path) -> PathBuf {
        path.to_path_buf()
    }

    fn windows(&self) -> bool {
        self.windows
    }
}

/// A recorded machine: what the probes saw, the world around it and the folder
/// Harbormap was started in. Tests use it, and so does the CLI in debug builds when
/// `HARBORMAP_FIXTURE` points at one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scenario {
    pub launch_dir: PathBuf,
    pub world: FixtureWorld,
    pub inventory: RawInventory,
}

```

Add this above the `#[cfg(test)]` line in `crates/hm-project/src/git.rs`:

```rust
//! The real world: git through gix (no `git` binary needed) and the local filesystem.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use hm_core::model::Worktree;

use crate::world::{RepoInfo, World};

/// Reads the machine. Repository lookups are cached per directory.
#[derive(Debug, Default)]
pub struct RealWorld {
    repos: RefCell<HashMap<PathBuf, Option<RepoInfo>>>,
}

impl RealWorld {
    pub fn new() -> Self {
        Self::default()
    }
}

impl World for RealWorld {
    fn repo_for(&self, dir: &Path) -> Option<RepoInfo> {
        if let Some(cached) = self.repos.borrow().get(dir) {
            return cached.clone();
        }
        let found = discover(dir);
        self.repos
            .borrow_mut()
            .insert(dir.to_path_buf(), found.clone());
        found
    }

    fn read_to_string(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn file_names(&self, dir: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn home(&self) -> Option<PathBuf> {
        std::env::home_dir().map(|home| self.canonicalize(&home))
    }

    fn canonicalize(&self, path: &Path) -> PathBuf {
        dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn windows(&self) -> bool {
        cfg!(windows)
    }
}

/// The repository containing `dir`: main worktree root plus every linked worktree.
pub fn discover(dir: &Path) -> Option<RepoInfo> {
    let repo = gix::discover(dir).ok()?;
    let main = repo.main_repo().ok()?;
    // A bare main repository has no root folder to call the project.
    let root = canonical(main.workdir()?);
    let mut worktrees = vec![Worktree {
        path: root.clone(),
        branch: branch(&main),
        head: head(&main),
    }];
    for proxy in main.worktrees().ok()? {
        let Ok(base) = proxy.base() else {
            continue;
        };
        let linked = proxy.into_repo_with_possibly_inaccessible_worktree().ok();
        worktrees.push(Worktree {
            path: canonical(&base),
            branch: linked.as_ref().and_then(branch),
            head: linked.as_ref().and_then(head),
        });
    }
    Some(RepoInfo { root, worktrees })
}

fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn branch(repo: &gix::Repository) -> Option<String> {
    repo.head_name()
        .ok()
        .flatten()
        .map(|name| name.shorten().to_string())
}

fn head(repo: &gix::Repository) -> Option<String> {
    repo.head_id().ok().map(|id| id.to_string())
}

```

Then re-export the world types at the crate root. Replace the whole content of `crates/hm-project/src/lib.rs` with:

```rust
//! Turns a raw inventory into a snapshot: which project and worktree each process
//! and container belongs to, what each program is (signatures), which services
//! exist and who owns every listening port.

pub mod git;
pub mod paths;
pub mod world;

pub use world::{FixtureWorld, RepoInfo, Scenario, World};
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p hm-project`
Expected: PASS — `test result: ok. 7 passed`

- [ ] **Step 5: Commit**

```bash
git add crates/hm-project/src/lib.rs crates/hm-project/src/paths.rs crates/hm-project/src/world.rs crates/hm-project/src/git.rs
git commit -s -m "feat(project): add path rules, recorded worlds and git discovery"
```

### Task 11: Manifests and signature matching

To tell Vite from a bare Node process, the matcher needs facts about each process: its executable, arguments, ancestors, the files in its working folder and the dependencies declared in the nearest manifest. This task adds the read-only manifest reader (`package.json`, `pyproject.toml`, `requirements*.txt`, `*.csproj`/`*.fsproj`, `go.mod`) and the signature matcher (v0.1 spec §8), and checks that every fixture in the embedded packs matches its own signature.

**Files:**
- Create: `crates/hm-project/src/manifest.rs`
- Create: `crates/hm-project/src/matcher.rs`
- Modify: `crates/hm-project/src/lib.rs` (add `pub mod manifest;` and `pub mod matcher;` after `pub mod git;`)
- Test: `crates/hm-project/src/manifest.rs`, `crates/hm-project/src/matcher.rs` (unit tests in the same files), `crates/hm-project/tests/pack_fixtures.rs`

**Interfaces:**
- Consumes: `hm_project::world::{World, FixtureWorld}` and `hm_project::paths::within` (Task 10); `hm_core::names::basename(path: &str) -> &str` and `hm_core::names::exe_stem(path: &str) -> String` (Task 2); `hm_core::model::Confidence` and `hm_core::signature::{Fixture, Signature, SignatureSet, When}` (Task 4); `hm_doctor::packs::load_embedded() -> Packs` (Task 9; dev-dependency, tests only).
- Produces:
  - `hm_project::manifest`: `pub struct Manifest { pub name: Option<String>, pub packages: BTreeSet<String> }`, `pub fn read_manifest(world: &dyn World, dir: &Path, stop: &Path) -> Manifest`
  - `hm_project::matcher`: `pub struct ProcFacts { pub exe: String, pub name: String, pub argv: Vec<String>, pub ancestors: Vec<String>, pub cwd_files: Vec<String>, pub packages: BTreeSet<String>, pub os: String }` (`Default`) with `pub fn from_fixture(fixture: &Fixture, os: &str) -> Self`; `pub struct Match<'s> { pub signature: &'s Signature, pub confidence: Confidence }` (`Copy`); `pub fn match_process<'s>(set: &'s SignatureSet, facts: &ProcFacts) -> Option<Match<'s>>`; `pub fn match_image<'s>(set: &'s SignatureSet, image: &str, os: &str) -> Option<Match<'s>>`; `pub fn image_name(image: &str) -> String`; `pub(crate) fn script_stem(arg: &str) -> String` (used by Task 12)

- [ ] **Step 1: Write the failing test**

Declare the two modules, create each file with only its tests, and add the pack test, which runs every `[[signature.fixture]]` of the embedded packs through the matcher and checks that OS-specific signatures stay on their OS.

Replace the whole content of `crates/hm-project/src/lib.rs` with:

```rust
//! Turns a raw inventory into a snapshot: which project and worktree each process
//! and container belongs to, what each program is (signatures), which services
//! exist and who owns every listening port.

pub mod git;
pub mod manifest;
pub mod matcher;
pub mod paths;
pub mod world;

pub use world::{FixtureWorld, RepoInfo, Scenario, World};
```

Create `crates/hm-project/src/manifest.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::FixtureWorld;
    use std::path::PathBuf;

    fn world(files: &[(&str, &str)]) -> FixtureWorld {
        FixtureWorld {
            files: files
                .iter()
                .map(|(path, text)| (PathBuf::from(path), text.to_string()))
                .collect(),
            ..FixtureWorld::default()
        }
    }

    #[test]
    fn nearest_package_json_gives_name_and_dependencies() {
        let world = world(&[
            (
                "/w/shop/package.json",
                r#"{"name":"shop","devDependencies":{"turbo":"2"}}"#,
            ),
            (
                "/w/shop/apps/web/package.json",
                r#"{"name":"@shop/web","dependencies":{"vite":"6","React":"19"}}"#,
            ),
        ]);
        let manifest = read_manifest(&world, Path::new("/w/shop/apps/web"), Path::new("/w/shop"));
        assert_eq!(manifest.name.as_deref(), Some("web"));
        assert_eq!(
            manifest.packages.into_iter().collect::<Vec<_>>(),
            vec!["react", "vite"]
        );
    }

    #[test]
    fn python_manifests_are_normalized() {
        let world = world(&[
            (
                "/w/api/pyproject.toml",
                "[project]\nname = \"shop-api\"\ndependencies = [\"fastapi[standard]>=0.115\", \"SQLAlchemy==2.0\"]\n",
            ),
            (
                "/w/api/requirements-dev.txt",
                "# tools\npytest_asyncio==0.24\n-r requirements.txt\n",
            ),
        ]);
        let manifest = read_manifest(&world, Path::new("/w/api"), Path::new("/w/api"));
        assert_eq!(manifest.name.as_deref(), Some("shop-api"));
        assert!(manifest.packages.contains("fastapi"));
        assert!(manifest.packages.contains("sqlalchemy"));
        assert!(manifest.packages.contains("pytest-asyncio"));
    }

    #[test]
    fn dotnet_and_go_projects_are_named_after_their_project_files() {
        let world = world(&[
            (
                "/w/api/Shop.Api.csproj",
                "<Project Sdk=\"Microsoft.NET.Sdk.Web\"/>",
            ),
            (
                "/w/svc/go.mod",
                "module github.com/acme/billing\n\ngo 1.23\n",
            ),
        ]);
        let api = read_manifest(&world, Path::new("/w/api"), Path::new("/w"));
        assert_eq!(api.name.as_deref(), Some("Shop.Api"));
        let svc = read_manifest(&world, Path::new("/w/svc"), Path::new("/w"));
        assert_eq!(svc.name.as_deref(), Some("billing"));
    }

    #[test]
    fn stops_at_the_boundary() {
        let world = world(&[("/w/package.json", r#"{"name":"outside"}"#)]);
        let manifest = read_manifest(&world, Path::new("/w/shop/api"), Path::new("/w/shop"));
        assert_eq!(manifest, Manifest::default());
    }
}
```

Create `crates/hm-project/src/matcher.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_names_drop_registry_tag_and_digest() {
        assert_eq!(image_name("postgres:16-alpine"), "postgres");
        assert_eq!(image_name("docker.io/bitnami/postgresql:16"), "postgresql");
        assert_eq!(image_name("localhost:5000/team/api@sha256:abc"), "api");
        assert_eq!(image_name("Redis"), "redis");
    }

    #[test]
    fn globs_support_one_wildcard() {
        assert!(glob("python*", "python3.13"));
        assert!(glob("*.csproj", "Shop.Api.csproj"));
        assert!(glob("node", "Node"));
        assert!(!glob("node", "nodemon"));
    }

    #[test]
    fn script_stems_ignore_extensions_and_separators() {
        assert_eq!(script_stem(r"C:\app\node_modules\vite\bin\vite.js"), "vite");
        assert_eq!(script_stem("manage.py"), "manage");
        assert_eq!(script_stem("vite:dev"), "vite:dev");
    }
}
```

Create `crates/hm-project/tests/pack_fixtures.rs` with:

```rust
//! Every signature in the embedded packs ships with fixtures; each must match it.

use hm_doctor::packs::load_embedded;
use hm_project::matcher::{ProcFacts, match_image, match_process};

#[test]
fn every_signature_fixture_matches_its_own_signature() {
    let packs = load_embedded();
    let mut failures = Vec::new();
    for signature in packs.signatures.iter() {
        for (index, fixture) in signature.fixture.iter().enumerate() {
            let os = fixture
                .os
                .as_deref()
                .or(signature.os.as_deref())
                .unwrap_or("linux");
            let found = match &fixture.image {
                Some(image) => match_image(&packs.signatures, image, os),
                None => match_process(&packs.signatures, &ProcFacts::from_fixture(fixture, os)),
            };
            let got = found.map(|found| found.signature.id.as_str());
            if got != Some(signature.id.as_str()) {
                failures.push(format!(
                    "{} fixture #{} matched {got:?}",
                    signature.id,
                    index + 1
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn os_specific_signatures_stay_on_their_os() {
    let packs = load_embedded();
    let facts = ProcFacts {
        exe: "controlcenter".into(),
        name: "controlcenter".into(),
        os: "linux".into(),
        ..ProcFacts::default()
    };
    assert!(match_process(&packs.signatures, &facts).is_none());
}
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p hm-project`
Expected: FAIL — compile errors such as ``error[E0425]: cannot find function `read_manifest` in this scope`` (unit tests) and `` error[E0432]: unresolved imports `hm_project::matcher::ProcFacts`, `hm_project::matcher::match_image`, `hm_project::matcher::match_process` `` (`tests/pack_fixtures.rs`).

- [ ] **Step 3: Write the implementation**

Add this above the `#[cfg(test)]` line in `crates/hm-project/src/manifest.rs`:

```rust
//! Reads project manifests near a working directory (read-only): the nearest
//! package.json and the nearest Python manifest give dependency names; the nearest
//! manifest of any kind gives the service name.

use std::collections::BTreeSet;
use std::path::Path;

use crate::paths::within;
use crate::world::World;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    /// Name from the nearest manifest, e.g. "web" for `{"name": "@shop/web"}`.
    pub name: Option<String>,
    /// Dependency names, lowercased; Python names use `-` instead of `_`.
    pub packages: BTreeSet<String>,
}

/// Looks in `dir` and its parents up to `stop` (inclusive). When `dir` is not inside
/// `stop`, only `dir` itself is read.
pub fn read_manifest(world: &dyn World, dir: &Path, stop: &Path) -> Manifest {
    let windows = world.windows();
    let mut manifest = Manifest::default();
    let mut found_node = false;
    let mut found_python = false;
    let mut current = Some(dir);
    while let Some(here) = current {
        let names = world.file_names(here);
        let has = |file: &str| names.iter().any(|name| name == file);

        if !found_node && has("package.json") {
            found_node = true;
            if let Some(text) = world.read_to_string(&here.join("package.json")) {
                read_package_json(&text, &mut manifest);
            }
        }
        if !found_python {
            if has("pyproject.toml")
                && let Some(text) = world.read_to_string(&here.join("pyproject.toml"))
            {
                found_python = true;
                read_pyproject(&text, &mut manifest);
            }
            for requirements in names
                .iter()
                .filter(|name| name.starts_with("requirements") && name.ends_with(".txt"))
            {
                found_python = true;
                if let Some(text) = world.read_to_string(&here.join(requirements)) {
                    read_requirements(&text, &mut manifest);
                }
            }
        }
        if manifest.name.is_none() {
            if let Some(project_file) = names
                .iter()
                .find(|name| name.ends_with(".csproj") || name.ends_with(".fsproj"))
            {
                manifest.name = project_file
                    .rsplit_once('.')
                    .map(|(stem, _)| stem.to_string());
            } else if has("go.mod")
                && let Some(text) = world.read_to_string(&here.join("go.mod"))
            {
                manifest.name = go_module_name(&text);
            }
        }

        let at_stop = within(stop, here, windows);
        if (found_node && found_python && manifest.name.is_some())
            || !within(here, stop, windows)
            || at_stop
        {
            break;
        }
        current = here.parent();
    }
    manifest
}

fn read_package_json(text: &str, manifest: &mut Manifest) {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    if manifest.name.is_none() {
        manifest.name = json
            .get("name")
            .and_then(|name| name.as_str())
            .map(|name| name.rsplit('/').next().unwrap_or(name).to_string())
            .filter(|name| !name.is_empty());
    }
    for section in ["dependencies", "devDependencies"] {
        if let Some(deps) = json.get(section).and_then(|deps| deps.as_object()) {
            manifest
                .packages
                .extend(deps.keys().map(|name| name.to_lowercase()));
        }
    }
}

fn read_pyproject(text: &str, manifest: &mut Manifest) {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        return;
    };
    let project = table.get("project");
    let poetry = table.get("tool").and_then(|tool| tool.get("poetry"));
    if manifest.name.is_none() {
        manifest.name = project
            .or(poetry)
            .and_then(|section| section.get("name"))
            .and_then(|name| name.as_str())
            .map(str::to_string);
    }
    if let Some(deps) = project
        .and_then(|project| project.get("dependencies"))
        .and_then(|deps| deps.as_array())
    {
        manifest.packages.extend(
            deps.iter()
                .filter_map(|dep| dep.as_str())
                .filter_map(python_requirement_name),
        );
    }
    if let Some(deps) = poetry
        .and_then(|poetry| poetry.get("dependencies"))
        .and_then(|deps| deps.as_table())
    {
        manifest.packages.extend(
            deps.keys()
                .filter(|name| name.as_str() != "python")
                .map(|name| normalize_python(name)),
        );
    }
}

fn read_requirements(text: &str, manifest: &mut Manifest) {
    manifest.packages.extend(
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('-'))
            .filter_map(python_requirement_name),
    );
}

/// `"fastapi[standard]>=0.115"` → `fastapi`.
fn python_requirement_name(requirement: &str) -> Option<String> {
    let name: String = requirement
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    (!name.is_empty()).then(|| normalize_python(&name))
}

fn normalize_python(name: &str) -> String {
    name.to_lowercase().replace(['_', '.'], "-")
}

/// `module github.com/acme/api` → `api`.
fn go_module_name(text: &str) -> Option<String> {
    let module = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("module "))?
        .trim()
        .trim_matches('"');
    module.rsplit('/').next().map(str::to_string)
}

```

Add this above the `#[cfg(test)]` line in `crates/hm-project/src/matcher.rs`:

```rust
//! Signature matching (v0.1 spec §8). Signatures are tried in priority order and the
//! first match wins; `when` blocks are alternatives, their conditions all must hold.

use std::collections::BTreeSet;

use hm_core::model::Confidence;
use hm_core::names::{basename, exe_stem};
use hm_core::signature::{Fixture, Signature, SignatureSet, When};

/// What is known about one process when matching.
#[derive(Debug, Clone, Default)]
pub struct ProcFacts {
    /// Stem of the executable path, or of the process name when the path is unknown.
    pub exe: String,
    /// Stem of the process name.
    pub name: String,
    pub argv: Vec<String>,
    /// Executable stems of the ancestors, nearest first.
    pub ancestors: Vec<String>,
    pub cwd_files: Vec<String>,
    pub packages: BTreeSet<String>,
    /// "macos", "linux" or "windows".
    pub os: String,
}

impl ProcFacts {
    /// Facts for a signature fixture, as the pack tests use them.
    pub fn from_fixture(fixture: &Fixture, os: &str) -> Self {
        let exe = fixture.exe.as_deref().map(exe_stem).unwrap_or_default();
        ProcFacts {
            name: exe.clone(),
            exe,
            argv: fixture.argv.clone(),
            ancestors: fixture.ancestors.iter().map(|a| exe_stem(a)).collect(),
            cwd_files: fixture.cwd_files.clone(),
            packages: fixture.packages.iter().map(|p| p.to_lowercase()).collect(),
            os: os.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Match<'s> {
    pub signature: &'s Signature,
    pub confidence: Confidence,
}

pub fn match_process<'s>(set: &'s SignatureSet, facts: &ProcFacts) -> Option<Match<'s>> {
    set.iter()
        .filter(|signature| os_allows(signature, &facts.os))
        .find_map(|signature| {
            signature
                .when
                .iter()
                .filter(|when| when.image.is_empty())
                .find(|when| process_matches(when, facts))
                .map(|when| Match {
                    signature,
                    confidence: confidence(when),
                })
        })
}

pub fn match_image<'s>(set: &'s SignatureSet, image: &str, os: &str) -> Option<Match<'s>> {
    let name = image_name(image);
    set.iter()
        .filter(|signature| os_allows(signature, os))
        .find_map(|signature| {
            signature
                .when
                .iter()
                .filter(|when| !when.image.is_empty())
                .find(|when| {
                    when.image
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(&name))
                })
                .map(|when| Match {
                    signature,
                    confidence: confidence(when),
                })
        })
}

/// `docker.io/bitnami/postgresql:16@sha256:...` → `postgresql`.
pub fn image_name(image: &str) -> String {
    let without_digest = image.split('@').next().unwrap_or(image);
    let last = without_digest.rsplit('/').next().unwrap_or(without_digest);
    last.split(':').next().unwrap_or(last).to_ascii_lowercase()
}

fn os_allows(signature: &Signature, os: &str) -> bool {
    signature.os.as_deref().is_none_or(|only| only == os)
}

fn confidence(when: &When) -> Confidence {
    if when.probably {
        Confidence::Probably
    } else {
        Confidence::Sure
    }
}

fn process_matches(when: &When, facts: &ProcFacts) -> bool {
    let exe_ok = when.exe.is_empty()
        || when
            .exe
            .iter()
            .any(|pattern| glob(pattern, &facts.exe) || glob(pattern, &facts.name));
    let basename_ok = when.argv_basename.is_empty()
        || facts.argv.iter().any(|arg| {
            let base = script_stem(arg);
            when.argv_basename
                .iter()
                .any(|wanted| wanted.eq_ignore_ascii_case(&base))
        });
    let contains_ok = when.argv_contains.is_empty()
        || facts.argv.iter().any(|arg| {
            when.argv_contains
                .iter()
                .any(|needle| arg.contains(needle.as_str()))
        });
    let prefix_ok = when.argv_prefix.is_empty()
        || facts.argv.iter().any(|arg| {
            when.argv_prefix
                .iter()
                .any(|prefix| arg.starts_with(prefix.as_str()))
        });
    let ancestor_ok = when.ancestor_exe.is_empty()
        || facts.ancestors.iter().any(|ancestor| {
            when.ancestor_exe
                .iter()
                .any(|pattern| glob(pattern, ancestor))
        });
    let files_ok = when.cwd_files.is_empty()
        || facts
            .cwd_files
            .iter()
            .any(|file| when.cwd_files.iter().any(|pattern| glob(pattern, file)));
    let package_ok = when.package.is_empty()
        || when
            .package
            .iter()
            .any(|package| facts.packages.contains(&package.to_lowercase()));
    exe_ok && basename_ok && contains_ok && prefix_ok && ancestor_ok && files_ok && package_ok
}

/// Basename without a script or executable extension: `vite.js` → `vite`.
pub(crate) fn script_stem(arg: &str) -> String {
    let base = basename(arg).to_ascii_lowercase();
    for ext in [".js", ".cjs", ".mjs", ".ts", ".py", ".exe", ".cmd"] {
        if let Some(stem) = base.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    base
}

/// Case-insensitive match with one `*` wildcard at the start or the end.
fn glob(pattern: &str, value: &str) -> bool {
    let (pattern, value) = (pattern.to_ascii_lowercase(), value.to_ascii_lowercase());
    if let Some(suffix) = pattern.strip_prefix('*') {
        value.ends_with(suffix)
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        pattern == value
    }
}

```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p hm-project`
Expected: PASS — the unit tests report `test result: ok. 14 passed` and `tests/pack_fixtures.rs` reports `test result: ok. 2 passed`

- [ ] **Step 5: Commit**

```bash
git add crates/hm-project/src/lib.rs crates/hm-project/src/manifest.rs crates/hm-project/src/matcher.rs crates/hm-project/tests/pack_fixtures.rs
git commit -s -m "feat(project): read manifests and match signatures"
```

### Task 12: Snapshot resolution

This task, the heart of the plan, turns a raw inventory into the `Snapshot` that `harbormap status` shows, and tests it on `tests/fixtures/shop.json`, a recorded Mac where Harbormap starts in `/Users/me/code/shop/apps/web`. That folder is inside the git project `shop`, whose linked worktree `/Users/me/code/shop-feature` runs a second Vite on port 5174, while the main worktree runs `npm run dev → sh -c → vite` (5173), FastAPI under uvicorn with a reload child (8000), `dotnet run` (5280), a Celery worker without a port, and a compose Postgres that Docker Desktop's `com.docker.backend` forwards on port 55432. Besides the shop's own programs, the machine has a second project `blog` (Next.js on 3000), a loose Redis container, AirPlay Receiver (5000 and 7000), cupsd, Chrome, Claude Code (an AI agent started in the shop folder, listening on 41234), and port 6000, whose owner is hidden. Resolution decides membership (a process belongs to the project whose worktree contains its working folder; a container, to the project that contains its compose `working_dir`, after resolving symlinks, because compose records the folder as it was typed) and who owns each listening port (a process, a container behind an engine forwarder, the system, or unknown). Listener owners and workers inside a project become services, and so does every container; a process service spans its whole wrapper chain, so `npm run dev → sh -c → vite` is one service with root PID 800 and main PID 802. Services are named after the nearest manifest or folder for the project's own servers, databases, caches and workers (`@shop/web` → `web`, `shop-api`, `Billing.csproj` → `Billing`), after the compose service or container name for containers (`db`, `lonely-redis`), and after their signature or program name otherwise; editors, AI agents, system owners and Harbormap itself never become services.

**Files:**
- Create: `crates/hm-project/src/resolve.rs`
- Modify: `crates/hm-project/src/lib.rs` (add `pub mod resolve;` and `pub use resolve::{ResolveInput, build_snapshot};`)
- Test: `crates/hm-project/tests/resolve.rs`, `crates/hm-project/tests/fixtures/shop.json`, `crates/hm-project/tests/fixtures/windows.json`

**Interfaces:**
- Consumes:
  - Task 2: `hm_core::names::exe_stem(path: &str) -> String`, `hm_core::names::basename(path: &str) -> &str`, `hm_core::env::EnvFacts` (copied from `RawProcess::env` to `Process::env`)
  - Task 4: `hm_core::ids::project_id(canonical_root: &Path) -> String`, `hm_core::ids::service_id(project_id: Option<&str>, worktree: Option<&Path>, name: &str) -> String`; `hm_core::model::{Snapshot, Host, Project, Worktree, Service, ServiceSource, Listener, Owner, Process, Container, Compose, Note, Section, Role, Kind, Confidence, BindScope, SNAPSHOT_SCHEMA}`, `BindScope::of(addr: IpAddr) -> BindScope`, `hm_core::model::canonical_ip(addr: IpAddr) -> IpAddr` (and `PortBinding` in the tests); `hm_core::raw::{RawInventory, RawProcess, RawListener, RawContainer}`; `hm_core::signature::{Signature, SignatureSet}`
  - Task 9: `hm_doctor::packs::load_embedded() -> Packs` (tests only)
  - Task 10: `hm_project::world::World` (including `World::canonicalize(&self, path: &Path) -> PathBuf`), `hm_project::paths::{within, is_boundary}`, and in the tests `hm_project::{Scenario, RepoInfo}` and `hm_project::git::RealWorld` (`RealWorld::new()`)
  - Task 11: `hm_project::manifest::{Manifest, read_manifest}`, `hm_project::matcher::{Match, ProcFacts, match_process, match_image, script_stem}`
- Produces: `pub struct ResolveInput<'a> { pub inventory: &'a RawInventory, pub world: &'a dyn World, pub signatures: &'a SignatureSet, pub launch_dir: &'a Path }`, `pub fn build_snapshot(input: ResolveInput<'_>) -> Snapshot`, both re-exported at the crate root (`hm_project::{ResolveInput, build_snapshot}`).

- [ ] **Step 1: Write the failing test**

Create the two recordings and the tests: they pin the shop decisions above (names, signatures, process trees, sections, owners and labels), determinism under shuffled input, the home-folder boundary, hidden Linux owners, exact-address container matching, the names of programs that are not the project's own (a headless browser, its twin and an unknown tool), and, from `windows.json`, case-insensitive Windows paths and PID 4 as the Windows HTTP service.

Create `crates/hm-project/tests/fixtures/shop.json` with:

```json
{
  "launch_dir": "/Users/me/code/shop/apps/web",
  "world": {
    "home": "/Users/me",
    "repos": [
      {"root": "/Users/me/code/shop", "worktrees": [{"path": "/Users/me/code/shop", "branch": "main", "head": "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0"}, {"path": "/Users/me/code/shop-feature", "branch": "feature/login", "head": "b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1"}]},
      {"root": "/Users/me/code/blog", "worktrees": [{"path": "/Users/me/code/blog", "branch": "main", "head": "c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2"}]}
    ],
    "files": {
      "/Users/me/code/shop/package.json": "{\"name\": \"shop\", \"private\": true, \"devDependencies\": {\"turbo\": \"2.5.0\"}}",
      "/Users/me/code/shop/compose.yaml": "services:\n  db:\n    image: postgres:16-alpine\n",
      "/Users/me/code/shop/apps/web/package.json": "{\"name\": \"@shop/web\", \"devDependencies\": {\"vite\": \"7.1.0\"}}",
      "/Users/me/code/shop/apps/api/pyproject.toml": "[project]\nname = \"shop-api\"\ndependencies = [\"fastapi>=0.115\", \"uvicorn[standard]\", \"celery>=5.4\"]\n",
      "/Users/me/code/shop/apps/api/main.py": "",
      "/Users/me/code/shop/services/billing/Billing.csproj": "<Project Sdk=\"Microsoft.NET.Sdk.Web\"/>",
      "/Users/me/code/shop/services/billing/Program.cs": "",
      "/Users/me/code/blog/package.json": "{\"name\": \"blog\", \"dependencies\": {\"next\": \"15.5.4\", \"react\": \"19.1.0\"}}"
    }
  },
  "inventory": {
    "taken_at": 1760500000,
    "os": "macos",
    "arch": "aarch64",
    "self_pid": 9000,
    "current_uid": "501",
    "processes": [
      {"pid": 1, "ppid": null, "start_time": 1760400000, "name": "launchd", "argv": ["/sbin/launchd"], "rss_bytes": 52428800, "exe": "/sbin/launchd", "cwd": "/", "uid": "0", "user": "root"},
      {"pid": 412, "ppid": 1, "start_time": 1760490000, "name": "cupsd", "argv": ["cupsd"], "rss_bytes": 52428800},
      {"pid": 500, "ppid": 1, "start_time": 1760490000, "name": "zsh", "argv": ["-zsh"], "rss_bytes": 52428800, "exe": "/bin/zsh", "cwd": "/Users/me/code/shop", "uid": "501", "user": "me"},
      {"pid": 640, "ppid": 1, "start_time": 1760490000, "name": "ControlCenter", "argv": ["/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter"], "rss_bytes": 52428800, "exe": "/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter", "cwd": "/", "uid": "501", "user": "me"},
      {"pid": 700, "ppid": 1, "start_time": 1760490000, "name": "com.docker.backend", "argv": ["/Applications/Docker.app/Contents/MacOS/com.docker.backend", "services"], "rss_bytes": 52428800, "exe": "/Applications/Docker.app/Contents/MacOS/com.docker.backend", "cwd": "/", "uid": "501", "user": "me"},
      {"pid": 800, "ppid": 500, "start_time": 1760490000, "name": "npm", "argv": ["npm run dev"], "rss_bytes": 52428800, "exe": "/opt/homebrew/bin/node", "cwd": "/Users/me/code/shop/apps/web", "uid": "501", "user": "me"},
      {"pid": 801, "ppid": 800, "start_time": 1760490000, "name": "sh", "argv": ["sh", "-c", "vite"], "rss_bytes": 52428800, "exe": "/bin/sh", "cwd": "/Users/me/code/shop/apps/web", "uid": "501", "user": "me"},
      {"pid": 802, "ppid": 801, "start_time": 1760492600, "name": "node", "argv": ["node", "/Users/me/code/shop/apps/web/node_modules/.bin/vite"], "rss_bytes": 52428800, "exe": "/opt/homebrew/bin/node", "cwd": "/Users/me/code/shop/apps/web", "uid": "501", "user": "me", "env": {"names": ["PATH", "PORT", "VITE_API_URL"], "ports": [{"name": "PORT", "port": 5173}], "urls": [{"name": "VITE_API_URL", "scheme": "http", "host": "localhost", "port": 8000}]}},
      {"pid": 900, "ppid": 500, "start_time": 1760490000, "name": "python3.13", "argv": ["/Users/me/code/shop/apps/api/.venv/bin/python3", "-m", "uvicorn", "main:app", "--reload", "--port", "8000"], "rss_bytes": 52428800, "exe": "/Users/me/code/shop/apps/api/.venv/bin/python3.13", "cwd": "/Users/me/code/shop/apps/api", "uid": "501", "user": "me"},
      {"pid": 901, "ppid": 900, "start_time": 1760490000, "name": "python3.13", "argv": ["/Users/me/code/shop/apps/api/.venv/bin/python3", "-c", "from multiprocessing.spawn import spawn_main; spawn_main(tracker_fd=5, pipe_handle=7)", "--multiprocessing-fork"], "rss_bytes": 52428800, "exe": "/Users/me/code/shop/apps/api/.venv/bin/python3.13", "cwd": "/Users/me/code/shop/apps/api", "uid": "501", "user": "me"},
      {"pid": 950, "ppid": 500, "start_time": 1760490000, "name": "dotnet", "argv": ["dotnet", "run"], "rss_bytes": 52428800, "exe": "/usr/local/share/dotnet/dotnet", "cwd": "/Users/me/code/shop/services/billing", "uid": "501", "user": "me"},
      {"pid": 951, "ppid": 950, "start_time": 1760490000, "name": "Billing", "argv": ["/Users/me/code/shop/services/billing/bin/Debug/net8.0/Billing"], "rss_bytes": 52428800, "exe": "/Users/me/code/shop/services/billing/bin/Debug/net8.0/Billing", "cwd": "/Users/me/code/shop/services/billing", "uid": "501", "user": "me"},
      {"pid": 970, "ppid": 500, "start_time": 1760490000, "name": "python3.13", "argv": ["/Users/me/code/shop/apps/api/.venv/bin/python3", "/Users/me/code/shop/apps/api/.venv/bin/celery", "-A", "shop", "worker"], "rss_bytes": 52428800, "exe": "/Users/me/code/shop/apps/api/.venv/bin/python3.13", "cwd": "/Users/me/code/shop/apps/api", "uid": "501", "user": "me"},
      {"pid": 1100, "ppid": 1, "start_time": 1760490000, "name": "zsh", "argv": ["-zsh"], "rss_bytes": 52428800, "exe": "/bin/zsh", "cwd": "/Users/me/code/blog", "uid": "501", "user": "me"},
      {"pid": 1200, "ppid": 1100, "start_time": 1760490000, "name": "node", "argv": ["next-server (v15.5.4)"], "rss_bytes": 52428800, "exe": "/opt/homebrew/bin/node", "cwd": "/Users/me/code/blog", "uid": "501", "user": "me"},
      {"pid": 1250, "ppid": 1, "start_time": 1760490000, "name": "zsh", "argv": ["-zsh"], "rss_bytes": 52428800, "exe": "/bin/zsh", "cwd": "/Users/me/code/shop-feature", "uid": "501", "user": "me"},
      {"pid": 1300, "ppid": 1250, "start_time": 1760490000, "name": "node", "argv": ["node", "/Users/me/code/shop-feature/apps/web/node_modules/.bin/vite", "--port", "5174"], "rss_bytes": 52428800, "exe": "/opt/homebrew/bin/node", "cwd": "/Users/me/code/shop-feature/apps/web", "uid": "501", "user": "me"},
      {"pid": 3000, "ppid": 500, "start_time": 1760490000, "name": "claude", "argv": ["claude"], "rss_bytes": 52428800, "exe": "/Users/me/.local/bin/claude", "cwd": "/Users/me/code/shop", "uid": "501", "user": "me"},
      {"pid": 9000, "ppid": 500, "start_time": 1760490000, "name": "harbormap", "argv": ["harbormap", "status"], "rss_bytes": 52428800, "exe": "/usr/local/bin/harbormap", "cwd": "/Users/me/code/shop/apps/web", "uid": "501", "user": "me"},
      {"pid": 76915, "ppid": 1, "start_time": 1760490000, "name": "Google Chrome", "argv": ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"], "rss_bytes": 52428800, "exe": "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "cwd": "/", "uid": "501", "user": "me"}
    ],
    "listeners": [
      {"addr": "::1", "port": 5173, "pids": [802]},
      {"addr": "127.0.0.1", "port": 5174, "pids": [1300]},
      {"addr": "127.0.0.1", "port": 8000, "pids": [900, 901]},
      {"addr": "127.0.0.1", "port": 5280, "pids": [951]},
      {"addr": "::", "port": 3000, "pids": [1200]},
      {"addr": "::", "port": 5000, "pids": [640]},
      {"addr": "::", "port": 7000, "pids": [640]},
      {"addr": "::", "port": 55432, "pids": [700]},
      {"addr": "::", "port": 6380, "pids": [700]},
      {"addr": "::1", "port": 631, "pids": [412]},
      {"addr": "127.0.0.1", "port": 9820, "pids": [76915]},
      {"addr": "127.0.0.1", "port": 41234, "pids": [3000]},
      {"addr": "0.0.0.0", "port": 6000, "pids": []}
    ],
    "containers": [
      {"engine": "e_1", "id": "4f1c2a9e8b7d6c5b4a39281706f5e4d3c2b1a09f8e7d6c5b4a3928170f6e5d4c", "name": "shop-db-1", "image": "postgres:16-alpine", "state": "running", "health": "healthy", "exit_code": 0, "restart_count": 0, "started_at": "2026-10-15T07:00:03Z", "ports": [{"container_port": 5432, "proto": "tcp", "host_ip": "0.0.0.0", "host_port": 55432}, {"container_port": 5432, "proto": "tcp", "host_ip": "::", "host_port": 55432}], "networks": ["shop_default"], "mounts": [{"kind": "volume", "name": "shop_pgdata", "source": null, "destination": "/var/lib/postgresql/data"}], "labels": {"com.docker.compose.project": "shop", "com.docker.compose.service": "db", "com.docker.compose.project.working_dir": "/Users/me/code/shop", "com.docker.compose.project.config_files": "/Users/me/code/shop/compose.yaml", "com.docker.compose.config-hash": "8c3e1f0a", "com.docker.compose.container-number": "1", "com.docker.compose.oneoff": "False"}},
      {"engine": "e_1", "id": "9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b", "name": "lonely-redis", "image": "redis:7", "state": "running", "exit_code": 0, "restart_count": 0, "started_at": "2026-10-15T06:00:00Z", "ports": [{"container_port": 6379, "proto": "tcp", "host_ip": "0.0.0.0", "host_port": 6380}], "networks": ["bridge"], "mounts": [], "labels": {}}
    ],
    "engines": [
      {"id": "e_1", "name": "Docker Desktop", "endpoint": "unix:///Users/me/.docker/run/docker.sock", "version": "29.2.1", "reachable": true, "error": null}
    ],
    "notes": []
  }
}
```

Create `crates/hm-project/tests/fixtures/windows.json` with:

```json
{
  "launch_dir": "C:\\work\\shop",
  "world": {
    "home": "C:\\Users\\me",
    "windows": true,
    "repos": [
      {
        "root": "C:\\work\\shop",
        "worktrees": [
          {
            "path": "C:\\work\\shop",
            "branch": "main",
            "head": null
          }
        ]
      }
    ],
    "files": {}
  },
  "inventory": {
    "taken_at": 1760500000,
    "os": "windows",
    "arch": "x86_64",
    "self_pid": 7000,
    "current_uid": "S-1-5-21-1-1001",
    "processes": [
      {
        "pid": 4,
        "ppid": 0,
        "start_time": 1760400000,
        "name": "System",
        "argv": [
          "System"
        ]
      },
      {
        "pid": 5100,
        "ppid": 5000,
        "start_time": 1760499000,
        "name": "node.exe",
        "exe": "C:\\Program Files\\nodejs\\node.exe",
        "argv": [
          "C:\\Program Files\\nodejs\\node.exe",
          "c:\\Work\\Shop\\web\\node_modules\\vite\\bin\\vite.js"
        ],
        "cwd": "c:\\Work\\Shop\\web\\",
        "uid": "S-1-5-21-1-1001",
        "user": "me"
      },
      {
        "pid": 5000,
        "ppid": 1,
        "start_time": 1760498000,
        "name": "pwsh.exe",
        "exe": "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
        "argv": [
          "pwsh.exe"
        ],
        "cwd": "C:\\work\\shop\\",
        "uid": "S-1-5-21-1-1001",
        "user": "me"
      }
    ],
    "listeners": [
      {
        "addr": "0.0.0.0",
        "port": 80,
        "pids": [
          4
        ]
      },
      {
        "addr": "127.0.0.1",
        "port": 5173,
        "pids": [
          5100
        ]
      }
    ],
    "containers": [],
    "engines": [],
    "notes": []
  }
}
```

Create `crates/hm-project/tests/resolve.rs` with:

```rust
//! Resolution against the recorded "shop" machine: one opened project with a linked
//! worktree, another project, Docker Desktop, macOS system owners, an AI agent and a
//! hidden owner.

use std::path::{Path, PathBuf};

use hm_core::model::{Owner, Section, ServiceSource, Snapshot};
use hm_core::raw::{RawListener, RawProcess};
use hm_doctor::packs::load_embedded;
use hm_project::{ResolveInput, Scenario, build_snapshot};

fn scenario() -> Scenario {
    serde_json::from_str(include_str!("fixtures/shop.json")).unwrap()
}

fn resolve(scenario: &Scenario) -> Snapshot {
    let packs = load_embedded();
    build_snapshot(ResolveInput {
        inventory: &scenario.inventory,
        world: &scenario.world,
        signatures: &packs.signatures,
        launch_dir: &scenario.launch_dir,
    })
}

fn service<'s>(snapshot: &'s Snapshot, name: &str, worktree: &str) -> &'s hm_core::model::Service {
    snapshot
        .services
        .iter()
        .find(|service| {
            service.name == name && service.worktree.as_deref() == Some(Path::new(worktree))
        })
        .unwrap_or_else(|| panic!("no service {name} in {worktree}"))
}

fn listener(snapshot: &Snapshot, port: u16) -> &hm_core::model::Listener {
    snapshot
        .listeners
        .iter()
        .find(|listener| listener.port == port)
        .unwrap_or_else(|| panic!("no listener on {port}"))
}

const SHOP: &str = "/Users/me/code/shop";
const FEATURE: &str = "/Users/me/code/shop-feature";

#[test]
fn the_launch_folder_opens_its_git_project_with_all_worktrees() {
    let snapshot = resolve(&scenario());
    let opened = snapshot.projects.iter().find(|p| p.opened).unwrap();
    assert_eq!(opened.root, PathBuf::from(SHOP));
    assert_eq!(opened.name, "shop");
    assert!(opened.git);
    assert_eq!(opened.worktrees.len(), 2);
    assert_eq!(snapshot.opened.as_deref(), Some(opened.id.as_str()));
}

#[test]
fn project_services_get_names_frameworks_and_process_trees() {
    let snapshot = resolve(&scenario());

    let web = service(&snapshot, "web", SHOP);
    assert_eq!(web.signature.as_deref(), Some("vite"));
    assert_eq!(web.section, Section::Project);
    assert_eq!(web.ports, vec![5173]);
    assert_eq!(web.urls, vec!["http://localhost:5173"]);
    assert_eq!(
        web.source,
        ServiceSource::Process {
            main_pid: 802,
            root_pid: 800,
            pids: vec![800, 801, 802]
        }
    );

    let api = service(&snapshot, "shop-api", SHOP);
    assert_eq!(api.signature.as_deref(), Some("fastapi"));
    assert_eq!(api.ports, vec![8000]);
    assert!(
        matches!(api.source, ServiceSource::Process { main_pid: 900, root_pid: 900, ref pids } if pids == &vec![900, 901])
    );

    let billing = service(&snapshot, "Billing", SHOP);
    assert_eq!(billing.signature.as_deref(), Some("dotnet"));
    assert!(matches!(
        billing.source,
        ServiceSource::Process {
            main_pid: 951,
            root_pid: 950,
            ..
        }
    ));

    let worker = service(&snapshot, "shop-api-celery", SHOP);
    assert_eq!(worker.signature.as_deref(), Some("celery"));
    assert!(worker.ports.is_empty());

    let db = service(&snapshot, "db", SHOP);
    assert_eq!(db.signature.as_deref(), Some("postgres"));
    assert_eq!(db.ports, vec![55432]);
    assert_eq!(db.health.as_deref(), Some("healthy"));
    assert_eq!(db.section, Section::Project);

    let feature_web = service(&snapshot, "web", FEATURE);
    assert_eq!(feature_web.ports, vec![5174]);
    assert_ne!(feature_web.id, web.id);
}

#[test]
fn other_projects_and_loose_containers_are_elsewhere() {
    let snapshot = resolve(&scenario());
    let blog = service(&snapshot, "blog", "/Users/me/code/blog");
    assert_eq!(blog.section, Section::Elsewhere);
    assert_eq!(blog.signature.as_deref(), Some("next"));
    let redis = snapshot
        .services
        .iter()
        .find(|s| s.name == "lonely-redis")
        .unwrap();
    assert_eq!(redis.section, Section::Elsewhere);
    assert_eq!(redis.project_id, None);
    assert_eq!(redis.signature.as_deref(), Some("redis"));
}

#[test]
fn every_listener_has_an_owner_and_a_section() {
    let snapshot = resolve(&scenario());

    assert_eq!(
        listener(&snapshot, 5000).owner,
        Owner::System {
            reason: "AirPlay Receiver".into(),
            pid: Some(640)
        }
    );
    assert_eq!(listener(&snapshot, 5000).section, Section::System);

    let db_port = listener(&snapshot, 55432);
    assert!(
        matches!(db_port.owner, Owner::ContainerForwarder { ref container_id } if container_id.starts_with("4f1c2a9e"))
    );
    assert_eq!(db_port.label, "db (PostgreSQL)");
    assert_eq!(db_port.section, Section::Project);

    let redis_port = listener(&snapshot, 6380);
    assert_eq!(redis_port.label, "lonely-redis (Redis)");

    assert_eq!(listener(&snapshot, 631).label, "cupsd (pid 412)");
    assert_eq!(listener(&snapshot, 9820).label, "Google Chrome (pid 76915)");

    let agent = listener(&snapshot, 41234);
    assert_eq!(agent.label, "Claude Code (pid 3000)");
    assert_eq!(agent.section, Section::System);
    assert_eq!(agent.service_id, None);

    assert!(matches!(
        listener(&snapshot, 6000).owner,
        Owner::Unknown { .. }
    ));
    assert!(
        snapshot
            .notes
            .iter()
            .any(|note| note.code == "owners_hidden")
    );
}

#[test]
fn harbormap_itself_is_never_a_service() {
    let snapshot = resolve(&scenario());
    assert!(
        snapshot
            .services
            .iter()
            .all(|service| match &service.source {
                ServiceSource::Process { pids, .. } => !pids.contains(&9000),
                ServiceSource::Container { .. } => true,
            })
    );
}

#[test]
fn resolution_is_deterministic_regardless_of_input_order() {
    let original = scenario();
    let mut shuffled = original.clone();
    shuffled.inventory.processes.reverse();
    shuffled.inventory.listeners.reverse();
    shuffled.inventory.containers.reverse();
    let a = serde_json::to_string(&resolve(&original)).unwrap();
    let b = serde_json::to_string(&resolve(&shuffled)).unwrap();
    assert_eq!(a, b);
}

#[test]
fn launching_from_the_home_folder_opens_no_project() {
    let mut scenario = scenario();
    scenario.launch_dir = PathBuf::from("/Users/me");
    let snapshot = resolve(&scenario);
    assert_eq!(snapshot.opened, None);
    assert!(
        snapshot
            .services
            .iter()
            .all(|s| s.section == Section::Elsewhere)
    );
    assert!(
        snapshot
            .notes
            .iter()
            .any(|note| note.code == "not_in_project")
    );
}

#[test]
fn a_git_repository_at_home_is_ignored() {
    let mut scenario = scenario();
    // Some people keep their whole home folder in git.
    scenario.world.repos.push(hm_project::RepoInfo {
        root: PathBuf::from("/Users/me"),
        worktrees: vec![hm_core::model::Worktree {
            path: PathBuf::from("/Users/me"),
            branch: Some("main".into()),
            head: None,
        }],
    });
    scenario.launch_dir = PathBuf::from("/Users/me/Desktop/newapp");
    let snapshot = resolve(&scenario);
    let opened = snapshot.projects.iter().find(|p| p.opened).unwrap();
    assert_eq!(opened.root, PathBuf::from("/Users/me/Desktop/newapp"));
    assert!(!opened.git);
}

#[test]
fn hidden_linux_owners_map_to_containers_by_published_port() {
    let mut scenario = scenario();
    scenario.inventory.os = "linux".into();
    // With docker-proxy running as root the owner PID is not visible.
    scenario
        .inventory
        .listeners
        .retain(|listener| listener.port != 55432);
    scenario.inventory.listeners.push(RawListener {
        proto: Default::default(),
        addr: "0.0.0.0".parse().unwrap(),
        port: 55432,
        pids: vec![],
        uid: Some(0),
    });
    let snapshot = resolve(&scenario);
    assert!(matches!(
        listener(&snapshot, 55432).owner,
        Owner::ContainerForwarder { .. }
    ));
    assert!(matches!(
        listener(&snapshot, 6000).owner,
        Owner::Unknown { .. }
    ));
}

#[test]
fn windows_paths_match_without_case_and_pid_4_is_the_http_service() {
    let scenario: Scenario = serde_json::from_str(include_str!("fixtures/windows.json")).unwrap();
    let snapshot = resolve(&scenario);
    assert_eq!(
        listener(&snapshot, 80).owner,
        Owner::System {
            reason: "Windows HTTP service".into(),
            pid: Some(4)
        }
    );
    let web = listener(&snapshot, 5173);
    assert_eq!(web.section, Section::Project);
    assert_eq!(web.label, "web (Vite)");
}

#[test]
fn a_hidden_owner_maps_to_the_container_bound_to_its_exact_address() {
    let mut scenario = scenario();
    let mut first = scenario.inventory.containers[1].clone();
    first.id = "aaaa".into();
    first.name = "on-loopback".into();
    first.ports = vec![hm_core::model::PortBinding {
        container_port: 80,
        proto: "tcp".into(),
        host_ip: Some("127.0.0.1".parse().unwrap()),
        host_port: Some(8080),
    }];
    let mut second = first.clone();
    second.id = "bbbb".into();
    second.name = "on-lan".into();
    second.ports[0].host_ip = Some("192.168.1.5".parse().unwrap());
    scenario.inventory.containers.extend([second, first]);
    scenario.inventory.listeners.push(RawListener {
        proto: Default::default(),
        addr: "127.0.0.1".parse().unwrap(),
        port: 8080,
        pids: vec![],
        uid: Some(0),
    });
    let snapshot = resolve(&scenario);
    assert_eq!(
        listener(&snapshot, 8080).owner,
        Owner::ContainerForwarder {
            container_id: "aaaa".into()
        }
    );
}

#[test]
fn programs_that_are_not_the_projects_own_are_named_after_what_they_are() {
    let mut scenario = scenario();
    let chrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
    // A test browser an agent left running inside the project, and an unknown tool.
    scenario.inventory.processes.push(RawProcess {
        pid: 4000,
        ppid: Some(1),
        start_time: 1760495000,
        name: "Google Chrome".into(),
        exe: Some(chrome.into()),
        argv: vec![
            chrome.into(),
            "--headless=new".into(),
            "--remote-debugging-port=9877".into(),
        ],
        cwd: Some("/Users/me/code/shop/apps/web".into()),
        uid: Some("501".into()),
        user: Some("me".into()),
        ..RawProcess::default()
    });
    scenario.inventory.processes.push(RawProcess {
        pid: 4100,
        ppid: Some(500),
        start_time: 1760495000,
        name: "mystery".into(),
        exe: Some("/Users/me/code/shop/tools/mystery".into()),
        argv: vec!["./tools/mystery".into()],
        cwd: Some("/Users/me/code/shop".into()),
        uid: Some("501".into()),
        user: Some("me".into()),
        ..RawProcess::default()
    });
    let mut twin = scenario.inventory.processes[scenario.inventory.processes.len() - 2].clone();
    twin.pid = 4001;
    twin.argv[2] = "--remote-debugging-port=9878".into();
    scenario.inventory.processes.push(twin);
    for (port, pid) in [(9877, 4000), (9878, 4001), (7777, 4100)] {
        scenario.inventory.listeners.push(RawListener {
            proto: Default::default(),
            addr: "127.0.0.1".parse().unwrap(),
            port,
            pids: vec![pid],
            uid: None,
        });
    }
    let snapshot = resolve(&scenario);
    assert_eq!(listener(&snapshot, 9877).label, "Headless browser");
    assert_eq!(listener(&snapshot, 9877).section, Section::Project);
    assert_eq!(
        listener(&snapshot, 9878).label,
        "headless-browser (2) (Headless browser)"
    );
    assert_eq!(listener(&snapshot, 7777).label, "mystery");
}

#[cfg(unix)]
#[test]
fn a_compose_folder_reached_through_a_symlink_stays_in_its_project() {
    use hm_core::raw::RawInventory;
    use hm_project::git::RealWorld;

    // Compose records `working_dir` as it was typed, symlinks included (on macOS, /tmp
    // is a symlink to /private/tmp), while project roots are canonical.
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("shop");
    std::fs::create_dir(&real).unwrap();
    let link = temp.path().join("shop-link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let inventory: RawInventory = serde_json::from_value(serde_json::json!({
        "os": std::env::consts::OS,
        "containers": [{
            "engine": "e_1",
            "id": "c0ffee",
            "name": "shop-db-1",
            "image": "postgres:16-alpine",
            "state": "running",
            "labels": {
                "com.docker.compose.project": "shop",
                "com.docker.compose.service": "db",
                "com.docker.compose.project.working_dir": link,
            },
        }],
    }))
    .unwrap();
    let packs = load_embedded();
    let snapshot = build_snapshot(ResolveInput {
        inventory: &inventory,
        world: &RealWorld::new(),
        signatures: &packs.signatures,
        launch_dir: &real,
    });
    let db = snapshot
        .services
        .iter()
        .find(|service| service.name == "db")
        .unwrap();
    assert_eq!(db.section, Section::Project);
    assert!(snapshot.opened.is_some());
    assert_eq!(db.project_id, snapshot.opened);
}
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p hm-project --test resolve`
Expected: FAIL — compile error such as `` error[E0432]: unresolved imports `hm_project::ResolveInput`, `hm_project::build_snapshot` ``.

- [ ] **Step 3: Write the implementation**

Create `crates/hm-project/src/resolve.rs`. Its module comment lists the four resolution steps: `Resolver::open` is step 1; `place_processes`, `match_processes` and `place_containers` are step 2; `owner` is step 3; `services` and `assemble` are step 4. `assemble` also makes names unique per project and worktree (a different program that wants a taken name gets its signature as a suffix, as in `shop-api-celery`; another copy of the same program gets a number, as in `headless-browser (2)`), adds URLs for HTTP signatures, labels every listener, keeps only the processes of services and of listener owners (plus each owner's parent), and adds the `not_in_project` and `owners_hidden` notes.

```rust
//! Raw inventory + world → snapshot (v0.1 spec §7 and §8).
//!
//! 1. The opened project is the git root of the launch folder (or the folder itself).
//! 2. A process belongs to a project when its working folder is inside one of the
//!    project's worktrees; a process whose folder is unreadable inherits from its
//!    nearest ancestor. A container belongs to the project that contains its compose
//!    `working_dir`.
//! 3. Each listening socket gets an owner: a process, a container (through an engine
//!    forwarder), the system, or "unknown" when the owner is not visible.
//! 4. Listener owners inside projects become services, grouped by the top of their
//!    wrapper chain (`npm run dev` → `sh -c` → `vite`). Background workers become
//!    services too. Editors and AI agents never do.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use hm_core::ids::{project_id, service_id};
use hm_core::model::{
    BindScope, Compose, Confidence, Container, Kind, Listener, Note, Owner, Process, Project, Role,
    SNAPSHOT_SCHEMA, Section, Service, ServiceSource, Snapshot, Worktree, canonical_ip,
};
use hm_core::names::exe_stem;
use hm_core::raw::{RawContainer, RawInventory, RawListener, RawProcess};
use hm_core::signature::{Signature, SignatureSet};

use crate::manifest::{Manifest, read_manifest};
use crate::matcher::{Match, ProcFacts, match_image, match_process, script_stem};
use crate::paths::{is_boundary, within};
use crate::world::World;

pub struct ResolveInput<'a> {
    pub inventory: &'a RawInventory,
    pub world: &'a dyn World,
    pub signatures: &'a SignatureSet,
    /// The folder Harbormap was started in.
    pub launch_dir: &'a Path,
}

/// Programs that only start another program: the service is the whole chain.
const WRAPPER_EXES: &[&str] = &[
    "npm", "npx", "pnpm", "pnpx", "yarn", "bun", "bunx", "uv", "uvx", "poetry", "pipenv", "pdm",
    "hatch", "rye", "dotnet", "go", "cargo", "air", "nodemon",
];
/// Node scripts that act as wrappers when run as `node <script>`.
const WRAPPER_SCRIPTS: &[&str] = &[
    "npm-cli",
    "npx-cli",
    "yarn",
    "pnpm",
    "nodemon",
    "tsx",
    "ts-node-dev",
    "corepack",
];
/// Shells count as wrappers only when they run a single command (`sh -c ...`).
const SHELLS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "fish",
    "cmd",
    "powershell",
    "pwsh",
];
const MAVEN_LAUNCHER: &str = "org.codehaus.plexus.classworlds";
/// Ends every "unknown owner" reason that root or admin rights would reveal.
const NEEDS_ADMIN: &str = "seeing it needs admin rights";
const COMPOSE: &str = "com.docker.compose.";

pub fn build_snapshot(input: ResolveInput<'_>) -> Snapshot {
    Resolver::new(input).run()
}

struct ProjectCtx {
    id: String,
    name: String,
    root: PathBuf,
    git: bool,
    opened: bool,
    worktrees: Vec<Worktree>,
}

impl ProjectCtx {
    fn new(root: PathBuf, git: bool, opened: bool, worktrees: Vec<Worktree>) -> Self {
        let name = folder_name(&root);
        ProjectCtx {
            id: project_id(&root),
            name,
            root,
            git,
            opened,
            worktrees,
        }
    }
}

/// Where something lives: project index plus the worktree path that contains it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Place {
    project: usize,
    worktree: PathBuf,
}

struct Resolver<'a> {
    inv: &'a RawInventory,
    world: &'a dyn World,
    signatures: &'a SignatureSet,
    windows: bool,
    home: Option<PathBuf>,
    projects: Vec<ProjectCtx>,
    procs: BTreeMap<u32, &'a RawProcess>,
    children: BTreeMap<u32, Vec<u32>>,
    places: HashMap<u32, Place>,
    matches: HashMap<u32, Match<'a>>,
    manifests: HashMap<(PathBuf, PathBuf), Manifest>,
}

/// A service before naming and deduplication.
struct Draft<'a> {
    place: Option<Place>,
    signature: Option<Match<'a>>,
    source: DraftSource,
    ports: BTreeSet<u16>,
    base_name: String,
    state: String,
    health: Option<String>,
}

enum DraftSource {
    Process {
        main: u32,
        root: u32,
        pids: Vec<u32>,
    },
    Container {
        id: String,
    },
}

impl<'a> Resolver<'a> {
    fn new(input: ResolveInput<'a>) -> Self {
        let world = input.world;
        let windows = world.windows() || input.inventory.os == "windows";
        let mut procs = BTreeMap::new();
        let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for process in &input.inventory.processes {
            procs.insert(process.pid, process);
            if let Some(ppid) = process.ppid
                && ppid != process.pid
            {
                children.entry(ppid).or_default().push(process.pid);
            }
        }
        let mut resolver = Resolver {
            inv: input.inventory,
            world,
            signatures: input.signatures,
            windows,
            home: world.home(),
            projects: Vec::new(),
            procs,
            children,
            places: HashMap::new(),
            matches: HashMap::new(),
            manifests: HashMap::new(),
        };
        resolver.open(input.launch_dir);
        resolver
    }

    /// Step 1: the opened project.
    fn open(&mut self, launch_dir: &Path) {
        let dir = self.world.canonicalize(launch_dir);
        if let Some(repo) = self.world.repo_for(&dir)
            && !is_boundary(&repo.root, self.home.as_deref(), self.windows)
        {
            self.projects
                .push(ProjectCtx::new(repo.root, true, true, repo.worktrees));
        } else if !is_boundary(&dir, self.home.as_deref(), self.windows) {
            self.projects.push(ProjectCtx::new(
                dir.clone(),
                false,
                true,
                vec![Worktree {
                    path: dir,
                    branch: None,
                    head: None,
                }],
            ));
        }
    }

    fn run(mut self) -> Snapshot {
        self.place_processes();
        self.match_processes();
        let containers = self.place_containers();
        let inv = self.inv;
        let listeners: Vec<(&RawListener, Owner)> = inv
            .listeners
            .iter()
            .map(|listener| (listener, self.owner(listener, &containers)))
            .collect();
        let services = self.services(&listeners, &containers);
        self.assemble(listeners, containers, services)
    }

    /// Step 2a: project membership by working folder.
    fn place_processes(&mut self) {
        let pids: Vec<u32> = self.procs.keys().copied().collect();
        for pid in &pids {
            if *pid == self.inv.self_pid {
                continue;
            }
            let cwd = self.procs[pid].cwd.clone();
            if let Some(cwd) = cwd
                && let Some(place) = self.locate(&cwd)
            {
                self.places.insert(*pid, place);
            }
        }
        // Unreadable working folders inherit from the nearest placed ancestor.
        for pid in &pids {
            if *pid == self.inv.self_pid || self.procs[pid].cwd.is_some() {
                continue;
            }
            if let Some(place) = self
                .ancestors(*pid)
                .into_iter()
                .take(8)
                .find_map(|ancestor| self.places.get(&ancestor).cloned())
            {
                self.places.insert(*pid, place);
            }
        }
    }

    /// Step 2b: signatures for listener owners and for everything inside projects.
    fn match_processes(&mut self) {
        let mut wanted: BTreeSet<u32> = self.places.keys().copied().collect();
        wanted.extend(
            self.inv
                .listeners
                .iter()
                .flat_map(|l| l.pids.iter().copied()),
        );
        for pid in wanted {
            let Some(process) = self.procs.get(&pid).copied() else {
                continue;
            };
            let facts = self.facts(process);
            if let Some(found) = match_process(self.signatures, &facts) {
                self.matches.insert(pid, found);
            }
        }
        // A dev program outside git but under the home folder gets a folder project.
        let owners: BTreeSet<u32> = self
            .inv
            .listeners
            .iter()
            .flat_map(|l| l.pids.iter().copied())
            .collect();
        for pid in owners {
            if self.places.contains_key(&pid) || pid == self.inv.self_pid {
                continue;
            }
            let (Some(process), Some(found)) =
                (self.procs.get(&pid).copied(), self.matches.get(&pid))
            else {
                continue;
            };
            let dev = matches!(
                found.signature.kind,
                Kind::DevServer | Kind::Database | Kind::Cache | Kind::Worker
            );
            let own = process.uid.is_some() && process.uid == self.inv.current_uid;
            if let (true, true, Some(cwd)) = (dev, own, process.cwd.clone())
                && self.under_home(&cwd)
            {
                let place = self.folder_project(&cwd);
                self.places.insert(pid, place);
            }
        }
    }

    fn facts(&mut self, process: &RawProcess) -> ProcFacts {
        let exe = process
            .exe
            .as_ref()
            .map(|exe| exe_stem(&exe.to_string_lossy()))
            .unwrap_or_else(|| exe_stem(&process.name));
        let ancestors = self
            .ancestors(process.pid)
            .into_iter()
            .take(16)
            .filter_map(|pid| self.procs.get(&pid))
            .map(|ancestor| stem_of(ancestor))
            .collect();
        let place = self.places.get(&process.pid).cloned();
        let readable_cwd = process
            .cwd
            .clone()
            .filter(|cwd| place.is_some() || self.under_home(cwd));
        let (cwd_files, packages) = match readable_cwd {
            Some(cwd) => {
                let stop = place.map_or_else(|| cwd.clone(), |place| place.worktree);
                let packages = self.manifest(&cwd, &stop).packages;
                (self.world.file_names(&cwd), packages)
            }
            None => (Vec::new(), BTreeSet::new()),
        };
        ProcFacts {
            exe,
            name: exe_stem(&process.name),
            argv: process.argv.clone(),
            ancestors,
            cwd_files,
            packages,
            os: self.inv.os.clone(),
        }
    }

    /// Step 2c: containers by compose working folder.
    fn place_containers(&mut self) -> Vec<(&'a RawContainer, Option<Place>, Option<Match<'a>>)> {
        let inv = self.inv;
        let mut placed = Vec::with_capacity(inv.containers.len());
        for container in &inv.containers {
            let working_dir = container
                .labels
                .get(&format!("{COMPOSE}project.working_dir"))
                // Compose keeps the folder as it was typed, symlinks included (on macOS,
                // /tmp is /private/tmp), while project roots are canonical.
                .map(|dir| self.world.canonicalize(Path::new(dir)));
            let place = match working_dir {
                Some(dir) => match self.locate(&dir) {
                    Some(place) => Some(place),
                    None if !is_boundary(&dir, self.home.as_deref(), self.windows) => {
                        Some(self.folder_project(&dir))
                    }
                    None => None,
                },
                None => None,
            };
            let found = match_image(self.signatures, &container.image, &inv.os);
            placed.push((container, place, found));
        }
        placed
    }

    /// Step 3: who holds a listening socket.
    fn owner(
        &self,
        listener: &RawListener,
        containers: &[(&RawContainer, Option<Place>, Option<Match<'_>>)],
    ) -> Owner {
        let forwarded = || container_for_port(listener, containers);
        if listener.pids.is_empty() {
            return match forwarded() {
                Some(container_id) => Owner::ContainerForwarder { container_id },
                None => Owner::Unknown {
                    reason: match listener.uid {
                        Some(0) => format!("it belongs to the system (root); {NEEDS_ADMIN}"),
                        Some(uid) => {
                            format!("it belongs to another user (uid {uid}); {NEEDS_ADMIN}")
                        }
                        None => NEEDS_ADMIN.to_string(),
                    },
                },
            };
        }
        if listener.pids == [0] {
            return Owner::System {
                reason: "the operating system kernel".into(),
                pid: None,
            };
        }
        let Some(main) = self.main_pid(&listener.pids) else {
            return Owner::Unknown {
                reason: "the program ended before Harbormap could look at it".into(),
            };
        };
        match self.matches.get(&main).map(|found| found.signature) {
            Some(signature) if signature.forwarder => match forwarded() {
                Some(container_id) => Owner::ContainerForwarder { container_id },
                None => Owner::System {
                    reason: format!("{} (no container publishes this port)", signature.name),
                    pid: Some(main),
                },
            },
            Some(signature) if signature.kind == Kind::System => Owner::System {
                reason: signature.name.clone(),
                pid: Some(main),
            },
            _ => Owner::Process { pid: main },
        }
    }

    /// The topmost visible process among those sharing a socket.
    fn main_pid(&self, pids: &[u32]) -> Option<u32> {
        let visible: Vec<u32> = pids
            .iter()
            .copied()
            .filter(|pid| self.procs.contains_key(pid))
            .collect();
        visible
            .iter()
            .copied()
            .filter(|pid| {
                self.procs[pid]
                    .ppid
                    .is_none_or(|ppid| !visible.contains(&ppid))
            })
            .min()
            .or_else(|| visible.iter().copied().min())
    }

    /// Step 4: services.
    fn services(
        &mut self,
        listeners: &[(&RawListener, Owner)],
        containers: &[(&'a RawContainer, Option<Place>, Option<Match<'a>>)],
    ) -> Vec<Draft<'a>> {
        let mut by_root: BTreeMap<u32, Draft<'a>> = BTreeMap::new();
        let mut owner_ports: BTreeMap<u32, BTreeSet<u16>> = BTreeMap::new();
        for (listener, owner) in listeners {
            if let Owner::Process { pid } = owner {
                owner_ports.entry(*pid).or_default().insert(listener.port);
            }
        }
        for (pid, ports) in owner_ports {
            if !self.can_be_service(pid) {
                continue;
            }
            let root = self.service_root(pid);
            let draft = by_root.entry(root).or_insert_with(|| Draft {
                place: self.places.get(&pid).cloned(),
                signature: self.matches.get(&pid).copied(),
                source: DraftSource::Process {
                    main: pid,
                    root,
                    pids: Vec::new(),
                },
                ports: BTreeSet::new(),
                base_name: String::new(),
                state: "running".into(),
                health: None,
            });
            draft.ports.extend(ports);
        }
        // Background workers have no ports but are services too.
        let workers: Vec<u32> = self
            .places
            .keys()
            .copied()
            .filter(|pid| {
                self.matches
                    .get(pid)
                    .is_some_and(|found| found.signature.kind == Kind::Worker)
            })
            .collect();
        let mut covered: BTreeSet<u32> = BTreeSet::new();
        for root in by_root.keys() {
            covered.extend(self.descendants(*root));
        }
        let mut workers = workers;
        workers.sort_unstable();
        for pid in workers {
            if covered.contains(&pid) {
                continue;
            }
            let root = self.service_root(pid);
            covered.extend(self.descendants(root));
            by_root.entry(root).or_insert_with(|| Draft {
                place: self.places.get(&pid).cloned(),
                signature: self.matches.get(&pid).copied(),
                source: DraftSource::Process {
                    main: pid,
                    root,
                    pids: Vec::new(),
                },
                ports: BTreeSet::new(),
                base_name: String::new(),
                state: "running".into(),
                health: None,
            });
        }

        let mut drafts: Vec<Draft<'a>> = by_root.into_values().collect();
        for draft in &mut drafts {
            if let DraftSource::Process { main, root, pids } = &mut draft.source {
                *pids = self.descendants(*root);
                let (main, place) = (*main, draft.place.clone());
                draft.base_name = self.process_service_name(main, place.as_ref(), draft.signature);
            }
        }
        for (container, place, found) in containers {
            drafts.push(Draft {
                place: place.clone(),
                signature: *found,
                source: DraftSource::Container {
                    id: container.id.clone(),
                },
                ports: container
                    .ports
                    .iter()
                    .filter_map(|binding| binding.host_port)
                    .collect(),
                base_name: container_service_name(container),
                state: container.state.clone(),
                health: container.health.clone(),
            });
        }
        drafts
    }

    fn can_be_service(&self, pid: u32) -> bool {
        if pid == self.inv.self_pid || !self.places.contains_key(&pid) {
            return false;
        }
        !self.matches.get(&pid).is_some_and(|found| {
            matches!(
                found.signature.kind,
                Kind::Ide | Kind::Terminal | Kind::AgentCli | Kind::System
            )
        })
    }

    /// Walks up through wrappers and same-signature parents inside the same project.
    fn service_root(&self, pid: u32) -> u32 {
        let mut current = pid;
        for _ in 0..16 {
            let Some(parent) = self.procs[&current]
                .ppid
                .and_then(|ppid| self.procs.get(&ppid).copied())
            else {
                break;
            };
            let same_project = self.places.get(&parent.pid).map(|place| place.project)
                == self.places.get(&current).map(|place| place.project);
            let same_signature = match (self.matches.get(&parent.pid), self.matches.get(&current)) {
                (Some(a), Some(b)) => a.signature.id == b.signature.id,
                _ => false,
            };
            if parent.pid == self.inv.self_pid
                || !same_project
                || !(is_wrapper(parent) || same_signature)
            {
                break;
            }
            current = parent.pid;
        }
        current
    }

    /// The project's own programs (servers, databases, workers) are named after their
    /// manifest or folder. Anything else is named after what it is: its signature
    /// (`headless-browser`) or, when unrecognised, its program name (`google-chrome`).
    fn process_service_name(
        &mut self,
        main: u32,
        place: Option<&Place>,
        found: Option<Match<'_>>,
    ) -> String {
        let process = self.procs[&main];
        let own_program = found.is_some_and(|found| {
            matches!(
                found.signature.kind,
                Kind::DevServer | Kind::Database | Kind::Cache | Kind::Worker
            )
        });
        if own_program && let (Some(cwd), Some(place)) = (process.cwd.clone(), place) {
            if let Some(name) = self.manifest(&cwd, &place.worktree).name {
                return name;
            }
            if !within(&place.worktree, &cwd, self.windows) {
                return folder_name(&cwd);
            }
        }
        match found {
            Some(found) => found.signature.id.clone(),
            None => slug(&stem_of(process)),
        }
    }

    fn assemble(
        self,
        listeners: Vec<(&RawListener, Owner)>,
        containers: Vec<(&RawContainer, Option<Place>, Option<Match<'_>>)>,
        drafts: Vec<Draft<'_>>,
    ) -> Snapshot {
        let section_of = |place: Option<&Place>| match place {
            Some(place) if self.projects[place.project].opened => Section::Project,
            _ => Section::Elsewhere,
        };

        // Names are unique within one project and worktree. Services with ports pick
        // first. A different program that wants a taken name gets its signature as a
        // suffix ("shop-api-celery"); another copy of the same program gets "(2)".
        let mut order: Vec<usize> = (0..drafts.len()).collect();
        order.sort_by(|&a, &b| draft_key(&drafts[a]).cmp(&draft_key(&drafts[b])));
        type Scope = (Option<usize>, Option<PathBuf>, String);
        let mut taken: BTreeMap<Scope, Option<&str>> = BTreeMap::new();
        let mut names = vec![String::new(); drafts.len()];
        for index in order {
            let draft = &drafts[index];
            let project = draft.place.as_ref().map(|place| place.project);
            let worktree = draft.place.as_ref().map(|place| place.worktree.clone());
            let key = |name: &str| (project, worktree.clone(), name.to_string());
            let signature = draft.signature.map(|found| found.signature.id.as_str());
            let base = &draft.base_name;
            let mut candidates = vec![base.clone()];
            if let Some(holder) = taken.get(&key(base))
                && *holder != signature
                && let Some(id) = signature
            {
                candidates.push(format!("{base}-{id}"));
            }
            candidates.extend((2..66).map(|n| format!("{base} ({n})")));
            let name = candidates
                .into_iter()
                .find(|candidate| !taken.contains_key(&key(candidate)))
                .unwrap_or_else(|| base.clone());
            taken.insert(key(&name), signature);
            names[index] = name;
        }

        let mut services: Vec<Service> = drafts
            .iter()
            .zip(names)
            .map(|(draft, name)| {
                let project = draft
                    .place
                    .as_ref()
                    .map(|place| &self.projects[place.project]);
                let signature: Option<&Signature> = draft.signature.map(|found| found.signature);
                let http = signature.is_some_and(|signature| signature.http);
                Service {
                    id: service_id(
                        project.map(|project| project.id.as_str()),
                        draft.place.as_ref().map(|place| place.worktree.as_path()),
                        &name,
                    ),
                    name,
                    section: section_of(draft.place.as_ref()),
                    project_id: project.map(|project| project.id.clone()),
                    worktree: draft.place.as_ref().map(|place| place.worktree.clone()),
                    signature: signature.map(|signature| signature.id.clone()),
                    framework: signature.map(|signature| signature.name.clone()),
                    confidence: draft
                        .signature
                        .map_or(Confidence::Sure, |found| found.confidence),
                    role: signature.map_or(Role::Unknown, |signature| signature.role),
                    kind: signature.map_or(Kind::Unknown, |signature| signature.kind),
                    source: match &draft.source {
                        DraftSource::Process { main, root, pids } => ServiceSource::Process {
                            main_pid: *main,
                            root_pid: *root,
                            pids: pids.clone(),
                        },
                        DraftSource::Container { id } => ServiceSource::Container {
                            container_id: id.clone(),
                        },
                    },
                    ports: draft.ports.iter().copied().collect(),
                    urls: if http {
                        draft
                            .ports
                            .iter()
                            .map(|port| format!("http://localhost:{port}"))
                            .collect()
                    } else {
                        Vec::new()
                    },
                    state: draft.state.clone(),
                    health: draft.health.clone(),
                }
            })
            .collect();
        services.sort_by_cached_key(|service| {
            (
                service.section,
                self.project_name(service.project_id.as_deref())
                    .to_lowercase(),
                service.name.to_lowercase(),
                service.id.clone(),
            )
        });

        let service_of_pid: HashMap<u32, &Service> = services
            .iter()
            .flat_map(|service| match &service.source {
                ServiceSource::Process { pids, .. } => {
                    pids.iter().map(|pid| (*pid, service)).collect::<Vec<_>>()
                }
                ServiceSource::Container { .. } => Vec::new(),
            })
            .collect();
        let service_of_container: HashMap<&str, &Service> = services
            .iter()
            .filter_map(|service| match &service.source {
                ServiceSource::Container { container_id } => Some((container_id.as_str(), service)),
                ServiceSource::Process { .. } => None,
            })
            .collect();

        let mut hidden_owners = 0;
        let mut out_listeners: Vec<Listener> = listeners
            .into_iter()
            .map(|(raw, owner)| {
                let service = match &owner {
                    Owner::Process { pid } => service_of_pid.get(pid).copied(),
                    Owner::ContainerForwarder { container_id } => {
                        service_of_container.get(container_id.as_str()).copied()
                    }
                    _ => None,
                };
                let label = match (&owner, service) {
                    // "web (Vite)", or just "Headless browser" when the name is the signature.
                    (_, Some(service)) => match &service.framework {
                        Some(framework)
                            if service.signature.as_deref() == Some(service.name.as_str()) =>
                        {
                            framework.clone()
                        }
                        Some(framework) => format!("{} ({framework})", service.name),
                        None => service.name.clone(),
                    },
                    (Owner::Process { pid }, None) => {
                        let what = self.matches.get(pid).map_or_else(
                            || self.procs[pid].name.clone(),
                            |found| found.signature.name.clone(),
                        );
                        format!("{what} (pid {pid})")
                    }
                    (Owner::System { reason, .. }, None) => reason.clone(),
                    (Owner::ContainerForwarder { container_id }, None) => {
                        format!("container {}", short_id(container_id))
                    }
                    (Owner::Unknown { reason }, None) => {
                        if reason.ends_with(NEEDS_ADMIN) {
                            hidden_owners += 1;
                        }
                        "unknown owner".into()
                    }
                };
                let addr = canonical_ip(raw.addr);
                Listener {
                    proto: raw.proto,
                    addr,
                    port: raw.port,
                    bind: BindScope::of(addr),
                    section: service.map_or(Section::System, |service| service.section),
                    project_id: service.and_then(|service| service.project_id.clone()),
                    service_id: service.map(|service| service.id.clone()),
                    label,
                    owner,
                }
            })
            .collect();
        out_listeners.sort_by_key(|listener| (listener.port, listener.addr));

        let mut keep: BTreeSet<u32> = service_of_pid.keys().copied().collect();
        for listener in &out_listeners {
            let owner_pid = match listener.owner {
                Owner::Process { pid } | Owner::System { pid: Some(pid), .. } => Some(pid),
                _ => None,
            };
            if let Some(pid) = owner_pid {
                keep.insert(pid);
                if let Some(ppid) = self.procs.get(&pid).and_then(|process| process.ppid) {
                    keep.insert(ppid);
                }
            }
        }
        let processes: Vec<Process> = keep
            .into_iter()
            .filter_map(|pid| self.procs.get(&pid).copied())
            .map(|raw| {
                let place = self.places.get(&raw.pid);
                let found = self.matches.get(&raw.pid);
                Process {
                    pid: raw.pid,
                    ppid: raw.ppid,
                    start_time: raw.start_time,
                    name: raw.name.clone(),
                    exe: raw.exe.clone(),
                    argv: raw.argv.clone(),
                    cwd: raw.cwd.clone(),
                    user: raw.user.clone(),
                    own: raw.uid.is_some() && raw.uid == self.inv.current_uid,
                    signature: found.map(|found| found.signature.id.clone()),
                    kind: found.map_or(Kind::Unknown, |found| found.signature.kind),
                    project_id: place.map(|place| self.projects[place.project].id.clone()),
                    worktree: place.map(|place| place.worktree.clone()),
                    rss_bytes: raw.rss_bytes,
                    uptime_s: self.inv.taken_at.saturating_sub(raw.start_time),
                    env: raw.env.clone(),
                }
            })
            .collect();

        let mut out_containers: Vec<Container> = containers
            .iter()
            .map(|(raw, place, found)| Container {
                id: raw.id.clone(),
                engine: raw.engine.clone(),
                name: raw.name.clone(),
                image: raw.image.clone(),
                state: raw.state.clone(),
                health: raw.health.clone(),
                exit_code: raw.exit_code,
                oom_killed: raw.oom_killed,
                restart_count: raw.restart_count,
                started_at: raw.started_at.clone(),
                ports: raw.ports.clone(),
                networks: raw.networks.clone(),
                mounts: raw.mounts.clone(),
                compose: compose_of(raw),
                signature: found.map(|found| found.signature.id.clone()),
                section: section_of(place.as_ref()),
                project_id: place
                    .as_ref()
                    .map(|place| self.projects[place.project].id.clone()),
            })
            .collect();
        out_containers.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));

        let used: BTreeSet<&str> = services
            .iter()
            .filter_map(|service| service.project_id.as_deref())
            .collect();
        let mut projects: Vec<Project> = self
            .projects
            .iter()
            .filter(|project| project.opened || used.contains(project.id.as_str()))
            .map(|project| Project {
                id: project.id.clone(),
                name: project.name.clone(),
                root: project.root.clone(),
                opened: project.opened,
                git: project.git,
                worktrees: project.worktrees.clone(),
            })
            .collect();
        projects.sort_by(|a, b| (!a.opened, &a.name, &a.root).cmp(&(!b.opened, &b.name, &b.root)));

        let mut engines = self.inv.engines.clone();
        engines.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));

        let mut notes = self.inv.notes.clone();
        let opened = self.projects.iter().find(|project| project.opened);
        if opened.is_none() {
            notes.push(Note {
                code: "not_in_project".into(),
                text: "This folder is not a project, so Harbormap shows the whole computer.".into(),
            });
        }
        if hidden_owners > 0 {
            let text = if hidden_owners == 1 {
                "1 listening port belongs to a program Harbormap can't see without admin rights."
                    .to_string()
            } else {
                format!(
                    "{hidden_owners} listening ports belong to programs Harbormap can't see without admin rights."
                )
            };
            notes.push(Note {
                code: "owners_hidden".into(),
                text,
            });
        }

        Snapshot {
            schema: SNAPSHOT_SCHEMA,
            taken_at: self.inv.taken_at,
            host: hm_core::model::Host {
                os: self.inv.os.clone(),
                arch: self.inv.arch.clone(),
            },
            opened: opened.map(|project| project.id.clone()),
            projects,
            services,
            listeners: out_listeners,
            processes,
            containers: out_containers,
            engines,
            notes,
        }
    }

    // ----- helpers -----

    fn locate(&mut self, dir: &Path) -> Option<Place> {
        if let Some(place) = self.find_known(dir) {
            return Some(place);
        }
        let repo = self.world.repo_for(dir)?;
        if is_boundary(&repo.root, self.home.as_deref(), self.windows) {
            return None;
        }
        self.projects
            .push(ProjectCtx::new(repo.root, true, false, repo.worktrees));
        self.find_known(dir)
    }

    fn find_known(&self, dir: &Path) -> Option<Place> {
        let mut best: Option<(usize, &Worktree)> = None;
        for (index, project) in self.projects.iter().enumerate() {
            for worktree in &project.worktrees {
                let deeper = best.is_none_or(|(_, current)| {
                    worktree.path.as_os_str().len() > current.path.as_os_str().len()
                });
                if deeper && within(dir, &worktree.path, self.windows) {
                    best = Some((index, worktree));
                }
            }
        }
        best.map(|(project, worktree)| Place {
            project,
            worktree: worktree.path.clone(),
        })
    }

    fn folder_project(&mut self, dir: &Path) -> Place {
        if let Some(place) = self.find_known(dir) {
            return place;
        }
        self.projects.push(ProjectCtx::new(
            dir.to_path_buf(),
            false,
            false,
            vec![Worktree {
                path: dir.to_path_buf(),
                branch: None,
                head: None,
            }],
        ));
        Place {
            project: self.projects.len() - 1,
            worktree: dir.to_path_buf(),
        }
    }

    fn under_home(&self, dir: &Path) -> bool {
        self.home.as_ref().is_some_and(|home| {
            within(dir, home, self.windows) && !is_boundary(dir, Some(home), self.windows)
        })
    }

    fn manifest(&mut self, dir: &Path, stop: &Path) -> Manifest {
        let key = (dir.to_path_buf(), stop.to_path_buf());
        if let Some(found) = self.manifests.get(&key) {
            return found.clone();
        }
        let manifest = read_manifest(self.world, dir, stop);
        self.manifests.insert(key, manifest.clone());
        manifest
    }

    fn ancestors(&self, pid: u32) -> Vec<u32> {
        let mut chain = Vec::new();
        let mut current = pid;
        while let Some(ppid) = self.procs.get(&current).and_then(|process| process.ppid) {
            if ppid == current || chain.contains(&ppid) || chain.len() >= 64 {
                break;
            }
            chain.push(ppid);
            current = ppid;
        }
        chain
    }

    fn descendants(&self, root: u32) -> Vec<u32> {
        let mut found = vec![root];
        let mut index = 0;
        while index < found.len() {
            if let Some(kids) = self.children.get(&found[index]) {
                for kid in kids {
                    if !found.contains(kid) {
                        found.push(*kid);
                    }
                }
            }
            index += 1;
        }
        found.sort_unstable();
        found
    }

    fn project_name(&self, id: Option<&str>) -> String {
        id.and_then(|id| self.projects.iter().find(|project| project.id == id))
            .map(|project| project.name.clone())
            .unwrap_or_default()
    }
}

/// Naming order: by base name, services with ports first, processes before
/// containers, then by PID or container ID.
fn draft_key(draft: &Draft<'_>) -> (String, bool, u8, String) {
    let portless = draft.ports.is_empty();
    match &draft.source {
        DraftSource::Process { main, .. } => {
            (draft.base_name.clone(), portless, 0, format!("{main:010}"))
        }
        DraftSource::Container { id } => (draft.base_name.clone(), portless, 1, id.clone()),
    }
}

fn stem_of(process: &RawProcess) -> String {
    process
        .exe
        .as_ref()
        .map(|exe| exe_stem(&exe.to_string_lossy()))
        .unwrap_or_else(|| exe_stem(&process.name))
}

fn is_wrapper(process: &RawProcess) -> bool {
    let stem = stem_of(process);
    let args = || process.argv.iter().skip(1);
    // npm and yarn rename themselves ("npm run dev"), so look at the name and the
    // first word of argv[0] as well as the executable.
    let title = process
        .argv
        .first()
        .and_then(|first| first.split_whitespace().next())
        .map(exe_stem)
        .unwrap_or_default();
    [
        stem.as_str(),
        exe_stem(&process.name).as_str(),
        title.as_str(),
    ]
    .iter()
    .any(|name| WRAPPER_EXES.contains(name))
        || args().any(|arg| WRAPPER_SCRIPTS.contains(&script_stem(arg).as_str()))
        || (SHELLS.contains(&stem.as_str())
            && args().any(|arg| {
                arg == "-c"
                    || arg.eq_ignore_ascii_case("/c")
                    || arg.eq_ignore_ascii_case("-command")
            }))
        || (stem == "java" && args().any(|arg| arg.contains(MAVEN_LAUNCHER)))
}

fn container_for_port(
    listener: &RawListener,
    containers: &[(&RawContainer, Option<Place>, Option<Match<'_>>)],
) -> Option<String> {
    let addr = canonical_ip(listener.addr);
    let mut best: Option<(u8, &str)> = None;
    for (container, _, _) in containers {
        if container.state != "running" {
            continue;
        }
        for binding in &container.ports {
            if binding.host_port != Some(listener.port) {
                continue;
            }
            let score = match binding.host_ip.map(canonical_ip) {
                Some(ip) if ip == addr => 2,
                Some(ip) if ip.is_unspecified() => 1,
                None => 1,
                Some(_) => continue,
            };
            if best.is_none_or(|(current, _)| score > current) {
                best = Some((score, container.id.as_str()));
            }
        }
    }
    best.map(|(_, id)| id.to_string())
}

fn container_service_name(container: &RawContainer) -> String {
    match container.labels.get(&format!("{COMPOSE}service")) {
        Some(service) => match container
            .labels
            .get(&format!("{COMPOSE}container-number"))
            .and_then(|number| number.parse::<u32>().ok())
        {
            Some(number) if number > 1 => format!("{service}-{number}"),
            _ => service.clone(),
        },
        None => container.name.clone(),
    }
}

fn compose_of(container: &RawContainer) -> Option<Compose> {
    let label = |key: &str| container.labels.get(&format!("{COMPOSE}{key}"));
    Some(Compose {
        project: label("project")?.clone(),
        service: label("service")?.clone(),
        working_dir: label("project.working_dir").map(PathBuf::from),
        config_files: label("project.config_files")
            .map(|files| files.split(',').map(PathBuf::from).collect())
            .unwrap_or_default(),
        config_hash: label("config-hash").cloned(),
        oneoff: label("oneoff").is_some_and(|value| value.eq_ignore_ascii_case("true")),
        number: label("container-number").and_then(|number| number.parse().ok()),
    })
}

/// Last folder name, for paths of any OS: `C:\\work\\shop\\` → `shop`.
fn folder_name(path: &Path) -> String {
    let text = path.to_string_lossy();
    let trimmed = text.trim_end_matches(['/', '\\']);
    let name = hm_core::names::basename(trimmed);
    if name.is_empty() {
        text.into_owned()
    } else {
        name.to_string()
    }
}

/// `Google Chrome` → `google-chrome`.
fn slug(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
}

fn short_id(id: &str) -> &str {
    &id[..id.len().min(12)]
}
```

Then replace the whole content of `crates/hm-project/src/lib.rs` with:

```rust
//! Turns a raw inventory into a snapshot: which project and worktree each process
//! and container belongs to, what each program is (signatures), which services
//! exist and who owns every listening port.

pub mod git;
pub mod manifest;
pub mod matcher;
pub mod paths;
pub mod resolve;
pub mod world;

pub use resolve::{ResolveInput, build_snapshot};
pub use world::{FixtureWorld, RepoInfo, Scenario, World};
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p hm-project --test resolve`
Expected: PASS — `test result: ok. 13 passed` (on Windows 12 passed, because the `#[cfg(unix)]` symlink test does not exist there)

Run: `cargo test -p hm-project`
Expected: PASS — the unit tests report `test result: ok. 14 passed`, `tests/pack_fixtures.rs` reports `test result: ok. 2 passed` and `tests/resolve.rs` reports `test result: ok. 13 passed` (12 on Windows)

- [ ] **Step 5: Commit**

```bash
git add crates/hm-project/src/resolve.rs crates/hm-project/src/lib.rs crates/hm-project/tests/resolve.rs crates/hm-project/tests/fixtures/shop.json crates/hm-project/tests/fixtures/windows.json
git commit -s -m "feat(project): resolve raw inventories into snapshots"
```

### Task 13: Card texts and rendering helpers

`harbormap why` explains a port or a service in plain sentences at three levels: beginner, builder and expert. This task adds the sentence templates (`packs/texts/cards.toml`, compiled into the binary like the signature packs), their loader, and the small helpers that fill templates and format durations, sizes, times and paths. Task 14 puts them together into cards.

**Files:**
- Create: `packs/texts/cards.toml`
- Create: `crates/hm-doctor/src/render.rs`
- Create: `crates/hm-doctor/src/texts.rs`
- Modify: `crates/hm-doctor/src/lib.rs` (add `pub mod render;` and `pub mod texts;` below `pub mod packs;`)
- Test: `crates/hm-doctor/src/render.rs` and `crates/hm-doctor/src/texts.rs` (unit tests in the same files)

**Interfaces:**
- Consumes: `hm_core::signature::{Level, Texts}` with `Texts::get(&self, level: Level) -> &str` (Task 4); the `serde` and `toml` crates (Task 1).
- Produces: in `hm_doctor::render`: `pub type Facts = BTreeMap<&'static str, String>`, `pub fn render_line(template: &str, facts: &Facts) -> Option<String>`, `pub fn render_lines(templates: &[String], facts: &Facts) -> Vec<String>`, `pub fn duration(secs: u64, level: Level) -> String`, `pub fn bytes(n: u64) -> String`, `pub fn utc(secs: u64) -> String`, `pub fn tilde(path: &Path, home: Option<&Path>) -> String`, `pub fn join_argv(argv: &[String]) -> String`. In `hm_doctor::texts`: `pub const CARDS_FILE: (&str, &str)` (path and embedded text), `pub struct LevelLines { pub title: Option<Texts>, pub beginner: Vec<String>, pub builder: Vec<String>, pub expert: Vec<String> }` with `pub fn get(&self, level: Level) -> &[String]`, `pub struct CardTexts { pub process: LevelLines, pub container: LevelLines, pub elsewhere: LevelLines, pub app: LevelLines, pub system: LevelLines, pub unknown: LevelLines, pub free: LevelLines, pub reach: BTreeMap<String, Texts>, pub health: BTreeMap<String, Texts>, pub guess: Option<Texts> }` (both `Debug, Clone, Default, Deserialize`), `pub fn load_embedded() -> (CardTexts, Vec<String>)`, `pub fn parse_cards(file: &str, text: &str, warnings: &mut Vec<String>) -> CardTexts`. And the file `packs/texts/cards.toml`.

- [ ] **Step 1: Write the failing test**

The tests check that a missing fact drops its sentence, that `||` falls back in order, that durations, sizes, UTC times and home paths read as the cards need, and that the embedded texts load without warnings while a broken file gives empty templates and one warning.

Declare both modules. The whole content of `crates/hm-doctor/src/lib.rs` is now:

```rust
//! Packs (signatures and card texts), the three-level card renderer and, from
//! alpha.2, the doctor rules.

pub mod packs;
pub mod render;
pub mod texts;
```

Create `crates/hm-doctor/src/render.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn facts(pairs: &[(&'static str, &str)]) -> Facts {
        pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
    }

    #[test]
    fn missing_facts_drop_the_sentence() {
        let f = facts(&[("name", "web"), ("url", "")]);
        assert_eq!(
            render_line("{name} runs.", &f).as_deref(),
            Some("web runs.")
        );
        assert_eq!(render_line("Open {url}", &f), None);
        assert_eq!(render_line("Port {port}", &f), None);
        assert_eq!(
            render_line("No facts here.", &f).as_deref(),
            Some("No facts here.")
        );
    }

    #[test]
    fn alternatives_fall_back_in_order() {
        let f = facts(&[("name", "db")]);
        assert_eq!(
            render_line("{name} is in {project}. || {name} has no project.", &f).as_deref(),
            Some("db has no project.")
        );
    }

    #[test]
    fn durations_read_naturally_per_level() {
        assert_eq!(duration(30, Level::Beginner), "less than a minute");
        assert_eq!(duration(60, Level::Beginner), "1 minute");
        assert_eq!(duration(7_380, Level::Beginner), "2 hours");
        assert_eq!(duration(7_380, Level::Builder), "2 h 3 min");
        assert_eq!(duration(7_385, Level::Expert), "2h3m5s");
        assert_eq!(duration(90_000, Level::Builder), "1 d 1 h");
    }

    #[test]
    fn formats_bytes_and_times() {
        assert_eq!(bytes(190_840_832), "182 MB");
        assert_eq!(bytes(1_503_238_553), "1.4 GB");
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(1_760_500_000), "2025-10-15T03:46:40Z");
        assert_eq!(utc(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn tilde_shortens_home_paths() {
        let home = Path::new("/home/me");
        assert_eq!(tilde(Path::new("/home/me/shop"), Some(home)), "~/shop");
        assert_eq!(tilde(Path::new("/home/me"), Some(home)), "~");
        assert_eq!(tilde(Path::new("/srv/app"), Some(home)), "/srv/app");
    }
}
```

Create `crates/hm-doctor/src/texts.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_card_texts_load_without_warnings() {
        let (cards, warnings) = load_embedded();
        assert!(warnings.is_empty(), "{warnings:#?}");
        assert!(!cards.process.beginner.is_empty());
        assert!(cards.process.title.is_some());
        assert!(cards.guess.is_some());
    }

    #[test]
    fn a_broken_file_gives_empty_templates_and_a_warning() {
        let mut warnings = Vec::new();
        let cards = parse_cards("cards.toml", "[process\nbroken", &mut warnings);
        assert!(cards.process.beginner.is_empty());
        assert_eq!(warnings.len(), 1);
    }
}
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p hm-doctor -- render texts`
Expected: FAIL — compile errors such as ``error[E0425]: cannot find function `render_line` in this scope`` and ``error[E0425]: cannot find function `load_embedded` in this scope``.

- [ ] **Step 3: Write the implementation**

Create `packs/texts/cards.toml` with exactly this content. Each card kind (`process`, `container`, `elsewhere`, `app`, `system`, `unknown`, `free`) has an optional title and one list of sentences per level; `reach`, `health` and `guess` hold the phrases that fill `{reach}`, `{health}` and `{guess}`. The template rules: a `{fact}` placeholder is filled from a fact the card collected from the snapshot, and a sentence with a missing or empty fact is dropped rather than guessed ("no fact, no claim"). `a || b` uses `b` when `a` has a missing fact; the sentence is dropped only when every alternative has one.

```toml
# Sentences for `harbormap why` cards, one list per level. Licensed CC0-1.0.
#
# {fact} placeholders are filled from what Harbormap observed. A sentence whose fact
# is missing is left out, so nothing is ever guessed. "a || b" uses b when a has a
# missing fact. {what} is the signature's own text at the same level; {reach},
# {health} and {guess} come from the tables at the end.

# A program that belongs to a project.
[process]
title.beginner = "Port {port}: {name} || {name}"
title.builder = ":{port} → {name} ({framework}) || {name} ({framework}) || {name}"
title.expert = ":{port}/tcp → {name} [{signature}] || {name} [{signature}] || {name}"
beginner = [
  "{name} is part of the project {project}.",
  "{what}",
  "{guess}",
  "Open it in your browser: {url}",
  "{reach}",
  "It has been running for {uptime}.",
]
builder = [
  "{what}",
  "{guess}",
  "Process {process}, pid {pid}, started {uptime} ago by {user}.",
  "Folder: {cwd_rel} (project {project}, branch {branch})",
  "Listening on {addr} ({reach}).",
  "Open: {url}",
  "Port settings: {env_ports}",
  "Talks to: {env_urls}",
]
expert = [
  "Signature: {signature} ({confidence}) · role {role} · kind {kind}",
  "{what}",
  "Socket: {addr} ({reach}) · owner pid {pid} ({process})",
  "Project: {project} ({project_id}) · worktree {worktree} ({branch})",
  "argv: {argv}",
  "exe: {exe}",
  "cwd: {cwd}",
  "Started {started_utc} (up {uptime}) · ppid {ppid} ({parent}) · rss {rss}",
  "Service pids: {pids}",
  "Env ports: {env_ports}",
  "Env targets: {env_urls}",
]

# A container of a project, or a container outside any project.
[container]
title.beginner = "Port {port}: {name} || {name}"
title.builder = ":{port} → {name} ({framework}) · Docker || {name} ({framework}) · Docker || {name} · Docker"
title.expert = ":{port}/tcp → container {container} [{signature}] || container {container} [{signature}] || container {container}"
beginner = [
  "{name} is part of the project {project}. It runs inside Docker. || {name} runs inside Docker.",
  "{what}",
  "{health}",
  "{reach}",
]
builder = [
  "{what}",
  "Container {container} from image {image}: {state}, {health}.",
  "Ports: {mapping}",
  "Engine: {engine}",
  "Compose: service {compose_service} from {compose_files}",
]
expert = [
  "Signature: {signature} ({confidence}) · role {role} · kind {kind}",
  "{what}",
  "Container {container_id} ({container}) · image {image} · state {state} · health {health} · restarts {restarts} · started {started_utc}",
  "Ports: {mapping}",
  "Networks: {networks}",
  "Volumes: {volumes}",
  "Compose project {compose_project}, service {compose_service}, files {compose_files}, working dir {working_dir}",
  "Engine: {engine} at {engine_endpoint}",
]

# Added to items that belong to another project.
[elsewhere]
beginner = [
  "This belongs to another project, {project} ({project_root}). Harbormap only shows it here. To manage it, open that folder and run harbormap there.",
]
builder = [
  "Read-only here: it belongs to {project} at {project_root}. Run harbormap in that folder to manage it.",
]
expert = ["Read-only: project {project_id} at {project_root}."]

# A program outside any project, such as a browser or an editor.
[app]
title.beginner = "Port {port}: {program}"
title.builder = ":{port} → {program} (pid {pid})"
title.expert = ":{port}/tcp → pid {pid} ({program})"
beginner = [
  "{program} is a program on your computer. It is not part of a project.",
  "{what}",
  "{reach}",
]
builder = [
  "{what}",
  "Program {program}, pid {pid}, user {user}; not part of a project.",
  "Executable: {exe}",
  "Listening on {addr} ({reach}).",
]
expert = [
  "Signature: {signature}",
  "{what}",
  "Socket: {addr} ({reach}) · owner pid {pid} ({program}), user {user}",
  "argv: {argv}",
  "exe: {exe}",
  "cwd: {cwd}",
  "Started {started_utc} · ppid {ppid} ({parent})",
]

# The operating system or a container engine holds the port.
[system]
title.beginner = "Port {port}: {reason}"
title.builder = ":{port} → {reason}"
title.expert = ":{port}/tcp → {reason}"
beginner = ["{reason} holds this port. It is part of your computer, not of your project.", "{what}"]
builder = ["{what}", "Held by {reason} (pid {pid}, {program})."]
expert = ["Socket: {addr} ({reach}) · system owner: {reason}", "{what}", "pid {pid} ({program}) · exe {exe}"]

# The owner is not visible to Harbormap.
[unknown]
title.beginner = "Port {port}: a program Harbormap can't see"
title.builder = ":{port} → unknown owner"
title.expert = ":{port}/tcp → unknown owner"
beginner = [
  "Something is using this port, but Harbormap can't see which program: {reason}.",
]
builder = ["Owner not visible: {reason}."]
expert = ["Socket: {addr} ({reach}) · owner unknown: {reason}"]

# Nothing listens on the port.
[free]
title.beginner = "Port {port} is free"
title.builder = ":{port} → free"
title.expert = ":{port}/tcp → no listener"
beginner = ["Nothing is using port {port} right now."]
builder = [":{port} is free."]
expert = [":{port}/tcp has no listener in this snapshot."]

[reach.loopback]
beginner = "Only this computer can open it."
builder = "this computer only"
expert = "loopback"

[reach.all]
beginner = "Other devices on your network can open it too."
builder = "every network interface"
expert = "all interfaces"

[reach.specific]
beginner = "It is reachable only through one network address of this computer."
builder = "one network interface"
expert = "specific interface"

[health.healthy]
beginner = "Docker says it is healthy."
builder = "healthy"
expert = "healthy"

[health.unhealthy]
beginner = "Docker says it is not healthy."
builder = "unhealthy"
expert = "unhealthy"

[health.starting]
beginner = "Docker says it is still starting."
builder = "starting"
expert = "starting"

[guess]
beginner = "Harbormap is guessing this from the project's files."
builder = "Guessed from the project's dependencies (probably)."
expert = "Match confidence: probably (inferred from dependencies)."
```

Add this above the `#[cfg(test)]` line in `crates/hm-doctor/src/render.rs`, with one blank line between them. `fill` stops at the first missing or empty fact, so `?` drops the whole alternative; `civil_from_days` turns Unix days into a calendar date without a time crate:

```rust
//! Template filling and the small formatters cards use.

use std::collections::BTreeMap;
use std::path::Path;

use hm_core::signature::Level;

/// Facts for one card; a missing or empty fact leaves its sentence out.
pub type Facts = BTreeMap<&'static str, String>;

/// Fills `{fact}` placeholders. `"a || b"` uses `b` when `a` has a missing fact.
/// Returns `None` when every alternative has a missing fact.
pub fn render_line(template: &str, facts: &Facts) -> Option<String> {
    template
        .split(" || ")
        .find_map(|alternative| fill(alternative, facts))
}

pub fn render_lines(templates: &[String], facts: &Facts) -> Vec<String> {
    templates
        .iter()
        .filter_map(|template| render_line(template, facts))
        .collect()
}

fn fill(template: &str, facts: &Facts) -> Option<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('}')?;
        let value = facts.get(&after[..end]).filter(|value| !value.is_empty())?;
        out.push_str(value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// "less than a minute" / "12 minutes" (beginner), "12 min" (builder), "12m5s" (expert).
pub fn duration(secs: u64, level: Level) -> String {
    let (days, hours, minutes, seconds) = (
        secs / 86_400,
        secs % 86_400 / 3_600,
        secs % 3_600 / 60,
        secs % 60,
    );
    match level {
        Level::Beginner => {
            let plural =
                |n: u64, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
            if secs < 60 {
                "less than a minute".into()
            } else if secs < 3_600 {
                plural(minutes, "minute")
            } else if secs < 86_400 {
                plural(hours, "hour")
            } else {
                plural(days, "day")
            }
        }
        Level::Builder => {
            if secs < 60 {
                format!("{seconds} s")
            } else if secs < 3_600 {
                format!("{minutes} min")
            } else if secs < 86_400 {
                format!("{hours} h {minutes} min")
            } else {
                format!("{days} d {hours} h")
            }
        }
        Level::Expert => {
            if days > 0 {
                format!("{days}d{hours}h{minutes}m{seconds}s")
            } else if hours > 0 {
                format!("{hours}h{minutes}m{seconds}s")
            } else if minutes > 0 {
                format!("{minutes}m{seconds}s")
            } else {
                format!("{seconds}s")
            }
        }
    }
}

/// Binary units: "512 KB", "182 MB", "1.4 GB".
pub fn bytes(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if n >= GB {
        format!("{:.1} GB", n as f64 / GB as f64)
    } else if n >= MB {
        format!("{} MB", n / MB)
    } else if n >= KB {
        format!("{} KB", n / KB)
    } else {
        format!("{n} B")
    }
}

/// Unix seconds → `2026-10-15T07:12:03Z`.
pub fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 → (year, month, day), after Howard Hinnant's algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe as i64 + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Replaces the home directory prefix with `~`.
pub fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Joins argv for display, quoting arguments that contain spaces.
pub fn join_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| {
            if arg.contains(' ') {
                format!("\"{arg}\"")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
```

Add this above the `#[cfg(test)]` line in `crates/hm-doctor/src/texts.rs`, with one blank line between them. `include_str!` embeds the TOML at compile time (the path is relative to `texts.rs`); a parse error or a missing `reach` or `health` entry becomes a warning, never a crash:

```rust
//! Card texts (`packs/texts/cards.toml`): sentence templates per card kind and level.

use std::collections::BTreeMap;

use serde::Deserialize;

use hm_core::signature::{Level, Texts};

pub const CARDS_FILE: (&str, &str) = (
    "texts/cards.toml",
    include_str!("../../../packs/texts/cards.toml"),
);

/// Templates for one card kind: an optional title and sentences per level.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelLines {
    #[serde(default)]
    pub title: Option<Texts>,
    pub beginner: Vec<String>,
    pub builder: Vec<String>,
    pub expert: Vec<String>,
}

impl LevelLines {
    pub fn get(&self, level: Level) -> &[String] {
        match level {
            Level::Beginner => &self.beginner,
            Level::Builder => &self.builder,
            Level::Expert => &self.expert,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardTexts {
    pub process: LevelLines,
    pub container: LevelLines,
    pub elsewhere: LevelLines,
    pub app: LevelLines,
    pub system: LevelLines,
    pub unknown: LevelLines,
    pub free: LevelLines,
    /// Keyed by bind scope: `loopback`, `all`, `specific`.
    pub reach: BTreeMap<String, Texts>,
    /// Keyed by container health: `healthy`, `unhealthy`, `starting`.
    pub health: BTreeMap<String, Texts>,
    /// Said when a signature matched only "probably".
    pub guess: Option<Texts>,
}

/// Parses the embedded card texts. Problems become warnings and empty templates.
pub fn load_embedded() -> (CardTexts, Vec<String>) {
    let mut warnings = Vec::new();
    let cards = parse_cards(CARDS_FILE.0, CARDS_FILE.1, &mut warnings);
    (cards, warnings)
}

pub fn parse_cards(file: &str, text: &str, warnings: &mut Vec<String>) -> CardTexts {
    match toml::from_str::<CardTexts>(text) {
        Ok(cards) => {
            for key in ["loopback", "all", "specific"] {
                if !cards.reach.contains_key(key) {
                    warnings.push(format!("{file}: [reach.{key}] is missing"));
                }
            }
            for key in ["healthy", "unhealthy", "starting"] {
                if !cards.health.contains_key(key) {
                    warnings.push(format!("{file}: [health.{key}] is missing"));
                }
            }
            cards
        }
        Err(error) => {
            warnings.push(format!("{file}: {error}"));
            CardTexts::default()
        }
    }
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p hm-doctor -- render texts`
Expected: PASS — `test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out` (the three `packs` tests from Task 9 are filtered out).

- [ ] **Step 5: Commit**

```bash
git add packs/texts/cards.toml crates/hm-doctor/src/lib.rs crates/hm-doctor/src/render.rs crates/hm-doctor/src/texts.rs
git commit -s -m "feat(doctor): card text templates and rendering helpers"
```

### Task 14: Cards

A card is a title plus a few sentences that explain one owner of a port, or one service, at the chosen level. This task collects the facts for a card from the snapshot (process, folder, branch, ports, container, engine) and renders the templates from Task 13. One card is made per distinct owner of a port, so the IPv4 and IPv6 sockets of one program share a card, while two programs that split a port get one card each.

**Files:**
- Create: `crates/hm-doctor/tests/fixtures/shop-snapshot.json`
- Create: `crates/hm-doctor/tests/cards.rs`
- Create: `crates/hm-doctor/src/cards.rs`
- Modify: `crates/hm-doctor/src/lib.rs` (add `pub mod cards;` above `pub mod packs;`)
- Test: `crates/hm-doctor/tests/cards.rs`

**Interfaces:**
- Consumes: `hm_core::model::{Snapshot, Service, ServiceSource, Listener, Owner, Process, Container, Confidence, Section, BindScope, Proto}` (Task 4); `hm_core::signature::{Level, SignatureSet}` with `SignatureSet::get(&self, id: &str) -> Option<&Signature>` and `Texts::get(&self, level: Level) -> &str` (Task 4); `hm_doctor::packs::{load_embedded, Packs}` (Task 9, in the tests); `hm_doctor::render::{Facts, render_line, render_lines, duration, bytes, utc, tilde, join_argv}` and `hm_doctor::texts::{CardTexts, LevelLines, load_embedded}` (Task 13); `serde_json` (Task 1).
- Produces: `pub struct Card { pub title: String, pub lines: Vec<String> }` (`Debug, Clone, PartialEq, Eq, Serialize`), `pub struct CardContext<'a> { pub snapshot: &'a Snapshot, pub signatures: &'a SignatureSet, pub texts: &'a CardTexts, pub level: Level, pub home: Option<&'a Path> }`, `pub fn port_cards(ctx: &CardContext<'_>, port: u16) -> Vec<Card>`, `pub fn service_card(ctx: &CardContext<'_>, service: &Service) -> Card`.

- [ ] **Step 1: Write the failing test**

The tests read a saved snapshot of the recorded "shop" machine. It is the snapshot hm-project resolves from `crates/hm-project/tests/fixtures/shop.json` (Task 12), saved as JSON so the card tests need no resolver: the opened project `shop` with a linked worktree and a Compose Postgres, the other project `blog`, a container outside any project, AirPlay, Chrome, Claude Code and an owner hidden behind admin rights.

Create `crates/hm-doctor/tests/fixtures/shop-snapshot.json` with:

```json
{
  "schema": 1,
  "taken_at": 1760500000,
  "host": {"os": "macos", "arch": "aarch64"},
  "opened": "p_1a812fe5ce48",
  "projects": [
    {"id": "p_1a812fe5ce48", "name": "shop", "root": "/Users/me/code/shop", "opened": true, "git": true, "worktrees": [{"path": "/Users/me/code/shop", "branch": "main", "head": "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0"}, {"path": "/Users/me/code/shop-feature", "branch": "feature/login", "head": "b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1"}]},
    {"id": "p_a491c21bbcd9", "name": "blog", "root": "/Users/me/code/blog", "opened": false, "git": true, "worktrees": [{"path": "/Users/me/code/blog", "branch": "main", "head": "c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2"}]}
  ],
  "services": [
    {"id": "s_22ccab3c72e3", "name": "Billing", "section": "project", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "signature": "dotnet", "framework": "ASP.NET Core", "confidence": "sure", "role": "work", "kind": "dev_server", "source": {"type": "process", "main_pid": 951, "root_pid": 950, "pids": [950, 951]}, "ports": [5280], "urls": ["http://localhost:5280"], "state": "running", "health": null},
    {"id": "s_4419dc4ed284", "name": "db", "section": "project", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "signature": "postgres", "framework": "PostgreSQL", "confidence": "sure", "role": "keep", "kind": "database", "source": {"type": "container", "container_id": "4f1c2a9e8b7d6c5b4a39281706f5e4d3c2b1a09f8e7d6c5b4a3928170f6e5d4c"}, "ports": [55432], "urls": [], "state": "running", "health": "healthy"},
    {"id": "s_5fc9a9c5f444", "name": "shop-api", "section": "project", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "signature": "fastapi", "framework": "FastAPI", "confidence": "sure", "role": "work", "kind": "dev_server", "source": {"type": "process", "main_pid": 900, "root_pid": 900, "pids": [900, 901]}, "ports": [8000], "urls": ["http://localhost:8000"], "state": "running", "health": null},
    {"id": "s_ed8b26b15b23", "name": "shop-api-celery", "section": "project", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "signature": "celery", "framework": "Celery", "confidence": "sure", "role": "work", "kind": "worker", "source": {"type": "process", "main_pid": 970, "root_pid": 970, "pids": [970]}, "ports": [], "urls": [], "state": "running", "health": null},
    {"id": "s_c7efece97dfc", "name": "web", "section": "project", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "signature": "vite", "framework": "Vite", "confidence": "sure", "role": "see", "kind": "dev_server", "source": {"type": "process", "main_pid": 802, "root_pid": 800, "pids": [800, 801, 802]}, "ports": [5173], "urls": ["http://localhost:5173"], "state": "running", "health": null},
    {"id": "s_e1120aa884ed", "name": "web", "section": "project", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop-feature", "signature": "vite", "framework": "Vite", "confidence": "sure", "role": "see", "kind": "dev_server", "source": {"type": "process", "main_pid": 1300, "root_pid": 1300, "pids": [1300]}, "ports": [5174], "urls": ["http://localhost:5174"], "state": "running", "health": null},
    {"id": "s_a09467341e6f", "name": "lonely-redis", "section": "elsewhere", "project_id": null, "worktree": null, "signature": "redis", "framework": "Redis", "confidence": "sure", "role": "keep", "kind": "cache", "source": {"type": "container", "container_id": "9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b"}, "ports": [6380], "urls": [], "state": "running", "health": null},
    {"id": "s_47728efacd8e", "name": "blog", "section": "elsewhere", "project_id": "p_a491c21bbcd9", "worktree": "/Users/me/code/blog", "signature": "next", "framework": "Next.js", "confidence": "sure", "role": "see", "kind": "dev_server", "source": {"type": "process", "main_pid": 1200, "root_pid": 1200, "pids": [1200]}, "ports": [3000], "urls": ["http://localhost:3000"], "state": "running", "health": null}
  ],
  "listeners": [
    {"proto": "tcp", "addr": "::1", "port": 631, "bind": "loopback", "owner": {"kind": "process", "pid": 412}, "section": "system", "project_id": null, "service_id": null, "label": "cupsd (pid 412)"},
    {"proto": "tcp", "addr": "::", "port": 3000, "bind": "all", "owner": {"kind": "process", "pid": 1200}, "section": "elsewhere", "project_id": "p_a491c21bbcd9", "service_id": "s_47728efacd8e", "label": "blog (Next.js)"},
    {"proto": "tcp", "addr": "::", "port": 5000, "bind": "all", "owner": {"kind": "system", "reason": "AirPlay Receiver", "pid": 640}, "section": "system", "project_id": null, "service_id": null, "label": "AirPlay Receiver"},
    {"proto": "tcp", "addr": "::1", "port": 5173, "bind": "loopback", "owner": {"kind": "process", "pid": 802}, "section": "project", "project_id": "p_1a812fe5ce48", "service_id": "s_c7efece97dfc", "label": "web (Vite)"},
    {"proto": "tcp", "addr": "127.0.0.1", "port": 5174, "bind": "loopback", "owner": {"kind": "process", "pid": 1300}, "section": "project", "project_id": "p_1a812fe5ce48", "service_id": "s_e1120aa884ed", "label": "web (Vite)"},
    {"proto": "tcp", "addr": "127.0.0.1", "port": 5280, "bind": "loopback", "owner": {"kind": "process", "pid": 951}, "section": "project", "project_id": "p_1a812fe5ce48", "service_id": "s_22ccab3c72e3", "label": "Billing (ASP.NET Core)"},
    {"proto": "tcp", "addr": "0.0.0.0", "port": 6000, "bind": "all", "owner": {"kind": "unknown", "reason": "seeing it needs admin rights"}, "section": "system", "project_id": null, "service_id": null, "label": "unknown owner"},
    {"proto": "tcp", "addr": "::", "port": 6380, "bind": "all", "owner": {"kind": "container_forwarder", "container_id": "9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b"}, "section": "elsewhere", "project_id": null, "service_id": "s_a09467341e6f", "label": "lonely-redis (Redis)"},
    {"proto": "tcp", "addr": "::", "port": 7000, "bind": "all", "owner": {"kind": "system", "reason": "AirPlay Receiver", "pid": 640}, "section": "system", "project_id": null, "service_id": null, "label": "AirPlay Receiver"},
    {"proto": "tcp", "addr": "127.0.0.1", "port": 8000, "bind": "loopback", "owner": {"kind": "process", "pid": 900}, "section": "project", "project_id": "p_1a812fe5ce48", "service_id": "s_5fc9a9c5f444", "label": "shop-api (FastAPI)"},
    {"proto": "tcp", "addr": "127.0.0.1", "port": 9820, "bind": "loopback", "owner": {"kind": "process", "pid": 76915}, "section": "system", "project_id": null, "service_id": null, "label": "Google Chrome (pid 76915)"},
    {"proto": "tcp", "addr": "127.0.0.1", "port": 41234, "bind": "loopback", "owner": {"kind": "process", "pid": 3000}, "section": "system", "project_id": null, "service_id": null, "label": "Claude Code (pid 3000)"},
    {"proto": "tcp", "addr": "::", "port": 55432, "bind": "all", "owner": {"kind": "container_forwarder", "container_id": "4f1c2a9e8b7d6c5b4a39281706f5e4d3c2b1a09f8e7d6c5b4a3928170f6e5d4c"}, "section": "project", "project_id": "p_1a812fe5ce48", "service_id": "s_4419dc4ed284", "label": "db (PostgreSQL)"}
  ],
  "processes": [
    {"pid": 1, "ppid": null, "start_time": 1760400000, "name": "launchd", "exe": "/sbin/launchd", "argv": ["/sbin/launchd"], "cwd": "/", "user": "root", "own": false, "signature": null, "kind": "unknown", "project_id": null, "worktree": null, "rss_bytes": 52428800, "uptime_s": 100000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 412, "ppid": 1, "start_time": 1760490000, "name": "cupsd", "exe": null, "argv": ["cupsd"], "cwd": null, "user": null, "own": false, "signature": null, "kind": "unknown", "project_id": null, "worktree": null, "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 500, "ppid": 1, "start_time": 1760490000, "name": "zsh", "exe": "/bin/zsh", "argv": ["-zsh"], "cwd": "/Users/me/code/shop", "user": "me", "own": true, "signature": null, "kind": "unknown", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 640, "ppid": 1, "start_time": 1760490000, "name": "ControlCenter", "exe": "/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter", "argv": ["/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter"], "cwd": "/", "user": "me", "own": true, "signature": "airplay", "kind": "system", "project_id": null, "worktree": null, "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 800, "ppid": 500, "start_time": 1760490000, "name": "npm", "exe": "/opt/homebrew/bin/node", "argv": ["npm run dev"], "cwd": "/Users/me/code/shop/apps/web", "user": "me", "own": true, "signature": "node", "kind": "dev_server", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 801, "ppid": 800, "start_time": 1760490000, "name": "sh", "exe": "/bin/sh", "argv": ["sh", "-c", "vite"], "cwd": "/Users/me/code/shop/apps/web", "user": "me", "own": true, "signature": "vite", "kind": "dev_server", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 802, "ppid": 801, "start_time": 1760492600, "name": "node", "exe": "/opt/homebrew/bin/node", "argv": ["node", "/Users/me/code/shop/apps/web/node_modules/.bin/vite"], "cwd": "/Users/me/code/shop/apps/web", "user": "me", "own": true, "signature": "vite", "kind": "dev_server", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "rss_bytes": 52428800, "uptime_s": 7400, "env": {"names": ["PATH", "PORT", "VITE_API_URL"], "ports": [{"name": "PORT", "port": 5173}], "urls": [{"name": "VITE_API_URL", "scheme": "http", "host": "localhost", "port": 8000}]}},
    {"pid": 900, "ppid": 500, "start_time": 1760490000, "name": "python3.13", "exe": "/Users/me/code/shop/apps/api/.venv/bin/python3.13", "argv": ["/Users/me/code/shop/apps/api/.venv/bin/python3", "-m", "uvicorn", "main:app", "--reload", "--port", "8000"], "cwd": "/Users/me/code/shop/apps/api", "user": "me", "own": true, "signature": "fastapi", "kind": "dev_server", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 901, "ppid": 900, "start_time": 1760490000, "name": "python3.13", "exe": "/Users/me/code/shop/apps/api/.venv/bin/python3.13", "argv": ["/Users/me/code/shop/apps/api/.venv/bin/python3", "-c", "from multiprocessing.spawn import spawn_main; spawn_main(tracker_fd=5, pipe_handle=7)", "--multiprocessing-fork"], "cwd": "/Users/me/code/shop/apps/api", "user": "me", "own": true, "signature": "python", "kind": "dev_server", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 950, "ppid": 500, "start_time": 1760490000, "name": "dotnet", "exe": "/usr/local/share/dotnet/dotnet", "argv": ["dotnet", "run"], "cwd": "/Users/me/code/shop/services/billing", "user": "me", "own": true, "signature": null, "kind": "unknown", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 951, "ppid": 950, "start_time": 1760490000, "name": "Billing", "exe": "/Users/me/code/shop/services/billing/bin/Debug/net8.0/Billing", "argv": ["/Users/me/code/shop/services/billing/bin/Debug/net8.0/Billing"], "cwd": "/Users/me/code/shop/services/billing", "user": "me", "own": true, "signature": "dotnet", "kind": "dev_server", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 970, "ppid": 500, "start_time": 1760490000, "name": "python3.13", "exe": "/Users/me/code/shop/apps/api/.venv/bin/python3.13", "argv": ["/Users/me/code/shop/apps/api/.venv/bin/python3", "/Users/me/code/shop/apps/api/.venv/bin/celery", "-A", "shop", "worker"], "cwd": "/Users/me/code/shop/apps/api", "user": "me", "own": true, "signature": "celery", "kind": "worker", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 1100, "ppid": 1, "start_time": 1760490000, "name": "zsh", "exe": "/bin/zsh", "argv": ["-zsh"], "cwd": "/Users/me/code/blog", "user": "me", "own": true, "signature": null, "kind": "unknown", "project_id": "p_a491c21bbcd9", "worktree": "/Users/me/code/blog", "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 1200, "ppid": 1100, "start_time": 1760490000, "name": "node", "exe": "/opt/homebrew/bin/node", "argv": ["next-server (v15.5.4)"], "cwd": "/Users/me/code/blog", "user": "me", "own": true, "signature": "next", "kind": "dev_server", "project_id": "p_a491c21bbcd9", "worktree": "/Users/me/code/blog", "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 1250, "ppid": 1, "start_time": 1760490000, "name": "zsh", "exe": "/bin/zsh", "argv": ["-zsh"], "cwd": "/Users/me/code/shop-feature", "user": "me", "own": true, "signature": null, "kind": "unknown", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop-feature", "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 1300, "ppid": 1250, "start_time": 1760490000, "name": "node", "exe": "/opt/homebrew/bin/node", "argv": ["node", "/Users/me/code/shop-feature/apps/web/node_modules/.bin/vite", "--port", "5174"], "cwd": "/Users/me/code/shop-feature/apps/web", "user": "me", "own": true, "signature": "vite", "kind": "dev_server", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop-feature", "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 3000, "ppid": 500, "start_time": 1760490000, "name": "claude", "exe": "/Users/me/.local/bin/claude", "argv": ["claude"], "cwd": "/Users/me/code/shop", "user": "me", "own": true, "signature": "claude-code", "kind": "agent_cli", "project_id": "p_1a812fe5ce48", "worktree": "/Users/me/code/shop", "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}},
    {"pid": 76915, "ppid": 1, "start_time": 1760490000, "name": "Google Chrome", "exe": "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "argv": ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"], "cwd": "/", "user": "me", "own": true, "signature": null, "kind": "unknown", "project_id": null, "worktree": null, "rss_bytes": 52428800, "uptime_s": 10000, "env": {"names": [], "ports": [], "urls": []}}
  ],
  "containers": [
    {"id": "9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b", "engine": "e_1", "name": "lonely-redis", "image": "redis:7", "state": "running", "health": null, "exit_code": 0, "oom_killed": false, "restart_count": 0, "started_at": "2026-10-15T06:00:00Z", "ports": [{"container_port": 6379, "proto": "tcp", "host_ip": "0.0.0.0", "host_port": 6380}], "networks": ["bridge"], "mounts": [], "compose": null, "signature": "redis", "section": "elsewhere", "project_id": null},
    {"id": "4f1c2a9e8b7d6c5b4a39281706f5e4d3c2b1a09f8e7d6c5b4a3928170f6e5d4c", "engine": "e_1", "name": "shop-db-1", "image": "postgres:16-alpine", "state": "running", "health": "healthy", "exit_code": 0, "oom_killed": false, "restart_count": 0, "started_at": "2026-10-15T07:00:03Z", "ports": [{"container_port": 5432, "proto": "tcp", "host_ip": "0.0.0.0", "host_port": 55432}, {"container_port": 5432, "proto": "tcp", "host_ip": "::", "host_port": 55432}], "networks": ["shop_default"], "mounts": [{"kind": "volume", "name": "shop_pgdata", "source": null, "destination": "/var/lib/postgresql/data"}], "compose": {"project": "shop", "service": "db", "working_dir": "/Users/me/code/shop", "config_files": ["/Users/me/code/shop/compose.yaml"], "config_hash": "8c3e1f0a", "oneoff": false, "number": 1}, "signature": "postgres", "section": "project", "project_id": "p_1a812fe5ce48"}
  ],
  "engines": [
    {"id": "e_1", "name": "Docker Desktop", "endpoint": "unix:///Users/me/.docker/run/docker.sock", "version": "29.2.1", "reachable": true, "error": null}
  ],
  "notes": [
    {"code": "owners_hidden", "text": "1 listening port belongs to a program Harbormap can't see without admin rights."}
  ]
}
```

Then create `crates/hm-doctor/tests/cards.rs`. It checks each kind of card at the level where it matters: a beginner story for a project server, process details for builders, raw facts for experts, a Compose database, a read-only service of another project, system, app, unknown and free ports, a port split between two programs, and a worker without ports:

```rust
//! Cards for the resolved "shop" snapshot (the output of hm-project's resolution of its
//! `shop.json` scenario, saved as a fixture so these tests need no resolver).

use std::path::Path;

use hm_core::model::{BindScope, Listener, Owner, Proto, Section, Snapshot};
use hm_core::signature::Level;
use hm_doctor::cards::{Card, CardContext, port_cards, service_card};
use hm_doctor::packs::{self, Packs};
use hm_doctor::texts::{self, CardTexts};

fn snapshot() -> Snapshot {
    serde_json::from_str(include_str!("fixtures/shop-snapshot.json")).unwrap()
}

fn cards_for(snapshot: &Snapshot, port: u16, level: Level) -> Vec<Card> {
    let Packs { signatures, .. } = packs::load_embedded();
    let (cards, _): (CardTexts, _) = texts::load_embedded();
    let ctx = CardContext {
        snapshot,
        signatures: &signatures,
        texts: &cards,
        level,
        home: Some(Path::new("/Users/me")),
    };
    port_cards(&ctx, port)
}

fn one(snapshot: &Snapshot, port: u16, level: Level) -> Card {
    let mut cards = cards_for(snapshot, port, level);
    assert_eq!(cards.len(), 1, "{cards:#?}");
    cards.remove(0)
}

#[test]
fn a_project_server_reads_as_a_story_for_beginners() {
    let card = one(&snapshot(), 5173, Level::Beginner);
    assert_eq!(card.title, "Port 5173: web");
    assert_eq!(
        card.lines,
        vec![
            "web is part of the project shop.",
            "This is the live preview of your website while you work on it. It updates as soon as you save a file.",
            "Open it in your browser: http://localhost:5173",
            "Only this computer can open it.",
            "It has been running for 2 hours.",
        ]
    );
}

#[test]
fn builders_see_the_process_folder_and_environment() {
    let card = one(&snapshot(), 5173, Level::Builder);
    assert_eq!(card.title, ":5173 → web (Vite)");
    assert!(
        card.lines
            .contains(&"Process node, pid 802, started 2 h 3 min ago by me.".to_string())
    );
    assert!(
        card.lines
            .contains(&"Folder: apps/web (project shop, branch main)".to_string())
    );
    assert!(
        card.lines
            .contains(&"Listening on [::1]:5173 (this computer only).".to_string())
    );
    assert!(card.lines.contains(&"Port settings: PORT=5173".to_string()));
    assert!(
        card.lines
            .contains(&"Talks to: VITE_API_URL → http://localhost:8000".to_string())
    );
}

#[test]
fn experts_get_raw_facts() {
    let card = one(&snapshot(), 5173, Level::Expert);
    assert_eq!(card.title, ":5173/tcp → web [vite]");
    assert!(
        card.lines
            .contains(&"Signature: vite (sure) · role see · kind dev_server".to_string())
    );
    assert!(
        card.lines
            .contains(&"Service pids: 800, 801, 802".to_string())
    );
    assert!(card.lines.iter().any(|line| {
        line.starts_with("argv: node /Users/me/code/shop/apps/web/node_modules/.bin/vite")
    }));
}

#[test]
fn a_compose_database_is_described_as_a_container() {
    let card = one(&snapshot(), 55432, Level::Builder);
    assert_eq!(card.title, ":55432 → db (PostgreSQL) · Docker");
    assert!(card.lines.contains(
        &"Container shop-db-1 from image postgres:16-alpine: running, healthy.".to_string()
    ));
    assert!(
        card.lines
            .contains(&"Ports: 0.0.0.0:55432 → 5432/tcp, [::]:55432 → 5432/tcp".to_string())
    );
    assert!(
        card.lines
            .contains(&"Engine: Docker Desktop 29.2.1".to_string())
    );
    let beginner = one(&snapshot(), 55432, Level::Beginner);
    assert_eq!(
        beginner.lines[0],
        "db is part of the project shop. It runs inside Docker."
    );
    assert!(
        beginner
            .lines
            .contains(&"Docker says it is healthy.".to_string())
    );
}

#[test]
fn other_projects_say_they_are_read_only() {
    let card = one(&snapshot(), 3000, Level::Builder);
    assert_eq!(card.title, ":3000 → blog (Next.js)");
    assert_eq!(
        card.lines.last().unwrap(),
        "Read-only here: it belongs to blog at ~/code/blog. Run harbormap in that folder to manage it."
    );
}

#[test]
fn system_apps_unknown_and_free_ports_have_their_own_cards() {
    let snapshot = snapshot();
    let airplay = one(&snapshot, 5000, Level::Beginner);
    assert_eq!(airplay.title, "Port 5000: AirPlay Receiver");
    assert!(airplay.lines[1].starts_with("This is AirPlay Receiver, a macOS feature"));

    let chrome = one(&snapshot, 9820, Level::Builder);
    assert_eq!(chrome.title, ":9820 → Google Chrome (pid 76915)");

    let agent = one(&snapshot, 41234, Level::Beginner);
    assert_eq!(agent.title, "Port 41234: Claude Code");
    assert!(agent.lines.contains(
        &"Claude Code is a program on your computer. It is not part of a project.".to_string()
    ));

    let hidden = one(&snapshot, 6000, Level::Beginner);
    assert_eq!(hidden.title, "Port 6000: a program Harbormap can't see");

    let free = one(&snapshot, 4321, Level::Beginner);
    assert_eq!(free.title, "Port 4321 is free");
    assert_eq!(free.lines, vec!["Nothing is using port 4321 right now."]);
}

#[test]
fn two_programs_splitting_a_port_get_two_cards() {
    let mut snapshot = snapshot();
    // Classic macOS surprise: something else holds 127.0.0.1 while Vite holds ::1.
    snapshot.listeners.push(Listener {
        proto: Proto::Tcp,
        addr: "127.0.0.1".parse().unwrap(),
        port: 5173,
        bind: BindScope::Loopback,
        owner: Owner::Process { pid: 76915 },
        section: Section::System,
        project_id: None,
        service_id: None,
        label: "Google Chrome (pid 76915)".into(),
    });
    let cards = cards_for(&snapshot, 5173, Level::Builder);
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0].title, ":5173 → web (Vite)");
    assert_eq!(cards[1].title, ":5173 → Google Chrome (pid 76915)");
}

#[test]
fn a_worker_without_ports_still_gets_a_card() {
    let snapshot = snapshot();
    let Packs { signatures, .. } = packs::load_embedded();
    let (cards, _) = texts::load_embedded();
    let ctx = CardContext {
        snapshot: &snapshot,
        signatures: &signatures,
        texts: &cards,
        level: Level::Builder,
        home: Some(Path::new("/Users/me")),
    };
    let worker = snapshot
        .services
        .iter()
        .find(|service| service.name == "shop-api-celery")
        .unwrap();
    let card = service_card(&ctx, worker);
    assert_eq!(card.title, "shop-api-celery (Celery)");
    assert!(card.lines[0].starts_with("Celery worker"));
}
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p hm-doctor --test cards`
Expected: FAIL — compile error `` error[E0432]: unresolved import `hm_doctor::cards` ``.

- [ ] **Step 3: Write the implementation**

Add `pub mod cards;` above `pub mod packs;` in `crates/hm-doctor/src/lib.rs`, so the file reads:

```rust
//! Packs (signatures and card texts), the three-level card renderer and, from
//! alpha.2, the doctor rules.

pub mod cards;
pub mod packs;
pub mod render;
pub mod texts;
```

Create `crates/hm-doctor/src/cards.rs`. A listener that belongs to a service gets the service's card (`process` or `container` templates); otherwise its owner decides: `app` for a program outside projects, `system` for the OS or an engine helper, `unknown` when the owner is hidden. When nothing listens but a container publishes the port (Docker without a userland proxy), that container's card is returned, else the `free` card. Services of another project get the `elsewhere` sentences at the end:

```rust
//! Three-level cards for `harbormap why`. Facts come from the snapshot; sentences come
//! from `packs/texts/cards.toml`. One card is made per distinct owner of a port, so an
//! IPv4 and an IPv6 socket of the same program share a card, while two programs that
//! split a port between IPv4 and IPv6 get two.

use std::path::Path;

use serde::Serialize;

use hm_core::model::{
    Confidence, Container, Listener, Owner, Process, Service, ServiceSource, Snapshot,
};
use hm_core::signature::{Level, SignatureSet};

use crate::render::{Facts, bytes, duration, join_argv, render_line, render_lines, tilde, utc};
use crate::texts::{CardTexts, LevelLines};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Card {
    pub title: String,
    pub lines: Vec<String>,
}

pub struct CardContext<'a> {
    pub snapshot: &'a Snapshot,
    pub signatures: &'a SignatureSet,
    pub texts: &'a CardTexts,
    pub level: Level,
    pub home: Option<&'a Path>,
}

/// Cards for everything listening on `port`. When nothing listens but a container
/// publishes the port (Docker without a userland proxy), that container's card is
/// returned; otherwise a "free" card.
pub fn port_cards(ctx: &CardContext<'_>, port: u16) -> Vec<Card> {
    let mut groups: Vec<(&Owner, Vec<&Listener>)> = Vec::new();
    for listener in ctx.snapshot.listeners.iter().filter(|l| l.port == port) {
        match groups
            .iter_mut()
            .find(|(owner, _)| **owner == listener.owner)
        {
            Some((_, members)) => members.push(listener),
            None => groups.push((&listener.owner, vec![listener])),
        }
    }
    if groups.is_empty() {
        if let Some(service) = ctx
            .snapshot
            .services
            .iter()
            .find(|service| service.ports.contains(&port))
        {
            return vec![service_card_at(ctx, service, Some(port), &[])];
        }
        let mut facts = Facts::new();
        facts.insert("port", port.to_string());
        return vec![card(ctx, &ctx.texts.free, &facts)];
    }
    groups
        .into_iter()
        .map(|(owner, listeners)| owner_card(ctx, owner, &listeners))
        .collect()
}

/// The card for one service (`harbormap why web`).
pub fn service_card(ctx: &CardContext<'_>, service: &Service) -> Card {
    let listeners: Vec<&Listener> = ctx
        .snapshot
        .listeners
        .iter()
        .filter(|listener| listener.service_id.as_deref() == Some(service.id.as_str()))
        .collect();
    let port = listeners
        .first()
        .map(|listener| listener.port)
        .or_else(|| service.ports.first().copied());
    service_card_at(ctx, service, port, &listeners)
}

fn owner_card(ctx: &CardContext<'_>, owner: &Owner, listeners: &[&Listener]) -> Card {
    let port = listeners[0].port;
    let service = listeners[0].service_id.as_deref().and_then(|id| {
        ctx.snapshot
            .services
            .iter()
            .find(|service| service.id == id)
    });
    if let Some(service) = service {
        return service_card_at(ctx, service, Some(port), listeners);
    }
    let mut facts = socket_facts(ctx, Some(port), listeners);
    match owner {
        Owner::Process { pid } => {
            if let Some(process) = find_process(ctx, *pid) {
                add_process_facts(ctx, &mut facts, process, None);
                facts.insert("program", program_name(ctx, process));
                add_signature_text(
                    ctx,
                    &mut facts,
                    process.signature.as_deref(),
                    Confidence::Sure,
                );
            }
            card(ctx, &ctx.texts.app, &facts)
        }
        Owner::System { reason, pid } => {
            facts.insert("reason", reason.clone());
            if let Some(process) = pid.and_then(|pid| find_process(ctx, pid)) {
                facts.insert("pid", process.pid.to_string());
                facts.insert("program", process.name.clone());
                if let Some(exe) = &process.exe {
                    facts.insert("exe", exe.display().to_string());
                }
                add_signature_text(
                    ctx,
                    &mut facts,
                    process.signature.as_deref(),
                    Confidence::Sure,
                );
            }
            card(ctx, &ctx.texts.system, &facts)
        }
        Owner::Unknown { reason } => {
            facts.insert("reason", reason.clone());
            card(ctx, &ctx.texts.unknown, &facts)
        }
        Owner::ContainerForwarder { container_id } => {
            // A container without a service cannot happen after resolution; describe it anyway.
            facts.insert("name", container_id.chars().take(12).collect());
            card(ctx, &ctx.texts.container, &facts)
        }
    }
}

fn service_card_at(
    ctx: &CardContext<'_>,
    service: &Service,
    port: Option<u16>,
    listeners: &[&Listener],
) -> Card {
    let mut facts = socket_facts(ctx, port, listeners);
    add_service_facts(ctx, &mut facts, service);
    let lines = match &service.source {
        ServiceSource::Process { main_pid, pids, .. } => {
            if let Some(process) = find_process(ctx, *main_pid) {
                add_process_facts(ctx, &mut facts, process, service.worktree.as_deref());
            }
            facts.insert(
                "pids",
                pids.iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            &ctx.texts.process
        }
        ServiceSource::Container { container_id } => {
            if let Some(container) = ctx
                .snapshot
                .containers
                .iter()
                .find(|container| &container.id == container_id)
            {
                add_container_facts(ctx, &mut facts, container);
            }
            &ctx.texts.container
        }
    };
    let mut built = card(ctx, lines, &facts);
    if service.section == hm_core::model::Section::Elsewhere && service.project_id.is_some() {
        built
            .lines
            .extend(render_lines(ctx.texts.elsewhere.get(ctx.level), &facts));
    }
    built
}

fn card(ctx: &CardContext<'_>, templates: &LevelLines, facts: &Facts) -> Card {
    let title = templates
        .title
        .as_ref()
        .and_then(|title| render_line(title.get(ctx.level), facts))
        .or_else(|| facts.get("port").map(|port| format!(":{port}")))
        .unwrap_or_default();
    Card {
        title,
        lines: render_lines(templates.get(ctx.level), facts),
    }
}

fn socket_facts(ctx: &CardContext<'_>, port: Option<u16>, listeners: &[&Listener]) -> Facts {
    let mut facts = Facts::new();
    if let Some(port) = port {
        facts.insert("port", port.to_string());
    }
    if !listeners.is_empty() {
        let addrs: Vec<String> = listeners
            .iter()
            .map(|listener| match listener.addr {
                std::net::IpAddr::V6(v6) => format!("[{v6}]:{}", listener.port),
                std::net::IpAddr::V4(v4) => format!("{v4}:{}", listener.port),
            })
            .collect();
        facts.insert("addr", addrs.join(", "));
        let scope = match listeners[0].bind {
            hm_core::model::BindScope::Loopback => "loopback",
            hm_core::model::BindScope::All => "all",
            hm_core::model::BindScope::Specific => "specific",
        };
        if let Some(reach) = ctx.texts.reach.get(scope) {
            facts.insert("reach", reach.get(ctx.level).to_string());
        }
    }
    facts
}

fn add_service_facts(ctx: &CardContext<'_>, facts: &mut Facts, service: &Service) {
    facts.insert("name", service.name.clone());
    if let Some(framework) = &service.framework {
        let framework = match service.confidence {
            Confidence::Probably => format!("{framework}, probably"),
            Confidence::Sure => framework.clone(),
        };
        facts.insert("framework", framework);
    }
    if let Some(signature) = &service.signature {
        facts.insert("signature", signature.clone());
    }
    facts.insert(
        "confidence",
        match service.confidence {
            Confidence::Sure => "sure".into(),
            Confidence::Probably => "probably".into(),
        },
    );
    facts.insert("role", snake(&service.role));
    facts.insert("kind", snake(&service.kind));
    if let Some(url) = service.urls.first() {
        facts.insert("url", url.clone());
    }
    add_signature_text(ctx, facts, service.signature.as_deref(), service.confidence);
    if let Some(project) = service.project_id.as_deref().and_then(|id| {
        ctx.snapshot
            .projects
            .iter()
            .find(|project| project.id == id)
    }) {
        facts.insert("project", project.name.clone());
        facts.insert("project_id", project.id.clone());
        facts.insert("project_root", tilde(&project.root, ctx.home));
        if let Some(worktree) = service.worktree.as_deref() {
            facts.insert("worktree", tilde(worktree, ctx.home));
            if let Some(branch) = project
                .worktrees
                .iter()
                .find(|candidate| candidate.path == worktree)
                .and_then(|candidate| candidate.branch.clone())
            {
                facts.insert("branch", branch);
            }
        }
    }
}

fn add_signature_text(
    ctx: &CardContext<'_>,
    facts: &mut Facts,
    signature: Option<&str>,
    confidence: Confidence,
) {
    if let Some(signature) = signature.and_then(|id| ctx.signatures.get(id)) {
        facts.insert("what", signature.texts.get(ctx.level).to_string());
        facts.insert("signature", signature.id.clone());
        if confidence == Confidence::Probably
            && let Some(guess) = &ctx.texts.guess
        {
            facts.insert("guess", guess.get(ctx.level).to_string());
        }
    }
}

fn add_process_facts(
    ctx: &CardContext<'_>,
    facts: &mut Facts,
    process: &Process,
    worktree: Option<&Path>,
) {
    facts.insert("process", process.name.clone());
    facts.insert("pid", process.pid.to_string());
    if let Some(user) = &process.user {
        facts.insert("user", user.clone());
    }
    facts.insert("uptime", duration(process.uptime_s, ctx.level));
    facts.insert("started_utc", utc(process.start_time));
    facts.insert("rss", bytes(process.rss_bytes));
    if let Some(exe) = &process.exe {
        facts.insert("exe", exe.display().to_string());
    }
    facts.insert("argv", join_argv(&process.argv));
    if let Some(cwd) = &process.cwd {
        facts.insert("cwd", tilde(cwd, ctx.home));
        let relative = worktree
            .and_then(|root| cwd.strip_prefix(root).ok())
            .map(|rest| {
                if rest.as_os_str().is_empty() {
                    ".".to_string()
                } else {
                    rest.display().to_string()
                }
            });
        facts.insert("cwd_rel", relative.unwrap_or_else(|| tilde(cwd, ctx.home)));
    }
    if let Some(ppid) = process.ppid {
        facts.insert("ppid", ppid.to_string());
        if let Some(parent) = find_process(ctx, ppid) {
            facts.insert("parent", parent.name.clone());
        }
    }
    if !process.env.ports.is_empty() {
        let ports: Vec<String> = process
            .env
            .ports
            .iter()
            .map(|entry| format!("{}={}", entry.name, entry.port))
            .collect();
        facts.insert("env_ports", ports.join(", "));
    }
    if !process.env.urls.is_empty() {
        let urls: Vec<String> = process
            .env
            .urls
            .iter()
            .map(|entry| {
                let scheme = if entry.scheme.is_empty() {
                    String::new()
                } else {
                    format!("{}://", entry.scheme)
                };
                let port = entry
                    .port
                    .map(|port| format!(":{port}"))
                    .unwrap_or_default();
                format!("{} → {scheme}{}{port}", entry.name, entry.host)
            })
            .collect();
        facts.insert("env_urls", urls.join(", "));
    }
}

fn add_container_facts(ctx: &CardContext<'_>, facts: &mut Facts, container: &Container) {
    facts.insert("container", container.name.clone());
    facts.insert("container_id", container.id.chars().take(12).collect());
    facts.insert("image", container.image.clone());
    facts.insert("state", container.state.clone());
    facts.insert("restarts", container.restart_count.to_string());
    if let Some(started) = &container.started_at {
        facts.insert("started_utc", started.clone());
    }
    if let Some(health) = container
        .health
        .as_deref()
        .and_then(|health| ctx.texts.health.get(health))
    {
        facts.insert("health", health.get(ctx.level).to_string());
    }
    let mapping: Vec<String> = container
        .ports
        .iter()
        .map(|binding| match binding.host_port {
            Some(host) => {
                let ip = binding
                    .host_ip
                    .map(|ip| match ip {
                        std::net::IpAddr::V6(v6) => format!("[{v6}]:"),
                        std::net::IpAddr::V4(v4) => format!("{v4}:"),
                    })
                    .unwrap_or_default();
                format!("{ip}{host} → {}/{}", binding.container_port, binding.proto)
            }
            None => format!(
                "{}/{} (not published)",
                binding.container_port, binding.proto
            ),
        })
        .collect();
    if !mapping.is_empty() {
        facts.insert("mapping", mapping.join(", "));
    }
    if !container.networks.is_empty() {
        facts.insert("networks", container.networks.join(", "));
    }
    let volumes: Vec<String> = container
        .mounts
        .iter()
        .filter(|mount| mount.kind == "volume")
        .filter_map(|mount| mount.name.clone())
        .collect();
    if !volumes.is_empty() {
        facts.insert("volumes", volumes.join(", "));
    }
    if let Some(compose) = &container.compose {
        facts.insert("compose_project", compose.project.clone());
        facts.insert("compose_service", compose.service.clone());
        let files: Vec<String> = compose
            .config_files
            .iter()
            .map(|file| tilde(file, ctx.home))
            .collect();
        if !files.is_empty() {
            facts.insert("compose_files", files.join(", "));
        }
        if let Some(dir) = &compose.working_dir {
            facts.insert("working_dir", tilde(dir, ctx.home));
        }
    }
    if let Some(engine) = ctx
        .snapshot
        .engines
        .iter()
        .find(|engine| engine.id == container.engine)
    {
        let name = match &engine.version {
            Some(version) => format!("{} {version}", engine.name),
            None => engine.name.clone(),
        };
        facts.insert("engine", name);
        facts.insert("engine_endpoint", engine.endpoint.clone());
    }
}

fn find_process<'a>(ctx: &CardContext<'a>, pid: u32) -> Option<&'a Process> {
    ctx.snapshot
        .processes
        .iter()
        .find(|process| process.pid == pid)
}

fn program_name(ctx: &CardContext<'_>, process: &Process) -> String {
    process
        .signature
        .as_deref()
        .and_then(|id| ctx.signatures.get(id))
        .map_or_else(|| process.name.clone(), |signature| signature.name.clone())
}

/// The serde name of an enum value (`Kind::DevServer` → `dev_server`).
fn snake<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p hm-doctor --test cards`
Expected: PASS — `test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`

- [ ] **Step 5: Commit**

```bash
git add crates/hm-doctor/src/cards.rs crates/hm-doctor/src/lib.rs crates/hm-doctor/tests/cards.rs crates/hm-doctor/tests/fixtures/shop-snapshot.json
git commit -s -m "feat(doctor): three-level cards for ports and services"
```

### Task 15: `harbormap status`

This task replaces the minimal CLI from Task 1 with `harbormap status`, which plain `harbormap` also runs for now. It probes the computer, resolves the snapshot and prints the opened project's services first, grouped by worktree, then one summary line for everything else. `--machine` lists everything, and `--json` prints the snapshot.

**Files:**
- Create: `crates/harbormap/src/cli.rs`
- Create: `crates/harbormap/src/source.rs`
- Create: `crates/harbormap/src/status.rs`
- Modify: `crates/harbormap/src/main.rs` (replace the whole file)
- Modify: `crates/harbormap/tests/cli.rs` (replace the whole file)
- Test: `crates/harbormap/tests/cli.rs`

**Interfaces:**
- Consumes: `hm_core::model::{Snapshot, Project, Section, Service, ServiceSource}` (Task 4); `hm_doctor::packs::load_embedded() -> Packs` (Task 9); `hm_doctor::render::tilde(path: &Path, home: Option<&Path>) -> String` (Task 13); `hm_probe::probe(options: &ProbeOptions) -> RawInventory` and `hm_probe::ProbeOptions` (`Default`) (Task 8); `hm_project::git::RealWorld` (`RealWorld::new()`), the `hm_project::World` trait (`canonicalize`, `home`) and `hm_project::Scenario { launch_dir, world, inventory }` (Task 10); `hm_project::{ResolveInput, build_snapshot(input: ResolveInput<'_>) -> Snapshot}` and the scenario `crates/hm-project/tests/fixtures/shop.json` (Task 12); `anyhow`, `clap`, `serde_json`, `assert_cmd` (Task 1).
- Produces (private modules of the binary): `cli::Cli { pub json: bool, pub command: Option<Command> }`, `cli::Command::Status(StatusArgs)`, `cli::StatusArgs { pub machine: bool }` (`Default`); `source::Input { pub snapshot: Snapshot, pub home: Option<PathBuf> }`, `source::load() -> anyhow::Result<Input>`; `status::run(input: &Input, args: &StatusArgs, json: bool) -> anyhow::Result<ExitCode>`, `status::render(snapshot: &Snapshot, home: Option<&Path>, machine: bool) -> String`. On the command line: `harbormap [status] [--machine] [--json]`, and the debug-only `HARBORMAP_FIXTURE` environment variable.

- [ ] **Step 1: Write the failing test**

The tests run the real binary against the recorded "shop" machine from Task 12 and compare its output with golden text; `same_text` compares line by line first, so a failure names the first line that differs.

Replace the whole content of `crates/harbormap/tests/cli.rs` (Task 1's version test) with:

```rust
//! The CLI against the recorded "shop" machine. Debug builds read the scenario named
//! by `HARBORMAP_FIXTURE` instead of probing this computer.

use assert_cmd::Command;

fn fixture() -> String {
    format!(
        "{}/../hm-project/tests/fixtures/shop.json",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn harbormap(args: &[&str]) -> assert_cmd::assert::Assert {
    Command::cargo_bin("harbormap")
        .unwrap()
        .env("HARBORMAP_FIXTURE", fixture())
        .args(args)
        .assert()
}

fn stdout(args: &[&str]) -> String {
    let output = harbormap(args).success().get_output().stdout.clone();
    String::from_utf8(output).unwrap()
}

/// Compares line by line first, so a failure points at the first differing line.
fn same_text(actual: &str, expected: &str) {
    for (number, (a, e)) in actual.lines().zip(expected.lines()).enumerate() {
        assert_eq!(a, e, "line {}", number + 1);
    }
    assert_eq!(actual, expected);
}

#[test]
fn prints_its_version() {
    Command::cargo_bin("harbormap")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("harbormap {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn status_shows_the_project_first_and_summarises_the_rest() {
    same_text(&stdout(&["status"]), STATUS);
}

#[test]
fn status_machine_lists_everything() {
    same_text(&stdout(&["status", "--machine"]), STATUS_MACHINE);
}

#[test]
fn no_subcommand_means_status() {
    assert_eq!(stdout(&[]), stdout(&["status"]));
}

#[test]
fn status_json_is_the_snapshot() {
    let snapshot: hm_core::model::Snapshot =
        serde_json::from_str(&stdout(&["status", "--json"])).unwrap();
    assert_eq!(snapshot.schema, 1);
    assert_eq!(snapshot.services.len(), 8);
    assert!(
        snapshot
            .listeners
            .iter()
            .all(|listener| !listener.label.is_empty())
    );
}

const STATUS: &str = r#"shop  ~/code/shop · main
  main  ~/code/shop
    Billing          ASP.NET Core  :5280   running · pid 951   http://localhost:5280
    db               PostgreSQL    :55432  running · healthy · container shop-db-1
    shop-api         FastAPI       :8000   running · pid 900   http://localhost:8000
    shop-api-celery  Celery        –       running · pid 970
    web              Vite          :5173   running · pid 802   http://localhost:5173
  feature/login  ~/code/shop-feature
    web              Vite          :5174   running · pid 1300   http://localhost:5174

Also on this computer: 2 services elsewhere (read-only) and 6 system or unknown ports.
Run `harbormap status --machine` to list them.

Docker: Docker Desktop 29.2.1
Note: 1 listening port belongs to a program Harbormap can't see without admin rights.
"#;

const STATUS_MACHINE: &str = r#"shop  ~/code/shop · main
  main  ~/code/shop
    Billing          ASP.NET Core  :5280   running · pid 951   http://localhost:5280
    db               PostgreSQL    :55432  running · healthy · container shop-db-1
    shop-api         FastAPI       :8000   running · pid 900   http://localhost:8000
    shop-api-celery  Celery        –       running · pid 970
    web              Vite          :5173   running · pid 802   http://localhost:5173
  feature/login  ~/code/shop-feature
    web              Vite          :5174   running · pid 1300   http://localhost:5174

Elsewhere on this computer (read-only)
  blog  ~/code/blog
    blog          Next.js  :3000  running · pid 1200   http://localhost:3000
  Containers outside projects
    lonely-redis  Redis    :6380  running · container lonely-redis

System & unknown
  :631         cupsd (pid 412)
  :5000 :7000  AirPlay Receiver
  :6000        unknown owner
  :9820        Google Chrome (pid 76915)
  :41234       Claude Code (pid 3000)

Docker: Docker Desktop 29.2.1
Note: 1 listening port belongs to a program Harbormap can't see without admin rights.
"#;
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p harbormap --test cli`
Expected: FAIL — `test result: FAILED. 1 passed; 4 failed`. The binary from Task 1 has no subcommands, so it exits with code 2 and `error: unexpected argument 'status' found`.

- [ ] **Step 3: Write the implementation**

Everything that only `why` needs waits for Task 16: there is no `--level` flag yet, and `Input` carries only the snapshot and the home folder. Unused fields fail `cargo clippy -- -D warnings` in a binary crate.

Create `crates/harbormap/src/cli.rs`. `--json` is global, so it works before or after the subcommand:

```rust
//! Command-line arguments.

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "harbormap",
    version,
    about = "Know what's running, why it's broken, and keep your AI agents from breaking it."
)]
pub struct Cli {
    /// Print JSON instead of text.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show what runs for this project and elsewhere on this computer.
    Status(StatusArgs),
}

#[derive(Debug, Default, Args)]
pub struct StatusArgs {
    /// List every project, system program and unknown owner instead of a summary.
    #[arg(long)]
    pub machine: bool,
}
```

Create `crates/harbormap/src/source.rs`. In debug builds only, `HARBORMAP_FIXTURE=<scenario.json>` makes the CLI resolve a recorded machine (the scenario format of the hm-project fixtures: launch folder, world and raw inventory) instead of probing this computer. The branch sits behind `#[cfg(debug_assertions)]`: `cargo test` and `cargo run` honour it, release builds do not contain it. That keeps the golden tests identical on every machine and OS:

```rust
//! Where the snapshot comes from: this computer, or — in debug builds only — a
//! recorded scenario named by `HARBORMAP_FIXTURE`, which keeps CLI tests independent
//! of the machine they run on.

use std::path::PathBuf;

use anyhow::Context;

use hm_core::model::Snapshot;
use hm_probe::ProbeOptions;
use hm_project::git::RealWorld;
use hm_project::{ResolveInput, World, build_snapshot};

pub struct Input {
    pub snapshot: Snapshot,
    pub home: Option<PathBuf>,
}

pub fn load() -> anyhow::Result<Input> {
    let packs = hm_doctor::packs::load_embedded();

    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("HARBORMAP_FIXTURE") {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", PathBuf::from(&path).display()))?;
        let scenario: hm_project::Scenario =
            serde_json::from_str(&text).context("HARBORMAP_FIXTURE is not a scenario file")?;
        let snapshot = build_snapshot(ResolveInput {
            inventory: &scenario.inventory,
            world: &scenario.world,
            signatures: &packs.signatures,
            launch_dir: &scenario.launch_dir,
        });
        return Ok(Input {
            snapshot,
            home: scenario.world.home.clone(),
        });
    }

    let inventory = hm_probe::probe(&ProbeOptions::default());
    let world = RealWorld::new();
    let launch_dir =
        world.canonicalize(&std::env::current_dir().context("cannot read the current folder")?);
    let snapshot = build_snapshot(ResolveInput {
        inventory: &inventory,
        world: &world,
        signatures: &packs.signatures,
        launch_dir: &launch_dir,
    });
    Ok(Input {
        snapshot,
        home: world.home(),
    })
}
```

Create `crates/harbormap/src/status.rs`. `--json` always prints the full snapshot: it is the public contract from Task 4 (stable field names and a `schema` version), and scripts and agents filter it themselves. `--machine` only changes how much the text view shows, so `status --json` and `status --machine --json` print the same JSON. Tables of one block share column widths so columns line up across worktrees, and system or unknown listeners with the same label share one line (`:5000 :7000  AirPlay Receiver`):

```rust
//! `harbormap status`: the opened project's services first, then a one-line summary
//! of everything else — or, with `--machine`, the full list.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use hm_core::model::{Project, Section, Service, ServiceSource, Snapshot};
use hm_doctor::render::tilde;

use crate::cli::StatusArgs;
use crate::source::Input;

pub fn run(input: &Input, args: &StatusArgs, json: bool) -> anyhow::Result<ExitCode> {
    if json {
        println!("{}", serde_json::to_string_pretty(&input.snapshot)?);
    } else {
        print!(
            "{}",
            render(&input.snapshot, input.home.as_deref(), args.machine)
        );
    }
    Ok(ExitCode::SUCCESS)
}

pub fn render(snapshot: &Snapshot, home: Option<&Path>, machine: bool) -> String {
    let mut out = String::new();
    let opened = snapshot.projects.iter().find(|project| project.opened);
    match opened {
        Some(project) => project_block(&mut out, snapshot, project, home),
        None => out.push_str("Not inside a project folder, so here is the whole computer.\n"),
    }

    let elsewhere: Vec<&Service> = snapshot
        .services
        .iter()
        .filter(|service| service.section == Section::Elsewhere)
        .collect();
    let system = system_groups(snapshot);
    if machine || opened.is_none() {
        if !elsewhere.is_empty() {
            out.push('\n');
            elsewhere_block(&mut out, snapshot, &elsewhere, home);
        }
        if !system.is_empty() {
            out.push('\n');
            out.push_str("System & unknown\n");
            let width = system
                .iter()
                .map(|(ports, _)| ports.len())
                .max()
                .unwrap_or(0);
            for (ports, label) in &system {
                out.push_str(&format!("  {ports:<width$}  {label}\n"));
            }
        }
    } else if !elsewhere.is_empty() || !system.is_empty() {
        let system_ports: usize = system
            .iter()
            .map(|(ports, _)| ports.split(' ').count())
            .sum();
        out.push('\n');
        out.push_str(&format!(
            "Also on this computer: {} elsewhere (read-only) and {}.\nRun `harbormap status --machine` to list them.\n",
            plural(elsewhere.len(), "service"),
            plural(system_ports, "system or unknown port"),
        ));
    }

    let engines: Vec<String> = snapshot
        .engines
        .iter()
        .filter(|engine| engine.reachable)
        .map(|engine| match &engine.version {
            Some(version) => format!("{} {version}", engine.name),
            None => engine.name.clone(),
        })
        .collect();
    if !engines.is_empty() {
        out.push_str(&format!("\nDocker: {}\n", engines.join(", ")));
    }
    for note in &snapshot.notes {
        out.push_str(&format!("Note: {}\n", note.text));
    }
    out
}

fn project_block(out: &mut String, snapshot: &Snapshot, project: &Project, home: Option<&Path>) {
    let branch = project
        .worktrees
        .first()
        .and_then(|worktree| worktree.branch.as_deref());
    match (project.git, branch) {
        (true, Some(branch)) => out.push_str(&format!(
            "{}  {} · {branch}\n",
            project.name,
            tilde(&project.root, home)
        )),
        (true, None) => out.push_str(&format!(
            "{}  {}\n",
            project.name,
            tilde(&project.root, home)
        )),
        (false, _) => out.push_str(&format!(
            "{}  {} (folder without git)\n",
            project.name,
            tilde(&project.root, home)
        )),
    }
    let services: Vec<&Service> = snapshot
        .services
        .iter()
        .filter(|service| service.project_id.as_deref() == Some(project.id.as_str()))
        .collect();
    if services.is_empty() {
        out.push_str("  Nothing is running for this project.\n");
        return;
    }
    let widths = widths(snapshot, &services);
    worktree_tables(out, snapshot, project, &services, home, "  ", widths);
}

/// One table, or one table per worktree when the services run in several worktrees.
fn worktree_tables(
    out: &mut String,
    snapshot: &Snapshot,
    project: &Project,
    services: &[&Service],
    home: Option<&Path>,
    indent: &str,
    widths: Widths,
) {
    let mut by_worktree: BTreeMap<usize, Vec<&Service>> = BTreeMap::new();
    for service in services {
        let index = project
            .worktrees
            .iter()
            .position(|worktree| Some(worktree.path.as_path()) == service.worktree.as_deref())
            .unwrap_or(0);
        by_worktree.entry(index).or_default().push(service);
    }
    if by_worktree.len() == 1 {
        table(out, snapshot, services, indent, widths);
        return;
    }
    let nested = format!("{indent}  ");
    for (index, group) in by_worktree {
        let worktree = &project.worktrees[index];
        let label = worktree.branch.clone().unwrap_or_else(|| "detached".into());
        out.push_str(&format!(
            "{indent}{label}  {}\n",
            tilde(&worktree.path, home)
        ));
        table(out, snapshot, &group, &nested, widths);
    }
}

fn elsewhere_block(
    out: &mut String,
    snapshot: &Snapshot,
    services: &[&Service],
    home: Option<&Path>,
) {
    out.push_str("Elsewhere on this computer (read-only)\n");
    let mut by_project: BTreeMap<Option<&str>, Vec<&Service>> = BTreeMap::new();
    for service in services {
        by_project
            .entry(service.project_id.as_deref())
            .or_default()
            .push(service);
    }
    let widths = widths(snapshot, services);
    for (project_id, group) in &by_project {
        let Some(project_id) = project_id else {
            continue;
        };
        let Some(project) = snapshot.projects.iter().find(|p| p.id == *project_id) else {
            continue;
        };
        out.push_str(&format!(
            "  {}  {}\n",
            project.name,
            tilde(&project.root, home)
        ));
        worktree_tables(out, snapshot, project, group, home, "    ", widths);
    }
    if let Some(group) = by_project.get(&None) {
        out.push_str("  Containers outside projects\n");
        table(out, snapshot, group, "    ", widths);
    }
}

type Widths = (usize, usize, usize);

/// Column widths shared by every table of one block, so columns line up.
fn widths(snapshot: &Snapshot, services: &[&Service]) -> Widths {
    let rows: Vec<[String; 4]> = services
        .iter()
        .map(|service| row(snapshot, service))
        .collect();
    let width = |column: usize| {
        rows.iter()
            .map(|row| row[column].chars().count())
            .max()
            .unwrap_or(0)
    };
    (width(0), width(1), width(2))
}

/// One row per service: name, framework, ports, state and the most useful detail.
fn table(
    out: &mut String,
    snapshot: &Snapshot,
    services: &[&Service],
    indent: &str,
    widths: Widths,
) {
    let (w0, w1, w2) = widths;
    for service in services {
        let row = row(snapshot, service);
        out.push_str(&format!(
            "{indent}{:<w0$}  {:<w1$}  {:<w2$}  {}\n",
            row[0], row[1], row[2], row[3]
        ));
    }
}

fn row(snapshot: &Snapshot, service: &Service) -> [String; 4] {
    let ports = if service.ports.is_empty() {
        "–".to_string()
    } else {
        service
            .ports
            .iter()
            .map(|port| format!(":{port}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut state = service.state.clone();
    if let Some(health) = &service.health {
        state.push_str(&format!(" · {health}"));
    }
    match &service.source {
        ServiceSource::Process { main_pid, .. } => state.push_str(&format!(" · pid {main_pid}")),
        ServiceSource::Container { container_id } => {
            if let Some(container) = snapshot.containers.iter().find(|c| &c.id == container_id) {
                state.push_str(&format!(" · container {}", container.name));
            }
        }
    }
    if let Some(url) = service.urls.first() {
        state.push_str(&format!("   {url}"));
    }
    [
        service.name.clone(),
        service.framework.clone().unwrap_or_else(|| "–".into()),
        ports,
        state,
    ]
}

/// System and unknown listeners grouped by label: ("5000 7000" ports, label).
fn system_groups(snapshot: &Snapshot) -> Vec<(String, String)> {
    let mut groups: Vec<(Vec<u16>, String)> = Vec::new();
    for listener in snapshot
        .listeners
        .iter()
        .filter(|listener| listener.section == Section::System)
    {
        match groups
            .iter_mut()
            .find(|(_, label)| *label == listener.label)
        {
            Some((ports, _)) => {
                if !ports.contains(&listener.port) {
                    ports.push(listener.port);
                }
            }
            None => groups.push((vec![listener.port], listener.label.clone())),
        }
    }
    groups.sort_by_key(|(ports, _)| ports.iter().min().copied());
    groups
        .into_iter()
        .map(|(ports, label)| {
            let ports = ports
                .iter()
                .map(|port| format!(":{port}"))
                .collect::<Vec<_>>()
                .join(" ");
            (ports, label)
        })
        .collect()
}

fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}
```

Replace the whole content of `crates/harbormap/src/main.rs` with this. With no subcommand it runs `status`; an error is printed as `harbormap: <error>` and exits with code 1:

```rust
//! The `harbormap` command line.

mod cli;
mod source;
mod status;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command, StatusArgs};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("harbormap: {error:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    let input = source::load()?;
    match cli
        .command
        .unwrap_or(Command::Status(StatusArgs::default()))
    {
        Command::Status(args) => status::run(&input, &args, cli.json),
    }
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p harbormap --test cli`
Expected: PASS — `test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`

- [ ] **Step 5: Commit**

```bash
git add crates/harbormap/src/cli.rs crates/harbormap/src/source.rs crates/harbormap/src/status.rs crates/harbormap/src/main.rs crates/harbormap/tests/cli.rs
git commit -s -m "feat(cli): harbormap status"
```

### Task 16: `harbormap why`

`harbormap why <port|name>` explains who holds a port, or what a service is, with the cards from Task 14 at the level chosen by the global `--level` flag (default `builder`). A name is `service` or `project/service`, compared without case; when several services match, the opened project wins, then the worktree Harbormap was started in, and otherwise the command lists the full names and exits with code 1.

**Files:**
- Create: `crates/harbormap/src/why.rs`
- Modify: `crates/harbormap/src/cli.rs` (the `--level` flag, `LevelArg`, `Command::Why` and `WhyArgs`)
- Modify: `crates/harbormap/src/main.rs` (`mod why;`, the level and the `Why` arm)
- Modify: `crates/harbormap/src/source.rs` (`Input` gains `signatures`, `texts` and `launch_dir`)
- Modify: `crates/harbormap/tests/cli.rs` (the `why` tests)
- Test: `crates/harbormap/tests/cli.rs`

**Interfaces:**
- Consumes: `hm_doctor::cards::{Card, CardContext, port_cards, service_card}` (Task 14); `hm_doctor::texts::{load_embedded, CardTexts}` and `hm_doctor::render::tilde` (Task 13); `hm_core::signature::{Level, SignatureSet}` and `hm_core::model::{Section, Service}` (Task 4); `hm_project::paths::within(child: &Path, parent: &Path, windows: bool) -> bool` (Task 10); `cli::{Cli, Command}` and `source::{Input, load}` (Task 15); `predicates` (Task 1).
- Produces: `why::run(input: &Input, args: &WhyArgs, json: bool, level: Level) -> anyhow::Result<ExitCode>`; `cli::WhyArgs { pub target: String }`, `cli::LevelArg { Beginner, Builder, Expert }` (`ValueEnum`) with `impl From<LevelArg> for Level`, `Cli` gains `pub level: LevelArg` (global `--level`, default `builder`), `Command::Why(WhyArgs)`; `source::Input` gains `pub signatures: SignatureSet`, `pub texts: CardTexts` and `pub launch_dir: PathBuf`. On the command line: `harbormap why <port|:port|name|project/name> [--level beginner|builder|expert] [--json]`; with `--json` it prints an object with `target` (the argument as given) and `cards`, a list of `{title, lines}`.

- [ ] **Step 1: Write the failing test**

The new tests explain a port to a beginner, find the same card by port, by name and by `project/name`, prefer the worktree Harbormap was started in, print JSON, describe a free port, and fail with a hint for an unknown name.

In `crates/harbormap/tests/cli.rs`, add this import below `use assert_cmd::Command;`:

```rust
use predicates::str::contains;
```

Add these tests between `status_json_is_the_snapshot` and `const STATUS`, with one blank line on each side:

```rust
#[test]
fn why_a_port_explains_it_for_beginners() {
    same_text(
        &stdout(&["why", "5173", "--level", "beginner"]),
        WHY_BEGINNER,
    );
}

#[test]
fn why_accepts_names_and_project_names() {
    let by_port = stdout(&["why", "55432"]);
    assert_eq!(stdout(&["why", "db"]), by_port);
    assert_eq!(stdout(&["why", "shop/db"]), by_port);
    assert!(by_port.starts_with(":55432 → db (PostgreSQL) · Docker\n"));
}

#[test]
fn why_prefers_the_worktree_it_was_started_in() {
    assert!(stdout(&["why", "web"]).starts_with(":5173 → web (Vite)\n"));
}

#[test]
fn why_json_has_title_and_lines() {
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&["why", ":5000", "--json"])).unwrap();
    assert_eq!(json["target"], ":5000");
    assert_eq!(json["cards"][0]["title"], ":5000 → AirPlay Receiver");
    assert!(json["cards"][0]["lines"].as_array().unwrap().len() >= 2);
}

#[test]
fn why_a_free_port_says_so() {
    assert_eq!(stdout(&["why", "4321"]), ":4321 → free\n  :4321 is free.\n");
}

#[test]
fn why_an_unknown_name_fails_with_a_hint() {
    harbormap(&["why", "nope"])
        .code(1)
        .stderr(contains("nothing called \"nope\" is running"));
}
```

Append the expected beginner card at the end of the file, after one blank line:

```rust
const WHY_BEGINNER: &str = r#"Port 5173: web
  web is part of the project shop.
  This is the live preview of your website while you work on it. It updates as soon as you save a file.
  Open it in your browser: http://localhost:5173
  Only this computer can open it.
  It has been running for 2 hours.
"#;
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p harbormap --test cli`
Expected: FAIL — `test result: FAILED. 5 passed; 6 failed`. The binary from Task 15 exits with code 2 and `error: unrecognized subcommand 'why'`.

- [ ] **Step 3: Write the implementation**

Create `crates/harbormap/src/why.rs`. A target that parses as a port, with or without a leading `:`, gives one card per owner of that port; anything else is looked up as a service name:

```rust
//! `harbormap why <port|name>`: who holds a port, or what a service is, at the chosen
//! level of detail.

use std::process::ExitCode;

use serde::Serialize;

use hm_core::model::{Section, Service};
use hm_core::signature::Level;
use hm_doctor::cards::{Card, CardContext, port_cards, service_card};
use hm_doctor::render::tilde;
use hm_project::paths::within;

use crate::cli::WhyArgs;
use crate::source::Input;

#[derive(Serialize)]
struct WhyOutput<'a> {
    target: &'a str,
    cards: Vec<Card>,
}

pub fn run(input: &Input, args: &WhyArgs, json: bool, level: Level) -> anyhow::Result<ExitCode> {
    let ctx = CardContext {
        snapshot: &input.snapshot,
        signatures: &input.signatures,
        texts: &input.texts,
        level,
        home: input.home.as_deref(),
    };
    let cards = match args.target.trim_start_matches(':').parse::<u16>() {
        Ok(port) => port_cards(&ctx, port),
        Err(_) => match find_service(input, &args.target) {
            Found::One(service) => vec![service_card(&ctx, service)],
            Found::None => {
                eprintln!(
                    "harbormap: nothing called \"{}\" is running. Run `harbormap status` to see the names.",
                    args.target
                );
                return Ok(ExitCode::from(1));
            }
            Found::Many(services) => {
                eprintln!(
                    "harbormap: several services are called \"{}\". Use one of:",
                    args.target
                );
                for service in services {
                    eprintln!("  {}", qualified_name(input, service));
                }
                return Ok(ExitCode::from(1));
            }
        },
    };
    if json {
        let output = WhyOutput {
            target: &args.target,
            cards,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        let blocks: Vec<String> = cards
            .iter()
            .map(|card| {
                let mut block = format!("{}\n", card.title);
                for line in &card.lines {
                    block.push_str(&format!("  {line}\n"));
                }
                block
            })
            .collect();
        print!("{}", blocks.join("\n"));
    }
    Ok(ExitCode::SUCCESS)
}

enum Found<'a> {
    None,
    One(&'a Service),
    Many(Vec<&'a Service>),
}

/// `name` or `project/name`, case-insensitive. Among several matches, the opened
/// project wins, then the worktree Harbormap was started in.
fn find_service<'a>(input: &'a Input, target: &str) -> Found<'a> {
    let snapshot = &input.snapshot;
    let (project_name, name) = match target.split_once('/') {
        Some((project, name)) => (Some(project), name),
        None => (None, target),
    };
    let mut matches: Vec<&Service> = snapshot
        .services
        .iter()
        .filter(|service| service.name.eq_ignore_ascii_case(name))
        .filter(|service| match project_name {
            Some(wanted) => {
                project_of(input, service).is_some_and(|p| p.eq_ignore_ascii_case(wanted))
            }
            None => true,
        })
        .collect();
    if matches.len() > 1 {
        let in_project: Vec<&Service> = matches
            .iter()
            .copied()
            .filter(|service| service.section == Section::Project)
            .collect();
        if !in_project.is_empty() {
            matches = in_project;
        }
    }
    if matches.len() > 1 {
        let windows = input.snapshot.host.os == "windows";
        let here: Vec<&Service> = matches
            .iter()
            .copied()
            .filter(|service| {
                service
                    .worktree
                    .as_deref()
                    .is_some_and(|worktree| within(&input.launch_dir, worktree, windows))
            })
            .collect();
        if here.len() == 1 {
            matches = here;
        }
    }
    match matches.len() {
        0 => Found::None,
        1 => Found::One(matches[0]),
        _ => Found::Many(matches),
    }
}

fn project_of<'a>(input: &'a Input, service: &Service) -> Option<&'a str> {
    let id = service.project_id.as_deref()?;
    input
        .snapshot
        .projects
        .iter()
        .find(|project| project.id == id)
        .map(|project| project.name.as_str())
}

fn qualified_name(input: &Input, service: &Service) -> String {
    let project = project_of(input, service).unwrap_or("(no project)");
    match &service.worktree {
        Some(worktree) => format!(
            "{project}/{}  ({})",
            service.name,
            tilde(worktree, input.home.as_deref())
        ),
        None => format!("{project}/{}", service.name),
    }
}
```

In `crates/harbormap/src/cli.rs`, replace the line `use clap::{Args, Parser, Subcommand};` with:

```rust
use clap::{Args, Parser, Subcommand, ValueEnum};

use hm_core::signature::Level;
```

In `Cli`, add the `--level` flag above `#[command(subcommand)]`, with one blank line after it:

```rust
    /// How much detail explanations give.
    #[arg(long, global = true, value_enum, default_value_t = LevelArg::Builder)]
    pub level: LevelArg,
```

In `Command`, add the `Why` variant below `Status(StatusArgs),`:

```rust
    /// Explain who holds a port or what a service is.
    Why(WhyArgs),
```

Append `WhyArgs`, `LevelArg` and its conversion to `Level` at the end of the file, after one blank line:

```rust
#[derive(Debug, Args)]
pub struct WhyArgs {
    /// A port number, a service name, or project/service.
    pub target: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum LevelArg {
    Beginner,
    Builder,
    Expert,
}

impl From<LevelArg> for Level {
    fn from(level: LevelArg) -> Self {
        match level {
            LevelArg::Beginner => Level::Beginner,
            LevelArg::Builder => Level::Builder,
            LevelArg::Expert => Level::Expert,
        }
    }
}
```

In `crates/harbormap/src/main.rs`, add this line below `mod status;`:

```rust
mod why;
```

In `run`, add this line below `let input = source::load()?;`:

```rust
    let level = cli.level.into();
```

Then add this arm below the `Command::Status(args) => status::run(&input, &args, cli.json),` arm:

```rust
        Command::Why(args) => why::run(&input, &args, cli.json, level),
```

Replace the whole content of `crates/harbormap/src/source.rs` with this version. It is Task 15's file plus what `why` reads: the signatures and card texts for the cards, and the folder Harbormap was started in, to prefer the service of the current worktree:

```rust
//! Where the snapshot comes from: this computer, or — in debug builds only — a
//! recorded scenario named by `HARBORMAP_FIXTURE`, which keeps CLI tests independent
//! of the machine they run on.

use std::path::PathBuf;

use anyhow::Context;

use hm_core::model::Snapshot;
use hm_core::signature::SignatureSet;
use hm_doctor::texts::CardTexts;
use hm_probe::ProbeOptions;
use hm_project::git::RealWorld;
use hm_project::{ResolveInput, World, build_snapshot};

pub struct Input {
    pub snapshot: Snapshot,
    pub signatures: SignatureSet,
    pub texts: CardTexts,
    pub home: Option<PathBuf>,
    /// The folder Harbormap was started in.
    pub launch_dir: PathBuf,
}

pub fn load() -> anyhow::Result<Input> {
    let packs = hm_doctor::packs::load_embedded();
    let (texts, _) = hm_doctor::texts::load_embedded();

    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("HARBORMAP_FIXTURE") {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", PathBuf::from(&path).display()))?;
        let scenario: hm_project::Scenario =
            serde_json::from_str(&text).context("HARBORMAP_FIXTURE is not a scenario file")?;
        let snapshot = build_snapshot(ResolveInput {
            inventory: &scenario.inventory,
            world: &scenario.world,
            signatures: &packs.signatures,
            launch_dir: &scenario.launch_dir,
        });
        return Ok(Input {
            snapshot,
            signatures: packs.signatures,
            texts,
            home: scenario.world.home.clone(),
            launch_dir: scenario.launch_dir,
        });
    }

    let inventory = hm_probe::probe(&ProbeOptions::default());
    let world = RealWorld::new();
    let launch_dir =
        world.canonicalize(&std::env::current_dir().context("cannot read the current folder")?);
    let snapshot = build_snapshot(ResolveInput {
        inventory: &inventory,
        world: &world,
        signatures: &packs.signatures,
        launch_dir: &launch_dir,
    });
    Ok(Input {
        snapshot,
        signatures: packs.signatures,
        texts,
        home: world.home(),
        launch_dir,
    })
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p harbormap --test cli`
Expected: PASS — `test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`

- [ ] **Step 5: Commit**

```bash
git add crates/harbormap/src/why.rs crates/harbormap/src/cli.rs crates/harbormap/src/main.rs crates/harbormap/src/source.rs crates/harbormap/tests/cli.rs
git commit -s -m "feat(cli): harbormap why"
```

- [ ] **Step 6: Try it on your computer**

These commands only read: they list listening ports, processes and containers and change nothing. Make sure `HARBORMAP_FIXTURE` is not set, or the debug build shows the recorded "shop" machine instead. From the repository root, which is a project folder too, run:

```bash
cargo run -p harbormap -- status
cargo run -p harbormap -- status --machine
cargo run -p harbormap -- why 3000 --level beginner
```

To look from one of your own projects, run the same commands in that folder and tell cargo where Harbormap is, for example `cargo run --manifest-path ~/code/harbormap/Cargo.toml -p harbormap -- status`. The output depends on what runs on your computer, so there is no expected output.

### Task 17: Exit-criteria harness

This task proves the first alpha.1 exit criterion (spec §2): a test harness starts a Vite server, a FastAPI server, a .NET server and a compose project with Postgres, and `harbormap status --json` maps every listener to the right project, service and framework. It adds four tiny apps under `tests/e2e/apps`, an ignored integration test that starts them, checks the snapshot and stops everything again, and a CI job that runs it on the three Tier-1 runners. A plain Python server in a second git repository must land under "elsewhere", so the test also checks that Harbormap does not pull other projects into the opened one.

**Files:**
- Create: `tests/e2e/apps/.gitignore`
- Create: `tests/e2e/apps/vite/package.json`, `tests/e2e/apps/vite/index.html`, `tests/e2e/apps/vite/package-lock.json` (generated by npm)
- Create: `tests/e2e/apps/fastapi/pyproject.toml`, `tests/e2e/apps/fastapi/requirements.txt`, `tests/e2e/apps/fastapi/main.py`
- Create: `tests/e2e/apps/dotnet/E2eDotnet.csproj`, `tests/e2e/apps/dotnet/Program.cs`
- Create: `tests/e2e/apps/compose/compose.yaml`
- Create: `crates/harbormap/tests/e2e.rs`
- Modify: `.github/workflows/ci.yml` (add the `e2e` job)

**Interfaces:**
- Consumes:
  - `harbormap status --json` prints the `Snapshot` as JSON (Task 15); `HARBORMAP_FIXTURE` replays a recorded machine in debug builds (Task 15), so the harness removes it from the child's environment.
  - `hm_core::model::{Listener, Section, Service, ServiceSource, Snapshot}` (Task 4): `Snapshot { opened, projects, services, listeners, notes, .. }`, `Service { id, name, signature, section, project_id, worktree, ports, source, .. }`, `ServiceSource::Process { main_pid, root_pid, pids }`, `Listener { addr, port, section, service_id, owner, label, .. }`, `Project { id, name, root, opened, git, .. }`, `Note { code, text }`.
  - Names and signatures from resolution (Task 12) and the packs (Task 9): `e2e-vite` (package.json name, signature `vite`), `e2e-fastapi` (pyproject name, `fastapi`), `E2eDotnet` (csproj file stem, `dotnet`), `db` (compose service, `postgres`), and a bare `python -m http.server` (`python`).
  - The `e2e` job uses the toolchain step and `Swatinem/rust-cache` from Task 1's workflow.
- Produces: `cargo test -p harbormap --test e2e -- --ignored --nocapture` (the exit criterion). `HM_E2E_DOCKER=1` adds the Postgres container; `HM_E2E_PYTHON` names the Python that creates the FastAPI venv and runs the second project (default `python3`, or `python` on Windows). Ports 47173 (Vite), 47800 (FastAPI), 47280 (.NET), 47432 (Postgres) and 47900 (the other project) must be free.

- [ ] **Step 1: Create the Vite app**

Each app is the smallest one its framework runs. Every port is fixed and unusual (47xxx), so the harness never collides with your own dev servers. Dependencies installed by the harness stay out of git.

Create `tests/e2e/apps/.gitignore`:

```gitignore
node_modules/
.venv/
bin/
obj/
__pycache__/
```

Create `tests/e2e/apps/vite/package.json`. The package name `e2e-vite` becomes the service name, and the `dev` script is what the harness starts (`npm run dev`), so the service's process tree runs from npm to Vite:

```json
{
  "name": "e2e-vite",
  "private": true,
  "version": "0.0.0",
  "scripts": {
    "dev": "vite --host 127.0.0.1 --port 47173 --strictPort"
  },
  "devDependencies": {
    "vite": "8.3.4"
  }
}
```

Create `tests/e2e/apps/vite/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>e2e-vite</title>
  </head>
  <body>
    <p>Harbormap end-to-end fixture: Vite.</p>
  </body>
</html>
```

Generate the lockfile, so `npm ci` installs exactly the same packages everywhere. It writes only `package-lock.json`, without installing anything:

Run (in `tests/e2e/apps/vite`): `npm install --package-lock-only --no-audit --no-fund`
Expected: `up to date in …s` and a new `package-lock.json` whose `packages["node_modules/vite"].version` is `8.3.4`.

- [ ] **Step 2: Create the FastAPI app**

Create `tests/e2e/apps/fastapi/pyproject.toml`. Its project name `e2e-fastapi` becomes the service name, and the `fastapi` dependency is what the signature looks for:

```toml
[project]
name = "e2e-fastapi"
version = "0.0.0"
description = "Harbormap end-to-end fixture: FastAPI."
requires-python = ">=3.11"
dependencies = ["fastapi", "uvicorn"]
```

Create `tests/e2e/apps/fastapi/requirements.txt`, the exact versions the harness installs into the app's `.venv`:

```text
# `pip freeze` after `pip install fastapi==0.143.1 uvicorn==0.54.0` (Python 3.13).
annotated-doc==0.0.5
annotated-types==0.8.0
anyio==4.15.1
click==8.5.0
fastapi==0.143.1
h11==0.16.0
idna==3.20
opentelemetry-api==1.45.1
pydantic==2.14.1
pydantic_core==2.50.1
starlette==1.8.0
typing-inspection==0.4.4
typing_extensions==4.16.0
uvicorn==0.54.0
```

Create `tests/e2e/apps/fastapi/main.py`:

```python
from fastapi import FastAPI

app = FastAPI()


@app.get("/")
def root() -> dict[str, str]:
    return {"app": "e2e-fastapi"}
```

- [ ] **Step 3: Create the .NET app**

Create `tests/e2e/apps/dotnet/E2eDotnet.csproj`. The file stem `E2eDotnet` becomes the service name. `RollForward` lets a machine with only a newer .NET runtime run it:

```xml
<Project Sdk="Microsoft.NET.Sdk.Web">

  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <!-- Run on the newest installed runtime when the .NET 8 runtime itself is missing. -->
    <RollForward>Major</RollForward>
    <Nullable>enable</Nullable>
    <ImplicitUsings>enable</ImplicitUsings>
  </PropertyGroup>

</Project>
```

Create `tests/e2e/apps/dotnet/Program.cs`:

```csharp
var app = WebApplication.CreateBuilder(args).Build();

app.MapGet("/", () => "Harbormap end-to-end fixture: .NET");

app.Run();
```

- [ ] **Step 4: Create the compose project**

Create `tests/e2e/apps/compose/compose.yaml`. Postgres publishes 47432 on `127.0.0.1` only, and the health check lets `docker compose up --wait` return once the database accepts connections:

```yaml
# Harbormap end-to-end fixture: a compose project with one Postgres service.
# The password is a throwaway value for this local, short-lived test database.
services:
  db:
    image: postgres:16-alpine
    environment:
      POSTGRES_PASSWORD: hm-e2e-throwaway
    ports:
      - "127.0.0.1:47432:5432"
    healthcheck:
      # Over TCP, so the temporary server that initdb runs (socket only) does not count.
      test: ["CMD", "pg_isready", "-h", "127.0.0.1", "-U", "postgres"]
      interval: 2s
      timeout: 5s
      retries: 30
```

- [ ] **Step 5: Write the harness**

The test is `#[ignore]`d, so `cargo test --workspace` never starts servers; you run it on purpose with `--ignored`. It fails at once if a port is already taken, prepares the apps on the first run (`npm ci` and a venv with the pinned requirements), starts every server in its own process group, waits until each one accepts connections, runs `harbormap status --json` from the repository root and checks every port:

- in the opened project: section `project` for the listener and the service, the expected service name and signature, the worktree that contains the repository, and, for the three host servers, a process tree rooted at the very process the test started (`npm run dev`, not only the `node vite` it runs);
- the other project: section `elsewhere`, signature `python`, and a project whose root is that second repository.

On failure it lists every problem and prints a summary of the whole snapshot. Servers, the compose project (`down -v`) and the temporary folder are removed when the test ends, also after a failed assertion.

Create `crates/harbormap/tests/e2e.rs`:

```rust
//! The alpha.1 exit criterion (v0.1 design §2): a test harness starts a Vite server, a
//! FastAPI server, a .NET server and a compose project with Postgres, and
//! `harbormap status --json` maps every listener to the right project, service and
//! framework. A plain Python server in another git repository must show up under
//! "elsewhere".
//!
//! The apps live in `tests/e2e/apps`. A first run installs `node_modules` and the FastAPI
//! `.venv` when they are missing (network needed), and `dotnet run` restores and builds,
//! so it can take a few minutes. Needs npm, Python 3.11+, the .NET SDK and git on PATH;
//! the Postgres part also needs Docker with compose:
//!
//! ```text
//! cargo test -p harbormap --test e2e -- --ignored --nocapture
//! HM_E2E_DOCKER=1 cargo test -p harbormap --test e2e -- --ignored --nocapture
//! ```
//!
//! `HM_E2E_PYTHON` names the Python that creates the venv and runs the other project
//! (default `python3`, or `python` on Windows).

use std::fmt::Write as _;
use std::fs::File;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hm_core::model::{Listener, Section, Service, ServiceSource, Snapshot};

const VITE: u16 = 47173;
const FASTAPI: u16 = 47800;
const DOTNET: u16 = 47280;
const POSTGRES: u16 = 47432;
const OTHER: u16 = 47900;

/// The first .NET start restores and builds the app.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(180);
const COMPOSE_PROJECT: &str = "hm-e2e";

#[test]
#[ignore = "starts real dev servers; needs npm, Python, the .NET SDK and git (and Docker with HM_E2E_DOCKER=1)"]
fn status_json_maps_dev_servers_to_projects_services_and_frameworks() {
    let repo = canonical(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let apps = repo.join("tests").join("e2e").join("apps");
    let docker = std::env::var("HM_E2E_DOCKER").is_ok_and(|value| value == "1");

    let mut ports = vec![VITE, FASTAPI, DOTNET, OTHER];
    if docker {
        ports.push(POSTGRES);
    }
    let busy: Vec<u16> = ports
        .iter()
        .copied()
        .filter(|port| accepts(*port))
        .collect();
    assert!(
        busy.is_empty(),
        "port(s) {busy:?} are already in use; stop whatever holds them and run again"
    );

    let vite_dir = apps.join("vite");
    let fastapi_dir = apps.join("fastapi");
    let dotnet_dir = apps.join("dotnet");
    prepare_vite(&vite_dir);
    prepare_fastapi(&fastapi_dir);

    // Declared first so it is removed last: the logs and the other project live here.
    let scratch = Scratch::new();
    let logs = scratch.dir("logs");
    let other = scratch.dir("other");
    run(Command::new("git").args(["init", "-q"]), &other);

    let mut vite = Command::new(if cfg!(windows) { "npm.cmd" } else { "npm" });
    vite.args(["run", "dev"]);

    let mut fastapi = Command::new(venv_python(&fastapi_dir));
    fastapi
        .args(["-m", "uvicorn", "main:app", "--host", "127.0.0.1", "--port"])
        .arg(FASTAPI.to_string())
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1");

    let mut dotnet = Command::new("dotnet");
    dotnet
        .args(["run", "--urls"])
        .arg(format!("http://127.0.0.1:{DOTNET}"))
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_GENERATE_ASPNET_CERTIFICATE", "false")
        // Leave no MSBuild worker nodes or compiler server behind after the build.
        .env("MSBUILDDISABLENODEREUSE", "1")
        .env("UseSharedCompilation", "false");

    let mut elsewhere = Command::new(base_python());
    elsewhere
        .args(["-m", "http.server"])
        .arg(OTHER.to_string())
        .args(["--bind", "127.0.0.1"]);

    let mut servers = vec![
        Server::start("vite", VITE, vite, &vite_dir, &logs),
        Server::start("fastapi", FASTAPI, fastapi, &fastapi_dir, &logs),
        Server::start("dotnet", DOTNET, dotnet, &dotnet_dir, &logs),
        Server::start("other", OTHER, elsewhere, &other, &logs),
    ];
    // Compose blocks until Postgres is healthy while .NET builds in the background.
    let compose = docker.then(|| Compose::up(&apps.join("compose").join("compose.yaml")));
    wait_until_listening(&mut servers);
    if compose.is_some() {
        assert!(
            accepts(POSTGRES),
            "Postgres is healthy but port {POSTGRES} refuses connections"
        );
    }

    let snapshot = status_json(&repo);

    let pid_of = |name: &str| {
        servers
            .iter()
            .find(|server| server.name == name)
            .map(|server| server.child.id())
    };
    let mut expected = vec![
        (VITE, "e2e-vite", "vite", pid_of("vite")),
        (FASTAPI, "e2e-fastapi", "fastapi", pid_of("fastapi")),
        (DOTNET, "E2eDotnet", "dotnet", pid_of("dotnet")),
    ];
    if docker {
        expected.push((POSTGRES, "db", "postgres", None));
    }
    let mut problems = Vec::new();
    for (port, name, signature, started) in expected {
        check_in_project(
            &snapshot,
            &repo,
            port,
            name,
            signature,
            started,
            &mut problems,
        );
    }
    check_elsewhere(&snapshot, &other, &mut problems);
    assert!(
        problems.is_empty(),
        "{} problem(s):\n  {}\n\n{}",
        problems.len(),
        problems.join("\n  "),
        summary(&snapshot)
    );
    println!("every listener maps as expected (docker: {docker})");
}

/// A port the opened project must own: listener and service in section "project", with
/// the expected service name and signature, in the worktree this test runs from. For
/// host programs, the service's process tree must start at the process this test
/// started (`npm run dev`, not only the `node vite` it runs).
fn check_in_project(
    snapshot: &Snapshot,
    repo: &Path,
    port: u16,
    name: &str,
    signature: &str,
    started: Option<u32>,
    problems: &mut Vec<String>,
) {
    for (listener, service) in services_on(snapshot, port, problems) {
        let mut wrong = |what: String| problems.push(format!("port {port}: {what}"));
        if service.name != name {
            wrong(format!(
                "service is named {:?}, expected {name:?}",
                service.name
            ));
        }
        if service.signature.as_deref() != Some(signature) {
            wrong(format!(
                "signature is {:?}, expected {signature:?}",
                service.signature
            ));
        }
        if listener.section != Section::Project || service.section != Section::Project {
            wrong(format!(
                "sections are {:?} (listener) and {:?} (service), expected project",
                listener.section, service.section
            ));
        }
        if snapshot.opened.is_none() || service.project_id != snapshot.opened {
            wrong(format!(
                "project is {:?}, expected the opened project {:?}",
                service.project_id, snapshot.opened
            ));
        }
        if !service
            .worktree
            .as_deref()
            .is_some_and(|worktree| repo.starts_with(worktree))
        {
            wrong(format!(
                "worktree is {:?}, expected one that contains {}",
                service.worktree,
                repo.display()
            ));
        }
        if let Some(pid) = started {
            match &service.source {
                ServiceSource::Process { root_pid, .. } if *root_pid == pid => {}
                source => wrong(format!(
                    "service source is {source:?}, expected a process tree rooted at {pid}"
                )),
            }
        }
    }
}

/// The Python server in the other repository: section "elsewhere", signature "python",
/// and a project whose root is that repository.
fn check_elsewhere(snapshot: &Snapshot, root: &Path, problems: &mut Vec<String>) {
    for (listener, service) in services_on(snapshot, OTHER, problems) {
        let mut wrong = |what: String| problems.push(format!("port {OTHER}: {what}"));
        if listener.section != Section::Elsewhere || service.section != Section::Elsewhere {
            wrong(format!(
                "sections are {:?} (listener) and {:?} (service), expected elsewhere",
                listener.section, service.section
            ));
        }
        if service.signature.as_deref() != Some("python") {
            wrong(format!(
                "signature is {:?}, expected \"python\"",
                service.signature
            ));
        }
        let project = service
            .project_id
            .as_ref()
            .and_then(|id| snapshot.projects.iter().find(|project| &project.id == id));
        match project {
            Some(project) if project.root == root && !project.opened => {}
            Some(project) => wrong(format!(
                "project root is {} (opened: {}), expected {}",
                project.root.display(),
                project.opened,
                root.display()
            )),
            None => wrong(format!(
                "project {:?} is not in the snapshot",
                service.project_id
            )),
        }
    }
}

/// Every listener on `port` with its service; a missing listener or service is a problem.
fn services_on<'s>(
    snapshot: &'s Snapshot,
    port: u16,
    problems: &mut Vec<String>,
) -> Vec<(&'s Listener, &'s Service)> {
    let listeners: Vec<&Listener> = snapshot
        .listeners
        .iter()
        .filter(|listener| listener.port == port)
        .collect();
    if listeners.is_empty() {
        problems.push(format!("port {port}: no listener in the snapshot"));
    }
    listeners
        .into_iter()
        .filter_map(|listener| {
            let service = listener
                .service_id
                .as_ref()
                .and_then(|id| snapshot.services.iter().find(|service| &service.id == id));
            if service.is_none() {
                problems.push(format!(
                    "port {port}: no service (owner {:?}, label {:?}, section {:?})",
                    listener.owner, listener.label, listener.section
                ));
            }
            service.map(|service| (listener, service))
        })
        .collect()
}

fn summary(snapshot: &Snapshot) -> String {
    let mut out = format!("Snapshot summary (opened project {:?})\n", snapshot.opened);
    for project in &snapshot.projects {
        let _ = writeln!(
            out,
            "  project {} {:?} root={} opened={} git={}",
            project.id,
            project.name,
            project.root.display(),
            project.opened,
            project.git
        );
    }
    for service in &snapshot.services {
        let _ = writeln!(
            out,
            "  service {} {:?} signature={:?} section={:?} project={:?} ports={:?} source={:?}",
            service.id,
            service.name,
            service.signature,
            service.section,
            service.project_id,
            service.ports,
            service.source
        );
    }
    for listener in &snapshot.listeners {
        let _ = writeln!(
            out,
            "  listener {} section={:?} service={:?} owner={:?} label={:?}",
            SocketAddr::new(listener.addr, listener.port),
            listener.section,
            listener.service_id,
            listener.owner,
            listener.label
        );
    }
    for note in &snapshot.notes {
        let _ = writeln!(out, "  note {}: {}", note.code, note.text);
    }
    out
}

fn status_json(repo: &Path) -> Snapshot {
    let output = Command::new(env!("CARGO_BIN_EXE_harbormap"))
        .args(["status", "--json"])
        .current_dir(repo)
        // Debug builds replay a recorded machine when this is set.
        .env_remove("HARBORMAP_FIXTURE")
        .output()
        .expect("could not run harbormap");
    assert!(
        output.status.success(),
        "harbormap status --json failed ({}):\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "harbormap status --json printed something that is not a snapshot ({error}):\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// `npm ci` unless `node_modules` already holds Vite.
fn prepare_vite(dir: &Path) {
    if !dir
        .join("node_modules")
        .join("vite")
        .join("package.json")
        .is_file()
    {
        let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
        run(
            Command::new(npm).args(["ci", "--no-audit", "--no-fund"]),
            dir,
        );
    }
}

/// A `.venv` with the pinned requirements, unless it already imports FastAPI and Uvicorn.
fn prepare_fastapi(dir: &Path) {
    let python = venv_python(dir);
    if !python.is_file() {
        run(
            Command::new(base_python()).args(["-m", "venv", ".venv"]),
            dir,
        );
    }
    let ready = Command::new(&python)
        .args(["-c", "import fastapi, uvicorn"])
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !ready {
        run(
            Command::new(&python).args([
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "--quiet",
                "-r",
                "requirements.txt",
            ]),
            dir,
        );
    }
}

fn venv_python(app: &Path) -> PathBuf {
    if cfg!(windows) {
        app.join(".venv").join("Scripts").join("python.exe")
    } else {
        app.join(".venv").join("bin").join("python")
    }
}

fn base_python() -> String {
    std::env::var("HM_E2E_PYTHON")
        .unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.to_string())
}

/// Runs a setup command to completion; fails with its output when it fails.
fn run(command: &mut Command, dir: &Path) {
    let output = command
        .current_dir(dir)
        .output()
        .unwrap_or_else(|error| panic!("could not run {command:?}: {error}"));
    assert!(
        output.status.success(),
        "{command:?} failed ({}):\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn accepts(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
}

/// The path as Harbormap stores it: symlinks resolved and, on Windows, without the
/// `\\?\` prefix that `std::fs::canonicalize` adds.
fn canonical(path: &Path) -> PathBuf {
    let path =
        std::fs::canonicalize(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

/// Waits until every server accepts TCP connections; fails early when one exits.
fn wait_until_listening(servers: &mut [Server]) {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut waiting: Vec<&mut Server> = servers.iter_mut().collect();
    while !waiting.is_empty() {
        let mut still = Vec::new();
        for server in waiting {
            if accepts(server.port) {
                println!(
                    "{} listens on {} after {:.1} s",
                    server.name,
                    server.port,
                    server.started.elapsed().as_secs_f64()
                );
            } else if let Ok(Some(status)) = server.child.try_wait() {
                panic!(
                    "{} exited ({status}) before listening on {}\n{}",
                    server.name,
                    server.port,
                    server.log_tail()
                );
            } else {
                still.push(server);
            }
        }
        waiting = still;
        if waiting.is_empty() {
            break;
        }
        if Instant::now() > deadline {
            let tails: Vec<String> = waiting
                .iter()
                .map(|server| {
                    format!(
                        "{} (port {})\n{}",
                        server.name,
                        server.port,
                        server.log_tail()
                    )
                })
                .collect();
            panic!(
                "not listening after {} s:\n{}",
                STARTUP_TIMEOUT.as_secs(),
                tails.join("\n")
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// A started program with its whole process tree, stopped when dropped.
struct Server {
    name: &'static str,
    port: u16,
    child: Child,
    log: PathBuf,
    started: Instant,
}

impl Server {
    fn start(
        name: &'static str,
        port: u16,
        mut command: Command,
        cwd: &Path,
        logs: &Path,
    ) -> Server {
        let log = logs.join(format!("{name}.log"));
        let out = File::create(&log).unwrap_or_else(|error| panic!("{}: {error}", log.display()));
        let err = out.try_clone().expect("could not duplicate the log handle");
        command
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // A group of its own, so the whole tree (npm → sh → node) stops together.
            command.process_group(0);
        }
        let child = command
            .spawn()
            .unwrap_or_else(|error| panic!("could not start {name} ({command:?}): {error}"));
        Server {
            name,
            port,
            child,
            log,
            started: Instant::now(),
        }
    }

    fn log_tail(&self) -> String {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let first = lines.len().saturating_sub(40);
        format!(
            "--- {} ---\n{}",
            self.log.display(),
            lines[first..].join("\n")
        )
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        stop_tree(&mut self.child);
    }
}

/// TERM to the process group, then KILL if it is still there after 10 s.
#[cfg(unix)]
fn stop_tree(child: &mut Child) {
    let group = format!("-{}", child.id());
    if signal_group(&group, "-TERM") && !group_gone(child, &group, Duration::from_secs(10)) {
        signal_group(&group, "-KILL");
        group_gone(child, &group, Duration::from_secs(5));
    }
    let _ = child.wait();
}

#[cfg(unix)]
fn signal_group(group: &str, signal: &str) -> bool {
    Command::new("kill")
        .args([signal, "--", group])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(unix)]
fn group_gone(child: &mut Child, group: &str, grace: Duration) -> bool {
    let deadline = Instant::now() + grace;
    loop {
        // Reap the group leader: as a zombie it would keep the group alive.
        let _ = child.try_wait();
        // `kill -0` succeeds while any process of the group is alive.
        if !signal_group(group, "-0") {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(windows)]
fn stop_tree(child: &mut Child) {
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID"])
        .arg(child.id().to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.wait();
}

/// `docker compose up -d --wait` for the fixture project; `down -v` when dropped.
struct Compose(PathBuf);

impl Compose {
    fn up(file: &Path) -> Compose {
        // Created first, so a failed `up` is cleaned up too.
        let compose = Compose(file.to_path_buf());
        let started = Instant::now();
        let output = compose
            .command(&["up", "-d", "--wait"])
            .output()
            .expect("could not run docker compose");
        assert!(
            output.status.success(),
            "docker compose up failed ({}):\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        println!(
            "postgres is healthy after {:.1} s",
            started.elapsed().as_secs_f64()
        );
        compose
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("docker");
        command
            .args(["compose", "-p", COMPOSE_PROJECT, "-f"])
            .arg(&self.0)
            .args(args);
        command
    }
}

impl Drop for Compose {
    fn drop(&mut self) {
        let _ = self.command(&["down", "-v"]).output();
    }
}

/// A fresh folder under the system temp folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path = std::env::temp_dir().join(format!("hm-e2e-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        Scratch(canonical(&path))
    }

    fn dir(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
```

- [ ] **Step 6: Check that it builds and stays out of the normal test run**

Run: `cargo test -p harbormap --test e2e`
Expected: `test status_json_maps_dev_servers_to_projects_services_and_frameworks ... ignored, starts real dev servers; needs npm, Python, the .NET SDK and git (and Docker with HM_E2E_DOCKER=1)` and `test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out`.

- [ ] **Step 7: Run the harness**

This is the exit criterion. The first run needs the network: it installs the Vite app's `node_modules`, creates the FastAPI `.venv`, and `dotnet run` restores and builds the app, so it can take a few minutes. Later runs take seconds.

Run: `cargo test -p harbormap --test e2e -- --ignored --nocapture`
Expected: PASS. The output shows when each server started listening, then the verdict:

```text
running 1 test
other listens on 47900 after 0.3 s
vite listens on 47173 after 2.3 s
fastapi listens on 47800 after 2.3 s
dotnet listens on 47280 after 13.8 s
every listener maps as expected (docker: false)
test status_json_maps_dev_servers_to_projects_services_and_frameworks ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

The order and the times differ from run to run. If Docker with compose is installed, run it again with Postgres:

Run: `HM_E2E_DOCKER=1 cargo test -p harbormap --test e2e -- --ignored --nocapture`
Expected: PASS, with `postgres is healthy after … s` first and `every listener maps as expected (docker: true)` at the end.

Afterwards nothing is left running: ports 47173, 47800, 47280, 47432 and 47900 are free, and `docker ps -a --filter label=com.docker.compose.project=hm-e2e` lists nothing.

- [ ] **Step 8: See the harness catch a wrong mapping**

A harness that cannot fail proves nothing. In `crates/harbormap/tests/e2e.rs`, temporarily change the expected Vite service name in `(VITE, "e2e-vite", "vite", pid_of("vite"))` to `"wrong-name"`.

Run: `cargo test -p harbormap --test e2e -- --ignored --nocapture`
Expected: FAIL — `1 problem(s):` with `port 47173: service is named "e2e-vite", expected "wrong-name"`, followed by the snapshot summary (`Snapshot summary (opened project Some("p_…"))` and one line per project, service, listener and note). The servers are still stopped afterwards.

Change it back to `"e2e-vite"` and run the same command again: PASS.

- [ ] **Step 9: Run the harness in CI**

The `e2e` job runs after `test` passes, on all three runners. Only the Linux runner can run the Postgres container (macOS runners have no Docker, and Windows runners only run Windows containers), so `HM_E2E_DOCKER=1` is set there only.

Replace the whole content of `.github/workflows/ci.yml` with:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

# A new push to a pull request cancels the older runs of that pull request.
# Pushes to main get their own group (the run id), so they are never cancelled.
concurrency:
  group: ${{ github.workflow }}-${{ github.event.pull_request.number || github.run_id }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}

env:
  CARGO_TERM_COLOR: always

jobs:
  fmt:
    runs-on: ubuntu-24.04
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      # rust-toolchain.toml pins channel 1.99.0 with clippy and rustfmt. Given no
      # toolchain name, `rustup toolchain install` (rustup >= 1.28) installs that
      # toolchain and its components. We do not rely on implicit installation on
      # first use: see https://github.com/rust-lang/rustup/issues/4836.
      - name: Install the pinned Rust toolchain
        run: rustup toolchain install --profile minimal --no-self-update
      - run: cargo fmt --all --check

  test:
    strategy:
      fail-fast: false
      matrix:
        os: [macos-26, ubuntu-24.04, windows-2025]
    runs-on: ${{ matrix.os }}
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - name: Install the pinned Rust toolchain
        run: rustup toolchain install --profile minimal --no-self-update
      # Must come after the toolchain step: the cache key uses the rustc version.
      - uses: Swatinem/rust-cache@v2
        with:
          save-if: ${{ github.ref == 'refs/heads/main' }}
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace

  deny:
    runs-on: ubuntu-24.04
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      # Runs `cargo deny --all-features check` (advisories, bans, licenses and
      # sources) with deny.toml from the repository root.
      - uses: EmbarkStudios/cargo-deny-action@v2

  e2e:
    # Starts real dev servers, so it runs only after the unit tests pass.
    needs: test
    strategy:
      fail-fast: false
      matrix:
        os: [macos-26, ubuntu-24.04, windows-2025]
    runs-on: ${{ matrix.os }}
    timeout-minutes: 45
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - name: Install the pinned Rust toolchain
        run: rustup toolchain install --profile minimal --no-self-update
      - uses: Swatinem/rust-cache@v2
        with:
          save-if: ${{ github.ref == 'refs/heads/main' }}
      - uses: actions/setup-node@v7
        with:
          node-version: "22"
          cache: npm
          cache-dependency-path: tests/e2e/apps/vite/package-lock.json
      - uses: actions/setup-python@v7
        with:
          python-version: "3.12"
          cache: pip
          cache-dependency-path: tests/e2e/apps/fastapi/requirements.txt
      - uses: actions/setup-dotnet@v6
        with:
          dotnet-version: "8.0.x"
      - name: Install the Vite app
        working-directory: tests/e2e/apps/vite
        run: npm ci
      - name: Create the FastAPI virtual environment
        working-directory: tests/e2e/apps/fastapi
        shell: bash
        run: |
          python -m venv .venv
          if [ "$RUNNER_OS" = "Windows" ]; then
            .venv/Scripts/python -m pip install -r requirements.txt
          else
            .venv/bin/python -m pip install -r requirements.txt
          fi
      # Only the Linux runners can run the Postgres (Linux) container: macOS
      # runners have no Docker and Windows runners only run Windows containers.
      # The variable stays unset on the other runners.
      - name: Enable the Docker scenario
        if: runner.os == 'Linux'
        shell: bash
        run: echo "HM_E2E_DOCKER=1" >> "$GITHUB_ENV"
      - run: cargo test -p harbormap --test e2e -- --ignored --nocapture
```

- [ ] **Step 10: Run the workspace checks**

Run: `cargo fmt --all --check`
Expected: no output, exit code 0.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: ``Finished `dev` profile [unoptimized + debuginfo] target(s)`` with no warnings.

Run: `cargo test --workspace`
Expected: PASS — 98 tests pass across the workspace (on Windows 95, because three `#[cfg(unix)]` tests do not exist there), and 2 are ignored: the `child_sleeper` helper in `hm-probe` and this harness.

Run: `git status --short`
Expected: only the files of this task. `node_modules/`, `.venv/`, `bin/` and `obj/` under `tests/e2e/apps` are ignored.

- [ ] **Step 11: Commit**

```bash
git add tests/e2e/apps/.gitignore tests/e2e/apps/vite/package.json tests/e2e/apps/vite/index.html tests/e2e/apps/vite/package-lock.json tests/e2e/apps/fastapi/pyproject.toml tests/e2e/apps/fastapi/requirements.txt tests/e2e/apps/fastapi/main.py tests/e2e/apps/dotnet/E2eDotnet.csproj tests/e2e/apps/dotnet/Program.cs tests/e2e/apps/compose/compose.yaml crates/harbormap/tests/e2e.rs .github/workflows/ci.yml
git commit -s -m "test(e2e): add the alpha.1 exit-criteria harness and its CI job"
```
