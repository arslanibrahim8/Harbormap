# Harbormap Learn: Technologies catalog and list builder — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new crate `hm-tech` with an embedded, validated technologies catalog (`packs/tech/`) and a pure list builder that turns declared dependencies, imports, config files, Docker images and language counts into the §7.6 technologies list, rendered in English and Turkish at both levels, with zero tokens.

**Architecture:** `packs/tech/*.toml` (CC0) hold one `[[tech]]` entry per technology with its package keys per ecosystem and four plain lines (EN/TR × Beginner/Builder), plus `[[config]]` and `[[image]]` tables that point at tech ids. `hm-tech` embeds the files with `include_str!`, validates them on load (a broken entry is skipped with a warning, a test demands zero warnings), and answers lookups. `hm_tech::build` is a pure function from a `TechInput` (which L3 will fill from the L2a manifest tables) to a `TechList`; `hm_tech::render` prints it. A dev example reads one or more `package.json` files directly so the feature can be seen before L3 wires it.

**Tech Stack:** Rust 1.99.0 (edition 2024). Existing workspace crates only: `serde`, `toml`, `serde_json` (example and tests). No new dependency, so the root `Cargo.toml` is not touched. A stdlib-only Python 3 script checks pack entries against the public registries (dev tool, never run by the product).

**Spec:** `docs/superpowers/specs/2026-11-02-harbormap-learn-engine-design.md` §7.6 (technologies), §8.3 (levels, Turkish caps), §8.4 (both languages), §12 call 15, §14 L3 entry (catalog and evidence; the `tech` command stays in L3). Conventions copied from `docs/superpowers/plans/2026-11-02-harbormap-learn-l1-indexer-core.md`.

## Where this plan fits

This is a slice of L3 pulled forward so it can run beside L2a. It delivers the catalog, the lookups, the builder and the rendering. It does **not** read manifests or lockfiles in product code (L2a), add the `harbormap tech` command or the journey step chips (L3), or draw the panel strip (L5).

**Scope decisions** (none removes a spec requirement):
- **Text lives in the tech pack.** Each technology carries its own four lines. The spec's "glossary term" link is left to L3, which can point a glossary term at a tech id.
- **Two levels, own enums.** `hm_tech::Level { Beginner, Builder }` and `hm_tech::Locale { En, Tr }`. `hm_core::signature::Level` has a third level that Learn does not use (spec §8.3).
- **Database drivers fold into the database.** `pg`, `psycopg`, `pgx` and `Npgsql` all map to `postgresql`. The reader learns "PostgreSQL"; the evidence line still names the package.
- **Category and evidence labels are Rust strings** (`render.rs`), not pack data: ten categories and seven phrases, fixed, in two languages.
- **Node built-ins are a Rust list** (`names.rs`); Go standard-library imports are recognised by the "no dot in the first segment" rule. Python's standard library is left to L2b, which adds Python imports.
- **Beginner prefers described items.** The 12 Beginner slots go to catalog items first, in rank order; unknown packages fill only the slots left. Builder shows every item.
- **The 12-item limit applies per manifest root.**
- **Beginner shows only usage lines** (language, imported, not declared, not imported). Builder adds the declaration, config and image lines.
- **Unknown Docker images are listed by name under Other**, like unknown packages, so nothing is dropped silently. Images that are a variable (`${BASE}`) or `scratch` are skipped.
- **Import coverage is explicit.** The input names the ecosystems whose imports were measured. A package of an unmeasured ecosystem (Python in L1) never gets "declared, not imported".
- **The batch verifier corrects in place.** Each data batch's second agent checks every entry against the registry and the package's own README, and removes or rewrites what is wrong or unsure (brief: "anything unsure is left out").

## Global Constraints

- **Toolchain and lints.** Toolchain `1.99.0`, edition `2024`; `unsafe_code = "forbid"`; `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` stay green after every task. Use `~/.cargo/bin/cargo`.
- **Zero tokens, no network in product code.** Only `scripts/tech/registry_check.py` (a dev tool) uses the network.
- **Pure builder.** `build` reads no files, no store, no clock; the same input gives the same output. Every list is sorted.
- **No invented descriptions.** A package not in the pack is shown by name with "no description yet". Pack lines must be factually right; anything unsure is left out.
- **Categories (exactly these ten, in this display order):** `language`, `framework`, `ui`, `data`, `auth`, `validation`, `testing`, `build`, `deploy`, `other`. Labels EN: Language, Framework, UI, Data and ORM, Auth and security, Validation, Testing, Build and tooling, Deploy and infra, Other. Labels TR: Dil, Çatı (framework), Arayüz, Veri ve ORM, Kimlik ve güvenlik, Doğrulama, Test, Derleme ve araçlar, Dağıtım ve altyapı, Diğer.
- **Text caps** (spec §8.3): Beginner ≤ 18 words (EN), ≤ 15 words (TR); Builder ≤ 30 words. One sentence, one line, ends with `.`. The Turkish line differs from the English one.
- **Limits** (spec §7.6): Beginner ≤ 12 items per root with "+N more"; Builder all; grouped by category, ranked by import count.
- **Privacy.** No private project name, path, identifier or text in any committed file. Tests use the synthetic "shop" app.
- **Pack licence.** Every `packs/tech/*.toml` starts with `# … Licensed CC0-1.0 (see packs/LICENSE). Schema: crates/hm-tech/src/catalog.rs.`
- **Commits.** `git commit -s`, conventional prefix, message ends with exactly `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Touch only the files your task lists.

## Review Focus

1. **Node built-ins and path aliases** (`fs`, `node:fs`, `fs/promises` → `fs`, `@/components/x`, `~/lib`, `#internal`, `astro:content`) never appear as "used but not declared". Task 3: `builtins_and_aliases_are_not_listed`.
2. **An ecosystem whose imports were not measured** (a Python project in L1) never claims "declared, not imported". Task 3: `unmeasured_ecosystem_makes_no_import_claim`.
3. **A monorepo that hoists dependencies to the root `package.json`**: `apps/web` importing `react` declared only at the root counts as declared, and the root does not say "not imported". Task 3: `a_hoisted_dependency_counts_as_declared`.
4. **Package spelling differs from the catalog key**: PyPI `Django` / `django_rest_framework`, NuGet `newtonsoft.json`, crates `serde-json` vs `serde_json`, a Go import `github.com/jackc/pgx/v5/pgxpool`. Task 1: `names_are_normalised_per_ecosystem`.
5. **Docker `FROM` lines with registry, port, tag, digest, variable or `scratch`** (`docker.io/library/node:20-alpine`, `localhost:5000/shop/api:1`, `postgres@sha256:…`, `${BASE}`). Task 1: `image_names_are_normalised`; Task 3: `variable_and_scratch_images_are_skipped`.

Also pinned: workspace packages of the project itself are not technologies (Task 3: `own_packages_are_not_technologies`).

---

## File map

| File | Responsibility |
|---|---|
| `crates/hm-tech/Cargo.toml` | New crate (`serde`, `toml`; dev `serde_json`) |
| `crates/hm-tech/src/lib.rs` | Modules and re-exports |
| `crates/hm-tech/src/model.rs` | `Category`, `Ecosystem`, `Level`, `Locale`, `Texts`, `Tech` |
| `crates/hm-tech/src/names.rs` | Name normalisation per ecosystem, image names, Node built-ins, npm specifier sanity |
| `crates/hm-tech/src/catalog.rs` | `PACK_FILES`, loading, validation, lookups |
| `crates/hm-tech/src/input.rs` | `TechInput` and its rows |
| `crates/hm-tech/src/list.rs` | `build`, `TechList`, `TechItem`, `Evidence`, `Shown` |
| `crates/hm-tech/src/render.rs` | Text rendering EN/TR, category labels |
| `crates/hm-tech/tests/{catalog,list,render,pack}.rs` | Tests |
| `crates/hm-tech/examples/tech_demo.rs` | Dev demo from `package.json` files |
| `packs/tech/*.toml` | The catalog (12 files, see Task 1) |
| `scripts/tech/registry_check.py` | Dev check of pack keys against public registries |

