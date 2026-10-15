//! Listening TCP sockets and the processes that hold them.
//!
//! - Linux: `/proc/net/tcp{,6}` plus socket inodes under `/proc/<pid>/fd`. Sockets
//!   whose owner we cannot see are kept with an empty `pids` list.
//! - macOS: libproc (through the `listeners` crate) for our own processes, plus
//!   `netstat -anv -p tcp` for other users' processes, which it shows without root.
//! - Windows: `GetExtendedTcpTable` (through the `listeners` crate), which reports
//!   every owner PID without admin rights.

use std::collections::BTreeMap;
use std::net::IpAddr;

use hm_core::model::{Proto, canonical_ip};
use hm_core::raw::RawListener;

pub mod linux;
pub mod netstat;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub fn read_listeners() -> Result<Vec<RawListener>, String> {
    platform_read().map(merge)
}

#[cfg(target_os = "linux")]
fn platform_read() -> Result<Vec<RawListener>, String> {
    linux::read()
}

#[cfg(target_os = "macos")]
fn platform_read() -> Result<Vec<RawListener>, String> {
    macos::read()
}

#[cfg(target_os = "windows")]
fn platform_read() -> Result<Vec<RawListener>, String> {
    windows::read()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_read() -> Result<Vec<RawListener>, String> {
    Err("this operating system is not supported".into())
}

/// Listening TCP sockets reported by the `listeners` crate: libproc on macOS,
/// `GetExtendedTcpTable` on Windows.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn from_listeners_crate() -> Result<Vec<RawListener>, String> {
    let all = listeners::get_all().map_err(|error| error.to_string())?;
    Ok(all
        .into_iter()
        .filter(|listener| {
            listener.protocol == listeners::Protocol::TCP
                && listener.state == listeners::SocketState::Listen
        })
        .map(|listener| RawListener {
            proto: Proto::Tcp,
            addr: listener.socket.ip(),
            port: listener.socket.port(),
            pids: vec![listener.process.pid],
            uid: None,
        })
        .collect())
}

/// One entry per (protocol, address, port), with the union of owner PIDs.
pub fn merge(raw: Vec<RawListener>) -> Vec<RawListener> {
    let mut merged: BTreeMap<(Proto, IpAddr, u16), RawListener> = BTreeMap::new();
    for listener in raw {
        let addr = canonical_ip(listener.addr);
        let entry = merged
            .entry((listener.proto, addr, listener.port))
            .or_insert_with(|| RawListener {
                proto: listener.proto,
                addr,
                port: listener.port,
                pids: Vec::new(),
                uid: None,
            });
        for pid in listener.pids {
            if !entry.pids.contains(&pid) {
                entry.pids.push(pid);
            }
        }
        entry.uid = entry.uid.or(listener.uid);
    }
    merged
        .into_values()
        .map(|mut listener| {
            listener.pids.sort_unstable();
            listener
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener(addr: &str, port: u16, pids: &[u32]) -> RawListener {
        RawListener {
            proto: Proto::Tcp,
            addr: addr.parse().unwrap(),
            port,
            pids: pids.to_vec(),
            uid: None,
        }
    }

    #[test]
    fn merge_unions_pids_of_the_same_socket() {
        let merged = merge(vec![
            listener("127.0.0.1", 8000, &[20]),
            listener("::ffff:127.0.0.1", 8000, &[10, 20]),
            listener("::", 8000, &[30]),
        ]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].addr, "127.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(merged[0].pids, vec![10, 20]);
        assert_eq!(merged[1].pids, vec![30]);
    }
}
