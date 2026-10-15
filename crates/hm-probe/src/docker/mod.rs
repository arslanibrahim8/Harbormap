//! Docker-compatible engines: discovery, then one container listing per reachable
//! engine. Problems become notes; probing never fails as a whole.

pub mod discover;

/// True when an executable with this name is on `PATH`.
pub fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        dir.join(name).is_file() || (cfg!(windows) && dir.join(format!("{name}.exe")).is_file())
    })
}
