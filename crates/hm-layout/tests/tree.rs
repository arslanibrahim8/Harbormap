//! The folder-first group tree on small hand-written projects.

use hm_layout::tree::{self, Tree};
use hm_layout::{InFile, InLink, Input, NodeKind, Role, role_for, role_word};

/// An input of `paths` (any order) and `links` between them, one reference each.
fn input(paths: &[&str], links: &[(&str, &str)]) -> Input {
    let mut paths: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
    paths.sort();
    paths.dedup();
    let index = |p: &str| paths.iter().position(|q| q == p).unwrap() as u32;
    let mut links: Vec<InLink> = links
        .iter()
        .map(|(s, d)| InLink {
            src: index(s),
            dst: index(d),
            refs: 1,
        })
        .collect();
    links.sort();
    Input {
        files: paths
            .iter()
            .map(|p| InFile {
                path: p.clone(),
                loc: 10,
                generated: false,
            })
            .collect(),
        links,
    }
}

fn names(tree: &Tree, nodes: &[usize]) -> Vec<String> {
    nodes.iter().map(|&n| tree.nodes[n].name.clone()).collect()
}

fn by_name(tree: &Tree, name: &str) -> usize {
    tree.nodes
        .iter()
        .position(|n| n.name == name)
        .unwrap_or_else(|| panic!("no node named {name:?}"))
}

#[test]
fn everything_under_src_starts_the_overview_inside_src() {
    let t = tree::build(&input(&["src/app/page.tsx", "src/lib/db.ts"], &[]), "shop");
    assert_eq!(t.nodes[t.root].name, "shop");
    assert_eq!(t.nodes[t.root].key, "");
    assert_eq!(names(&t, t.top()), ["app", "lib"]);
    for &n in t.top() {
        assert_eq!(t.nodes[n].kind, NodeKind::Folder);
    }
    assert_eq!(t.nodes[t.top()[0]].key, "src/app");
}

#[test]
fn single_child_chains_merge_into_one_named_node() {
    let t = tree::build(
        &input(
            &[
                "src/features/orders/a.ts",
                "src/features/orders/b.ts",
                "src/app/page.tsx",
            ],
            &[],
        ),
        "shop",
    );
    assert_eq!(names(&t, t.top()), ["app", "features/orders"]);
    let orders = by_name(&t, "features/orders");
    assert_eq!(t.nodes[orders].key, "src/features/orders");
    assert_eq!(names(&t, &t.nodes[orders].children), ["a.ts", "b.ts"]);
    assert!(!t.nodes.iter().any(|n| n.name == "features"));
}

fn hub_project(importers: usize) -> Input {
    let mut paths = vec!["src/lib/db.ts".to_string(), "src/app/page.tsx".to_string()];
    paths.extend((0..importers).map(|i| format!("src/orders/o{i:02}.ts")));
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    let links: Vec<(&str, &str)> = refs[2..].iter().map(|p| (*p, "src/lib/db.ts")).collect();
    input(&refs, &links)
}

#[test]
fn a_file_imported_by_21_files_moves_to_shared() {
    let t = tree::build(&hub_project(20), "shop");
    assert!(!t.nodes.iter().any(|n| n.kind == NodeKind::Shared));
    assert_eq!(names(&t, t.top()), ["app", "lib", "orders"]);

    let t = tree::build(&hub_project(21), "shop");
    let shared = t
        .top()
        .iter()
        .copied()
        .find(|&n| t.nodes[n].kind == NodeKind::Shared)
        .expect("a Shared box on the overview");
    let s = &t.nodes[shared];
    assert_eq!(
        (s.key.as_str(), s.name.as_str(), s.role),
        ("~shared", "Shared", Role::Shared)
    );
    assert_eq!(names(&t, &s.children), ["db.ts"]);
    assert_eq!(t.nodes[s.children[0]].key, "src/lib/db.ts");
    // `lib` held only the hub, so it is gone.
    assert!(!t.nodes.iter().any(|n| n.key == "src/lib"));
}

