//! Walks a worktree: which files exist, what class each has, and which the indexer
//! will not parse (Learn spec §5.2).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::hash::content_hash;
use crate::lang::Lang;

/// Files larger than this are counted but not parsed.
pub const MAX_PARSE_BYTES: u64 = 1_048_576;

/// Directories that are never entered, matched by name. A dependency folder can hold
/// 100k files, so they are counted as one directory each, not file by file.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "vendor",
    "third_party",
    "dist",
    "build",
    "out",
    ".next",
    "target",
    "bin",
    "obj",
    "coverage",
    ".venv",
    "venv",
    "__pycache__",
    ".tox",
    "site-packages",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
];

/// The names of the ignore files the walk honours, highest precedence first.
pub(crate) const IGNORE_FILES: &[&str] = &[".harbormapignore", ".gitignore"];

/// How much of a file is read to detect binary and minified content.
const SNIFF_BYTES: u64 = 64 * 1024;
/// Binary files are recognised by a NUL byte in this many leading bytes.
const BINARY_BYTES: usize = 8 * 1024;
/// A file whose average line is longer than this (in the first 64 KiB) is minified.
const MINIFIED_AVG_LINE: usize = 500;
/// Minified detection needs at least this much text. A short file with one long line
/// is not worth skipping, and a generated barrel (`export { A, B, ... } from`) is such
/// a file whose facts later files need.
const MINIFIED_MIN_BYTES: usize = 16 * 1024;
/// How much of a file `classify` sees.
const HEAD_BYTES: usize = 1024;

/// What a file is for. Later views can hide whole classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileClass {
    Source,
    Test,
    Generated,
    Doc,
    Infra,
}

impl FileClass {
    /// The name used in the store and in printed output.
    pub fn as_str(&self) -> &'static str {
        match self {
            FileClass::Source => "source",
            FileClass::Test => "test",
            FileClass::Generated => "generated",
            FileClass::Doc => "doc",
            FileClass::Infra => "infra",
        }
    }
}

/// Why the indexer does not parse a file it saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// Over [`MAX_PARSE_BYTES`], or minified (very long lines).
    TooLarge,
    /// A NUL byte in the first 8 KiB.
    Binary,
    /// The file could not be opened: no permission, or (on Windows) another program
    /// holds it without sharing.
    Unreadable,
}

/// One file the walk admitted.
#[derive(Debug, Clone)]
pub struct WalkedFile {
    /// Path relative to the walk root, `/`-separated on every OS.
    pub rel: String,
    pub abs: PathBuf,
    pub size: u64,
    /// Modification time in nanoseconds since the epoch, 0 when unavailable.
    pub mtime_ns: i64,
    pub lang: Option<Lang>,
    /// Always `Generated` when the file is too large or binary; an unreadable file is
    /// classed by its path alone.
    pub class: FileClass,
    pub skip: Option<Skip>,
}

/// What a walk found.
#[derive(Debug, Clone)]
pub struct WalkResult {
    /// Sorted by `rel`.
    pub files: Vec<WalkedFile>,
    /// Skip-directory name to the number of such directories left unentered.
    pub skipped_dirs: BTreeMap<String, u32>,
    /// Admitted folders, relative and sorted; `""` is the root.
    pub dirs: Vec<String>,
    /// Folders that could not be read, and files that could not even be stat-ed,
    /// relative and sorted. Nothing below such a folder is seen. A file that can be
    /// stat-ed but not opened is in `files` with [`Skip::Unreadable`].
    pub unreadable: Vec<String>,
    /// Relative paths of the ignore files the walk applied, sorted: `.gitignore`,
    /// `.harbormapignore` and the repository's `.git/info/exclude`.
    pub ignore_files: Vec<String>,
    pub rules: WalkRules,
}

/// One ignore file, applying to its own folder and below.
#[derive(Debug, Clone)]
struct Matcher {
    /// The folder holding the file, relative to the root (`""` for the root).
    dir: String,
    /// How many folders deep `dir` is.
    depth: usize,
    /// Lower wins within one folder: `.harbormapignore`, `.gitignore`, then
    /// `.git/info/exclude`.
    priority: u8,
    /// The content hash of the ignore file, so that two sets of rules compare.
    hash: String,
    ignore: Gitignore,
}

