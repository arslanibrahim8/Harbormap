//! Docker-compatible engines: discovery, then one container listing per reachable
//! engine. Problems become notes; probing never fails as a whole.

pub mod convert;
pub mod discover;

use std::time::Duration;

use bollard::query_parameters::ListContainersOptions;
use bollard::{API_DEFAULT_VERSION, Docker};

use hm_core::ids::engine_id;
use hm_core::model::{Engine, Note};
use hm_core::raw::RawContainer;

use discover::{Candidate, DiscoverEnv};

/// Seconds bollard waits for a single API request.
const REQUEST_TIMEOUT_SECS: u64 = 5;

#[derive(Debug, Default)]
pub struct DockerReport {
    pub engines: Vec<Engine>,
    pub containers: Vec<RawContainer>,
    pub notes: Vec<Note>,
}

impl DockerReport {
    pub fn failed(reason: &str) -> Self {
        DockerReport {
            notes: vec![note(
                "docker_unavailable",
                &format!("Harbormap couldn't check Docker: {reason}."),
            )],
            ..DockerReport::default()
        }
    }
}

/// Probes every candidate engine, giving each at most `timeout`.
pub fn probe_docker(env: &DiscoverEnv, timeout: Duration) -> DockerReport {
    let candidates = discover::candidates(env);
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(probe_all(candidates, timeout)),
        Err(error) => DockerReport::failed(&error.to_string()),
    }
}

async fn probe_all(candidates: Vec<Candidate>, timeout: Duration) -> DockerReport {
    let mut report = DockerReport::default();
    if candidates.is_empty() {
        report
            .notes
            .push(if on_path("docker") || on_path("podman") {
                not_running()
            } else {
                note(
                    "docker_not_installed",
                    "Docker isn't installed, so there are no containers to show.",
                )
            });
        return report;
    }
    let mut timed_out = false;
    for candidate in candidates {
        match tokio::time::timeout(timeout, probe_one(&candidate)).await {
            Ok(Ok((engine, containers))) => {
                // Two sockets can reach the same daemon; keep the first.
                if report.engines.iter().any(|known| known.id == engine.id) {
                    continue;
                }
                report.engines.push(engine);
                report.containers.extend(containers);
            }
            Ok(Err(error)) => report.engines.push(unreachable(&candidate, error)),
            Err(_) => {
                timed_out = true;
                report.engines.push(unreachable(
                    &candidate,
                    format!("no answer within {} ms", timeout.as_millis()),
                ));
            }
        }
    }
    if !report.engines.iter().any(|engine| engine.reachable) {
        report.notes.push(if timed_out {
            note(
                "docker_not_answering",
                "Docker didn't answer in time. It may still be starting; try again in a moment.",
            )
        } else {
            not_running()
        });
    }
    report
}

async fn probe_one(candidate: &Candidate) -> Result<(Engine, Vec<RawContainer>), String> {
    let docker = connect(&candidate.endpoint)?
        .negotiate_version()
        .await
        .map_err(|error| error.to_string())?;
    let info = docker.info().await.map_err(|error| error.to_string())?;
    let key = info
        .id
        .clone()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| candidate.endpoint.clone());
    let id = engine_id(&key);
    let summaries = docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            ..ListContainersOptions::default()
        }))
        .await
        .map_err(|error| error.to_string())?;
    let mut containers = Vec::new();
    for summary in summaries {
        let Some(container_id) = summary.id else {
            continue;
        };
        // A container can disappear between listing and inspecting; skip it then.
        if let Ok(inspect) = docker.inspect_container(&container_id, None).await
            && let Some(container) = convert::from_inspect(&id, inspect)
        {
            containers.push(container);
        }
    }
    containers.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    let engine = Engine {
        id,
        name: refine_label(&candidate.label, info.operating_system.as_deref()),
        endpoint: candidate.endpoint.clone(),
        version: info.server_version,
        reachable: true,
        error: None,
    };
    Ok((engine, containers))
}