#[test]
fn token_groups_need_three_files() {
    let mut paths = vec![
        "src/api/ordersController.ts".to_string(),
        "src/api/cartController.ts".to_string(),
        "src/api/userController.ts".to_string(),
    ];
    paths.extend((1..=30).map(|i| format!("src/api/file{i:02}.ts")));
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    let t = tree::build(&input(&refs, &[]), "shop");
    let group = by_name(&t, "Controller files");
    assert_eq!(t.nodes[group].kind, NodeKind::Token);
    assert_eq!(t.nodes[group].key, "src/api/*controller");
    assert_eq!(
        names(&t, &t.nodes[group].children),
        [
            "cartController.ts",
            "ordersController.ts",
            "userController.ts"
        ]
    );
    assert!(t.top().len() <= 12);

    // Two files sharing a word are not a group.
    let t = tree::build(&input(&refs[1..], &[]), "shop");
    assert!(!t.nodes.iter().any(|n| n.kind == NodeKind::Token));
}

#[test]
fn roles_come_from_path_words() {
    assert_eq!(role_for("src/components/Cart.tsx"), Role::See);
    assert_eq!(role_for("src/db/client.ts"), Role::Keep);
    assert_eq!(role_for("src/api/orders.ts"), Role::Work);
    assert_eq!(role_for("src/misc/x.ts"), Role::Work);
    // The deepest folder word wins; a file's own name counts only when no folder does.
    assert_eq!(role_for("src/app/models/order.ts"), Role::Keep);
    assert_eq!(role_for("src/misc/OrderRepository.ts"), Role::Keep);
    assert_eq!(role_for("src/misc/date-utils.ts"), Role::Shared);
    assert_eq!(role_for("src/integrations/payments"), Role::Outside);

    let t = tree::build(
        &input(
            &[
                "src/components/Cart.tsx",
                "src/db/client.ts",
                "src/api/orders.ts",
                "src/misc/x.ts",
            ],
            &[],
        ),
        "shop",
    );
    let role = |name: &str| t.nodes[by_name(&t, name)].role;
    assert_eq!(role("components"), Role::See);
    assert_eq!(role("Cart.tsx"), Role::See);
    assert_eq!(role("db"), Role::Keep);
    assert_eq!(role("client.ts"), Role::Keep);
    assert_eq!(role("api"), Role::Work);
    assert_eq!(role("misc"), Role::Work);
    assert_eq!(role("x.ts"), Role::Work);
    assert_eq!(role_word(role("api"), "api"), "Server endpoints");
    assert_eq!(role_word(Role::Work, "misc"), "Logic");
    assert_eq!(role_word(Role::See, "components"), "Screens");
    assert_eq!(role_word(Role::Keep, "db"), "Data");
    assert_eq!(role_word(Role::Outside, "payments"), "Outside services");
    assert_eq!(role_word(Role::Shared, "Shared"), "Shared helpers");
}

