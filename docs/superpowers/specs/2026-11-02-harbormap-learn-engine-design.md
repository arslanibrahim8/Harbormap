# Harbormap Learn — learn-your-project engine — Design

- **Status:** approved, 2026-11-02. Amended 2026-11-03: technologies (§7.6), Go server entry and inner names (§6.3, §7.2), plan order (§14).
- **Parent:** [Product design](2026-10-15-harbormap-product-design.md). Amends the
  [v0.1](2026-10-15-harbormap-v0.1-runtime-doctor-design.md) and
  [v0.2](2026-10-15-harbormap-v0.2-harbor-chart-design.md) specs (section 13).
- **Evidence:**
  - [Visual-understanding research](../../research/2026-10-16-visual-understanding/README.md), 2026-10-16.
  - A journey-tour spike, 2026-10-18, on a private Next.js project (three journeys, 33 steps, fact-checked).
  - A six-part harvest of Understand-Anything (UA) 2.9.7 (pipeline, model, UI, LLM agents, languages, evaluation), 2026-11-01.
  - The spike and the harvest quote private code, so they are kept out of the repository.
- **UI draft:** [panel draft](../../design/2026-10-16-panel-draft/index.html).
- **Examples:** all examples in this document are synthetic (a small "shop" app) or come from the public evaluation repositories in section 12.

## 1. Why

People build projects with AI tools and ship them without understanding them, and then they miss simple things. UA showed that a map plus guided tours helps, but a beginner could not read it:
- **Unreadable text.** A typical file summary packed six facts into one sentence, used three undefined terms, never said who calls the file, and made one false claim.
- **Topical tours.** Tours went "overview → privacy → …" instead of following what happens when you press a button.
- **Unreadable function rows.** The function list under a file was the most useful feature, but each row was a clamped one-liner with no callers.
- **Tangled tour edges.** Tours highlighted boxes on the full tangled graph.
- **Levels that only hid boxes.** "Overview / Learn / Deep Dive" never changed the text.
- **Slow and stale.** The layout was recomputed in the browser on every load (1.6 MB layout library, 4.1 s for 10 nodes), and the graph was pinned to one commit.

Harbormap Learn lets a **beginner** understand their own project:
- its files and code structure;
- who does what;
- the coding concepts in it.

It does this in the simplest form, visually, and it stays true to the code as the code changes. Experts are served by a second level of text.

## 2. Goals, non-goals, anti-goals

**Goals**
1. **Journeys.** A journey answers "When I press *Place order*, what happens?" in numbered steps (usually 6–12, never more than 12) that follow one request through the code in execution order, and introduces concepts where the request meets them. Journeys are listed by area (the map group of their handler), which keeps UA's liked "domain → flow" view without an LLM.
2. **Cards.** File and function cards say what a thing is for, who calls it, the jobs inside it, the methods it uses and structure observations with evidence.
3. **A map overview.** At most 12 boxes with counted edges, with drill-down in place.
4. **Live facts.** Deterministic facts update within 300 ms (p95) of a file save, with zero tokens.
5. **Verified prose.** Prose is written by Claude Code subagents on the user's own subscription, checked by a script and an independent verifier, and tied to the hash of the facts it was written from.
6. **Languages.** JS/TS (with TSX), Python, Go, Java and C#, with entry-point and route detection for their common frameworks. Rust is included without routes.
7. **Technologies.** A list of what the project is built with (languages, frameworks, libraries, tools), with versions, where each is declared and how much the code uses it, explained in plain words (§7.6).

**Non-goals (this milestone)**
- **Branches tab and project facts.** The git branch map and going-live, CI, cloud and hosting detection come in the next milestone (section 14). The technologies list (§7.6) reports only what manifests and imports declare.
- **Other providers.** API-key and local-model (Ollama) providers come later.
- **Very large repositories.** WebGL rendering, tile pyramids and repositories over about 10k source files are deferred: the v0.2 large-map work stays in v0.2.
- **Light theme, other locales, mobile layout.**
- **Runtime traces.** Journeys are static; the live view only decorates them with ports and state from the P1 snapshot.

**Anti-goals (failure even if it "works")**
- A step or card teaches something false and nothing flags it.
- Text a beginner cannot read: more than one new term per step, or undefined jargon.
- Tokens are spent without a preview and the user's click.
- The LLM adds edges, decides what is linked, or invents a hop or a file.
- The view tangles: the whole graph is drawn under a journey.
- Text silently describes code that has changed.

## 3. Where it sits

**Roadmap order** (decided):
1. alpha.1 P1 inventory (done);
2. **Learn** (this spec);
3. Project facts (branch map; going-live, CI, cloud and database detection from files);
4. P2 safe stop;
5. P3 daemon;
6. P4 full dashboard.

**What Learn takes from v0.2 (Harbor Chart).** v0.2's indexer (§2), grouping (§3), edge aggregation (§4), the layout core (§5.1–5.3), tours (§10) and route journeys (§11.2) move into Learn and are revised here. What stays in v0.2:
- the large-repository path (§5.4 tiles, §6 WebGL);
- the full code↔runtime bridge (§11.1 beyond the static service link of section 5.6);
- the 50k-file budgets.

**Decided basis**
- **No fork.** UA's ideas are rebuilt as Harbormap's own design. UA is MIT-licensed. The only data reused is UA's path-pattern table for layer names (`packages/core/src/layer-detector.ts`); it ships as a role pack with UA's MIT notice and attribution in `THIRD_PARTY.md`. No UA code is copied.
- **Engine in Rust** inside the workspace, using tree-sitter through Rust crates. Users need no Node.js.
- **LLM through Claude Code.** No API key in this milestone.
- **The core stays deterministic and zero-token.** Every view works without an LLM by using templates (section 8.1).

## 4. Architecture

Two layers. Facts never depend on prose.

```
            ┌───────────── facts layer (Rust, zero tokens) ─────────────┐
 files ──▶  │ walker → parse (tree-sitter) → resolve → routes/entries   │
 (notify)   │   → journeys (paths) → cards (templates) → layout         │──▶ panel (browser draws)
            │   store: SQLite per repository, hashes, staleness         │──▶ CLI (text/JSON)
            └───────────────────────────────▲───────────────────────────┘
                                            │ facts bundle (JSON)  ▲ draft JSON
            ┌──── prose layer (Claude Code plugin, user-started) ────┐
            │ writer → validator (Rust) → verifier → fixer → commit   │
            └─────────────────────────────────────────────────────────┘
```

**New crates**

| Crate | Responsibility |
|---|---|
| `hm-store` | SQLite (`rusqlite`, bundled), schema migrations, transactions |
| `hm-index` | Walker, per-language extractors and resolvers, route and entry rules, incremental update, watcher |
| `hm-learn` | Journeys (seeds, paths, steps), cards, patterns, observations, glossary, facts bundles, prose validator, staleness, cost estimates |
| `hm-layout` | Grouping, edge aggregation, precomputed positions (v0.2 §3–§5.3), cached by group hash |
| `hm-serve` | Local HTTP server for the panel (axum, 127.0.0.1, token), server-sent events; moves into `hm-daemon` at P3 |

**Reused crates**
- `hm-core`: ids, `Confidence`, redaction.
- `hm-project`: git discovery, worktrees, manifests.
- `harbormap`: CLI.

**Other files**
- `ui/learn/`: TypeScript source of the panel, compiled at development time and embedded with `rust-embed`.
- `plugins/claude-code/`: the Claude Code plugin.

**Process model.** Until the P3 daemon exists, `harbormap learn [path]` runs in the foreground:
- it indexes;
- it watches files;
- it serves the panel on 127.0.0.1 and opens the browser;
- it stops on Ctrl-C.

