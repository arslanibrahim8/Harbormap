# Harbormap spec gaps vs requirements A-G (2026-10-16)

Abbreviations: PD = product design, V1 = v0.1 spec, V2 = v0.2 spec, RS = research summary, P1 = alpha.1 plan part 1.

## 1. Per requirement

**A. Visual, first-look explanation**
- Decided: PD §3 (three levels), PD §8 + V1 §13.3 (lane map: People / See / Work / Keep / Outside), V1 §13.4 and V2 §9 (story panel, 6 chapters), V2 §7 (L1 Harbor default for beginners, budgets <=12 boxes).
- Missing: a single "first screen" definition. Story panel is prose-first (PD §3), but the draft is map-first with a drawer. Nothing says what the one-line headline or "project at a glance" strip contains.
- Conflict: PD §3 "story first, then small map" vs the draft's map-first layout.

**B. File-graph complexity (hairballs, recompute)**
- Decided, well: V2 §1 (UA lessons), §3 (folder-first grouping, hubs), §4 (one aggregated edge per ordered pair, count labels, hidden by default, no spatial bundling), §5 (Rust precompute, Merkle-hash cache, stable layout), §6 (tiles, binary arrays), §12 budgets, PD §8 readability budgets.
- Missing: the draft's "folder edges with counts; file edges only for the selected file" matches V2 §4 but nothing says it is the default Code-tab behaviour. V2 §5-6 (tile pyramid, WebGL, deck.gl/Sigma) is heavy overkill for the single-template UI; no "light path" (static JSON, SVG, <=150 elements) is specified.
- Conflict: PD §7 says React+Vite embedded UI; the new UI is a single HTML template. V2 §4 says "network edges" are visible by default in L1; the draft shows folder bundles only.

**C. Deploy/CI, services, databases, Docker, cloud each in a clear place**
- Decided: runtime services, Docker and databases (V1 §6.4, §8 roles, `keep` lane); V2 §2.2 "Infra files as nodes: Dockerfile, compose, CI workflows (names only)"; V2 §11.1 code-to-runtime chain.
- Missing: any "Going live" concept. No deploy targets (Vercel, Fly, Render, k8s, Terraform), no CI pipeline content (jobs, triggers, deploy step), no cloud resources, no DB schema/tables (V2 §9 mentions `writes_to`/ORM facts but §2.2 never extracts them), no env/secrets-presence view. Lane model has no lane for "Build and ship" or "Cloud".
- Conflict: lanes are fixed at five (PD §8, V1 §13.3). CI "names only" (V2 §2.2) is too thin for a ship lane.

**D. Optional cached LLM explanations**
- Decided: PD §5.2 (lazy, cached, cost-previewed, may rephrase facts, never decides), PD §6 "Later, not scheduled", V2 §10.8 (per step, ~200 tokens, cache by facts hash, rejected if it adds names/numbers).
- Missing: scope beyond tours (cards, file drawer, project summary), provider config (local vs API key; conflicts with "no network" PD §5.1/§10), cost preview UI, cache table, what is sent (redaction rules for code snippets), and "what the file is for" generation.
- Conflict: PD §5.1 "no network requests" vs any hosted LLM call. Needs an explicit exception (user-supplied key or local model, off by default, listed in privacy page).

**E. Insight too basic (patterns, "does the structure fit")**
- Decided: facts and evidence tags (PD §5.5, V2 §2.3 parsed/matched/config/inferred/observed), file roles (V2 §3.6), hotspots, cycles, conventions only in v0.5 (PD §6).
- Missing: a "patterns" extractor (REST endpoint, DI, retry, ORM model...) and a structure-fit verdict. Neither is in V2 §2.2 nor in v0.5's list. "Methods it uses" with `matched`/`config` is a call-graph view over V2 §2.2 calls + §11.2 route matching, which is covered; "patterns" and "fit" are not.
- Conflict: PD §5.5 "no absolute verdicts"; a fit verdict must be worded as evidence-backed observation. Rule overlap with v0.5 (god files, cycles, conventions) must be resolved.

**F. Git branch map**
- Decided: only `Worktree{path, branch, head}` (V1 §3, `model.rs`), WT-001..003 (V1 §9.7), "Change" tour kind (V2 §10.1), freshness stamp commit (V2 §2.4).
- Missing: everything else: branch list, ahead/behind main, merged, stale, running services per branch (service already has `worktree`, so the join is cheap), "what went into main" timeline, `harbormap branches` CLI, MCP exposure.
- Conflict: none directly; V1 §18 / PD §6 have no git-history feature. PD §5.1 allows no network, so no remote/PR data (local refs only).

**G. Single template, dark, red/black**
- Decided: nothing; the opposite is decided. PD §8 and V2 §8: blue/violet/amber/pink/slate role colours, light+dark themes, WCAG AA both themes, tokens per theme (V1 §13.2).
- Conflicts: (1) role palette is overridden; (2) "light and dark from v0.1" vs dark only; (3) PD §8 rule "colour has two jobs, never mixed" is broken if red is both brand and state/problem/cycle/"taken". Red-as-brand needs another state cue (icon + word, which PD already requires) and a separate danger tone; (4) lanes are told apart by colour plus words; with one hue, role must rely on lane headers, icons, and shape; (5) provenance colours (amber inferred, teal observed) need re-mapping.

## 2. Roadmap and best fit

Current: v0.1 alpha.1 See (P1 inventory built; P2 safe stop; P3 daemon; P4 dashboard), alpha.2 Diagnose, alpha.3 Start, alpha.4 Recipes, beta Agents, 0.1.0; v0.2 Harbor Chart (indexer, tours, bridge); v0.3 Safety Net (signed); v0.4 Agent Context; v0.5 Health & Conventions.

