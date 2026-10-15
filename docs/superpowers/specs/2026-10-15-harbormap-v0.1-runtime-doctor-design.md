# Harbormap v0.1 — Runtime Doctor — Design

- **Status:** approved design, 2026-10-15
- **Parent:** [Product design](2026-10-15-harbormap-product-design.md)
- **Evidence:** [Research summary](../../research/2026-10-15-research-summary.md)

## 1. Goal

v0.1 shows everything that runs for a project, explains problems in plain language, and starts and stops the project safely. It does this for people and for their AI agents, on macOS, Linux and Windows, with no LLM and no network.

**Non-goals for v0.1:**
- a code map (v0.2);
- save points and the leak check (v0.3);
- MCP tools over code (v0.4);
- health and conventions checks (v0.5);
- signed installers (v0.3).

## 2. Scope by milestone

Each milestone ships as a pre-release and has exit criteria.

### alpha.1 — "See"

**Features**
- **Machine-wide inventory** of listening sockets, processes and containers.
  - The project's own items are fully actionable.
  - Other projects are listed read-only.
  - "System & unknown" items are explained, not hidden.
- **Project and worktree resolution:** the git root of the launch directory, plus every worktree of the same repository.
- **Docker engine discovery** (§6.4) and compose label mapping.
- **Plain-language cards** for about 30 signatures (§8), at three levels.
- **Runtime Harbor map** for the project (SVG, §13.3), plus the **story panel** generated from the runtime (§13.4).
- **Safe stop** (§10.2), with plan, approval, identity re-check, process-tree handling and an audit log.
- **CLI:** `harbormap`, `status`, `why`, `stop`, `doctor`, `daemon`.
- **Dashboard:** list view, map view, item drawer, Activity (audit log), light and dark themes.
- **Security baseline** (§15).

**Exit criteria**
- On Tier-1 OSes, a test harness starts a Vite server, a FastAPI server, a .NET server and a compose project with Postgres. `harbormap status --json` maps every listener to the right project, service and framework.
- Stopping a supervised or unsupervised dev server never touches an out-of-project process in the negative test suite.

### alpha.2 — "Diagnose"

**Features**
- The doctor rules catalogue in §9:
  - crash reasons;
  - error dictionary;
  - phantom ports;
  - connection doctor;
  - staleness;
  - expected vs actual;
  - resource hogs.
- Findings shown on the map as badges and sentences, with "Copy prompt for my AI agent".

**Exit criteria**
- Every rule has at least one positive and one negative fixture.
- The scenario suite (§16.3) produces the expected finding IDs on Tier-1 OSes.

### alpha.3 — "Start"

**Features**
- Workspace trust.
- Preflight (§10.3).
- Install-and-start with approval.
- Single-service start under supervision, with logs, readiness wait, open URL and restart.

**Exit criteria**
- From a fresh clone of each fixture app (Next.js, Vite + Express, FastAPI, Django, .NET, Go, Spring), a user can press **Start** and reach a working URL, or get one precise, plain-language reason why not.

### alpha.4 — "Recipes"

**Features**
- Multi-service start recipe (§11): ordering, per-step readiness, stop-all in reverse order, merged and per-service logs.
- Optional `harbormap.toml`.

**Exit criteria**
- The Online Boutique and awesome-compose fixtures start in dependency order.
- A failing step stops the chain with an explanation.

### beta — "Agents"

**Features**
- Agent badges.
- Orphan and twin detection.
- MCP server with six tools (§12.2).
- Runtime-aware hooks for Claude Code and Codex (§12.3).
- Worktree collision guard (§12.4).

**Exit criteria**
- Contract tests pass against recorded hook payloads.
- An agent in a fixture session cannot stop anything without dashboard approval.
- A duplicate `npm run dev` is denied with a helpful message.

### 0.1.0

**Activities**
- Five-user test (at least two non-developers) of the first-five-minutes tasks.
- Documentation.
- Release through all channels.

**Exit criteria**
- Every product-level success measure for v0.1 is met (Product design §15).

## 3. Concepts and data model

All types live in `hm-core`. IDs are stable strings derived from content, never sequential numbers.

**Project**
- `id`: hash of the canonical root path.
- `root`, `name`, `worktrees: [Worktree]`, `trusted: bool`.
- `expectations: [Expectation]`, `recipe: Option<Recipe>`.

**Worktree**
- `path`, `branch`, `head`.

**Process**
- Identity: `pid` plus `start_time` together form the identity; `exe`.
- `argv_redacted`, `cwd: Option`, `user`, `parent_chain: [pid]`.
- `env_markers: [Marker]`: names only, matched against the agents pack.
- `project_ref: Option`, `worktree_ref: Option`.
- `kind`: dev_server | database | cache | worker | system | ide | terminal | agent_cli | unknown.
- `signature: Option<SignatureId>`.
- `started_by`: user | harbormap | agent(name, session).
- Metrics: `rss_bytes`, `cpu_pct`, `uptime`.

