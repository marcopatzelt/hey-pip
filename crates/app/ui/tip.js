// "Good to know" bubbles: a curated rule with its source, and the next steps [Remind me on …] [Write a short
// question] [Why?]. The app sends an ordinary bubble: its text is the tip ("Good to know: …") and, on the last line,
// the source ("§ 46 Satz 2 SGB V"); its buttons have ids "app:tip_…". Pure functions only (no DOM): main.js draws
// the bubble, ui/peek.test.mjs tests them with node.
"use strict";

(function (root) {
  // The buttons that make a bubble a tip bubble (tip, tip after "Why?", the "Why?" bubble itself). The question
  // offer ([Yes, send it] [Not now]) is an ordinary question bubble.
  const TIP_BUTTONS = ["app:tip_remind", "app:tip_question", "app:tip_why", "app:tip_ok"];
  // "Good to know: …" / "Gut zu wissen: …": a short heading before the first colon becomes the label.
  const LABEL = /^([^:\n]{2,24}):\s+/;

  /** True when the bubble is a "Good to know" tip (or its "Why?"). */
  function isTip(buttons) {
    return (buttons || []).some((b) => b && TIP_BUTTONS.includes(String(b.id)));
  }

  /**
   * Split a tip bubble's text: { label, body, source }. The last line is the source (only when there are two
   * lines or more); a short heading before the first colon is the label ("Good to know"); body is the rest.
   */
  function parts(text) {
    const lines = String(text ?? "").split("\n").map((l) => l.trim()).filter(Boolean);
    const source = lines.length > 1 ? lines[lines.length - 1] : "";
    let body = (lines.length > 1 ? lines.slice(0, -1) : lines).join(" ");
    let label = "";
    const m = body.match(LABEL);
    // "Good to know: your insurer …" -> label "Good to know", text "Your insurer …"
    if (m) { label = m[1].trim(); body = body.slice(m[0].length); body = body.charAt(0).toUpperCase() + body.slice(1); }
    return { label, body, source };
  }

  const Tip = { TIP_BUTTONS, isTip, parts };
  root.Tip = Tip;
  if (typeof module !== "undefined" && module.exports) module.exports = Tip;
})(typeof window !== "undefined" ? window : globalThis);
