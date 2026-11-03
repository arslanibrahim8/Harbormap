import { test } from "node:test";
import assert from "node:assert/strict";
import { createLive } from "../src/live.ts";
import type { LiveDeps } from "../src/live.ts";
import { getState, setState } from "../src/state.ts";
import type { FactsDelta, Overview } from "../src/types.ts";

const tick = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

function overview(seq: number, updated_ms = 1000): Overview {
  return {
    v: 1,
    seq,
    repo: { name: "shop", branch: "main", commit: "a1b2c3d" },
    updated_ms,
    boxes: [],
    arrows: [],
    hidden_arrows: 0,
    excluded: [],
    files_total: 0,
  };
}

function delta(over: Partial<FactsDelta> = {}): FactsDelta {
  return {
    seq: 5,
    at_ms: 4242,
    took_ms: 3,
    files_changed: [],
    files_removed: [],
    regrouped: false,
    groups_changed: [],
    ...over,
  };
}

/** A live object over stand-ins; `fire` reaches the handlers `start` registered. */
function setup(seqs: number[]) {
  const calls = { overview: 0, drawer: 0, code: 0 };
  const fire = {
    hello: (_seq: number): void => {},
    facts: (_d: FactsDelta): void => {},
    state: (_live: boolean): void => {},
  };
  const deps: LiveDeps = {
    fetchOverview: () => {
      const seq = seqs[Math.min(calls.overview, seqs.length - 1)] as number;
      calls.overview += 1;
      return Promise.resolve(overview(seq));
    },
    subscribe: (hello, facts, state) => {
      fire.hello = hello;
      fire.facts = facts;
      fire.state = state;
      return () => {};
    },
    refreshDrawer: () => {
      calls.drawer += 1;
      return Promise.resolve();
    },
    reloadCode: () => {
      calls.code += 1;
      return Promise.resolve();
    },
  };
  const live = createLive(deps);
  live.start();
  return { live, calls, fire };
}

function reset(): void {
  setState({ seq: 0, updatedMs: 0, live: false, connecting: true, overview: null, code: null });
}

test("an overview newer than the one on screen is applied; an older one is dropped", async () => {
  reset();
  const x = setup([5, 4, 6]);
  await x.live.loadOverview();
  assert.equal(getState().seq, 5);
  assert.equal(getState().updatedMs, 1000);
  await x.live.loadOverview();
  assert.equal(getState().seq, 5, "the older answer (seq 4) was dropped");
  await x.live.loadOverview();
  assert.equal(getState().seq, 6);
  reset();
});

test("a facts event refreshes the open card even when its file is not listed", async () => {
  reset();
  setState({ seq: 4, code: { path: "src/a.ts", from: 0, to: 0 } });
  const x = setup([5]);
  x.fire.facts(delta({ files_changed: ["src/other.ts"] }));
  assert.equal(getState().updatedMs, 4242, "the clock moves before the refetch returns");
  await tick();
  assert.equal(x.calls.overview, 1);
  assert.equal(x.calls.drawer, 1);
  assert.equal(x.calls.code, 0, "the Code tab's own file did not change");
  assert.equal(getState().seq, 5);
  reset();
});

test("the Code tab reloads when its file changed or was removed", async () => {
  reset();
  setState({ code: { path: "src/a.ts", from: 3, to: 5 } });
  const x = setup([1]);
  x.fire.facts(delta({ files_changed: ["src/a.ts"] }));
  x.fire.facts(delta({ files_removed: ["src/a.ts"] }));
  await tick();
  assert.equal(x.calls.code, 2);
  assert.equal(x.calls.drawer, 2);
  reset();
});

test("a hello with a newer seq refetches everything on screen; an equal one does nothing", async () => {
  reset();
  setState({ seq: 5 });
  const x = setup([8]);
  x.fire.hello(5);
  await tick();
  assert.deepEqual(x.calls, { overview: 0, drawer: 0, code: 0 });
  x.fire.hello(8);
  await tick();
  assert.deepEqual(x.calls, { overview: 1, drawer: 1, code: 1 });
  reset();
});

test("the stream's state updates live and ends the Connecting state, on open or on a first failure", () => {
  reset();
  const x = setup([1]);
  assert.equal(getState().connecting, true);
  x.fire.state(false);
  assert.equal(getState().live, false);
  assert.equal(getState().connecting, false);
  x.fire.state(true);
  assert.equal(getState().live, true);
  reset();
});

test("a failing refetch is swallowed, so the process survives and the next event can retry", async () => {
  reset();
  const failing = createLive({
    fetchOverview: () => Promise.reject(new Error("down")),
    subscribe: (_hello, facts) => {
      facts(delta());
      return () => {};
    },
    refreshDrawer: () => Promise.reject(new Error("down")),
    reloadCode: () => Promise.reject(new Error("down")),
  });
  failing.start();
  await tick(); // an unhandled rejection would fail the run here
  assert.equal(getState().overview, null);
  reset();
});