**Listener**
- `proto`, `addr` (IPv4 or IPv6, bind scope: loopback | all | specific), `port`.
- `owner`, one of:
  - `Process(pid)`
  - `ContainerForwarder(container_id)`
  - `System(reason)`
  - `Reserved(range)`
  - `Unknown(reason)`
- `established_conns`: sampled.

**Container**
- `id`, `engine`, `name`, `image`, `state`, `health`, `exit_code`, `oom_killed`, `restart_count`, `started_at`.
- `ports: [binding]`, `networks`, `mounts`.
- `compose`: project, service, working_dir, config_files, config_hash, oneoff.

**Expectation**
- `service`, `port: Option`, `command: Option`.
- `source`: file + line + kind (compose | package_json | env | procfile | launch_settings | sonar_yaml | framework_default).

**Finding**
- `rule_id`, `severity`: info | warn | problem | danger.
- `subjects: [ref]`, `evidence: [Evidence{kind, text, source?}]`.
- `texts{beginner, builder, expert}`, `actions: [ActionPlanRef]`, `agent_prompt`.
- `fingerprint`: rule + subjects + key evidence. Used to deduplicate and to remember dismissals.

**ActionPlan**
- `id`, `action`: stop | start | restart | install | recipe_start | recipe_stop.
- `targets`, `steps`, `risk`: none | restartable | data_at_risk.
- `requires`: trust | approval | elevation.
- `reversible_by`, `created_by`: user | agent(session).

**AuditEntry**
- `time`, `actor`, `plan_id`, `outcome`, `details`.

**Snapshot**
- Every entity above at one instant, plus `topology_hash`.

## 4. Crates and responsibilities

| Crate | Responsibility | Key dependencies |
|---|---|---|
| `hm-core` | Types; policy predicates (`is_protected`, `owns`); redaction | none heavy |
| `hm-probe` | OS adapters (listeners, processes, cwd, argv, env-names); Docker engines and events; WSL; phantom-port sources | sysinfo, listeners, libproc (macOS), windows-sys, bollard |
| `hm-project` | Project, worktree and git resolution; expectation sources; framework and role detection; signature matching | gix, serde_json / serde_yaml / toml |
| `hm-doctor` | Rules engine; pack loading and validation; template rendering | — |
| `hm-actions` | Planner, policy check, executor (signals, Docker stop), supervisor (spawn, logs, readiness), preflight, recipes | tokio, process-wrap (or own), portable-pty (only if needed) |
| `hm-layout` | v0.1 lane layout for the runtime map (§13.3) | petgraph |
| `hm-store` | SQLite schema and migrations; snapshots, events, audit, trust, recipes, dismissals | rusqlite |
| `hm-daemon` | Lifecycle, scheduler, state diffing, IPC server, HTTP/WebSocket server, embedded UI | axum, tokio, rust-embed |
| `hm-mcp` | stdio MCP server (thin client of the daemon) | rmcp |
| `hm-hooks` | Agent hook adapters (payload parse, decision output) | — |
| `harbormap` | CLI (clap); self-check (`doctor`) | clap |

## 5. Process model and interfaces

### 5.1 Daemon lifecycle

- **Started on demand** by any client:
  1. Check for a live daemon via the socket.
  2. If none is running, spawn `harbormap daemon run` detached.
  3. Wait for it to answer (up to 3 s).
- **Single instance per OS user**, enforced by a lock file in the runtime directory.
- **Idle exit** after 15 minutes with no clients and no supervised processes. Configurable.
- **Crash handling.** If the daemon crashes, clients fall back to a one-shot in-process probe (read-only) and restart it in the background.

### 5.2 IPC (CLI, MCP and hooks ↔ daemon)

- **Transport:** newline-delimited JSON-RPC 2.0.
- **Endpoint:**
  - Linux: `$XDG_RUNTIME_DIR/harbormap/daemon.sock`
  - macOS: `$TMPDIR/harbormap-$UID/daemon.sock`
  - The containing directory is mode 0700 and the socket 0600.
  - Windows: named pipe `\\.\pipe\harbormap-<user-sid>`, with a DACL that allows only that SID.

**Methods (v0.1)**

| Method | Purpose |
|---|---|
| `status.get {project?, scope: project\|machine}` | Snapshot summary |
| `inventory.list {scope}` | Inventory |
| `explain.port {port}` | Explanation for one port |
| `explain.item {ref}` | Explanation for one item |
| `findings.list {project?, subject?}` | Findings |
| `plan.create {action, target}` | Create a plan |
| `plan.execute {plan_id, confirm: true}` | Interactive callers only; never MCP or hooks |
| `approval.request {plan_id, requester}` | Open an approval request |
| `approval.wait {request_id, timeout_ms}` | Wait for the decision |
| `logs.tail {target, lines, follow}` | Logs |
| `wait.ready {target, timeout_ms}` | Readiness wait |
| `trust.get {project}` / `trust.set {project, trusted}` | Workspace trust |
| `recipe.get {project}` / `recipe.save {project, recipe, to_repo: bool}` | Recipes |
| `session.register {agent, session_id, cwd, pid}` | Agent sessions |
| `hook.evaluate {agent, event, payload}` | Hook decisions |
| `doctor.self` | Self-check |
| `daemon.shutdown` | Stop the daemon |