---

### Task 1: `hm-tech` crate — model, names, catalog loading and lookups (sonnet)

**Files:**
- Create: `crates/hm-tech/Cargo.toml`, `src/lib.rs`, `src/model.rs`, `src/names.rs`, `src/catalog.rs`, `tests/catalog.rs`, `tests/pack.rs`
- Create (header comment only, filled by Task 6): `packs/tech/languages.toml`, `shared.toml`, `npm-web.toml`, `npm-data.toml`, `npm-tooling.toml`, `npm-other.toml`, `pypi.toml`, `go.toml`, `maven.toml`, `nuget.toml`, `crates.toml`, `files.toml`

**Interfaces:**
- Produces (`model.rs`, all `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`; enums also `Copy, Hash, PartialOrd, Ord` with `#[serde(rename_all = "snake_case")]`):
  - `enum Category { Language, Framework, Ui, Data, Auth, Validation, Testing, Build, Deploy, Other }` (declaration order = display order); `Category::ALL: [Category; 10]`; `fn as_str(self) -> &'static str`.
  - `enum Ecosystem { Npm, Pypi, Go, Maven, Nuget, Crates }`; `Ecosystem::ALL`; `fn as_str(self) -> &'static str` (`"npm"`, `"pypi"`, …).
  - `enum Level { Beginner, Builder }`, `enum Locale { En, Tr }`.
  - `struct Texts { beginner: String, builder: String }` (`deny_unknown_fields`).
  - `struct Tech { id, name: String, category: Category, en: Texts, tr: Texts, npm, pypi, go, maven, nuget, crates, languages: Vec<String> }` (`deny_unknown_fields`, key lists `#[serde(default)]`); `fn text(&self, Locale, Level) -> &str`; `fn keys(&self, Ecosystem) -> &[String]`.
- Produces (`names.rs`): `fn normalize(eco: Ecosystem, name: &str) -> String`; `fn normalize_image(image: &str) -> String`; `fn is_builtin(eco: Ecosystem, name: &str) -> bool`; `fn is_package_specifier(eco: Ecosystem, name: &str) -> bool`.
- Produces (`catalog.rs`): `const PACK_FILES: &[(&str, &str)]` (the 12 files above, `include_str!("../../../packs/tech/<f>")`); `struct Catalog`; `Catalog::embedded() -> &'static Catalog` (`OnceLock`); `Catalog::load(files: &[(&str, &str)]) -> Catalog`; `fn warnings(&self) -> &[String]`; `fn tech(&self, id: &str) -> Option<&Tech>`; `fn iter(&self) -> impl Iterator<Item = &Tech>` (file order); `fn package(&self, eco, name: &str) -> Option<&Tech>`; `fn config_file(&self, rel_path: &str) -> Option<&Tech>`; `fn image(&self, image: &str) -> Option<&Tech>`; `fn language(&self, name: &str) -> Option<&Tech>`; `fn key_count(&self, eco: Ecosystem) -> usize`.

**Pack format** (every file may hold all three tables; any other top-level key is a warning):

```toml
# Data and ORM packages for npm. Licensed CC0-1.0 (see packs/LICENSE). Schema: crates/hm-tech/src/catalog.rs.
[[tech]]
id = "drizzle"
name = "Drizzle ORM"
category = "data"
npm = ["drizzle-orm", "drizzle-kit"]
en.beginner = "a library that turns code into database queries."
en.builder = "a TypeScript ORM and SQL query builder with typed schemas, plus drizzle-kit for migrations."
tr.beginner = "kodu veritabanı sorgularına çeviren bir kütüphane."
tr.builder = "Türü belli şemalarla çalışan bir TypeScript ORM ve SQL sorgu kurucusu; drizzle-kit göçleri yönetir."

[[config]]
tech = "drizzle"
files = ["drizzle.config.*"]

[[image]]
tech = "postgresql"
names = ["postgres"]
```

**Names rules** (`names.rs`):
- `normalize`: npm → lowercase; pypi → lowercase, every run of `-`, `_`, `.` becomes `-` (PEP 503); go and maven → unchanged; nuget → lowercase; crates → lowercase, `_` → `-`. Patterns are normalised the same way (the trailing `*` is kept).
- `normalize_image`: lowercase; drop `@digest`; drop a `:tag` only when the last `:` is after the last `/`; drop a leading `docker.io/` or `index.docker.io/`, then a leading `library/`.
- `is_builtin`: npm → a `node:` or `bun:` prefix, or the name is in `NODE_BUILTINS` (assert async_hooks buffer child_process cluster console constants crypto dgram diagnostics_channel dns domain events fs http http2 https inspector module net os path perf_hooks process punycode querystring readline repl stream string_decoder sys timers tls trace_events tty url util v8 vm wasi worker_threads zlib); go → the first `/` segment has no `.`; others → false.
- `is_package_specifier`: npm → false for empty, a leading `.`, `/`, `~`, `#`, `@/`, `@` with no `/`, or any `:`; others → non-empty.

**Validation** (`Catalog::load`; an entry that fails is skipped with `"<file>: tech #<n> (<id>) skipped: <reason>"`; never panics):
- id matches `[a-z0-9-]+` and is unique across files; name non-empty; category known (serde).
- each of the four lines: non-empty, no newline, ends with `.`, ≤ caps from Global Constraints (`split_whitespace().count()`); `tr.X != en.X`.
- a key may contain `*` only as its last character, with a non-empty prefix; normalised keys are unique per ecosystem across the pack (a duplicate key is a warning and the later one is dropped); language names lowercase and unique.
- `[[config]]` patterns contain no `/` and are unique; `[[image]]` names are unique after `normalize_image`; both must name a known tech id (`"unknown tech id <id>"`).
- after all files: a tech with no package key, language, config or image says `"tech <id> has nothing that matches it"`.

**Lookup rules:**
- `package`: if `!is_package_specifier` → `None`. Normalise; exact key wins; else the longest matching `prefix*` pattern; for Go also the longest exact key that is a `/`-segment prefix of the name (`github.com/jackc/pgx` matches `github.com/jackc/pgx/v5/pgxpool`), and a segment-prefix exact key beats a shorter pattern.
- `config_file`: match the last `/` segment against each pattern (`*` matches any run of characters, case-sensitive); the most specific wins (most non-`*` characters, then file order).
- `image`: `normalize_image` both sides, exact match. `language`: lowercase exact.

- [ ] **Step 1: Write the failing tests** in `crates/hm-tech/tests/catalog.rs`, using an inline test pack (not the embedded one, which is empty until Task 6):

```rust
const PACK: &str = r#"
[[tech]]
id = "react"
name = "React"
category = "ui"
npm = ["react", "react-dom"]
en.beginner = "a library for building screens from small reusable pieces."
en.builder = "a component library for building user interfaces with JSX and hooks."
tr.beginner = "ekranları küçük, tekrar kullanılabilir parçalardan kuran bir kütüphane."
tr.builder = "JSX ve hook'larla kullanıcı arayüzü kurmaya yarayan bir bileşen kütüphanesi."
# … radix-ui (npm "@radix-ui/*"), django (pypi "Django"), drf (pypi "djangorestframework"),
#   newtonsoft (nuget "Newtonsoft.Json"), serde-json (crates "serde_json"),
#   postgresql (go "github.com/jackc/pgx", npm "pg"), typescript (languages ["typescript", "tsx"]),
#   nextjs (npm "next"), docker; [[config]] nextjs ["next.config.*"], typescript ["tsconfig.json", "tsconfig.*.json"],
#   docker ["Dockerfile", "Dockerfile.*", "*.Dockerfile"]; [[image]] postgresql ["postgres"]
"#;
```

