//! Three-level cards for `harbormap why`. Facts come from the snapshot; sentences come
//! from `packs/texts/cards.toml`. One card is made per distinct owner of a port, so an
//! IPv4 and an IPv6 socket of the same program share a card, while two programs that
//! split a port between IPv4 and IPv6 get two.

use std::path::Path;

use serde::Serialize;

use hm_core::model::{
    Confidence, Container, Listener, Owner, Process, Service, ServiceSource, Snapshot,
};
use hm_core::signature::{Level, SignatureSet};

use crate::render::{Facts, bytes, duration, join_argv, render_line, render_lines, tilde, utc};
use crate::texts::{CardTexts, LevelLines};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Card {
    pub title: String,
    pub lines: Vec<String>,
}

pub struct CardContext<'a> {
    pub snapshot: &'a Snapshot,
    pub signatures: &'a SignatureSet,
    pub texts: &'a CardTexts,
    pub level: Level,
    pub home: Option<&'a Path>,
}

/// Cards for everything listening on `port`. When nothing listens but a service claims
/// the port (a container Docker publishes without a userland proxy), that service's card
/// is returned; otherwise a "free" card.
pub fn port_cards(ctx: &CardContext<'_>, port: u16) -> Vec<Card> {
    let mut groups: Vec<(&Owner, Vec<&Listener>)> = Vec::new();
    for listener in ctx.snapshot.listeners.iter().filter(|l| l.port == port) {
        match groups
            .iter_mut()
            .find(|(owner, _)| **owner == listener.owner)
        {
            Some((_, members)) => members.push(listener),
            None => groups.push((&listener.owner, vec![listener])),
        }
    }
    if groups.is_empty() {
        if let Some(service) = ctx
            .snapshot
            .services
            .iter()
            .find(|service| service.ports.contains(&port))
        {
            return vec![service_card_at(ctx, service, Some(port), &[])];
        }
        let mut facts = Facts::new();
        facts.insert("port", port.to_string());
        return vec![card(ctx, &ctx.texts.free, &facts)];
    }
    groups
        .into_iter()
        .map(|(owner, listeners)| owner_card(ctx, owner, &listeners))
        .collect()
}

/// The card for one service (`harbormap why web`).
pub fn service_card(ctx: &CardContext<'_>, service: &Service) -> Card {
    let listeners: Vec<&Listener> = ctx
        .snapshot
        .listeners
        .iter()
        .filter(|listener| listener.service_id.as_deref() == Some(service.id.as_str()))
        .collect();
    let port = listeners
        .first()
        .map(|listener| listener.port)
        .or_else(|| service.ports.first().copied());
    service_card_at(ctx, service, port, &listeners)
}

fn owner_card(ctx: &CardContext<'_>, owner: &Owner, listeners: &[&Listener]) -> Card {
    let port = listeners[0].port;
    let service = listeners[0].service_id.as_deref().and_then(|id| {
        ctx.snapshot
            .services
            .iter()
            .find(|service| service.id == id)
    });
    if let Some(service) = service {
        return service_card_at(ctx, service, Some(port), listeners);
    }
    let mut facts = socket_facts(ctx, Some(port), listeners);
    match owner {
        Owner::Process { pid } => {
            if let Some(process) = find_process(ctx, *pid) {
                add_process_facts(ctx, &mut facts, process, None);
                facts.insert("program", program_name(ctx, process));
                add_signature_text(
                    ctx,
                    &mut facts,
                    process.signature.as_deref(),
                    Confidence::Sure,
                );
            }
            card(ctx, &ctx.texts.app, &facts)
        }
        Owner::System { reason, pid } => {
            facts.insert("reason", reason.clone());
            if let Some(process) = pid.and_then(|pid| find_process(ctx, pid)) {
                facts.insert("pid", process.pid.to_string());
                facts.insert("program", process.name.clone());
                if let Some(exe) = &process.exe {
                    facts.insert("exe", exe.display().to_string());
                }
                add_signature_text(
                    ctx,
                    &mut facts,
                    process.signature.as_deref(),
                    Confidence::Sure,
                );
            }
            card(ctx, &ctx.texts.system, &facts)
        }
        Owner::Unknown { reason } => {
            facts.insert("reason", reason.clone());
            card(ctx, &ctx.texts.unknown, &facts)
        }
        Owner::ContainerForwarder { container_id } => {
            // A container without a service cannot happen after resolution; describe it anyway.
            facts.insert("name", container_id.chars().take(12).collect());
            card(ctx, &ctx.texts.container, &facts)
        }
    }
}

