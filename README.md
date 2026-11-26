# Harbormap

[![CI](https://github.com/arslanibrahim8/Harbormap/actions/workflows/ci.yml/badge.svg)](https://github.com/arslanibrahim8/Harbormap/actions/workflows/ci.yml)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Status: v0.1.0-alpha.1](https://img.shields.io/badge/status-v0.1.0--alpha.1-orange.svg)](https://github.com/arslanibrahim8/Harbormap/releases)

**Understand your own project — both the code on disk and the services running on your machine.**

Harbormap is a fast, local-first Rust tool that combines a **live code map** with a **runtime doctor**. It parses your repository (TypeScript/JavaScript, Go, Python), detects routes, entry points, and tech stack with plain-language explanations, inspects running dev servers, ports, and Docker containers, and serves a live browser panel from a single native binary — **with zero AI tokens spent on facts**.

- **Local-first & private** — runs entirely on `127.0.0.1` with a per-run token and zero telemetry. Your source code never leaves your computer.
- **Zero AI tokens for facts** — deterministic Tree-sitter parsing, SQLite storage, and OS process probing mean no hallucinations and no API bill just to understand your repo.
- **Single Rust binary** — the browser panel is embedded directly into the executable; end users never need Node.js or Python installed.
- **Live updates in `< 300ms`** — incremental file watching pushes structured deltas over Server-Sent Events as you (or your coding agents) save files.

---

## Built for Both Vibe-Coders and Engineers

| If you are... | How Harbormap helps |
|---|---|
| **Building with AI / non-programmer** | Open `harbormap learn` to see what your project actually contains in plain English — its parts, routes, and tech stack — or run `harbormap why 5173 --level beginner` to find out what is running on a port without guessing. |
| **Software engineer / tech lead** | Jump into an unfamiliar TS/JS, Go, or Python codebase in seconds, inspect symbol cards (`harbormap card`), monitor multi-worktree dev servers and Docker containers (`harbormap status --machine`), and script against `--json` output. |

---

## What Works Today (`v0.1.0-alpha.1`) vs. In Progress (Wave 2)

We keep this table honest so you know exactly what is live on `main` right now and what is actively being built next.

| Capability | Status | Details |
|---|---|---|
| **`harbormap learn`** | **Live today** | Indexes the project, launches the embedded browser panel on `127.0.0.1` with a per-run token, and streams live incremental updates as files change. |
| **`harbormap index [--watch]`** | **Live today** | Deterministic Tree-sitter indexing for TypeScript/JavaScript, Go, and Python into a local SQLite facts store, with route/entry detection and tech stack recognition. |
| **`harbormap card <target>`** | **Live today** | Inspects what the map knows about any file (`src/orders/service.ts`) or symbol (`src/orders/service.ts#placeOrder`), with `--json` support. |
| **`harbormap status [--machine]`** | **Live today** | Runtime doctor showing running services across git worktrees, listening ports, and Docker containers for your project and (optionally) the rest of your machine. |
| **`harbormap why <target>`** | **Live today** | Explains who owns a port, service name, or `project/service` at three detail levels: `--level beginner`, `builder` (default), or `expert`. |
| **Clean Code Map & Connections / Code Reader** | **In progress (Wave 2)** | Refined visual architecture map, interactive connection tracing, and an integrated source code reader inside the panel. |
| **Zero-Token Journeys** | **In progress (Wave 2)** | Deterministic end-to-end request and user-journey traces derived straight from static routes and call graphs. |
| **Database & Agents Views** | **In progress (Wave 2)** | Dedicated panel tabs for inspecting detected schemas/databases and active AI coding agents. |
| **Optional On-Demand Claude Explanations** | **In progress (Wave 2)** | Strictly opt-in, user-triggered natural-language explanations layered on top of verified deterministic facts. |

---

## Why Harbormap?

| Dimension | Harbormap | LLM-only repo chat | Heavy IDE / SaaS analyzers |
|---|---|---|---|
| **Cost for factual understanding** | **Zero AI tokens** (deterministic Tree-sitter + OS probes) | Burns tokens & context window on every question | Subscription / cloud seats |
| **Privacy** | **100% local-first** (`127.0.0.1`, no cloud, no telemetry) | Sends code snippets to remote APIs | Often uploads code to cloud |
| **Freshness** | **`< 300ms` p95** incremental file-watch updates | Stale snapshots / manual re-indexing | Slow batch scans |
| **Runtime overhead** | **Single native Rust binary** (no Node.js required) | Editor plugins + cloud round-trips | Heavy JVM/Electron runtimes |
| **Runtime awareness** | **Built-in runtime doctor** (ports, processes, git worktrees, Docker) | Blind to running processes | Separate tooling required |

---

## Quickstart

### Prerequisites (Building from Source)

Harbormap targets **Rust 1.99+** (pinned in [`rust-toolchain.toml`](rust-toolchain.toml)). Prebuilt binaries for Apple Silicon macOS (`aarch64-apple-darwin`), Linux x86_64 (`x86_64-unknown-linux-gnu`), and Windows x86_64 (`x86_64-pc-windows-msvc`) will be published on [GitHub Releases](https://github.com/arslanibrahim8/Harbormap/releases) once release tags are cut.

### Install from Source

```bash
git clone https://github.com/arslanibrahim8/Harbormap.git
cd Harbormap
cargo install --path crates/harbormap --locked
```

Or run directly without installing globally:

```bash
cargo run --release --locked -p harbormap -- --help
```

### Everyday Commands

```bash
# Open the live browser map for the current project (watches for file saves)
harbormap learn

# Print the localhost URL without auto-opening a browser
harbormap learn --no-open

# Index the current repository once, or keep the local SQLite facts store updated
harbormap index
harbormap index --watch

# Inspect a specific file or symbol from the map
harbormap card src/orders/service.ts
harbormap card src/orders/service.ts#placeOrder

# See what services, ports, and Docker containers are running for this project
harbormap status
harbormap status --machine

# Ask who holds a port or what a service does (beginner | builder | expert)
harbormap why 5173 --level beginner
harbormap why db
harbormap why shop/db --json
```

> **Tip:** Every command supports `--json` for machine-readable output.

---

## Architecture Overview

Harbormap is structured as a modular Rust workspace with an embedded zero-framework TypeScript UI:

```text
crates/
├── harbormap/   # CLI entrypoint (learn, index, card, status, why)
├── hm-index/    # Tree-sitter incremental indexer (TS/JS, Go, Python), routes & watcher
├── hm-store/    # Local SQLite facts store
├── hm-layout/   # Deterministic graph grouping and layout
├── hm-tech/     # Tech stack detector backed by CC0 packs (packs/tech/)
├── hm-serve/    # Local-only 127.0.0.1 Axum server, auth guard, SSE deltas & embedded UI
├── hm-probe/    # OS listening ports, processes, and Docker container inspection
├── hm-project/  # Git repository and worktree discovery
├── hm-doctor/   # Plain-language runtime explanation engine
└── hm-core/     # Shared types, stable IDs, signatures, and output redaction

ui/learn/        # Zero-framework TypeScript panel (bundled with esbuild into dist/)
packs/           # Technology & explanation signature packs (CC0-1.0)
```

---

## Documentation Map

Design specifications, architecture plans, and background research live in [`docs/`](docs/):

- **Product & Engine Specs** — [`docs/superpowers/specs/`](docs/superpowers/specs/)
  - [Product Design](docs/superpowers/specs/2026-10-15-harbormap-product-design.md)
  - [Runtime Doctor (`v0.1`)](docs/superpowers/specs/2026-10-15-harbormap-v0.1-runtime-doctor-design.md)
  - [Learn Engine Design](docs/superpowers/specs/2026-11-02-harbormap-learn-engine-design.md)
  - [Harbor Chart (`v0.2`)](docs/superpowers/specs/2026-10-15-harbormap-v0.2-harbor-chart-design.md)
- **Implementation Plans** — [`docs/superpowers/plans/`](docs/superpowers/plans/)
- **Visual Understanding Research** — [`docs/research/`](docs/research/)
- **UI Panel Notes** — [`ui/learn/README.md`](ui/learn/README.md)
- **Community & Security** — [`CONTRIBUTING.md`](CONTRIBUTING.md), [`SECURITY.md`](SECURITY.md), [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md)

---

## Contributing

Contributions are welcome! Please read [`CONTRIBUTING.md`](CONTRIBUTING.md) for local setup, formatting/lint rules, UI bundle verification, and pull request guidelines. All participants agree to abide by our [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

---

## License

- **Harbormap source code & binaries:** [GNU Affero General Public License v3.0 (`AGPL-3.0-only`)](LICENSE).
  - **Important clarification:** **Your own analyzed codebase is completely unaffected by Harbormap's AGPL-3.0 license.** Running Harbormap on a proprietary, commercial, or differently licensed project never changes the license of your project or its outputs. AGPL-3.0 only applies if you distribute modified versions of Harbormap itself or run a modified version as a network service for others.
- **Knowledge & signature packs (`packs/`):** Dedicated to the public domain under [CC0 1.0 Universal (`CC0-1.0`)](packs/LICENSE).
