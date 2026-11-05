//! Properties of the map's views over synthetic shop projects.

use std::collections::BTreeMap;

use hm_layout::{
    BoxView, GroupView, HUB_IN_DEGREE, InLink, Input, MAX_ITEMS, Map, NodeKind, OverviewView, synth,
};
use proptest::prelude::*;

/// The overview and the view of every group, in tree order.
fn views(m: &Map) -> (OverviewView, Vec<GroupView>) {
    let t = m.tree();
    let groups = t
        .nodes
        .iter()
        .enumerate()
        .filter(|&(i, n)| i != t.root && n.kind != NodeKind::File)
        .map(|(_, n)| m.group(&n.id).expect("every group has a view"))
        .collect();
    (m.overview(), groups)
}

fn json(m: &Map) -> Vec<String> {
    let (o, groups) = views(m);
    let mut out = vec![serde_json::to_string(&o).unwrap()];
    out.extend(groups.iter().map(|g| serde_json::to_string(g).unwrap()));
    out
}

/// Every box's rectangle by view and id; group sizes under the key `""`.
fn positions(m: &Map) -> BTreeMap<(String, String), (u32, u32, u32, u32)> {
    let (o, groups) = views(m);
    let mut out = BTreeMap::new();
    let mut put = |view: &str, b: &BoxView| {
        out.insert((view.to_string(), b.id.clone()), (b.x, b.y, b.w, b.h));
    };
    for b in &o.boxes {
        put("overview", b);
    }
    for g in &groups {
        for b in &g.children {
            put(&g.id, b);
        }
    }
    for g in &groups {
        out.insert((g.id.clone(), String::new()), (0, 0, g.w, g.h));
    }
    out
}

fn disjoint(a: &BoxView, b: &BoxView) -> bool {
    a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y
}

fn no_overlap(view: &str, boxes: &[BoxView]) -> Result<(), TestCaseError> {
    for (i, a) in boxes.iter().enumerate() {
        for b in &boxes[i + 1..] {
            prop_assert!(disjoint(a, b), "{view}: {} overlaps {}", a.name, b.name);
        }
    }
    Ok(())
}

/// A link between two files that are not linked yet and that leaves the tree as it
/// is (the target stays below the hub threshold), searched from `(a, b)`.
fn new_link(input: &Input, a: usize, b: usize) -> Option<InLink> {
    let n = input.files.len();
    let mut in_degree = vec![0usize; n];
    for l in &input.links {
        in_degree[l.dst as usize] += 1;
    }
    for k in 0..n * n {
        let (src, dst) = ((a + k / n) % n, (b + k) % n);
        let taken = input
            .links
            .iter()
            .any(|l| l.src as usize == src && l.dst as usize == dst);
        if src != dst && !taken && in_degree[dst] < HUB_IN_DEGREE {
            return Some(InLink {
                src: src as u32,
                dst: dst as u32,
                refs: 1,
            });
        }
    }
    None
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    #[test]
    fn cold_builds_are_byte_identical(seed in 0u64..64, n in 1u32..600) {
        let input = synth::input(seed, n);
        prop_assert_eq!(json(&Map::build(&input, "shop", None)), json(&Map::build(&input, "shop", None)));
    }

    #[test]
    fn boxes_never_overlap_and_stay_inside_their_group(seed in 0u64..64, n in 1u32..600) {
        let m = Map::build(&synth::input(seed, n), "shop", None);
        let (o, groups) = views(&m);
        no_overlap("overview", &o.boxes)?;
        for g in &groups {
            no_overlap(&g.name, &g.children)?;
            for c in &g.children {
                prop_assert!(c.x + c.w <= g.w && c.y + c.h <= g.h, "{} leaves {}", c.name, g.name);
            }
        }
    }

    #[test]
    fn items_per_view_never_exceed_150(seed in 0u64..64, n in 1u32..600) {
        let m = Map::build(&synth::input(seed, n), "shop", None);
        let (o, groups) = views(&m);
        prop_assert!(o.boxes.len() + o.arrows.len() <= MAX_ITEMS);
        for g in &groups {
            let items = g.children.len() + g.arrows.len() + g.elsewhere.len();
            prop_assert!(items <= MAX_ITEMS, "{} has {items} items", g.name);
        }
    }

    #[test]
    fn adding_a_link_moves_no_box(seed in 0u64..64, n in 2u32..600, a in 0usize..600, b in 0usize..600) {
        let input = synth::input(seed, n);
        let old = Map::build(&input, "shop", None);
        let link = new_link(&input, a % n as usize, b % n as usize);
        prop_assume!(link.is_some());
        let mut more = input.clone();
        more.links.push(link.unwrap());
        more.links.sort();
        let new = Map::build(&more, "shop", Some(&old));
        prop_assert_eq!(positions(&new), positions(&old));
    }

    #[test]
    fn reusing_the_previous_map_equals_a_cold_build_for_the_same_input(seed in 0u64..64, n in 1u32..600) {
        let input = synth::input(seed, n);
        let cold = Map::build(&input, "shop", None);
        let warm = Map::build(&input, "shop", Some(&cold));
        prop_assert!(warm == cold, "a warm build differs from the cold one");
        prop_assert_eq!(json(&warm), json(&cold));
    }
}
