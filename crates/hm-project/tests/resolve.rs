//! Resolution against the recorded "shop" machine: one opened project with a linked
//! worktree, another project, Docker Desktop, macOS system owners, an AI agent and a
//! hidden owner.

use std::path::{Path, PathBuf};

use hm_core::model::{Owner, Section, ServiceSource, Snapshot};
use hm_core::raw::{RawListener, RawProcess};
use hm_doctor::packs::load_embedded;
use hm_project::{ResolveInput, Scenario, build_snapshot};

fn scenario() -> Scenario {
    serde_json::from_str(include_str!("fixtures/shop.json")).unwrap()
}

fn resolve(scenario: &Scenario) -> Snapshot {
    let packs = load_embedded();
    build_snapshot(ResolveInput {
        inventory: &scenario.inventory,
        world: &scenario.world,
        signatures: &packs.signatures,
        launch_dir: &scenario.launch_dir,
    })
}

fn service<'s>(snapshot: &'s Snapshot, name: &str, worktree: &str) -> &'s hm_core::model::Service {
    snapshot
        .services
        .iter()
        .find(|service| {
            service.name == name && service.worktree.as_deref() == Some(Path::new(worktree))
        })
        .unwrap_or_else(|| panic!("no service {name} in {worktree}"))
}

fn listener(snapshot: &Snapshot, port: u16) -> &hm_core::model::Listener {
    snapshot
        .listeners
        .iter()
        .find(|listener| listener.port == port)
        .unwrap_or_else(|| panic!("no listener on {port}"))
}

const SHOP: &str = "/Users/me/code/shop";
const FEATURE: &str = "/Users/me/code/shop-feature";

#[test]
fn the_launch_folder_opens_its_git_project_with_all_worktrees() {
    let snapshot = resolve(&scenario());
    let opened = snapshot.projects.iter().find(|p| p.opened).unwrap();
    assert_eq!(opened.root, PathBuf::from(SHOP));
    assert_eq!(opened.name, "shop");
    assert!(opened.git);
    assert_eq!(opened.worktrees.len(), 2);
    assert_eq!(snapshot.opened.as_deref(), Some(opened.id.as_str()));
}

#[test]
fn project_services_get_names_frameworks_and_process_trees() {
    let snapshot = resolve(&scenario());

    let web = service(&snapshot, "web", SHOP);
    assert_eq!(web.signature.as_deref(), Some("vite"));
    assert_eq!(web.section, Section::Project);
    assert_eq!(web.ports, vec![5173]);
    assert_eq!(web.urls, vec!["http://localhost:5173"]);
    assert_eq!(
        web.source,
        ServiceSource::Process {
            main_pid: 802,
            root_pid: 800,
            pids: vec![800, 801, 802]
        }
    );

    let api = service(&snapshot, "shop-api", SHOP);
    assert_eq!(api.signature.as_deref(), Some("fastapi"));
    assert_eq!(api.ports, vec![8000]);
    assert!(
        matches!(api.source, ServiceSource::Process { main_pid: 900, root_pid: 900, ref pids } if pids == &vec![900, 901])
    );

    let billing = service(&snapshot, "Billing", SHOP);
    assert_eq!(billing.signature.as_deref(), Some("dotnet"));
    assert!(matches!(
        billing.source,
        ServiceSource::Process {
            main_pid: 951,
            root_pid: 950,
            ..
        }
    ));

    let worker = service(&snapshot, "shop-api-celery", SHOP);
    assert_eq!(worker.signature.as_deref(), Some("celery"));
    assert!(worker.ports.is_empty());

    let db = service(&snapshot, "db", SHOP);
    assert_eq!(db.signature.as_deref(), Some("postgres"));
    assert_eq!(db.ports, vec![55432]);
    assert_eq!(db.health.as_deref(), Some("healthy"));
    assert_eq!(db.section, Section::Project);

    let feature_web = service(&snapshot, "web", FEATURE);
    assert_eq!(feature_web.ports, vec![5174]);
    assert_ne!(feature_web.id, web.id);
}

#[test]
fn other_projects_and_loose_containers_are_elsewhere() {
    let snapshot = resolve(&scenario());
    let blog = service(&snapshot, "blog", "/Users/me/code/blog");
    assert_eq!(blog.section, Section::Elsewhere);
    assert_eq!(blog.signature.as_deref(), Some("next"));
    let redis = snapshot
        .services
        .iter()
        .find(|s| s.name == "lonely-redis")
        .unwrap();
    assert_eq!(redis.section, Section::Elsewhere);
    assert_eq!(redis.project_id, None);
    assert_eq!(redis.signature.as_deref(), Some("redis"));
}

