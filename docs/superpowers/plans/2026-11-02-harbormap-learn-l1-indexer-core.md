# Harbormap Learn, Plan L1: Indexer core and TS/JS — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `harbormap index [path] [--watch]` command that walks a project, parses its TypeScript and JavaScript files with tree-sitter, and stores files, symbols, imports and calls with evidence in a per-worktree SQLite store. The store is kept current within 300 ms (p95) of a save, with zero tokens. Alongside it, `harbormap card <path>[#symbol]` prints a facts-only card.

**Architecture:** Two new library crates.
- `hm-store` owns SQLite: connection settings, schema migrations and transactions.
- `hm-index` owns the work: walking files, extracting facts with tree-sitter queries, resolving TS/JS modules and calls, applying incremental changes, and watching files.

Every parsed file's raw facts are stored as JSON, so dependents can be re-resolved without being re-parsed. The CLI gains two subcommands that do not touch the runtime probe.

**Tech Stack:** Rust 1.99.0 (edition 2024).
- New crates: rusqlite 0.41.1 (`bundled`), tree-sitter 0.27.1, tree-sitter-typescript 0.23.2, tree-sitter-javascript 0.25.0, streaming-iterator 0.1.9, ignore 0.4.34, notify 8.2.1, jsonc-parser 0.34.0 (`serde`), directories 6.0.0.
- Existing crates: blake3, serde, serde_json, gix (through `hm-project`), clap, proptest, tempfile, assert_cmd.

**Spec:** `docs/superpowers/specs/2026-11-02-harbormap-learn-engine-design.md`. This plan implements §4 (store location and access, the `index` command), §5.1–5.5 for files, symbols, imports and calls, §5.7 (save-latency rows), §6.1, §6.2 (TS/JS row) and the L1 entry of §14. Principles: `docs/superpowers/specs/2026-10-15-harbormap-product-design.md` §5.

## Where this plan fits

| Plan | Delivers |
|---|---|
| **L1 (this plan)** | `hm-store`, `hm-index`; walker, hashes, TS/JS/TSX facts and resolution, incremental updates, watcher, `harbormap index`, `harbormap card` (facts only), latency harness, eval smoke test |
| L2 | Python, Go, Java, C#, Rust extractors and resolvers; route and entry rules; client → server matching; patterns; `inferred` edges |
| L3 | Journeys and cards (deterministic), glossary packs, eval runner |
| L4 | Claude Code plugin and the prose pipeline |
| L5 | Panel (`hm-layout`, `hm-serve`, TypeScript/SVG UI) |

L2–L5 are outlined in spec §14 and are written after L1 lands, against the real code.

**Scope decisions inside L1** (none removes a spec requirement):
- **Edge kinds.** L1 stores only `imports`, `calls` and `extends`. `reads_env` and `http_calls` come with routes in L2, and `tested_by` with journeys in L3. The table already accepts every kind.
- **Confidence.** L1 creates only `Sure` edges, with provenance `parsed`. A call it cannot pin to one symbol stays a ref. Name and DI matches (`Probably` / `inferred`) arrive in L2, together with the DI rules.
- **No per-folder Merkle hashes.** A branch switch or a watcher overflow is handled by a full rescan with the (size, mtime) precheck. The latency harness measures this against the §5.7 branch-switch row. Merkle hashes are added only if that row fails.
- **Facts store only.** The prose store, routes, journeys and layout tables arrive through later migrations. `hm-store` already carries the migration mechanism.
- **Directories that are never entered.** Dependency folders and build outputs are not walked (`node_modules`, `vendor`, `third_party`, `dist`, `build`, `out`, `.next`, `target`, `bin`, `obj`, `coverage`, `.git`, plus `.gitignore` matches). They are reported as skipped directories with their names. A dependency folder can hold 100k files, so counting them file by file is not worth the time.
- **Generated files inside the repository** (`*.gen.ts`, an `@generated` header and similar) are parsed, because they sit on real request paths (for example a generated API client). They carry class `generated`, so later views can hide them.
- **Files hidden by `.gitignore` or `.harbormapignore` are the user's own choice.** They are not counted (the `ignore` crate does not report them). The report names the ignore files it applied. This narrows the "never silently dropped" rule to files Harbormap itself decides to skip.
- **What lives in `facts_json`, not in columns.** A call's receiver text and argument count are kept in the per-file `facts_json`; a symbol keeps only its lines (`l0`, `l1`), not its byte range (L3 adds bytes if it needs them). Edge rows carry the call-site line. `contains` edges are not stored, because `symbol.file_id` already says which file holds a symbol. Dependency folders are skipped directories, so no file has class `vendored`.
- **Own TS resolver.** L1 writes a small bundler-style resolver (Task 5) instead of `oxc_resolver`. The evaluation repositories need only relative paths, `paths`/`baseUrl` and extension/index probing, and a second resolver crate would add a dependency tree for features L1 does not use. This settles the §6.1 check.
- **Rename links** (an unchanged `content_hash` linking an old id to a new one, §5.3) are needed only when prose exists, so they arrive with the prose store in L4.
- **Symbol id collisions.** Two symbols in one file with the same (owner, name, kind) use their `sig_hash` as disambiguator. If that also collides, the second and later ones in source order get `#2`, `#3`. Ids are therefore unique, and §5.5's `UNIQUE(file_id, owner, name, kind, sig_hash)` is left out on purpose.
- **Readers run one pass.** As spec §4 says, `harbormap card` runs one `full()` pass every time before it reads (cheap with the stat precheck).

## Before you start

