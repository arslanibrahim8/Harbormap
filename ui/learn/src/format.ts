// Small pure helpers for the words and counts the panel shows. No DOM here, so
// `node --test` runs them directly.

import type { Arrow, Box } from "./types.ts";

/** Beginner explains in plain words and shows less; Builder uses the usual terms. */
export type Level = "beginner" | "builder";

const BEGINNER_ARROWS = 10;
const BUILDER_ARROWS = 40;

/** How long ago `ms` (epoch milliseconds) was, as of `now`. */
export function ago(ms: number, now: number): string {
  const d = now - ms;
  if (d < 5_000) return "just now";
  if (d < 60_000) return `${Math.floor(d / 1_000)} s ago`;
  if (d < 3_600_000) return `${Math.floor(d / 60_000)} min ago`;
  return `${Math.floor(d / 3_600_000)} h ago`;
}

/** `1 file`, `2 files`. */
export function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** How many files import a file. */
export function usedFrom(level: Level, n: number): string {
  return level === "beginner"
    ? `Used from ${plural(n, "place", "places")}`
    : plural(n, "importer", "importers");
}

/** How many places call a symbol. */
export function calledFrom(level: Level, n: number): string {
  return level === "beginner"
    ? `Called from ${plural(n, "place", "places")}`
    : plural(n, "caller", "callers");
}

/** The arrows to draw: the server sorts them heaviest first, so a prefix is the
 * most important ones. */
export function arrowsFor(level: Level, arrows: Arrow[]): Arrow[] {
  return arrows.slice(0, level === "beginner" ? BEGINNER_ARROWS : BUILDER_ARROWS);
}

/** The boxes to draw: Beginner keeps only the ones the server flagged. */
export function childrenFor(level: Level, boxes: Box[]): Box[] {
  return level === "beginner" ? boxes.filter((b) => b.beginner) : boxes;
}

/** The last part of a `/`-separated path. */
export function fileName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/** Everything before the last `/`, or an empty string for a top-level file. */
export function dirName(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? "" : path.slice(0, i);
}
