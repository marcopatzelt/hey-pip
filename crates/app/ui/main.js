// Pip overlay. Listens to core events ("ui") and app events ("app"); sends UserCommand via
// invoke("ui_command"). Coordinates from core are physical screen pixels; the page works in CSS px.
"use strict";

const TAURI = window.__TAURI__;
const invoke = (cmd, args) => TAURI.core.invoke(cmd, args);
const send = (cmd) => invoke("ui_command", { cmd }).catch((e) => console.warn("ui_command", e));
const $ = (id) => document.getElementById(id);

const PNG = { idle: "idle", listening: "listen", thinking: "think", pointing: "point", acting: "point",
  asking: "ask", happy: "happy", sleeping: "sleep", error: "ask" };
const P = 96;          // Pip size (CSS px)
const GAP = 24;        // distance between Pip and a target
const EDGE = 8;        // keep this far from screen edges
const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

let S = {};
const s = (k) => S[k] || k;
let info = { x: 0, y: 0, w: innerWidth, h: innerHeight, scale: 1, windowed: false };
const dpr = () => window.devicePixelRatio || 1;

const st = {
  pos: { x: innerWidth + 40, y: innerHeight - P - 24 },
  home: true,
  colSide: null,        // "left" | "right" while pointing (column away from the target)
  pipState: "idle",
  pose: null,           // temporary pose overriding the state (wave, listen while dragging)
  acting: false,
  paused: false,
  dragging: false,
  onboarding: null,
  bubbleKind: null,
  bubbleSticky: false,   // the bubble waits for an answer (buttons, question, reminder, onboarding)
  bubbleTimer: 0,
  queue: [],             // sticky bubbles waiting for the one on screen: { text, opts }
  shownOf: 0,            // how many sticky bubbles came in this row ("Nachricht 1 von 2")
  noteTimer: 0,
  marks: [],             // places Pip filled in during this run (CSS rects): the card must not cover them
  card: null,
  lastSpoke: 0,
  flight: 0,
};

const el = {
  pip: $("pip"), body: $("pip-body"), img: $("pip-img"),
  hl: $("highlight"), hlLabel: $("hl-label"), arrow: $("arrow"),
  col: $("column"), dock: $("dock"),
  bubble: $("bubble"), bText: $("bubble-text"), bExtra: $("bubble-extra"), bButtons: $("bubble-buttons"),
  bClose: $("bubble-close"), bCount: $("bubble-count"), note: $("note"),
  progress: $("progress"), pText: $("progress-text"), stop: $("stop"),
  ask: $("ask"), askInput: $("ask-input"), askMic: $("ask-mic"),
  card: $("card"), panel: $("panel"),
};

// ---------- geometry ----------
const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
const homePos = () => ({ x: innerWidth - P - 28, y: innerHeight - P - 20 });
function physRectToCss(r) {
  const k = dpr();
  return { x: (r.x - info.x) / k, y: (r.y - info.y) / k, w: r.w / k, h: r.h / k };
}
function physPointToCss(x, y) {
  const k = dpr();
  return { x: (x - info.x) / k, y: (y - info.y) / k };
}

// Where Pip sits next to a target: left of it if there is room, else right; never on top.
function besideTarget(r) {
  let side = "left";
  let x = r.x - GAP - P;
  if (x < EDGE) { side = "right"; x = r.x + r.w + GAP; }
  x = clamp(x, EDGE, innerWidth - P - EDGE);
  const y = clamp(r.y + r.h / 2 - P * 0.55, EDGE, innerHeight - P - EDGE);
  return { x, y, side };
}

// ---------- Pip ----------
function placePip(x, y) {
  st.pos = { x, y };
  el.pip.style.transform = `translate(${x}px, ${y}px)`;
  layout();
}

const easeOut = (t) => 1 - Math.pow(1 - t, 3);
function flyTo(x, y, ms) {
  cancelAnimationFrame(st.flight);
  const p0 = { ...st.pos };
  const dist = Math.hypot(x - p0.x, y - p0.y);
  if (reduceMotion || dist < 2) { placePip(x, y); return Promise.resolve(); }
  const dur = ms ?? (st.home ? 500 : 300);
  const lift = Math.min(140, dist * 0.35);
  const c = { x: (p0.x + x) / 2, y: Math.min(p0.y, y) - lift };
  el.pip.classList.add("flying");
  el.pip.style.rotate = `${clamp((x - p0.x) / 40, -8, 8)}deg`;
  return new Promise((done) => {
    const t0 = performance.now();
    const frame = (now) => {
      const t = easeOut(Math.min(1, (now - t0) / dur));
      const u = 1 - t;
      placePip(u * u * p0.x + 2 * u * t * c.x + t * t * x, u * u * p0.y + 2 * u * t * c.y + t * t * y);
      if (t < 1) { st.flight = requestAnimationFrame(frame); return; }
      el.pip.classList.remove("flying");
      el.pip.style.rotate = "";
      done();
    };
    st.flight = requestAnimationFrame(frame);
  });
}

