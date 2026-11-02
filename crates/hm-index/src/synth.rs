//! Synthetic TypeScript projects and edits for property tests and the latency
//! harness. Everything is a pure function of the seed and the tree, so a failure
//! reproduces from its seed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// Modules per folder.
const FOLDER_SIZE: usize = 20;
/// Every this-many-th module is a barrel.
const BARREL_EVERY: usize = 10;
/// Barrel chains longer than this are not followed when scanning a tree.
const MAX_HOPS: usize = 12;

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

/// One re-export line of a barrel.
enum Reexport {
    /// `export * from "<module>"`.
    Star(usize),
    /// `export { <name> as <alias> } from "<module>"`.
    Named(usize, String, String),
}

struct Module {
    /// Exported functions (empty for a barrel).
    funcs: Vec<String>,
    /// Re-exports (empty for a plain module).
    reexports: Vec<Reexport>,
}

fn is_barrel(i: usize) -> bool {
    i.is_multiple_of(BARREL_EVERY)
}

fn module_path(i: usize) -> String {
    format!("src/d{:03}/m{:04}.ts", i / FOLDER_SIZE, i)
}

/// The relative specifier from module `from` to module `to` (no extension).
fn relative_spec(from: usize, to: usize) -> String {
    let (from_folder, to_folder) = (from / FOLDER_SIZE, to / FOLDER_SIZE);
    if from_folder == to_folder {
        format!("./m{to:04}")
    } else {
        format!("../d{to_folder:03}/m{to:04}")
    }
}

fn alias_spec(to: usize) -> String {
    format!("@/d{:03}/m{to:04}", to / FOLDER_SIZE)
}

fn write_file(dir: &Path, rel: &str, content: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().expect("a file path has a parent"))
        .expect("create the folder of a synthetic file");
    fs::write(&path, content).expect("write a synthetic file");
}

/// Writes a deterministic TypeScript project into `dir`: a `tsconfig.json` with
/// `@/*` → `src/*`, and `files` modules in folders of 20. Plain modules export 3–6
/// functions, each calling 1–4 functions imported from up to 3 other modules, by a
/// relative or an `@/` specifier. Every 10th module is a barrel that re-exports
/// from the modules after it, and often from the next barrel as well, so
/// re-exports chain over several hops; other modules import through barrels.
pub fn project(dir: &Path, seed: u64, files: u32) {
    let n = files as usize;
    let mut rng = Rng(seed);
    write_file(
        dir,
        "tsconfig.json",
        "{\n  \"compilerOptions\": { \"paths\": { \"@/*\": [\"src/*\"] } }\n}\n",
    );

    // Plan first: a module's imports depend on what the barrels re-export.
    let mut mods: Vec<Module> = (0..n)
        .map(|i| {
            let count = if is_barrel(i) { 0 } else { rng.range(3, 6) };
            Module {
                funcs: (0..count).map(|k| format!("f{i}_{k}")).collect(),
                reexports: Vec::new(),
            }
        })
        .collect();
    // `vis[i]` is every name another module can import from module `i`. Barrels
    // only look forward, so planning from the end sees each source's names first.
    let mut vis: Vec<Vec<String>> = vec![Vec::new(); n];
    for i in (0..n).rev() {
        if is_barrel(i) {
            let cands: Vec<usize> = (i + 1..(i + BARREL_EVERY).min(n)).collect();
            let want = rng.range(2, 3);
            let mut reexports = Vec::new();
            for src in rng.pick_distinct(&cands, want) {
                if rng.chance(50) {
                    reexports.push(Reexport::Star(src));
                } else {
                    let name = mods[src].funcs[rng.range(0, mods[src].funcs.len() - 1)].clone();
                    reexports.push(Reexport::Named(src, name.clone(), format!("x{i}_{name}")));
                }
            }
            // A chain: this barrel re-exports the next one.
            let next = i + BARREL_EVERY;
            if next < n && !vis[next].is_empty() && rng.chance(70) {
                if rng.chance(50) {
                    reexports.push(Reexport::Star(next));
                } else {
                    let name = vis[next][rng.range(0, vis[next].len() - 1)].clone();
                    reexports.push(Reexport::Named(next, name.clone(), format!("x{i}_{name}")));
                }
            }
            mods[i].reexports = reexports;
        }
        let mut names = mods[i].funcs.clone();
        for r in &mods[i].reexports {
            match r {
                Reexport::Star(src) => names.extend(vis[*src].iter().cloned()),
                Reexport::Named(_, _, alias) => names.push(alias.clone()),
            }
        }
        vis[i] = names;
    }

    for (i, m) in mods.iter().enumerate() {
        let source = if is_barrel(i) {
            render_barrel(m, i)
        } else {
            render_module(m, &vis, i, &mut rng)
        };
        write_file(dir, &module_path(i), &source);
    }
}

