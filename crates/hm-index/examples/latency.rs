//! The save-latency harness (spec §5.7, §11.3): indexes a synthetic project, then
//! times saves from the file write to the delta that reports the file.
//!
//! ```text
//! cargo run --release -p hm-index --example latency -- --files 1000 --saves 200 \
//!     --seed 7 --max-p95-body-ms 300 --max-p95-sig-ms 500 --max-cold-ms 2000 \
//!     --branch-switch 300
//! ```
//!
//! Prints one JSON object `{files, cold_ms, body:{p50,p95}, sig:{p50,p95},
//! branch_switch_ms}` and exits 2 when a maximum is exceeded. Times are in
//! milliseconds; `branch_switch_ms` is `null` without `--branch-switch`.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use hm_index::index::{Delta, Indexer};
use hm_index::synth::{self, Edit};
use hm_index::walk::walk;
use hm_index::watch::Watcher;
use hm_store::Store;

/// How long to wait for a delta before the run is given up.
const DELTA_TIMEOUT: Duration = Duration::from_secs(20);

/// Every flag the harness takes; anything else is a typo that would silently skip a
/// measurement, so it is refused.
const FLAGS: [&str; 7] = [
    "files",
    "saves",
    "seed",
    "max-p95-body-ms",
    "max-p95-sig-ms",
    "max-cold-ms",
    "branch-switch",
];

const USAGE: &str = "usage: latency --files N --saves S [--seed K] --max-p95-body-ms X \
--max-p95-sig-ms Y --max-cold-ms Z [--branch-switch N]";

struct Args {
    files: u32,
    saves: u32,
    seed: u64,
    max_p95_body_ms: f64,
    max_p95_sig_ms: f64,
    max_cold_ms: f64,
    branch_switch: Option<u32>,
}

fn parse_args() -> Result<Args, String> {
    let mut values = std::collections::BTreeMap::<String, String>::new();
    let mut argv = std::env::args().skip(1);
    while let Some(flag) = argv.next() {
        let name = flag
            .strip_prefix("--")
            .filter(|name| FLAGS.contains(name))
            .ok_or_else(|| format!("unknown argument {flag}\n{USAGE}"))?;
        let value = argv.next().ok_or_else(|| format!("{flag} needs a value"))?;
        values.insert(name.to_string(), value);
    }
    fn get<T: std::str::FromStr>(
        values: &std::collections::BTreeMap<String, String>,
        name: &str,
    ) -> Result<Option<T>, String> {
        values
            .get(name)
            .map(|v| v.parse().map_err(|_| format!("--{name}: bad value {v}")))
            .transpose()
    }
    let need = |name: &str| -> Result<f64, String> {
        get(&values, name)?.ok_or_else(|| format!("--{name} is required"))
    };
    Ok(Args {
        files: get(&values, "files")?.ok_or("--files is required")?,
        saves: get(&values, "saves")?.ok_or("--saves is required")?,
        seed: get(&values, "seed")?.unwrap_or(7),
        max_p95_body_ms: need("max-p95-body-ms")?,
        max_p95_sig_ms: need("max-p95-sig-ms")?,
        max_cold_ms: need("max-cold-ms")?,
        branch_switch: get(&values, "branch-switch")?,
    })
}

/// A splitmix64 generator, so a run reproduces from its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Nearest-rank percentile of sorted samples.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// The `.ts` modules of the project, from a walk of its tree.
fn modules(root: &Path) -> Vec<String> {
    let walked = walk(root).expect("walk the synthetic project");
    walked
        .files
        .into_iter()
        .map(|f| f.rel)
        .filter(|rel| rel.ends_with(".ts"))
        .collect()
}

/// Waits until every path of `wanted` has appeared in a delta's `files_changed`,
/// and returns the time of the delta that completed the set.
fn wait_for(deltas: &Receiver<(Instant, Delta)>, mut wanted: BTreeSet<String>) -> Instant {
    let give_up = Instant::now() + DELTA_TIMEOUT;
    loop {
        let left = give_up.saturating_duration_since(Instant::now());
        let (at, delta) = match deltas.recv_timeout(left) {
            Ok(received) => received,
            Err(RecvTimeoutError::Disconnected) => panic!("the apply thread stopped"),
            Err(RecvTimeoutError::Timeout) => {
                panic!(
                    "no delta for {} files within {DELTA_TIMEOUT:?}",
                    wanted.len()
                )
            }
        };
        for path in &delta.files_changed {
            wanted.remove(path);
        }
        if wanted.is_empty() {
            return at;
        }
    }
}

