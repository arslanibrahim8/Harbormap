import { test } from "node:test";
import assert from "node:assert/strict";
import { fileCardRows, lineRange, symbolCardRows } from "../src/drawer.ts";
import type { FileCard, SymbolCard } from "../src/types.ts";

function fileCard(over: Partial<FileCard> = {}): FileCard {
  return {
    path: "src/orders/checkout.ts",
    lang: "typescript",
    class: "source",
    parse_status: "ok",
    lines: 120,
    symbols: [
      { name: "Checkout", kind: "class", l0: 3, l1: 90, exported: true, called_from: 2 },
      { name: "Checkout.placeOrder", kind: "method", l0: 10, l1: 40, exported: false, called_from: 1 },
      { name: "totalWithTax", kind: "function", l0: 92, l1: 92, exported: true, called_from: 0 },
    ],
    used_by: ["a.ts", "b.ts", "c.ts", "d.ts", "e.ts", "f.ts", "g.ts"],
    uses: ["src/db/connection.ts", "zod (external)"],
    unresolved: 2,
    ...over,
  };
}

function symbolCard(over: Partial<SymbolCard> = {}): SymbolCard {
  return {
    name: "Checkout.placeOrder",
    kind: "method",
    path: "src/orders/checkout.ts",
    l0: 10,
    l1: 40,
    sig: "placeOrder(cart: Cart): Promise<Order>",
    callers: ["submitForm", "retryOrder"],
    callees: ["totalWithTax", "Db.insert"],
    unresolved_calls: ["db.insert"],
    ...over,
  };
}

test("Beginner shows the first five importers and says how many more", () => {
  const rows = fileCardRows(fileCard(), "beginner");
  assert.deepEqual(rows.usedBy.rows.map((r) => r.text), ["a.ts", "b.ts", "c.ts", "d.ts", "e.ts"]);
  assert.equal(rows.usedBy.more, "and 2 more");
  assert.equal(rows.usedBy.summary, "Used from 7 places");
});

test("Beginner with five or fewer importers has no 'and N more'", () => {
  const rows = fileCardRows(fileCard({ used_by: ["a.ts", "b.ts", "c.ts", "d.ts", "e.ts"] }), "beginner");
  assert.equal(rows.usedBy.rows.length, 5);
  assert.equal(rows.usedBy.more, null);
});

test("Builder lists every importer, in the order the server gave", () => {
  const rows = fileCardRows(fileCard(), "builder");
  assert.equal(rows.usedBy.rows.length, 7);
  assert.equal(rows.usedBy.more, null);
  assert.equal(rows.usedBy.rows[0]?.text, "a.ts");
  assert.equal(rows.usedBy.summary, "7 importers");
});

test("job rows keep the full names and the level's words for callers", () => {
  const beginner = fileCardRows(fileCard(), "beginner").jobs;
  assert.deepEqual(beginner.map((j) => j.name), ["Checkout", "Checkout.placeOrder", "totalWithTax"]);
  assert.deepEqual(beginner.map((j) => j.kind), ["class", "method", "function"]);
  assert.equal(beginner[0]?.calls, "Called from 2 places");
  assert.equal(beginner[1]?.calls, "Called from 1 place");
  assert.equal(beginner[2]?.calls, "Called from 0 places");
  const builder = fileCardRows(fileCard(), "builder").jobs;
  assert.equal(builder[0]?.calls, "2 callers");
  assert.equal(builder[1]?.calls, "1 caller");
});

test("a very long job name is never cut", () => {
  const name = `Owner.${"veryLongName".repeat(12)}`;
  const rows = fileCardRows(
    fileCard({ symbols: [{ name, kind: "method", l0: 1, l1: 2, exported: false, called_from: 0 }] }),
    "beginner",
  );
  assert.equal(rows.jobs[0]?.name, name);
  assert.equal(rows.jobs[0]?.symbol, name);
});

test("only imports of project files can be opened; packages are plain text", () => {
  const rows = fileCardRows(fileCard(), "builder");
  assert.deepEqual(rows.uses, [
    { text: "src/db/connection.ts", path: "src/db/connection.ts" },
    { text: "zod (external)", path: null },
  ]);
  assert.equal(rows.usedBy.rows[0]?.path, "a.ts");
});

test("technical details carry lines, language, class, parse status, unresolved, symbol lines", () => {
  const rows = fileCardRows(fileCard(), "builder");
  assert.deepEqual(rows.details, [
    { label: "Lines", value: "120" },
    { label: "Language", value: "typescript" },
    { label: "Class", value: "source" },
    { label: "Parse status", value: "ok" },
    { label: "Unresolved references", value: "2" },
  ]);
  assert.deepEqual(rows.symbolLines, [
    { name: "Checkout", lines: "3–90" },
    { name: "Checkout.placeOrder", lines: "10–40" },
    { name: "totalWithTax", lines: "92" },
  ]);
});

test("a file with no known language says so", () => {
  const rows = fileCardRows(fileCard({ lang: null }), "beginner");
  assert.equal(rows.details[1]?.value, "unknown");
});

test("the file card title is the file name, with the path beside it", () => {
  const rows = fileCardRows(fileCard(), "beginner");
  assert.equal(rows.title, "checkout.ts");
  assert.equal(rows.path, "src/orders/checkout.ts");
  assert.equal(rows.lead, "Written descriptions arrive with the prose version.");
});

test("lineRange gives one number for a one-line symbol", () => {
  assert.equal(lineRange(3, 90), "3–90");
  assert.equal(lineRange(5, 5), "5");
});

test("Builder adds the signature to a function card; Beginner does not", () => {
  const builder = symbolCardRows(symbolCard(), "builder");
  assert.equal(builder.signature, "placeOrder(cart: Cart): Promise<Order>");
  const beginner = symbolCardRows(symbolCard(), "beginner");
  assert.equal(beginner.signature, null);
});

test("an empty signature is not shown even in Builder", () => {
  assert.equal(symbolCardRows(symbolCard({ sig: "" }), "builder").signature, null);
});

test("a function card lists callers, callees, calls it cannot follow, and its place", () => {
  const rows = symbolCardRows(symbolCard(), "builder");
  assert.equal(rows.title, "Checkout.placeOrder");
  assert.equal(rows.kind, "method");
  assert.deepEqual(rows.callers, ["submitForm", "retryOrder"]);
  assert.deepEqual(rows.callees, ["totalWithTax", "Db.insert"]);
  assert.deepEqual(rows.unresolved, ["db.insert"]);
  assert.deepEqual(rows.details, [
    { label: "Lines", value: "10–40" },
    { label: "File", value: "src/orders/checkout.ts" },
  ]);
  assert.equal(rows.path, "src/orders/checkout.ts");
  assert.equal(rows.from, 10);
  assert.equal(rows.to, 40);
  assert.equal(symbolCardRows(symbolCard(), "beginner").callersSummary, "Called from 2 places");
  assert.equal(rows.callersSummary, "2 callers");
});