fn render_barrel(m: &Module, i: usize) -> String {
    if m.reexports.is_empty() {
        return "export {};\n".to_string();
    }
    let mut out = String::new();
    for r in &m.reexports {
        match r {
            Reexport::Star(src) => {
                out.push_str(&format!("export * from \"{}\";\n", relative_spec(i, *src)));
            }
            Reexport::Named(src, name, alias) => out.push_str(&format!(
                "export {{ {name} as {alias} }} from \"{}\";\n",
                relative_spec(i, *src)
            )),
        }
    }
    out
}

fn render_module(m: &Module, vis: &[Vec<String>], i: usize, rng: &mut Rng) -> String {
    // What module `j` offers this one: never its own functions (a barrel can
    // re-export them), nor a name it imports already.
    let importable = |j: usize, imported: &[String]| -> Vec<String> {
        vis[j]
            .iter()
            .filter(|nm| !m.funcs.contains(nm) && !imported.contains(nm))
            .cloned()
            .collect()
    };
    let mut targets: Vec<usize> = Vec::new();
    let wanted = rng.range(1, 3);
    for _ in 0..40 {
        if targets.len() == wanted {
            break;
        }
        let j = rng.range(0, vis.len() - 1);
        if j != i && !targets.contains(&j) && !importable(j, &[]).is_empty() {
            targets.push(j);
        }
    }
    let mut out = String::new();
    let mut imported: Vec<String> = Vec::new();
    for j in targets {
        let pool = importable(j, &imported);
        if pool.is_empty() {
            continue;
        }
        let want = rng.range(1, pool.len().min(2));
        let picks = rng.pick_distinct(&(0..pool.len()).collect::<Vec<_>>(), want);
        let names: Vec<String> = picks.iter().map(|&k| pool[k].clone()).collect();
        let spec = if rng.chance(50) {
            relative_spec(i, j)
        } else {
            alias_spec(j)
        };
        out.push_str(&format!(
            "import {{ {} }} from \"{spec}\";\n",
            names.join(", ")
        ));
        imported.extend(names);
    }
    out.push('\n');
    for name in &m.funcs {
        out.push_str(&format!(
            "export function {name}(a: number, b: string): number {{\n"
        ));
        let mut terms = String::new();
        if !imported.is_empty() {
            for c in 0..rng.range(1, 4) {
                let callee = &imported[rng.range(0, imported.len() - 1)];
                out.push_str(&format!("  const v{c} = {callee}(a, b);\n"));
                terms.push_str(&format!(" + v{c}"));
            }
        }
        out.push_str(&format!("  return a + b.length{terms};\n}}\n\n"));
    }
    out
}

/// What [`apply_edit`] does to a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// A new statement in one function; no signature changes.
    BodyChange,
    /// A new defaulted parameter on one exported function.
    SignatureChange,
    /// Renames an exported function that a barrel re-exports (through any number
    /// of barrels), and nothing else, so importers go unresolved.
    Rename,
    /// A new import of another module's function, called from one function.
    AddImport,
    /// Removes one import line; its calls stay.
    RemoveImport,
    /// A new module that imports one existing function.
    AddFile,
    /// Deletes one module.
    DeleteFile,
    /// Appends an unterminated function to one module.
    BreakSyntax,
}

