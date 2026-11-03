// Frame-by-frame animation shared by the map and its camera. Everything runs on
// requestAnimationFrame and lasts at most GROW_MS; the browser's reduced-motion
// setting turns it into a jump to the end state.

/** How long a box takes to grow and the camera takes to glide, in milliseconds. */
export const GROW_MS = 240;

export function reducedMotion(): boolean {
  return typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Run `step(p)` for p from 0 to 1 over `ms` on animation frames, counted from `t0`;
 * `alive` stops it early (a newer picture replaced this one). */
export function animate(ms: number, step: (p: number) => void, alive: () => boolean, t0 = performance.now()): void {
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
