//! `/api/overview`, `/api/group/{id}` and the links and ancestors of a file card.

mod common;

use common::{get_json, shop, start};
use serde_json::Value;

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create a temporary directory");
    shop(dir.path());
    dir
}

/// The ids of a list of boxes, arrows' ends or link ends.
fn ids(list: &Value, key: &str) -> Vec<String> {
    list.as_array()
        .expect("a list")
        .iter()
        .map(|v| v[key].as_str().expect("an id").to_string())
        .collect()
}

/// The ids of the top boxes and of every group below them, in walk order.
fn all_groups(live: &common::Live) -> Vec<String> {
    let (_, overview) = get_json(live, "/api/overview");
    let mut todo: Vec<Value> = overview["boxes"].as_array().expect("boxes").clone();
    let mut groups = Vec::new();
    while let Some(b) = todo.pop() {
        if b["expandable"] != true {
            continue;
        }
        let id = b["id"].as_str().expect("an id").to_string();
        let (status, view) = get_json(live, &format!("/api/group/{id}"));
        assert_eq!(status, 200, "{id}");
        todo.extend(view["children"].as_array().expect("children").clone());
        groups.push(id);
    }
    groups
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_overview_has_boxes_arrows_and_excluded_counts() {
    let dir = project();
    let live = start(dir.path()).await;
    let (status, body) = get_json(&live, "/api/overview");
    assert_eq!(status, 200);
    assert_eq!(body["v"], 1);
    assert_eq!(body["seq"], 0);
    assert_eq!(body["repo"]["name"], "shop");
    assert_eq!(body["repo"]["branch"], "main");
    assert_eq!(body["repo"]["commit"], "abc1234");
    assert!(body["updated_ms"].as_u64().expect("a time") > 1_600_000_000_000);

    let boxes = body["boxes"].as_array().expect("boxes");
    assert!(!boxes.is_empty() && boxes.len() <= 12);
    for b in boxes {
        for key in ["id", "name", "kind", "role", "role_word", "files", "loc"] {
            assert!(!b[key].is_null(), "{key}");
        }
        assert!(b["id"].as_str().expect("an id").starts_with("g_"));
        for key in ["x", "y", "w", "h"] {
            assert!(b[key].is_u64(), "{key}");
        }
    }
    // The shop has four source files and, left off the map, a test and a README.
    let files: u64 = boxes.iter().map(|b| b["files"].as_u64().unwrap()).sum();
    assert_eq!(files, 4);
    assert_eq!(body["files_total"], 4);
    assert_eq!(
        body["excluded"],
        serde_json::json!([{ "class": "doc", "files": 1 }, { "class": "test", "files": 1 }])
    );
    // Arrows join boxes of the overview.
    let box_ids = ids(&body["boxes"], "id");
    for a in body["arrows"].as_array().expect("arrows") {
        assert!(box_ids.contains(&a["from"].as_str().unwrap().to_string()));
        assert!(box_ids.contains(&a["to"].as_str().unwrap().to_string()));
        assert!(a["count"].as_u64().unwrap() >= 1);
    }
    assert!(body["hidden_arrows"].is_u64());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_group_is_404() {
    let dir = project();
    let live = start(dir.path()).await;
    let (status, body) = get_json(&live, "/api/group/g_000000000000");
    assert_eq!(status, 404);
    assert_eq!(body["v"], 1);
    assert_eq!(get_json(&live, "/api/group/nope").0, 404);
    // A file node is not a group.
    let file = hm_core::ids::file_id("src/db/client.ts");
    assert_eq!(get_json(&live, &format!("/api/group/{file}")).0, 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_group_of_a_top_box_has_children_and_breadcrumb() {
    let dir = project();
    let live = start(dir.path()).await;
    let groups = all_groups(&live);
    assert!(!groups.is_empty(), "the shop has a box that opens");
    let (_, overview) = get_json(&live, "/api/overview");
    let top = ids(&overview["boxes"], "id");
    let id = groups
        .iter()
        .find(|g| top.contains(g))
        .expect("a top box opens");
    let (status, view) = get_json(&live, &format!("/api/group/{id}"));
    assert_eq!(status, 200);
    assert_eq!(view["v"], 1);
    assert_eq!(view["seq"], 0);
    assert_eq!(view["id"], *id);
    assert!(view["w"].as_u64().unwrap() > 0 && view["h"].as_u64().unwrap() > 0);
    let children = view["children"].as_array().expect("children");
    assert!(!children.is_empty() && children.len() <= 25);
    // The breadcrumb of a top box is the box alone.
    assert_eq!(
        view["breadcrumb"],
        serde_json::json!([{ "id": id, "name": view["name"] }])
    );
    assert!(view["elsewhere"].is_array());
    assert!(view["elsewhere_more"].is_u64());
    assert!(view["hidden_arrows"].is_u64());
    // A box below it has a longer breadcrumb that ends with itself.
    let deeper = children.iter().find(|c| c["expandable"] == true);
    if let Some(child) = deeper {
        let cid = child["id"].as_str().unwrap();
        let (_, v2) = get_json(&live, &format!("/api/group/{cid}"));
        let crumbs = ids(&v2["breadcrumb"], "id");
        assert_eq!(crumbs.first(), Some(id));
        assert_eq!(crumbs.last().map(String::as_str), Some(cid));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publishing_bumps_the_seq_in_the_overview() {
    let dir = project();
    let live = start(dir.path()).await;
    let (_, before) = get_json(&live, "/api/overview");
    assert_eq!(before["seq"], 0);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let model = (*live.handle.model()).clone();
    live.handle.publish(model, hm_serve::FactsDelta::default());
    let (_, after) = get_json(&live, "/api/overview");
    assert_eq!(after["seq"], 1);
    assert!(after["updated_ms"].as_u64() > before["updated_ms"].as_u64());
    let groups = all_groups(&live);
    let (_, view) = get_json(&live, &format!("/api/group/{}", groups[0]));
    assert_eq!(view["seq"], 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_file_card_links_lift_to_the_view() {
    let dir = project();
    let live = start(dir.path()).await;
    let (_, overview) = get_json(&live, "/api/overview");
    let top = ids(&overview["boxes"], "id");

    // Default view: the overview, whose ends are top boxes.
    let (status, card) = get_json(&live, "/api/card?path=src/orders/service.ts");
    assert_eq!(status, 200);
    let links = &card["links"];
    assert_eq!(links["view"], "overview");
    let ends: Vec<String> = ["incoming", "outgoing"]
        .iter()
        .flat_map(|k| ids(&links[*k], "id"))
        .collect();
    assert!(!ends.is_empty(), "service.ts has links to other boxes");
    for e in &ends {
        assert!(top.contains(e), "{e} is a top box");
    }
    let (_, explicit) = get_json(&live, "/api/card?path=src/orders/service.ts&view=overview");
    assert_eq!(explicit["links"], card["links"]);

    // A group view: ends inside the group are its children.
    let groups = all_groups(&live);
    let mut saw_child_end = false;
    for g in &groups {
        let (_, view) = get_json(&live, &format!("/api/group/{g}"));
        let children = ids(&view["children"], "id");
        let (_, c) = get_json(
            &live,
            &format!("/api/card?path=src/orders/service.ts&view={g}"),
        );
        let l = &c["links"];
        if l.is_null() {
            continue;
        }
        assert_eq!(l["view"], *g);
        for k in ["incoming", "outgoing"] {
            for e in ids(&l[k], "id") {
                saw_child_end |= children.contains(&e);
                assert!(children.contains(&e) || top.contains(&e), "{e}");
            }
        }
    }
    assert!(saw_child_end, "some view names a child of its group");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_view_gives_null_links_not_an_error() {
    let dir = project();
    let live = start(dir.path()).await;
    let (status, card) = get_json(&live, "/api/card?path=src/orders/service.ts&view=g_nothing");
    assert_eq!(status, 200);
    assert!(card["links"].is_null());
    assert_eq!(card["card"]["path"], "src/orders/service.ts");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_file_card_has_null_links() {
    let dir = project();
    let live = start(dir.path()).await;
    let (status, card) = get_json(&live, "/api/card?path=src/orders/service.test.ts");
    assert_eq!(status, 200);
    assert!(card["links"].is_null());
    assert!(card["ancestors"].is_null());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_card_names_the_groups_that_hold_the_file() {
    let dir = project();
    let live = start(dir.path()).await;
    let (_, card) = get_json(&live, "/api/card?path=src/orders/service.ts");
    let ancestors = ids_of_strings(&card["ancestors"]);
    assert!(!ancestors.is_empty(), "the file is below a top box");
    let (_, overview) = get_json(&live, "/api/overview");
    assert!(ids(&overview["boxes"], "id").contains(&ancestors[0]));
    // Every ancestor is a group that opens, and the last one lists the file.
    for (i, g) in ancestors.iter().enumerate() {
        let (status, view) = get_json(&live, &format!("/api/group/{g}"));
        assert_eq!(status, 200, "{g}");
        if let Some(next) = ancestors.get(i + 1) {
            assert!(ids(&view["children"], "id").contains(next));
        } else {
            let paths: Vec<&str> = view["children"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|c| c["path"].as_str())
                .collect();
            assert!(paths.contains(&"src/orders/service.ts"), "{paths:?}");
        }
    }
}

fn ids_of_strings(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("a list")
        .iter()
        .map(|s| s.as_str().expect("a string").to_string())
        .collect()
}
