// The panel's one store: a plain object and a list of observers. Modules read it
// with `getState`, change it with `setState`, and redraw themselves from `onChange`.
// No DOM here, so `node --test` runs it directly.

import type { Level } from "./format.ts";
import type { Overview } from "./types.ts";

export type Tab = "journeys" | "map" | "code";

export interface State {
  level: Level;
  tab: Tab;
  /** The model version of the overview on screen. */
  seq: number;
  /** When the facts last changed, in epoch milliseconds (0 before the first answer). */
  updatedMs: number;
  /** False while the live stream is down. */
  live: boolean;
  /** True until the stream has either opened or failed once; the bar then says
   * "Connecting…" instead of claiming an outage. */
  connecting: boolean;
  overview: Overview | null;
  /** Ids of the expanded groups, outermost first. */
  expanded: string[];
  selected: { path: string; symbol: string | null } | null;
  /** The file shown in the Code tab; `from` and `to` are 1-based, 0 when no range. */
  code: { path: string; from: number; to: number } | null;
}

let state: State = {
  level: "beginner",
  tab: "map",
  seq: 0,
  updatedMs: 0,
  live: false,
  connecting: true,
  overview: null,
  expanded: [],
  selected: null,
  code: null,
};

const observers: ((s: State, prev: State) => void)[] = [];

/** The current state. Treat it as read-only: change it with `setState`. */
export function getState(): State {
  return state;
}

/** Merge `patch` into the state and tell every observer, with the previous state.
 * A patch that changes no value (compared with `===`) tells nobody. */
export function setState(patch: Partial<State>): void {
  const prev = state;
  const keys = Object.keys(patch) as (keyof State)[];
  if (keys.every((k) => patch[k] === prev[k])) return;
  state = { ...prev, ...patch };
  for (const fn of observers) fn(state, prev);
}

/** Call `fn` after every change of the state. */
export function onChange(fn: (s: State, prev: State) => void): void {
  observers.push(fn);
}

/** The view a card asks links for: the innermost expanded group, or the overview. */
export function currentView(): string {
  return state.expanded[state.expanded.length - 1] ?? "overview";
}