impl PartialEq for Matcher {
    fn eq(&self, other: &Matcher) -> bool {
        (&self.dir, self.depth, self.priority, &self.hash)
            == (&other.dir, other.depth, other.priority, &other.hash)
    }
}

impl Eq for Matcher {}

/// The walker's include rules, for asking about one path without walking. Two rules
/// are equal when they come from the same ignore files with the same content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkRules {
    root: PathBuf,
    /// Deepest folder first; in one folder `.harbormapignore` before `.gitignore`.
    matchers: Vec<Matcher>,
}

impl WalkRules {
    /// Whether the walker would include the file at `rel` (`/`-separated, relative to
    /// the root). Every folder above the last component is a directory: it must not
    /// be `.git` or a skip directory, and no ignore file may hide it. The last
    /// component is taken to be a file, so only `.git` and ignore rules apply to it.
    pub fn admits(&self, rel: &str) -> bool {
        self.admits_as(rel, false)
    }

    /// Whether the walker would enter the folder at `rel`: like [`admits`], but the
    /// last component is a directory too, so skip names and dir-only ignore patterns
    /// (`ignored/`) apply to it.
    ///
    /// [`admits`]: WalkRules::admits
    pub fn admits_dir(&self, rel: &str) -> bool {
        self.admits_as(rel, true)
    }

    fn admits_as(&self, rel: &str, last_is_dir: bool) -> bool {
        let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
        let Some((last, dirs)) = parts.split_last() else {
            return true;
        };
        let never = |d: &str| d == ".git" || SKIP_DIRS.contains(&d);
        if dirs.iter().any(|d| never(d)) || *last == ".git" || (last_is_dir && never(last)) {
            return false;
        }
        // Like git, a path under an ignored folder stays hidden even if a deeper
        // ignore file whitelists it, so every prefix is checked.
        let mut prefix = String::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                prefix.push('/');
            }
            prefix.push_str(part);
            let is_dir = last_is_dir || i + 1 < parts.len();
            if self.hides(&prefix, is_dir) {
                return false;
            }
        }
        true
    }

    /// Whether the nearest ignore file with an opinion hides the path.
    fn hides(&self, rel: &str, is_dir: bool) -> bool {
        let abs = self.root.join(rel);
        for m in &self.matchers {
            let inside = m.dir.is_empty()
                || rel
                    .strip_prefix(m.dir.as_str())
                    .is_some_and(|rest| rest.starts_with('/'));
            if !inside {
                continue;
            }
            let verdict = m.ignore.matched(&abs, is_dir);
            if verdict.is_ignore() {
                return true;
            }
            if verdict.is_whitelist() {
                return false;
            }
        }
        false
    }
}

