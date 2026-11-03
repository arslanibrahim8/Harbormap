// Keeps the state current: the first overview, and the server-sent events that say
// the facts changed.

import { overview as fetchOverview, subscribe } from "./api.ts";
import { reloadCode } from "./code.ts";
import { drawerPath, refreshDrawer } from "./drawer.ts";
import { getState, setState } from "./state.ts";
import type { FactsDelta } from "./types.ts";

let request = 0;

/** Fetch the overview into the state. A response older than the one on screen, or
 * overtaken by a newer request, is dropped. Rejects when the server cannot be
 * reached. */
export async function loadOverview(): Promise<void> {
  const mine = ++request;
  const o = await fetchOverview();
  if (mine !== request || o.seq < getState().seq) return;
  setState({ overview: o, seq: o.seq, updatedMs: o.updated_ms });
}

function quietly(p: Promise<void>): void {
  p.catch(() => {
    // the next event retries; the panel keeps what it shows
  });
}

/** Open the event stream. Returns a function that closes it. */
export function startLive(): () => void {
  return subscribe(
    (seq) => {
      // A reconnect that finds newer facts missed some deltas: refresh everything
      // that is on screen, since there is no list of what changed.
      if (seq > getState().seq) {
        quietly(loadOverview());
        quietly(refreshDrawer());
        quietly(reloadCode());
      }
    },
    (d: FactsDelta) => {
      setState({ updatedMs: d.at_ms });
      quietly(loadOverview());
      const changed = new Set([...d.files_changed, ...d.files_removed]);
      const open = drawerPath();
      if (open !== null && changed.has(open)) quietly(refreshDrawer());
      const code = getState().code;
      if (code !== null && changed.has(code.path)) quietly(reloadCode());
    },
    (live) => setState({ live }),
  );
}
