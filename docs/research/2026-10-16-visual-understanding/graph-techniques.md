# Harbormap: readable, fast dependency maps (research)

Evidence tags: [V] = seen in a fetched search result (URL given); [K] = from my own knowledge, not verified this session. Vendor claims are marked as such. I found no independent benchmark for most performance questions, so numbers below are indicative and must be measured on Harbormap's own graphs.

## Core finding
Every tool that stays readable at scale does the same thing: it never draws the leaf graph first. It shows a small, aggregated, hierarchical view, and reveals file-level edges only for a selection. The 3,085-node / 13,301-edge hairball is the leaf graph drawn flat. The cure is data reduction (aggregate, rank, filter), not a faster layout engine.

## 1. Hierarchical aggregation / semantic zoom
- **C4 model / Structurizr**: four fixed zoom levels (Context, Container, Component, Code). Only one level is shown at a time, and Code is usually skipped. [V] https://itnext.io/software-architecture-diagrams-with-c4-model-898adcb534c2 , https://www.freecodecamp.org/news/how-to-create-software-architecture-diagrams-using-the-c4-model/ . Harbormap's "service boxes with port tabs" already is a Container-level view.
- **NDepend**: its own material says the matrix scales better and the graph is hard to read at size. It reworked graphs in 2020.1 with nested child graphs inside parent boxes sized by lines of code. It suggests switching to the matrix when the graph gets dense (vendor claims). https://blog.ndepend.com/visualize-code-with-software-architecture-diagrams , https://www.ndepend.com/features/dependency-graph-matrix-architecture , CppDepend says the same: https://www.cppdepend.com/features/dependency-matrix
- **Nx graph**: "group by folder" produces composite nodes that expand in place; `--focus <project>` plus a proximity slider gives an ego graph. [V] https://nx.dev/features/explore-graph
- **dependency-cruiser**: `archi` reporter aggregates to a folder level (default: just below `packages`/`src`/`lib`, adjustable with a collapse pattern); `ddot` summarises per folder. [V] https://raw.githubusercontent.com/sverweij/dependency-cruiser/HEAD/doc/faq.md , https://xebia.com/blog/taking-frontend-architecture-serious-with-dependency-cruiser/
- **Sourcetrail / CodeSee / Structure101 / Emerge / CodeScene**: Sourcetrail pairs a focused graph (one symbol and its neighbours) with a code view and search; it is a focus tool, not an overview tool. [V] https://isocpp.org/blog/2017/10/better-code-understanding-with-sourcetrail . CodeSee is commercial, with little verifiable detail (https://www.codesee.io/code-visibility). Structure101 (tangle/XS "fat" edges between modules) and Emerge (folder-level views) I could not verify this session [K].
- **AI-wiki tools (DeepWiki, Google Code Wiki, gitdiagram, GitNexus, Graphify, codebase-memory-mcp)**: the search returned nothing usable, so I cannot say what they show first. The pattern I would expect, unverified [K], is an LLM-written high-level diagram plus prose. Treat them as a UX reference only, and do not rely on this.

## 2. Edge clutter reduction
| Technique | Source | Fit for Harbormap |
|---|---|---|
| Hierarchical edge bundling: edges bend toward the path through the folder tree (up to the lowest common ancestor and down) | Holten 2006 [V] https://www.cg.tuwien.ac.at/courses/InfoVis/HallOfFame/2007/Grasberger/InfoVis2006%20Hierarchical%20Edge%20Bundles%20-%20Visualization%20of%20Adjacency%20Relations%20in%20Hierarchical%20Data%20%5BDanny%20Holten%5D.pdf ; d3 example https://observablehq.com/notebook-kit/ex/d3/hierarchical-edge-bundling | Good for a radial "module ring" overview. The d3 example shows edges on hover only. |
| Edge aggregation (A to B with count and weight) | Standard; dependency-cruiser `archi`, Nx composite nodes | Essential. Collapses 13k edges into the few hundred module-pair edges. |
| Focus + context (edges only for the selection or hover) | Sourcetrail, Nx focus, d3 bundle example | Essential. Drawing no edges at rest is the cheapest fix. |
| DSM matrix | NDepend / CppDepend (above) | Good as a second view for layering and cycles. N modules cost N² cells, with no layout step. |
| k-hop ego graph | Nx proximity, Sourcetrail | Good for the detail panel. |
| Hide hubs by degree (logger, utils, types) | Common practice [K] | Move them to a "shared" strip or badge instead of drawing edges. |
| Cycle highlight / layer violations only | NDepend DSM idea | Good as a filter toggle: "show only problems". |