#[test]
fn every_listener_has_an_owner_and_a_section() {
    let snapshot = resolve(&scenario());

    assert_eq!(
        listener(&snapshot, 5000).owner,
        Owner::System {
            reason: "AirPlay Receiver".into(),
            pid: Some(640)
        }
    );
    assert_eq!(listener(&snapshot, 5000).section, Section::System);

    let db_port = listener(&snapshot, 55432);
    assert!(
        matches!(db_port.owner, Owner::ContainerForwarder { ref container_id } if container_id.starts_with("4f1c2a9e"))
    );
    assert_eq!(db_port.label, "db (PostgreSQL)");
    assert_eq!(db_port.section, Section::Project);

    let redis_port = listener(&snapshot, 6380);
    assert_eq!(redis_port.label, "lonely-redis (Redis)");

    assert_eq!(listener(&snapshot, 631).label, "cupsd (pid 412)");
    assert_eq!(listener(&snapshot, 9820).label, "Google Chrome (pid 76915)");

    let agent = listener(&snapshot, 41234);
    assert_eq!(agent.label, "Claude Code (pid 3000)");
    assert_eq!(agent.section, Section::System);
    assert_eq!(agent.service_id, None);

    assert!(matches!(
        listener(&snapshot, 6000).owner,
        Owner::Unknown { .. }
    ));
    assert!(
        snapshot
            .notes
            .iter()
            .any(|note| note.code == "owners_hidden")
    );
}

#[test]
fn harbormap_itself_is_never_a_service() {
    let snapshot = resolve(&scenario());
    assert!(
        snapshot
            .services
            .iter()
            .all(|service| match &service.source {
                ServiceSource::Process { pids, .. } => !pids.contains(&9000),
                ServiceSource::Container { .. } => true,
            })
    );
}

#[test]
fn resolution_is_deterministic_regardless_of_input_order() {
    let original = scenario();
    let mut shuffled = original.clone();
    shuffled.inventory.processes.reverse();
    shuffled.inventory.listeners.reverse();
    shuffled.inventory.containers.reverse();
    let a = serde_json::to_string(&resolve(&original)).unwrap();
    let b = serde_json::to_string(&resolve(&shuffled)).unwrap();
    assert_eq!(a, b);
}

#[test]
fn launching_from_the_home_folder_opens_no_project() {
    let mut scenario = scenario();
    scenario.launch_dir = PathBuf::from("/Users/me");
    let snapshot = resolve(&scenario);
    assert_eq!(snapshot.opened, None);
    assert!(
        snapshot
            .services
            .iter()
            .all(|s| s.section == Section::Elsewhere)
    );
    assert!(
        snapshot
            .notes
            .iter()
            .any(|note| note.code == "not_in_project")
    );
}

#[test]
fn a_git_repository_at_home_is_ignored() {
    let mut scenario = scenario();
    // Some people keep their whole home folder in git.
    scenario.world.repos.push(hm_project::RepoInfo {
        root: PathBuf::from("/Users/me"),
        worktrees: vec![hm_core::model::Worktree {
            path: PathBuf::from("/Users/me"),
            branch: Some("main".into()),
            head: None,
        }],
    });
    scenario.launch_dir = PathBuf::from("/Users/me/Desktop/newapp");
    let snapshot = resolve(&scenario);
    let opened = snapshot.projects.iter().find(|p| p.opened).unwrap();
    assert_eq!(opened.root, PathBuf::from("/Users/me/Desktop/newapp"));
    assert!(!opened.git);
}

#[test]
fn hidden_linux_owners_map_to_containers_by_published_port() {
    let mut scenario = scenario();
    scenario.inventory.os = "linux".into();
    // With docker-proxy running as root the owner PID is not visible.
    scenario
        .inventory
        .listeners
        .retain(|listener| listener.port != 55432);
    scenario.inventory.listeners.push(RawListener {
        proto: Default::default(),
        addr: "0.0.0.0".parse().unwrap(),
        port: 55432,
        pids: vec![],
        uid: Some(0),
    });
    let snapshot = resolve(&scenario);
    assert!(matches!(
        listener(&snapshot, 55432).owner,
        Owner::ContainerForwarder { .. }
    ));
    assert!(matches!(
        listener(&snapshot, 6000).owner,
        Owner::Unknown { .. }
    ));
}

#[test]
fn windows_paths_match_without_case_and_pid_4_is_the_http_service() {
    let scenario: Scenario = serde_json::from_str(include_str!("fixtures/windows.json")).unwrap();
    let snapshot = resolve(&scenario);
    assert_eq!(
        listener(&snapshot, 80).owner,
        Owner::System {
            reason: "Windows HTTP service".into(),
            pid: Some(4)
        }
    );
    let web = listener(&snapshot, 5173);
    assert_eq!(web.section, Section::Project);
    assert_eq!(web.label, "web (Vite)");
}