function flyHome() {
  hideTarget();
  st.colSide = null;
  el.pip.classList.remove("mirror", "away");
  const h = homePos();
  const p = flyTo(h.x, h.y, st.home ? undefined : 500);
  st.home = true;
  return p;
}

function showPose() {
  const name = st.pose || PNG[st.pipState] || "idle";
  const src = `pip/${name}.png`;
  if (!el.img.src.endsWith(src)) el.img.src = src;
}

function setState(state) {
  const prev = st.pipState;
  st.pipState = state;
  el.pip.className = el.pip.className.replace(/\bstate-\S+/g, "").trim() + ` state-${state}`;
  showPose();
  if (state === "happy" && prev !== "happy") hop();
}

function hop() {
  el.body.classList.remove("hop");
  void el.body.offsetWidth;
  el.body.classList.add("hop");
}

(function blinkLoop() {
  setTimeout(() => {
    if (st.pipState !== "sleeping" && !st.dragging) {
      el.body.classList.add("blink");
      setTimeout(() => el.body.classList.remove("blink"), 120);
    }
    blinkLoop();
  }, 3000 + Math.random() * 3500);
})();

// ---------- pointing ----------
function hideTarget() {
  el.hl.hidden = true;
  el.arrow.hidden = true;
}

async function pointAt(rect, label) {
  if (info.windowed) return; // corner fallback: Pip only says the target
  const r = physRectToCss(rect);
  const pad = 6;
  Object.assign(el.hl.style, { left: `${r.x - pad}px`, top: `${r.y - pad}px`, width: `${r.w + 2 * pad}px`, height: `${r.h + 2 * pad}px` });
  el.hlLabel.textContent = label || "";
  el.hl.hidden = false;
  el.hl.style.animation = "none"; void el.hl.offsetWidth; el.hl.style.animation = "";
  const b = besideTarget(r);
  st.colSide = b.side;
  el.arrow.hidden = true;
  const wasHome = st.home;
  st.home = false;
  el.pip.classList.add("away");
  el.pip.classList.toggle("mirror", b.side === "right");
  await flyTo(b.x, b.y, wasHome ? 500 : 300);
  // short arrow in the gap, pointing at the target
  const cy = clamp(r.y + r.h / 2, EDGE, innerHeight - EDGE) - 9;
  el.arrow.classList.toggle("left", b.side === "right");
  el.arrow.style.setProperty("--nudge", b.side === "right" ? "-4px" : "4px");
  el.arrow.style.left = `${b.side === "left" ? r.x - pad - 17 : r.x + r.w + pad + 4}px`;
  el.arrow.style.top = `${cy}px`;
  el.arrow.hidden = el.hl.hidden;
}

function flyNear(x, y) {
  if (info.windowed) return;
  const p = physPointToCss(x, y);
  const b = besideTarget({ x: p.x, y: p.y, w: 0, h: 0 });
  st.colSide = b.side;
  const wasHome = st.home;
  st.home = false;
  el.pip.classList.add("away");
  el.pip.classList.toggle("mirror", b.side === "right");
  return flyTo(b.x, b.y, wasHome ? 500 : 300);
}

// ---------- the column beside Pip ----------
// Everything Pip says sits in one column next to him. It must never cover the target he points at:
// try beside (away from the target), above, below, the other side; take the first spot that fits.
function overlap(a, b) {
  if (!b) return 0;
  const w = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
  const h = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
  return w > 0 && h > 0 ? w * h : 0;
}
function rectOf(node) {
  if (node.hidden) return null;
  const r = node.getBoundingClientRect();
  return r.width && r.height ? { x: r.left, y: r.top, w: r.width, h: r.height } : null;
}

