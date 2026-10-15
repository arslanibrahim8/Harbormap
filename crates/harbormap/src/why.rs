//! `harbormap why <port|name>`: who holds a port, or what a service is, at the chosen
//! level of detail.

use std::process::ExitCode;

use serde::Serialize;

use hm_core::model::{Section, Service};
use hm_core::signature::Level;
use hm_doctor::cards::{Card, CardContext, port_cards, service_card};
use hm_doctor::render::tilde;
use hm_project::paths::within;

use crate::cli::WhyArgs;
use crate::source::Input;
use crate::status::{printable, write_stdout};

#[derive(Serialize)]
struct WhyOutput<'a> {
    target: &'a str,
    cards: Vec<Card>,
}

pub fn run(input: &Input, args: &WhyArgs, json: bool, level: Level) -> anyhow::Result<ExitCode> {
    let ctx = CardContext {
        snapshot: &input.snapshot,
        signatures: &input.signatures,
        texts: &input.texts,
        level,
        home: input.home.as_deref(),
    };
    let target = printable(&args.target);
    let digits = args.target.strip_prefix(':').unwrap_or(&args.target);
    let cards = if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
        match digits.parse::<u16>() {
            Ok(port) if port > 0 => port_cards(&ctx, port),
            _ => {
                eprintln!("harbormap: {target} is not a port number (1 to 65535).");
                return Ok(ExitCode::from(1));
            }
        }
    } else {
        match find_service(input, &args.target) {
            Found::One(service) => vec![service_card(&ctx, service)],
            Found::None => {
                eprintln!(
                    "harbormap: nothing called \"{target}\" is running. Run `harbormap status` to see the names."
                );
                return Ok(ExitCode::from(1));
            }
            Found::Many(services) => {
                eprintln!(
                    "harbormap: several services are called \"{target}\". Ask for one by its port:"
                );
                for service in services {
                    let ports: Vec<String> = service
                        .ports
                        .iter()
                        .map(|port| format!(":{port}"))
                        .collect();
                    let name = printable(&qualified_name(input, service));
                    match ports.first() {
                        Some(port) => eprintln!("  harbormap why {port}   {name}"),
                        None => eprintln!("  {name}"),
                    }
                }
                return Ok(ExitCode::from(1));
            }
        }
    };
    let text = if json {
        let output = WhyOutput {
            target: &args.target,
            cards,
        };
        format!("{}\n", serde_json::to_string_pretty(&output)?)
    } else {
        let blocks: Vec<String> = cards
            .iter()
            .map(|card| {
                let mut block = format!("{}\n", printable(&card.title));
                for line in &card.lines {
                    block.push_str(&format!("  {}\n", printable(line)));
                }
                block
            })
            .collect();
        blocks.join("\n")
    };
    write_stdout(&text)?;
    Ok(ExitCode::SUCCESS)
}

enum Found<'a> {
    None,
    One(&'a Service),
    Many(Vec<&'a Service>),
}

/// `name` or `project/name`, case-insensitive. Among several matches, the opened
/// project wins, then the exact spelling, then the worktree Harbormap was started in.
fn find_service<'a>(input: &'a Input, target: &str) -> Found<'a> {
    let snapshot = &input.snapshot;
    let (project_name, name) = match target.split_once('/') {
        Some((project, name)) => (Some(project), name),
        None => (None, target),
    };
    let mut matches: Vec<&Service> = snapshot
        .services
        .iter()
        .filter(|service| service.name.eq_ignore_ascii_case(name))
        .filter(|service| match project_name {
            Some(wanted) => {
                project_of(input, service).is_some_and(|p| p.eq_ignore_ascii_case(wanted))
            }
            None => true,
        })
        .collect();
    if matches.len() > 1 {
        let in_project: Vec<&Service> = matches
            .iter()
            .copied()
            .filter(|service| service.section == Section::Project)
            .collect();
        if !in_project.is_empty() {
            matches = in_project;
        }
    }
    // `Web` and `web` can both exist; the exact spelling wins.
    if matches.len() > 1 && matches.iter().any(|service| service.name == name) {
        matches.retain(|service| service.name == name);
    }
    if matches.len() > 1 {
        let windows = input.snapshot.host.os == "windows";
        let mut here: Vec<&Service> = matches
            .iter()
            .copied()
            .filter(|service| {
                service
                    .worktree
                    .as_deref()
                    .is_some_and(|worktree| within(&input.launch_dir, worktree, windows))
            })
            .collect();
        // A worktree can sit inside another; the deepest one is where Harbormap started.
        let deepest = here
            .iter()
            .filter_map(|service| service.worktree.as_deref())
            .map(|worktree| worktree.as_os_str().len())
            .max();
        here.retain(|service| {
            service
                .worktree
                .as_deref()
                .map(|worktree| worktree.as_os_str().len())
                == deepest
        });
        if here.len() == 1 {
            matches = here;
        }
    }
    match matches.len() {
        0 => Found::None,
        1 => Found::One(matches[0]),
        _ => Found::Many(matches),
    }
}

fn project_of<'a>(input: &'a Input, service: &Service) -> Option<&'a str> {
    let id = service.project_id.as_deref()?;
    input
        .snapshot
        .projects
        .iter()
        .find(|project| project.id == id)
        .map(|project| project.name.as_str())
}

fn qualified_name(input: &Input, service: &Service) -> String {
    let name = match project_of(input, service) {
        Some(project) => format!("{project}/{}", service.name),
        None => service.name.clone(),
    };
    match &service.worktree {
        Some(worktree) => format!("{name}  ({})", tilde(worktree, input.home.as_deref())),
        None => name,
    }
}
