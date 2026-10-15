//! The snapshot: everything Harbormap knows about the computer at one instant, after
//! projects, services and owners have been resolved. `harbormap status --json` prints
//! this structure, so its field names are a public contract.

use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::env::EnvFacts;

/// Bumped when a field is renamed or removed. Adding a field does not bump it.
pub const SNAPSHOT_SCHEMA: u32 = 1;

/// Paths print as text even when they are not valid UTF-8 (a Linux folder with a Latin-1
/// name): such bytes become U+FFFD, so one odd path cannot fail a whole snapshot.
mod lossy {
    use std::path::{Path, PathBuf};

    use serde::Serializer;

    pub fn path<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&path.to_string_lossy())
    }

    pub fn option<S: Serializer>(path: &Option<PathBuf>, serializer: S) -> Result<S::Ok, S::Error> {
        match path {
            Some(path) => serializer.serialize_some(&*path.to_string_lossy()),
            None => serializer.serialize_none(),
        }
    }

    pub fn list<S: Serializer>(paths: &[PathBuf], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(paths.iter().map(|path| path.to_string_lossy()))
    }
}

/// Lane on the Harbor map (Product design §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    See,
    Work,
    Keep,
    Outside,
    Shared,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    DevServer,
    Database,
    Cache,
    Worker,
    System,
    Ide,
    Terminal,
    AgentCli,
    Unknown,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Proto {
    #[default]
    Tcp,
}

/// Who can reach a listening socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindScope {
    /// 127.0.0.0/8 or ::1: only this computer.
    Loopback,
    /// 0.0.0.0 or ::: every network interface.
    All,
    /// One specific interface address.
    Specific,
}

impl BindScope {
    pub fn of(addr: IpAddr) -> Self {
        let addr = canonical_ip(addr);
        if addr.is_loopback() {
            BindScope::Loopback
        } else if addr.is_unspecified() {
            BindScope::All
        } else {
            BindScope::Specific
        }
    }
}

/// Turns IPv4-mapped IPv6 addresses (`::ffff:127.0.0.1`) into plain IPv4.
pub fn canonical_ip(addr: IpAddr) -> IpAddr {
    match addr {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        IpAddr::V4(v4) => IpAddr::V4(v4),
    }
}

/// Which part of the screen an item belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    /// The project Harbormap was opened in.
    Project,
    /// Other projects and containers on this computer. Read-only.
    Elsewhere,
    /// System programs, apps outside projects and owners Harbormap cannot see.
    System,
}

