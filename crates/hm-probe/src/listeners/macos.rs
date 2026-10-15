//! macOS: libproc for our own processes, `netstat` for everyone else's.

use std::collections::HashSet;
use std::process::Command;

use hm_core::model::Proto;
use hm_core::raw::RawListener;

use super::netstat;

pub fn read() -> Result<Vec<RawListener>, String> {
    let mut out = super::from_listeners_crate()?;

    // netstat fills in sockets of processes libproc may not inspect (root, other users).
    let seen: HashSet<u32> = out
        .iter()
        .flat_map(|listener| listener.pids.clone())
        .collect();
    if let Ok(output) = Command::new("/usr/sbin/netstat")
        .args(["-anv", "-p", "tcp"])
        .output()
    {
        for row in netstat::parse(&String::from_utf8_lossy(&output.stdout)) {
            if row.pid.is_some_and(|pid| seen.contains(&pid)) {
                continue;
            }
            out.push(RawListener {
                proto: Proto::Tcp,
                addr: row.addr,
                port: row.port,
                pids: row.pid.into_iter().collect(),
                uid: None,
            });
        }
    }
    Ok(out)
}
