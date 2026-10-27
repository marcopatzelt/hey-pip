// Hit rects (ui/hits.js): what of the overlay takes clicks. Run: node --test crates/app/tools/hits.test.mjs
// Live Windows run 27.10.: "Skip it" on a question bubble (Pip away mid-screen) went through to Edge.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const Hits = require("../ui/hits.js");
const Peek = require("../ui/peek.js");
const P = 96;
const W = 1536, H = 816; // 1920x1020 at 125 %

// Pip flown beside the "Sick since" field in the middle of the page; the question bubble left of him, the
// progress pill and Stop under him.
const pipAway = { x: 700, y: 380, w: P, h: P };
const bubble = { x: 246, y: 300, w: 440, h: 176 };
const skipButton = { x: 400, y: 410, w: 110, h: 48 };  // inside the bubble, lower middle
const fixedButton = { x: 270, y: 410, w: 120, h: 48 };
const progress = { x: 640, y: 486, w: 215, h: 41 };
const stop = { x: 690, y: 535, w: 110, h: 52 };
const card = { x: 246, y: 120, w: 440, h: 380 };
const notYet = { x: 470, y: 430, w: 180, h: 56 };

const head = (state) => {
  const g = Peek.geometry(state, P, H, 0, W);
  return { x: g.x, y: g.y, w: P, h: g.visibleH, bottom: g.bottom };
};
const mid = (r) => [r.x + r.w / 2, r.y + r.h / 2];
const corners = (r) => [[r.x, r.y], [r.x + r.w - 0.01, r.y], [r.x, r.y + r.h - 0.01], [r.x + r.w - 0.01, r.y + r.h - 0.01]];

/** Every point of `r` (CSS px) is inside the screen rects, at this origin and scale. */
function covered(rects, r, origin, k) {
  return [mid(r), ...corners(r)].every(([x, y]) => Hits.inside(rects, origin.x + x * k, origin.y + y * k));
}

const SCALES = [
  { k: 1, origin: { x: 0, y: 0 } },
  { k: 1.25, origin: { x: 0, y: 0 } },
  { k: 1.25, origin: { x: -1920, y: 40 } }, // a monitor left of the primary one, below a top taskbar
];

for (const { k, origin } of SCALES) {
  const name = `${k * 100} % at ${origin.x},${origin.y}`;

  test(`${name}: a question bubble with Pip away mid-screen takes its clicks`, () => {
    const css = Hits.cssRects({
      pip: { box: pipAway, home: false, peek: "up", head: head("up") },
      boxes: [bubble, null, null, null, progress, stop, null],
    });
    const rects = Hits.toPhys(css, origin, k);
    for (const r of [bubble, skipButton, fixedButton, progress, stop, pipAway]) {
      assert.ok(covered(rects, r, origin, k), `${JSON.stringify(r)} not covered by ${JSON.stringify(rects)}`);
    }
    // The page around stays clickable: right of Pip, above the bubble.
    assert.ok(!Hits.inside(rects, origin.x + 1200 * k, origin.y + 400 * k));
    assert.ok(!Hits.inside(rects, origin.x + 300 * k, origin.y + 200 * k));
  });

  test(`${name}: the approval card takes its clicks ("Not yet")`, () => {
    // The card hides the dock (progress and Stop measure as nothing).
    const css = Hits.cssRects({
      pip: { box: pipAway, home: false, peek: "up", head: head("up") },
      boxes: [null, card, null, null, null, null, null],
    });
    const rects = Hits.toPhys(css, origin, k);
    assert.ok(covered(rects, card, origin, k));
    assert.ok(covered(rects, notYet, origin, k));
  });

  test(`${name}: after the peek slide the bubble takes its clicks, and peek never shrinks Pip while it is open`, () => {
    const up = Peek.geometry("up", P, H, 0, W);
    const box = { x: up.x, y: up.y, w: P, h: P };
    const homeBubble = { x: up.x - 14 - 440, y: up.y + P - 150, w: 440, h: 150 };
    // Slid out ("up"), bubble beside him.
    let rects = Hits.toPhys(Hits.cssRects({ pip: { box, home: true, peek: "up", head: head("up") }, boxes: [homeBubble] }), origin, k);
    assert.ok(covered(rects, homeBubble, origin, k));
    assert.ok(covered(rects, box, origin, k));
    // The peek state still says "rest" or "hover" (a hover arrived mid-slide): with the bubble open Pip keeps his
    // whole box, not the resting head.
    for (const peek of ["rest", "hover"]) {
      rects = Hits.toPhys(Hits.cssRects({ pip: { box, home: true, peek, head: head(peek) }, boxes: [homeBubble] }), origin, k);
      assert.ok(covered(rects, homeBubble, origin, k), peek);
      assert.ok(covered(rects, box, origin, k), `${peek}: Pip's whole box`);
    }
  });

  test(`${name}: resting with nothing open only the head takes clicks`, () => {
    const g = head("rest");
    const box = { x: g.x, y: g.y, w: P, h: P }; // the drawn box reaches below the work area (clipped)
    const rects = Hits.toPhys(Hits.cssRects({ pip: { box, home: true, peek: "rest", head: g }, boxes: [null, null] }), origin, k);
    assert.equal(rects.length, 1);
    assert.ok(covered(rects, { x: g.x, y: g.y, w: P, h: g.h }, origin, k));
    assert.ok(!Hits.inside(rects, origin.x + (g.x + P / 2) * k, origin.y + (g.bottom + 10) * k), "nothing below the work area");
  });
}

test("toPhys rounds outward and drops broken numbers", () => {
  assert.deepEqual(Hits.toPhys([{ x: 10.3, y: 0, w: 10.3, h: 4 }], { x: 0, y: 0 }, 1.25), [{ x: 12, y: 0, w: 14, h: 5 }]);
  assert.deepEqual(Hits.toPhys([{ x: NaN, y: 0, w: 5, h: 5 }, { x: 0, y: 0, w: 0, h: 5 }], { x: 0, y: 0 }, 1), []);
});

test("holding the mic keeps the whole overlay", () => {
  const css = Hits.cssRects({ pip: null, boxes: [], micDown: true, viewport: { w: W, h: H } });
  assert.deepEqual(css, [{ x: 0, y: 0, w: W, h: H }]);
});

test("anyOpen", () => {
  assert.equal(Hits.anyOpen([null, null]), false);
  assert.equal(Hits.anyOpen([null, { x: 0, y: 0, w: 0, h: 0 }]), false);
  assert.equal(Hits.anyOpen([null, bubble]), true);
});