**Store access.**
- While `learn` runs, it is the only writer of the facts store. Other commands read the store directly; when no `learn` is running, they run one indexing pass first.
- `llm commit` writes only the prose store (below). `learn` notices commits from other processes by polling SQLite's `PRAGMA data_version` every 500 ms and pushes the change to the panel.
- SQLite runs in WAL mode with a busy timeout, so a reader never blocks the writer.
- At P3 the daemon takes over the watcher and the server unchanged.

**Store location.** The platform data directory (`directories` crate). Nothing is written inside the user's repository, including `.git`.
- **Facts store:** one database per worktree, keyed by the worktree root. Facts are cheap to rebuild, so a moved or recloned repository simply re-indexes. This replaces v0.2's per-worktree overlays in this milestone.
- **Prose store:** one database per repository identity, shared by all its worktrees and branches. Prose is content-addressed (§9.5), so the same facts on another branch reuse the same text.
- **Repository identity:** the hash of the repository's root commit(s) when git exists (it survives moves and reclones), otherwise the P1 `project_id`.

**CLI** (all with `--json`)

| Command | Purpose |
|---|---|
| `harbormap learn [path]` | Index, watch, serve the panel |
| `harbormap index [path]` | One indexing pass; prints counts, exclusions and timings |
| `harbormap journeys` | List suggested and saved journeys |
| `harbormap journey <id\|query>` | Print a journey: template text, or prose when present |
| `harbormap card <path>[#symbol]` | Print a file or function card |
| `harbormap tech [path]` | Print the technologies list (§7.6) |
| `harbormap facts journey <id>` / `facts card <path>` | Facts bundle for the plugin |
| `harbormap llm plan [--journey id\|--file path\|--since-commit]` | Cost preview of what is stale |
| `harbormap llm validate <draft.json>` | Run the prose validator |
| `harbormap llm commit <draft.json> <verdicts.json>` (or `--stdin`) | Validate again and store in the prose store |
| `harbormap llm write <target>` | Preview, confirm, run the headless pipeline (section 9.2) |

## 5. Facts layer: indexer and fact model

### 5.1 What is extracted

- **Files.**
  - Fields: path, language, lines, `content_hash`, class.
  - Classes: source, test, generated, vendored, doc, infra (v0.2 §2.5 patterns).
  - Excluded classes are counted and shown ("Not on this map: tests · 120 files"), never silently dropped.
  - Ignore rules: `.gitignore`, defaults, and an optional `.harbormapignore`. No confirmation prompt.
- **Symbols.**
  - Kinds: function, method, class, interface/trait, module-level route callback.
  - Fields: owner, name, signature, byte and line range, exported, metrics (lines, branches).
  - An anonymous route callback gets a synthetic name such as `POST /api/orders`.
- **Edges.**
  - Seven kinds: `imports`, `calls`, `extends`, `reads_env`, `http_calls`, `tested_by`, `contains`.
  - A `calls` edge keeps the receiver text, the argument count and the call-site line.
- **Refs.** An unresolved reference is stored as a ref and retried when files change. A ref is never drawn as an edge.
- **Routes.**
  - Fields: method, normalised path, handler symbol, framework rule, entry kind.
  - Entry kinds: `http`, `page`, `server_action`, `cli`, `main`, `cron`, `event`.
- **Entry points.**
  - `main`, CLI subcommands, `package.json` `bin`/`main`/`scripts.start`, `__main__`, `Program.cs`, `@SpringBootApplication`.
  - CLI subcommands: clap derive `Subcommand` enum variants and the `match` arm that dispatches each one (Rust); `argparse`/`click`/`typer` subcommands (Python); `cobra` commands (Go); `commander`/`yargs` commands (JS).
  - Example and test `main`s rank low.
  - Today `hm-project/manifest.rs` reads only the name and dependency names from `package.json` and Python manifests. Learn extends it with `bin`/`main`/`scripts`, `go.mod`, `pom.xml`/`build.gradle` and `.csproj` (plan L2).
- **Gaps.** Dynamic code the parser cannot follow (`eval`, `getattr` with a variable, reflection, `import(variable)`) becomes a gap fact: "Harbormap cannot see what happens here".
- **Patterns and observations** (section 7.3, 7.4) are computed from the above.

### 5.2 Evidence and confidence

Every fact row carries:
- `file_id`, `l0`, `l1`: the place that proves it;
- `rule`: the rule id;
- `prov`: one of `parsed`, `matched`, `config`, `inferred`, `observed`;
- `conf`: P1's `Confidence { Sure, Probably }`.

Resolution outcomes map as follows:
- exact (one candidate) → `Sure`, `parsed`;
- name or DI match → `Probably`, `inferred`;
- no candidate → no edge, a ref only.

Beginners see a `Probably` hop as a dashed arrow labelled "probably". This replaces v0.2's `exact/probable` spelling.

### 5.3 Ids

Ids follow `hm-core/ids.rs`: a prefix plus 12 hex characters of BLAKE3 over stable parts. Ids contain no line numbers and no counters, so they survive edits.

| Entity | Prefix | Hashed from |
|---|---|---|
| file | `f_` | relative path |
| symbol | `y_` | file id, owner, name, kind, signature tie-breaker for duplicates |
| route | `r_` | method, normalised path |
| journey | `j_` | trigger kind, trigger id |
| infra unit | `n_` | file, unit name |

Ids are relative to the repository, not to its location on disk, so the same code in two worktrees has the same ids. That is what lets prose be shared across worktrees (§9.5). `hm-core/ids.rs` today hashes absolute paths for P1's runtime ids; Learn adds these repository-relative constructors next to them.

A rename produces a new id. An unchanged `content_hash` links the old id to the new one so that prose can follow the rename.

### 5.4 Hashes and dirty propagation

**Per file**
- `content_hash`: BLAKE3 of the raw bytes.
- `interface_hash`: exports and symbol signatures.
- `facts_hash`: sorted symbols, imports, routes and env names.

**Per symbol**
- `sig_hash`: name, parameters, return type, visibility.
- `body_hash`: the token stream with comments and whitespace removed. String literals are kept.

**Per folder.** A Merkle hash over its children (name, hash).

**On a change** (one SQLite transaction; no LLM):
1. Precheck (size, mtime). If both are unchanged, stop. Otherwise compute BLAKE3; if the hash is equal, stop.
2. Re-parse the file and diff its symbols: added, removed, signature changed, body changed, moved.
3. **Body-only change:** replace this file's outgoing edges and refs. Importers are untouched.
4. **Interface change:** re-resolve the edges and refs of the files in `reverse_imports`, and retry unresolved refs that may now resolve.
5. Apply deltas to aggregated group edges, commit, and push a **facts delta** to subscribers (the panel, through server-sent events).
6. Then, as a second pass: find the journeys that use a changed edge or symbol through `journey_edge`, recompute only those paths, and recompute the staleness state of every prose item whose cited facts changed (§9.6). Push a **journey delta**.
7. If a folder gained or lost files, mark grouping stale and re-lay out only the affected groups.

**Watcher**
- `notify` with a 100 ms quiet period and a 500 ms maximum wait. This replaces v0.2's 2 s debounce.
- Atomic-save renames are normalised to "path changed".
- On watcher overflow or a branch switch, the indexer does a Merkle rescan.

### 5.5 Store (sketch)

