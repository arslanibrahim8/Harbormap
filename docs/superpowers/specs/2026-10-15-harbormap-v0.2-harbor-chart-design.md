# Harbormap v0.2 — Harbor Chart (code map, story, tours) — Design

- **Status:** approved design, 2026-10-15. Designed now, built after v0.1. Amended 2026-11-02 by the [Learn design](2026-11-02-harbormap-learn-engine-design.md): see the note below.
- **Parent:** [Product design](2026-10-15-harbormap-product-design.md)
- **Evidence:** [Research summary](../../research/2026-10-15-research-summary.md), sections 5–7

> **Amended 2026-11-02.** The indexer (§2), grouping (§3), edge aggregation (§4), the layout core (§5.1–5.3), tours (§10) and route journeys (§11.2) are built earlier, in the Learn milestone, as revised by the [Learn design](2026-11-02-harbormap-learn-engine-design.md). Where they differ, Learn wins:
> - §2.3: confidence is P1's `Sure/Probably` plus these provenance tags (Learn §5.2);
> - §2.4: 100 ms debounce, per-symbol signature and body hashes (Learn §5.4);
> - §2.6: store schema replaced by Learn §5.5;
> - §7, §10.5: two text levels (Beginner, Builder) plus a Technical details fold;
> - §8: dark red/black template (Product design §8 note);
> - §10: tours are request journeys with a fixed step template (Learn §7–§8); "LLM-written tours" is no longer a non-goal: prose is optional, user-started and verified (Learn §9), and §10.8 is superseded;
> - §11.2: route rules in Learn §6.3.
>
> v0.2 keeps the large-repository path (§5.4 tiles, §6 WebGL), the full code↔runtime bridge (§11.1) and the 50k-file budgets (§12).

## 1. Goal

Show how a project's code fits together, on the same map as what runs, so that it stays readable from 50 to 50,000 files. Explain it from the big picture down, step by step, with visuals. The browser must never lag or crash, and no LLM is needed.

**Non-goals for v0.2:**
- whole-repository node-link views;
- 3D views;
- LLM-written tours;
- request journeys for every framework (only the frameworks in §11.2).

**Design rules learned from Understand-Anything (UA), all verified in its source and issues:**

| UA behaviour | Harbormap rule |
|---|---|
| ELK ran on the UI thread | Layout never runs on the main thread when a view opens or a tour step changes |
| A tour step was only a list of node IDs, and each step could clear caches, auto-expand containers without limit, and re-lay out a whole layer | Tours are precomputed data. A step changes highlights and the camera only |
| The whole graph was sent to the browser and every file was rendered as a DOM card | The browser receives only what it draws |
| Tour order came from Kahn's algorithm over every edge type; tests came first | Typed, directed, entry-seeded ordering |
| Steps listed "Key files: <every name>" | Hard limits on what a step shows |
| Personas only filtered node types | Real three-level text per step |
| Tours went stale silently (one graph sat ~650 commits behind) | Anchors, staleness detection and badges |

## 2. Indexer (`hm-index`)

### 2.1 Languages and grammars

| Language | Grammar |
|---|---|
| JavaScript / TypeScript (incl. JSX/TSX) | tree-sitter-javascript, tree-sitter-typescript |
| Python | tree-sitter-python |
| Go | tree-sitter-go |
| C# | tree-sitter-c-sharp |
| Java | tree-sitter-java |

### 2.2 Extracted facts

All facts are deterministic.
- Files and directories, with language, size, line count and a generated/vendored/test classification.
- Module and package boundaries: package.json workspaces, pyproject/`__init__` packages, go.mod modules, `.csproj` projects, Maven/Gradle modules.
- **Imports**, resolved to files wherever possible:

| Language | Resolution |
|---|---|
| TS/JS | Node resolution plus tsconfig `paths` / `baseUrl` |
| Python | Package-relative and absolute imports within the project |
| Go | Module paths via go.mod |
| Java | Package → directory |
| C# | `using` namespace → declaring files (heuristic, so marked `inferred`) |