Tests and their assertions:
- `a_valid_pack_loads_without_warnings`: `Catalog::load(&[("t.toml", PACK)]).warnings().is_empty()`.
- `scoped_patterns_and_aliases_resolve`: `package(Npm, "@radix-ui/react-dialog")` → `radix-ui`; `package(Npm, "react-dom")` → `react`; `package(Npm, "left-pad")` → `None`; `package(Npm, "@radix-ui")` → `None`.
- `names_are_normalised_per_ecosystem`: pypi `"Django"` → `django`; pypi `"DjangoRestFramework"` → `drf`; pypi `"Flask_SQLAlchemy"` equals `normalize(Pypi, "flask.sqlalchemy")` (`"flask-sqlalchemy"`); nuget `"newtonsoft.json"` → `newtonsoft`; crates `"serde-json"` → `serde-json` tech; go `"github.com/jackc/pgx/v5/pgxpool"` → `postgresql`; go `"github.com/jackc/pgxx"` → `None`.
- `config_files_match_by_basename`: `"apps/web/next.config.mjs"` → `nextjs`; `"tsconfig.build.json"` → `typescript`; `"deploy/api.Dockerfile"` → `docker`; `"src/next.config.ts.bak"` → `nextjs` (documents that `*` spans dots); `"README.md"` → `None`.
- `image_names_are_normalised`: `image("docker.io/library/postgres:16-alpine")`, `image("postgres@sha256:abc")`, `image("POSTGRES")` → `postgresql`; `normalize_image("localhost:5000/shop/api:1")` == `"localhost:5000/shop/api"`.
- `languages_resolve`: `language("tsx")` → `typescript`; `language("kotlin")` → `None`.
- `builtins_and_specifiers`: `is_builtin(Npm, "fs")`, `is_builtin(Npm, "node:test")`, `is_builtin(Go, "net/http")` true; `is_builtin(Go, "github.com/x/y")` false; `is_package_specifier(Npm, "@/lib")`, `"~/x"`, `"#db"`, `"astro:content"`, `"./a"` all false.
- `broken_entries_are_skipped_with_a_reason`: one pack with a bad id (`"Bad Id"`), a 19-word EN Beginner line, a TR line equal to EN, a duplicate key `react` in a second tech, `*` in the middle of a key, a `[[config]]` pointing at `nope`, a tech with no keys, and an unknown field `npmm`: each yields one warning naming the id or file and reason; the valid entries still load.
- `embedded_pack_loads`: `Catalog::embedded()` does not panic (warnings are checked in Task 7). An empty pack file is not a warning.
- In `crates/hm-tech/tests/pack.rs`: `each_pack_file_is_valid_on_its_own`: for each `PACK_FILES` entry, `Catalog::load(&[entry])` has no warnings except ones containing `unknown tech id` or `has nothing that matches it` (Task 6 writers run it; Task 7 adds the rest of this file).

- [ ] **Step 2: Run** `~/.cargo/bin/cargo test -p hm-tech --test catalog` → FAIL (crate missing).
- [ ] **Step 3: Implement** the crate per the Interfaces and rules above. `Cargo.toml` copies `hm-index/Cargo.toml`'s `workspace = true` fields; description `"Harbormap Learn technologies: catalog pack, lookups and the technologies list"`. Do not add `hm-tech` to the root `Cargo.toml` (members are `crates/*`; no crate depends on it yet). Create the 12 pack files with only their header line.
- [ ] **Step 4: Run** `cargo test -p hm-tech`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` → PASS.
- [ ] **Step 5: Commit** `feat(tech): add the hm-tech crate with catalog loading and lookups`.

---

### Task 2: Registry check script (haiku)

**Files:** Create `scripts/tech/registry_check.py` (stdlib only, executable, `#!/usr/bin/env python3`).

**Interfaces:** `python3 -I scripts/tech/registry_check.py <pack.toml> [<pack.toml>…]` prints one tab-separated line per key: `tech_id<TAB>ecosystem<TAB>key<TAB>STATUS<TAB>upstream description`, then `summary: N ok, M missing, P patterns`. STATUS is `OK`, `MISSING`, `PATTERN` (key ends in `*`; not checked), or `ERROR <reason>`. Reads `[[tech]]` keys and `[[image]]` names (`tech_id` from the table). Exit code 0 always.

Sources (timeout 15 s, `User-Agent: harbormap-pack-check (https://github.com/harbormap/harbormap)`):
- npm: `https://registry.npmjs.org/<name, "/" as %2F>/latest` → `description`; 404 → MISSING.
- pypi: `https://pypi.org/pypi/<name>/json` → `info.summary`.
- go: `https://proxy.golang.org/<module with each uppercase letter X as !x>/@latest` → OK with description `see https://pkg.go.dev/<module>`. 404/410 → MISSING (a key that is a segment prefix but not a module can be MISSING and still right; the verifier judges).
- maven: `https://search.maven.org/solrsearch/select?q=g:"<g>"+AND+a:"<a>"&rows=1&wt=json`; when found, fetch `https://repo1.maven.org/maven2/<g with . as />/<a>/<latestVersion>/<a>-<latestVersion>.pom` and print its first `<description>` (or `-`).
- nuget: `https://azuresearch-usnc.nuget.org/query?q=packageid:<id>&take=1&prerelease=false` → `data[0].description` (first 200 chars); empty `data` → MISSING.
- crates: `https://crates.io/api/v1/crates/<name>` → `crate.description`; sleep 1 s between crates.io calls.
- images: Docker Hub only — `https://hub.docker.com/v2/repositories/<ns>/<name>/` (`ns` = `library` for one-segment names) → `description`; names with a registry host (a `.` or `:` in the first segment) print `PATTERN`.

- [ ] **Step 1:** Write the script. **Step 2:** Run it on a scratch TOML holding `npm = ["react", "@radix-ui/*", "no-such-pkg-hm-zzz"]`, `pypi = ["Django"]`, `go = ["github.com/gin-gonic/gin"]`, `maven = ["org.projectlombok:lombok"]`, `nuget = ["Newtonsoft.Json"]`, `crates = ["serde"]` and an image `postgres`; expected: 7 OK, 1 MISSING, 1 PATTERN. **Step 3: Commit** `chore(tech): add a registry check script for pack entries`.

---

### Task 3: Input and list builder (sonnet; after Task 1, parallel with Tasks 2 and 6)

**Files:** Create `crates/hm-tech/src/input.rs`, `crates/hm-tech/src/list.rs`, `crates/hm-tech/tests/list.rs`; Modify `crates/hm-tech/src/lib.rs` (add `pub mod` and re-exports only).

**Interfaces:**
- Consumes: Task 1 `Catalog`, `Category`, `Ecosystem`, `Level`, `names::*`.
- Produces (`input.rs`, all `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`, and `Default` on `TechInput`; paths are repository-relative with `/`, `""` = repository root):

```rust
pub struct TechInput {
    pub roots: Vec<String>,               // manifest roots (dirs holding a manifest)
    pub own_packages: Vec<PackageRef>,    // packages the project itself defines (workspaces)
    pub declared: Vec<DeclaredDep>,
    pub imports: Vec<ExternalImport>,     // one row per (importer file, package)
    pub import_coverage: Vec<Ecosystem>,  // ecosystems whose imports were measured
    pub configs: Vec<String>,             // any file paths; only catalog matches are kept
    pub images: Vec<DockerImage>,
    pub languages: Vec<LanguageCount>,
}
pub struct PackageRef { pub ecosystem: Ecosystem, pub name: String }
pub struct DeclaredDep { pub ecosystem: Ecosystem, pub name: String, pub declared: Option<String>,
    pub resolved: Option<String>, pub dev: bool, pub file: String, pub line: u32, pub root: String }
pub struct ExternalImport { pub ecosystem: Ecosystem, pub package: String, pub importer: String }
pub struct DockerImage { pub image: String, pub file: String, pub line: u32 }
pub struct LanguageCount { pub path: String, pub language: String, pub files: u32 }
```

- Produces (`list.rs`, `Serialize`):

