//! Cards for the resolved "shop" snapshot (the output of hm-project's resolution of its
//! `shop.json` scenario, saved as a fixture so these tests need no resolver).

use std::path::Path;

use hm_core::model::{BindScope, Listener, Owner, Proto, Section, Snapshot};
use hm_core::signature::Level;
use hm_doctor::cards::{Card, CardContext, port_cards, service_card};
use hm_doctor::packs::{self, Packs};
use hm_doctor::texts::{self, CardTexts};

fn snapshot() -> Snapshot {
    serde_json::from_str(include_str!("fixtures/shop-snapshot.json")).unwrap()
}

fn cards_for(snapshot: &Snapshot, port: u16, level: Level) -> Vec<Card> {
    let Packs { signatures, .. } = packs::load_embedded();
    let (cards, _): (CardTexts, _) = texts::load_embedded();
    let ctx = CardContext {
        snapshot,
        signatures: &signatures,
        texts: &cards,
        level,
        home: Some(Path::new("/Users/me")),
    };
    port_cards(&ctx, port)
}

fn one(snapshot: &Snapshot, port: u16, level: Level) -> Card {
    let mut cards = cards_for(snapshot, port, level);
    assert_eq!(cards.len(), 1, "{cards:#?}");
    cards.remove(0)
}

#[test]
fn a_project_server_reads_as_a_story_for_beginners() {
    let card = one(&snapshot(), 5173, Level::Beginner);
    assert_eq!(card.title, "Port 5173: web");
    assert_eq!(
        card.lines,
        vec![
            "web is part of the project shop.",
            "This is the live preview of your website while you work on it. It updates as soon as you save a file.",
            "Open it in your browser: http://localhost:5173",
            "Only this computer can open it.",
            "It has been running for 2 hours.",
        ]
    );
}

#[test]
fn builders_see_the_process_folder_and_environment() {
    let card = one(&snapshot(), 5173, Level::Builder);
    assert_eq!(card.title, ":5173 → web (Vite)");
    assert!(
        card.lines
            .contains(&"Process node, pid 802, started 2 h 3 min ago by me.".to_string())
    );
    assert!(
        card.lines
            .contains(&"Folder: apps/web (project shop, branch main)".to_string())
    );
    assert!(
        card.lines
            .contains(&"Listening on [::1]:5173 (this computer only).".to_string())
    );
    assert!(card.lines.contains(&"Port settings: PORT=5173".to_string()));
    assert!(
        card.lines
            .contains(&"Talks to: VITE_API_URL → http://localhost:8000".to_string())
    );

    // One card covers both sockets of a program and states the wider reach.
    let mut wide = snapshot();
    let mut all = wide
        .listeners
        .iter()
        .find(|listener| listener.port == 5173)
        .unwrap()
        .clone();
    all.addr = "::".parse().unwrap();
    all.bind = BindScope::All;
    wide.listeners.push(all);
    let card = one(&wide, 5173, Level::Builder);
    assert!(
        card.lines
            .contains(&"Listening on [::1]:5173, [::]:5173 (every network interface).".to_string())
    );

    // A service on two ports links the one asked about.
    let mut two = snapshot();
    let mut debug = two
        .listeners
        .iter()
        .find(|listener| listener.port == 8000)
        .unwrap()
        .clone();
    debug.port = 9229;
    two.listeners.push(debug);
    let api = two
        .services
        .iter_mut()
        .find(|service| service.name == "shop-api")
        .unwrap();
    api.ports.push(9229);
    api.urls.push("http://localhost:9229".into());
    let card = one(&two, 9229, Level::Builder);
    assert!(
        card.lines
            .contains(&"Open: http://localhost:9229".to_string())
    );
}

#[test]
fn experts_get_raw_facts() {
    let card = one(&snapshot(), 5173, Level::Expert);
    assert_eq!(card.title, ":5173/tcp → web [vite]");
    assert!(
        card.lines
            .contains(&"Signature: vite (sure) · role see · kind dev_server".to_string())
    );
    assert!(
        card.lines
            .contains(&"Service pids: 800, 801, 802".to_string())
    );
    assert!(card.lines.iter().any(|line| {
        line.starts_with("argv: node /Users/me/code/shop/apps/web/node_modules/.bin/vite")
    }));

    // Without a known parent the line still gives the memory.
    let mut orphan = snapshot();
    for process in &mut orphan.processes {
        if process.pid == 802 {
            process.ppid = None;
        }
    }
    let card = one(&orphan, 5173, Level::Expert);
    assert!(
        card.lines
            .iter()
            .any(|line| line.starts_with("Started ") && line.ends_with(") · rss 50 MB")),
        "{card:#?}"
    );
}

