//! Arrows, chips, positions and views of the map on small hand-written projects.

use hm_layout::{
    BEGINNER_CHILDREN, BUILDER_ARROWS, InFile, InLink, Input, MAX_CHIPS, Map, OverviewView,
};

/// An input of `paths` (any order) and `links` `(src, dst, refs)` between them.
fn input(paths: &[&str], links: &[(&str, &str, u32)]) -> Input {
    let mut paths: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
    paths.sort();
    paths.dedup();
    let index = |p: &str| {
        paths
            .iter()
            .position(|q| q == p)
            .unwrap_or_else(|| panic!("no file {p}")) as u32
    };
    let mut links: Vec<InLink> = links
        .iter()
        .map(|&(s, d, refs)| InLink {
            src: index(s),
            dst: index(d),
            refs,
        })
        .collect();
    links.sort();
    Input {
        files: paths
            .iter()
            .map(|p| InFile {
                path: p.clone(),
                loc: 10,
                generated: p.contains("generated"),
            })
            .collect(),
        links,
    }
}

fn map(input: &Input) -> Map {
    Map::build(input, "shop", None)
}

/// The id of the box named `name` (anywhere in the tree).
fn id(m: &Map, name: &str) -> String {
    let t = m.tree();
    t.nodes
        .iter()
        .find(|n| n.name == name)
        .unwrap_or_else(|| panic!("no node named {name:?}"))
        .id
        .clone()
}

/// The overview box named `name`.
fn top_box<'a>(o: &'a OverviewView, name: &str) -> &'a hm_layout::BoxView {
    o.boxes
        .iter()
        .find(|b| b.name == name)
        .unwrap_or_else(|| panic!("no overview box {name:?}"))
}

fn arrows(m: &Map, arrows: &[hm_layout::ArrowView]) -> Vec<(String, String, u64)> {
    let name = |id: &str| {
        let t = m.tree();
        t.nodes[t.node(id).unwrap()].name.clone()
    };
    arrows
        .iter()
        .map(|a| (name(&a.from), name(&a.to), a.count))
        .collect()
}

#[test]
fn overview_arrows_count_refs_one_per_ordered_pair() {
    let m = map(&input(
        &[
            "src/orders/create.ts",
            "src/orders/list.ts",
            "src/db/client.ts",
            "src/db/schema.ts",
        ],
        &[
            ("src/orders/create.ts", "src/db/client.ts", 3),
            ("src/orders/list.ts", "src/db/schema.ts", 2),
            ("src/db/client.ts", "src/orders/create.ts", 1),
            // Inside one box: never an overview arrow.
            ("src/orders/create.ts", "src/orders/list.ts", 7),
        ],
    ));
    let o = m.overview();
    assert_eq!(
        arrows(&m, &o.arrows),
        [
            ("orders".into(), "db".into(), 5),
            ("db".into(), "orders".into(), 1)
        ]
    );
    assert_eq!(o.hidden_arrows, 0);
    assert!(o.boxes.iter().all(|b| b.beginner));
}

#[test]
fn arrows_beyond_40_are_counted_as_hidden() {
    // 12 top folders, one file each, every ordered pair linked: 132 arrows.
    let paths: Vec<String> = (0..12).map(|i| format!("src/f{i:02}/x{i:02}.ts")).collect();
    let refs: Vec<String> = paths.clone();
    let mut links = Vec::new();
    for (i, a) in refs.iter().enumerate() {
        for (j, b) in refs.iter().enumerate() {
            if i != j {
                // A few heavy arrows, the rest weigh 1.
                let r = if i == 3 && j < 3 { 10 + j as u32 } else { 1 };
                links.push((a.as_str(), b.as_str(), r));
            }
        }
    }
    let p: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    let m = map(&input(&p, &links));
    let o = m.overview();
    assert_eq!(o.boxes.len(), 12);
    assert_eq!(o.arrows.len(), BUILDER_ARROWS);
    assert_eq!(o.hidden_arrows, 132 - BUILDER_ARROWS);
    let a = arrows(&m, &o.arrows);
    assert_eq!(a[0], ("f03".into(), "f02".into(), 12));
    assert_eq!(a[1], ("f03".into(), "f01".into(), 11));
    assert_eq!(a[2], ("f03".into(), "f00".into(), 10));
    // Then count desc, `from`, `to` by id.
    for w in o.arrows.windows(2) {
        assert!(
            (w[1].count, &w[0].from, &w[0].to) <= (w[0].count, &w[1].from, &w[1].to),
            "{:?} before {:?}",
            w[0],
            w[1]
        );
    }
}

