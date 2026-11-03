// The Map view, drawn in SVG: the overview boxes in role lanes with counted arrows,
// the in-place drill-down (a box grows over its dimmed neighbours and shows its
// children), "+N elsewhere" chips, the links of the selected file, live pulses and
// the camera. The pure geometry lives in geometry.ts; this file holds the DOM.
//
// Coordinates: overview boxes are in world units; the children of a group are
// relative to the group's top-left corner, so each level of expansion is one `<g>`
// translated to that corner.

import { ApiError, group as fetchGroup } from "./api.ts";
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
  clampZoom,
  easeOut,
  ensureVisible,
  fit,
  lerp,
  resizeView,
  strokeWidth,
  truncate,
  zoomAt,
} from "./geometry.ts";
import type { Pt, Rect } from "./geometry.ts";
import { onFacts } from "./live.ts";
import { currentView, getState, onChange, setState } from "./state.ts";
import { defs, s, text, title, viewBoxOf } from "./svg.ts";
import type { Arrow, Box, Chip, GroupView } from "./types.ts";

/** How long a box takes to grow, and a camera move takes, in milliseconds. */
const GROW_MS = 240;
/** A pulse of a changed box. */
const PULSE_MS = 300;
/** Opacity of everything an expanded box covers. */
const DIM = 0.35;
/** Pixels the pointer must travel before a press becomes a pan. */
const DRAG_PX = 4;
/** Zoom limits, relative to the width of the fitted overview. */
const ZOOM_IN = 8;
const ZOOM_OUT = 4;

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

