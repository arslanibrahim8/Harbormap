import { test } from "node:test";
import assert from "node:assert/strict";
import { createCardLoader, fileCardRows, goneNotice, lineRange, symbolCardRows } from "../src/drawer.ts";
import type { CardDeps, Target } from "../src/drawer.ts";
import { ApiError } from "../src/api.ts";
import type { CardPayload, FileCard, SymbolCard } from "../src/types.ts";

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

// ---- The card loader ----------------------------------------------------------

const T: Target = { path: "src/orders/checkout.ts", symbol: null, view: "overview" };

function payloadAt(seq: number): CardPayload {
  return { v: 1, seq, kind: "file", card: fileCard(), links: null };
}

const tick = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

/** Stand-in collaborators: `answers` are returned in turn (an Error is thrown). */
function harness(answers: (CardPayload | Error)[], stateSeq: number) {
  const log: string[] = [];
  const shown: number[] = [];
  let calls = 0;
  const deps: CardDeps = {
    fetchCard: () => {
      const a = answers[Math.min(calls, answers.length - 1)] as CardPayload | Error;
      calls += 1;
      return a instanceof Error ? Promise.reject(a) : Promise.resolve(a);
    },
    seq: () => stateSeq,
    loading: () => log.push("loading"),
    show: (p) => {
      log.push("show");
      shown.push(p.seq);
    },
    gone: () => log.push("gone"),
    failed: () => log.push("failed"),
  };
  return { loader: createCardLoader(deps), log, shown, calls: () => calls };
}

test("a card for a file the server no longer knows (404) is reported gone, and no stale card is shown", async () => {
  const x = harness([new ApiError(404, "/api/card")], 3);
  await x.loader.load(T, false);
  assert.deepEqual(x.log, ["loading", "gone"]);
  assert.equal(goneNotice(T), "This file is gone from the project.");
  assert.equal(goneNotice({ ...T, symbol: "Checkout.placeOrder" }), "This function is gone from the project.");
});

test("a live refresh that gets a 404 also reports gone, but another failure is silent", async () => {
  const gone = harness([new ApiError(404, "/api/card")], 3);
  await gone.loader.load(T, true);
  assert.deepEqual(gone.log, ["gone"]);
  const down = harness([new ApiError(500, "/api/card")], 3);
  await down.loader.load(T, true);
  assert.deepEqual(down.log, []);
  const asked = harness([new ApiError(500, "/api/card")], 3);
  await asked.loader.load(T, false);
  assert.deepEqual(asked.log, ["loading", "failed"]);
});

test("an answer older than the overview is asked for again, also on a live refresh", async () => {
  const x = harness([payloadAt(4), payloadAt(5)], 5);
  await x.loader.load(T, true);
  assert.equal(x.calls(), 2);
  assert.deepEqual(x.shown, [5]);
});

test("a card that stays older than the overview is shown after the last attempt", async () => {
  const x = harness([payloadAt(1)], 9);
  await x.loader.load(T, false);
  assert.equal(x.calls(), 3);
  assert.deepEqual(x.shown, [1]);
});

test("a fresh answer is shown at once, and an equal seq is not stale", async () => {
  const x = harness([payloadAt(5)], 5);
  await x.loader.load(T, false);
  assert.equal(x.calls(), 1);
  assert.deepEqual(x.log, ["loading", "show"]);
});

test("the newest request wins, and close() drops what is still on its way", async () => {
  const resolvers: ((p: CardPayload) => void)[] = [];
  const shown: number[] = [];
  const loader = createCardLoader({
    fetchCard: () => new Promise((r) => resolvers.push(r)),
    seq: () => 0,
    loading: () => {},
    show: (p) => shown.push(p.seq),
    gone: () => {},
    failed: () => {},
  });
  const first = loader.load(T, false);
  const second = loader.load({ ...T, path: "other.ts" }, false);
  (resolvers[1] as (p: CardPayload) => void)(payloadAt(2));
  (resolvers[0] as (p: CardPayload) => void)(payloadAt(1));
  await Promise.all([first, second]);
  assert.deepEqual(shown, [2]);
  const third = loader.load(T, false);
  loader.close();
  (resolvers[2] as (p: CardPayload) => void)(payloadAt(3));
  await third;
  assert.deepEqual(shown, [2]);
  assert.equal(loader.target(), null);
});

test("refresh refetches the open target and does nothing when nothing is open", async () => {
  const x = harness([payloadAt(5)], 5);
  await x.loader.refresh();
  assert.equal(x.calls(), 0);
  await x.loader.load(T, false);
  await x.loader.refresh();
  assert.equal(x.calls(), 2);
  assert.deepEqual(x.log, ["loading", "show", "show"]);
  await tick();
});