#[test]
fn links_leaving_a_group_become_chips() {
    let m = map(&input(
        &[
            "src/orders/a.ts",
            "src/orders/b.ts",
            "src/db/c.ts",
            "src/cart/d.ts",
            "src/users/e.ts",
        ],
        &[
            ("src/orders/a.ts", "src/db/c.ts", 2),
            ("src/orders/b.ts", "src/db/c.ts", 1),
            ("src/cart/d.ts", "src/orders/a.ts", 4),
            ("src/orders/a.ts", "src/orders/b.ts", 1),
            ("src/users/e.ts", "src/db/c.ts", 9),
        ],
    ));
    let g = m.group(&id(&m, "orders")).expect("orders is a group");
    assert_eq!(arrows(&m, &g.arrows), [("a.ts".into(), "b.ts".into(), 1)]);
    let chips: Vec<(&str, u64, u64)> = g
        .elsewhere
        .iter()
        .map(|c| (c.name.as_str(), c.out, c.into))
        .collect();
    assert_eq!(chips, [("cart", 0, 4), ("db", 3, 0)]);
    assert_eq!(g.elsewhere[0].id, id(&m, "cart"));
    assert_eq!(g.elsewhere_more, 0);

    // A group linked to 10 other boxes keeps the 8 busiest chips.
    let mut paths = vec!["src/hub/h.ts".to_string()];
    let mut links = Vec::new();
    for i in 0..10u32 {
        paths.push(format!("src/t{i}/x.ts"));
    }
    for i in 0..10u32 {
        links.push(("src/hub/h.ts".to_string(), format!("src/t{i}/x.ts"), i + 1));
    }
    let p: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    let l: Vec<(&str, &str, u32)> = links
        .iter()
        .map(|(a, b, r)| (a.as_str(), b.as_str(), *r))
        .collect();
    let m = map(&input(&p, &l));
    let g = m.group(&id(&m, "hub")).unwrap();
    assert_eq!(g.elsewhere.len(), MAX_CHIPS);
    assert_eq!(g.elsewhere_more, 2);
    assert_eq!(g.elsewhere[0].name, "t9");
    assert_eq!(g.elsewhere[7].name, "t2");
}

#[test]
fn file_links_lift_to_the_visible_boxes_of_the_view() {
    let route = "src/orders/api/route.ts";
    let m = map(&input(
        &[
            route,
            "src/orders/api/handler.ts",
            "src/orders/model/order.ts",
            "src/orders/index.ts",
            "src/db/client.ts",
            "src/cart/view.ts",
        ],
        &[
            (route, "src/orders/model/order.ts", 2),
            (route, "src/db/client.ts", 1),
            ("src/cart/view.ts", route, 3),
            ("src/orders/api/handler.ts", route, 1),
        ],
    ));
    let ends = |v: &[hm_layout::LinkEnd]| -> Vec<(String, u64)> {
        let t = m.tree();
        v.iter()
            .map(|e| (t.nodes[t.node(&e.id).unwrap()].name.clone(), e.count))
            .collect()
    };

    let o = m
        .file_links(route, "overview")
        .expect("route is on the map");
    assert_eq!(o.view, "overview");
    assert_eq!(ends(&o.outgoing), [("db".to_string(), 1)]);
    assert_eq!(ends(&o.incoming), [("cart".to_string(), 3)]);

    let orders = id(&m, "orders");
    let g = m.file_links(route, &orders).unwrap();
    assert_eq!(g.view, orders);
    assert_eq!(
        ends(&g.outgoing),
        [("model".to_string(), 2), ("db".to_string(), 1)]
    );
    assert_eq!(ends(&g.incoming), [("cart".to_string(), 3)]);

    assert!(m.file_links("src/nope.ts", "overview").is_none());
    assert!(m.file_links(route, "g_000000000000").is_none());
    assert!(m.file_links(route, &id(&m, "route.ts")).is_none());
}

#[test]
fn see_lane_is_left_of_keep_lane() {
    let m = map(&input(
        &[
            "src/app/page.tsx",
            "src/components/cart.tsx",
            "src/lib/price.ts",
            "src/db/client.ts",
            "src/integrations/stripe.ts",
            "src/utils/format.ts",
        ],
        &[("src/app/page.tsx", "src/lib/price.ts", 1)],
    ));
    let o = m.overview();
    let (app, comp, lib, db, ext, utils) = (
        top_box(&o, "app"),
        top_box(&o, "components"),
        top_box(&o, "lib"),
        top_box(&o, "db"),
        top_box(&o, "integrations"),
        top_box(&o, "utils"),
    );
    assert_eq!(app.x, comp.x, "both see boxes share the first lane");
    assert!(app.x < lib.x && lib.x < db.x && db.x < ext.x);
    assert_eq!((app.x, lib.y, db.y, ext.y), (0, 0, 0, 0));
    assert_eq!((app.w, app.h), (200, 72));
    // The shared row sits under the tallest lane (see: two boxes).
    assert!(utils.y >= comp.y.max(app.y) + 72);
    assert_eq!(utils.x, 0);
}

