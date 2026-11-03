# Harbormap Learn, Plan L2b part 1: Go and Python extraction and resolution — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `harbormap index` parses Go and Python files as well as TS/JS: it stores their symbols, imports, calls and class bases with evidence, resolves them across files without type checking (spec §6.2 rows Python and Go), and keeps the store equal to a cold index after every save.

**Architecture:** Two new language folders in `hm-index`, each shaped like `ts/`.
- `go/` holds the extractor and queries, `go.mod` loading and the resolver.
- `py/` holds the extractor and queries, package-root discovery and the resolver.

A new `langs.rs` dispatches extraction and resolution by language and holds all three module resolvers. `index.rs` calls it instead of the TS modules directly, and it learns three more ways a change reaches other files: Go package siblings, Go method names used by inferred calls, and Python name chains. Go and Python produce the first `Probably`/`inferred` edges.

**Tech Stack:** Rust 1.99.0 (edition 2024). New crates: `tree-sitter-go` 0.25.0 and `tree-sitter-python` 0.25.0 (both MIT, published 2025-08/09). Existing: tree-sitter 0.27.1, streaming-iterator, serde, rusqlite, proptest, tempfile.

**Spec:** `docs/superpowers/specs/2026-11-02-harbormap-learn-engine-design.md`. This plan implements §5.1 (files, symbols, edges, refs for Go and Python), §5.2 (evidence, the first `Probably` edges), §5.4 (dirty propagation for the new languages), §6.1 (two more grammars), §6.2 (Python and Go rows), §11.1 (wtf and fastapi-template backend as smoke tests) and the extraction and resolution part of the L2b entry of §14. L1's plan (`docs/superpowers/plans/2026-11-02-harbormap-learn-l1-indexer-core.md`) set the conventions this plan copies.

## Where this plan fits

| Plan | Delivers |
|---|---|
| L1 (done) | Indexer core, TS/JS facts and resolution, incremental updates, watcher, `index` and `card` |
| L2a (in parallel) | TS/JS routes and entries, Next.js, client → server matching, `package.json` manifests |
| **L2b part 1 (this plan)** | Go and Python extraction and resolution, incremental propagation for them, eval smoke tests |
| L2b part 2 (later) | Go/Python route and entry rules (FastAPI, Flask, Django, net/http including the `http.Server{Handler}` catch-all, Gin/Echo/Chi), `go.mod`/`pyproject` manifests; Java, C#, Rust |