fn run(args: &Args) -> Result<serde_json::Value, String> {
    let project = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    // Canonical, so the paths the watcher reports match on macOS (`/var` -> `/private/var`).
    let root = std::fs::canonicalize(project.path()).map_err(|e| e.to_string())?;
    synth::project(&root, args.seed, args.files);
    let files = modules(&root);
    if files.is_empty() {
        return Err("the synthetic project has no modules".into());
    }

    let store = Store::open(&store_dir.path().join("facts.db")).map_err(|e| e.to_string())?;
    let mut indexer = Indexer::open(&root, store).map_err(|e| e.to_string())?;
    let started = Instant::now();
    indexer.full().map_err(|e| e.to_string())?;
    let cold = started.elapsed();

    // The watcher and the loop that applies its batches, like `harbormap index --watch`.
    let walked = walk(&root).map_err(|e| e.to_string())?;
    let watcher = Watcher::start(&root, walked.rules, &walked.dirs).map_err(|e| e.to_string())?;
    let (send, deltas) = mpsc::channel::<(Instant, Delta)>();
    let stop = Arc::new(AtomicBool::new(false));
    let looping = {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let Some(batch) = watcher.recv_timeout(Duration::from_millis(50)) else {
                    continue;
                };
                let delta = if batch.rescan {
                    indexer.rescan()
                } else {
                    indexer.apply(&batch.paths)
                }
                .expect("apply a batch");
                if send.send((Instant::now(), delta)).is_err() {
                    return;
                }
            }
        })
    };

    let mut rng = Rng(args.seed ^ 0x006C_6174_656E_6379);
    let mut fresh_seed = args.seed << 20;
    let mut next_seed = || {
        fresh_seed += 1;
        fresh_seed
    };
    let (mut body, mut sig) = (Vec::new(), Vec::new());
    let mut last: Option<usize> = None;
    for i in 0..args.saves {
        let (kind, samples) = if i % 2 == 0 {
            (Edit::BodyChange, &mut body)
        } else {
            (Edit::SignatureChange, &mut sig)
        };
        // A file with no function to edit is left alone; try another. Never the same
        // file twice in a row, so a late second event for the previous write cannot
        // satisfy this save's wait.
        let (t_write, changed) = loop {
            let mut pick = rng.below(files.len());
            while files.len() > 1 && Some(pick) == last {
                pick = rng.below(files.len());
            }
            last = Some(pick);
            let rel = &files[pick];
            while deltas.try_recv().is_ok() {}
            let t_write = Instant::now();
            let changed = synth::edit_function(&root, rel, next_seed(), kind);
            if !changed.is_empty() {
                break (t_write, changed);
            }
        };
        let t_delta = wait_for(&deltas, changed);
        samples.push(ms(t_delta.saturating_duration_since(t_write)));
    }

    let branch_switch_ms = args.branch_switch.map(|n| {
        let n = n as usize;
        let mut pool = files.clone();
        let mut rewritten = BTreeSet::new();
        while deltas.try_recv().is_ok() {}
        let t_write = Instant::now();
        // A body edit in each of `n` distinct modules; barrels have no function and
        // are passed over.
        while rewritten.len() < n && !pool.is_empty() {
            let rel = pool.swap_remove(rng.below(pool.len()));
            rewritten.extend(synth::edit_function(
                &root,
                &rel,
                next_seed(),
                Edit::BodyChange,
            ));
        }
        let t_delta = wait_for(&deltas, rewritten);
        round1(ms(t_delta.saturating_duration_since(t_write)))
    });

    stop.store(true, Ordering::Relaxed);
    looping.join().map_err(|_| "the apply loop panicked")?;

    body.sort_by(f64::total_cmp);
    sig.sort_by(f64::total_cmp);
    Ok(serde_json::json!({
        "files": args.files,
        "cold_ms": round1(ms(cold)),
        "body": { "p50": round1(percentile(&body, 0.5)), "p95": round1(percentile(&body, 0.95)) },
        "sig": { "p50": round1(percentile(&sig, 0.5)), "p95": round1(percentile(&sig, 0.95)) },
        "branch_switch_ms": branch_switch_ms,
    }))
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("latency: {error}");
            return ExitCode::from(64);
        }
    };
    let result = match run(&args) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("latency: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("{result}");

    let value = |a: &str, b: Option<&str>| {
        let v = &result[a];
        b.map_or(v, |b| &v[b]).as_f64().unwrap_or(f64::INFINITY)
    };
    let checks = [
        ("body p95", value("body", Some("p95")), args.max_p95_body_ms),
        ("sig p95", value("sig", Some("p95")), args.max_p95_sig_ms),
        ("cold", value("cold_ms", None), args.max_cold_ms),
    ];
    let mut missed = false;
    for (name, got, max) in checks {
        if got > max {
            eprintln!("latency: {name} {got:.1} ms is above the maximum {max:.1} ms");
            missed = true;
        }
    }
    if missed {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}