fn connect(endpoint: &str) -> Result<Docker, String> {
    let docker = if endpoint.starts_with("tcp://") || endpoint.starts_with("http://") {
        Docker::connect_with_http(endpoint, REQUEST_TIMEOUT_SECS, API_DEFAULT_VERSION)
    } else {
        connect_local(endpoint)?
    };
    docker.map_err(|error| error.to_string())
}

#[cfg(unix)]
fn connect_local(endpoint: &str) -> Result<Result<Docker, bollard::errors::Error>, String> {
    match endpoint.strip_prefix("unix://") {
        Some(path) => Ok(Docker::connect_with_unix(
            path,
            REQUEST_TIMEOUT_SECS,
            API_DEFAULT_VERSION,
        )),
        None => Err(format!("unsupported endpoint {endpoint}")),
    }
}

#[cfg(windows)]
fn connect_local(endpoint: &str) -> Result<Result<Docker, bollard::errors::Error>, String> {
    if endpoint.starts_with("npipe://") {
        Ok(Docker::connect_with_named_pipe(
            endpoint,
            REQUEST_TIMEOUT_SECS,
            API_DEFAULT_VERSION,
        ))
    } else {
        Err(format!("unsupported endpoint {endpoint}"))
    }
}

fn refine_label(label: &str, operating_system: Option<&str>) -> String {
    match operating_system {
        Some(os) if os.contains("Docker Desktop") => "Docker Desktop".into(),
        Some(os) if os.contains("OrbStack") => "OrbStack".into(),
        _ => label.to_string(),
    }
}

fn unreachable(candidate: &Candidate, error: String) -> Engine {
    Engine {
        id: engine_id(&candidate.endpoint),
        name: candidate.label.clone(),
        endpoint: candidate.endpoint.clone(),
        version: None,
        reachable: false,
        error: Some(error),
    }
}

fn not_running() -> Note {
    note(
        "docker_not_running",
        "Docker isn't running. Start Docker Desktop (or your container engine) to see containers.",
    )
}

fn note(code: &str, text: &str) -> Note {
    Note {
        code: code.into(),
        text: text.into(),
    }
}

/// True when an executable with this name is on `PATH`.
pub fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        dir.join(name).is_file() || (cfg!(windows) && dir.join(format!("{name}.exe")).is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_refined_by_the_engine_os() {
        assert_eq!(
            refine_label("Docker", Some("Docker Desktop")),
            "Docker Desktop"
        );
        assert_eq!(refine_label("Docker", Some("OrbStack")), "OrbStack");
        assert_eq!(refine_label("Colima", Some("Ubuntu 24.04 LTS")), "Colima");
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn unreachable_candidates_say_why() {
        let report = runtime().block_on(probe_all(
            vec![Candidate {
                endpoint: "tcp://127.0.0.1:1".into(),
                label: "Docker".into(),
            }],
            Duration::from_secs(2),
        ));
        assert_eq!(report.engines.len(), 1);
        assert!(!report.engines[0].reachable);
        assert!(report.engines[0].error.is_some());
        assert_eq!(report.notes[0].code, "docker_not_running");
    }

    #[cfg(unix)]
    #[test]
    fn a_hanging_engine_times_out_with_a_note() {
        // Unix socket paths are short (104 bytes on macOS), so stay out of a long TMPDIR.
        let dir = tempfile::Builder::new()
            .prefix("hm")
            .tempdir_in("/tmp")
            .unwrap();
        let path = dir.path().join("hang.sock");
        let socket = std::os::unix::net::UnixListener::bind(&path).unwrap();
        // Accept connections and hold them open without ever answering.
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in socket.incoming() {
                held.push(stream);
            }
        });
        let started = std::time::Instant::now();
        let report = runtime().block_on(probe_all(
            vec![Candidate {
                endpoint: format!("unix://{}", path.display()),
                label: "Docker".into(),
            }],
            Duration::from_millis(500),
        ));
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(!report.engines[0].reachable);
        assert_eq!(report.notes[0].code, "docker_not_answering");
    }
}
