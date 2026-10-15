//! Where the snapshot comes from: this computer, or — in debug builds only — a
//! recorded scenario named by `HARBORMAP_FIXTURE`, which keeps CLI tests independent
//! of the machine they run on.

use std::path::PathBuf;

use anyhow::Context;

use hm_core::model::Snapshot;
use hm_probe::ProbeOptions;
use hm_project::git::RealWorld;
use hm_project::{ResolveInput, World, build_snapshot};

pub struct Input {
    pub snapshot: Snapshot,
    pub home: Option<PathBuf>,
}

pub fn load() -> anyhow::Result<Input> {
    let packs = hm_doctor::packs::load_embedded();

    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("HARBORMAP_FIXTURE") {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", PathBuf::from(&path).display()))?;
        let scenario: hm_project::Scenario =
            serde_json::from_str(&text).context("HARBORMAP_FIXTURE is not a scenario file")?;
        let snapshot = build_snapshot(ResolveInput {
            inventory: &scenario.inventory,
            world: &scenario.world,
            signatures: &packs.signatures,
            launch_dir: &scenario.launch_dir,
        });
        return Ok(Input {
            snapshot,
            home: scenario.world.home.clone(),
        });
    }

    let inventory = hm_probe::probe(&ProbeOptions::default());
    let world = RealWorld::new();
    let launch_dir =
        world.canonicalize(&std::env::current_dir().context("cannot read the current folder")?);
    let snapshot = build_snapshot(ResolveInput {
        inventory: &inventory,
        world: &world,
        signatures: &packs.signatures,
        launch_dir: &launch_dir,
    });
    Ok(Input {
        snapshot,
        home: world.home(),
    })
}