/** Pick the candidate spot (w x h) with the lowest cost: covering obstacles (weighted) + being pushed. */
function placeBest(w, h, cands, obstacles) {
  const fitX = (x) => clamp(x, EDGE, innerWidth - EDGE - w);
  const fitY = (y) => clamp(y, EDGE, innerHeight - EDGE - h);
  let best = null;
  for (const c of cands) {
    const r = { x: fitX(c.x), y: fitY(c.y), w, h };
    let cost = Math.abs(r.x - c.x) + Math.abs(r.y - c.y);
    for (const [o, weight] of obstacles) cost += overlap(r, o) * weight;
    if (!best || cost < best.cost) best = { ...c, x: r.x, y: r.y, cost };
    if (cost < 24) break;
  }
  return best;
}

function layout() {
  const W = innerWidth, p = st.pos;
  const leftHalf = p.x + P / 2 < W / 2;
  const hl = rectOf(el.hl);
  // the label chip sits above the highlight: keep that free too
  const target = hl && { x: hl.x - 8, y: hl.y - 48, w: hl.w + 16, h: hl.h + 56 };
  const pipBox = { x: p.x, y: p.y, w: P, h: P };

  // 1) dock (progress + Stopp): under Pip, else beside him at his feet, else above
  const dw = el.dock.offsetWidth, dh = el.dock.offsetHeight;
  let dockBox = null;
  if (dw && dh) {
    const away = st.colSide || (leftHalf ? "right" : "left");
    const d = placeBest(dw, dh, [
      { x: p.x + P / 2 - dw / 2, y: p.y + P + 10, cls: "" },
      away === "left" ? { x: p.x - 12 - dw, y: p.y + P - dh, cls: "side-left" } : { x: p.x + P + 12, y: p.y + P - dh, cls: "side-right" },
      away === "left" ? { x: p.x + P + 12, y: p.y + P - dh, cls: "side-right" } : { x: p.x - 12 - dw, y: p.y + P - dh, cls: "side-left" },
      { x: p.x + P / 2 - dw / 2, y: p.y - 10 - dh, cls: "" },
    ], [[target, 100], [pipBox, 50]]);
    el.dock.className = `dock ${d.cls}`;
    el.dock.style.left = `${d.x}px`;
    el.dock.style.top = `${d.y}px`;
    dockBox = { x: d.x, y: d.y, w: dw, h: dh };
  }

  // 2) column (bubble, card, ask box, list): beside Pip away from the target, above, below, other side
  const colW = Math.min(440, W - 2 * EDGE);
  el.col.style.width = `${colW}px`;
  const h = el.col.offsetHeight;
  if (h) {
    const first = st.colSide || (leftHalf ? "right" : "left");
    const topY = dockBox ? Math.min(p.y, dockBox.y) : p.y;
    const botY = dockBox ? Math.max(p.y + P, dockBox.y + dockBox.h) : p.y + P;
    const spot = {
      left: { x: p.x - 14 - colW, y: p.y + P - h, cls: "left" },
      right: { x: p.x + P + 14, y: p.y + P - h, cls: "right" },
      above: { x: leftHalf ? p.x : p.x + P - colW, y: topY - 12 - h, cls: leftHalf ? "right vert" : "left vert" },
      below: { x: leftHalf ? p.x : p.x + P - colW, y: botY + 12, cls: leftHalf ? "right vert" : "left vert" },
    };
    const order = [first, "above", "below", first === "left" ? "right" : "left"];
    const obstacles = [[target, 100], [pipBox, 30], [dockBox, 30]];
    // While the card is open, the fields Pip filled in must stay visible so the user can check them.
    if (!el.card.hidden) for (const m of st.marks) obstacles.push([m, 60]);
    const c = placeBest(colW, h, order.map((k) => spot[k]), obstacles);
    el.col.className = `column ${c.cls}`;
    el.col.style.left = `${c.x}px`;
    el.col.style.top = `${c.y}px`;
    el.col.style.maxHeight = `${innerHeight - 2 * EDGE}px`;
  }
  scheduleHits();
}