```sql
file(id PK, repo_id, path, lang, class, loc, content_hash, interface_hash, facts_hash,
     parse_status, UNIQUE(repo_id, path))
symbol(id PK, file_id, kind, name, owner, sig, sig_hash, body_hash, l0, l1, exported,
       lines, branches, UNIQUE(file_id, owner, name, kind, sig_hash))
edge(kind, src_id, dst_id, file_id, line, rule, prov, conf, PK(kind, src_id, dst_id, line))
ref(file_id, kind, text, line, PRIMARY KEY(file_id, kind, text, line))
reverse_import(dst_file_id, src_file_id, PK(dst_file_id, src_file_id))
route(id PK, repo_id, method, pattern, handler_id, entry_kind, rule, file_id, line, conf)
gap(file_id, line, kind)
pattern(subject_id, kind, file_id, line, rule, conf)
observation(subject_id, kind, value, file_id, line, rule)
grp(id PK, parent_id, kind, name, role, hash) ; group_edge(src, dst, count)
layout(group_hash, params_version, blob)
journey(id PK, trigger_kind, trigger_id, area_grp_id, title_key, path_hash, state)
journey_step(journey_id, n, anchor_id, l0, l1, quote_hash, step_hash, term_candidates,
             PK(journey_id, n))
journey_edge(edge_key, journey_id)
service_link(project_id, worktree, service_name, grp_id, entry_file_id, prov, conf)
```

The prose store (one per repository identity, §4):

```sql
prose(cache_key PK, subject_kind, subject_id, lang, body_json, cited_hash,
      template_version, model, verdicts_hash, tokens, written_at)
prose_head(worktree, subject_kind, subject_id, lang, cache_key,
           PK(worktree, subject_kind, subject_id, lang))  -- latest text per worktree
finding(id PK, cache_key, subject_id, file, line, text, status)
model_term(slug, lang, name, definition, analogy, PK(slug, lang))  -- terms a pack lacked
```

