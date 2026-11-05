// The Map view, drawn in SVG: the overview boxes in role lanes with counted arrows,
// the in-place drill-down (a box grows over its dimmed neighbours and shows its
// children), "+N elsewhere" chips, the links of the selected file, live pulses and
// the camera. The pure geometry lives in geometry.ts; this file holds the DOM.
//
// Coordinates: overview boxes are in world units; the children of a group are
// relative to the group's top-left corner, so each level of expansion is one `<g>`
// translated to that corner.

import { ApiError, group as fetchGroup } from "./api.ts";
import { createCamera } from "./camera.ts";
import { clear, h, isTyping, notify } from "./dom.ts";
import { closeDrawer, currentCard, onCard, openFile } from "./drawer.ts";
import { arrowsFor, childrenFor } from "./format.ts";
import type { Level } from "./format.ts";
import {
  CHIP_H,
  CHIP_W,
  GROUP_HEADER,
  arrowCurve,
  arrowPath,
  boxLines,
  changedBoxes,
  chipLabel,
  chipSlots,
  chipsFor,
  easeOut,
  lerp,
  strokeWidth,
  truncate,
} from "./geometry.ts";
import type { Pt, Rect } from "./geometry.ts";
import { createGroupCache } from "./groupcache.ts";
import { onFacts, onResync } from "./live.ts";
import { GROW_MS, animate, reducedMotion } from "./motion.ts";
import { currentView, getState, onChange, setState } from "./state.ts";
import { defs, s, text, title } from "./svg.ts";
import type { Arrow, Box, Chip, GroupView } from "./types.ts";

/** A pulse of a changed box. */
const PULSE_MS = 300;
/** Opacity of everything an expanded box covers. */
const DIM = 0.35;

/** One level of the picture: the overview, or one expanded group. */
interface Layer {
  /** Where this level's (0, 0) is in the world. */
  origin: Pt;
  /** The group, or null for the overview. */
  group: GroupView | null;
  /** Every box of the level, whatever the level of detail. */
  all: Box[];
  /** The boxes the current level of detail shows. */
  shown: Box[];
  arrows: Arrow[];
  hiddenArrows: number;
  /** The box of this level that is expanded, if any. */
  expandedId: string | null;
}

/** The ids a box element is registered under, for pulses. */
interface Registered {
  el: SVGElement;
  dim: boolean;
}

