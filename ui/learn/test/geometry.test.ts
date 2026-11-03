import { test } from "node:test";
import assert from "node:assert/strict";
import {
  FIT_MARGIN,
  arrowCurve,
  arrowPath,
  boxLines,
  changedBoxes,
  chipLabel,
  chipSlots,
  chipsFor,
  clampZoom,
  edgePoint,
  ensureVisible,
  fit,
  resizeView,
  strokeWidth,
  truncate,
  zoomAt,
} from "../src/geometry.ts";
import type { Rect } from "../src/geometry.ts";
import type { Arrow, Box, Chip } from "../src/types.ts";

const box = (x: number, y: number, w = 200, h = 72): Rect => ({ x, y, w, h });

function mk(id: string, over: Partial<Box> = {}): Box {
  return {
    id,
    name: id,
    kind: "folder",
    role: "work",
    role_word: "Logic",
    files: 3,
    loc: 120,
    x: 0,
    y: 0,
    w: 200,
    h: 72,
    expandable: true,
    beginner: true,
    ...over,
  };
}

test("an edge point of a wide box toward a far right point is on its right side", () => {
  const b = box(100, 50);
  const p = edgePoint(b, { x: 5000, y: 50 + 36 });
  assert.equal(p.x, b.x + 200);
  assert.equal(p.y, 50 + 36);
});

test("an edge point toward a point below leaves through the bottom", () => {
  const b = box(0, 0);
  const p = edgePoint(b, { x: 100, y: 5000 });
  assert.equal(p.y, 72);
  assert.equal(p.x, 100);
});

test("an edge point toward the centre is the centre", () => {
  assert.deepEqual(edgePoint(box(0, 0), { x: 100, y: 36 }), { x: 100, y: 36 });
});

test("A to B and B to A take different paths with different midpoints", () => {
  const a = box(0, 0);
  const b = box(320, 40);
  assert.ok(arrowPath(a, b) !== arrowPath(b, a));
  const m1 = arrowCurve(a, b).mid;
  const m2 = arrowCurve(b, a).mid;
  assert.ok(Math.hypot(m1.x - m2.x, m1.y - m2.y) >= 14, "the two curves are at least 14 apart");
});

/** True when `p` lies on the outline of `r`, within half a pixel. */
function onOutline(r: Rect, p: { x: number; y: number }): boolean {
  const eps = 0.5;
  const inX = p.x >= r.x - eps && p.x <= r.x + r.w + eps;
  const inY = p.y >= r.y - eps && p.y <= r.y + r.h + eps;
  const onVertical = Math.abs(p.x - r.x) <= eps || Math.abs(p.x - (r.x + r.w)) <= eps;
  const onHorizontal = Math.abs(p.y - r.y) <= eps || Math.abs(p.y - (r.y + r.h)) <= eps;
  return (onVertical && inY) || (onHorizontal && inX);
}

test("an arrow starts exactly on the outline of its first box and ends on the outline of its second", () => {
  const pairs: [Rect, Rect][] = [
    [box(0, 0), box(400, 0)],
    [box(0, 0), box(0, 300)],
    [box(0, 0), box(320, 180)],
    [box(500, 400), box(40, 20)],
    [box(0, 0, 176, 52), box(272, 76, 176, 52)],
  ];
  for (const [a, b] of pairs) {
    for (const [from, to] of [[a, b], [b, a]] as [Rect, Rect][]) {
      const c = arrowCurve(from, to);
      assert.ok(onOutline(from, c.start), `start ${JSON.stringify(c.start)} of ${JSON.stringify(from)}`);
      assert.ok(onOutline(to, c.end), `end ${JSON.stringify(c.end)} of ${JSON.stringify(to)}`);
    }
  }
});

test("stroke width is 1.25 for one reference, grows with the log and stops at 6", () => {
  assert.equal(strokeWidth(1), 1.25);
  assert.equal(strokeWidth(2), 2.25);
  assert.equal(strokeWidth(1e6), 6);
  assert.equal(strokeWidth(0), 1.25);
});

