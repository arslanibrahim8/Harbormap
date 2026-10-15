# Harbormap — Research Summary

**Background research for the [product design](../superpowers/specs/2026-10-15-harbormap-product-design.md), carried out 2026-10-14 → 2026-10-15.**

**Method**
- Three rounds of parallel research agents, 13 in total:
  - Round 1: competitive landscape, token efficiency, runtime, health and conventions, UX and form factor.
  - Round 2: demand evidence for runtime and for code understanding, needs of non-technical users, a skeptic's feasibility review, and emerging ideas.
  - Round 3: readable graph design, an engine and algorithms teardown, tour design, and a teardown of existing code-map UIs.
- Key GitHub issue and repository claims were spot-checked against the GitHub API.
- Understand-Anything's behaviour was checked directly in its source at HEAD `f2d0164`.
- Reddit was unreachable (HTTP 403), so the evidence comes from GitHub, Stack Overflow, Hacker News, vendor docs, papers and surveys.
- Numbers are as of the dates above.

## 1. Landscape (who does what)

| Tool | Approach | Key fact | Lesson for Harbormap |
|---|---|---|---|
| [Understand-Anything](https://github.com/Egonex-AI/Understand-Anything) (MIT, ~86k★) | tree-sitter facts + 5-agent LLM pipeline + React Flow dashboard | Froze at 2,747 nodes ([#14](https://github.com/Egonex-AI/Understand-Anything/issues/14)); its "persona" only filters node types | Borrow: layer grouping, committed graph file. Avoid: main-thread layout, tours as node lists |
| [GitNexus](https://github.com/abhigyanpatwari/GitNexus) (PolyForm-NC, ~48k★) | Static graph + MCP + sigma web UI | Chat-only mode above 25k nodes ([#2185](https://github.com/abhigyanpatwari/GitNexus/issues/2185)); route_map misses ([#3487](https://github.com/abhigyanpatwari/GitNexus/issues/3487), [#3488](https://github.com/abhigyanpatwari/GitNexus/issues/3488)) | Ideas only (licence) |
| [codebase-memory-mcp](https://github.com/DeusData/codebase-memory-mcp) (MIT, ~47k★) | C binary, graph + MCP + browser UI | 3D view froze on 43,729 nodes ([#498](https://github.com/DeusData/codebase-memory-mcp/issues/498)); unbounded octree used 17.6 GB ([#917](https://github.com/DeusData/codebase-memory-mcp/issues/917)) | Bounded algorithms; 2D first |
| [raskrebs/sonar](https://github.com/raskrebs/sonar) (MIT, Go, ~1.1k★, since 2026-03) | Ports by git root, Docker/Compose, kill tree, `sonar.yaml`, daemon, desktop beta, MCP + hooks | Strongest runtime competitor; no explanations, no connection doctor, no MCP kill policy | Compete on diagnosis, explanation, agent policy; read `sonar.yaml` |
| codegraph (~74k★), Graphify (~126k★), Serena (~30k★), Aider repo-map | Code graph / LSP tools for agents | Commodity layer | Don't build "yet another graph MCP" first |
| lazydocker (53k★), Docker Desktop, Portainer, Dockge | Docker UIs | No link to code, no plain-language reasons | Explain, don't just show |
| portless (12.7k★), port-killer (5.1k★), kill-port | Ports | kill-port defaults to SIGKILL with no owner check | Safety model |

## 2. Demand evidence (selected)

**Runtime**

| Pain | Evidence |
|---|---|
| "What's on port X / kill it" | ~25M Stack Overflow views across 9 questions (e.g. [Mac 5.22M](https://stackoverflow.com/questions/3855127), [Windows 6.37M](https://stackoverflow.com/questions/48198)) |
| `localhost` inside vs outside containers | [2.93M views](https://stackoverflow.com/q/24319662); [docker/for-mac#155](https://github.com/docker/for-mac/issues/155), 191👍 |
| Local DB refuses connections | ~4.8M views across Postgres/Mongo/Redis questions |
| Docker/WSL disk and memory | [WSL#4699](https://github.com/microsoft/WSL/issues/4699), 1,190👍; WSL#4166, 436👍 |
| Windows reserved ports with no process to kill | ~1M views; [docker/for-win#3171](https://github.com/docker/for-win/issues/3171), 184👍 |
| Stale containers / rebuilds | "How to rebuild docker container", [1.34M views](https://stackoverflow.com/q/36884991) |
| Agents killing the wrong server or leaving servers | [claude-code#90070](https://github.com/anthropics/claude-code/issues/90070) (pkill killed a sibling project's server); Cursor forum "agents start a server but don't kill it" (5,146 views) |

**Code understanding and agents**

| Pain | Evidence |
|---|---|
| Agents burn quota rediscovering code | [claude-code#16157](https://github.com/anthropics/claude-code/issues/16157): 695👍, 1,499 comments |
| AGENTS.md support | [claude-code#6235](https://github.com/anthropics/claude-code/issues/6235): 5,186👍 |
| Comprehension debt | HN 532 pts; Anthropic RCT: AI-assisted learners scored 50% vs 67% |
| "Almost right" AI code | Stack Overflow 2025: 66% |
| Agents edit without reading | [claude-code#42796](https://github.com/anthropics/claude-code/issues/42796): reads per edit fell from 6.6 to 2.0 |
| Duplication up, reuse down | GitClear 2026 (vendor data): duplicated blocks +81% |
| Context files vs results | [arXiv 2602.11988](https://arxiv.org/abs/2602.11988): context files give no general success gain and +20% cost; explicit non-standard rules help |

**Non-technical builders**

| Pain | Evidence |
|---|---|
| Exposed secrets | [Escape](https://escape.tech/blog/methodology-how-we-discovered-vulnerabilities-apps-built-with-vibe-coding/): 5,600 vibe-coded apps → 400+ exposed secrets |
| AI-assisted commits leak more | [GitGuardian 2026](https://blog.gitguardian.com/the-state-of-secrets-sprawl-2026/): 3.2% vs 1.5% baseline |
| Rollback | [claude-code#353](https://github.com/anthropics/claude-code/issues/353): 178 reactions; Replit agent deleted a production DB (AIID 5578) |
| Getting a project running locally | `ERR_OSSL_EVP_UNSUPPORTED` 3.5M views; "'npm' is not recognized" 2.5M views |

## 3. Token efficiency

- **Per lookup**, graph tools save 10–100×. **Whole agent sessions** save between −6% and ~62%, and sometimes lose quality: codebase-memory-mcp's preprint reports 83% vs 92% answer quality ([arXiv 2603.27277](https://arxiv.org/abs/2603.27277)).
- **What makes an MCP tool cheap for the calling agent:**
  - few tools, since their definitions are paid on every turn;
  - precomputed one-call answers;
  - hard `max_tokens` budgets with cursors;
  - plain text rather than JSON (about 2× fewer tokens);
  - freshness stamps.

  Claude Code warns above 10k tokens per tool response.
- **Zero-LLM indexing** (tree-sitter → SQLite) is proven fast. Aider-style ranked repo maps fit 1–8k tokens.
- **Version-matched docs** in AGENTS.md raised Vercel's Next.js eval pass rate from 53% to 100%.

## 4. Runtime engineering facts

- On macOS without root, `lsof` hides other users' sockets; `netstat -anv` shows their PIDs. Docker Desktop's ports appear as owned by `com.docker.backend`. ControlCenter (AirPlay) holds ports 5000 and 7000.
- On Linux, with `userland-proxy: false`, published container ports have no host listener, so the Docker API is authoritative.
- On Windows:
  - PID 4 (http.sys) can own a port;
  - excluded port ranges cause error 10013 with no owning process;
  - there is no SIGTERM: `CTRL_BREAK` works only on a separate process group, and the JVM prints a thread dump instead of exiting;
  - unsupervised processes usually need `taskkill /T /F`.
- **Compose labels**
  - `project`, `service`, `project.working_dir`, `project.config_files`, `config-hash`.
  - `docker compose config --hash` matches the `config-hash` label, which reveals outdated containers.
- **Claude Code's child-process environment** (verified on 2026-10-15, v2.1.299) contains `AI_AGENT`, `CLAUDECODE`, `CLAUDE_CODE_SESSION_ID` and `CLAUDE_PID`. It also contains a secret-bearing variable, which is why Harbormap reads names only.

## 5. Readable graph design

- **Path finding:** accuracy drops below 50% above ~50 nodes at 6 edges per node (Yoghourdjian et al. 2021, [arXiv 2008.07944](https://arxiv.org/abs/2008.07944)).
- **Matrix vs node-link:** matrices win most tasks above ~20 nodes, except path finding (Ghoniem et al. 2004).
- **Working memory** holds about 4 chunks (Cowan 2001).
- **Architecture recovery:** no algorithm is accurate, and directory baselines beat them on well-organised systems (Lutellier et al. 2018). Louvain can produce badly connected communities; Leiden fixes this (Traag 2019).
- **What scales:** tools that stay usable at scale (Structure101, NDepend, Sourcetrail, Nx, dependency-cruiser) group by folders, hide most edges and answer specific questions in focused views.
- **Stable layouts** speed orientation (Archambault & Purchase). Spatial edge bundling creates false connections and slows path-following.
- **Code-map UIs** that recovered converged on three things:
  - named boxes with counted edges;
  - stable layouts stacked by dependency level;
  - focus on one question at a time.

  No tool joins code and runtime well.

## 6. Engine lessons (verified)

- **Understand-Anything:**
  - `utils/elk-layout.ts` imports `elkjs/lib/elk.bundled.js` with no `workerUrl`, so layout runs on the main thread.
  - `TourStep` is `{order, title, description, nodeIds, languageLesson?}`.
  - A tour step can clear caches, auto-expand containers and re-lay out a layer ([#114](https://github.com/Egonex-AI/Understand-Anything/issues/114) "tour navigation glitches", [#492](https://github.com/Egonex-AI/Understand-Anything/issues/492)).
  - The fallback tour generator writes "Key files: <every name>" and "Step N: Code Walkthrough".
- **All three graph UIs studied** (Understand-Anything, GitNexus, codebase-memory-mcp) share the same mistakes:
  - the whole graph goes to the browser;
  - layout runs when a view opens;
  - highlighting costs O(n+m);
  - expansion is unbounded;
  - cache keys can collide;
  - nothing guards against degenerate input.
- **Evidence for the chosen design:**
  - MSAGLJS's tile pyramid held p95 ≤18.4 ms up to 32k nodes / 237k edges; building the pyramid in the browser took 41–173 s.
  - Binary struct-of-arrays: 50k nodes + 150k edges ≈ 1.6 MB, vs 6.55 MB as JSON.
- **Licences checked:**

| Package | Version | Licence |
|---|---|---|
| petgraph | 0.8.3 | MIT/Apache |
| leiden-rs | 0.8.1 | MIT/Apache |
| rust-sugiyama | 0.4.0 | MIT |
| dugong | 0.9.0 | MIT/Apache |
| manatee | 0.9.0 | MIT/Apache |
| deck.gl | 9.4.0 | MIT |
| sigma | 4.0.2 | MIT |
| @xyflow/react | 12.12.0 | MIT |
| elkjs | 0.12.0 | EPL-2.0 OR GPL-3.0-or-later |

## 7. Tours and explanation

- **Comprehension:** top-down with beacons, bottom-up when needed (Brooks; von Mayrhauser & Vans). Newcomers need a starting point (Sillito et al.).
- **Narrative structure:** the martini glass (Segel & Heer 2010). Small transitions win; jumps between general and specific are least liked (Hullman et al. 2013).
- **Motion:** about 1 s, staged (Heer & Robertson 2007); smooth zoom (van Wijk & Nuij 2003).
- **Curated vs AI-only tours:** curated tours beat AI-only ones on quizzes, 83% vs 57% (LACY, FSE 2026, small study).
- **Anchors** belong on symbols, not lines: users called CodeTour's line anchors "unusable".

## 8. The founder's own Understand-Anything run

These are aggregate counts only, read from `.ua/knowledge-graph.json`:
- A ~915-code-file project produced **3,085 nodes and 13,301 edges** (4.9 MB of JSON).
  - Node types: functions 1,887, files 966, tables 55, docs 50, configs 48, concepts 47, services 25, classes 7.
  - Edge types: imports 3,884, calls 3,106, contains 2,019, exports 1,612, tested_by 582, and others.
- **10 layers**, including tests (262 nodes) and docs (40 nodes).
- **A 15-step tour** with 3–5 nodes per step.

The graph is above the documented freeze threshold, which matches the founder's experience of lag and crashes during tours.

## 9. Distribution and licensing facts

- **macOS signing:** Apple Developer ID costs $99 per year.
- **Windows signing:**
  - Microsoft Artifact Signing is open to individuals only in the US and Canada.
  - SignPath Foundation is free for open-source projects, with per-release approval and no commercial dual licensing.
  - EV certificates no longer bypass SmartScreen.
- **Homebrew 6.0** (June 2026) requires `brew trust` for third-party taps. homebrew/core requires notability.
- **AGPL network clause:** effectively irrelevant for a localhost-only dashboard. A source link with the commit hash is still provided.
- **Semgrep:** the Semgrep Rules License restricts its rules, so registry rules are not bundled.
- **Do not reuse code** from PolyForm-NC (GitNexus) or BSL (qlty) projects.

## 10. Not verified / caveats

- Reddit sentiment is missing (HTTP 403).
- Several star counts are large and unaudited.
- Vendor statistics (GitClear, Escape) are self-published.
- Hook formats for Codex, Cursor and Gemini CLI must be verified during the v0.1 beta.
- Restart-trigger rules per framework (for example, whether `next dev` reloads `.env`) must be verified with fixtures before they are marked verified.
- Every performance budget is a target, not a measurement.