**Scope decisions inside this plan** (each is a ruling; none removes a spec requirement):
- **No schema change.** The facts store keeps schema v1. `file.lang` is free text, so `go` and `python` need nothing new. Three optional fields are added to `FileFacts`, which lives in `file.facts_json`: `package` (the Go package clause), `labels` (inner names, below) and `attrs` (Python attribute types). Each is left out of the JSON when empty, so TS facts serialise exactly as before.
- **Inner names (spec §7.2 item 12).** Go function literals and Python nested functions are recorded as `labels` (owner symbol, name, lines), never as symbols. Their calls stay attributed to the enclosing symbol. TS labels are left to L3, which reads them from the tree (spec §7.2.12).
- **Go method calls without types.** `x.M()`, where `x` is neither a package nor the method's own receiver, links to the single method named `M` in the repository's Go code (`Probably`, `inferred`). Two or more methods named `M` give a ref. This is how "an interface call is inferred when exactly one implementation exists" is met without type checking. *Cost if wrong:* a call on an external type that shares a method name with exactly one project method becomes a wrong `Probably` edge (dashed, labelled "probably").
- **Go `imports` edges** go to every non-test `.go` file of the imported package's folder, because the package is the unit (spec §6.2). Package-level `var`/`const` are not symbols, struct embedding is not an `extends` edge, and `go.work` and `replace` directives are not read.
- **Python `self.x.y()`** is inferred only from a type the class states for `x`: a class-level annotation, `self.x: T`, `self.x = K(...)`, or `self.x = p` with `p: T` in the same method. The attribute name alone never picks a class. Without a stated type the call is a ref.
- **Python names.** `__all__` is not read; `from m import *` binds every top-level name of `m` that does not start with `_`. Of two top-level definitions with one name, the last one wins (Python's own rule). `self` and `cls` are recognised by name. Module-level assignments to a plain name that does not start with `_` are `Variable` symbols, as exported TS `const`s are.
- **`.pyi` stubs** are parsed as Python. Module lookup prefers `.py`, so a stub is used only when no `.py` module exists (a compiled extension).
- **Python package roots** come from the file layout: the repository root, every folder that holds `pyproject.toml`, `setup.py` or `setup.cfg`, and the `src/` folder of each of these. The contents of those files are not read in this plan.
- **Builtins and conversions make no ref.** Calls to Go builtins and predeclared types (`len`, `make`, `string(x)`), Go conversions to a type of the project, and calls to Python builtins (`print`, `len`, `ValueError(...)`) produce neither an edge nor a ref. This cuts noise in the ref counts that L1's private run reported.
- **Python decorators.** A call inside a decorator (`@router.get("/x")`) belongs to the scope that defines the decorated function: the file for a top-level function, the class for a method. The decorated symbol's lines include its decorators.
- **Skipped folders.** `.venv`, `venv`, `__pycache__`, `.tox`, `site-packages`, `.mypy_cache`, `.pytest_cache` and `.ruff_cache` join the never-entered folders, so an un-ignored virtualenv is counted as one folder, not walked.
- **Manifests are later.** `go.mod` `require` lines and `pyproject` dependencies are not read here. L3's technologies list (§7.6) and L2b part 2 need them.
- **Eval pins.** `wtf` is pinned at `05bc90c940d5f9e2490fc93cf467d9e8aa48ad63`. The fastapi backend reuses the `fastapi-template` name and commit with `subdir = "backend"`; `fetch.sh` leaves an existing checkout at the pinned commit alone, so the repository is fetched once.

## Before you start

- **Worktree.** `/Users/ibrahim/Desktop/CodeOrbit-wt/s3-l2b`, branch `feat/learn-l2b-go-python`. Touch only the files listed in your task.
- **Cargo** may not be on `PATH`: use `~/.cargo/bin/cargo`.
- **Commits.** `git commit -s`, and end every message with the trailer line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` exactly.
- **After every task**, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` stay clean.
- **Shared files** (other sessions change them too): edits to root `Cargo.toml`, `lib.rs`, `index.rs`, `facts.rs`, `hash.rs`, `ts/*`, `walk.rs`, `lang.rs`, `cli`/`index` code are additive and minimal. No restyling, no reordering, no drive-by refactors.

## Global Constraints

- **Toolchain and licences.** Toolchain `1.99.0`, edition `2024`; `unsafe_code = "forbid"`; clippy warnings fail. New dependencies are pinned exactly and must pass `deny.toml` (MIT is allowed). Both grammars must depend only on `tree-sitter-language ^0.1` and load under `tree-sitter` 0.27.1 (spec §6.1).
- **Zero tokens, no network.** Nothing here calls a network or an LLM. Fetched evaluation trees are read, never built or run.
- **Ids** (spec §5.3): `symbol_ids` in `ts/resolve.rs` stays the only place symbol ids are made, for every language.
- **Evidence** (spec §5.2): exact (one candidate) → `Sure`, `parsed`; name match → `Probably`, `inferred`; no candidate → a ref, never an edge. `conf` is stored as `sure`/`probably`.
- **No guessing.** A reference that does not name exactly one target is a ref.
- **Parse errors** (spec §6.1): a tree with errors is `parsed_with_errors`; the facts that parsed are kept.
- **One parse per file.** Each file is parsed once; all its queries run on that tree.
- **Determinism.** The same tree gives the same store rows; every fact list is in source order; every resolved list is sorted.
- **Incremental equals cold** (spec §11.3): after any edit, `Indexer::apply` leaves the store equal to a cold index of the same tree.
- **Latency targets** (spec §5.7, 1k files): body-only edit p95 ≤ 300 ms, signature edit p95 ≤ 500 ms, cold index ≤ 2 s. They must still hold for TS, and are measured for Go and Python.
- **Privacy.** No name, path, identifier or text of the user's private projects appears in any committed file. Committed fixtures are synthetic; smoke tests use the public pinned repositories.

## Review Focus

The spec does not list these inputs, but a person indexing a real Go or Python project will meet them. Each has a test in the task that owns the code.

1. **Several `package main` folders** (`cmd/a`, `cmd/b`) each define `main` and `run`: a call stays inside its own folder, never crossing to the other `run`. Task 5: `same_named_functions_in_two_main_packages_stay_apart`.
2. **An import path that shares a prefix with the module path but not a path boundary** (`example.com/shopping` with module `example.com/shop`) is not inside the module. Task 5: `module_prefix_matches_only_at_a_path_boundary`.
3. **A relative import with more dots than folders** (`from ....x import y` near the root) is an unresolved ref, never a panic or a path outside the root. Task 6: `a_relative_import_above_the_root_is_unresolved`.
4. **A save in the middle of typing** leaves a Go or Python syntax error; the functions that still parse are kept, with status `parsed_with_errors`. Task 3: `a_go_syntax_error_keeps_the_functions_that_parse`; Task 4: `a_python_syntax_error_keeps_the_functions_that_parse`.
5. **An un-ignored `.venv`** with thousands of library files is one skipped folder, not thousands of indexed files. Task 1: `virtualenv_and_cache_folders_are_skipped`.

---

## File map

| File | Responsibility |
|---|---|
| `Cargo.toml` (root), `crates/hm-index/Cargo.toml` | The two grammar crates |
| `crates/hm-index/src/lang.rs` | `.pyi` is Python; `Lang::parsed` |
| `crates/hm-index/src/walk.rs` | Python test/generated classes, skipped Python folders |
| `crates/hm-index/src/facts.rs` | `package`, `labels`, `attrs`; `FileFacts::holder` |
| `crates/hm-index/src/hash.rs` | `interface_hash` covers `package` and `attrs` |
| `crates/hm-index/src/ts/resolve.rs`, `ts/extract.rs` | `EdgeRow.inferred`; new `FileFacts` fields in the literal |
| `crates/hm-index/src/go/{mod.rs,extract.rs,queries/*.scm}` | Go extraction |
| `crates/hm-index/src/go/{gomod.rs,resolve.rs}` | `go.mod` modules, Go resolution |
| `crates/hm-index/src/py/{mod.rs,extract.rs,queries/*.scm}` | Python extraction |
| `crates/hm-index/src/py/{modules.rs,resolve.rs}` | Package roots and module lookup, Python resolution |
| `crates/hm-index/src/langs.rs` | Per-language dispatch: extract, resolvers, resolve, dependents |
| `crates/hm-index/src/index.rs` | Uses `langs`; evidence columns; new dependents |
| `crates/hm-index/src/synth_go.rs`, `synth_py.rs`, `examples/latency.rs` | Synthetic Go/Python projects; `--lang` |
| `crates/harbormap/src/index.rs` | Go and Python count as read |
| `crates/hm-index/tests/fixtures/{go-basic,py-basic}/**` | Fixture projects |
| `tests/eval/repos.toml`, `crates/hm-index/tests/eval_l2b.rs` | Pins and smoke tests |

## Task graph

| Wave | Tasks (parallel within a wave) | Needs |
|---|---|---|
| A | 1 (deps, lang, walk) · 2 (facts, hashes, evidence) | — |
| B | 3 (Go extract) · 4 (Python extract) | 1, 2 |
| C | 5 (Go resolve) · 6 (Python resolve) | 5 needs 3; 6 needs 4 |
| D | 7 (dispatch and incremental) | 5, 6 |
| E | 8 (synthetic projects, property test, latency) · 9 (eval) | 7 |

---

### Task 1: Grammar crates, `Lang`, and walker classes

**Files:**
- Modify: `Cargo.toml` (root `[workspace.dependencies]`, append two lines), `crates/hm-index/Cargo.toml`
- Modify: `crates/hm-index/src/lang.rs`, `crates/hm-index/src/walk.rs`
- Test: `crates/hm-index/src/lang.rs` (unit tests), `crates/hm-index/tests/walk.rs`

**Interfaces:**
- Produces:
  - Root: `tree-sitter-go = "0.25.0"` and `tree-sitter-python = "0.25.0"`, appended after `tree-sitter-typescript`; `hm-index` gains `tree-sitter-go.workspace = true` and `tree-sitter-python.workspace = true`.
  - `Lang::from_path`: `"pyi"` → `Lang::Python`.
  - `pub fn parsed(&self) -> bool` on `Lang`: true for TypeScript, Tsx, JavaScript, Python and Go. `parsed_in_l1` stays as it is.
  - `classify`: `*_test.py` and `conftest.py` → `Test`; `*_pb2_grpc.py` and `*_pb2.pyi` → `Generated` (`*_pb2.py`, `*_test.go`, `test_*.py`, `*.pb.go` and the `Code generated` marker exist already).
  - `SKIP_DIRS` gains `.venv`, `venv`, `__pycache__`, `.tox`, `site-packages`, `.mypy_cache`, `.pytest_cache`, `.ruff_cache`, appended at the end.

- [ ] **Step 1: Write the failing tests.** In `lang.rs`: `Lang::from_path("a.pyi") == Some(Lang::Python)`, and `parsed_marks_ts_js_python_and_go` asserting `[true, true, true, true, true, false, false, false]` over the same order as the existing test. In `tests/walk.rs`:
  - `python_test_and_generated_files_are_classed`: `classify("pkg/models_test.py", b"")` and `classify("tests/conftest.py", b"")` and `classify("conftest.py", b"")` are `Test`; `classify("api/v1/user_pb2_grpc.py", b"")` and `classify("api/v1/user_pb2.pyi", b"")` are `Generated`; `classify("cmd/x/main.go", b"// Code generated by stringer. DO NOT EDIT.\n\npackage main\n")` is `Generated`.
  - `virtualenv_and_cache_folders_are_skipped`: a temp tree with `app/main.py`, `.venv/lib/python3.12/site-packages/x/__init__.py`, `venv/y.py`, `app/__pycache__/main.cpython-312.pyc`, `.tox/z.py`; `walk` lists only `app/main.py`, and `skipped_dirs` holds `.venv`, `venv`, `__pycache__` and `.tox` with count 1 each.
- [ ] **Step 2: Run** `cargo test -p hm-index --lib lang && cargo test -p hm-index --test walk`. Expected: FAIL.
- [ ] **Step 3: Implement** the four changes above. Run `cargo tree -p hm-index -i tree-sitter-language` and check a single `0.1.x` version is in the tree.
- [ ] **Step 4: Run** the same tests. Expected: PASS, and `cargo test --workspace` stays green (Python files are still not parsed: `index.rs` keeps calling `parsed_in_l1` until Task 7).
- [ ] **Step 5: Commit** `feat(index): add the Go and Python grammars and their file classes`

---

### Task 2: Facts fields, interface hash and edge evidence

**Files:**
- Modify: `crates/hm-index/src/facts.rs`, `crates/hm-index/src/hash.rs`
- Modify: `crates/hm-index/src/ts/extract.rs` (the `FileFacts` literal only), `crates/hm-index/src/ts/resolve.rs` (`EdgeRow` and the one place it is built)
- Modify: `crates/hm-index/src/index.rs` (evidence columns only), `crates/hm-index/src/lib.rs`
- Create: `crates/hm-index/src/go/mod.rs`, `crates/hm-index/src/py/mod.rs` (a `//!` doc line each, nothing else)
- Test: `crates/hm-index/tests/facts_model.rs` (new), unit test in `index.rs`

**Interfaces:**
- Produces (`facts.rs`; same derives as the other facts):

  ```rust
  /// A named nested function or closure inside a symbol (spec §7.2 item 12). It is a
  /// label for the lines it spans, not a symbol.
  pub struct LabelFact { pub owner: Option<(String, String)>, pub name: String, pub l0: u32, pub l1: u32 }
  /// The type a class states for one of its attributes, as written (`Repo`, `models.Repo`).
  pub struct AttrFact { pub class: String, pub attr: String, pub type_name: String, pub line: u32 }
  // FileFacts gains, each `#[serde(default, skip_serializing_if = ...)]` (Option::is_none / Vec::is_empty):
  pub package: Option<String>,     // the Go package clause
  pub labels: Vec<LabelFact>,
  pub attrs: Vec<AttrFact>,
  impl FileFacts {
      /// The index of the symbol that holds a call at `line` whose fact names `(owner, name)`:
      /// of the functions, methods and classes with that pair that span the line, the one
      /// with the smallest span; `None` when there is none or the smallest span is shared.
      pub fn holder(&self, owner: &str, name: &str, line: u32) -> Option<usize>;
  }
  ```

  `FileFacts::empty` and the TS literal set the new fields empty.
- `hash::interface_hash` adds the key `p\u{1f}{package}` when `package` is set and `a\u{1f}{class}\u{1f}{attr}\u{1f}{type_name}` per attr, so it is unchanged for TS facts. `facts_hash` is not changed.
- `ts::resolve::EdgeRow` gains `pub inferred: bool` (doc: "a `Probably` edge with provenance `inferred`; `false` means `Sure` and `parsed`"). The TS resolver sets `false`.
- `index.rs`: `const PROV_INFERRED: &str = "inferred";`, `const CONF_PROBABLY: &str = "probably";` and `fn evidence(e: &EdgeRow) -> (&'static str, &'static str)` giving `(prov, conf)`. `replace_resolved` and `sync_resolved` write it, and `sync_resolved`'s difference key becomes `(kind, src, dst, line, rule, prov, conf)` (replace the comment that says L2 must add them).
- `lib.rs`: `pub mod go;` and `pub mod py;` in alphabetical place.

- [ ] **Step 1: Write the failing tests** (`tests/facts_model.rs`):
  - `ts_facts_json_has_no_new_keys_and_old_json_reads`: the JSON of `ts::extract("export function f() {}", Lang::TypeScript)` contains none of `"package"`, `"labels"`, `"attrs"`; `serde_json::from_str::<FileFacts>` of that JSON gives equal facts.
  - `package_and_attrs_change_the_interface_hash`: start from `FileFacts::empty(ParseStatus::Parsed)`; setting `package = Some("shop")` changes `interface_hash`, and so does pushing an `AttrFact`; pushing a `LabelFact` does not.
  - `holder_picks_the_smallest_span_and_refuses_a_tie`: two symbols with `("C", "m")` spanning lines 1–10 and 3–4 give `holder("C", "m", 3) == Some(1)`; two `("", "f")` symbols both spanning 5–5 give `None`; a `Variable` symbol never holds.
  - In `index.rs` tests: `the_confidence_constants_are_how_the_model_spells_them` asserts `CONF_PROBABLY` equals the serde value of `Confidence::Probably` (next to the existing `Sure` check).
- [ ] **Step 2: Run** `cargo test -p hm-index --test facts_model`. Expected: FAIL (fields missing).
- [ ] **Step 3: Implement** as listed.
- [ ] **Step 4: Run** `cargo test -p hm-index`. Expected: PASS, every L1 test unchanged.
- [ ] **Step 5: Commit** `feat(index): add Go package, inner-name labels and attribute types to the facts, and store edge evidence`

---

### Task 3: Go extractor and the `go-basic` fixture

**Files:**
- Modify: `crates/hm-index/src/go/mod.rs`
- Create: `crates/hm-index/src/go/extract.rs`, `crates/hm-index/src/go/queries/{symbols,imports,calls}.scm`
- Create: `crates/hm-index/tests/fixtures/go-basic/**` (below)
- Test: `crates/hm-index/tests/go_extract.rs`

**Interfaces:**
- Consumes: Task 1 (grammar), Task 2 (`package`, `labels`).
- Produces: `go::extract(src: &str) -> FileFacts` (`go/mod.rs`: `pub mod extract; pub use extract::extract;`). Shape it like `ts/extract.rs`: queries compiled once (`OnceLock`), one parser per thread, query matches through `StreamingIterator`, one parse.

**Extraction rules:**

| Fact | Syntax |
|---|---|
| `package` | the `package_clause` name |
| Function | `func_declaration`; `exported` = the name starts with an upper-case letter |
| Method | `method_declaration`; owner = the receiver's type name without `*` and without type parameters (`(l *List[T])` → `List`); `exported` by the same rule |
| Interface | `type_spec` whose type is an `interface_type` |
| Class | every other `type_spec` and `type_alias` (struct and named types; methods attach to them) |
| Import | each `import_spec`: `specifier` = the path without quotes; `names` = `[{imported: "*", local: <alias>}]` with the alias as written (`web`, `.`, `_`), or `[{imported: "*", local: ""}]` without one (the resolver fills in the package's name); kind `Static` |
| Call | `call_expression` with an `identifier` function (callee = it, no receiver) or a `selector_expression` function (callee = field, receiver = operand text, collapsed, cut to 80 characters); `args` = named children of `argument_list`; calls whose function is anything else (generic instantiation, a call result, a func literal) are skipped |
| Label | a `func_literal` that is (a) a right-hand value of a `short_var_declaration`, `assignment_statement` or `var_spec` (name = the left identifier at the same position), (b) any argument of a call that is such a right-hand value (`h := http.HandlerFunc(func(...) {...})` → `h`), or (c) the value of a `keyed_element` (name = the key). Owner = the enclosing symbol, `None` at top level; lines = the func literal's |
| Branches | `if_statement`, `for_statement`, `expression_case`, `type_case`, `communication_case`, and `binary_expression` with `&&` or `||` |

- `sig` = the declaration from `func`/the type name to the body start, comments removed, whitespace collapsed; a type's `sig` is its whole `type_spec` and its `body_hash` is over the `type_spec` (as TS interfaces do). `sig_hash` = `hash_parts(&[kind, owner, name, sig])`.
- Only top-level declarations are symbols. Calls inside function literals belong to the enclosing symbol.
- `heritage` and `exports` stay empty for Go.

**Fixture `tests/fixtures/go-basic/`** (Go files use tabs):

`go.mod`
```
module example.com/shop

go 1.22
```

`shop.go`
```go
package shop

import "errors"

// ErrBad is returned for an order with a negative total.
var ErrBad = errors.New("bad order")

type Order struct {
	ID    int
	Total int
}

type OrderStore interface {
	Save(o *Order) error
	Find(id int) (*Order, error)
}

type Notifier interface {
	Notify(msg string)
}

func Validate(o *Order) error {
	if o.Total < 0 {
		return ErrBad
	}
	return nil
}
```

`store/memory.go`
```go
package store

import "example.com/shop"

type Memory struct {
	orders map[int]*shop.Order
}

func NewMemory() *Memory {
	return &Memory{orders: make(map[int]*shop.Order)}
}

func (m *Memory) Save(o *shop.Order) error {
	m.orders[o.ID] = o
	return nil
}

func (m *Memory) Find(id int) (*shop.Order, error) {
	return m.lookup(id), nil
}

func (m *Memory) lookup(id int) *shop.Order {
	return m.orders[id]
}
```

`store/memory_test.go`
```go
package store

import "testing"

func TestSave(t *testing.T) {
	m := NewMemory()
	if err := m.Save(nil); err != nil {
		t.Fatal(err)
	}
	helper(t)
}

func helper(t *testing.T) {}
```

`notify/email.go`
```go
package notify

type Email struct{}

func (e Email) Notify(msg string) {
	send(msg)
}
```

`notify/sms.go`
```go
package notify

import "fmt"

type SMS struct{}

func (s *SMS) Notify(msg string) {
	send("sms: " + msg)
}

func send(msg string) {
	fmt.Println(msg)
}
```

`http/server.go` (the folder is `http`, the package is `web`)
```go
package web

import (
	"net/http"

	"example.com/shop"
)

type Server struct {
	Store    shop.OrderStore
	Notifier shop.Notifier
	mux      *http.ServeMux
}

func NewServer(st shop.OrderStore) *Server {
	s := &Server{Store: st, mux: http.NewServeMux()}
	s.routes()
	return s
}

func (s *Server) routes() {
	s.mux.HandleFunc("/orders", s.handleOrder)
}

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	s.handleOrder(w, r)
}

func (s *Server) handleOrder(w http.ResponseWriter, r *http.Request) {
	o := &shop.Order{}
	if err := shop.Validate(o); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	s.Store.Save(o)
	s.Notifier.Notify("saved")
	writeJSON(w, o)
}
```

`http/json.go`
```go
package web

import (
	"encoding/json"
	"net/http"
)

func writeJSON(w http.ResponseWriter, v any) {
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(v)
}
```

`cmd/shopd/main.go`
```go
package main

import (
	"log"
	"net/http"

	"example.com/shop/http"
	"example.com/shop/store"
)

func main() {
	st := store.NewMemory()
	srv := web.NewServer(st)
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		log.Printf("%s %s", r.Method, r.URL.Path)
		srv.ServeHTTP(w, r)
	})
	log.Fatal(http.ListenAndServe(":8080", handler))
}
```

`gen/api.pb.go`
```go
// Code generated by protoc-gen-go. DO NOT EDIT.

package gen

func Hello() string { return "hi" }
```

- [ ] **Step 1: Write the failing tests** (`tests/go_extract.rs`, reading fixtures with `common::read_fixture`):
  - `the_go_grammar_loads_under_the_pinned_core`: `go::extract("package a\n\nfunc F() {}\n").status == ParseStatus::Parsed`.
  - `server_go_yields_its_package_symbols_and_calls`, on `go-basic/http/server.go`:
    - `package == Some("web")`;
    - symbols `(owner, name, kind)` in order: `("", "Server", Class)`, `("", "NewServer", Function)`, `("Server", "routes", Method)`, `("Server", "ServeHTTP", Method)`, `("Server", "handleOrder", Method)`; `NewServer` has `(l0, l1, exported) == (15, 19, true)`, `routes` has `exported == false`;
    - imports: `[("net/http", ""), ("example.com/shop", "")]` as `(specifier, names[0].local)`;
    - calls contain `("routes", Some("s"), 17)`, `("handleOrder", Some("s"), 26)`, `("Validate", Some("shop"), 31)`, `("Save", Some("s.Store"), 35)`, `("Notify", Some("s.Notifier"), 36)` and `("writeJSON", None, 37)`; the `writeJSON` call has `owner == Some(("Server", "handleOrder"))` and `args == 2`;
    - `branches` of `handleOrder` is 1.
  - `kinds_and_receivers_in_shop_and_store`: `shop.go` gives `Order` Class, `OrderStore` and `Notifier` Interface, `Validate` Function; `ErrBad` is not a symbol. `store/memory.go` gives `("Memory", "Save", Method)` and `("Memory", "lookup", Method)` with sig `func (m *Memory) lookup(id int) *shop.Order`.
  - `a_closure_assigned_to_a_name_is_a_label`, on `cmd/shopd/main.go`: `labels == [LabelFact { owner: Some(("", "main")), name: "handler", l0: 14, l1: 17 }]`; the call `ServeHTTP` (receiver `srv`, line 16) has owner `("", "main")`; the import of `example.com/shop/http` has local `""`.
  - `labels_cover_keyed_values_and_skip_unnamed_literals`: inline source with `x := T{Run: func() {}}`, `go func() {}()` and `var h = func() {}` at top level gives labels `Run` (owner the enclosing function) and `h` (owner `None`), and nothing for the `go` statement.
  - `aliases_dot_and_blank_imports_keep_their_names`: `import (web "a/http"; . "a/b"; _ "a/c")` gives locals `web`, `.`, `_`.
  - `a_go_syntax_error_keeps_the_functions_that_parse`: `"package a\n\nfunc Ok() {}\n\nfunc Half( {\n"` is `ParsedWithErrors` and keeps `Ok`.
  - `a_comment_edit_keeps_the_go_body_hash`: adding `// note` inside a body leaves `body_hash` and `sig_hash` equal; changing a string literal changes `body_hash` only.
- [ ] **Step 2: Run** `cargo test -p hm-index --test go_extract`. Expected: FAIL. If `the_go_grammar_loads_under_the_pinned_core` fails with a language-version error after the implementation, stop and report it (spec §15 risk; the fix is the maintainer's call).
- [ ] **Step 3: Implement** `go/extract.rs` and the three queries. Find a call's enclosing symbol by byte containment over the symbols (innermost wins), as TS does.
- [ ] **Step 4: Run** `cargo test -p hm-index --test go_extract`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(index): extract Go symbols, imports, calls and inner-name labels`

---

### Task 4: Python extractor and the `py-basic` fixture

**Files:**
- Modify: `crates/hm-index/src/py/mod.rs`
- Create: `crates/hm-index/src/py/extract.rs`, `crates/hm-index/src/py/queries/{symbols,imports,calls}.scm`
- Create: `crates/hm-index/tests/fixtures/py-basic/**` (below)
- Test: `crates/hm-index/tests/py_extract.rs`

**Interfaces:**
- Consumes: Task 1 (grammar), Task 2 (`labels`, `attrs`).
- Produces: `py::extract(src: &str) -> FileFacts`, shaped like `go::extract`.

**Extraction rules:**

| Fact | Syntax |
|---|---|
| Function | a top-level `function_definition` (also `async`, also inside `decorated_definition`); `exported` = the name does not start with `_` |
| Class, Method | a top-level `class_definition`; each `function_definition` directly in its body is a Method with owner = the class (`exported = false`). Nested classes are not symbols |
| Variable | a top-level `expression_statement` holding an `assignment` whose left side is one `identifier` that does not start with `_` (`log = ...`, `X: int = 1`); a `lambda` value makes it a Function instead |
| Range | a decorated function or class spans from its first decorator; `sig` = the text from that start to the body `block`, comments removed, whitespace collapsed; a Variable's `sig` is the text up to its value |
| Import | `import a.b` → `{specifier: "a.b", names: [{"*", "a"}]}`; `import a.b as m` → `[{"*", "m"}]`; `from M import n as k` → `{specifier: "M", names: [{"n", "k"}]}` with `M` as written, dots included (`.`, `..pkg`); `from M import *` → `[{"*", "*"}]`; one fact per dotted name of a multi-name `import`; `from __future__ import ...` gives nothing; kind `Static` |
| Call | `call` with an `identifier` function (no receiver) or an `attribute` function (callee = attribute name, receiver = object text, collapsed, cut to 80 characters); `args` = named children of `argument_list` |
| Heritage | each positional base of a class (`identifier` or `attribute`, as text); keyword arguments (`metaclass=`, `table=True`) are skipped |
| Attr | in a class: a class-body `x: T` or `x: T = ...`; in its methods: `self.x: T = ...`, `self.x = K(...)` (type = the callee text `K` or `m.K`), `self.x = p` where `p` is a parameter of that method annotated `T`. A type is used when it is an `identifier` or `attribute`, or `Optional[T]` / `T \| None` / `None \| T` around one (then `T`); any other annotation is skipped |
| Label | a `function_definition` nested in a function or method body (name = its name), or `name = lambda ...` inside a function body; owner = the enclosing symbol |
| Branches | `if_statement`, `elif_clause`, `for_statement`, `while_statement`, `except_clause`, `conditional_expression`, `boolean_operator`, `case_clause`, `if_clause` |

- A call inside a `decorator` gets as owner the symbol around the decorated definition: the class for a method, `None` for a top-level function. Every other call's owner is the innermost enclosing symbol.
- `exports` stays empty for Python; `package` stays `None`.

**Fixture `tests/fixtures/py-basic/`** (4-space indents):

`pyproject.toml`
```toml
[project]
name = "shop"
version = "0.1.0"
```

`src/shop/__init__.py`
```python
from .service import place_order
from .models import Order as Order
```

`src/shop/models.py`
```python
class Base:
    def save(self):
        return self.validate()

    def validate(self):
        return True


class Order(Base):
    def __init__(self, total: int):
        self.total = total

    def validate(self):
        return self.total >= 0

    def persist(self):
        return self.save()
```

`src/shop/repo.py`
```python
from .models import Order


class Repo:
    def add(self, order: Order) -> None:
        order.save()

    def all(self):
        return []
```

`src/shop/mailer.py`
```python
class Mailer:
    def send(self, order):
        print(order)
```

`src/shop/service.py`
```python
import logging

from shop.repo import Repo
from . import models
from .mailer import Mailer

log = logging.getLogger(__name__)


class Service:
    repo: Repo

    def __init__(self, mailer: Mailer):
        self.repo = Repo()
        self.mailer = mailer

    def place(self, total):
        order = models.Order(total)
        self.repo.add(order)
        self.mailer.send(order)
        self._audit()
        return order

    def _audit(self):
        log.info("placed")


def place_order(total):
    def check(t):
        return t > 0

    if not check(total):
        raise ValueError("bad total")
    return Service(Mailer()).place(total)
```

`src/shop/fast.pyi`
```python
def speedup(x: int) -> int: ...
```

`src/shop/api.py`
```python
import shop.models
from shop import Order, place_order
from shop.fast import speedup


def route(path):
    def wrap(fn):
        return fn

    return wrap


@route("/orders")
def create(total):
    order = place_order(speedup(total))
    shop.models.Order(total)
    return Order
```

`src/shop/api_test.py`
```python
from shop.api import create


def test_create():
    create(1)
```

`tests/test_service.py`
```python
from shop.service import Service


def test_place():
    Service(None).place(1)
```

`tests/conftest.py`
```python
import pytest


@pytest.fixture
def service():
    return None
```

`gen/shop_pb2.py`
```python
# Generated by the protocol buffer compiler.  DO NOT EDIT!
DESCRIPTOR = None
```

- [ ] **Step 1: Write the failing tests** (`tests/py_extract.rs`):
  - `the_python_grammar_loads_under_the_pinned_core`: `py::extract("def f():\n    pass\n").status == Parsed`.
  - `service_py_yields_symbols_imports_calls_attrs_and_labels`, on `py-basic/src/shop/service.py`:
    - symbols in order: `("", "log", Variable)`, `("", "Service", Class)`, `("Service", "__init__", Method)`, `("Service", "place", Method)`, `("Service", "_audit", Method)`, `("", "place_order", Function)`; `place_order` is `(28, 34, exported true)`;
    - imports `(specifier, names)`: `("logging", [*→logging])`, `("shop.repo", [Repo→Repo])`, `(".", [models→models])`, `(".mailer", [Mailer→Mailer])`;
    - calls contain `("Repo", None, 14)`, `("Order", Some("models"), 18)`, `("add", Some("self.repo"), 19)`, `("send", Some("self.mailer"), 20)`, `("_audit", Some("self"), 21)`, `("info", Some("log"), 25)`, `("check", None, 32)`, `("ValueError", None, 33)`, `("place", Some("Service(Mailer())"), 34)`;
    - `attrs` = `[(Service, repo, Repo, 11), (Service, repo, Repo, 14), (Service, mailer, Mailer, 15)]` as `(class, attr, type_name, line)`;
    - `labels` = `[LabelFact { owner: Some(("", "place_order")), name: "check", l0: 29, l1: 30 }]`.
  - `models_py_has_heritage_and_init_attrs`: `heritage == [HeritageFact { class: "Order", extends: "Base", line: 9 }]`; attrs hold `(Order, total, int, 11)`.
  - `decorator_calls_belong_to_the_defining_scope`, on `api.py`: `create` spans 13–17; the call `route` at line 13 has `owner == None`; labels hold `route › wrap` (7–8); the import of `shop.models` binds `*→shop`.
  - `bases_skip_keywords_and_annotations_unwrap_optional`: inline `class U(Base, mixins.M, table=True):\n    a: Optional[Repo] = None\n    b: "Repo"\n    c: Repo | None\n` gives heritage `Base`, `mixins.M` and attrs `a→Repo`, `c→Repo`, none for `b`.
  - `relative_star_and_future_imports`: `from __future__ import annotations\nfrom ..pkg.mod import a as b, c\nfrom x import *\nimport os, sys as system\n` gives `("..pkg.mod", [a→b, c→c])`, `("x", [*→*])`, `("os", [*→os])`, `("sys", [*→system])` and nothing for `__future__`.
  - `a_python_syntax_error_keeps_the_functions_that_parse`: `"def ok():\n    return 1\n\ndef half(:\n"` is `ParsedWithErrors` and keeps `ok`.
  - `a_stub_file_parses`: `src/shop/fast.pyi` gives Function `speedup` with sig `def speedup(x: int) -> int:`.
- [ ] **Step 2: Run** `cargo test -p hm-index --test py_extract`. Expected: FAIL. Stop and report a grammar-version error, as in Task 3.
- [ ] **Step 3: Implement** `py/extract.rs` and the queries.
- [ ] **Step 4: Run** `cargo test -p hm-index --test py_extract`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(index): extract Python symbols, imports, calls, attribute types and inner-name labels`

---

### Task 5: Go modules and Go resolution

**Files:**
- Modify: `crates/hm-index/src/go/mod.rs` (add `pub mod gomod; pub mod resolve;`)
- Create: `crates/hm-index/src/go/gomod.rs`, `crates/hm-index/src/go/resolve.rs`
- Test: `crates/hm-index/tests/go_resolve.rs`

**Interfaces:**
- Consumes: Task 3 (`go::extract`, fixture); `ts::resolve::{EdgeRow, RefRow, ResolvedFile, symbol_ids}`; `FileFacts::holder`.
- Produces:

  ```rust
  // gomod.rs
  pub fn module_path(go_mod: &str) -> Option<String>;           // the `module` line; quotes and `//` comments allowed
  pub enum GoTarget { Package(String /* folder */), External(String /* import path */), Unresolved }
  pub struct GoModules { /* known files, (folder, module path) pairs, notes */ }
  impl GoModules {
      pub fn new(known: BTreeSet<String>, modules: BTreeMap<String /* go.mod folder */, String /* module path */>, notes: Vec<String>) -> GoModules;
      pub fn load(root: &Path, known: &BTreeSet<String>) -> GoModules; // reads every known `go.mod`
      pub fn resolve(&self, import_path: &str) -> GoTarget;
      pub fn files_in(&self, folder: &str) -> Vec<&str>;               // `.go` files directly in it, sorted
      pub fn config_sources(&self) -> BTreeSet<String>;                // every `go.mod` path
      pub fn notes(&self) -> &[String];
  }
  // resolve.rs
  pub struct MethodIndex { /* method name -> [(file, symbol index)] */ }
  impl MethodIndex { pub fn build(facts: &BTreeMap<String, FileFacts>) -> MethodIndex; } // over `.go` files only
  pub struct GoProject<'a> { pub facts: &'a BTreeMap<String, FileFacts>, pub modules: &'a GoModules, pub methods: &'a MethodIndex }
  pub fn resolve_file(p: &GoProject, rel: &str) -> ResolvedFile;
  pub fn package_siblings(facts: &BTreeMap<String, FileFacts>, rel: &str) -> Vec<String>; // other `.go` files with facts in rel's folder
  pub fn method_names(facts: &FileFacts) -> BTreeSet<String>;                             // names of its Method symbols
  pub fn calls_any_method(facts: &FileFacts, names: &BTreeSet<String>) -> bool;           // a call with a receiver whose callee is in names
  ```

**Module rules:**
- `resolve(P)`: of the modules whose path equals `P` or is a prefix of `P` followed by `/`, take the longest; the folder is the `go.mod` folder joined with the rest of `P`. A folder with at least one non-test `.go` file is `Package`; otherwise `Unresolved`. A path under no module is `External(P)`.
- A `go.mod` without a `module` line adds the note `go.mod at <path> has no module line` and is not a module.

**Resolution rules** (the caller's *package* = the `.go` files with facts in its folder whose `package` equals its own; `_test.go` files count only when the caller is one):

| Fact | Resolution | Rule | Evidence |
|---|---|---|---|
| `import "P"`, `P` a `Package` | one `imports` edge per non-test `.go` file with facts in the folder; all of them go to `imports_files` | `go.import` | Sure |
| `import "P"`, `External` / `Unresolved` | ref `import` with text `P`, status `external` / `unresolved` | | |
| Import local name | the alias; or the `package` of the target's non-test files (first file in path order); for an external path, its last segment after dropping a final `vN` segment, then without a leading `go-` and a trailing `-go`, `.vN` or `.go` | | |
| `F()` | a Function `F` of the caller's package; else of a dot-imported package | `go.call.local` / `go.call.package` | Sure |
| `p.F()`, `p` an internal import's local name | Function `F` in that package's non-test files | `go.call.package` | Sure |
| `r.M()`, `r` the receiver variable of the enclosing method (read from its `sig`: `func (r *T)`) | Method `M` with owner `T` in the caller's package | `go.call.receiver` | Sure |
| any other `x.M()` (fields `s.Store.M`, locals, `p.Var.M`, and `r.M()` not found on `T`) | the single Method named `M` in all `.go` files, minus `_test.go` files outside the caller's folder | `go.call.method` | Probably |
| Builtins (`append cap clear close complex copy delete imag len make max min new panic print println real recover`), predeclared types (`any bool byte comparable complex64 complex128 error float32 float64 int int8 int16 int32 int64 rune string uint uint8 uint16 uint32 uint64 uintptr`), and `T(x)` / `p.T(x)` where `T` is a Class or Interface of the package | nothing | | |
| `p.F()`, `p.X.M()` with `p` an external import; a receiver whose root identifier is an external import | ref `call` with text `receiver.callee`, status `external` | | |
| anything else | ref `call`, status `unresolved` | | |

- A call's source is `facts.holder(owner, name, line)`'s symbol id, or the file id when there is none (as TS does).
- `MethodIndex::build` lists Method symbols of `.go` files only, with their files, so one build serves every file of a pass.

- [ ] **Step 1: Write the failing tests** (`tests/go_resolve.rs`; a local helper walks the fixture, runs `go::extract` on every `.go` file, builds `GoModules::load` and `MethodIndex::build`, resolves every Go file, and names ids with `display_name` as `(kind, src, dst, rule, inferred)`; refs as `(file, kind, text, status)`):
  - `go_basic_resolves_package_receiver_and_method_calls` asserts these edges:
    - `imports store/memory.go → shop.go` (`go.import`), and `imports cmd/shopd/main.go → http/server.go` and `→ http/json.go`;
    - `calls http/server.go#Server.handleOrder → shop.go#Validate` (`go.call.package`, Sure);
    - `calls http/server.go#Server.handleOrder → http/json.go#writeJSON` (`go.call.local`, Sure);
    - `calls http/server.go#Server.ServeHTTP → http/server.go#Server.handleOrder` (`go.call.receiver`);
    - `calls store/memory.go#Memory.Find → store/memory.go#Memory.lookup` (`go.call.receiver`);
    - `calls notify/email.go#Email.Notify → notify/sms.go#send` (`go.call.local`, across files);
    - `calls cmd/shopd/main.go#main → store/memory.go#NewMemory` and `→ http/server.go#NewServer` (`go.call.package`; `web` comes from the package clause);
    - `calls http/server.go#NewServer → http/server.go#Server.routes`, `calls http/server.go#Server.handleOrder → store/memory.go#Memory.Save`, `calls cmd/shopd/main.go#main → http/server.go#Server.ServeHTTP` (all `go.call.method`, `inferred == true`);
    - `calls store/memory_test.go#TestSave → store/memory.go#NewMemory` (`go.call.local`) and `→ store/memory_test.go#helper`;
    
    and these refs: `(http/server.go, call, s.Notifier.Notify, unresolved)` (two implementations), `(http/server.go, call, s.mux.HandleFunc, unresolved)`, `(http/server.go, call, http.Error, external)`, `(http/server.go, import, net/http, external)`, `(http/json.go, call, json.NewEncoder, external)`. No ref or edge names `make`, and no edge leaves `http/server.go` for `shop.go#Order`.
  - `module_prefix_matches_only_at_a_path_boundary`: with modules `{"" → "example.com/shop", "tools" → "example.com/shop/tools"}`, `resolve("example.com/shopping/x")` is `External`, `resolve("example.com/shop/tools/gen")` is `Package("tools/gen")` (longest module) when `tools/gen/a.go` is known, and `resolve("example.com/shop/missing")` is `Unresolved`.
  - `same_named_functions_in_two_main_packages_stay_apart`: inline facts for `cmd/a/main.go` and `cmd/b/main.go`, both `package main` with `main` calling `run()`, and only `cmd/b` defining `run`: `cmd/b`'s call is an edge to `cmd/b/main.go#run`; `cmd/a`'s is a ref `run` (`unresolved`).
  - `a_test_package_and_dot_imports`: a `package shop_test` file in the root folder importing `example.com/shop` resolves `shop.Validate()` through the import, not as a local call; a file with `. "example.com/shop/store"` resolves `NewMemory()` with `go.call.package`.
  - `a_method_name_on_two_types_is_a_ref_and_test_methods_stay_home`: two `Close` methods on different types give a ref; a `Close` method in `x/fake_test.go` does not count for a caller in `y/`, so a single non-test `Close` gives an edge there.
  - `go_mod_without_a_module_line_is_noted`: `GoModules::load` on a temp tree whose `go.mod` holds only `go 1.22` has `notes() == ["go.mod at go.mod has no module line"]`.
