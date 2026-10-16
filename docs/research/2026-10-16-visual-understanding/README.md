# Visual understanding: research summary (2026-10-16)

Question: how should Harbormap show a project (services, databases, Docker, cloud, how it
goes live, code, branches) so it is clear at first sight, stays fast on big repositories, and
gives deeper insight than Understand-Anything (UA), with an optional LLM on top?

Five reports, one per question:

| Report | Question |
|---|---|
| [ua-local.md](ua-local.md) | How UA (v2.9.7) builds and draws its graph, measured on a local 1.4k-file analysis |
| [graph-techniques.md](graph-techniques.md) | How readable tools avoid the hairball, and how to lay out fast |
| [infra-detection.md](infra-detection.md) | Which repository files reveal services, data, Docker, cloud and deployment |
| [llm-explanations.md](llm-explanations.md) | Deterministic facts first, LLM prose second: trust, cache, cost |
| [spec-gaps.md](spec-gaps.md) | What the existing specs already decide, what is missing, what conflicts |

The UI draft these reports were checked against: `docs/design/2026-10-16-panel-draft/index.html`
(open it in a browser; example data).

## Findings

1. **UA's hairball is a data-model problem, not a speed problem.** On the local analysis UA holds
   4,104 nodes and 17,957 edges; it re-runs ELK layout on the main thread at every page load, and
   detail views can draw every raw edge (4,225 `calls` edges). Its layers cover 1,463 of the 4,104
   nodes. Every readable tool (Nx, dependency-cruiser, NDepend, C4) aggregates first: folders or
   modules with counted edges, and file edges only for the selection.
2. **What UA does well and Harbormap should keep:** an aggregated layer overview with drill-down,
   guided tours, a domain/flow view, personas, a path finder, a diff overlay, hash-based
   incremental updates, and tree-sitter before any LLM.
3. **What UA lacks:** anything live. Compose files become 25 plain service cards; there are no
   ports, environment links, database state, deploy targets or CI steps. A file card shows a
   summary, tags, complexity and connections, but not what methods it uses or whether its
   structure suits its job. Its LLM output needs manual repair.
4. **Layout:** precompute per container in Rust, cache by content hash, re-lay out only changed
   boxes and pin the rest. At ≤150 visible items SVG is enough; WebGL is not needed. A dependency
   matrix (DSM) is the better second view for cycles and layering.
5. **Choosing the ≤150 items:** rank by entry points, PageRank on the import graph (as aider's
   repo-map does) and git churn × complexity hotspots (as CodeScene does); demote hubs like
   `utils` and loggers.
6. **Going live, cloud and data can be read from files with no network.** About 35 file patterns
   are graded A (declared: compose, render.yaml, fly.toml, wrangler.toml, Terraform resource
   types, k8s kinds, Prisma schema, Procfile, serverless/SAM), B (conventional) or C (hints:
   `.env` names, SDK dependencies). Secrets: names only, never values.
7. **One screen, clear states.** Railway/Render-style cards: one shape per thing, a type icon, one
   status, few edges (only real references). Three states that survive grayscale: running locally
   (solid), configured but not running (dashed, with the start command), declared for the cloud
   only (dotted, "declared"; Harbormap never claims something is live in the cloud).
8. **LLM: facts in, prose out.** Every sentence carries the fact ids it rests on; sentences that
   fail validation are dropped or replaced by a deterministic template; the LLM never adds an edge.
   Cache key: content hash + facts hash + prompt version + model + level, rolled up Merkle-style
   for folders and services. Estimated cost for card prose on 1k files with a cheap model: about
   $0.6–1.2 (an estimate, not a measurement). Default: explain on click, with a cost preview and a
   user-set cap; local models for single-file prose.
9. **Patterns without an LLM:** decorators and attributes, manifest dependencies, the import and
   layer graph, naming and path conventions, and library calls (retry, queues, DI) cover most of
   "methods it uses". No published accuracy exists, so Harbormap must measure on labelled repos.

## Conflicts with the current specs

- The red/black single theme overrides the role palette and the "light and dark from v0.1" rule,
  and red cannot mean both "brand" and "problem" (product §8).
- Any hosted LLM call breaks "no network requests" (product §5.1, §10) unless an explicit,
  off-by-default, user-configured exception is added.
- The draft is map-first; product §3 says story first.
- There is no place yet for deploy/CI/cloud, branches, patterns or a structure-fit observation.

The full list of sections to amend is in [spec-gaps.md §4](spec-gaps.md).