function reducedMotion(): boolean {
  return typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Run `step(p)` for p from 0 to 1 over `ms` on animation frames, counted from `t0`;
 * `alive` stops it early (a newer picture replaced this one). */
function animate(ms: number, step: (p: number) => void, alive: () => boolean, t0 = performance.now()): void {
  if (reducedMotion()) {
    step(1);
    return;
  }
  const frame = (now: number): void => {
    if (!alive()) return;
    const p = Math.min(1, Math.max(0, (now - t0) / ms));
    step(p);
    if (p < 1) requestAnimationFrame(frame);
  };
  frame(performance.now());
}

/** Draw the map into `root` and keep it in step with the state. */
export function renderMap(root: HTMLElement): void {
  const svg = s("svg", { class: "map-svg", role: "group", "aria-label": "Map of the project" }) as SVGSVGElement;
  const scene = s("g", { class: "scene" });
  svg.append(defs(), scene);
  const crumbs = h("nav", { class: "crumbs", "aria-label": "Where you are in the map", hidden: "" });
  const note = h("div", { class: "map-note", role: "status", hidden: "" });

  // ---- State of the picture ------------------------------------------------

  const groups = new Map<string, GroupView>();
  /** Bumped by every drawing; animations of an older drawing stop. */
  let drawing = 0;
  let registered = new Map<string, Registered>();
  /** The expansion the last drawing showed, to tell growth from a refresh. */
  let drawnExpanded: string[] = [];
  /** Bumped by every request to expand, so only the newest one lands. */
  let expandRequest = 0;

  // ---- Camera --------------------------------------------------------------

  let view: Rect | null = null;
  /** The fitted overview: the zoom limits are relative to it. */
  let base: Rect | null = null;
  /** True while the camera is the fitted one, so a resize or a refetch refits it. */
  let auto = true;
  let size = { w: 0, h: 0 };
  let cameraRun = 0;
  /** The innermost expanded group with its chips, in the world; null when collapsed. */
  let focus: Rect | null = null;

  const applyView = (): void => {
    if (view !== null) svg.setAttribute("viewBox", viewBoxOf(view));
  };

  const overviewRects = (): Rect[] => getState().overview?.boxes ?? [];

  const fitNow = (): void => {
    view = fit(overviewRects(), size);
    base = view;
    auto = true;
    applyView();
  };

  /** Move the camera to `to` over GROW_MS. */
  const glideTo = (to: Rect): void => {
    const from = view;
    const run = ++cameraRun;
    if (from === null) {
      view = to;
      applyView();
      return;
    }
    animate(
      GROW_MS,
      (p) => {
        const e = easeOut(p);
        view = { x: lerp(from.x, to.x, e), y: lerp(from.y, to.y, e), w: lerp(from.w, to.w, e), h: lerp(from.h, to.h, e) };
        applyView();
      },
      () => run === cameraRun,
    );
  };

  const measure = (): void => {
    const w = root.clientWidth;
    const h2 = root.clientHeight;
    if (w === 0 || h2 === 0) return;
    if (view === null || auto) {
      size = { w, h: h2 };
      fitNow();
    } else if (w !== size.w || h2 !== size.h) {
      view = resizeView(view, size, { w, h: h2 });
      // A narrower map (the drawer opened) must not push the expanded group off it.
      if (focus !== null) view = ensureVisible(view, focus, 24);
      size = { w, h: h2 };
      applyView();
    }
  };

  /** The world point under a pointer event. */
  const toWorld = (clientX: number, clientY: number): Pt => {
    const r = svg.getBoundingClientRect();
    const v = view ?? { x: 0, y: 0, w: 1, h: 1 };
    return { x: v.x + ((clientX - r.left) / (r.width || 1)) * v.w, y: v.y + ((clientY - r.top) / (r.height || 1)) * v.h };
  };

  svg.addEventListener(
    "wheel",
    (e) => {
      if (view === null) return;
      e.preventDefault();
      cameraRun += 1;
      const unit = e.deltaMode === 1 ? 16 : 1;
      const factor = Math.exp(Math.max(-200, Math.min(200, e.deltaY * unit)) * 0.0015);
      let next = zoomAt(view, factor, toWorld(e.clientX, e.clientY));
      if (base !== null) next = clampZoom(next, base.w / ZOOM_IN, base.w * ZOOM_OUT);
      view = next;
      auto = false;
      applyView();
    },
    { passive: false },
  );

  let press: { x: number; y: number; view: Rect; id: number } | null = null;
  let dragged = false;
  svg.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || view === null) return;
    press = { x: e.clientX, y: e.clientY, view, id: e.pointerId };
    dragged = false;
  });
  svg.addEventListener("pointermove", (e) => {
    if (press === null || e.pointerId !== press.id) return;
    const dx = e.clientX - press.x;
    const dy = e.clientY - press.y;
    if (!dragged) {
      if (Math.hypot(dx, dy) < DRAG_PX) return;
      dragged = true;
      cameraRun += 1;
      // Capture only once it is a drag, so a plain click still reaches its box.
      svg.setPointerCapture(e.pointerId);
      svg.classList.add("dragging");
    }
    const r = svg.getBoundingClientRect();
    view = {
      x: press.view.x - (dx * press.view.w) / (r.width || 1),
      y: press.view.y - (dy * press.view.h) / (r.height || 1),
      w: press.view.w,
      h: press.view.h,
    };
    auto = false;
    applyView();
  });
  const release = (e: PointerEvent): void => {
    if (press === null || e.pointerId !== press.id) return;
    press = null;
    svg.classList.remove("dragging");
    if (dragged) {
      // The click that follows a drag is not a click on a box.
      window.setTimeout(() => {
        dragged = false;
      }, 0);
    }
  };
  svg.addEventListener("pointerup", release);
  svg.addEventListener("pointercancel", release);
  svg.addEventListener(
    "click",
    (e) => {
      if (dragged) e.stopPropagation();
    },
    true,
  );
  svg.addEventListener("click", (e) => {
    // A click on the empty map (not on a box or a chip) clears the selection.
    if ((e.target as Element).closest("[data-act]") === null && getState().selected !== null) closeDrawer();
  });
  svg.addEventListener("dblclick", (e) => {
    if ((e.target as Element).closest("[data-act]") !== null || size.w === 0) return;
    cameraRun += 1;
    const to = fit(overviewRects(), size);
    auto = true;
    base = to;
    glideTo(to);
  });

  // ---- Expanding and collapsing ---------------------------------------------

  /** After the view changed, the open file card is asked again, so its links are
   * counted in the boxes that are visible now. */
  const reopenCard = (): void => {
    const sel = getState().selected;
    if (sel !== null && sel.symbol === null) void openFile(sel.path, currentView());
  };

  const sameIds = (a: string[], b: string[]): boolean => a.length === b.length && a.every((x, i) => x === b[i]);

  /** Show the groups `ids` (outermost first): fetch the ones not held, then switch. */
  async function expandTo(ids: string[]): Promise<void> {
    if (sameIds(ids, getState().expanded)) return;
    const mine = ++expandRequest;
    try {
      await Promise.all(
        ids
          .filter((id) => !groups.has(id))
          .map(async (id) => {
            groups.set(id, await fetchGroup(id));
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
      const g = groups.get(expanded[i] ?? "");
      if (parent === undefined || box === undefined || g === undefined) break;
      layers.push({
        origin: { x: parent.origin.x + box.x, y: parent.origin.y + box.y },
        group: g,
        all: g.children,
        shown: childrenFor(level, g.children),
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
    g.addEventListener("click", () => activate(box, layerIndex));
    g.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        activate(box, layerIndex);
      }
    });
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
      const go = (): void => void expandTo([c.id]);
      g.addEventListener("click", go);
      g.addEventListener("keydown", (e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          go();
        }
      });
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
    /** True when the innermost group grew in this drawing. */
    grew: boolean;
  }

  /** Boxes that changed lately and when, so a redraw in the middle of a pulse (the
   * card refreshes right after the overview) carries it on instead of ending it. */
  const pulses = new Map<string, number>();

  function draw(): Drawn {
    const level = getState().level;
    const layers = buildLayers(level);
    const mine = ++drawing;
    const alive = (): boolean => mine === drawing;
    registered = new Map();
    scene.replaceChildren();
    if (layers.length === 0) return { focus: null, grew: false };

    const expanded = getState().expanded.slice(0, layers.length - 1);
    // Growth, not a refresh: the innermost level is new, or a different group.
    const last = expanded.length - 1;
    const growing = last >= 0 && drawnExpanded[last] !== expanded[last] && !reducedMotion();
    drawnExpanded = expanded;

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
    const now = performance.now();
    for (const [id, t0] of pulses) {
      if (now - t0 >= PULSE_MS) pulses.delete(id);
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
    if (grow !== null) animate(GROW_MS, grow, alive);
    return { focus, grew: grow !== null };
  }

  function breadcrumbs(): void {
    const expanded = getState().expanded;
    const names: string[] = [];
    for (const id of expanded) {
      const g = groups.get(id);
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
    for (const id of pulseIds) pulses.set(id, t);
    mount();
    breadcrumbs();
    measure();
    const drawn = draw();
    focus = drawn.focus;
    const grew = drawn.grew;
    if (o.boxes.length === 0) {
      note.textContent = "No files on this map yet.";
      note.hidden = false;
    }
    // A new expansion may reach past the edge of the screen: bring it into view.
    if (grew && focus !== null && view !== null) {
      const to = ensureVisible(view, focus, 24);
      if (to !== view) {
        auto = false;
        glideTo(to);
      }
    }
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
      const g = groups.get(id);
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

  /** Fetch the expanded groups again; a 404 collapses to the level above it. */
  async function refetchGroups(): Promise<void> {
    const ids = getState().expanded;
    if (ids.length === 0) return;
    const results = await Promise.allSettled(ids.map((id) => fetchGroup(id)));
    if (!sameIds(ids, getState().expanded)) return;
    const pulses = new Set<string>();
    let gone = -1;
    results.forEach((r, i) => {
      const id = ids[i] ?? "";
      if (r.status === "fulfilled") {
        const old = groups.get(id);
        if (old !== undefined) {
          for (const c of changedBoxes(old.children, old.arrows, r.value.children, r.value.arrows)) pulses.add(c);
        }
        groups.set(id, r.value);
      } else if (r.reason instanceof ApiError && r.reason.status === 404 && gone < 0) {
        gone = i;
        groups.delete(id);
      }
    });
    if (gone >= 0) {
      expandRequest += 1;
      setState({ expanded: ids.slice(0, gone) });
      reopenCard();
    } else {
      render(pulses);
    }
  }

  onChange((st, prev) => {
    if (st.overview !== prev.overview) {
      if (st.overview !== null && prev.overview !== null) {
        const t = performance.now();
        for (const id of changedBoxes(prev.overview.boxes, prev.overview.arrows, st.overview.boxes, st.overview.arrows)) {
          pulses.set(id, t);
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
  onCard(() => render());
  onFacts((d) => {
    const ids = getState().expanded;
    if (d.regrouped || ids.some((id) => d.groups_changed.includes(id))) void refetchGroups();
  });

  if (typeof ResizeObserver === "function") {
    // The map has no size while its tab is hidden: the first measure that finds one
    // fits the overview, and every later one keeps the zoom.
    new ResizeObserver(() => {
      measure();
      if (getState().overview !== null) applyView();
    }).observe(root);
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
