# Harbormap Learn, Plan L5a: the panel on L1 facts (Map, Code, cards, live) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `harbormap learn [path]` indexes a project, watches it, and serves a local browser panel (Map, Code, file and function cards, Beginner/Builder levels) that shows a saved change within 300 ms (p95), with zero tokens and no Node.js for users.

**Architecture:** Two new library crates and a TypeScript front end.
- `hm-layout` turns file facts into a folder-first group tree with roles, counted arrows and precomputed positions, and answers the per-view questions (overview, one group, one file's links). It knows nothing about SQLite or HTTP.
- `hm-serve` is an axum server on 127.0.0.1 with a per-run token. It reads cards and source from the facts store (a second, read-only use of the WAL database), holds the current `hm-layout` map, serves JSON per view and pushes facts deltas over server-sent events.
- `ui/learn/` is TypeScript with no framework, drawn in SVG, bundled by a pinned esbuild into `ui/learn/dist/`, which is committed and embedded with `rust-embed`.
- `harbormap learn` runs the L1 indexer and watcher on a thread, rebuilds the map after each delta and hands it to the server.

**Tech Stack:** Rust 1.99.0 (edition 2024).
- New Rust crates: axum 0.8.9, tower-http 0.6.11 (`compression-gzip`), tokio-stream 0.1.18 (`sync`), rust-embed 8.13.0, getrandom 0.4.3 (already in `Cargo.lock`); dev: ureq 3.4.2 (no default features), flate2 1.1.9.
- Existing: tokio 1.53.3 (extra features per crate), serde, serde_json, blake3, proptest, tempfile, assert_cmd, hm-index, hm-store, hm-project, hm-core.
- Front end (development only): esbuild 0.28.2 and typescript 6.0.3, pinned exactly in `ui/learn/package-lock.json`; Node 24 runs `node --test` on `.ts` files by type stripping.

**Spec:** `docs/superpowers/specs/2026-11-02-harbormap-learn-engine-design.md` (§4 process model and `learn`, §8.2–8.3 cards and levels, §10 panel, §14 L5). Grouping and aggregation: `docs/superpowers/specs/2026-10-15-harbormap-v0.2-harbor-chart-design.md` §3–§5.3, as revised by the Learn spec.

## Where this plan fits

L5 is split. **L5a (this plan)** runs early, on L1 facts only, so the user sees their project live in the browser now. **L5b** (after L3/L4) adds journey drawing (§10.2), the technologies list content, Write/Refresh with the token preview, and route badges. In L5a the Journeys tab and the technologies strip are wired to endpoints that return an empty, versioned shape.

## Scope decisions inside L5a (none removes a spec requirement; each names its cost if wrong)

- **Map content.** Files with class `source` or `generated` are on the map; `test`, `doc` and `infra` are counted as "Not on this map". Files in languages L1 does not parse yet are boxes without arrows. *Cost if wrong: a filter change in `view::map_facts`.*
- **Arrow weight.** An arrow between two files counts their `calls` and `extends` edges (symbol references, v0.2 §3.7); an import with no resolved reference counts 1, so a link is never invisible. *Cost: one SQL expression.*
- **Roles from paths only.** L1 has no routes, UI-framework or DB facts, so roles come from a small role table of folder and file-name words written for Harbormap (`hm-layout/src/roles.rs`). The UA path table (§3 "role pack" with MIT notice) is not imported in L5a. *Cost: some boxes get the default role `work` until L2a/L3 facts feed roles.*
- **Splitting large folders.** v0.2 §3 steps 1–2 and 4 are implemented (same-token groups, a "files" group, then a "more" bucket for the smallest items). Leiden community detection (step 3.3) is not. *Cost: a huge flat folder splits by name and size rather than by cohesion.*
- **Layout.** A deterministic lane layout (overview: role lanes see → work → keep → outside, shared below) and a small layered layout inside a group (SCC condensation, longest-path ranks, two barycentre sweeps). No Sugiyama crate, no stress layout: groups never exceed 25 children. *Cost: more crossings than network-simplex would give in dense groups.*
- **Expansion is in place, on top.** Expanding a box grows it from its own top-left corner over its neighbours, which stay where they are and are dimmed; links to them show as chips on the expanded box ("→ Data · 7", "+N elsewhere"). *Cost if the user wants neighbours pushed aside: a client-side camera change, no data change.*
- **Position stability beats freshness.** A group's child positions are keyed by the set of its children (the "shape hash"), not by its edges, so a save that adds an import never moves a box. Positions change only when a group gains or loses children. *Cost: ordering can be slightly worse than a cold layout until the next restart.*
- **Position cache in memory only.** The spec's `layout` table belongs to the store schema, which only the L2a session may migrate; L5a caches positions in the `Map` value across rebuilds within one run. A cold start lays out again (milliseconds for ≤ 12 + 25-box groups). *Cost: none visible; the coordinator may move the cache into the store later.*
- **Levels in the browser.** The server always sends up to 40 arrows and up to 25 children with a `beginner` flag; the client shows ≤ 10 arrows and the flagged ≤ 12 children in Beginner. The level switch never fetches and never lays out. *Cost: Beginner payloads carry ~30 extra arrows (a few KB).*
- **Sidebar.** Search (`/`), the "Not on this map" counts and an empty journeys section. A lazily loaded file tree is deferred to L5b. *Cost: users reach files through the map and search only.*
- **Source size.** `/api/file` serves at most 512 KiB of a file, cut at a line end, with `truncated: true`; source text is the user's own file on demand and is outside the 50 KB per-view budget. *Cost: a very long generated file is shown in part.*
- **Card text.** Until L3/L4, the drawer shows facts with fixed labels (no prose, no template sentences). Level changes wording of labels and which rows show, never positions. *Cost: drawer copy is rewritten when L3 template text arrives.*

## Before you start

- `cargo` may not be on PATH: `export PATH="$HOME/.cargo/bin:$PATH"`.
- Work only in `/Users/ibrahim/Desktop/CodeOrbit-wt/s2-l5a` (branch `feat/learn-l5a-panel`). Touch only the files listed in your task.
- Commits: `git commit -s`, conventional prefix, and the message ends with exactly this trailer line (no other model trailer):
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
- After every task: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` stay clean. Front-end tasks also run `npm --prefix ui/learn run check`, `npm --prefix ui/learn test` and `npm --prefix ui/learn run build`, and commit `ui/learn/dist/`.
- **Privacy.** Committed code, fixtures and messages use synthetic names (a "shop" app) only.

## Global Constraints

- Toolchain `1.99.0`, edition `2024`, `unsafe_code = "forbid"`, clippy `-D warnings`; licences must pass `deny.toml`; new versions pinned exactly in the root `[workspace.dependencies]` (append only).
- Zero tokens, no network: nothing calls an LLM or a remote host. The server binds `127.0.0.1` only, on a port the OS picks (`:0`).
- Read-only on the project: nothing is written inside the user's repository. `/api/file` only serves a path that is a row in the `file` table and whose canonical path stays inside the worktree root.
- Auth: a random token of 32 lowercase hex characters (16 bytes from `getrandom`) per run. A request is allowed when it carries `?t=<token>` (only on `/`), the cookie `hm_learn=<token>`, or `Authorization: Bearer <token>`; otherwise **401**. A `Host` header other than `127.0.0.1:<port>` or `localhost:<port>` gets **403** before the token is checked. Comparison is constant-time.
- Panel limits (spec §10.3): overview ≤ 12 boxes; Beginner ≤ 12 boxes and ≤ 10 arrows; Builder ≤ 25 and ≤ 40; never more than 150 items (boxes + arrows + chips) in one view.
- Hubs (v0.2 §3.4): a file whose distinct importing files number more than 20 moves to the top-level "Shared" group.
- Budgets (spec §10.5): first screen (index.html + app.js + app.css + overview + journeys + tech) ≤ 100 KB gzipped; each view payload (group, card, search) ≤ 50 KB gzipped; JS bundle ≤ 150 KB gzipped; never the whole index in one response.
- Live: watcher 100 ms quiet / 500 ms max wait (L1); facts delta on screen ≤ 300 ms p95 after the write.
- Determinism: the same facts give byte-identical `overview` and `group` JSON on a cold build. Lists are sorted with explicit tie-breaks.
- Ids: group ids are `g_` + 12 hex chars of BLAKE3 over the group key; file nodes use `hm_core::ids::file_id(path)`.
- Every JSON response has a top-level `"v": 1`.
- UI: dark theme only, all colours as CSS variables on `:root`; red means brand and problems (a problem always has an icon and words); selection is a white outline. UI labels are English.
- Shared files (RULES §6): root `Cargo.toml` append-only (done once in Task 0); `crates/harbormap/src/cli.rs` and `main.rs` get one new variant, one `mod`, one match arm; `crates/hm-index/src/lib.rs` gets one `pub mod view;` line; no change to `query.rs`, `index.rs` or the store schema.

## Review Focus

1. **A project whose files all sit in one flat folder** (or at the root, 3 000 files): the overview still has ≤ 12 boxes and every group ≤ 25 children, with readable names. Task 2: `a_flat_folder_of_3000_files_stays_within_budget`.
2. **The open file is deleted or the expanded group disappears after a save:** the server answers 404 for that card or group, the delta lists the file under `files_removed`, and the client closes the drawer or returns to the overview instead of showing stale data. Task 4: `a_removed_file_card_is_404`; Task 9: `deleting_a_file_sends_a_removed_delta`.
3. **A symlink inside the project that points outside it** (`src/leak.ts -> /etc/hosts`): `/api/file` refuses it even if the indexer stored the path. Task 4: `a_symlink_leaving_the_project_is_refused` (Unix only).
4. **Paths with spaces, `#`, `%` or non-ASCII letters** (`src/çay #2.ts`): card and file requests round-trip through URL encoding. Task 4: `a_path_with_spaces_hash_and_unicode_round_trips`.
5. **The browser reconnects after sleep** and missed deltas: the SSE `hello` event carries the current `seq`, so a client whose `seq` is older refetches. Task 4: `hello_carries_the_current_seq`.

---

## API contract (server ↔ panel)

The Rust payload structs (Tasks 3, 4, 7) serialize to exactly these shapes; `ui/learn/src/types.ts` (Task 5) mirrors them. All paths are under the server origin; `seq` is the model version (starts at 0, +1 per published delta).

```ts
type Role = "see" | "work" | "keep" | "outside" | "shared";
type NodeKind = "folder" | "token" | "files" | "more" | "shared" | "file";
interface Box { id: string; name: string; kind: NodeKind; role: Role; role_word: string;
  files: number; loc: number; x: number; y: number; w: number; h: number;
  expandable: boolean; beginner: boolean; path?: string /* file nodes only */; generated?: boolean }
interface Arrow { from: string; to: string; count: number }
interface Chip { id: string; name: string; role: Role; out: number; in: number }
interface Repo { name: string; branch: string | null; commit: string | null /* 7 chars */ }

// GET /api/overview
interface Overview { v: 1; seq: number; repo: Repo; updated_ms: number;
  boxes: Box[]; arrows: Arrow[]; hidden_arrows: number;
  excluded: { class: string; files: number }[]; files_total: number }
// GET /api/group/{id}            404 when unknown
interface GroupView { v: 1; seq: number; id: string; name: string; role: Role; role_word: string;
  w: number; h: number; breadcrumb: { id: string; name: string }[];
  children: Box[]; arrows: Arrow[]; hidden_arrows: number; elsewhere: Chip[]; elsewhere_more: number }
// GET /api/card?path=P[&symbol=S][&view=overview|<group id>]   404 not indexed, 400 bad path
interface FileCardPayload { v: 1; seq: number; kind: "file"; card: FileCard /* hm_index::query::FileCard */;
  links: { view: string; incoming: { id: string; count: number }[]; outgoing: { id: string; count: number }[] } | null }
interface SymbolCardPayload { v: 1; seq: number; kind: "symbol"; card: SymbolCard /* hm_index::query::SymbolCard */ }
// GET /api/file?path=P            404 not indexed, 400 bad path, 403 outside the project
interface SourceFile { v: 1; path: string; lang: string | null; text: string; lines: number; truncated: boolean }
// GET /api/search?q=Q             ≤ 20 results, files before symbols, then by label
interface Search { v: 1; results: { kind: "file" | "symbol"; path: string; symbol: string | null; label: string }[] }
// GET /api/journeys  →  { v: 1, journeys: [] }        GET /api/tech  →  { v: 1, categories: [] }
// GET /api/events  (text/event-stream)
//   event: hello  data: { "seq": number }
//   event: facts  data: FactsDelta
interface FactsDelta { seq: number; at_ms: number; took_ms: number; files_changed: string[];
  files_removed: string[]; regrouped: boolean; groups_changed: string[] }
```

Box geometry: overview boxes are in world coordinates; group children are relative to the group's top-left. Every response sets `Cache-Control: no-store`; HTML sets `Content-Security-Policy: default-src 'self'; img-src 'self' data:`, `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`.

---

### Task 0: Scaffold the crates and dependencies (controller)

**Files:**
- Modify: `Cargo.toml` (append to `[workspace.dependencies]`)
- Create: `crates/hm-layout/Cargo.toml`, `crates/hm-layout/src/lib.rs`, `crates/hm-serve/Cargo.toml`, `crates/hm-serve/src/lib.rs`
- Modify: `Cargo.lock` (by cargo)

- [ ] **Step 1:** Append `hm-layout`, `hm-serve` (path deps), `axum = "0.8.9"`, `tower-http = { version = "0.6.11", features = ["compression-gzip"] }`, `tokio-stream = { version = "0.1.18", features = ["sync"] }`, `rust-embed = "8.13.0"`, `getrandom = "0.4.3"`, `ureq = { version = "3.4.2", default-features = false }`, `flate2 = "1.1.9"`.
- [ ] **Step 2:** `hm-layout` deps: blake3, hm-core, serde; dev: proptest, serde_json. `hm-serve` deps: axum, tokio (`features = ["rt-multi-thread", "net", "sync", "macros", "signal", "time"]`), tokio-stream, tower-http, rust-embed, getrandom, serde, serde_json, dunce, hm-core, hm-index, hm-layout, hm-project, hm-store; dev: ureq, flate2, tempfile. Both `[lints] workspace = true`. Each `lib.rs` is a crate doc comment only.
- [ ] **Step 3:** `cargo build --workspace` passes; check each new crate's licence with `cargo metadata --format-version 1` against `deny.toml` `allow`.
- [ ] **Step 4:** Commit `build: scaffold hm-layout and hm-serve`.

---

### Task 1: `hm-index::view` — the facts the panel reads

**Model:** sonnet. **Wave A.**

**Files:**
- Create: `crates/hm-index/src/view.rs`
- Modify: `crates/hm-index/src/lib.rs` (one line: `pub mod view;`)
- Test: `crates/hm-index/tests/view.rs`

**Interfaces:**
- Consumes: the facts store (tables `file`, `symbol`, `edge`), `hm_index::index::Indexer` in tests.
- Produces:
  ```rust
  pub struct MapFacts { pub files: Vec<MapFile>, pub links: Vec<MapLink>, pub excluded: BTreeMap<String, u32> }
  pub struct MapFile { pub path: String, pub lang: Option<String>, pub loc: u32, pub generated: bool }
  pub struct MapLink { pub src: u32, pub dst: u32, pub refs: u32 } // indexes into `files`
  pub fn map_facts(store: &Store) -> Result<MapFacts>;
  pub struct IndexedFile { pub path: String, pub lang: Option<String>, pub class: String }
  pub fn indexed_file(store: &Store, rel: &str) -> Result<Option<IndexedFile>>;
  pub struct Hit { pub kind: &'static str /* "file" | "symbol" */, pub path: String, pub symbol: Option<String> }
  pub fn search(store: &Store, query: &str, limit: usize) -> Result<Vec<Hit>>;
  ```
  All derive `Debug, Clone, PartialEq, Eq, Serialize`. `Result` is `hm_index::index::Result`.

Rules: `files` are rows with class `source` or `generated`, sorted by path. `excluded` counts every other class by name. Links lift edge endpoints to files (a symbol id → its `symbol.file_id`; a file id stays), drop self-links and endpoints not in `files`, and merge per ordered (src, dst): `refs` = count of `calls` + `extends` rows, or 1 when only `imports` rows exist. `links` sorted by (src, dst). `search` matches case-insensitively as a substring of the path or of the symbol name (`Owner.name` for methods); ranking: exact file-name match, then file-name prefix, then shorter path, then path; files first, then symbols; at most `limit`; an empty or whitespace query returns nothing; `%` and `_` in the query are literal.

- [ ] **Step 1: Write the failing tests** in `crates/hm-index/tests/view.rs`, using a temp project indexed with `Indexer` (copy the setup style of `tests/card.rs`), synthetic shop names:
  - `map_facts_lists_source_files_and_counts_the_rest`: `src/orders/service.ts`, `src/orders/service.test.ts`, `README.md` → `files` = the service only; `excluded["test"] == 1`, `excluded["doc"] == 1`.
  - `links_count_calls_and_fall_back_to_one_for_bare_imports`: `a.ts` calls two functions of `b.ts` and imports a type only from `c.ts` → links `(a→b, refs 2)`, `(a→c, refs 1)`; no `b→a`.
  - `self_links_and_external_packages_are_dropped`.
  - `indexed_file_knows_only_rows_in_the_store`: a file on disk but ignored by `.gitignore` → `None`.
  - `search_ranks_exact_names_first_and_treats_percent_literally`: files `src/cart.ts`, `src/cart-view.ts`, `src/discount%.ts` → query `cart` gives `src/cart.ts` first; query `%` gives only `src/discount%.ts`.
- [ ] **Step 2:** Run `cargo test -p hm-index --test view` → fails (no `view` module).
- [ ] **Step 3:** Implement `view.rs` with SQL `LEFT JOIN symbol` on both ends, grouped per (src file, dst file, kind); search with `LIKE ? ESCAPE '\'`.
- [ ] **Step 4:** `cargo test -p hm-index --test view` passes; full workspace checks.
- [ ] **Step 5:** Commit `feat(index): read map facts, indexed files and search hits for the panel`.

---

### Task 2: `hm-layout` input, roles and the group tree

**Model:** opus. **Wave A.**

**Files:**
- Create: `crates/hm-layout/src/input.rs`, `src/roles.rs`, `src/tree.rs`, `src/synth.rs`, `src/map.rs` (skeleton only)
- Modify: `crates/hm-layout/src/lib.rs`
- Test: `crates/hm-layout/tests/tree.rs`, `crates/hm-layout/tests/tree_props.rs`

**Interfaces:**
- Produces:
  ```rust
  // input.rs — files sorted by path and unique; links unique per (src, dst), src != dst
  pub struct Input { pub files: Vec<InFile>, pub links: Vec<InLink> }
  pub struct InFile { pub path: String, pub loc: u32, pub generated: bool }
  pub struct InLink { pub src: u32, pub dst: u32, pub refs: u32 }
  // roles.rs
  #[serde(rename_all = "lowercase")] pub enum Role { See, Work, Keep, Outside, Shared }
  pub fn role_for(path: &str) -> Role;          // the deepest matching path word wins; default Work
  pub fn role_word(role: Role, name: &str) -> &'static str;
  // tree.rs
  #[serde(rename_all = "lowercase")] pub enum NodeKind { Folder, Token, Files, More, Shared, File }
  pub struct Node { pub id: String, pub key: String, pub kind: NodeKind, pub name: String, pub role: Role,
                    pub parent: Option<usize>, pub children: Vec<usize>, pub files: u32, pub loc: u32,
                    pub file: Option<u32> /* index into Input.files for File nodes */,
                    pub shape_hash: [u8; 32], pub merkle: [u8; 32] }
  pub struct Tree { pub nodes: Vec<Node>, pub root: usize, pub file_node: Vec<usize> /* per input file */ }
  pub const OVERVIEW_BUDGET: usize = 12;  pub const GROUP_BUDGET: usize = 25;  pub const HUB_IN_DEGREE: usize = 20;
  pub fn build(input: &Input, project_name: &str) -> Tree;
  impl Tree { pub fn node(&self, id: &str) -> Option<usize>; pub fn top(&self) -> &[usize] /* root children */ }
  // synth.rs  (#[doc(hidden)] pub mod synth)
  pub fn input(seed: u64, files: u32) -> Input;   // deterministic synthetic shop project, nested folders, ~3 links per file, a few hubs
  // map.rs (skeleton; Task 3 fills in positions and views)
  pub struct Map { /* tree + input links */ }
  impl Map { pub fn build(input: &Input, project_name: &str, previous: Option<&Map>) -> Map; pub fn tree(&self) -> &Tree; }
  ```

**Grouping algorithm** (v0.2 §3, as decided above):
1. **Hubs:** a file with more than `HUB_IN_DEGREE` distinct importing files goes under one top-level node `kind = Shared`, key `~shared`, name `"Shared"`, role `Shared`.
2. **Folder tree** of the remaining files; then **chain merge**: a folder whose only child is one folder becomes one node named `a/b`. The root itself descends through a single-folder chain, so a project with everything under `src/` shows `src`'s children at the top.
3. **Partition** each folder's items (subfolders + its own files) to the budget (`OVERVIEW_BUDGET` for the root's children, counting the Shared node; `GROUP_BUDGET` below), in this order, stopping as soon as the count fits:
   1. *Token groups:* split each loose file's stem into words (camelCase, `-`, `_`, `.`); a word shared as the first or last word by ≥ 3 loose files forms a node `kind = Token`, key `<folder>/*<word>`, name `"<word> files"`. Bigger groups claim files first; ties by word.
   2. *Files group:* remaining loose files (≥ 2) go into `kind = Files`, key `<folder>/~files`, name `"other files"` (or `"files"` when the folder has no subfolders left).
   3. *More bucket:* sort items by (files desc, name asc); keep the first `budget − 1`; the rest go into `kind = More`, key `<folder>/~more`, name `"<N> more"`.
   New Token/Files/More nodes are partitioned again with `GROUP_BUDGET`, recursively.
