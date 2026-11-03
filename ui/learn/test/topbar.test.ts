import { test } from "node:test";
import assert from "node:assert/strict";
import { SESSION_ENDED, levelCookie, levelFromCookie, liveLabel, repoChip } from "../src/topbar.ts";

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

test("before the first connection attempt settles the bar says Connecting, not Offline", () => {
  assert.equal(liveLabel(false, 0, 160_000, true), "Connecting…");
});

test("an ended session says how to get back in, whatever the connection state", () => {
  const said = "This panel's session ended. Open the link that `harbormap learn` printed.";
  assert.equal(SESSION_ENDED, said);
  assert.equal(liveLabel(false, 100_000, 160_000, false, true), said);
  assert.equal(liveLabel(true, 100_000, 160_000, false, true), said);
  assert.equal(liveLabel(false, 0, 160_000, true, true), said);
});

test("the remembered level is read from a plain cookie, which survives a new port", () => {
  assert.equal(levelFromCookie("hm_level=builder"), "builder");
  assert.equal(levelFromCookie("a=1; hm_level=beginner; b=2"), "beginner");
  assert.equal(levelFromCookie("hm_level=expert"), null);
  assert.equal(levelFromCookie("xhm_level=builder"), null);
  assert.equal(levelFromCookie(""), null);
});

test("the level cookie lasts a year, on every path, and is not sent across sites", () => {
  assert.equal(levelCookie("builder"), "hm_level=builder; Path=/; SameSite=Strict; Max-Age=31536000");
});
