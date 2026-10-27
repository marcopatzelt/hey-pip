// Peek mode: Pip rests as a head at the screen edge and rises on hover or when he has something to say.
// Pure functions only (no DOM): main.js drives them, tools/peek.test.mjs tests them with node.
//   states: "rest" (only the head shows above the work-area bottom), "hover" (rest raised a little), "up" (fully out)
//   events: hover_in, hover_out, click, needs_attention, quiet, timeout, mode_always, mode_peek
"use strict";

(function (root) {
  const STATES = ["rest", "hover", "up"];
  const EVENTS = ["hover_in", "hover_out", "click", "needs_attention", "quiet", "timeout", "mode_always", "mode_peek"];
  // Share of Pip's 96 px box that shows at rest: the head (tuft + eyes). The drawn figure starts 16 % below the
  // box top, so this is the top 40 % of the figure itself plus the eyes (they sit at 44-58 % of the box).
  const HEAD = 0.62;
  const RISE = 14;           // px the head rises on hover
  const EDGE_X = 16;         // gap to the right screen edge at rest and hover
  const HOME_X = 28;         // "up" = the home position main.js always had
  const HOME_Y = 20;
  const TASKBAR_FALLBACK = 48;

  /** The next state. Unknown states count as "up" (safe: nothing hides), unknown events change nothing. */
  function next(state, event) {
    const s = STATES.includes(state) ? state : "up";
    switch (event) {
      case "hover_in": return s === "rest" ? "hover" : s;
      case "hover_out": return s === "hover" ? "rest" : s;
      case "click":
      case "needs_attention":
      case "mode_always": return "up";
      // quiet only starts the clock (main.js); the timeout lowers Pip
      case "quiet": return s;
      case "timeout": return "rest";
      // back to peek: Pip stays where he is until the quiet clock runs out
      case "mode_peek": return s;
      default: return s;
    }
  }

  /**
   * Where Pip sits in a state, in the overlay's CSS px (top-left is 0,0).
   * screenCssH: the overlay's height; taskbarCssH: how much of its bottom the taskbar covers (not a number: 48);
   * screenCssW: the overlay's width (default: the window's).
   * Returns { x, y, visibleH, hitRect }: hitRect is the part of Pip that is on screen (only the head at rest).
   */
  function geometry(state, pipPx, screenCssH, taskbarCssH, screenCssW) {
    const bar = Number.isFinite(taskbarCssH) ? Math.max(0, taskbarCssH) : TASKBAR_FALLBACK;
    const w = Number.isFinite(screenCssW) ? screenCssW : (typeof window !== "undefined" ? window.innerWidth : 0);
    const bottom = screenCssH - bar;
    const head = Math.round(pipPx * HEAD);
    let x, y, visibleH;
    if (state === "rest" || state === "hover") {
      x = w - pipPx - EDGE_X;
      visibleH = head + (state === "hover" ? RISE : 0);
      y = bottom - visibleH;
    } else {
      x = w - pipPx - HOME_X;
      y = bottom - pipPx - HOME_Y;
      visibleH = pipPx;
    }
    return { x, y, visibleH, hitRect: { x, y, w: pipPx, h: visibleH }, bottom };
  }

  /**
   * How much of the overlay's bottom the taskbar covers (CSS px). The real overlay already is the work area
   * (0); a full-screen window over the taskbar gets the overlap. No screen data at all: NaN (geometry: 48).
   */
  function taskbarOverlap(scr, winTop, winH) {
    if (!scr || !scr.height || !scr.availHeight) return NaN;
    const workBottom = (scr.availTop || 0) + scr.availHeight;
    return Math.max(0, Math.min(160, (winTop || 0) + winH - workBottom));
  }

  const Peek = { STATES, EVENTS, HEAD, RISE, next, geometry, taskbarOverlap };
  root.Peek = Peek;
  if (typeof module !== "undefined" && module.exports) module.exports = Peek;
})(typeof window !== "undefined" ? window : globalThis);