```rust
pub const BEGINNER_LIMIT: usize = 12;
pub fn build(catalog: &Catalog, input: &TechInput) -> TechList;
pub struct TechList { pub roots: Vec<RootList> }                 // sorted by root path; roots with no items left out
pub struct RootList { pub root: String, pub items: Vec<TechItem> } // rank order
pub struct TechItem { pub tech: Option<String>, pub name: String, pub category: Category,
    pub score: u32, pub evidence: Vec<Evidence> }
pub enum Evidence {   // this declaration order is the evidence order inside an item
    Language { files: u32 },
    Imported { files: u32 },
    NotDeclared { packages: Vec<String> },
    NotImported,
    Declared { file: String, line: u32, name: String, declared: Option<String>, resolved: Option<String>, dev: bool },
    Config { path: String },
    Image { image: String, file: String, line: u32 },
}
pub struct Shown<'a> { pub groups: Vec<(Category, Vec<&'a TechItem>)>, pub more: usize }
impl RootList { pub fn shown(&self, level: Level) -> Shown<'_>; }
```

**Algorithm** (the decisions the tests pin):
1. Roots = `input.roots` ∪ every `declared[].root` ∪ `{""}`. A path belongs to the deepest root `r` with `r == ""`, `path == r` or `path` starting with `r + "/"`.
2. Drop declared deps and imports whose `(ecosystem, normalize(name))` is in `own_packages` (Go: or the import has an own module as a `/`-segment prefix). Drop imports where `is_builtin` or `!is_package_specifier`.
3. Each declared dep becomes an item in its own root, keyed by the catalog tech id, or, when unknown, by `"<eco>:<normalized name>"` with `name` = the declared name and category `Other`. It adds `Declared {…}`.
4. Each import: start at the importer's root and walk up the ancestor roots; the first root that declares the package (normalised equal; Go: the longest declared module that is a segment prefix) gets the importer, on that dep's item. If no root declares it, the importer's own root gets it on the catalog item (or unknown `"<eco>:<normalized>"` item named by the package), and the package name joins `NotDeclared`.
5. Languages: `catalog.language` → that tech's item in the path's root, `files` summed; an unknown language is an item named by the language, category `Language`. Configs: only `catalog.config_file` matches, one `Config` each (deduplicated). Images: skip `scratch` and anything containing `$`; `catalog.image` → that item, else an unknown item named by `normalize_image(image)` in `Other`.
6. Per item: `Imported { files }` = distinct importers (when > 0); `NotImported` when it has no importer and at least one `Declared` whose ecosystem is in `import_coverage`; `NotDeclared { packages }` sorted, deduplicated. `score = max(distinct importers, language files)`. Evidence sorted by kind order above, then by content.
7. Rank: `(score desc, category display order, dev_only, name lowercase, name)` where `dev_only` = has `Declared` and every `Declared` is `dev`. (Changed during execution: the category key was added after the demo on a real project showed alphabetical Beginner picks without import data.)
8. `shown(Builder)`: all items grouped by `Category` order, rank order inside a group, `more = 0`. `shown(Beginner)`: walk items in rank order taking those with `tech.is_some()` up to 12, then fill remaining slots with unknown items in rank order; group the same way; `more = items.len() - shown`.

- [ ] **Step 1: Write the failing tests** in `tests/list.rs` with an inline test catalog (React, Radix UI, Next.js, Zod, Vitest, TypeScript (languages typescript/tsx, npm typescript, config tsconfig.json), PostgreSQL (npm pg, pypi psycopg, image postgres), Django (pypi django), and 13 more simple `other` techs `t01`…`t13` with npm keys `pkg-01`…`pkg-13` for the limit test). Each test builds a `TechInput` by hand:
  - `ranks_by_import_count_inside_categories`: react imported by 3 files, radix by 5, zod by 1, next by 2 → UI group order `[Radix UI, React]`; group order `[Framework, UI, Validation]` (by `Category` order, not by score).
  - `the_three_evidence_lines`: `next` declared and imported in 2 → `[Imported{2}, Declared{…}]`; `vitest` declared dev, npm in coverage, no import → `[NotImported, Declared{dev: true…}]`; import of `zod` with no declaration → `[Imported{1}, NotDeclared{["zod"]}]`.
  - `unmeasured_ecosystem_makes_no_import_claim`: pypi `django` declared, coverage `[Npm]` → evidence exactly `[Declared{…}]`.
  - `beginner_shows_twelve_with_more`: 13 known `t` items + 1 unknown `left-pad` + React → Beginner shows 12 known, `more == 3`; Builder shows 15, `more == 0`. A second case with 3 known + 2 unknown → Beginner shows all 5 (unknown fill).
  - `aliases_merge_into_one_item`: `react` and `react-dom` declared, imported by overlapping files `{a, b}` and `{b, c}` → one React item with `Imported{3}` and two `Declared`.
  - `monorepo_roots_are_listed_separately`: roots `""`, `apps/web`, `apps/api`; next in web, pg in api; languages `apps/web/src` typescript 10 → two `RootList`s (`apps/api`, `apps/web`), the root `""` left out (no items).
  - `a_hoisted_dependency_counts_as_declared`: react declared in root `""`; `apps/web` is a root with only `next`; importer `apps/web/src/a.tsx` imports react → React sits in root `""` with `Imported{1}` and no `NotImported`, and `apps/web` has no `NotDeclared`.
  - `builtins_and_aliases_are_not_listed`: imports `fs`, `node:path`, `@/components/x`, `~/lib`, `#db`, `astro:content` → `build` returns no items.
  - `own_packages_are_not_technologies`: `own_packages = [npm "@shop/ui"]`, declared `@shop/ui` `workspace:*` and imported → no item for it.
  - `variable_and_scratch_images_are_skipped`: images `scratch`, `${BASE_IMAGE}`, `postgres:16` (Dockerfile:1), `acme/worker:2` → items PostgreSQL (`Image{…}`) and unknown `acme/worker` in `Other`.
  - `languages_and_configs_join_their_tech`: language typescript 40 files + tsx 2 files + npm `typescript` dev + `tsconfig.json` → one TypeScript item, `score 42`, evidence `[Language{42}, NotImported, Declared{dev}, Config{"tsconfig.json"}]` (npm covered).
  - `build_is_deterministic`: shuffling every input vector gives an identical `TechList` (compare `serde_json::to_string`).
- [ ] **Step 2: Run** `cargo test -p hm-tech --test list` → FAIL.
- [ ] **Step 3: Implement** `input.rs` and `list.rs` (use `BTreeMap`/`BTreeSet` keyed by root and item key so ordering is free).
- [ ] **Step 4: Run** `cargo test -p hm-tech`, clippy, fmt → PASS.
- [ ] **Step 5: Commit** `feat(tech): build the technologies list from declared, imported and detected items`.

---

### Task 4: Rendering in English and Turkish (sonnet; after Task 3)

**Files:** Create `crates/hm-tech/src/render.rs`, `crates/hm-tech/tests/render.rs`; Modify `src/lib.rs` (`pub mod` and re-exports only).

**Interfaces:** `pub fn render(list: &TechList, catalog: &Catalog, level: Level, locale: Locale) -> String`; `pub fn category_label(category: Category, locale: Locale) -> &'static str` (labels in Global Constraints); `pub fn evidence_line(evidence: &Evidence, locale: Locale) -> String`.

**Exact copy:**