## 3. Layout and rendering performance
- **ELK in JS**: the ELK paper itself says elkjs is slower than Java ELK on larger graphs. [V] https://arxiv.org/pdf/2311.00533 . One PostHog benchmark (synthetic, 800 nodes / ~950 edges, ELK 0.10.0): NETWORK_SIMPLEX placement ~3500 ms, BRANDES_KOEPF ~670 ms. [V] https://github.com/PostHog/posthog/pull/73408 . This matches Understand-Anything's reported lag. At 3,000 nodes / 13,000 edges, layered layout in JS is likely tens of seconds, extrapolated and not measured. Instagit pages claim worker use there, but they look auto-generated. https://instagit.com/Egonex-AI/Understand-Anything/what-graph-layout-algorithm-large-scale-visualization/
- **Rust layered crates**: `rust-sugiyama` (petgraph-based, 0.2.0 from April 2024, "unstable") [V] https://www.lib.rs/crates/rust-sugiyama ; `dagre` (a dagre.js port) https://docs.rs/crate/dagre/0.1.1 ; `bra0-layout` https://docs.rs/bra0-layout . None of them has published benchmarks that I could find. All are young, so expect to benchmark them and possibly patch them. On ≤150 visible items, any of them should take single-digit milliseconds in native code [K, to be measured]. `layout-rs` and Graphviz (via `dot`) are other options, but I did not verify them. A pragmatic alternative: hand-write a simple layered layout for the small aggregated graph. At ≤150 nodes, an explicit grid or treemap-style placement by folder is also deterministic.
- **Stability**: for a stable map, persist positions and layer assignments and only re-place the changed neighbourhood. Use deterministic ordering (sort by path hash), and pin existing nodes while new ones are slotted in. Layout the hierarchy level by level (each folder box lays out only its children), so a change dirties one box and not the whole map. [K]
- **Renderers**: sigma.js + graphology is a WebGL renderer that draws an already-laid-out graph; the "100k nodes at 30-60 fps" claim is marketing and unverified. [V] https://knogin.com/developers/sigma-large-graph-rendering , https://v4.sigmajs.org/ . Cosmos / Cosmograph runs force layout on the GPU, but it is CC-BY-NC-4.0, which is a licence problem for an open-source tool that others may use commercially, and force layout is unstable and not hierarchical. [V] https://github.com/supernotes/cosmos , https://cosmograph.app/docs-general/concept/ . For ≤150 items, plain SVG or canvas with CSS transforms is enough, and WebGL is unnecessary.

