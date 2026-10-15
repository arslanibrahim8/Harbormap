# Harbormap — Product Design

- **Status:** approved design, 2026-10-15
- **Owner:** Ibrahim ARSLAN
- **Former working title:** CodeOrbit. It was renamed because `codeorbit` is taken on npm by a project in the same niche.
- **Companion specs:**
  - [v0.1 Runtime Doctor](2026-10-15-harbormap-v0.1-runtime-doctor-design.md)
  - [v0.2 Harbor Chart](2026-10-15-harbormap-v0.2-harbor-chart-design.md)
  - [Research summary](../../research/2026-10-15-research-summary.md)
- **Visual mockup** (private Design canvas, ask the owner for access): https://claude.ai/artifact/4uH5KC9WCYASMJWevxzsnB

## 1. Summary

Harbormap is a local, open-source tool. It shows what is running for your project, explains in plain language why something is broken, and keeps you and your AI coding agents from breaking it. Later releases draw the project's code on the same map ("Harbor Chart") and walk people through it step by step.

> **Tagline:** Know what's running, why it's broken, and keep your AI agents from breaking it.

It ships as **one native binary** with three faces:
- a CLI;
- a dashboard served on `127.0.0.1` and opened in the user's own browser;
- an MCP server for AI coding agents.

It needs no account, makes no network requests and spends no LLM tokens by default.

## 2. Problem

1. **Comprehension debt.** Projects grow through AI-assisted additions until nobody understands the structure, and people only learn their own code by asking an AI. Existing explain-the-repo tools run LLM pipelines over the whole repository and burn tokens. Understand-Anything produced 3,085 nodes and 13,301 edges for one of the founder's ~915-file projects, which is above the size where its dashboard is documented to freeze.
2. **Runtime chaos.** People lose track of local runtime state:
   - forgotten dev servers;
   - ports that are busy for no visible reason;
   - Docker setups nobody can explain;
   - containers stuck in restart loops;
   - "it can't connect" errors.

   AI agents make it worse: they start servers and leave them running, and they `pkill` the wrong thing. Users keep asking agents to "start the project" and "free the port".
3. **Beginners are unprotected.** Non-technical builders leak keys, cannot roll back what an agent broke, and cannot read error messages.

Evidence, with numbers and sources, is in the [research summary](../../research/2026-10-15-research-summary.md).

## 3. Users and levels

Every view exists at three levels. The user picks one with a goal-named lens, and can change it per panel. The system may *suggest* a level and never switches silently.

| Level | Lens name | Who | What they get |
|---|---|---|---|
| Beginner | "Just run my project" | Non-technical builders, "vibe coders" | A plain-language **story first**, then a small map (≤12 boxes), sentences instead of codes, and one obvious action |
| Builder | "Understand it" | Intermediate developers | Modules, counts, the evidence behind each link, env var names, logs |
| Expert | "Everything" | Experienced developers | Symbols, provenance on every link, paths, a dependency matrix, raw data, CLI and JSON |

The beginner view always opens with a narrative panel ("The story of this project"), because many people cannot read a diagram at first sight. The map illustrates the story; the story does not depend on the map.

## 4. Positioning

Harbormap does not try to be the best port lister or code-graph engine; both spaces are crowded. It owns the combination nobody offers:

1. **Diagnose and explain, not just list.**
   - A connection doctor.
   - A staleness detector ("running, but not your latest code").
   - Crash reasons, phantom ports.
   - Error messages tied to the live state.
2. **Code and runtime on one map.** Port → process or container → compose service → code folder → routes.
3. **Protection.**
   - A safe action catalogue with plans and approvals.
   - Agent-aware runtime: badges, orphans, hooks, worktree guard.
   - Later: save points and a leak check.
4. **Three real levels plus a story**, generated deterministically from facts at zero token cost.

| Area | Strongest today | What it lacks (our lane) |
|---|---|---|
| Ports and processes | `raskrebs/sonar` (MIT, Go) | Explanations, connection doctor, beginner language, agent stop policy |
| Docker | Docker Desktop, lazydocker | Link to the project's code, plain-language failure reasons |
| Code graphs for agents | codebase-memory-mcp, GitNexus | Runtime, beginner layer |
| Visual explanation | Understand-Anything | Token cost; freezes on large graphs; no runtime |
| Beginner safety | Lovable / Replit / Bolt | Only inside their own clouds |

