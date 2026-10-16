# Harbormap: detecting and placing infrastructure parts

Method note: URLs marked (fetched) were read this session. Others (unmarked, "known docs") are from prior knowledge of the official docs and were not re-verified; treat details there as likely, not confirmed.

## 1. Deterministic detection from repo files

Principle: separate three evidence grades. A = declared and structured (a parser gives a precise answer). B = conventional (a name or pattern strongly implies it). C = weak hint (variable names, dependency presence). The UI should show the grade.

| File / pattern | Reveals | Confidence |
|---|---|---|
| `docker-compose*.yml`, `compose.yaml` `services:` | Each service. `build:` = your own code. `image:` only = third-party (postgres, redis, mysql, mongo, rabbitmq, kafka, minio, mailhog). `ports` = reachable from host. `depends_on` = edges. `volumes` named = stateful. `profiles` = optional. Role inferred from the image name (fetched: https://docs.docker.com/reference/compose-file/services/) | A for existence and edges, B for role |
| `Dockerfile` (`FROM`, `EXPOSE`, `CMD`) | One buildable unit. `EXPOSE` gives its port, `FROM node/python/golang` gives the runtime | A existence, B role |
| `.devcontainer/devcontainer.json` | Dev environment, `dockerComposeFile`, `forwardPorts`, `features` (often DBs) | A |
| `Procfile` | Process types: `web`, `worker`, `release`, `clock`. Heroku/Dokku/Railway style | A |
| `fly.toml` | App name, `[http_service]`, `internal_port`, `[processes]`, `[[mounts]]`, deploy target = Fly | A |
| `render.yaml` | `services[].type` in web/pserv/worker/cron/keyvalue, `databases:` list, `fromDatabase` env links, so full topology (fetched: https://render.com/docs/blueprint-spec) | A |
| `railway.json` / `railway.toml` | Build and deploy settings only (builder, start command, healthcheck). Does not list databases | A for target, none for topology |
| `vercel.json`, `.vercel/project.json` | Deploy target = Vercel, crons, rewrites, functions. The framework comes from `package.json` | A target |
| `netlify.toml` | Deploy target, build command, publish dir, functions, redirects | A |
| `app.yaml` (GAE) | Runtime, service name, `handlers`, deploy target = Google App Engine | A, but the name is generic: require the `runtime:` key |
| `wrangler.toml/jsonc` | Cloudflare Worker, bindings: `kv_namespaces`, `d1_databases`, `r2_buckets`, `queues`, `durable_objects`, `services` (https://developers.cloudflare.com/workers/wrangler/configuration/) | A, the best structured source for data stores |
| `firebase.json`, `.firebaserc` | Hosting, functions, firestore, storage; project IDs per alias (dev/prod) | A |
| `supabase/config.toml`, `supabase/migrations/` | Local Supabase stack (Postgres, auth, storage ports) and schema | A |
| `k8s/*.yaml` with `apiVersion`+`kind` | `Deployment`/`StatefulSet`/`CronJob` = workloads; `Service`/`Ingress` = exposure; `PersistentVolumeClaim`; container `image`, `env` | A when parsed by `kind`. Do not guess from the folder name |
| `Chart.yaml`+`values.yaml` (Helm) | Chart name, dependencies (subcharts like postgresql, redis). Templates are not renderable without Helm | A for dependencies, B for the rest |
| `kustomization.yaml`, `overlays/{dev,prod}` | Base plus per-environment overlays; resources list | A |
| `*.tf` (Terraform/OpenTofu): `resource "aws_db_instance"`, `aws_s3_bucket`, `google_sql_*` | Cloud resources by type prefix (provider) and type (category) | A for declared. It does not show what is actually deployed |
| `Pulumi.yaml`, `Pulumi.<stack>.yaml` | Project, runtime, stacks (environments). Resources are in code | A stacks, C resources |
| `cdk.json` + `lib/*-stack.ts` | AWS CDK app. Constructs like `new s3.Bucket`, `new rds.DatabaseInstance` found by regex | B |
| `serverless.yml` | `provider`, `functions`, `events` (http, sqs, schedule), `resources` (CloudFormation) | A |
| `template.yaml` / `samconfig.toml` (SAM) | `AWS::Serverless::Function/Api`, `AWS::DynamoDB::Table`, etc. Check for the `Transform: AWS::Serverless` header | A |
| `.github/workflows/*.yml` | Jobs, triggers (`push` to branch, `tags`, `release`), `environment:` (staging/production), `needs`, and deploy actions or commands (`aws-actions/*`, `azure/*`, `google-github-actions/*`, `flyctl deploy`, `vercel`, `kubectl`, `helm upgrade`, `terraform apply`, `docker push`). See https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax (fetched, but the page was truncated before the `environment` key) | A for the jobs, B for the target, which is read from the step text |
| `.gitlab-ci.yml` (`stages`, `environment:`) , `.circleci/config.yml` (`workflows`, `orbs`), `Jenkinsfile`, `azure-pipelines.yml`, `bitbucket-pipelines.yml` | Same idea: stages, jobs, deploy commands. GitLab `environment: name:` is explicit | A/B |
| `.env.example`, `.env.sample`, `.env.template` | Variable NAMES only. `DATABASE_URL`, `REDIS_URL`, `MONGODB_URI`, `STRIPE_`, `SENDGRID_`, `AWS_`, `S3_BUCKET`, `SMTP_`, `OPENAI_` | C. Names are hints, not proof |
| Real `.env`, `.env.local` | Same, plus values. Read the URL scheme/host only (see section 4) | C |
| `package.json` deps: `stripe`, `@sendgrid/mail`, `@aws-sdk/client-s3`, `pg`, `ioredis`, `mongoose`, `@prisma/client`, `@supabase/supabase-js` | Outside services and database drivers | B. A dependency may be unused or a type-only import |
| `requirements.txt` / `pyproject.toml` / `go.mod` / `Cargo.toml` / `Gemfile` / `pom.xml` / `*.csproj` | Same via `psycopg`, `redis`, `boto3`, `stripe`, `sqlalchemy`, `lib/pq`, `sqlx`, `Npgsql` | B |
| `prisma/schema.prisma` | `datasource db { provider = "postgresql" }` gives the DB engine; `model X` gives tables; `env("DATABASE_URL")` ties the DB to the variable (https://www.prisma.io/docs/orm/prisma-schema/overview) | A |
| `alembic.ini`, `alembic/versions/*.py` | Python DB migrations; `op.create_table(...)` names tables. Engine comes from `sqlalchemy.url` or the code | A tables, B engine |
| EF Core `Migrations/*.cs` | `migrationBuilder.CreateTable(name: ...)`; provider in `UseNpgsql/UseSqlServer` | A tables |
| Others: `db/migrate/*.rb` + `config/database.yml` (Rails), `knexfile`, `drizzle.config.ts`, `flyway`/`liquibase`, `schema.sql` | Engine and tables | A/B |
| Code with a literal port, e.g. `listen(3000)` | Port intent | C. Only a hint for matching a running process |

Practical tips:
- Parse structured files with real parsers (YAML, TOML, HCL, JSON). Use regex only for CDK and for CI step text.
- Image-name table for compose and k8s role: `postgres|mysql|mariadb|mongo|cassandra|clickhouse` = database; `redis|valkey|memcached|dragonfly` = cache; `rabbitmq|kafka|nats|redpanda` = queue; `minio|localstack` = storage or cloud emulator; `mailhog|mailpit` = email catcher; `nginx|traefik|caddy` = proxy.
- Connect "env var name -> service" by matching the name (DATABASE_URL) to the compose service whose image is a database, as Render does with `fromDatabase`.

## 2. Prior art and how it presents

- Backstage catalog (fetched: https://backstage.io/docs/features/software-catalog/descriptor-format). Entity kinds: Component (typed `service`/`website`/`library`, with a lifecycle), Resource (database, bucket, cluster), System (a group), API (what a component exposes), plus `dependsOn`. This is a good vocabulary: Component vs Resource vs System. Weakness: it is hand-written YAML; nothing is detected.
- C4 / Structurizr deployment diagrams (fetched: https://c4model.com/diagrams/deployment). One diagram per environment; nested deployment nodes; infrastructure nodes (DNS, load balancer); an icon key is required. Lesson: draw one environment at a time.
- Inframap (fetched: https://github.com/cycloidio/inframap). It reads tfstate or HCL and goes beyond `terraform graph` by grouping (VPC/region), filtering unconnected nodes, and showing only key resources. Lesson: raw graphs are unreadable; provider-specific simplification and a "show everything" escape (`--raw`) work. Rover and Pluralith are interactive Terraform plan visualisers (https://github.com/im2nguyen/rover); same lesson, with plan diff colours (create/update/delete).
- Diagrams by mingrammer (https://diagrams.mingrammer.com/), Mermaid architecture diagrams (https://mermaid.js.org/syntax/architecture.html): diagrams-as-code with groups, services, and icons. They are good export targets: Harbormap could emit Mermaid.
- Cloudcraft, Brainboard, AWS Application Composer: cloud-account or IaC-driven drawings with provider icons and swim-lanes by network (VPC/subnet). They need cloud credentials or templates, so they do not fit a no-network tool, except Application Composer (reads SAM/CloudFormation locally, which is similar to our parser).
- Port.io, Cortex: catalog and scorecards; the UI is lists plus a dependency graph. Not canvas-first.
- Docker Desktop: a compose view as a list of containers with ports and state (running/exited). Simple, status-first.
- Railway (https://docs.railway.com/overview/the-basics, fetched; the page confirms services and volumes appear on the project canvas and share a private network, but did not describe databases or environments): a canvas of cards, one per service or database, with a type icon, a name, a status dot, a deploy line (last deploy, source repo or image), a volume attached as a small card, and lines for private-network references. An environment switcher at the top. Render Blueprints (https://render.com/docs/blueprint-spec) are the same idea from `render.yaml`.
- Vercel/Netlify project pages: deploy-centric (production branch, preview deploys, domain), not topology.

Why Railway/Render canvases read well (observation, partly from my knowledge of their UIs): (1) one card per thing, same size and shape; (2) the card type is shown by icon, not text; (3) status is a single dot, not a paragraph; (4) very few edges, only real references; (5) the person's own services sit on top, data stores below; (6) a single environment at a time; (7) details only on click. Harbormap should copy 1-3 and 5-7.

## 3. Proposed taxonomy and one-screen layout

Rows (top to bottom), each a lane of identical cards:
1. People: users/browsers, admins, mobile (only if detected: frontend present, or a domain in config).
2. What you see: frontends, sites, static hosting, CDN.
3. What does the work: backends, APIs, workers, cron jobs, serverless functions, queues consumers.
4. Where data is kept: databases, caches, queues/brokers, object storage, search. Each shows engine, plus tables or models when known (Prisma, Alembic, EF).
5. Outside services: Stripe, SendGrid, S3 (if cloud, not local), auth providers, AI APIs, monitoring. Each shows "how we know" (SDK, env var).

Side lane (left-to-right under or beside the main block): "How it goes live": Source (repo, branch) -> Build (Dockerfile, build command) -> CI (workflow and jobs) -> Deploy target (Vercel, Fly, k8s, AWS, with environment names). Each deploy job is linked to the card(s) it deploys when determinable (same service name, same image, same directory in a monorepo).

State encoding, applied to every card, so it is readable in grayscale and without colour:
- Running locally now: solid border, filled status dot, port number (from the existing port/process/Docker scan).
- Configured but not running: dashed border, hollow dot, label "not running", with the command that would start it if known (`docker compose up web`).
- Only in the cloud: dotted border plus a cloud badge, "declared in Terraform / render.yaml, not on this machine." Never claim it is live; say "declared". Harbormap cannot know the cloud truth without network access.
- Third state for evidence: a small tag A/B/C ("declared", "likely", "hint") so a hint such as an unused SDK does not look like a fact.
Edges: only these three kinds. (a) runtime link (compose `depends_on`, env var -> service), (b) deploy link (CI job -> target), (c) data use (service -> database via DATABASE_URL). Default to showing edges on hover or select, as Inframap suggests (filter unconnected, simplify).
Beginners: one plain sentence per card ("Stores your data. PostgreSQL. Running on port 5432"). Pros: a drawer with file, line, and confidence.

## 4. Pitfalls

- Monorepos: detect per package (workspaces, `apps/*`, `services/*`, nearest Dockerfile/package.json). Attach infra files to the package that owns them (nearest ancestor, `context:` or `working-directory:` in CI). Root-level compose or Terraform belongs to the whole project. Show a group box per app, not one blob.
- Environments: do not merge dev/staging/prod. Detect them from `.env.<name>`, Pulumi stacks, `environment:` in CI, k8s overlays, Terraform workspaces/`*.tfvars`, Firebase aliases, and offer an environment switcher (C4: one diagram per environment). The twelve-factor guide notes that named environments multiply and config is better treated per variable (fetched: https://12factor.net/config), so keep "Unknown environment" as a valid bucket.
- Secrets: read `.env` files only for names and for non-secret facts (scheme, host, port, db name), and strip the user:password part of URLs before storing. Never show or log values; skip values for keys matching `KEY|SECRET|TOKEN|PASSWORD|PRIVATE`; do not read `*.tfstate` values, `terraform.tfvars`, `credentials`, `.pem`. Prefer `.env.example` first. Also exclude secrets from any text sent to an optional LLM. An existing project fix (mask a secret flag's value) shows the team already cares.
- False positives:
  - Dependency present but unused (stale SDK) -> grade C, show "possible".
  - `examples/`, `docs/`, `test/`, `fixtures/`, `node_modules`, `vendor`, `.terraform`, `target` -> exclude by default; commented-out compose services; compose `profiles`.
  - Templated files (Helm templates, Jinja, `${VAR}`) cannot be parsed reliably -> show names only.
  - `app.yaml`, `config.yaml`, `deploy.sh` names are too generic; require content markers.
  - Tests that use SQLite/localstack look like production services.
  - Env var names like `AWS_REGION` do not prove S3.
  - CI deploy detection by keyword can hit a doc echo string; require it to be in `run:`/`uses:`.
  - A Terraform module `source` pointing remote hides its resources; show the module as one box.
- Ground the mapping to running things: match by port, container name or compose project label, and by cwd. Report unmatched as "unknown" rather than forcing a guess.
- Staleness: files are the intent, processes are the fact; label which is which, and never infer "deployed" from a workflow file alone.