/** Draw the map into `root` and keep it in step with the state. */
export function renderMap(root: HTMLElement): void {
  const svg = s("svg", { class: "map-svg", role: "group", "aria-label": "Map of the project" }) as SVGSVGElement;
  const scene = s("g", { class: "scene" });
  svg.append(defs(), scene);
  const crumbs = h("nav", { class: "crumbs", "aria-label": "Where you are in the map", hidden: "" });
  const note = h("div", { class: "map-note", role: "status", hidden: "" });

  // ---- State of the picture ------------------------------------------------

  const cache = createGroupCache();
  /** Bumped by every drawing; animations of an older drawing stop. */
  let drawing = 0;
  let registered = new Map<string, Registered>();
  /** The expansion the last drawing showed, to tell growth from a refresh. */
  let drawnExpanded: string[] = [];
  /** The growth in progress: which expansion, and when it began, so a redraw in the
   * middle of it carries the animation on instead of cutting it short. */
  let growth: { key: string; t0: number } | null = null;
  /** The links of the card that the last drawing showed (see `linksKey`). */
  let drawnLinks = "";
  /** The file whose card was last checked for being out of sight (see `maybeReveal`). */
  let settled: string | null = null;
  /** Bumped by every request to expand, so only the newest one lands. */
  let expandRequest = 0;
  /** Bumped by every refetch of the expanded groups; only the newest one applies. */
  let refetchRun = 0;
  /** True when a refetch failed and the next facts delta must try again. */
  let retryNext = false;

  const camera = createCamera({ root, svg, rects: () => getState().overview?.boxes ?? [] });

  svg.addEventListener("click", (e) => {
    // A click on the empty map (not on a box or a chip) clears the selection.
    if ((e.target as Element).closest("[data-act]") === null && getState().selected !== null) closeDrawer();
  });

  /** Make `el` do `fn` on a click, and on Enter or Space when it has the focus. */
  const activatable = (el: Element, fn: () => void): void => {
    el.addEventListener("click", fn);
    el.addEventListener("keydown", (e) => {
      const k = e as KeyboardEvent;
      if (k.key === "Enter" || k.key === " ") {
        k.preventDefault();
        fn();
      }
    });
  };

  // ---- Expanding and collapsing ---------------------------------------------

  /** After the view changed, the open file card is asked again, so its links are
   * counted in the boxes that are visible now. */
  const reopenCard = (): void => {
    const sel = getState().selected;
    if (sel !== null && sel.symbol === null) {
      // The user moved the view: the card must not move it back to reveal the file.
      settled = sel.path;
      void openFile(sel.path, currentView());
    }
  };

  const sameIds = (a: string[], b: string[]): boolean => a.length === b.length && a.every((x, i) => x === b[i]);

  /** Show the groups `ids` (outermost first): fetch the ones that are not held or
   * are older than the overview, then switch. */
  async function expandTo(ids: string[]): Promise<void> {
    if (sameIds(ids, getState().expanded)) return;
    const mine = ++expandRequest;
    const version = cache.version;
    const seq = getState().seq;
    try {
      await Promise.all(
        ids
          .filter((id) => cache.needsFetch(id, seq))
          .map(async (id) => {
            cache.set(id, await fetchGroup(id));
          }),
      );
    } catch (e) {
      if (mine !== expandRequest) return;
      notify(e instanceof ApiError && e.status === 404 ? "That group is gone from the map." : "Could not open that group.");
      return;
    }
    if (mine !== expandRequest) return;
    setState({ expanded: ids });
    reopenCard();
    // The facts changed while the groups were on their way: they may be old already.
    if (cache.version !== version) void refetchGroups();
  }

  /** Go back to the first `n` levels of the expansion. */
  function collapseTo(n: number): void {
    expandRequest += 1;
    const cur = getState().expanded;
    if (n >= cur.length) return;
    setState({ expanded: cur.slice(0, n) });
    reopenCard();
  }

  function activate(box: Box, layer: number): void {
    if (box.kind === "file" && box.path !== undefined) {
      settled = box.path;
      void openFile(box.path, currentView());
    } else if (box.expandable) {
      void expandTo([...getState().expanded.slice(0, layer), box.id]);
    }
  }

  // ---- Building the layers -----------------------------------------------------

  function buildLayers(level: Level): Layer[] {
    const o = getState().overview;
    if (o === null) return [];
    const expanded = getState().expanded;
    const layers: Layer[] = [
      {
        origin: { x: 0, y: 0 },
        group: null,
        all: o.boxes,
        shown: o.boxes,
        arrows: o.arrows,
        hiddenArrows: o.hidden_arrows,
        expandedId: expanded[0] ?? null,
      },
    ];
    for (let i = 0; i < expanded.length; i += 1) {
      const parent = layers[i];
      const box = parent?.all.find((b) => b.id === expanded[i]);
      const g = cache.get(expanded[i] ?? "");
      if (parent === undefined || box === undefined || g === undefined) break;
      layers.push({
        origin: { x: parent.origin.x + box.x, y: parent.origin.y + box.y },
        group: g,
        all: g.children,
        shown: childrenFor(level, g.children, getState().selected?.path ?? null),
        arrows: g.arrows,
        hiddenArrows: g.hidden_arrows,
        expandedId: expanded[i + 1] ?? null,
      });
    }
    return layers;
  }

  /** The arrows of a level to draw, and how many of its links stay out of sight. A
   * link that touches a file is drawn only for the selected file, so file nodes are
   * left out here; the level's own limit applies after that. */
  function visibleArrows(layer: Layer, level: Level): { arrows: Arrow[]; hidden: number } {
    const shownIds = new Set(layer.shown.map((b) => b.id));
    const fileIds = new Set(layer.all.filter((b) => b.kind === "file").map((b) => b.id));
    const candidates = layer.arrows.filter(
      (a) =>
        shownIds.has(a.from) &&
        shownIds.has(a.to) &&
        !fileIds.has(a.from) &&
        !fileIds.has(a.to) &&
        a.from !== layer.expandedId &&
        a.to !== layer.expandedId,
    );
    const arrows = arrowsFor(level, candidates);
    return { arrows, hidden: layer.hiddenArrows + (candidates.length - arrows.length) };
  }

  // ---- Drawing -----------------------------------------------------------------

  function boxEl(box: Box, layerIndex: number, level: Level, compact: boolean, dim: boolean): SVGElement {
    const sel = getState().selected;
    const selected = box.kind === "file" && sel !== null && box.path === sel.path;
    const lines = boxLines(box, level);
    const g = s("g", {
      class: `box role-${box.role}${selected ? " sel" : ""}${box.generated === true ? " generated" : ""}${
        box.expandable && box.kind !== "file" ? " expandable" : ""
      }`,
      transform: `translate(${box.x} ${box.y})`,
      "data-id": box.id,
      "data-act": "box",
      tabindex: 0,
      role: "button",
      "aria-label": `${lines.title}, ${lines.sub}, ${lines.meta}`,
    });
    const tip = box.kind === "file" && box.path !== undefined ? box.path : `${lines.title} · ${lines.sub} · ${lines.meta}`;
    g.append(
      title(tip),
      s("rect", { class: "card", width: box.w, height: box.h, rx: 9 }),
      s("rect", { class: "bar", x: 0, y: 8, width: 4, height: box.h - 16, rx: 2 }),
    );
    const titleChars = Math.floor((box.w - 30) / 7.4);
    const subChars = Math.floor((box.w - 30) / 6.2);
    if (compact) {
      g.append(
        text(16, 22, "t", truncate(lines.title, titleChars)),
        // The count stays whole; the longer first part is what gets cut.
        text(16, 40, "s", `${truncate(lines.sub, Math.max(4, subChars - lines.meta.length - 3))} · ${lines.meta}`),
      );
    } else {
      g.append(
        text(16, 26, "t", truncate(lines.title, titleChars)),
        text(16, 45, "s", truncate(lines.sub, subChars)),
        text(16, 62, "m", truncate(lines.meta, subChars)),
      );
    }
    if (dim) g.classList.add("dim");
    activatable(g, () => activate(box, layerIndex));
    return g;
  }

  /** The arrow path and its count, for each arrow whose two boxes are known. */
  function arrowEls(arrows: Arrow[], rects: Map<string, Rect>, cls: string, marker: string): SVGElement[] {
    const out: SVGElement[] = [];
    for (const a of arrows) {
      const from = rects.get(a.from);
      const to = rects.get(a.to);
      if (from === undefined || to === undefined) continue;
      const mid = arrowCurve(from, to).mid;
      const g = s("g", { class: cls });
      g.append(
        s("path", { d: arrowPath(from, to), "stroke-width": strokeWidth(a.count), "marker-end": `url(#${marker})` }),
        text(mid.x, mid.y + 4, "arrow-n", String(a.count)),
      );
      out.push(g);
    }
    return out;
  }

  /** The links of the open file card, as arrows from or to its box. */
  function selectionArrows(layer: Layer, rects: Map<string, Rect>, chipRects: Map<string, Rect>): SVGElement[] {
    const sel = getState().selected;
    const payload = currentCard();
    if (sel === null || payload === null || payload.kind !== "file" || payload.card.path !== sel.path) return [];
    const links = payload.links;
    if (links === null || links.view !== currentView()) return [];
    const node = layer.shown.find((b) => b.kind === "file" && b.path === sel.path);
    if (node === undefined) return [];
    const all = new Map([...rects, ...chipRects]);
    const mine: Arrow[] = [
      ...links.outgoing.map((l) => ({ from: node.id, to: l.id, count: l.count })),
      ...links.incoming.map((l) => ({ from: l.id, to: node.id, count: l.count })),
    ].filter((a) => a.from !== a.to);
    return arrowEls(mine, all, "arrow ink", "head-sel");
  }

  function chipEls(group: GroupView, level: Level): { els: SVGElement[]; rects: Map<string, Rect> } {
    const shown = chipsFor(level, group.elsewhere, group.elsewhere_more);
    const slots = chipSlots(group.w, shown.chips.length + (shown.more > 0 ? 1 : 0));
    const els: SVGElement[] = [];
    const rects = new Map<string, Rect>();
    shown.chips.forEach((c: Chip, i) => {
      const r = slots[i];
      if (r === undefined) return;
      rects.set(c.id, r);
      const g = s("g", {
        class: `mchip role-${c.role}`,
        transform: `translate(${r.x} ${r.y})`,
        "data-act": "chip",
        tabindex: 0,
        role: "button",
        "aria-label": `${c.name}: ${c.out} out, ${c.in} in`,
      });
      g.append(
        title(`${c.name}: ${c.out} links out of this group, ${c.in} in`),
        s("rect", { width: r.w, height: r.h, rx: r.h / 2 }),
        text(12, 16, "t", chipLabel(c)),
      );
      activatable(g, () => void expandTo([c.id]));
      els.push(g);
    });
    const last = slots[shown.chips.length];
    if (shown.more > 0 && last !== undefined) {
      const g = s("g", { class: "mchip etc", transform: `translate(${last.x} ${last.y})` });
      g.append(s("rect", { width: last.w, height: last.h, rx: last.h / 2 }), text(12, 16, "t", `+${shown.more} elsewhere`));
      els.push(g);
    }
    return { els, rects };
  }

  /** What the caller needs after a drawing. */
  interface Drawn {
    /** The rectangle (in the world) of the innermost group and its chips. */
    focus: Rect | null;
    /** True when the innermost group began to grow in this drawing. */
    fresh: boolean;
  }

  /** Boxes that changed lately and when, so a redraw in the middle of a pulse (the
   * card refreshes right after the overview) carries it on instead of ending it. */
  const recent = new Map<string, number>();

  function draw(): Drawn {
    const level = getState().level;
    const layers = buildLayers(level);
    const mine = ++drawing;
    const alive = (): boolean => mine === drawing;
    registered = new Map();
    scene.replaceChildren();
    if (layers.length === 0) return { focus: null, fresh: false };

    const expanded = getState().expanded.slice(0, layers.length - 1);
    // Growth, not a refresh: the innermost level is new, or a different group.
    const last = expanded.length - 1;
    const now = performance.now();
    const fresh = last >= 0 && drawnExpanded[last] !== expanded[last];
    if (fresh) growth = { key: expanded.join("/"), t0: now };
    // A redraw within the 240 ms of a growth continues it from where it is.
    const growing = !reducedMotion() && growth !== null && growth.key === expanded.join("/") && now - growth.t0 < GROW_MS;
    drawnExpanded = expanded;
    drawnLinks = linksKey();

    const innermost = layers.length - 1;
    const dimsByLayer: SVGElement[][] = [];
    let grow: ((p: number) => void) | null = null;
    let focus: Rect | null = null;
    let hiddenLinks = 0;

    layers.forEach((layer, li) => {
      const lg = s("g", { class: `layer${layer.group !== null ? ` role-${layer.group.role}` : ""}`, transform: `translate(${layer.origin.x} ${layer.origin.y})` });
      scene.append(lg);
      const grp = layer.group;
      const active = li === innermost;
      let content: SVGElement = lg;
      let frame: SVGElement | null = null;
      let head: SVGElement | null = null;
      if (grp !== null) {
        frame = s("rect", { class: `frame role-${grp.role}`, width: grp.w, height: grp.h, rx: 12 });
        head = s("g", { class: "frame-head" });
        head.append(
          text(18, 28, "t", truncate(grp.name, Math.floor((grp.w - 36) / 8))),
          text(grp.w - 18, 28, "s end", grp.role_word),
        );
        content = s("g", { class: "content" });
        lg.append(frame, head, content);
      }

      const { arrows, hidden } = visibleArrows(layer, level);
      const rects = new Map<string, Rect>(layer.shown.map((b) => [b.id, b]));
      const dims: SVGElement[] = [];
      dimsByLayer[li] = dims;
      const expandedHere = layer.expandedId !== null;
      const chips = active && grp !== null ? chipEls(grp, level) : null;
      const links = active ? selectionArrows(layer, rects, chips?.rects ?? new Map()) : [];

      const arrowLayer = s("g", { class: "arrows" });
      for (const a of arrowEls(arrows, rects, "arrow", "head")) {
        if (links.length > 0) a.classList.add("faded");
        if (expandedHere) dims.push(a);
        arrowLayer.append(a);
      }
      // The open file's links come last, so they sit on top of the level's own arrows.
      arrowLayer.append(...links);
      content.append(arrowLayer);

      for (const b of layer.shown) {
        if (b.id === layer.expandedId) continue;
        const el = boxEl(b, li, level, grp !== null, expandedHere);
        if (expandedHere) dims.push(el);
        registered.set(b.id, { el, dim: expandedHere });
        content.append(el);
      }
      if (chips !== null) content.append(...chips.els);
      if (active) hiddenLinks = hidden;

      // Dim now, or leave the opacity to the growth that follows.
      const dimNow = growing && li === innermost - 1 ? 1 : DIM;
      for (const el of dims) el.setAttribute("opacity", String(dimNow));

      if (active && grp !== null && frame !== null && head !== null) {
        const box = layers[li - 1]?.all.find((b) => b.id === grp.id);
        const stack = chips === null ? 0 : chips.els.length * (CHIP_H + 8);
        focus = {
          x: layer.origin.x,
          y: layer.origin.y,
          w: grp.w + (chips !== null && chips.els.length > 0 ? CHIP_W + 12 : 0),
          h: Math.max(grp.h, GROUP_HEADER + stack),
        };
        if (growing && box !== undefined) {
          const outer = dimsByLayer[li - 1] ?? [];
          const f = frame;
          const hd = head;
          const ct = content;
          grow = (p) => {
            const e = easeOut(p);
            f.setAttribute("width", String(lerp(box.w, grp.w, e)));
            f.setAttribute("height", String(lerp(box.h, grp.h, e)));
            ct.setAttribute("opacity", String(e));
            hd.setAttribute("opacity", String(e));
            for (const d of outer) d.setAttribute("opacity", String(lerp(1, DIM, e)));
          };
        }
      }
    });

    // Boxes that changed lately pulse (once, or the rest of it after a redraw).
    for (const [id, t0] of recent) {
      if (now - t0 >= PULSE_MS) recent.delete(id);
      const r = registered.get(id);
      if (r !== undefined && !r.dim && now - t0 < PULSE_MS) {
        const el = r.el;
        animate(
          PULSE_MS,
          (p) => (p >= 1 ? el.removeAttribute("opacity") : el.setAttribute("opacity", String(lerp(0.6, 1, p)))),
          alive,
          t0,
        );
      }
    }

    if (hiddenLinks > 0) {
      note.textContent = `${hiddenLinks} more ${hiddenLinks === 1 ? "link" : "links"} hidden`;
      note.hidden = false;
    } else {
      note.hidden = true;
    }
    if (grow !== null && growth !== null) animate(GROW_MS, grow, alive, growth.t0);
    return { focus, fresh };
  }

  function breadcrumbs(): void {
    const expanded = getState().expanded;
    const names: string[] = [];
    for (const id of expanded) {
      const g = cache.get(id);
      if (g === undefined) break;
      names.push(g.name);
    }
    clear(crumbs);
    crumbs.hidden = names.length === 0;
    if (names.length === 0) return;
    const part = (label: string, n: number): HTMLElement => {
      if (n === names.length) return h("span", { class: "crumb here", "aria-current": "page" }, label);
      const b = h("button", { type: "button", class: "crumb" }, label);
      b.addEventListener("click", () => collapseTo(n));
      return b;
    };
    crumbs.append(part("Overview", 0));
    names.forEach((n, i) => {
      crumbs.append(h("span", { class: "sep", "aria-hidden": "true" }, "›"), part(n, i + 1));
    });
  }

  function mount(): void {
    // main.ts writes a first-load error into the map; the first picture replaces it.
    if (svg.parentElement !== root) root.replaceChildren(crumbs, svg, note);
  }

  /** Redraw everything from the state. `pulseIds` are boxes to pulse. */
  function render(pulseIds: Iterable<string> = []): void {
    const o = getState().overview;
    if (o === null) return;
    const t = performance.now();
    for (const id of pulseIds) recent.set(id, t);
    mount();
    breadcrumbs();
    const drawn = draw();
    camera.setFocus(drawn.focus);
    camera.sync();
    if (o.boxes.length === 0) {
      note.textContent = "No files on this map yet.";
      note.hidden = false;
    }
    // A new expansion may reach past the edge of the screen: bring it into view.
    if (drawn.fresh && drawn.focus !== null) camera.reveal(drawn.focus);
  }

  /** What the picture draws from the open card: the links of the selected file in the
   * view on screen. Empty when there are none. */
  function linksKey(): string {
    const sel = getState().selected;
    const payload = currentCard();
    if (sel === null || payload === null || payload.kind !== "file" || payload.card.path !== sel.path) return "";
    const links = payload.links;
    if (links === null || links.view !== currentView()) return "";
    return `${sel.path}|${JSON.stringify(links)}`;
  }

  /** A file card opened from outside the map (a search hit, a link in the drawer) for a
   * file that is not in the picture: expand along the card's `ancestors` so the file
   * appears and its links draw. Done once per card; a card the map itself asked for
   * again after the user moved the view never moves it back. */
  function maybeReveal(): void {
    const sel = getState().selected;
    const payload = currentCard();
    if (sel === null || sel.symbol !== null || payload === null || payload.kind !== "file") return;
    if (payload.card.path !== sel.path || settled === sel.path) return;
    settled = sel.path;
    const ancestors = payload.ancestors;
    if (ancestors === undefined || ancestors === null || ancestors.length === 0) return;
    const layers = buildLayers(getState().level);
    const inner = layers[layers.length - 1];
    if (inner?.shown.some((b) => b.kind === "file" && b.path === sel.path) === true) return;
    void expandTo(ancestors);
  }

  // ---- Wiring ----------------------------------------------------------------

  /** Drop the part of the expansion the new data no longer has. */
  function pruneExpanded(): void {
    const o = getState().overview;
    if (o === null) return;
    const cur = getState().expanded;
    let keep = 0;
    let boxes: Box[] = o.boxes;
    for (const id of cur) {
      const b = boxes.find((x) => x.id === id);
      const g = cache.get(id);
      if (b === undefined || g === undefined) break;
      keep += 1;
      boxes = g.children;
    }
    if (keep < cur.length) {
      expandRequest += 1;
      setState({ expanded: cur.slice(0, keep) });
      reopenCard();
    }
  }

  /** Fetch the expanded groups again; a 404 collapses to the level above it. Any other
   * failure keeps what is on screen and tries again on the next facts delta. */
  async function refetchGroups(): Promise<void> {
    const ids = getState().expanded;
    if (ids.length === 0) return;
    const mine = ++refetchRun;
    retryNext = false;
    const results = await Promise.allSettled(ids.map((id) => fetchGroup(id)));
    if (mine !== refetchRun || !sameIds(ids, getState().expanded)) return;
    const changed = new Set<string>();
    let gone = -1;
    results.forEach((r, i) => {
      const id = ids[i] ?? "";
      if (r.status === "fulfilled") {
        const old = cache.get(id);
        if (old !== undefined) {
          for (const c of changedBoxes(old.children, old.arrows, r.value.children, r.value.arrows)) changed.add(c);
        }
        cache.set(id, r.value);
      } else if (r.reason instanceof ApiError && r.reason.status === 404) {
        if (gone < 0) gone = i;
        cache.delete(id);
      } else {
        retryNext = true;
      }
    });
    if (gone >= 0) {
      expandRequest += 1;
      setState({ expanded: ids.slice(0, gone) });
      reopenCard();
    } else {
      render(changed);
    }
  }

  onChange((st, prev) => {
    if (st.selected === null) settled = null;
    if (st.overview !== prev.overview) {
      if (st.overview !== null && prev.overview !== null) {
        const t = performance.now();
        for (const id of changedBoxes(prev.overview.boxes, prev.overview.arrows, st.overview.boxes, st.overview.arrows)) {
          recent.set(id, t);
        }
        const before = st.expanded;
        pruneExpanded();
        // pruning changes the state, which already drew; draw again only if it did not
        if (getState().expanded === before) render();
      } else {
        render();
      }
    } else if (st.level !== prev.level || st.expanded !== prev.expanded || st.selected !== prev.selected) {
      render();
    }
    if (st.tab === "map" && prev.tab !== "map") requestAnimationFrame(() => render());
  });
  onCard(() => {
    maybeReveal();
    // The card refreshes after every delta: redraw only when its links are not the
    // ones on screen, so a growth in progress is not disturbed for nothing.
    if (linksKey() !== drawnLinks) render();
  });
  onFacts((d) => {
    const ids = getState().expanded;
    cache.invalidate(d, ids);
    if (retryNext || d.regrouped || ids.some((id) => d.groups_changed.includes(id))) void refetchGroups();
  });
  // A reconnect that found newer facts missed deltas, and none of them says which
  // groups changed: treat it as a regroup. The expanded groups are fetched again (a 404
  // collapses them) and the rest of the cache goes.
  onResync(() => {
    cache.invalidate({ regrouped: true, groups_changed: [] }, getState().expanded);
    void refetchGroups();
  });

  if (typeof ResizeObserver === "function") {
    // The map has no size while its tab is hidden: the first measure that finds one
    // fits the overview, and every later one keeps the zoom.
    new ResizeObserver(() => camera.sync()).observe(root);
  }

  // Esc goes up one level, unless a card is open: then the drawer takes it first.
  document.addEventListener(
    "keydown",
    (e) => {
      const st = getState();
      if (e.key !== "Escape" || isTyping(e.target) || st.tab !== "map" || st.selected !== null) return;
      if (st.expanded.length === 0) return;
      e.preventDefault();
      collapseTo(st.expanded.length - 1);
    },
    true,
  );

  render();
}
