//! What the probes report before any interpretation.
//!
//! Debug builds of the CLI can load a recorded inventory from JSON
//! (`HARBORMAP_FIXTURE`), which keeps resolution tests independent of the machine
//! they run on. Every optional field therefore has a serde default.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::env::EnvFacts;
use crate::model::{Engine, Mount, Note, PortBinding, Proto};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RawInventory {
    /// Unix seconds when the probe ran.
    pub taken_at: u64,
    /// `std::env::consts::OS` of the probing machine.
    pub os: String,
    pub arch: String,
    /// PID of the Harbormap process that ran the probe.
    pub self_pid: u32,
    /// Platform user key of the current user: the uid on Unix, the SID on Windows.
    pub current_uid: Option<String>,
    pub processes: Vec<RawProcess>,
    pub listeners: Vec<RawListener>,
    pub containers: Vec<RawContainer>,
    pub engines: Vec<Engine>,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RawProcess {
    pub pid: u32,
    pub ppid: Option<u32>,
    /// Unix seconds.
    pub start_time: u64,
    pub name: String,
    pub exe: Option<PathBuf>,
    /// Already redacted.
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Same format as `RawInventory::current_uid`.
    pub uid: Option<String>,
    pub user: Option<String>,
    pub rss_bytes: u64,
    pub env: EnvFacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawListener {
    #[serde(default)]
    pub proto: Proto,
    pub addr: IpAddr,
    pub port: u16,
    /// Every process that holds the socket. Empty when the owner is not visible.
    #[serde(default)]
    pub pids: Vec<u32>,
    /// Socket owner uid when the OS reports one without a visible process (Linux).
    #[serde(default)]
    pub uid: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RawContainer {
    /// ID of the engine that reported it.
    pub engine: String,
    pub id: String,
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
    /// Only `com.docker.compose.*` labels are kept.
    pub labels: BTreeMap<String, String>,
}