- **Symbol definitions:** functions, methods, classes, interfaces, structs, enums, with signatures, byte ranges and body hashes.
- **Calls**, resolved by name and import qualification:
  - `parsed`: the target is unique and import-qualified;
  - `inferred`: otherwise.
- **Inheritance and implementation**, from class and interface headers.
- **Routes** from framework packs (§11.2).
- **HTTP client calls** with literal or template-literal URLs: fetch, axios, requests, httpx, `http.Get`/`NewRequest`, `HttpClient`, RestTemplate/WebClient.
- **Env reads:** `process.env.X`, `os.environ[...]`/`getenv`, `os.Getenv`, `Environment.GetEnvironmentVariable`, `System.getenv`, framework config bindings. Key names only.
- **Infra files as nodes:** Dockerfile (base image, EXPOSE, CMD), compose services, CI workflows (names only).

### 2.3 Provenance

Every edge carries a provenance tag:

| Tag | Meaning |
|---|---|
| `parsed` | Read directly from the code |
| `matched` | Resolved by pattern, e.g. a URL template matched to a route |
| `config` | Comes from env or compose |
| `inferred` | Heuristic |
| `observed` | Seen at runtime, from v0.1 data |

It also carries a confidence level of `exact` or `probable`.

### 2.4 Incremental indexing

- **Change detection:** a (size, mtime) precheck, then a BLAKE3 content hash. A Merkle tree over directories detects branch switches and checkouts cheaply.
- **File watching:** notify, with a 2 s debounce (amended: 100 ms, Learn §5.4). Only changed files are re-parsed, followed by a graph diff.
- **Freshness stamp:** the index records the commit and the time it last caught up. Every view and every MCP answer shows it.
- **Shared across worktrees:** one store keyed by repository, with per-worktree overlays.

### 2.5 Exclusions (counted, never silently dropped)

- **Tests:** `test/`, `tests/`, `__tests__`, `*_test.go`, `*.spec.*`, `*Test.java`, `*Tests.cs`.
- **Generated:** header markers, `*.g.cs`, `*_pb2.py`, `dist/`, `build/`, `.next/`, `target/`, `bin/`, `obj/`.
- **Vendored:** `node_modules/`, `vendor/`, `third_party/`.
- **Docs.**

The beginner view lists them as "Not on this map, to keep it simple: tests · N files …". Tests stay available to experts as a filter.

### 2.6 Storage (SQLite)

Tables:
- `files`, `symbols`, `edges` (typed), `routes`, `http_calls`, `env_reads`;
- `groups` (the hierarchy) and `group_edges` (aggregated);
- `layouts` (blobs keyed by group hash + layout-parameter version);
- `tiles`;
- `tours`, `steps`, `anchors`;
- `stories`.

## 3. Grouping and hierarchy

Grouping is deterministic and folder-first.

**Why folders first:** architecture-recovery algorithms are neither accurate nor stable across versions, and directory baselines beat them on well-organised systems.

1. **Primary hierarchy:** monorepo packages and apps, then the folder tree. Runtime units (compose services, processes) map onto their code folders (§11.1).
2. **Repairs:**
   - merge chains of single-child folders;
   - show a folder with ≤10 children flat;
   - split a folder with >20 children (step 3).
3. **Splitting an oversized flat folder,** in this order:
   1. Merge siblings with identical dependency sets into one "×N" node.
   2. Group by dominant file-name tokens (`*Repository`, `*Controller`).
   3. Last resort: Leiden community detection inside that folder only, with a fixed seed, stable input order and ≤~60 children per group.
   4. A cohesive flat folder stays one group named after the folder.
4. **Hubs:** files with in-degree above 20 go to a "Shared" group. Their incoming edges become a badge with a count.
5. **Names**, in priority order: folder name → dominant file-name token → hub file → "Shared". Never "Cluster N". Beginners also see a role word from the roles pack (routes → "Server endpoints").
6. **Roles for colour and lanes:** `see`, `work`, `keep`, `outside`, `shared`, inferred from signatures, routes, UI frameworks, DB access, external HTTP hosts and runtime links.
7. **Edge weights** count symbol references, not import statements.

