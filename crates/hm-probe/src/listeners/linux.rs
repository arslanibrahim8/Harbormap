//! Linux: parse `/proc/net/tcp` and `/proc/net/tcp6`, then map socket inodes to
//! the processes whose file descriptors point at them.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// One listening socket from a `/proc/net/tcp*` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcSocket {
    pub addr: IpAddr,
    pub port: u16,
    pub uid: u32,
    pub inode: u64,
}

const TCP_LISTEN: &str = "0A";

/// Parses a `/proc/net/tcp` (IPv4) or `/proc/net/tcp6` (IPv6) table and keeps the
/// sockets in LISTEN state. Addresses are printed in host byte order per 32-bit word.
pub fn parse_proc_net_tcp(text: &str) -> Vec<ProcSocket> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 10 || cols[3] != TCP_LISTEN {
                return None;
            }
            let (addr_hex, port_hex) = cols[1].split_once(':')?;
            Some(ProcSocket {
                addr: parse_hex_addr(addr_hex)?,
                port: u16::from_str_radix(port_hex, 16).ok()?,
                uid: cols[7].parse().ok()?,
                inode: cols[9].parse().ok()?,
            })
        })
        .collect()
}

fn parse_hex_addr(hex: &str) -> Option<IpAddr> {
    let word = |chunk: &str| u32::from_str_radix(chunk, 16).ok().map(u32::to_ne_bytes);
    match hex.len() {
        8 => Some(IpAddr::V4(Ipv4Addr::from(word(hex)?))),
        32 => {
            let mut bytes = [0u8; 16];
            for (index, out) in bytes.chunks_mut(4).enumerate() {
                out.copy_from_slice(&word(hex.get(index * 8..index * 8 + 8)?)?);
            }
            Some(IpAddr::V6(Ipv6Addr::from(bytes)))
        }
        _ => None,
    }
}

/// `socket:[12345]` → `12345`.
pub fn parse_socket_link(link: &str) -> Option<u64> {
    link.strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

#[cfg(target_os = "linux")]
pub fn read() -> Result<Vec<hm_core::raw::RawListener>, String> {
    use std::collections::HashMap;

    use hm_core::model::Proto;
    use hm_core::raw::RawListener;

    let mut sockets = Vec::new();
    for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
        match std::fs::read_to_string(path) {
            Ok(text) => sockets.extend(parse_proc_net_tcp(&text)),
            // tcp6 is missing when IPv6 is disabled.
            Err(error) if path.ends_with('6') && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot read {path}: {error}")),
        }
    }

    let mut owners: HashMap<u64, Vec<u32>> = HashMap::new();
    let proc_dir =
        std::fs::read_dir("/proc").map_err(|error| format!("cannot read /proc: {error}"))?;
    for entry in proc_dir.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        // Other users' fd directories are unreadable without root; skip them.
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd.path())
                && let Some(inode) = target.to_str().and_then(parse_socket_link)
            {
                owners.entry(inode).or_default().push(pid);
            }
        }
    }

    Ok(sockets
        .into_iter()
        .map(|socket| RawListener {
            proto: Proto::Tcp,
            addr: socket.addr,
            port: socket.port,
            pids: owners.get(&socket.inode).cloned().unwrap_or_default(),
            uid: Some(socket.uid),
        })
        .collect())
}

#[cfg(all(test, target_endian = "little"))]
mod tests {
    use super::*;

    const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41234 1 0000000000000000 100 0 0 10 0
   1: 00000000:1538 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 41300 1 0000000000000000 100 0 0 10 0
   2: 0100007F:D431 0100007F:1F90 01 00000000:00000000 00:00000000 00000000  1000        0 41400 1 0000000000000000 20 4 30 10 -1
";

    const TCP6: &str = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:0BB8 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 52000 1 0000000000000000 100 0 0 10 0
   1: 00000000000000000000000000000000:1F40 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 52001 1 0000000000000000 100 0 0 10 0
";

    #[test]
    fn parses_ipv4_listeners_and_skips_other_states() {
        let sockets = parse_proc_net_tcp(TCP);
        assert_eq!(
            sockets,
            vec![
                ProcSocket {
                    addr: "127.0.0.1".parse().unwrap(),
                    port: 8080,
                    uid: 1000,
                    inode: 41234
                },
                ProcSocket {
                    addr: "0.0.0.0".parse().unwrap(),
                    port: 5432,
                    uid: 0,
                    inode: 41300
                },
            ]
        );
    }

    #[test]
    fn parses_ipv6_listeners() {
        let sockets = parse_proc_net_tcp(TCP6);
        assert_eq!(sockets[0].addr, "::1".parse::<IpAddr>().unwrap());
        assert_eq!(sockets[0].port, 3000);
        assert_eq!(sockets[1].addr, "::".parse::<IpAddr>().unwrap());
        assert_eq!(sockets[1].port, 8000);
    }

    #[test]
    fn parses_socket_links() {
        assert_eq!(parse_socket_link("socket:[41234]"), Some(41234));
        assert_eq!(parse_socket_link("pipe:[123]"), None);
        assert_eq!(parse_socket_link("/dev/null"), None);
    }
}
