//! The alpha.1 exit criterion (v0.1 design §2): a test harness starts a Vite server, a
//! FastAPI server, a .NET server and a compose project with Postgres, and
//! `harbormap status --json` maps every listener to the right project, service and
//! framework. A plain Python server in another git repository must show up under
//! "elsewhere".
//!
//! The apps live in `tests/e2e/apps`. A first run installs `node_modules` and the FastAPI
//! `.venv` when they are missing (network needed), and `dotnet run` restores and builds,
//! so it can take a few minutes. Needs npm, Python 3.11+, the .NET SDK and git on PATH;
//! the Postgres part also needs Docker with compose:
//!
//! ```text
//! cargo test -p harbormap --test e2e -- --ignored --nocapture
//! HM_E2E_DOCKER=1 cargo test -p harbormap --test e2e -- --ignored --nocapture
//! ```
//!
//! `HM_E2E_PYTHON` names the Python that creates the venv and runs the other project
//! (default `python3`, or `python` on Windows).
//!
//! The test stops everything it started when it ends, also when it fails. A run that is
//! interrupted (Ctrl-C) cannot: stop whatever still listens on ports 47173, 47800, 47280
//! and 47900, and run `docker compose -p hm-e2e down -v`.

use std::fmt::Write as _;
use std::fs::File;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hm_core::model::{Listener, Section, Service, ServiceSource, Snapshot};

const VITE: u16 = 47173;
const FASTAPI: u16 = 47800;
const DOTNET: u16 = 47280;
const POSTGRES: u16 = 47432;
const OTHER: u16 = 47900;

/// The first .NET start restores and builds the app.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(180);
const COMPOSE_PROJECT: &str = "hm-e2e";