// ---------- hit rects (click-through + screenshot masking) ----------
let hitsQueued = false;
let lastHits = "";
let micDown = false;   // the mic button is held
function scheduleHits() {
  if (hitsQueued) return;
  hitsQueued = true;
  setTimeout(() => { hitsQueued = false; reportHits(); }, 40);
}
function reportHits() {
  const k = dpr();
  const nodes = [el.pip, el.bubble, el.card, el.ask, el.panel, el.progress, el.stop, el.note];
  const rects = [];
  // Holding the mic button: keep every mouse event (the release may happen off the button).
  if (micDown) rects.push({ x: info.x, y: info.y, w: Math.round(innerWidth * k), h: Math.round(innerHeight * k) });
  for (const n of nodes) {
    if (n.hidden) continue;
    const r = n.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) continue;
    const m = 6;
    rects.push({
      x: Math.round(info.x + (r.left - m) * k), y: Math.round(info.y + (r.top - m) * k),
      w: Math.round((r.width + 2 * m) * k), h: Math.round((r.height + 2 * m) * k),
    });
  }
  const json = JSON.stringify(rects);
  if (json === lastHits) return;
  lastHits = json;
  send({ type: "hit_rects", rects });
}
const ro = new ResizeObserver(() => layout());
ro.observe(el.col);
ro.observe(el.dock);

// ---------- bubble ----------
// One bubble on screen. A bubble that waits for an answer (buttons, question, reminder, onboarding) is never
// replaced by a later one: plain sentences go to the small note line, other waiting bubbles queue behind it.
function hideBubble() {
  clearTimeout(st.bubbleTimer);
  el.bubble.hidden = true;
  const wasSticky = st.bubbleSticky;
  st.bubbleKind = null;
  st.bubbleSticky = false;
  if (wasSticky && st.queue.length) {
    const next = st.queue.shift();
    showBubble(next.text, next.opts);
    return;
  }
  if (!st.queue.length) st.shownOf = 0;
  layout();
}

function makeButton(label, primary, onClick) {
  const b = document.createElement("button");
  b.className = primary ? "btn primary" : "btn";
  b.textContent = label;
  b.addEventListener("click", onClick);
  return b;
}

const isSticky = (opts) => !!(opts.sticky || (opts.buttons && opts.buttons.length));

/** Every bubble goes through here. opts: { kind, buttons, extra, sticky, closeX, replace } */
function say(text, opts = {}) {
  const busy = !el.bubble.hidden && st.bubbleSticky;
  const replace = opts.replace || (opts.kind && opts.kind === st.bubbleKind && (opts.kind === "onboarding" || opts.kind === "setup"));
  if (busy && !replace) {
    if (!isSticky(opts)) { showNote(text); return; }
    if (el.bText.textContent === text) { showBubble(text, opts); return; } // same question again: refresh
    st.queue.push({ text, opts });
    showCount();
    return;
  }
  showBubble(text, opts);
}

function showNote(text) {
  clearTimeout(st.noteTimer);
  el.note.textContent = text;
  el.note.hidden = false;
  st.noteTimer = setTimeout(() => { el.note.hidden = true; layout(); }, 6000 + text.length * 70);
  st.lastSpoke = performance.now();
  layout();
}

function showCount() {
  const n = st.shownOf + st.queue.length;
  el.bCount.hidden = n < 2;
  if (n >= 2) el.bCount.textContent = s("queue_of").replace("{i}", String(st.shownOf)).replace("{n}", String(n));
  layout();
}

function showBubble(text, opts = {}) {
  clearTimeout(st.bubbleTimer);
  // The same sentence again (core and app both confirm a stop): update quietly, no second pop.
  const same = !el.bubble.hidden && el.bText.textContent === text && st.bubbleKind === (opts.kind || "say");
  el.bubble.className = "bubble" + (opts.kind ? ` ${opts.kind}` : "");
  st.bubbleKind = opts.kind || "say";
  const sticky = isSticky(opts);
  if (sticky && !st.bubbleSticky) st.shownOf += 1;
  if (!sticky) st.shownOf = 0;
  st.bubbleSticky = sticky;
  el.bText.textContent = text;
  el.bExtra.replaceChildren(...(opts.extra || []));
  el.bButtons.replaceChildren(...(opts.buttons || []).map((b) => makeButton(b.label, b.primary, b.onClick)));
  el.bClose.hidden = !opts.closeX;
  el.bubble.classList.toggle("has-x", !!opts.closeX);
  el.bubble.hidden = false;
  if (!same) { el.bubble.style.animation = "none"; void el.bubble.offsetWidth; el.bubble.style.animation = ""; }
  const closable = !sticky;
  el.bubble.classList.toggle("closable", closable);
  if (closable && !opts.closeX) st.bubbleTimer = setTimeout(hideBubble, 6000 + text.length * 70);
  st.lastSpoke = performance.now();
  if (!same) hop();
  showCount();
}
el.bubble.addEventListener("click", (e) => {
  if (el.bubble.classList.contains("closable") && !e.target.closest("button")) hideBubble();
});
el.bClose.addEventListener("click", () => hideBubble());

