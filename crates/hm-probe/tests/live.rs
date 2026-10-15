//! Probes the real machine: sockets and processes this test creates must show up.

use std::net::TcpListener;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use hm_probe::listeners::read_listeners;
use hm_probe::processes::read_processes;
use hm_probe::{ProbeOptions, probe};

/// Spawned by other tests as a long-lived child; does nothing when run normally.
#[test]
#[ignore = "helper process for other tests"]
fn child_sleeper() {
    if std::env::var_os("HM_CHILD").is_some() {
        std::thread::sleep(Duration::from_secs(30));
    }
}

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_child(cwd: &std::path::Path) -> KillOnDrop {
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["child_sleeper", "--exact", "--ignored", "--nocapture"])
        .current_dir(cwd)
        .env("HM_CHILD", "1")
        .env("PORT", "5999")
        .env("DATABASE_URL", "postgres://app:pw12345@db.local:5432/shop")
        .env("HM_TEST_SECRET", "topsecret123")
        .spawn()
        .unwrap();
    KillOnDrop(child)
}

fn wait_until<T>(mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < deadline, "condition not met within 10 s");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn listeners_include_a_socket_this_process_holds() {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    let listeners = read_listeners().unwrap();
    let ours = listeners
        .iter()
        .find(|listener| listener.port == port)
        .expect("our port is listed");
    assert_eq!(ours.addr, "127.0.0.1".parse::<std::net::IpAddr>().unwrap());
    assert!(
        ours.pids.contains(&std::process::id()),
        "pids: {:?}",
        ours.pids
    );
}

#[test]
fn processes_include_cwd_and_env_facts_but_no_secret_values() {
    let dir = tempfile::tempdir().unwrap();
    let child = spawn_child(dir.path());
    let pid = child.0.id();
    let process = wait_until(|| {
        read_processes()
            .processes
            .into_iter()
            .find(|process| process.pid == pid && process.cwd.is_some())
    });
    assert_eq!(
        dunce::canonicalize(process.cwd.as_ref().unwrap()).unwrap(),
        dunce::canonicalize(dir.path()).unwrap()
    );
    assert!(
        process
            .env
            .names
            .iter()
            .any(|name| name == "HM_TEST_SECRET")
    );
    assert!(
        process
            .env
            .ports
            .iter()
            .any(|p| p.name == "PORT" && p.port == 5999)
    );
    assert!(
        process.env.urls.iter().any(|url| url.name == "DATABASE_URL"
            && url.host == "db.local"
            && url.port == Some(5432))
    );
    let json = serde_json::to_string(&process).unwrap();
    assert!(!json.contains("topsecret123"));
    assert!(!json.contains("pw12345"));
}

#[test]
fn probe_reports_itself_and_its_socket() {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    let inventory = probe(&ProbeOptions {
        docker_timeout: Duration::from_secs(2),
    });
    assert_eq!(inventory.self_pid, std::process::id());
    assert!(inventory.current_uid.is_some());
    assert!(
        inventory
            .processes
            .iter()
            .any(|p| p.pid == inventory.self_pid)
    );
    assert!(inventory.listeners.iter().any(|l| l.port == port));
    assert_eq!(inventory.os, std::env::consts::OS);
}