#[test]
fn a_compose_database_is_described_as_a_container() {
    let card = one(&snapshot(), 55432, Level::Builder);
    assert_eq!(card.title, ":55432 → db (PostgreSQL) · Docker");
    assert!(card.lines.contains(
        &"Container shop-db-1 from image postgres:16-alpine: running, healthy.".to_string()
    ));
    assert!(
        card.lines
            .contains(&"Ports: 0.0.0.0:55432 → 5432/tcp, [::]:55432 → 5432/tcp".to_string())
    );
    assert!(
        card.lines
            .contains(&"Engine: Docker Desktop 29.2.1".to_string())
    );
    let beginner = one(&snapshot(), 55432, Level::Beginner);
    assert_eq!(
        beginner.lines[0],
        "db is part of the project shop. It runs inside Docker."
    );
    assert!(
        beginner
            .lines
            .contains(&"Docker says it is healthy.".to_string())
    );

    // Without a health check the container line still says what runs.
    let redis = one(&snapshot(), 6380, Level::Builder);
    assert!(
        redis
            .lines
            .contains(&"Container lonely-redis from image redis:7: running.".to_string())
    );

    // Without a start time the expert line keeps health and restarts.
    let mut unstarted = snapshot();
    for container in &mut unstarted.containers {
        container.started_at = None;
    }
    let expert = one(&unstarted, 55432, Level::Expert);
    assert!(
        expert.lines.contains(
            &"Container 4f1c2a9e8b7d (shop-db-1) · image postgres:16-alpine · state running · health healthy · restarts 0"
                .to_string()
        ),
        "{expert:#?}"
    );
}

#[test]
fn other_projects_say_they_are_read_only() {
    let card = one(&snapshot(), 3000, Level::Builder);
    assert_eq!(card.title, ":3000 → blog (Next.js)");
    assert_eq!(
        card.lines.last().unwrap(),
        "Read-only here: it belongs to blog at ~/code/blog. Run harbormap in that folder to manage it."
    );
}

#[test]
fn system_apps_unknown_and_free_ports_have_their_own_cards() {
    let snapshot = snapshot();
    let airplay = one(&snapshot, 5000, Level::Beginner);
    assert_eq!(airplay.title, "Port 5000: AirPlay Receiver");
    assert!(airplay.lines[1].starts_with("This is AirPlay Receiver, a macOS feature"));

    let chrome = one(&snapshot, 9820, Level::Builder);
    assert_eq!(chrome.title, ":9820 → Google Chrome (pid 76915)");

    let agent = one(&snapshot, 41234, Level::Beginner);
    assert_eq!(agent.title, "Port 41234: Claude Code");
    assert!(agent.lines.contains(
        &"Claude Code is a program on your computer. It is not part of a project.".to_string()
    ));

    let hidden = one(&snapshot, 6000, Level::Beginner);
    assert_eq!(hidden.title, "Port 6000: a program Harbormap can't see");

    let free = one(&snapshot, 4321, Level::Beginner);
    assert_eq!(free.title, "Port 4321 is free");
    assert_eq!(free.lines, vec!["Nothing is using port 4321 right now."]);
}

#[test]
fn two_programs_splitting_a_port_get_two_cards() {
    let mut snapshot = snapshot();
    // Classic macOS surprise: something else holds 127.0.0.1 while Vite holds ::1.
    snapshot.listeners.push(Listener {
        proto: Proto::Tcp,
        addr: "127.0.0.1".parse().unwrap(),
        port: 5173,
        bind: BindScope::Loopback,
        owner: Owner::Process { pid: 76915 },
        section: Section::System,
        project_id: None,
        service_id: None,
        label: "Google Chrome (pid 76915)".into(),
    });
    let cards = cards_for(&snapshot, 5173, Level::Builder);
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0].title, ":5173 → web (Vite)");
    assert_eq!(cards[1].title, ":5173 → Google Chrome (pid 76915)");
}

#[test]
fn a_worker_without_ports_still_gets_a_card() {
    let snapshot = snapshot();
    let Packs { signatures, .. } = packs::load_embedded();
    let (cards, _) = texts::load_embedded();
    let ctx = CardContext {
        snapshot: &snapshot,
        signatures: &signatures,
        texts: &cards,
        level: Level::Builder,
        home: Some(Path::new("/Users/me")),
    };
    let worker = snapshot
        .services
        .iter()
        .find(|service| service.name == "shop-api-celery")
        .unwrap();
    let card = service_card(&ctx, worker);
    assert_eq!(card.title, "shop-api-celery (Celery)");
    assert!(card.lines[0].starts_with("Celery worker"));
}
