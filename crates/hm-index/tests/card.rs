mod common;

use common::indexed;
use hm_index::query::{file_card, symbol_card};

#[test]
fn service_card_lists_symbols_callers_and_users() {
    let (ix, _dir) = indexed("ts-basic");
    let card = file_card(ix.store(), "src/orders/service.ts")
        .unwrap()
        .expect("the file is indexed");

    assert_eq!(card.path, "src/orders/service.ts");
    assert_eq!(card.lang.as_deref(), Some("typescript"));
    assert_eq!(card.class, "source");
    assert_eq!(card.parse_status, "parsed");
    assert_eq!(card.lines, 21);

    let place = card
        .symbols
        .iter()
        .find(|s| s.name == "placeOrder")
        .expect("placeOrder is listed");
    assert_eq!(place.kind, "function");
    assert!(place.exported);
    assert_eq!(place.called_from, 3); // main, run, the test file's top level
    assert_eq!((place.l0, place.l1), (5, 9));
    // Sorted by position; methods carry their class name.
    let names: Vec<&str> = card.symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "placeOrder",
            "OrderService",
            "OrderService.find",
            "OrderService.total",
            "OrderService.sum"
        ]
    );

    assert_eq!(
        card.used_by.first().map(String::as_str),
        Some("src/index.ts")
    );
    assert_eq!(
        card.used_by.last().map(String::as_str),
        Some("src/orders/service.test.ts")
    );
    for want in ["src/db.ts", "src/orders/mail.ts", "zod (external)"] {
        assert!(
            card.uses.iter().any(|u| u == want),
            "{want} in {:?}",
            card.uses
        );
    }
    // Files first, then packages.
    assert_eq!(card.uses.last().map(String::as_str), Some("zod (external)"));
    assert!(card.unresolved >= 1);
}

#[test]
fn symbol_card_shows_callers_and_unresolved_calls() {
    let (ix, _dir) = indexed("ts-basic");
    let card = symbol_card(ix.store(), "src/orders/service.ts", "placeOrder")
        .unwrap()
        .expect("the symbol exists");

    assert_eq!(card.name, "placeOrder");
    assert_eq!(card.kind, "function");
    assert_eq!(card.path, "src/orders/service.ts");
    assert_eq!((card.l0, card.l1), (5, 9));
    assert!(card.sig.contains("placeOrder"));
    assert_eq!(
        card.callers,
        [
            "src/index.ts#main",
            "src/legacy.js#run",
            "src/orders/service.test.ts"
        ]
    );
    assert_eq!(card.callees, ["src/orders/mail.ts#sendReceipt"]);
    assert!(card.unresolved_calls.iter().any(|c| c == "db.insert"));
}

#[test]
fn unknown_files_and_symbols_are_none() {
    let (ix, _dir) = indexed("ts-basic");
    assert!(file_card(ix.store(), "src/nope.ts").unwrap().is_none());
    assert!(
        symbol_card(ix.store(), "src/orders/service.ts", "nope")
            .unwrap()
            .is_none()
    );
    assert!(
        symbol_card(ix.store(), "src/nope.ts", "placeOrder")
            .unwrap()
            .is_none()
    );
    let method = symbol_card(ix.store(), "src/orders/service.ts", "OrderService.total")
        .unwrap()
        .expect("a method is found by Owner.name");
    assert_eq!(method.name, "OrderService.total");
    assert_eq!(method.callees, ["src/orders/service.ts#OrderService.sum"]);
}
