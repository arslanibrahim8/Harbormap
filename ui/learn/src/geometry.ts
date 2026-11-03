// The pure side of the Map view: where a line leaves a box, the curve of an arrow,
// the camera (fit, zoom, pan) and the words on boxes and chips. No DOM and nothing
// that runs at import time, so `node --test` runs it directly.

import type { Level } from "./format.ts";
import { plural } from "./format.ts";
import type { Arrow, Box, Chip } from "./types.ts";

export interface Pt {
  x: number;
  y: number;
}

/** A rectangle; also the shape of an SVG `viewBox`. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The margin `fit` leaves around the boxes, in world units. */
export const FIT_MARGIN = 32;

/** `fit` never zooms in further than this many pixels per world unit, so a map of
 * two boxes is not blown up to the size of the window. */
const FIT_MAX_SCALE = 1.5;

export function centre(b: Rect): Pt {
  return { x: b.x + b.w / 2, y: b.y + b.h / 2 };
}

/** Where the line from the centre of `box` toward `toward` leaves the box. A point
 * at the centre has no direction, so the centre itself is returned. */
export function edgePoint(box: Rect, toward: Pt): Pt {
  const c = centre(box);
  const dx = toward.x - c.x;
  const dy = toward.y - c.y;
  if (dx === 0 && dy === 0) return c;
  const tx = dx === 0 ? Infinity : box.w / 2 / Math.abs(dx);
  const ty = dy === 0 ? Infinity : box.h / 2 / Math.abs(dy);
  const t = Math.min(tx, ty);
  return { x: c.x + dx * t, y: c.y + dy * t };
}

/** The three points of a quadratic curve and the point halfway along it. */
export interface Curve {
  start: Pt;
  ctrl: Pt;
  end: Pt;
  mid: Pt;
}

/** The curve of an arrow from `from` to `to`. The control point sits to the right
 * of the direction of travel, so B→A bows to the other side than A→B and the two
 * never lie on top of each other. */
export function arrowCurve(from: Rect, to: Rect): Curve {
  const a = centre(from);
  const b = centre(to);
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len = Math.hypot(dx, dy) || 1;
  const bow = Math.min(40, Math.max(14, len * 0.12));
  const ctrl = { x: (a.x + b.x) / 2 + (dy / len) * bow, y: (a.y + b.y) / 2 - (dx / len) * bow };
  const start = edgePoint(from, ctrl);
  const end = edgePoint(to, ctrl);
  const mid = {
    x: 0.25 * start.x + 0.5 * ctrl.x + 0.25 * end.x,
    y: 0.25 * start.y + 0.5 * ctrl.y + 0.25 * end.y,
  };
  return { start, ctrl, end, mid };
}

const num = (n: number): string => String(Math.round(n * 10) / 10);

/** The SVG path of an arrow from `from` to `to` (a gentle quadratic curve). */
export function arrowPath(from: Rect, to: Rect): string {
  const { start, ctrl, end } = arrowCurve(from, to);
  return `M${num(start.x)} ${num(start.y)}Q${num(ctrl.x)} ${num(ctrl.y)} ${num(end.x)} ${num(end.y)}`;
}

/** How thick an arrow with `count` references is: it grows by one unit each time the
 * count doubles, and stops at 6. */
export function strokeWidth(count: number): number {
  return Math.min(6, 1.25 + Math.log2(Math.max(1, count)));
}

/** The `viewBox` that shows every box with a margin, centred, with the aspect of
 * the viewport so nothing is letter-boxed. */