## 4. Edge aggregation and visibility

- **Aggregation.** Every edge is lifted to the deepest visible ancestors of its endpoints, so there is one edge per ordered pair.
  - Width is proportional to log(count), and the count is shown as a label.
  - A→B and B→A are never merged.
  - Computed in O(E) by attaching each edge at the lowest common ancestor (Euler tour + sparse table).
- **One edge type per layer:** calls, imports, inherits, network, co-change. Filter chips switch layers on and off.
- **Visible by default:**
  - problems: cycle and upward edges, rule violations;
  - the selected box's edges;
  - network edges between runtime boxes in L1.

  Everything else appears on hover or selection.
- **Bundling.** Spatial bundling is never used, because it creates false connections and slows path-following. Hierarchical bundling is available only in an optional expert "coupling wheel" view.
- **Cycles:**
  - each strongly connected component is condensed into a red **tangle** node with a "↻N" badge;
  - inside the tangle, a minimum feedback set is drawn dashed, routed to one side;
  - collapsed groups carry "↻N" and "↑N" (upward edges) badges.
- **Direction** is shown by layout position (tiers flow one way) plus a mid-edge arrow or tapered edge, never by colour alone.
- **Evidence.** Clicking any edge opens the exact source lines behind it: imports, call sites, the config line.

## 5. Layout engine (Rust, precomputed)

Three rules:
1. The browser lays out at most ~150 nodes, and only for ad-hoc subgraphs.
2. The browser never receives more than it draws.
3. Tours are data, not computation.

### 5.1 Graph core

- Dense u32 IDs, with a CSR adjacency per edge type.
- petgraph for strongly connected components, condensation and topological sort.

### 5.2 Per-group layout

Sizes are computed bottom-up and positions top-down into one world coordinate space. Groups are laid out in parallel with rayon.

| Group shape | Algorithm |
|---|---|
| Mostly acyclic | Layered (Sugiyama): feedback-arc removal → network-simplex layering → median crossing reduction → Brandes–Köpf placement. Candidates: `rust-sugiyama` (MIT) or `dugong` (MIT/Apache, version pinned); otherwise our own implementation |
| Cyclic | Stress majorisation seeded with PivotMDS; sparse stress / SGD above ~1k nodes |
| Groups with constraints | fCoSE-style constrained layout (`manatee`, MIT/Apache) |

- **Tiers** (entry → logic → data → external) are enforced as partitions, so dependencies flow one way.
- **Optional "galaxy" overview** of all files: ForceAtlas2 with Barnes–Hut, seeded from hierarchical positions.

### 5.3 Determinism and stability

- Stable sorts and fixed seeds; a layout-parameter version is stored with each layout.
- Positions are cached per group by a Merkle hash: the hash of the group's children plus its own edges.
- **Incremental relayout:**
  - only changed groups are laid out again;
  - each starts from its previous coordinates, with an anchoring term;
  - parents keep 15–20% spare room so a child change rarely forces a parent repack;
  - repacking happens only past that threshold.
- Code changes are shown as overlays (a "changed since" tint). Regrouping happens only on explicit **Refresh layout**.
- Expanding a group reveals its contents in place, and neighbouring groups keep their positions. Back/forward history and breadcrumbs are kept.

### 5.4 Level of detail and tiles

- **Tile pyramid:** a quadtree over world space with about 500 elements per tile, built in Rust. A published pyramid design held p95 frame time ≤18.4 ms up to 32k nodes and 237k edges; building it in the browser took 41–173 s.
- **Labels:** ranked by PageRank. A non-overlapping label set is precomputed per zoom level (R-tree).
- **Tiles invalidated** only where a change touches them. Changed IDs are pushed over the WebSocket, and the client animates moved nodes in ≤300 ms.

### 5.5 Guards against degenerate input