| Piece | EN | TR |
|---|---|---|
| Title | `Technologies` | `Teknolojiler` |
| Title with roots (only when the list has ≥ 2 roots) | `Technologies · <root or .>` | `Teknolojiler · <root or .>` |
| Item | `  <name>: <line>` | same |
| No line | `no description yet` | `henüz açıklama yok` |
| Language | `used in <n> files` / `used in 1 file` | `<n> dosyada kullanılıyor` |
| Imported | `imported in <n> files` / `imported in 1 file` | `<n> dosyada içe aktarılıyor` |
| Not declared | `used but not declared: <a, b>` | `kullanılıyor ama tanımlı değil: <a, b>` |
| Not imported | `declared, not imported (may be a command-line tool)` | `tanımlı ama içe aktarılmıyor (bir komut satırı aracı olabilir)` |
| Declared | `declared: <file>:<line> · <name>[ <declared>][ (resolved <v>)][ · dev]` | `tanım: <file>:<line> · <name>[ <declared>][ (kurulu <v>)][ · geliştirme]` |
| Config | `config file <path>` | `ayar dosyası <path>` |
| Image | `Docker image <image> (<file>:<line>)` | `Docker imajı <image> (<file>:<line>)` |
| More | `+<n> more` | `+<n> tane daha` |

Layout: title line; per group a category label line at column 0; each item line indented 2; each evidence line indented 4; Beginner prints only Language, Imported, NotDeclared, NotImported lines; Builder prints all; `+N more` on its own line after the last group when `more > 0`; roots separated by one blank line; output ends with `\n`. `<line>` is `tech.text(locale, level)`; unknown items use "no description yet".

