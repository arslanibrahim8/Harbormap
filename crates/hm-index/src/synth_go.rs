//! Synthetic Go projects and edits for property tests and the latency harness.
//! Like [`crate::synth`], everything is a pure function of the seed and the tree,
//! so a failure reproduces from its seed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::synth::Edit;

/// Files per package folder.
const PACKAGE_SIZE: usize = 10;
/// Every this-many-th file of a package is a `_test.go` file.
const TEST_EVERY: usize = 10;
/// The module path of the synthetic project.
const MODULE: &str = "example.com/synth";

/// A splitmix64 generator: small, fast and the same on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `lo..=hi`.
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() % (hi - lo + 1) as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    /// `n` distinct values from `pool`, in the order drawn.
    fn pick_distinct(&mut self, pool: &[usize], n: usize) -> Vec<usize> {
        let mut rest = pool.to_vec();
        let mut out = Vec::new();
        while out.len() < n && !rest.is_empty() {
            let i = self.range(0, rest.len() - 1);
            out.push(rest.swap_remove(i));
        }
        out
    }
}

/// What one file of the plan declares.
struct Plan {
    pkg: usize,
    test: bool,
    /// Exported functions.
    funcs: Vec<String>,
    /// The one type of the file, and its methods.
    ty: String,
    methods: Vec<String>,
}

fn is_test_index(i: usize) -> bool {
    i % PACKAGE_SIZE == TEST_EVERY - 1
}

fn file_path(i: usize) -> String {
    let pkg = i / PACKAGE_SIZE;
    if is_test_index(i) {
        format!("p{pkg}/f{i:04}_test.go")
    } else {
        format!("p{pkg}/f{i:04}.go")
    }
}

fn write_file(dir: &Path, rel: &str, content: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().expect("a file path has a parent"))
        .expect("create the folder of a synthetic file");
    fs::write(&path, content).expect("write a synthetic file");
}

fn read(dir: &Path, rel: &str) -> String {
    fs::read_to_string(dir.join(rel)).expect("read a synthetic file")
}

/// Writes a deterministic Go project into `dir`: a `go.mod` for
/// `example.com/synth` and `files` files in packages `p0`, `p1`, … of 10 files each.
/// Every file has 3–6 exported functions and one type with 1–3 methods; every 10th
/// one is a `_test.go` file. Functions call functions of their package in other
/// files, `pN.F()` of the packages below (which they import), the other methods of
/// their receiver, and `x.M()` by method name. Some method names belong to two
/// types, so those calls stay refs until one of the two goes away.
pub fn project(dir: &Path, seed: u64, files: u32) {
    let n = files as usize;
    let mut rng = Rng(seed);
    write_file(dir, "go.mod", "module example.com/synth\n\ngo 1.22\n");

    // Two neighbouring files often implement the same method name.
    let shares: Vec<bool> = (0..n.div_ceil(2)).map(|_| rng.chance(60)).collect();
    let mut plans: Vec<Plan> = Vec::new();
    for i in 0..n {
        let count = rng.range(3, 6);
        let mut methods: Vec<String> = (0..rng.range(1, 3)).map(|k| format!("M{i}_{k}")).collect();
        if shares[i / 2] && (i ^ 1) < n {
            methods[0] = format!("Sync{}", i / 2);
        }
        plans.push(Plan {
            pkg: i / PACKAGE_SIZE,
            test: is_test_index(i),
            funcs: (0..count).map(|k| format!("F{i}_{k}")).collect(),
            ty: format!("T{i}"),
            methods,
        });
    }
    for i in 0..n {
        write_file(dir, &file_path(i), &render(&plans, i, &mut rng));
    }
}

/// A file a call can target: the package qualifier (`None` for the caller's own
/// package) and the index of the file.
type Target = (Option<usize>, usize);

fn qualifier(q: Option<usize>) -> String {
    q.map_or(String::new(), |pk| format!("p{pk}."))
}

