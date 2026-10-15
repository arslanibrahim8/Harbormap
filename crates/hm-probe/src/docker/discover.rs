//! Where Docker-compatible engines might be listening (v0.1 spec §6.4), in order:
//! `DOCKER_HOST`, the current docker context, then well-known sockets. Only engines on
//! this computer count: a remote `tcp://` or `ssh://` host is skipped (spec §15).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// `unix:///path`, `npipe:////./pipe/name` or `tcp://host:port`.
    pub endpoint: String,
    /// Engine name guessed from the endpoint; refined after connecting.
    pub label: String,
}

/// Everything discovery reads, so tests can point it at a temporary home.
#[derive(Debug, Clone, Default)]
pub struct DiscoverEnv {
    pub docker_host: Option<String>,
    pub docker_context: Option<String>,
    /// `DOCKER_CONFIG`, defaulting to `~/.docker`.
    pub docker_config: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub xdg_runtime_dir: Option<PathBuf>,
    /// Absolute sockets outside the home directory, such as `/var/run/docker.sock`.
    pub system_sockets: Vec<PathBuf>,
    pub windows: bool,
}

impl DiscoverEnv {
    pub fn from_process() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        let mut system_sockets = vec![PathBuf::from("/var/run/docker.sock")];
        system_sockets.extend(podman_machine_socket());
        DiscoverEnv {
            docker_host: var("DOCKER_HOST"),
            docker_context: var("DOCKER_CONTEXT"),
            docker_config: var("DOCKER_CONFIG").map(PathBuf::from),
            home: std::env::home_dir(),
            xdg_runtime_dir: var("XDG_RUNTIME_DIR").map(PathBuf::from),
            system_sockets,
            windows: cfg!(windows),
        }
    }
}

pub fn candidates(env: &DiscoverEnv) -> Vec<Candidate> {
    let mut found: Vec<Candidate> = Vec::new();
    let mut push = |endpoint: String| {
        if !is_local(&endpoint) {
            return;
        }
        let canonical = canonical_endpoint(&endpoint);
        if !found
            .iter()
            .any(|candidate| candidate.endpoint == canonical)
        {
            // The name as configured says more than the resolved path, which may point
            // into a VM folder.
            let label = match label_for(&endpoint).as_str() {
                "Docker" => label_for(&canonical),
                known => known.to_string(),
            };
            found.push(Candidate {
                label,
                endpoint: canonical,
            });
        }
    };

    if let Some(host) = &env.docker_host {
        push(host.clone());
    }
    if let Some(host) = context_host(env) {
        push(host);
    }

    let mut sockets: Vec<PathBuf> = Vec::new();
    if let Some(home) = &env.home {
        for relative in [
            ".docker/run/docker.sock",
            ".orbstack/run/docker.sock",
            ".colima/default/docker.sock",
            ".rd/docker.sock",
        ] {
            sockets.push(home.join(relative));
        }
    }
    sockets.extend(env.system_sockets.iter().cloned());
    if let Some(runtime) = &env.xdg_runtime_dir {
        sockets.push(runtime.join("docker.sock"));
        sockets.push(runtime.join("podman/podman.sock"));
    }
    for socket in sockets {
        if socket.exists() {
            push(format!("unix://{}", socket.display()));
        }
    }
    if env.windows {
        push("npipe:////./pipe/docker_engine".to_string());
    }
    found
}

/// Reads `currentContext` from the docker config (or `DOCKER_CONTEXT`) and returns that
/// context's docker endpoint. The built-in `default` context has no metadata file.
fn context_host(env: &DiscoverEnv) -> Option<String> {
    let config_dir = env
        .docker_config
        .clone()
        .or_else(|| env.home.as_ref().map(|home| home.join(".docker")))?;
    let name = match &env.docker_context {
        Some(name) => name.clone(),
        None => {
            let config = std::fs::read_to_string(config_dir.join("config.json")).ok()?;
            let config: serde_json::Value = serde_json::from_str(&config).ok()?;
            config.get("currentContext")?.as_str()?.to_string()
        }
    };
    if name.is_empty() || name == "default" {
        return None;
    }
    // Metadata directories are named by a hash of the context name; scan them instead.
    for entry in std::fs::read_dir(config_dir.join("contexts/meta"))
        .ok()?
        .flatten()
    {
        let Ok(text) = std::fs::read_to_string(entry.path().join("meta.json")) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if meta.get("Name").and_then(|value| value.as_str()) == Some(name.as_str()) {
            return meta
                .pointer("/Endpoints/docker/Host")
                .and_then(|value| value.as_str())
                .map(str::to_string);
        }
    }
    None
}

/// Sockets and pipes are local; a TCP endpoint counts only on a loopback address.
fn is_local(endpoint: &str) -> bool {
    match endpoint.split_once("://") {
        Some(("unix" | "npipe", _)) => true,
        Some(("tcp" | "http", rest)) => {
            let authority = rest.split('/').next().unwrap_or(rest);
            let host = match authority.rsplit_once(':') {
                Some((host, port)) if port.bytes().all(|byte| byte.is_ascii_digit()) => host,
                _ => authority,
            };
            let host = host.trim_start_matches('[').trim_end_matches(']');
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        }
        _ => false,
    }
}

