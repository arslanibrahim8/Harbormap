//! `harbormap learn`: read a project, serve the panel on this computer, open it in the
//! browser, and keep it current while files are saved.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;

use hm_index::index::{Delta, Indexer};
use hm_index::location::{facts_db, worktree_root};
use hm_index::watch::{Batch, Watcher};
use hm_project::git::discover;
use hm_serve::{FactsDelta, Handle, Model, Repo, ServeConfig, Server};

use crate::cli::LearnArgs;
use crate::index::open;
use crate::status::{printable, write_stdout};

/// How long the indexer thread waits for a batch before it looks around again.
const POLL: Duration = Duration::from_secs(1);

pub fn run(args: &LearnArgs, json: bool) -> anyhow::Result<ExitCode> {
    let start = match &args.path {
        Some(path) => path.clone(),
        None => std::env::current_dir().context("cannot read the current folder")?,
    };
    let root = worktree_root(&start)?;
    let name = root.file_name().map_or_else(
        || root.display().to_string(),
        |n| n.to_string_lossy().into(),
    );
    let mut indexer = open(&root)?;
    indexer.full()?;
    let model = Model::load(indexer.store(), &name, None)?;
    let config = ServeConfig {
        root: root.clone(),
        db: facts_db(&root),
        repo: repo(&root, &name),
        token: None,
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("cannot start the server")?;
    runtime.block_on(async {
        let server = Server::bind(config, model)
            .await
            .context("cannot start the server")?;
        let url = server.url();
        let text = if json {
            format!(
                "{}\n",
                serde_json::json!({ "url": url, "port": server.port() })
            )
        } else {
            format!(
                "Harbormap Learn is running for {}\n  Open: {url}\n  Ctrl-C stops it.\n",
                printable(&name)
            )
        };
        write_stdout(&text)?;
        if !args.no_open && !open_browser(&url) {
            let note = "Could not open a browser; open the link above.";
            if json {
                eprintln!("{note}");
            } else {
                write_stdout(&format!("{note}\n"))?;
            }
        }

        // The indexer has its own thread: it blocks on the watcher and on SQLite.
        let stop = Arc::new(AtomicBool::new(false));
        let live = {
            let (handle, stop) = (server.handle(), Arc::clone(&stop));
            std::thread::spawn(move || {
                if let Err(error) = live_loop(&mut indexer, &root, &name, &handle, json, &stop) {
                    eprintln!("harbormap: {error:#}");
                }
            })
        };
        let served = server
            .run(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await;
        // The thread notices at its next look around; the process does not wait for it.
        stop.store(true, Ordering::Relaxed);
        drop(live);
        served.context("the server stopped")
    })?;
    Ok(ExitCode::SUCCESS)
}

/// The branch and 7-character commit of the worktree at `root`; both `None` outside git.
fn repo(root: &Path, name: &str) -> Repo {
    let worktree =
        discover(root).and_then(|info| info.worktrees.into_iter().find(|w| w.path == root));
    Repo {
        name: name.to_string(),
        branch: worktree.as_ref().and_then(|w| w.branch.clone()),
        commit: worktree
            .and_then(|w| w.head)
            .map(|head| head.chars().take(7).collect()),
    }
}

/// Asks the system to open `url`. `false` when no opener could be started.
fn open_browser(url: &str) -> bool {
    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            // Reap it without holding anything up; an opener may stay for a while.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

/// The same loop as `harbormap index --watch`, but each non-empty change rebuilds the
/// map and goes to the server. Runs until `stop` is set, the reader of a JSON stream
/// goes away, or the watcher cannot be started.
fn live_loop(
    indexer: &mut Indexer,
    root: &Path,
    name: &str,
    handle: &Handle,
    json: bool,
    stop: &AtomicBool,
) -> anyhow::Result<()> {
    let mut watcher = start_watcher(indexer, root)?;
    // A save made while the first pass ran may have come before the watch began, so
    // the watch begins with a rescan. Such a catch-up publishes only what it found.
    let mut catch_up = true;
    // A batch that failed is not applied again, so the next batch rescans.
    let mut failed = false;
    // Files the store has but the browser was not told about, because a rebuild failed.
    let mut unsent: (BTreeSet<String>, BTreeSet<String>) = Default::default();
    while !stop.load(Ordering::Relaxed) {
        let mut batch = if std::mem::take(&mut catch_up) {
            Batch {
                paths: BTreeSet::new(),
                rescan: true,
                first_event: Instant::now(),
            }
        } else {
            let Some(batch) = watcher.recv_timeout(POLL) else {
                continue;
            };
            batch
        };
        batch.rescan |= std::mem::take(&mut failed);
        let rules = indexer.walk_rules().cloned();
        let applied = if batch.rescan {
            indexer.rescan()
        } else {
            indexer.apply(&batch.paths)
        };
        let delta = match applied {
            Ok(delta) => delta,
            Err(error) => {
                eprintln!("harbormap: {error}");
                failed = true;
                continue;
            }
        };
        if indexer.walk_rules() != rules.as_ref() {
            // An ignore file changed. The watcher still filters with the old rules
            // (and on Linux lacks watches for the folders the new ones admit), so a
            // new one replaces it, and a rescan finds what changed in between.
            watcher = start_watcher(indexer, root)?;
            catch_up = true;
        }
        unsent.0.extend(delta.files_changed.iter().cloned());
        unsent.1.extend(delta.files_removed.iter().cloned());
        if delta == Delta::default() && unsent.0.is_empty() && unsent.1.is_empty() {
            // A catch-up rescan that found nothing, or a save that changed nothing.
            continue;
        }

        // Handle::publish takes the new model, so the old one is read back from the
        // handle first (an `Arc`, nothing is copied).
        let previous = handle.model();
        let model = match Model::load(indexer.store(), name, Some(&previous)) {
            Ok(model) => model,
            Err(error) => {
                // The store is current but the map is not: the next change publishes
                // the files named here along with its own.
                eprintln!("harbormap: {error}");
                continue;
            }
        };
        let changed = model.map.changed_since(&previous.map);
        let (files_changed, files_removed) = std::mem::take(&mut unsent);
        let took = batch.first_event.elapsed();
        let took_ms = took.as_millis() as u64;
        let delta = FactsDelta {
            took_ms,
            // When the first event of the batch arrived.
            at_ms: now_ms().saturating_sub(took_ms),
            files_changed: files_changed.into_iter().collect(),
            files_removed: files_removed.into_iter().collect(),
            regrouped: changed.regrouped,
            groups_changed: changed.groups,
            ..FactsDelta::default()
        };
        let files = (delta.files_changed.len() + delta.files_removed.len()) as u32;
        let seq = handle.publish(model, delta);
        if json {
            let line = serde_json::json!({ "seq": seq, "files": files, "took_ms": took_ms });
            if !write_line(&line.to_string())? {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// A watcher that filters with the indexer's current rules and watches the folders
/// its last walk admitted.
fn start_watcher(indexer: &Indexer, root: &Path) -> anyhow::Result<Watcher> {
    let rules = indexer
        .walk_rules()
        .cloned()
        .context("the project was not read yet")?;
    Watcher::start(root, rules, indexer.admitted_dirs())
        .context("cannot watch the project for changes")
}

/// Now, in Unix milliseconds.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Prints one line and flushes. `false` when the reader has gone away.
fn write_line(text: &str) -> anyhow::Result<bool> {
    let mut stdout = std::io::stdout().lock();
    match writeln!(stdout, "{text}").and_then(|()| stdout.flush()) {
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        other => other.map(|()| true).map_err(Into::into),
    }
}
