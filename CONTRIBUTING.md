# Contributing to Harbormap

Thank you for your interest in contributing to Harbormap! Whether you are fixing a bug, adding language or framework support, improving explanations in `packs/`, or refining the Learn panel UI, we appreciate your help.

Please note that this project follows the [Code of Conduct](CODE_OF_CONDUCT.md). By participating, you agree to uphold it.

---

## Development Setup

### Requirements

- **Rust toolchain:** Pinned via [`rust-toolchain.toml`](rust-toolchain.toml) (`1.99.0` with `clippy` and `rustfmt`). Running `rustup toolchain install --profile minimal --no-self-update` installs the exact version CI uses.
- **Node.js 24+** *(only required when modifying `ui/learn/`)*. End users and Rust-only contributors do not need Node.js.
- **Line endings:** This repository enforces LF (`\n`) line endings via [`.gitattributes`](.gitattributes). Please keep LF endings across platforms.

---

## Workspace Layout

- `crates/harbormap` — CLI binary (`learn`, `index`, `card`, `status`, `why`)
- `crates/hm-index` — Tree-sitter parsers (TypeScript/JavaScript, Go, Python), route extractors, file watcher
- `crates/hm-store` — SQLite facts store
- `crates/hm-layout` — deterministic graph layout and grouping
- `crates/hm-tech` — tech stack detection against `packs/tech/`
- `crates/hm-serve` — local-only Axum server (`127.0.0.1`), token guard, SSE updates, embedded UI
- `crates/hm-probe`, `crates/hm-project`, `crates/hm-doctor`, `crates/hm-core` — runtime doctor & core models
- `ui/learn` — zero-framework TypeScript panel bundled into `ui/learn/dist/`
- `packs/` — tech catalog & explanation packs (CC0-1.0)

---

## Running Checks Locally

Before opening a pull request, run the same checks that CI enforces:

### Rust Checks

```bash
# Formatting
cargo fmt --all --check

# Lints (warnings are denied in CI; workspace forbids unsafe_code)
cargo clippy --locked --workspace --all-targets -- -D warnings

# Unit and integration tests
cargo test --locked --workspace
```

If you have [`cargo-deny`](https://embarkstudios.github.io/cargo-deny/) installed, you can also verify dependency advisories and AGPL-3.0 license compatibility:

```bash
cargo deny --all-features --locked check
```

### Learn Panel UI Checks (when editing `ui/learn/`)

The compiled bundle under `ui/learn/dist/` is committed to git and embedded into the Rust binary via `rust-embed` so users never need Node.js. Whenever you modify files under `ui/learn/src/`, `build.mjs`, `package.json`, `package-lock.json`, or `tsconfig.json`:

```bash
cd ui/learn
npm ci
npm run build
npm run check
npm test
npm run verify
```

Commit the updated `ui/learn/dist/` files alongside your source changes. CI runs `npm run verify` (and `cargo test -p hm-serve --test bundle`) to guarantee `dist/` reproduces the source byte for byte.

---

## Pull Request Guidelines

1. **Keep changes focused:** One logical feature or fix per PR makes reviews fast and clear.
2. **Use clear commit messages:** Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:`, `refactor:`, `test:`) are preferred.
3. **Add tests:** Include unit or integration tests covering new behavior or bug fixes.
4. **Preserve privacy & safety invariants:**
   - Do not add `unsafe` code (`unsafe_code = "forbid"` is set workspace-wide).
   - Keep Harbormap strictly local-first with zero telemetry or silent outbound network requests.
   - Sanitize external strings before writing to terminals or HTML.
5. **No AI tokens for deterministic facts:** Static code facts, routes, tech stack detection, and runtime probing must remain deterministic and offline.

---

## Need Ideas or Have Questions?

- Check existing [Issues](https://github.com/arslanibrahim8/Harbormap/issues) or open a feature request before starting large changes so we can align on design.
- Review the design specs under [`docs/superpowers/specs/`](docs/superpowers/specs/) for architectural context.