#[test]
#[ignore = "starts real dev servers; needs npm, Python, the .NET SDK and git (and Docker with HM_E2E_DOCKER=1)"]
fn status_json_maps_dev_servers_to_projects_services_and_frameworks() {
    let repo = canonical(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let apps = repo.join("tests").join("e2e").join("apps");
    let docker = std::env::var("HM_E2E_DOCKER").is_ok_and(|value| value == "1");

    let mut ports = vec![VITE, FASTAPI, DOTNET, OTHER];
    if docker {
        ports.push(POSTGRES);
    }
    let busy: Vec<u16> = ports
        .iter()
        .copied()
        .filter(|port| accepts(*port))
        .collect();
    assert!(
        busy.is_empty(),
        "port(s) {busy:?} are already in use; stop whatever holds them and run again"
    );

    let vite_dir = apps.join("vite");
    let fastapi_dir = apps.join("fastapi");
    let dotnet_dir = apps.join("dotnet");
    prepare_vite(&vite_dir);
    prepare_fastapi(&fastapi_dir);

    // Declared first so it is removed last: the logs and the other project live here.
    let scratch = Scratch::new();
    let logs = scratch.dir("logs");
    let other = scratch.dir("other");
    // Not the repository a git hook running this test points at.
    run(
        Command::new("git")
            .args(["init", "-q"])
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE"),
        &other,
    );

    let mut vite = Command::new(if cfg!(windows) { "npm.cmd" } else { "npm" });
    vite.args(["run", "dev"]);

    let mut fastapi = Command::new(venv_python(&fastapi_dir));
    fastapi
        .args(["-m", "uvicorn", "main:app", "--host", "127.0.0.1", "--port"])
        .arg(FASTAPI.to_string())
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1");

    let mut dotnet = Command::new("dotnet");
    dotnet
        .args(["run", "--urls"])
        .arg(format!("http://127.0.0.1:{DOTNET}"))
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_GENERATE_ASPNET_CERTIFICATE", "false")
        // Leave no MSBuild worker nodes or compiler server behind after the build.
        .env("MSBUILDDISABLENODEREUSE", "1")
        .env("UseSharedCompilation", "false");

    let mut elsewhere = Command::new(base_python());
    elsewhere
        .args(["-m", "http.server"])
        .arg(OTHER.to_string())
        .args(["--bind", "127.0.0.1"]);

    let mut servers = vec![
        Server::start("vite", VITE, vite, &vite_dir, &logs),
        Server::start("fastapi", FASTAPI, fastapi, &fastapi_dir, &logs),
        Server::start("dotnet", DOTNET, dotnet, &dotnet_dir, &logs),
        Server::start("other", OTHER, elsewhere, &other, &logs),
    ];
    // Compose blocks until Postgres is healthy while .NET builds in the background.
    let compose = docker.then(|| Compose::up(&apps.join("compose").join("compose.yaml")));
    wait_until_listening(&mut servers);
    if compose.is_some() {
        assert!(
            accepts(POSTGRES),
            "Postgres is healthy but port {POSTGRES} refuses connections"
        );
    }

    let snapshot = status_json(&repo);

    let pid_of = |name: &str| {
        servers
            .iter()
            .find(|server| server.name == name)
            .map(|server| server.child.id())
            .unwrap_or_else(|| panic!("no server named {name}"))
    };
    let mut expected = vec![
        (VITE, "e2e-vite", "vite", Some(pid_of("vite"))),
        (FASTAPI, "e2e-fastapi", "fastapi", Some(pid_of("fastapi"))),
        (DOTNET, "E2eDotnet", "dotnet", Some(pid_of("dotnet"))),
    ];
    if docker {
        expected.push((POSTGRES, "db", "postgres", None));
    }
    let mut problems = Vec::new();
    for (port, name, signature, started) in expected {
        check_in_project(
            &snapshot,
            &repo,
            port,
            name,
            signature,
            started,
            &mut problems,
        );
    }
    check_elsewhere(&snapshot, &other, &mut problems);
    assert!(
        problems.is_empty(),
        "{} problem(s):\n  {}\n\n{}",
        problems.len(),
        problems.join("\n  "),
        summary(&snapshot)
    );
    println!("every listener maps as expected (docker: {docker})");
}

/// A port the opened project must own: listener and service in section "project", with
/// the expected service name and signature, in the worktree this test runs from (the
/// repository root, `repo`). For host programs, the service's process tree must start
/// at the process this test started (`npm run dev`, not only the `node vite` it runs).
fn check_in_project(
    snapshot: &Snapshot,
    repo: &Path,
    port: u16,
    name: &str,
    signature: &str,
    started: Option<u32>,
    problems: &mut Vec<String>,
) {
    for (listener, service) in services_on(snapshot, port, problems) {
        let mut wrong = |what: String| problems.push(format!("port {port}: {what}"));
        if service.name != name {
            wrong(format!(
                "service is named {:?}, expected {name:?}",
                service.name
            ));
        }
        if service.signature.as_deref() != Some(signature) {
            wrong(format!(
                "signature is {:?}, expected {signature:?}",
                service.signature
            ));
        }
        if listener.section != Section::Project || service.section != Section::Project {
            wrong(format!(
                "sections are {:?} (listener) and {:?} (service), expected project",
                listener.section, service.section
            ));
        }
        if snapshot.opened.is_none() || service.project_id != snapshot.opened {
            wrong(format!(
                "project is {:?}, expected the opened project {:?}",
                service.project_id, snapshot.opened
            ));
        }
        if service.worktree.as_deref() != Some(repo) {
            wrong(format!(
                "worktree is {:?}, expected {}",
                service.worktree,
                repo.display()
            ));
        }
        if let Some(pid) = started {
            match &service.source {
                ServiceSource::Process { root_pid, .. } if *root_pid == pid => {}
                source => wrong(format!(
                    "service source is {source:?}, expected a process tree rooted at {pid}"
                )),
            }
        }
    }
}

/// The Python server in the other repository: section "elsewhere", signature "python",
/// and a project whose root is that repository.
fn check_elsewhere(snapshot: &Snapshot, root: &Path, problems: &mut Vec<String>) {
    for (listener, service) in services_on(snapshot, OTHER, problems) {
        let mut wrong = |what: String| problems.push(format!("port {OTHER}: {what}"));
        if listener.section != Section::Elsewhere || service.section != Section::Elsewhere {
            wrong(format!(
                "sections are {:?} (listener) and {:?} (service), expected elsewhere",
                listener.section, service.section
            ));
        }
        if service.signature.as_deref() != Some("python") {
            wrong(format!(
                "signature is {:?}, expected \"python\"",
                service.signature
            ));
        }
        let project = service
            .project_id
            .as_ref()
            .and_then(|id| snapshot.projects.iter().find(|project| &project.id == id));
        match project {
            Some(project) if project.root == root && !project.opened => {}
            Some(project) => wrong(format!(
                "project root is {} (opened: {}), expected {}",
                project.root.display(),
                project.opened,
                root.display()
            )),
            None => wrong(format!(
                "project {:?} is not in the snapshot",
                service.project_id
            )),
        }
    }
}

/// Every listener on `port` with its service; a missing listener or service is a problem.
fn services_on<'s>(
    snapshot: &'s Snapshot,
    port: u16,
    problems: &mut Vec<String>,
) -> Vec<(&'s Listener, &'s Service)> {
    let listeners: Vec<&Listener> = snapshot
        .listeners
        .iter()
        .filter(|listener| listener.port == port)
        .collect();
    if listeners.is_empty() {
        problems.push(format!("port {port}: no listener in the snapshot"));
    }
    listeners
        .into_iter()
        .filter_map(|listener| {
            let service = listener
                .service_id
                .as_ref()
                .and_then(|id| snapshot.services.iter().find(|service| &service.id == id));
            if service.is_none() {
                problems.push(format!(
                    "port {port}: no service (owner {:?}, label {:?}, section {:?})",
                    listener.owner, listener.label, listener.section
                ));
            }
            service.map(|service| (listener, service))
        })
        .collect()
}