/** Core / app buttons: "cmd:<type>" -> UserCommand, "app:<action>" -> app_action, "close" -> just close. */
function runButton(id, qid) {
  if (qid) { send({ type: "answer", question_id: qid, choice: id }); return; }
  if (id.startsWith("cmd:")) send({ type: id.slice(4) });
  else if (id.startsWith("app:")) invoke("app_action", { action: id.slice(4) }).catch(() => {});
}

function errorBubble(text, retry, pick) {
  const buttons = [];
  if (pick) buttons.push({ label: s("btn_other_photo"), primary: true, onClick: () => { hideBubble(); send({ type: "pick_file" }); } });
  else if (retry) buttons.push({ label: s("btn_retry"), primary: true, onClick: () => { hideBubble(); send(retry); } });
  say(text, { kind: "error", closeX: true, buttons, replace: st.bubbleKind === "error" });
}

function para(text, cls) {
  const p = document.createElement("p");
  p.className = cls;
  p.textContent = text;
  return p;
}

// ---------- progress + stop ----------
function setProgress(text) {
  el.pText.textContent = text || "";
  el.progress.hidden = !text;
  if (text) st.lastSpoke = performance.now();
  layout();
}

// Never silent for more than 3 s while busy.
setInterval(() => {
  const busy = st.pipState === "thinking" || st.pipState === "acting";
  if (busy && el.progress.hidden && el.bubble.hidden && el.card.hidden && performance.now() - st.lastSpoke > 3000) {
    setProgress(s("thinking"));
  }
}, 500);

el.stop.addEventListener("click", stopNow);
// Stop first (Pip says "Ich habe aufgehört. Nichts wurde abgeschickt."), then close an open card as "no".
function stopNow() {
  send({ type: "stop" });
  if (st.card) answerCard("no");
}

// ---------- approval card ----------
function showCard(card) {
  if (!st.bubbleSticky) hideBubble();
  closeAsk();
  st.card = card.id;
  $("card-title").textContent = card.title;
  $("card-what").textContent = card.what;
  $("card-to").textContent = card.to;
  const att = $("card-att");
  att.replaceChildren(...(card.attachments || []).map((name) => {
    const d = document.createElement("div");
    d.innerHTML = '<svg><use href="#i-clip"/></svg>';
    d.append(document.createTextNode(name));
    return d;
  }));
  const filled = card.filled || [];
  $("card-filled").hidden = !filled.length;
  $("card-filled-title").textContent = s("approve_filled");
  $("card-filled-list").replaceChildren(...filled.map((line) => {
    const li = document.createElement("li");
    li.textContent = line;
    return li;
  }));
  const crop = $("card-crop");
  if (card.crop_png_b64) {
    $("card-crop-img").src = `data:image/png;base64,${card.crop_png_b64}`;
    $("card-crop-cap").textContent = s("approve_button_preview");
    crop.hidden = false;
  } else crop.hidden = true;
  $("card-yes").textContent = card.yes_label;
  $("card-no").textContent = card.no_label;
  el.card.hidden = false;
  st.lastSpoke = performance.now();
  hop();
  layout();
}
function hideCard() {
  st.card = null;
  el.card.hidden = true;
  layout();
}
function answerCard(answer) {
  if (!st.card) return;
  send({ type: "approve", id: st.card, answer });
  hideCard();
}
$("card-yes").addEventListener("click", () => answerCard("yes"));
$("card-no").addEventListener("click", () => answerCard("no"));

// ---------- reminder ----------
function showReminder(ev) {
  const why = para(ev.why, "why");
  why.hidden = true;
  const whyBtn = document.createElement("button");
  whyBtn.className = "link";
  whyBtn.textContent = s("btn_why");
  whyBtn.addEventListener("click", () => { why.hidden = !why.hidden; layout(); });
  say(ev.text, {
    kind: "reminder",
    sticky: true,
    extra: [whyBtn, why],
    buttons: ev.buttons.map((b) => ({
      label: b.label, primary: b.primary,
      onClick: () => { hideBubble(); send({ type: "reminder_answer", reminder_id: ev.reminder_id, choice: b.id }); },
    })),
  });
}

