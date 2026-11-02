//! The file watcher: turns raw filesystem events into batches of changed paths with a
//! 100 ms quiet period and a 500 ms cap (Learn spec §5.4).
//!
//! The [`Debouncer`] is pure (the caller passes the clock), so its timing is tested
//! without sleeping. The [`Watcher`] wraps `notify`, drops paths the walk rules
//! reject and runs the debouncer on its own thread.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::event::{CreateKind, ModifyKind};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::walk::WalkRules;

/// How long the tree must be silent before a batch is flushed.
pub const QUIET: Duration = Duration::from_millis(100);
/// The longest a batch waits, however steady the stream of events.
pub const MAX_WAIT: Duration = Duration::from_millis(500);

/// Whether one watch per admitted folder is needed. macOS and Windows watch the root
/// as one recursive stream; inotify has no recursion, so on Linux `node_modules` and
/// the other skipped folders get no watch at all.
const PER_FOLDER_WATCHES: bool = cfg!(target_os = "linux");

/// The changes that settled together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    /// Changed paths, relative to the root and `/`-separated. A path may name a folder
    /// that was removed or moved away; callers treat a vanished path as removing
    /// everything indexed under it.
    pub paths: BTreeSet<String>,
    /// Events were lost or the tree moved wholesale: the caller must rescan.
    pub rescan: bool,
    /// When the first event of this batch arrived.
    pub first_event: Instant,
}

/// Collects events and says when they have settled. It never reads the clock itself.
#[derive(Debug)]
pub struct Debouncer {
    quiet: Duration,
    max_wait: Duration,
    paths: BTreeSet<String>,
    rescan: bool,
    /// The first and the last event of the pending batch.
    window: Option<(Instant, Instant)>,
}

impl Debouncer {
    pub fn new(quiet: Duration, max_wait: Duration) -> Self {
        Debouncer {
            quiet,
            max_wait,
            paths: BTreeSet::new(),
            rescan: false,
            window: None,
        }
    }

    /// Records a change to `rel` at `now`.
    pub fn push(&mut self, now: Instant, rel: String) {
        self.touch(now);
        self.paths.insert(rel);
    }

    /// Records that events were lost: the next batch asks for a rescan.
    pub fn push_rescan(&mut self, now: Instant) {
        self.touch(now);
        self.rescan = true;
    }

    fn touch(&mut self, now: Instant) {
        let first = self.window.map_or(now, |(first, _)| first);
        self.window = Some((first, now));
    }

    /// The pending batch, if its quiet period or its cap has passed at `now`.
    pub fn poll(&mut self, now: Instant) -> Option<Batch> {
        if now < self.next_deadline()? {
            return None;
        }
        let (first_event, _) = self.window.take()?;
        Some(Batch {
            paths: std::mem::take(&mut self.paths),
            rescan: std::mem::take(&mut self.rescan),
            first_event,
        })
    }

    /// When the pending batch is due, or `None` when nothing is pending.
    pub fn next_deadline(&self) -> Option<Instant> {
        let (first, last) = self.window?;
        Some((last + self.quiet).min(first + self.max_wait))
    }
}

/// What the watcher thread listens to.
enum Msg {
    Event(notify::Result<Event>),
    Stop,
}

/// Watches a worktree and hands out debounced batches.
pub struct Watcher {
    batches: Receiver<Batch>,
    control: Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    /// Starts watching `root`. `dirs` are the admitted folders from the walk (`""` is
    /// the root); `rules` drop the paths the walk would not index.
    pub fn start(root: &Path, rules: WalkRules, dirs: &[String]) -> io::Result<Watcher> {
        let root = dunce::canonicalize(root)?;
        let (control, events) = mpsc::channel::<Msg>();
        let (out, batches) = mpsc::channel::<Batch>();

        let sender = control.clone();
        let mut watcher = notify::recommended_watcher(move |res| {
            // The thread is gone once the watcher is dropped; nothing to tell then.
            let _ = sender.send(Msg::Event(res));
        })
        .map_err(io::Error::other)?;
        if PER_FOLDER_WATCHES {
            for dir in dirs {
                watcher
                    .watch(&join_rel(&root, dir), RecursiveMode::NonRecursive)
                    .map_err(io::Error::other)?;
            }
        } else {
            watcher
                .watch(&root, RecursiveMode::Recursive)
                .map_err(io::Error::other)?;
        }

        let mut run = Run {
            root,
            rules,
            watcher,
            debouncer: Debouncer::new(QUIET, MAX_WAIT),
        };
        let thread = std::thread::Builder::new()
            .name("hm-watch".into())
            .spawn(move || run.serve(&events, &out))?;
        Ok(Watcher {
            batches,
            control,
            thread: Some(thread),
        })
    }