- **Toolchain.** Rust is installed with rustup (`rustup toolchain install` inside the repository installs the pinned 1.99.0). On the maintainer's machine Rust is not installed system-wide yet: ask before installing it.
- **C compiler.** tree-sitter grammars and bundled SQLite compile C code. You need Xcode Command Line Tools on macOS, `build-essential` on Linux, or MSVC Build Tools on Windows.
- **Branch.** Work on a branch: `git switch -c learn-l1-indexer`.
- **Commits.** Every commit is signed off: `git commit -s`.
- **After every task**, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` stay clean.

## Global Constraints

- **Toolchain and licences.** Toolchain `1.99.0`, edition `2024`; `unsafe_code = "forbid"`; clippy warnings fail CI. Code is `AGPL-3.0-only`, and new dependencies must pass `deny.toml` (notify is CC0-1.0, `option-ext` under `directories` is MPL-2.0, the rest MIT or MIT/Apache-2.0; all are allowed).
- **Zero tokens, no network.** Nothing in this plan calls a network or an LLM.
- **Read-only on the project.** Nothing is written inside the user's repository, including `.git`. Stores live in the platform data directory under `learn/<worktree_key>/facts.sqlite`; tests override the directory with `HARBORMAP_DATA_DIR`.
- **Ids** (spec §5.3): prefix + 12 hex characters of BLAKE3, repository-relative, no line numbers. Relative paths always use `/`, on every OS.
- **Evidence.** Every edge row stores `file_id`, `line`, `rule`, `prov` and `conf`. `conf` is `hm_core::model::Confidence` serialised as `sure`/`probably`.
- **Hashes.** `content_hash` is BLAKE3 over the raw bytes. `sig_hash` and `body_hash` are BLAKE3; the body hash covers the token stream with comments removed and string literals kept.
- **Watcher.** 100 ms quiet period, 500 ms maximum wait (spec §5.4).
- **Latency targets** (spec §5.7, measured from the file write to the delta; 1k files):

  | Case | p50 / p95 |
  |---|---|
  | Body-only edit | 130 / 300 ms |
  | Signature edit | 180 / 500 ms |
  | Cold index | 2 s |
  | Branch switch with 300 changed files | 1.5 s |

- **No silent drops.** Every file the walker sees is either indexed or counted under a named reason: a class, too large, binary, or a skipped directory. Ignore files are listed by name (scope decisions).
- **Determinism.** The same tree gives the same store rows. Every list a command prints is sorted.
- **No guessing.** A reference that does not resolve to exactly one target is stored as a ref, never as an edge.
- **Not a project.** A path whose root is the home folder, `/` or a drive root is refused, as in P1.

## Review Focus

The spec does not list these inputs, but a person using Harbormap will meet them. Each has a test in the task that owns the code.

1. **A save in the middle of typing** leaves a syntax error. The file keeps the symbols that still parse, its status is `parsed_with_errors`, and the other files' edges into the intact symbols stay. Task 4: `a_syntax_error_keeps_the_symbols_that_parse`. Task 8: `typing_an_error_keeps_edges_into_intact_symbols`.
2. **A file deleted or renamed between the watcher event and the read** (editors' atomic saves) is a delete plus an add, never an error. Task 8: `a_file_that_vanishes_before_reading_is_a_delete`.
3. **A minified bundle, a 5 MB generated file or a binary file with a `.js` name** is counted as `too_large` or `binary`, never parsed and never fatal. Task 3: `minified_and_binary_files_are_counted_not_parsed`.
4. **A `tsconfig.json` with comments and trailing commas, or an `extends` chain that loops**, is read, and the loop stops. Task 5: `tsconfig_with_comments_and_an_extends_loop_loads`.
5. **A project on Windows**, or one opened through a symlinked path, gets the same relative paths and ids as on macOS (`/`-separated). A symlink that points outside the root is not followed. Task 3: `paths_are_slash_separated_and_symlinks_stay_inside`.

---

## File map

| File | Responsibility |
|---|---|
| `Cargo.toml` (root) | New workspace members and dependency versions |
| `crates/hm-core/src/ids.rs` | Add `file_id`, `symbol_id`, `worktree_key` |
| `crates/hm-store/src/lib.rs` | `Store`: open, pragmas, migrations, transactions |
| `crates/hm-store/src/schema/facts_v1.sql` | Facts schema v1 |
| `crates/hm-index/src/lib.rs` | Module list, re-exports |
| `crates/hm-index/src/lang.rs` | `Lang` from a path |
| `crates/hm-index/src/walk.rs` | Walk, classes, skips |
| `crates/hm-index/src/facts.rs` | `FileFacts` and its parts (serde) |
| `crates/hm-index/src/hash.rs` | Content, signature, body and file-level hashes |
| `crates/hm-index/src/ts/mod.rs`, `ts/extract.rs`, `ts/queries/*.scm` | TS/JS/TSX extraction |
| `crates/hm-index/src/ts/tsconfig.rs`, `ts/resolve.rs` | Module, name and call resolution |
| `crates/hm-index/src/index.rs` | `Indexer`: full pass, incremental apply, store writes |
| `crates/hm-index/src/watch.rs` | Debouncer and watcher |
| `crates/hm-index/src/query.rs` | Named edge dump, file card |
| `crates/hm-index/src/location.rs` | Store path per worktree |
| `crates/hm-index/examples/latency.rs`, `crates/hm-index/src/synth.rs` | Synthetic repository and latency harness |
| `crates/harbormap/src/{cli.rs,main.rs,index.rs,card.rs}` | `index` and `card` subcommands |
| `crates/hm-index/tests/fixtures/ts-basic/**` | The shared TS fixture project |
| `tests/eval/repos.toml`, `scripts/eval/fetch.sh`, `crates/hm-index/tests/eval_l1.rs` | Pinned public repositories and the smoke test |

---

### Task 1: Workspace dependencies, repository-relative ids, and `hm-store`

This task adds every new dependency to the workspace at once, so no later task edits the root `Cargo.toml`. It also adds the id constructors and the SQLite store with schema v1.

**Files:**
- Modify: `Cargo.toml` (members are `crates/*` already; add dependencies)
- Modify: `crates/hm-core/src/ids.rs`
- Create: `crates/hm-store/Cargo.toml`, `crates/hm-store/src/lib.rs`, `crates/hm-store/src/schema/facts_v1.sql`
- Test: `crates/hm-store/src/lib.rs` (unit tests), `crates/hm-core/src/ids.rs` (unit tests)

**Interfaces:**
- Produces:
  - Root `[workspace.dependencies]` gains:

    ```toml
    hm-store = { path = "crates/hm-store" }
    hm-index = { path = "crates/hm-index" }
    rusqlite = { version = "0.41.1", features = ["bundled"] }
    tree-sitter = "0.27.1"
    tree-sitter-typescript = "0.23.2"
    tree-sitter-javascript = "0.25.0"
    streaming-iterator = "0.1.9"
    ignore = "0.4.34"
    notify = "8.2.1"
    jsonc-parser = { version = "0.34.0", features = ["serde"] }
    directories = "6.0.0"
    ```

  - `hm_core::ids`:
    - `file_id(rel_path: &str) -> String` hashes `"file\0" + rel_path` and returns `f_…`;
    - `symbol_id(file_id: &str, owner: &str, name: &str, kind: &str, disambiguator: &str) -> String` hashes the parts NUL-separated and returns `y_…`;
    - `worktree_key(canonical_root: &Path) -> String` returns `w_…`.
  - `hm_store`:
    - `pub const FACTS_SCHEMA: u32 = 1;`
    - `pub struct Store` with:
      - `open(path: &Path) -> Result<Store>`, which creates the parent folders;
      - `open_in_memory() -> Result<Store>`;
      - `schema_version(&self) -> Result<u32>`;
      - `conn(&self) -> &rusqlite::Connection`;
      - `write<T>(&mut self, f: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<T>) -> Result<T>`, which commits on `Ok` and rolls back on `Err`.
    - `pub enum StoreError { Sqlite(rusqlite::Error), Io(std::io::Error), NewerSchema { found: u32, supported: u32 } }` with `Display` and `std::error::Error`.
    - `pub type Result<T> = std::result::Result<T, StoreError>;`
  - Schema v1 (`facts_v1.sql`, applied when `PRAGMA user_version` is 0, then set to 1):

    ```sql
    CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE TABLE file(id TEXT PRIMARY KEY, path TEXT NOT NULL UNIQUE, lang TEXT, class TEXT NOT NULL,
      size INTEGER NOT NULL, mtime_ns INTEGER NOT NULL, loc INTEGER NOT NULL, content_hash TEXT NOT NULL,
      interface_hash TEXT, facts_hash TEXT, parse_status TEXT NOT NULL, facts_json TEXT);
    CREATE TABLE symbol(id TEXT PRIMARY KEY, file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE,
      kind TEXT NOT NULL, name TEXT NOT NULL, owner TEXT NOT NULL, sig TEXT NOT NULL, sig_hash TEXT NOT NULL,
      body_hash TEXT NOT NULL, l0 INTEGER NOT NULL, l1 INTEGER NOT NULL, exported INTEGER NOT NULL,
      lines INTEGER NOT NULL, branches INTEGER NOT NULL);
    CREATE INDEX symbol_file ON symbol(file_id);
    CREATE TABLE edge(kind TEXT NOT NULL, src_id TEXT NOT NULL, dst_id TEXT NOT NULL,
      file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE, line INTEGER NOT NULL,
      rule TEXT NOT NULL, prov TEXT NOT NULL, conf TEXT NOT NULL, PRIMARY KEY(kind, src_id, dst_id, line));
    CREATE INDEX edge_dst ON edge(dst_id, kind);
    CREATE INDEX edge_file ON edge(file_id);
    CREATE TABLE ref(file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE, kind TEXT NOT NULL,
      text TEXT NOT NULL, line INTEGER NOT NULL, status TEXT NOT NULL,
      PRIMARY KEY(file_id, kind, text, line));
    CREATE TABLE reverse_import(dst_file_id TEXT NOT NULL, src_file_id TEXT NOT NULL
      REFERENCES file(id) ON DELETE CASCADE, PRIMARY KEY(dst_file_id, src_file_id));
    ```

    Values used across the plan:
    - `file.class`: `source`, `test`, `generated`, `doc`, `infra`.
    - `file.parse_status`: `parsed`, `parsed_with_errors`, `not_supported_yet`, `too_large`, `binary`, `not_parsed`.
    - `ref.status`: `unresolved`, `external`.

- [ ] **Step 1: Write the failing tests**

In `crates/hm-core/src/ids.rs`:

```rust
#[test]
fn learn_ids_are_relative_and_stable() {
    let f = file_id("src/orders/service.ts");
    assert_eq!(f, file_id("src/orders/service.ts"));
    assert_ne!(f, file_id("src/orders/mail.ts"));
    assert!(f.starts_with("f_") && f.len() == 14);
    let y = symbol_id(&f, "OrderService", "find", "method", "");
    assert_eq!(y, symbol_id(&f, "OrderService", "find", "method", ""));
    assert_ne!(y, symbol_id(&f, "OrderService", "find", "method", "sig:abc"));
    assert_ne!(y, symbol_id(&f, "", "find", "method", ""));
    assert!(y.starts_with("y_") && y.len() == 14);
    let w = worktree_key(Path::new("/work/shop"));
    assert!(w.starts_with("w_") && w != worktree_key(Path::new("/work/shop-wt")));
}
```

In `crates/hm-store/src/lib.rs`:
- `fresh_store_has_schema_1_and_wal`: open a store in a tempdir. `schema_version()` is 1, `PRAGMA journal_mode` returns `wal`, `PRAGMA foreign_keys` returns 1, and opening it again keeps version 1 without error.
- `a_newer_schema_is_refused`: set `user_version = 99` on a file, then `open` returns `StoreError::NewerSchema { found: 99, supported: 1 }`.
- `write_rolls_back_on_error`: insert a `meta` row inside `write`, return `Err(rusqlite::Error::QueryReturnedNoRows)`, and check that the row is absent afterwards.
- `deleting_a_file_cascades`: insert a file, a symbol, an edge and a ref, delete the file, and check that all four tables are empty.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p hm-core ids && cargo test -p hm-store`
Expected: compile errors (`file_id` not found; package `hm-store` not found).

- [ ] **Step 3: Implement**

- Add the dependencies above. `crates/hm-store/Cargo.toml` depends on `rusqlite` and has `tempfile` as a dev-dependency.
- `open` sets these pragmas: `journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout=5000`.
- Migrations: a `&[(u32, &str)]` list with `include_str!("schema/facts_v1.sql")` as version 1, applied in a transaction.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p hm-core ids && cargo test -p hm-store`
Expected: PASS. Then run `cargo deny check licenses` if cargo-deny is installed; otherwise CI checks it.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/hm-core/src/ids.rs crates/hm-store
git commit -s -m "feat(store): add the SQLite facts store and repository-relative ids"
```

---

### Task 2: `hm-index` skeleton, `Lang` and the fixture project

**Files:**
- Create: `crates/hm-index/Cargo.toml`, `crates/hm-index/src/lib.rs`, `crates/hm-index/src/lang.rs`
- Create: `crates/hm-index/tests/fixtures/ts-basic/**` (exact contents below)
- Test: `crates/hm-index/src/lang.rs` (unit tests)

**Interfaces:**
- Produces:
  - `hm_index::lang::Lang` with variants `TypeScript, Tsx, JavaScript, Python, Go, Java, CSharp, Rust`.
    - `Lang::from_path(path: &Path) -> Option<Lang>`. Extensions: `ts mts cts` → TypeScript; `tsx` → Tsx; `js jsx mjs cjs` → JavaScript; `py` → Python; `go` → Go; `java` → Java; `cs` → CSharp; `rs` → Rust. `.d.ts` → TypeScript.
    - `Lang::as_str(&self) -> &'static str`, giving `typescript tsx javascript python go java csharp rust`.
    - `Lang::parsed_in_l1(&self) -> bool`: true for TypeScript, Tsx and JavaScript.
  - `crates/hm-index/Cargo.toml` depends on: `hm-core`, `hm-project`, `hm-store`, `rusqlite`, `tree-sitter`, `tree-sitter-typescript`, `tree-sitter-javascript`, `streaming-iterator`, `ignore`, `notify`, `jsonc-parser`, `directories`, `dunce`, `blake3`, `serde`, `serde_json`. Dev-dependencies: `tempfile`, `proptest`.
  - The fixture project `tests/fixtures/ts-basic/`, used by Tasks 3–9 and 11. Its exact files:

    `tsconfig.base.json`:
    ```jsonc
    { "compilerOptions": { "baseUrl": "." } }
    ```
    `tsconfig.json`:
    ```jsonc
    {
      // comments and trailing commas are allowed here
      "extends": "./tsconfig.base.json",
      "compilerOptions": { "paths": { "@/*": ["src/*"], }, },
    }
    ```
    `src/index.ts`:
    ```ts
    import { placeOrder } from "@/orders/service";
    export function main() {
      placeOrder(1);
    }
    ```
    `src/orders/service.ts`:
    ```ts
    import { z } from "zod";
    import { db } from "../db";
    import * as mail from "./mail";

    export function placeOrder(id: number) {
      const order = db.insert(id);
      mail.sendReceipt(order);
      return z.object({}).parse(order);
    }

    export class OrderService {
      static find(id: number) {
        return db.get(id);
      }
      total() {
        return this.sum();
      }
      sum() {
        return 0;
      }
    }
    ```
    `src/orders/mail.ts`:
    ```ts
    export function sendReceipt(order: unknown) {
      return order;
    }
    ```
    `src/orders/index.ts`:
    ```ts
    export * from "./service";
    export { sendReceipt as receipt } from "./mail";
    ```
    `src/db.ts`:
    ```ts
    export const db = {
      insert(id: number) { return { id }; },
      get(id: number) { return { id }; },
    };
    ```
    `src/ui/Button.tsx`:
    ```tsx
    export default function Button() {
      return <button />;
    }
    ```
    `src/ui/Page.tsx`:
    ```tsx
    import Button from "./Button";
    import { OrderService, receipt } from "@/orders";
    export function Page() {
      OrderService.find(2);
      receipt(null);
      return <Button />;
    }
    ```
    `src/legacy.js`:
    ```js
    const { placeOrder } = require("./orders/service");
    module.exports = function run() {
      placeOrder(3);
    };
    ```
    `src/api.gen.ts`:
    ```ts
    // @generated by a code generator. Do not edit.
    export function ping() { return 1; }
    ```
    `src/orders/service.test.ts`:
    ```ts
    import { placeOrder } from "./service";
    placeOrder(9);
    ```
    `src/scripts/tool.py` (`print("not parsed in L1")`), `docs/guide.md` (`# Guide`), `Dockerfile` (`FROM node:22`), `dist/bundle.js` (`var a=1;`), `node_modules/left-pad/index.js` (`module.exports = 1;`), and `.gitignore` containing `ignored/` plus `ignored/secret.ts` (`export const x = 1;`).

    The fixture's own `.gitignore` hides `ignored/`, and the repository's rules may hide `node_modules/` and `dist/`. Commit those fixture files with `git add -f crates/hm-index/tests/fixtures/ts-basic/{ignored/secret.ts,node_modules/left-pad/index.js,dist/bundle.js}`. Then check with `git ls-files crates/hm-index/tests/fixtures/ts-basic` that all 19 fixture files are tracked.

- [ ] **Step 1: Write the failing test** `lang_comes_from_the_extension`. It checks that `a.tsx` → Tsx, `a.d.ts` → TypeScript, `a.cjs` → JavaScript, `a.cs` → CSharp, `Makefile` → None, and that `parsed_in_l1()` is true only for the three JS/TS variants.
- [ ] **Step 2: Run** `cargo test -p hm-index lang`. Expected: FAIL (crate missing).
- [ ] **Step 3: Implement** `lang.rs`. Create the crate and the fixture files.
- [ ] **Step 4: Run** `cargo test -p hm-index lang`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -s -m "feat(index): add the hm-index crate, languages and the TS fixture"`

---

### Task 3: Walker, file classes and skips

**Files:**
- Create: `crates/hm-index/src/walk.rs`
- Create: `crates/hm-index/tests/common/mod.rs` (shared test helpers; later tasks add to it)
- Test: `crates/hm-index/tests/walk.rs`

**Interfaces:**
- Consumes: `Lang` (Task 2).
- Produces:
  - `pub enum FileClass { Source, Test, Generated, Doc, Infra }` with `as_str()` (`source test generated doc infra`).
  - `pub enum Skip { TooLarge, Binary }`.
  - `pub struct WalkedFile { pub rel: String, pub abs: PathBuf, pub size: u64, pub mtime_ns: i64, pub lang: Option<Lang>, pub class: FileClass, pub skip: Option<Skip> }`.
  - `pub struct WalkResult { pub files: Vec<WalkedFile> /* sorted by rel */, pub skipped_dirs: BTreeMap<String, u32> /* dir name → count of such dirs */, pub dirs: Vec<String> /* admitted folders, rel, sorted; "" is the root */, pub ignore_files: Vec<String> /* rel paths of .gitignore/.harbormapignore applied */, pub rules: WalkRules }`.
  - `pub fn walk(root: &Path) -> std::io::Result<WalkResult>`.
  - `pub struct WalkRules` (`Clone`) with `admits(&self, rel: &str) -> bool`. It answers "would the walker include this path?" for one path, without walking: false for a path under a skip directory or `.git`, false for a path matched by any ignore file the walk found (each matcher applies to its own folder and below; use `ignore::gitignore::GitignoreBuilder` per ignore file), true otherwise. Tasks 8 and 9 use it for single paths.
  - `pub fn classify(rel: &str, head: &[u8]) -> FileClass`. `head` is up to the first 1 KiB of the file. A file the walker skips (too large or binary) always gets class `Generated`, whatever `classify` says.
  - `tests/common/mod.rs`: `fixture(name: &str) -> PathBuf` (the folder under `tests/fixtures`), `read_fixture(rel: &str) -> String`, and `copy_fixture(name: &str) -> tempfile::TempDir`, which copies the tree, including the ignored and skipped files.
  - `pub const MAX_PARSE_BYTES: u64 = 1_048_576;`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_fixture_walks_with_classes_and_skips() {
    let r = walk(&fixture("ts-basic")).unwrap();
    let class = |p: &str| r.files.iter().find(|f| f.rel == p).map(|f| f.class);
    assert_eq!(class("src/orders/service.ts"), Some(FileClass::Source));
    assert_eq!(class("src/orders/service.test.ts"), Some(FileClass::Test));
    assert_eq!(class("src/api.gen.ts"), Some(FileClass::Generated));
    assert_eq!(class("docs/guide.md"), Some(FileClass::Doc));
    assert_eq!(class("Dockerfile"), Some(FileClass::Infra));
    assert_eq!(class("src/scripts/tool.py"), Some(FileClass::Source));
    assert!(class("ignored/secret.ts").is_none());          // .gitignore
    assert!(class("dist/bundle.js").is_none());
    assert_eq!(r.skipped_dirs.get("node_modules"), Some(&1));
    assert_eq!(r.skipped_dirs.get("dist"), Some(&1));
    let rels: Vec<_> = r.files.iter().map(|f| f.rel.clone()).collect();
    let mut sorted = rels.clone(); sorted.sort();
    assert_eq!(rels, sorted);
}

#[test]
fn minified_and_binary_files_are_counted_not_parsed() { /* tempdir:
   big.js = 2 MiB of "a=1;" → skip TooLarge;
   app.min.js = one line of 600 kB → skip TooLarge (average line > 500 bytes, checked on the first 64 KiB);
   logo.js = bytes [0x89, b'P', b'N', b'G', 0, 0] → skip Binary (a NUL byte in the first 8 KiB);
   all three appear in files with class Generated and skip Some(..). */ }

#[test]
fn ignore_rules_apply_in_a_copy_without_git() {
    let dir = copy_fixture("ts-basic");                      // a tempdir, not a git repository
    let r = walk(dir.path()).unwrap();
    assert!(!r.files.iter().any(|f| f.rel == "ignored/secret.ts"));
    assert_eq!(r.ignore_files, vec![".gitignore"]);
    assert!(!r.rules.admits("ignored/secret.ts") && !r.rules.admits("node_modules/x.js"));
    assert!(r.rules.admits("src/new.ts"));
}

#[test]
fn paths_are_slash_separated_and_symlinks_stay_inside() { /* tempdir with a/b/c.ts:
   rel is "a/b/c.ts" on every OS;
   on unix, a symlink `out -> /tmp` (outside the root) is not followed, and a symlink
   `inner -> a` inside the root is not walked twice (each file appears once). */ }
```