- SQLite runs in WAL mode with `synchronous=NORMAL`.
- `body_json` holds all fields of one subject for both levels: a journey's steps (titles, sentences, the chosen term per step, watch-outs), or a card's sentences. One write produces one row.
- Terms: glossary packs are static data shipped with Harbormap, keyed `(slug, lang)`. Facts store only `term_candidates` (pack slugs whose triggers appear in the step's code). The writer picks at most one per step, and the choice lives in the prose. "Each term once per journey" is a validator check. Facts therefore never depend on prose.
- Code quotes are not stored. They are cut from the live file when a bundle or view is built, and redacted (§9.9).

### 5.6 Join with the P1 runtime snapshot

- **Live data stays out.** The live snapshot (pids, ports, state) never enters this store. Hashing it would make prose stale on every restart.
- **Static link.** `service_link` connects a service key `(project_id, worktree, service_name)`, the same parts `ids::service_id` hashes, to a group and an entry file. It is built from compose `build.context`, the process working directory plus the manifest name, and Dockerfile `EXPOSE`.
- **Read-time join.** At read time Rust joins route → group → `service_link` → snapshot service (ports, URLs, state). A journey's last hop can then say "the API on port 8000 (running)".
- **Declared vs listening ports.** A declared port is a hint. Only listening ports are facts.

### 5.7 Latency targets (to be measured; laptop, warm cache)

All "save" timings run from the file write to the delta arriving at a subscriber, so they include the 100 ms quiet period.

| Case | 1k files p50 / p95 | 5k files p95 |
|---|---|---|
| Body-only edit, one file → facts delta | 130 / 300 ms | 350 ms |
| Signature edit, ≤ 50 importers → facts delta | 180 / 500 ms | 600 ms |
| Any edit on a journey's path → journey delta (path and staleness) | 1 s | 1 s |
| Branch switch, 300 files changed | 1.5 s | 2.5 s |
| Cold index (facts, no journeys) | 2 s | 10 s |
| One journey path, warm | 100 ms | 100 ms |
| Suggestion ranking after a cold index | 3 s | 10 s |

## 6. Languages and routes

### 6.1 Parsers

- **Core.** `tree-sitter` 0.27.x with the grammar crates `tree-sitter-typescript` (TS, TSX), `tree-sitter-javascript`, `tree-sitter-python`, `tree-sitter-go`, `tree-sitter-java`, `tree-sitter-c-sharp` and `tree-sitter-rust`.
- **Compatibility.** All grammars are MIT and depend only on `tree-sitter-language ^0.1`.
- **Queries.** Extraction uses embedded `.scm` queries. Each file is parsed once and fed to all extractors.
- **Parse errors.** A tree with errors is "parsed with errors": partial results are kept, never an empty file.
- **Not used:**
  - `stack-graphs` (archived 2025-09);
  - oxc, swc and ruff ASTs, since each would be a second AST shape. `oxc_resolver` may be used for TS module resolution only if it supports tsconfig `paths`/`extends` (to be verified in plan L1); otherwise the plan writes a small resolver.

### 6.2 Import and call resolution (no type checking)

| Language | Imports | Calls |
|---|---|---|
| TS/JS | Relative paths with extension and `index` probing; tsconfig `baseUrl`/`paths`/`extends` from the nearest tsconfig; barrel `export *` walk, at most 5 hops | Imported names exact; method calls on untyped receivers inferred or unresolved |
| Python | Package tree, relative imports, `__init__` re-exports | `module.fn` exact; `self.x.y()` inferred by attribute name |
| Go | `go.mod` module path → directory; the package is the unit | Package-qualified exact; interface calls inferred when one implementation exists |
| Java, C# | Namespace/package → files index; type names | DI through an interface links to the single implementation (inferred); several implementations → ref only |
| Rust | `mod`, `use crate::`/`super::` | Path-qualified exact; trait and macro calls unresolved |

### 6.3 Route and entry rules (v1)

Reliability levels:
- **H:** syntactic and local.
- **M:** needs a prefix join or a naming convention.

Rules marked [K] were written from memory and are verified against the framework docs while plan L2 is built.

| Framework | Rule | Rel. |
|---|---|---|
| Next.js app router | `app/**/page.(t\|j)sx` = page; `app/**/route.(t\|j)s` exporting `GET`…`OPTIONS` = API; `[id]`, `[...s]`, `(group)` translated; `_private` skipped; `src/app` or `app` | H |
| Next.js pages router | `pages/**` page; `pages/api/**` default export; skip `_app`, `_document` | H |
| Next.js server actions | `'use server'` at file or function top; linked to forms and props by import (often inferred) | H detect, M link |
| Next.js middleware | `middleware.ts` or `proxy.ts` (v16); matcher shown as "applies to many routes" | H |
| Express | `X.get/post/…('/p', …fns)` with X bound to `express()`/`Router()`; `use('/pre', router)` joins prefixes; string must start with `/` and a function argument must follow [K] | M |
| NestJS | `@Controller('p')` + `@Get('x')` …; global prefix from `main.ts` [K] | H |
| FastAPI | `@app.get`, `@router.post`; `APIRouter(prefix=)`, `include_router(prefix=)`; `Depends(fn)` is an edge [K] | H / M prefix |
| Flask | `@app.route(methods=)`, `@bp.get`; `register_blueprint(url_prefix=)`; no `methods` = GET [K] | H / M prefix |
| Django | `urlpatterns` `path`/`re_path`/`include` (dotted string resolved); `as_view()` verbs by method name; DRF routers documented as a limit [K] | M |
| Go net/http | `HandleFunc("/p", h)`, Go 1.22 `"GET /p"`; wrappers unwrapped one level. The `Handler` field of `http.Server{…}` and the handler argument of `ListenAndServe`/`ListenAndServeTLS` is the server's catch-all entry (`ANY /*`), followed through local variables in the same function [K] | H |
| Gin, Echo, Chi | `r.GET("/p", h)`, `Group("/api")` within one function; framework import required [K] | H / M groups |
| ASP.NET Core minimal | `MapGet/MapPost/…`, `MapGroup`; follow extension calls from `Program.cs` | H |
| ASP.NET controllers | `[ApiController]`/`ControllerBase`, `[Route("api/[controller]")]`, `[HttpGet("x")]` [K] | H |
| Spring MVC | `@RestController`/`@Controller`, class `@RequestMapping`, `@GetMapping`…, constant paths resolved in the same file [K] | H |
| Entry points | as in section 5.1 | H |

- **Client → server hops.** URL literals passed to `fetch`, `axios`, `requests`, generated SDK functions and form `action`s are normalised (`/api/orders/${id}` → `/api/orders/{}`) and matched to the route table by segments and method. A match is `inferred`. Variable-built URLs are not matched.

**Framework-mediated hops** (the call is made by the framework, not by the code). Each becomes an `inferred` edge with its own rule id:

| Hop | Rule |
|---|---|
| React Query / SWR | `useMutation({ mutationFn: f })`, `useQuery({ queryFn: f })`, `mutate()` → `f`; `onSuccess`/`onSettled` callbacks become a "back" step |
| Generated API clients (OpenAPI) | A function whose body passes a literal `url: '/api/…'` and `method` to a request helper is a client hop to that route |
| Forms | A `<form>` with no `action` posts to the page's own route (Thymeleaf, Razor, Django templates, Next.js) |
| Spring | `@ModelAttribute` methods of the same controller run before the handler |
| FastAPI | `Depends(f)` runs `f` before the handler |
| NestJS / ASP.NET / Spring DI | A constructor-injected interface → its single implementation |
| Middleware and guards | Express `use`, Next.js middleware/proxy, NestJS guards, ASP.NET filters: shown as one "on the way in" step, not as a call chain |
| Events and websockets | A publish and a subscribe/handler with the same literal channel or event name (stretch goal for this milestone; until then the journey ends with "then an event is published: `order.created`") |
- **Language order** inside the milestone: TS/JS and Next.js first, then Python, Go, Java/C# and Rust. All six ship in v1.
- **Rust.** Rust ships without web routes. axum and actix are stretch goals.

## 7. Journeys and cards (deterministic part)

### 7.1 Journey seeds and suggestions

**Seeds**
- **UI action:** a button or form whose handler reaches a client → server hop.
- **Route:** an HTTP route on its own.
- **Entry point:** a CLI subcommand (its dispatch arm), `main`, a cron or an event handler.
- **Any function**, chosen by the user ("Start a journey here"), for flows that no route or command starts.

**Suggestions.** Up to 5 per project, ranked in two passes:
1. A cheap score for every seed: entry score, PageRank over the import graph from entries, and the number of "work" symbols (database, network, state touches) reachable within the depth limit, from a bounded reachability count.
2. Full paths only for the top 20 seeds, then the final order by path work.

**Choosing a journey.** The user can pick any other seed by searching a button label, URL, command or file. Free-text questions map to seeds by search only; no LLM picks the seed.

**Areas.** Each journey belongs to the map group of its handler ("Orders", "Accounts"). The sidebar lists journeys by area, which gives the domain → flow view without an LLM.

**Titles** are templates ("Place an order", from the button label or the route) until prose exists.

If a project has neither routes nor entry points, the tab says so plainly.

### 7.2 Path computation

A journey must follow **execution order**, so the walk is depth-first in call-site order, not breadth-first.

1. Start at the UI action (if any), follow its client hop (including framework-mediated hops, §6.3) to the route, run the "on the way in" steps (middleware, guards, `Depends`, `@ModelAttribute`), and start the server part at the handler symbol.
2. Walk `Sure` and `Probably` call edges depth-first. A function's callees are visited in the order of their call-site lines. Limits:
   - call depth ≤ 6 from the handler;
   - ≤ 40 symbols visited;
   - ≤ 3 callees per node when the candidate set is wider (ranked by work).
3. **Continuations.** When a callee returns and the caller does more work after the call (for example, `run` loads data and then prints it), the caller reappears as a later step ("back in `run`"). The panel draws it once with two numbers.
4. Database, outbound HTTP, file system and queue calls are terminal steps.
5. Hub demotion: a node whose in-degree is above about 5% of files, or that sits on a `utils`/`logger`/`types` path, becomes a badge on the calling step instead of a stop.
6. A stop must do real work: a database, network or state touch, a branch on the request, or a function above a size threshold. Small helpers fold into the step that calls them.
7. On a fork (an `if` on the request with work on both sides), follow the branch with more work. The other branches become "+N other cases".
8. The result has at most 12 steps. A short path stays short (a 3-step journey is honest). A longer path merges consecutive steps in the same file.
9. A generated file that is missing from the repository becomes a step that says so ("built at compile time, not in the repository"). The path is never guessed past it.
10. **Limits stated, not hidden.** Data that flows by value, not by call (a value stored now and printed later), and event hops without a literal match are not followed. The journey ends with a plain note about what happens next, without a step.
11. Every journey records the commit and branch it was computed on, and whether the working tree had changes.
12. **Inner names.** Nested functions are not symbols (plan L1), so a step whose call site sits inside a named nested function or a closure assigned to a variable is labelled with that inner name and cut to its lines: `LoginForm › onSubmit`, `main › handler`. L3 finds the inner name from the file's syntax tree at step time; the symbol table does not change. A long closure (for example a 400-line request handler inside `main`) is split into steps by its call sites like any other symbol.

### 7.3 Patterns (rules, not LLM)

**Detected:**
- ORM read/write;
- transactions;
- auth guard (an early return or not-found on an identity check);
- input validation (zod, pydantic, Bean Validation, data annotations);
- caching (`@Cacheable`, cache wrappers);
- retry/timeout;
- dependency injection;
- queues and background jobs;
- env reads;
- password hashing;
- tokens/sessions.

**Sources:** decorators and attributes, imports of known libraries, call shapes and paths (packs, extendable).

Each pattern carries a line and a glossary term.

### 7.4 Structure observations (rules, not LLM)

Each observation is evidence, never a verdict. It carries the rule, the line(s) and one line on why it may matter. Examples:
- one file both writes to the database and sends email;
- a file larger than 95% of the project's files;
- a function called from ≥ 5 places with no `tested_by` edge;
- a route handler with no validation pattern before a write;
- a write after a read with no transaction (check-then-write);
- a comment containing a risk word (atomic, cached, rolled back, retried, validated) on code the rule cannot confirm; it is not shown as an observation but sent to the verifier as a comment claim.

Wording never uses "bad", "wrong" or "insecure". These rules are the seed of v0.5 Health & Conventions and share its rule ids.

### 7.5 Template text (zero tokens)

Every journey step and card has a template text built only from facts, for example: "Step 3 · `create_order` in services/orders.py · saves to the database · called by POST /api/orders". The panel and CLI always show it until prose exists. When prose is stale, the template is shown beside the old prose.

### 7.6 Technologies (zero tokens)

The technologies list answers "what was this project built with?", so a reader learns the tools before the code.

- **Sources:**
  - declared dependencies and versions from manifests: `package.json` (dependencies and devDependencies, with the resolved version from the lockfile when present), `pyproject.toml`/`requirements*.txt`, `go.mod`, `pom.xml`/`build.gradle`, `.csproj`, `Cargo.toml`;
  - language counts from the index;
  - `Dockerfile` `FROM` lines and `docker-compose` images;
  - known config files (`next.config.*`, `tsconfig.json`, `drizzle.config.*`, `tailwind.config.*`, `vitest.config.*`, `playwright.config.*`).
- **Catalog.** An embedded pack maps package names and config files to a technology, a category and a glossary term. Categories: language, framework, UI, data and ORM, auth and security, validation, testing, build and tooling, deploy and infra, other. The pack is data and extendable, like the other packs.
- **Evidence per item:**
  - where it is declared (`file:line`) and its version (the declared range, plus the resolved version when a lockfile has it);
  - how much the code uses it, from the facts: "imported in 42 files";
  - a declared package that no file imports says "declared, not imported (may be a command-line tool)";
  - an import with no manifest entry is listed under "used but not declared".
- **Text.** One plain line per technology from the glossary pack, at both levels ("Drizzle: a library that turns code into database queries"). A package missing from the pack is listed by name with "no description yet"; it never gets an invented description. Its description can be written through the normal prose path (preview and click), as a card batch item.
- **Links.** Each journey step shows the technologies its file imports as chips, so "this step uses Zod" opens the same item.
- **Limits.** Beginner shows at most 12 items, grouped by category and ranked by import count, with "+N more"; Builder shows all. A monorepo lists technologies per manifest root.
- **Scope.** CI, cloud, hosting and going-live detection stay in Project facts (§3).

## 8. Text: steps, cards, levels, language

### 8.1 Journey step template

| Field | Content | Source |
|---|---|---|
| Title | A plain verb phrase | LLM (template until then) |
| What happens | ≤ 3 sentences (Beginner) | LLM |
| Why | Design reason; claims taken from comments must be verified | LLM |
| Code | 2–8 lines, cut by Rust, hand-off line marked | Rust |
| New term + analogy | ≤ 1 per step, never repeated in a journey; must appear in this step's code or text | Rust offers pack candidates found in the step's code; the writer picks one or none, and writes a term only when no pack term fits |
| Watch out | Optional; must cite a line | LLM, verified |
| Files in this step, who calls it, next hop | Chips | Rust |

**Synthetic Beginner example, step "The server adds up the cart again":**
- **What happens:** The browser sends the cart. The server ignores the total the browser sent and adds up the prices itself.
- **Why:** Anyone can change what their browser sends. Prices from the database cannot be changed that way.
- **Code:** `total = sum(item.qty * prices[item.product_id] for item in cart.items)`
- **New term:** *Server-side check*. The server checks or recomputes what the browser claims. Like a cashier who scans every item instead of trusting the total the customer wrote on the list.

### 8.2 Cards

**File card** (drawer), in order:
1. **What it is for:** 1–2 sentences.
2. **Who uses it:** callers and importers ranked as pages and routes first, tests last ("used from 5 places, mostly the Orders page").
3. **Jobs inside:** one row per function: name, a verb phrase of ≤ 12 words, "called from N places", and the journeys it is in. Rows are not truncated, and clicking a row opens the function card.
4. **Methods it uses:** patterns (7.3), with lines and term chips.
5. **Structure observations:** from 7.4.
6. **Findings:** verified findings (section 9.4), if any.

**Function card:**
- what it takes and returns, in plain words;
- who calls it and what it calls;
- side effects (database, network, files, env);
- for large functions only, a 2–5 bullet "step by step";
- watch-outs with lines;
- findings.

**Who writes what.**
- **Prose (LLM):** "what it is for", the verb phrase of each job row, "what it takes and returns", the step-by-step bullets, watch-outs.
- **Rust:** callers and importers, job names and counts, journeys, patterns, observations, side effects, signatures, lines.

**What a card's prose cites** (its staleness hash): the subject's own signatures, outgoing calls, effect patterns and, for watch-outs, the cited lines. Callers are not cited. They are shown live from Rust, so a new caller never makes a card stale.

### 8.3 Levels

Two levels. One LLM call writes both levels. Switching changes the text and never moves a box. In the Map tab, Builder may show more items inside a group the user expands (budgets in §10.3); those positions are precomputed, so no layout runs.

| Rule | Beginner | Builder |
|---|---|---|
| Length | ≤ 3 sentences; ≤ 18 words each (English), ≤ 15 (Turkish, provisional) | ≤ 5 sentences |
| Content | One fact per sentence; "who calls this" always | Names, framework, edge verbs; hand-off line marked |
| Code names in prose | ≤ 1 | free |
| File paths in prose | none (paths live in chips) | allowed |
| Terms | Every term defined at first use (chip) | Assumed |

The draft's third level ("Expert") becomes a **Technical details** fold in the drawer: line numbers, fan-in/out, provenance and confidence.

### 8.4 Language and glossary

- **Prose language** is a setting, Turkish or English, applied per project. Prose is written in that language only.
- **Glossary packs** ship in both languages: about 100 terms, each with a definition, an analogy and trigger patterns. The LLM writes a term only when the pack lacks it, and the new term is stored with `origin = model`.
- UI labels are English in this milestone; the Turkish UI comes with the locale work.

## 9. Prose layer: Claude Code pipeline

### 9.1 Who runs the pipeline

**Primary path: Rust orchestrates, one Claude Code call per role.**
- The panel's **Write** button, or `harbormap llm write <target>`, shows the cost preview. Nothing runs before the user confirms.
- `hm-learn` then runs the user's local Claude Code non-interactively, once per role: writer, verifier, and the fixer only when needed.
- Each call:
  - gets its role prompt as the system prompt;
  - gets the bundle on stdin;
  - has no tools;
  - returns JSON on stdout;
  - reports its token usage.
- Rust runs the validator between calls, keeps the running token total, stores the result, and can stop between calls.
- The user's own subscription is used; no API key.

**Isolation of each call.**
- The working directory is an empty scratch folder, never the repository. Claude Code therefore loads no project `CLAUDE.md` and puts no git status in the prompt.
- MCP servers are disabled, user and project settings are not loaded, and all tools are disallowed.
- The exact `claude` flags are pinned and verified at the start of plan L4, against the installed Claude Code version. If isolation cannot be guaranteed, the headless path stays off and the fallback is used.

**Opt-in.** The headless path is off until the user turns it on once in settings. The privacy page lists what it sends.

**Fallback: inside the user's Claude Code session.**
- Used when `claude` is missing, not signed in, or the headless path is off. Harbormap queues the target, and the user runs `/harbormap:write` in Claude Code.
- The skill:
  1. runs `harbormap facts …`;
  2. dispatches the plugin's agents (no tools; each returns JSON in its final message);
  3. pipes each result to `harbormap llm validate` and `harbormap llm commit --stdin`.
- In this path the session's own context also grows. Usage is therefore estimated, not measured, and the cost preview says so.

**Plugin layout (`plugins/claude-code/`).** The role prompts live here once and are used by both paths.

```
.claude-plugin/plugin.json
agents/journey-writer.md     # model: sonnet; tools: none
agents/claim-verifier.md     # model: sonnet; tools: none; fresh context
agents/fixer.md              # model: sonnet (haiku to be tested); tools: none
agents/card-writer.md        # model: haiku (to be tested; fall back to sonnet); tools: none
skills/write/SKILL.md        # fallback orchestration for one journey or one card batch
skills/refresh/SKILL.md      # `harbormap llm plan`, preview, run what the user confirms
hooks/hooks.json             # PostToolUse on Bash `git commit …`: print `harbormap llm plan --since-commit` only
```

Agents never write the store. Only `harbormap llm commit` does.

### 9.2 Commit time

The decided rule is that stale text is rewritten on open "or at commit time, after a cost preview". Harbormap installs no git hook and writes nothing into `.git`. Instead, commits are noticed in two ways:
- **`learn` (and later the daemon) watches HEAD read-only.** After a new commit it shows a preview card: "You committed: 3 steps in 1 journey are stale · about 20k tokens · Write now?"
- **The plugin hook.** When Claude Code itself runs `git commit`, the hook prints the same preview in the session.

Either way, nothing is written until the user confirms.

### 9.3 Roles and contracts

1. **Facts bundle.** `harbormap facts journey <id>` returns compact JSON:
   - `lang`, plus a `cited` map with the hash of each step's cited facts at bundle time;
   - `steps[]` with `{n, file, symbol, quote_lines (numbered, redacted), callers, callees, patterns, term_candidates, comment_claims[]}`;
   - `glossary` (pack terms that may be used);
   - `symbols[]` (names allowed in prose).

   Comments are sent only as claims to verify, never as evidence.
2. **Writer.** Fills the step template (8.1) for both levels in one JSON draft. It picks at most one term per step from `term_candidates`, or writes a new term only when none fits. It never emits ids, edges, paths, quotes or file lists; Rust owns those.
3. **Validator** (Rust, `llm validate`, zero tokens). Errors block; warnings go to the fixer. Checks:
   1. The schema is valid (strict, `deny_unknown_fields`).
   2. Every quote reference points into the step's numbered quote, and the quote is 2–8 lines.
   3. Every hop between steps is an edge in the facts.
   4. Every code name in prose is in `symbols[]`, and every number appears in the facts or the quote.
   5. Beginner limits (8.3): sentences, words, code names, no paths.
   6. Terms: ≤ 1 per step, never repeated in the journey, with a definition and an analogy; the term's trigger appears in this step's code or text.
   7. Jargon: glossary words used before their definition are an error.
   8. Every "watch out" cites a line inside the step's range.
   9. Banned verdict words in observations ("bad", "wrong", "insecure", "secure ✓").
4. **Verifier** (fresh context). It gets:
   - the draft's claims;
   - the cited ranges with ±5 lines of context;
   - the definitions (signature and up to 40 lines) of the functions called inside each quoted range;
   - the definitions of helpers named in comment claims (for example the transaction helper behind "this rolls back").

   Without the callee definitions, a wrong claim about what a helper does would come back as `UNSUPPORTED` and be deleted, instead of being caught as `WRONG` or reported as a `FINDING`. The verifier returns one verdict per claim:

   | Verdict | Meaning |
   |---|---|
   | `OK` | Supported by the code |
   | `WRONG` | Contradicted by the code |
   | `UNSUPPORTED` | No line shows it |
   | `MISLEADING` | True, but implies more than the code does |
   | `COMMENT_CLAIM_FALSE` | Repeats a comment that the code contradicts |
   | `FINDING` | A real defect, with its line |

   Claims containing risk words (rollback, atomic, idempotent, cached, thread-safe, retries, validated, secure) get forced scrutiny.
5. **Fixer** (one round; only flagged fields; may not add terms, numbers, names or claims):
   - `WRONG` or `COMMENT_CLAIM_FALSE`: rewrite from the verifier's evidence line, or delete.
   - `UNSUPPORTED`: delete.
   - `MISLEADING`: narrow it.

   The validator runs again. A step that still fails falls back to its template text and is marked `unverified`.
6. **Commit.** `llm commit` validates again, then compares the bundle's `cited` hashes with the current facts:
   - **All equal:** the prose is stored as fresh.
   - **Some changed:** the user saved during the run. The prose is stored, but those steps are marked stale with "the code changed while this was written". No tokens are spent again automatically.
   - **An anchor is gone:** that step is dropped and marked broken.

**Verification depth**
- **Journeys:** every claim is verified (decided).
- **Cards:** the validator always runs. The verifier checks causal and comment-derived sentences only, because the card writer sees facts, not code. This is a builder call that the user can switch to full verification.

### 9.4 Findings ("gözden kaçanlar")

- A `FINDING` verdict creates a `finding` row with its line and the verifier's sentence.
- It is never published automatically. The panel lists it under the subject with "verified by Harbormap's checker; please confirm", and the user can dismiss or keep it.
- Structure observations (7.4) are not findings.

### 9.5 Cache key

The cache key is BLAKE3 over:
- the subject id (repository-relative, §5.3);
- the subject's cited-facts hash (§8.2 for cards; for a journey step: anchor signature, quoted text whitespace-normalised, the edge to the next step and the comments it cites);
- `template_version`;
- `lang`.

The model id is metadata and not part of the key. Prose rows are keyed by this cache key in the per-repository prose store, and `prose_head` points each worktree to its current row. The same facts on another branch or worktree therefore find the same prose. A journey hash is the Merkle hash of its step hashes.

### 9.6 Staleness states

| State | Condition | Cost |
|---|---|---|
| fresh | cited hashes equal | 0 |
| moved | quoted text unchanged, lines shifted; Rust re-anchors | 0 |
| drifted | a body changed, but no cited fact changed | 0; Builder shows "code changed since written" |
| stale | a cited fact changed | rewritten only on open or after a commit preview (§9.2) |
| broken | the anchor symbol is gone | flagged; never guessed |

**Rewrite scope.** Only stale steps are rewritten; fresh steps go in as one-line context. If more than half the steps are stale, the whole journey is rewritten.

### 9.7 Cost preview and budgets

**Estimate for a journey.** `tokens ≈ s × (w + v) + c × o`, plus 15% when a fixer pass is likely, where:
- `s` is the number of stale steps;
- `w` = 2.5k per step for the writer (target; the spike measured 6.4k with code exploration);
- `v` = 3k per step for the verifier, including callee definitions (target; the spike measured 5.3k);
- `c` is the number of calls (2–3);
- `o` = 6k per call, the fixed overhead of Claude Code's own system prompt and the role prompt (estimate).

**Worked examples:**
- a 7-step journey ≈ 7 × 5.5k + 3 × 6k ≈ 57k, or ≈ 65k with a fixer pass;
- a 12-step journey ≈ 66k + 18k ≈ 84k, or ≈ 97k with a fixer pass.

Compared with the spike's 115–140k per journey, this is a 30–55% saving, mostly from removing code exploration. The constants are replaced by measured medians after plan L4.

**Estimate for a card:** 2–4k within a batch of 12, plus one call's overhead per batch.

**What counts.** Every token Claude Code reports for the call counts: input, output, cache writes and cache reads. Counting cache reads in full keeps the preview and the caps conservative.

**Preview text:** "7 steps stale in 1 journey · about 57k tokens · about 3 min". No dollars on a subscription. In the fallback path, the preview adds "estimate only".

**Limits:**
- Before each call, Rust checks the running total plus that call's estimate against 1.3× the preview and against the cap. If either would be passed, the run stops and the finished parts are kept.
- Journey cap: 130k tokens (decided).
- Card cap: 4k.
- Commit-time cap: 150k per commit; anything over that stays stale and is queued.
- At most 5 suggested journeys are offered for "write all".

### 9.8 Token economy (design rules)

1. **No exploration.** Agents get a pre-cut bundle and no tools. This is the largest saving: the spike's writers spent most of their tokens reading code.
2. **Rust owns all deterministic output:** numbering, chips, callers, quotes, file lists.
3. **Glossary packs** supply definitions and analogies.
4. **One call writes both levels**, in one language.
5. **The validator runs before the verifier**, so the verifier never sees a broken draft. The fixer runs at most once, and only on flagged steps.
6. **Content-addressed cache** shared across branches and worktrees. `moved` and `drifted` cost nothing. Only stale steps are rewritten.
7. **Model per role:** haiku is tried for cards and the fixer, and kept only if the gates pass.
8. **Few calls, stable prompts.** A journey takes 2–3 calls in total, not one per step, so the per-call overhead is paid at most three times. Cards go 12 per batch. The prompt prefix (rules, glossary) is stable and placed first, so that prompt caching can apply; its effect on subscription limits is measured, not assumed.
9. **Nothing runs on its own:** writing happens on open or on click, and commits only show a preview.

### 9.9 Privacy

The product principle "no network calls" gets one explicit exception: the user-started, previewed prose path through the user's own Claude Code. It is off by default and on per user setting.

**What is sent:** only the facts bundle (paths, names, redacted quotes, glossary). The repository itself is never sent: each call runs in an empty folder (§9.1).

**Redaction of code.** `hm-core/redact.rs` was built for command lines. Applied to code, it would also mask `password === input` or `apiKey = process.env.API_KEY` and corrupt the quote. Learn adds a code-aware mode:
- it masks only string and number **literals** assigned to secret-named keys;
- it masks high-entropy string literals;
- it masks known key prefixes inside literals.

Quote checks compare redacted text with redacted text.

**Repository content is untrusted** (product design §10, threat 3). Code and comments in a bundle may contain text aimed at the model. The pipeline limits the damage:
- Bundles wrap quotes and comments as data fields, and every role prompt says that this content is never an instruction.
- Agents have no tools, and each headless call runs in an empty folder with MCP and settings disabled.
- Everything an agent returns passes the Rust validator, which rejects any name, number, file or hop that is not in the facts.

The privacy page lists all of this.

## 10. Panel

### 10.1 Template

One template, dark only, built on the panel draft:
- **Colours:** CSS variables, so a light theme can come later. Red means brand and problems; a problem always carries an icon and words. Selection is a white outline.

| Area | Contents |
|---|---|
| Left sidebar | Search (`/`), journeys grouped by area (verb titles, step counts, state badge), file tree with excluded-class counts |
| Centre tabs | **Journeys**, **Map**, **Code** (Branches comes with project facts) |
| Right drawer | Journey step or file/function card; Technical details fold |
| Top bar | Beginner/Builder switch, "Live · updated 2 s ago", branch@commit, Write/Refresh with token estimate |

### 10.2 Journeys tab

- **What is drawn:** only the numbered path. n boxes and n−1 arrows, each arrow with a verb ("asks", "reads", "saves"), plus at most 3 grey context boxes without edges.
- **Placement:** boxes run left to right by step and sit in role lanes (see / work / keep / outside).
- **Repeated boxes and returns:** a box visited twice appears once with badges "2, 5". A return is one arrow labelled "back".
- **Hidden parts:** branches show as "+N other cases"; hubs show as badges. Everything else is hidden, not faded.
- **Navigation:** Prev/Next or clicking a number moves the camera with zero layouts. Leaving the tab returns to the Map in the same positions.
- **Findings:** a step with a finding carries the problem icon and words.

### 10.3 Map tab

- **Overview:** ≤ 12 boxes (folder-first groups with roles; v0.2 §3) and one counted arrow per ordered pair.
- **Technologies:** a strip above the overview with one chip per category; a click opens the technology list (§7.6) in the drawer.
- **Drill-down:** expanding a box opens it in place with precomputed positions, and neighbours stay put. Neighbours outside the box show as "+N elsewhere" chips.
- **File edges** are drawn only for the selected file.
- **Limits:** Beginner ≤ 12 boxes and ≤ 10 arrows; Builder ≤ 25 and ≤ 40; never more than 150 items.

### 10.4 Code tab

A read-only viewer with syntax highlighting. It shows the journey quote's lines and the hand-off line marked. The file is served by Rust on demand.

### 10.5 Performance budgets (targets)

| Metric | Target |
|---|---|
| First screen data | ≤ 100 KB gzipped; per-view payloads lazy (≤ 50 KB); never the whole index |
| JS bundle | ≤ 150 KB gzipped; no layout library, no framework |
| First paint after server reply | ≤ 150 ms |
| Level switch | ≤ 50 ms, zero layouts |
| Expand a group | ≤ 100 ms (animation ≤ 300 ms) |
| Journey step change | 1 frame |
| Live update after save | facts delta on screen ≤ 300 ms p95 after the file write; journey delta ≤ 1 s (§5.7) |

### 10.6 Technology

- **Front end:** TypeScript with no framework, SVG for drawing, compiled at development time (esbuild) and embedded with `rust-embed`. Users need no Node.js. This replaces the product spec's React + Vite choice for this milestone; P4 may revisit it.
- **Server:** `hm-serve` (axum) on 127.0.0.1 only, with a random per-run token in the URL and a session cookie.
  - JSON endpoints per view: overview, group, journey, card, file.
  - Server-sent events for deltas.

## 11. Testing and quality gates

### 11.1 Evaluation set

| Repository | Licence | Commit | Shape | Journeys |
|---|---|---|---|---|
| github.com/fastapi/full-stack-fastapi-template | MIT | `f4bcfaba7c85` | React SPA + FastAPI + SQLModel; UI calls a generated SDK | 4 (log in, add item, recover password, list items) |
| github.com/benbjohnson/wtf | MIT | `05bc90c940d5` | Go web + CLI + SQLite + websocket; interface with 3 implementations; generated templates absent | 4 (create dial, GitHub sign-in, CLI sets value → browser updates, list dials) |
| github.com/spring-projects/spring-petclinic | Apache-2.0 | `500158f73241` | Spring MVC + Thymeleaf + JPA; hidden hops (`@ModelAttribute`, interface repositories) | 4 (search owners, add owner, book visit, list vets) |
| Harbormap (this repository) | AGPL-3.0 | pinned per run | Rust CLI workspace | 3 (`harbormap` with no args, `harbormap why 3000`, a secret on a command line stays hidden) |
| github.com/vercel/next-learn (dashboard/final-example) | MIT | `bb2558441a66` | Next.js App Router + server actions + NextAuth | 3–4 (chosen in L3) |
| A private project | — | pinned | Next.js App Router web app | 4 (kept outside the repository) |

**Gold paths**
- Each journey has a gold path: an ordered `file:symbol (line)` list with core and optional hops, written by reading the code.
- Each journey also carries one trap claim that a tour must not repeat: a comment that lies, a check-then-write race, or an event sent before the data is saved.
- Hops a static call walk cannot follow (data flow by value, websocket pushes without a literal match) are tagged `limit` in the gold path. They are scored separately: the journey must end with the plain "what happens next" note (§7.2.10), not with an invented step.
- The gold sets live in `tests/eval/` as data. Repositories are fetched shallow at the pinned commit into a scratch folder, treated as untrusted, and never built or run.

### 11.2 Gates

**Hard gates** (a journey that fails one is not shipped):
- quote integrity 100%;
- 0 `WRONG` after verification;
- 0 undefined jargon;
- 100% of trap claims caught.

**Soft gates** (mean over the set):

| Gate | Threshold | Spike baseline |
|---|---|---|
| Path recall (core hops found) | ≥ 0.90, none < 0.80 | not scored |
| Path precision | ≥ 0.80 | a helper became a stop |
| Order (LCS of core hops) | ≥ 0.95 | 3 of 3 correct |
| Boundary hops (UI→route, interface→impl, annotation, websocket) | ≥ 0.90 | not tested |
| UNSUPPORTED after fix | ≤ 2% of claims | 1 of 33 steps before fix |
| MISLEADING after fix | ≤ 5% of steps | 12 of 33 |
| Step size | ≤ 90 words; 6–12 steps | 10–12 steps |
| Beginner sentence length | mean ≤ 18 words (EN), ≤ 15 (TR, provisional) | not measured |
| Staleness | one-line edit on a path → flagged ≤ 1 s | not available |
| On-save latency | p95 ≤ 300 ms (1k files) | not measured |
| Tokens per journey | ≤ 65k for ≤ 7 steps, ≤ 100k for 12 steps (§9.7), cap 130k; cached re-open 0 | 115–140k |
| Wall time per journey | ≤ 6 min | 2–5 min |

### 11.3 Automated tests

- **Golden tests per language and framework rule:** small fixture projects with expected symbols, edges and routes.
- **Property test:** after random edits, the incremental state equals a cold index of the same tree.
- **Latency harness:** real file writes in 1k and 5k-file fixtures, with timestamps at write, event, commit and delta. CI fails on a 20% p95 regression.
- **Validator unit tests** for every check, including the spike's failure cases rewritten synthetically:
  - a 15-line quote;
  - a term box unrelated to its step;
  - a term one step early;
  - a watch-out with no line.
- **Eval runner:** `harbormap eval` (dev-only) scores paths against the gold set with zero tokens. With `--prose` it runs the pipeline and records verdicts and tokens.

## 12. Calls made by the designer (check these)

1. `harbormap learn` runs in the foreground until P3. Stores live in the platform data directory: facts per worktree, prose per repository identity (root commit).
2. Five staleness states, with `drifted` for body edits that change no cited fact. Callers are not part of a card's cited facts.
3. Two levels plus a Technical details fold instead of three levels.
4. Cards verify only causal and comment-derived sentences.
5. Rust orchestrates the headless path with one isolated `claude -p` call per role (no tools, empty working folder). It is off until enabled; `/harbormap:write` is the fallback.
6. "At commit time" means Harbormap watches HEAD read-only and shows a preview, plus a Claude Code hook when Claude commits. No git hook is installed.
7. No front-end framework; esbuild at development time.
8. Path walk: depth-first in call-site order with continuations; depth 6, 40 symbols, 3 callees; at most 12 steps, short paths stay short; data-flow and unmatched event hops are stated as limits.
9. Journeys are grouped by area in the sidebar, in place of UA's domain view.
10. The term per step is picked by the writer from Rust's pack candidates; terms live in the prose layer.
11. Token estimate per journey ≈ 5.5k per step + 6k per call (≈ 57k for 7 steps, ≈ 84k for 12), replacing the earlier "~50k" target; the 130k cap is unchanged.
12. Turkish Beginner sentence cap of 15 words, provisional until measured.
13. The UI stays English in this milestone; the prose language is a setting.
14. Reuse of UA's layer path table with attribution; no UA code.
15. Technologies (§7.6) come from manifests, imports and a catalog pack with zero tokens; an unknown package is named, never described by guess.
16. Plan order is a TS-first vertical slice (§14), so a real Next.js project shows in the panel before the other languages land.
17. Go: the `http.Server` handler is the catch-all entry; nested functions and closures label steps by inner name without becoming symbols (§7.2.12).

## 13. Amendments to existing specs

- **Product design**
  - §5.1 and §10: add the prose exception (9.9).
  - §5.2: the optional LLM layer is now in Learn.
  - §6: new roadmap order (section 3); the "Later" item "optional LLM explanations" moves to Learn.
  - §7: new crates; UI stack for Learn.
  - §8: the red/black dark template and colour rules (10.1).
- **v0.1 spec**
  - §2: Learn and Project facts inserted before P2.
  - §13.2: dark only for now.
  - §3: Learn adds no Snapshot fields; `service_link` lives in the Learn store.
- **v0.2 spec**
  - §1: the indexer, tours and journeys move to Learn.
  - §2.3: `Confidence` + provenance.
  - §2.4: 100 ms debounce.
  - §2.6: the store schema is replaced by 5.5.
  - §10.5: two levels + fold.
  - §10.8: superseded by section 9.
  - §11.2: route rules in 6.3.

## 14. Plans

Each plan produces working, tested software.

**Order (amended 2026-11-03).** The plans run as a TS-first vertical slice, so a real Next.js project (the private evaluation project, §11.1) shows in the panel as early as possible: **L1 → L2a → L3 → L5 → L4 → L2b**. Until L4 lands, the panel shows template text (§7.5); the technologies list needs no prose.

- **L1 — Indexer core and TS/JS** (detailed plan: `docs/superpowers/plans/2026-11-02-harbormap-learn-l1-indexer-core.md`).
  - Adds `hm-store` and `hm-index`: walker, file classes and exclusion counts, BLAKE3 hashes, the SQLite schema for files, symbols, edges, refs and reverse imports, and the TS/JS/TSX extractor and resolver (tsconfig paths, barrels).
  - Adds incremental dirty propagation, the `notify` watcher, `harbormap index` and `harbormap card <path>` (template card, facts only).
  - Adds the latency harness and the incremental-equals-cold property test.
  - Done when indexing the evaluation repositories' TS parts and Harbormap's own fixtures meets the latency targets.
- **L2a — TS/JS routes and Next.js.**
  - Adds the TS/JS route and entry rules in 6.3 (Next.js app and pages routers, server actions, middleware, Express, NestJS), client → server URL matching, the TS framework-mediated hops, gaps, and the patterns (7.3) for TS libraries.
  - Adds manifest reading for `package.json` (`bin`/`main`/`scripts`, versions, lockfile) used by entries and §7.6.
  - Golden fixtures per rule.
  - Done when the Next.js evaluation repositories index with their routes and entries found.
- **L2b — Other languages.**
  - Adds extractors, resolvers, route and entry rules, and manifests for Python, Go, Java, C# and Rust.
  - Done when every evaluation repository indexes with its routes and entries found, and the L3 path gates are rechecked on them.
- **L3 — Journeys and cards (deterministic).**
  - Adds seeds and ranking, path computation (7.2), steps with Rust-cut quotes, structure observations (7.4) and template text (7.5).
  - Adds framework-mediated hops (§6.3), areas, glossary packs (EN/TR), the `journeys`, `journey` and `card` commands, the `journey_edge` index and the working-tree state per journey (needs gix status features).
  - Adds the technologies list (§7.6): catalog pack, evidence and the `tech` command.
  - Adds the zero-token eval runner.
  - Done when the path gates in 11.2 pass on the TS/Next.js part of the evaluation set; the rest follows L2b.
- **L4 — Claude Code plugin and prose pipeline.**
  - Starts by pinning and testing the isolated headless `claude -p` call (§9.1) on the installed Claude Code.
  - Adds facts bundles with callee definitions, code-aware redaction, the Rust validator, `llm plan/validate/commit/write`, the prose store and staleness states, and the per-role headless runner with usage capture.
  - Adds the plugin (agents, fallback skills, hook), HEAD watching for commit-time previews, findings and the cost preview.
  - Measures tokens and replaces the estimate constants.
  - Done when the hard and soft prose gates pass on the evaluation set within the token cap.
- **L5 — Panel.**
  - Adds `hm-layout` (grouping, aggregation, cached positions), `hm-serve`, the TypeScript/SVG panel (Journeys, Map, Code, drawer cards, levels, Write/Refresh with preview), live deltas and the performance budgets.
  - Adds the technologies strip and list (§10.3).
  - Done when the panel budgets in 10.5 are met on the TS/Next.js part of the evaluation set.

## 15. Risks and open questions

- **Calls without types.** The accuracy of calls resolved without types is unknown; the boundary-hop gate measures it. If it falls short, journeys show more "probably" hops and fewer steps rather than guessing.
- **Grammar coverage.** The TS and Java grammar releases are from 2024. Coverage of current syntax is to be tested on the evaluation repositories in L1/L2.
- **Headless Claude Code.** Behaviour and usage reporting of `claude -p` with a plugin, and subscription limits (windows, concurrency), are to be verified at the start of L4; if this fails, the fallback becomes primary.
- **Token estimate.** The per-step and per-call constants are estimates until L4 measures them; Claude Code's own overhead per call is the least known.
- **Verifier depth.** Whether a verifier with cited ranges plus callee definitions catches what the spike's exploring verifier caught (the trap gate) is measured in L4. If it does not, the verifier bundle grows before the gate is relaxed.
- **Isolation flags.** The headless isolation in §9.1 depends on `claude` flags that may change between versions; L4 pins and tests them, and the headless path refuses to run when they are missing.
- **Build toolchain.** tree-sitter grammars and bundled SQLite need a C compiler on macOS, Linux and Windows CI; L1 proves the build on all three.
- **Haiku quality.** Whether haiku meets the card style gates is tested on 20 files in L4.
- **Turkish caps.** The Turkish readability caps need measurement.
- **Monorepos.** Which tsconfig, `go.mod` or pom owns a file uses the nearest ancestor; edge cases are documented as limits.
- **Gold paths.** The gold paths come from one reader. A second reading of the stress journeys (CLI → browser update, log in) is done in L3 before the gates are frozen.