## 4. Incremental indexing
- **tree-sitter** for per-file symbol and import extraction: fast and error-tolerant, with a Rust-native binding [K].
- **Aider repo-map**: tree-sitter definitions/references, a graph over files, personalised PageRank, then render the top items within a token budget. [V, secondary] https://github.com/Cryect/RepoMapper , https://pyshine.com/aider/ ; official doc [K] https://aider.chat/docs/repomap.html . This is the closest prior art for "pick the important N items".
- **SCIP** (replaces LSIF, 10-20% of LSIF size for scip-clang) gives precise cross-file references but needs a per-language indexer (heavy). Incremental indexing was still a roadmap item. [V] https://sourcegraph.com/blog/announcing-scip , https://sourcegraph.com/blog/announcing-scip-clang . Use it as an optional upgrade, not the base.
- **stack-graphs**: I believe GitHub archived it in 2024 (unverified, check the repo). Avoid depending on it.
- **salsa** (rust-analyzer's incremental query framework) [K] https://github.com/salsa-rs/salsa . It is probably heavier than needed. A simple model is enough: key = file content hash (or mtime+size, then hash), store the per-file `{defs, imports}`, re-resolve only edges touching changed files, re-aggregate only affected folder pairs, then re-layout only dirty boxes.

## 5. Ranking "what matters" (the ≤150 selection)
- **CodeScene**: hotspot = complexity x change frequency (relative churn is an alternative), prioritised further by co-change, author count and coordination cost. Red/yellow tiers. [V] https://docs.enterprise.codescene.io/versions/4.1.4/guides/technical/hotspots.html
- **PageRank / in-degree centrality** (Aider) finds the shared core; **entry points** (main, routes, bin, exported API, service definitions, Dockerfile/compose) anchor the top of the map [K]; **git churn** gives live relevance and is cheap (`git log --numstat` once, then incremental from HEAD).
- Suggested score: `0.4*pagerank(norm) + 0.3*entrypoint_proximity + 0.2*recent_churn + 0.1*size`, with hubs (very high in-degree and low fan-out, e.g. utils) demoted for layout but kept in a "shared" strip. The weights are my guess and need tuning on real repos.

## Comparison table
| Approach | First view | Clutter control | Scales to 1k+ files | Layout cost | Verdict for Harbormap |
|---|---|---|---|---|---|
| Flat file graph (Understand-Anything style) | everything | none | no | seconds to tens of seconds in JS | reject |
| C4 levels | system, then container | one level at a time | yes | tiny | adopt as the model |
| Folder/module composite nodes (Nx, depcruise archi, NDepend nesting) | top folders with counts | aggregation + expand in place | yes | small | adopt as the core |
| Edge bundling (Holten) | radial ring | bundling, hover | medium (draw cost) | no layout, only a tree | optional overview mode |
| DSM matrix | grid | no edges at all | yes (N²) | none | add as a second view for cycles/layers |
| Ego graph / focus | selected node + k hops | selection only | yes | tiny | adopt for the detail panel |
| Force layout on GPU (Cosmograph) | blob | weak, unstable | yes | GPU | reject (licence, unstable) |
| Ranked top-N (Aider-style PageRank + churn) | the important N | filter | yes | tiny | adopt as the selector |

## Recommendation: what Harbormap should do, in order
1. **Fix the data model first: a containment tree plus aggregated edges.** Nodes: project, service, folder/module, file. Store folder-to-folder edges with `{count, kinds, cycle?}`, computed in Rust and updated incrementally. This single step removes most of the hairball.
2. **First screen = one level of the tree, capped at ≤150 items.** Services as boxes with port tabs (C4 Container), modules as children. Pick items with the ranking in step 5. Anything beyond the cap is folded into a "+N more" item that can be expanded, never silently dropped.
3. **Draw edges sparingly.** Show aggregated module edges with counts only, thickness by weight, and cap them (for example the top 3 per node plus all cycle edges). Show file-level edges only for the selected or hovered item, as an ego graph with k=1 (expand to 2 on demand). Demote hubs (high in-degree, such as logger/utils) to a "shared" badge or strip with no drawn edges.
4. **Precompute layout in Rust, per container.** Lay out each box's children separately (a layered layout via `rust-sugiyama`/`dagre`, or a plain grid for siblings without edges), and cache coordinates keyed by node id. On updates re-layout only dirty boxes and pin everything else, so the map does not jump. Benchmark the crates on 150-node and 1,000-node boxes before committing, because no published numbers exist. The browser gets finished coordinates and only draws with SVG/canvas, so no ELK in the browser and no worker is needed.
5. **Rank what matters.** Combine centrality (PageRank on the file or module graph, as in Aider) with entry-point proximity and git churn (hotspot idea from CodeScene). Expose "why is this shown" in the UI. Tune the weights on 3-5 real repos.
6. **Index incrementally with tree-sitter.** Keep a per-file fingerprint (size+mtime, then content hash), a per-file record `{symbols, imports}`, resolve imports to files, and re-aggregate only the affected folder pairs. Skip SCIP, stack-graphs and salsa for now. Treat SCIP as an optional precise-reference plug-in later.
7. **Add the analysis views that fit the aggregated data.** A DSM matrix of top-level modules for cycles and layering (cheap, no layout), a "problems only" filter (cycles, layer violations), and optionally a bundled radial overview.
8. **Only if a user-visible level exceeds the cap**, add viewport culling or WebGL (sigma.js). Do not start with it.

## Open risks
- The Rust layered crates are young and unbenchmarked; have a fallback (grid or treemap placement).
- Import resolution accuracy per language drives the quality of every edge; the tree-sitter approach is approximate for dynamic languages.
- Several tools in the brief (DeepWiki, Code Wiki, GitNexus, Graphify, Emerge, Structure101, CodeSee internals) were not verifiable through search this session.
