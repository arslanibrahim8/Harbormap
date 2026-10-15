//! Windows: `GetExtendedTcpTable` through the `listeners` crate.

use hm_core::raw::RawListener;

pub fn read() -> Result<Vec<RawListener>, String> {
    super::from_listeners_crate()
}
