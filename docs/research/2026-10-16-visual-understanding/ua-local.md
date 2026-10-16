# Understand-Anything (UA) v2.9.7, local findings

Source: `~/.claude/plugins/cache/understand-anything/understand-anything/2.9.7/` (agents/, skills/understand/, packages/{core,dashboard}). Data: `a local `.ua/` output folder`. Not measured: wall-clock/token cost per step (no logs on disk). Where I give cost, it is inferred from the design.

## 1. Pipeline (skills/understand/SKILL.md, 7 phases)
| Phase | Kind | Output |
|---|---|---|
| 0 Preflight | code | full vs incremental, via git commit hash in `meta.json` |
| 1 Scan (`scan-project.mjs`, `extract-import-map.mjs`) | code, plus a project-scanner LLM agent (SKILL notes ~157k tokens if Phase 1 must be re-dispatched) | `intermediate/scan-result.json` (491 KB): files, languages, frameworks, importMap |
| 1.5 Batch (`compute-batches.mjs`) | code, tree-sitter | semantic batches + neighbor map |
| 2 Analyze | LLM `file-analyzer` agents, up to 5 concurrent. Each first runs the bundled tree-sitter `extract-structure.mjs` (functions, classes, imports, exports), then the LLM writes summary, tags, complexity, languageNotes and semantic edges | per-batch graphs, merged by `merge-batch-graphs.py` |
| 3 Assemble review | LLM (assemble-reviewer), full analysis only | issue list (`flow-notes/assemble-review.json`) |
| 4 Architecture | LLM (architecture-analyzer, 22 KB prompt plus per-language and per-framework addenda) | `layers` |
| 5 Tour | LLM (tour-builder) after a graph-metrics script | 5-15 steps |
| 6 Review | default: inline deterministic validator; `--review`: LLM graph-reviewer | pass/fail |
| 7 Save | code: `build-fingerprints.mjs`, `meta.json` | `fingerprints.json`, `meta.json` |

- **Domain graph** (`/understand-domain`, domain-analyzer agent, LLM): separate `domain-graph.json`, with domain, flow and step nodes.
- **Incremental updates**: `prepare-incremental.mjs`, `finalize-incremental.mjs`, `change-classifier.ts`.
  - `fingerprints.json` (2.44 MB) holds per-file contentHash, functions, classes, imports, exports and totalLines.
  - Outcomes are SKIP (zero LLM tokens), PARTIAL_UPDATE, ARCHITECTURE_UPDATE, FULL_UPDATE.
  - Local use: 620 files re-read for 10.10 to 15.10 (rehber/PANEL-REHBERI.md).
- **Cost, evidence on this machine**:
  - the sample project is 1,414 files. `flow-notes/` has 19 flow-trace notes (about 14-18 KB each), and the rehber says each was checked by a second verifier agent.
  - The graph is dominated by LLM text: 4,104 summaries, one per node.
  - The user's own `tools/` (finalize-kg, fix-enrichment, hybrid-existing, merge-domain) show hand-patching of agent output.
  - `flow-notes/review.json` has `approved:false`: tags not kebab-case, orphan nodes, documents missing `documents` edges. LLM output needs repair passes.
- **Summary language**: everything is written in Turkish (`config.json` `outputLanguage: "tr"`).

## 2. Data model
`knowledge-graph.json` is 6.54 MB, with keys version, project, nodes, edges, layers, tour. `domain-graph.json` is 410 KB.

**Nodes: 4,104**
- function 2,582
- file 1,270
- table 59
- config 56
- document 53
- concept 50
- service 25
- class 9

Schema allows 13 types. Unused here: module, endpoint, pipeline, schema, resource.

**Node fields** (all nodes):
- id, type, name, summary, tags[], complexity (simple/moderate/complex; here 2,459 / 1,157 / 488)
- Optional: filePath, lineRange (2,627 nodes), languageNotes (303 nodes)
- ID convention: `file:<path>`, `function:<path>:<name>`.

**Edges: 17,957**
| Type | Count |
|---|---|
| imports | 5,682 |
| calls | 4,225 |
| contains | 2,688 |
| exports | 2,131 |
| tested_by | 864 |
| implements | 534 |
| related | 481 |
| depends_on | 358 |
| documents | 250 |
| reads_from | 166 |
| writes_to | 162 |
| configures | 141 |
| middleware | 83 |
| validates | 79 |
| migrates | 76 |
| deploys | 21 |
| routes | 12 |

Rare types (1 each): inherits, triggers, serves, defines_schema. The schema defines 26 edge types.
- Edge fields: source, target, type, direction, weight (0.5-1.0 by type).
- 12 `routes` edges vs 56 `route.ts` files on disk, so route coverage is thin.

**Layers: 10**. These cover only 1,463 of the 4,104 nodes (files only). Largest are Test and Dev Tools at 462, Presentation at 310, Service and Data at 137 each.

**Tour: 15 steps**, with title, description and nodeIds, as long prose.

**Domain graph: 382 nodes, 427 edges**
- 8 domains, 46 flows, 328 steps.
- Edges: flow_step 328, cross_domain 53, contains_flow 46.

**Table nodes are migration files** (`table:drizzle/0023_video-interviews.sql`), not tables or columns.

## 3. Dashboard (packages/dashboard, about 9.6k lines TSX/TS)
- **Libraries**:
  - @xyflow/react 12 (React Flow), elkjs 0.9 (bundled elk chunk 1.44 MB), @dagrejs/dagre 2
  - d3-force, graphology + Louvain, zustand 5, React 19, Tailwind 4, react-markdown, prism-react-renderer
