// Pip overlay. Listens to core events ("ui") and app events ("app"); sends UserCommand via
// invoke("ui_command"). Coordinates from core are physical screen pixels; the page works in CSS px.
"use strict";

const TAURI = window.__TAURI__;
const invoke = (cmd, args) => TAURI.core.invoke(cmd, args);
const jsLog = (level, msg) => { try { invoke("js_log", { level, msg: String(msg) }).catch(() => {}); } catch (_) {} };
window.addEventListener("error", (e) => jsLog("error", `${e.message} at ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => jsLog("error", `promise: ${e.reason}`));
const send = (cmd) => invoke("ui_command", { cmd }).catch((e) => jsLog("warn", `ui_command ${cmd.type}: ${e}`));
const $ = (id) => document.getElementById(id);

const PNG = { idle: "idle", listening: "listen", thinking: "think", pointing: "point", acting: "point",
  asking: "ask", happy: "happy", sleeping: "sleep", error: "ask" };
const P = 96;          // Pip size (CSS px)
const GAP = 24;        // distance between Pip and a target
const EDGE = 8;        // keep this far from screen edges
const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const Peek = window.Peek;  // ui/peek.js: rest / hover / up state machine and geometry
const PIP_TITLE = "Click to ask me. Hold to talk to me.";
let peekSecs = 10;         // PIP_PEEK_SECS from the app: quiet this long -> back to the edge (0 = never)

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
  actingEndedAt: -1e9,   // performance.now() when the last task ended (a "happy" right after it = celebrate)
  paused: false,
  dragging: false,
  onboarding: null,
  bubbleKind: null,
  bubbleQid: null,       // question id of the bubble on screen (a guard bubble asked by a run)
  guardPose: false,      // the guard bubble put Pip in the "ask" pose; cleared when it closes
  bubbleSticky: false,   // the bubble waits for an answer (buttons, question, reminder, onboarding)
  bubbleTimer: 0,
  queue: [],             // sticky bubbles waiting for the one on screen: { text, opts }
  shownOf: 0,            // how many sticky bubbles came in this row ("Nachricht 1 von 2")
  noteTimer: 0,
  marks: [],             // places Pip filled in during this run (CSS rects): the card must not cover them
  card: null,
  lastSpoke: 0,
  flight: 0,
  mode: "peek",         // "peek" (rests as a head at the screen edge) | "always" (tray "Always show Pip")
  peek: "up",           // Peek state: "rest" | "hover" | "up"
  quietSince: 0,        // performance.now() since nothing is shown and Pip is idle (0 = not quiet)
  lastPointer: 0,       // last pointer event over the overlay (a hover whose leave never came ends anyway)
  tipTimer: 0,
  slideTimer: 0,
};

const el = {
  pip: $("pip"), body: $("pip-body"), img: $("pip-img"),
  hl: $("highlight"), hlLabel: $("hl-label"), arrow: $("arrow"),
  col: $("column"), dock: $("dock"),
  bubble: $("bubble"), bText: $("bubble-text"), bExtra: $("bubble-extra"), bButtons: $("bubble-buttons"), bAfter: $("bubble-after"),
  bClose: $("bubble-close"), bCount: $("bubble-count"), note: $("note"),
  progress: $("progress"), pText: $("progress-text"), stop: $("stop"),
  ask: $("ask"), askInput: $("ask-input"), askMic: $("ask-mic"),
  card: $("card"), panel: $("panel"), confetti: $("confetti"),
};

// ---------- geometry ----------
const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
// How much of the overlay's bottom the taskbar covers (0: the real overlay already is the work area).
// The preview can say it (info.taskbar_css); no screen data at all -> NaN -> Peek.geometry's 48 px.
function taskbarCss() {
  if (Number.isFinite(info.taskbar_css)) return info.taskbar_css;
  return Peek.taskbarOverlap(window.screen, window.screenY || 0, innerHeight);
}
const peekGeo = (state) => Peek.geometry(state, P, innerHeight, taskbarCss(), innerWidth);
const workBottom = () => peekGeo("up").bottom;
const homePos = () => { const g = peekGeo("up"); return { x: g.x, y: g.y }; };
/** Where Pip sits at home in the current peek state (rest/hover: only the head above the work-area bottom). */
const restPos = () => { const g = peekGeo(st.peek); return { x: g.x, y: g.y }; };
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
  const y = clamp(r.y + r.h / 2 - P * 0.55, EDGE, workBottom() - P - EDGE);
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
  endSlide();
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
      scheduleHits(); // where he landed (the column moved with him)
      done();
    };
    st.flight = requestAnimationFrame(frame);
  });
}

function flyHome() {
  hideTarget();
  st.colSide = null;
  el.pip.classList.remove("mirror", "away");
  const h = restPos();
  const p = flyTo(h.x, h.y, st.home ? undefined : 500);
  st.home = true;
  return p;
}

// ---------- peek: Pip rests as a head at the screen edge ----------
// Only at home: while he points at something he is "up" anyway. The slide is a CSS transition (400 ms ease-out,
// none with reduced motion); the body below the work-area bottom is clipped (never over the taskbar).
function endSlide() {
  clearTimeout(st.slideTimer);
  el.pip.classList.remove("sliding");
}
function updateClip() {
  const low = st.home && st.peek !== "up";
  const hidden = low ? Math.max(0, st.pos.y + P - workBottom()) : 0;
  el.pip.style.clipPath = hidden > 0 ? `inset(-60px -60px ${hidden}px -60px)` : "";
}
function slideTo(x, y) {
  cancelAnimationFrame(st.flight);
  el.pip.classList.remove("flying");
  el.pip.style.rotate = "";
  endSlide();
  const down = y > st.pos.y;
  const animate = !reduceMotion && Math.hypot(x - st.pos.x, y - st.pos.y) >= 2;
  if (animate) {
    el.pip.classList.add("sliding");
    // once he is there: cut the body, and report his real rect (the hits were taken mid-slide)
    st.slideTimer = setTimeout(() => { el.pip.classList.remove("sliding"); updateClip(); scheduleHits(); }, 420);
  }
  placePip(x, y);
  // going down, cut the body only once he is there (cutting early would clip him mid-slide)
  if (!animate || !down) updateClip();
}

function setPeek(next) {
  if (next === st.peek) return;
  st.peek = next;
  el.pip.classList.toggle("peek-rest", next === "rest");
  el.pip.classList.toggle("peek-hover", next === "hover");
  // the native "Click to ask me. Hold to talk to me." only when he is fully out (no second tooltip at the edge)
  el.pip.title = next === "up" ? PIP_TITLE : "";
  clearTimeout(st.tipTimer);
  if (next === "hover") st.tipTimer = setTimeout(showPeekTip, 1000);
  else { hidePeekTip(); el.img.style.translate = ""; }
  if (next === "up") st.quietSince = 0;
  if (!st.home) { updateClip(); return; }
  const p = restPos();
  slideTo(p.x, p.y);
}

/** Something is about to be shown: Pip comes fully out first (no-op when he is up). */
function rise() {
  st.quietSince = 0;
  if (st.peek !== "up") setPeek(Peek.next(st.peek, "needs_attention"));
}

function showPeekTip() {
  if (st.peek !== "hover") return;
  const tip = $("peek-tip");
  tip.textContent = s("peek_hint");
  tip.hidden = false;
  const g = peekGeo("hover");
  const w = tip.offsetWidth, h = tip.offsetHeight;
  tip.style.left = `${clamp(st.pos.x - w - 12, EDGE, innerWidth - w - EDGE)}px`;
  tip.style.top = `${clamp(g.y + g.visibleH / 2 - h / 2, EDGE, g.bottom - h - 4)}px`;
}
function hidePeekTip() {
  clearTimeout(st.tipTimer);
  $("peek-tip").hidden = true;
}

// Eyes follow the cursor: the whole picture moves up to 3 px toward it.
function lookAt(x, y) {
  const cx = st.pos.x + P / 2, cy = st.pos.y + P * 0.3;
  el.img.style.translate = `${clamp((x - cx) / 30, -3, 3).toFixed(1)}px ${clamp((y - cy) / 30, -3, 3).toFixed(1)}px`;
}

function applyMode(mode) {
  st.mode = mode === "always" ? "always" : "peek";
  setPeek(Peek.next(st.peek, st.mode === "always" ? "mode_always" : "mode_peek"));
  st.quietSince = 0;
}

const somethingShown = () => [el.bubble, el.card, el.ask, el.panel, el.progress, el.note, el.stop].some((n) => !n.hidden);
// Peek clock: quiet (nothing shown, idle, at home) for peekSecs -> back to the edge. A hover whose leave never
// arrived (the pointer left the hit rect too fast) ends after 5 s without a pointer event.
setInterval(() => {
  const now = performance.now();
  if (st.peek === "hover" && press.id === null && now - st.lastPointer > 5000) setPeek(Peek.next(st.peek, "hover_out"));
  if (st.mode !== "peek" || st.peek !== "up") { st.quietSince = 0; return; }
  const quiet = st.pipState === "idle" && st.home && !somethingShown() && !st.acting && !st.card && !st.onboarding
    && !st.dragging && !st.paused && !micDown && press.id === null && !el.pip.classList.contains("flying");
  if (!quiet) { st.quietSince = 0; return; }
  if (!st.quietSince) { st.quietSince = now; return; } // "quiet": the clock starts
  if (peekSecs > 0 && now - st.quietSince >= peekSecs * 1000) setPeek(Peek.next(st.peek, "timeout"));
}, 250);

function showPose() {
  const name = st.pose || PNG[st.pipState] || "idle";
  const src = `pip/${name}.png`;
  if (!el.img.src.endsWith(src)) el.img.src = src;
}

function setState(state) {
  const prev = st.pipState;
  st.pipState = state;
  if (state !== "idle") rise();
  el.pip.className = el.pip.className.replace(/\bstate-\S+/g, "").trim() + ` state-${state}`;
  showPose();
  // A task just finished (the daily brief and a read letter are happy too, but never while acting): celebrate.
  // "acting off" comes straight from the app while "happy" goes through the core bus, so it may arrive a few
  // ms earlier: a happy right after acting still counts.
  const justActed = st.acting || performance.now() - st.actingEndedAt < 1500;
  if (state === "happy" && prev !== "happy") { if (justActed) celebrate(); else hop(); }
}

function hop(big) {
  if (st.peek === "rest") return; // resting: only breathing
  // the done bubble arrives right before or after the celebration: its small hop must not cut the big one
  if (!big && el.body.getAnimations().some((a) => a.animationName === "hop-big" && a.playState === "running")) return;
  el.body.classList.remove("hop", "hop-big");
  void el.body.offsetWidth;
  el.body.classList.add(big ? "hop-big" : "hop");
}

// Confetti from Pip's middle and a big hop. Pure decoration: pointer-events:none, not in reportHits(),
// gone within 1.2 s (the model's screenshots do not mask it). At most once per 3 s; nothing with reduced motion.
const CONFETTI_COLORS = ["#0866ff", "#7b5cff", "#f2a33a", "#2ecc71"];
let lastCelebrate = -1e9;
function celebrate() {
  const now = performance.now();
  if (reduceMotion || now - lastCelebrate < 3000) { hop(); return; }
  lastCelebrate = now;
  hop(true);
  const cx = st.pos.x + P / 2, cy = st.pos.y + P / 2;
  const pieces = [];
  for (let i = 0; i < 18; i++) {
    const c = document.createElement("i");
    c.style.left = `${cx - 5}px`;
    c.style.top = `${cy - 7}px`;
    c.style.background = CONFETTI_COLORS[i % CONFETTI_COLORS.length];
    let dx = Math.round(-160 + Math.random() * 320);
    if (cx + dx < EDGE || cx + dx > innerWidth - EDGE) dx = -dx; // at the screen edge: throw inward
    c.style.setProperty("--dx", `${dx}px`);
    c.style.setProperty("--dy", `${Math.round(-220 + Math.random() * 160)}px`);
    c.style.setProperty("--r", `${Math.round(-540 + Math.random() * 1080)}deg`);
    c.addEventListener("animationend", () => c.remove());
    pieces.push(c);
  }
  el.confetti.append(...pieces);
  // animationend never comes when animations are off: clean up anyway
  setTimeout(() => pieces.forEach((c) => c.remove()), 1200);
}

(function blinkLoop() {
  setTimeout(() => {
    if (st.pipState !== "sleeping" && !st.dragging && st.peek !== "rest") {
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
  rise();
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
  rise();
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
  const fitY = (y) => clamp(y, EDGE, workBottom() - EDGE - h);
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
    el.col.style.maxHeight = `${workBottom() - 2 * EDGE}px`;
  }
  scheduleHits();
}

// ---------- hit rects (click-through + screenshot masking) ----------
// The app lets clicks through everywhere outside these rects (ui/hits.js has the rules). They are sent after
// every layout, flight, slide and animation end, and again every second while anything is open: a lost or
// stale report must never leave a bubble's buttons click-through.
const Hits = window.Hits;
let hitsQueued = false;
let lastHits = "";
let lastHitsSent = 0;
let micDown = false;   // the mic button is held
function scheduleHits() {
  if (hitsQueued) return;
  hitsQueued = true;
  setTimeout(() => { hitsQueued = false; reportHits(); }, 40);
}
function hitScene() {
  const g = peekGeo(st.peek);
  return {
    pip: el.pip.hidden ? null : { box: rectOf(el.pip), home: st.home, peek: st.peek, head: { x: g.x, y: g.y, w: P, h: g.visibleH, bottom: g.bottom } },
    boxes: [el.bubble, el.card, el.ask, el.panel, el.progress, el.stop, el.note].map(rectOf),
    micDown,
    viewport: { w: innerWidth, h: innerHeight },
  };
}
/** again: send even if nothing changed (at most once a second). */
function reportHits(again) {
  const k = dpr();
  const css = Hits.cssRects(hitScene());
  const rects = Hits.toPhys(css, info, k);
  const json = JSON.stringify(rects);
  const now = performance.now();
  if (json === lastHits && !(again && now - lastHitsSent >= 1000)) return;
  lastHits = json;
  lastHitsSent = now;
  invoke("set_hit_rects", { css, dpr: k, rects })
    .catch((e) => { lastHits = ""; jsLog("warn", `set_hit_rects: ${e}`); send({ type: "hit_rects", rects }); });
}
// Animations move and scale boxes (the bubble pops in from 8 px lower): measure again once they end.
for (const ev of ["animationend", "animationcancel", "transitionend"]) document.addEventListener(ev, scheduleHits, true);
// Safety net: while anything is open or Pip is away, report every half second (sent when changed, else 1/s).
setInterval(() => {
  const open = Hits.anyOpen(hitScene().boxes);
  if (open || !st.home || micDown) reportHits(true); else reportHits(false);
}, 500);
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
  if (st.guardPose) {
    st.guardPose = false;
    if (st.pose === "ask") st.pose = null;
    showPose();
  }
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

function makeButton(label, primary, onClick, cls) {
  const b = document.createElement("button");
  b.className = (primary ? "btn primary" : "btn") + (cls ? ` ${cls}` : "");
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
  rise();
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
  rise();
  clearTimeout(st.bubbleTimer);
  // The same sentence again (core and app both confirm a stop): update quietly, no second pop.
  const same = !el.bubble.hidden && el.bText.textContent === text && st.bubbleKind === (opts.kind || "say");
  el.bubble.className = "bubble" + (opts.kind ? ` ${opts.kind}` : "");
  st.bubbleKind = opts.kind || "say";
  st.bubbleQid = opts.qid || null;
  // the guard bubble (also one shown later from the queue) puts Pip in the ask pose until it closes
  if (st.bubbleKind === "guard") { st.guardPose = true; st.pose = "ask"; showPose(); }
  const sticky = isSticky(opts);
  if (sticky && !st.bubbleSticky) st.shownOf += 1;
  if (!sticky) st.shownOf = 0;
  st.bubbleSticky = sticky;
  el.bText.textContent = text;
  decorateStats(text);
  el.bExtra.replaceChildren(...(opts.extra || []));
  el.bButtons.replaceChildren(...(opts.buttons || []).map((b) => makeButton(b.label, b.primary, b.onClick, b.cls)));
  el.bAfter.replaceChildren(...(opts.after || []));
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
// The sentences with numbers ("Done in 14 s. By hand ...", "2nd time on this site: ...", "... saved since we met.")
// become highlighted chips. Built from text nodes only: el.bText.textContent stays exactly `text` (the
// same-bubble check and the "timed" handler compare it). The icon comes from CSS ::before.
const STAT_TEMPLATES = [
  // [i18n key, kind, German, English]: S (UI_KEYS in bridge.rs) carries these; the copies are a safety net
  ["done_time", "time", "Fertig in {secs} s. Von Hand dauert das etwa {minutes} Minuten.", "Done in {secs} s. By hand this takes about {minutes} minutes."],
  ["done_time_one", "time", "Fertig in {secs} s. Von Hand dauert das etwa eine Minute.", "Done in {secs} s. By hand this takes about a minute."],
  ["done_total", "total", "Seit wir uns kennen, habe ich dir etwa {total} gespart.", "That's about {total} saved since we met."],
];
const escapeRe = (t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
let statPatterns = null;
function statRegexes() {
  if (statPatterns) return statPatterns;
  const pats = [];
  for (const [key, kind, de, en] of STAT_TEMPLATES) {
    for (const tpl of new Set([S[key], de, en].filter(Boolean))) {
      pats.push([kind, escapeRe(tpl).replace(/\\\{\w+\\\}/g, "(.+?)")]);
    }
  }
  pats.push(["fast", "\\d+(?:st|nd|rd|th) time on this site: [^]*?\\."]);
  pats.push(["fast", "Zum \\d+\\. Mal auf dieser Seite: [^]*?beim letzten Mal\\."]);
  // a stat sentence starts the text or follows a space
  statPatterns = pats.map(([kind, src]) => [kind, new RegExp(`(?<=^|\\s)${src}`, "g")]);
  return statPatterns;
}
function decorateStats(text) {
  const hits = [];
  for (const [kind, re] of statRegexes()) {
    re.lastIndex = 0;
    for (const m of text.matchAll(re)) hits.push({ start: m.index, end: m.index + m[0].length, kind });
  }
  if (!hits.length) return;
  hits.sort((a, b) => a.start - b.start || b.end - a.end);
  const parts = [];
  let at = 0;
  for (const h of hits) {
    if (h.start < at) continue; // overlaps an earlier chip
    if (h.start > at) parts.push(document.createTextNode(text.slice(at, h.start)));
    const span = document.createElement("span");
    span.className = `stat ${h.kind}`;
    span.textContent = text.slice(h.start, h.end);
    parts.push(span);
    at = h.end;
  }
  if (at < text.length) parts.push(document.createTextNode(text.slice(at)));
  el.bText.replaceChildren(...parts);
}

el.bubble.addEventListener("click", (e) => {
  if (el.bubble.classList.contains("closable") && !e.target.closest("button")) hideBubble();
});
el.bClose.addEventListener("click", () => hideBubble());

/** Core / app buttons: "cmd:<type>" -> UserCommand, "app:<action>" -> app_action, "close" -> just close. */
// A question without choices: a text field and one button that sends what was typed.
function openQuestion(ev, qid) {
  const input = document.createElement("input");
  input.type = "text";
  input.maxLength = 200;
  input.setAttribute("aria-label", ev.text);
  const submit = () => {
    const v = input.value.trim();
    if (!v) { input.focus(); return; }
    hideBubble();
    invoke("release_focus").catch(() => {});
    send({ type: "answer", question_id: qid, choice: v });
  };
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") submit(); });
  const b = ev.buttons[0];
  say(ev.text, {
    kind: "question", sticky: true, qid, extra: [input],
    buttons: [{ label: b.label, primary: true, onClick: submit }],
  });
  invoke("focus_window").catch(() => {});
  setTimeout(() => input.focus(), 120);
}

// The guard bubble (a page that only looks like the real one). The core sends an ordinary bubble whose button
// ids start with "guard:"; its text is several lines: the title sentence, one line per warning sign, and the
// honest closing sentence. The title is the bubble text (the core speaks it), the signs become amber chips.
// Sticky like a question: it never closes by itself and a later plain sentence goes to the note line.
const isGuard = (buttons) => (buttons || []).some((b) => String(b.id).startsWith("guard:"));

function guardBubble(ev, qid) {
  const lines = String(ev.text).split("\n").map((l) => l.trim()).filter(Boolean);
  const title = lines[0] || String(ev.text);
  const signs = lines.length > 2 ? lines.slice(1, -1) : [];
  const closing = lines.length > 1 ? lines[lines.length - 1] : "";
  const extra = [];
  if (signs.length) {
    const ul = document.createElement("ul");
    ul.className = "signs";
    for (const sign of signs) {
      const li = document.createElement("li");
      li.className = "sign";
      li.textContent = sign;
      ul.append(li);
    }
    extra.push(ul);
  }
  if (closing) extra.push(para(closing, "guard-close"));
  // The safe way out first and big; "continue anyway" last, small and quiet.
  const go = ev.buttons.filter((b) => b.id !== "guard:continue");
  const cont = ev.buttons.filter((b) => b.id === "guard:continue");
  const buttons = [
    ...go.map((b, i) => ({ label: b.label, primary: i === 0, cls: i === 0 ? "big safe" : "", id: b.id })),
    ...cont.map((b) => ({ label: b.label, primary: false, cls: "minor", id: b.id })),
  ].map((b) => ({ ...b, onClick: () => { hideBubble(); runButton(b.id, qid); } }));
  say(title, { kind: "guard", sticky: true, qid, buttons, extra });
}

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
  if (text) rise();
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
  rise();
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
  const remembered = card.remembered || [];
  $("card-remembered").hidden = !remembered.length;
  $("card-remembered").replaceChildren(...remembered.map((line) => {
    const li = document.createElement("li");
    li.textContent = line;
    return li;
  }));
  // "Don't ask again for <site>": only on plain send cards on a known site.
  $("card-remember").hidden = !card.remember_label;
  $("card-remember-text").textContent = card.remember_label || "";
  $("card-remember-box").checked = false;
  $("card-yes").textContent = card.yes_label;
  $("card-no").textContent = card.no_label;
  el.card.hidden = false;
  // The card says it all and has its own "Noch nicht": the status line must never sit on its buttons.
  el.dock.hidden = true;
  st.lastSpoke = performance.now();
  hop();
  layout();
}
function hideCard() {
  st.card = null;
  el.card.hidden = true;
  el.dock.hidden = false;
  layout();
}
function answerCard(answer) {
  if (!st.card) return;
  const remember = answer === "yes" && !$("card-remember").hidden && $("card-remember-box").checked;
  send({ type: "approve", id: st.card, answer, remember_site: remember });
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
  rise();
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
$("ask-knows").addEventListener("click", () => { closeAsk(); invoke("open_knowledge").catch(() => {}); });
$("ask-card").addEventListener("click", () => { closeAsk(); invoke("app_action", { action: "my_card" }).catch(() => {}); });
$("ask-pause").addEventListener("click", () => { closeAsk(); invoke("app_action", { action: "pause" }).catch(() => {}); });
// "Look at my screen": Pip looks at what is open and offers concrete help (works for anything).
function lookAtScreen() {
  send({ type: "text", text: s("task_look_screen") });
}

function fillChips() {
  const chips = [
    ["btn_look_screen", lookAtScreen],
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
  rise();
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

// ---------- "What Pip knows" (same panel) ----------
function showKnowledge(ev) {
  rise();
  closeAsk();
  $("panel-title").textContent = ev.title;
  const rows = [];
  for (const g of ev.groups) {
    const head = document.createElement("li");
    head.className = "group";
    const h = document.createElement("h3"); h.textContent = g.title;
    head.append(h);
    rows.push(head);
    if (!g.items.length) { const li = document.createElement("li"); li.className = "empty"; li.textContent = g.empty; rows.push(li); continue; }
    for (const it of g.items) {
      const li = document.createElement("li");
      const b = document.createElement("b"); b.textContent = it.title;
      li.append(b);
      if (it.detail) li.append(para(it.detail, "why"));
      const row = document.createElement("div"); row.className = "row-buttons";
      row.append(makeButton(s("btn_forget"), false, () => send({ type: "forget", kind: it.kind, id: it.id, task: it.task || "" })));
      li.append(row);
      rows.push(li);
    }
  }
  $("panel-list").replaceChildren(...rows);
  el.panel.hidden = false;
  layout();
}

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
      hideBubble();
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
    const firstJob = (value, then) => () => {
      hideBubble();
      send({ type: "onboarding_answer", step: "first_letter", value });
      then();
    };
    say(s("onb_drop_letter"), {
      kind: "onboarding", sticky: true, replace: true,
      buttons: [
        { label: `👀 ${s("btn_look_screen")}`, primary: true, onClick: firstJob("look", lookAtScreen) },
        { label: `📄 ${s("btn_read_letter")}`, primary: false, onClick: firstJob("letter", () => send({ type: "pick_file" })) },
        { label: `💬 ${s("btn_type_or_say")}`, primary: false, onClick: firstJob("later_quiet", () => openAsk()) },
      ],
      after: [para(s("onb_privacy"), "privacy")],
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
      if (qid && ev.buttons.some((b) => b.id === "__text__")) { openQuestion(ev, qid); break; }
      if (isGuard(ev.buttons)) { guardBubble(ev, qid); break; }
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
      if (ev.on) rise();
      st.acting = ev.on;
      if (!ev.on) st.actingEndedAt = performance.now();
      el.stop.hidden = !ev.on;
      if (ev.on) {
        closeAsk(); el.panel.hidden = true; st.marks = [];
        // a warning left over from an ended run must not hold the new run's questions in the queue
        if (st.bubbleKind === "guard" && !st.bubbleQid) hideBubble();
      }
      else {
        setProgress(""); hideCard(); st.marks = [];
        // Questions of the ended run wait for nobody: drop them from the queue, keep the shown one readable.
        st.queue = st.queue.filter((q) => !q.opts.qid);
        if (st.bubbleKind === "question") {
          st.bubbleSticky = false;
          const next = st.queue.shift();
          if (next) showBubble(next.text, next.opts); else showBubble(el.bText.textContent);
        } else if (st.bubbleKind === "guard" && st.bubbleQid) {
          // The run that asked has ended: the warning stays readable, its buttons answer nobody any more.
          st.bubbleQid = null;
          el.bButtons.replaceChildren();
          el.bClose.hidden = false;
          el.bubble.classList.add("has-x");
          layout();
        }
      }
      layout();
      break;
    case "paused":
      if (ev.on) rise();
      st.paused = ev.on;
      if (ev.on) { closeAsk(); hideTarget(); if (!st.home) flyHome(); }
      break;
    case "drag":
      if (ev.state === "enter") {
        rise();
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
    case "knowledge": showKnowledge(ev); break;
    case "error": errorBubble(ev.text, ev.retry, ev.pick); break;
    case "timed": {
      const text = ev.text;
      say(text, { buttons: ev.buttons.map((b) => ({ label: b.label, primary: b.primary, onClick: () => { hideBubble(); runButton(b.id); } })) });
      setTimeout(() => { if (!el.bubble.hidden && el.bText.textContent === text) { st.bubbleSticky = false; showBubble(text); } }, ev.ttl_secs * 1000);
      break;
    }
    case "setup": showSetup(ev.text); break;
    case "mode": applyMode(ev.mode); break;
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

// ---------- input on Pip: click = ask, hold = talk ----------
const HOLD_MS = 350;
const HOLD_SLOP = 8;   // px the pointer may move before a press is no longer a click or hold
const press = { id: null, x: 0, y: 0, timer: 0, holding: false, used: false };
function pipClick() {
  if (st.dragging) return;
  if (st.peek !== "up") setPeek(Peek.next(st.peek, "click"));
  if (st.paused) { invoke("wake").catch(() => {}); return; }
  if (st.onboarding || st.acting || st.card) { hop(); return; }
  if (el.ask.hidden) openAsk(); else closeAsk();
}
function holdStart() {
  press.timer = 0;
  press.used = true;
  if (st.paused || st.onboarding || st.acting || st.card || st.dragging) { hop(); return; }
  press.holding = true;
  rise(); // held at the edge: he comes out and listens
  micDown = true; // keep the release inside the overlay, like the mic button
  scheduleHits();
  closeAsk();
  el.pip.classList.add("holding");
  st.pose = "listen";
  showPose();
  send({ type: "voice_start" });
}
el.pip.addEventListener("pointerdown", (e) => {
  if (e.button !== 0) return;
  // A release that never reached us (capture lost, window hidden) must not leave Pip deaf: end that press first.
  if (press.id !== null) pipUp({ pointerId: press.id, type: "pointercancel" });
  e.preventDefault();
  try { el.pip.setPointerCapture(e.pointerId); } catch (_) { /* synthetic pointer */ }
  Object.assign(press, { id: e.pointerId, x: e.clientX, y: e.clientY, holding: false, used: false });
  press.timer = setTimeout(holdStart, HOLD_MS);
});
el.pip.addEventListener("pointermove", (e) => {
  if (e.pointerId !== press.id || !press.timer) return;
  if (Math.hypot(e.clientX - press.x, e.clientY - press.y) > HOLD_SLOP) { clearTimeout(press.timer); press.timer = 0; press.used = true; }
});
const pipUp = (e) => {
  if (e.pointerId !== press.id) return;
  const wasTimer = !!press.timer;
  clearTimeout(press.timer);
  press.timer = 0;
  press.id = null;
  if (press.holding) {
    press.holding = false;
    micDown = false;
    scheduleHits();
    el.pip.classList.remove("holding");
    st.pose = null;
    showPose();
    send({ type: "voice_stop" });
  } else if (wasTimer && e.type === "pointerup" && !press.used) pipClick();
};
el.pip.addEventListener("pointerup", pipUp);
// Hover at the edge: the head rises a little, the eyes follow, "Need help? Click me" after 1 s.
el.pip.addEventListener("pointerenter", () => {
  st.lastPointer = performance.now();
  if (st.mode === "peek" && st.home) setPeek(Peek.next(st.peek, "hover_in"));
});
el.pip.addEventListener("pointerleave", () => {
  if (press.id !== null) return; // a held press keeps him where he is
  setPeek(Peek.next(st.peek, "hover_out"));
  el.img.style.translate = "";
});
document.addEventListener("pointermove", (e) => {
  st.lastPointer = performance.now();
  if (st.peek === "hover") lookAt(e.clientX, e.clientY);
  // the 5 s fallback lowered him under a still cursor: the next move over the head raises him again
  else if (st.peek === "rest" && st.mode === "peek" && st.home && e.target instanceof Element && e.target.closest("#pip")) {
    setPeek(Peek.next(st.peek, "hover_in"));
  }
});
el.pip.addEventListener("pointercancel", pipUp);
el.pip.addEventListener("lostpointercapture", pipUp); // after a normal release this is a no-op (press.id is null)
window.addEventListener("keydown", (e) => {
  // Enter on the card = the primary button (only when Pip's window has the keyboard: an Enter typed into
  // another program never answers a card).
  const otherButton = document.activeElement instanceof HTMLButtonElement && document.activeElement.id !== "card-yes";
  if (e.key === "Enter" && st.card && !e.repeat && !otherButton) { e.preventDefault(); answerCard("yes"); return; }
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
  $("ask-knows").querySelector("span").textContent = s("tray_knowledge");
  $("ask-card").querySelector("span").textContent = S.btn_my_card || "What Pip did for me";
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
  if (st.home) { const h = restPos(); placePip(h.x, h.y); updateClip(); } else layout();
});

async function start() {
  try { S = await invoke("ui_strings"); } catch (e) { console.warn("ui_strings", e); }
  await refreshInfo();
  if (Number.isFinite(info.peek_secs)) peekSecs = info.peek_secs;
  applyStaticText();
  await TAURI.event.listen("ui", ({ payload }) => onUi(payload));
  await TAURI.event.listen("app", ({ payload }) => onApp(payload));
  // Pip flies in from the right edge: to his resting place at the edge (peek) or home (always).
  applyMode(info.mode);
  if (st.mode === "peek") setPeek("rest");
  const h = restPos();
  placePip(innerWidth + 20, homePos().y - 30);
  setState("idle");
  await flyTo(h.x, h.y, 900);
  updateClip();
  invoke("ui_ready").catch((e) => console.warn("ui_ready", e));
}
start();