/// Who holds a listening socket.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Owner {
    Process {
        pid: u32,
    },
    ContainerForwarder {
        container_id: String,
    },
    /// The operating system or an engine helper; `pid` when a process is visible.
    System {
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pid: Option<u32>,
    },
    Unknown {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Sure,
    Probably,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema: u32,
    /// Unix seconds when the probe ran.
    pub taken_at: u64,
    pub host: Host,
    /// ID of the project Harbormap was opened in, if any.
    pub opened: Option<String>,
    pub projects: Vec<Project>,
    pub services: Vec<Service>,
    pub listeners: Vec<Listener>,
    pub processes: Vec<Process>,
    pub containers: Vec<Container>,
    pub engines: Vec<Engine>,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(serialize_with = "lossy::path")]
    pub root: PathBuf,
    pub opened: bool,
    /// False for a plain folder without a git repository.
    pub git: bool,
    pub worktrees: Vec<Worktree>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    #[serde(serialize_with = "lossy::path")]
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub section: Section,
    /// `None` for containers that belong to no project.
    pub project_id: Option<String>,
    #[serde(serialize_with = "lossy::option")]
    pub worktree: Option<PathBuf>,
    pub signature: Option<String>,
    pub framework: Option<String>,
    pub confidence: Confidence,
    pub role: Role,
    pub kind: Kind,
    pub source: ServiceSource,
    pub ports: Vec<u16>,
    pub urls: Vec<String>,
    pub state: String,
    pub health: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServiceSource {
    /// `main_pid` holds the port, `root_pid` is the top of its wrapper chain
    /// (for example `npm run dev`), and `pids` is the root plus all descendants.
    Process {
        main_pid: u32,
        root_pid: u32,
        pids: Vec<u32>,
    },
    Container {
        container_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listener {
    pub proto: Proto,
    pub addr: IpAddr,
    pub port: u16,
    pub bind: BindScope,
    pub owner: Owner,
    pub section: Section,
    pub project_id: Option<String>,
    pub service_id: Option<String>,
    /// Short human label for the owner, e.g. "web (Vite)" or "AirPlay Receiver".
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Process {
    pub pid: u32,
    pub ppid: Option<u32>,
    /// Unix seconds. Together with `pid` this is the process identity.
    pub start_time: u64,
    pub name: String,
    #[serde(serialize_with = "lossy::option")]
    pub exe: Option<PathBuf>,
    /// Redacted before it ever reaches this struct.
    pub argv: Vec<String>,
    #[serde(serialize_with = "lossy::option")]
    pub cwd: Option<PathBuf>,
    pub user: Option<String>,
    /// True when the current OS user owns the process.
    pub own: bool,
    pub signature: Option<String>,
    pub kind: Kind,
    pub project_id: Option<String>,
    #[serde(serialize_with = "lossy::option")]
    pub worktree: Option<PathBuf>,
    pub rss_bytes: u64,
    pub uptime_s: u64,
    pub env: EnvFacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Container {
    pub id: String,
    /// ID of the engine that runs it.
    pub engine: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub health: Option<String>,
    pub exit_code: Option<i64>,
    pub oom_killed: bool,
    pub restart_count: i64,
    pub started_at: Option<String>,
    pub ports: Vec<PortBinding>,
    pub networks: Vec<String>,
    pub mounts: Vec<Mount>,
    pub compose: Option<Compose>,
    pub signature: Option<String>,
    pub section: Section,
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PortBinding {
    pub container_port: u16,
    pub proto: String,
    pub host_ip: Option<IpAddr>,
    pub host_port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mount {
    /// "volume", "bind", "tmpfs", ...
    pub kind: String,
    /// Volume name, for volumes.
    pub name: Option<String>,
    /// Host path, for bind mounts.
    #[serde(serialize_with = "lossy::option")]
    pub source: Option<PathBuf>,
    pub destination: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compose {
    pub project: String,
    pub service: String,
    #[serde(serialize_with = "lossy::option")]
    pub working_dir: Option<PathBuf>,
    #[serde(serialize_with = "lossy::list")]
    pub config_files: Vec<PathBuf>,
    pub config_hash: Option<String>,
    pub oneoff: bool,
    pub number: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Engine {
    pub id: String,
    /// "Docker Desktop", "OrbStack", "Colima", "Rancher Desktop", "Podman" or "Docker".
    pub name: String,
    pub endpoint: String,
    pub version: Option<String>,
    pub reachable: bool,
    pub error: Option<String>,
}

/// Something the user should know about the snapshot itself, such as a gap in what
/// Harbormap could see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub code: String,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_scope_classifies_addresses() {
        assert_eq!(
            BindScope::of("127.0.0.1".parse().unwrap()),
            BindScope::Loopback
        );
        assert_eq!(BindScope::of("::1".parse().unwrap()), BindScope::Loopback);
        assert_eq!(
            BindScope::of("::ffff:127.0.0.1".parse().unwrap()),
            BindScope::Loopback
        );
        assert_eq!(BindScope::of("0.0.0.0".parse().unwrap()), BindScope::All);
        assert_eq!(BindScope::of("::".parse().unwrap()), BindScope::All);
        assert_eq!(
            BindScope::of("192.168.1.20".parse().unwrap()),
            BindScope::Specific
        );
    }

    #[test]
    fn owner_serializes_with_a_kind_tag() {
        let json = serde_json::to_value(Owner::Process { pid: 812 }).unwrap();
        assert_eq!(json, serde_json::json!({"kind": "process", "pid": 812}));
        let json = serde_json::to_value(Owner::ContainerForwarder {
            container_id: "abc".into(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "container_forwarder", "container_id": "abc"})
        );
    }

    #[test]
    fn paths_that_are_not_utf8_still_serialize() {
        #[cfg(unix)]
        let odd = {
            use std::os::unix::ffi::OsStrExt;
            PathBuf::from(std::ffi::OsStr::from_bytes(b"/srv/caf\xe9"))
        };
        #[cfg(windows)]
        let odd = {
            use std::os::windows::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_wide(&[0x43, 0x3a, 0x5c, 0xd800]))
        };
        let worktree = Worktree {
            path: odd.clone(),
            branch: None,
            head: None,
        };
        let compose = Compose {
            project: "shop".into(),
            service: "db".into(),
            working_dir: Some(odd.clone()),
            config_files: vec![odd],
            config_hash: None,
            oneoff: false,
            number: None,
        };
        for json in [
            serde_json::to_string(&worktree).unwrap(),
            serde_json::to_string(&compose).unwrap(),
        ] {
            assert!(json.contains('\u{FFFD}'), "{json}");
        }
    }

    #[test]
    fn enums_use_snake_case() {
        assert_eq!(serde_json::to_value(Kind::DevServer).unwrap(), "dev_server");
        assert_eq!(serde_json::to_value(Role::See).unwrap(), "see");
        assert_eq!(
            serde_json::to_value(Section::Elsewhere).unwrap(),
            "elsewhere"
        );
        let source = ServiceSource::Container {
            container_id: "c1".into(),
        };
        assert_eq!(
            serde_json::to_value(source).unwrap(),
            serde_json::json!({"type": "container", "container_id": "c1"})
        );
    }
}
