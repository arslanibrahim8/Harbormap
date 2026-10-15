//! Windows: `GetExtendedTcpTable` through the `listeners` crate.

use hm_core::model::Proto;
use hm_core::raw::RawListener;

pub fn read() -> Result<Vec<RawListener>, String> {
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