    /// The next batch, or `None` if none settled within `d`.
    pub fn recv_timeout(&self, d: Duration) -> Option<Batch> {
        self.batches.recv_timeout(d).ok()
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.control.send(Msg::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// `root` joined with a `/`-separated relative path (`""` is `root` itself).
fn join_rel(root: &Path, rel: &str) -> PathBuf {
    if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    }
}

/// The state of the watcher thread.
struct Run {
    root: PathBuf,
    rules: WalkRules,
    watcher: RecommendedWatcher,
    debouncer: Debouncer,
}

impl Run {
    fn serve(&mut self, events: &Receiver<Msg>, out: &Sender<Batch>) {
        loop {
            let wait = match self.debouncer.next_deadline() {
                Some(deadline) => deadline.saturating_duration_since(Instant::now()),
                None => Duration::from_secs(3600),
            };
            match events.recv_timeout(wait) {
                Ok(Msg::Event(res)) => self.handle(Instant::now(), res),
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
            if let Some(batch) = self.debouncer.poll(Instant::now())
                && out.send(batch).is_err()
            {
                return;
            }
        }
    }

    fn handle(&mut self, now: Instant, res: notify::Result<Event>) {
        let event = match res {
            Ok(event) => event,
            Err(_) => return self.debouncer.push_rescan(now),
        };
        if event.need_rescan() {
            self.debouncer.push_rescan(now);
        }
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        // A created or renamed-to folder may arrive with content that no event
        // reports (a moved tree, or files written before the watch was added).
        let may_bring_content = matches!(
            event.kind,
            EventKind::Create(CreateKind::Any | CreateKind::Folder)
                | EventKind::Modify(ModifyKind::Name(_))
        );
        // Rename events carry the old and the new path; both are reported.
        for path in &event.paths {
            let Some(rel) = rel_of(&self.root, path) else {
                continue;
            };
            if rel.is_empty() {
                continue;
            }
            let is_dir = fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
            if is_dir {
                if may_bring_content && self.rules.admits_dir(&rel) {
                    self.enter_dir(now, &rel);
                }
            } else if self.rules.admits(&rel) {
                self.debouncer.push(now, rel);
            }
        }
    }

    /// Watches the new folder `rel` (on Linux) and reports the files already in it.
    fn enter_dir(&mut self, now: Instant, rel: &str) {
        if PER_FOLDER_WATCHES {
            let added = self
                .watcher
                .watch(&join_rel(&self.root, rel), RecursiveMode::NonRecursive);
            // A folder that is gone again is reported by its own event. Any other
            // failure (an exhausted inotify limit, no permission) leaves files in
            // this folder unseen, so the caller must rescan.
            if let Err(e) = added
                && on_watch_error(&e) == WatchErrorAction::Rescan
            {
                self.debouncer.push_rescan(now);
            }
        }
        let Ok(entries) = fs::read_dir(join_rel(&self.root, rel)) else {
            return;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let child = format!("{rel}/{name}");
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if self.rules.admits_dir(&child) {
                    self.enter_dir(now, &child);
                }
            } else if self.rules.admits(&child) {
                self.debouncer.push(now, child);
            }
        }
    }
}

/// What to do when adding a watch for a new folder fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchErrorAction {
    /// The folder vanished before the watch was added; its own event says so.
    Ignore,
    /// Changes in the folder may go unseen; ask the caller to rescan.
    Rescan,
}

fn on_watch_error(e: &notify::Error) -> WatchErrorAction {
    match &e.kind {
        notify::ErrorKind::PathNotFound => WatchErrorAction::Ignore,
        notify::ErrorKind::Io(io) if io.kind() == io::ErrorKind::NotFound => {
            WatchErrorAction::Ignore
        }
        _ => WatchErrorAction::Rescan,
    }
}

/// `path` relative to `root`, `/`-separated; `None` when it lies outside the root.
fn rel_of(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<&str> = rel
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect::<Option<_>>()?;
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn set<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
        items.into_iter().map(str::to_owned).collect()
    }

    #[test]
    fn only_a_vanished_folder_is_forgiven_when_a_watch_cannot_be_added() {
        use notify::{Error, ErrorKind};
        let gone = |e: Error| on_watch_error(&e);
        assert_eq!(gone(Error::path_not_found()), WatchErrorAction::Ignore);
        let not_found = Error::io(io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(gone(not_found), WatchErrorAction::Ignore);
        let denied = Error::io(io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(gone(denied), WatchErrorAction::Rescan);
        assert_eq!(
            gone(Error::new(ErrorKind::MaxFilesWatch)),
            WatchErrorAction::Rescan
        );
        assert_eq!(gone(Error::generic("x")), WatchErrorAction::Rescan);
    }

    #[test]
    fn quiet_period_flushes_after_100ms_of_silence() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(QUIET, MAX_WAIT);
        d.push(t0, "a.ts".into());
        d.push(t0 + ms(60), "b.ts".into());
        assert!(d.poll(t0 + ms(150)).is_none()); // only 90 ms since the last event
        let b = d.poll(t0 + ms(161)).unwrap();
        assert_eq!(b.paths, set(["a.ts", "b.ts"]));
        assert!(!b.rescan);
        assert_eq!(b.first_event, t0);
    }

    #[test]
    fn a_steady_stream_flushes_at_500ms() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(QUIET, MAX_WAIT);
        for i in 0..17 {
            d.push(t0 + ms(i * 30), format!("f{i}.ts")); // last push at 480 ms
        }
        assert!(d.poll(t0 + ms(499)).is_none());
        assert!(d.poll(t0 + ms(500)).is_some());
    }

    #[test]
    fn a_rescan_request_rides_in_the_batch_and_the_debouncer_resets() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(QUIET, MAX_WAIT);
        assert!(d.next_deadline().is_none());
        d.push_rescan(t0);
        assert_eq!(d.next_deadline(), Some(t0 + ms(100)));
        let b = d.poll(t0 + ms(100)).unwrap();
        assert!(b.rescan && b.paths.is_empty());
        assert!(d.next_deadline().is_none());
        assert!(d.poll(t0 + ms(1000)).is_none());
    }
}
