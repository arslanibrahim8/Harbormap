//! The process table, read through sysinfo and reduced to what Harbormap may keep:
//! redacted argv and environment *names* plus the allowed values.

use std::path::{Path, PathBuf};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, Uid, UpdateKind, Users};

use hm_core::env;
use hm_core::raw::RawProcess;
use hm_core::redact::redact_argv;

pub struct ProcessTable {
    pub processes: Vec<RawProcess>,
    pub self_pid: u32,
    pub current_uid: Option<String>,
}

pub fn read_processes() -> ProcessTable {
    let refresh = ProcessRefreshKind::nothing()
        .with_memory()
        .with_user(UpdateKind::Always)
        .with_cwd(UpdateKind::Always)
        .with_cmd(UpdateKind::Always)
        .with_exe(UpdateKind::Always)
        .with_environ(UpdateKind::Always)
        .without_tasks();
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
    let users = Users::new_with_refreshed_list();

    let self_pid = sysinfo::get_current_pid().map_or(std::process::id(), Pid::as_u32);
    let current_uid = system
        .process(Pid::from_u32(self_pid))
        .and_then(|process| process.user_id())
        .map(uid_key);

    let mut processes: Vec<RawProcess> = system
        .processes()
        .values()
        .map(|process| {
            let name = process.name().to_string_lossy().into_owned();
            let mut argv: Vec<String> = process
                .cmd()
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            if cfg!(target_os = "macos") {
                drop_environment_tail(&mut argv);
            }
            if argv.is_empty() {
                argv.push(name.clone());
            }
            RawProcess {
                pid: process.pid().as_u32(),
                ppid: process.parent().map(Pid::as_u32),
                start_time: process.start_time(),
                name,
                exe: process.exe().map(ToOwned::to_owned),
                argv: redact_argv(&argv),
                cwd: process.cwd().map(clean_cwd),
                uid: process.user_id().map(uid_key),
                user: process
                    .user_id()
                    .and_then(|uid| users.get_user_by_id(uid))
                    .map(|user| user.name().to_string()),
                rss_bytes: process.memory(),
                env: env::extract_from_pairs(
                    process
                        .environ()
                        .iter()
                        .map(|entry| entry.to_string_lossy()),
                ),
            }
        })
        .collect();
    processes.sort_by_key(|process| process.pid);
    ProcessTable {
        processes,
        self_pid,
        current_uid,
    }
}

/// Linux reports a working folder that was removed as "/path (deleted)". The
/// process still belongs where it started, so the suffix is dropped.
pub fn clean_cwd(cwd: &Path) -> PathBuf {
    match cwd
        .to_str()
        .and_then(|text| text.strip_suffix(" (deleted)"))
    {
        Some(original) => PathBuf::from(original),
        None => cwd.to_path_buf(),
    }
}

/// On macOS, a process that renamed itself (node, which runs npm and Next.js, or perl)
/// keeps its new title in argv[0] and blanks the rest of its original arguments. The
/// argument reader skips the blanks and returns the first environment entries in their
/// place, values included. Environment values must never leave the probe (spec §15), so
/// a run of `NAME=value` or blank arguments at the end is dropped. Real argument lists
/// rarely end that way, and dropping one loses only display detail.
fn drop_environment_tail(argv: &mut Vec<String>) {
    while argv
        .last()
        .is_some_and(|arg| arg.trim().is_empty() || is_environment_entry(arg))
    {
        argv.pop();
    }
}

/// `NAME=value`, where NAME is a letter or `_` followed by letters, digits or `_`.
fn is_environment_entry(arg: &str) -> bool {
    arg.split_once('=').is_some_and(|(name, _)| {
        name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The uid on Unix, the SID string on Windows.
fn uid_key(uid: &Uid) -> String {
    (**uid).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleted_working_folders_keep_their_original_path() {
        assert_eq!(
            clean_cwd(Path::new("/work/shop/apps/web (deleted)")),
            PathBuf::from("/work/shop/apps/web")
        );
        assert_eq!(
            clean_cwd(Path::new("/work/shop")),
            PathBuf::from("/work/shop")
        );
    }

    #[test]
    fn environment_entries_left_by_a_renamed_process_are_dropped() {
        // What macOS returns for node after `process.title = "npm run dev"`, and for perl
        // after `$0 = "worker"`: the title, perhaps a blank, then environment entries.
        let mut node = args(&[
            "npm run dev",
            "COMMAND_MODE=unix2003",
            "HOME=/Users/me",
            "API_TOKEN=s3cr3t",
        ]);
        drop_environment_tail(&mut node);
        assert_eq!(node, args(&["npm run dev"]));

        let mut perl = args(&["worker", "        ", "HOME=/Users/me"]);
        drop_environment_tail(&mut perl);
        assert_eq!(perl, args(&["worker"]));

        // Ordinary arguments stay, including a `NAME=value` that is not last.
        let make = args(&["make", "CC=clang", "-j4"]);
        let mut kept = make.clone();
        drop_environment_tail(&mut kept);
        assert_eq!(kept, make);

        let vite = args(&["node", "vite", "--port=5173", "http://localhost/?a=b"]);
        let mut kept = vite.clone();
        drop_environment_tail(&mut kept);
        assert_eq!(kept, vite);
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }
}