fn service_card_at(
    ctx: &CardContext<'_>,
    service: &Service,
    port: Option<u16>,
    listeners: &[&Listener],
) -> Card {
    let mut facts = socket_facts(ctx, port, listeners);
    add_service_facts(ctx, &mut facts, service, port);
    let lines = match &service.source {
        ServiceSource::Process { main_pid, pids, .. } => {
            if let Some(process) = find_process(ctx, *main_pid) {
                add_process_facts(ctx, &mut facts, process, service.worktree.as_deref());
            }
            facts.insert(
                "pids",
                pids.iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            &ctx.texts.process
        }
        ServiceSource::Container { container_id } => {
            if let Some(container) = ctx
                .snapshot
                .containers
                .iter()
                .find(|container| &container.id == container_id)
            {
                add_container_facts(ctx, &mut facts, container);
            }
            &ctx.texts.container
        }
    };
    let mut built = card(ctx, lines, &facts);
    if service.section == hm_core::model::Section::Elsewhere && service.project_id.is_some() {
        built
            .lines
            .extend(render_lines(ctx.texts.elsewhere.get(ctx.level), &facts));
    }
    built
}

fn card(ctx: &CardContext<'_>, templates: &LevelLines, facts: &Facts) -> Card {
    let title = templates
        .title
        .as_ref()
        .and_then(|title| render_line(title.get(ctx.level), facts))
        .or_else(|| facts.get("port").map(|port| format!(":{port}")))
        .unwrap_or_default();
    Card {
        title,
        lines: render_lines(templates.get(ctx.level), facts),
    }
}

fn socket_facts(ctx: &CardContext<'_>, port: Option<u16>, listeners: &[&Listener]) -> Facts {
    let mut facts = Facts::new();
    if let Some(port) = port {
        facts.insert("port", port.to_string());
    }
    if !listeners.is_empty() {
        let addrs: Vec<String> = listeners
            .iter()
            .map(|listener| match listener.addr {
                std::net::IpAddr::V6(v6) => format!("[{v6}]:{}", listener.port),
                std::net::IpAddr::V4(v4) => format!("{v4}:{}", listener.port),
            })
            .collect();
        facts.insert("addr", addrs.join(", "));
        // One card covers every socket of its owner, so it states the widest reach.
        let reaches = |scope| listeners.iter().any(|listener| listener.bind == scope);
        let scope = if reaches(hm_core::model::BindScope::All) {
            "all"
        } else if reaches(hm_core::model::BindScope::Specific) {
            "specific"
        } else {
            "loopback"
        };
        if let Some(reach) = ctx.texts.reach.get(scope) {
            facts.insert("reach", reach.get(ctx.level).to_string());
        }
    }
    facts
}

fn add_service_facts(
    ctx: &CardContext<'_>,
    facts: &mut Facts,
    service: &Service,
    port: Option<u16>,
) {
    facts.insert("name", service.name.clone());
    if let Some(framework) = &service.framework {
        let framework = match service.confidence {
            Confidence::Probably => format!("{framework}, probably"),
            Confidence::Sure => framework.clone(),
        };
        facts.insert("framework", framework);
    }
    if let Some(signature) = &service.signature {
        facts.insert("signature", signature.clone());
    }
    facts.insert(
        "confidence",
        match service.confidence {
            Confidence::Sure => "sure".into(),
            Confidence::Probably => "probably".into(),
        },
    );
    facts.insert("role", snake(&service.role));
    facts.insert("kind", snake(&service.kind));
    // The URL of the port asked about, else the service's first one.
    let url = port
        .and_then(|port| {
            let suffix = format!(":{port}");
            service.urls.iter().find(|url| url.ends_with(&suffix))
        })
        .or_else(|| service.urls.first());
    if let Some(url) = url {
        facts.insert("url", url.clone());
    }
    add_signature_text(ctx, facts, service.signature.as_deref(), service.confidence);
    if let Some(project) = service.project_id.as_deref().and_then(|id| {
        ctx.snapshot
            .projects
            .iter()
            .find(|project| project.id == id)
    }) {
        facts.insert("project", project.name.clone());
        facts.insert("project_id", project.id.clone());
        facts.insert("project_root", tilde(&project.root, ctx.home));
        if let Some(worktree) = service.worktree.as_deref() {
            facts.insert("worktree", tilde(worktree, ctx.home));
            if let Some(branch) = project
                .worktrees
                .iter()
                .find(|candidate| candidate.path == worktree)
                .and_then(|candidate| candidate.branch.clone())
            {
                facts.insert("branch", branch);
            }
        }
    }
}

fn add_signature_text(
    ctx: &CardContext<'_>,
    facts: &mut Facts,
    signature: Option<&str>,
    confidence: Confidence,
) {
    if let Some(signature) = signature.and_then(|id| ctx.signatures.get(id)) {
        facts.insert("what", signature.texts.get(ctx.level).to_string());
        facts.insert("signature", signature.id.clone());
        if confidence == Confidence::Probably
            && let Some(guess) = &ctx.texts.guess
        {
            facts.insert("guess", guess.get(ctx.level).to_string());
        }
    }
}

fn add_process_facts(
    ctx: &CardContext<'_>,
    facts: &mut Facts,
    process: &Process,
    worktree: Option<&Path>,
) {
    facts.insert("process", process.name.clone());
    facts.insert("pid", process.pid.to_string());
    if let Some(user) = &process.user {
        facts.insert("user", user.clone());
    }
    facts.insert("uptime", duration(process.uptime_s, ctx.level));
    facts.insert("started_utc", utc(process.start_time));
    facts.insert("rss", bytes(process.rss_bytes));
    if let Some(exe) = &process.exe {
        facts.insert("exe", exe.display().to_string());
    }
    facts.insert("argv", join_argv(&process.argv));
    if let Some(cwd) = &process.cwd {
        facts.insert("cwd", tilde(cwd, ctx.home));
        let relative = worktree
            .and_then(|root| cwd.strip_prefix(root).ok())
            .map(|rest| {
                if rest.as_os_str().is_empty() {
                    ".".to_string()
                } else {
                    rest.display().to_string()
                }
            });
        facts.insert("cwd_rel", relative.unwrap_or_else(|| tilde(cwd, ctx.home)));
    }
    if let Some(ppid) = process.ppid {
        facts.insert("ppid", ppid.to_string());
        if let Some(parent) = find_process(ctx, ppid) {
            facts.insert("parent", parent.name.clone());
        }
    }
    if !process.env.ports.is_empty() {
        let ports: Vec<String> = process
            .env
            .ports
            .iter()
            .map(|entry| format!("{}={}", entry.name, entry.port))
            .collect();
        facts.insert("env_ports", ports.join(", "));
    }
    if !process.env.urls.is_empty() {
        let urls: Vec<String> = process
            .env
            .urls
            .iter()
            .map(|entry| {
                let scheme = if entry.scheme.is_empty() {
                    String::new()
                } else {
                    format!("{}://", entry.scheme)
                };
                let port = entry
                    .port
                    .map(|port| format!(":{port}"))
                    .unwrap_or_default();
                format!("{} → {scheme}{}{port}", entry.name, entry.host)
            })
            .collect();
        facts.insert("env_urls", urls.join(", "));
    }
}

fn add_container_facts(ctx: &CardContext<'_>, facts: &mut Facts, container: &Container) {
    facts.insert("container", container.name.clone());
    facts.insert("container_id", container.id.chars().take(12).collect());
    facts.insert("image", container.image.clone());
    facts.insert("state", container.state.clone());
    facts.insert("restarts", container.restart_count.to_string());
    if let Some(started) = &container.started_at {
        facts.insert("started_utc", started.clone());
    }
    if let Some(health) = container
        .health
        .as_deref()
        .and_then(|health| ctx.texts.health.get(health))
    {
        facts.insert("health", health.get(ctx.level).to_string());
    }
    let mapping: Vec<String> = container
        .ports
        .iter()
        .map(|binding| match binding.host_port {
            Some(host) => {
                let ip = binding
                    .host_ip
                    .map(|ip| match ip {
                        std::net::IpAddr::V6(v6) => format!("[{v6}]:"),
                        std::net::IpAddr::V4(v4) => format!("{v4}:"),
                    })
                    .unwrap_or_default();
                format!("{ip}{host} → {}/{}", binding.container_port, binding.proto)
            }
            None => format!(
                "{}/{} (not published)",
                binding.container_port, binding.proto
            ),
        })
        .collect();
    if !mapping.is_empty() {
        facts.insert("mapping", mapping.join(", "));
    }
    if !container.networks.is_empty() {
        facts.insert("networks", container.networks.join(", "));
    }
    let volumes: Vec<String> = container
        .mounts
        .iter()
        .filter(|mount| mount.kind == "volume")
        .filter_map(|mount| mount.name.clone())
        .collect();
    if !volumes.is_empty() {
        facts.insert("volumes", volumes.join(", "));
    }
    if let Some(compose) = &container.compose {
        facts.insert("compose_project", compose.project.clone());
        facts.insert("compose_service", compose.service.clone());
        let files: Vec<String> = compose
            .config_files
            .iter()
            .map(|file| tilde(file, ctx.home))
            .collect();
        if !files.is_empty() {
            facts.insert("compose_files", files.join(", "));
        }
        if let Some(dir) = &compose.working_dir {
            facts.insert("working_dir", tilde(dir, ctx.home));
        }
    }
    if let Some(engine) = ctx
        .snapshot
        .engines
        .iter()
        .find(|engine| engine.id == container.engine)
    {
        let name = match &engine.version {
            Some(version) => format!("{} {version}", engine.name),
            None => engine.name.clone(),
        };
        facts.insert("engine", name);
        facts.insert("engine_endpoint", engine.endpoint.clone());
    }
}

fn find_process<'a>(ctx: &CardContext<'a>, pid: u32) -> Option<&'a Process> {
    ctx.snapshot
        .processes
        .iter()
        .find(|process| process.pid == pid)
}

fn program_name(ctx: &CardContext<'_>, process: &Process) -> String {
    process
        .signature
        .as_deref()
        .and_then(|id| ctx.signatures.get(id))
        .map_or_else(|| process.name.clone(), |signature| signature.name.clone())
}

/// The serde name of an enum value (`Kind::DevServer` → `dev_server`).
fn snake<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}
