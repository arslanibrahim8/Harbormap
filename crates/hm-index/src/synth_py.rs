//! Synthetic Python projects and edits for property tests and the latency harness.
//! Like [`crate::synth`], everything is a pure function of the seed and the tree,
//! so a failure reproduces from its seed.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::synth::Edit;

/// Modules per package.
const PACKAGE_SIZE: usize = 10;

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

    /// Between `lo` and `hi` distinct values from `pool`, in the order drawn.
    fn pick_upto(&mut self, pool: &[usize], lo: usize, hi: usize) -> Vec<usize> {
        let n = self.range(lo, hi);
        self.pick_distinct(pool, n)
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

/// What one module of the plan defines.
struct Module {
    pkg: usize,
    funcs: Vec<String>,
    class: String,
    methods: Vec<String>,
}

/// A name a package's `__init__.py` re-exports.
struct Export {
    /// The module it comes from.
    module: usize,
    /// The name there, and the name the package gives it.
    name: String,
    alias: String,
    is_class: bool,
}

fn module_path(i: usize) -> String {
    format!("src/synth/p{}/m{i:04}.py", i / PACKAGE_SIZE)
}

fn module_name(i: usize) -> String {
    format!("synth.p{}.m{i:04}", i / PACKAGE_SIZE)
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

/// Writes a deterministic Python project into `dir`: a `pyproject.toml` and, under
/// `src/synth/`, `files` modules in packages `p0`, `p1`, … of 10. Each package's
/// `__init__.py` re-exports some names of its modules, some under another name.
/// Modules define functions and one class; a class may extend a class of another
/// module, call inherited methods through `self`, and set `self.x = K()` to call
/// `self.x.m()`. Imports are absolute (`synth.pN.mod`, `synth.pN`) and relative
/// (`.mod`, `..pN`), and some bind a module that is then called through.
pub fn project(dir: &Path, seed: u64, files: u32) {
    let n = files as usize;
    let mut rng = Rng(seed);
    write_file(
        dir,
        "pyproject.toml",
        "[project]\nname = \"synth\"\nversion = \"0.1.0\"\n",
    );
    write_file(dir, "src/synth/__init__.py", "# The synthetic package.\n");

    let mods: Vec<Module> = (0..n)
        .map(|i| {
            let funcs = (0..rng.range(3, 6)).map(|k| format!("f{i}_{k}")).collect();
            let methods = (0..rng.range(1, 3)).map(|k| format!("m{i}_{k}")).collect();
            Module {
                pkg: i / PACKAGE_SIZE,
                funcs,
                class: format!("C{i}"),
                methods,
            }
        })
        .collect();

    // What each package re-exports.
    let packages = n.div_ceil(PACKAGE_SIZE);
    let mut vis: Vec<Vec<Export>> = Vec::new();
    for pk in 0..packages {
        let mut exports = Vec::new();
        let (lo, hi) = (pk * PACKAGE_SIZE, ((pk + 1) * PACKAGE_SIZE).min(n));
        for (i, m) in mods.iter().enumerate().take(hi).skip(lo) {
            if !rng.chance(55) {
                continue;
            }
            let idx: Vec<usize> = (0..m.funcs.len()).collect();
            for k in rng.pick_upto(&idx, 1, 2) {
                let name = m.funcs[k].clone();
                let alias = if rng.chance(40) {
                    format!("g{}", &name[1..])
                } else {
                    name.clone()
                };
                exports.push(Export {
                    module: i,
                    name,
                    alias,
                    is_class: false,
                });
            }
            if rng.chance(50) {
                let alias = if rng.chance(25) {
                    format!("K{i}")
                } else {
                    m.class.clone()
                };
                exports.push(Export {
                    module: i,
                    name: m.class.clone(),
                    alias,
                    is_class: true,
                });
            }
        }
        vis.push(exports);
    }

    for (pk, exports) in vis.iter().enumerate() {
        write_file(
            dir,
            &format!("src/synth/p{pk}/__init__.py"),
            &render_init(exports),
        );
    }
    for i in 0..n {
        write_file(dir, &module_path(i), &render(&mods, &vis, i, &mut rng));
    }
}

fn render_init(exports: &[Export]) -> String {
    let mut out = String::new();
    let mut from: Vec<usize> = exports.iter().map(|e| e.module).collect();
    from.dedup();
    for m in from {
        let names: Vec<String> = exports
            .iter()
            .filter(|e| e.module == m)
            .map(|e| {
                if e.alias == e.name {
                    e.name.clone()
                } else {
                    format!("{} as {}", e.name, e.alias)
                }
            })
            .collect();
        out.push_str(&format!("from .m{m:04} import {}\n", names.join(", ")));
    }
    if out.is_empty() {
        out.push_str("# Nothing is re-exported.\n");
    }
    out
}

/// A name a module binds by an import.
struct Bound {
    local: String,
    /// The module whose class or function it is.
    module: usize,
    is_class: bool,
}

fn render(mods: &[Module], vis: &[Vec<Export>], i: usize, rng: &mut Rng) -> String {
    let me = &mods[i];
    let first = me.pkg * PACKAGE_SIZE;
    let mut lines: Vec<String> = Vec::new();
    let mut names: Vec<Bound> = Vec::new();
    // Modules bound by `from . import m` and the like.
    let mut modules: Vec<(String, usize)> = Vec::new();
    let taken = |names: &[Bound], local: &str| names.iter().any(|b| b.local == local);

    for _ in 0..rng.range(1, 4) {
        let way = rng.range(0, 9);
        match way {
            // `from synth.pN.mod import a, b`, or `from .mod import a, b` in the package.
            0..=1 | 6..=7 => {
                let sibling = way >= 6;
                let from = if sibling { first } else { 0 };
                if i <= from {
                    continue;
                }
                let j = rng.range(from, i - 1);
                let m = &mods[j];
                let pool: Vec<(&str, bool)> = m
                    .funcs
                    .iter()
                    .map(|f| (f.as_str(), false))
                    .chain([(m.class.as_str(), true)])
                    .collect();
                let idx: Vec<usize> = (0..pool.len()).collect();
                let picked: Vec<(&str, bool)> = rng
                    .pick_upto(&idx, 1, 2)
                    .into_iter()
                    .map(|k| pool[k])
                    .filter(|(name, _)| !taken(&names, name))
                    .collect();
                if picked.is_empty() {
                    continue;
                }
                let spec = if sibling {
                    format!(".m{j:04}")
                } else {
                    module_name(j)
                };
                let list: Vec<&str> = picked.iter().map(|(name, _)| *name).collect();
                lines.push(format!("from {spec} import {}", list.join(", ")));
                for (name, is_class) in picked {
                    names.push(Bound {
                        local: name.to_string(),
                        module: j,
                        is_class,
                    });
                }
            }
            // `from synth.pN import name` or `from ..pN import name`: through the
            // `__init__.py` of a lower package.
            2..=5 => {
                let pools: Vec<usize> = (0..me.pkg).filter(|&p| !vis[p].is_empty()).collect();
                if pools.is_empty() {
                    continue;
                }
                let pj = pools[rng.range(0, pools.len() - 1)];
                let idx: Vec<usize> = (0..vis[pj].len()).collect();
                let picked: Vec<&Export> = rng
                    .pick_upto(&idx, 1, 2)
                    .into_iter()
                    .map(|k| &vis[pj][k])
                    .filter(|e| !taken(&names, &e.alias))
                    .collect();
                if picked.is_empty() {
                    continue;
                }
                let spec = if rng.chance(50) {
                    format!("synth.p{pj}")
                } else {
                    format!("..p{pj}")
                };
                let list: Vec<&str> = picked.iter().map(|e| e.alias.as_str()).collect();
                lines.push(format!("from {spec} import {}", list.join(", ")));
                for e in picked {
                    names.push(Bound {
                        local: e.alias.clone(),
                        module: e.module,
                        is_class: e.is_class,
                    });
                }
            }
            // `from . import mod`: a sibling module that is called through.
            8 => {
                if i <= first {
                    continue;
                }
                let j = rng.range(first, i - 1);
                let local = format!("m{j:04}");
                if modules.iter().any(|(l, _)| *l == local) {
                    continue;
                }
                lines.push(format!("from . import {local}"));
                modules.push((local, j));
            }
            // `from synth.pN import mod`: a module of a lower package.
            _ => {
                if me.pkg == 0 {
                    continue;
                }
                let j = rng.range(0, first - 1);
                let local = format!("m{j:04}");
                if modules.iter().any(|(l, _)| *l == local) {
                    continue;
                }
                lines.push(format!("from synth.p{} import {local}", mods[j].pkg));
                modules.push((local, j));
            }
        }
    }

    let funcs_in: Vec<&Bound> = names.iter().filter(|b| !b.is_class).collect();
    let classes_in: Vec<&Bound> = names.iter().filter(|b| b.is_class).collect();
    // A call from a function or a method: `total += ...`.
    let call = |rng: &mut Rng| -> Option<String> {
        let start = rng.range(0, 3);
        for step in 0..4 {
            match (start + step) % 4 {
                0 if !funcs_in.is_empty() => {
                    let b = funcs_in[rng.range(0, funcs_in.len() - 1)];
                    return Some(format!("total += {}(a, b)", b.local));
                }
                1 if !modules.is_empty() => {
                    let (local, j) = &modules[rng.range(0, modules.len() - 1)];
                    let f = &mods[*j].funcs[rng.range(0, mods[*j].funcs.len() - 1)];
                    return Some(format!("total += {local}.{f}(a, b)"));
                }
                2 if !classes_in.is_empty() => {
                    let b = classes_in[rng.range(0, classes_in.len() - 1)];
                    let ms = &mods[b.module].methods;
                    let m = &ms[rng.range(0, ms.len() - 1)];
                    return Some(format!("total += {}.{m}(None, a, b)", b.local));
                }
                3 if !classes_in.is_empty() => {
                    let b = classes_in[rng.range(0, classes_in.len() - 1)];
                    let ms = &mods[b.module].methods;
                    let m = &ms[rng.range(0, ms.len() - 1)];
                    return Some(format!("total += {}(a).{m}(a, b)", b.local));
                }
                _ => {}
            }
        }
        None
    };

    let mut out = String::new();
    if !lines.is_empty() {
        out.push_str(&lines.join("\n"));
        out.push_str("\n\n\n");
    }
    out.push_str(&format!("LIMIT_{i} = {}\n\n\n", i + 10));
    for (k, name) in me.funcs.iter().enumerate() {
        out.push_str(&format!("def {name}(a, b):\n    total = a + len(b)\n"));
        if rng.chance(20) {
            out.push_str(&format!(
                "\n    def inner(x):\n        return x + {}\n\n    total += inner(a)\n",
                rng.range(1, 9)
            ));
        }
        // A call to an earlier function of the module.
        if k > 0 && rng.chance(40) {
            out.push_str(&format!(
                "    total += {}(a, b)\n",
                me.funcs[rng.range(0, k - 1)]
            ));
        }
        for _ in 0..rng.range(1, 3) {
            if let Some(line) = call(rng) {
                out.push_str(&format!("    {line}\n"));
            }
        }
        out.push_str("    return total\n\n\n");
    }

    // The class: it may extend an imported class and hold an attribute of another.
    let base = (!classes_in.is_empty() && rng.chance(65))
        .then(|| classes_in[rng.range(0, classes_in.len() - 1)]);
    let attr = (!classes_in.is_empty() && rng.chance(60))
        .then(|| classes_in[rng.range(0, classes_in.len() - 1)]);
    match base {
        Some(b) => out.push_str(&format!("class {}({}):\n", me.class, b.local)),
        None => out.push_str(&format!("class {}:\n", me.class)),
    }
    out.push_str("    def __init__(self, a):\n");
    if base.is_some() {
        out.push_str("        super().__init__(a)\n");
    }
    out.push_str("        self.a = a\n");
    if let Some(k) = attr {
        out.push_str(&format!("        self.x = {}(a)\n", k.local));
    }
    for (k, name) in me.methods.iter().enumerate() {
        out.push_str(&format!(
            "\n    def {name}(self, a, b):\n        total = a\n"
        ));
        if me.methods.len() > 1 && rng.chance(60) {
            let other = (k + rng.range(1, me.methods.len() - 1)) % me.methods.len();
            out.push_str(&format!(
                "        total += self.{}(a, b)\n",
                me.methods[other]
            ));
        }
        if let Some(b) = base {
            let ms = &mods[b.module].methods;
            if rng.chance(70) {
                let m = &ms[rng.range(0, ms.len() - 1)];
                out.push_str(&format!("        total += self.{m}(a, b)\n"));
            }
            if rng.chance(25) {
                let m = &ms[rng.range(0, ms.len() - 1)];
                out.push_str(&format!("        total += super().{m}(a, b)\n"));
            }
        }
        if let Some(k) = attr
            && rng.chance(70)
        {
            let ms = &mods[k.module].methods;
            let m = &ms[rng.range(0, ms.len() - 1)];
            out.push_str(&format!("        total += self.x.{m}(a, b)\n"));
        }
        if rng.chance(30)
            && let Some(line) = call(rng)
        {
            out.push_str(&format!("        {line}\n"));
        }
        out.push_str("        return total\n");
    }
    out
}

/// A function, method or class found in a file.
struct Def {
    name: String,
    /// The index of its line.
    line: usize,
    /// 0 for a function or a class, 4 for a method.
    indent: usize,
    is_class: bool,
}

/// The definitions of `src` that edits touch: top-level functions and classes, and
/// the methods of classes, never dunder methods or the `broken` function that
/// [`Edit::BreakSyntax`] appends.
fn defs(src: &str) -> Vec<Def> {
    let mut out = Vec::new();
    let mut in_class = false;
    for (line, l) in src.lines().enumerate() {
        let (rest, indent, is_class) = if let Some(rest) = l.strip_prefix("class ") {
            in_class = true;
            (rest, 0, true)
        } else if let Some(rest) = l.strip_prefix("def ") {
            in_class = false;
            (rest, 0, false)
        } else if let Some(rest) = l.strip_prefix("    def ").filter(|_| in_class) {
            (rest, 4, false)
        } else {
            if !l.is_empty() && !l.starts_with([' ', '#']) {
                in_class = false;
            }
            continue;
        };
        let end = rest.find(['(', ':']).unwrap_or(rest.len());
        let name = &rest[..end];
        if !name.is_empty() && !name.starts_with("__") && !name.starts_with("broken") {
            out.push(Def {
                name: name.to_string(),
                line,
                indent,
                is_class,
            });
        }
    }
    out
}

/// Every `.py` file under `dir`, as sorted `/`-separated relative paths.
fn py_files(dir: &Path) -> Vec<String> {
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
            } else if name.ends_with(".py") {
                out.insert(rel);
            }
        }
    }
    let mut set = BTreeSet::new();
    walk(dir, "", &mut set);
    set.into_iter().collect()
}