/// Every `.ts` file under `dir`, as sorted `/`-separated relative paths.
fn ts_files(dir: &Path) -> Vec<String> {
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
            } else if name.ends_with(".ts") {
                out.insert(rel);
            }
        }
    }
    let mut set = BTreeSet::new();
    walk(dir, "", &mut set);
    set.into_iter().collect()
}

fn read(dir: &Path, rel: &str) -> String {
    fs::read_to_string(dir.join(rel)).expect("read a synthetic file")
}

/// The exported functions of `src`: (name, index of the declaration line). The
/// `broken` function that [`Edit::BreakSyntax`] appends is not one of them.
fn functions(src: &str) -> Vec<(String, usize)> {
    src.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let rest = l.strip_prefix("export function ")?;
            let name = &rest[..rest.find('(')?];
            (!name.starts_with("broken")).then(|| (name.to_string(), i))
        })
        .collect()
}

/// The module that the relative `spec` names, seen from the file `from`.
fn resolve_relative(from: &str, spec: &str) -> Option<String> {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for seg in spec.split('/') {
        match seg {
            "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(format!("{}.ts", parts.join("/")))
}

/// A re-export line of a barrel found in a tree: the target file, and for a named
/// one the (name, alias).
enum Line {
    Star(String),
    Named(String, String, String),
}

/// A function reached through barrels.
#[derive(Clone)]
struct Through {
    /// The file that declares it, and its name there.
    file: String,
    name: String,
    /// The name a module importing from the barrel sees.
    seen_as: String,
    /// The barrels between the importer and the declaration.
    hops: usize,
}

/// The re-exports of a tree, by barrel.
struct Barrels<'a> {
    dir: &'a Path,
    lines: BTreeMap<String, Vec<Line>>,
    memo: BTreeMap<String, Vec<Through>>,
}

impl<'a> Barrels<'a> {
    /// Reads every re-export line (`export * from` or `export { a as b } from`) of
    /// `files`. A file with such a line is a barrel.
    fn scan(dir: &'a Path, files: &[String]) -> Barrels<'a> {
        let mut lines: BTreeMap<String, Vec<Line>> = BTreeMap::new();
        for f in files {
            let src = read(dir, f);
            for l in src.lines() {
                let Some(spec) = l
                    .contains(" from \"")
                    .then(|| l.split('"').nth(1))
                    .flatten()
                else {
                    continue;
                };
                let Some(target) = resolve_relative(f, spec).filter(|t| files.contains(t)) else {
                    continue;
                };
                let line = if l.starts_with("export * from ") {
                    Line::Star(target)
                } else if let Some((name, alias)) = l
                    .strip_prefix("export { ")
                    .and_then(|r| r.split(" }").next())
                    .and_then(|r| r.split_once(" as "))
                {
                    Line::Named(target, name.to_string(), alias.to_string())
                } else {
                    continue;
                };
                lines.entry(f.clone()).or_default().push(line);
            }
        }
        Barrels {
            dir,
            lines,
            memo: BTreeMap::new(),
        }
    }

    /// What `file` (a barrel) lets importers reach: lines into plain modules give
    /// their functions, lines into barrels pass on what those give.
    fn of(&mut self, file: &str, depth: usize) -> Vec<Through> {
        if let Some(done) = self.memo.get(file) {
            return done.clone();
        }
        if depth > MAX_HOPS {
            return Vec::new();
        }
        let mut out = Vec::new();
        for i in 0..self.lines.get(file).map_or(0, Vec::len) {
            let (target, pick) = match &self.lines[file][i] {
                Line::Star(t) => (t.clone(), None),
                Line::Named(t, name, alias) => (t.clone(), Some((name.clone(), alias.clone()))),
            };
            let below: Vec<Through> = if self.lines.contains_key(&target) {
                self.of(&target, depth + 1)
            } else {
                functions(&read(self.dir, &target))
                    .into_iter()
                    .map(|(name, _)| Through {
                        file: target.clone(),
                        seen_as: name.clone(),
                        name,
                        hops: 0,
                    })
                    .collect()
            };
            for t in below {
                match &pick {
                    None => out.push(Through {
                        hops: t.hops + 1,
                        ..t
                    }),
                    Some((name, alias)) if t.seen_as == *name => out.push(Through {
                        seen_as: alias.clone(),
                        hops: t.hops + 1,
                        ..t
                    }),
                    Some(_) => {}
                }
            }
        }
        self.memo.insert(file.to_string(), out.clone());
        out
    }

    /// Every function that some barrel re-exports.
    fn all(&mut self) -> Vec<Through> {
        let barrels: Vec<String> = self.lines.keys().cloned().collect();
        barrels.iter().flat_map(|b| self.of(b, 0)).collect()
    }
}

/// The `@/` specifier of a `src/...` file.
fn alias_of(file: &str) -> String {
    let stem = file.strip_suffix(".ts").unwrap_or(file);
    format!("@/{}", stem.strip_prefix("src/").unwrap_or(stem))
}

/// `src` with the line at `idx` replaced.
fn replace_line(src: &str, idx: usize, new: &str) -> String {
    let mut lines: Vec<&str> = src.lines().collect();
    lines[idx] = new;
    lines.join("\n") + "\n"
}

/// `src` with `stmt` inserted before the first `  return` after the declaration
/// line `at`.
fn insert_before_return(src: &str, at: usize, stmt: &str) -> String {
    let mut lines: Vec<&str> = src.lines().collect();
    let ret = (at..lines.len())
        .find(|&i| lines[i].starts_with("  return "))
        .unwrap_or(at + 1);
    lines.insert(ret, stmt);
    lines.join("\n") + "\n"
}

/// `src` with a body or a signature edit in one of its functions, or `None` when
/// it has no function to edit.
fn edit_function_source(src: &str, seed: u64, kind: Edit, rng: &mut Rng) -> Option<String> {
    let funcs = functions(src);
    if funcs.is_empty() {
        return None;
    }
    let at = funcs[rng.range(0, funcs.len() - 1)].1;
    Some(match kind {
        Edit::SignatureChange => {
            let line = src.lines().nth(at).unwrap_or_default();
            let new_line =
                line.replace("): number {", &format!(", x{seed}: number = 0): number {{"));
            replace_line(src, at, &new_line)
        }
        _ => insert_before_return(src, at, &format!("  const t{seed} = {seed};")),
    })
}

/// Writes `new` to `rel` when it differs from `old`, and says whether it did.
fn write_if_changed(dir: &Path, rel: &str, old: &str, new: &str) -> bool {
    if old == new {
        return false;
    }
    write_file(dir, rel, new);
    true
}

/// A body or signature edit in one function of the file `rel`, chosen from `seed`.
/// Unlike [`apply_edit`] it reads and writes that one file only, so a harness can
/// time saves without a tree scan. A body edit adds a statement and a signature
/// edit adds a defaulted parameter named after `seed`, so use a new seed for each
/// save of the same file. Returns `{rel}`, or an empty set when the file has no
/// function to edit.
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

/// Applies one edit to the project in `dir`, chosen deterministically from the
/// tree and `seed`, and returns the changed relative paths (`/`-separated; the
/// deleted path for [`Edit::DeleteFile`]), exactly the files whose bytes changed.
/// The set is empty only when the tree has nothing the edit could touch. It scans
/// the whole tree; use [`edit_function`] to edit a file you already know.
pub fn apply_edit(dir: &Path, seed: u64, edit: Edit) -> BTreeSet<String> {
    let mut rng = Rng(seed ^ ((edit as u64 + 1) << 56));
    let files = ts_files(dir);
    let mut changed = BTreeSet::new();
    if files.is_empty() {
        return changed;
    }
    // Files with at least one exported function, for the edits that need one.
    let with_funcs: Vec<String> = files
        .iter()
        .filter(|f| !functions(&read(dir, f)).is_empty())
        .cloned()
        .collect();
    match edit {
        Edit::BodyChange | Edit::SignatureChange => {
            if with_funcs.is_empty() {
                return changed;
            }
            let file = with_funcs[rng.range(0, with_funcs.len() - 1)].clone();
            let src = read(dir, &file);
            if let Some(new) = edit_function_source(&src, seed, edit, &mut rng)
                && write_if_changed(dir, &file, &src, &new)
            {
                changed.insert(file);
            }
        }
        Edit::AddImport => {
            if with_funcs.is_empty() {
                return changed;
            }
            let file = with_funcs[rng.range(0, with_funcs.len() - 1)].clone();
            let src = read(dir, &file);
            let funcs = functions(&src);
            let at = funcs[rng.range(0, funcs.len() - 1)].1;
            let others: Vec<&String> = with_funcs.iter().filter(|f| **f != file).collect();
            if others.is_empty() {
                return changed;
            }
            let target = others[rng.range(0, others.len() - 1)];
            let callee = functions(&read(dir, target))[0].0.clone();
            let local = format!("added{seed}");
            let body =
                insert_before_return(&src, at, &format!("  const {local}v = {local}(a, b);"));
            let new = format!(
                "import {{ {callee} as {local} }} from \"{}\";\n{body}",
                alias_of(target)
            );
            if write_if_changed(dir, &file, &src, &new) {
                changed.insert(file);
            }
        }
        Edit::Rename => {
            // A function that some barrel re-exports, through any number of
            // barrels; any function when the tree has no barrel.
            let reexported: BTreeSet<(String, String)> = Barrels::scan(dir, &files)
                .all()
                .into_iter()
                .map(|t| (t.file, t.name))
                .collect();
            let pool: Vec<(String, String)> = if reexported.is_empty() {
                with_funcs
                    .iter()
                    .flat_map(|f| {
                        functions(&read(dir, f))
                            .into_iter()
                            .map(|(name, _)| (f.clone(), name))
                    })
                    .collect()
            } else {
                reexported.into_iter().collect()
            };
            if pool.is_empty() {
                return changed;
            }
            let (file, name) = pool[rng.range(0, pool.len() - 1)].clone();
            let src = read(dir, &file);
            let at = functions(&src)
                .into_iter()
                .find(|(n, _)| *n == name)
                .expect("the function was found in this file")
                .1;
            let line = src.lines().nth(at).unwrap_or_default();
            let new_line = line.replacen(
                &format!("export function {name}("),
                &format!("export function {name}R("),
                1,
            );
            if write_if_changed(dir, &file, &src, &replace_line(&src, at, &new_line)) {
                changed.insert(file);
            }
        }
        Edit::RemoveImport => {
            let with_imports: Vec<&String> = files
                .iter()
                .filter(|f| read(dir, f).lines().any(|l| l.starts_with("import ")))
                .collect();
            if with_imports.is_empty() {
                return changed;
            }
            let file = with_imports[rng.range(0, with_imports.len() - 1)].clone();
            let src = read(dir, &file);
            let imports: Vec<usize> = src
                .lines()
                .enumerate()
                .filter(|(_, l)| l.starts_with("import "))
                .map(|(i, _)| i)
                .collect();
            let drop = imports[rng.range(0, imports.len() - 1)];
            let kept: Vec<&str> = src
                .lines()
                .enumerate()
                .filter(|(i, _)| *i != drop)
                .map(|(_, l)| l)
                .collect();
            if write_if_changed(dir, &file, &src, &(kept.join("\n") + "\n")) {
                changed.insert(file);
            }
        }
        Edit::AddFile => {
            let mut k = 0;
            let rel = loop {
                let rel = format!("src/added/n{seed}_{k}.ts");
                if !dir.join(&rel).exists() {
                    break rel;
                }
                k += 1;
            };
            let stem = format!("n{seed}_{k}");
            let mut out = String::new();
            let mut callee = None;
            if !with_funcs.is_empty() {
                let target = &with_funcs[rng.range(0, with_funcs.len() - 1)];
                let name = functions(&read(dir, target))[0].0.clone();
                out.push_str(&format!(
                    "import {{ {name} }} from \"{}\";\n\n",
                    alias_of(target)
                ));
                callee = Some(name);
            }
            for f in 0..3 {
                out.push_str(&format!(
                    "export function {stem}_{f}(a: number, b: string): number {{\n"
                ));
                match &callee {
                    Some(c) => out.push_str(&format!("  return {c}(a, b) + {f};\n}}\n\n")),
                    None => out.push_str(&format!("  return a + b.length + {f};\n}}\n\n")),
                }
            }
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
            let src = read(dir, &file);
            write_file(dir, &file, &format!("{src}\nexport function broken( {{\n"));
            changed.insert(file);
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use tempfile::TempDir;

    use super::*;
    use crate::facts::ParseStatus;
    use crate::lang::Lang;
    use crate::ts::extract;

    const ALL_EDITS: [Edit; 8] = [
        Edit::BodyChange,
        Edit::SignatureChange,
        Edit::Rename,
        Edit::AddImport,
        Edit::RemoveImport,
        Edit::AddFile,
        Edit::DeleteFile,
        Edit::BreakSyntax,
    ];

    /// Every file under `dir`: relative path and bytes.
    fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(base: &Path, rel: &str, out: &mut BTreeMap<String, Vec<u8>>) {
            for entry in fs::read_dir(base).unwrap().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let rel = if rel.is_empty() {
                    name
                } else {
                    format!("{rel}/{name}")
                };
                if entry.path().is_dir() {
                    walk(&entry.path(), &rel, out);
                } else {
                    out.insert(rel, fs::read(entry.path()).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(dir, "", &mut out);
        out
    }

    /// The paths whose bytes differ between two trees, or that exist in one only.
    fn diff(a: &BTreeMap<String, Vec<u8>>, b: &BTreeMap<String, Vec<u8>>) -> BTreeSet<String> {
        a.keys()
            .chain(b.keys())
            .filter(|k| a.get(*k) != b.get(*k))
            .cloned()
            .collect()
    }

    fn status(dir: &Path, rel: &str) -> ParseStatus {
        extract(&read(dir, rel), Lang::TypeScript).status
    }

    fn assert_all_parse(dir: &Path) {
        for rel in ts_files(dir) {
            assert_eq!(status(dir, &rel), ParseStatus::Parsed, "{rel}");
        }
    }

    fn is_barrel_file(rel: &str) -> bool {
        let idx: usize = rel[rel.len() - 7..rel.len() - 3].parse().unwrap();
        is_barrel(idx)
    }

    fn imports_a_barrel(src: &str) -> bool {
        src.lines()
            .filter(|l| l.starts_with("import "))
            .filter_map(|l| l.split('"').nth(1))
            .any(|spec| {
                let idx: usize = spec[spec.len() - 4..].parse().unwrap();
                is_barrel(idx)
            })
    }

    #[test]
    fn synth_is_deterministic() {
        let (a, b) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        project(a.path(), 7, 50);
        project(b.path(), 7, 50);
        let tree_a = tree(a.path());
        assert_eq!(tree_a, tree(b.path()));
        // 50 modules plus the tsconfig, and every module parses cleanly.
        assert_eq!(tree_a.len(), 51);
        assert_all_parse(a.path());
        let c = TempDir::new().unwrap();
        project(c.path(), 8, 50);
        assert_ne!(tree_a, tree(c.path()));
    }

    #[test]
    fn the_project_has_the_promised_shape() {
        let dir = TempDir::new().unwrap();
        project(dir.path(), 3, 45);
        let files = ts_files(dir.path());
        assert_eq!(files.len(), 45);
        assert!(files.contains(&"src/d002/m0044.ts".to_string()));
        let barrel = read(dir.path(), "src/d000/m0010.ts");
        assert!(barrel.contains(" from \"./m00"), "{barrel}");
        let mut seen_alias_import = false;
        let mut through_barrel = false;
        for rel in &files {
            let src = read(dir.path(), rel);
            seen_alias_import |= src.contains("from \"@/d");
            through_barrel |= !is_barrel_file(rel) && imports_a_barrel(&src);
            if !is_barrel_file(rel) {
                let n = functions(&src).len();
                assert!((3..=6).contains(&n), "{rel}: {n} functions");
            }
        }
        assert!(seen_alias_import && through_barrel);
    }

    #[test]
    fn a_module_never_imports_its_own_functions() {
        for (seed, files) in [(7, 200), (1, 60), (5, 30)] {
            let dir = TempDir::new().unwrap();
            project(dir.path(), seed, files);
            for rel in ts_files(dir.path()) {
                let src = read(dir.path(), &rel);
                let own: Vec<String> = functions(&src).into_iter().map(|(n, _)| n).collect();
                for line in src.lines().filter(|l| l.starts_with("import ")) {
                    let names = line.split(['{', '}']).nth(1).unwrap();
                    for name in names.split(',').map(str::trim) {
                        assert!(!own.iter().any(|o| o == name), "{rel} imports {name}");
                    }
                }
            }
        }
    }

    #[test]
    fn barrels_chain_over_several_hops() {
        let mut deepest = 0;
        for seed in 0..10 {
            let dir = TempDir::new().unwrap();
            project(dir.path(), seed, 100);
            assert_all_parse(dir.path());
            let files = ts_files(dir.path());
            let hops = Barrels::scan(dir.path(), &files)
                .all()
                .iter()
                .map(|t| t.hops)
                .max()
                .unwrap();
            deepest = deepest.max(hops);
        }
        assert!(deepest >= 4, "the longest chain has {deepest} hops");
    }

    #[test]
    fn each_edit_changes_files_and_keeps_the_tree_parsing() {
        for edit in ALL_EDITS {
            let dir = TempDir::new().unwrap();
            project(dir.path(), 11, 30);
            let before = tree(dir.path());
            let changed = apply_edit(dir.path(), 5, edit);
            assert!(!changed.is_empty(), "{edit:?}");
            let after = tree(dir.path());
            assert_eq!(changed, diff(&before, &after), "{edit:?}");
            if edit == Edit::BreakSyntax {
                for rel in &changed {
                    assert_eq!(status(dir.path(), rel), ParseStatus::ParsedWithErrors);
                }
            } else {
                assert_all_parse(dir.path());
            }
        }
    }

    #[test]
    fn the_changed_set_is_exactly_the_files_whose_bytes_changed() {
        // Broken files stay in the tree, so later edits meet `export function broken(`.
        let dir = TempDir::new().unwrap();
        project(dir.path(), 4, 30);
        for seed in 0..40 {
            for edit in ALL_EDITS {
                let before = tree(dir.path());
                let changed = apply_edit(dir.path(), seed, edit);
                assert_eq!(changed, diff(&before, &tree(dir.path())), "{edit:?} {seed}");
            }
        }
    }

    #[test]
    fn rename_renames_a_function_that_a_barrel_re_exports() {
        for seed in 0..50 {
            let dir = TempDir::new().unwrap();
            project(dir.path(), 11, 30);
            let files = ts_files(dir.path());
            let before = tree(dir.path());
            let reachable: BTreeSet<(String, String)> = Barrels::scan(dir.path(), &files)
                .all()
                .into_iter()
                .map(|t| (t.file, t.name))
                .collect();
            let changed = apply_edit(dir.path(), seed, Edit::Rename);
            assert_eq!(changed.len(), 1);
            let file = changed.iter().next().unwrap();
            let old_src = String::from_utf8(before[file].clone()).unwrap();
            let new_src = read(dir.path(), file);
            let old_name = functions(&old_src)
                .into_iter()
                .map(|(n, _)| n)
                .find(|n| !new_src.contains(&format!("export function {n}(")))
                .expect("a function lost its name");
            assert!(new_src.contains(&format!("export function {old_name}R(")));
            // Some barrel line names the old function, or star-exports its file.
            let barrels = Barrels::scan(dir.path(), &files);
            let reexports = barrels.lines.values().flatten().any(|l| match l {
                Line::Named(t, n, _) => t == file && *n == old_name,
                Line::Star(t) => t == file,
            });
            assert!(reexports, "{file}::{old_name} is not re-exported");
            assert!(reachable.contains(&(file.clone(), old_name)));
        }
    }

    #[test]
    fn edit_function_edits_one_known_file_only() {
        for kind in [Edit::BodyChange, Edit::SignatureChange] {
            let dir = TempDir::new().unwrap();
            project(dir.path(), 11, 30);
            let before = tree(dir.path());
            let rel = "src/d000/m0003.ts";
            let changed = edit_function(dir.path(), rel, 9, kind);
            assert_eq!(changed, BTreeSet::from([rel.to_string()]));
            assert_eq!(diff(&before, &tree(dir.path())), changed);
            assert_all_parse(dir.path());
            // The same inputs on an identical tree give the same bytes.
            let other = TempDir::new().unwrap();
            project(other.path(), 11, 30);
            edit_function(other.path(), rel, 9, kind);
            assert_eq!(tree(dir.path()), tree(other.path()));
            // A new seed edits again.
            assert_eq!(edit_function(dir.path(), rel, 10, kind).len(), 1);
        }
    }

    #[test]
    fn edit_function_on_a_file_without_functions_changes_nothing() {
        let dir = TempDir::new().unwrap();
        project(dir.path(), 11, 30);
        let before = tree(dir.path());
        // m0000 is a barrel.
        assert!(edit_function(dir.path(), "src/d000/m0000.ts", 1, Edit::BodyChange).is_empty());
        assert_eq!(before, tree(dir.path()));
    }

    #[test]
    #[should_panic(expected = "edit_function takes BodyChange or SignatureChange")]
    fn edit_function_refuses_other_kinds() {
        let dir = TempDir::new().unwrap();
        project(dir.path(), 11, 30);
        edit_function(dir.path(), "src/d000/m0003.ts", 1, Edit::Rename);
    }

    #[test]
    fn small_and_large_projects_parse_and_survive_every_edit() {
        for (seed, files) in [(0, 1), (1, 2), (7, 8), (3, 11), (9, 200)] {
            let dir = TempDir::new().unwrap();
            project(dir.path(), seed, files);
            assert_all_parse(dir.path());
            for (k, edit) in ALL_EDITS.into_iter().enumerate() {
                if edit != Edit::BreakSyntax {
                    apply_edit(dir.path(), k as u64, edit);
                }
            }
            assert_all_parse(dir.path());
        }
    }

    #[test]
    fn edits_are_deterministic() {
        for edit in ALL_EDITS {
            let (a, b) = (TempDir::new().unwrap(), TempDir::new().unwrap());
            project(a.path(), 2, 30);
            project(b.path(), 2, 30);
            let ca = apply_edit(a.path(), 9, edit);
            let cb = apply_edit(b.path(), 9, edit);
            assert_eq!(ca, cb, "{edit:?}");
            assert_eq!(tree(a.path()), tree(b.path()), "{edit:?}");
        }
    }

    #[test]
    #[ignore = "timing, run with --release -- --ignored --nocapture"]
    fn project_generation_time() {
        for files in [1_000, 5_000] {
            let dir = TempDir::new().unwrap();
            let t = Instant::now();
            project(dir.path(), 7, files);
            eprintln!("project({files}): {:?}", t.elapsed());
        }
    }
}