| Case | Guard |
|---|---|
| 10k files in one folder | Forced split (§3) |
| One hub with 20k edges | Hub bucket plus a badge |
| A 5k-node strongly connected component | Tangle view with a matrix |
| Coincident points | Jitter by stable hash |
| Any expansion beyond budget | Siblings collapse with a notice |

## 6. Rendering (browser)

| View | Renderer | Cap |
|---|---|---|
| Overview, cluster map, file map | One WebGL2 canvas: **deck.gl 9.4** (OrthographicView, TileLayer, Scatterplot / SolidPolygon / Line / Text layers, GPU label collision) **or Sigma 4** — decided by a week-1 benchmark | Tiles, ≤5k drawn elements per frame |
| Focused neighbourhood, path, dock interior | React Flow (`@xyflow/react` 12.12), positions from the backend, no in-browser layout | ≤300 nodes |
| Tour flows and sequence diagrams | SVG or React Flow, precomputed left-to-right layout | ≤50 nodes |
| L1 Harbor view (runtime + top groups) | SVG (shared with v0.1) | ≤40 elements |

**Data path**
- A data worker fetches **binary struct-of-arrays** payloads over HTTP. For scale: 50k nodes plus 150k edges is about 1.6 MB, against 6.55 MB as JSON objects.
- The worker builds typed arrays, a Flatbush hit-test index and a search index, then transfers the buffers without copying. Each request has an AbortController.
- Labels come from a string table loaded on demand.
- The WebSocket carries only invalidation events.

**Main thread**
- React draws only the chrome. The canvas is driven outside React.
- Highlight state is a per-node GPU attribute or texture, so a highlight change costs only the nodes that changed.
- Rarely, ad-hoc layouts of ≤300 nodes run in a layout worker with elkjs ≥0.12 (`workerUrl` set, GPL-3.0-or-later option). They have a time budget and are cancelled with `terminate()`.

**Frame rules**
- Straight instanced edges; no antialiasing or translucency on bulk edges.
- Edge labels only on focus; ≤300 labels at a time.
- One WebGL context, rebuilt from cached buffers if it is lost.
- No per-frame allocation; no infinite CSS animations.

## 7. Views and budgets

All views live in one window, with breadcrumbs and back/forward.

**Per-screen budgets** (hard cap ~150 elements; the rest collapse into "+N more" chips; problems always survive as badges):

| Level | Budget |
|---|---|
| Beginner | ≤12 boxes, ≤10 arrows |
| Builder | ≤25 nodes, ≤40 edges |
| Expert | ≤50 nodes as node-link (≤100 when there are ≤2 edges per node), otherwise the matrix (DSM) |

**Views**

| View | Default for | Content |
|---|---|---|
| **L1 Harbor** | Beginner | Story panel first (§9), then lanes of runtime boxes and top-level code groups, with plain-verb network labels. Main action: **Follow a request** |
| **L2 Dock** | Builder | A box expanded in place. Modules stacked by dependency level with counted edges; route tabs on the box's port; change tint; cycle badges; an edge-evidence panel |
| **L3 Blueprint** | Expert | Symbol focus: callers left, callees right. Provenance on every edge, path finder, matrix toggle, metrics |
| **Focus** | Builder / Expert | Dependents left, dependencies right; depth 1 by default; depth 2 ranked by degree of interest and capped; hidden neighbours shown as counted stubs |
| **Path A→B** | Builder / Expert | Left to right; long chains collapse into a dashed "indirect" edge that can be expanded |
| **Flow** | Builder / Expert | Sequence diagram for a route or a recorded request |
| **DSM** | Expert | Levelised row order, cycles as red blocks, cell counts; clicking a cell opens the references; context help built in |

**Suggestions, never silent switches.** The UI suggests:
- the matrix when there are >50 nodes or ≥4 edges per node;
- a list when one hub has dozens of leaves;
- Path when two nodes are selected;
- Flow when a route is selected;
- DSM when a tangle is opened.

The only automatic rule is the budget: siblings collapse, with a notice.