- [ ] **Step 2: Run** `cargo test -p hm-index --test go_resolve`. Expected: FAIL.
- [ ] **Step 3: Implement** `gomod.rs` and `resolve.rs`. Collect edges and refs in sorted sets as the TS resolver does (first rule wins on an equal key).
- [ ] **Step 4: Run** `cargo test -p hm-index --test go_resolve`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(index): resolve Go imports by go.mod and calls by package, receiver and method name`

---

### Task 6: Python package roots and Python resolution

**Files:**
- Modify: `crates/hm-index/src/py/mod.rs` (add `pub mod modules; pub mod resolve;`)
- Create: `crates/hm-index/src/py/modules.rs`, `crates/hm-index/src/py/resolve.rs`
- Test: `crates/hm-index/tests/py_resolve.rs`

**Interfaces:**
- Consumes: Task 4 (`py::extract`, fixture); `ts::resolve::{EdgeRow, RefRow, ResolvedFile, symbol_ids}`; `FileFacts::holder`.
- Produces:

  ```rust
  // modules.rs
  /// A module or package: its file (`a/b.py`, `a/b/__init__.py`) and, for a package, its folder.
  pub struct PyModule { pub file: Option<String>, pub package_dir: Option<String> }
  pub enum PyTarget { Module(PyModule), External(String /* first segment */), Unresolved }
  pub struct PyModules { /* known files, roots */ }
  impl PyModules {
      pub fn new(known: BTreeSet<String>) -> PyModules;
      pub fn roots(&self) -> &[String];                                      // sorted; "" is the repository root
      pub fn resolve(&self, from_rel: &str, specifier: &str) -> PyTarget;    // "a.b", ".", "..x"
      pub fn submodule(&self, package_dir: &str, name: &str) -> Option<PyModule>;
  }
  // resolve.rs
  pub struct PyProject<'a> { pub facts: &'a BTreeMap<String, FileFacts>, pub modules: &'a PyModules }
  pub fn resolve_file(p: &PyProject, rel: &str) -> ResolvedFile;
  /// Whether resolving a name in `rel` can lead on into `target`: one of its imports resolves to it.
  pub fn forwards(p: &PyProject, rel: &str, target: &str) -> bool;
  ```

**Module rules:**
- **Roots:** `""`; each folder holding a known `pyproject.toml`, `setup.py` or `setup.cfg`; and `<that folder>/src` (and `src` under the repository root) when a known `.py`/`.pyi` file lies under it. For a file, the roots that contain it are tried first, deepest first, then the others in sorted order.
- **A dotted path `P` under root `R`:** `R/P/__init__.py`, `R/P.py`, `R/P/__init__.pyi`, `R/P.pyi`; otherwise a namespace package when a known `.py`/`.pyi` file lies under `R/P/` (no file, folder only). The first root that has the first segment of `P` decides: a missing rest is `Unresolved`. No root has it → `External(first segment)`.
- **Relative:** the base is the importing file's folder, one folder up per dot after the first; going above the repository root is `Unresolved`.
- **A name `n` in module `M`** (followed for at most 5 hops): the last top-level symbol named `n`; else an import in `M` that binds `n` (followed); else, for a package, its submodule `n`; else a unique hit among `M`'s star imports. Names starting with `_` are not carried by a star import.

**Resolution rules:**

| Fact | Resolution | Rule | Evidence |
|---|---|---|---|
| `import a.b` | an `imports` edge to the module file of `a.b` (none for a namespace package) | `py.import` | Sure |
| `from M import n` | an edge to `M`'s file when `n` is a name bound in it; an edge to the submodule's file when `n` is a submodule | `py.import` | Sure |
| An import that is `External` / `Unresolved` | ref `import` with the first segment / the specifier as written | | |
| `f()` / `K()` | the file's own top-level symbol, else an import binding (followed), else a unique star-import name | `py.call.local` / `py.call.import` | Sure |
| any call whose target is a Class | that class | `py.call.class` (overrides the rule above) | Sure |
| `m.f()`, `a.b.f()`: the receiver's first segment is bound to a module, and the rest walks submodules or names | the function or class `f` there | `py.call.module` | Sure |
| `K.m()`, `K` a class (local or imported) | Method `m` of `K` | `py.call.static` | Sure |
| `self.m()`, `cls.m()` in a method of `C` | Method `m` of `C` | `py.call.self` | Sure |
| the same when `C` has no `m`; `super().m()` | the first `m` on `C`'s bases, depth first, left to right, at most 5 hops | `py.call.inherited` | Probably |
| `self.x.y()` in a method of `C` | when every `AttrFact` for `x` on `C` (then on its bases) names one and the same class `K`, Method `y` of `K` or of its bases | `py.call.attr` | Probably |
| a base in `class C(B)` | `B` resolved like a call target, when it is a class | `py.extends` | Sure |
| Python builtins (`abs all any ascii bin bool breakpoint bytearray bytes callable chr classmethod compile complex delattr dict dir divmod enumerate eval exec filter float format frozenset getattr globals hasattr hash help hex id input int isinstance issubclass iter len list locals map max memoryview min next object oct open ord pow print property range repr reversed round set setattr slice sorted staticmethod str sum super tuple type vars zip __import__`, and the exceptions `BaseException Exception ArithmeticError AssertionError AttributeError ConnectionError FileExistsError FileNotFoundError ImportError IndexError KeyError LookupError ModuleNotFoundError NotImplementedError OSError PermissionError RuntimeError StopIteration TimeoutError TypeError UnicodeDecodeError UnicodeEncodeError ValueError ZeroDivisionError`) when no symbol or import binds the name | nothing | | |
| a receiver whose first segment is bound to an external import | ref `call` with text `receiver.callee`, status `external` | | |
| anything else | ref (`call` or `extends`), status `unresolved` | | |

- `imports_files` holds every file an `imports` edge points to.

- [ ] **Step 1: Write the failing tests** (`tests/py_resolve.rs`, with a helper like Task 5's):
  - `py_basic_resolves_imports_calls_and_inheritance` asserts these edges:
    - `imports src/shop/service.py → src/shop/repo.py`, `→ src/shop/models.py` (from `from . import models`), `→ src/shop/mailer.py`; `imports src/shop/api.py → src/shop/__init__.py`, `→ src/shop/models.py`, `→ src/shop/fast.pyi`; `imports tests/test_service.py → src/shop/service.py`;
    - `calls src/shop/api.py#create → src/shop/service.py#place_order` (`py.call.import`, through the `__init__` re-export) and `→ src/shop/fast.pyi#speedup`;
    - `calls src/shop/api.py#create → src/shop/models.py#Order` (`py.call.class`, from `shop.models.Order(total)`);
    - `calls src/shop/api.py → src/shop/api.py#route` (source is the file: a decorator call), `py.call.local`;
    - `calls src/shop/service.py#Service.__init__ → src/shop/repo.py#Repo`, `calls src/shop/service.py#Service.place → src/shop/models.py#Order` (`py.call.class`);
    - `calls src/shop/service.py#Service.place → src/shop/service.py#Service._audit` (`py.call.self`, Sure);
    - `calls src/shop/service.py#Service.place → src/shop/repo.py#Repo.add` and `→ src/shop/mailer.py#Mailer.send` (`py.call.attr`, `inferred == true`);
    - `calls src/shop/models.py#Base.save → src/shop/models.py#Base.validate` (`py.call.self`); `calls src/shop/models.py#Order.persist → src/shop/models.py#Base.save` (`py.call.inherited`, `inferred == true`);
    - `extends src/shop/models.py#Order → src/shop/models.py#Base` (`py.extends`);
    - `calls src/shop/service.py#place_order → src/shop/service.py#Service` and `→ src/shop/mailer.py#Mailer`;
    
    and these refs: `(src/shop/service.py, import, logging, external)`, `(src/shop/service.py, call, logging.getLogger, external)`, `(src/shop/service.py, call, log.info, unresolved)`, `(src/shop/service.py, call, check, unresolved)`, `(src/shop/service.py, call, Service(Mailer()).place, unresolved)`, `(src/shop/repo.py, call, order.save, unresolved)`, `(tests/conftest.py, import, pytest, external)`. No edge or ref names `ValueError` or `print`.
  - `roots_follow_pyproject_and_src_layout`: `PyModules::new` over the fixture's known files has `roots() == ["", "src"]`; over `{"backend/pyproject.toml", "backend/app/main.py", "tools/x.py"}` it is `["", "backend"]`, and `resolve("backend/app/main.py", "app.core")` with `backend/app/core/__init__.py` known is that file.
  - `a_name_in_init_beats_a_submodule_of_the_same_name`: package `p/` with `p/__init__.py` defining `def x()` and a file `p/x.py`: `from p import x; x()` resolves to `p/__init__.py#x`; without the `def`, `from p import x` is the submodule and `x.f()` resolves into `p/x.py`.
  - `a_relative_import_above_the_root_is_unresolved`: from `a/b.py`, `from ...x import y` gives a ref `import` with text `...x`, status `unresolved`.
  - `a_py_module_beats_its_stub`: with both `m.py` and `m.pyi` defining `f`, `from m import f; f()` resolves to `m.py#f`.
  - `ambiguous_attr_types_and_star_imports_are_refs`: `self.x = A()` and `self.x = B()` in one class make `self.x.y()` a ref; two star imports that both offer `f` make `f()` a ref.
