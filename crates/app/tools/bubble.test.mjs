// Bubble buttons (ui/bubble.js): one press = one action, the same bubble keeps its buttons, an ended run keeps the
// shown question answerable. Run: node --test crates/app/tools/bubble.test.mjs
// VM 27.10. 12:32 and 13:58: "Yes" / "No" showed the pressed style, but no answer reached the core.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const Bubble = require("../ui/bubble.js");

// ---------- fake DOM: just enough for buttons in a bubble ----------
class FakeNode {
  constructor(label) {
    this.label = label;
    this.listeners = {};
    this.children = [];
    this.parent = null;
  }
  addEventListener(type, fn) {
    (this.listeners[type] ||= []).push(fn);
  }
  /** Dispatch like the browser: only to a node that is still in the bubble. Returns the event. */
  dispatch(type, init = {}) {
    const ev = { type, button: 0, cancelable: true, defaultPrevented: false, ...init };
    ev.preventDefault = () => { ev.defaultPrevented = true; };
    for (const fn of this.listeners[type] || []) fn(ev);
    return ev;
  }
  replaceChildren(...nodes) {
    for (const c of this.children) c.parent = null;
    this.children = nodes;
    for (const c of nodes) c.parent = this;
  }
}

/** A clock the test moves by hand (ms). */
const fakeClock = () => {
  const c = { t: 1000 };
  c.now = () => c.t;
  return c;
};

/**
 * The bubble as main.js runs it: showBubble rebuilds the buttons only when the bubble is not the same one.
 * Buttons get the press guard; every press is recorded.
 */
function fakeBubble(now) {
  const box = new FakeNode("buttons");
  const pressed = [];
  const clock = Bubble.pressClock();
  let sig = null;
  let shown = false;
  const show = (text, opts) => {
    const next = Bubble.signature(text, opts);
    const keep = shown && Bubble.sameBubble(sig, next);
    sig = next;
    shown = true;
    if (!keep) {
      box.replaceChildren(...(opts.buttons || []).map((b) => {
        const n = new FakeNode(b.label);
        Bubble.attach(n, (via) => { pressed.push({ label: b.label, via }); b.onClick(); }, { clock, now });
        return n;
      }));
    }
  };
  const hide = () => { shown = false; sig = null; };
  return { box, pressed, show, hide, button: (label) => box.children.find((n) => n.label === label) };
}

const yesNo = (answers) => [
  { label: "Yes", onClick: () => answers.push("yes") },
  { label: "No", onClick: () => answers.push("no") },
];

test("the same bubble shown again between pointerdown and pointerup: the button fires once", () => {
  const c = fakeClock();
  const answers = [];
  const b = fakeBubble(c.now);
  const opts = { kind: "question", qid: "q1", buttons: yesNo(answers) };
  b.show("Are you still unable to work today?", opts);
  const yes = b.button("Yes");
  const down = yes.dispatch("pointerdown");
  assert.ok(down.defaultPrevented, "pointerdown must only preventDefault (no activation click)");
  assert.deepEqual(answers, [], "pointerdown alone never answers");
  // the core and the app both send the question again; layout, hits and the peek slide run in between
  c.t += 30;
  b.show("Are you still unable to work today?", { kind: "question", qid: "q1", buttons: yesNo(answers) });
  assert.equal(b.button("Yes"), yes, "the open bubble's button was rebuilt");
  c.t += 60;
  yes.dispatch("pointerup");
  c.t += 5;
  yes.dispatch("click");
  assert.deepEqual(answers, ["yes"]);
  assert.deepEqual(b.pressed, [{ label: "Yes", via: "pointerup" }]);
});

test("a button never fires twice for one press (pointerup + click)", () => {
  const c = fakeClock();
  const answers = [];
  const b = fakeBubble(c.now);
  b.show("Shall I go on?", { kind: "question", qid: "q2", buttons: yesNo(answers) });
  const no = b.button("No");
  no.dispatch("pointerdown");
  c.t += 80;
  no.dispatch("pointerup");
  c.t += 10;
  no.dispatch("click");
  assert.deepEqual(answers, ["no"]);
  // a click without a pointerup (keyboard, accessibility tools) fires on its own, and its late pointerup does not
  c.t += 1000;
  no.dispatch("click");
  c.t += 50;
  no.dispatch("pointerup");
  assert.deepEqual(answers, ["no", "no"]);
});

