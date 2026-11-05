//! `harbormap card <path>[#Symbol]`: what the map knows about a file or a symbol.

use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};

use hm_index::location::worktree_root;
use hm_index::query::{FileCard, SymbolCard, file_card, symbol_card};
use hm_project::git::discover;
use hm_project::paths::within;

use crate::cli::CardArgs;
use crate::index::{lang_label, open};
use crate::status::{printable, write_stdout};

pub fn run(args: &CardArgs, json: bool) -> anyhow::Result<ExitCode> {
    let (path, symbol) = match args.target.split_once('#') {
        Some((path, symbol)) => (path, Some(symbol)),
        None => (args.target.as_str(), None),
    };
    let (root, rel) = locate(Path::new(path))?;

    // A reader runs one indexing pass first (Learn spec §4), so the card shows the
    // tree as it is now. Files whose size and mtime match their rows are not read.
    let mut indexer = open(&root)?;
    indexer.full()?;
    let store = indexer.store();
    let missing = || {
        anyhow::anyhow!(
            "{} is not in the index (it may be ignored, skipped or not saved yet)",
            printable(&rel)
        )
    };
    let text = match symbol {
        None => {
            let card = file_card(store, &rel)?.ok_or_else(missing)?;
            if json {
                format!("{}\n", serde_json::to_string_pretty(&card)?)
            } else {
                render_file(&card)
            }
        }
        Some(symbol) => {
            // The file may be missing from the index or just lack the symbol.
            if file_card(store, &rel)?.is_none() {
                return Err(missing());
            }
            let Some(card) = symbol_card(store, &rel, symbol)? else {
                bail!(
                    "{} has no symbol called {}",
                    printable(&rel),
                    printable(symbol)
                );
            };
            if json {
                format!("{}\n", serde_json::to_string_pretty(&card)?)
            } else {
                render_symbol(&card)
            }
        }
    };
    write_stdout(&text)?;
    Ok(ExitCode::SUCCESS)
}

/// The worktree that holds `path` (relative to the current folder, or absolute) and the
/// path inside it, `/`-separated. The worktree comes from the target's own folder; the
/// current folder only resolves a relative target, except that outside any git
/// repository a project folder the target sits under is taken as the project.
fn locate(path: &Path) -> anyhow::Result<(PathBuf, String)> {
    let cwd = std::env::current_dir().context("cannot read the current folder")?;
    let abs = canonical(&cwd.join(path));
    // The nearest folder that exists, starting at the target itself or its parent.
    let mut dir = abs.as_path();
    while !dir.is_dir() {
        dir = dir.parent().unwrap_or(&cwd);
    }
    let mut root = worktree_root(dir)?;
    if discover(dir).is_none()
        && let Ok(base) = worktree_root(&cwd)
        && within(&abs, &base, cfg!(windows))
    {
        // No git around the target: the folder the user is working in is the project.
        root = base;
    }
    let rel = abs
        .strip_prefix(&root)
        .ok()
        .map(|rel| {
            rel.components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .filter(|rel| !rel.is_empty())
        .with_context(|| format!("{} is not a file in the project", path.display()))?;
    Ok((root, rel))
}

/// `path` made absolute and canonical even when its last parts do not exist: `.` and
/// `..` are resolved by name, then the longest existing prefix by the filesystem.
fn canonical(path: &Path) -> PathBuf {
    let mut parts: Vec<Component> = Vec::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if matches!(parts.last(), Some(Component::Normal(_))) => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let mut existing: PathBuf = parts.iter().collect();
    let mut tail = Vec::new();
    while !existing.exists() {
        let Some(name) = existing.file_name().map(ToOwned::to_owned) else {
            break;
        };
        tail.push(name);
        existing.pop();
    }
    let mut out = dunce::canonicalize(&existing).unwrap_or(existing);
    out.extend(tail.iter().rev());
    out
}

/// One line of text with control characters made harmless and runs of white space
/// folded, so a signature stays on its line.
fn one_line(text: &str) -> String {
    printable(&text.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn list(out: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("\n{title} ({})\n", items.len()));
    for item in items {
        out.push_str(&format!("  {}\n", printable(item)));
    }
}

pub fn render_file(card: &FileCard) -> String {
    let mut details = Vec::new();
    if let Some(lang) = &card.lang {
        details.push(lang_label(lang).to_string());
    }
    details.push(card.class.clone());
    details.push(plural(card.lines as usize, "line", "lines"));
    let mut out = format!("{}\n  {}\n", printable(&card.path), details.join(" · "));
    if !matches!(card.parse_status.as_str(), "parsed" | "parsed_with_errors") {
        out.push_str(&format!(
            "  Not read for code: {}\n",
            card.parse_status.replace('_', " ")
        ));
    } else if card.parse_status == "parsed_with_errors" {
        out.push_str("  The parser found syntax errors; the map may be incomplete.\n");
    }

    if !card.symbols.is_empty() {
        out.push_str(&format!("\nSymbols ({})\n", card.symbols.len()));
        let rows: Vec<[String; 3]> = card
            .symbols
            .iter()
            .map(|s| {
                [
                    printable(&s.name),
                    printable(&s.kind),
                    format!("lines {}-{}", s.l0, s.l1),
                ]
            })
            .collect();
        let width = |col: usize| {
            rows.iter()
                .map(|r| r[col].chars().count())
                .max()
                .unwrap_or(0)
        };
        let (name_w, kind_w, lines_w) = (width(0), width(1), width(2));
        for (s, [name, kind, lines]) in card.symbols.iter().zip(&rows) {
            let mut flags = Vec::new();
            if s.exported {
                flags.push("exported".to_string());
            }
            if s.called_from > 0 {
                flags.push(format!("called from {}", s.called_from));
            }
            let row = format!(
                "  {name:<name_w$}  {kind:<kind_w$}  {lines:<lines_w$}  {}",
                flags.join(" · ")
            );
            out.push_str(row.trim_end());
            out.push('\n');
        }
    }
    let labels: Vec<String> = card.routes.iter().map(|r| r.label.clone()).collect();
    list(&mut out, "Routes", &labels);
    list(&mut out, "Used by", &card.used_by);
    list(&mut out, "Uses", &card.uses);
    if card.unresolved > 0 {
        out.push_str(&format!(
            "\n{} could not be matched to code in the project.\n",
            plural(card.unresolved as usize, "reference", "references")
        ));
    }
    out
}

pub fn render_symbol(card: &SymbolCard) -> String {
    let mut out = format!(
        "{}#{}\n  {} · lines {}-{}\n",
        printable(&card.path),
        printable(&card.name),
        printable(&card.kind),
        card.l0,
        card.l1
    );
    let sig = one_line(&card.sig);
    if !sig.is_empty() {
        out.push_str(&format!("  {sig}\n"));
    }
    for role in &card.roles {
        out.push_str(&format!("  Role: {}\n", printable(role)));
    }
    list(&mut out, "Called from", &card.callers);
    list(&mut out, "Calls", &card.callees);
    list(
        &mut out,
        "Not matched to project code",
        &card.unresolved_calls,
    );
    out
}