/// Walks `root`: every regular file outside skip directories and ignore rules.
/// Symlinks are not followed, so nothing outside the root is read and nothing inside
/// it is seen twice.
pub fn walk(root: &Path) -> io::Result<WalkResult> {
    let mut skipped_dirs = BTreeMap::<String, u32>::new();
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        // Only ignore files inside the root count, so the result does not depend on
        // where the root sits and `WalkRules` can reproduce it.
        .parents(false)
        // No `.ignore` / `.rgignore` files: Harbormap has its own `.harbormapignore`.
        .ignore(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .require_git(false)
        .follow_links(false)
        .add_custom_ignore_filename(".harbormapignore")
        .filter_entry(move |entry| {
            if entry.depth() == 0 {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            if name == ".git" {
                // A directory, or a file in a linked worktree; never reported.
                return false;
            }
            // Skip directories are counted by `count_skip_dirs`, which also sees the
            // ones that ignore rules hide before this filter runs.
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            !(is_dir && SKIP_DIRS.contains(&name.as_ref()))
        });

    let mut files = Vec::new();
    let mut dirs = Vec::new();
    let mut ignore_files = Vec::new();
    let mut matchers = Vec::new();
    let mut unreadable = BTreeSet::new();
    for entry in builder.build() {
        let entry = match entry {
            Ok(entry) => entry,
            // A file that vanished mid-walk is not an error.
            Err(err)
                if err
                    .io_error()
                    .is_some_and(|e| e.kind() == io::ErrorKind::NotFound) =>
            {
                continue;
            }
            // A folder that may not be read is counted, and the walk goes on.
            Err(err) => match (err.io_error(), error_path(&err)) {
                (Some(io), Some(path)) if is_unreadable(io) => {
                    unreadable.insert(rel_path(root, path));
                    continue;
                }
                _ => return Err(io::Error::other(err)),
            },
        };
        let rel = rel_path(root, entry.path());
        let Some(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            count_skip_dirs(entry.path(), &mut skipped_dirs);
            dirs.push(rel);
        } else if file_type.is_file() {
            let file = match walked_file(rel.clone(), entry.path()) {
                Ok(file) => file,
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) if is_unreadable(&err) => {
                    unreadable.insert(rel);
                    continue;
                }
                Err(err) => return Err(err),
            };
            // An ignore file the walker could not read applied nothing.
            let name = entry.file_name().to_string_lossy();
            if IGNORE_FILES.contains(&name.as_ref()) && file.skip != Some(Skip::Unreadable) {
                matchers.push(matcher_for(root, &rel, entry.path()));
                ignore_files.push(rel);
            }
            files.push(file);
        }
    }

    // The walker also honours `.git/info/exclude`; it has the lowest precedence.
    let exclude = root.join(".git").join("info").join("exclude");
    if exclude.is_file() {
        let mut builder = GitignoreBuilder::new(root);
        builder.add(&exclude);
        if let Ok(ignore) = builder.build() {
            matchers.push(Matcher {
                dir: String::new(),
                depth: 0,
                priority: 2,
                hash: hash_of(&exclude),
                ignore,
            });
            ignore_files.push(".git/info/exclude".to_string());
        }
    }

    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    // The walker reports a folder before it fails to read it.
    dirs.retain(|d| !unreadable.contains(d));
    dirs.sort();
    ignore_files.sort();
    // Deepest folder first, then by priority within a folder.
    matchers.sort_by_key(|m| (std::cmp::Reverse(m.depth), m.priority));
    Ok(WalkResult {
        files,
        skipped_dirs,
        dirs,
        unreadable: unreadable.into_iter().collect(),
        ignore_files,
        rules: WalkRules {
            root: root.to_path_buf(),
            matchers,
        },
    })
}

/// The path an error of the walker is about.
fn error_path(err: &ignore::Error) -> Option<&Path> {
    match err {
        ignore::Error::WithPath { path, .. } => Some(path),
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
            error_path(err)
        }
        _ => None,
    }
}

/// Whether reading failed because the path may not be read: no permission, or (on
/// Windows) a sharing or lock violation, as when another program holds the file.
pub(crate) fn is_unreadable(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::PermissionDenied
        || (cfg!(windows) && matches!(err.raw_os_error(), Some(32 | 33)))
}

/// Counts the skip-named directories directly inside `dir`, whether or not an ignore
/// rule hides them: only files hidden by the user's rules go unreported, not the
/// folders Harbormap itself decides never to enter.
fn count_skip_dirs(dir: &Path, counts: &mut BTreeMap<String, u32>) {
    let Ok(children) = std::fs::read_dir(dir) else {
        return;
    };
    for child in children.flatten() {
        let name = child.file_name();
        let name = name.to_string_lossy();
        if SKIP_DIRS.contains(&name.as_ref()) && child.file_type().is_ok_and(|t| t.is_dir()) {
            *counts.entry(name.into_owned()).or_insert(0) += 1;
        }
    }
}

/// Builds the matcher of the ignore file at `abs`, which sits at `rel` under `root`.
fn matcher_for(root: &Path, rel: &str, abs: &Path) -> Matcher {
    let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
    let depth = if dir.is_empty() {
        0
    } else {
        dir.matches('/').count() + 1
    };
    let priority = if name == ".harbormapignore" { 0 } else { 1 };
    let mut builder = GitignoreBuilder::new(abs.parent().unwrap_or(root));
    // A malformed line is skipped, as the walker itself does.
    builder.add(abs);
    let ignore = builder.build().unwrap_or_else(|_| Gitignore::empty());
    Matcher {
        dir: dir.to_string(),
        depth,
        priority,
        hash: hash_of(abs),
        ignore,
    }
}

