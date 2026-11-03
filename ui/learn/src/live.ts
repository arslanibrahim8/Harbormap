// Keeps the state current: the first overview, and the server-sent events that say
// the facts changed. `createLive` takes its collaborators as arguments so the rules
// run under `node --test`; the exports at the bottom wire the real ones.

import { overview as fetchOverview, subscribe } from "./api.ts";
import { reloadCode } from "./code.ts";
import { refreshDrawer } from "./drawer.ts";
import { getState, setState } from "./state.ts";
import type { FactsDelta, Overview } from "./types.ts";

export interface LiveDeps {
  fetchOverview(): Promise<Overview>;
  subscribe: typeof subscribe;
  refreshDrawer(): Promise<void>;
  reloadCode(): Promise<void>;
}

export function createLive(deps: LiveDeps) {
  let request = 0;
  const factListeners: ((d: FactsDelta) => void)[] = [];

  /** Fetch the overview into the state. A response older than the one on screen, or
   * overtaken by a newer request, is dropped. Rejects when the server cannot be
   * reached. */
  async function loadOverview(): Promise<void> {
    const mine = ++request;
    const o = await deps.fetchOverview();
    if (mine !== request || o.seq < getState().seq) return;
    setState({ overview: o, seq: o.seq, updatedMs: o.updated_ms });
  }

  function quietly(p: Promise<void>): void {
    p.catch(() => {
      // the next event retries; the panel keeps what it shows
    });
  }

  /** Open the event stream. Returns a function that closes it. */
  function start(): () => void {
    return deps.subscribe(
      (seq) => {
        // A reconnect that finds newer facts missed some deltas: refresh everything
        // that is on screen, since there is no list of what changed.
        if (seq > getState().seq) {
          quietly(loadOverview());
          quietly(deps.refreshDrawer());
          quietly(deps.reloadCode());
        }
      },
      (d: FactsDelta) => {
        setState({ updatedMs: d.at_ms });
        for (const fn of factListeners) fn(d);
        quietly(loadOverview());
        // Any file's change can alter the open card ("Who uses it", the callers),
        // so it is fetched on every delta; the Code tab only when its own file moved.
        quietly(deps.refreshDrawer());
        const code = getState().code;
        if (code !== null && (d.files_changed.includes(code.path) || d.files_removed.includes(code.path))) {
          quietly(deps.reloadCode());
        }
      },
      (live) => setState({ live, connecting: false }),
    );
  }

  /** Call `fn` with every facts delta (the map refetches the groups it shows). */
  function onFacts(fn: (d: FactsDelta) => void): void {
    factListeners.push(fn);
  }

  return { loadOverview, start, onFacts };
}

const live = createLive({ fetchOverview, subscribe, refreshDrawer, reloadCode });
export const loadOverview = live.loadOverview;
export const startLive = live.start;
export const onFacts = live.onFacts;