function contains(view: Rect, b: Rect, margin: number): boolean {
  return (
    b.x - margin >= view.x - 1e-9 &&
    b.y - margin >= view.y - 1e-9 &&
    b.x + b.w + margin <= view.x + view.w + 1e-9 &&
    b.y + b.h + margin <= view.y + view.h + 1e-9
  );
}

test("fit contains every box with the margin and has the aspect of the viewport", () => {
  const boxes = [box(0, 0), box(320, 0), box(640, 400), box(-50, 300)];
  for (const vp of [{ w: 1000, h: 600 }, { w: 400, h: 900 }, { w: 1440, h: 700 }]) {
    const v = fit(boxes, vp);
    for (const b of boxes) assert.ok(contains(v, b, FIT_MARGIN), `${JSON.stringify(vp)} ${JSON.stringify(b)}`);
    assert.ok(Math.abs(v.w / v.h - vp.w / vp.h) < 1e-9);
  }
});

test("fit does not blow a tiny map up past 1.5 times", () => {
  const v = fit([box(0, 0)], { w: 1440, h: 900 });
  assert.ok(1440 / v.w <= 1.5 + 1e-9);
  assert.ok(contains(v, box(0, 0), FIT_MARGIN));
});

test("fit with nothing to show is the viewport itself", () => {
  assert.deepEqual(fit([], { w: 800, h: 600 }), { x: 0, y: 0, w: 800, h: 600 });
});

test("zooming keeps the point under the cursor where it is", () => {
  const view = { x: 10, y: 20, w: 1000, h: 500 };
  const at = { x: 300, y: 120 };
  const v = zoomAt(view, 0.5, at);
  assert.equal(v.w, 500);
  // the fraction of the view the point sits at is unchanged
  assert.ok(Math.abs((at.x - v.x) / v.w - (at.x - view.x) / view.w) < 1e-9);
  assert.ok(Math.abs((at.y - v.y) / v.h - (at.y - view.y) / view.h) < 1e-9);
});

test("clampZoom limits the width about the centre", () => {
  const v = clampZoom({ x: 0, y: 0, w: 10000, h: 5000 }, 100, 2000);
  assert.equal(v.w, 2000);
  assert.equal(v.h, 1000);
  assert.equal(v.x + v.w / 2, 5000);
  const same = { x: 0, y: 0, w: 500, h: 250 };
  assert.equal(clampZoom(same, 100, 2000), same);
});

test("ensureVisible leaves a view that already shows the target alone", () => {
  const view = { x: 0, y: 0, w: 1000, h: 600 };
  assert.equal(ensureVisible(view, box(100, 100), 24), view);
});

test("ensureVisible slides the view to the target, and zooms out only when it must", () => {
  const view = { x: 0, y: 0, w: 1000, h: 600 };
  const slid = ensureVisible(view, box(900, 100, 300, 100), 20);
  assert.equal(slid.w, 1000);
  assert.ok(contains(slid, box(900, 100, 300, 100), 20));
  const big = ensureVisible(view, box(0, 0, 3000, 100), 0);
  assert.ok(big.w >= 3000);
  assert.ok(Math.abs(big.w / big.h - 1000 / 600) < 1e-9);
  assert.ok(contains(big, box(0, 0, 3000, 100), 0));
});

test("resizeView keeps the scale and the centre", () => {
  const v = resizeView({ x: 0, y: 0, w: 1000, h: 500 }, { w: 1000, h: 500 }, { w: 600, h: 500 });
  assert.equal(v.w, 600);
  assert.equal(v.h, 500);
  assert.equal(v.x + v.w / 2, 500);
  const same = { x: 1, y: 2, w: 3, h: 4 };
  assert.equal(resizeView(same, { w: 0, h: 0 }, { w: 5, h: 5 }), same);
});