test("the right mouse button never answers", () => {
  const c = fakeClock();
  const answers = [];
  const b = fakeBubble(c.now);
  b.show("Shall I go on?", { kind: "question", qid: "q3", buttons: yesNo(answers) });
  b.button("Yes").dispatch("pointerup", { button: 2 });
  assert.deepEqual(answers, []);
});

test("Enter and Space on a focused button fire once, a held key does not repeat", () => {
  const c = fakeClock();
  const answers = [];
  const b = fakeBubble(c.now);
  b.show("Shall I go on?", { kind: "question", qid: "q4", buttons: yesNo(answers) });
  const yes = b.button("Yes");
  const ev = yes.dispatch("keydown", { key: "Enter" });
  assert.ok(ev.defaultPrevented, "Enter's own click is suppressed");
  yes.dispatch("click"); // came anyway: ignored
  c.t += 500;
  yes.dispatch("keydown", { key: " ", repeat: true });
  assert.deepEqual(answers, ["yes"]);
  yes.dispatch("keydown", { key: " " });
  assert.deepEqual(answers, ["yes", "yes"]);
  yes.dispatch("keydown", { key: "a" });
  assert.equal(answers.length, 2);
});

test("the same bubble twice keeps the same button nodes; a changed label, text, kind or qid rebuilds", () => {
  const c = fakeClock();
  const b = fakeBubble(c.now);
  const opts = (labels, extra = {}) => ({ kind: "question", qid: "q5", buttons: labels.map((label) => ({ label, onClick() {} })), ...extra });
  b.show("Use the IBAN from your card?", opts(["Yes", "No"]));
  const first = [...b.box.children];
  b.show("Use the IBAN from your card?", opts(["Yes", "No"]));
  assert.deepEqual(b.box.children, first);
  b.show("Use the IBAN from your card?", opts(["Yes", "Other IBAN"]));
  assert.notEqual(b.box.children[1], first[1], "a changed label must rebuild");
  const second = [...b.box.children];
  b.show("Use the IBAN from your card? ", opts(["Yes", "Other IBAN"]));
  assert.notEqual(b.box.children[0], second[0], "changed text must rebuild");
  const third = [...b.box.children];
  b.show("Use the IBAN from your card? ", opts(["Yes", "Other IBAN"], { qid: "q6" }));
  assert.notEqual(b.box.children[0], third[0], "a new question id must rebuild");
  // a bubble that was closed and comes again is built fresh
  const fourth = [...b.box.children];
  b.hide();
  b.show("Use the IBAN from your card? ", opts(["Yes", "Other IBAN"], { qid: "q6" }));
  assert.notEqual(b.box.children[0], fourth[0]);
});

test("a press that closes a bubble does not fire the button shown in its place", () => {
  const c = fakeClock();
  const b = fakeBubble(c.now);
  const log = [];
  // Continue closes the bubble and the next bubble's Continue sits at the same spot
  const step = (n) => ({ kind: "question", qid: `c${n}`, buttons: [{ label: "Continue", onClick: () => { log.push(n); b.hide(); } }] });
  b.show("Step 1", step(1));
  const first = b.button("Continue");
  first.dispatch("pointerdown");
  first.dispatch("pointerup");
  b.show("Step 2", step(2));
  const second = b.button("Continue");
  assert.notEqual(second, first);
  c.t += 5;
  second.dispatch("click"); // the release's click lands on the new button
  assert.deepEqual(log, [1]);
  // a real second press later works
  c.t += 600;
  second.dispatch("pointerdown");
  second.dispatch("pointerup");
  assert.deepEqual(log, [1, 2]);
});