#[test]
fn a_hidden_owner_maps_to_the_container_bound_to_its_exact_address() {
    let mut scenario = scenario();
    let mut first = scenario.inventory.containers[1].clone();
    first.id = "aaaa".into();
    first.name = "on-loopback".into();
    first.ports = vec![hm_core::model::PortBinding {
        container_port: 80,
        proto: "tcp".into(),
        host_ip: Some("127.0.0.1".parse().unwrap()),
        host_port: Some(8080),
    }];
    let mut second = first.clone();
    second.id = "bbbb".into();
    second.name = "on-lan".into();
    second.ports[0].host_ip = Some("192.168.1.5".parse().unwrap());
    scenario.inventory.containers.extend([second, first]);
    scenario.inventory.listeners.push(RawListener {
        proto: Default::default(),
        addr: "127.0.0.1".parse().unwrap(),
        port: 8080,
        pids: vec![],
        uid: Some(0),
    });
    let snapshot = resolve(&scenario);
    assert_eq!(
        listener(&snapshot, 8080).owner,
        Owner::ContainerForwarder {
            container_id: "aaaa".into()
        }
    );
}

#[test]
fn programs_that_are_not_the_projects_own_are_named_after_what_they_are() {
    let mut scenario = scenario();
    let chrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
    // A test browser an agent left running inside the project, and an unknown tool.
    scenario.inventory.processes.push(RawProcess {
        pid: 4000,
        ppid: Some(1),
        start_time: 1760495000,
        name: "Google Chrome".into(),
        exe: Some(chrome.into()),
        argv: vec![
            chrome.into(),
            "--headless=new".into(),
            "--remote-debugging-port=9877".into(),
        ],
        cwd: Some("/Users/me/code/shop/apps/web".into()),
        uid: Some("501".into()),
        user: Some("me".into()),
        ..RawProcess::default()
    });
    scenario.inventory.processes.push(RawProcess {
        pid: 4100,
        ppid: Some(500),
        start_time: 1760495000,
        name: "mystery".into(),
        exe: Some("/Users/me/code/shop/tools/mystery".into()),
        argv: vec!["./tools/mystery".into()],
        cwd: Some("/Users/me/code/shop".into()),
        uid: Some("501".into()),
        user: Some("me".into()),
        ..RawProcess::default()
    });
    let mut twin = scenario.inventory.processes[scenario.inventory.processes.len() - 2].clone();
    twin.pid = 4001;
    twin.argv[2] = "--remote-debugging-port=9878".into();
    scenario.inventory.processes.push(twin);
    for (port, pid) in [(9877, 4000), (9878, 4001), (7777, 4100)] {
        scenario.inventory.listeners.push(RawListener {
            proto: Default::default(),
            addr: "127.0.0.1".parse().unwrap(),
            port,
            pids: vec![pid],
            uid: None,
        });
    }
    let snapshot = resolve(&scenario);
    assert_eq!(listener(&snapshot, 9877).label, "Headless browser");
    assert_eq!(listener(&snapshot, 9877).section, Section::Project);
    assert_eq!(
        listener(&snapshot, 9878).label,
        "headless-browser (2) (Headless browser)"
    );
    assert_eq!(listener(&snapshot, 7777).label, "mystery");
}

#[cfg(unix)]
#[test]
fn a_compose_folder_reached_through_a_symlink_stays_in_its_project() {
    use hm_core::raw::RawInventory;
    use hm_project::git::RealWorld;

    // Compose records `working_dir` as it was typed, symlinks included (on macOS, /tmp
    // is a symlink to /private/tmp), while project roots are canonical.
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("shop");
    std::fs::create_dir(&real).unwrap();
    let link = temp.path().join("shop-link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let inventory: RawInventory = serde_json::from_value(serde_json::json!({
        "os": std::env::consts::OS,
        "containers": [{
            "engine": "e_1",
            "id": "c0ffee",
            "name": "shop-db-1",
            "image": "postgres:16-alpine",
            "state": "running",
            "labels": {
                "com.docker.compose.project": "shop",
                "com.docker.compose.service": "db",
                "com.docker.compose.project.working_dir": link,
            },
        }],
    }))
    .unwrap();
    let packs = load_embedded();
    let snapshot = build_snapshot(ResolveInput {
        inventory: &inventory,
        world: &RealWorld::new(),
        signatures: &packs.signatures,
        launch_dir: &real,
    });
    let db = snapshot
        .services
        .iter()
        .find(|service| service.name == "db")
        .unwrap();
    assert_eq!(db.section, Section::Project);
    assert!(snapshot.opened.is_some());
    assert_eq!(db.project_id, snapshot.opened);
}