Harbormap interoperates rather than replaces:
- It reads `sonar.yaml`, compose files, Procfiles, `package.json` and `launchSettings.json`.
- It imports and exports CodeTour `.tours`.
- It writes AGENTS.md only on request.

## 5. Principles (non-negotiable)

1. **Local only.**
   - No telemetry, no update checks, no network calls.
   - The only outside connection is to the local Docker engine.
2. **Zero tokens by default.** Every feature works without an LLM.
   - A later optional LLM layer is lazy, cached and cost-previewed.
   - It may rephrase facts. It never decides what is flagged, linked or done.
3. **Facts are deterministic.** The same input gives the same output, and golden tests pin it.
4. **Plan before acting.** Every state-changing action is shown as a plan first.
   - Harbormap never touches what does not belong to the user's project.
   - Destructive actions need a human, even when an agent asks.
5. **Evidence with every claim.** Each finding says how we know ("parsed", "observed", "config", "inferred").
   - There are no absolute verdicts such as "Secure ✓".
   - Uncertainty is said out loud ("probably").
6. **Stable visuals.** The map never reshuffles silently.
   - Expanding reveals content in place.
   - Code changes appear as overlays.
7. **Interoperate.** Prefer standard files and formats over our own.

## 6. Roadmap

| Release | Name | Audience | Content | Distribution |
|---|---|---|---|---|
| **v0.1** | Runtime Doctor | Developers first | See, diagnose, start and stop what runs. Small runtime Harbor map plus story panel. MCP tools and hooks for agents. | Unsigned binaries |
| **v0.2** | Harbor Chart | Developers | Code map (JS/TS, Python, Go, C#, Java), story panel for code, guided tours, code↔runtime bridge | Unsigned |
| **v0.3** | Safety Net | Beginners | Save points, leak and exposure check, "what changed since my save point", missing-pieces checklist | **Signed installers** (macOS notarization; Windows via SignPath Foundation) |
| **v0.4** | Agent Context | AI-agent users | Budgeted MCP tools over the code graph, ranked repo map, task context packs, version-truth block, agent read ledger, index shared across worktrees | Signed |
| **v0.5** | Health & Conventions | Teams | Cycles, god files, churn×complexity hotspots, "new issues only" baseline, duplicate-intent detection, inferred conventions → confirmed AGENTS.md block, AGENTS.md fact-checker, change-risk badge | Signed |

**v0.1 milestones** (details in the v0.1 spec):
1. `alpha.1` See
2. `alpha.2` Diagnose
3. `alpha.3` Start
4. `alpha.4` Recipes
5. `beta` Agents
6. `0.1.0`, after five real users complete the first-five-minutes tasks.

**Later, not scheduled:**
- optional LLM explanations, cached and cost-previewed;
- journeys for more frameworks;
- a live route-hit dev proxy;
- a desktop app (Tauri, with the core as a library);
- a VS Code extension;
- an MCP App runtime card;
- cost watch;
- dependency health;
- a post-session comprehension check.

**Deliberately cut:**

| Cut | Reason |
|---|---|
| Aggregating external linters into SARIF | No demand found; runs repository code; version hell |
| Standalone token dashboard | Covered by ccusage and codeburn; replaced by the agent read ledger in v0.4 |
| Course mode with learner memory | A separate product |
| Named `.localhost` URLs | portless does it; costly on three OSes |
| Our own dead-code and duplication detectors | knip, vulture and jscpd exist |
| Restarting processes Harbormap did not start | Their terminal and environment are lost; "copy start command" instead |
| Team-shared explanation cache, public benchmark, 3D views | Deferred or dropped |

## 7. Architecture overview

```
 Browser (dashboard) ── HTTP + WebSocket, 127.0.0.1 only, session cookie ──┐
 CLI ─────────────────── user-only local socket / named pipe ──────────────┤
 AI agent ── MCP over stdio ── `harbormap mcp` (thin client) ──────────────┤
 Agent hook ── `harbormap hook <agent> <event>` (300 ms, fail-open) ───────┤
                                                                           ▼
        ┌──────────── harbormap daemon (one per OS user, starts on demand) ────────────┐
        │ probe → resolve → rules → findings          actions (plan/approve/execute)   │
        │   ▲ OS adapters, Docker engines, events     supervisor (processes we start)  │
        │   └──── store: SQLite (snapshots, events, audit log, layouts, index) ────────┘
```

**One Rust binary, `harbormap`.**
- Subcommands open the dashboard, print status, explain a port, stop, start, run the MCP server and handle hooks.
- The daemon starts on demand. It supervises the processes Harbormap starts, keeps event history and agent sessions, and answers hooks in under 50 ms.
- It exits after an idle period when it supervises nothing.

**UI.** React + TypeScript + Vite, embedded in the binary, with no CDN.
- v0.1 renders the small runtime map as SVG.
- v0.2 adds a WebGL renderer fed by layouts precomputed in Rust (see the v0.2 spec).

**Storage**, in platform directories via the `directories` crate:
- SQLite database, logs and layout cache in the data directory;
- `config.toml` in the config directory;
- the IPC socket in the runtime directory.

The optional project file `harbormap.toml` is written only on request.

**Workspace crates:**

| Crate | Responsibility |
|---|---|
| `hm-core` | Domain model, policy types |
| `hm-probe` | OS adapters, Docker engines |
| `hm-project` | Projects, worktrees, expectations, framework detection |
| `hm-doctor` | Rules and pack loading |
| `hm-actions` | Planner, policy, executor, supervisor, recipes |
| `hm-layout` | Map layout |
| `hm-store` | SQLite |
| `hm-daemon` | Orchestration, HTTP/WebSocket server, IPC |
| `hm-mcp` | MCP server |
| `hm-hooks` | Agent hooks |
| `harbormap` | CLI binary |

`hm-index` (code indexing) arrives in v0.2.

**Key dependencies.** All licences are AGPL-compatible:

| Purpose | Crates / packages |
|---|---|
| Async, HTTP, CLI | tokio, axum, clap |
| Storage, git | rusqlite, gix |
| Processes and ports | sysinfo, listeners, plus our own OS adapters |
| Docker | bollard |
| MCP | rmcp |
| Graph algorithms | petgraph (v0.2) |
| Parsing | tree-sitter and grammars (v0.2) |
| Embedding the UI | rust-embed |
| File watching | notify |
| UI | React, TanStack Query; v0.2: deck.gl or Sigma 4 (decided by benchmark), React Flow for focused views, elkjs ≥0.12 under its GPL option for rare in-browser layouts |

## 8. Visual language: Harbor Chart

**Shapes**
- **Boxes are things that run:** a process, a container, a compose service.
- **Ports are tabs on the box edge,** labelled `:3000`:
  - filled means listening;
  - hollow with a dashed outline means set up but closed;
  - red means taken by something else.
- **Code sits inside the box that runs it** (v0.2).
- **Shared code** sits in a neutral lane.

**Lines**
- **Solid:** code uses code. The label is a count when aggregated.
- **Dashed:** network. The label says what connects it.
  - Beginners see a plain verb ("asks", "saves", "remembers").
  - Builders and experts see the env var or route (`DATABASE_URL`, `POST /api/login`).
- **Provenance:** inferred links are dashed amber; observed links are teal.

**Lanes, read left to right**

| Lane | Role colour |
|---|---|
| People | Neutral |
| What people see | Blue |
| What does the work | Violet |
| Where things are kept | Amber |
| Outside services | Pink |

Shared code is slate. A backward arrow is a cycle and is drawn red.

**Colour has two jobs, never mixed**
1. **Role:** blue, violet, amber, pink, slate, neutral. Each lane is also labelled in words and listed in the legend, so colour is never the only cue.
2. **State:** running, problem, stopped, changed. State is always paired with words and an icon.

The tour focus is a "highlighter" yellow (light theme) or a muted gold (dark theme).

**Themes.** Light and dark from v0.1.
- Default to the OS preference, with a manual toggle.
- Tokens are defined once per theme (the mockup's palette is the starting point).
- Both themes meet WCAG 2.2 AA contrast for text, at 4.5:1 below 24 px.
- Port tab text is dark on the light role colours in the dark theme.

**Readability budgets.** A map never shows the whole graph.

| Level | Budget per screen |
|---|---|
| Beginner | ≤12 boxes |
| Builder | ≤25 boxes and ≤40 links |
| Expert | ≤50 boxes; above that, suggest the matrix |

- Hard cap of ~150 visible elements. The rest collapse into "+N more" chips.
- Problems survive any aggregation as badges.
- Links are hidden by default except problems, upward or cycle links, and the selected box's links.
- The UI suggests a different view and never switches on its own.

**Motion**
- One camera move (~1 s, eased), then a reveal.
- Input skips the animation. Reduced-motion settings replace it with a crossfade.
- No infinite animations.

**Accessibility**
- Every map has a table equivalent.
- Full keyboard navigation and real buttons and links.
- Touch targets ≥44 px. Screen-reader labels on icon-only controls.

## 9. Safety model for actions

- **Ownership.** Harbormap acts only on processes owned by the current OS user whose working directory is inside the opened project, and on containers whose compose `working_dir` is inside the project. Everything else is read-only. A one-click "Open that project in Harbormap" switches scope.
- **Never touched:**
  - PID 1, system services, other users' processes;
  - Docker engines and VM helpers (`com.docker.backend`, vpnkit, limactl, wslrelay, pasta);
  - IDEs, terminals, shells, agent CLIs;
  - Harbormap and its parents;
  - Windows PID 4.
- **Data at risk.** Databases and containers with volumes need an extra confirmation that names the data at risk. Harbormap never runs `down -v` and never deletes a volume.
- **Plan, approve, execute, audit.**
  - Every action is first a dry-run plan listing exact targets, method and risk.
  - Immediately before signalling, the target's identity is re-checked (PID + start time + executable), via a pidfd on Linux and a process handle on Windows.
  - Every action is written to an audit log the user can see.
- **Agents never approve themselves.** MCP `request_action` creates an approval request in the dashboard and CLI and waits up to 60 s. Hooks can *deny* risky agent commands but never run actions.
- **Workspace trust.** Running repository code (start, install) requires a one-time trust decision per project. The exact commands are shown first.

## 10. Security and privacy

**Threat model**
1. A malicious web page reaching the localhost dashboard (DNS rebinding, CSRF).
2. Other local users.
3. Malicious repository content (prompt injection aimed at agents, untrusted scripts).
4. A misbehaving AI agent.

**Mitigations**

| Area | Mitigation |
|---|---|
| Network binding | Bind to `127.0.0.1` on a random port; strict `Host` allowlist |
| Browser requests | `Origin` check on every state change and WebSocket upgrade; no CORS; state-changing requests also need an `X-Harbormap: 1` header |
| Dashboard session | The CLI opens a one-time launch URL, exchanged for an `HttpOnly; SameSite=Strict` session cookie |
| Local IPC | A user-only socket (mode 0600 in a user-only directory); on Windows a named pipe whose DACL allows only the current user |
| MCP | stdio only; tool outputs never contain env values or unmasked argv |
| Repository content | Rendered as escaped text, never as instructions to any model |
| Testing | CI tests for DNS rebinding, CSRF and Origin; fuzzing of IPC and HTTP parsers; human review of kill, HTTP and IPC code paths |

**Secrets rule**
- Harbormap never stores or shows environment variable *values*, with two narrow exceptions:
  - values of keys matching `PORT` / `*_PORT`, parsed as an integer;
  - URL-like keys (`*_URL`, `*_HOST`, `*_DSN`, `*_URI`), parsed in memory to scheme, host and port only. Credentials are dropped immediately.
- Command lines are redacted before storage: passwords, tokens, credentialed URLs, known key prefixes such as `sk_live_` and `ghp_`, and long high-entropy strings.

**Network.** No outbound network at all. There is no update check either; `harbormap self-update --check` is a manual, explicit command and arrives with signed installers.

## 11. Packs (community content)

Content that the community grows lives in `packs/`. It is licensed **CC0** so that text Harbormap writes into users' repositories (for example, AGENTS.md blocks) carries no obligations.

| Pack | Content |
|---|---|
| `signatures/*.toml` | How to recognise a program (argv, executable, image, package dependency), its role, default port, restart-trigger files and 3-level texts |
| `errors/*.toml` | Error dictionary: pattern, explanation per level, fix, links to live-state checks |
| `texts/*.toml` | 3-level texts for every rule, the story panel and tour templates |
| `agents.toml` | AI agent markers (env var *names*, executables) and the agent version each marker was verified against |
| `glossary.toml` | Beginner glossary entries |

**Rules for packs**
- Every pack entry must ship with a fixture test.
- CI validates packs against a JSON Schema.
- Unverified entries are marked and produce hedged wording.

## 12. Distribution and updates

- Built with `cargo-dist`. Released on GitHub Releases for macOS (arm64, x64), Linux (x64, arm64, static musl) and Windows (x64, arm64).
- Each release includes checksums, an SBOM and build attestations.
- **Channels:** shell and PowerShell install scripts, a Homebrew tap, `cargo install`, and an npm wrapper (`npx harbormap` downloads the prebuilt binary).
- **v0.1 and v0.2 are unsigned**, and the README says so.
- **Before v0.3,** add Apple Developer ID signing and notarization ($99/yr), and Windows signing through SignPath Foundation (free for OSS, requires no commercial dual licensing). Then winget and Scoop.
- There is no auto-update. Harbormap prints the right upgrade command for the channel it was installed from.

## 13. Licensing and governance

- **Code:** AGPL-3.0-only. **Packs and templates:** CC0-1.0.
- **Contributions:** DCO (`Signed-off-by`). No CLA; no commercial dual licensing.
- **Dependencies:** checked with `cargo-deny` and an npm licence check. No code is copied from projects under PolyForm-Noncommercial (GitNexus) or BSL (qlty). Shelling out to GPL tools is allowed.
- **Repository files:** `SECURITY.md` with private vulnerability reporting; `CONTRIBUTING.md` describing how to add a pack entry with its fixture; `CODE_OF_CONDUCT.md`.
- **Documentation** follows the Diátaxis structure. It includes a "first five minutes" guide, a support-tier matrix, a "What Harbormap never does" page and a privacy statement.

## 14. Quality strategy

**Test layers**
1. Unit tests and golden fixtures for rules, packs, templates and layouts.
2. OS adapter tests on GitHub Actions for macOS, Linux and Windows, using real spawned servers.
3. Docker integration tests on Linux, using real compose projects drawn from awesome-compose.
4. Recorded Docker Engine API fixtures for OrbStack, Colima and Podman.
5. Negative safety tests: PID reuse, protected processes, foreign projects.
6. Security tests.
7. Playwright UI tests.
8. Contract tests for agent hooks.

**Support tiers**

| Tier | Coverage |
|---|---|
| Tier 1 | macOS arm64, Linux x64, Windows x64 with Docker Desktop |
| Tier 2 | OrbStack, Colima, Podman, WSL2, Linux arm64, Windows arm64 |

The tiers are documented.

**Fixture repositories**
- small synthetic polyglot repos;
- awesome-compose samples;
- Online Boutique;
- RealWorld implementations;
- for v0.2 scale tests: fastapi, nest, django, kafka, spring-framework, kubernetes, roslyn, elasticsearch and kibana.

## 15. Success measures

| Release | Measure |
|---|---|
| v0.1 | Five real users (at least two non-developers) do the first-five-minutes tasks unaided: see what runs, open the app, free a busy port, understand one problem, start the project. |
| v0.1 | "Who holds port X and why?" is answered in under 5 s, through the CLI or MCP. |
| v0.1 | Zero wrong-process stops in the safety test suite; no secret values in any output fixture. |
| v0.2 | Performance budgets in the v0.2 spec hold on the benchmark repositories. |
| v0.2 | No tour step triggers a layout. |

## 16. Top risks

| Risk | Mitigation |
|---|---|
| Sonar keeps improving the runtime basics | Compete on diagnosis, explanation, agent policy and code↔runtime; stay compatible with `sonar.yaml` |
| Scope too large for a solo maintainer working with AI agents | Milestones with exit criteria; declarative packs instead of code for long tails; cut list enforced |
| Cross-platform variance | Support tiers; honest "owner unknown" states; recorded fixtures |
| Security of a local server that can stop processes | Section 10; security tests in CI; human review |
| Explanations that are wrong or condescending | Facts-only templates, evidence chips, user tests with five people per level |
| Signing costs and SmartScreen for beginners | Signing lands before the beginner-focused v0.3 |

## 17. Open questions

- Is the name `harbormap` available on Homebrew, winget and in trademark searches? Check before the first public release.
- Which Codex, Cursor and Gemini CLI hook payload versions do we support? Verify during the beta milestone; Claude Code's is verified.
- Do we import CodeTour `.tours` in v0.2 or later?