| Req | Best fit | Why / guard |
|---|---|---|
| G theme/palette | P4 (dashboard template) now; amend PD §8 first | Cheap if decided before P4 is written. Keep tokens in CSS variables so a light theme can return. |
| A first look | P4 + story panel (V1 §13.4) | Runtime-only story is already scoped; add a headline strip. |
| F branches | Data + CLI in P2-P3 timing (after P1, no UI), screen in P4 | New `hm-project` git module on gix; adds `branches` to CLI. Does not touch the safety work. |
| C runtime parts (services, DB containers, Docker) | P4 (already in snapshot) | Done by lanes. |
| C going live/CI/cloud | New small "Project facts" step: file-based detectors (workflows, Dockerfile, compose, fly.toml, vercel.json, Terraform, k8s) in alpha.2 or a new alpha.1-P5; UI in P4 as a read-only "Going live" lane/section | Pure file parsing, zero-token, no network. V2's `hm-index` is too late/heavy for this. |
| C DB schemas | v0.2 indexer (ORM/migration/SQL files) | Needs parsing; flag in V2 §2.2. A lighter v0.1 slice: migration folder + compose DB image only. |
| B file graph | v0.2 (as specified) | Draft's Code tab should ship only as a sample/disabled in v0.1 dashboard, to avoid promising v0.2 data. Add a "lite" mode (folder-level only, static JSON) if the Code tab must exist earlier. |
| E patterns, fit | v0.2 (patterns, evidence tags) + v0.5 (fit) | Fit verdict is a v0.5 conventions feature; in v0.2 show patterns only. |
| D LLM | After v0.2, "Later" in PD §6; design the cache table now | Core stays zero-token; cache keyed by facts hash (V2 §10.8). |

## 3. Data: P1 snapshot has vs needs

Has (`Snapshot` schema 1): host, projects (id, name, root, opened, git, worktrees with branch+head), services (name, section, project_id, worktree, signature, framework, confidence, role, kind, source, ports, urls, state, health), listeners, processes (cwd, uptime, rss, env names + allowed values), containers (image, state, health, ports, networks, mounts, compose project/service/working_dir/config files/hash), engines, notes. Manifest reading (`hm-project/manifest.rs`) and signature packs.

| Needed | Status | Where it belongs |
|---|---|---|
| Deploy/CI (workflow jobs, triggers, deploy targets, environments) | absent (V2 §2.2 names only) | new `ProjectFacts` section, file parsed; add to Snapshot or a sibling `ProjectModel` |
| Cloud resources (Terraform, k8s, platform configs, env-key names per environment) | absent | same; names only, no values |
| DB schemas (tables, columns, migrations, ORM models) | absent; `keep` role exists only for running DBs | v0.2 indexer; link table to container via env URL |
| Env-var graph (who reads which key, `.env.example` keys) | process env names only | V1 §10.3 preflight / V2 §2.2 env reads |
| File graph, symbols, routes, patterns | absent (v0.2) | `hm-index` + SQLite tables V2 §2.6; add `patterns`, `fit` |
| Git branches: list, tip, ahead/behind vs main, merged, last commit date, worktree link, merge history | worktree branch+head only | new `Branch`, `MergeEvent` in `hm-core`; Snapshot schema bump to 2 |
| Running services per branch | derivable: `Service.worktree` + worktree branch | join in view |
| LLM cache | absent | `hm-store` table, key = facts hash |

Note: Snapshot is a public contract (`SNAPSHOT_SCHEMA = 1`); additions must be new optional sections or a schema bump. Branch and deploy data change slowly, so keep them out of the 2 s sampling loop (V1 §6.1): separate cached facts with their own freshness stamp.

## 4. Spec sections to amend

- PD §1/§5.2/§10: LLM exception for optional, user-configured, off-by-default calls (D).
- PD §3: first screen contract (A); level model vs single-template UI.
- PD §6: add "Going live" detectors, Branches, Insight (patterns) to milestones; move/redefine "optional LLM".
- PD §7: UI stack (React/Vite) vs single template; crates (`hm-git` or branch module in `hm-project`); `hm-index` timing.
- PD §8 (Visual language), Lanes table, "Colour has two jobs", Themes (G, C).
- PD §2 Problem and §4 Positioning: add branch/deploy lanes of value.
- V1 §2 milestone lists: P4 scope (Branches screen, Going live section), alpha.2 or P5 for file-based project facts.
- V1 §3 data model: `Branch`, `MergeEvent`, `DeployTarget`, `Pipeline`, `CloudResource`; `Snapshot` schema bump.
- V1 §5.4 CLI: add `harbormap branches`; V1 §5.2 methods: `branches.list`; V1 §12.2 MCP (optional branch tool).
- V1 §9.7 / §12.4: tie WT rules to branch map.
- V1 §13.1 Screens, §13.2 Themes, §13.3 Lanes (fifth/sixth lane: build and ship, cloud), §13.4 story chapters (add "How it goes live").
- V2 §2.2 (CI beyond names, deploy, cloud, DB schema, patterns), §2.6 tables, §3.6 roles/lanes, §4 (default edge visibility rule matches draft), §6-7 (light rendering path; Code and Branches tabs), §8 colour/theme, §9 story chapters, §10.8 LLM scope, §14 risks.
- V2 §10.1 "Change" tour: reuse "what went into main" timeline.
- v0.5 list in PD §6: align "fit" verdict with patterns.
- RS: no change needed, except a note on UA complexity numbers feeding requirement B.
