import { test } from "node:test";
import assert from "node:assert/strict";
import { liveLabel, repoChip } from "../src/topbar.ts";

test("the repo chip is branch@commit, or 'no git' when both are missing", () => {
  assert.equal(repoChip({ name: "shop", branch: "main", commit: "a1b2c3d" }), "main@a1b2c3d");
  assert.equal(repoChip({ name: "shop", branch: null, commit: null }), "no git");
});

test("a detached head or an unborn branch still shows what it has", () => {
  assert.equal(repoChip({ name: "shop", branch: null, commit: "a1b2c3d" }), "a1b2c3d");
  assert.equal(repoChip({ name: "shop", branch: "main", commit: null }), "main");
});

test("the live label says how long ago, or that the connection is down", () => {
  assert.equal(liveLabel(true, 100_000, 160_000), "Live · updated 1 min ago");
  assert.equal(liveLabel(true, 100_000, 101_000), "Live · updated just now");
  assert.equal(liveLabel(true, 0, 160_000), "Live");
  assert.equal(liveLabel(false, 100_000, 160_000), "Offline — reconnecting");
});