- **Views**:
  - Structural overview of layer clusters; click a layer to open layer detail.
  - Domain view (DomainGraphView, ELK LR).
  - Knowledge view (KnowledgeGraphView, force layout in a Web Worker, Louvain communities).
  - Detail toggle: file vs class vs function.
- **Features**:
  - Guided tour (LearnPanel)
  - Search (SearchBar; core has fuzzy `search.ts` and `embedding-search.ts`)
  - FilterPanel; PathFinderModal (find path between nodes); persona selector (non-technical / junior / experienced, which filters node types)
  - Diff toggle (`/understand-diff` overlay); ExportMenu; ThemePicker; CodeViewer (Prism); StalenessBanner
  - Breadcrumb, FileExplorer, mobile layout, i18n locales
  - Token gate (access token required)
- **Chat**: `/understand-chat` is a CLI skill, not a dashboard panel. The dashboard has no chat component.
- **Layout**:
  - Overview and layer detail use two-stage ELK, called async in the main thread (`GraphView.tsx:334, 760, 843`). Cluster atoms are laid out first, then children are laid out per container on expand.
  - Results are cached per container (`containerLayoutCache`), in memory only, so every page load recomputes.
  - Only the force layout (and a dagre worker) runs in a worker.
- **Clutter control (it does exist)**:
  - `aggregateLayerEdges` collapses cross-layer edges into one weighted edge per layer pair.
  - `containers.ts` auto-groups by folder or Louvain community; max container 800x600.
  - Portal nodes stand in for other layers.
  - Fixed node size 280x120.
- **Where it still gets slow or cluttered**:
  - Layer detail of Test/Dev (462 files) or Presentation (310) draws raw file edges inside the layer.
  - Function-level view has 2,582 function nodes and 4,225 `calls` edges, all drawn at once.
  - The ELK bundle is 1.4 MB and runs on the main thread.
  - Local probe (a local probe file): only 10 nodes rendered, first node 1.0 s, total 4.1 s. That is the layer overview, which is the aggregated view.
  - The rehber says Safari showed 61% CPU, "use Chrome" (PANEL-REHBERI.md:39). It also blames iCloud and swap for general slowness.

## 4. Clicking a file (NodeInfo.tsx, 560 lines)
Fields shown:
1. Name, type badge, complexity badge.
2. `summary` (free LLM prose).
3. filePath plus `L<start>-<end>`.
4. `languageNotes` (language-specific note, only 303 nodes have one).
5. Tags (chips).
6. Child nodes (functions/classes) each with complexity and summary.
7. Connections list, grouped by edge type and direction.
8. Source code via CodeViewer.

Example, `scripts/seed.ts`: complex; tags entry-point, seed, cli-script, orchestration; a one-paragraph summary plus languageNotes.

**Insight depth**:
- Tags like `singleton`, `factory`, `barrel`, `middleware` come from a fixed vocabulary in file-analyzer.md:209, so patterns are only named, not explained.
- There is no "why this design", no "what this structure suits", no metrics (fan-in/out, churn, size, coupling), no method explanation beyond the summary text.
- The `concept:` nodes (50) and `tour` hold the deeper explanation, but they are LLM prose.

## 5. Worth adopting vs weaknesses
**Adopt**
- Aggregated layer overview with one weighted edge per layer pair, and portal nodes (`edgeAggregation.ts`).
- Progressive disclosure: overview, layer, container, file, function.
- Automatic folder or Louvain containers; cap on container size.
- Guided tour with a step-by-step narrative (15 steps).
- Domain, flow and step view that gives a business-level map (8 domains, 46 flows).
- Personas that filter node types; PathFinder; diff overlay (`/understand-diff`); staleness banner.
- Content-hash fingerprints for incremental updates, with a zero-token SKIP path.
- Deterministic structure first (tree-sitter), LLM only for semantics.
- Per-node `complexity` and `languageNotes`.
- Export, themes, keyboard shortcuts.
- Onboarding guide generation (`onboarding.md`, 96 KB).

**Weaknesses (evidence)**
- Everything semantic depends on LLM runs, and the output needs fixing: `review.json` approved:false, local fix scripts in the user's local tools folder, and 19 plus verifier agents for flows.
- Not live: the graph is stale until someone reruns it (rehber: "otomatik güncellenmez"). `meta.json` is pinned to a commit.
- Heavy payload: 6.5 MB graph plus 2.4 MB fingerprints; the dashboard needs a token and its own server.
- Layout is recomputed on every load on the main thread, with no persisted layout cache.
- Layers cover only 1,463 of 4,104 nodes; 2,582 function nodes sit outside any layer.
- Shallow insight per file: prose summaries and tags, with no quantitative metrics and no explanation of patterns.
- Tests are the biggest layer (462), which adds noise.
- Table nodes are migrations, not schemas (no columns or relations).

## 6. Runtime knowledge
Static only; UA never looks at the running system.
- `service` nodes (25) are parsed from Dockerfile and compose files: stage names (`service:Dockerfile:builder`), compose services (`deploy/compose.yml:caddy|app|launch|db|backup`). They carry summary and tags only.
- `deploys` edges: 21. `configures`: 141. CI shows up as `config:azure-pipelines.yml` (tags ci-cd). There are zero `pipeline`, `endpoint` or `resource` nodes, even though the schema allows them.
- `.env` examples appear as config nodes with prose, with no structured port, env var or secret fields.
- The 16 node summaries mentioning "port" are accidental prose, not structured data.
- No running processes, no live ports (5174 only appears in the user's serve script), no DB connection or live schema, no container state, no health.
- Presentation: these are ordinary nodes in the "Altyapı ve Yapılandırma" layer (44 nodes) with the same card UI. There is no topology diagram of services, DBs and ingress.
- This is Harbormap's clearest gap to fill: runtime topology, ports, DBs and deploy flow as first-class structured entities.