### 5.3 HTTP API and WebSocket (dashboard ↔ daemon)

- `GET /api/v1/...` mirrors the read methods.
- `POST /api/v1/plans`, `POST /api/v1/plans/:id/execute` and `POST /api/v1/approvals/:id/{approve|deny}` require the session cookie, a valid `Origin` and the `X-Harbormap: 1` header.
- `WS /api/v1/events` pushes:
  - `snapshot.invalidated {topology_hash}`
  - `findings.changed`
  - `action.progress {plan_id, step, status}`
  - `approval.requested {request_id, plan summary, requester}`
  - `log.line` (only while subscribed)
- The client re-fetches over HTTP; the WebSocket carries only small events.

### 5.4 CLI

| Command | Behaviour |
|---|---|
| `harbormap` | Ensure the daemon is running, then open the dashboard through a one-time launch URL |
| `harbormap status [--machine] [--json]` | Tree of project → services → ports, with states and top findings |
| `harbormap why <port\|name> [--json]` | Who holds it, why it is there, what is wrong, what you can do |
| `harbormap stop <port\|name\|pid> [--dry-run] [--yes]` | Prints the plan; asks unless `--yes`; refuses out-of-policy targets with the reason |
| `harbormap start [service] [--yes]` | Recipe or single-service start (from alpha.3) |
| `harbormap restart <name>` | Restart a supervised service |
| `harbormap logs <name> [-f]` | Logs |
| `harbormap trust [--revoke]` | Workspace trust |
| `harbormap doctor [--json]` | Self-check: permissions, engines found, socket, versions. The JSON output is masked and safe to paste in issues |
| `harbormap mcp` | stdio MCP server |
| `harbormap hooks install\|uninstall --claude-code\|--codex [--scope project\|user] [--print]` | Hook installation |
| `harbormap hook <agent> <event>` | Internal entry point for hooks |
| `harbormap daemon [run\|stop\|status]` | Daemon control |

Every command supports `--json`. Exit codes: `0` OK, `1` error, `2` refused by policy, `3` needs approval.

## 6. Discovery (`hm-probe`)

### 6.1 Common

- **Sampling.** Listeners and processes are sampled every 2 s while a dashboard is connected and every 15 s otherwise. The Docker event stream is pushed in real time.
- **Idle detection** uses `established_conns` sampled over a rolling window.
- **Honest gaps.** A permission denial yields `Unknown("needs admin rights")`, never a guess.

### 6.2 Per OS

**macOS**
- **Own processes:** sockets via libproc `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)`; cwd via `proc_pidinfo(PROC_PIDVNODEPATHINFO)`; argv and env names via `sysctl KERN_PROCARGS2`.
- **Other users:** parse `netstat -anv -p tcp`, which shows PIDs without root.
- **Docker Desktop and OrbStack:** published ports appear as `com.docker.backend` / OrbStack helpers and are resolved through the engine API.
- **AirPlay:** `ControlCenter` on :5000 and :7000 is classified `System`.

**Linux**
- **Own processes:** `/proc/net/tcp{,6}` plus socket inodes in `/proc/<pid>/fd`; cwd from `/proc/<pid>/cwd`; env names from `/proc/<pid>/environ`.
- **Other users:** `ss -ltnp` as fallback (PIDs may be hidden). `hidepid` is respected.
- **Docker:** with `userland-proxy: false` there is no host listener, so the engine API is authoritative.
- **Rootless Podman:** `pasta` and `rootlessport` are forwarders.

**Windows**
- **Listeners:** `GetExtendedTcpTable(TCP_TABLE_OWNER_PID_LISTENER)` for IPv4 and IPv6.
- **Process details:** sysinfo; cwd from the PEB. The build is 64-bit only so it can read both 32- and 64-bit processes.
- **Special owners:**
  - PID 4 → `System("http.sys / kernel")`.
  - `wslrelay.exe` → WSL forwarder.
- **Excluded port ranges:** parsed from `netsh int ipv4 show excludedportrange protocol=tcp` (and ipv6). A port inside a range → `Reserved(range)`.

### 6.3 WSL (Tier 2)

- WSL distros are detected with `wsl.exe -l -q`.
- An in-distro probe runs through `wsl.exe -d <distro> -- harbormap status --json`. It works only if Harbormap is installed in the distro; otherwise we say so.

### 6.4 Docker engines

**Discovery order**
1. `DOCKER_HOST`
2. The current docker context (from `~/.docker/config.json` and context metadata)
3. Known sockets:
   - `~/.docker/run/docker.sock`
   - `/var/run/docker.sock`
   - `~/.orbstack/run/docker.sock`
   - `~/.colima/default/docker.sock`
   - `~/.rd/docker.sock`
   - the Podman machine socket (from `podman machine inspect`)
   - Windows `//./pipe/docker_engine`