/// The content hash of the file at `abs`, empty when it cannot be read.
fn hash_of(abs: &Path) -> String {
    std::fs::read(abs)
        .map(|bytes| content_hash(&bytes))
        .unwrap_or_default()
}

/// Reads the facts about one file: size, mtime, language, and whether it is skipped.
/// A file that can be stat-ed but not opened is [`Skip::Unreadable`]; one that cannot
/// even be stat-ed is an error.
pub(crate) fn walked_file(rel: String, abs: &Path) -> io::Result<WalkedFile> {
    let meta = std::fs::metadata(abs)?;
    let size = meta.len();
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_nanos()).ok())
        .unwrap_or(0);

    let mut sniff = Vec::new();
    let skip = if size > MAX_PARSE_BYTES {
        Some(Skip::TooLarge)
    } else {
        match File::open(abs).and_then(|f| f.take(SNIFF_BYTES).read_to_end(&mut sniff)) {
            Ok(_) => sniff_skip(&sniff),
            Err(err) if is_unreadable(&err) => Some(Skip::Unreadable),
            Err(err) => return Err(err),
        }
    };
    let class = match skip {
        // Nothing was read, so only the path says what the file is for.
        Some(Skip::Unreadable) => classify(&rel, b""),
        Some(_) => FileClass::Generated,
        None => classify(&rel, &sniff[..sniff.len().min(HEAD_BYTES)]),
    };
    Ok(WalkedFile {
        lang: Lang::from_path(Path::new(&rel)),
        rel,
        abs: abs.to_path_buf(),
        size,
        mtime_ns,
        class,
        skip,
    })
}

/// Binary (a NUL byte early on) or minified (long average line) content.
fn sniff_skip(sniff: &[u8]) -> Option<Skip> {
    if sniff[..sniff.len().min(BINARY_BYTES)].contains(&0) {
        return Some(Skip::Binary);
    }
    let lines = sniff.iter().filter(|&&b| b == b'\n').count() + 1;
    if sniff.len() >= MINIFIED_MIN_BYTES && sniff.len() / lines > MINIFIED_AVG_LINE {
        return Some(Skip::TooLarge);
    }
    None
}

/// `path` relative to `root` as `/`-separated components.
fn rel_path(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The class of the file at `rel`. `head` is up to the first 1 KiB of the file.
/// The first matching rule wins: test, generated, doc, infra, then source.
pub fn classify(rel: &str, head: &[u8]) -> FileClass {
    let mut segments: Vec<&str> = rel.split('/').collect();
    let name = segments.pop().unwrap_or("");
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);

    if segments
        .iter()
        .any(|s| matches!(*s, "test" | "tests" | "__tests__" | "testdata"))
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.ends_with("_test.go")
        || (name.starts_with("test_") && ext == "py")
        || name.ends_with("Test.java")
        || name.ends_with("Tests.cs")
        || name.ends_with("_test.py")
        || name == "conftest.py"
    {
        return FileClass::Test;
    }

    if name.contains(".gen.")
        || name.ends_with(".g.cs")
        || name.ends_with("_pb2.py")
        || name.ends_with("_pb2_grpc.py")
        || name.ends_with("_pb2.pyi")
        || name.ends_with(".pb.go")
        || name.ends_with(".min.js")
        || has_generated_marker(head)
    {
        return FileClass::Generated;
    }

    let is_code = Lang::from_path(Path::new(name)).is_some();
    if matches!(ext, "md" | "mdx" | "rst" | "txt" | "adoc")
        || (segments.contains(&"docs") && !is_code)
    {
        return FileClass::Doc;
    }

    if name.starts_with("Dockerfile")
        || (name.contains("compose") && matches!(ext, "yml" | "yaml"))
        || (segments.windows(2).any(|w| w == [".github", "workflows"]))
        || ext == "tf"
        || matches!(
            name,
            "Procfile" | "fly.toml" | "vercel.json" | "render.yaml"
        )
    {
        return FileClass::Infra;
    }

    FileClass::Source
}

/// Whether one of the first five lines says the file is generated.
fn has_generated_marker(head: &[u8]) -> bool {
    String::from_utf8_lossy(head).lines().take(5).any(|l| {
        l.contains("@generated") || l.contains("Code generated") || l.contains("auto-generated")
    })
}
