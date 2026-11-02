//! `harbormap index`: read a project into the facts store, print what was found and,
//! with `--watch`, keep the store current.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::Context;

use hm_index::index::{Delta, IndexReport, Indexer};
use hm_index::location::{facts_db, worktree_root};
use hm_index::watch::{Batch, Watcher};
use hm_store::Store;

use crate::cli::IndexArgs;
use crate::status::{printable, write_stdout};

/// How long the watch loop waits for a batch before it looks around again.
const POLL: Duration = Duration::from_secs(1);

pub fn run(args: &IndexArgs, json: bool) -> anyhow::Result<ExitCode> {
    let start = match &args.path {
        Some(path) => path.clone(),
        None => std::env::current_dir().context("cannot read the current folder")?,
    };
    let root = worktree_root(&start)?;
    let mut indexer = open(&root)?;
    let report = indexer.full()?;
    let text = if json && args.watch {
        // A stream: one object per line, the report first.
        format!("{}\n", serde_json::to_string(&report)?)
    } else if json {
        format!("{}\n", serde_json::to_string_pretty(&report)?)
    } else {
        render(&folder_name(&root), &report)
    };
    write_stdout(&text)?;
    if args.watch {
        watch(&mut indexer, &root, json)?;
    }
    Ok(ExitCode::SUCCESS)
}

/// The indexer for the worktree at `root`, over its store in the data directory.
pub fn open(root: &Path) -> anyhow::Result<Indexer> {
    let db = facts_db(root);
    let store = Store::open(&db).with_context(|| format!("cannot open {}", db.display()))?;
    Ok(Indexer::open(root, store)?)
}

fn folder_name(root: &Path) -> String {
    root.file_name().map_or_else(
        || root.display().to_string(),
        |n| n.to_string_lossy().into(),
    )
}

/// The names the report uses for languages, as printed, with whether L1 reads them.
const LANGS: &[(&str, &str, bool)] = &[
    ("typescript", "TypeScript", true),
    ("tsx", "TSX", true),
    ("javascript", "JavaScript", true),
    ("python", "Python", false),
    ("go", "Go", false),
    ("java", "Java", false),
    ("csharp", "C#", false),
    ("rust", "Rust", false),
];

/// The printed name of the language the store calls `name`.
pub fn lang_label(name: &str) -> &str {
    LANGS
        .iter()
        .find(|(key, _, _)| *key == name)
        .map_or(name, |(_, label, _)| label)
}

/// The short text report; the numbers come from `report`, the wording is fixed here.
pub fn render(name: &str, report: &IndexReport) -> String {
    let total: u32 = report.files.values().sum();
    let count = |map: &std::collections::BTreeMap<String, u32>, key: &str| {
        map.get(key).copied().unwrap_or(0)
    };
    let mut out = format!(
        "Indexed {} ({total} {})\n",
        printable(name),
        plural(total, "file", "files")
    );

    let mut languages: Vec<String> = LANGS
        .iter()
        .filter(|(key, _, read)| *read && count(&report.files, key) > 0)
        .map(|(key, label, _)| format!("{label} {}", count(&report.files, key)))
        .collect();
    let unread: Vec<String> = LANGS
        .iter()
        .filter(|(key, _, read)| !*read && count(&report.files, key) > 0)
        .map(|(key, label, _)| format!("{label} {}", count(&report.files, key)))
        .collect();
    if !unread.is_empty() {
        languages.push(format!("not yet supported: {}", unread.join(", ")));
    }
    line(&mut out, &languages.join(" · "));

    let classes: Vec<String> = [
        ("test", "Tests"),
        ("generated", "Generated"),
        ("doc", "Docs"),
        ("infra", "Infra"),
    ]
    .iter()
    .filter(|(key, _)| count(&report.classes, key) > 0)
    .map(|(key, label)| format!("{label} {}", count(&report.classes, key)))
    .collect();
    line(&mut out, &classes.join(" · "));

    let mut folders: Vec<(&str, u32)> = report
        .skipped
        .iter()
        .filter_map(|(key, n)| key.strip_prefix("dir:").map(|dir| (dir, *n)))
        .collect();
    // Biggest first, then by name, so the order does not depend on how the map sorts.
    folders.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let mut skipped: Vec<String> = folders
        .iter()
        .map(|(dir, n)| {
            format!(
                "{} ({n} {})",
                printable(dir),
                plural(*n, "folder", "folders")
            )
        })
        .collect();
    let large = count(&report.skipped, "too_large");
    if large > 0 {
        skipped.push(format!(
            "{large} {} too large to read",
            plural(large, "file", "files")
        ));
    }
    let binary = count(&report.skipped, "binary");
    if binary > 0 {
        skipped.push(format!(
            "{binary} binary {}",
            plural(binary, "file", "files")
        ));
    }
    let unreadable = count(&report.skipped, "unreadable");
    if unreadable > 0 {
        skipped.push(format!(
            "{unreadable} unreadable {}",
            plural(unreadable, "file or folder", "files or folders")
        ));
    }
    if !skipped.is_empty() {
        line(
            &mut out,
            &format!("Not on this map: {}", skipped.join(", ")),
        );
    }
    if !report.ignore_files.is_empty() {
        let files: Vec<String> = report.ignore_files.iter().map(|f| printable(f)).collect();
        line(&mut out, &format!("Ignore files: {}", files.join(", ")));
    }

    line(
        &mut out,
        &format!(
            "Symbols {} · imports {} · calls {} · unresolved {} · external {}",
            report.symbols,
            count(&report.edges, "imports"),
            count(&report.edges, "calls"),
            count(&report.refs, "unresolved"),
            count(&report.refs, "external"),
        ),
    );
    for note in &report.notes {
        line(&mut out, &format!("Note: {}", printable(note)));
    }
    line(&mut out, &format!("Took {} ms", report.took_ms));
    out
}

