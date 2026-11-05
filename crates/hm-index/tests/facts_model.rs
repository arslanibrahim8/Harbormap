//! The facts model: the optional fields, their hash effect and `FileFacts::holder`.

use hm_index::facts::{AttrFact, FileFacts, LabelFact, ParseStatus, SymbolFact, SymbolKind};
use hm_index::hash::interface_hash;
use hm_index::lang::Lang;
use hm_index::ts::extract;

fn symbol(kind: SymbolKind, owner: &str, name: &str, l0: u32, l1: u32) -> SymbolFact {
    SymbolFact {
        kind,
        name: name.to_string(),
        owner: owner.to_string(),
        sig: String::new(),
        sig_hash: String::new(),
        body_hash: String::new(),
        l0,
        l1,
        exported: false,
        default_export: false,
        lines: l1 - l0 + 1,
        branches: 0,
    }
}

#[test]
fn ts_facts_json_has_no_new_keys_and_old_json_reads() {
    let facts = extract("export function f() {}", Lang::TypeScript);
    let json = serde_json::to_string(&facts).unwrap();
    for key in ["\"package\"", "\"labels\"", "\"attrs\""] {
        assert!(!json.contains(key), "{key} in {json}");
    }
    let back: FileFacts = serde_json::from_str(&json).unwrap();
    assert_eq!(back, facts);
}

#[test]
fn package_and_attrs_change_the_interface_hash() {
    let base = FileFacts::empty(ParseStatus::Parsed);
    let before = interface_hash(&base);

    let mut with_package = base.clone();
    with_package.package = Some("shop".to_string());
    assert_ne!(interface_hash(&with_package), before);

    let mut with_attr = base.clone();
    with_attr.attrs.push(AttrFact {
        class: "Cart".to_string(),
        attr: "repo".to_string(),
        type_name: "Repo".to_string(),
        line: 3,
    });
    assert_ne!(interface_hash(&with_attr), before);

    let mut with_label = base.clone();
    with_label.labels.push(LabelFact {
        owner: Some((String::new(), "f".to_string())),
        name: "inner".to_string(),
        l0: 2,
        l1: 4,
    });
    assert_eq!(interface_hash(&with_label), before);
}

#[test]
fn holder_picks_the_smallest_span_and_refuses_a_tie() {
    let mut f = FileFacts::empty(ParseStatus::Parsed);
    f.symbols.push(symbol(SymbolKind::Method, "C", "m", 1, 10));
    f.symbols.push(symbol(SymbolKind::Method, "C", "m", 3, 4));
    assert_eq!(f.holder("C", "m", 3), Some(1));
    assert_eq!(f.holder("C", "m", 8), Some(0));
    assert_eq!(f.holder("C", "m", 11), None);
    assert_eq!(f.holder("C", "other", 3), None);

    let mut tie = FileFacts::empty(ParseStatus::Parsed);
    tie.symbols
        .push(symbol(SymbolKind::Function, "", "f", 5, 5));
    tie.symbols
        .push(symbol(SymbolKind::Function, "", "f", 5, 5));
    assert_eq!(tie.holder("", "f", 5), None);

    let mut var = FileFacts::empty(ParseStatus::Parsed);
    var.symbols
        .push(symbol(SymbolKind::Variable, "", "v", 1, 3));
    assert_eq!(var.holder("", "v", 2), None);
}