export function fit(boxes: Rect[], viewport: { w: number; h: number }): Rect {
  if (boxes.length === 0 || viewport.w <= 0 || viewport.h <= 0) {
    return { x: 0, y: 0, w: Math.max(1, viewport.w), h: Math.max(1, viewport.h) };
  }
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (const b of boxes) {
    x0 = Math.min(x0, b.x);
    y0 = Math.min(y0, b.y);
    x1 = Math.max(x1, b.x + b.w);
    y1 = Math.max(y1, b.y + b.h);
  }
  const cw = x1 - x0 + 2 * FIT_MARGIN;
  const ch = y1 - y0 + 2 * FIT_MARGIN;
  const aspect = viewport.w / viewport.h;
  let w = cw;
  let h = cw / aspect;
  if (h < ch) {
    h = ch;
    w = ch * aspect;
  }
  // Do not zoom in past FIT_MAX_SCALE: grow the box around the same centre instead.
  const minW = viewport.w / FIT_MAX_SCALE;
  if (w < minW) {
    w = minW;
    h = minW / aspect;
  }
  return { x: (x0 + x1) / 2 - w / 2, y: (y0 + y1) / 2 - h / 2, w, h };
}

/** Zoom `view` by `factor` (above 1 shows more of the world) keeping the world
 * point `at` where it is on the screen. */
export function zoomAt(view: Rect, factor: number, at: Pt): Rect {
  return {
    x: at.x - (at.x - view.x) * factor,
    y: at.y - (at.y - view.y) * factor,
    w: view.w * factor,
    h: view.h * factor,
  };
}

/** Keep the width of `view` within `[minW, maxW]` (about its centre); the height
 * follows, so the aspect is kept. */
export function clampZoom(view: Rect, minW: number, maxW: number): Rect {
  const w = Math.min(maxW, Math.max(minW, view.w));
  if (w === view.w) return view;
  const f = w / view.w;
  const c = centre(view);
  return { x: c.x - (view.w * f) / 2, y: c.y - (view.h * f) / 2, w, h: view.h * f };
}

/** The same view moved so that `target` (plus `margin`) is inside it; zoomed out
 * only when the target does not fit at the current zoom. Returns `view` itself when
 * the target is already inside. */
export function ensureVisible(view: Rect, target: Rect, margin: number): Rect {
  const t = { x: target.x - margin, y: target.y - margin, w: target.w + 2 * margin, h: target.h + 2 * margin };
  const inside =
    t.x >= view.x && t.y >= view.y && t.x + t.w <= view.x + view.w && t.y + t.h <= view.y + view.h;
  if (inside) return view;
  let { w, h } = view;
  if (t.w > w || t.h > h) {
    const f = Math.max(t.w / w, t.h / h);
    w *= f;
    h *= f;
  }
  // Slide the smallest distance that brings the target inside.
  let x = view.x + (view.w - w) / 2;
  let y = view.y + (view.h - h) / 2;
  if (t.x < x) x = t.x;
  else if (t.x + t.w > x + w) x = t.x + t.w - w;
  if (t.y < y) y = t.y;
  else if (t.y + t.h > y + h) y = t.y + t.h - h;
  return { x, y, w, h };
}

/** Adapt `view` when the viewport changes from `from` to `to` pixels: the same
 * scale around the same centre, so the map does not jump when the drawer opens. */
export function resizeView(view: Rect, from: { w: number; h: number }, to: { w: number; h: number }): Rect {
  if (from.w <= 0 || from.h <= 0 || to.w <= 0 || to.h <= 0) return view;
  const w = (view.w * to.w) / from.w;
  const h = (view.h * to.h) / from.h;
  const c = centre(view);
  return { x: c.x - w / 2, y: c.y - h / 2, w, h };
}

/** Ease-out cubic on `t` in 0..1. */
export function easeOut(t: number): number {
  const u = 1 - Math.min(1, Math.max(0, t));
  return 1 - u * u * u;
}

export function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

// ---- Chips -----------------------------------------------------------------

export const CHIP_W = 164;
export const CHIP_H = 24;
const CHIP_GAP = 8;
const CHIP_OFFSET = 12;
/** The height of the header the layout leaves at the top of a group. */
export const GROUP_HEADER = 44;

/** Beginner lists this many chips and folds the rest into "+N elsewhere". */
const BEGINNER_CHIPS = 3;

export interface ChipShown {
  /** The chips to draw, in order. */
  chips: Chip[];
  /** N of "+N elsewhere", or 0 for no such chip. */
  more: number;
}