**Behaviour**
- Every reachable engine is probed; several can run at once.
- **Container facts** come from the engine API (bollard): state, health, exit code, OOMKilled, restart count, ports, networks, mounts, labels.
- **Live updates** come from `/events`.
- **Compose:** when the `docker` CLI exists, `docker compose -p <project> config --format json` gives the canonical model, and `--hash '*'` gives config hashes. Otherwise a basic YAML parse covers services, ports, depends_on, healthcheck, environment keys and image/build. Hash checks are then skipped and we say so.

## 7. Project resolution and expectations (`hm-project`)

### 7.1 Project resolution

- **Project** = the git root of the launch directory (via gix), or the directory itself if there is no repository. Worktrees come from `git worktree list`.
- **A process belongs to the project** when its cwd is inside the root or a worktree, or when its parent chain reaches a process that does.
- **A container belongs to the project** when its compose `working_dir` is inside the root or a worktree.
- **Other projects** are grouped by their own git root (machine scope, read-only).

### 7.2 Expectation sources

All parsing is read-only.

| Source | What is read |
|---|---|
| compose | services, published ports, depends_on, healthchecks |
| `package.json` | `scripts.dev` / `scripts.start` and port flags (`--port N`, `-p N`, `PORT=N`); dependencies select a signature |
| `.env*` | Keys only, plus the allowed value patterns in §15 (PORT-like and URL-like keys) |
| Procfile | Process names and commands |
| `launchSettings.json` | `applicationUrl` |
| `sonar.yaml` | Services, ports, health paths |
| Framework defaults (signatures pack) | Next.js 3000, Vite 5173, Angular 4200, CRA 3000, Django 8000, Flask 5000, uvicorn/FastAPI 8000, Rails 3000, Spring 8080 |

`vite.config.*` is code. A literal `server.port` is extracted only by a simple pattern; otherwise the framework default applies, marked "probably".

## 8. Classification and signatures

- **A signature** matches on argv tokens, the executable name, the container image, package dependencies in the cwd's manifest and project files (`manage.py`, `*.csproj`, `go.mod`, `pom.xml`, `build.gradle`).
- **Next.js** renames argv to `next-server (vX)`. It is matched through the parent chain plus `package.json`.
- **Role** (for lanes and colours):

| Role | Examples |
|---|---|
| `see` | UI dev servers |
| `work` | API servers, workers |
| `keep` | Postgres, MySQL, Mongo, Redis, OpenSearch/Elasticsearch, MinIO, RabbitMQ, Kafka |
| `outside` | Hosts from URL-like env values that are not local |
| `shared` | (code only, v0.2) |
| `unknown` | Anything else |

- **v0.1 signature set (~30):**
  - **UI and web:** vite, next, CRA/webpack-dev-server, Angular, Nuxt, Remix, Astro, SvelteKit.
  - **Servers and runtimes:** Express/Node, NestJS, Django, Flask, uvicorn/FastAPI, gunicorn, Rails, dotnet/Kestrel, Spring Boot, Go binaries (`go run`), Celery worker.
  - **Data and infra:** postgres, mysql/mariadb, mongo, redis, opensearch/elasticsearch, minio, rabbitmq, nginx.
  - **Engines, forwarders and system owners:** Docker forwarders, OrbStack, Colima/Lima, wslrelay, AirPlay (ControlCenter), http.sys.
- **Each signature carries:**
  - `role`;
  - `default_port`;
  - restart-trigger file globs (used by STALE-001), marked `verified` or `unverified`;
  - three-level texts.

## 9. Doctor rules catalogue (v0.1)

**How rules work**
- Rules are Rust functions over a Snapshot plus project data.
- Texts, error patterns and thresholds come from packs.
- Severity: `info` < `warn` < `problem` < `danger`.
- Every finding includes evidence, three-level text, at least one action (or "nothing to do") and an agent prompt.

### 9.1 Runtime

| ID | Trigger | Example (beginner) |
|---|---|---|
| RUN-001 | Listener in project not matched to any expectation or signature | "Something is listening on 9000 from your project, but we don't know what it is." |
| RUN-010 | Expected port held by another process | "Port 3000 is taken by blog's Vite server, so web can't use it." |
| RUN-011 | Service running on expected port + 1…10 | "web moved to 3001 because 3000 was busy." |
| RUN-012 | Expected service not running | "api isn't running. It's listed in docker-compose.yml, line 12." |
| RUN-013 | Process's parent is init/launchd/services and its terminal is gone | "This server was left running after its terminal closed." |
| RUN-014 | No established connections over the window and uptime > 6 h | "Nobody has used this server for 6 hours." |
| RUN-015 | Same project + service running twice | "There are two copies of web: the old one may show old code." |
| RUN-020 | Project memory or Docker VM memory vs container usage above threshold | "Docker reserves 8 GB; your containers use 1.2 GB." |

