//! Parser for macOS `netstat -anv -p tcp`, which reports the PID of every socket,
//! including other users' sockets, without root.
//!
//! Recent macOS prints a `process:pid` column whose process name may contain spaces
//! ("Google Chrome:76915"); older releases print a numeric `pid` column. Columns are
//! located from the header, so both layouts parse.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetstatRow {
    pub addr: IpAddr,
    pub port: u16,
    pub pid: Option<u32>,
    pub process: Option<String>,
}

pub fn parse(text: &str) -> Vec<NetstatRow> {
    let mut lines = text.lines();
    let Some(header) = lines
        .by_ref()
        .find(|line| line.trim_start().starts_with("Proto"))
    else {
        return Vec::new();
    };
    let header: Vec<&str> = header.split_whitespace().collect();
    let Some(pid_col) = header
        .iter()
        .position(|word| *word == "process:pid" || *word == "pid")
    else {
        return Vec::new();
    };
    // "Local Address" and "Foreign Address" are two header words but one row token each.
    let Some(start) = pid_col.checked_sub(2) else {
        return Vec::new();
    };
    let tail = header.len() - pid_col - 1;

    lines
        .filter_map(|line| {
            let tokens: Vec<&str> = line.split_whitespace().collect();
            if tokens.len() < 6 || !tokens[0].starts_with("tcp") || tokens[5] != "LISTEN" {
                return None;
            }
            let (addr, port) = parse_local(tokens[0], tokens[3])?;
            let end = tokens.len().checked_sub(tail)?;
            let (pid, process) = if start < end {
                parse_process(&tokens[start..end].join(" "))
            } else {
                (None, None)
            };
            Some(NetstatRow {
                addr,
                port,
                pid,
                process,
            })
        })
        .collect()
}

/// `127.0.0.1.8080`, `*.5432`, `::1.3000` or `fe80::1%lo0.631`.
fn parse_local(proto: &str, local: &str) -> Option<(IpAddr, u16)> {
    let (host, port) = local.rsplit_once('.')?;
    let port = port.parse().ok()?;
    let addr = if host == "*" {
        if proto == "tcp4" {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        }
    } else {
        let host = host.split('%').next()?;
        host.parse().ok()?
    };
    Some((addr, port))
}

/// `Google Chrome:76915` → (76915, "Google Chrome"); `812` → (812, none).
fn parse_process(field: &str) -> (Option<u32>, Option<String>) {
    match field.rsplit_once(':') {
        Some((name, pid)) => (pid.parse().ok(), Some(name.to_string())),
        None => (field.parse().ok(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODERN: &str = "Active Internet connections (including servers)
Proto Recv-Q Send-Q  Local Address          Foreign Address        (state)          rxbytes      txbytes  rhiwat  shiwat          process:pid    state  options           gencnt    flags   flags1 usecnt rtncnt fltrs
tcp4       0      0  10.0.0.5.55810         160.79.104.10.443      ESTABLISHED         7868         9585  131072  131072           claude:31903  00102 00000000 0000000000a0e5ca 00000081 04000900      2      0 000000
tcp4       0      0  127.0.0.1.9820         *.*                    LISTEN                 0            0  131072  131072    Google Chrome:76915  00100 00000006 0000000000858d85 00000000 00000800      1      0 000000
tcp46      0      0  *.3330                 *.*                    LISTEN                 0            0  131072  131072             node:89507  00100 00000106 00000000003e6d86 00000001 00000800      1      0 000000
tcp6       0      0  ::1.631                *.*                    LISTEN                 0            0  131072  131072            cupsd:412    00100 00000106 00000000003e6d87 00000001 00000800      1      0 000000
tcp4       0      0  *.5000                 *.*                    LISTEN                 0            0  131072  131072    ControlCenter:640  00100 00000106 00000000003e6d88 00000001 00000800      1      0 000000
";

    const LEGACY: &str = "Active Internet connections (including servers)
Proto Recv-Q Send-Q  Local Address          Foreign Address        (state)     rhiwat shiwat    pid   epid  state    options
tcp4       0      0  127.0.0.1.8080         *.*                    LISTEN      131072 131072   1234      0 0x0100 0x00000006
";

    #[test]
    fn parses_modern_rows_with_spaces_in_process_names() {
        let rows = parse(MODERN);
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows[0],
            NetstatRow {
                addr: "127.0.0.1".parse().unwrap(),
                port: 9820,
                pid: Some(76915),
                process: Some("Google Chrome".into())
            }
        );
        assert_eq!(rows[1].addr, "::".parse::<IpAddr>().unwrap());
        assert_eq!(rows[1].port, 3330);
        assert_eq!(rows[2].addr, "::1".parse::<IpAddr>().unwrap());
        assert_eq!(rows[3].addr, "0.0.0.0".parse::<IpAddr>().unwrap());
        assert_eq!(rows[3].process.as_deref(), Some("ControlCenter"));
    }

    #[test]
    fn parses_legacy_numeric_pid_column() {
        assert_eq!(
            parse(LEGACY),
            vec![NetstatRow {
                addr: "127.0.0.1".parse().unwrap(),
                port: 8080,
                pid: Some(1234),
                process: None
            }]
        );
    }

    #[test]
    fn returns_nothing_without_a_header() {
        assert!(parse("garbage\nmore garbage").is_empty());
    }
}
