//! Properties of the group tree over synthetic shop projects and large flat folders.

use std::collections::BTreeSet;

use hm_layout::tree::{self, Tree};
use hm_layout::{GROUP_BUDGET, InFile, Input, NodeKind, OVERVIEW_BUDGET, synth};
use proptest::prelude::*;

/// Every file sits in exactly one leaf, every leaf is a file and the links of the
/// arena agree with each other.
fn every_file_is_in_exactly_one_leaf(input: &Input, t: &Tree) -> Result<(), TestCaseError> {
    prop_assert_eq!(t.file_node.len(), input.files.len());
    let mut seen = vec![0u32; input.files.len()];
    for (i, n) in t.nodes.iter().enumerate() {
        for &c in &n.children {
            prop_assert_eq!(t.nodes[c].parent, Some(i));
        }
        if n.kind == NodeKind::File {
            prop_assert!(n.children.is_empty(), "file {} has children", n.key);
            let f = n.file.expect("a file node names its file") as usize;
            prop_assert_eq!(&n.key, &input.files[f].path);
            prop_assert_eq!(t.file_node[f], i);
            seen[f] += 1;
        } else {
            prop_assert!(n.file.is_none());
            prop_assert!(!n.children.is_empty(), "group {} is empty", n.key);
        }
    }
    prop_assert!(seen.iter().all(|&s| s == 1), "file counts {:?}", seen);
    prop_assert_eq!(t.nodes[t.root].parent, None);
    prop_assert_eq!(t.nodes[t.root].files as usize, input.files.len());
    Ok(())
}

fn the_overview_has_at_most_12_boxes(t: &Tree) -> Result<(), TestCaseError> {
    prop_assert!(t.top().len() <= OVERVIEW_BUDGET, "{} boxes", t.top().len());
    Ok(())
}

fn no_group_has_more_than_25_children(t: &Tree) -> Result<(), TestCaseError> {
    for n in &t.nodes {
        prop_assert!(
            n.children.len() <= GROUP_BUDGET,
            "{} has {} children",
            n.key,
            n.children.len()
        );
        let keys: Vec<&str> = n
            .children
            .iter()
            .map(|&c| t.nodes[c].key.as_str())
            .collect();
        prop_assert!(
            keys.windows(2).all(|w| w[0] < w[1]),
            "{} children unsorted",
            n.key
        );
    }
    Ok(())
}

fn names_are_never_empty_or_cluster(t: &Tree) -> Result<(), TestCaseError> {
    for n in &t.nodes {
        prop_assert!(!n.name.trim().is_empty(), "empty name at {:?}", n.key);
        prop_assert!(!n.name.starts_with("Cluster"), "{}", n.name);
    }
    Ok(())
}

fn ids_are_unique(t: &Tree) -> Result<(), TestCaseError> {
    let ids: BTreeSet<&str> = t.nodes.iter().map(|n| n.id.as_str()).collect();
    prop_assert_eq!(ids.len(), t.nodes.len());
    for (i, n) in t.nodes.iter().enumerate() {
        prop_assert_eq!(t.node(&n.id), Some(i));
    }
    Ok(())
}

fn check(input: &Input) -> Result<(), TestCaseError> {
    let t = tree::build(input, "shop");
    every_file_is_in_exactly_one_leaf(input, &t)?;
    the_overview_has_at_most_12_boxes(&t)?;
    no_group_has_more_than_25_children(&t)?;
    names_are_never_empty_or_cluster(&t)?;
    ids_are_unique(&t)?;
    // building_twice_is_identical: `Tree: PartialEq` compares every field, hashes
    // included; the byte-level JSON check runs once in the fixed cases below.
    prop_assert!(t == tree::build(input, "shop"), "two builds differ");
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn synthetic_projects_keep_every_tree_property(seed in 0u64..64, n in 1u32..800) {
        check(&synth::input(seed, n))?;
    }
}

#[test]
fn synthetic_inputs_are_deterministic_and_well_formed() {
    for (seed, n) in [(0, 1), (1, 2), (7, 50), (42, 799)] {
        let a = synth::input(seed, n);
        assert_eq!(a, synth::input(seed, n));
        assert_eq!(a.files.len(), n as usize);
        assert!(a.files.windows(2).all(|w| w[0].path < w[1].path));
        assert!(
            a.links
                .windows(2)
                .all(|w| (w[0].src, w[0].dst) < (w[1].src, w[1].dst))
        );
        assert!(
            a.links
                .iter()
                .all(|l| l.src != l.dst && l.dst < n && l.refs >= 1)
        );
    }
    let big = synth::input(3, 799);
    let per_file = big.links.len() as f64 / big.files.len() as f64;
    assert!((2.0..=4.5).contains(&per_file), "{per_file} links per file");
    let t = tree::build(&big, "shop");
    assert_eq!(
        serde_json::to_string(&t).unwrap(),
        serde_json::to_string(&tree::build(&big, "shop")).unwrap()
    );
    assert!(t.top().iter().any(|&n| t.nodes[n].kind == NodeKind::Shared));
    assert!(
        t.nodes.iter().any(|n| n.name.contains('/')),
        "no merged chain"
    );
    assert!(big.files.iter().any(|f| f.generated));
}

/// 3 000 files directly in `dir` (or at the root when `dir` is empty), half with
/// shared name words and half without.
fn flat(dir: &str) -> Input {
    const WORDS: [&str; 6] = ["order", "cart", "user", "invoice", "coupon", "review"];
    let mut paths: Vec<String> = (0..3000)
        .map(|i| {
            let name = if i % 2 == 0 {
                format!("{}-{i:04}.ts", WORDS[i % WORDS.len()])
            } else {
                format!("item{i:04}.ts")
            };
            if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            }
        })
        .collect();
    paths.sort();
    Input {
        files: paths
            .into_iter()
            .map(|path| InFile {
                path,
                loc: 20,
                generated: false,
            })
            .collect(),
        links: Vec::new(),
    }
}

#[test]
fn a_flat_folder_of_3000_files_stays_within_budget() {
    check(&flat("src/everything")).unwrap();
}

#[test]
fn a_flat_root_of_3000_files_stays_within_budget() {
    check(&flat("")).unwrap();
}