Classification rules for `classify`:

| Class | Rule |
|---|---|
| Test | Path segment `test`, `tests` or `__tests__`; or a name matching `*.test.*`, `*.spec.*`, `*_test.go`, `test_*.py`, `*Test.java`, `*Tests.cs` |
| Generated | Name `*.gen.*`, `*.g.cs`, `*_pb2.py`, `*.pb.go`, `*.min.js`; or `head` contains `@generated`, `Code generated` or `auto-generated` within the first 5 lines |
| Doc | `md mdx rst txt adoc`, or a `docs/` segment for non-code |
| Infra | `Dockerfile*`, `*compose*.y*ml`, `.github/workflows/*`, `*.tf`, `Procfile`, `fly.toml`, `vercel.json`, `render.yaml` |
| Source | Everything else |

- [ ] **Step 2: Run** `cargo test -p hm-index --test walk`. Expected: FAIL (`walk` missing).
- [ ] **Step 3: Implement.**
  - `ignore::WalkBuilder` with `.hidden(false)` (but `.git` skipped), `git_ignore(true)`, `require_git(false)` (so `.gitignore` applies in folders that are not repositories, such as test copies), `git_global(false)` (a developer's global excludes must not change results), `git_exclude(true)`, `add_custom_ignore_filename(".harbormapignore")`, and `follow_links(false)`.
  - The skip directories listed in the scope decisions are matched by name in `filter_entry` and counted.
  - `rel` is built from path components joined with `/`.
  - `mtime_ns` comes from `metadata.modified()` as nanoseconds since the epoch, or 0 when unavailable.
- [ ] **Step 4: Run** `cargo test -p hm-index --test walk`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -s -m "feat(index): walk files with classes, skips and ignore rules"`

---

### Task 4: Facts model, hashes and the TS/JS extractor

This task also checks the risk named in spec §15: the 2024 TypeScript grammar must load under tree-sitter 0.27.1.

**Files:**
- Create: `crates/hm-index/src/facts.rs`, `crates/hm-index/src/hash.rs`
- Create: `crates/hm-index/src/ts/mod.rs`, `crates/hm-index/src/ts/extract.rs`
- Create: `crates/hm-index/src/ts/queries/symbols.scm`, `imports.scm`, `calls.scm`
- Test: `crates/hm-index/tests/ts_extract.rs`

**Interfaces:**
- Consumes: `Lang`.
- Produces (`facts.rs`; all types `Serialize + Deserialize + Clone + Debug + PartialEq`):

  ```rust
  pub enum SymbolKind { Function, Method, Class, Interface, Variable }   // as_str: function method class interface variable
  pub enum ParseStatus { Parsed, ParsedWithErrors }
  pub struct SymbolFact { pub kind: SymbolKind, pub name: String, pub owner: String /* "" at top level, else class name */,
      pub sig: String, pub sig_hash: String, pub body_hash: String, pub l0: u32, pub l1: u32 /* 1-based, inclusive */,
      pub exported: bool, pub default_export: bool, pub lines: u32, pub branches: u32 }
  pub enum ImportKind { Static, Dynamic, Require, ReExport }
  pub struct ImportedName { pub imported: String /* "default", "*", or a name */, pub local: String }
  pub struct ImportFact { pub specifier: String, pub kind: ImportKind, pub names: Vec<ImportedName>, pub type_only: bool, pub line: u32 }
  pub struct ExportFact { pub name: String /* exported name */, pub local: Option<String>, pub from: Option<String>, pub star: bool, pub line: u32 }
  pub struct CallFact { pub owner: Option<(String, String)> /* (owner, name) of the enclosing symbol; None = top level */,
      pub callee: String, pub receiver: Option<String>, pub args: u32, pub jsx: bool, pub line: u32 }
  pub struct HeritageFact { pub class: String, pub extends: String, pub line: u32 }
  pub struct FileFacts { pub status: ParseStatus, pub symbols: Vec<SymbolFact>, pub imports: Vec<ImportFact>,
      pub exports: Vec<ExportFact>, pub calls: Vec<CallFact>, pub heritage: Vec<HeritageFact> }
  ```

- `hash.rs`:
  - `content_hash(bytes: &[u8]) -> String` (64 hex);
  - `hash_parts(parts: &[&str]) -> String` (BLAKE3, NUL-separated, 32 hex);
  - `body_hash(node: tree_sitter::Node, src: &[u8]) -> String`, over leaf tokens with every `comment` node skipped, tokens joined with `\u{1f}`;
  - `interface_hash(f: &FileFacts) -> String`, over the sorted (exported name, sig_hash) pairs plus the sorted export facts;
  - `facts_hash(f: &FileFacts) -> String`, over the sorted (owner, name, kind, sig_hash) tuples, imports (specifier, names) and exports. It never includes `body_hash`.
- `ts::extract(src: &str, lang: Lang) -> FileFacts`. TypeScript uses `LANGUAGE_TYPESCRIPT`, Tsx uses `LANGUAGE_TSX`, and JavaScript uses `tree_sitter_javascript::LANGUAGE`, which handles JSX.

**Extraction rules (what the queries must catch):**

| Fact | Syntax |
|---|---|
| Function | `function f()`, `export function f()`, `export default function f()`, `const f = () => …`, `const f = function …`, `module.exports = function f()` (name `f`; anonymous gets name `default`) |
| Class, Method | `class C`, its `method_definition`s (owner `C`), and `static` methods (owner `C`). Methods of object literals (`{ insert() {} }`) are not symbols in L1 |
| Interface | `interface I`, `type T = …` (kind Interface) |
| Variable | An exported `const`/`let` that is not a function (for example `export const db = {…}`) |
| Import | `import … from "s"`, `import type …` (`type_only`), `import("s")` with a literal, `require("s")` with a literal (destructured `const { a } = require()` gives the names), `export … from "s"` (ReExport) |
| Export | `export function/class/const`, `export { a as b }`, `export default <identifier>`, `export * from`, `export { a as b } from` |
| Call | `call_expression` with an identifier callee (`f()`) or a member callee (`a.b()` → callee `b`, receiver `a`; `this.b()` → receiver `this`); `new C()` → callee `C`; JSX opening or self-closing element with a capitalised name → callee = name, `jsx = true`; `args` = argument count |
| Heritage | `class A extends B` |
| Branches | Count of `if`, `for*`, `while`, `switch_case`, `catch_clause`, `ternary_expression`, `&&`/`||` inside a symbol |

- **Which functions are symbols.** Only top-level declarations and class members. A function nested inside another function (for example `const login = async () => …` inside a hook) is part of the enclosing symbol, and its calls are attributed to that symbol.
- **Calls that are imports.** `require(...)` and `import(...)` are import facts, never call facts.
- **Receiver text** is the receiver expression's source text with whitespace collapsed, cut to 80 characters (for example `z.object({})` for `z.object({}).parse(x)`).
- **`export *`** gives `ExportFact { name: "*", local: None, from: Some(spec), star: true }`.
- **Order.** Every vector in `FileFacts` is in source order (by start byte).
- `sig` is the declaration text from its start to the start of the body, with whitespace collapsed to single spaces.
- `sig_hash` = `hash_parts(&[kind, owner, name, sig])`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn grammars_load_under_the_pinned_core() {
    for (src, lang) in [("const a = 1 satisfies number;", Lang::TypeScript),
                        ("export default () => <div/>;", Lang::Tsx),
                        ("export const f = (a) => a?.b ?? 1;", Lang::JavaScript)] {
        assert_eq!(extract(src, lang).status, ParseStatus::Parsed, "{lang:?}");
    }
}

#[test]
fn service_ts_yields_its_symbols_imports_and_calls() {
    let f = extract(&read_fixture("ts-basic/src/orders/service.ts"), Lang::TypeScript);
    let names: Vec<_> = f.symbols.iter().map(|s| (s.owner.as_str(), s.name.as_str(), s.kind)).collect();
    assert_eq!(names, vec![("", "placeOrder", SymbolKind::Function), ("", "OrderService", SymbolKind::Class),
        ("OrderService", "find", SymbolKind::Method), ("OrderService", "total", SymbolKind::Method),
        ("OrderService", "sum", SymbolKind::Method)]);
    let place = &f.symbols[0];
    assert_eq!((place.l0, place.l1, place.exported), (5, 9, true));
    let specs: Vec<_> = f.imports.iter().map(|i| i.specifier.as_str()).collect();
    assert_eq!(specs, vec!["zod", "../db", "./mail"]);
    assert_eq!(f.imports[2].names, vec![ImportedName { imported: "*".into(), local: "mail".into() }]);
    let calls: Vec<_> = f.calls.iter().map(|c| (c.callee.as_str(), c.receiver.as_deref(), c.line)).collect();
    assert!(calls.contains(&("insert", Some("db"), 6)));
    assert!(calls.contains(&("sendReceipt", Some("mail"), 7)));
    assert!(calls.contains(&("sum", Some("this"), 16)));
}

#[test]
fn a_comment_edit_keeps_the_body_hash_and_a_literal_edit_changes_it() {
    let a = extract("function f() { return \"x\"; }", Lang::TypeScript);
    let b = extract("function f() { /* note */ return \"x\"; }", Lang::TypeScript);
    let c = extract("function f() { return \"y\"; }", Lang::TypeScript);
    assert_eq!(a.symbols[0].body_hash, b.symbols[0].body_hash);
    assert_ne!(a.symbols[0].body_hash, c.symbols[0].body_hash);
    assert_eq!(a.symbols[0].sig_hash, c.symbols[0].sig_hash);
    assert_eq!(facts_hash(&a), facts_hash(&c));
}

#[test]
fn a_syntax_error_keeps_the_symbols_that_parse() {
    let f = extract("export function ok() { return 1; }\nexport function half( {", Lang::TypeScript);
    assert_eq!(f.status, ParseStatus::ParsedWithErrors);
    assert!(f.symbols.iter().any(|s| s.name == "ok" && s.exported));
}
```

Add these as well:
- `jsx_and_require_and_reexports_are_seen`, on `ui/Page.tsx`, `legacy.js` and `orders/index.ts`:
  - `Page` has a call with callee `Button` and `jsx = true`;
  - `legacy.js` has a `Require` import of `./orders/service` with names `[placeOrder→placeOrder]`, and a function `run`;
  - `orders/index.ts` has `ExportFact { star: true, from: Some("./service") }` and `ExportFact { name: "receipt", local: Some("sendReceipt"), from: Some("./mail") }`.

- [ ] **Step 2: Run** `cargo test -p hm-index --test ts_extract`. Expected: FAIL (`extract` missing). If `grammars_load_under_the_pinned_core` fails with a language-version error after the implementation, stop and report it. The grammar-to-core pairing is a spec risk (§15), and the fix (pinning another tree-sitter core) is a decision for the maintainer.
- [ ] **Step 3: Implement.** Use one `Parser` per thread (`thread_local!`) and compile each `Query` once per language (`OnceLock`). Iterate matches with `streaming_iterator::StreamingIterator`. Find the enclosing symbol of a call by byte-range containment over the symbols already extracted (the innermost one wins).
- [ ] **Step 4: Run** `cargo test -p hm-index --test ts_extract`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -s -m "feat(index): extract TS/JS symbols, imports, exports and calls with tree-sitter"`

---

### Task 5: tsconfig loading and module resolution

**Files:**
- Create: `crates/hm-index/src/ts/tsconfig.rs`, `crates/hm-index/src/ts/resolve.rs` (module part)
- Test: `crates/hm-index/tests/ts_resolve.rs`

**Interfaces:**
- Consumes: `walk::WalkResult` (Task 3) as the set of known relative paths.
- Produces:
  - `pub struct TsConfig { pub base_url: Option<String> /* rel dir, already resolved against the file that set it */, pub paths: Vec<(String, Vec<String>)> /* targets already resolved to rel paths, against baseUrl or else the file that set them */, pub sources: Vec<String> /* every tsconfig file read for this one, extends included */ }`.
  - `pub struct TsConfigs { pub by_dir: BTreeMap<String /* rel dir of a tsconfig.json/jsconfig.json */, TsConfig>, pub notes: Vec<String> }`.
  - `pub fn load_tsconfigs(root: &Path, known: &BTreeSet<String>) -> TsConfigs`. It loads every `tsconfig.json` and `jsconfig.json` in the walked set and follows relative `extends` (at most 10 hops; a seen-set stops loops). `paths` are inherited and overridden as TypeScript does. Package-name `extends` values (for example `@tsconfig/node20`) are not fetched; each one adds a line to `notes` ("tsconfig.json extends @tsconfig/node20, which Harbormap does not read").
  - `pub enum ModuleTarget { File(String), External(String) /* package name */, Unresolved }`.
  - `pub struct ModuleResolver { /* known files, tsconfigs */ }` with:
    - `ModuleResolver::new(known: BTreeSet<String>, tsconfigs: TsConfigs) -> Self`;
    - `config_sources(&self) -> BTreeSet<String>`: every tsconfig file read, extended ones included (Task 8 rebuilds the resolver when one of them changes);
    - `resolve(&self, from_rel: &str, specifier: &str) -> ModuleTarget`.

**Resolution order** (bundler style, which matches the evaluation repositories):
1. A specifier starting with `./` or `../` is joined to the importing file's folder.
2. Otherwise, the nearest ancestor tsconfig's `paths` patterns are tried in order (`*` captures the rest). A specifier that matches a pattern but none of its targets is `Unresolved`, not external: the project claims that prefix.
3. Otherwise, `baseUrl` + specifier.
4. Otherwise, the specifier is external: the package name is the first segment, or the first two when it starts with `@`. A specifier that looks like a package but matches no path is `External`.
5. Candidate probing for a path: exact; then `.ts .tsx .d.ts .js .jsx .mjs .cjs` appended; then `/index` with the same list. A `.js` specifier also tries `.ts`/`.tsx` (ESM-style TS imports).
6. A relative specifier that matches nothing is `Unresolved`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn fixture_specifiers_resolve() {
    let r = resolver_for("ts-basic");
    assert_eq!(r.resolve("src/index.ts", "@/orders/service"), ModuleTarget::File("src/orders/service.ts".into()));
    assert_eq!(r.resolve("src/ui/Page.tsx", "@/orders"), ModuleTarget::File("src/orders/index.ts".into()));
    assert_eq!(r.resolve("src/orders/service.ts", "../db"), ModuleTarget::File("src/db.ts".into()));
    assert_eq!(r.resolve("src/ui/Page.tsx", "./Button"), ModuleTarget::File("src/ui/Button.tsx".into()));
    assert_eq!(r.resolve("src/orders/service.ts", "zod"), ModuleTarget::External("zod".into()));
    assert_eq!(r.resolve("src/x.ts", "@scope/pkg/sub"), ModuleTarget::External("@scope/pkg".into()));
    assert_eq!(r.resolve("src/index.ts", "./missing"), ModuleTarget::Unresolved);
}

#[test]
fn tsconfig_with_comments_and_an_extends_loop_loads() {
    // tempdir: a/tsconfig.json extends ../b/tsconfig.json, which extends ../a/tsconfig.json;
    // a/ sets "paths": {"~/*": ["lib/*"]} with a trailing comma and a // comment.
    // load_tsconfigs returns within the test and a/'s paths contain ("~/*", ["lib/*"]).
}
```

- [ ] **Step 2: Run** `cargo test -p hm-index --test ts_resolve`. Expected: FAIL.
- [ ] **Step 3: Implement.** Parse JSONC with `jsonc_parser::parse_to_serde_value::<serde_json::Value>(text, &Default::default())`; the default options allow comments and trailing commas. Normalise joined paths lexically (resolve `.` and `..`). A path that climbs above the root is `Unresolved`.
- [ ] **Step 4: Run** `cargo test -p hm-index --test ts_resolve`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -s -m "feat(index): resolve TS/JS modules with tsconfig paths and extension probing"`

---

### Task 6: Name and call resolution

**Files:**
- Modify: `crates/hm-index/src/ts/resolve.rs` (name and call part)
- Create: `crates/hm-index/src/query.rs` (`display_name` now; Task 10 adds the cards)
- Modify: `crates/hm-index/tests/common/mod.rs` (add `resolver_for`, `named_edges_for`)
- Test: `crates/hm-index/tests/ts_resolve.rs`

**Interfaces:**
- Consumes: `ModuleResolver` (Task 5), `FileFacts` (Task 4), `hm_core::ids::{file_id, symbol_id}`.
- Produces:
  - `pub struct Project<'a> { pub facts: &'a BTreeMap<String, FileFacts> /* rel → facts */, pub modules: &'a ModuleResolver }`.
  - `pub fn symbol_ids(rel: &str, facts: &FileFacts) -> Vec<String>`, one id per symbol in order, and the only place symbol ids are made: `symbol_id(&file_id(rel), owner, name, kind.as_str(), d)`. `d` is `""` when (owner, name, kind) is unique in the file, otherwise the symbol's `sig_hash`; if that still collides, the second and later ones in source order get `sig_hash + "#2"`, `"#3"` (scope decisions).
  - `pub fn resolve_export(p: &Project, rel: &str, name: &str) -> Option<(String /* rel */, usize /* symbol index */)>`. It follows `export { a as b } from`, `export * from` and `export default <ident>` for at most 5 hops (spec §6.2); `"default"` finds `default_export`.
  - `pub struct ResolvedFile { pub edges: Vec<EdgeRow>, pub refs: Vec<RefRow>, pub imports_files: BTreeSet<String> }`, where:
    - `EdgeRow { kind: &'static str /* imports|calls|extends */, src_id: String, dst_id: String, line: u32, rule: &'static str }`;
    - `RefRow { kind: &'static str /* import|call */, text: String, line: u32, status: &'static str /* unresolved|external */ }`.
  - `pub fn resolve_file(p: &Project, rel: &str) -> ResolvedFile`.

**Rules** (each `rule` id in brackets; every edge is `Sure`/`parsed`):
- **Imports:**
  - a resolved import → an `imports` edge from `file_id(rel)` to `file_id(target)` [`ts.import`];
  - an external import → a ref with status `external` (text = the package name);
  - an unresolved import → a ref with status `unresolved` (text = the specifier);
  - `type_only` imports still create import edges (the file depends on it) but never feed call resolution.
- **Calls.** The `src_id` is the enclosing symbol's id, or the file id at top level. Cases:
  - `f()` where `f` is a top-level symbol in the same file [`ts.call.local`];
  - `f()` where `f` is a named or default import → `resolve_export` → the target symbol [`ts.call.import`];
  - `ns.f()` where `ns` is a namespace import → `resolve_export(target, "f")` [`ts.call.namespace`];
  - `C.m()` where `C` is a local class, or an imported name that resolves to a class → the method `m` owned by `C` in that file [`ts.call.static`];
  - `this.m()` inside a method of class `C` → the method `C.m` in the same file [`ts.call.this`];
  - `new C()` → class `C` [`ts.call.new`];
  - `<C />` → symbol `C` through the same local/import rules [`ts.jsx`];
  - a destructured `require` name → like a named import [`ts.call.require`];
  - a call whose receiver's root identifier is an import of an external package (`z.object()`, `z.object({}).parse()`) → a ref with kind `call`, status `external`, and text `receiver.callee`;
  - anything else (`db.insert()`, `obj.m()`, callbacks) → a ref with kind `call`, status `unresolved`, and text `receiver.callee` or `callee`.
- **Extends:** `class A extends B`, where `B` resolves by the same local/import rules → an `extends` edge [`ts.extends`]; otherwise a ref.
- **Duplicates:** edges with the same (kind, src, dst, line) appear once. All output vectors are sorted.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn fixture_calls_resolve_to_the_right_symbols() {
    let names = named_edges_for("ts-basic"); // helper: resolve every file, map ids back to "path#Owner.name"
    for e in [
        ("calls", "src/index.ts#main", "src/orders/service.ts#placeOrder"),
        ("calls", "src/orders/service.ts#placeOrder", "src/orders/mail.ts#sendReceipt"),
        ("calls", "src/ui/Page.tsx#Page", "src/orders/service.ts#OrderService.find"),   // via `export *`
        ("calls", "src/ui/Page.tsx#Page", "src/orders/mail.ts#sendReceipt"),            // via alias `receipt`
        ("calls", "src/ui/Page.tsx#Page", "src/ui/Button.tsx#Button"),                  // JSX, default import
        ("calls", "src/orders/service.ts#OrderService.total", "src/orders/service.ts#OrderService.sum"),
        ("calls", "src/legacy.js#run", "src/orders/service.ts#placeOrder"),
        ("calls", "src/orders/service.test.ts", "src/orders/service.ts#placeOrder"),    // top level
        ("imports", "src/ui/Page.tsx", "src/orders/index.ts"),
        ("imports", "src/orders/index.ts", "src/orders/mail.ts"),
    ] { assert!(names.edges.contains(&(e.0.into(), e.1.into(), e.2.into())), "missing {e:?}"); }
    assert!(names.refs.contains(&("src/orders/service.ts".into(), "call".into(), "db.insert".into(), "unresolved".into())));
    assert!(names.refs.contains(&("src/orders/service.ts".into(), "import".into(), "zod".into(), "external".into())));
    assert!(!names.edges.iter().any(|(_, _, dst)| dst.contains("db.ts#insert")));   // no guessing
}

#[test]
fn a_reexport_loop_stops() { /* a.ts: export * from "./b"; b.ts: export * from "./a";
   resolve_export(.., "a.ts", "nothing") returns None without hanging */ }
```

- [ ] **Step 2: Run** `cargo test -p hm-index --test ts_resolve`. Expected: FAIL.
- [ ] **Step 3: Implement** as specified. `query::display_name(rel: &str, sym: Option<&SymbolFact>) -> String` gives `rel`, `rel#Owner.name` or `rel#name`. `named_edges_for(name)` returns `struct Named { edges: BTreeSet<(String, String, String)> /* kind, src, dst */, refs: BTreeSet<(String, String, String, String)> /* file, kind, text, status */ }`.
- [ ] **Step 4: Run** `cargo test -p hm-index --test ts_resolve`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -s -m "feat(index): resolve TS/JS names, calls and re-exports without guessing"`

---

### Task 7: `Indexer` full pass and store writes

**Files:**
- Create: `crates/hm-index/src/index.rs`
- Test: `crates/hm-index/tests/index_full.rs`

**Interfaces:**
- Consumes: `walk` (Task 3), `ts::extract` (Task 4), `ModuleResolver`/`load_tsconfigs` (Task 5), `resolve_file`/`symbol_ids` (Task 6), `hm_store::Store` (Task 1).
- Produces:
  - `pub struct Indexer { root: PathBuf, store: Store, /* in-memory: facts map, resolver */ }`.
  - `Indexer::open(root: &Path, store: Store) -> Result<Indexer>`. It loads existing rows into memory: the facts map from `file.facts_json`, and the resolver from the known paths.
  - `Indexer::store(&self) -> &Store` (read access for queries and tests).
  - `Indexer::full(&mut self) -> Result<IndexReport>`. It walks, then (size, mtime) prechecks against the stored rows; changed files are hashed, and parsed only when their `content_hash` differs. All files are resolved, and everything is written in one transaction. A cold store parses every file.
  - `pub struct IndexReport { pub files: BTreeMap<String /* lang or "other" */, u32>, pub classes: BTreeMap<String, u32>, pub skipped: BTreeMap<String /* too_large|binary|dir:<name> */, u32>, pub hashed: u32 /* files whose bytes were hashed */, pub parsed: u32, pub parsed_with_errors: u32, pub not_supported_yet: u32, pub symbols: u32, pub edges: BTreeMap<String, u32>, pub refs: BTreeMap<String, u32>, pub took_ms: u64 }` (Serialize).
  - `pub type Result<T> = std::result::Result<T, IndexError>`, with `IndexError { Store(StoreError), Io(std::io::Error), Sqlite(rusqlite::Error) }`, `Display` and `From` impls.
  - **Writes per parsed file:**
    - `file` row: `facts_json` = `serde_json::to_string(&FileFacts)`, plus `interface_hash` and `facts_hash`;
    - `symbol` rows from `symbol_ids`;
    - `edge`, `ref` and `reverse_import` rows from `resolve_file`. `reverse_import(dst, src)` exists for each `imports_files` entry.
  - **Files that are not parsed** get a `file` row with `parse_status`:
    - `not_supported_yet`: a file with a `Lang` that L1 does not parse (Python, Go, Java, C#, Rust);
    - `not_parsed`: a file with no `Lang` (JSON, `.gitignore`, images), and every doc or infra file;
    - `too_large` or `binary`: a walker skip;

  - The `meta` table stores `indexed_at` (Unix seconds) and `root`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn full_index_of_the_fixture_reports_and_stores() {
    let (mut ix, _dir) = indexer_for("ts-basic");          // copies the fixture into a tempdir, Store::open_in_memory
    let r = ix.full().unwrap();
    assert_eq!(r.parsed, 10);                              // 7 .ts + 2 .tsx + 1 .js under src (gitignored and skipped dirs excluded)
    assert_eq!(r.not_supported_yet, 1);                    // tool.py
    assert_eq!(r.skipped.get("dir:node_modules"), Some(&1));
    assert_eq!(r.classes.get("test"), Some(&1));
    assert_eq!(r.edges.get("calls"), Some(&8));
    let again = ix.full().unwrap();                        // nothing changed: nothing re-parsed
    assert_eq!(again.parsed, 0);
    assert_eq!(dump(ix.store()), dump(fresh_index("ts-basic").store()));
}

#[test]
fn an_unchanged_file_with_a_new_mtime_is_hashed_not_parsed() {
    // set src/db.ts's mtime to now + 10 s with std::fs::File::set_modified; full():
    // hashed == 1, parsed == 0, and the dump is unchanged.
}
```

`dump(&Store) -> String` is a test helper in `tests/common/mod.rs`. It prints every row of `file` (path, class, parse_status, content_hash, interface_hash, facts_hash), `symbol` (all columns except `id`, plus the display name), `edge` (display names), `ref` and `reverse_import`, sorted. Tasks 8 and 12 reuse it. The expected `10` and `8` must match the fixture exactly. If your count differs, recount from the fixture listing in Task 2 and fix the code, not the number, unless the fixture listing itself is wrong; in that case fix both and say so in the commit.

- [ ] **Step 2: Run** `cargo test -p hm-index --test index_full`. Expected: FAIL.
Also add to `tests/common/mod.rs`: `indexer_for(name) -> (Indexer, TempDir)` (copy + in-memory store), `fresh_index(name) -> Indexer`, `fresh_index_of(dir: &Path) -> Indexer`, `indexed(name) -> (Indexer, TempDir)` (after `full()`), `edit(dir, rel, from, to)` and `set([...]) -> BTreeSet<String>`. Return the `TempDir` last in tuples and drop the `Indexer` before it: Windows cannot delete a folder while SQLite or the watcher holds a handle in it.

- [ ] **Step 3: Implement.** Parse in parallel with `std::thread::scope` across `available_parallelism()` chunks; extraction is pure, so this is safe. Do all writes on one thread in one `Store::write`.
- [ ] **Step 4: Run** `cargo test -p hm-index --test index_full`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -s -m "feat(index): index a project into the facts store in one pass"`

---

### Task 8: Incremental apply and dirty propagation

**Files:**
- Modify: `crates/hm-index/src/index.rs`
- Test: `crates/hm-index/tests/index_incremental.rs`

**Interfaces:**
- Consumes: Task 7.
- Produces:
  - `Indexer::apply(&mut self, changed: &BTreeSet<String> /* rel paths */) -> Result<Delta>`.
  - `Indexer::rescan(&mut self) -> Result<Delta>`. It equals `full()`, but returns a `Delta`.
  - `pub struct Delta { pub files_changed: Vec<String>, pub files_removed: Vec<String>, pub interface_changed: Vec<String>, pub reresolved: Vec<String>, pub symbols_added: Vec<String>, pub symbols_removed: Vec<String>, pub symbols_body_changed: Vec<String>, pub symbols_sig_changed: Vec<String>, pub edges_added: u32, pub edges_removed: u32 }` (Serialize, sorted vectors, ids). L3 uses the `symbols_*` lists to mark journeys and cards.

**Algorithm** (one `Store::write` per call; spec §5.4 steps 1–5):
1. For each path that `WalkRules::admits` (others are ignored):
   - A path that no longer exists is a removal: delete its `file` row (cascades).
   - A path that exists is always hashed: no (size, mtime) precheck inside `apply`, because coarse mtimes miss fast same-size saves. If `content_hash` is equal, stop.
   - Otherwise re-parse it and diff its symbols by id: added, removed, `sig_hash` changed, `body_hash` changed.
2. Write the new `file` and `symbol` rows. Re-resolve the file itself, replacing its own `edge`, `ref` and `reverse_import` rows.
3. **Dependents.** Build the re-resolve set:
   - If a file was **added or removed**, every file is re-resolved (module targets can change anywhere: `orders.ts` beats `orders/index.ts`, and a new `<baseUrl>/zod.ts` replaces an external).
   - Otherwise, if `interface_hash` changed, take the importers of the file (`reverse_import`). For each importer that re-exports from it (`ExportFact.from` resolving to that file), also take that importer's importers. Repeat up to 5 hops (spec §6.2).
   - A change to any file in `ModuleResolver::config_sources()` rebuilds the resolver, and every file is re-resolved. Nothing is re-parsed.
4. Re-resolve every file in the set from its stored `facts_json`. Compare with its stored outgoing rows and write only the differences, so a full re-resolve stays cheap on 5k files.
5. `Delta.reresolved` lists the re-resolved files, without the files that were themselves changed or removed.

Rename normalisation lives in the watcher (Task 9): a rename arrives here as a removal plus an addition.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_body_edit_reparses_one_file_and_touches_no_importer() {
    let (mut ix, dir) = indexed("ts-basic");
    edit(&dir, "src/orders/mail.ts", "return order;", "return { order };");
    let d = ix.apply(&set(["src/orders/mail.ts"])).unwrap();
    assert_eq!(d.files_changed, vec!["src/orders/mail.ts"]);
    assert!(d.interface_changed.is_empty() && d.reresolved.is_empty());
    assert_eq!(d.symbols_body_changed.len(), 1);
    assert_eq!(dump(ix.store()), dump(fresh_index_of(&dir).store()));
}

#[test]
fn renaming_an_export_reresolves_importers() {
    // rename sendReceipt → sendMail in mail.ts only: service.ts and orders/index.ts are re-resolved, and so is
    // ui/Page.tsx (it reaches sendReceipt through the orders/index.ts re-export). The edges placeOrder → sendReceipt
    // and Page → sendReceipt disappear; refs "mail.sendReceipt" and "receipt" (unresolved) appear;
    // no edge row has a dst_id that is not a symbol or file id in the store.
}

#[test]
fn adding_the_missing_file_resolves_a_waiting_import() {
    // add `import { x } from "./later"` to index.ts (ref unresolved), then create src/later.ts exporting x:
    // apply(["src/later.ts"]) adds the imports edge index.ts → later.ts.
}

#[test]
fn a_file_that_vanishes_before_reading_is_a_delete() {
    // delete src/db.ts, then apply(["src/db.ts"]): Ok, files_removed == ["src/db.ts"],
    // service.ts is re-resolved and its "../db" import becomes an unresolved ref.
}

#[test]
fn typing_an_error_keeps_edges_into_intact_symbols() {
    // append "\nexport function half( {" to mail.ts; apply: parse_status parsed_with_errors,
    // edge placeOrder → sendReceipt still present.
}

#[test]
fn a_tsconfig_change_reresolves_without_reparsing() { /* change "@/*" to "src/orders/*"; apply(["tsconfig.json"]);
   files_changed is ["tsconfig.json"] only; index.ts's "@/orders/service" becomes unresolved */ }
```

Then the property test, in `tests/index_property.rs`:

```rust
proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]
    #[test]
    fn incremental_equals_cold(edits in vec(edit_strategy(), 1..12)) {
        // Start from synth::project(seed=7, files=8) (Task 11 adds synth; until then use a small inline generator).
        // Each edit is one of the synth::Edit names: BodyChange, SignatureChange, Rename, AddImport, RemoveImport,
        // AddFile, DeleteFile, BreakSyntax; the project has barrels, so Rename exercises re-export propagation.
        // After applying each edit to disk and calling apply(changed): dump(ix) == dump(cold index of the same tree).
    }
}
```

- [ ] **Step 2: Run** `cargo test -p hm-index --test index_incremental --test index_property`. Expected: FAIL.
- [ ] **Step 3: Implement** the algorithm.
- [ ] **Step 4: Run** the same command. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -s -m "feat(index): apply saves incrementally and keep the store equal to a cold index"`

---

### Task 9: Debouncer and watcher

**Files:**
- Create: `crates/hm-index/src/watch.rs`
- Test: `crates/hm-index/src/watch.rs` (debouncer unit tests), `crates/hm-index/tests/watch.rs`

**Interfaces:**
- Consumes: `walk` rules (to drop ignored paths); `Indexer::apply`/`rescan` (by the caller).
- Produces:
  - `pub struct Batch { pub paths: BTreeSet<String> /* rel */, pub rescan: bool, pub first_event: Instant }`.
  - `pub struct Debouncer { quiet: Duration, max_wait: Duration, /* pending */ }` with:
    - `new(quiet, max_wait)`;
    - `push(&mut self, now: Instant, rel: String)`;
    - `push_rescan(&mut self, now: Instant)`;
    - `poll(&mut self, now: Instant) -> Option<Batch>`;
    - `next_deadline(&self) -> Option<Instant>`.
  - `pub const QUIET: Duration = Duration::from_millis(100); pub const MAX_WAIT: Duration = Duration::from_millis(500);`
  - `pub struct Watcher` with:
    - `Watcher::start(root: &Path, rules: WalkRules, dirs: &[String] /* admitted folders from the walk */) -> std::io::Result<Watcher>`. It canonicalises `root` with `dunce::canonicalize` (macOS `/private/var`, Windows `\\?\` prefixes) and runs a thread with the debouncer. On macOS and Windows it watches the root recursively (one stream). On Linux it watches each admitted folder non-recursively and adds a watch when an admitted folder is created, so `node_modules` never uses inotify watches.;
    - `Watcher::recv_timeout(&self, d: Duration) -> Option<Batch>`.
  - **Event handling:**
    - every event path is turned into `rel` against the canonical root; paths that `WalkRules::admits` rejects are dropped;
    - rename events add both the old and the new path;
    - an event with `need_rescan()`, or an error from notify, calls `push_rescan`.

- [ ] **Step 1: Write the failing tests**

Unit tests (pure, no timing flakiness):

```rust
#[test]
fn quiet_period_flushes_after_100ms_of_silence() {
    let t0 = Instant::now(); let mut d = Debouncer::new(QUIET, MAX_WAIT);
    d.push(t0, "a.ts".into()); d.push(t0 + ms(60), "b.ts".into());
    assert!(d.poll(t0 + ms(150)).is_none());              // only 90 ms since the last event
    let b = d.poll(t0 + ms(161)).unwrap();
    assert_eq!(b.paths, set(["a.ts", "b.ts"])); assert!(!b.rescan);
}

#[test]
fn a_steady_stream_flushes_at_500ms() {
    let t0 = Instant::now(); let mut d = Debouncer::new(QUIET, MAX_WAIT);
    for i in 0..17 { d.push(t0 + ms(i * 30), format!("f{i}.ts")); }   // last push at 480 ms
    assert!(d.poll(t0 + ms(499)).is_none());
    assert!(d.poll(t0 + ms(500)).is_some());
}
```

Integration test (`tests/watch.rs`, tolerant timing): start a `Watcher` on a tempdir copy of the fixture, write `src/db.ts`, and expect `recv_timeout(2 s)` to return a batch that contains `src/db.ts`. Then write `node_modules/left-pad/x.js` and collect batches for 600 ms: none of them may contain a `node_modules` path. (Late duplicate events for `src/db.ts`, common with macOS FSEvents, are allowed.) Drop the `Watcher` before the `TempDir`.

- [ ] **Step 2: Run** `cargo test -p hm-index watch`. Expected: FAIL.
- [ ] **Step 3: Implement.** The watcher thread blocks on the notify channel with a timeout equal to the debouncer's next deadline.
- [ ] **Step 4: Run** `cargo test -p hm-index watch`. Expected: PASS on macOS, Linux and Windows (CI).
- [ ] **Step 5: Commit** `git commit -s -m "feat(index): watch files with a 100 ms quiet period and a 500 ms cap"`

---

### Task 10: Store location, `harbormap index` and `harbormap card`

**Files:**
- Create: `crates/hm-index/src/location.rs`, `crates/hm-index/src/query.rs` (card part)
- Create: `crates/harbormap/src/index.rs`, `crates/harbormap/src/card.rs`
- Modify: `crates/harbormap/Cargo.toml` (add `hm-index`, `hm-store`), `crates/harbormap/src/cli.rs`, `crates/harbormap/src/main.rs`
- Test: `crates/harbormap/tests/learn_cli.rs`, `crates/hm-index/tests/card.rs`

**Interfaces:**
- Consumes: `Indexer`, `Watcher`, `hm_project::git::discover`, `hm_core::ids::worktree_key`.
- Produces:
  - `location::worktree_root(path: &Path) -> Result<PathBuf, LocationError>`:
    - the worktree from `discover(path)` whose path contains `path`, or else `dunce::canonicalize(path)`;
    - `LocationError::NotAProject(PathBuf)` when `hm_project::paths::is_boundary(root, RealWorld::new().home().as_deref(), cfg!(windows))` is true (home folder, `/` or a drive root; `RealWorld::home()` is already canonical). Its `Display` is `<path> is not a project folder (it is your home folder or a drive root)`.
  - `location::facts_db(worktree_root: &Path) -> PathBuf`, which gives `<data>/learn/<worktree_key>/facts.sqlite`. `<data>` is `$HARBORMAP_DATA_DIR` when that is set, otherwise `directories::ProjectDirs::from("", "", "harbormap").data_dir()`.
  - `query::file_card(store: &Store, rel: &str) -> Result<Option<FileCard>>` with:

    ```rust
    pub struct FileCard { pub path: String, pub lang: Option<String>, pub class: String, pub parse_status: String, pub lines: u32,
        pub symbols: Vec<SymbolRow>, pub used_by: Vec<String> /* importer paths: source before test, then by path */,
        pub uses: Vec<String> /* imported paths, then "pkg (external)" */, pub unresolved: u32 }
    pub struct SymbolRow { pub name: String /* Owner.name or name */, pub kind: String, pub l0: u32, pub l1: u32,
        pub exported: bool, pub called_from: u32 /* distinct caller symbols or files */ }
    ```

  - `query::symbol_card(store: &Store, rel: &str, symbol: &str) -> Result<Option<SymbolCard>>` with `SymbolCard { name, kind, path, l0, l1, sig, callers: Vec<String>, callees: Vec<String>, unresolved_calls: Vec<String> }`. Callers and callees are display names, sorted.
  - CLI:
    - `Command::Index(IndexArgs { path: Option<PathBuf>, watch: bool })` and `Command::Card(CardArgs { target: String /* path or path#Symbol */ })`.
    - `main.rs` loads the probe source only for `Status` and `Why`.
    - `index` prints a short text report, or `IndexReport` as JSON with `--json`:

      ```
      Indexed ts-basic (16 files)
        TypeScript 7 · TSX 2 · JavaScript 1 · not yet supported: Python 1
        Tests 1 · Generated 1 · Docs 1 · Infra 1
        Not on this map: node_modules (1 folder), dist (1 folder)
        Symbols 12 · imports 9 · calls 8 · unresolved 2 · external 3
        Took 41 ms
      ```

      Numbers come from the report; the wording is fixed in code (product spec §5: CLI framing text).
    - `index --watch`, after the first report, prints one line per batch, for example `src/db.ts · 1 file · 7 ms · +0 −0 edges`, until Ctrl-C.
    - `card src/orders/service.ts` prints the card; `card src/orders/service.ts#placeOrder` prints the symbol card; `--json` prints the struct. The path is relative to the current folder (or absolute) and is turned into a worktree-relative path. When the worktree has no store yet, `card` runs one `full()` pass first (spec §4). A path that is not in the index exits 1 with `harbormap: src/x.ts is not in the index (it may be ignored, skipped or not saved yet)`.

- [ ] **Step 1: Write the failing tests**
  - `crates/hm-index/tests/card.rs`:
    - `service_card_lists_symbols_callers_and_users`. The `placeOrder` row has `called_from == 3` (main, run, the test file's top level). `used_by` starts with `src/index.ts` and lists `src/orders/service.test.ts` last. `uses` contains `src/db.ts`, `src/orders/mail.ts` and `zod (external)`.
    - `symbol_card_shows_callers_and_unresolved_calls`. `placeOrder` has callers `src/index.ts#main`, `src/legacy.js#run`, `src/orders/service.test.ts`; callees `src/orders/mail.ts#sendReceipt`; and `unresolved_calls` contains `db.insert`.
  - `crates/harbormap/tests/learn_cli.rs` (assert_cmd, `HARBORMAP_DATA_DIR` set to a tempdir, the fixture copied to a tempdir):
    - `index_prints_counts_and_writes_the_store_outside_the_project`. Exit 0; stdout contains `TypeScript 7 · TSX 2 · JavaScript 1` and `node_modules (1 folder)`; the store file exists under `HARBORMAP_DATA_DIR/learn/w_*/facts.sqlite`; and the project tree is byte-identical before and after (compare a sorted list of (path, size, content hash)).
    - `index_json_is_the_report`. `--json` parses as JSON with `parsed == 10`.
    - `card_without_an_index_indexes_first`. With an empty data dir, `card src/orders/service.ts` exits 0 and prints the `placeOrder` row; `card src/nope.ts` exits 1 with the sentence above.
    - `index_refuses_the_home_folder`. `HOME` and `USERPROFILE` are both set to a tempdir (Windows reads `USERPROFILE`) and the command is run with that path. Exit 1, and stderr contains `is not a project folder`.
    - `status_still_works`. `harbormap status --json` with `HARBORMAP_FIXTURE` set to one of the scenarios that `tests/cli.rs` already uses still exits 0. This is a regression guard for the `main.rs` change that does not probe the CI machine.

- [ ] **Step 2: Run** `cargo test -p hm-index --test card && cargo test -p harbormap --test learn_cli`. Expected: FAIL.
- [ ] **Step 3: Implement.** The text renderers live in `crates/harbormap/src/{index,card}.rs` and follow `status.rs`.
- [ ] **Step 4: Run** the same command, plus `cargo test -p harbormap`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -s -m "feat(cli): add harbormap index and harbormap card"`

---

### Task 11: Synthetic repositories and the latency harness

**Files:**
- Create: `crates/hm-index/src/synth.rs` (`#[doc(hidden)] pub mod synth`, always compiled, so integration tests and the example can use it)
- Create: `crates/hm-index/examples/latency.rs`
- Create: `crates/hm-index/latency-baseline.json` (step 5)
- Modify: `.github/workflows/ci.yml` (new `latency` job)
- Modify: `crates/hm-index/tests/index_property.rs` (use `synth::project`)
- Test: `crates/hm-index/src/synth.rs` (unit test)

**Interfaces:**
- Produces:
  - `synth::project(dir: &Path, seed: u64, files: u32)`. It writes a deterministic TS project with a `tsconfig.json` (with `@/*` paths). The project has `files` modules in folders of 20. Each module has 3–6 exported functions; each function makes 1–4 calls to functions imported from up to 3 other modules (relative or `@/` imports), and every 10th module is a barrel.
  - `synth::Edit` with `BodyChange`, `SignatureChange`, `Rename`, `AddImport`, `RemoveImport`, `AddFile`, `DeleteFile`, `BreakSyntax`, and `synth::apply_edit(dir, seed, edit) -> BTreeSet<String>` (the changed relative paths).
  - `examples/latency.rs` takes the arguments `--files N --saves S --seed K --max-p95-body-ms X --max-p95-sig-ms Y --max-cold-ms Z [--branch-switch 300]`. It prints one JSON object `{files, cold_ms, body:{p50,p95}, sig:{p50,p95}, branch_switch_ms}` and exits 2 when any maximum is exceeded.
    - **Method:** cold `Indexer::full`, then start a `Watcher` and a loop that calls `apply` for each batch and sends `(Instant, Delta)` on a channel. For each save, the harness records `t_write`, writes the file, waits for the delta that contains the path, and records `t_delta`. Saves alternate between body and signature edits on random files.
    - **Branch switch:** rewrite 300 files at once. "Done" is the moment every one of the 300 paths has appeared in some delta; 300 writes can arrive as several batches.

- [ ] **Step 1: Write the failing test** `synth_is_deterministic`: two calls with seed 7 and 50 files give identical trees, and a cold index of it parses all 50 files with zero `parsed_with_errors`.
- [ ] **Step 2: Run** `cargo test -p hm-index synth`. Expected: FAIL.
- [ ] **Step 3: Implement** `synth.rs` and the example, and switch the property test to `synth::project(seed, 8)` with `synth::Edit`.
- [ ] **Step 4: Run locally**:
  - `cargo run --release -p hm-index --example latency -- --files 1000 --saves 200 --seed 7 --max-p95-body-ms 300 --max-p95-sig-ms 500 --max-cold-ms 2000 --branch-switch 300`;
  - the same with `--files 5000 --max-p95-body-ms 350 --max-p95-sig-ms 600 --max-cold-ms 10000`.

  Expected: exit 0, with the JSON printed. Record both outputs and the machine (CPU, OS) in the commit message. If a target is missed, do not raise the threshold: report it as a finding. The scope decision about Merkle hashes says what to try for the branch-switch row.
- [ ] **Step 5: Add the CI job.** `latency` runs on `ubuntu-24.04` after `test` and executes the 1k-file command three times, keeping the best run. Shared runners are noisy, so:
  - **Spec targets** (300/500 ms, 2 s, 1.5 s) are reported in the job summary and print a warning when missed. A miss is a finding to look at, not a red build.
  - **Regressions fail the build** (spec §11.3): the job fails when a p95 or cold time is more than 20% above `crates/hm-index/latency-baseline.json`.
  - **The baseline file** holds the best-of-3 result of the job's first green run on `main`, which the job uploads as an artifact; commit it with a message that names the run. Until it exists, the job only warns.
- [ ] **Step 6: Commit** `git commit -s -m "test(index): add synthetic repositories and the save-latency harness"`

---

### Task 12: Pinned public repositories and the L1 smoke test

This task closes two items from the spec. §11.1 asks for a public Next.js App Router repository to be chosen in L1, and §15 asks for a grammar check on real code.

**Files:**
- Create: `tests/eval/repos.toml`, `scripts/eval/fetch.sh`, `crates/hm-index/tests/eval_l1.rs`
- Modify: `docs/superpowers/specs/2026-11-02-harbormap-learn-engine-design.md` §11.1 (fill in the Next.js row)

**Interfaces:**
- Produces:
  - `tests/eval/repos.toml` (data only):

    ```toml
    [[repo]]
    name = "next-learn"
    url = "https://github.com/vercel/next-learn"
    commit = "bb2558441a6673ab76c89914c25018bffa27a2ba"
    licence = "MIT"
    subdir = "dashboard/final-example"

    [[repo]]
    name = "fastapi-template"
    url = "https://github.com/fastapi/full-stack-fastapi-template"
    commit = "f4bcfaba7c85be3c6332cc74a20819f586dcdbc6"
    licence = "MIT"
    subdir = "frontend"
    ```

    The other evaluation repositories (wtf, spring-petclinic) are added by L2, when their languages are parsed.
  - `scripts/eval/fetch.sh <dest>`. For each repository it runs `git init`, `git fetch --depth 1 <url> <commit>` and `git checkout FETCH_HEAD` into `<dest>/<name>/`, each in its own empty folder. It never runs anything from the fetched trees.
  - `eval_l1.rs`. Its tests are all `#[ignore]` and read `HARBORMAP_EVAL_DIR`; when that is unset they skip with a message.

- [ ] **Step 1: Write the tests**
  - `next_learn_parses_cleanly_and_resolves`. Index `next-learn/dashboard/final-example` and check:
    - `parsed_with_errors == 0`;
    - edges `imports app/dashboard/invoices/create/page.tsx → app/lib/data.ts` and `→ app/ui/invoices/create-form.tsx`;
    - `calls app/dashboard/invoices/create/page.tsx#Page → app/lib/data.ts#fetchCustomers`;
    - `imports app/ui/invoices/create-form.tsx → app/lib/actions.ts`;
    - a ref `next` with kind `import` and status `external` (from `import Link from 'next/link'`; external refs carry the package name).
  - `fastapi_frontend_parses_and_follows_the_barrel`. Index `fastapi-template/frontend` and check:
    - `parsed_with_errors == 0`;
    - `imports src/hooks/useAuth.ts → src/client/index.ts`, `→ src/utils.ts` and `→ src/hooks/useCustomToast.ts`;
    - `calls src/hooks/useAuth.ts#… → src/client/sdk.gen.ts#LoginService.loginAccessToken` (any caller symbol in useAuth.ts), which goes through the named re-export in `src/client/index.ts`;
    - `src/client/sdk.gen.ts` has class `generated` and is parsed.
- [ ] **Step 2: Run** `scripts/eval/fetch.sh "$TMPDIR/hm-eval" && HARBORMAP_EVAL_DIR="$TMPDIR/hm-eval" cargo test -p hm-index --test eval_l1 -- --ignored`. Expected: PASS. A failure is a real finding: fix the extractor or resolver, never the expectation, unless the expectation misreads the code. In that case quote the code line in the commit.
- [ ] **Step 3: Record timings.** Run `harbormap index --json` on both repositories and note `took_ms` and the counts in the commit message.
- [ ] **Step 4: Update spec §11.1.** Replace the row "A public Next.js App Router app | MIT, chosen in plan L1" with `github.com/vercel/next-learn (dashboard/final-example) | MIT | bb2558441a66 | Next.js App Router + server actions + NextAuth | 3–4 (chosen in L3)`.
- [ ] **Step 5: Commit** `git commit -s -m "test(eval): pin public TS repositories and add the L1 smoke test"`

---

## Done when

- `cargo test --workspace` passes on macOS, Linux and Windows CI, and `cargo deny` passes.
- The `latency` CI job passes at the spec's 1k-file targets. Local 1k and 5k results are recorded.
- `eval_l1` passes against the pinned commits.
- `harbormap index` and `harbormap card` work on Harbormap's own repository. Its Rust files are listed as `not yet supported` until L2.

## Self-review notes (writer)

- **Review fixes folded in** (opus review, 2026-11-02): ignore rules in non-git copies, forced adds for fixture files, re-export propagation over 5 hops, full re-resolve on add/remove, hashing on every apply, single-path `WalkRules`, Linux watch strategy, Windows home and drop order, best-of-3 latency job with a 20% regression gate.
- **Spec coverage.** §4 store location and access (Task 10; `data_version` polling belongs to `learn` in L5). §5.1 files, symbols, edges, refs (Tasks 3, 4, 6); gaps and entry points are L2. §5.2 evidence (Tasks 1, 6). §5.3 ids (Tasks 1, 6). §5.4 (Tasks 7–9; Merkle per the scope decision). §5.5 facts subset (Task 1). §5.7 (Task 11). §6.1 (Task 4). §6.2 TS/JS (Tasks 5, 6). §11.1 Next.js choice (Task 12). §15 grammar and C-toolchain risks (Task 4 step 2, CI on three OSes).
- **Out of L1, owned by later plans:** routes, journeys, prose, panel, other languages, `Probably` edges.
