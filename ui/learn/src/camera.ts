// The camera of the map: it owns the SVG's `viewBox`. It fits the overview, zooms
// on the wheel about the cursor, pans on a drag, resets on a double click, keeps
// the scale when the map gets narrower (the drawer opens) and glides when an
// expansion reaches past the edge. The rules themselves are pure and live in
// geometry.ts; this file only wires them to the pointer.

import { clampZoom, easeOut, ensureVisible, fit, lerp, resizeView, zoomAt } from "./geometry.ts";
import type { Pt, Rect } from "./geometry.ts";
import { GROW_MS, animate } from "./motion.ts";
import { viewBoxOf } from "./svg.ts";

/** Pixels the pointer must travel before a press becomes a pan. */
const DRAG_PX = 4;
/** Zoom limits, relative to the width of the fitted overview. */
const ZOOM_IN = 8;
const ZOOM_OUT = 4;
/** World units kept free around a group the camera brings into view. */
const REVEAL_MARGIN = 24;

export interface Camera {
  /** Measure the map and fit or keep the view: untouched and collapsed refits the
   * overview each time (a collapse back to it included); otherwise a size change keeps
   * the scale and centre. Does nothing while the map has no size (its tab is hidden). */
  sync(): void;
  /** Tell the camera which rectangle (the innermost expanded group with its chips)
   * must stay on screen, or null when the overview is shown. */
  setFocus(focus: Rect | null): void;
  /** Glide the view, if it has to move, so `rect` is on screen. */
  reveal(rect: Rect): void;
}

export function createCamera(opts: { root: HTMLElement; svg: SVGSVGElement; rects: () => Rect[] }): Camera {
  const { root, svg } = opts;
  let view: Rect | null = null;
  /** The fitted overview: the zoom limits are relative to it. */
  let base: Rect | null = null;
  /** True once the user zoomed or panned: the camera then stops refitting. */
  let touched = false;
  let focus: Rect | null = null;
  let size = { w: 0, h: 0 };
  /** Bumped to stop a glide that a newer move replaced. */
  let run = 0;
  let dragged = false;

  const apply = (): void => {
    if (view !== null) svg.setAttribute("viewBox", viewBoxOf(view));
  };

  const fitNow = (): void => {
    view = fit(opts.rects(), size);
    base = view;
    apply();
  };

  const glideTo = (to: Rect): void => {
    const from = view;
    const mine = ++run;
    if (from === null) {
      view = to;
      apply();
      return;
    }
    animate(
      GROW_MS,
      (p) => {
        const e = easeOut(p);
        view = { x: lerp(from.x, to.x, e), y: lerp(from.y, to.y, e), w: lerp(from.w, to.w, e), h: lerp(from.h, to.h, e) };
        apply();
      },
      () => mine === run,
    );
  };

  /** The world point under a pointer position. */
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
      run += 1;
      const unit = e.deltaMode === 1 ? 16 : 1;
      const factor = Math.exp(Math.max(-200, Math.min(200, e.deltaY * unit)) * 0.0015);
      let next = zoomAt(view, factor, toWorld(e.clientX, e.clientY));
      if (base !== null) next = clampZoom(next, base.w / ZOOM_IN, base.w * ZOOM_OUT);
      view = next;
      touched = true;
      apply();
    },
    { passive: false },
  );

  let press: { x: number; y: number; view: Rect; id: number } | null = null;
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
      run += 1;
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
    touched = true;
    apply();
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
  svg.addEventListener("dblclick", (e) => {
    if ((e.target as Element).closest("[data-act]") !== null || size.w === 0) return;
    const to = fit(opts.rects(), size);
    touched = false;
    base = to;
    glideTo(to);
  });

  return {
    sync(): void {
      const w = root.clientWidth;
      const h = root.clientHeight;
      if (w === 0 || h === 0) return;
      if (view === null || (!touched && focus === null)) {
        size = { w, h };
        fitNow();
      } else if (w !== size.w || h !== size.h) {
        view = resizeView(view, size, { w, h });
        // A narrower map (the drawer opened) must not push the expanded group off it.
        if (focus !== null) view = ensureVisible(view, focus, REVEAL_MARGIN);
        size = { w, h };
        apply();
      }
    },
    setFocus(f: Rect | null): void {
      focus = f;
    },
    reveal(rect: Rect): void {
      if (view === null) return;
      const to = ensureVisible(view, rect, REVEAL_MARGIN);
      if (to !== view) glideTo(to);
    },
  };
}
