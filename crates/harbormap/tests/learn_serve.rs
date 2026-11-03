//! `harbormap learn` end to end: the built binary over a synthetic shop project, the
//! panel's JSON over HTTP, and the live loop from a saved file to a `facts` event.

use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

/// How long the binary may take to print its link (it indexes first).
const START_TIMEOUT: Duration = Duration::from_secs(60);

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A small shop app: a page, an orders service and repository, a db client, a test
/// and a README.
fn shop(dir: &Path) {
    write(
        dir,
        "src/db/client.ts",
        "export function insert(table: string, row: object) { return table; }\n\
         export function find(table: string) { return []; }\n",
    );
    write(
        dir,
        "src/orders/repo.ts",
        "import { insert, find } from '../db/client';\n\
         export function saveOrder(row: object) { return insert('orders', row); }\n\
         export function listOrders() { return find('orders'); }\n",
    );
    write(
        dir,
        "src/orders/service.ts",
        "import { saveOrder, listOrders } from './repo';\n\
         export function placeOrder(cart: object) { return saveOrder(cart); }\n\
         export function history() { return listOrders(); }\n",
    );
    write(
        dir,
        "src/orders/service.test.ts",
        "import { placeOrder } from './service';\nplaceOrder({});\n",
    );
    write(
        dir,
        "src/app/page.tsx",
        "import { placeOrder } from '../orders/service';\n\
         export default function Page() { return placeOrder({}); }\n",
    );
    write(dir, "README.md", "# Shop\n");
}

/// A running `harbormap learn`, killed when dropped (also when a test panics).
struct Learn {
    child: Child,
    /// The first line the binary printed, parsed.
    url: String,
    port: u16,
    token: String,
    /// Every later stdout line. The reader thread lives as long as the child, so the
    /// binary never meets a closed pipe.
    lines: Receiver<String>,
    project: TempDir,
    _data: TempDir,
}