- [ ] **Step 1: Write failing tests** in `tests/render.rs` (inline catalog from Task 3's style; build a list with Next.js imported in 2 files, an unknown `left-pad` declared not imported, 12 filler techs `t01`…`t12`, so Beginner shows Next.js and 11 fillers and hides one filler and `left-pad`):
  - `beginner_turkish_golden`: the full expected string for `Level::Beginner, Locale::Tr`, including `Çatı (framework)`, `  Next.js: <tr beginner line>`, `    2 dosyada içe aktarılıyor`, and `+2 tane daha`.
  - `builder_english_golden`: full string with `declared: package.json:12 · next ^15.0.0 (resolved 15.0.3)` and `declared: package.json:30 · left-pad ^1.3.0 · dev`.
  - `singular_file_counts`: `imported in 1 file`, `used in 1 file`.
  - `monorepo_titles`: two roots → `Technologies · .` and `Technologies · apps/web`; one root → plain `Technologies`.
  - `every_category_has_both_labels`: for every `Category::ALL`, EN and TR labels are non-empty and differ.
- [ ] **Step 2–4:** Run → FAIL; implement; run all → PASS.
- [ ] **Step 5: Commit** `feat(tech): render the technologies list in English and Turkish`.

---

### Task 5: Dev demo `tech_demo` (sonnet; after Task 4)

**Files:** Create `crates/hm-tech/examples/tech_demo.rs`; Modify `crates/hm-tech/Cargo.toml` (`[dev-dependencies] serde_json.workspace = true` if Task 1 did not add it).

**Behaviour:** `cargo run -p hm-tech --example tech_demo -- [--builder] [--en] <package.json>…`. The file starts with a doc comment: `//! DEV DEMO ONLY. Reads package.json files directly to show the technologies list before L3 wires it; product manifest reading belongs to L2a.`
- For each path: parse with `serde_json`; root = the path's parent as given (`/`-separated, `""` for the current folder); `own_packages` gets its `name`; each `dependencies` / `devDependencies` key becomes a `DeclaredDep` (`declared` = the range, `resolved = None`, `line` = 1-based line of the first line containing `"<name>":` inside the file text, else 1).
- `configs` = names of the files in each package.json's folder (not recursive), prefixed by the root. `imports`, `images`, `languages` empty; `import_coverage` empty, so no import lines are claimed.
- Prints `render(&build(..), Catalog::embedded(), level, locale)`; defaults Beginner and Turkish. Prints `Catalog::embedded().warnings()` to stderr if any.

- [ ] **Step 1:** Write the example. **Step 2:** Make `crates/hm-tech/tests/fixtures/shop/package.json` (synthetic shop app: next, react, react-dom, zod, drizzle-orm, left-pad; dev typescript, vitest) and run the demo on it; expected: exit 0, title `Teknolojiler`, a line `  left-pad: henüz açıklama yok` (after Task 6 lands, Next.js and React show their Turkish lines). **Step 3: Commit** `feat(tech): add a dev demo that lists technologies from package.json`.

---

### Task 6: Catalog data batches (haiku writers, sonnet verifiers; after Task 1; batches run in parallel)

Each batch owns exactly one pack file and touches nothing else. It runs in its own worktree on a branch from the Task 1 commit; the controller cherry-picks after the verifier passes.

**Writer rules (every batch):**
- One `[[tech]]` per technology; aliases go in one entry (`npm = ["react", "react-dom"]`). Scope or prefix patterns end with `*` (`"@radix-ui/*"`).
- Category guide: **language** languages and runtimes · **framework** app and server frameworks that shape the project · **ui** components, styling, icons, charts, animation, client state, forms · **data** databases and drivers, ORMs, query builders, migrations, caches, queues, data fetching · **auth** login, sessions, tokens, password hashing, crypto, security headers, sanitising · **validation** schema and input validation · **testing** runners, assertions, mocks, browser automation, fake data · **build** compilers, bundlers, linters, formatters, task runners, type definitions, env loaders · **deploy** containers, cloud SDKs, hosting CLIs, infra as code, monitoring and error tracking · **other** everything else (HTTP clients, dates, logging, CLI helpers, email, payments, AI SDKs, files).
- Lines read after `"<Name>: "`, start lowercase unless the first word is a proper name, end with `.`. **Beginner** says what it does for the project in everyday words (no "ORM", "SSR", "middleware", "runtime", "hook" unexplained), e.g. `a library that turns code into database queries.` **Builder** says what it is technically and its main job. Turkish: plain, natural Turkish a beginner understands (`kütüphane`, `veritabanı`, `sunucu`, `tarayıcı`, `sayfa`, `test` are fine; avoid untranslated jargon), e.g. `kodu veritabanı sorgularına çeviren bir kütüphane.`
- Only packages you are sure of, as they are published (exact registry spelling). If unsure what it does, leave it out. Do not define ids owned by another batch (table below); if a package of yours belongs to one, list it in your report under "cross-ecosystem keys" instead. Ids: npm batches use the bare name (`zod`); batches 6f–6j use the bare name only when it is clearly unique to their ecosystem (`django`, `gin`, `lombok`), and `<name>-<py|go|java|dotnet|rs>` for generic names that other ecosystems also use (`bcrypt-py`, `uuid-rs`, `validator-rs`). The whole-pack test catches any clash left; the controller renames.
- Run `~/.cargo/bin/cargo test -p hm-tech --test pack each_pack_file_is_valid_on_its_own` (added by Task 1). Zero warnings for your file. Commit `feat(pack): add <file> technologies`.

**Verifier rules (sonnet, per batch, same worktree):** run `python3 -I scripts/tech/registry_check.py packs/tech/<file>`; for every entry check (a) each key exists with that spelling (MISSING → remove, unless it is a valid Go segment prefix), (b) the four lines match the upstream description/README (open the README with WebFetch for anything not obvious), (c) category per the guide, (d) Turkish is correct and plain, (e) no key belongs to another batch. Fix in place, remove anything unsure, commit `fix(pack): verify <file> against the registries`, and report: entries kept, removed (with reason), rewritten.

**Batches** (ids in **bold** are required, because other files point at them; the rest of each list is the packages to cover, by registry name):

| Batch | File | Packages |
|---|---|---|
| 6a | `languages.toml` + `shared.toml` | languages.toml: **typescript** (languages typescript, tsx; npm typescript), **javascript**, **python**, **go**, **java**, **csharp**, **rust**, **nodejs**, **bun**, **deno**, **dotnet**, kotlin (maven `org.jetbrains.kotlin:*`). shared.toml, one tech each with keys in every ecosystem listed: **postgresql** (npm pg postgres pg-promise · pypi psycopg psycopg2 psycopg2-binary asyncpg · go github.com/jackc/pgx github.com/lib/pq · maven org.postgresql:postgresql · nuget Npgsql Npgsql.EntityFrameworkCore.PostgreSQL · crates tokio-postgres postgres), **mysql** (mysql mysql2 · mysqlclient PyMySQL mysql-connector-python aiomysql · github.com/go-sql-driver/mysql · com.mysql:mysql-connector-j mysql:mysql-connector-java · MySqlConnector MySql.Data Pomelo.EntityFrameworkCore.MySql · mysql mysql_async), **sqlite** (better-sqlite3 sqlite3 sqlite · aiosqlite · github.com/mattn/go-sqlite3 modernc.org/sqlite · org.xerial:sqlite-jdbc · Microsoft.Data.Sqlite Microsoft.EntityFrameworkCore.Sqlite · rusqlite libsqlite3-sys), **mongodb** (mongodb · pymongo motor · go.mongodb.org/mongo-driver · org.mongodb:mongodb-driver-sync · MongoDB.Driver · mongodb), **redis** (redis ioredis @redis/client · redis · github.com/redis/go-redis github.com/go-redis/redis github.com/gomodule/redigo · redis.clients:jedis io.lettuce:lettuce-core · StackExchange.Redis · redis), **sqlserver** (mssql tedious · pymssql · github.com/microsoft/go-mssqldb github.com/denisenkom/go-mssqldb · com.microsoft.sqlserver:mssql-jdbc · Microsoft.Data.SqlClient System.Data.SqlClient Microsoft.EntityFrameworkCore.SqlServer · tiberius), **elasticsearch** (@elastic/elasticsearch · elasticsearch · github.com/elastic/go-elasticsearch · co.elastic.clients:elasticsearch-java · Elastic.Clients.Elasticsearch NEST · elasticsearch), **kafka** (kafkajs · kafka-python confluent-kafka aiokafka · github.com/segmentio/kafka-go github.com/IBM/sarama github.com/confluentinc/confluent-kafka-go · org.apache.kafka:kafka-clients org.springframework.kafka:spring-kafka · Confluent.Kafka · rdkafka), **rabbitmq** (amqplib · pika aio-pika · github.com/rabbitmq/amqp091-go · com.rabbitmq:amqp-client org.springframework.amqp:spring-rabbit · RabbitMQ.Client · lapin), graphql (graphql graphql-tag · graphql-core · github.com/graphql-go/graphql · com.graphql-java:graphql-java), grpc (@grpc/grpc-js @grpc/proto-loader · grpcio grpcio-tools · google.golang.org/grpc · io.grpc:grpc-* · Grpc.AspNetCore Grpc.Net.Client Grpc.Tools · tonic), protobuf (protobufjs google-protobuf @bufbuild/protobuf · protobuf · google.golang.org/protobuf github.com/golang/protobuf · com.google.protobuf:protobuf-java · Google.Protobuf · prost), opentelemetry (@opentelemetry/* · opentelemetry-* · go.opentelemetry.io/otel go.opentelemetry.io/contrib · io.opentelemetry:* · OpenTelemetry OpenTelemetry.* · opentelemetry opentelemetry-*), sentry (@sentry/* · sentry-sdk · github.com/getsentry/sentry-go · io.sentry:* · Sentry Sentry.* · sentry sentry-*), prometheus (prom-client · prometheus-client · github.com/prometheus/client_golang · io.prometheus:* · prometheus-net prometheus-net.* · prometheus), stripe (stripe @stripe/stripe-js @stripe/react-stripe-js · stripe · github.com/stripe/stripe-go · com.stripe:stripe-java · Stripe.net · async-stripe), aws-sdk (@aws-sdk/* aws-sdk · boto3 botocore aioboto3 · github.com/aws/aws-sdk-go-v2 github.com/aws/aws-sdk-go · software.amazon.awssdk:* com.amazonaws:* · AWSSDK.* · aws-sdk-* aws-config), google-cloud (@google-cloud/* · google-cloud-* · cloud.google.com/go · com.google.cloud:* · Google.Cloud.*), azure-sdk (@azure/* · azure-* · github.com/Azure/azure-sdk-for-go · com.azure:* · Azure.*), firebase (firebase firebase-admin firebase-tools firebase-functions · firebase-admin · firebase.google.com/go · com.google.firebase:firebase-admin · FirebaseAdmin), **supabase** (@supabase/* · supabase), openai (openai · openai · github.com/openai/openai-go github.com/sashabaranov/go-openai · com.openai:openai-java · OpenAI · async-openai), anthropic (@anthropic-ai/sdk · anthropic · github.com/anthropics/anthropic-sdk-go · com.anthropic:anthropic-java), langchain (langchain @langchain/* · langchain langchain-*), selenium (selenium-webdriver · selenium · org.seleniumhq.selenium:* · Selenium.WebDriver Selenium.Support), testcontainers (testcontainers @testcontainers/* · testcontainers · github.com/testcontainers/testcontainers-go · org.testcontainers:* · Testcontainers Testcontainers.* · testcontainers), **docker** (dockerode · docker · github.com/docker/docker · com.github.docker-java:docker-java · Docker.DotNet · bollard) |
| 6b | `npm-web.toml` | **nextjs** (next), **react** (react react-dom), **vue**, **nuxt**, **angular** (@angular/*), **svelte**, **sveltekit** (@sveltejs/kit), **astro**, solid-js, preact, lit, alpinejs, htmx.org, jquery, gatsby, **remix** (@remix-run/*), react-router react-router-dom, @tanstack/react-router, express, fastify, koa @koa/router, **nestjs** (@nestjs/*), hono, @hapi/hapi, elysia, @trpc/*, socket.io socket.io-client, ws, electron, react-native, expo, @builder.io/qwik, **tailwindcss** (tailwindcss @tailwindcss/*), daisyui, @radix-ui/*, @mui/*, @chakra-ui/*, antd, @mantine/*, bootstrap, react-bootstrap, styled-components, @emotion/*, sass, framer-motion motion, lucide-react, react-icons, @heroicons/react, @fortawesome/*, class-variance-authority, clsx, classnames, tailwind-merge, @headlessui/react, sonner, react-hot-toast, recharts, chart.js react-chartjs-2, d3, three @react-three/*, @tiptap/*, react-markdown, embla-carousel-react, swiper, vaul, cmdk, @dnd-kit/*, react-day-picker, next-themes, @tanstack/react-table, leaflet react-leaflet, @floating-ui/*, vuetify, element-plus, @heroui/*, react-select, zustand, redux @reduxjs/toolkit react-redux, mobx, jotai, pinia, vuex, vue-router, @vueuse/core, i18next react-i18next, next-intl, react-hook-form, formik |
| 6c | `npm-data.toml` | **prisma** (prisma @prisma/client), **drizzle** (drizzle-orm drizzle-kit), **typeorm**, sequelize, mongoose, **knex**, kysely, @mikro-orm/*, objection, @neondatabase/serverless, @planetscale/database, @libsql/client, @vercel/postgres, @upstash/redis, bullmq, bull, dexie, idb, @tanstack/react-query @tanstack/vue-query, swr, axios, ky, got, node-fetch, @apollo/client, @apollo/server, graphql-request, urql, convex, next-auth @auth/*, @clerk/*, better-auth, lucia, passport passport-*, jsonwebtoken, jose, bcrypt bcryptjs, argon2, helmet, cors, express-rate-limit, iron-session, @auth0/*, @kinde-oss/*, dompurify isomorphic-dompurify, sanitize-html, xss, zod, yup, joi, valibot, ajv, class-validator, class-transformer, superstruct, @sinclair/typebox, io-ts, arktype, @hookform/resolvers, validator, @t3-oss/env-nextjs |
| 6d | `npm-tooling.toml` | **type-definitions** (@types/*), **vite** (vite @vitejs/*), **webpack** (webpack webpack-cli), **rollup**, esbuild, @swc/core @swc/*, **babel** (@babel/*), parcel, **turbo**, **nx**, **lerna**, **tsup**, tsx, ts-node, nodemon, concurrently, cross-env, dotenv, **eslint** (eslint @eslint/* eslint-config-* eslint-plugin-* @typescript-eslint/* typescript-eslint), **prettier** (prettier prettier-plugin-*), **biome** (@biomejs/biome), husky, lint-staged, @commitlint/*, **postcss** (postcss), autoprefixer, rimraf, npm-run-all, @changesets/cli, semantic-release, **jest** (jest ts-jest babel-jest @jest/*), **vitest** (vitest @vitest/*), mocha, chai, sinon, ava, @testing-library/*, **playwright** (@playwright/test playwright), **cypress**, puppeteer, supertest, nock, msw, jsdom, happy-dom, @faker-js/faker, storybook @storybook/*, c8, nyc, **cloudflare-workers** (wrangler), vercel, pm2, serverless, aws-cdk aws-cdk-lib, @pulumi/*, sst |
| 6e | `npm-other.toml` | lodash lodash-es, underscore, ramda, date-fns, dayjs, moment, luxon, uuid, nanoid, chalk, commander, yargs, inquirer @inquirer/*, ora, debug, winston, pino, morgan, cheerio, sharp, multer, formidable, nodemailer, resend, @sendgrid/mail, ai @ai-sdk/*, rxjs, immer, superjson, cookie, cookie-parser, express-session, body-parser, compression, http-proxy-middleware, cron node-cron, xlsx, papaparse, pdfkit, pdf-lib, jspdf, marked, gray-matter, @mdx-js/*, next-mdx-remote, contentlayer, @sanity/* sanity, contentful, @vercel/analytics, @vercel/speed-insights, posthog-js posthog-node, mixpanel-browser, twilio, pusher pusher-js, ably, qrcode, mime, glob, fs-extra, chokidar, execa, zx, semver, yaml, js-yaml, qs, query-string, eventemitter3, p-limit, async, bluebird |
| 6f | `pypi.toml` | django, djangorestframework, flask, fastapi, starlette, uvicorn, gunicorn, aiohttp, tornado, sanic, litestar, pydantic, pydantic-settings, marshmallow, sqlalchemy, **alembic**, sqlmodel, peewee, tortoise-orm, celery, rq, dramatiq, requests, httpx, urllib3, beautifulsoup4, lxml, scrapy, **pytest** (pytest pytest-*), coverage, **tox**, nox, hypothesis, faker, factory-boy, freezegun, responses, respx, moto, locust, black, **ruff**, flake8, pylint, **mypy**, isort, **pre-commit**, setuptools, wheel, hatchling, numpy, pandas, scipy, scikit-learn, matplotlib, seaborn, plotly, jupyter notebook ipython, torch, tensorflow, keras, transformers, llama-index, tiktoken, huggingface-hub, pillow, opencv-python, click, typer, rich, tqdm, python-dotenv, pyyaml, tomli, jinja2, werkzeug, passlib, bcrypt, pyjwt, python-jose, authlib, cryptography, python-multipart, email-validator, orjson, structlog, loguru, streamlit, gradio, dash, flask-sqlalchemy, flask-login, flask-cors, wtforms, django-cors-headers, django-environ, channels, strawberry-graphql, graphene, polars, pyarrow, dask, networkx, sympy, statsmodels, xgboost, lightgbm, openpyxl, reportlab, paramiko, kubernetes, apscheduler, arrow, pendulum, python-dateutil, pytz, attrs, websockets, python-socketio, mkdocs mkdocs-material, sphinx, nltk, spacy |
| 6g | `go.toml` | github.com/gin-gonic/gin, github.com/labstack/echo, github.com/gofiber/fiber, github.com/go-chi/chi, github.com/gorilla/mux, github.com/gorilla/websocket, github.com/gorilla/sessions, github.com/julienschmidt/httprouter, github.com/valyala/fasthttp, gorm.io/gorm gorm.io/driver, github.com/jmoiron/sqlx, entgo.io/ent, github.com/uptrace/bun, github.com/golang-migrate/migrate, github.com/pressly/goose, github.com/spf13/cobra, github.com/spf13/viper, github.com/spf13/pflag, github.com/urfave/cli, github.com/stretchr/testify, github.com/onsi/ginkgo, github.com/onsi/gomega, github.com/golang/mock go.uber.org/mock, github.com/DATA-DOG/go-sqlmock, github.com/google/go-cmp, go.uber.org/zap, github.com/sirupsen/logrus, github.com/rs/zerolog, go.uber.org/fx, github.com/google/wire, github.com/golang-jwt/jwt, github.com/go-playground/validator, github.com/google/uuid, github.com/joho/godotenv, github.com/kelseyhightower/envconfig, golang.org/x/crypto, golang.org/x/oauth2, golang.org/x/sync, golang.org/x/net, golang.org/x/text, golang.org/x/time, **nats** (github.com/nats-io/nats.go), github.com/grpc-ecosystem/grpc-gateway, github.com/swaggo/swag, github.com/charmbracelet/bubbletea, github.com/charmbracelet/lipgloss, github.com/fatih/color, github.com/pkg/errors, github.com/samber/lo, github.com/mitchellh/mapstructure, gopkg.in/yaml.v3, github.com/BurntSushi/toml, github.com/pelletier/go-toml, github.com/tidwall/gjson, github.com/goccy/go-json, github.com/go-resty/resty, k8s.io/client-go k8s.io/apimachinery, sigs.k8s.io/controller-runtime, github.com/robfig/cron, github.com/go-co-op/gocron, github.com/hibiken/asynq, github.com/coder/websocket, github.com/a-h/templ, github.com/99designs/gqlgen, github.com/markbates/goth, github.com/casbin/casbin, github.com/minio/minio-go, github.com/shopspring/decimal, github.com/cenkalti/backoff, github.com/patrickmn/go-cache, github.com/dgraph-io/badger, go.etcd.io/bbolt |
| 6h | `maven.toml` | **spring-boot** (org.springframework.boot:*), spring-framework (org.springframework:*), spring-security (org.springframework.security:* and org.springframework.boot:spring-boot-starter-security), spring-data (org.springframework.data:* and org.springframework.boot:spring-boot-starter-data-jpa), spring-cloud (org.springframework.cloud:*), hibernate (org.hibernate.orm:hibernate-core org.hibernate:hibernate-core), hibernate-validator (org.hibernate.validator:hibernate-validator org.springframework.boot:spring-boot-starter-validation), jakarta.persistence:jakarta.persistence-api, jakarta.validation:jakarta.validation-api, jakarta.servlet:jakarta.servlet-api, org.projectlombok:lombok, org.mapstruct:mapstruct, com.fasterxml.jackson.core:*, com.google.code.gson:gson, com.google.guava:guava, org.apache.commons:commons-lang3, commons-io:commons-io, org.slf4j:slf4j-api, ch.qos.logback:logback-classic, org.apache.logging.log4j:*, junit (org.junit.jupiter:* junit:junit), org.mockito:*, org.assertj:assertj-core, io.rest-assured:rest-assured, org.awaitility:awaitility, flyway (org.flywaydb:*), org.liquibase:liquibase-core, io.micronaut:*, io.quarkus:*, io.vertx:*, io.projectreactor:reactor-core, io.reactivex.rxjava3:rxjava, com.squareup.okhttp3:okhttp, com.squareup.retrofit2:retrofit, org.apache.httpcomponents.client5:httpclient5, io.jsonwebtoken:*, com.auth0:java-jwt, org.springdoc:*, io.swagger.core.v3:*, org.jooq:jooq, org.mybatis:mybatis, com.h2database:h2, io.micrometer:*, com.zaxxer:HikariCP, org.apache.poi:*, com.github.ben-manes.caffeine:caffeine, org.thymeleaf:*, org.keycloak:*, org.bouncycastle:*, org.jsoup:jsoup, info.picocli:picocli |
| 6i | `nuget.toml` | **aspnetcore** (Microsoft.AspNetCore.*), Microsoft.AspNetCore.Authentication.JwtBearer, Microsoft.AspNetCore.Identity.EntityFrameworkCore, blazor (Microsoft.AspNetCore.Components.*), signalr (Microsoft.AspNetCore.SignalR.*), ef-core (Microsoft.EntityFrameworkCore Microsoft.EntityFrameworkCore.*), Dapper, Newtonsoft.Json, System.Text.Json, Serilog Serilog.*, NLog, dotnet-extensions (Microsoft.Extensions.*), AutoMapper, MediatR, FluentValidation FluentValidation.*, Swashbuckle.AspNetCore Swashbuckle.AspNetCore.*, NSwag.*, xunit xunit.*, NUnit NUnit3TestAdapter, MSTest.TestFramework MSTest.TestAdapter, Moq, NSubstitute, FluentAssertions, Shouldly, coverlet.collector, Microsoft.NET.Test.Sdk, Bogus, Polly Polly.*, Hangfire Hangfire.*, MassTransit MassTransit.*, Quartz, Refit, RestSharp, Duende.IdentityServer, BCrypt.Net-Next, System.IdentityModel.Tokens.Jwt, Microsoft.Identity.Web, MudBlazor, CommunityToolkit.Mvvm, Avalonia, CsvHelper, ClosedXML, YamlDotNet, FastEndpoints, Spectre.Console, System.CommandLine |
| 6j | `crates.toml` | tokio, async-std, futures, serde, serde_json, serde_yaml, toml, axum, actix-web, rocket, warp, hyper, reqwest, tower, tower-http, sqlx, diesel, sea-orm, clap, anyhow, thiserror, eyre color-eyre, tracing tracing-subscriber, log, env_logger, chrono, time, uuid, rand, regex, once_cell, lazy_static, itertools, rayon, crossbeam, parking_lot, dashmap, bytes, base64, sha2, ring, rustls, openssl, jsonwebtoken, argon2, bcrypt, validator, garde, proptest, quickcheck, criterion, insta, mockall, rstest, assert_cmd, predicates, tempfile, wasm-bindgen web-sys js-sys, yew, leptos, dioxus, tauri, egui eframe, bevy, iced, ratatui, crossterm, indicatif, dialoguer, tree-sitter, notify, walkdir, ignore, config, dotenvy, askama, tera, handlebars, utoipa, lettre, image, ndarray, polars, nom, pest, syn quote proc-macro2 |
| 6k | `files.toml` | `[[config]]` (to the bold ids above, defining only these file-only techs here: **docker-compose**, nginx, caddy, golangci-lint, sqlc, buf): next.config.* → nextjs · nuxt.config.* → nuxt · vite.config.* → vite · vitest.config.* vitest.workspace.* → vitest · jest.config.* → jest · playwright.config.* → playwright · cypress.config.* → cypress · tailwind.config.* → tailwindcss · postcss.config.* → postcss · drizzle.config.* → drizzle · schema.prisma → prisma · tsconfig.json tsconfig.*.json → typescript · astro.config.* → astro · svelte.config.* → sveltekit · angular.json → angular · nest-cli.json → nestjs · remix.config.* → remix · webpack.config.* → webpack · rollup.config.* → rollup · babel.config.* .babelrc → babel · eslint.config.* .eslintrc* → eslint · prettier.config.* .prettierrc* → prettier · biome.json biome.jsonc → biome · turbo.json → turbo · nx.json → nx · lerna.json → lerna · tsup.config.* → tsup · knexfile.* → knex · ormconfig.* → typeorm · wrangler.toml wrangler.json wrangler.jsonc → cloudflare-workers · Dockerfile Dockerfile.* *.Dockerfile → docker · docker-compose.yml docker-compose.yaml compose.yml compose.yaml → docker-compose · pytest.ini → pytest · tox.ini → tox · mypy.ini → mypy · ruff.toml .ruff.toml → ruff · .pre-commit-config.yaml → pre-commit · alembic.ini → alembic · nginx.conf → nginx · Caddyfile → caddy · .golangci.yml .golangci.yaml → golangci-lint · sqlc.yaml sqlc.yml sqlc.json → sqlc · buf.yaml buf.gen.yaml → buf. `[[image]]` (defining here: mariadb, memcached, valkey, nginx, caddy, traefik, apache-httpd, alpine, debian, ubuntu, minio, mailpit, localstack, adminer, pgadmin, grafana, jaeger, clickhouse, cassandra, neo4j, opensearch, keycloak): node → nodejs · python → python · golang → go · rust → rust · eclipse-temurin openjdk amazoncorretto → java · mcr.microsoft.com/dotnet/sdk mcr.microsoft.com/dotnet/aspnet mcr.microsoft.com/dotnet/runtime → dotnet · oven/bun → bun · denoland/deno → deno · postgres → postgresql · supabase/postgres → supabase · mysql → mysql · mongo → mongodb · redis → redis · mcr.microsoft.com/mssql/server → sqlserver · rabbitmq → rabbitmq · elasticsearch docker.elastic.co/elasticsearch/elasticsearch → elasticsearch · apache/kafka bitnami/kafka confluentinc/cp-kafka → kafka · nats → nats · mariadb memcached valkey/valkey nginx caddy traefik httpd alpine debian ubuntu minio/minio axllent/mailpit localstack/localstack adminer dpage/pgadmin4 grafana/grafana jaegertracing/all-in-one clickhouse/clickhouse-server cassandra neo4j opensearchproject/opensearch quay.io/keycloak/keycloak → their own techs |

- [ ] **Step 1:** Controller creates one worktree per batch from the Task 1 commit (`git worktree add <scratchpad>/wt/6x -b s4/6x <sha>`), dispatches the 11 writers in parallel (haiku).
- [ ] **Step 2:** Per finished writer, dispatch its verifier (sonnet) in the same worktree.
- [ ] **Step 3:** Controller cherry-picks each verified batch onto `feat/learn-tech-catalog`, runs `cargo test -p hm-tech` after each pick, and adds the "cross-ecosystem keys" from the reports to the owning tech (one `fix(pack):` commit).

---

### Task 7: Whole-pack checks on the real catalog (sonnet; after Task 6)

**Files:** Modify `crates/hm-tech/tests/pack.rs` (Task 1 created it with the first test); Modify pack files only to fix what these tests reveal.

- [ ] **Step 1: Write tests:**
  - `embedded_pack_has_no_warnings`: `Catalog::embedded().warnings()` is empty (print them on failure).
  - `pack_size_floors`: `key_count` ≥ npm 270, pypi 100, go 65, maven 50, nuget 35, crates 55 (the "quality beats count" floor; a lower number needs a ruling).
  - `real_lookups`: npm `@radix-ui/react-dialog` → `radix-ui`; npm `react-dom` → `react`; npm `@types/node` → `type-definitions`; npm `next` → `nextjs`; pypi `Django` → a tech named `Django`; go `github.com/jackc/pgx/v5` → `postgresql`; go `gorm.io/driver/postgres` → the GORM tech; maven `org.springframework.boot:spring-boot-starter-web` → `spring-boot`; maven `org.springframework.boot:spring-boot-starter-security` → `spring-security`; nuget `microsoft.entityframeworkcore.sqlserver` → `sqlserver`; crates `serde_json` → a tech named `serde_json`; config `apps/web/next.config.ts` → `nextjs`; image `postgres:16-alpine` → `postgresql`; language `tsx` → `typescript`; npm `left-pad` → `None`.
  - `every_category_is_used`: every `Category::ALL` has at least one tech.
- [ ] **Step 2:** Run; fix pack data until green. **Step 3: Commit** `test(tech): check the embedded catalog as a whole`.

---

### Controller steps after Task 7 (not delegated)

- **Private coverage check** (brief): snapshot the private evaluation project per RULES §4 into the scratchpad, list every `dependencies`/`devDependencies` key of its `package.json` files, compute the share `Catalog::package(Npm, _)` covers, add well-known public packages that were missing (through a Task 6-style writer and verifier), run `tech_demo` on it, and write numbers to the private notes. Nothing from it is committed except generic public package entries.
- **Final review** on opus over the whole branch, then fix rounds as needed.
