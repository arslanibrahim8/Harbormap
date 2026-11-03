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

/** What the bar says when the server no longer accepts this page's cookie. */
export const SESSION_ENDED = "This panel's session ended. Open the link that `harbormap learn` printed.";

/** The words next to the green dot. */
export function liveLabel(
  live: boolean,
  updatedMs: number,
  now: number,
  connecting = false,
  ended = false,
): string {
  if (ended) return SESSION_ENDED;
  if (connecting) return "Connecting…";
  if (!live) return "Offline — reconnecting";
  return updatedMs === 0 ? "Live" : `Live · updated ${ago(updatedMs, now)}`;
}

/** The level named by a `document.cookie` string, or null. */
export function levelFromCookie(cookie: string): Level | null {
  for (const pair of cookie.split(";")) {
    const [name, value] = pair.trim().split("=");
    if (name === LEVEL_KEY && (value === "beginner" || value === "builder")) return value;
  }
  return null;
}

/** The `document.cookie` assignment that remembers `level` for a year. A cookie is
 * shared by every port on 127.0.0.1, so the choice outlives a run (localStorage is
 * per origin, and the port changes every run). */
export function levelCookie(level: Level): string {
  return `${LEVEL_KEY}=${level}; Path=/; SameSite=Strict; Max-Age=31536000`;
}

/** The level the user chose last time, or Beginner. The cookie is read first;
 * localStorage is a fallback for a browser that keeps it but drops the cookie. */
function savedLevel(): Level {
  try {
    const c = levelFromCookie(document.cookie);
    if (c !== null) return c;
  } catch {
    // cookies blocked: try storage
  }
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
    document.cookie = levelCookie(level);
  } catch {
    // cookies blocked: storage may still work
  }
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
    live.classList.toggle("ended", s.sessionEnded);
    dot.textContent = s.sessionEnded ? "⚠" : "";
    label.textContent = liveLabel(s.live, s.updatedMs, Date.now(), s.connecting, s.sessionEnded);
    dot.className = s.live && !s.sessionEnded ? "dot" : "dot off";
    const said = s.sessionEnded ? SESSION_ENDED : s.connecting ? "" : s.live ? "Live" : "Offline — reconnecting";
    if (announce.textContent !== said) announce.textContent = said;
    for (const [level, b] of buttons) b.setAttribute("aria-pressed", String(s.level === level));
  };
  onChange(draw);
  draw();
  // "updated 12 s ago" has to keep counting while nothing else changes.
  window.setInterval(draw, 1000);
}
