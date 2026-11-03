// The top bar: the Harbormap Learn mark, the project, where it is in git, whether the
// live stream is up, and the Beginner/Builder switch.

import { byId, clear, h } from "./dom.ts";
import { ago } from "./format.ts";
import type { Level } from "./format.ts";
import { getState, onChange, setState } from "./state.ts";
import type { Repo } from "./types.ts";

const LEVEL_KEY = "hm_level";

/** `branch@commit`; just the part that exists when one is missing; `no git` when
 * both are. */
export function repoChip(repo: Repo): string {
  if (repo.branch !== null && repo.commit !== null) return `${repo.branch}@${repo.commit}`;
  return repo.branch ?? repo.commit ?? "no git";
}

/** The words next to the green dot. */
export function liveLabel(live: boolean, updatedMs: number, now: number, connecting = false): string {
  if (connecting) return "Connecting…";
  if (!live) return "Offline — reconnecting";
  return updatedMs === 0 ? "Live" : `Live · updated ${ago(updatedMs, now)}`;
}

/** The level the user chose last time, or Beginner. */
function savedLevel(): Level {
  try {
    const v = localStorage.getItem(LEVEL_KEY);
    if (v === "beginner" || v === "builder") return v;
  } catch {
    // storage blocked: the default stands
  }
  return "beginner";
}

function saveLevel(level: Level): void {
  try {
    localStorage.setItem(LEVEL_KEY, level);
  } catch {
    // storage blocked: the choice lasts until the page closes
  }
}

/** Build the bar and keep it in step with the state. */
export function initTopbar(): void {
  setState({ level: savedLevel() });

  const name = h("span", { class: "project" });
  const chip = h("span", { class: "chip mono" });
  const dot = h("i", { class: "dot", "aria-hidden": "true" });
  const label = h("span");
  // The visible label changes every second; this one changes only when the
  // connection does, so a screen reader is not interrupted by a clock.
  const announce = h("span", { class: "sr-only", role: "status" });
  const live = h("div", { class: "live" }, dot, label, announce);
  const seg = h("div", { class: "seg", role: "group", "aria-label": "Level" });
  const buttons = new Map<Level, HTMLElement>();
  for (const [level, text] of [["beginner", "Beginner"], ["builder", "Builder"]] as const) {
    const b = h("button", { type: "button" }, text);
    b.addEventListener("click", () => {
      setState({ level });
      saveLevel(level);
    });
    buttons.set(level, b);
    seg.append(b);
  }

  const bar = byId("topbar");
  clear(bar);
  bar.append(h("span", { class: "mark" }, "Harbormap Learn"), name, chip, live, seg);

  const draw = (): void => {
    const s = getState();
    name.textContent = s.overview?.repo.name ?? "";
    chip.textContent = s.overview ? repoChip(s.overview.repo) : "";
    chip.hidden = s.overview === null;
    label.textContent = liveLabel(s.live, s.updatedMs, Date.now(), s.connecting);
    dot.className = s.live ? "dot" : "dot off";
    const said = s.connecting ? "" : s.live ? "Live" : "Offline — reconnecting";
    if (announce.textContent !== said) announce.textContent = said;
    for (const [level, b] of buttons) b.setAttribute("aria-pressed", String(s.level === level));
  };
  onChange(draw);
  draw();
  // "updated 12 s ago" has to keep counting while nothing else changes.
  window.setInterval(draw, 1000);
}