// ---------- ask box ----------
function openAsk() {
  el.panel.hidden = true;
  if (st.bubbleKind === "say") hideBubble();
  el.ask.hidden = false;
  st.askOpen = true;
  layout();
  invoke("focus_window").catch(() => {});
  setTimeout(() => el.askInput.focus(), 50);
}
function closeAsk() {
  el.ask.hidden = true;
  el.askMic.classList.remove("on");
  if (st.askOpen) { st.askOpen = false; invoke("release_focus").catch(() => {}); }
  layout();
}
$("ask-close").addEventListener("click", closeAsk);
$("ask-form").addEventListener("submit", (e) => {
  e.preventDefault();
  const text = el.askInput.value.trim();
  if (!text) { el.askInput.focus(); return; }
  el.askInput.value = "";
  closeAsk();
  send({ type: "text", text });
});
el.askInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); $("ask-form").requestSubmit(); }
});
$("ask-letter").addEventListener("click", () => { closeAsk(); send({ type: "pick_file" }); });
$("ask-deadlines").addEventListener("click", () => { closeAsk(); invoke("open_deadlines").catch(() => {}); });
$("ask-pause").addEventListener("click", () => { closeAsk(); invoke("app_action", { action: "pause" }).catch(() => {}); });
function fillChips() {
  const chips = [
    ["chip_send_au", () => send({ type: "text", text: s("chip_send_au") })],
    ["chip_last_letter", () => invoke("app_action", { action: "last_letter" }).catch(() => {})],
    ["chip_deadlines", () => invoke("open_deadlines").catch(() => {})],
  ];
  $("ask-chips").replaceChildren(...chips.map(([key, go]) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "chip";
    b.textContent = s(key);
    b.addEventListener("click", () => { closeAsk(); go(); });
    return b;
  }));
}

// hold to talk
el.askMic.addEventListener("pointerdown", (e) => {
  e.preventDefault();
  el.askMic.setPointerCapture(e.pointerId);
  micDown = true;
  scheduleHits();
  el.askMic.classList.add("on");
  send({ type: "voice_start" });
});
const micUp = () => {
  if (!micDown) return;
  micDown = false;
  scheduleHits();
  el.askMic.classList.remove("on");
  closeAsk();
  send({ type: "voice_stop" });
};
el.askMic.addEventListener("pointerup", micUp);
el.askMic.addEventListener("pointercancel", micUp);

// ---------- deadlines panel ----------
function showPanel(items) {
  closeAsk();
  $("panel-title").textContent = s("deadlines_title");
  const list = $("panel-list");
  if (!items.length) list.replaceChildren(para(s("deadlines_none"), "small"));
  else list.replaceChildren(...items.map((it) => {
    const li = document.createElement("li");
    li.classList.toggle("overdue", !!it.overdue);
    const b = document.createElement("b"); b.textContent = it.title;
    const when = document.createElement("span"); when.className = "when"; when.textContent = it.when;
    const due = document.createElement("span"); due.className = "due"; due.textContent = it.due;
    const line = document.createElement("div"); line.append(when, due);
    li.append(b, line);
    const row = document.createElement("div"); row.className = "row-buttons";
    if (it.why) {
      const why = para(it.why, "why"); why.hidden = true;
      const t = document.createElement("button"); t.className = "link"; t.textContent = s("btn_why");
      t.addEventListener("click", () => { why.hidden = !why.hidden; layout(); });
      row.append(t);
      li.append(row, why);
    } else li.append(row);
    if (it.id > 0) {
      row.append(makeButton(s("btn_done"), false, () => invoke("app_action", { action: `deadline_done:${it.id}` }).catch(() => {})));
    }
    return li;
  }));
  el.panel.hidden = false;
  layout();
}
$("panel-close").addEventListener("click", () => { el.panel.hidden = true; layout(); });

