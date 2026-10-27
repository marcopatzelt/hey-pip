// "Good to know" bubbles (ui/tip.js) and how they move Pip in peek mode (ui/peek.js).
// Run: node --test crates/app/ui/peek.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const Tip = require("./tip.js");
const Peek = require("./peek.js");

// The bubble the app sends after Erika's sick note (bridge.rs show_tip): the tip, then its source on the last line.
const TIP_TEXT = "Good to know: your insurer may only pay sick pay without a gap. If you're still ill, the next note "
  + "usually has to be issued by Monday, 2 November (the next working day after Friday, 30 October). Please check "
  + "with your insurer.\n§ 46 Satz 2 SGB V";
const TIP_BUTTONS = [
  { id: "app:tip_remind", label: "Remind me on Monday, 2 November", primary: true },
  { id: "app:tip_question", label: "Write a short question", primary: false },
  { id: "app:tip_why", label: "Why?", primary: false },
];

test("a tip bubble is known by its app:tip_ buttons", () => {
  assert.ok(Tip.isTip(TIP_BUTTONS));
  assert.ok(Tip.isTip(TIP_BUTTONS.slice(0, 2)), "the tip again after Why? (no second Why?)");
  assert.ok(Tip.isTip([{ id: "app:tip_ok", label: "OK", primary: true }]), "the Why? bubble");
  assert.ok(!Tip.isTip([{ id: "app:tip_send", label: "Yes, send it" }, { id: "app:tip_not_now", label: "Not now" }]),
    "the question offer is an ordinary question bubble");
  assert.ok(!Tip.isTip([{ id: "close", label: "OK" }]));
  assert.ok(!Tip.isTip([{ id: "guard:real" }]));
  assert.ok(!Tip.isTip(undefined) && !Tip.isTip([]));
});

test("label, text and source chip come apart", () => {
  const p = Tip.parts(TIP_TEXT);
  assert.equal(p.label, "Good to know");
  assert.ok(p.body.startsWith("Your insurer may only pay sick pay"), p.body);
  assert.ok(p.body.endsWith("Please check with your insurer."));
  assert.equal(p.source, "§ 46 Satz 2 SGB V");
  const de = Tip.parts("Gut zu wissen: Du hast meist einen Monat.\n§ 84 SGG / § 70 VwGO");
  assert.deepEqual(de, { label: "Gut zu wissen", body: "Du hast meist einen Monat.", source: "§ 84 SGG / § 70 VwGO" });
  assert.equal(Tip.parts("Gut zu wissen: deine Kasse zahlt.\n§ 1").body, "Deine Kasse zahlt.", "the first letter is capital");
});

test("the Why? bubble keeps its sentence whole (the colon in the link is no label)", () => {
  const why = "That comes from the rule § 46 Satz 2 SGB V (https://www.gesetze-im-internet.de/sgb_5/__46.html). "
    + "It is a hint, not legal advice. You decide.\n§ 46 Satz 2 SGB V";
  const p = Tip.parts(why);
  assert.equal(p.label, "");
  assert.ok(p.body.startsWith("That comes from the rule"), p.body);
  assert.equal(p.source, "§ 46 Satz 2 SGB V");
});

test("one line only: no source, the text stays", () => {
  assert.deepEqual(Tip.parts("Okay, I'll remind you on Monday, 2 November."),
    { label: "", body: "Okay, I'll remind you on Monday, 2 November.", source: "" });
  assert.deepEqual(Tip.parts(""), { label: "", body: "", source: "" });
  assert.deepEqual(Tip.parts(null), { label: "", body: "", source: "" });
});

test("a tip raises Pip from the edge like a brief and he slides back after it is closed", () => {
  // Pip rests at the edge; the letter bubble, then the tip bubble (showBubble calls rise = needs_attention)
  let s = "rest";
  s = Peek.next(s, "needs_attention");
  assert.equal(s, "up", "the letter bubble raises him");
  for (const ev of ["needs_attention", "quiet", "click"]) s = Peek.next(s, ev);
  assert.equal(s, "up", "the tip bubble and its buttons keep him up");
  // Why?, OK, the tip again: still up
  for (const ev of ["needs_attention", "needs_attention", "quiet"]) s = Peek.next(s, ev);
  assert.equal(s, "up");
  // closed (x or a button): the quiet clock runs out and he slides back to the edge
  s = Peek.next(s, "timeout");
  assert.equal(s, "rest");
  // the head at the edge never covers the taskbar
  const g = Peek.geometry(s, 96, 900, 48, 1600);
  assert.ok(g.y + g.visibleH <= 900 - 48);
});

test("with Always show Pip a tip never lowers him", () => {
  let s = Peek.next("rest", "mode_always");
  for (const ev of ["needs_attention", "quiet"]) s = Peek.next(s, ev);
  assert.equal(s, "up");
});