### 9.2 Containers

| ID | Trigger | Example |
|---|---|---|
| CTR-001 | Container exited; explained per exit code: 137 = OOM/killed, 139 = crash, 127 = command not found, 126 = cannot execute, 1 = app error with last log line + error-dictionary match | "db stopped. Its last message says the password is wrong." |
| CTR-002 | Restart count rising within 10 min | "db has restarted 14 times." |
| CTR-003 | Healthcheck failing (last probe output) | "db is up but not healthy yet." |
| CTR-004 | `config-hash` label ≠ current compose hash (needs docker CLI) | "This container is older than your compose file." |
| CTR-005 | Image created before the newest file in its build context, with no bind mount | "This image was built before your last code change." |

### 9.3 Connections

Static rules over env URLs, listeners, containers and networks.

| ID | Trigger |
|---|---|
| CONN-001 | A URL-like env value targets a host:port where nothing listens. |
| CONN-002 | A container's env uses `localhost` / `127.0.0.1` for a service that runs on the host or in another container. Suggests `host.docker.internal` or the compose service name. |
| CONN-003 | A host process's env uses a compose service name (e.g. `db:5432`). Suggests `localhost:<published port>`. |
| CONN-004 | A server bound to `127.0.0.1` only is targeted from a container or another host. |
| CONN-005 | A target is `localhost`, the server listens on IPv4 only, and the client runtime prefers IPv6 (e.g. Node ≥17). Suggests `127.0.0.1`. |
| CONN-006 | The compose published port ≠ the port in the env URL (e.g. `5433:5432` vs `:5432`). |

### 9.4 Staleness

| ID | Trigger |
|---|---|
| STALE-001 | The process started before a restart-trigger file changed (signature globs: `.env*`, framework config). Unverified globs say "might be". |
| STALE-002 | The lockfile is newer than the installed marker (`node_modules/.package-lock.json`, venv, etc.). "Dependencies changed; reinstall." |

### 9.5 Phantom ports

| ID | Trigger |
|---|---|
| PH-001 | The port is in a Windows excluded range ("Windows reserved this range for Hyper-V/WSL; no program to stop.") |
| PH-002 | macOS AirPlay Receiver on 5000/7000 |
| PH-003 | http.sys / PID 4 owns the port |
| PH-004 | The engine reports the port allocated, but there is no container or listener (stale Docker allocation) |

### 9.6 Error dictionary (ERR-*)

About 25 entries, each matched against supervised logs, container logs and CLI error text, then tied to live state:

| Ecosystem | Entries |
|---|---|
| Network and ports | `EADDRINUSE`, "port is already allocated", `ECONNREFUSED`, `EACCES` on ports <1024, "address not available", Windows `10013` |
| Node / npm | `ERR_OSSL_EVP_UNSUPPORTED`, `ERESOLVE`, "'npm' is not recognized" |
| Python / pip | `externally-managed-environment`, `ModuleNotFoundError` after a dependency change |
| PowerShell | "running scripts is disabled" |
| Databases | Postgres "password authentication failed", Postgres "role does not exist", MySQL "Access denied" |
| Configuration | Missing env var patterns per framework |
| Containers | Docker "no space left on device", OOM messages |

Each entry points to a live check. For example, `EADDRINUSE :3000` → "Right now :3000 is held by blog's Vite server (pid 812)".

### 9.7 Agents and worktrees (beta)

| ID | Trigger |
|---|---|
| AGENT-001 | Server started by an agent session that has ended (orphan) |
| AGENT-002 | Agent-started twin (an old copy still serving) |
| WT-001 | Two worktrees want the same port |
| WT-002 | A worktree's config targets another worktree's port or database |
| WT-003 | A database container or volume is shared across worktrees |

## 10. Actions (`hm-actions`)

### 10.1 Policy (single function, exhaustively unit-tested)

`allowed(actor, action, target)` returns `Allow`, `NeedsApproval`, `NeedsTrust`, `NeedsElevation` or `Deny(reason)`, following Product design §9:

| Rule | Result |
|---|---|
| Agent actors | Never `Allow` for state changes; at best `NeedsApproval` |
| Out-of-project targets | `Deny(OpenThatProject)` |
| Protected targets | `Deny(Protected(kind))` |
| Databases / volumes | Plan risk `data_at_risk` and an extra confirmation |

### 10.2 Stop

**Plan.** The target set is the listener's process plus its tree, or the container.
- npm/yarn/pnpm wrappers are included through the parent chain.
- The method is chosen per OS and per target type.

**Execute**
1. Re-check identity (PID + start time + executable; pidfd on Linux, handle on Windows).
2. Signal the target:
   - **Unix:** SIGTERM to the process group, wait 5 s, then SIGKILL the remaining processes in the tree.
   - **Windows, supervised:** the process was started with `CREATE_NEW_PROCESS_GROUP` inside a Job Object. Send `CTRL_BREAK_EVENT`, except to JVM processes, which print a thread dump instead. Wait 5 s, then terminate the Job Object.
   - **Windows, unsupervised:** say that a graceful stop isn't possible, confirm, then run `taskkill /T /F`.
   - **Containers:** engine API `stop` (10 s timeout). Never `rm`, never volumes.