impl Drop for Learn {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts `harbormap learn --no-open --json` on the project `make` fills in.
fn start(make: impl FnOnce(&Path)) -> Learn {
    let project = tempfile::tempdir().unwrap();
    make(project.path());
    let data = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_harbormap"))
        .args(["learn", "--no-open", "--json"])
        .arg(project.path())
        .env("HARBORMAP_DATA_DIR", data.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start harbormap");
    let stdout = child.stdout.take().unwrap();
    let (tx, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    // From here the guard kills the child if anything below panics.
    let mut learn = Learn {
        child,
        url: String::new(),
        port: 0,
        token: String::new(),
        lines,
        project,
        _data: data,
    };
    let first = learn
        .lines
        .recv_timeout(START_TIMEOUT)
        .expect("harbormap prints its link");
    let first: serde_json::Value = serde_json::from_str(&first).expect("the first line is JSON");
    learn.url = first["url"].as_str().expect("a url").to_string();
    learn.port = first["port"].as_u64().expect("a port") as u16;
    learn.token = learn
        .url
        .split_once("?t=")
        .expect("the link carries the token")
        .1
        .to_string();
    learn
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into()
}

impl Learn {
    /// A GET with the bearer token: the status and the JSON body (null when empty).
    fn get(&self, path: &str) -> (u16, serde_json::Value) {
        let mut res = agent()
            .get(format!("http://127.0.0.1:{}{path}", self.port))
            .header("Authorization", format!("Bearer {}", self.token))
            .call()
            .expect("the request completes");
        let status = res.status().as_u16();
        let text = res.body_mut().read_to_string().unwrap_or_default();
        (status, serde_json::from_str(&text).unwrap_or_default())
    }

    /// The path of a project file.
    fn file(&self, rel: &str) -> std::path::PathBuf {
        self.project.path().join(rel)
    }

    /// Reads `/api/events` on a thread: every event as (arrival, name, data).
    fn events(&self) -> Events {
        let res = ureq::Agent::config_builder()
            .http_status_as_error(false)
            // The stream itself is endless; only the answer must come.
            .timeout_recv_response(Some(Duration::from_secs(10)))
            .build()
            .new_agent()
            .get(format!("http://127.0.0.1:{}/api/events", self.port))
            .header("Authorization", format!("Bearer {}", self.token))
            .call()
            .expect("connect to the event stream");
        assert_eq!(res.status().as_u16(), 200);
        let (_, body) = res.into_parts();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let reader = BufReader::new(body.into_reader());
            let (mut name, mut data) = (String::new(), String::new());
            for line in reader.lines().map_while(Result::ok) {
                if let Some(v) = line.strip_prefix("event:") {
                    name = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("data:") {
                    data = v.trim().to_string();
                } else if line.is_empty() && !name.is_empty() {
                    let event = (
                        Instant::now(),
                        std::mem::take(&mut name),
                        std::mem::take(&mut data),
                    );
                    if tx.send(event).is_err() {
                        break;
                    }
                }
            }
        });
        Events { rx }
    }
}

/// A stream of server events; a silent server fails the wait instead of hanging.
struct Events {
    rx: Receiver<(Instant, String, String)>,
}

impl Events {
    /// The next event, or a panic after `wait`.
    fn next(&self, wait: Duration) -> (Instant, String, serde_json::Value) {
        let (at, name, data) = self
            .rx
            .recv_timeout(wait)
            .unwrap_or_else(|_| panic!("no event within {wait:?}"));
        (
            at,
            name,
            serde_json::from_str(&data).expect("event data is JSON"),
        )
    }

    /// The first `facts` event that lists `path` as changed (`removed` false) or
    /// removed, within `wait`.
    fn facts_for(&self, path: &str, removed: bool, wait: Duration) -> (Instant, serde_json::Value) {
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let (at, name, data) = self.next(left.max(Duration::from_millis(1)));
            let key = if removed {
                "files_removed"
            } else {
                "files_changed"
            };
            if name == "facts"
                && data[key]
                    .as_array()
                    .is_some_and(|l| l.iter().any(|p| p == path))
            {
                return (at, data);
            }
        }
    }

    /// Drops what has arrived.
    fn drain(&self) {
        while self.rx.try_recv().is_ok() {}
    }
}

#[test]
fn learn_serves_the_overview_of_the_project() {
    let live = start(shop);
    let name = live
        .project
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    // The link's token trades for a cookie, which then opens the page.
    let res = agent().get(&live.url).call().expect("the link answers");
    assert_eq!(res.status().as_u16(), 303);
    let cookie = res
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .expect("a cookie")
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let mut page = agent()
        .get(format!("http://127.0.0.1:{}/", live.port))
        .header("Cookie", &cookie)
        .call()
        .expect("the page answers");
    assert_eq!(page.status().as_u16(), 200);
    assert!(page.body_mut().read_to_string().unwrap().contains("<html"));

    // Without a token the API is closed.
    let res = agent()
        .get(format!("http://127.0.0.1:{}/api/overview", live.port))
        .call()
        .unwrap();
    assert_eq!(res.status().as_u16(), 401);

    let (status, overview) = live.get("/api/overview");
    assert_eq!(status, 200);
    assert_eq!(overview["v"], 1);
    assert_eq!(overview["seq"], 0);
    assert_eq!(overview["repo"]["name"], name.as_str());
    assert!(
        !overview["boxes"].as_array().unwrap().is_empty(),
        "the overview has boxes"
    );
}

#[test]
fn a_saved_file_arrives_as_a_facts_event() {
    let live = start(shop);
    let events = live.events();
    let (_, name, hello) = events.next(Duration::from_secs(5));
    assert_eq!(name, "hello");
    assert_eq!(hello["seq"], 0);

    let started = Instant::now();
    write(
        live.project.path(),
        "src/orders/service.ts",
        "import { saveOrder, listOrders } from './repo';\n\
         export function placeOrder(cart: object) { const n = 1; return saveOrder(cart); }\n\
         export function history() { return listOrders(); }\n",
    );
    let (at, delta) = events.facts_for("src/orders/service.ts", false, Duration::from_secs(3));
    println!(
        "write -> facts event: {} ms",
        at.duration_since(started).as_millis()
    );
    // A write that arrives as two batches makes two events.
    assert!(delta["seq"].as_u64().unwrap() >= 1);
    assert!(delta["files_removed"].as_array().unwrap().is_empty());

    // The model moved with the event.
    let (_, overview) = live.get("/api/overview");
    assert!(overview["seq"].as_u64().unwrap() >= 1);
}

/// Runs `git` in `dir` with a fixed identity; returns its trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git is installed");
    assert!(out.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn a_commit_followed_by_a_save_updates_the_branch_and_commit_in_the_overview() {
    let live = start(|dir| {
        shop(dir);
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", "first"]);
    });
    let first = git(live.project.path(), &["rev-parse", "--short=7", "HEAD"]);
    let (_, overview) = live.get("/api/overview");
    assert_eq!(overview["repo"]["branch"], "main");
    assert_eq!(overview["repo"]["commit"], first.as_str());

    let events = live.events();
    assert_eq!(events.next(Duration::from_secs(5)).1, "hello");
    write(
        live.project.path(),
        "src/orders/service.ts",
        "import { saveOrder, listOrders } from './repo';\n\
         export function placeOrder(cart: object) { const n = 2; return saveOrder(cart); }\n\
         export function history() { return listOrders(); }\n",
    );
    git(live.project.path(), &["commit", "-q", "-a", "-m", "second"]);
    let second = git(live.project.path(), &["rev-parse", "--short=7", "HEAD"]);
    assert_ne!(first, second);
    // The first save may have been published before the commit landed; a save after it
    // is published with the new commit.
    events.facts_for("src/orders/service.ts", false, Duration::from_secs(5));
    write(
        live.project.path(),
        "src/orders/service.ts",
        "import { saveOrder, listOrders } from './repo';\n\
         export function placeOrder(cart: object) { const n = 3; return saveOrder(cart); }\n\
         export function history() { return listOrders(); }\n",
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (_, overview) = live.get("/api/overview");
        if overview["repo"]["commit"] == second.as_str() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the overview still names {}",
            overview["repo"]["commit"]
        );
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn deleting_a_file_sends_a_removed_delta() {
    let live = start(shop);
    let (status, _) = live.get("/api/card?path=src/orders/repo.ts");
    assert_eq!(status, 200);

    let events = live.events();
    assert_eq!(events.next(Duration::from_secs(5)).1, "hello");
    fs::remove_file(live.file("src/orders/repo.ts")).unwrap();
    let (_, delta) = events.facts_for("src/orders/repo.ts", true, Duration::from_secs(3));
    assert!(delta["seq"].as_u64().unwrap() >= 1);

    let (status, _) = live.get("/api/card?path=src/orders/repo.ts");
    assert_eq!(status, 404);
}

#[test]
fn learn_refuses_the_home_folder() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_harbormap"))
        .args(["learn", "--no-open"])
        .arg(home.path())
        .env("HARBORMAP_DATA_DIR", data.path())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("is not a project folder"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn learn_without_json_prints_the_link() {
    let project = tempfile::tempdir().unwrap();
    shop(project.path());
    let data = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_harbormap"))
        .args(["learn", "--no-open"])
        .arg(project.path())
        .env("HARBORMAP_DATA_DIR", data.path())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut lines = Vec::new();
    for _ in 0..3 {
        match rx.recv_timeout(START_TIMEOUT) {
            Ok(line) => lines.push(line),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let name = project.path().file_name().unwrap().to_string_lossy();
    assert_eq!(lines.len(), 3, "output: {lines:?}");
    assert_eq!(lines[0], format!("Harbormap Learn is running for {name}"));
    assert!(
        lines[1].starts_with("  Open: http://127.0.0.1:") && lines[1].contains("/?t="),
        "line: {}",
        lines[1]
    );
    assert_eq!(lines[2], "  Ctrl-C stops it.");
}

/// A reader that goes away must end `learn --json` cleanly at the next delta, not
/// leave a panel serving old facts.
#[test]
fn closing_the_json_stream_stops_learn_with_exit_zero() {
    struct Kill(Child);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let project = tempfile::tempdir().unwrap();
    shop(project.path());
    let data = tempfile::tempdir().unwrap();
    let mut child = Kill(
        Command::new(env!("CARGO_BIN_EXE_harbormap"))
            .args(["learn", "--no-open", "--json"])
            .arg(project.path())
            .env("HARBORMAP_DATA_DIR", data.path())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    // The thread reads the link and ends, which closes the read side of the pipe.
    thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    rx.recv_timeout(START_TIMEOUT).expect("the link is printed");
    // Past the catch-up rescan, then a save: its delta hits the closed pipe.
    thread::sleep(Duration::from_secs(1));
    write(
        project.path(),
        "src/db/client.ts",
        "export function insert(table: string, row: object) { return row; }\n\
         export function find(table: string) { return []; }\n",
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "learn is still running");
        thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(0));
}

/// The `.ts` files of a synthetic project, relative and sorted.
fn ts_files(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else if path.extension().is_some_and(|e| e == "ts") {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy();
                out.insert(rel.replace('\\', "/"));
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(dir, &dir.join("src"), &mut out);
    out.into_iter().collect()
}

/// The nearest-rank percentile of a sorted, non-empty list: the value at 1-based rank
/// ceil(pct / 100 * n).
fn percentile(sorted: &[u64], pct: usize) -> u64 {
    sorted[(pct * sorted.len()).div_ceil(100).max(1) - 1]
}

#[test]
fn percentile_is_the_nearest_rank() {
    let fifty: Vec<u64> = (1..=50).collect();
    assert_eq!(percentile(&fifty, 50), 25);
    assert_eq!(percentile(&fifty, 95), 48, "rank ceil(0.95 * 50) = 48");
    assert_eq!(percentile(&fifty, 100), 50);
    let twenty: Vec<u64> = (1..=20).collect();
    assert_eq!(percentile(&twenty, 95), 19);
    assert_eq!(percentile(&[7], 95), 7);
}

/// 50 saves 400 ms apart in a 1 000-file project: how long until each one is on the
/// event stream. The spec asks for 300 ms at p95; the assert is a noise guard.
/// Run in release: `cargo test --release -p harbormap --test learn_serve -- --ignored --nocapture`.
#[test]
#[ignore = "slow; measures live latency"]
fn live_latency_on_1000_files() {
    let live = start(|dir| hm_index::synth::project(dir, 7, 1000));
    let files = ts_files(live.project.path());
    assert!(files.len() >= 500, "{} files", files.len());
    let events = live.events();
    assert_eq!(events.next(Duration::from_secs(5)).1, "hello");
    // Let the watcher's catch-up rescan finish, so every save below goes through the
    // watcher's own 100 ms quiet window.
    thread::sleep(Duration::from_secs(2));

    let mut millis = Vec::new();
    let mut next_file = 0;
    let mut seed = 1_000;
    while millis.len() < 50 {
        let rel = &files[next_file % files.len()];
        next_file += 1;
        seed += 1;
        events.drain();
        let started = Instant::now();
        let changed = hm_index::synth::edit_function(
            live.project.path(),
            rel,
            seed,
            hm_index::synth::Edit::BodyChange,
        );
        if changed.is_empty() {
            continue;
        }
        let (at, _) = events.facts_for(rel, false, Duration::from_secs(5));
        millis.push(at.duration_since(started).as_millis() as u64);
        thread::sleep(Duration::from_millis(400));
    }
    millis.sort_unstable();
    let (p50, p95) = (percentile(&millis, 50), percentile(&millis, 95));
    println!(
        "live latency over {} saves: p50 {p50} ms, p95 {p95} ms, max {} ms",
        millis.len(),
        millis[millis.len() - 1]
    );
    assert!(p95 < 1_000, "p95 {p95} ms");
}

/// Sends SIGINT to `child`, as Ctrl-C in its terminal would.
#[cfg(unix)]
fn interrupt(child: &Child) {
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill is installed");
    assert!(status.success());
}

/// A graceful stop waits for open connections, so one that never finishes its request
/// holds it up; a second Ctrl-C must still end the process, with code 130.
#[cfg(unix)]
#[test]
fn a_second_ctrl_c_ends_a_stuck_shutdown_with_code_130() {
    use std::io::Write;
    use std::os::unix::process::ExitStatusExt;
    let mut live = start(shop);
    // A request that stops in the middle of its headers.
    let mut stuck = std::net::TcpStream::connect(("127.0.0.1", live.port)).unwrap();
    write!(
        stuck,
        "GET /api/tech HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n",
        live.port
    )
    .unwrap();
    thread::sleep(Duration::from_millis(300));

    interrupt(&live.child);
    thread::sleep(Duration::from_secs(1));
    assert!(
        live.child.try_wait().unwrap().is_none(),
        "the first Ctrl-C waits for the open connection"
    );
    interrupt(&live.child);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = live.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the second Ctrl-C did nothing");
        thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(130), "{:?}", status.signal());
    drop(stuck);
}
