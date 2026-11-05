import { test } from "node:test";
import assert from "node:assert/strict";
import { createGroupCache } from "../src/groupcache.ts";
import type { GroupView } from "../src/types.ts";

function group(id: string, seq = 1): GroupView {
  return {
    v: 1,
    seq,
    id,
    name: id,
    role: "work",
    role_word: "Logic",
    w: 100,
    h: 100,
    breadcrumb: [],
    children: [],
    arrows: [],
    hidden_arrows: 0,
    elsewhere: [],
    elsewhere_more: 0,
  };
}

const delta = (groups_changed: string[], regrouped = false) => ({ regrouped, groups_changed });

test("a group a delta names is dropped even when it is not on screen", () => {
  const c = createGroupCache();
  c.set("a", group("a"));
  c.set("b", group("b"));
  c.invalidate(delta(["a"]), []);
  assert.equal(c.get("a"), undefined);
  assert.ok(c.get("b") !== undefined);
  assert.equal(c.needsFetch("a", 1), true);
});

test("a regroup empties the cache, so it cannot grow without bound", () => {
  const c = createGroupCache();
  for (const id of ["a", "b", "c"]) c.set(id, group(id));
  c.invalidate(delta([], true), []);
  assert.equal(c.size, 0);
});

test("groups on screen stay until their refetch replaces them", () => {
  const c = createGroupCache();
  c.set("a", group("a"));
  c.set("b", group("b"));
  c.invalidate(delta(["a"], true), ["a"]);
  assert.ok(c.get("a") !== undefined);
  assert.equal(c.get("b"), undefined);
  c.invalidate(delta(["a"]), ["a"]);
  assert.ok(c.get("a") !== undefined);
});

test("a delta that names no group and no regroup changes nothing", () => {
  const c = createGroupCache();
  c.set("a", group("a"));
  const v = c.version;
  c.invalidate(delta([]), []);
  assert.equal(c.size, 1);
  assert.equal(c.version, v);
});

test("an invalidation bumps the version, for fetches that were in flight", () => {
  const c = createGroupCache();
  const v = c.version;
  c.invalidate(delta(["x"]), []);
  assert.ok(c.version > v);
});

test("a held group older than the overview is fetched again, a current one is not", () => {
  const c = createGroupCache();
  assert.equal(c.needsFetch("a", 0), true);
  c.set("a", group("a", 4));
  assert.equal(c.needsFetch("a", 4), false);
  assert.equal(c.needsFetch("a", 3), false);
  assert.equal(c.needsFetch("a", 5), true);
});

test("expand, collapse, a save to the group, expand again gets a new fetch", () => {
  const c = createGroupCache();
  c.set("a", group("a", 1)); // expanded once
  // collapsed: the delta names a, which is not on screen
  c.invalidate(delta(["a"]), []);
  assert.equal(c.needsFetch("a", 2), true);
});