## 8. Visual language

Defined in Product design §8 and kept here.

| Element | Meaning |
|---|---|
| Boxes | Things that run |
| Port tabs | Filled = listening, hollow = closed, red = taken |
| Code inside boxes | Code runs in the box that contains it |
| Solid line | Code uses code |
| Dashed line | Network; labelled with a verb (Beginner) or env var / route (Builder) |
| Dashed amber line | Inferred |

**Colour**
- Role colours: see = blue, work = violet, keep = amber, outside = pink, shared = slate, people = neutral.
- State colours are separate and always paired with words.
- Tour focus is highlighter yellow (light) or muted gold (dark).

**Themes and accessibility**
- Light and dark themes with the same tokens.
- WCAG 2.2 AA contrast; a table equivalent for every view; keyboard navigation.

## 9. Story panel (narrative first)

**Why:** many people cannot read a diagram at first sight, so the beginner view leads with prose.

**Chapters** are generated from facts by templates in `packs/texts`, at three levels:
1. **What it is.** README title and first sentence, shown as escaped text and never fed to a model, plus the counts of parts.
2. **What people see.** `see`-role groups and services, with URLs when they are running.
3. **What does the work.** `work`-role groups, the main API, workers.
4. **Where things are kept.** `keep`-role stores and what writes to them (from `writes_to` / ORM facts).
5. **Outside services.** External hosts from HTTP calls and env URLs.
6. **Right now.** Open findings (from v0.1) and changes since the last commit or agent session.

**Behaviour**
- Clicking a chapter highlights its parts on the map, using the same highlight mechanism as tours.
- **Absence:** when a fact is missing, the sentence is omitted. Nothing is guessed.
- Templates follow the rule "no fact, no claim". Every claim can show its evidence.

## 10. Tour engine

### 10.1 Tour kinds

| Kind | Content |
|---|---|
| **Overview** | ≤12 steps: what it is → what runs → where it starts → chapter map → 2–4 key components → one request journey → recap and optional quiz → a safe starter task → where to go next |
| **Journey** | "What happens when you X", e.g. log in. Path: UI page → client call → route → handler → service → data access → DB config → container → port. ≤6 boundary hops; labelled "possible path" when any hop is `inferred` |
| **Chapter** | One per top-level group, 3–7 in total |
| **Startup** | Boot order: config → connections → routes → listen |
| **Change** | What an agent session or a commit changed, at the architecture level |

### 10.2 Structure (martini glass)

- **Between chapters:** author-led, general → specific.
- **Within a chapter:** follow execution order.
- **Per step:** one change, the same step pattern every time, ≤1 focus, ≤3 new and ≤3 context nodes. Items left out are summarised ("+N like this", with one example and the reason).
- **At the end:** free exploration.

### 10.3 Generation (at index time, cancellable background job)

1. **Simplify.** Roll symbol edges up to files and modules, weighted by kind. Exclude tests, generated and vendored code, but count them.
2. **Entry points**, scored by evidence:
   - declared: package.json `main` / `bin` / scripts, console_scripts, Go `func main`, C# `Main`, Dockerfile `CMD`, compose `command`;
   - framework convention: routes, pages, CLI commands;
   - graph shape: never imported, far-reaching.

   Keep the top 1–3 per kind.
3. **Rank.** PageRank restarting at the entry points, with reverse edges at half weight. Utilities (high fan-in, low fan-out) are demoted. Users can pin.
4. **Depth bands.** Condense cycles (Tarjan), then band by distance from the entry points: UI → API → domain → data → infra.
5. **Chapters.** Cut the group tree into 3–7 sizeable chapters. Leftovers become "Shared helpers".
6. **Journeys.** From each top entry point, take the best path of ≤6 boundary hops that skips utilities and ends at storage, an external call or a runtime link. Merge hops that fall within one chapter.
7. **Choreography,** precomputed for each step and level:
   - camera bounding box;
   - highlight sets (focus, new, context) as sorted u32 indices;
   - ordered path edges;
   - transition type;
   - code anchors.
