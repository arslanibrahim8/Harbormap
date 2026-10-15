//! OS adapters: what runs on this computer, before any interpretation.

pub mod docker;
pub mod listeners;
pub mod processes;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hm_core::model::Note;
use hm_core::raw::RawInventory;

use docker::DockerReport;
use docker::discover::DiscoverEnv;

#[derive(Debug, Clone)]
pub struct ProbeOptions {
    /// Upper bound for each Docker engine.
    pub docker_timeout: Duration,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        ProbeOptions {
            docker_timeout: Duration::from_secs(3),
        }
    }
}

/// Reads listeners, processes and containers. Docker is probed on its own thread
/// while the OS tables are read.
pub fn probe(options: &ProbeOptions) -> RawInventory {
    let taken_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let docker_env = DiscoverEnv::from_process();
    let (table, listeners, docker) = std::thread::scope(|scope| {
        let docker = scope.spawn(|| docker::probe_docker(&docker_env, options.docker_timeout));
        // Listeners first: an owner that starts later does not matter, one that exits
        // in between shows up as "process ended".
        let listeners = listeners::read_listeners();
        let table = processes::read_processes();
        let docker = docker
            .join()
            .unwrap_or_else(|_| DockerReport::failed("the Docker probe stopped unexpectedly"));
        (table, listeners, docker)
    });

    let mut notes = Vec::new();
    let listeners = listeners.unwrap_or_else(|error| {
        notes.push(Note {
            code: "listeners_unavailable".into(),
            text: format!("Harbormap couldn't read listening ports: {error}"),
        });
        Vec::new()
    });
    notes.extend(docker.notes);

    RawInventory {
        taken_at,
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        self_pid: table.self_pid,
        current_uid: table.current_uid,
        processes: table.processes,
        listeners,
        containers: docker.containers,
        engines: docker.engines,
        notes,
    }
}