3. Verify that the port is released.
4. Write an audit entry.

**Restart** exists only for processes Harbormap supervises. For other processes the plan offers "Copy start command", reconstructed from argv, cwd and the npm script found in the parent chain.

### 10.3 Start, preflight and install (alpha.3)

**Command candidates**, in order:
1. Recipe or `harbormap.toml`
2. `sonar.yaml` / Procfile
3. compose (`docker compose up -d`)
4. `package.json` `dev`, then `start`
5. `manage.py runserver`
6. uvicorn / FastAPI CLI
7. `dotnet run` (launchSettings profile)
8. `go run .` (main package)
9. `./mvnw spring-boot:run` or `./gradlew bootRun`

The user picks when there are several candidates, and the choice is remembered.

**Workspace trust** (first time per project): show the exact commands and their source files, then ask "Trust and start". Trust is stored as root path + git remote URL hash + time.

**Preflight checks**
- Each check returns pass, fail or warn, with three-level text and a copyable fix.
- Nothing is installed automatically.

| Check | What is compared |
|---|---|
| Runtime present and version | `.nvmrc` / `.node-version` / `engines`, `.python-version` / `requires-python`, `global.json`, `go.mod` `go`/toolchain, Java version (pom/gradle) |
| Dependencies installed | `node_modules` vs lockfile; venv presence; `obj/` restore; Go module cache (best effort) |
| Required env keys | `.env.example` keys present in `.env*` (names only) |
| Docker engine | Running, if the recipe needs compose |
| Target ports | Free; if busy, the owner is explained (RUN-010) |
| Windows only | PowerShell execution policy (blocks `npm.ps1`) |

**Install & start.** The user explicitly clicks it. The install command is chosen from the lockfile (`npm ci`, `pnpm install --frozen-lockfile`, `yarn install --immutable`, `uv sync`, `pip install -r requirements.txt` inside an existing venv, `dotnet restore`, `go mod download`, `./mvnw -q dependency:go-offline`). It is shown first and then runs supervised.

**Supervision**
- Each service gets its own process group or Job Object.
- stdout and stderr go to a ring buffer and to a rotating file (10 MB × 3) in the data directory; `FORCE_COLOR=1`.
- Readiness is one of: port listening, HTTP 2xx at a path, Docker health = healthy, or a log line matching a pattern. Timeout 120 s, with progress shown.
- When the service is ready, the UI shows **Open**.
- **Crash** → CTR/ERR-style finding with the last 20 log lines and a dictionary match.

## 11. Recipes (alpha.4)

A recipe is an ordered set of services with readiness conditions.

**Sources**
- compose (`depends_on` plus healthchecks);
- `sonar.yaml` / Procfile;
- monorepo workspaces (pnpm/npm/yarn workspaces, turbo, nx), whose `dev` scripts become one service per package;
- language entry points;
- env URL links. If web's `API_URL` points at api's port, web depends on api.

**First run**
- The full recipe is shown: commands, working directories and reasons for the order.
- The user can edit or remove steps; approval makes the recipe trusted.
- Any later change to the recipe requires approval again.

**Storage.** By default in Harbormap's state, keyed by project. "Save to repo" writes `harbormap.toml` after showing a diff:

```toml
[project]
name = "shop"

[[service]]
name = "db"
run = "docker compose up -d db"
ready = { docker_health = "db" }

[[service]]
name = "api"
cwd = "apps/api"
run = "uvicorn app:app --port 8000"
ready = { http = "http://localhost:8000/health" }
depends_on = ["db"]

[[service]]
name = "web"
cwd = "apps/web"
run = "npm run dev"
ready = { port = 3000 }
depends_on = ["api"]
```

**Rules**
- Data-changing steps (migrations, seeds, resets) are never added automatically. A user may add one with `data_changing = true`, and it then requires its own confirmation on every run.
- **Execution:** topological order, with independent services started in parallel. If a step fails, the chain stops and already-started services keep running. **Stop all** stops services in reverse order.
- **Logs:** per service and merged; services are colour-coded and errors highlighted.

## 12. Agent layer (beta)

### 12.1 Attribution, orphans and twins

**Who started a process?** Three sources, strongest first:
1. A session registered by a hook (SessionStart gives the session id and cwd).
2. Env-var *names* from `packs/agents.toml`. Verified on 2026-10-15 for Claude Code 2.1.299: `AI_AGENT`, `CLAUDECODE`, `CLAUDE_CODE_SESSION_ID`, `CLAUDE_PID`. Others are marked unverified until checked.
3. A parent chain that reaches a known agent CLI executable.

**Session liveness:** the agent process exists, or the hook has sent a heartbeat.