8. **Determinism.** Stable sorts, fixed seeds, a generator version stamp, and golden tests on fixture repositories.

### 10.4 Step data model

| Field | Content |
|---|---|
| `id` | Stable, from the subject (e.g. `journey:POST /api/login:data`), never a position |
| `kind` | Step type |
| `facts` | Each fact with its proving edges, plus a facts hash |
| per level | Template values, highlight roles, camera box, path edges, transition |
| `anchors` | Path, symbol, signature hash, body hash, hand-off line, last-known line range |
| `quiz` | Optional |
| `omitted` | Summary of what the step leaves out |

### 10.5 Text and quizzes

- **Templates.** Per step type and level, with ICU-style placeholders filled only from facts. Template variants are chosen by hashing the step ID, so wording is stable.

| Level | Text |
|---|---|
| Beginner | Role words, glossary chips, ≤3 sentences, a "peek" at code with a plain-English line note |
| Builder | Names, frameworks, edge verbs, ≤12-line snippets with the hand-off line highlighted |
| Expert | Bullets with fan-in/fan-out, cycles, violations and line numbers |

- **Quizzes** (optional) test application, not recall: "order the hops", "which file serves POST /api/login?", "what breaks if X changes?". Distractors are same-type nodes from other chapters, chosen by hash.

### 10.6 Anchors and staleness

- Anchors point to symbols, not line numbers.
- **On re-index**, each anchor is resolved in order:
  1. same signature and body;
  2. ≥75% body similarity in the same file;
  3. same signature with ≥50% body similarity elsewhere;
  4. line-level matching;
  5. otherwise the anchor is broken.
- Moves are fixed silently. Missing code is flagged, never guessed.
- Tours are rebuilt cheaply, and "changed since you saw it" badges appear on affected steps.
- CodeTour `.tours` can be imported and exported; human pins and notes are stored as patches keyed by step ID.

### 10.7 Playback (cost proportional to the step's highlight set; zero layouts)

On **Next**:
1. Cancel in-flight work.
2. Write the difference between the old and new highlight sets into the GPU state attribute: ≤~40 flags.
3. Animate the camera with van Wijk–Nuij smooth zoom (≤1 s), or crossfade when reduced motion is on.
4. Show the step text immediately.
5. Show the snippet from a prefetch cache. Snippets are highlighted on the server by tree-sitter.

**Memory**
- Prefetch steps i±1 into a bounded cache: 3 steps, 32 snippets, ≤64 MB. Memory stays flat however long the tour is.
- Rapid clicks merge into one step change.

**On screen**
- Off-step nodes at ~15–18% opacity; context at ~50–60%; new nodes at full opacity with one pulse.
- Path edges draw one by one.
- Text sits in a fixed side panel; the camera keeps that area clear.

### 10.8 Optional LLM (later, opt-in)

> Superseded 2026-11-02 by Learn §9 (Claude Code prose pipeline).

An LLM may rewrite one step's text from that step's facts only (~200 tokens):
- the result is cached by facts hash;
- it is rejected if it names anything outside the facts or changes a number;
- it never picks nodes or order.

## 11. Code ↔ runtime bridge

### 11.1 Mapping chain

| Link | How it is made |
|---|---|
| Port → process / container | v0.1 |
| Container → compose service | Labels, `working_dir` |
| Compose service → build context or folder | Compose `build.context` / volumes |
| Process → folder | cwd and the package manifest |
| Folder → app / package group → entry file | §10.3 step 2 |
| Entry file → routes | Framework packs |

Each link carries provenance. Unmapped links are shown as "unlinked", never guessed.

### 11.2 Routes and journeys (v0.2 frameworks)

| Language | Frameworks |
|---|---|
| TS/JS | Express, Next.js (app and pages routers) |
| Python | FastAPI, Django (`urls.py`, basic) |
| Go | net/http, Gin |
| C# | ASP.NET attribute routes |
| Java | Spring `@*Mapping` |

