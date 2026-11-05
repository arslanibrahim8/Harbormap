// The three tabs above the centre. The panes stay in the page; the inactive ones are
// hidden.

import { byId, clear, h } from "./dom.ts";
import { getState, onChange, setState } from "./state.ts";
import type { Tab } from "./state.ts";

const TABS: [Tab, string][] = [
  ["journeys", "Journeys"],
  ["map", "Map"],
  ["code", "Code"],
];

/** The note a Journeys view shows until journeys exist. */
export const JOURNEYS_NOTE = "Journeys arrive in the next version";

export function initTabs(): void {
  const nav = byId("tabs");
  clear(nav);
  nav.setAttribute("role", "tablist");
  const buttons = new Map<Tab, HTMLElement>();
  for (const [tab, text] of TABS) {
    const b = h("button", { type: "button", role: "tab", class: "tab" }, text);
    b.addEventListener("click", () => setState({ tab }));
    buttons.set(tab, b);
    nav.append(b);
  }

  const journeys = byId("pane-journeys");
  journeys.append(
    h("p", { class: "empty" }, JOURNEYS_NOTE),
    h("p", { class: "empty-sub" }, "The map and the cards already work: open the Map tab and click a box."),
  );

  const draw = (): void => {
    const current = getState().tab;
    for (const [tab] of TABS) {
      const on = tab === current;
      buttons.get(tab)?.setAttribute("aria-selected", String(on));
      byId(`pane-${tab}`).hidden = !on;
    }
  };
  onChange((s, prev) => {
    if (s.tab !== prev.tab) draw();
  });
  draw();
}