**Orphan:** the session has ended but its server is still alive → AGENT-001, with a **Stop** action.

**Twin:** the same project + service + signature running twice → AGENT-002 / RUN-015. The plan suggests stopping the older copy.

### 12.2 MCP tools (stdio, `harbormap mcp`)

**General rules**
- Plain-text responses by default; JSON with `format: "json"`.
- Default budget ~1,500 tokens (estimated as bytes ÷ 4); hard cap 8,000; `has_more` plus a cursor.
- Never returns env values or unmasked argv.

| Tool | Annotation | Inputs | Output |
|---|---|---|---|
| `runtime_status` | readOnly | `project?`, `scope?` | Services, ports, states, top findings, suggested worktree ports |
| `explain_port` | readOnly | `port` | Owner, why it is there, findings, safe actions |
| `diagnose` | readOnly | `subject?` (service, port, or free text such as "api can't reach db") | Findings with evidence |
| `wait_ready` | readOnly | `target`, `timeout_s` (≤120) | Ready / not ready, with the reason |
| `plan_action` | readOnly | `action` (start / stop / restart), `target` | Plan with ID and risks; nothing is executed |
| `request_action` | destructive | `plan_id`, `reason` | Opens an approval in the dashboard and CLI; waits ≤60 s; returns approved / denied / timeout. An approved plan is executed by the daemon on the human's approval |

Annotations are advisory only. The daemon enforces approval regardless of client settings.

### 12.3 Hooks (Claude Code and Codex first)

**Installation**
- `harbormap hooks install --claude-code` shows the exact settings diff, then merges it into the project or user settings.
- `--uninstall` removes exactly what was added. `--print` only prints the diff.

**PreToolUse(Bash)** — the hook classifies the command string:
1. **Starts a dev server already running for this worktree** → deny with the reason: "Already running at http://localhost:3000 (pid 4121, started 2 h ago by you). Logs: `harbormap logs web`. To restart: `harbormap restart web`."
2. **Broad kill or destructive container command** (`pkill`, `killall`, `kill $(lsof -t …)`, `fuser -k`, `taskkill /IM … /F`, `docker compose down -v`, `docker volume rm`, `docker system prune`) → resolve to concrete targets with the same matcher semantics. If any target is outside the session's project or worktree, protected, or holds data → deny. The reason names exactly what would have been hit and suggests `harbormap stop <name>`.
3. **Anything else** → allow silently.

**SessionStart.** Register the session and inject at most 5 lines: what runs for this worktree, open problems, suggested ports.

**Fail-open**
- Daemon unreachable, or no reply within 300 ms → allow, with no output.
- Parse errors → allow, logged locally.

**Contract tests** run against recorded payloads per agent version. Claude Code's PreToolUse deny-with-reason and SessionStart `additionalContext` are documented. Codex has a hooks engine in `codex-rs/hooks`; its decision format is verified during the beta.

### 12.4 Worktree guard

