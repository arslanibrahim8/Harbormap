// The groups the map has fetched, with the rules for when one may still be shown.
// The cache exists only so that expanding a group again does not flicker; the
// server stays the authority, so an entry is dropped when a facts delta names its
// group, everything goes on a regroup, and an entry older than the overview is
// fetched again. No DOM here, so `node --test` runs it directly.

import type { GroupView } from "./types.ts";

/** The part of a facts delta the cache reads. */
export interface Invalidation {
  regrouped: boolean;
  groups_changed: string[];
}

export function createGroupCache() {
  const entries = new Map<string, GroupView>();
  let version = 0;

  return {
    get: (id: string): GroupView | undefined => entries.get(id),
    set(id: string, g: GroupView): void {
      entries.set(id, g);
    },
    delete(id: string): void {
      entries.delete(id);
    },
    get size(): number {
      return entries.size;
    },
    /** Bumped by every invalidation, so a fetch that was in flight while the facts
     * changed can tell its answer may already be old. */
    get version(): number {
      return version;
    },
    /** True when `id` must be fetched before it is shown: it is not held, or what is
     * held is older than the overview (`seq`). This is asked when a group is opened;
     * the groups already open are refreshed by the facts deltas and, after a
     * reconnect that missed some, by the map's resync. */
    needsFetch(id: string, seq: number): boolean {
      const g = entries.get(id);
      return g === undefined || g.seq < seq;
    },
    /** A facts delta arrived: drop the entries it names, or all of them after a
     * regroup. `keep` are the groups on screen; they are refetched by the caller, so
     * they stay until their new answer (or a 404) replaces them. */
    invalidate(d: Invalidation, keep: readonly string[]): void {
      if (!d.regrouped && d.groups_changed.length === 0) return;
      version += 1;
      const kept = new Set(keep);
      const drop = d.regrouped ? [...entries.keys()] : d.groups_changed;
      for (const id of drop) if (!kept.has(id)) entries.delete(id);
    },
  };
}

export type GroupCache = ReturnType<typeof createGroupCache>;