fn summary(snapshot: &Snapshot) -> String {
    let mut out = format!("Snapshot summary (opened project {:?})\n", snapshot.opened);
    for project in &snapshot.projects {
        let _ = writeln!(
            out,
            "  project {} {:?} root={} opened={} git={}",
            project.id,
            project.name,
            project.root.display(),
            project.opened,
            project.git
        );
    }
    for service in &snapshot.services {
        let _ = writeln!(
            out,
            "  service {} {:?} signature={:?} section={:?} project={:?} ports={:?} source={:?}",
            service.id,
            service.name,
            service.signature,
            service.section,
            service.project_id,
            service.ports,
            service.source
        );
    }
    for listener in &snapshot.listeners {
        let _ = writeln!(
            out,
            "  listener {} section={:?} service={:?} owner={:?} label={:?}",
            SocketAddr::new(listener.addr, listener.port),
            listener.section,
            listener.service_id,
            listener.owner,
            listener.label
        );
    }
    for note in &snapshot.notes {
        let _ = writeln!(out, "  note {}: {}", note.code, note.text);
    }
    out
}

fn status_json(repo: &Path) -> Snapshot {
    let output = Command::new(env!("CARGO_BIN_EXE_harbormap"))
        .args(["status", "--json"])
        .current_dir(repo)
        // Debug builds replay a recorded machine when this is set.
        .env_remove("HARBORMAP_FIXTURE")
        .output()
        .expect("could not run harbormap");
    assert!(
        output.status.success(),
        "harbormap status --json failed ({}):\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "harbormap status --json printed something that is not a snapshot ({error}):\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// `npm ci` unless `node_modules` already holds Vite.
fn prepare_vite(dir: &Path) {
    if !dir
        .join("node_modules")
        .join("vite")
        .join("package.json")
        .is_file()
    {
        let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
        run(
            Command::new(npm).args(["ci", "--no-audit", "--no-fund"]),
            dir,
        );
    }
}

/// A `.venv` with the pinned requirements, unless it already imports FastAPI and Uvicorn.
fn prepare_fastapi(dir: &Path) {
    let python = venv_python(dir);
    if !python.is_file() {
        run(
            Command::new(base_python()).args(["-m", "venv", ".venv"]),
            dir,
        );
    }
    let ready = Command::new(&python)
        .args(["-c", "import fastapi, uvicorn"])
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !ready {
        run(
            Command::new(&python).args([
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "--quiet",
                "-r",
                "requirements.txt",
            ]),
            dir,
        );
    }
}

fn venv_python(app: &Path) -> PathBuf {
    if cfg!(windows) {
        app.join(".venv").join("Scripts").join("python.exe")
    } else {
        app.join(".venv").join("bin").join("python")
    }
}

fn base_python() -> String {
    std::env::var("HM_E2E_PYTHON")
        .unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.to_string())
}

/// Runs a setup command to completion; fails with its output when it fails.
fn run(command: &mut Command, dir: &Path) {
    let output = command
        .current_dir(dir)
        .output()
        .unwrap_or_else(|error| panic!("could not run {command:?}: {error}"));
    assert!(
        output.status.success(),
        "{command:?} failed ({}):\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn accepts(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
}

/// The path as Harbormap stores it: symlinks resolved and, on Windows, without the
/// `\\?\` prefix that `std::fs::canonicalize` adds.
fn canonical(path: &Path) -> PathBuf {
    let path =
        std::fs::canonicalize(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

/// Waits until every server accepts TCP connections; fails early when one exits.
fn wait_until_listening(servers: &mut [Server]) {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut waiting: Vec<&mut Server> = servers.iter_mut().collect();
    while !waiting.is_empty() {
        let mut still = Vec::new();
        for server in waiting {
            if accepts(server.port) {
                println!(
                    "{} listens on {} after {:.1} s",
                    server.name,
                    server.port,
                    server.started.elapsed().as_secs_f64()
                );
            } else if let Ok(Some(status)) = server.child.try_wait() {
                panic!(
                    "{} exited ({status}) before listening on {}\n{}",
                    server.name,
                    server.port,
                    server.log_tail()
                );
            } else {
                still.push(server);
            }
        }
        waiting = still;
        if waiting.is_empty() {
            break;
        }
        if Instant::now() > deadline {
            let tails: Vec<String> = waiting
                .iter()
                .map(|server| {
                    format!(
                        "{} (port {})\n{}",
                        server.name,
                        server.port,
                        server.log_tail()
                    )
                })
                .collect();
            panic!(
                "not listening after {} s:\n{}",
                STARTUP_TIMEOUT.as_secs(),
                tails.join("\n")
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// A started program with its whole process tree, stopped when dropped.
struct Server {
    name: &'static str,
    port: u16,
    child: Child,
    log: PathBuf,
    started: Instant,
}

impl Server {
    fn start(
        name: &'static str,
        port: u16,
        mut command: Command,
        cwd: &Path,
        logs: &Path,
    ) -> Server {
        let log = logs.join(format!("{name}.log"));
        let out = File::create(&log).unwrap_or_else(|error| panic!("{}: {error}", log.display()));
        let err = out.try_clone().expect("could not duplicate the log handle");
        command
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // A group of its own, so the whole tree (npm → sh → node) stops together.
            command.process_group(0);
        }
        let child = command
            .spawn()
            .unwrap_or_else(|error| panic!("could not start {name} ({command:?}): {error}"));
        Server {
            name,
            port,
            child,
            log,
            started: Instant::now(),
        }
    }

    fn log_tail(&self) -> String {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let first = lines.len().saturating_sub(40);
        format!(
            "--- {} ---\n{}",
            self.log.display(),
            lines[first..].join("\n")
        )
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        stop_tree(&mut self.child);
    }
}

/// TERM to the process group, then KILL if it is still there after 10 s.
#[cfg(unix)]
fn stop_tree(child: &mut Child) {
    let group = format!("-{}", child.id());
    if signal_group(&group, "-TERM")
        && !group_gone(child, &group, Duration::from_secs(10))
        && !(signal_group(&group, "-KILL") && group_gone(child, &group, Duration::from_secs(5)))
    {
        eprintln!(
            "process group {} survived KILL; stop it by hand",
            child.id()
        );
    }
    // Without a working `kill`, at least the process this test started stops.
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn signal_group(group: &str, signal: &str) -> bool {
    Command::new("kill")
        .args([signal, "--", group])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(unix)]
fn group_gone(child: &mut Child, group: &str, grace: Duration) -> bool {
    let deadline = Instant::now() + grace;
    loop {
        // Reap the group leader: as a zombie it would keep the group alive.
        let _ = child.try_wait();
        // `kill -0` succeeds while any process of the group is alive.
        if !signal_group(group, "-0") {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(windows)]
fn stop_tree(child: &mut Child) {
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID"])
        .arg(child.id().to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    // Without taskkill, at least the process this test started stops.
    let _ = child.kill();
    let _ = child.wait();
}

/// `docker compose up -d --wait` for the fixture project; `down -v` when dropped.
struct Compose(PathBuf);

impl Compose {
    fn up(file: &Path) -> Compose {
        // Created first, so a failed `up` is cleaned up too.
        let compose = Compose(file.to_path_buf());
        let started = Instant::now();
        let output = compose
            .command(&["up", "-d", "--wait"])
            .output()
            .expect("could not run docker compose");
        assert!(
            output.status.success(),
            "docker compose up failed ({}):\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        println!(
            "postgres is healthy after {:.1} s",
            started.elapsed().as_secs_f64()
        );
        compose
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("docker");
        command
            .args(["compose", "-p", COMPOSE_PROJECT, "-f"])
            .arg(&self.0)
            .args(args);
        command
    }
}

impl Drop for Compose {
    fn drop(&mut self) {
        let removed = self
            .command(&["down", "-v"])
            .output()
            .is_ok_and(|output| output.status.success());
        if !removed {
            eprintln!(
                "could not remove the compose project; run `docker compose -p {COMPOSE_PROJECT} down -v`"
            );
        }
    }
}

/// A fresh folder under the system temp folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path = std::env::temp_dir().join(format!("hm-e2e-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        Scratch(canonical(&path))
    }

    fn dir(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("could not remove {}: {error}", self.0.display());
        }
    }
}