fn is_init(rel: &str) -> bool {
    rel.ends_with("/__init__.py")
}

/// The dotted module name of a file under `src/`, as `synth.p1.m0003`, or the
/// package for an `__init__.py`.
fn dotted(rel: &str) -> String {
    let stem = rel.strip_prefix("src/").unwrap_or(rel);
    let stem = stem.strip_suffix("/__init__.py").unwrap_or(stem);
    stem.strip_suffix(".py").unwrap_or(stem).replace('/', ".")
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Every identifier of `src`.
fn words(src: &str) -> BTreeSet<String> {
    src.split(|c: char| !is_ident(c))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// `src` with the line at `idx` replaced.
fn replace_line(src: &str, idx: usize, new: &str) -> String {
    let mut lines: Vec<&str> = src.lines().collect();
    lines[idx] = new;
    lines.join("\n") + "\n"
}

/// `src` with `stmt` inserted before the `return total` of the definition `d`, or
/// at the end of its body when it has none.
fn insert_stmt(src: &str, d: &Def, stmt: &str) -> String {
    let mut lines: Vec<&str> = src.lines().collect();
    let want = format!("{}return total", " ".repeat(d.indent + 4));
    let mut at = lines.len();
    for (i, l) in lines.iter().enumerate().skip(d.line + 1) {
        if *l == want {
            at = i;
            break;
        }
        if !l.trim().is_empty() && indent_of(l) <= d.indent {
            at = i;
            break;
        }
    }
    while at > d.line + 1 && lines[at - 1].trim().is_empty() {
        at -= 1;
    }
    let stmt = format!("{}{stmt}", " ".repeat(d.indent + 4));
    lines.insert(at, &stmt);
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

/// `src` with a body or a signature edit in one of its functions or methods, or
/// `None` when it has none. A body edit adds a statement and a signature edit adds a
/// defaulted parameter named after `seed` (callers stay valid), or renames the one
/// an earlier edit added.
fn edit_function_source(src: &str, seed: u64, kind: Edit, rng: &mut Rng) -> Option<String> {
    let pool: Vec<Def> = defs(src).into_iter().filter(|d| !d.is_class).collect();
    if pool.is_empty() {
        return None;
    }
    let d = &pool[rng.range(0, pool.len() - 1)];
    Some(match kind {
        Edit::SignatureChange => {
            let line = src.lines().nth(d.line).unwrap_or_default();
            let open = line.find('(')? + 1;
            let close = line.rfind(')')?;
            let params = &line[open..close];
            let kept = params.find(", extra").map_or(params, |at| &params[..at]);
            let new_line = format!(
                "{}{kept}, extra{seed}=None{}",
                &line[..open],
                &line[close..]
            );
            replace_line(src, d.line, &new_line)
        }
        _ => insert_stmt(src, d, &format!("total += {}", seed % 1_000_000_007)),
    })
}

/// A body or signature edit in one function or method of the file `rel`, chosen from
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

/// The lines of `src` that import.
fn is_import(line: &str) -> bool {
    line.starts_with("from ") || line.starts_with("import ")
}

/// The functions a package's `__init__.py` re-exports: (package, visible name).
fn reexported_functions(files: &[String], sources: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (f, src) in files.iter().zip(sources) {
        if !is_init(f) {
            continue;
        }
        for line in src.lines().filter(|l| l.starts_with("from .")) {
            let Some((_, list)) = line.split_once(" import ") else {
                continue;
            };
            for item in list.split(',') {
                let visible = item.rsplit(" as ").next().unwrap_or(item).trim();
                if visible.starts_with(|c: char| c.is_lowercase()) {
                    out.push((dotted(f), visible.to_string()));
                }
            }
        }
    }
    out
}

/// Applies one edit to the project in `dir`, chosen deterministically from the tree
/// and `seed`, and returns the changed relative paths (`/`-separated; the deleted
/// path for [`Edit::DeleteFile`]), exactly the files whose bytes changed. The set is
/// empty only when the tree has nothing the edit could touch. It scans the whole
/// tree; use [`edit_function`] to edit a file you already know.
pub fn apply_edit(dir: &Path, seed: u64, edit: Edit) -> BTreeSet<String> {
    let mut rng = Rng(seed ^ ((edit as u64 + 1) << 56));
    let files = py_files(dir);
    let mut changed = BTreeSet::new();
    if files.is_empty() && edit != Edit::AddFile {
        return changed;
    }
    let sources: Vec<String> = files.iter().map(|f| read(dir, f)).collect();
    // Files with a function or a method, for the edits that need one.
    let with_funcs: Vec<usize> = (0..files.len())
        .filter(|&k| defs(&sources[k]).iter().any(|d| !d.is_class))
        .collect();
    match edit {
        Edit::BodyChange | Edit::SignatureChange => {
            if with_funcs.is_empty() {
                return changed;
            }
            let k = with_funcs[rng.range(0, with_funcs.len() - 1)];
            if let Some(new) = edit_function_source(&sources[k], seed, edit, &mut rng)
                && write_if_changed(dir, &files[k], &sources[k], &new)
            {
                changed.insert(files[k].clone());
            }
        }
        Edit::AddImport => {
            if with_funcs.is_empty() {
                return changed;
            }
            let k = with_funcs[rng.range(0, with_funcs.len() - 1)];
            // Functions of other modules, and of packages through their re-exports.
            let mut options: Vec<(String, String)> = Vec::new();
            for (g, f) in files.iter().enumerate() {
                if g == k || is_init(f) {
                    continue;
                }
                for d in defs(&sources[g])
                    .iter()
                    .filter(|d| !d.is_class && d.indent == 0)
                {
                    options.push((dotted(f), d.name.clone()));
                }
            }
            let through = reexported_functions(&files, &sources);
            if options.is_empty() && through.is_empty() {
                return changed;
            }
            let (module, callee) = if !through.is_empty() && (options.is_empty() || rng.chance(45))
            {
                through[rng.range(0, through.len() - 1)].clone()
            } else {
                options[rng.range(0, options.len() - 1)].clone()
            };
            let ds: Vec<Def> = defs(&sources[k])
                .into_iter()
                .filter(|d| !d.is_class)
                .collect();
            let d = &ds[rng.range(0, ds.len() - 1)];
            let local = format!("added{seed}");
            let body = insert_stmt(&sources[k], d, &format!("total += {local}(a, b)"));
            let new = format!("from {module} import {callee} as {local}\n{body}");
            if write_if_changed(dir, &files[k], &sources[k], &new) {
                changed.insert(files[k].clone());
            }
        }
        Edit::RemoveImport => {
            let with_imports: Vec<usize> = (0..files.len())
                .filter(|&k| sources[k].lines().any(is_import))
                .collect();
            if with_imports.is_empty() {
                return changed;
            }
            let k = with_imports[rng.range(0, with_imports.len() - 1)];
            let imports: Vec<usize> = sources[k]
                .lines()
                .enumerate()
                .filter(|(_, l)| is_import(l))
                .map(|(i, _)| i)
                .collect();
            let drop = imports[rng.range(0, imports.len() - 1)];
            let kept: Vec<&str> = sources[k]
                .lines()
                .enumerate()
                .filter(|(i, _)| *i != drop)
                .map(|(_, l)| l)
                .collect();
            let new = if kept.is_empty() {
                String::new()
            } else {
                kept.join("\n") + "\n"
            };
            if write_if_changed(dir, &files[k], &sources[k], &new) {
                changed.insert(files[k].clone());
            }
        }
        Edit::Rename => {
            // A function, method or class that another file mentions (through a
            // re-export, a sibling import or a call); any when none is.
            let seen: Vec<BTreeSet<String>> = sources.iter().map(|s| words(s)).collect();
            let init_words: BTreeSet<&String> = files
                .iter()
                .zip(&seen)
                .filter(|(f, _)| is_init(f))
                .flat_map(|(_, w)| w)
                .collect();
            let mut reached: Vec<(usize, String)> = Vec::new();
            let mut all: Vec<(usize, String)> = Vec::new();
            for (k, src) in sources.iter().enumerate() {
                for d in defs(src) {
                    if seen
                        .iter()
                        .enumerate()
                        .any(|(g, w)| g != k && w.contains(&d.name))
                    {
                        reached.push((k, d.name.clone()));
                    }
                    all.push((k, d.name));
                }
            }
            // Half the time, prefer what a package re-exports.
            let reexported: Vec<(usize, String)> = reached
                .iter()
                .filter(|(_, n)| init_words.contains(n))
                .cloned()
                .collect();
            let pool = if !reexported.is_empty() && rng.chance(50) {
                reexported
            } else if reached.is_empty() {
                all
            } else {
                reached
            };
            if pool.is_empty() {
                return changed;
            }
            let (k, name) = pool[rng.range(0, pool.len() - 1)].clone();
            let d = defs(&sources[k])
                .into_iter()
                .find(|d| d.name == name)
                .expect("the definition was found in this file");
            let line = sources[k].lines().nth(d.line).unwrap_or_default();
            let keyword = if d.is_class { "class" } else { "def" };
            let new_line = line.replacen(
                &format!("{keyword} {name}"),
                &format!("{keyword} {name}R"),
                1,
            );
            if write_if_changed(
                dir,
                &files[k],
                &sources[k],
                &replace_line(&sources[k], d.line, &new_line),
            ) {
                changed.insert(files[k].clone());
            }
        }
        Edit::AddFile => {
            let packages: BTreeSet<String> = files
                .iter()
                .filter_map(|f| f.strip_prefix("src/synth/")?.split_once('/'))
                .map(|(p, _)| p.to_string())
                .collect();
            let pkgs: Vec<String> = if packages.is_empty() {
                vec!["p0".to_string()]
            } else {
                packages.into_iter().collect()
            };
            let pkg = &pkgs[rng.range(0, pkgs.len() - 1)];
            let mut k = 0;
            let rel = loop {
                let rel = format!("src/synth/{pkg}/n{seed}_{k}.py");
                if !dir.join(&rel).exists() {
                    break rel;
                }
                k += 1;
            };
            let stem = format!("{seed}_{k}");
            // A module with a function and a class to build on.
            let hosts: Vec<usize> = (0..files.len())
                .filter(|&g| {
                    let ds = defs(&sources[g]);
                    ds.iter().any(|d| d.is_class) && ds.iter().any(|d| !d.is_class && d.indent == 0)
                })
                .collect();
            let mut out = String::new();
            let mut callee = None;
            let mut base: Option<(String, Vec<String>)> = None;
            if !hosts.is_empty() {
                let g = hosts[rng.range(0, hosts.len() - 1)];
                let ds = defs(&sources[g]);
                let funcs: Vec<&Def> = ds.iter().filter(|d| !d.is_class && d.indent == 0).collect();
                let func = funcs[rng.range(0, funcs.len() - 1)].name.clone();
                let class = ds
                    .iter()
                    .find(|d| d.is_class)
                    .expect("the host has a class")
                    .name
                    .clone();
                let methods: Vec<String> = ds
                    .iter()
                    .filter(|d| d.indent == 4)
                    .map(|d| d.name.clone())
                    .collect();
                out.push_str(&format!(
                    "from {} import {func}, {class}\n\n\n",
                    dotted(&files[g])
                ));
                callee = Some(func);
                base = Some((class, methods));
            }
            for f in 0..3 {
                out.push_str(&format!("def n{stem}_{f}(a, b):\n    total = a + len(b)\n"));
                if let Some(c) = &callee {
                    out.push_str(&format!("    total += {c}(a, b)\n"));
                }
                out.push_str("    return total\n\n\n");
            }
            match &base {
                Some((class, _)) => out.push_str(&format!("class N{stem}({class}):\n")),
                None => out.push_str(&format!("class N{stem}:\n")),
            }
            out.push_str("    def __init__(self, a):\n");
            if base.is_some() {
                out.push_str("        super().__init__(a)\n");
            }
            out.push_str("        self.a = a\n");
            out.push_str(&format!(
                "\n    def mn{stem}(self, a, b):\n        total = a\n"
            ));
            if let Some((_, methods)) = &base
                && !methods.is_empty()
            {
                let m = &methods[rng.range(0, methods.len() - 1)];
                out.push_str(&format!("        total += self.{m}(a, b)\n"));
            }
            out.push_str("        return total\n");
            write_file(dir, &rel, &out);
            changed.insert(rel);
        }
        Edit::DeleteFile => {
            let file = files[rng.range(0, files.len() - 1)].clone();
            fs::remove_file(dir.join(&file)).expect("delete a synthetic file");
            changed.insert(file);
        }
        Edit::BreakSyntax => {
            let k = rng.range(0, files.len() - 1);
            write_file(dir, &files[k], &format!("{}\ndef broken(:\n", sources[k]));
            changed.insert(files[k].clone());
        }
    }
    changed
}