fn render(plans: &[Plan], i: usize, rng: &mut Rng) -> String {
    let me = &plans[i];
    let imports: Vec<usize> = if me.pkg == 0 {
        Vec::new()
    } else {
        let pool: Vec<usize> = (0..me.pkg).collect();
        let want = rng.range(1, pool.len().min(2));
        let mut picked = rng.pick_distinct(&pool, want);
        picked.sort_unstable();
        picked
    };
    // Every non-test file the caller can reach, from its own package or an import.
    let mut targets: Vec<Target> = (0..plans.len())
        .filter(|&j| j != i && plans[j].pkg == me.pkg && !plans[j].test)
        .map(|j| (None, j))
        .collect();
    for &pk in &imports {
        targets.extend(
            (0..plans.len())
                .filter(|&j| plans[j].pkg == pk && !plans[j].test)
                .map(|j| (Some(pk), j)),
        );
    }

    let mut used: BTreeSet<usize> = BTreeSet::new();
    // The lines of a call to a function (`fun`) or, through a fresh value, to a
    // method of a type of the target file.
    let mut call = |rng: &mut Rng, c: usize, fun: bool| -> Option<Vec<String>> {
        if targets.is_empty() {
            return None;
        }
        let (q, j) = targets[rng.range(0, targets.len() - 1)];
        if let Some(pk) = q {
            used.insert(pk);
        }
        let t = &plans[j];
        Some(if fun {
            let f = &t.funcs[rng.range(0, t.funcs.len() - 1)];
            vec![format!("\tsum += {}{f}(a, b)", qualifier(q))]
        } else {
            let m = &t.methods[rng.range(0, t.methods.len() - 1)];
            vec![
                format!("\tv{c} := &{}{}{{}}", qualifier(q), t.ty),
                format!("\tsum += v{c}.{m}(a, b)"),
            ]
        })
    };

    // Function bodies, then method bodies.
    let mut funcs: Vec<Vec<String>> = Vec::new();
    for _ in &me.funcs {
        let mut body = Vec::new();
        for c in 0..rng.range(1, 4) {
            let fun = rng.chance(65);
            body.extend(call(rng, c, fun).into_iter().flatten());
        }
        funcs.push(body);
    }
    let mut methods: Vec<Vec<String>> = Vec::new();
    for k in 0..me.methods.len() {
        let mut body = Vec::new();
        if me.methods.len() > 1 && rng.chance(70) {
            let other = (k + rng.range(1, me.methods.len() - 1)) % me.methods.len();
            body.push(format!("\tsum += t.{}(a, b)", me.methods[other]));
        }
        for c in 0..rng.range(0, 2) {
            let fun = rng.chance(50);
            body.extend(call(rng, c, fun).into_iter().flatten());
        }
        methods.push(body);
    }
    let test_call = if me.test {
        let fun = targets.iter().position(|(q, _)| q.is_none()).map(|p| {
            let (_, j) = targets[p];
            let f = &plans[j].funcs[0];
            format!("\tsum := {f}(1, \"x\")")
        });
        Some(fun.unwrap_or_else(|| "\tsum := len(\"x\")".to_string()))
    } else {
        None
    };
    // An import that nothing calls would not compile; call into it from the first
    // function.
    for &pk in imports.iter().filter(|pk| !used.contains(pk)) {
        if let Some(j) = (0..plans.len()).find(|&j| plans[j].pkg == pk && !plans[j].test) {
            funcs[0].push(format!("\tsum += p{pk}.{}(a, b)", plans[j].funcs[0]));
        }
    }

    let mut out = format!("package p{}\n\n", me.pkg);
    if me.test || !imports.is_empty() {
        out.push_str("import (\n");
        if me.test {
            out.push_str("\t\"testing\"\n");
            if !imports.is_empty() {
                out.push('\n');
            }
        }
        for pk in &imports {
            out.push_str(&format!("\t\"{MODULE}/p{pk}\"\n"));
        }
        out.push_str(")\n\n");
    }
    out.push_str(&format!(
        "// {} holds the state of file {i}.\ntype {} struct {{\n\tn int\n}}\n\n",
        me.ty, me.ty
    ));
    for (name, body) in me.funcs.iter().zip(&funcs) {
        out.push_str(&format!(
            "func {name}(a int, b string) int {{\n\tsum := a + len(b)\n"
        ));
        for line in body {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("\treturn sum\n}\n\n");
    }
    for (name, body) in me.methods.iter().zip(&methods) {
        out.push_str(&format!(
            "func (t *{}) {name}(a int, b string) int {{\n\tsum := a + t.n + len(b)\n",
            me.ty
        ));
        for line in body {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("\treturn sum\n}\n\n");
    }
    if let Some(first) = test_call {
        out.push_str(&format!(
            "func Test{i}(t *testing.T) {{\n{first}\n\tif sum < 0 {{\n\t\tt.Fatal(\"negative\")\n\t}}\n}}\n\n"
        ));
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

/// A function or method declaration found in a file.
struct Decl {
    name: String,
    /// The index of its `func` line.
    line: usize,
    method: bool,
}

/// The declarations of `src`. The `broken` function that [`Edit::BreakSyntax`]
/// appends is not one of them.
fn decls(src: &str) -> Vec<Decl> {
    src.lines()
        .enumerate()
        .filter_map(|(line, l)| {
            let rest = l.strip_prefix("func ")?;
            let (rest, method) = match rest.strip_prefix('(') {
                Some(r) => (r.split_once(") ")?.1, true),
                None => (rest, false),
            };
            let name = &rest[..rest.find('(')?];
            (!name.is_empty() && !name.starts_with("broken")).then(|| Decl {
                name: name.to_string(),
                line,
                method,
            })
        })
        .collect()
}

/// Every `.go` file under `dir`, as sorted `/`-separated relative paths.
fn go_files(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, rel: &str, out: &mut BTreeSet<String>) {
        let Ok(entries) = fs::read_dir(base) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let path = entry.path();
            if path.is_dir() {
                walk(&path, &rel, out);
            } else if name.ends_with(".go") {
                out.insert(rel);
            }
        }
    }
    let mut set = BTreeSet::new();
    walk(dir, "", &mut set);
    set.into_iter().collect()
}

fn is_test_file(rel: &str) -> bool {
    rel.ends_with("_test.go")
}

/// The package folder of a file.
fn pkg_of(rel: &str) -> &str {
    rel.split('/').next().unwrap_or(rel)
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The identifiers of `src` outside declaration lines: what the file refers to.
fn used_names(src: &str) -> BTreeSet<String> {
    src.lines()
        .filter(|l| !l.starts_with("func "))
        .flat_map(|l| l.split(|c: char| !is_ident(c)))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// `src` with the line at `idx` replaced.
fn replace_line(src: &str, idx: usize, new: &str) -> String {
    let mut lines: Vec<&str> = src.lines().collect();
    lines[idx] = new;
    lines.join("\n") + "\n"
}

/// `src` with `stmt` inserted before the `return` of the declaration at line `at`,
/// or before its closing brace when it has none.
fn insert_stmt(src: &str, at: usize, stmt: &str) -> String {
    let mut lines: Vec<&str> = src.lines().collect();
    let end = (at + 1..lines.len())
        .find(|&i| lines[i] == "}")
        .unwrap_or(at + 1);
    let at = if end > 0 && lines[end - 1] == "\treturn sum" {
        end - 1
    } else {
        end
    };
    lines.insert(at, stmt);
    lines.join("\n") + "\n"
}

/// Writes `new` to `rel` when it differs from `old`, and says whether it did.
fn write_if_changed(dir: &Path, rel: &str, old: &str, new: &str) -> bool {
    if old == new {
        return false;
    }
    write_file(dir, rel, new);
    true
}

/// `src` with a body or a signature edit in one of its declarations, or `None` when
/// it has none to edit. A body edit adds a statement. A signature edit adds a
/// variadic parameter named after `seed` (callers stay valid), or renames the one
/// an earlier edit added.
fn edit_function_source(src: &str, seed: u64, kind: Edit, rng: &mut Rng) -> Option<String> {
    let pool: Vec<Decl> = decls(src)
        .into_iter()
        .filter(|d| kind != Edit::SignatureChange || !d.name.starts_with("Test"))
        .collect();
    if pool.is_empty() {
        return None;
    }
    let d = &pool[rng.range(0, pool.len() - 1)];
    Some(match kind {
        Edit::SignatureChange => {
            let line = src.lines().nth(d.line).unwrap_or_default();
            let open = line.find(&format!("{}(", d.name))? + d.name.len() + 1;
            let close = open + line[open..].find(')')?;
            let params = &line[open..close];
            let kept = params.find(", extra").map_or(params, |at| &params[..at]);
            let new_line = format!(
                "{}{kept}, extra{seed} ...int{}",
                &line[..open],
                &line[close..]
            );
            replace_line(src, d.line, &new_line)
        }
        _ => insert_stmt(src, d.line, &format!("\tsum += {}", seed % 1_000_000_007)),
    })
}

/// A body or signature edit in one declaration of the file `rel`, chosen from
/// `seed`. Unlike [`apply_edit`] it reads and writes that one file only. Returns
/// `{rel}`, or an empty set when the file has nothing to edit.
///
/// # Panics
///
/// When `kind` is not [`Edit::BodyChange`] or [`Edit::SignatureChange`].
pub fn edit_function(dir: &Path, rel: &str, seed: u64, kind: Edit) -> BTreeSet<String> {
    assert!(
        matches!(kind, Edit::BodyChange | Edit::SignatureChange),
        "edit_function takes BodyChange or SignatureChange, not {kind:?}"
    );
    let mut rng = Rng(seed ^ ((kind as u64 + 1) << 56));
    let src = read(dir, rel);
    let mut changed = BTreeSet::new();
    if let Some(new) = edit_function_source(&src, seed, kind, &mut rng)
        && write_if_changed(dir, rel, &src, &new)
    {
        changed.insert(rel.to_string());
    }
    changed
}

/// The packages the `import` lines of `src` name, as `pN`.
fn imported_packages(src: &str) -> BTreeSet<String> {
    let prefix = format!("\"{MODULE}/");
    src.lines()
        .filter_map(|l| {
            let at = l.find(&prefix)? + prefix.len();
            let name = l[at..].strip_suffix('"')?;
            Some(name.to_string())
        })
        .collect()
}

/// Applies one edit to the project in `dir`, chosen deterministically from the tree
/// and `seed`, and returns the changed relative paths (`/`-separated; the deleted
/// path for [`Edit::DeleteFile`]), exactly the files whose bytes changed. The set is
/// empty only when the tree has nothing the edit could touch. It scans the whole
/// tree; use [`edit_function`] to edit a file you already know.
pub fn apply_edit(dir: &Path, seed: u64, edit: Edit) -> BTreeSet<String> {
    let mut rng = Rng(seed ^ ((edit as u64 + 1) << 56));
    let files = go_files(dir);
    let mut changed = BTreeSet::new();
    if files.is_empty() && edit != Edit::AddFile {
        return changed;
    }
    let sources: BTreeMap<String, String> =
        files.iter().map(|f| (f.clone(), read(dir, f))).collect();
    // Files with a declaration, for the edits that need one.
    let with_decls: Vec<&String> = files
        .iter()
        .filter(|f| !decls(&sources[*f]).is_empty())
        .collect();
    let packages: BTreeSet<&str> = files.iter().map(|f| pkg_of(f)).collect();
    match edit {
        Edit::BodyChange | Edit::SignatureChange => {
            let pool: Vec<&String> = with_decls
                .iter()
                .copied()
                .filter(|f| {
                    edit == Edit::BodyChange
                        || decls(&sources[*f])
                            .iter()
                            .any(|d| !d.name.starts_with("Test"))
                })
                .collect();
            if pool.is_empty() {
                return changed;
            }
            let file = pool[rng.range(0, pool.len() - 1)];
            if let Some(new) = edit_function_source(&sources[file], seed, edit, &mut rng)
                && write_if_changed(dir, file, &sources[file], &new)
            {
                changed.insert(file.clone());
            }
        }
        Edit::AddImport => {
            // The plain functions each package offers.
            let offers: BTreeMap<&str, Vec<String>> = packages
                .iter()
                .map(|pkg| {
                    let names: Vec<String> = files
                        .iter()
                        .filter(|f| pkg_of(f) == *pkg && !is_test_file(f))
                        .flat_map(|f| decls(&sources[f]))
                        .filter(|d| !d.method)
                        .map(|d| d.name)
                        .collect();
                    (*pkg, names)
                })
                .filter(|(_, names)| !names.is_empty())
                .collect();
            // Files that have a declaration and a package left to import.
            let importers: Vec<&String> = with_decls
                .iter()
                .copied()
                .filter(|f| {
                    let imported = imported_packages(&sources[*f]);
                    offers
                        .keys()
                        .any(|p| *p != pkg_of(f) && !imported.contains(*p))
                })
                .collect();
            if importers.is_empty() {
                return changed;
            }
            let file = importers[rng.range(0, importers.len() - 1)];
            let src = &sources[file];
            let imported = imported_packages(src);
            let options: Vec<(&str, &Vec<String>)> = offers
                .iter()
                .filter(|(p, _)| **p != pkg_of(file) && !imported.contains(**p))
                .map(|(p, names)| (*p, names))
                .collect();
            let (pkg, names) = options[rng.range(0, options.len() - 1)];
            let callee = names[rng.range(0, names.len() - 1)].clone();
            let ds = decls(src);
            let at = ds[rng.range(0, ds.len() - 1)].line;
            let body = insert_stmt(src, at, &format!("\tsum += {pkg}.{callee}(1, \"x\")"));
            let mut lines: Vec<&str> = body.lines().collect();
            let line = format!("\t\"{MODULE}/{pkg}\"");
            let line_single = format!("import \"{MODULE}/{pkg}\"");
            match lines.iter().position(|l| *l == "import (") {
                Some(open) => {
                    let close = (open + 1..lines.len())
                        .find(|&i| lines[i] == ")")
                        .expect("an import block is closed");
                    lines.insert(close, &line);
                }
                None => {
                    lines.insert(1, "");
                    lines.insert(2, &line_single);
                }
            }
            if write_if_changed(dir, file, src, &(lines.join("\n") + "\n")) {
                changed.insert(file.clone());
            }
        }
        Edit::RemoveImport => {
            let line_of_import = |l: &str| {
                l.trim_start_matches("import ")
                    .trim_start()
                    .starts_with(&format!("\"{MODULE}/"))
            };
            let with_imports: Vec<&String> = files
                .iter()
                .filter(|f| sources[*f].lines().any(line_of_import))
                .collect();
            if with_imports.is_empty() {
                return changed;
            }
            let file = with_imports[rng.range(0, with_imports.len() - 1)];
            let src = &sources[file];
            let mut lines: Vec<&str> = src.lines().collect();
            let imports: Vec<usize> = (0..lines.len())
                .filter(|&i| line_of_import(lines[i]))
                .collect();
            let drop = imports[rng.range(0, imports.len() - 1)];
            lines.remove(drop);
            // An import block left empty goes, with a blank line it leaves behind.
            if let Some(open) = lines.iter().position(|l| *l == "import (")
                && lines.get(open + 1) == Some(&")")
            {
                lines.drain(open..=open + 1);
            }
            let mut out: Vec<&str> = Vec::new();
            for l in lines {
                if l.is_empty() && out.last() == Some(&"") {
                    continue;
                }
                out.push(l);
            }
            if write_if_changed(dir, file, src, &(out.join("\n") + "\n")) {
                changed.insert(file.clone());
            }
        }
        Edit::Rename => {
            // A declaration that another file refers to; any when none is.
            let used: Vec<BTreeSet<String>> =
                files.iter().map(|f| used_names(&sources[f])).collect();
            let mut reached: BTreeSet<(usize, String)> = BTreeSet::new();
            let mut all: BTreeSet<(usize, String)> = BTreeSet::new();
            for (fi, f) in files.iter().enumerate() {
                for d in decls(&sources[f]) {
                    if d.name.starts_with("Test") {
                        continue;
                    }
                    if used
                        .iter()
                        .enumerate()
                        .any(|(gi, u)| gi != fi && u.contains(&d.name))
                    {
                        reached.insert((fi, d.name.clone()));
                    }
                    all.insert((fi, d.name));
                }
            }
            let pool: Vec<(usize, String)> = if reached.is_empty() { all } else { reached }
                .into_iter()
                .collect();
            if pool.is_empty() {
                return changed;
            }
            let (fi, name) = pool[rng.range(0, pool.len() - 1)].clone();
            let file = &files[fi];
            let src = &sources[file];
            let d = decls(src)
                .into_iter()
                .find(|d| d.name == name)
                .expect("the declaration was found in this file");
            let line = src.lines().nth(d.line).unwrap_or_default();
            let new_line = line.replacen(&format!("{name}("), &format!("{name}R("), 1);
            if write_if_changed(dir, file, src, &replace_line(src, d.line, &new_line)) {
                changed.insert(file.clone());
            }
        }
        Edit::AddFile => {
            let pkgs: Vec<&str> = if packages.is_empty() {
                vec!["p0"]
            } else {
                packages.iter().copied().collect()
            };
            let pkg = pkgs[rng.range(0, pkgs.len() - 1)];
            let mut k = 0;
            let rel = loop {
                let rel = format!("{pkg}/n{seed}_{k}.go");
                if !dir.join(&rel).exists() {
                    break rel;
                }
                k += 1;
            };
            let stem = format!("{seed}_{k}");
            let local: Vec<String> = files
                .iter()
                .filter(|f| pkg_of(f) == pkg && !is_test_file(f))
                .flat_map(|f| decls(&sources[f]))
                .filter(|d| !d.method)
                .map(|d| d.name)
                .collect();
            let existing: Vec<String> = files
                .iter()
                .filter(|f| !is_test_file(f))
                .flat_map(|f| decls(&sources[f]))
                .filter(|d| d.method)
                .map(|d| d.name)
                .collect();
            // A method named like one of the tree makes a type share its name.
            let method = if !existing.is_empty() && rng.chance(50) {
                existing[rng.range(0, existing.len() - 1)].clone()
            } else {
                format!("MN{stem}")
            };
            let ty = format!("TN{stem}");
            let mut out = format!(
                "package {pkg}\n\n// {ty} holds the state of a new file.\ntype {ty} struct {{\n\tn int\n}}\n\n"
            );
            for f in 0..3 {
                out.push_str(&format!(
                    "func FN{stem}_{f}(a int, b string) int {{\n\tsum := a + len(b)\n"
                ));
                if !local.is_empty() {
                    let callee = &local[rng.range(0, local.len() - 1)];
                    out.push_str(&format!("\tsum += {callee}(a, b)\n"));
                }
                out.push_str("\treturn sum\n}\n\n");
            }
            out.push_str(&format!(
                "func (t *{ty}) {method}(a int, b string) int {{\n\tsum := a + t.n + len(b)\n"
            ));
            if !existing.is_empty() {
                let callee = &existing[rng.range(0, existing.len() - 1)];
                out.push_str(&format!("\tv := &{ty}{{}}\n\tsum += v.{callee}(a, b)\n"));
            }
            out.push_str("\treturn sum\n}\n");
            write_file(dir, &rel, &out);
            changed.insert(rel);
        }
        Edit::DeleteFile => {
            let file = files[rng.range(0, files.len() - 1)].clone();
            fs::remove_file(dir.join(&file)).expect("delete a synthetic file");
            changed.insert(file);
        }
        Edit::BreakSyntax => {
            let file = files[rng.range(0, files.len() - 1)].clone();
            let src = &sources[&file];
            write_file(dir, &file, &format!("{src}\nfunc broken( {{\n"));
            changed.insert(file);
        }
    }
    changed
}