**Client→route matching** joins HTTP client calls with literal or template URLs to route patterns.
- Any part of a link that comes from a base-URL wrapper is marked `inferred`.
- Calls that cannot be matched are listed as "unlinked".

**Publishing accuracy.** Precision and recall are published per framework, measured on fixture repositories. A known baseline from prior art: 21/27 routes found plus 32 spurious entries on GitNexus's own server, and 0/9 callers found through base-URL wrappers. We must beat it.

## 12. Performance budgets (p95) and benchmarks

**Budgets**

| Metric | Target |
|---|---|
| Overview interactive | ≤1.0 s at 50k files and ≤400 ms at 10k (warm cache) |
| Cold precompute | ≤30 s at 50k files; the folder tree is shown meanwhile |
| Frame time | ≤16.7 ms; ≤4 ms main-thread JS; no long task over 50 ms; ≤5k drawn elements |
| Tour step | Highlight within 1 frame; text ≤50 ms; snippet ≤100 ms (≤16 ms when prefetched); **zero layouts** |
| Edit 10 files | ≤400 ms end to end; ≤1% of unchanged visible nodes move more than 20 px |
| Memory | JS heap ≤300 MB; GPU ≤256 MB; ≤5% heap growth after 500 tour steps |
| Idle CPU | ≈0 |
| Interaction to Next Paint (INP) | ≤100 ms |

**Benchmark repositories**

| Repository | Code files (js/ts/py/go/cs/java) |
|---|---|
| fastapi | 1,171 |
| nest | 2,043 |
| django | 3,044 |
| kafka | 6,419 |
| spring-framework | 9,299 |
| go | 11,849 |
| vscode | 15,075 |
| kubernetes | 17,934 |
| roslyn | 18,267 |
| home-assistant | 19,539 |
| elasticsearch | 33,165 |
| kibana (overload test) | ≥103k |

Plus synthetic graphs of 10k, 50k and 100k nodes with 3–10× edges, and the degenerate cases in §5.5.

**Harness**
- **Tools:** Playwright with Chrome DevTools Protocol traces, the Long Animation Frames API, frame-gap timing and heap snapshots.
- **Machines and browsers:** an 8 GB laptop with integrated graphics, an Apple M1, and Linux with Mesa, each on Chrome, Firefox and WebKit.
- **Scripts:**
  - cold and warm open;
  - 12 zoom dives;
  - a pan sweep;
  - a 500-step tour with random jumps;
  - typing in search;
  - edits to 1, 10 and 100 files;
  - a 30-minute idle soak.
- **CI:** small and medium repositories on every PR; large ones nightly; hard failure thresholds.
- **Week-1 spike** (throwaway): deck.gl vs Sigma 4 at 50k nodes and 150k edges on the harness, which decides the overview renderer.

## 13. Testing

- **Indexer:** golden fact files per language fixture (imports, symbols, calls, routes, env reads) with `parsed` / `inferred` expectations.
- **Grouping and layout:** determinism (same input → byte-identical layout blob); stability (an edit moves ≤1% of nodes); degenerate-input guards.
- **Tours:** golden tours on fixture repositories; anchor resolution after scripted refactors (rename, move, extract); every step obeys the step budget.
- **Story:** "no fact, no claim" property tests; every sentence has a source fact.
- **UI:** Playwright scripts for budgets and performance (§12); axe checks; keyboard navigation; light and dark themes.

## 14. Risks and open questions

| Item | Plan |
|---|---|
| Rust layout crates are young (`rust-sugiyama`, `dugong`, `manatee`, `fa2`) | Wrap them behind our own trait. Be ready to own a Sugiyama implementation (about 1–2k lines). Benchmark in week 1 |
| Name-based call resolution has false positives | Provenance tags and "probable" wording; optional SCIP or language-server precision later |
| deck.gl vs Sigma 4 | Decided by the week-1 spike |
| Journey accuracy per framework | Publish precision/recall; "unlinked" and "possible path" states |
| Generated story wording quality | Five-user tests per level before v0.2.0 |