4. **Names** are never empty and never "Cluster N": folder → its (merged) folder path relative to the parent; root → `project_name`.
5. **Roles:** folder nodes take `role_for(key)`; Token/Files/More take their parent's role; file nodes take `role_for(path)`.
6. **Ids:** group `g_` + first 12 hex of BLAKE3(key); file nodes `hm_core::ids::file_id(path)`; root key `""`.
7. **Hashes:** `shape_hash` = BLAKE3 over the sorted keys of the direct children; `merkle` = BLAKE3 over the node's kind and key, a file's path and loc, the children's `merkle` in key order, and the sorted (src, dst) pairs of input links between different direct children. All child lists are ordered by key.

**Role table** (words matched against path segments and file stems, case-insensitive): see — `app pages page components component views view ui screens screen layouts layout styles public frontend web client`; work — `api routes route server controllers controller handlers handler actions middleware services service lib core domain jobs workers`; keep — `db database models model schema schemas prisma migrations repositories repository entities entity store stores data dal`; outside — `integrations integration external vendor sdk adapters gateways gateway webhooks`; shared — `shared common types utils util helpers constants config`. Role words: see `"Screens"`, work `"Logic"` (but `"Server endpoints"` when the node's name is one of `api routes route controllers handlers`), keep `"Data"`, outside `"Outside services"`, shared `"Shared helpers"`.

- [ ] **Step 1: Write the failing tests** (`tests/tree.rs`):
  - `everything_under_src_starts_the_overview_inside_src`: files `src/app/page.tsx`, `src/lib/db.ts` → top nodes are `app` and `lib`.
  - `single_child_chains_merge_into_one_named_node`: `src/features/orders/{a,b}.ts` + `src/app/page.tsx` → a top node named `features/orders`.
  - `a_file_imported_by_21_files_moves_to_shared` (20 importers → stays; 21 → `Shared`).
  - `token_groups_need_three_files`: `ordersController.ts`, `cartController.ts`, `userController.ts` + 30 other files in one folder → a `Controller files` node with exactly those three.
  - `roles_come_from_path_words`: `src/components/Cart.tsx` → see; `src/db/client.ts` → keep; `src/api/orders.ts` → work with role word `"Server endpoints"`; `src/misc/x.ts` → work.
  - `ids_are_stable_and_prefixed`: building twice gives the same ids; groups start `g_`, files `f_`.
- [ ] **Step 2: Write the property tests** (`tests/tree_props.rs`, proptest over `synth::input(seed, n)` for seed 0..64, n 1..800, plus fixed cases):
  - `every_file_is_in_exactly_one_leaf`, `the_overview_has_at_most_12_boxes`, `no_group_has_more_than_25_children`, `names_are_never_empty_or_cluster`, `ids_are_unique`, `building_twice_is_identical` (compare `serde_json::to_string` of a `Debug`-free projection or derive `PartialEq`).
  - `a_flat_folder_of_3000_files_stays_within_budget` and the same with all 3 000 files at the root.
- [ ] **Step 3:** Run `cargo test -p hm-layout` → fails.
- [ ] **Step 4:** Implement `input.rs`, `roles.rs`, `tree.rs`, `synth.rs`.
- [ ] **Step 5:** `cargo test -p hm-layout` passes (proptests under 30 s in debug); workspace checks.
- [ ] **Step 6:** Commit `feat(layout): folder-first group tree with roles, hubs and budgets`.

---

### Task 3: `hm-layout` positions, arrows and views

**Model:** opus. **Wave B** (after Task 2).

**Files:**
- Create: `crates/hm-layout/src/place.rs`
- Modify: `crates/hm-layout/src/map.rs`, `crates/hm-layout/src/lib.rs`
- Test: `crates/hm-layout/tests/map.rs`, `crates/hm-layout/tests/map_props.rs`

**Interfaces:**
- Consumes: Task 2 (`Input`, `Tree`, `build`, `Role`, `NodeKind`, the `Map` skeleton whose `build` signature stays as is).
- Produces:
  ```rust
  pub const PARAMS_VERSION: u32 = 1;
  pub const BEGINNER_ARROWS: usize = 10; pub const BUILDER_ARROWS: usize = 40;
  pub const BEGINNER_CHILDREN: usize = 12; pub const MAX_ITEMS: usize = 150; pub const MAX_CHIPS: usize = 8;
  pub struct Map { /* tree, input links, placements cache */ }
  impl Map {
      pub fn build(input: &Input, project_name: &str, previous: Option<&Map>) -> Map;
      pub fn overview(&self) -> OverviewView;          // boxes, arrows, hidden_arrows
      pub fn group(&self, id: &str) -> Option<GroupView>; // id, name, role, role_word, w, h, breadcrumb, children, arrows, hidden_arrows, elsewhere, elsewhere_more
      pub fn file_links(&self, path: &str, view: &str) -> Option<FileLinks>; // view = "overview" or a group id
      pub fn ancestors(&self, path: &str) -> Option<Vec<String>>; // group ids from the top box down to the file's parent
      pub fn changed_since(&self, old: &Map) -> Changed;   // regrouped: node id sets differ; groups: ids whose merkle differs, sorted
  }
  pub struct Changed { pub regrouped: bool, pub groups: Vec<String> }
  ```
  `OverviewView`, `GroupView`, `BoxView`, `ArrowView`, `ChipView`, `Crumb`, `FileLinks`, `LinkEnd` derive `Serialize` with field names exactly as the API contract (`Box`, `Arrow`, `Chip`; the server adds `v`, `seq` and the rest).

**Arrows:** lift every input link to the visible node holding each end (overview: top boxes; group view: the group's children, and links leaving the group become chips); drop self-arrows; sum `refs` per ordered pair; never merge A→B with B→A; sort by count desc, then `from`, then `to`; keep the first `BUILDER_ARROWS`, report the rest as `hidden_arrows`.

**Chips:** for a group view, each other top box linked to any file inside the group gives a chip with `out` (from the group) and `in` (into it) sums; sort by `out + in` desc, then name; first `MAX_CHIPS`, the rest counted in `elsewhere_more`.

**Beginner flag:** in a group, the `BEGINNER_CHILDREN` children with the highest (link weight in + out inside the whole map, files, then name) get `beginner: true`; all overview boxes are `beginner: true`.

**Positions** (`place.rs`; all integers, pixels):
- Overview boxes `200 × 72`, column gap 120, row gap 40. Columns are the non-empty role lanes in the order see, work, keep, outside; `shared` boxes form one row under the tallest column. Inside a lane, order by barycentre of linked boxes in earlier lanes (forward sweep, then backward), ties by (files desc, name).
- Group children `176 × 52`, gaps 96 × 24, padding 24, header 44. Ranks: Tarjan SCC over child links, longest path from sources on the condensation, at most 5 columns (ranks ≥ 4 share column 4). Within a column, `beginner` children first, then two barycentre sweeps, ties by name. The group's `w`/`h` fit its grid.
- A `File` child with no children has `expandable: false`; every other node `true`.
- **Cache:** placements are keyed by (`shape_hash`, `PARAMS_VERSION`); `build` with `previous` reuses a placement whose key matches, so only groups whose child set changed are laid out again.

- [ ] **Step 1: Write the failing tests** (`tests/map.rs`): `overview_arrows_count_refs_one_per_ordered_pair` (A→B 3 refs + 2 refs from another file pair = 5; B→A separate), `arrows_beyond_40_are_counted_as_hidden`, `links_leaving_a_group_become_chips`, `file_links_lift_to_the_visible_boxes_of_the_view`, `see_lane_is_left_of_keep_lane`, `beginner_children_are_the_12_most_linked`, `changed_since_reports_the_group_of_an_edited_file` (change one file's loc → its ancestors' ids, `regrouped == false`; add a new folder → `regrouped == true`).
- [ ] **Step 2: Write the property tests** (`tests/map_props.rs` over `synth::input`): `cold_builds_are_byte_identical` (serde_json of overview and every group), `boxes_never_overlap_and_stay_inside_their_group`, `items_per_view_never_exceed_150` (boxes + arrows + chips), `adding_a_link_moves_no_box` (build, add one new link between existing files, build with `previous` → every box position equal), `reusing_the_previous_map_equals_a_cold_build_for_the_same_input`.
- [ ] **Step 3:** `cargo test -p hm-layout` → fails. **Step 4:** implement. **Step 5:** passes; workspace checks.
- [ ] **Step 6:** Commit `feat(layout): precomputed positions, counted arrows, chips and views`.

---

### Task 4: `hm-serve` core — auth, assets, cards, source, search, events

**Model:** sonnet. **Wave B** (after Tasks 1, 2 and 5; uses `Map::build` from Task 2 and `ui/learn/dist/` from Task 5).

**Files:**
- Create: `crates/hm-serve/src/{auth.rs, assets.rs, model.rs, api.rs, events.rs, source.rs}`
- Modify: `crates/hm-serve/src/lib.rs`
- Test: `crates/hm-serve/tests/common/mod.rs`, `tests/auth.rs`, `tests/api.rs`, `tests/events.rs`

**Interfaces:**
- Consumes: `hm_index::view::{map_facts, indexed_file, search}`, `hm_index::query::{file_card, symbol_card}`, `hm_layout::Map::build`, `hm_project::paths::within`.
- Produces:
  ```rust
  pub struct Repo { pub name: String, pub branch: Option<String>, pub commit: Option<String> }
  pub struct ServeConfig { pub root: PathBuf, pub db: PathBuf, pub repo: Repo, pub token: Option<String> /* None → random */ }
  pub struct Model { pub map: hm_layout::Map, pub excluded: Vec<(String, u32)>, pub files_total: u32 }
  impl Model { pub fn load(store: &Store, project_name: &str, previous: Option<&Model>) -> hm_index::index::Result<Model>; }
  #[derive(Serialize)] pub struct FactsDelta { pub seq: u64, pub at_ms: u64, pub took_ms: u64, pub files_changed: Vec<String>,
      pub files_removed: Vec<String>, pub regrouped: bool, pub groups_changed: Vec<String> }
  pub struct Server; impl Server {
      pub async fn bind(config: ServeConfig, model: Model) -> std::io::Result<Server>; // 127.0.0.1:0
      pub fn port(&self) -> u16; pub fn token(&self) -> &str;
      pub fn url(&self) -> String;               // "http://127.0.0.1:{port}/?t={token}"
      pub fn handle(&self) -> Handle;
      pub async fn run(self, shutdown: impl Future<Output = ()> + Send + 'static) -> std::io::Result<()>;
  }
  #[derive(Clone)] pub struct Handle; impl Handle {
      /// Swaps in the new model, sets `delta.seq` to the next seq, and sends the `facts` event.
      pub fn publish(&self, model: Model, delta: FactsDelta) -> u64;
      pub fn seq(&self) -> u64;
  }
  ```
- Router: `/` (index.html; with `?t=` sets the cookie `hm_learn=<token>; HttpOnly; SameSite=Strict; Path=/` and answers `303` to `/`), `/assets/{*file}` (embedded `ui/learn/dist/`, `#[folder = "../../ui/learn/dist/"]`, content type from the extension), `/api/card`, `/api/file`, `/api/search`, `/api/journeys`, `/api/tech`, `/api/events`. Auth and host middleware wrap everything; tower-http gzip compression wraps the JSON routes and assets (not `/api/events`).
- Store reads use a second `Store::open(&config.db)` behind a `std::sync::Mutex`, inside `tokio::task::spawn_blocking`.
- `/api/file`: reject (400) empty paths, absolute paths, `\`, NUL and any `..` or `.` segment; 404 unless `indexed_file` finds it; join to the root, `dunce::canonicalize`, 403 unless `within(canonical, root)`; read bytes, decode lossily, cut at the last `\n` at or before 512 KiB with `truncated: true`; `lines` counts the returned text.
- `/api/card`: same path checks; `symbol` selects `symbol_card`; file cards carry `links: null` here (Task 7 fills it once `Map::file_links` exists).
- `/api/events`: first event `hello` with the current `seq`, then every published `facts` delta; keep-alive comment every 15 s; a lagging receiver skips to the latest (send a fresh `hello`).

- [ ] **Step 1: Write the test helper** `tests/common/mod.rs`: `fn shop(dir: &Path)` writes a synthetic shop project (`src/app/page.tsx`, `src/orders/service.ts`, `src/orders/repo.ts`, `src/db/client.ts`, a test file, a README); `async fn start(dir) -> (Server-running task, port, token)` indexes it with `Indexer`, loads `Model`, binds, spawns `run`; `fn get(port, path, auth: Auth) -> (u16, String)` with ureq configured to not treat 4xx as errors, where `Auth` is `None | Cookie | Bearer | Query`.
- [ ] **Step 2: Write the failing tests:**
  - `auth.rs`: `no_token_is_401`, `a_wrong_token_is_401`, `the_query_token_sets_a_strict_http_only_cookie_and_redirects`, `the_cookie_and_the_bearer_token_both_work`, `a_foreign_host_header_is_403` (raw `TcpStream` with `Host: evil.example`), `the_server_listens_on_loopback_only` (`url()` starts with `http://127.0.0.1:`), `tokens_are_32_hex_and_differ_per_run`.
  - `api.rs`: `the_file_card_lists_jobs`, `a_symbol_card_names_callers`, `a_removed_file_card_is_404`, `dot_dot_and_absolute_paths_are_400`, `a_file_outside_the_index_is_404` (`.git/config`, an ignored file), `a_symlink_leaving_the_project_is_refused` (`#[cfg(unix)]`), `a_large_file_is_cut_at_a_line_end` (600 KiB file), `invalid_utf8_is_replaced_not_refused`, `a_path_with_spaces_hash_and_unicode_round_trips` (`src/çay #2.ts`), `search_returns_at_most_20`, `journeys_and_tech_are_empty_and_versioned`, `every_api_response_has_v_1_and_no_store`, `index_html_and_the_bundle_are_served_with_csp`.
  - `events.rs`: `hello_carries_the_current_seq` (publish twice → a new subscriber's hello says 2), `a_published_delta_arrives_as_a_facts_event`.
- [ ] **Step 3:** `cargo test -p hm-serve` → fails. **Step 4:** implement. **Step 5:** passes; workspace checks.
- [ ] **Step 6:** Commit `feat(serve): local panel server with token auth, cards, source and events`.

---

### Task 5: Front-end toolchain, contract types, pure modules and the bundle check

**Model:** sonnet. **Wave A.**

**Files:**
- Create: `ui/learn/package.json`, `ui/learn/package-lock.json`, `ui/learn/tsconfig.json`, `ui/learn/build.mjs`, `ui/learn/.gitignore` (`node_modules/`), `ui/learn/README.md` (how to build; one paragraph)
- Create: `ui/learn/src/index.html`, `ui/learn/src/styles.css` (tokens only), `ui/learn/src/main.ts` (boot stub that fetches `/api/overview` and prints the project name), `ui/learn/src/types.ts`, `ui/learn/src/api.ts`, `ui/learn/src/format.ts`, `ui/learn/src/highlight.ts`
- Create: `ui/learn/test/format.test.ts`, `ui/learn/test/highlight.test.ts`
- Create: `ui/learn/dist/{index.html, app.js, app.css, SOURCE_HASH}` (built)
- Create: `crates/hm-serve/tests/bundle.rs`
- Modify: `.github/workflows/ci.yml` (one new job `ui`)

**Interfaces:**
- Produces (TypeScript, used by Tasks 6 and 8):
  - `types.ts`: the API contract types above, exported.
  - `api.ts`: `getJSON<T>(path: string): Promise<T>` (same origin, `credentials: "same-origin"`, throws `ApiError { status }` on non-2xx); `overview()`, `group(id)`, `card(path, symbol?, view?)`, `file(path)`, `search(q)`, `journeys()`, `tech()`; `subscribe(onHello: (seq) => void, onFacts: (d: FactsDelta) => void, onState: (live: boolean) => void): () => void` over `EventSource("/api/events")`.
  - `format.ts`: `type Level = "beginner" | "builder"`; `ago(ms: number, now: number): string` (`"just now"` < 5 s, `"N s ago"` < 60 s, `"N min ago"`, `"N h ago"`); `plural(n, one, many)`; `usedFrom(level, n)` (Beginner `"Used from N places"`, Builder `"N importers"`); `calledFrom(level, n)`; `arrowsFor(level, arrows)` (first 10 / 40); `childrenFor(level, boxes)` (Beginner: `beginner` only); `fileName(path)`; `dirName(path)`.
  - `highlight.ts`: `type Token = { kind: "kw" | "str" | "com" | "num" | "fn" | "type" | "punct" | "plain"; text: string }`; `tokenize(lang: string | null, line: string, state: LexState): { tokens: Token[]; state: LexState }` with `LexState` carrying an open block comment or template string across lines; keyword sets for typescript/tsx/javascript, python, go, rust, java, csharp; any other language → one `plain` token. `toHtml(tokens): string` escapes `& < > " '`.
- `build.mjs`: bundles `src/main.ts` → `dist/app.js` (`bundle`, `minify`, `format: "iife"`, `target: "es2022"`, `legalComments: "none"`, no sourcemap), `src/styles.css` → `dist/app.css`, copies `src/index.html` (references `/assets/app.js` and `/assets/app.css`), and writes `dist/SOURCE_HASH`: FNV-1a 64-bit, lowercase 16-hex, over every file of `src/**`, `build.mjs`, `package.json`, `package-lock.json`, `tsconfig.json` in sorted `/`-relative path order, each fed as `path` + `\0` + content with every `\r` removed + `\0`. `node build.mjs --check` builds into a temp folder and exits 1 if any `dist` file differs.
- `package.json` scripts: `build` (`node build.mjs`), `check` (`tsc --noEmit`), `test` (`node --test test/`), `verify` (`node build.mjs --check`). devDependencies pinned exactly: `"esbuild": "0.28.2"`, `"typescript": "6.0.3"`. `tsconfig.json`: `strict`, `noEmit`, `erasableSyntaxOnly`, `allowImportingTsExtensions`, `module` `"esnext"`, `moduleResolution` `"bundler"`, `target` `es2022`, `lib` `["es2022","dom"]`; imports use `.ts` extensions so `node --test` runs the sources directly.
- `styles.css` tokens (from the design draft): `--bg:#09090B; --bg2:#0E0E11; --panel:#131317; --panel2:#18181D; --line:#24242B; --line2:#2F2F38; --ink:#F2F2F3; --sub:#A1A1AA; --dim:#6B6B75; --red:#F43F4E; --red2:#B91C2C; --redbg:rgba(244,63,78,.10); --redbd:rgba(244,63,78,.38); --sel:#F2F2F3; --selbg:rgba(242,242,243,.07); --okdot:#4ADE80`, plus role colours `--see`, `--work`, `--keep`, `--outside`, `--shared` (muted, distinguishable, not red) and `--mono`/`--sans` stacks.

- [ ] **Step 1:** `npm install --prefix ui/learn --save-exact -D esbuild@0.28.2 typescript@6.0.3`; write `package.json` scripts and `tsconfig.json`.
- [ ] **Step 2: Write the failing tests:** `format.test.ts`: `ago` boundaries (4 999 ms → "just now", 5 000 → "5 s ago", 60 000 → "1 min ago"); `arrowsFor("beginner", 40 arrows)` has 10; `childrenFor("beginner")` keeps only flagged. `highlight.test.ts`: `const x = "a // b"; // c` → kw, plain, str (`"a // b"`), com; a `/*` opened on line 1 keeps line 2 a comment; a template string spanning lines; `toHtml` escapes `<script>`; unknown language → one plain token; Python `def` is a keyword.
- [ ] **Step 3:** `npm --prefix ui/learn test` → fails. **Step 4:** implement the modules and `build.mjs`. **Step 5:** `check`, `test`, `build`, `verify` pass; bundle gzipped size printed by `build.mjs`.
- [ ] **Step 6: Bundle check in Rust** (`crates/hm-serve/tests/bundle.rs`, no Node needed): `the_committed_bundle_matches_its_source` recomputes the FNV-1a hash exactly as `build.mjs` does and compares it with `ui/learn/dist/SOURCE_HASH` (failure message: "ui/learn changed; run `npm --prefix ui/learn run build` and commit ui/learn/dist"); `the_bundle_stays_under_150_kb_gzipped` (flate2 on `app.js`).
- [ ] **Step 7: CI job** `ui` in `.github/workflows/ci.yml` on ubuntu-24.04: checkout (`persist-credentials: false`), `actions/setup-node@v7` with `node-version: "24"` and npm cache on `ui/learn/package-lock.json`, then `npm ci`, `npm run check`, `npm test`, `npm run verify`, all with `working-directory: ui/learn`.
- [ ] **Step 8:** Commit `feat(ui): panel toolchain, contract types, formatting and highlighting`.

---

### Task 6: Panel shell — top bar, tabs, sidebar, drawer cards, Code tab, live

**Model:** sonnet. **Wave B** (after Task 5).

**Files:**
- Create: `ui/learn/src/{state.ts, topbar.ts, tabs.ts, sidebar.ts, drawer.ts, code.ts, live.ts, dom.ts}`
- Modify: `ui/learn/src/{index.html, styles.css, main.ts}`, `ui/learn/dist/*`
- Test: `ui/learn/test/state.test.ts`, `ui/learn/test/drawer.test.ts`

**Interfaces:**
- Consumes: Task 5 modules and the API contract.
- Produces (for Task 8):
  ```ts
  // state.ts — one store, plain observer
  interface State { level: Level; tab: "journeys" | "map" | "code"; seq: number; updatedMs: number; live: boolean;
    overview: Overview | null; expanded: string[] /* group ids, outermost first */;
    selected: { path: string; symbol: string | null } | null; code: { path: string; from: number; to: number } | null }
  function getState(): State; function setState(patch: Partial<State>): void; function onChange(fn: (s: State, prev: State) => void): void;
  // drawer.ts
  function openFile(path: string, view: string): Promise<void>; function openSymbol(path: string, symbol: string): Promise<void>; function closeDrawer(): void;
  // code.ts
  function showCode(path: string, from?: number, to?: number): Promise<void>;
  // dom.ts
  function h(tag: string, attrs?: Record<string, string>, ...children: (Node | string)[]): HTMLElement;
  ```
- Layout (CSS grid): top bar; left sidebar 260 px; centre with tabs; right drawer 380 px (closed by default). The Map tab hosts `<div id="map">` (Task 8 draws into it) and above it `<div id="tech-strip" hidden>`.
- **Top bar:** "Harbormap Learn" mark (red), project name, `branch@commit` chip (`no git` when both are null), `Live · updated <ago>` with the green dot (re-rendered every second; grey "Offline — reconnecting" when the SSE is down), Beginner/Builder segmented switch (remembered in `localStorage` inside `try/catch`; default Beginner).
- **Tabs:** Journeys (empty state text exactly `Journeys arrive in the next version`, plus one line saying the map and cards already work), Map (default), Code (empty state "Pick a file on the map or search with /").
- **Sidebar:** search box (`/` focuses, debounce 120 ms, Enter opens the first hit; a file hit opens its card, a symbol hit its function card), results list; "Not on this map" list from `overview.excluded` (`tests · 120 files`); Journeys section with the same empty note.
- **Drawer, file card:** title (file name) with a path chip; `What it is for` placeholder line ("Written descriptions arrive with the prose version."); **Who uses it** (`used_by`, source before tests as given, Beginner shows the first 5 and "and N more"); **Jobs inside** (each symbol row: name, kind word, `calledFrom(level, n)`, never truncated; click → function card); **Uses**; **Technical details** fold (`<details>`: lines, language, class, parse status, unresolved references, symbol lines `l0–l1`); button **Open in Code**.
- **Drawer, function card:** name and kind; Builder shows the signature in mono; **Who calls it**, **What it calls**, **Calls Harbormap cannot follow yet** (`unresolved_calls`); Technical details (lines, path); **Open in Code** at `l0–l1`.
- A 404 on a card closes the drawer with a one-line notice "This file is gone from the project." (no stale card is left on screen).
- **Code tab:** path header, line numbers, `highlight.ts` per line carrying `LexState`, the range `from–to` marked and scrolled into view, a `truncated` notice; text built as one HTML string per render.
- **Live (`live.ts`):** `subscribe`; on `hello` with a `seq` greater than the state's → refetch overview; on `facts` → set `updatedMs`, refetch overview, refetch the open card if its path is in `files_changed` or `files_removed`, reload the Code tab if its file changed; discard any response whose `seq` is older than the state's.

- [ ] **Step 1: Write the failing tests:** `state.test.ts`: `setState` notifies with `prev`; level change keeps `expanded`. `drawer.test.ts`: the pure row builders `fileCardRows(card, level)` and `symbolCardRows(card, level)` (export them from `drawer.ts`) — Beginner limits `used_by` to 5 plus "and N more"; job rows keep full names; Builder adds the signature.
- [ ] **Step 2:** `npm --prefix ui/learn test` → fails. **Step 3:** implement. **Step 4:** `check`, `test`, `build`, `verify` pass; `cargo test -p hm-serve --test bundle` passes.
- [ ] **Step 5:** Commit `feat(ui): panel shell with cards, code view, levels and live state`.

---

### Task 7: `hm-serve` map endpoints and payload budgets

**Model:** sonnet. **Wave C** (after Tasks 3 and 4).

**Files:**
- Modify: `crates/hm-serve/src/api.rs`, `crates/hm-serve/src/lib.rs` (routes only)
- Test: `crates/hm-serve/tests/map_api.rs`, `crates/hm-serve/tests/budgets.rs`

**Interfaces:**
- Consumes: `Map::{overview, group, file_links}`, `Model`, `Handle`.
- Produces: `GET /api/overview` and `GET /api/group/{id}` exactly as in the API contract (`updated_ms` is the publish time of the current model in Unix ms; `excluded` sorted by class), and the file card's `links` = `map.file_links(path, view)` (`view` defaults to `overview`; `null` when the file is not on the map, such as a test file).

- [ ] **Step 1: Write the failing tests:** `map_api.rs`: `the_overview_has_boxes_arrows_and_excluded_counts`, `an_unknown_group_is_404`, `the_group_of_a_top_box_has_children_and_breadcrumb`, `publishing_bumps_the_seq_in_the_overview`, `the_file_card_links_lift_to_the_view` (`view=overview` names top boxes; `view=<group>` names that group's children), `a_test_file_card_has_null_links`. `budgets.rs` (a server on `hm_layout::synth::input(7, 5000)` converted into a `Model` through a test constructor `Model::from_input(input, excluded)` — add it as `#[doc(hidden)] pub`): `the_first_screen_is_under_100_kb_gzipped` (index.html + app.js + app.css + overview + journeys + tech, gzip each with flate2 and sum), `every_group_view_is_under_50_kb_gzipped` (walk every expandable node), `a_card_of_the_busiest_file_is_under_50_kb_gzipped` (index `hm_index::synth::project(dir, 7, 400)` and request the card of its most-imported file), `no_response_lists_every_file` (overview + any group carries fewer file paths than `files_total`).
- [ ] **Step 2:** fails. **Step 3:** implement. **Step 4:** passes; workspace checks.
- [ ] **Step 5:** Commit `feat(serve): overview and group endpoints within the payload budgets`.

---

### Task 8: The Map view in SVG, with drill-down, chips, file links and the technologies strip

**Model:** sonnet. **Wave C** (after Task 6).

**Files:**
- Create: `ui/learn/src/{map.ts, svg.ts, geometry.ts, techstrip.ts}`
- Modify: `ui/learn/src/{main.ts, styles.css}`, `ui/learn/dist/*`
- Test: `ui/learn/test/geometry.test.ts`

**Interfaces:**
- Consumes: Task 6 (`State`, `setState`, `onChange`, `openFile`, `openSymbol`), Task 5 (`api.ts`, `format.ts`).
- Produces: `renderMap(root: HTMLElement): void` (subscribes to state), `geometry.ts` pure helpers: `edgePoint(box, toward: {x, y}): {x, y}` (where the line from the box centre toward a point leaves the box), `arrowPath(from: Box, to: Box): string` (a gentle quadratic curve, offset sideways so A→B and B→A never overlap), `strokeWidth(count: number): number` (`1.25 + log2(count)`, capped at 6), `fit(boxes, viewport): {x, y, w, h}` (viewBox with 32 px margin).

Behaviour:
- **Overview:** one `<g>` per box: rounded rect with a role-colour left bar, name, role word (Beginner) or folder path (Builder), file count; arrows under boxes with an arrowhead marker and the count as a label at mid-curve; Beginner shows `arrowsFor` (≤ 10), Builder ≤ 40; when `hidden_arrows > 0` a small note "N more links hidden". The level switch only toggles visibility (no fetch, no new positions).
- **Expand:** clicking an expandable box fetches its group, grows that box from its own top-left to the group's `w × h` over 240 ms (`requestAnimationFrame`, ease-out), dims every other box to 35 % opacity, draws the children at their local positions and the arrows among them, and lists chips down the expanded box's right edge: in Beginner the first 3 chips plus `+N elsewhere` (N = remaining chips + `elsewhere_more`), in Builder all chips. A breadcrumb above the map shows `Overview › group › …`; clicking a crumb or pressing Esc collapses one level; clicking an expandable child drills in (the same growth, inside the expanded box).
- **Selection:** clicking a file child (or a search hit, using the card's `links`) gives it a white outline, opens its file card (`openFile(path, view)`), and draws only that file's links to the visible nodes (`links.incoming`/`outgoing`) in the ink colour; other arrows fade. File edges are drawn only for the selected file.
- **Live:** when the overview is refetched, boxes keep their positions (same ids); a box whose `files`/arrows changed pulses once (opacity 0.6 → 1 over 300 ms). If the expanded group's id is in `groups_changed` it is refetched; a 404 on it collapses to the overview.
- **Camera:** `fit` the overview into the viewport on load and on resize; wheel zoom and drag pan on the SVG `viewBox`; double-click resets.
- **Technologies strip** (`techstrip.ts`): fetch `/api/tech`; when `categories` is empty the strip stays `hidden`; otherwise one chip per category (L5b fills it).

- [ ] **Step 1: Write the failing tests** (`geometry.test.ts`): `edgePoint` on a 200 × 72 box toward a far-right point returns `x = box.x + 200`; `arrowPath(a, b) !== arrowPath(b, a)` and their midpoints differ; `strokeWidth(1) === 1.25`, `strokeWidth(1e6) === 6`; `fit` contains every box.
- [ ] **Step 2:** fails. **Step 3:** implement. **Step 4:** `check`, `test`, `build`, `verify` pass; gzipped `app.js` stays ≤ 150 KB.
- [ ] **Step 5:** Commit `feat(ui): map view with in-place drill-down, chips and file links`.

---

### Task 9: `harbormap learn` — index, watch, serve, open the browser; end-to-end smoke

**Model:** sonnet. **Wave D** (after Tasks 7 and 8).

**Files:**
- Create: `crates/harbormap/src/learn.rs`
- Modify: `crates/harbormap/src/cli.rs` (one variant `Learn(LearnArgs)` appended after `Card`, one `LearnArgs` struct), `crates/harbormap/src/main.rs` (one `mod learn;`, one match arm), `crates/harbormap/Cargo.toml` (deps `hm-layout`, `hm-serve`, `tokio` with `rt-multi-thread`, `signal`; dev `ureq`)
- Test: `crates/harbormap/tests/learn_serve.rs`

**Interfaces:**
- Consumes: `crate::index::open`, `hm_index::{location::{facts_db, worktree_root}, watch::{Batch, Watcher}}`, `hm_project::git::discover`, `hm_serve::{Server, ServeConfig, Model, Repo, FactsDelta, Handle}`.
- Produces: `harbormap learn [path] [--no-open]` (and the global `--json`).
  ```rust
  /// Run the panel for the project at `path` (the current folder when left out).
  #[derive(Debug, Args)] pub struct LearnArgs { pub path: Option<PathBuf>, #[arg(long)] pub no_open: bool }
  ```

Behaviour:
- Resolve the worktree root, open the indexer (`crate::index::open`), run `full()`, load `Model` (project name = root folder name), find branch and 7-char commit of this worktree through `discover(root)` (`None` without git).
- Start a tokio multi-thread runtime (2 workers), `Server::bind`, print. Text: `Harbormap Learn is running for <name>` / `  Open: <url>` / `  Ctrl-C stops it.`; JSON: one line `{"url": ..., "port": ...}`, then one line per published delta `{"seq", "files", "took_ms"}`.
- Unless `--no-open`, open the URL: `open <url>` (macOS), `xdg-open <url>` (Linux), `cmd /C start "" "<url>"` (Windows); a failure prints `Could not open a browser; open the link above.` and continues.
- The indexer runs on its own thread with the same loop as `harbormap index --watch` (catch-up rescan at start, a failed batch forces a rescan, a changed ignore rule starts a new watcher; copy the private helper rather than changing `index.rs`). After a non-empty delta: `Model::load(store, name, Some(&previous))`, `changed_since`, then `handle.publish(model, FactsDelta { took_ms: batch.first_event.elapsed(), files_changed, files_removed, regrouped, groups_changed, .. })`.
- Ctrl-C (`tokio::signal::ctrl_c`) stops the server; the process exits 0.

- [ ] **Step 1: Write the failing tests** (`learn_serve.rs`, spawning the built binary with `HARBORMAP_DATA_DIR` set to a temp dir, `--no-open --json`, on a temp synthetic shop project; read the first stdout line for the URL):
  - `learn_serves_the_overview_of_the_project` (cookie from `?t=` redirect or bearer; overview names the project folder; boxes non-empty).
  - `a_saved_file_arrives_as_a_facts_event` (open `/api/events`, read `hello`, write a body change to `src/orders/service.ts`, receive `event: facts` listing that path within 3 s; print the measured write→event milliseconds).
  - `deleting_a_file_sends_a_removed_delta` (and `/api/card` for it is then 404).
  - `learn_refuses_the_home_folder` (non-zero exit, message from `LocationError`).
  - `learn_without_json_prints_the_link` (text output contains `Open: http://127.0.0.1:` and `?t=`).
  Kill the child at the end of each test.
- [ ] **Step 2:** fails. **Step 3:** implement. **Step 4:** `cargo test -p harbormap --test learn_serve` passes; workspace checks.
- [ ] **Step 5: Live latency check.** Add `#[ignore] live_latency_on_1000_files` to `learn_serve.rs`: write `hm_index::synth::project(dir, 7, 1000)`, start the binary, open `/api/events`, make 50 body edits 400 ms apart (`hm_index::synth::edit_function`), and measure write → `facts` event per edit; print p50/p95 and assert p95 < 1 000 ms (a noise guard; the spec target is 300 ms). Run it once with `cargo test --release -p harbormap --test learn_serve -- --ignored --nocapture` and put the numbers in the task report.
- [ ] **Step 6:** Commit `feat(learn): harbormap learn indexes, watches and serves the panel`.

---

## Execution waves (for the controller)

| Wave | Tasks (parallel within a wave, each in its own worktree) |
|---|---|
| 0 | Task 0 (controller) |
| A | Task 1 (sonnet), Task 2 (opus), Task 5 (sonnet) |
| B | Task 3 (opus), Task 4 (sonnet), Task 6 (sonnet) |
| C | Task 7 (sonnet), Task 8 (sonnet) |
| D | Task 9 (sonnet) |
| — | Whole-branch review (opus), then the private smoke (controller) |

Tasks in one wave touch disjoint files. The controller cherry-picks each wave's commits in task order and runs the full checks after each pick.