// ---------- onboarding ----------
function onboarding(step) {
  st.onboarding = step === "done" ? null : step;
  el.pip.classList.toggle("inviting", step === "first_letter");
  if (step === "ask_name") {
    st.pose = "wave";
    showPose();
    const input = document.createElement("input");
    input.type = "text";
    input.maxLength = 40;
    input.placeholder = s("onb_name_placeholder");
    input.setAttribute("aria-label", s("onb_ask_name"));
    const submit = () => {
      const v = input.value.trim();
      if (!v) { input.focus(); return; }
      st.pose = null;
      showPose();
      send({ type: "onboarding_answer", step: "ask_name", value: v });
    };
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") submit(); });
    say(s("onb_hello"), {
      kind: "onboarding", sticky: true, replace: true,
      extra: [para(s("onb_ask_name"), "strong"), input],
      buttons: [{ label: s("onb_next"), primary: true, onClick: submit }],
    });
    invoke("focus_window").catch(() => {});
    setTimeout(() => input.focus(), 120);
  } else if (step === "first_letter") {
    say(s("onb_drop_letter"), {
      kind: "onboarding", sticky: true, replace: true,
      extra: [para(s("onb_privacy"), "privacy")],
      buttons: [
        { label: s("btn_pick_letter"), primary: true, onClick: () => send({ type: "pick_file" }) },
        { label: s("btn_onb_later"), primary: false, onClick: () => { hideBubble(); send({ type: "onboarding_answer", step: "first_letter", value: "later" }); } },
      ],
    });
  } else if (step === "ask_state") {
    const answer = (v) => { hideBubble(); send({ type: "onboarding_answer", step: "ask_state", value: v }); };
    const states = s("states_list").split("|");
    say(s("onb_ask_state"), {
      kind: "onboarding", sticky: true, replace: true,
      buttons: [
        { label: s("btn_yes"), primary: true, onClick: () => answer(states[9] || "Nordrhein-Westfalen") },
        { label: s("btn_elsewhere"), primary: false, onClick: () => {
          const sel = document.createElement("select");
          sel.setAttribute("aria-label", s("onb_state_pick"));
          for (const name of states) sel.append(new Option(name, name));
          showBubble(s("onb_state_pick"), {
            kind: "onboarding", sticky: true, extra: [sel],
            buttons: [{ label: s("onb_next"), primary: true, onClick: () => answer(sel.value) }],
          });
          invoke("focus_window").catch(() => {});
        } },
      ],
    });
  } else if (st.bubbleKind === "onboarding") {
    hideBubble();
  }
}

// ---------- events from core ----------
function onUi(ev) {
  switch (ev.type) {
    case "pip":
      setState(ev.state);
      // reading the first letter: the invitation has done its job
      if (ev.state === "thinking" && st.bubbleKind === "onboarding" && st.onboarding === "first_letter") hideBubble();
      break;
    case "bubble": {
      const qid = ev.question_id;
      say(ev.text, {
        buttons: ev.buttons.map((b) => ({
          label: b.label, primary: b.primary,
          onClick: () => { hideBubble(); runButton(b.id, qid); },
        })),
        sticky: !!qid,
        kind: qid ? "question" : undefined,
        qid,
      });
      break;
    }
    case "progress": setProgress(ev.text); break;
    case "point_at":
      if (st.acting) st.marks.push(physRectToCss(ev.rect));
      pointAt(ev.rect, ev.label);
      break;
    case "point_clear": hideTarget(); break;
    case "fly_to": flyNear(ev.x, ev.y); break;
    case "fly_home": flyHome(); break;
    case "approval_request": showCard(ev.card); break;
    case "approval_closed": if (st.card === ev.id) hideCard(); break;
    case "reminder": showReminder(ev); break;
    case "onboarding": onboarding(ev.step); break;
    // The app follows a core error with its own "error" (with the right Nochmal); this one only informs.
    case "error": errorBubble(ev.text, null, false); break;
    default: break; // speak / notify are handled in Rust
  }
}