/// Resolves symlinks in `unix://` endpoints so two names for one socket collapse.
fn canonical_endpoint(endpoint: &str) -> String {
    match endpoint.strip_prefix("unix://") {
        Some(path) => match std::fs::canonicalize(path) {
            Ok(real) => format!("unix://{}", real.display()),
            Err(_) => endpoint.to_string(),
        },
        None => endpoint.to_string(),
    }
}

/// Engine name guessed from folder names in the endpoint, never from parts of a name
/// (a user called `halima` is not running Colima).
pub fn label_for(endpoint: &str) -> String {
    let lower = endpoint.to_ascii_lowercase().replace('\\', "/");
    let label = if lower.contains("/.orbstack/") {
        "OrbStack"
    } else if lower.contains("/.colima/") || lower.contains("/.lima/") {
        "Colima"
    } else if lower.contains("/.rd/") {
        "Rancher Desktop"
    } else if lower.contains("/podman/")
        || lower.contains("podman.sock")
        || lower.contains("/pipe/podman")
    {
        "Podman"
    } else if lower.contains("/.docker/run/") || lower.contains("docker_engine") {
        "Docker Desktop"
    } else {
        "Docker"
    };
    label.to_string()
}

/// Asks `podman machine inspect` for the API socket when the podman CLI exists. Podman can
/// wait on a machine lock, so it gets two seconds before it is stopped.
fn podman_machine_socket() -> Option<PathBuf> {
    if !super::on_path("podman") {
        return None;
    }
    let mut child = Command::new("podman")
        .args([
            "machine",
            "inspect",
            "--format",
            "{{.ConnectionInfo.PodmanSocket.Path}}",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    let path = output.trim();
    (status.success() && !path.is_empty() && Path::new(path).is_absolute())
        .then(|| PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn docker_host_comes_first_then_existing_sockets() {
        let home = tempfile::tempdir().unwrap();
        touch(&home.path().join(".orbstack/run/docker.sock"));
        let env = DiscoverEnv {
            docker_host: Some("tcp://127.0.0.1:2375".into()),
            home: Some(home.path().to_path_buf()),
            ..DiscoverEnv::default()
        };
        let found = candidates(&env);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].endpoint, "tcp://127.0.0.1:2375");
        assert_eq!(found[1].label, "OrbStack");
        // Compare path components: Windows canonical paths use `\` and a `\\?\` prefix.
        let socket = found[1].endpoint.trim_start_matches("unix://");
        assert!(Path::new(socket).ends_with(".orbstack/run/docker.sock"));
    }

    #[test]
    fn reads_the_current_context_endpoint() {
        let home = tempfile::tempdir().unwrap();
        let docker = home.path().join(".docker");
        std::fs::create_dir_all(docker.join("contexts/meta/abc123")).unwrap();
        std::fs::write(docker.join("config.json"), r#"{"currentContext":"colima"}"#).unwrap();
        std::fs::write(
            docker.join("contexts/meta/abc123/meta.json"),
            r#"{"Name":"colima","Endpoints":{"docker":{"Host":"unix:///Users/me/.colima/default/docker.sock"}}}"#,
        )
        .unwrap();
        let env = DiscoverEnv {
            home: Some(home.path().to_path_buf()),
            ..DiscoverEnv::default()
        };
        let found = candidates(&env);
        assert_eq!(
            found[0].endpoint,
            "unix:///Users/me/.colima/default/docker.sock"
        );
        assert_eq!(found[0].label, "Colima");
    }

    #[test]
    fn missing_sockets_are_skipped_and_windows_gets_the_pipe() {
        let home = tempfile::tempdir().unwrap();
        let env = DiscoverEnv {
            home: Some(home.path().to_path_buf()),
            system_sockets: vec![home.path().join("nope.sock")],
            windows: true,
            ..DiscoverEnv::default()
        };
        let found = candidates(&env);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].endpoint, "npipe:////./pipe/docker_engine");
        assert_eq!(found[0].label, "Docker Desktop");

        // A remote engine is not this computer's: Harbormap never connects to it.
        let remote = DiscoverEnv {
            docker_host: Some("tcp://10.0.0.5:2375".into()),
            ..DiscoverEnv::default()
        };
        assert!(candidates(&remote).is_empty());
        // Every loopback address is this computer.
        for host in [
            "tcp://127.0.0.2:2375",
            "tcp://LOCALHOST:2375",
            "tcp://[::1]:2375",
        ] {
            let local = DiscoverEnv {
                docker_host: Some(host.into()),
                ..DiscoverEnv::default()
            };
            assert_eq!(candidates(&local)[0].endpoint, host);
        }
        // Labels come from folder names, not from parts of a user name.
        assert_eq!(
            label_for("unix:///Users/halima/.docker/run/docker.sock"),
            "Docker Desktop"
        );
        assert_eq!(
            label_for("unix:///Users/me/.lima/default/sock/docker.sock"),
            "Colima"
        );
        assert_eq!(
            label_for("npipe:////./pipe/podman-machine-default"),
            "Podman"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_sockets_collapse_to_one_candidate() {
        let home = tempfile::tempdir().unwrap();
        let real = home.path().join(".docker/run/docker.sock");
        touch(&real);
        let link = home.path().join("var-run-docker.sock");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let env = DiscoverEnv {
            home: Some(home.path().to_path_buf()),
            system_sockets: vec![link],
            ..DiscoverEnv::default()
        };
        assert_eq!(candidates(&env).len(), 1);
    }
}
