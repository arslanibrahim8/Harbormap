//! Synthetic TypeScript projects and edits for property tests and the latency
//! harness. Everything is a pure function of the seed and the tree, so a failure
//! reproduces from its seed.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Modules per folder.
const FOLDER_SIZE: usize = 20;
/// Every this-many-th module is a barrel.
const BARREL_EVERY: usize = 10;

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

/// The names other modules can import from module `i`.
fn visible(mods: &[Module], i: usize) -> Vec<String> {
    let m = &mods[i];
    let mut names = m.funcs.clone();
    for r in &m.reexports {
        match r {
            Reexport::Star(src) => names.extend(mods[*src].funcs.iter().cloned()),
            Reexport::Named(_, _, alias) => names.push(alias.clone()),
        }
    }
    names
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
/// from the modules after it, and other modules import through it.
pub fn project(dir: &Path, seed: u64, files: u32) {
    let n = files as usize;
    let mut rng = Rng(seed);
    write_file(
        dir,
        "tsconfig.json",
        "{\n  \"compilerOptions\": { \"paths\": { \"@/*\": [\"src/*\"] } }\n}\n",
    );

    // Plan first: a module's imports depend on what the barrels re-export.
    let mut mods: Vec<Module> = Vec::with_capacity(n);
    for i in 0..n {
        if is_barrel(i) {
            let cands: Vec<usize> = (i + 1..(i + BARREL_EVERY).min(n)).collect();
            let want = rng.range(2, 3);
            let mut reexports = Vec::new();
            for src in rng.pick_distinct(&cands, want) {
                if rng.chance(50) {
                    reexports.push(Reexport::Star(src));
                } else {
                    // Every plain module has an `f<src>_0`.
                    let name = format!("f{src}_0");
                    let alias = format!("x{name}");
                    reexports.push(Reexport::Named(src, name, alias));
                }
            }
            mods.push(Module {
                funcs: Vec::new(),
                reexports,
            });
        } else {
            let count = rng.range(3, 6);
            mods.push(Module {
                funcs: (0..count).map(|k| format!("f{i}_{k}")).collect(),
                reexports: Vec::new(),
            });
        }
    }

    for i in 0..n {
        let source = if is_barrel(i) {
            render_barrel(&mods[i], i)
        } else {
            render_module(&mods, i, &mut rng)
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

fn render_module(mods: &[Module], i: usize, rng: &mut Rng) -> String {
    let others: Vec<usize> = (0..mods.len())
        .filter(|&j| j != i && !visible(mods, j).is_empty())
        .collect();
    let mut out = String::new();
    let mut imported: Vec<String> = Vec::new();
    let targets = rng.range(1, 3);
    for j in rng.pick_distinct(&others, targets) {
        let pool = visible(mods, j);
        let want = rng.range(1, pool.len().min(2));
        let picks = rng.pick_distinct(&(0..pool.len()).collect::<Vec<_>>(), want);
        let names: Vec<String> = picks
            .iter()
            .map(|&k| pool[k].clone())
            .filter(|nm| !imported.contains(nm))
            .collect();
        if names.is_empty() {
            continue;
        }
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
    for name in &mods[i].funcs {
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
    /// Renames an exported function that a barrel re-exports, and nothing else, so
    /// importers go unresolved.
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

/// The exported functions of `src`: (name, index of the declaration line).
fn functions(src: &str) -> Vec<(String, usize)> {
    src.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let rest = l.strip_prefix("export function ")?;
            Some((rest[..rest.find('(')?].to_string(), i))
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

/// The files that barrels re-export from.
fn barrel_sources(dir: &Path, files: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for f in files {
        for line in read(dir, f).lines() {
            if !line.starts_with("export ") || !line.contains(" from \"") {
                continue;
            }
            let spec = line.split('"').nth(1).unwrap_or_default();
            out.extend(resolve_relative(f, spec));
        }
    }
    out
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

/// Applies one edit to the project in `dir`, chosen deterministically from the
/// tree and `seed`, and returns the changed relative paths (`/`-separated; the
/// deleted path for [`Edit::DeleteFile`]). The set is empty only when the tree has
/// nothing the edit could touch.
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
        Edit::BodyChange | Edit::SignatureChange | Edit::AddImport => {
            if with_funcs.is_empty() {
                return changed;
            }
            let file = with_funcs[rng.range(0, with_funcs.len() - 1)].clone();
            let src = read(dir, &file);
            let funcs = functions(&src);
            let at = funcs[rng.range(0, funcs.len() - 1)].1;
            let new = match edit {
                Edit::SignatureChange => {
                    let line = src.lines().nth(at).unwrap_or_default();
                    let new_line =
                        line.replace("): number {", &format!(", x{seed}: number = 0): number {{"));
                    replace_line(&src, at, &new_line)
                }
                Edit::BodyChange => {
                    insert_before_return(&src, at, &format!("  const t{seed} = {seed};"))
                }
                _ => {
                    let others: Vec<&String> = with_funcs.iter().filter(|f| **f != file).collect();
                    if others.is_empty() {
                        return changed;
                    }
                    let target = others[rng.range(0, others.len() - 1)];
                    let callee = functions(&read(dir, target))[0].0.clone();
                    let local = format!("added{seed}");
                    let body = insert_before_return(
                        &src,
                        at,
                        &format!("  const {local}v = {local}(a, b);"),
                    );
                    format!(
                        "import {{ {callee} as {local} }} from \"{}\";\n{body}",
                        alias_of(target)
                    )
                }
            };
            write_file(dir, &file, &new);
            changed.insert(file);
        }
        Edit::Rename => {
            // Prefer a function that a barrel re-exports.
            let sources: Vec<String> = barrel_sources(dir, &files)
                .into_iter()
                .filter(|s| with_funcs.contains(s))
                .collect();
            let pool = if sources.is_empty() {
                &with_funcs
            } else {
                &sources
            };
            if pool.is_empty() {
                return changed;
            }
            let file = pool[rng.range(0, pool.len() - 1)].clone();
            let src = read(dir, &file);
            let funcs = functions(&src);
            let (name, at) = funcs[rng.range(0, funcs.len() - 1)].clone();
            let line = src.lines().nth(at).unwrap_or_default();
            let new_line = line.replacen(
                &format!("export function {name}("),
                &format!("export function {name}R("),
                1,
            );
            write_file(dir, &file, &replace_line(&src, at, &new_line));
            changed.insert(file);
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
            write_file(dir, &file, &(kept.join("\n") + "\n"));
            changed.insert(file);
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
    use std::collections::BTreeMap;

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

    fn status(dir: &Path, rel: &str) -> ParseStatus {
        extract(&read(dir, rel), Lang::TypeScript).status
    }

    fn assert_all_parse(dir: &Path) {
        for rel in ts_files(dir) {
            assert_eq!(status(dir, &rel), ParseStatus::Parsed, "{rel}");
        }
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
    fn each_edit_changes_files_and_keeps_the_tree_parsing() {
        for edit in ALL_EDITS {
            let dir = TempDir::new().unwrap();
            project(dir.path(), 11, 30);
            let before = tree(dir.path());
            let changed = apply_edit(dir.path(), 5, edit);
            assert!(!changed.is_empty(), "{edit:?}");
            let after = tree(dir.path());
            for rel in &changed {
                match edit {
                    Edit::DeleteFile => {
                        assert!(before.contains_key(rel) && !after.contains_key(rel))
                    }
                    Edit::AddFile => assert!(!before.contains_key(rel) && after.contains_key(rel)),
                    _ => assert_ne!(before.get(rel), after.get(rel), "{edit:?} {rel}"),
                }
            }
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
    fn rename_renames_a_function_that_a_barrel_re_exports() {
        for seed in 0..20 {
            let dir = TempDir::new().unwrap();
            project(dir.path(), 11, 30);
            let files = ts_files(dir.path());
            let sources = barrel_sources(dir.path(), &files);
            let changed = apply_edit(dir.path(), seed, Edit::Rename);
            let file = changed.iter().next().unwrap();
            assert!(sources.contains(file), "{file} is not behind a barrel");
            let renamed = read(dir.path(), file);
            assert!(renamed.contains("R(a: number"), "{renamed}");
        }
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
}
