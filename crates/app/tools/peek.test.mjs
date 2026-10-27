// Peek mode state machine and geometry (ui/peek.js). Run: node --test crates/app/tools/peek.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const Peek = require("../ui/peek.js");
const P = 96;

// [from, event, to] for every state x event
const TABLE = [
  ["rest", "hover_in", "hover"], ["hover", "hover_in", "hover"], ["up", "hover_in", "up"],
  ["rest", "hover_out", "rest"], ["hover", "hover_out", "rest"], ["up", "hover_out", "up"],
  ["rest", "click", "up"], ["hover", "click", "up"], ["up", "click", "up"],
  ["rest", "needs_attention", "up"], ["hover", "needs_attention", "up"], ["up", "needs_attention", "up"],
  ["rest", "quiet", "rest"], ["hover", "quiet", "hover"], ["up", "quiet", "up"],
  ["rest", "timeout", "rest"], ["hover", "timeout", "rest"], ["up", "timeout", "rest"],
  ["rest", "mode_always", "up"], ["hover", "mode_always", "up"], ["up", "mode_always", "up"],
  ["rest", "mode_peek", "rest"], ["hover", "mode_peek", "hover"], ["up", "mode_peek", "up"],
];

for (const [from, ev, to] of TABLE) {
  test(`${from} + ${ev} -> ${to}`, () => assert.equal(Peek.next(from, ev), to));
}

test("the table covers every state and event", () => {
  for (const s of Peek.STATES) for (const e of Peek.EVENTS) {
    assert.ok(TABLE.some(([f, ev]) => f === s && ev === e), `${s} + ${e} missing`);
  }
});

test("unknown event keeps the state, unknown state counts as up", () => {
  assert.equal(Peek.next("rest", "wiggle"), "rest");
  assert.equal(Peek.next("bogus", "hover_in"), "up");
  assert.equal(Peek.next(undefined, "timeout"), "rest");
});

test("a whole day: rest, hover, leave, a bubble, quiet, back to rest", () => {
  let s = "rest";
  for (const ev of ["hover_in", "hover_out", "hover_in", "click", "quiet", "timeout", "needs_attention", "timeout"]) s = Peek.next(s, ev);
  assert.equal(s, "rest");
});

// The overlay as the real app gets it: 100 % = 1920x1080 with a 48 px taskbar; 166 % = the 2880x1800 laptop.
const SCREENS = [
  { name: "100 %", w: 1920, h: 1080, bar: 48 },
  { name: "166 %", w: 2880 / 1.66, h: 1800 / 1.66, bar: 48 },
];

for (const sc of SCREENS) {
  test(`rest at ${sc.name}: only the head shows, never over the taskbar`, () => {
    const g = Peek.geometry("rest", P, sc.h, sc.bar, sc.w);
    const workBottom = sc.h - sc.bar;
    assert.equal(g.visibleH, Math.round(P * Peek.HEAD));
    assert.ok(g.visibleH < P * 0.65, "more than the head shows");
    assert.ok(g.y + g.visibleH <= workBottom + 1e-9, "head overlaps the taskbar");
    assert.ok(Math.abs(g.y + g.visibleH - workBottom) < 1e-9, "head does not reach the work-area bottom");
    assert.equal(g.x, sc.w - P - 16);
    assert.equal(g.hitRect.h, g.visibleH);
    assert.deepEqual(g.hitRect, { x: g.x, y: g.y, w: P, h: g.visibleH });
    assert.ok(g.hitRect.x + g.hitRect.w <= sc.w);
  });

  test(`hover at ${sc.name}: rest raised by 14 px, still above the taskbar`, () => {
    const r = Peek.geometry("rest", P, sc.h, sc.bar, sc.w);
    const g = Peek.geometry("hover", P, sc.h, sc.bar, sc.w);
    assert.equal(r.y - g.y, 14);
    assert.equal(g.visibleH, r.visibleH + 14);
    assert.equal(g.x, r.x);
    assert.ok(g.y + g.visibleH <= sc.h - sc.bar + 1e-9);
    assert.equal(g.hitRect.h, g.visibleH);
  });

  test(`up at ${sc.name}: the old home position, fully on screen`, () => {
    const g = Peek.geometry("up", P, sc.h, sc.bar, sc.w);
    assert.equal(g.x, sc.w - P - 28);
    assert.equal(g.y, sc.h - sc.bar - P - 20);
    assert.equal(g.visibleH, P);
    assert.deepEqual(g.hitRect, { x: g.x, y: g.y, w: P, h: P });
  });
}

test("no taskbar number: 48 px fallback", () => {
  const g = Peek.geometry("rest", P, 1080, undefined, 1920);
  assert.equal(g.y + g.visibleH, 1080 - 48);
  assert.equal(Peek.geometry("rest", P, 1080, NaN, 1920).y, g.y);
});

test("overlay that already is the work area: no taskbar overlap", () => {
  assert.equal(Peek.taskbarOverlap({ height: 1080, availHeight: 1032, availTop: 0 }, 0, 1032), 0);
  assert.equal(Peek.taskbarOverlap({ height: 1080, availHeight: 1032, availTop: 0 }, 0, 1080), 48);
  assert.ok(Number.isNaN(Peek.taskbarOverlap(null, 0, 1080)));
});

test("bigger Pip scales the head", () => {
  const g = Peek.geometry("rest", 120, 900, 0, 1600);
  assert.equal(g.visibleH, Math.round(120 * Peek.HEAD));
  assert.equal(g.y, 900 - g.visibleH);
});