#[test]
fn beginner_children_are_the_12_most_linked() {
    let mut paths: Vec<String> = (0..20).map(|i| format!("src/orders/o{i:02}.ts")).collect();
    paths.push("src/cart/c.ts".into());
    let links: Vec<(String, String, u32)> = (0..20u32)
        .map(|i| {
            (
                "src/cart/c.ts".into(),
                format!("src/orders/o{i:02}.ts"),
                i + 1,
            )
        })
        .collect();
    let p: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    let l: Vec<(&str, &str, u32)> = links
        .iter()
        .map(|(a, b, r)| (a.as_str(), b.as_str(), *r))
        .collect();
    let m = map(&input(&p, &l));
    let g = m.group(&id(&m, "orders")).unwrap();
    assert_eq!(g.children.len(), 20);
    let beginners: Vec<&str> = g
        .children
        .iter()
        .filter(|c| c.beginner)
        .map(|c| c.name.as_str())
        .collect();
    let want: Vec<String> = (8..20).map(|i| format!("o{i:02}.ts")).collect();
    let mut got: Vec<&str> = beginners.clone();
    got.sort();
    assert_eq!(got.len(), BEGINNER_CHILDREN);
    assert_eq!(got, want);
    // Beginner children come first in their column.
    let col0: Vec<bool> = {
        let mut c: Vec<_> = g.children.iter().filter(|c| c.x == 24).collect();
        c.sort_by_key(|c| c.y);
        c.iter().map(|c| c.beginner).collect()
    };
    assert!(col0.windows(2).all(|w| w[0] >= w[1]), "{col0:?}");
    assert!(g.children.iter().all(|c| !c.expandable));
}

#[test]
fn changed_since_reports_the_group_of_an_edited_file() {
    let paths = [
        "src/orders/api/route.ts",
        "src/orders/api/handler.ts",
        "src/orders/model/order.ts",
        "src/db/client.ts",
        "src/cart/view.ts",
    ];
    let base = input(
        &paths,
        &[("src/cart/view.ts", "src/orders/api/route.ts", 1)],
    );
    let old = map(&base);

    // One file's loc: its ancestors, top box first, root left out.
    let mut edited = base.clone();
    edited.files[3].loc += 5; // src/orders/api/route.ts (sorted paths)
    assert_eq!(edited.files[3].path, "src/orders/api/route.ts");
    let new = Map::build(&edited, "shop", Some(&old));
    let c = new.changed_since(&old);
    assert!(!c.regrouped);
    let mut want = new.ancestors("src/orders/api/route.ts").unwrap();
    assert_eq!(want, [id(&new, "orders"), id(&new, "api")]);
    want.sort();
    assert_eq!(c.groups, want);
    assert!(new.changed_since(&new).groups.is_empty());

    // A new link: both ends' groups, even where no merkle moved (db's chips).
    let linked = input(
        &paths,
        &[
            ("src/cart/view.ts", "src/orders/api/route.ts", 1),
            ("src/orders/model/order.ts", "src/db/client.ts", 1),
        ],
    );
    let new = Map::build(&linked, "shop", Some(&old));
    let c = new.changed_since(&old);
    assert!(!c.regrouped);
    let mut want = vec![id(&new, "orders"), id(&new, "model"), id(&new, "db")];
    want.sort();
    assert_eq!(c.groups, want);
    // Removing it again reports the same groups.
    assert_eq!(old.changed_since(&new).groups, want);

    // A refs-only change on a link across top boxes: both ends' groups.
    let heavier = input(
        &paths,
        &[("src/cart/view.ts", "src/orders/api/route.ts", 4)],
    );
    let c = map(&heavier).changed_since(&old);
    assert!(!c.regrouped);
    let mut want = vec![id(&old, "cart"), id(&old, "orders"), id(&old, "api")];
    want.sort();
    assert_eq!(c.groups, want);

    // A file moving to an existing Shared box keeps every id but changes its parent:
    // a regroup.
    let mut hub_paths: Vec<String> = (0..21).map(|i| format!("src/cart/c{i:02}.ts")).collect();
    hub_paths.extend(["src/db/client.ts", "src/db/schema.ts", "src/lib/db.ts"].map(String::from));
    let hub_links: Vec<(String, String, u32)> = (0..21)
        .map(|i| (format!("src/cart/c{i:02}.ts"), "src/db/client.ts".into(), 1))
        .collect();
    let always: Vec<(String, String, u32)> = (0..21)
        .map(|i| (format!("src/cart/c{i:02}.ts"), "src/lib/db.ts".into(), 1))
        .collect();
    let build = |k: usize| {
        let p: Vec<&str> = hub_paths.iter().map(|s| s.as_str()).collect();
        let l: Vec<(&str, &str, u32)> = (hub_links[..k].iter().chain(&always))
            .map(|(a, b, r)| (a.as_str(), b.as_str(), *r))
            .collect();
        map(&input(&p, &l))
    };
    let (twenty, hub) = (build(20), build(21));
    let ids = |m: &Map| -> Vec<String> {
        let mut v: Vec<String> = m.tree().nodes.iter().map(|n| n.id.clone()).collect();
        v.sort();
        v
    };
    assert_eq!(ids(&twenty), ids(&hub), "the same nodes on both sides");
    assert!(hub.changed_since(&twenty).regrouped);

    // A new folder regroups.
    let mut more = paths.to_vec();
    more.push("src/search/find.ts");
    let new = Map::build(&input(&more, &[]), "shop", Some(&old));
    assert!(new.changed_since(&old).regrouped);
}

