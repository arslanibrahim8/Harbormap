use hm_tech::{Catalog, PACK_FILES};

/// Each file is checked alone, so a data writer can run this on their own file. References
/// to a tech from another file and techs only matched from another file are expected.
#[test]
fn each_pack_file_is_valid_on_its_own() {
    for entry in PACK_FILES {
        let catalog = Catalog::load(&[*entry]);
        let problems: Vec<_> = catalog
            .warnings()
            .iter()
            .filter(|warning| {
                !warning.contains("unknown tech id")
                    && !warning.contains("has nothing that matches it")
            })
            .collect();
        assert!(problems.is_empty(), "{}: {problems:#?}", entry.0);
    }
}

#[test]
fn there_are_twelve_pack_files() {
    assert_eq!(PACK_FILES.len(), 12);
}