/// Adds an indented line; an empty one is left out.
fn line(out: &mut String, text: &str) {
    if !text.is_empty() {
        out.push_str("  ");
        out.push_str(text);
        out.push('\n');
    }
}

fn plural<'a>(n: u32, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

/// One line per settled batch of changes, until the process is stopped: text, or with
/// `json` one JSON object per line.
fn watch(indexer: &mut Indexer, root: &Path, json: bool) -> anyhow::Result<()> {
    let mut watcher = start_watcher(indexer, root)?;
    // A save made while the first pass ran may have come before the watch began, so
    // the watch begins with a rescan. Such a catch-up prints a line only when it
    // found a change.
    let mut catch_up = true;
    // A batch that failed is not applied again, so the next batch rescans.
    let mut failed = false;
    loop {
        let quiet = catch_up;
        let mut batch = if std::mem::take(&mut catch_up) {
            Batch {
                paths: BTreeSet::new(),
                rescan: true,
                first_event: Instant::now(),
            }
        } else {
            let Some(batch) = watcher.recv_timeout(POLL) else {
                continue;
            };
            batch
        };
        batch.rescan |= std::mem::take(&mut failed);
        let rules = indexer.walk_rules().cloned();
        let applied = if batch.rescan {
            indexer.rescan()
        } else {
            indexer.apply(&batch.paths)
        };
        let delta = match applied {
            Ok(delta) => delta,
            Err(error) => {
                eprintln!("harbormap: {error}");
                failed = true;
                continue;
            }
        };
        if indexer.walk_rules() != rules.as_ref() {
            // An ignore file changed. The watcher still filters with the old rules
            // (and on Linux lacks watches for the folders the new ones admit), so a
            // new one replaces it, and a rescan finds what changed in between.
            watcher = start_watcher(indexer, root)?;
            catch_up = true;
        }
        if quiet && delta == Delta::default() {
            continue;
        }
        let took = batch.first_event.elapsed();
        let text = if json {
            batch_json(&batch, took, &delta)
        } else {
            batch_line(&batch, took, &delta)
        };
        if !write_line(&text)? {
            return Ok(());
        }
    }
}

/// A watcher that filters with the indexer's current rules and watches the folders
/// its last walk admitted.
fn start_watcher(indexer: &Indexer, root: &Path) -> anyhow::Result<Watcher> {
    let rules = indexer
        .walk_rules()
        .cloned()
        .context("the project was not read yet")?;
    Watcher::start(root, rules, indexer.admitted_dirs())
        .context("cannot watch the project for changes")
}

/// The files a delta touched, changed and removed ones together.
fn delta_files(delta: &Delta) -> u32 {
    (delta.files_changed.len() + delta.files_removed.len()) as u32
}

/// `src/db.ts · 1 file · 7 ms · +0 −0 edges`
fn batch_line(batch: &Batch, took: Duration, delta: &Delta) -> String {
    let n = delta_files(delta);
    let what = match (batch.rescan, batch.paths.first()) {
        (true, _) | (_, None) => "rescan".to_string(),
        (false, Some(first)) => printable(first),
    };
    format!(
        "{what} · {n} {} · {} ms · +{} −{} edges",
        plural(n, "file", "files"),
        took.as_millis(),
        delta.edges_added,
        delta.edges_removed,
    )
}

/// `{"paths":[...],"files":1,"took_ms":7,"edges_added":0,"edges_removed":0}`
fn batch_json(batch: &Batch, took: Duration, delta: &Delta) -> String {
    serde_json::json!({
        "paths": batch.paths,
        "files": delta_files(delta),
        "took_ms": took.as_millis() as u64,
        "edges_added": delta.edges_added,
        "edges_removed": delta.edges_removed,
    })
    .to_string()
}

/// Prints one line and flushes. `false` when the reader has gone away.
fn write_line(text: &str) -> anyhow::Result<bool> {
    let mut stdout = std::io::stdout().lock();
    match writeln!(stdout, "{text}").and_then(|()| stdout.flush()) {
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        other => other.map(|()| true).map_err(Into::into),
    }
}
