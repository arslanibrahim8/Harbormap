//! `harbormap status`: the opened project's services first, then a one-line summary
//! of everything else — or, with `--machine`, the full list.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use hm_core::model::{Confidence, Project, Section, Service, ServiceSource, Snapshot};
use hm_doctor::render::tilde;

use crate::cli::StatusArgs;
use crate::source::Input;

pub fn run(input: &Input, args: &StatusArgs, json: bool) -> anyhow::Result<ExitCode> {
    let text = if json {
        format!("{}\n", serde_json::to_string_pretty(&input.snapshot)?)
    } else {
        render(&input.snapshot, input.home.as_deref(), args.machine)
    };
    write_stdout(&text)?;
    Ok(ExitCode::SUCCESS)
}

/// Text from other programs (names, arguments, paths) for a terminal: control characters
/// and bidirectional overrides become U+FFFD, so they cannot move the cursor, retitle the
/// window or forge a line.
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|c| {
            let unsafe_char = c.is_control()
                || matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
            if unsafe_char { '\u{FFFD}' } else { c }
        })
        .collect()
}

/// Writes to stdout. A reader that stops early (`| head`) is not an error.
pub fn write_stdout(text: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

pub fn render(snapshot: &Snapshot, home: Option<&Path>, machine: bool) -> String {
    let mut out = String::new();
    // Without an opened project, the snapshot's note says so; the rest is the computer.
    let opened = snapshot.projects.iter().find(|project| project.opened);
    if let Some(project) = opened {
        project_block(&mut out, snapshot, project, home);
    }

    let elsewhere: Vec<&Service> = snapshot
        .services
        .iter()
        .filter(|service| service.section == Section::Elsewhere)
        .collect();
    let system = system_groups(snapshot);
    if machine || opened.is_none() {
        if !elsewhere.is_empty() {
            separate(&mut out);
            elsewhere_block(&mut out, snapshot, &elsewhere, home);
        }
        if !system.is_empty() {
            separate(&mut out);
            out.push_str("System & unknown\n");
            let width = system
                .iter()
                .map(|(ports, _)| ports.len())
                .max()
                .unwrap_or(0);
            for (ports, label) in &system {
                out.push_str(&format!("  {ports:<width$}  {}\n", printable(label)));
            }
        }
    } else if !elsewhere.is_empty() || !system.is_empty() {
        let system_ports: usize = system
            .iter()
            .map(|(ports, _)| ports.split(' ').count())
            .sum();
        let mut parts = Vec::new();
        if !elsewhere.is_empty() {
            parts.push(format!(
                "{} elsewhere (read-only)",
                plural(elsewhere.len(), "service")
            ));
        }
        if system_ports > 0 {
            parts.push(plural(system_ports, "system or unknown port"));
        }
        separate(&mut out);
        out.push_str(&format!(
            "Also on this computer: {}.\nRun `harbormap status --machine` to list them.\n",
            parts.join(" and "),
        ));
    }

    let engines: Vec<String> = snapshot
        .engines
        .iter()
        .filter(|engine| engine.reachable)
        .map(|engine| match &engine.version {
            Some(version) => format!("{} {version}", engine.name),
            None => engine.name.clone(),
        })
        .collect();
    if !engines.is_empty() {
        separate(&mut out);
        out.push_str(&format!("Docker: {}\n", engines.join(", ")));
    } else if !snapshot.notes.is_empty() {
        separate(&mut out);
    }
    for note in &snapshot.notes {
        out.push_str(&format!("Note: {}\n", note.text));
    }
    out
}

fn project_block(out: &mut String, snapshot: &Snapshot, project: &Project, home: Option<&Path>) {
    let branch = project
        .worktrees
        .first()
        .and_then(|worktree| worktree.branch.as_deref());
    let (name, root) = (
        printable(&project.name),
        printable(&tilde(&project.root, home)),
    );
    match (project.git, branch) {
        (true, Some(branch)) => {
            out.push_str(&format!("{name}  {root} · {}\n", printable(branch)));
        }
        (true, None) => out.push_str(&format!("{name}  {root}\n")),
        (false, _) => out.push_str(&format!("{name}  {root} (folder without git)\n")),
    }
    let services: Vec<&Service> = snapshot
        .services
        .iter()
        .filter(|service| service.project_id.as_deref() == Some(project.id.as_str()))
        .collect();
    if services.is_empty() {
        out.push_str("  Nothing is running for this project.\n");
        return;
    }
    let widths = widths(snapshot, &services);
    worktree_tables(out, snapshot, project, &services, home, "  ", widths);
}

/// One table, or one table per worktree when the services run in several worktrees.
fn worktree_tables(
    out: &mut String,
    snapshot: &Snapshot,
    project: &Project,
    services: &[&Service],
    home: Option<&Path>,
    indent: &str,
    widths: Widths,
) {
    let mut by_worktree: BTreeMap<usize, Vec<&Service>> = BTreeMap::new();
    for service in services {
        let index = project
            .worktrees
            .iter()
            .position(|worktree| Some(worktree.path.as_path()) == service.worktree.as_deref())
            .unwrap_or(0);
        by_worktree.entry(index).or_default().push(service);
    }
    // The worktree is named unless everything runs in the main one.
    if by_worktree.len() == 1 && by_worktree.contains_key(&0) {
        table(out, snapshot, services, indent, widths);
        return;
    }
    let nested = format!("{indent}  ");
    for (index, group) in by_worktree {
        let worktree = &project.worktrees[index];
        let label = worktree.branch.clone().unwrap_or_else(|| "detached".into());
        out.push_str(&format!(
            "{indent}{}  {}\n",
            printable(&label),
            printable(&tilde(&worktree.path, home))
        ));
        table(out, snapshot, &group, &nested, widths);
    }
}

fn elsewhere_block(
    out: &mut String,
    snapshot: &Snapshot,
    services: &[&Service],
    home: Option<&Path>,
) {
    out.push_str("Elsewhere on this computer (read-only)\n");
    let mut by_project: BTreeMap<Option<&str>, Vec<&Service>> = BTreeMap::new();
    for service in services {
        by_project
            .entry(service.project_id.as_deref())
            .or_default()
            .push(service);
    }
    let widths = widths(snapshot, services);
    // Projects in the snapshot's order, which is by name.
    for project in &snapshot.projects {
        let Some(group) = by_project.get(&Some(project.id.as_str())) else {
            continue;
        };
        out.push_str(&format!(
            "  {}  {}\n",
            printable(&project.name),
            printable(&tilde(&project.root, home))
        ));
        worktree_tables(out, snapshot, project, group, home, "    ", widths);
    }
    if let Some(group) = by_project.get(&None) {
        out.push_str("  Containers outside projects\n");
        table(out, snapshot, group, "    ", widths);
    }
}

type Widths = (usize, usize, usize);

/// Column widths shared by every table of one block, so columns line up.
fn widths(snapshot: &Snapshot, services: &[&Service]) -> Widths {
    let rows: Vec<[String; 4]> = services
        .iter()
        .map(|service| row(snapshot, service))
        .collect();
    let width = |column: usize| {
        rows.iter()
            .map(|row| row[column].chars().count())
            .max()
            .unwrap_or(0)
    };
    (width(0), width(1), width(2))
}

/// One row per service: name, framework, ports, state and the most useful detail.
fn table(
    out: &mut String,
    snapshot: &Snapshot,
    services: &[&Service],
    indent: &str,
    widths: Widths,
) {
    let (w0, w1, w2) = widths;
    for service in services {
        let row = row(snapshot, service);
        out.push_str(&format!(
            "{indent}{:<w0$}  {:<w1$}  {:<w2$}  {}\n",
            row[0], row[1], row[2], row[3]
        ));
    }
}

fn row(snapshot: &Snapshot, service: &Service) -> [String; 4] {
    let ports = if service.ports.is_empty() {
        "–".to_string()
    } else {
        service
            .ports
            .iter()
            .map(|port| format!(":{port}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut state = service.state.clone();
    if let Some(health) = &service.health {
        state.push_str(&format!(" · {health}"));
    }
    match &service.source {
        ServiceSource::Process { main_pid, .. } => state.push_str(&format!(" · pid {main_pid}")),
        ServiceSource::Container { container_id } => {
            if let Some(container) = snapshot.containers.iter().find(|c| &c.id == container_id) {
                state.push_str(&format!(" · container {}", container.name));
            }
        }
    }
    if let Some(url) = service.urls.first() {
        state.push_str(&format!("   {url}"));
    }
    let framework = match (&service.framework, service.confidence) {
        (Some(framework), Confidence::Probably) => format!("{framework}, probably"),
        (Some(framework), Confidence::Sure) => framework.clone(),
        (None, _) => "–".into(),
    };
    [
        printable(&service.name),
        printable(&framework),
        ports,
        printable(&state),
    ]
}

/// System and unknown listeners grouped by label: ("5000 7000" ports, label).
fn system_groups(snapshot: &Snapshot) -> Vec<(String, String)> {
    let mut groups: Vec<(Vec<u16>, String)> = Vec::new();
    for listener in snapshot
        .listeners
        .iter()
        .filter(|listener| listener.section == Section::System)
    {
        match groups
            .iter_mut()
            .find(|(_, label)| *label == listener.label)
        {
            Some((ports, _)) => {
                if !ports.contains(&listener.port) {
                    ports.push(listener.port);
                }
            }
            None => groups.push((vec![listener.port], listener.label.clone())),
        }
    }
    groups.sort_by_key(|(ports, _)| ports.iter().min().copied());
    groups
        .into_iter()
        .map(|(ports, label)| {
            let ports = ports
                .iter()
                .map(|port| format!(":{port}"))
                .collect::<Vec<_>>()
                .join(" ");
            (ports, label)
        })
        .collect()
}

/// A blank line between blocks, but not before the first one.
fn separate(out: &mut String) {
    if !out.is_empty() {
        out.push('\n');
    }
}

fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}
