import { test } from "node:test";
import assert from "node:assert/strict";
import {
  ago,
  arrowsFor,
  calledFrom,
  childrenFor,
  dirName,
  fileName,
  plural,
  usedFrom,
} from "../src/format.ts";
import type { Arrow, Box } from "../src/types.ts";

function box(id: string, beginner: boolean): Box {
  return {
    id,
    name: id,
    kind: "folder",
    role: "work",
    role_word: "Work",
    files: 1,
    loc: 10,
    x: 0,
    y: 0,
    w: 100,
    h: 50,
    expandable: false,
    beginner,
  };
}

test("ago switches unit at 5 s, 60 s, 60 min and 24 h", () => {
  const now = 1_000_000_000;
  assert.equal(ago(now - 4_999, now), "just now");
  assert.equal(ago(now - 5_000, now), "5 s ago");
  assert.equal(ago(now - 59_999, now), "59 s ago");
  assert.equal(ago(now - 60_000, now), "1 min ago");
  assert.equal(ago(now - 3_599_999, now), "59 min ago");
  assert.equal(ago(now - 3_600_000, now), "1 h ago");
  assert.equal(ago(now - 7_300_000, now), "2 h ago");
});

test("ago treats a time in the future as just now", () => {
  assert.equal(ago(2_000, 1_000), "just now");
});

test("plural picks the singular only for exactly one", () => {
  assert.equal(plural(1, "file", "files"), "1 file");
  assert.equal(plural(0, "file", "files"), "0 files");
  assert.equal(plural(12, "file", "files"), "12 files");
});

test("usedFrom and calledFrom word the count per level", () => {
  assert.equal(usedFrom("beginner", 3), "Used from 3 places");
  assert.equal(usedFrom("beginner", 1), "Used from 1 place");
  assert.equal(usedFrom("builder", 3), "3 importers");
  assert.equal(usedFrom("builder", 1), "1 importer");
  assert.equal(calledFrom("beginner", 2), "Called from 2 places");
  assert.equal(calledFrom("beginner", 1), "Called from 1 place");
  assert.equal(calledFrom("builder", 2), "2 callers");
  assert.equal(calledFrom("builder", 1), "1 caller");
});

test("arrowsFor keeps 10 arrows for Beginner and 40 for Builder", () => {
  const arrows: Arrow[] = Array.from({ length: 50 }, (_, i) => ({
    from: `a${i}`,
    to: `b${i}`,
    count: 50 - i,
  }));
  const beginner = arrowsFor("beginner", arrows);
  assert.equal(beginner.length, 10);
  assert.equal(beginner[0]?.from, "a0");
  assert.equal(arrowsFor("builder", arrows).length, 40);
  assert.equal(arrowsFor("beginner", arrows.slice(0, 40)).length, 10);
  assert.equal(arrowsFor("builder", arrows.slice(0, 5)).length, 5);
});

test("childrenFor keeps only flagged boxes for Beginner", () => {
  const boxes = [box("a", true), box("b", false), box("c", true)];
  assert.deepEqual(
    childrenFor("beginner", boxes).map((b) => b.id),
    ["a", "c"],
  );
  assert.equal(childrenFor("builder", boxes).length, 3);
});

test("childrenFor keeps the selected file in Beginner even when the server did not flag it", () => {
  const file = { ...box("f", false), kind: "file" as const, path: "src/a/f.ts" };
  const other = { ...box("g", false), kind: "file" as const, path: "src/a/g.ts" };
  const boxes = [box("a", true), file, other, box("c", true)];
  const ids = (list: Box[]): string[] => list.map((b) => b.id);
  assert.deepEqual(ids(childrenFor("beginner", boxes, "src/a/f.ts")), ["a", "f", "c"], "in the server's order");
  assert.deepEqual(ids(childrenFor("beginner", boxes, "src/none.ts")), ["a", "c"]);
  assert.deepEqual(ids(childrenFor("beginner", boxes, null)), ["a", "c"]);
  assert.deepEqual(ids(childrenFor("builder", boxes, "src/a/f.ts")), ["a", "f", "g", "c"]);
});

test("fileName and dirName split on the last slash", () => {
  assert.equal(fileName("src/orders/cart.ts"), "cart.ts");
  assert.equal(dirName("src/orders/cart.ts"), "src/orders");
  assert.equal(fileName("main.rs"), "main.rs");
  assert.equal(dirName("main.rs"), "");
});