let dragPrevBubble = false;
function onApp(ev) {
  switch (ev.type) {
    case "acting":
      st.acting = ev.on;
      el.stop.hidden = !ev.on;
      if (ev.on) { closeAsk(); el.panel.hidden = true; st.marks = []; }
      else {
        setProgress(""); hideCard(); st.marks = [];
        // Questions of the ended run wait for nobody: drop them from the queue, keep the shown one readable.
        st.queue = st.queue.filter((q) => !q.opts.qid);
        if (st.bubbleKind === "question") {
          st.bubbleSticky = false;
          const next = st.queue.shift();
          if (next) showBubble(next.text, next.opts); else showBubble(el.bText.textContent);
        }
      }
      layout();
      break;
    case "paused":
      st.paused = ev.on;
      if (ev.on) { closeAsk(); hideTarget(); if (!st.home) flyHome(); }
      break;
    case "drag":
      if (ev.state === "enter") {
        st.dragging = true;
        el.pip.classList.add("dragging");
        st.pose = "listen";
        showPose();
        dragPrevBubble = st.bubbleKind === "onboarding";
        if (!dragPrevBubble && !st.bubbleSticky) showBubble(s("drop_hint"), { sticky: true, kind: "drop" });
      } else {
        st.dragging = false;
        el.pip.classList.remove("dragging");
        st.pose = null;
        showPose();
        if (st.bubbleKind === "drop") { st.bubbleSticky = false; hideBubble(); }
      }
      break;
    case "deadlines": showPanel(ev.items); break;
    case "error": errorBubble(ev.text, ev.retry, ev.pick); break;
    case "timed": {
      const text = ev.text;
      say(text, { buttons: ev.buttons.map((b) => ({ label: b.label, primary: b.primary, onClick: () => { hideBubble(); runButton(b.id); } })) });
      setTimeout(() => { if (!el.bubble.hidden && el.bText.textContent === text) { st.bubbleSticky = false; showBubble(text); } }, ev.ttl_secs * 1000);
      break;
    }
    case "setup": showSetup(ev.text); break;
    default: break;
  }
}

// ---------- setup: paste the key ----------
function showSetup(text) {
  const label = para(s("setup_key_label"), "strong");
  const input = document.createElement("input");
  input.type = "password";
  input.autocomplete = "off";
  input.setAttribute("aria-label", s("setup_key_label"));
  const err = para("", "setup-error");
  err.hidden = true;
  const save = () => {
    const key = input.value.trim();
    if (!key) { input.focus(); return; }
    invoke("save_key", { key }).then(() => { input.value = ""; }).catch((msg) => { err.textContent = String(msg); err.hidden = false; layout(); });
  };
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") save(); });
  say(text, { kind: "setup", sticky: true, replace: true, extra: [label, input, err], buttons: [{ label: s("setup_key_save"), primary: true, onClick: save }] });
  invoke("focus_window").catch(() => {});
  setTimeout(() => input.focus(), 120);
}

// ---------- input on Pip ----------
el.pip.addEventListener("click", () => {
  if (st.dragging) return;
  if (st.paused) { invoke("wake").catch(() => {}); return; }
  if (st.onboarding || st.acting || st.card) { hop(); return; }
  if (el.ask.hidden) openAsk(); else closeAsk();
});
window.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (st.acting) { stopNow(); return; }
  if (st.card) answerCard("no");
  if (!el.ask.hidden) closeAsk();
  else if (!el.panel.hidden) { el.panel.hidden = true; layout(); }
  else if (el.bubble.classList.contains("closable") || st.bubbleKind === "error") hideBubble();
});

// ---------- start ----------
function applyStaticText() {
  $("ask-title").textContent = s("ask_what");
  el.askInput.placeholder = s("ask_placeholder");
  el.askMic.querySelector("span").textContent = s("btn_mic");
  $("ask-go").textContent = s("btn_go");
  $("ask-letter").querySelector("span").textContent = s("btn_pick_letter");
  $("ask-deadlines").querySelector("span").textContent = s("tray_deadlines");
  $("ask-pause").querySelector("span").textContent = s("btn_pause_link");
  el.bClose.setAttribute("aria-label", s("btn_close"));
  fillChips();
  $("ask-close").setAttribute("aria-label", s("btn_close"));
  $("panel-close").textContent = s("btn_close");
  el.stop.querySelector("span").textContent = s("btn_stop");
}

async function refreshInfo() {
  try { info = await invoke("overlay_info"); } catch (e) { console.warn("overlay_info", e); }
}

window.addEventListener("resize", async () => {
  await refreshInfo();
  if (st.home) { const h = homePos(); placePip(h.x, h.y); } else layout();
});

async function start() {
  try { S = await invoke("ui_strings"); } catch (e) { console.warn("ui_strings", e); }
  await refreshInfo();
  applyStaticText();
  await TAURI.event.listen("ui", ({ payload }) => onUi(payload));
  await TAURI.event.listen("app", ({ payload }) => onApp(payload));
  // Pip flies in from the right edge.
  const h = homePos();
  placePip(innerWidth + 20, h.y - 30);
  setState("idle");
  await flyTo(h.x, h.y, 900);
  invoke("ui_ready").catch((e) => console.warn("ui_ready", e));
}
start();
