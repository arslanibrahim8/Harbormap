// The technologies strip above the map: one chip per category the server reports.
// Until a later level fills /api/tech the list is empty and the strip stays hidden.

import { tech as fetchTech } from "./api.ts";
import { byId, clear, h } from "./dom.ts";

/** The words of one category. The server's shape is not fixed yet, so a plain string
 * is shown as is, an object by its `name`, `label` or `title`, anything else not at
 * all. */
export function categoryLabel(c: unknown): string {
  if (typeof c === "string") return c;
  if (typeof c === "object" && c !== null) {
    const o = c as Record<string, unknown>;
    for (const key of ["name", "label", "title"]) {
      const v = o[key];
      if (typeof v === "string" && v !== "") return v;
    }
  }
  return "";
}

/** Fill the strip from a list of categories; it is hidden when nothing is left to
 * show. */
export function drawTechStrip(strip: HTMLElement, categories: unknown[]): void {
  const labels = categories.map(categoryLabel).filter((l) => l !== "");
  clear(strip);
  strip.append(...labels.map((l) => h("span", { class: "tech-chip" }, l)));
  strip.hidden = labels.length === 0;
}

/** Fetch the categories once and draw them. A failed fetch leaves the strip hidden:
 * it is decoration, not something to report. */
export function initTechStrip(): void {
  const strip = byId("tech-strip");
  strip.hidden = true;
  fetchTech().then(
    (t) => drawTechStrip(strip, t.categories),
    () => {
      strip.hidden = true;
    },
  );
}