- [ ] **Step 2: Run** `cargo test -p hm-index --test py_resolve`. Expected: FAIL.
- [ ] **Step 3: Implement** `modules.rs` and `resolve.rs`.
- [ ] **Step 4: Run** `cargo test -p hm-index --test py_resolve`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(index): resolve Python imports by package roots and calls by module, class and stated attribute type`

---

### Task 7: Language dispatch and incremental propagation for Go and Python

**Files:**
- Create: `crates/hm-index/src/langs.rs`
- Modify: `crates/hm-index/src/lib.rs` (`pub mod langs;`), `crates/hm-index/src/index.rs`
- Modify: `crates/harbormap/src/index.rs` (`LANGS`: `python` and `go` become read), `crates/harbormap/tests/learn_cli.rs` (the `not yet supported: Python 1` line)
- Modify: `crates/hm-index/tests/index_full.rs` (the `tool.py` expectations: it is now parsed; `not_supported_yet == 0`)
- Test: `crates/hm-index/tests/index_go_py.rs`

**Interfaces:**
- Consumes: Tasks 2, 5, 6.
- Produces (`langs.rs`):

  ```rust
  pub fn extract(src: &str, lang: Lang) -> FileFacts;   // ts / go / py; any other language: FileFacts::empty(ParsedWithErrors)
  pub struct Resolvers { pub ts: ModuleResolver, pub go: GoModules, pub py: PyModules }
  impl Resolvers {
      pub fn load(root: &Path, known: BTreeSet<String>) -> Resolvers;
      pub fn config_sources(&self) -> BTreeSet<String>;   // tsconfig sources ∪ go.mod paths
      pub fn notes(&self) -> Vec<String>;                 // tsconfig notes and go.mod notes, sorted
  }
  pub struct Workspace<'a> { /* facts, resolvers, a lazily built go::resolve::MethodIndex */ }
  impl<'a> Workspace<'a> {
      pub fn new(facts: &'a BTreeMap<String, FileFacts>, resolvers: &'a Resolvers) -> Workspace<'a>;
      pub fn ts(&self) -> ts::resolve::Project<'a>;
      pub fn go(&self) -> go::resolve::GoProject<'_>;     // builds the MethodIndex on first use (OnceCell)
      pub fn py(&self) -> py::resolve::PyProject<'a>;
  }
  pub fn resolve_file(w: &Workspace, rel: &str) -> ResolvedFile;                 // by Lang::from_path(rel)
  pub fn implicit_dependents(w: &Workspace, rel: &str) -> Vec<String>;           // Go: package_siblings; else empty
  pub fn inferred_names(rel: &str, facts: &FileFacts) -> BTreeSet<String>;      // Go: method_names; else empty
  pub fn reads_names(rel: &str, facts: &FileFacts, names: &BTreeSet<String>) -> bool; // Go: calls_any_method; else false
  ```

**Changes in `index.rs`** (keep each one small; do not restyle the rest):
1. `read_one` calls `crate::langs::extract`; `parses` and `unparsed_status` use `Lang::parsed`.
2. `Indexer.resolver: ModuleResolver` becomes `resolvers: Resolvers`; `open`, `full` and the rebuild in `bring_in_line` call `Resolvers::load`; `report.notes = resolvers.notes()`; the rebuild check uses `Resolvers::config_sources`.
3. Every `Project { facts, modules }` becomes `Workspace::new(&facts, &resolvers)`; `replace_resolved` and `sync_resolved` call `langs::resolve_file`.
4. `forwards(w: &Workspace, rel, target)`: Python files use `py::resolve::forwards(&w.py(), ..)`, Go files return `false`, other files keep today's body with `let p = w.ts();`. `dependents` takes the `Workspace`.
5. `Surface::of`: for a Python file, `forwarded` lists every import (any name of a Python import can be imported from it), and `Surface` gains `heritage: Vec<(&str, &str)>` (class, base), filled for Python files only.
6. In `bring_in_line`, without a rebuild, the files to resolve again also include:
   - `langs::implicit_dependents` of every seed (Go siblings read the package's symbols and package clause);
   - every file whose facts `reads_names` the names that a written or removed file gained or lost (`inferred_names(old) △ inferred_names(new)`), because a `go.call.method` edge depends on every Go file's method names.

- [ ] **Step 1: Write the failing tests** (`tests/index_go_py.rs`, using `common::{indexed, fresh_index, edit, set, dump, fresh_index_of}`):
  - `go_basic_indexes_with_evidence`: a full pass over `go-basic` reports `files["go"] == 9`, `parsed == 9`, `parsed_with_errors == 0`, `not_supported_yet == 0`, classes `test == 1` and `generated == 1`; the edge `http/server.go#Server.handleOrder → store/memory.go#Memory.Save` is stored with `(rule, prov, conf) == ("go.call.method", "inferred", "probably")`, and `…handleOrder → shop.go#Validate` with `("go.call.package", "parsed", "sure")`.
  - `py_basic_indexes_with_evidence`: `files["python"] == 11` (the 10 `.py` files and `fast.pyi`), `parsed_with_errors == 0`; classes `test == 3`, `generated == 1`; `Order.persist → Base.save` is `("py.call.inherited", "inferred", "probably")`.
  - `renaming_a_go_method_updates_inferred_callers_elsewhere`: in a copy of `go-basic`, rename `Save` to `Put` in `store/memory.go` only (`func (m *Memory) Save(` → `func (m *Memory) Put(`); `apply({store/memory.go})` lists `http/server.go` in `reresolved` (it does not import `store`) and `store/memory_test.go`; the edge `handleOrder → Memory.Save` is gone, a ref `s.Store.Save` (`unresolved`) is there, and `dump` equals a cold index.
  - `a_go_sibling_change_reresolves_the_package`: rename `writeJSON` to `writeBody` in `http/json.go`; `http/server.go` is re-resolved, its call becomes a ref `writeJSON`, and `dump` equals cold.
  - `a_python_reexport_chain_reresolves_importers`: rename `def place_order(` to `def place(` in `service.py`; `api.py` is re-resolved (through `__init__.py`), its edge becomes a ref `place_order`, and `dump` equals cold. Then delete the first line of `src/shop/__init__.py`: `dump` still equals cold.
  - `a_python_base_class_change_reresolves_subclasses_in_other_files`: add `src/shop/premium.py` with `from .models import Order\n\nclass Premium(Order):\n    def go(self):\n        return self.save()\n` (an added file, so everything resolves), then rename `def save(` to `def store(` in `models.py`; `premium.py` is re-resolved and `dump` equals cold.
  - In `crates/harbormap/tests/learn_cli.rs`, the existing assertion on `not yet supported: Python 1` becomes: the `harbormap index` text lists `Python 1` among the read languages and has no `not yet supported` part.