/** The chips a level shows: Beginner the first 3 and "+N elsewhere" (N = the rest
 * plus the server's `elsewhere_more`), Builder all of them. */
export function chipsFor(level: Level, chips: Chip[], elsewhereMore: number): ChipShown {
  if (level === "builder") return { chips, more: elsewhereMore };
  return { chips: chips.slice(0, BEGINNER_CHIPS), more: Math.max(0, chips.length - BEGINNER_CHIPS) + elsewhereMore };
}

/** The slots of `n` chips down the right edge of a group `w` wide, in the group's
 * own coordinates (just outside the edge, below the header). */
export function chipSlots(w: number, n: number): Rect[] {
  const out: Rect[] = [];
  for (let i = 0; i < n; i += 1) {
    out.push({ x: w + CHIP_OFFSET, y: GROUP_HEADER + i * (CHIP_H + CHIP_GAP), w: CHIP_W, h: CHIP_H });
  }
  return out;
}

/** The text of a chip: `→ Data · 7` when only the group uses it, `← Data · 3` when
 * only it uses the group, `↔ Data · 10` when both. */
export function chipLabel(c: Chip): string {
  const arrow = c.out > 0 && c.in > 0 ? "↔" : c.in > 0 ? "←" : "→";
  return `${arrow} ${truncate(c.name, 14)} · ${c.out + c.in}`;
}

// ---- Words -----------------------------------------------------------------

/** `text` cut to `max` characters with an ellipsis. */
export function truncate(text: string, max: number): string {
  return text.length <= max ? text : `${text.slice(0, Math.max(1, max - 1))}…`;
}

const KIND_WORD: Record<string, string> = {
  token: "name group",
  files: "loose files",
  more: "smaller items",
  shared: "shared",
};

export interface BoxLines {
  title: string;
  /** Beginner: the role word. Builder: the folder path (or what kind of group). */
  sub: string;
  meta: string;
}

/** The three lines of text on a box at a level. */
export function boxLines(box: Box, level: Level): BoxLines {
  const meta =
    box.kind === "file"
      ? `${box.loc} lines${box.generated === true ? " · generated" : ""}`
      : plural(box.files, "file", "files");
  if (level === "beginner") return { title: box.name, sub: box.role_word, meta };
  if (box.kind === "file") {
    const p = box.path ?? "";
    const i = p.lastIndexOf("/");
    return { title: box.name, sub: i < 0 ? box.role_word : p.slice(0, i), meta };
  }
  if (box.kind === "folder") {
    return { title: box.name.slice(box.name.lastIndexOf("/") + 1), sub: `${box.name}/`, meta };
  }
  return { title: box.name, sub: KIND_WORD[box.kind] ?? box.role_word, meta };
}

// ---- Live changes ----------------------------------------------------------

/** The ids of the boxes of `next` that changed since `prev`: new ones, ones whose
 * file or line count changed, and ones an arrow to or from which was added,
 * removed or re-counted. */
export function changedBoxes(prev: Box[], prevArrows: Arrow[], next: Box[], nextArrows: Arrow[]): string[] {
  const old = new Map(prev.map((b) => [b.id, b]));
  const out = new Set<string>();
  for (const b of next) {
    const was = old.get(b.id);
    if (was === undefined || was.files !== b.files || was.loc !== b.loc) out.add(b.id);
  }
  const key = (a: Arrow): string => `${a.from}\u0000${a.to}`;
  const before = new Map(prevArrows.map((a) => [key(a), a.count]));
  const after = new Map(nextArrows.map((a) => [key(a), a.count]));
  for (const a of nextArrows) {
    if (before.get(key(a)) !== a.count) {
      out.add(a.from);
      out.add(a.to);
    }
  }
  for (const a of prevArrows) {
    if (!after.has(key(a))) {
      out.add(a.from);
      out.add(a.to);
    }
  }
  const present = new Set(next.map((b) => b.id));
  return [...out].filter((id) => present.has(id)).sort();
}