test("Continue x5 while Pip slides: every press answers once", () => {
  const c = fakeClock();
  const b = fakeBubble(c.now);
  const log = [];
  for (let n = 1; n <= 5; n++) {
    b.show("Shall I go on?", { kind: "question", qid: `p${n}`, buttons: [{ label: "Continue", onClick: () => { log.push(n); b.hide(); } }] });
    const btn = b.button("Continue");
    btn.dispatch("pointerdown");
    // the slide, hits and the queue count run between down and up: the same bubble again
    b.show("Shall I go on?", { kind: "question", qid: `p${n}`, buttons: [{ label: "Continue", onClick: () => {} }] });
    c.t += 90;
    btn.dispatch("pointerup");
    btn.dispatch("click");
    c.t += 1500;
  }
  assert.deepEqual(log, [1, 2, 3, 4, 5]);
});

test("sameBubble compares text, kind, qid and labels only", () => {
  const a = Bubble.signature("Hi", { kind: "question", qid: "x", buttons: [{ label: "OK", onClick: () => 1 }] });
  const b = Bubble.signature("Hi", { kind: "question", qid: "x", buttons: [{ label: "OK", onClick: () => 2 }] });
  assert.ok(Bubble.sameBubble(a, b));
  assert.ok(!Bubble.sameBubble(null, b));
  assert.ok(Bubble.sameBubble(Bubble.signature("Hi"), Bubble.signature("Hi", {})), "say bubbles without buttons");
  assert.ok(!Bubble.sameBubble(Bubble.signature("Hi"), Bubble.signature("Hi", { kind: "tip" })));
  // a reminder with the same words but another id (its buttons answer that reminder) is another bubble
  const r1 = Bubble.signature("Sick note due Monday", { kind: "reminder", id: "r1", buttons: [{ label: "Done" }] });
  const r2 = Bubble.signature("Sick note due Monday", { kind: "reminder", id: "r2", buttons: [{ label: "Done" }] });
  assert.ok(!Bubble.sameBubble(r1, r2));
  assert.ok(Bubble.sameBubble(r1, Bubble.signature("Sick note due Monday", { kind: "reminder", id: "r1", buttons: [{ label: "Done" }] })));
});

test("an ended run: the shown question keeps its buttons, queued questions wait for nobody", () => {
  const queue = [
    { text: "Another question?", opts: { qid: "q8", kind: "question" } },
    { text: "A tip", opts: { kind: "tip" } },
  ];
  const end = Bubble.afterRun({ kind: "question", qid: "q7", buttons: 2 }, queue);
  assert.ok(end.keep, "the offer's buttons must still answer");
  assert.deepEqual(end.queue.map((q) => q.text), ["A tip"]);
  // a question without buttons, or a plain bubble, is not kept as a question
  assert.ok(!Bubble.afterRun({ kind: "question", qid: "q7", buttons: 0 }, []).keep);
  assert.ok(!Bubble.afterRun({ kind: "say", qid: null, buttons: 1 }, []).keep);
  assert.ok(!Bubble.afterRun({ kind: "guard", qid: "g", buttons: 2 }, []).keep);
  assert.ok(!Bubble.afterRun(null, queue).keep);
});

test("the kept question's press sends the answer with its question id", () => {
  // main.js: the bubble's onClick is () => { hideBubble(); runButton(b.id, qid) } -> {type:"answer", ...}
  const c = fakeClock();
  const sent = [];
  const runButton = (id, qid) => { if (qid) sent.push({ type: "answer", question_id: qid, choice: id }); };
  const b = fakeBubble(c.now);
  const qid = "6f1c2a9e-0000-4000-8000-000000000001";
  const buttons = [{ id: "yes", label: "Yes, let's do it" }, { id: "no", label: "Not now" }]
    .map((x) => ({ label: x.label, onClick: () => runButton(x.id, qid) }));
  b.show("Shall I also claim the delay?", { kind: "question", qid, buttons });
  const before = b.button("Not now");
  // the run ends (acting off): kept, no rebuild
  assert.ok(Bubble.afterRun({ kind: "question", qid, buttons: b.box.children.length }, []).keep);
  assert.equal(b.button("Not now"), before);
  before.dispatch("pointerdown");
  before.dispatch("pointerup");
  assert.deepEqual(sent, [{ type: "answer", question_id: qid, choice: "no" }]);
});