#[test]
fn roles_and_kinds_serialize_lowercase_and_the_map_holds_the_tree() {
    let roles = serde_json::to_string(&[Role::See, Role::Outside]).unwrap();
    assert_eq!(roles, r#"["see","outside"]"#);
    let kinds = serde_json::to_string(&[NodeKind::More, NodeKind::Shared]).unwrap();
    assert_eq!(kinds, r#"["more","shared"]"#);
    assert_eq!(NodeKind::Token.as_str(), "token");

    let inp = input(
        &["src/app/page.tsx", "src/lib/db.ts"],
        &[("src/app/page.tsx", "src/lib/db.ts")],
    );
    let map = hm_layout::Map::build(&inp, "shop", None);
    assert_eq!(map.tree(), &tree::build(&inp, "shop"));
    assert_eq!(map.links(), inp.clean_links().as_slice());
    assert_eq!(map, hm_layout::Map::build(&inp, "shop", Some(&map)));
}

#[test]
fn ids_are_stable_and_prefixed() {
    let inp = input(
        &["src/app/page.tsx", "src/lib/db.ts", "src/lib/money.ts"],
        &[("src/app/page.tsx", "src/lib/db.ts")],
    );
    let a = tree::build(&inp, "shop");
    let b = tree::build(&inp, "shop");
    assert_eq!(a, b);
    for n in &a.nodes {
        let prefix = if n.kind == NodeKind::File { "f_" } else { "g_" };
        assert!(n.id.starts_with(prefix) && n.id.len() == 14, "{}", n.id);
        assert_eq!(a.node(&n.id), b.node(&n.id));
    }
    let lib = by_name(&a, "lib");
    assert_eq!(a.node(&a.nodes[lib].id), Some(lib));
    assert_eq!(a.nodes[a.file_node[1]].key, "src/lib/db.ts");
    assert_eq!(
        a.nodes[a.file_node[1]].id,
        hm_core::ids::file_id("src/lib/db.ts")
    );
    assert_eq!(a.node("g_000000000000"), None);
}

#[test]
fn group_names_say_what_they_hold() {
    // 40 loose files with no shared words: one "files" group, then a "more" bucket.
    let paths: Vec<String> = (0..40).map(|i| format!("lib/f{i:02}.ts")).collect();
    let mut refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    refs.push("lib/sub/a.ts");
    let t = tree::build(&input(&refs, &[]), "shop");
    let files = by_name(&t, "other files");
    assert_eq!(t.nodes[files].kind, NodeKind::Files);
    assert_eq!(t.nodes[files].key, "lib/~files");
    assert_eq!(t.nodes[files].files, 40);
    // 40 files do not fit in 25: the 24 biggest items (all one file, so by name) stay
    // and the other 16 go into one "more" box, which fits and is not split further.
    let more = by_name(&t, "16 more");
    assert_eq!(t.nodes[more].kind, NodeKind::More);
    assert_eq!(t.nodes[more].key, "lib/~files/~more");
    assert_eq!(t.nodes[more].parent, Some(files));
    assert_eq!(t.nodes[files].children.len(), 25);
}

#[test]
fn a_new_link_keeps_shapes_but_changes_the_merkle_of_the_common_group() {
    let paths = [
        "src/orders/a.ts",
        "src/orders/b.ts",
        "src/cart/c.ts",
        "src/cart/d.ts",
    ];
    let before = tree::build(
        &input(&paths, &[("src/orders/a.ts", "src/orders/b.ts")]),
        "shop",
    );
    let after = tree::build(
        &input(
            &paths,
            &[
                ("src/orders/a.ts", "src/orders/b.ts"),
                ("src/orders/a.ts", "src/cart/c.ts"),
            ],
        ),
        "shop",
    );
    assert_eq!(before.nodes.len(), after.nodes.len());
    for (b, a) in before.nodes.iter().zip(&after.nodes) {
        assert_eq!(b.key, a.key);
        assert_eq!(b.shape_hash, a.shape_hash, "{}", b.key);
        // Only the root holds the new link between two of its children.
        assert_eq!(b.merkle == a.merkle, !b.key.is_empty(), "{}", b.key);
    }
}

#[test]
fn a_loc_change_changes_the_merkle_of_the_file_and_its_ancestors() {
    let paths = ["src/orders/a.ts", "src/orders/b.ts", "src/cart/c.ts"];
    let before = input(&paths, &[]);
    let mut after = before.clone();
    after.files[2].loc += 1; // src/orders/b.ts
    let (b, a) = (tree::build(&before, "shop"), tree::build(&after, "shop"));
    let changed: Vec<&str> = b
        .nodes
        .iter()
        .zip(&a.nodes)
        .filter(|(x, y)| x.merkle != y.merkle)
        .map(|(x, _)| x.key.as_str())
        .collect();
    assert_eq!(changed, ["", "src/orders", "src/orders/b.ts"]);
    assert_eq!(a.nodes[a.file_node[2]].loc, 11);
    assert_eq!(a.nodes[a.root].loc, 31);
}

#[test]
fn a_refs_only_change_alters_the_merkle_of_the_lowest_common_node() {
    let paths = ["src/orders/a.ts", "src/orders/b.ts", "src/cart/c.ts"];
    let before = input(
        &paths,
        &[
            ("src/orders/a.ts", "src/orders/b.ts"),
            ("src/orders/a.ts", "src/cart/c.ts"),
        ],
    );
    let mut after = before.clone();
    // src/orders/a.ts -> src/cart/c.ts: its lowest common node is the root.
    let link = after.links.iter_mut().find(|l| l.dst == 0).unwrap();
    link.refs += 4;
    let (b, a) = (tree::build(&before, "shop"), tree::build(&after, "shop"));
    for (x, y) in b.nodes.iter().zip(&a.nodes) {
        assert_eq!(x.key, y.key);
        assert_eq!(x.shape_hash, y.shape_hash, "{}", x.key);
        assert_eq!(x.merkle == y.merkle, !x.key.is_empty(), "{}", x.key);
    }
    // Inside src/orders this time.
    let mut inside = before.clone();
    inside.links.iter_mut().find(|l| l.dst == 2).unwrap().refs = 9;
    let i = tree::build(&inside, "shop");
    let changed: Vec<&str> = b
        .nodes
        .iter()
        .zip(&i.nodes)
        .filter(|(x, y)| x.merkle != y.merkle)
        .map(|(x, _)| x.key.as_str())
        .collect();
    assert_eq!(changed, ["", "src/orders"]);
}

#[test]
fn a_generated_flag_change_alters_the_file_merkle() {
    let before = input(&["src/db/a.ts", "src/db/b.ts", "src/app/c.ts"], &[]);
    let mut after = before.clone();
    after.files[1].generated = true; // src/db/a.ts
    let (b, a) = (tree::build(&before, "shop"), tree::build(&after, "shop"));
    let changed: Vec<&str> = b
        .nodes
        .iter()
        .zip(&a.nodes)
        .filter(|(x, y)| x.merkle != y.merkle)
        .map(|(x, _)| x.key.as_str())
        .collect();
    assert_eq!(changed, ["", "src/db", "src/db/a.ts"]);
}

#[test]
fn shared_is_never_bucketed_when_the_root_has_more_than_11_items() {
    let mut paths = vec!["src/lib/db.ts".to_string()];
    for f in 0..15 {
        for i in 0..2 {
            paths.push(format!("src/feature{f:02}/m{i}.ts"));
        }
    }
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    let links: Vec<(&str, &str)> = refs[1..22].iter().map(|p| (*p, "src/lib/db.ts")).collect();
    let t = tree::build(&input(&refs, &links), "shop");
    assert_eq!(t.top().len(), 12);
    let kinds: Vec<NodeKind> = t.top().iter().map(|&n| t.nodes[n].kind).collect();
    assert_eq!(kinds.iter().filter(|k| **k == NodeKind::Shared).count(), 1);
    assert_eq!(kinds.iter().filter(|k| **k == NodeKind::More).count(), 1);
    let more = t
        .top()
        .iter()
        .copied()
        .find(|&n| t.nodes[n].kind == NodeKind::More)
        .unwrap();
    assert_eq!(t.nodes[more].name, "5 more");
    assert!(
        !t.nodes[more]
            .children
            .iter()
            .any(|&c| t.nodes[c].kind == NodeKind::Shared)
    );
}

#[test]
fn invalid_links_are_ignored_and_duplicates_add_up() {
    let mut paths = vec!["src/lib/db.ts".to_string(), "src/app/page.tsx".to_string()];
    paths.extend((0..3).map(|i| format!("src/orders/o{i}.ts")));
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    let mut inp = input(&refs, &[("src/orders/o0.ts", "src/lib/db.ts")]);
    let db = inp
        .files
        .iter()
        .position(|f| f.path == "src/lib/db.ts")
        .unwrap() as u32;
    let o0 = inp
        .files
        .iter()
        .position(|f| f.path == "src/orders/o0.ts")
        .unwrap() as u32;
    // 25 copies of one pair: one importer, so no hub; refs add up.
    for _ in 0..24 {
        inp.links.push(InLink {
            src: o0,
            dst: db,
            refs: 2,
        });
    }
    inp.links.push(InLink {
        src: db,
        dst: db,
        refs: 1,
    }); // self
    inp.links.push(InLink {
        src: 99,
        dst: db,
        refs: 1,
    }); // out of range
    inp.links.push(InLink {
        src: db,
        dst: 1000,
        refs: 1,
    }); // out of range
    let t = tree::build(&inp, "shop");
    assert!(!t.nodes.iter().any(|n| n.kind == NodeKind::Shared));
    assert_eq!(
        inp.clean_links(),
        [InLink {
            src: o0,
            dst: db,
            refs: 49
        }]
    );
    let map = hm_layout::Map::build(&inp, "shop", None);
    assert_eq!(map.links(), inp.clean_links().as_slice());
}
