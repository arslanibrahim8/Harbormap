mod common;

use common::{dump, fresh_index_of};
use hm_index::index::Indexer;
use hm_index::synth::{self, Edit};
use hm_store::Store;
use proptest::collection::vec;
use proptest::prelude::*;

fn edit_strategy() -> impl Strategy<Value = (Edit, u64)> {
    let edit = prop_oneof![
        Just(Edit::BodyChange),
        Just(Edit::SignatureChange),
        Just(Edit::Rename),
        Just(Edit::AddImport),
        Just(Edit::RemoveImport),
        Just(Edit::AddFile),
        Just(Edit::DeleteFile),
        Just(Edit::BreakSyntax),
    ];
    (edit, any::<u64>())
}

/// Indexes the synthetic project of `seed` with `files` modules, then applies `edits`
/// one by one and compares the store with a cold index of the same tree after each.
fn check(seed: u64, files: u32, edits: &[(Edit, u64)]) -> Result<(), TestCaseError> {
    let dir = tempfile::tempdir().unwrap();
    synth::project(dir.path(), seed, files);
    let mut ix = Indexer::open(dir.path(), Store::open_in_memory().unwrap()).unwrap();
    ix.full().unwrap();
    for (i, (edit, seed)) in edits.iter().enumerate() {
        let changed = synth::apply_edit(dir.path(), *seed, *edit);
        ix.apply(&changed).unwrap();
        prop_assert_eq!(
            dump(ix.store()),
            dump(fresh_index_of(dir.path()).store()),
            "after edit {} ({:?}, seed {}) of {:?}",
            i,
            edit,
            seed,
            changed
        );
    }
    drop(ix);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]
    #[test]
    fn incremental_equals_cold(edits in vec(edit_strategy(), 1..12)) {
        check(7, 8, &edits)?;
    }
}

proptest! {
    // 60 modules hold six barrels that often re-export each other, so a rename can
    // travel several re-export hops. Each case draws its own project; there are fewer
    // cases, because every step indexes the tree cold.
    #![proptest_config(ProptestConfig { cases: 16, ..ProptestConfig::default() })]
    #[test]
    fn incremental_equals_cold_through_barrel_chains(
        project in any::<u64>(),
        edits in vec(edit_strategy(), 1..8),
    ) {
        check(project, 60, &edits)?;
    }
}