test("Beginner shows 3 chips and folds the rest and the server's count into +N", () => {
  const chips: Chip[] = ["a", "b", "c", "d", "e"].map((n) => ({ id: n, name: n, role: "keep", out: 1, in: 0 }));
  const b = chipsFor("beginner", chips, 4);
  assert.deepEqual(b.chips.map((c) => c.id), ["a", "b", "c"]);
  assert.equal(b.more, 2 + 4);
  const few = chipsFor("beginner", chips.slice(0, 2), 0);
  assert.equal(few.chips.length, 2);
  assert.equal(few.more, 0);
  const x = chipsFor("builder", chips, 4);
  assert.equal(x.chips.length, 5);
  assert.equal(x.more, 4);
});

test("chip slots run down the right edge without overlapping", () => {
  const s = chipSlots(500, 3);
  assert.equal(s.length, 3);
  for (const r of s) assert.ok(r.x > 500);
  assert.ok(s[1]!.y >= s[0]!.y + s[0]!.h);
});

test("a chip says which way the links go and how many", () => {
  const c = (out: number, inn: number): Chip => ({ id: "x", name: "Data", role: "keep", out, in: inn });
  assert.equal(chipLabel(c(7, 0)), "→ Data · 7");
  assert.equal(chipLabel(c(0, 3)), "← Data · 3");
  assert.equal(chipLabel(c(6, 4)), "↔ Data · 10");
  assert.equal(chipLabel({ id: "x", name: "A very long neighbour name", role: "keep", out: 1, in: 0 }), "→ A very long n… · 1");
});

test("truncate cuts with an ellipsis and leaves short text alone", () => {
  assert.equal(truncate("abc", 5), "abc");
  assert.equal(truncate("abcdefgh", 5), "abcd…");
});

test("a box says its role word in Beginner and its folder path in Builder", () => {
  const b = mk("g_1", { name: "features/orders", role_word: "Logic", files: 12 });
  assert.deepEqual(boxLines(b, "beginner"), { title: "features/orders", sub: "Logic", meta: "12 files" });
  assert.deepEqual(boxLines(b, "builder"), { title: "orders", sub: "features/orders/", meta: "12 files" });
  const f = mk("f_1", { kind: "file", name: "cart.ts", path: "src/lib/cart.ts", loc: 80, generated: true });
  assert.equal(boxLines(f, "builder").sub, "src/lib");
  assert.equal(boxLines(f, "builder").meta, "80 lines · generated");
  assert.equal(boxLines(mk("g_2", { kind: "more", name: "9 more", files: 1 }), "builder").sub, "smaller items");
  assert.equal(boxLines(mk("g_3", { files: 1 }), "beginner").meta, "1 file");
});

test("changedBoxes names new boxes, changed counts and boxes at either end of a changed arrow", () => {
  const prev = [mk("a"), mk("b"), mk("c"), mk("d")];
  const arrows: Arrow[] = [
    { from: "a", to: "b", count: 2 },
    { from: "c", to: "d", count: 1 },
  ];
  assert.deepEqual(changedBoxes(prev, arrows, prev, arrows), []);
  assert.deepEqual(changedBoxes(prev, arrows, [...prev.slice(0, 3), mk("d", { files: 9 })], arrows), ["d"]);
  assert.deepEqual(changedBoxes(prev, arrows, [...prev, mk("e")], arrows), ["e"]);
  assert.deepEqual(changedBoxes(prev, arrows, prev, [{ from: "a", to: "b", count: 3 }, arrows[1]!]), ["a", "b"]);
  // a removed arrow counts; an arrow to a box that is gone is not named
  assert.deepEqual(changedBoxes(prev, arrows, prev, [arrows[0]!]), ["c", "d"]);
  assert.deepEqual(changedBoxes(prev, arrows, prev.slice(0, 3), [arrows[0]!]), ["c"]);
});