- [ ] **Step 2: Run** `cargo test -p hm-index --test index_go_py`. Expected: FAIL.
- [ ] **Step 3: Implement** `langs.rs` and the six `index.rs` changes; update `harbormap`'s `LANGS` and the two L1 tests named above.
- [ ] **Step 4: Run** `cargo test --workspace`. Expected: PASS, including every L1 incremental and property test.
- [ ] **Step 5: Commit** `feat(index): index Go and Python files and propagate their changes`

---

### Task 8: Synthetic Go and Python projects, the property test and latency

**Files:**
- Create: `crates/hm-index/src/synth_go.rs`, `crates/hm-index/src/synth_py.rs`
- Modify: `crates/hm-index/src/lib.rs` (`#[doc(hidden)] pub mod synth_go;` and `synth_py`), `crates/hm-index/examples/latency.rs` (`--lang`)
- Test: `crates/hm-index/tests/index_property_go_py.rs`

**Interfaces:**
- Consumes: Task 7; `synth::Edit`.
- Produces, in each of `synth_go` and `synth_py` (pure functions of the seed and the tree, like `synth`):

  ```rust
  pub fn project(dir: &Path, seed: u64, files: u32);
  pub fn apply_edit(dir: &Path, seed: u64, edit: Edit) -> BTreeSet<String>; // the changed relative paths
  pub fn edit_function(dir: &Path, rel: &str, seed: u64, kind: Edit) -> BTreeSet<String>; // BodyChange or SignatureChange only
  ```