- **Group** listeners, processes and containers by worktree.
- **Detect** WT-001, WT-002 and WT-003 (§9.7).
- **Suggest** a port block per worktree (`base + 10 × index`, where index is the worktree's stable order) and the exact env var or flag to set. Nothing is written in v0.1.
- **Expose** the suggested ports in `runtime_status`, so agents pick non-colliding ports.

## 13. UI (dashboard)

### 13.1 Screens

- **Home: Harbor view (map)** for the project, with:
  - the story panel on the left in Beginner;
  - "Elsewhere on this computer" (read-only) and "System & unknown" sections;
  - a list view toggle (a table equivalent of the map).
- **Item drawer:**
  - card (three levels), evidence and findings;
  - logs (when supervised or a container);
  - actions;
  - "Copy prompt for my AI agent".
- **Activity:** the audit log, plus approval requests with Approve / Deny.
- **Start:** preflight checklist, recipe editor and progress, and merged logs.
- **Settings:** theme, default level, polling, protected-process additions, trust list, hook status.

### 13.2 Levels and themes

- A segmented level control appears on every screen. Choosing a level re-renders text and density on the same layout.
- Themes:
  - light and dark, defaulting to `prefers-color-scheme`;
  - a toggle stored in Harbormap's settings;
  - CSS custom properties per the Product design §8 tokens.

### 13.3 Runtime map layout (v0.1, `hm-layout`)

**Layout**
1. **Lanes by role:** People | What people see | What does the work | Where things are kept | Outside services.
2. **Order within a lane:** two barycenter passes over network links, with ties broken by name.
3. **Placement:**
   - box size by content (width fixed per lane);
   - gaps of ≥48 px, and ≥(label width + 16) where a labelled link passes;
   - port tabs on the left edge at the y of their main incoming link (or on the top edge when the link comes from above);
   - links routed orthogonally through gaps, with many-to-one links using a shared bar.

**Determinism and stability**
- Positions are cached by topology hash.
- A refresh keeps existing positions. New items are appended to their lane; removed items leave gaps until the user presses **Tidy**.
- Budgets follow Product design §8: ≤12 boxes in Beginner, ≤25 boxes and ≤40 links in Builder, ≤50 boxes in Expert. Anything beyond collapses into "+N more"; runtime maps rarely reach these limits.

### 13.4 Story panel (runtime-only, v0.1)

Chapters come from templates over the Snapshot:
1. What runs
2. What people see (UI-role services with clickable URLs)
3. What does the work
4. Where things are kept
5. Outside services (hosts parsed from URL-like env values)
6. Right now (open problems, each with an action)

Each chapter highlights its parts on the map when clicked. Text comes from `packs/texts` at the chosen level. A missing fact means the sentence is omitted, never invented.

## 14. Errors and degraded modes

| Condition | Behaviour |
|---|---|
| No Docker engine | Card "Docker isn't running" (or "not installed"), with a fix; runtime still shown |
| Permission denied on a process | Owner "unknown (needs admin rights)"; no actions offered on it |
| Compose CLI missing | Hash-based checks skipped, with a note |
| WSL distro without Harbormap | "Install Harbormap inside <distro> to see its servers" |
| Daemon crash | CLI falls back to a read-only one-shot probe; daemon restarted |
| Hook timeout | Allow silently |
| Pack entry fails schema | Entry skipped, warning in `harbormap doctor`; never fatal |

## 15. Security specifics

- Everything in Product design §10 applies.
- **Allowed env value reads:**
  - keys matching `^(PORT|.*_PORT)$`, parsed as an integer;
  - keys matching `^.*_(URL|URI|HOST|DSN)$`, parsed in memory to scheme, host and port. Userinfo, path, query and fragment are dropped before anything is stored or rendered.
- **Redaction** (argv and log lines shown in the UI) before storage:
  - `--password[= ]…` and `-p<val>` for mysql-like CLIs;
  - `(token|secret|key|password)=…`;
  - `Authorization: Bearer …`;
  - URL userinfo;
  - known key prefixes (`sk_live_`, `sk_test_`, `ghp_`, `gho_`, `xox[bp]-`, `AKIA`, `AIza`);
  - strings of ≥24 characters with ≥3.5 bits/char entropy.

  Property tests check that no fixture secret ever appears in an output.
- Logs shown in the UI are redacted with the same rules. Raw log files stay in the user's data directory with 0600 permissions.

## 16. Testing and verification

### 16.1 Unit and golden tests

- policy predicates;
- redaction;
- every rule (positive and negative fixtures);
- every pack entry;
- templates at all three levels;
- layout determinism.

### 16.2 OS adapter tests

On CI runners for macOS (arm64), Ubuntu (x64) and Windows (x64):
- spawn known servers (Node, Python, .NET, Go, Java) on chosen ports;
- assert listener → process → project → signature mapping, cwd, start-time identity and tree stop.

### 16.3 Scenario suite

Each scenario runs on Linux CI with Docker; other OSes use recorded API fixtures.

| Scenario | Expected finding |
|---|---|
| Postgres with a wrong password | CTR-001/002 + ERR-pg-password |
| `localhost` inside a container | CONN-002 |
| Vite port drift | RUN-011 |
| Orphaned server | RUN-013 |
| Stale image | CTR-005 |
| Outdated container | CTR-004 |
| Windows excluded range | PH-001 (Windows runner) |
| AirPlay | PH-002 (macOS runner) |

### 16.4 Safety tests

- PID reuse race (the identity re-check must refuse);
- a protected-process list for each OS;
- an out-of-project process;
- an agent `request_action` without approval.

### 16.5 Other tests

- **Security:** DNS rebinding (wrong Host), cross-origin POST, WebSocket from a foreign Origin, missing header, other-user socket access; fuzzing of the IPC and HTTP JSON parsers.
- **UI:** Playwright on Linux, light and dark themes, keyboard-only navigation, axe accessibility checks.
- **Hooks:** contract tests on recorded payloads.

## 17. Acceptance criteria (v0.1.0)

1. All milestone exit criteria are met.
2. The first-five-minutes test passes with five users, at least two of them non-developers. Each completes, unaided:
   - see what runs and open the app;
   - free a busy port;
   - understand one problem from its card;
   - start the project.
3. No secret value appears in any fixture output, log view or MCP response; property tests are green.
4. Zero wrong-process actions in the safety suite on all Tier-1 OSes.
5. The README "first five minutes", the support-tier matrix, the "What Harbormap never does" page and SECURITY.md are published.

## 18. Out of scope for v0.1

| Feature | Lands in |
|---|---|
| Active HTTP probes for wedged servers | v0.2 |
| Credentials lookup | v0.3 |
| `.env` drift beyond key names | v0.3 |
| Socket-based live dependency graph | Dropped |
| Named local URLs | Dropped |
| Host-process logs for processes we did not start | Not possible |
| Code map | v0.2 |
| Save points and leak check | v0.3 |
| MCP over code | v0.4 |
| Health | v0.5 |
