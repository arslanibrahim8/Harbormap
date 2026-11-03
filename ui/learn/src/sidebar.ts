// The left column: search, what the map leaves out, and the journeys note.

import { search as fetchSearch } from "./api.ts";
import { byId, clear, h, isTyping } from "./dom.ts";
import { plural } from "./format.ts";
import { openFile, openSymbol } from "./drawer.ts";
import { currentView, getState, onChange } from "./state.ts";
import { JOURNEYS_NOTE } from "./tabs.ts";
import type { SearchResult } from "./types.ts";

const DEBOUNCE_MS = 120;

/** What a class of file is called in the list of files the map leaves out. */
function classWord(cls: string): string {
  return cls === "test" ? "tests" : cls === "doc" ? "docs" : cls;
}

function openHit(r: SearchResult): void {
  if (r.kind === "symbol" && r.symbol !== null) void openSymbol(r.path, r.symbol);
  else void openFile(r.path, currentView());
}

export function initSidebar(): void {
  const input = h("input", {
    type: "search",
    placeholder: "Find a file or function",
    "aria-label": "Search files and functions",
    autocomplete: "off",
    spellcheck: "false",
  }) as HTMLInputElement;
  const box = h("label", { class: "search" }, input, h("kbd", {}, "/"));
  const results = h("ul", { class: "rows results" });
  const excluded = h("ul", { class: "rows" });

  const side = byId("sidebar");
  clear(side);
  side.append(
    h("div", { class: "side-top" }, box, results),
    h("section", { class: "sec" }, h("h4", {}, "Not on this map"), excluded),
    h("section", { class: "sec" }, h("h4", {}, "Journeys"), h("p", { class: "none" }, JOURNEYS_NOTE)),
  );

  // Only the newest search may draw: a slow answer for an old query is dropped.
  let latest = 0;
  async function run(q: string): Promise<SearchResult[]> {
    const mine = ++latest;
    clear(results);
    if (q === "") return [];
    try {
      const found = (await fetchSearch(q)).results;
      if (mine !== latest) return [];
      if (found.length === 0) results.append(h("li", { class: "none" }, "No match."));
      for (const r of found) {
        const b = h(
          "button",
          { type: "button", class: "item" },
          h("span", { class: "t" }, r.label),
          h("span", { class: "n" }, r.kind === "symbol" ? r.path : ""),
        );
        b.addEventListener("click", () => openHit(r));
        results.append(h("li", {}, b));
      }
      return found;
    } catch {
      if (mine === latest) results.append(h("li", { class: "none problem" }, "⚠ Search failed."));
      return [];
    }
  }

  let timer: number | undefined;
  input.addEventListener("input", () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => void run(input.value.trim()), DEBOUNCE_MS);
  });
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      window.clearTimeout(timer);
      void run(input.value.trim()).then((found) => {
        const first = found[0];
        if (first !== undefined) openHit(first);
      });
    } else if (e.key === "Escape") {
      window.clearTimeout(timer);
      input.value = "";
      void run("");
      input.blur();
    }
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "/" && !isTyping(e.target) && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      input.focus();
    }
  });

  const drawExcluded = (): void => {
    clear(excluded);
    const list = getState().overview?.excluded ?? [];
    if (list.length === 0) excluded.append(h("li", { class: "none" }, "Every file is on the map."));
    for (const x of list) {
      excluded.append(
        h("li", { class: "item static" }, h("span", { class: "t" }, `${classWord(x.class)} · ${plural(x.files, "file", "files")}`)),
      );
    }
  };
  onChange((s, prev) => {
    if (s.overview !== prev.overview) drawExcluded();
  });
  drawExcluded();
}