**What the projects must contain** (so that every propagation path is exercised):
- **Go:** a `go.mod` (`module example.com/synth`), packages of 10 files in folders `p0/`, `p1/`, … Each file has 3–6 exported functions and one type with 1–3 methods. Calls go to the same package across files, to `pN.F()` of imported packages, to the receiver's own methods, and to `x.M()` method names, some unique and some shared by two types (so both `go.call.method` edges and refs occur). Every 10th package-file is a `_test.go` file.
- **Python:** a `pyproject.toml` and a `src/synth/` tree of packages of 10 modules. Each package's `__init__.py` re-exports some names from its modules. Modules define functions and classes; some classes extend a class from another module and call inherited methods through `self`; some set `self.x = K()` and call `self.x.m()`. Imports are absolute (`synth.pN.mod`) and relative (`.mod`, `..pM`).
- **Edits** map the `Edit` variants: `Rename` renames a function or method that another file reaches (through a re-export, a package sibling, or a method-name call); `AddImport`/`RemoveImport` add or remove one import line (the calls stay); the rest as in `synth`.

- [ ] **Step 1: Write the failing tests** (`tests/index_property_go_py.rs`, copying `index_property.rs`'s `check` with the project and edit functions passed in): `go_incremental_equals_cold` (48 cases, project seed 7, 24 files, 1–12 edits) and `python_incremental_equals_cold` (48 cases, project seed 7, 24 files, 1–12 edits), plus `go_and_python_projects_are_deterministic` (two `project` calls with one seed give identical trees, and the tree indexes with `parsed_with_errors == 0`).
- [ ] **Step 2: Run** `cargo test -p hm-index --test index_property_go_py`. Expected: FAIL (modules missing). After Step 3, a counterexample is a real propagation bug in Task 7's code: fix it there in its own commit, never by narrowing the generator.
- [ ] **Step 3: Implement** the two generators and `--lang ts|go|python` in `examples/latency.rs` (default `ts`; picks `project` and `edit_function`; add `lang` to the printed JSON and to `FLAGS`/`USAGE`).
- [ ] **Step 4: Run** `cargo test -p hm-index --test index_property_go_py` (PASS), then for each of `ts`, `go`, `python`:
  `cargo run --release -p hm-index --example latency -- --lang <lang> --files 1000 --saves 200 --seed 7 --max-p95-body-ms 300 --max-p95-sig-ms 500 --max-cold-ms 2000 --branch-switch 300`. Expected: exit 0 for `ts`. For `go` and `python`, record the numbers; a miss is reported in the commit message and the handoff, not hidden.
- [ ] **Step 5: Commit** `test(index): property-test Go and Python incremental updates and measure their latency`, with the three JSON lines in the message body.

---

### Task 9: Pinned Go and Python repositories and the smoke test

**Files:**
- Modify: `tests/eval/repos.toml` (append two `[[repo]]` tables)
- Create: `crates/hm-index/tests/eval_l2b.rs`

**Interfaces:**
- Consumes: Task 7; `scripts/eval/fetch.sh`.
- Produces, appended to `repos.toml`:

  ```toml
  [[repo]]
  name = "wtf"
  url = "https://github.com/benbjohnson/wtf"
  commit = "05bc90c940d5f9e2490fc93cf467d9e8aa48ad63"
  licence = "MIT"
  subdir = ""

  [[repo]]
  name = "fastapi-template"
  url = "https://github.com/fastapi/full-stack-fastapi-template"
  commit = "f4bcfaba7c85be3c6332cc74a20819f586dcdbc6"
  licence = "MIT"
  subdir = "backend"
  ```

- `eval_l2b.rs` copies `eval_l1.rs`'s `Indexed`, `eval_root` and `index` helpers and adds `has_ref(file, kind, text, status)`. Its tests are `#[ignore]` and skip when `HARBORMAP_EVAL_DIR` is unset.

- [ ] **Step 1: Write the tests.**
  - `wtf_indexes_and_resolves` (root `wtf`):
    - the exact set of `parsed_with_errors` files (pin what the run shows; for each file, a comment names the grammar gap and its line, as `eval_l1.rs` does; an empty set is asserted as empty);
    - `imports cmd/wtfd/main.go → sqlite/dial.go`;
    - `calls cmd/wtfd/main.go#Main.Run → sqlite/dial.go#NewDialService`, `→ http/server.go#ListenAndServeDebug` (the import's name is the package clause `http`) and `→ cmd/wtfd/main.go#expandDSN`;
    - ref `(http/dial.go, call, s.DialService.FindDialByID, unresolved)`: the interface has three implementations (`sqlite`, `http`, `mock`);
    - ref `(cmd/wtfd/main.go, import, github.com/pelletier/go-toml, external)`;
    - `eprintln!` the report (symbols, edges by kind, refs by status, `took_ms`).
  - `fastapi_backend_indexes_and_resolves` (root `fastapi-template/backend`):
    - the exact `parsed_with_errors` set, as above;
    - `imports app/api/routes/login.py → app/crud.py`, `→ app/models.py`, `→ app/api/deps.py`; `imports app/api/main.py → app/api/routes/login.py` (a submodule named in `from app.api.routes import items, login, ...`);
    - `calls app/api/routes/login.py#login_access_token → app/crud.py#authenticate` (`crud.authenticate(...)`, `from app import crud`), `→ app/core/security.py#create_access_token` and `→ app/models.py#Token`;
    - ref `(app/api/routes/login.py, import, fastapi, external)`;
    - `tests/conftest.py` has class `test`.
- [ ] **Step 2: Run** `scripts/eval/fetch.sh "$SCRATCH/hm-eval" && HARBORMAP_EVAL_DIR="$SCRATCH/hm-eval" cargo test -p hm-index --test eval_l2b -- --ignored --nocapture` (`$SCRATCH` = your scratchpad folder). Expected: PASS. A failure is a finding: fix the extractor or resolver in its own commit, never the expectation, unless the expectation misreads the code (then quote the code line in the commit). Also run `eval_l1` again: it must still pass.
- [ ] **Step 3: Commit** `test(eval): pin wtf and the fastapi backend and add the Go/Python smoke test`, with the two reports' counts and `took_ms` in the message body.

---

## Done when

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass.
- `eval_l1` and `eval_l2b` pass against the pinned commits.
- TS latency still meets the 1k-file targets; Go and Python latency is measured and recorded.
- `harbormap index` on `go-basic` and `py-basic` lists Go and Python as read languages, and `harbormap card http/server.go` shows its calls.

## Self-review notes (writer)

- **Spec coverage.** §5.1 symbols, edges, refs for Go/Python (Tasks 3–7); routes, entries and gaps are L2b part 2. §5.2 evidence and the first `Probably` edges (Tasks 2, 5, 6). §5.3 ids through `symbol_ids` (Tasks 5, 6). §5.4 dirty propagation (Task 7) and its proof (Task 8). §5.7 latency (Task 8). §6.1 grammars (Tasks 1, 3, 4). §6.2 Python and Go rows (Tasks 5, 6). §7.2 item 12 inner names as labels (Tasks 3, 4). §11.1 wtf and the fastapi backend (Task 9). §11.3 golden fixtures, property test, latency harness (Tasks 3–8).
- **Out of this plan:** route and entry rules, framework hops, manifests, Java/C#/Rust, the schema.
