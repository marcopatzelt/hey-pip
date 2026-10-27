// Hit rects: the parts of the overlay that take clicks (Pip, bubble, card, ask box, list, progress, Stop).
// Everywhere else clicks go through to the page. Pure functions only (no DOM): main.js measures the nodes,
// tools/hits.test.mjs tests the rules with node.
//   cssRects(scene) -> rects in CSS px (viewport origin), with a small margin
//   toPhys(rects, origin, k) -> screen pixels: origin + css * k, rounded outward (the app does the same in Rust
//   with the window's live position; these are its fallback)
"use strict";

(function (root) {
  const MARGIN = 6;           // around every box: a click on a button's edge still lands on Pip's window
  const HEAD_MARGIN = { rest: 6, hover: 24 }; // resting head: wider while hovering, so the leave reaches us

  const usable = (r) => !!r && [r.x, r.y, r.w, r.h].every(Number.isFinite) && r.w >= 1 && r.h >= 1;
  const grow = (r, m) => ({ x: r.x - m, y: r.y - m, w: r.w + 2 * m, h: r.h + 2 * m });

  /** Something besides Pip is on screen (bubble, card, ask box, list, progress, Stop, note). */
  function anyOpen(boxes) {
    return (boxes || []).some(usable);
  }

  /**
   * scene: {
   *   pip: { box, home, peek, head } | null   box: Pip's drawn box (CSS rect); head: Peek.geometry's visible part
   *                                            { x, y, w, h, bottom } (only used at the edge with nothing open)
   *   boxes: [rect | null]                     every other visible node (null = hidden)
   *   micDown: bool, viewport: { w, h }        holding the mic: the whole overlay keeps the mouse
   * }
   * Peek never shrinks the rects while something is open: then Pip counts with his whole box.
   */
  function cssRects(scene) {
    const out = [];
    const boxes = scene.boxes || [];
    if (scene.micDown && scene.viewport) out.push({ x: 0, y: 0, w: scene.viewport.w, h: scene.viewport.h });
    const pip = scene.pip;
    if (pip && usable(pip.box)) {
      const resting = pip.home && (pip.peek === "rest" || pip.peek === "hover");
      if (resting && !anyOpen(boxes) && usable(pip.head)) {
        const g = pip.head, m = HEAD_MARGIN[pip.peek];
        const top = g.y - m;
        const bottom = Number.isFinite(g.bottom) ? Math.min(g.bottom, g.y + g.h + m) : g.y + g.h + m;
        out.push({ x: g.x - m, y: top, w: g.w + 2 * m, h: bottom - top });
      } else {
        out.push(grow(pip.box, MARGIN));
      }
    }
    for (const b of boxes) if (usable(b)) out.push(grow(b, MARGIN));
    return out;
  }

  /** CSS rects -> screen pixels (origin: the window's top-left on the screen, k: devicePixelRatio). */
  function toPhys(rects, origin, k) {
    const ox = (origin && Number.isFinite(origin.x)) ? origin.x : 0;
    const oy = (origin && Number.isFinite(origin.y)) ? origin.y : 0;
    const s = Number.isFinite(k) && k > 0 ? k : 1;
    return rects.filter(usable).map((r) => {
      const x0 = Math.floor(ox + r.x * s), y0 = Math.floor(oy + r.y * s);
      const x1 = Math.ceil(ox + (r.x + r.w) * s), y1 = Math.ceil(oy + (r.y + r.h) * s);
      return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
    });
  }

  /** True if the point (same units as the rects) is inside one of them. */
  function inside(rects, x, y) {
    return rects.some((r) => x >= r.x && y >= r.y && x < r.x + r.w && y < r.y + r.h);
  }

  const Hits = { MARGIN, anyOpen, cssRects, toPhys, inside };
  root.Hits = Hits;
  if (typeof module !== "undefined" && module.exports) module.exports = Hits;
})(typeof window !== "undefined" ? window : globalThis);
