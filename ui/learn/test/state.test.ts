import { test } from "node:test";
import assert from "node:assert/strict";
import { currentView, getState, onChange, setState } from "../src/state.ts";
import type { State } from "../src/state.ts";

test("the store starts in Beginner on the Map tab with nothing selected", () => {
  const s = getState();
  assert.equal(s.level, "beginner");
  assert.equal(s.tab, "map");
  assert.equal(s.live, false);
  assert.equal(s.overview, null);
  assert.equal(s.selected, null);
  assert.equal(s.code, null);
  assert.deepEqual(s.expanded, []);
});

test("setState merges the patch and tells observers the new state and the previous one", () => {
  const seen: [State, State][] = [];
  onChange((s, prev) => seen.push([s, prev]));
  const before = getState();
  setState({ seq: 7, live: true });
  assert.equal(seen.length, 1);
  const [now, prev] = seen[0] as [State, State];
  assert.equal(now.seq, 7);
  assert.equal(now.live, true);
  assert.equal(prev.seq, before.seq);
  assert.equal(prev.live, false);
  assert.equal(getState(), now);
  setState({ seq: 0, live: false });
});

test("a patch that changes nothing does not notify", () => {
  let calls = 0;
  onChange(() => (calls += 1));
  setState({ seq: getState().seq });
  setState({});
  assert.equal(calls, 0);
});

test("changing the level keeps the expanded groups", () => {
  const expanded = ["g_aaaaaaaaaaaa", "g_bbbbbbbbbbbb"];
  setState({ expanded });
  setState({ level: "builder" });
  assert.equal(getState().level, "builder");
  assert.deepEqual(getState().expanded, expanded);
  setState({ level: "beginner" });
  assert.deepEqual(getState().expanded, expanded);
});

test("currentView is the innermost expanded group, or the overview", () => {
  setState({ expanded: [] });
  assert.equal(currentView(), "overview");
  setState({ expanded: ["g_a", "g_b"] });
  assert.equal(currentView(), "g_b");
  setState({ expanded: [] });
});
