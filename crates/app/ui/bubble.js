// Bubble rules: when an open bubble is "the same" (its buttons are kept, never rebuilt), how one press on a button
// fires exactly once, and what an ended run leaves of the shown question. Pure functions only (no DOM): main.js
// wires them to the nodes, tools/bubble.test.mjs tests them with fake elements.
// VM 27.10. 12:32 / 13:58: "Yes" and "No" showed their pressed style but no answer arrived. A rebuilt button (same
// bubble shown again between pointerdown and pointerup) never gets its `click`; WebView2 may also take the first
// press on a window that never had the keyboard as an activation click. Buttons act on `pointerup` now.
"use strict";

(function (root) {
  /** A press fires once: a second signal of the same press (pointerup, then click) within this many ms is ignored. */
  const PRESS_MS = 400;

  /** What makes a bubble the same one: text, kind, question id (or a reminder's id) and the button labels. */
  function signature(text, opts) {
    const o = opts || {};
    return {
      text: String(text == null ? "" : text),
      kind: o.kind || "say",
      qid: o.qid || o.id || null,
      labels: (o.buttons || []).map((b) => String(b && b.label != null ? b.label : "")),
    };
  }

  /** The open bubble (prev) and the one to show (next) are the same: keep its buttons, do not rebuild them. */
  function sameBubble(prev, next) {
    if (!prev || !next) return false;
    if (prev.text !== next.text || prev.kind !== next.kind || (prev.qid || null) !== (next.qid || null)) return false;
    const a = prev.labels || [], b = next.labels || [];
    return a.length === b.length && a.every((l, i) => l === b[i]);
  }

  /** One clock for all buttons: a press that closed a bubble cannot fire the button shown in its place. */
  function pressClock() {
    return { at: -Infinity };
  }
  const shared = pressClock();

  /**
   * One button's press handling. onPress(via, event) runs exactly once per press: on `pointerup` (left button)
   * or `click`, whichever comes first, plus Enter / Space on the focused button. `pointerdown` only calls
   * preventDefault (the webview must not treat the first press as an activation click).
   * opts: { clock (shared), now () -> ms }
   */
  function pressGuard(onPress, opts) {
    const o = opts || {};
    const clock = o.clock || shared;
    const now = o.now || (() => (typeof performance !== "undefined" ? performance.now() : Date.now()));
    const fire = (via, e) => {
      const t = now();
      if (t - clock.at < PRESS_MS) return false;
      clock.at = t;
      onPress(via, e);
      return true;
    };
    return {
      pointerdown(e) {
        if (e && e.cancelable !== false && typeof e.preventDefault === "function") e.preventDefault();
      },
      pointerup(e) {
        if (e && Number.isFinite(e.button) && e.button !== 0) return false;
        return fire("pointerup", e);
      },
      click(e) {
        return fire("click", e);
      },
      keydown(e) {
        if (!e || (e.key !== "Enter" && e.key !== " ")) return false;
        if (typeof e.preventDefault === "function") e.preventDefault(); // no second, native click
        if (e.repeat) return false;
        return fire("key", e);
      },
    };
  }

  /** Wire a guard to a node (anything with addEventListener). Returns the guard. */
  function attach(node, onPress, opts) {
    const g = pressGuard(onPress, opts);
    for (const ev of ["pointerdown", "pointerup", "click", "keydown"]) node.addEventListener(ev, g[ev]);
    return g;
  }

  /**
   * The run ended (acting off). shown: { kind, qid, buttons } of the bubble on screen (null = none); queue: the
   * waiting bubbles ({ text, opts }). Questions of the ended run that wait in the queue are dropped (they wait for
   * nobody). The shown question keeps its buttons and its id when it has any: the pressed option still goes to the
   * core as the answer. Returns { queue, keep } (keep: the shown question stays as it is).
   */
  function afterRun(shown, queue) {
    const rest = (queue || []).filter((q) => !(q && q.opts && q.opts.qid));
    const keep = !!(shown && shown.kind === "question" && shown.qid && shown.buttons > 0);
    return { queue: rest, keep };
  }

  const Bubble = { PRESS_MS, signature, sameBubble, pressClock, pressGuard, attach, afterRun };
  root.Bubble = Bubble;
  if (typeof module !== "undefined" && module.exports) module.exports = Bubble;
})(typeof window !== "undefined" ? window : globalThis);
