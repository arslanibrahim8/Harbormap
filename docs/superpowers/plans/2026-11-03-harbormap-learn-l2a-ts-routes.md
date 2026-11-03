# Harbormap Learn, Plan L2a: TS/JS routes, entries, Next.js and package.json — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `harbormap index` finds the routes and entry points of a TS/JS project (Next.js app and pages routers, server actions, middleware/proxy, Express, NestJS, `package.json` entries), links browser code to those routes (`fetch`, axios, generated clients, forms, React Query, SWR, server-action forms), reads `package.json` manifests with lockfile versions, records patterns and gaps, and classifies noise refs. Everything stays incremental-equals-cold and zero-token.

**Architecture:** Three layers, each a pure function of the one before.
- **Extraction** (per file, at parse time): a new `WebFacts` part of `FileFacts`, filled by `ts/web/` (three query files, one per area). Express route callbacks become synthetic symbols here (spec §5.1).
- **Manifests** (per `package.json` and lockfile): `manifest/` parses them from bytes, cached by content hash.
- **Derive** (whole project, after resolution): `routes/` turns facts + manifests into routes, client calls, inferred edges, patterns and gaps. It runs on every pass that changed anything; its output is written to the store as a row-level diff, so incremental equals cold by construction.

**Tech Stack:** Rust 1.99.0, edition 2024. No new crates: tree-sitter 0.27.1 (TS/TSX/JS grammars), jsonc-parser 0.34.0, rusqlite 0.41.1, serde/serde_json, toml 1.1.8 (pack), blake3, proptest, tempfile, assert_cmd.

**Spec:** `docs/superpowers/specs/2026-11-02-harbormap-learn-engine-design.md`. This plan implements §5.1 (routes, entries, gaps for TS/JS), §5.2 (`matched`/`config`/`inferred` evidence), §5.5 (`route`, `gap`, `pattern` tables), §6.3 (every TS/JS row, client → server hops, the TS framework-mediated hops), §7.3 (TS libraries), §7.6 (what the manifest must supply), §11.3 golden tests and the L2a entry of §14. Conventions: `docs/superpowers/plans/2026-11-02-harbormap-learn-l1-indexer-core.md`.

## Where this plan fits

| Plan | Delivers |
|---|---|
| L1 (done) | store, walker, TS/JS facts and resolution, incremental updates, watcher, `index`, `card` |
| **L2a (this plan)** | schema v2, web facts, routes and entries (TS/JS), manifests, client → server hops, mediated hops, patterns, gaps, ref noise |
| L3 | journeys, cards, technologies list (consumes the manifest and import queries of Task 13) |
| L2b | Python, Go, Java, C#, Rust (adds rows to the same `route` table) |