#[test]
fn group_views_carry_breadcrumb_geometry_and_file_fields() {
    let m = map(&input(
        &[
            "src/orders/api/route.ts",
            "src/orders/api/handler.ts",
            "src/orders/generated/types.ts",
            "src/db/client.ts",
        ],
        &[("src/orders/api/route.ts", "src/orders/api/handler.ts", 1)],
    ));
    let api = m.group(&id(&m, "api")).unwrap();
    let crumbs: Vec<&str> = api.breadcrumb.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(crumbs, ["orders", "api"]);
    assert_eq!(api.role_word, "Server endpoints");
    // route → handler: two columns, one row.
    assert_eq!((api.w, api.h), (24 * 2 + 176 * 2 + 96, 44 + 52 + 24));
    let route = api.children.iter().find(|c| c.name == "route.ts").unwrap();
    let handler = api
        .children
        .iter()
        .find(|c| c.name == "handler.ts")
        .unwrap();
    assert_eq!((route.x, route.y, route.w, route.h), (24, 44, 176, 52));
    assert_eq!((handler.x, handler.y), (24 + 176 + 96, 44));

    let json = serde_json::to_value(route).unwrap();
    assert_eq!(json["path"], "src/orders/api/route.ts");
    assert_eq!(json["generated"], false);
    assert_eq!(json["kind"], "file");
    assert_eq!(json["expandable"], false);
    let gen_dir = m.group(&id(&m, "generated")).unwrap();
    assert_eq!(
        serde_json::to_value(&gen_dir.children[0]).unwrap()["generated"],
        true
    );
    let orders = m.group(&id(&m, "orders")).unwrap();
    let api_box = serde_json::to_value(&orders.children[0]).unwrap();
    assert!(api_box.get("path").is_none() && api_box.get("generated").is_none());
    assert_eq!(api_box["expandable"], true);
    let view_json = serde_json::to_value(m.group(&id(&m, "orders")).unwrap()).unwrap();
    for key in [
        "id",
        "name",
        "role",
        "role_word",
        "w",
        "h",
        "breadcrumb",
        "children",
        "arrows",
        "hidden_arrows",
        "elsewhere",
        "elsewhere_more",
    ] {
        assert!(view_json.get(key).is_some(), "group view lacks {key}");
    }

    assert_eq!(
        m.ancestors("src/orders/api/route.ts").unwrap(),
        [id(&m, "orders"), id(&m, "api")]
    );
    assert!(m.ancestors("src/nope.ts").is_none());
    assert!(m.group(&m.tree().nodes[m.tree().root].id).is_none());
    assert!(m.group(&id(&m, "route.ts")).is_none());
    assert!(m.group("g_000000000000").is_none());
}

#[test]
fn chips_serialize_in_and_out() {
    let m = map(&input(
        &["src/orders/a.ts", "src/db/c.ts"],
        &[("src/db/c.ts", "src/orders/a.ts", 2)],
    ));
    let g = m.group(&id(&m, "orders")).unwrap();
    let chip = serde_json::to_value(&g.elsewhere[0]).unwrap();
    assert_eq!(chip["in"], 2);
    assert_eq!(chip["out"], 0);
    assert_eq!(chip["role"], "keep");
}