**Scope decisions inside L2a** (none removes a spec requirement):
- **No `harbormap routes` command.** Spec §4 has none. Routes show in `harbormap index` (summary + list), `index --json` (`routes` array) and `harbormap card` (role line).
- **Route ids.** `r_` + BLAKE3 over `(root, entry_kind, method, pattern)`, where `root` is the manifest root (`""` for the repository root). Spec §5.3 names only method and path; root and kind are added so two apps in one monorepo, and a page and an API route on the same path, never collide. A second route with the same four parts is dropped with a note (`"app/x/route.ts:3: GET /x is also declared at app/x/page.tsx:1"`), so every id is unique and edges to it are valid.
- **Route pattern spelling.** Segments joined by `/`, always starting with `/`. Parameters are `{name}`, catch-alls `{...name}`, optional catch-alls `{...name?}`. Express/NestJS `:id` → `{id}`, `*` → `{...}`. Client URLs replace each `${…}` with `{}`, drop `?query` and `#hash`, and drop a literal `http(s)://host` prefix only when the host is `localhost`, `127.0.0.1`, `0.0.0.0` or `[::1]` (any port). An absolute URL to any other host keeps it as `//host/path` and never matches; so does a protocol-relative `//host/path`. An axios call on an instance bound at module level (in the same file, or imported from the file that binds and exports it) to `axios.create({ baseURL })` has a literal `baseURL` joined before a relative URL; a `baseURL` that is not a literal makes the URL start with `{}`.
- **Methods.** HTTP verbs upper-case; `ANY` for a route that takes every method (pages-router API, `app.all`, middleware); `ACTION` for server actions; `BIN`, `MAIN`, `START` for `package.json` entries. Pages are `GET`.
- **`route.handler_id`** holds a symbol id (`y_…`), a file id (`f_…`) when the handler is the file itself (a `bin` script, a middleware whose default export is an expression), or NULL.
- **Entry kinds.** Spec's list plus `middleware` (Next.js middleware/proxy, so "on the way in" has a row to point from).
- **Next.js needs a manifest.** Next.js rules apply only under a manifest root whose `package.json` declares `next` (any section). A `pages/` or `app/` folder in another project is not a route tree.
- **Middleware matchers.** A matcher made only of literal paths and `:param`, `:param?`, `:param*`, `:param+` segments is evaluated as a full-path match, as the [Next.js docs](https://nextjs.org/docs/app/api-reference/file-conventions/proxy) say (`/about/:path` matches `/about/a`, not `/about/a/c`): a literal or `:param` takes exactly one segment, `:param?` zero or one, `:param*` zero or more, `:param+` one or more; a route's leftover segments are covered only by a trailing `*`/`+` parameter, a route catch-all takes whatever the matcher still has, and `/` covers only `/`. Any other matcher (regex groups, lookaheads) makes the middleware apply to every route of its app, `conf = probably`, and its `detail` says `"evaluated": false` (spec: "applies to many routes").
- **"On the way in" edges** are `calls` edges whose source is a route id (`r_…`) and whose target is the middleware, guard or `use` function, with rules `ts.next.middleware`, `ts.express.use`, `ts.nest.guard`. L3 tells them from call chains by the `r_` source.
- **Client calls are stored even when they match nothing** (`client_call` table), so L2b's Python routes match the fastapi-template frontend without re-indexing it. A match becomes an `http_calls` edge from the calling symbol to the route id.
- **Matching.** Segment by segment; a route `{p}` matches any one segment, `{...p}` the rest; a client `{}` matches only a parameter. Method must equal, or either side is `ANY` (a form `method`/`formMethod` that is not a string literal is `ANY`). Programmatic clients (`fetch`, axios, request helpers, SWR) match `http` routes only; forms (`ts.client.form`, `ts.client.form_self`) match pages and `http` routes. A call looks first among the routes of its own manifest root (the `package.json` that governs its file, `""` when none); only when none of them fits does it look among the routes of every root. The most specific route wins (more literal segments); a tie gives no edge (no guessing), also across roots. A URL whose first part is `${…}` is never matched. A form whose `action` has no path (`""`, `#…`, `?…`) is a form without action: on a page it submits to that page (`ts.client.form_self`).
- **Express.** Only receivers bound in the same file to `express()`, `express.Router()` or `Router()` imported from `express`. `use('/pre', router)` joins prefixes across files through imports, up to 5 levels, cycle-guarded. A router never mounted keeps its own paths with `detail` `"mounted": false`. `app.route('/x').get(…)` chains are a documented limit.
- **Inner functions.** Nested functions stay non-symbols (L1). A hop whose target is a nested local name (`mutationFn: login` with `login` declared inside the hook) gives no edge; the fact stays in `facts_json` for L3's inner-name step labels (spec §7.2.12).
- **Env reads** become `env_read` patterns with the variable name in `detail`. `reads_env` edges need an env-name id, which no plan defines yet; they are left to L3.
- **Auth-guard pattern** (an early return on an identity check, §7.3) needs control flow; it is deferred to L3 with the PDG-free path walk. Every other §7.3 kind has TS rules.
- **Lockfiles.** npm (`package-lock.json`, `npm-shrinkwrap.json`, v1–v3), pnpm (`pnpm-lock.yaml` v6–v9 `importers`, read line by line, no YAML crate) and yarn (v1 and berry). `bun.lock` is deferred. A lockfile is read even when the walker skipped it as too large (up to 64 MiB).
- **Manifest reader location.** The new reader lives in `hm-index/src/manifest/`; `hm-project/src/manifest.rs` (P1) is not changed.
- **Ref noise.** Two new `ref.status` values: `builtin` (a JS/TS/DOM/Node global such as `JSON.parse`, `new Date`, `console.log`, `setTimeout`) and `local` (the callee or receiver root is not bound at module level and is not a known global: a parameter, a local variable or a nested function). `unresolved` now means "bound at module level, target not pinned".
- **`commander`/`yargs` subcommands** (spec §5.1, JS) are deferred to L2b with the other CLI-subcommand rules.
- **TanStack Router file routes** (fastapi-template frontend) are not a §6.3 rule; that frontend has client calls and a manifest, and zero routes.

## Global Constraints

- **Toolchain and licences.** Toolchain `1.99.0`, edition `2024`; `unsafe_code = "forbid"`; clippy warnings fail. No new dependency in this plan.
- **Zero tokens, no network at run time.** Implementers may read framework docs on the web while building (the [K] checks); the code never does.
- **Shared files** (session rules §6): root `Cargo.toml` untouched; `crates/hm-index/src/lib.rs` gains `pub mod` lines only; `crates/harbormap/src/cli.rs`/`main.rs` unchanged (no new subcommand). Only this plan changes the facts schema.
- **Commits.** `git commit -s`; every commit ends with exactly `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Conventional prefixes.
- **After every task:** `~/.cargo/bin/cargo fmt --all --check`, `~/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings`, `~/.cargo/bin/cargo test --workspace` clean.
- **Evidence** (spec §5.2). Route rows: Next.js, NestJS → `prov = matched`, `conf = sure`; Express (prefix join) → `matched`, `probably`; `package.json` entries → `config`, `sure`; middleware with an unevaluated matcher → `probably`. Every inferred edge: `prov = inferred`, `conf = probably`.
- **Rule ids** (the only spellings; L3 keys on them):

  | Area | Rule ids |
  |---|---|
  | Routes | `ts.next.app.page`, `ts.next.app.route`, `ts.next.pages.page`, `ts.next.pages.api`, `ts.next.action.file`, `ts.next.action.fn`, `ts.next.middleware`, `ts.express.route`, `ts.nest.route`, `npm.bin`, `npm.main`, `npm.start` |
  | Client calls (`client_call.rule` and `http_calls` edge rule) | `ts.client.fetch`, `ts.client.axios`, `ts.client.request_helper`, `ts.client.swr`, `ts.client.form`, `ts.client.form_self` |
  | Mediated `calls` edges | `ts.react_query.fn`, `ts.react_query.back`, `ts.swr.fetcher`, `ts.next.action.form`, `ts.next.action.state`, `ts.di.constructor` |
  | On the way in (`calls`, source `r_…`) | `ts.next.middleware`, `ts.express.use`, `ts.nest.guard` |
  | Patterns | `ts.pattern.<pack id>`, `ts.pattern.env`, `ts.pattern.di` |

- **No guessing.** A name that does not resolve to exactly one symbol gives no edge. Ties give no edge.
- **Determinism.** Every list in `Derived`, every query result and every printed list is sorted.
- **Privacy.** Fixtures, tests and docs use synthetic "shop" code or the public eval repositories only.
- **Latency** (spec §5.7, `examples/latency.rs`): body-only edit p95 ≤ 300 ms, cold ≤ 2 s per 1k files, still.
- **[K] rules.** Every rule marked [K] in spec §6.3 (Express, NestJS) and every Next.js file convention is checked against the official docs while building. The test of each rule cites the doc URL in a comment. If the docs disagree with this plan, follow the docs and say so in the task report.

## Review Focus

1. **A project that has an `app/` or `pages/` folder but is not a Next.js app** (a Vue app, an Express server with `pages/` templates) gets no Next.js routes. Task 7: `a_pages_folder_without_next_is_not_a_route_tree`.
2. **A monorepo with two Next.js apps that both have `/` and `/api/health`**, and one pnpm lockfile at the workspace root, keeps two sets of routes with distinct ids, each app's dependencies resolved from its own importer. Task 6: `pnpm_importers_resolve_per_manifest_root`. Task 7: `two_next_apps_keep_their_own_routes`.
3. **A route file deleted, renamed or broken mid-typing** drops (or keeps) exactly what a cold index would: its routes, the `http_calls` edges into them and the middleware edges from them. Task 8: `deleting_a_route_file_drops_its_routes_and_hops` and the extended property test.
4. **A `package.json` with comments, trailing commas, a wrong type (`"bin": 5`) or no closing brace, or a lockfile in an unknown format,** gives a note, never an error; what parsed is kept. Task 6: `a_broken_package_json_is_a_note_not_an_error`.
5. **A URL built from variables, an absolute URL, a query string, or two routes that match equally** never yields a wrong edge. Task 10: `variable_built_and_ambiguous_urls_never_match`.

---

## File map

| File | Responsibility | Task |
|---|---|---|
| `crates/hm-store/src/schema/facts_v2.sql`, `crates/hm-store/src/lib.rs` | Schema v2 migration | 1 |
| `crates/hm-core/src/ids.rs` | `route_id` | 1 |
| `crates/hm-index/tests/common/mod.rs` | `dump` covers the new tables | 1 |
| `crates/hm-index/src/facts.rs` | `WebFacts` and its parts | 2 |
| `crates/hm-index/src/ts/web/mod.rs` | Web extraction entry, spans, URL normaliser | 2 |
| `crates/hm-index/src/ts/web/{next,client,server}.rs`, `ts/web/queries/{next,client,server}.scm` | Web extraction by area | 3, 4, 5 |
| `crates/hm-index/src/ts/resolve.rs` | Public resolution helpers (2); ref noise (11) | 2, 11 |
| `crates/hm-index/src/manifest/{mod,npm,lock}.rs` | `package.json` and lockfiles | 6 |
| `crates/hm-index/src/routes/mod.rs`, `routes/next.rs`, `routes/entries.rs` | `Derived`, `derive`, Next.js, `package.json` entries | 7 |
| `crates/hm-index/src/routes/{express,nest}.rs` | Express, NestJS, constructor DI | 9 |
| `crates/hm-index/src/routes/hops.rs` | Client → server matching, mediated hops | 10 |
| `crates/hm-index/src/routes/patterns.rs`, `packs/learn/patterns-ts.toml` | Patterns and gaps | 12 |
| `crates/hm-index/src/index.rs`, `crates/hm-index/src/synth.rs` | Running derive, writing it, report, delta, synthetic routes | 8 |
| `crates/hm-index/src/query.rs` | Route, manifest and import queries; role labels | 13 |
| `crates/harbormap/src/{index,card}.rs` | CLI output | 13 |
| `crates/hm-index/tests/fixtures/{next-app,next-pages,next-mono,express-api,nest-api,client-hops,npm-locks}/**` | Golden fixtures | 3–12 |
| `crates/hm-index/tests/eval_l2a.rs` | Eval gate | 14 |

## Execution order

| Wave | Tasks (parallel inside a wave; disjoint files) |
|---|---|
| 1 | 1 |
| 2 | 2 |
| 3 | 3, 4, 5, 6, 11 |
| 4 | 7 |
| 5 | 8, 9, 10, 12 |
| 6 | 13 |
| 7 | 14 |

---

### Task 1: Facts schema v2, route ids, and the dump

**Files:**
- Create: `crates/hm-store/src/schema/facts_v2.sql`
- Modify: `crates/hm-store/src/lib.rs` (`FACTS_SCHEMA = 2`, migration list, tests)
- Modify: `crates/hm-core/src/ids.rs` (`route_id`)
- Modify: `crates/hm-index/src/index.rs` (`sync_resolved` and `count_edges` touch only `prov = 'parsed'` rows)
- Modify: `crates/hm-index/tests/common/mod.rs` (`dump` adds the new tables)

**Interfaces:**
- Produces:
  - `hm_core::ids::route_id(root: &str, entry_kind: &str, method: &str, pattern: &str) -> String`: BLAKE3 over `"route\0" + root + "\0" + entry_kind + "\0" + method + "\0" + pattern`, `r_` + 12 hex.
  - `hm_store::FACTS_SCHEMA == 2`. `facts_v2.sql`, applied after v1:

    ```sql
    -- prov joins the edge key: a parsed and an inferred edge with the same ends and line both stand.
    CREATE TABLE edge_v2(kind TEXT NOT NULL, src_id TEXT NOT NULL, dst_id TEXT NOT NULL,
      file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE, line INTEGER NOT NULL,
      rule TEXT NOT NULL, prov TEXT NOT NULL, conf TEXT NOT NULL,
      PRIMARY KEY(kind, src_id, dst_id, line, prov));
    INSERT INTO edge_v2 SELECT kind, src_id, dst_id, file_id, line, rule, prov, conf FROM edge;
    DROP TABLE edge;
    ALTER TABLE edge_v2 RENAME TO edge;
    CREATE INDEX edge_dst ON edge(dst_id, kind);
    CREATE INDEX edge_file ON edge(file_id);
    CREATE INDEX edge_prov ON edge(prov);
    CREATE TABLE route(id TEXT PRIMARY KEY, root TEXT NOT NULL, entry_kind TEXT NOT NULL,
      method TEXT NOT NULL, pattern TEXT NOT NULL, handler_id TEXT,
      file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE, line INTEGER NOT NULL,
      rule TEXT NOT NULL, prov TEXT NOT NULL, conf TEXT NOT NULL, detail TEXT);
    CREATE INDEX route_file ON route(file_id);
    CREATE INDEX route_handler ON route(handler_id);
    CREATE TABLE client_call(file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE,
      src_id TEXT NOT NULL, line INTEGER NOT NULL, method TEXT NOT NULL, url TEXT NOT NULL,
      rule TEXT NOT NULL, PRIMARY KEY(file_id, line, method, url, rule));
    CREATE TABLE manifest(file_id TEXT PRIMARY KEY REFERENCES file(id) ON DELETE CASCADE,
      root TEXT NOT NULL, kind TEXT NOT NULL, name TEXT, lockfile TEXT, detail TEXT NOT NULL);
    CREATE TABLE dependency(file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE,
      section TEXT NOT NULL, name TEXT NOT NULL, declared TEXT NOT NULL, resolved TEXT,
      dev INTEGER NOT NULL, line INTEGER NOT NULL, PRIMARY KEY(file_id, section, name));
    CREATE TABLE pattern(file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE,
      subject_id TEXT NOT NULL, kind TEXT NOT NULL, line INTEGER NOT NULL, rule TEXT NOT NULL,
      conf TEXT NOT NULL, term TEXT, detail TEXT, PRIMARY KEY(file_id, subject_id, kind, line, rule));
    CREATE TABLE gap(file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE,
      subject_id TEXT NOT NULL, kind TEXT NOT NULL, line INTEGER NOT NULL,
      PRIMARY KEY(file_id, kind, line));
    -- Facts gain web facts in L2a: every file is parsed again on the next pass.
    UPDATE file SET facts_json = NULL;
    ```

    `manifest.kind` is `npm`; `manifest.lockfile` is the lockfile's relative path; `manifest.detail` is JSON `{"bin":[[name,target,line]],"main":[path,line]|null,"scripts":[[name,command,line]]}`. `dependency.section` is `dependencies`, `devDependencies`, `peerDependencies` or `optionalDependencies`; `dev` is 1 only for `devDependencies`.
  - `sync_resolved` reads, deletes and counts only `prov = 'parsed'` edges (fix the comment that says L2 must add prov). `replace_resolved` still clears every edge of the file; Task 8 re-adds the inferred ones in the same transaction.
  - `common::dump` appends sorted sections for `route`, `client_call`, `manifest`, `dependency`, `pattern`, `gap`, every column, ids mapped to names the way edges already are.

- [ ] **Step 1: Write the failing tests** in `crates/hm-store/src/lib.rs`:
  - rename `fresh_store_has_schema_1_and_wal` → `fresh_store_has_schema_2_and_wal` (asserts `2`);
  - `a_newer_schema_is_refused` expects `(99, 2)`;
  - `a_v1_store_migrates_and_keeps_its_rows`: build a file with `facts_v1.sql` and `user_version = 1`, insert one file row with `facts_json = '{}'` and one `parsed` edge; `Store::open` → version 2, the edge row is still there, `facts_json IS NULL`, and inserting the same edge with `prov = 'inferred'` succeeds;
  - `deleting_a_file_cascades` also inserts one row in each new table and expects all of them empty after the delete.

  In `crates/hm-core/src/ids.rs`: `route_ids_are_stable_and_keyed_by_root_and_kind` — same input same id, `r_` + 12 hex; differs when root, kind, method or pattern differs.
- [ ] **Step 2:** `~/.cargo/bin/cargo test -p hm-store -p hm-core` → the new tests fail.
- [ ] **Step 3:** Write `facts_v2.sql`, append `(2, include_str!("schema/facts_v2.sql"))`, bump `FACTS_SCHEMA`; add `route_id`; restrict `sync_resolved`/`count_edges`; extend `dump`.
- [ ] **Step 4:** `~/.cargo/bin/cargo test --workspace` → all pass (the L1 property test still passes: no inferred edges exist yet).
- [ ] **Step 5: Commit** `feat(store): facts schema v2 for routes, manifests, patterns and gaps`.

---

### Task 2: Web facts model, extraction plumbing and resolution helpers

**Files:**
- Modify: `crates/hm-index/src/facts.rs`
- Create: `crates/hm-index/src/ts/web/mod.rs`, empty stubs `ts/web/next.rs`, `ts/web/client.rs`, `ts/web/server.rs`
- Modify: `crates/hm-index/src/ts/mod.rs` (`pub mod web;`), `crates/hm-index/src/ts/extract.rs` (calls into `web`)
- Modify: `crates/hm-index/src/ts/resolve.rs` (public helpers; `call_source` uses `source_id`)
- Test: `crates/hm-index/tests/ts_web.rs` (new)

**Interfaces:**
- Produces in `facts.rs` (all `Debug, Clone, PartialEq, Serialize, Deserialize`; enums `rename_all = "snake_case"`; every list in source order):

  ```rust
  pub type Owner = Option<(String, String)>; // (owner, name) of the enclosing symbol, None at top level

  #[serde(tag = "t", content = "v")]
  pub enum ArgFact {
      Str(String),                    // string literal; template literal with each `${…}` as `{}`
      Name(String),                   // identifier or member path: `createInvoice`, `UsersService.readUserMe`
      Inline(Option<String>),         // inline function; Some(name) when it became a synthetic symbol
      Object { path: Option<String> },// object literal; its `path:` string property
      Other,
  }
  pub struct DirectiveFact { pub owner: Owner, pub function: Option<String>, pub text: String, pub line: u32 }
  //   function: None for the file prologue; Some(name) of the function whose body starts with it ("" if anonymous)
  pub struct UrlCallFact { pub owner: Owner, pub client: String, pub method: Option<String>, pub url: String, pub line: u32 }
  //   client: "fetch" | "axios" | "request_helper" | "swr"; method upper-case; url normalised by `normalize_client_url`
  pub struct RouteCallFact { pub owner: Owner, pub receiver: String, pub method: String, pub args: Vec<ArgFact>, pub line: u32 }
  pub struct BindingFact { pub owner: Owner, pub name: String, pub init: String, pub line: u32 }
  pub struct DecoratorFact { pub target: (String, String), pub name: String, pub args: Vec<ArgFact>, pub line: u32 }
  pub struct CtorFieldFact { pub class: String, pub field: String, pub type_name: String, pub line: u32 }
  pub struct FormFact { pub owner: Owner, pub element: String, pub action: Option<ArgFact>, pub method: Option<String>, pub on_submit: bool, pub line: u32 }
  pub struct AliasFact { pub owner: Owner, pub local: String, pub target: String, pub via: String, pub line: u32 }
  pub struct HookFact { pub owner: Owner, pub hook: String, pub key: String, pub target: ArgFact, pub line: u32 }
  pub struct MatcherFact { pub values: Vec<String>, pub line: u32 }
  pub struct GapFact { pub owner: Owner, pub kind: String, pub line: u32 }
  pub struct EnvFact { pub owner: Owner, pub name: String, pub line: u32 }

  #[derive(Default)] #[serde(default)]
  pub struct WebFacts {
      pub directives: Vec<DirectiveFact>, pub url_calls: Vec<UrlCallFact>, pub route_calls: Vec<RouteCallFact>,
      pub bindings: Vec<BindingFact>, pub decorators: Vec<DecoratorFact>, pub ctor_fields: Vec<CtorFieldFact>,
      pub forms: Vec<FormFact>, pub aliases: Vec<AliasFact>, pub hooks: Vec<HookFact>,
      pub matcher: Option<MatcherFact>, pub gaps: Vec<GapFact>, pub env: Vec<EnvFact>,
  }
  impl WebFacts { pub fn is_empty(&self) -> bool }
  // FileFacts gains, last:
  #[serde(default, skip_serializing_if = "WebFacts::is_empty")] pub web: WebFacts,
  ```

  `FileFacts::empty` sets `web: WebFacts::default()`. A file without web facts serialises exactly as in L1.
- Produces in `ts/web/mod.rs`:
  - `pub(crate) struct Span { pub start: usize, pub end: usize, pub owner: String, pub name: String }` and `pub(crate) fn owner_at(spans: &[Span], pos: usize) -> Owner` (innermost span, as `Extractor::enclosing`).
  - `pub(crate) struct Callback { pub fact: SymbolFact, pub start: usize, pub end: usize }`.
  - `pub(crate) fn route_callbacks(root: Node, src: &[u8], lang: Lang) -> Vec<Callback>` → `server::route_callbacks` (stub: empty).
  - `pub(crate) fn extract_web(root: Node, src: &[u8], lang: Lang, spans: &[Span]) -> WebFacts` → calls `next::extract`, `client::extract`, `server::extract`, each `fn extract(root: Node, src: &[u8], lang: Lang, spans: &[Span], out: &mut WebFacts)` (stubs: nothing).
  - `pub(crate) fn compile(scm: &str, lang: Lang) -> Option<tree_sitter::Query>` reusing `extract::query_source` (make it `pub(crate)`), with one `OnceLock` per (area, lang) kept by each area module.
  - `pub fn normalize_client_url(raw: &str) -> String`: input is the literal's text without quotes, with `${…}` already replaced by `{}`; drops `?…`/`#…`, drops a leading `http://host` or `https://host`, collapses `//`, keeps a leading `{}`.
- `extract.rs` changes: after `collect_symbols`, push `web::route_callbacks(...)` into `ex.symbols` (as `Sym`), so calls inside a callback get it as owner; after `collect_calls`, set `web = web::extract_web(root, bytes, lang, &spans)` with spans built from `ex.symbols`.
- Produces in `resolve.rs`:
  - `pub fn source_id(rel: &str, facts: &FileFacts, owner: Option<&(String, String)>, line: u32) -> String` — the logic of `Resolver::call_source`, which now calls it.
  - `pub fn resolve_callee(p: &Project, rel: &str, receiver: Option<&str>, callee: &str) -> Option<(String, usize)>` — `resolve_name` with `jsx = false`, `owner = None`, `heritage = false`; `Some((file, symbol index))` only for an edge outcome.
  - `pub fn binding_package(p: &Project, rel: &str, local: &str) -> Option<(String, String)>` — `(package, imported name)` when `local` is bound by an import (or require) that resolves to an external package; else `None`.

- [ ] **Step 1: Write the failing tests** in `crates/hm-index/tests/ts_web.rs`:
  - `files_without_web_facts_serialise_as_in_l1`: `extract("export function a() { return b(); }", Lang::TypeScript)` → `serde_json::to_value` has no `web` key; round-trips.
  - `old_facts_json_without_web_reads`: `serde_json::from_str::<FileFacts>` of an L1 JSON string (no `web`) → `web.is_empty()`.
  - `client_urls_normalise`: `normalize_client_url` on `"/api/orders/{}"` → `"/api/orders/{}"`; `"/api/orders?page=2"` → `"/api/orders"`; `"https://shop.example/api/x#top"` → `"/api/x"`; `"{}/api/orders"` → `"{}/api/orders"`; `"/api//x/"` → `"/api/x"`.
  - `resolution_helpers_name_symbols_and_packages` on a two-file in-memory project (`a.ts`: `import express from 'express'; import { place } from './b'; export function run() { place(); }`, `b.ts`: `export function place() {}`): `resolve_callee(p, "a.ts", None, "place") == Some(("b.ts", 0))`; `binding_package(p, "a.ts", "express") == Some(("express", "default"))`; `source_id("a.ts", facts, Some(&("".into(), "run".into())), 1)` is the id of `run`.
- [ ] **Step 2:** `~/.cargo/bin/cargo test -p hm-index --test ts_web` → fails to compile.
- [ ] **Step 3:** Implement the model, the plumbing with stubs, and the helpers. Fix every `FileFacts { … }` literal in the crate and its tests.
- [ ] **Step 4:** `~/.cargo/bin/cargo test --workspace` → all pass, L1 golden tests unchanged.
- [ ] **Step 5: Commit** `feat(index): web facts model and extraction plumbing`.

---

### Task 3: Next.js and React form facts (`ts/web/next.rs`)

**Files:**
- Modify: `crates/hm-index/src/ts/web/next.rs`; Create: `crates/hm-index/src/ts/web/queries/next.scm`
- Create: fixture `crates/hm-index/tests/fixtures/next-app/**` (shared with Task 7, which adds to it); Test: `crates/hm-index/tests/web_next.rs`

**Interfaces:**
- Consumes: Task 2 types, `Span`, `owner_at`, `compile`.
- Produces `next::extract` filling:
  - `directives`: a `'use server'`, `'use client'` or `'use cache'` expression statement that is first in the file (owner `None`, `function: None`) or first in a function body (`function: Some(name)`, the function's own name, `""` for an anonymous one; `owner` from `owner_at`).
  - `forms`: every JSX `form` element (`action` attribute) and every `button`/`input` with `formAction`; `action` is `Str` for a string literal, `Name` for `{ident}` or `{a.b}`, `Other` for any other expression, `None` when absent; `method` upper-cased from a literal `method` attribute; `on_submit` when an `onSubmit` attribute exists.
  - `aliases`: inside a function, `const L = X.bind(…)` → `{local: L, target: X, via: "bind"}`; `const [a, L, …] = useActionState(X, …)` or `useFormState(X, …)` → `{local: L, target: X, via: "useActionState"}` (second array element). `X` must be an identifier.
  - `matcher`: `export const config = { matcher: … }` where `matcher` is a string, an array of strings, or an array of objects with a `source` string; every string in source order.

- [ ] **Step 1: Write the fixture and failing tests.** Fixture `next-app/` (all synthetic "shop" code): `package.json` (`{"name":"shop-web","dependencies":{"next":"15.2.0","react":"19.0.0"}}`), `app/lib/actions.ts` (file `'use server'`, exports `placeOrder`, `cancelOrder`), `app/ui/order-form.tsx` (`const [state, formAction] = useActionState(placeOrder, null)`, `<form action={formAction}>`), `app/ui/cancel-button.tsx` (`const cancelWithId = cancelOrder.bind(null, id)`, `<form action={cancelWithId}>`), `app/ui/inline.tsx` (a component with an inner `async function save() { 'use server'; … }` and `<form action={save}>`), `app/ui/search.tsx` (`<form action="/search" method="get">` and `<form onSubmit={…}>`), `middleware.ts` (`export const config = { matcher: ['/account/:path*', '/checkout'] }`). Tests:
  - `file_directive_is_found` (actions.ts: one directive, `owner == None`, `function == None`, `text == "use server"`, line 1);
  - `function_directive_names_its_function` (inline.tsx: `function == Some("save")`, owner is the component);
  - `forms_and_aliases_are_recorded` (order-form: alias `formAction → placeOrder` via `useActionState`, form action `Name("formAction")`; cancel-button: alias via `bind`; search.tsx: one form `Str("/search")` method `GET`, one with `action: None, on_submit: true`);
  - `middleware_matcher_strings_are_read` (`["/account/:path*", "/checkout"]`);
  - `a_directive_after_code_is_not_a_directive` (`const a = 1; 'use server';` → none).
  Cite in a comment: https://react.dev/reference/rsc/use-server, https://nextjs.org/docs/app/api-reference/directives/use-server, https://nextjs.org/docs/app/api-reference/file-conventions/proxy (matcher).
- [ ] **Step 2:** `~/.cargo/bin/cargo test -p hm-index --test web_next` → fails.
- [ ] **Step 3:** Implement with `next.scm` (sections `;;@jsx` for JSX patterns) and tree walking in `next.rs`.
- [ ] **Step 4:** Test passes; `~/.cargo/bin/cargo test --workspace` clean.
- [ ] **Step 5: Commit** `feat(index): extract Next.js directives, forms and matchers`.

---

### Task 4: Client call and data-hook facts (`ts/web/client.rs`)

**Files:**
- Modify: `crates/hm-index/src/ts/web/client.rs`; Create: `ts/web/queries/client.scm`
- Create: fixture `crates/hm-index/tests/fixtures/client-hops/**`; Test: `crates/hm-index/tests/web_client.rs`

**Interfaces:**
- Consumes: Task 2.
- Produces `client::extract` filling:
  - `url_calls`:
    - `fetch(u, opts?)` (also `window.fetch`): `client = "fetch"`, method from a literal `method:` in `opts` (upper-cased), else `GET`.
    - `axios.get|post|put|patch|delete|head|options(u, …)` → that verb; `axios(u)` → `GET`; `axios({ url, method })` and `axios.request({…})` → from the object. Also any receiver whose root is named `axios` or ends in `api`/`client`/`http` with a verb method (`api.post('/x')`) — `client = "axios"`.
    - Request helpers: any call whose object argument has a literal `url:` property: method from a literal `method:` property, else from the callee name when it is an HTTP verb (`client.post({ url: '/api/v1/login' })`), else `None`; `client = "request_helper"`.
    - `useSWR(key, …)`, `useSWRMutation(key, …)` with a string key: `client = "swr"`, method `GET` (`useSWR`) or `None`.
    - `u` must be a string literal or a template literal; a template's `${…}` parts become `{}`; then `normalize_client_url`. Any other `u` (a variable) gives no fact.
  - `hooks`: `useMutation`, `useQuery`, `useSuspenseQuery`, `useInfiniteQuery` with an object argument: one fact per `mutationFn`, `queryFn`, `onSuccess`, `onSettled`, `onError` property (`target` as `ArgFact`); `useSWR(key, fetcher)` and `useSWRMutation(key, fetcher)` → `key = "fetcher"`.

- [ ] **Step 1: Fixture and failing tests.** Fixture `client-hops/` with `package.json` (`next` 15.2.0, `axios` 1.7.0, `@tanstack/react-query` 5.0.0, `swr` 2.2.0), `app/api/orders/route.ts` (`GET`, `POST`), `app/api/orders/[id]/route.ts` (`GET`, `DELETE`), `lib/api.ts` (`export async function loadOrders() { return fetch('/api/orders') }`, `export function removeOrder(id: string) { return fetch(\`/api/orders/${id}\`, { method: 'DELETE' }) }`, `export const createOrder = (o) => axios.post('/api/orders', o)`, `export function fromVar(u: string) { return fetch(u) }`, `export function elsewhere() { return fetch(\`${BASE}/api/orders\`) }`), `lib/generated.ts` (`export class OrdersService { static list() { return client.get({ url: '/api/orders' }) } }`), `ui/orders.tsx` (`useMutation({ mutationFn: createOrder, onSuccess: refresh })`, `useQuery({ queryKey: ['o'], queryFn: loadOrders })`, `useSWR('/api/orders/7', fetcher)`). Tests:
  - `fetch_and_axios_urls_are_recorded`: `loadOrders` → (`fetch`, `GET`, `/api/orders`); `removeOrder` → (`fetch`, `DELETE`, `/api/orders/{}`); `createOrder` → (`axios`, `POST`, `/api/orders`); `elsewhere` → url `{}/api/orders`; `fromVar` → no fact.
  - `request_helper_takes_the_verb_from_the_callee` (`OrdersService.list` → `request_helper`, `GET`, `/api/orders`).
  - `swr_keys_and_hooks_are_recorded` (url `/api/orders/7` client `swr`; hook facts `mutationFn → Name("createOrder")`, `onSuccess → Name("refresh")`, `queryFn → Name("loadOrders")`, `fetcher → Name("fetcher")`).
  Cite: https://axios-http.com/docs/api_intro, https://tanstack.com/query/latest/docs/framework/react/reference/useMutation, https://swr.vercel.app/docs/api.
- [ ] **Step 2:** run `~/.cargo/bin/cargo test -p hm-index --test web_client` → fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Passes; workspace clean.
- [ ] **Step 5: Commit** `feat(index): extract client calls and data-hook options`.

---

### Task 5: Server facts, route callbacks, gaps and env reads (`ts/web/server.rs`)

**Files:**
- Modify: `crates/hm-index/src/ts/web/server.rs`; Create: `ts/web/queries/server.scm`
- Create: fixtures `crates/hm-index/tests/fixtures/express-api/**`, `crates/hm-index/tests/fixtures/nest-api/**`; Test: `crates/hm-index/tests/web_server.rs`

**Interfaces:**
- Consumes: Task 2.
- Produces:
  - `server::extract` filling:
    - `route_calls`: a member call `R.m(args…)` with `R` an identifier and `m` in `get post put patch delete head options all use setGlobalPrefix`, where the first argument is a string starting with `/` (or, for `use` only, any first argument; for `setGlobalPrefix`, any string). `args` as `ArgFact`s, inline functions as `Inline(Some(name))` when `route_callbacks` made a symbol for them, `Inline(None)` otherwise.
    - `bindings`: `const X = C(…)`, `const X = await C(…)`, `const X = new C(…)` where the callee text `C` is one of `express`, `Router`, `express.Router`, `NestFactory.create`.
    - `decorators`: on a class (target `("", Class)`) and on a class method (target `(Class, method)`); `name` is the decorator callee (`Controller`, `Get`, `UseGuards`, `Injectable`, …); `args` as `ArgFact` (`@Controller({ path: 'orders' })` → `Object { path: Some("orders") }`).
    - `ctor_fields`: TS constructor parameter properties with a type annotation that is a plain type name (`constructor(private readonly orders: OrdersService)` → `{class, field: "orders", type_name: "OrdersService"}`).
    - `gaps`: `eval(…)` → `eval`; `new Function(…)` → `new_function`; `import(x)` with a non-literal argument → `dynamic_import`; `require(x)` with a non-literal argument → `dynamic_require`.
    - `env`: `process.env.NAME`, `process.env['NAME']`, `import.meta.env.NAME` → `name = "NAME"`.
  - `server::route_callbacks`: for each **top-level statement** `R.m('/path', …, inline)` as above with `m` an HTTP verb or `all`, the **last** argument when it is an inline function becomes a `SymbolFact { kind: Function, name: "<METHOD> <path as written>" (METHOD upper-case, `ALL` for `all`), owner: "", sig: same as name, sig_hash/body_hash from `hash`, exported: false, default_export: false, … }` with the function's byte range.

- [ ] **Step 1: Fixtures and failing tests.** `express-api/`: `package.json` (`express` 4.21.2), `src/app.ts` (`import express from 'express'; import orders from './routes/orders'; import { requireUser } from './auth'; const app = express(); app.use(requireUser); app.use('/api', orders); app.get('/health', (req, res) => res.send('ok')); export default app;`), `src/routes/orders.ts` (`import { Router } from 'express'; import { listOrders, placeOrder } from '../orders'; const router = Router(); router.get('/orders', listOrders); router.post('/orders', validate, placeOrder); router.delete('/orders/:id', async (req, res) => { … }); export default router;`), `src/orders.ts`, `src/auth.ts`, `src/plugins.ts` (`eval(code)`, `import(name)`, `process.env.SHOP_KEY`). `nest-api/`: `package.json` (`@nestjs/common` 11.0.0, `@nestjs/core` 11.0.0), `src/main.ts` (`const app = await NestFactory.create(AppModule); app.setGlobalPrefix('api');` inside `async function bootstrap()`), `src/orders/orders.controller.ts` (`@Controller('orders')` class with `@Get()` `list`, `@Get(':id')` `one`, `@Post()` `@UseGuards(AuthGuard)` `place`, constructor `private readonly orders: OrdersService`, and `this.orders.findAll()`), `src/orders/orders.service.ts` (`@Injectable() export class OrdersService { findAll() {} }`), `src/auth.guard.ts`. Tests:
  - `express_calls_and_bindings_are_recorded` (app.ts bindings `app ← express`; route calls `use [Name(requireUser)]`, `use [Str("/api"), Name(orders)]`, `get [Str("/health"), Inline(Some("GET /health"))]`; orders.ts binding `router ← Router`).
  - `top_level_inline_callbacks_become_symbols` (orders.ts has a function symbol `DELETE /orders/:id`; a call inside it has that owner).
  - `nest_decorators_and_ctor_fields_are_recorded` (`Controller [Str("orders")]` on `("", "OrdersController")`; `Get [Str(":id")]` on `("OrdersController","one")`; `UseGuards [Name("AuthGuard")]`; ctor field `orders: OrdersService`; main.ts binding `app ← NestFactory.create` with owner `bootstrap`, route call `setGlobalPrefix [Str("api")]`).
  - `gaps_and_env_reads_are_recorded` (plugins.ts: gaps `eval`, `dynamic_import`; env `SHOP_KEY`).
  - `a_map_get_with_a_key_is_not_a_route_call` (`cache.get('k')` → none; `cache.get('/k')` is recorded, Task 9 drops it because `cache` is not bound to express).
  Cite: https://expressjs.com/en/guide/routing.html, https://docs.nestjs.com/controllers.
- [ ] **Step 2:** `~/.cargo/bin/cargo test -p hm-index --test web_server` → fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Passes; workspace clean (the L1 property test is unaffected: the synthetic project has no Express code).
- [ ] **Step 5: Commit** `feat(index): extract server routes, decorators, gaps and env reads`.

---

### Task 6: `package.json` and lockfiles (`manifest/`)

**Files:**
- Create: `crates/hm-index/src/manifest/mod.rs`, `manifest/npm.rs`, `manifest/lock.rs`; Modify: `crates/hm-index/src/lib.rs` (`pub mod manifest;`)
- Create: fixture `crates/hm-index/tests/fixtures/npm-locks/**`; Test: `crates/hm-index/tests/manifest.rs`

**Interfaces:**
- Produces (consumed by Tasks 7, 8, 12, 13 and, through queries, by L3 and session S4):

  ```rust
  pub struct Dependency { pub name: String, pub section: String, pub dev: bool, pub declared: String,
                          pub resolved: Option<String>, pub line: u32 }
  pub struct NpmManifest { pub path: String, pub root: String, pub name: Option<String>,
      pub bin: Vec<(String, String, u32)>, pub main: Option<(String, u32)>,
      pub scripts: Vec<(String, String, u32)>, pub deps: Vec<Dependency>,
      pub lockfile: Option<String>, pub notes: Vec<String> }
  pub fn parse_package_json(rel: &str, text: &str) -> NpmManifest;   // never fails
  pub struct LockIndex { … }
  pub fn parse_lockfile(rel: &str, text: &str) -> Result<LockIndex, String>;
  impl LockIndex { pub fn resolved(&self, importer: &str, name: &str, declared: &str) -> Option<String>; }
  #[derive(Default)] pub struct ManifestCache { … }  // rel -> (blake3 of bytes, parsed)
  #[derive(Default)] pub struct Manifests { pub npm: Vec<NpmManifest>, pub notes: Vec<String> }
  impl Manifests {
      pub fn load(root: &Path, known: &BTreeSet<String>, cache: &mut ManifestCache) -> Manifests;
      pub fn root_of(&self, rel: &str) -> Option<&NpmManifest>;     // nearest ancestor manifest
      pub fn declares(&self, rel: &str, package: &str) -> bool;     // root_of(rel) declares it, any section
  }
  pub const LOCKFILES: &[&str] = &["pnpm-lock.yaml", "package-lock.json", "npm-shrinkwrap.json", "yarn.lock"];
  ```

  - `root` of `apps/web/package.json` is `apps/web`; of `package.json` it is `""`. Lists sorted by name; `deps` sorted by (section, name).
  - `bin`: a string → one entry named after `name` (without scope); a map → one per key. `line` is the 1-based line of the key (from jsonc-parser's AST ranges).
  - `resolved`: the lockfile version without peer suffixes (`16.0.10(react@19)` → `16.0.10`); `link:`/`workspace:`/`file:` versions → `None`.
  - `importer`: path from the lockfile's folder to the manifest root, `.` when the same.
  - npm: `packages["<importer>/node_modules/<name>"]`, then `packages["node_modules/<name>"]` (v2/v3); `dependencies[name].version` (v1). pnpm: `importers.<importer>.<section>.<name>.version`, read line by line. yarn v1 (`"name@range", name@range2:` blocks with `version "x"`) and berry (`"name@npm:range":` with `version: x`): the block whose keys include `name@declared` or `name@npm:declared`.
  - `load` uses `known` (the walked relative paths) to find every `package.json` (never under a skipped folder, so never in `node_modules`) and the nearest lockfile at or above each manifest root (first folder upward that holds one; preference in `LOCKFILES` order). It reads each file (lockfiles up to 64 MiB), hashes it, and re-parses only when the hash changed. Problems become `notes` lines: `"apps/web/package.json: not valid JSON (line 4)"`, `"pnpm-lock.yaml: lockfile format not recognised"`.

- [ ] **Step 1: Fixtures and failing tests.** `npm-locks/`: `pnpm/` (`pnpm-lock.yaml` v9 with importers `.` and `apps/web`, root `package.json` with devDependency `typescript ^5.6.0`, `apps/web/package.json` with `next ^15.0.0` and `"bin": {"shop": "./bin/shop.js"}`, `"main": "index.js"`, scripts `dev`, `start`), `npm/` (`package-lock.json` v3 with `node_modules/express` 4.21.2), `npm-v1/`, `yarn-v1/`, `yarn-berry/`, `broken/` (`package.json` with a comment, a trailing comma and `"bin": 5`). Tests:
  - `package_json_fields_and_lines_are_read` (apps/web: bin `("shop", "./bin/shop.js", 4)`, main, scripts in order, `next` declared `^15.0.0` line N, dev false; root: `typescript` dev true);
  - `pnpm_importers_resolve_per_manifest_root` (`next` → `15.2.1`, `typescript` → `5.6.3`; both manifests have `lockfile == Some("pnpm-lock.yaml")`);
  - `npm_v1_v3_and_yarn_lockfiles_resolve` (express `4.21.2` in each);
  - `a_broken_package_json_is_a_note_not_an_error` (comments and trailing commas parse; `bin: 5` gives a note and no bin; unterminated file gives a note and an empty manifest);
  - `peer_suffix_and_link_versions` (`16.0.10(react@19.0.0)` → `16.0.10`; `link:../ui` → `None`);
  - `manifests_reload_only_changed_files` (load twice with one cache; change one byte in one `package.json`; the second load re-parses only that one — assert through a `ManifestCache::parses()` counter used only in tests).
  Cite: https://docs.npmjs.com/cli/v11/configuring-npm/package-json (`bin`, `main`), https://docs.npmjs.com/cli/v11/configuring-npm/package-lock-json.
- [ ] **Step 2:** `~/.cargo/bin/cargo test -p hm-index --test manifest` → fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Passes; workspace clean.
- [ ] **Step 5: Commit** `feat(index): read package.json manifests and lockfile versions`.

---

### Task 7: Derive core — Next.js routes, server actions, middleware and `package.json` entries

**Files:**
- Create: `crates/hm-index/src/routes/mod.rs`, `routes/next.rs`, `routes/entries.rs`; stubs `routes/express.rs`, `routes/nest.rs`, `routes/hops.rs`, `routes/patterns.rs`
- Modify: `crates/hm-index/src/lib.rs` (`pub mod routes;`)
- Modify fixture `next-app/` (add pages and route files); Create fixtures `next-pages/**`, `next-mono/**`; Test: `crates/hm-index/tests/routes_next.rs`

**Interfaces:**
- Consumes: Tasks 2, 3, 6; `resolve::{symbol_ids, resolve_export, resolve_callee, source_id, Project}`; `hm_core::ids::{route_id, file_id}`.
- Produces in `routes/mod.rs` (all `Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize`):

  ```rust
  pub struct Route { pub id: String, pub root: String, pub entry_kind: String, pub method: String,
      pub pattern: String, pub handler_id: Option<String>, pub file: String, pub line: u32,
      pub rule: String, pub prov: String, pub conf: String, pub detail: Option<String> }
  pub struct ClientCall { pub file: String, pub src_id: String, pub line: u32, pub method: String,
      pub url: String, pub rule: String }                       // method "ANY" when unknown
  pub struct InferredEdge { pub kind: String, pub src_id: String, pub dst_id: String,
      pub file: String, pub line: u32, pub rule: String }       // stored prov inferred, conf probably
  pub struct Pattern { pub file: String, pub subject_id: String, pub kind: String, pub line: u32,
      pub rule: String, pub conf: String, pub term: Option<String>, pub detail: Option<String> }
  pub struct Gap { pub file: String, pub subject_id: String, pub kind: String, pub line: u32 }
  #[derive(Default)]
  pub struct Derived { pub routes: Vec<Route>, pub client_calls: Vec<ClientCall>,
      pub edges: Vec<InferredEdge>, pub patterns: Vec<Pattern>, pub gaps: Vec<Gap>, pub notes: Vec<String> }
  pub struct Inputs<'a> { pub project: &'a Project<'a>, pub manifests: &'a Manifests }
  pub fn derive(inputs: &Inputs) -> Derived;
  #[doc(hidden)] pub fn derive_tree(root: &Path) -> Derived;   // walk + extract + resolver + manifests, no store
  pub(crate) fn route(root: &str, kind: &str, method: &str, pattern: &str, …) -> Route; // fills id via route_id
  ```

  `derive` runs, in order: `next::routes`, `entries::routes`, `express::routes`, `nest::routes` (each pushes `Route`s); then drops duplicates by `id` keeping the first in `(file, line)` order, with a note; then `next::middleware_edges`, `express::before_edges`, `nest::before_edges`, `hops::derive`, `patterns::derive`; then sorts every list. Stubs for Tasks 9, 10, 12 take `(inputs, &mut Derived)` and do nothing.
- `next::routes` — for each manifest that declares `next`, with root `R`:
  - App tree: `R/app` if any known file is under it, else `R/src/app`. Pages tree likewise with `pages`. Both may exist (app and pages routers together).
  - App router: `page.(js|jsx|ts|tsx)` → `page`, `GET`, rule `ts.next.app.page`, handler = default-export symbol. `route.(js|ts)` → one `http` route per exported `GET POST PUT PATCH DELETE HEAD OPTIONS` (an exported function, an exported `const`, or `export { h as GET }`), handler = that symbol, rule `ts.next.app.route`.
  - Segments: `(group)` removed; `@slot` removed; a folder starting with `_` (private) skips its subtree; `[id]` → `{id}`; `[...s]` → `{...s}`; `[[...s]]` → `{...s?}`; intercepting folders `(.)x`, `(..)x`, `(...)x` skip their subtree (limit). Root page → `/`.
  - Pages router: every `.js/.jsx/.ts/.tsx` under the tree except `_app`, `_document`, `_error` → `page`, `GET`, rule `ts.next.pages.page`; under `api/` → `http`, `ANY`, rule `ts.next.pages.api`; `index` → its folder; same bracket rules. Handler = default export.
  - Server actions: a file with a file-level `use server` directive → one `server_action`, `ACTION`, pattern `<rel>#<name>` per exported function or exported `const` of a function, rule `ts.next.action.file`. A function-level directive → pattern `<rel>#<owner name>` or `<rel>#<owner name>›<inner>` when the function is a nested one, handler = owner symbol, rule `ts.next.action.fn`. Server actions apply anywhere under a Next root, not only in the app tree.
  - Middleware: `R/middleware.(js|ts)`, `R/proxy.(js|ts)`, or the same under `R/src/` → `middleware`, `ANY`, pattern = the first matcher or `/*`, handler = default export symbol (a function named `middleware`/`proxy` if exported), else the file id; `detail` `{"matcher":[…],"evaluated":bool}`.
  - `middleware_edges`: from every `page`/`http` route of the same root that the matcher covers (see scope decisions) to the middleware handler, kind `calls`, rule `ts.next.middleware`, file = middleware file, line = middleware row line.
- `entries::routes` — per manifest: each `bin` → `cli`, `BIN`, pattern = bin name, handler = file id of the target (relative to the root, normalised) if known, rule `npm.bin`; `main` → `main`, `MAIN`, pattern = the path as written, rule `npm.main`; `scripts.start` → `main`, `START`, pattern `start`, detail `{"command": "…"}`, handler = the file named by `node|tsx|ts-node|bun <file>` if known, rule `npm.start`. File = the `package.json`, line = the key line.

- [ ] **Step 1: Fixtures and failing tests.** Extend `next-app/`: `app/page.tsx`, `app/(shop)/orders/page.tsx`, `app/(shop)/orders/[id]/page.tsx`, `app/docs/[...slug]/page.tsx`, `app/help/[[...topic]]/page.tsx`, `app/_components/card/page.tsx` (private), `app/@modal/(.)orders/[id]/page.tsx` (intercepting), `app/api/orders/route.ts` (`export async function GET`, `export const POST = async () => …`), `app/api/orders/[id]/route.ts` (`async function remove() {}; export { remove as DELETE }`), `app/account/page.tsx`, `package.json` gains `"scripts": {"start": "next start"}`. `next-pages/`: `package.json` with `next`, `src/pages/index.tsx`, `src/pages/orders/[id].tsx`, `src/pages/_app.tsx`, `src/pages/api/orders.ts`. `next-mono/`: `apps/shop/{package.json,app/page.tsx,app/api/health/route.ts}`, `apps/admin/{same}`, `packages/ui/{package.json,pages/index.tsx}` (no `next` → not routes). Tests (each cites its Next.js doc URL: https://nextjs.org/docs/app/getting-started/project-structure, https://nextjs.org/docs/app/api-reference/file-conventions/route, https://nextjs.org/docs/app/api-reference/file-conventions/dynamic-routes, https://nextjs.org/docs/app/api-reference/file-conventions/route-groups, https://nextjs.org/docs/app/api-reference/file-conventions/src-folder, https://nextjs.org/docs/pages/building-your-application/routing/api-routes, https://nextjs.org/docs/app/api-reference/file-conventions/proxy):
  - `app_router_pages_and_verbs` — exactly the set of `(entry_kind, method, pattern)`: page `/`, `/orders`, `/orders/{id}`, `/docs/{...slug}`, `/help/{...topic?}`, `/account`; http `GET /api/orders`, `POST /api/orders`, `DELETE /api/orders/{id}`; nothing from `_components` or `(.)orders`; `DELETE` handler is `remove`.
  - `pages_router_under_src` — page `/`, `/orders/{id}`; http `ANY /api/orders`; no `_app`.
  - `server_actions_from_directives` — `ACTION app/lib/actions.ts#placeOrder`, `…#cancelOrder`, `ACTION app/ui/inline.tsx#Inline›save`.
  - `middleware_matcher_reaches_matching_routes` — middleware row `ANY /account/:path*` with `"evaluated":true`; `ts.next.middleware` edges from `/account` (page) and none from `/orders`.
  - `an_unevaluated_matcher_applies_to_every_route` — replace the matcher with `'/((?!api|_next).*)'` in a temp copy; edges from every page and http route; middleware row `conf == "probably"`.
  - `a_pages_folder_without_next_is_not_a_route_tree` (`next-mono/packages/ui`).
  - `two_next_apps_keep_their_own_routes` — `/` and `GET /api/health` twice, roots `apps/shop` and `apps/admin`, four distinct ids.
  - `start_script_is_an_entry` — `main START start` with detail command `next start`, rule `npm.start`, `prov == "config"`.
  - `derive_output_is_sorted_and_stable` — two `derive_tree` calls are equal; every list sorted.
- [ ] **Step 2:** `~/.cargo/bin/cargo test -p hm-index --test routes_next` → fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Passes; workspace clean.
- [ ] **Step 5: Commit** `feat(index): derive Next.js routes, server actions, middleware and npm entries`.

---

### Task 8: Run derive in the indexer, write it as a diff, and prove incremental equals cold

**Files:**
- Modify: `crates/hm-index/src/index.rs`, `crates/hm-index/src/synth.rs`
- Modify: `crates/hm-index/tests/index_property.rs`, `crates/hm-index/tests/index_incremental.rs`, `crates/hm-index/tests/index_full.rs`

**Interfaces:**
- Consumes: Task 7 `derive`, `Derived`; Task 6 `Manifests`, `ManifestCache`.
- Produces:
  - `Indexer` keeps a `ManifestCache` and the last `Manifests`. `full()` always runs derive and writes it. `bring_in_line` runs it when it wrote or removed any file. Manifests are reloaded only when a changed or removed path is a `package.json` or in `LOCKFILES`, or when the set of known `package.json` paths changed.
  - `fn sync_derived(tx: &Transaction, derived: &Derived) -> rusqlite::Result<DerivedCounts>`: for `route`, `client_call`, `pattern`, `gap`, `manifest`, `dependency` and `edge WHERE prov = 'inferred'`, load the stored rows, delete the stored-not-derived, insert the derived-not-stored. Runs after `replace_resolved`/`sync_resolved` in the same transaction. Rows whose `file` is not in the `file` table are skipped (cannot happen; asserted in debug builds).
  - `manifest` and `dependency` rows come from `Manifests` (`detail` JSON as in Task 1).
  - `IndexReport` gains `routes: BTreeMap<String, u32>` (count by `entry_kind`), `client_calls: u32`, `client_calls_matched: u32` (distinct `(file, line)` with an `http_calls` edge), `manifests: u32`, `dependencies: u32`, `patterns: BTreeMap<String, u32>`, `gaps: u32`. Derive and manifest notes join `notes` (sorted).
  - `Delta` gains `routes_added: Vec<String>`, `routes_removed: Vec<String>` (ids, sorted); `edges_added`/`edges_removed` include inferred edges.
  - `synth::project` adds, from the seed: a root `package.json` declaring `next`, some modules placed under `app/api/<name>/route.ts` exporting `GET` and maybe `POST` that call other modules, `app/<name>/page.tsx` pages, and `fetch('/api/<name>')` calls in other modules. New `synth::Edit::EditManifest` (changes the `next` version or adds/removes a dependency) and `Edit::ToggleVerb` (adds or removes `POST` in a route file). Existing edits keep working on the new files.

- [ ] **Step 1: Failing tests.**
  - `index_full.rs`: `report_counts_routes_and_manifests` on a copy of `client-hops/` — `routes["http"] == 4`, `manifests == 1`, `dependencies == 4`, `client_calls >= 4`.
  - `index_incremental.rs`:
    - `deleting_a_route_file_drops_its_routes_and_hops` (client-hops: delete `app/api/orders/[id]/route.ts`; `apply` → `delta.routes_removed` has both ids, no `http_calls` edge points at them, store equals a cold index — `dump`);
    - `a_body_edit_keeps_routes_and_writes_nothing_derived` (edit inside `loadOrders` body; `routes_added`/`routes_removed` empty; dump equals cold);
    - `breaking_a_route_file_keeps_the_verbs_that_parse` (append `export async function PUT( {` to `app/api/orders/route.ts`; `GET` and `POST` routes stay; dump equals cold);
    - `editing_package_json_updates_dependencies` (bump a version; the `dependency` row changes; dump equals cold).
  - `index_property.rs`: add `Edit::EditManifest` and `Edit::ToggleVerb` to the strategy; the comparison already covers the new tables through `dump`.
- [ ] **Step 2:** `~/.cargo/bin/cargo test -p hm-index --test index_full --test index_incremental --test index_property` → fail.
- [ ] **Step 3:** Implement; then run `~/.cargo/bin/cargo run --release -p hm-index --example latency` and check body p95 ≤ 300 ms and cold ≤ 2 s per 1k files. If body p95 grew by more than 30 ms over L1, cache per-file derive parts by facts identity before going on (report the numbers either way).
- [ ] **Step 4:** All pass; workspace clean.
- [ ] **Step 5: Commit** `feat(index): derive routes on every pass and write them as a diff`.

---

### Task 9: Express and NestJS routes, on-the-way-in edges, constructor DI

**Files:**
- Modify: `crates/hm-index/src/routes/express.rs`, `crates/hm-index/src/routes/nest.rs`
- Test: `crates/hm-index/tests/routes_server.rs` (fixtures from Task 5)

**Interfaces:**
- Consumes: Task 5 facts; Task 7 `Inputs`, `Derived`, `route(…)`; `resolve::{binding_package, resolve_callee, resolve_export, source_id, symbol_ids}`; `ModuleResolver::resolve`.
- Produces:
  - `express::routes`: receivers are bindings whose `init` callee root is bound by `binding_package` to package `express` (`express`, `express.Router`, `Router` imported from `express`). Each verb route call (`get post put patch delete head options all`) → `http` route, method upper-case (`all` → `ANY`), pattern = mount prefix + path (Express `:id` → `{id}`, `*` → `{...}`), handler = the last argument (`Inline(Some(name))` → that synthetic symbol; `Name` → `resolve_callee`), rule `ts.express.route`, `prov matched`, `conf probably`. Mounts: `X.use('/pre', Y)` with `Y` a local router binding or an import whose target file has a router binding exported under the imported name (`export default router`, `export { router }`, `export const router = Router()`); prefixes join through up to 5 levels, cycle-guarded; a router mounted twice gives both sets; an unmounted router keeps its paths with `detail {"mounted": false}`.
  - `express::before_edges`: `X.use(fn)` (no path, or a path and a function that is not a router) → `calls` edges from every route of `X` (and of routers mounted on `X`) registered at a later line in the same file, prefix-matched when a path is given, to `fn`; plus the non-last function arguments of a route call → edges from that route; rule `ts.express.use`.
  - `nest::routes`: classes with a `Controller` decorator whose `binding_package` is `@nestjs/common`; class path from `Str`, `Object { path }` or none; method decorators `Get Post Put Patch Delete Options Head All` (`All` → `ANY`), path from `Str` or none; pattern = `/` + global prefix + class path + method path, empty parts dropped, `:id` → `{id}`; handler = the method symbol; rule `ts.nest.route`, `prov matched`, `conf sure`. Global prefix: a `setGlobalPrefix` route call on a binding from `NestFactory.create` in any file of the same manifest root (two different prefixes → none, with a note).
  - `nest::before_edges`: `UseGuards(Name(G))` on the method or its class → `calls` edge from each route of that method to the resolved guard class, rule `ts.nest.guard`.
  - DI (in `nest.rs`, applies to any TS class): a call fact with receiver `this.<field>` inside a class whose `ctor_fields` give `field → T`, `T` resolving (local or import) to a class with exactly one method of the callee's name → `InferredEdge { kind: "calls", src: source_id(call), dst: that method, rule: "ts.di.constructor" }`. An interface type gives no edge.

- [ ] **Step 1: Failing tests** (cite https://expressjs.com/en/guide/routing.html, https://expressjs.com/en/5x/api.html#router, https://docs.nestjs.com/controllers, https://docs.nestjs.com/faq/global-prefix, https://docs.nestjs.com/guards):
  - `express_routes_join_mount_prefixes` — exactly `GET /api/orders` (handler `src/orders.ts#listOrders`), `POST /api/orders` (handler `placeOrder`), `DELETE /api/orders/{id}` (handler `src/routes/orders.ts#DELETE /orders/:id`), `GET /health`; all `conf probably`.
  - `express_use_runs_on_the_way_in` — `ts.express.use` edges from all four routes to `requireUser`, and from `POST /api/orders` to `validate`.
  - `an_unmounted_router_keeps_its_paths` (a temp copy without `app.use('/api', orders)` → `GET /orders` with detail `"mounted":false`).
  - `a_get_on_a_map_is_not_a_route`.
  - `nest_routes_use_the_global_prefix` — `GET /api/orders`, `GET /api/orders/{id}`, `POST /api/orders`; handlers are the methods.
  - `nest_guard_is_on_the_way_in` (`ts.nest.guard` edge from `POST /api/orders` to `AuthGuard`).
  - `constructor_injection_links_the_service_method` (`OrdersController.list` → `OrdersService.findAll`, rule `ts.di.constructor`).
- [ ] **Step 2:** `~/.cargo/bin/cargo test -p hm-index --test routes_server` → fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Passes; workspace clean.
- [ ] **Step 5: Commit** `feat(index): derive Express and NestJS routes with their middleware`.

---

### Task 10: Client → server matching and framework-mediated hops

**Files:**
- Modify: `crates/hm-index/src/routes/hops.rs`
- Test: `crates/hm-index/tests/routes_hops.rs` (fixtures `client-hops/`, `next-app/`)

**Interfaces:**
- Consumes: Tasks 3, 4, 7; `resolve::{resolve_callee, source_id}`.
- Produces in `hops::derive(inputs, &mut Derived)` (runs after every route exists):
  - **Client calls.** Every `url_calls` fact → `ClientCall { src_id: source_id(owner, line), method: method or "ANY", url, rule: "ts.client.<client>" }`. Every form with `action: Some(Str(u))` → `ClientCall` with method from the form (default `GET`), rule `ts.client.form`. A `form` with no `action` and no `onSubmit` in a file that is itself a `page` route → `ClientCall` to that page's pattern, method from the form or `GET`, rule `ts.client.form_self`.
  - **Matching** (scope decisions): a private `Matcher` over the `http` routes (and pages, for forms), own root first, then every root (a client may call another app). A match → `InferredEdge { kind: "http_calls", src: call.src_id, dst: route.id, file, line, rule: call.rule }`.
  - **Mediated hops** (kind `calls`, `src = source_id(owner, line)`):
    - `hooks` with `key` `mutationFn`/`queryFn` and `target: Name(n)` → `resolve_callee` → rule `ts.react_query.fn`; `onSuccess`/`onSettled`/`onError` → rule `ts.react_query.back`; `fetcher` → `ts.swr.fetcher`. A `Name` that is a nested local gives nothing.
    - Server actions: a form `action: Name(n)` (or `formAction`) where `n` is an alias in the same owner → its `target`; the target must resolve to a symbol that is a `server_action` route handler → rule `ts.next.action.form`. An alias with `via = "useActionState"` (directly, or through a `bind` alias) → an extra edge with rule `ts.next.action.state` at the alias line. A form whose `Name` resolves directly to a server action → `ts.next.action.form`.

- [ ] **Step 1: Failing tests:**
  - `fetch_axios_and_helpers_match_routes` — `http_calls` edges: `lib/api.ts#loadOrders → GET /api/orders`, `removeOrder → DELETE /api/orders/{id}`, `createOrder → POST /api/orders`, `lib/generated.ts#OrdersService.list → GET /api/orders`, `ui/orders.tsx#Orders → GET /api/orders/{id}` (SWR key).
  - `variable_built_and_ambiguous_urls_never_match` — `elsewhere` and `fromVar` give no edge; in a temp copy with an extra route `app/api/orders/[slug]/route.ts` exporting `GET`, the SWR call to `/api/orders/7` gives no edge (two equal matches) while `DELETE /api/orders/{}` still matches the one `DELETE`; `fetch('https://pay.example/api/orders')` matches like `/api/orders` (host dropped) — assert this is the documented behaviour.
  - `client_calls_are_kept_without_a_route` — `client_call` rows exist for `elsewhere` (`{}/api/orders`).
  - `react_query_and_swr_hops_are_inferred` — `ui/orders.tsx#Orders → lib/api.ts#createOrder` (`ts.react_query.fn`), `→ loadOrders` (`ts.react_query.fn`), `→ refresh` (`ts.react_query.back`).
  - `server_action_forms_link_through_aliases` (`next-app`) — `OrderForm → placeOrder` with `ts.next.action.form` and `ts.next.action.state`; `CancelButton → cancelOrder` with `ts.next.action.form`; the search form gives `ClientCall GET /search` with rule `ts.client.form`.
- [ ] **Step 2:** `~/.cargo/bin/cargo test -p hm-index --test routes_hops` → fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Passes; workspace clean.
- [ ] **Step 5: Commit** `feat(index): match client calls to routes and infer framework hops`.

---

### Task 11: Ref noise classification

**Files:**
- Modify: `crates/hm-index/src/ts/resolve.rs` (only `resolve_name`'s ref outcomes and a new `globals` table)
- Modify: `crates/hm-index/tests/ts_resolve.rs`

**Interfaces:**
- Produces: `ref.status` values `unresolved`, `external`, `builtin`, `local` (spec §5.1 refs; scope decisions). `const JS_GLOBALS: &[&str]` (sorted, binary-searched) holds ECMAScript globals (`Array Boolean Date Error JSON Math Number Object Promise Proxy Reflect RegExp Map Set WeakMap WeakSet Symbol BigInt Intl String URL URLSearchParams TextEncoder TextDecoder AbortController structuredClone parseInt parseFloat isNaN isFinite encodeURIComponent decodeURIComponent setTimeout clearTimeout setInterval clearInterval queueMicrotask`), DOM/browser (`window document navigator location history localStorage sessionStorage console fetch alert confirm FormData Headers Request Response Blob File crypto performance requestAnimationFrame`) and Node (`process Buffer require module exports __dirname __filename global globalThis`). Rules:
  - no receiver: callee not a module-level binding → `builtin` if in `JS_GLOBALS`, else `local`;
  - receiver: its root identifier not a module-level binding → `builtin` if the root is in `JS_GLOBALS`, else `local` (`this.x.y()` where the class has no such method stays `unresolved`);
  - everything that is bound at module level but not pinned stays `unresolved`; `external` unchanged.
- [ ] **Step 1: Failing tests** in `ts_resolve.rs`: `globals_are_builtin_refs` (`JSON.parse(s)`, `new Date()`, `console.log(x)`, `setTimeout(f)`, `document.querySelector('a')`, `process.exit(1)` → `builtin`); `calls_on_local_values_are_local_refs` (`function f(items) { items.map(g); const h = () => 1; h(); }` → `items.map` and `h` are `local`); `module_level_names_that_do_not_resolve_stay_unresolved` (`import { a } from './missing'; a();` and a top-level `const db = make(); db.insert()` stay `unresolved`). Update existing assertions that expected `unresolved` for these shapes.
- [ ] **Step 2:** `~/.cargo/bin/cargo test -p hm-index --test ts_resolve` → fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Passes; workspace clean (card `unresolved` counts only `status = 'unresolved'` already).
- [ ] **Step 5: Commit** `feat(index): classify built-in and local-value refs`.

---

### Task 12: Patterns and gaps for TS libraries

**Files:**
- Create: `packs/learn/patterns-ts.toml`
- Modify: `crates/hm-index/src/routes/patterns.rs`
- Create: fixture `crates/hm-index/tests/fixtures/patterns-ts/**`; Test: `crates/hm-index/tests/routes_patterns.rs`

**Interfaces:**
- Consumes: Task 5 (`gaps`, `env`, `decorators`), Task 6 (`Manifests::declares`), call facts, `resolve::{binding_package, source_id}`.
- Produces:
  - Pack (embedded with `include_str!("../../../../packs/learn/patterns-ts.toml")`), one `[[rule]]` per row:

    ```toml
    [[rule]]
    id = "prisma.read"          # rule id becomes "ts.pattern.prisma.read"
    kind = "orm_read"           # orm_read orm_write transaction validation caching retry di queue password_hash token_session
    package = "@prisma/client"
    scope = "manifest"          # "import": the file imports `package`; "manifest": the file's manifest declares it
    callees = ["findMany", "findUnique", "findFirst", "count", "aggregate"]
    receiver = "any"            # "any" | "package" (receiver root bound to `package`) | "none" (bare call)
    decorators = []             # alternative to callees: decorator names
    term = "orm"                # glossary slug
    ```

    Rows at least for: Prisma (read, write: `create update upsert delete createMany updateMany deleteMany`, transaction: `$transaction`), Drizzle (`select`/`insert`/`update`/`delete`, `transaction`), TypeORM, Mongoose, `postgres`/`@vercel/postgres` (`sql` tagged calls, receiver none), zod (`parse safeParse parseAsync`), yup, joi, class-validator (decorators `IsString IsEmail …`), `next/cache` (`unstable_cache revalidatePath revalidateTag`), `react` `cache`, `p-retry`, `AbortSignal.timeout`, `@nestjs/common` `Injectable` (kind `di`), `bullmq` (`Queue.add`, `Worker`), bcrypt/bcryptjs/argon2 (`hash compare verify`), `jsonwebtoken` (`sign verify`), `jose` (`SignJWT jwtVerify`), `next-auth` (`signIn signOut auth getServerSession`), `next/headers` `cookies`.
  - `patterns::derive`: per call fact, the rules whose `callees` contain the callee and whose scope and receiver hold → `Pattern { subject_id: source_id(owner, line), kind, rule: "ts.pattern.<id>", conf: "sure" when the receiver root or callee is bound to `package` else "probably", term, detail: None }`. Decorator rules on `decorators` facts (subject = the decorated class or method). `env` facts → `Pattern { kind: "env_read", rule: "ts.pattern.env", conf: "sure", term: Some("env_var"), detail: Some(name) }`. `gaps` → `Gap { subject_id: source_id(owner, line), kind, line }`.
- [ ] **Step 1: Failing tests.** Fixture `patterns-ts/` (`package.json` with `@prisma/client`, `zod`, `bcryptjs`, `jsonwebtoken`; `src/orders.ts` with `prisma.order.findMany()`, `prisma.$transaction([...])`, `OrderInput.parse(body)` where `OrderInput = z.object(...)` and `z` from `zod`, `bcrypt.hash(pw, 10)`, `jwt.sign(...)`, `process.env.SHOP_SECRET`, `eval(x)`). Tests: `pack_parses_and_every_kind_is_known`; `prisma_zod_hash_and_token_patterns_are_found` (kinds and conf per line); `env_reads_and_gaps_are_recorded`; `no_manifest_no_manifest_scoped_pattern` (a temp copy without `@prisma/client` in `package.json` → no `orm_read`).
- [ ] **Step 2:** `~/.cargo/bin/cargo test -p hm-index --test routes_patterns` → fails.
- [ ] **Step 3:** Implement and write the pack.
- [ ] **Step 4:** Passes; workspace clean.
- [ ] **Step 5: Commit** `feat(index): detect TS library patterns and record gaps`.

---

### Task 13: Queries for L3/S2/S4 and the CLI

**Files:**
- Modify: `crates/hm-index/src/query.rs`, `crates/harbormap/src/index.rs`, `crates/harbormap/src/card.rs`
- Test: `crates/hm-index/tests/card.rs`, `crates/harbormap/tests/learn_cli.rs`

**Interfaces:**
- Consumes: the tables of Task 1 filled by Task 8.
- Produces in `query.rs` (all results sorted; `Serialize`):

  ```rust
  pub struct RouteRow { pub id: String, pub root: String, pub entry_kind: String, pub method: String,
      pub pattern: String, pub handler_id: Option<String>, pub handler: Option<String>, // display name
      pub file: String, pub line: u32, pub rule: String, pub prov: String, pub conf: String,
      pub detail: Option<String>, pub label: String }               // label = role_label
  pub fn routes(store: &Store) -> Result<Vec<RouteRow>>;                       // by (root, entry_kind, pattern, method)
  pub fn routes_for_file(store: &Store, file_id: &str) -> Result<Vec<RouteRow>>; // evidence file or handler in it
  pub fn routes_for_symbol(store: &Store, symbol_id: &str) -> Result<Vec<RouteRow>>;
  pub fn role_label(entry_kind: &str, method: &str, pattern: &str) -> String;
  //   http "API · POST /api/auth/login"; page "Page · /orders/{id}"; server_action "Server action · placeOrder";
  //   middleware "Runs before /account/:path*" or "Runs before many routes"; cli "Command · shop";
  //   main START "Start script · npm start"; main MAIN "Entry · index.js"
  pub struct ManifestRow { pub path: String, pub root: String, pub kind: String, pub name: Option<String>,
      pub lockfile: Option<String>, pub bin: Vec<(String, String, u32)>, pub main: Option<(String, u32)>,
      pub scripts: Vec<(String, String, u32)> }
  pub fn manifests(store: &Store) -> Result<Vec<ManifestRow>>;
  pub struct DependencyRow { pub manifest: String, pub root: String, pub name: String, pub section: String,
      pub dev: bool, pub declared: String, pub resolved: Option<String>, pub line: u32 }
  pub fn dependencies(store: &Store, root: Option<&str>) -> Result<Vec<DependencyRow>>;
  pub struct ExternalImport { pub file: String, pub root: String, pub package: String, pub line: u32 }
  pub fn external_imports(store: &Store) -> Result<Vec<ExternalImport>>;  // ref kind import, status external;
  //   root = nearest manifest root of the file
  pub fn import_counts(store: &Store, root: &str) -> Result<BTreeMap<String, u32>>; // package -> distinct files
  ```

  `FileCard` gains `routes: Vec<RouteRow>`; `SymbolCard` gains `roles: Vec<String>` (labels of the routes it handles).
- CLI:
  - `harbormap index` text adds, after the existing lines, `Routes: 7 pages · 3 API · 4 server actions · 1 middleware` (only non-zero kinds; `No routes found` when none), `Entries: 1 start script`, `Calls to the server: 18 found, 7 matched to a route`, `Manifests: 2 (31 dependencies)`, and a list of up to 15 routes as `  API · GET /api/orders → app/api/orders/route.ts#GET`, then `  +N more (harbormap index --json lists all)`. Ref line counts by status in words: `refs: 12 unresolved · 40 external · 210 built-in · 95 local values`.
  - `harbormap index --json` prints the report object with an added `"routes"` array (`query::routes`). With `--watch --json`, batch lines carry `routes_added`/`routes_removed`.
  - `harbormap card <file>` adds a `Routes` block (labels); `card <file>#<symbol>` adds `Role: API · POST /api/orders` lines.
- [ ] **Step 1: Failing tests.** `card.rs`: `file_card_lists_routes` and `symbol_card_shows_its_role` (client-hops fixture: `app/api/orders/route.ts#POST` → `["API · POST /api/orders"]`); `role_labels_read_plainly` (every example above). hm-index test: `manifest_and_import_queries_answer_per_root` (`npm-locks/pnpm`: two manifests; `dependencies(Some("apps/web"))` has `next` resolved `15.2.1`; `import_counts` on `client-hops` gives `axios → 1`). `learn_cli.rs`: `index_prints_routes_summary` (stdout contains `Routes:` and `API · GET /api/orders`), `index_json_has_routes` (`.routes` is an array with `entry_kind` fields), `card_prints_role`.
- [ ] **Step 2:** `~/.cargo/bin/cargo test -p hm-index --test card && ~/.cargo/bin/cargo test -p harbormap --test learn_cli` → fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Pass; workspace clean.
- [ ] **Step 5: Commit** `feat(cli): show routes, entries and roles in index and card`.

---

### Task 14: Eval gate and latency on the public repositories

**Files:**
- Create: `crates/hm-index/tests/eval_l2a.rs`
- Modify: `docs/superpowers/plans/2026-11-03-harbormap-learn-l2a-ts-routes.md` (a short "Results" section at the end: eval counts and latency numbers)

**Interfaces:**
- Consumes: everything above; `HARBORMAP_EVAL_DIR` as in `eval_l1.rs` (copy its `eval_root` helper).
- [ ] **Step 1: Write the tests** (`#[ignore]`, like L1):
  - `next_learn_routes_and_entries_are_found` (`next-learn/dashboard/final-example`): pages exactly `/`, `/dashboard`, `/dashboard/customers`, `/dashboard/invoices`, `/dashboard/invoices/create`, `/dashboard/invoices/{id}/edit`, `/login`; http `GET /query`, `GET /seed`; server actions `app/lib/actions.ts#authenticate`, `#createInvoice`, `#deleteInvoice`, `#updateInvoice`; one middleware from `proxy.ts` (handler is the file id, matcher not evaluated); entry `START start`; edges `ts.next.action.state` from `app/ui/login-form.tsx#LoginForm` to `authenticate`, from `app/ui/invoices/create-form.tsx#Form` to `createInvoice`, from `app/ui/invoices/edit-form.tsx#EditInvoiceForm` to `updateInvoice`, and `ts.next.action.form` from `app/ui/invoices/buttons.tsx#DeleteInvoice` to `deleteInvoice` (read the component names from the fetched files before asserting); dependency `next` declared `latest` resolved from `pnpm-lock.yaml` (`16.0.10`); patterns include `password_hash` (bcrypt) and `validation` (zod) in `app/lib/actions.ts` or `auth.ts`.
  - `fastapi_frontend_client_calls_are_found` (`fastapi-template/frontend`): zero routes; `client_call` rows include `POST /api/v1/login/access-token` from `src/client/sdk.gen.ts#LoginService.loginAccessToken` with rule `ts.client.request_helper`; at least 20 client calls; `dependencies(None)` has `axios` declared `1.20.0`, `resolved == None` (no lockfile in the subfolder); `refs` has some `builtin` and `local` statuses.
- [ ] **Step 2:** Fetch the repos into the scratchpad (`scripts/eval/fetch.sh <scratch>/eval`) and run `HARBORMAP_EVAL_DIR=<scratch>/eval ~/.cargo/bin/cargo test -p hm-index --test eval_l2a -- --ignored`. Fix extraction or derive bugs it shows in the owning module, each with a regression test in that module's test file; never weaken an assertion to match a bug.
- [ ] **Step 3:** Run `~/.cargo/bin/cargo run --release -p hm-index --example latency`; record cold and body p50/p95 in the Results section. Both targets must hold.
- [ ] **Step 4:** Workspace clean; `eval_l1` ignored tests still pass with `--ignored`.
- [ ] **Step 5: Commit** `test(eval): L2a routes and entries on the public repositories`.

---

## Interfaces for other sessions (summary)

| Consumer | Read |
|---|---|
| L3 journeys | `route` rows (`query::routes`, `routes_for_symbol`); `http_calls` edges (src symbol → `r_…`); mediated `calls` edges (`prov = inferred`, rules above); on-the-way-in edges (`calls` with `src_id LIKE 'r_%'`); `pattern`, `gap` rows; `FileFacts.web` in `facts_json` for inner names |
| L3 / S4 technologies | `query::manifests`, `query::dependencies(root)`, `query::external_imports`, `query::import_counts(root)` |
| S2 panel | `query::routes_for_file(file_id)` → `RouteRow.label` for badges |
| L2b languages | `route` table and `route_id`; `client_call` rows already stored for TS clients; the same rule-id style (`py.fastapi.route`, …) |

---

## Results (Task 14, 2026-11-04)

**Eval** (`HARBORMAP_EVAL_DIR=<scratch>/eval cargo test -p hm-index --test eval_l2a -- --ignored`, both green; `eval_l1` still green). Repositories at the commits pinned in `tests/eval/repos.toml`; one cold pass each.

| | next-learn `dashboard/final-example` | fastapi-template `frontend` |
|---|---|---|
| Routes by kind | 16: `page` 7, `http` 2 (`GET /query`, `GET /seed`), `server_action` 5, `middleware` 1, `main` 1 (`START start`) | 0 (TanStack Router file routes are not a rule) |
| Client calls / matched | 0 / 0 | 23 / 0 (all `ts.client.request_helper` in `src/client/sdk.gen.ts`; the backend is Python, L2b) |
| Manifests / dependencies | 1 / 19, lockfile `pnpm-lock.yaml` (`next` `latest` → `16.0.10`) | 1 / 46, no lockfile in the subfolder (`axios` `1.20.0`, unresolved) |
| Patterns | 34: `orm_read` 19, `env_read` 5, `caching` 3, `validation` 3, `password_hash` 2, `token_session` 2; gaps 0 | 11 `env_read`; gaps 0 |
| Inferred edges | 9 `ts.next.middleware` (every page and route → `proxy.ts`), 3 `ts.next.action.state`, 4 `ts.next.action.form` | none: every named `mutationFn` is a function declared inside its component or hook (no edge by design) |
| Refs by status | external 172, builtin 48, local 53, unresolved 34 | external 830, local 819, builtin 117, unresolved 92 |

Found by the eval and fixed: the middleware matcher was stored as its source spelling (`.*\\.png`); it is now the string's value (`.*\.png`), as Next.js reads it (`ts/web/next.rs`, test `matcher_strings_are_their_values_not_their_source_spelling`). The fifth server action is the inline `'use server'` sign-out form in `app/ui/dashboard/sidenav.tsx` (owned by `SideNav`), a server action by the Next.js rules. The generated SDK's `(options.client ?? client).post({ url: … })` is already read as a request helper: a receiver that is not a plain name falls through to the `url:` rule.

**Latency** (`cargo run --release -p hm-index --example latency -- --files 1000 --saves 200 --seed 7 --max-p95-body-ms 300 --max-p95-sig-ms 500 --max-cold-ms 2000 --branch-switch 300`, Apple M4, three runs, all exit 0):

| Run | cold | body p50 | body p95 | sig p50 | sig p95 | branch switch |
|---|---|---|---|---|---|---|
| 1 | 240.5 ms | 119.8 ms | 125.1 ms | 121.3 ms | 129.0 ms | 474.5 ms |
| 2 | 236.4 ms | 119.0 ms | 124.1 ms | 120.5 ms | 127.0 ms | 541.4 ms |
| 3 | 230.6 ms | 119.4 ms | 121.5 ms | 120.6 ms | 126.7 ms | 542.3 ms |

Both targets hold per 1k files: cold ≤ 2 s (worst 240.5 ms), body-only p95 ≤ 300 ms (worst 125.1 ms); signature p95 ≤ 500 ms (worst 129.0 ms).
