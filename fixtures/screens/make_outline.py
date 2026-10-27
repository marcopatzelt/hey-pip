#!/usr/bin/env python3
"""Screenshot + PAGE ELEMENTS (outline) + target boxes of one page state, so page variants need no hand-written outlines.

Run: python3 fixtures/screens/make_outline.py <page-url-or-file> <stem> [?query] [--target name=#css ...]
                                              [--out DIR] [--no-outline] [--note TEXT] [--click #css]
                                              [--scroll-bottom]

Drives headless Firefox through /snap/bin/geckodriver (plain WebDriver over urllib, no selenium). The viewport is
exactly 1600x1000; the PNG is the viewport, so the rects in the outline are the same pixels as the picture.
Writes into DIR (default: this folder):
  <stem>.png           the viewport screenshot
  <stem>.outline.json  the PAGE ELEMENTS as Windows UI Automation would report them, in the shape of
                       demo-uni.outline.json (note, image, items[]); left out with --no-outline (a "-filled" state
                       keeps the list Pip typed into, so it must not bring its own)
  <stem>.boxes.json    {name: box xyxy} for every --target name=<css selector> (fit tests, click targets)
--click <css> clicks that element before the capture (a cookie button, a "Next" button). One page load per call.
--scroll-bottom scrolls the page to its end before the capture (the `-scrolled` states of a tall page): the rects are
then those of the scrolled viewport, elements above it are negative and `offscreen`.
An element that is laid out but `visibility:hidden` is listed with `offscreen: true` (UI Automation lists it, the
picture does not show it); a heading's `aria-label` is its name.
`capture(..., labels={name: (sam_query, description)})` writes those into the boxes (make_screens.py does this);
without, the boxes get a generic query and the target name as description.
"""

import base64
import json
import pathlib
import socket
import subprocess
import sys
import time
import urllib.request

OUT = pathlib.Path(__file__).resolve().parent
GECKO = "/snap/bin/geckodriver"
W, H = 1600, 1000

# Walks the page in document order. Roles as the app's UI Automation reader names them.
WALK_JS = r"""
const W = arguments[0], H = arguments[1];
const clean = (s) => (s || "").replace(/\s+/g, " ").trim();
const textOf = (el) => {
  // Visible text without aria-hidden parts (icons) and without the control's own value.
  let s = "";
  for (const n of el.childNodes) {
    if (n.nodeType === 3) s += n.textContent;
    else if (n.nodeType === 1 && n.getAttribute("aria-hidden") !== "true" && !["INPUT", "SELECT", "TEXTAREA", "SCRIPT", "STYLE"].includes(n.tagName)) s += " " + textOf(n);
  }
  return clean(s);
};
const visible = (el) => el.getClientRects().length > 0 && getComputedStyle(el).display !== "none";
// In the DOM and laid out, but not drawn (visibility:hidden): UI Automation still lists it, as off-screen.
const hidden = (el) => getComputedStyle(el).visibility === "hidden";
const labelFor = (el) => {
  if (el.id) {
    const l = document.querySelector(`label[for="${CSS.escape(el.id)}"]`);
    if (l) return l;
  }
  return el.closest("label");
};
const out = [];
const sel = "input,textarea,select,button,a[href],[role=button],h1,h2,h3,.error,[role=alert]";
for (const el of document.querySelectorAll(sel)) {
  const tag = el.tagName, type = (el.getAttribute("type") || "").toLowerCase();
  if (tag === "INPUT" && type === "hidden") continue;
  let role = null, name = "", value = null, box = el;
  const isPw = tag === "INPUT" && type === "password";
  if (tag === "TEXTAREA" || (tag === "INPUT" && ["", "text", "email", "number", "tel", "date", "time", "datetime-local", "month", "search", "url", "password"].includes(type))) {
    role = "edit";
    // Edge reports an empty date field as dd.mm.yyyy and an empty time field as --:--.
    const empty = { date: "dd.mm.yyyy", time: "--:--", "datetime-local": "dd.mm.yyyy --:--", month: "-------- ----" };
    value = isPw ? null : (!el.value && empty[type] ? empty[type] : el.value);
  } else if (tag === "SELECT") {
    role = "combo";
    value = el.selectedIndex >= 0 ? clean(el.options[el.selectedIndex].text) : "";
  } else if (tag === "INPUT" && ["submit", "button", "reset"].includes(type)) {
    role = "button"; name = clean(el.value);
  } else if (tag === "INPUT" && type === "file") {
    role = "button";
    const l = labelFor(el);
    // A hidden file input behind a button-like label: the label is what the user sees and clicks.
    const r = el.getBoundingClientRect();
    if (l && (r.width < 4 || r.height < 4 || getComputedStyle(el).opacity === "0")) box = l;
  } else if (tag === "INPUT" && type === "checkbox") {
    role = "checkbox"; value = el.checked ? "checked" : "unchecked";
  } else if (tag === "INPUT" && type === "radio") {
    role = "radio"; value = el.checked ? "checked" : "unchecked";
  } else if (tag === "BUTTON" || el.getAttribute("role") === "button") {
    role = "button"; name = textOf(el);
  } else if (tag === "A") {
    role = "link"; name = textOf(el);
  } else if (/^H[1-3]$/.test(tag) || el.classList.contains("error") || el.getAttribute("role") === "alert") {
    // A heading's aria-label is its UI Automation name (what a page aimed at the assistant hides there).
    role = "text"; name = clean(el.getAttribute("aria-label")) || textOf(el);
    if (!name) continue;
  }
  if (!role || !visible(box)) continue;
  if (!name && role !== "text") {
    const l = labelFor(el);
    name = (l && textOf(l)) || clean(el.getAttribute("aria-label")) || clean(el.getAttribute("placeholder")) || textOf(el);
  }
  const r = box.getBoundingClientRect();
  const rect = { x: Math.round(r.left), y: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height) };
  const offscreen = r.bottom <= 0 || r.top >= H || r.right <= 0 || r.left >= W || hidden(box);
  out.push({ role, name, value: role === "text" ? null : value, rect, offscreen, enabled: !el.disabled, is_password: isPw, _sel: null });
}
return out;
"""

BOX_JS = r"""
const el = document.querySelector(arguments[0]);
if (!el) return null;
let box = el;
if (el.tagName === "INPUT" && el.type === "file" && el.id) {
  const l = document.querySelector(`label[for="${CSS.escape(el.id)}"]`);
  if (l) box = l;
}
const r = box.getBoundingClientRect();
return [Math.round(r.left), Math.round(r.top), Math.round(r.right) - 1, Math.round(r.bottom) - 1];
"""


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Driver:
    def __init__(self) -> None:
        self.port = free_port()
        self.proc = subprocess.Popen([GECKO, "--port", str(self.port)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.base = f"http://127.0.0.1:{self.port}"
        for _ in range(100):
            try:
                urllib.request.urlopen(self.base + "/status", timeout=1).read()
                break
            except OSError:
                time.sleep(0.1)
        caps = {"capabilities": {"alwaysMatch": {"moz:firefoxOptions": {"args": ["-headless"]}}}}
        self.sid = self.call("POST", "/session", caps)["sessionId"]

    def call(self, method: str, path: str, body: dict | None = None):
        data = json.dumps(body if body is not None else {}).encode() if method == "POST" else None
        req = urllib.request.Request(self.base + path, data=data, method=method, headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=60) as r:
            v = json.loads(r.read())["value"]
        if isinstance(v, dict) and "error" in v:
            raise RuntimeError(f"{path}: {v['error']}: {v.get('message')}")
        return v

    def s(self, method: str, path: str, body: dict | None = None):
        return self.call(method, f"/session/{self.sid}{path}", body)

    def js(self, script: str, *args):
        return self.s("POST", "/execute/sync", {"script": script, "args": list(args)})

    def viewport(self) -> None:
        """Set the window so that the viewport (innerWidth x innerHeight) is exactly W x H."""
        ww, wh = W, H
        for _ in range(6):
            self.s("POST", "/window/rect", {"width": ww, "height": wh})
            iw, ih = self.js("return [window.innerWidth, window.innerHeight];")
            if (iw, ih) == (W, H):
                return
            ww, wh = ww + (W - iw), wh + (H - ih)
        raise RuntimeError(f"viewport stays {iw}x{ih}, wanted {W}x{H}")

    def quit(self) -> None:
        try:
            self.call("DELETE", f"/session/{self.sid}")
        finally:
            self.proc.terminate()
            self.proc.wait(timeout=10)


def page_url(page: str, query: str) -> str:
    if "://" not in page:
        page = pathlib.Path(page).resolve().as_uri()
    return page + query


def capture(page: str, stem: str, query: str = "", targets: dict | None = None, out: pathlib.Path = OUT,
            outline: bool = True, note: str = "", clicks: list | None = None, scroll_bottom: bool = False,
            labels: dict | None = None) -> dict:
    out.mkdir(parents=True, exist_ok=True)
    d = Driver()
    try:
        d.viewport()
        d.s("POST", "/url", {"url": page_url(page, query)})
        for c in clicks or []:
            d.js("document.querySelector(arguments[0]).click();", c)
        time.sleep(0.4)  # fonts and layout settle
        d.viewport()  # a scrollbar that appeared after load changes nothing: innerWidth includes it
        if scroll_bottom:
            d.js("window.scrollTo(0, document.documentElement.scrollHeight);")
            time.sleep(0.2)
        png = base64.b64decode(d.s("GET", "/screenshot"))
        (out / f"{stem}.png").write_bytes(png)
        items = d.js(WALK_JS, W, H)
        boxes = {}
        for name, css in (targets or {}).items():
            b = d.js(BOX_JS, css)
            if b is None:
                sys.exit(f"--target {name}={css}: no such element on {stem}")
            boxes[name] = b
    finally:
        d.quit()
    for n, it in enumerate(items, 1):
        it.pop("_sel", None)
        it = {"id": n, **it}
        items[n - 1] = it
    if outline:
        doc = {
            "note": note or f"UI Automation outline of {stem}.png ({page_url(page, query)}, {W}x{H}), written by "
                            "make_outline.py from the live page. Rects are physical pixels of the PNG.",
            "image": f"{stem}.png",
            "items": items,
        }
        (out / f"{stem}.outline.json").write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")
    if targets is not None:
        meta = {"note": "Pixel boxes (x1,y1,x2,y2) in the PNG, from make_outline.py. A click hits if it lands inside the box.",
                "w": W, "h": H, "image": f"{stem}.png"}
        def label(k: str) -> tuple:
            return (labels or {}).get(k) or ("button" if "button" in k else "text input field", k.replace("_", " "))
        (out / f"{stem}.boxes.json").write_text(json.dumps({**meta, "targets": {
            k: {"box": v, "sam_query": label(k)[0], "description": label(k)[1]}
            for k, v in boxes.items()}}, indent=2, ensure_ascii=False) + "\n")
    print(f"{out / stem}.png: {len(items)} elements, {len(boxes)} targets")
    return {"items": items, "boxes": boxes}


def parse(argv: list) -> dict:
    if len(argv) < 2:
        sys.exit(__doc__)
    a = {"page": argv[0], "stem": argv[1], "query": "", "targets": None, "out": OUT, "outline": True, "note": "", "clicks": [],
         "scroll_bottom": False}
    rest = argv[2:]
    if rest and rest[0].startswith("?"):
        a["query"] = rest.pop(0)
    while rest:
        flag = rest.pop(0)
        if flag == "--target":
            name, _, css = rest.pop(0).partition("=")
            a["targets"] = {**(a["targets"] or {}), name: css}
        elif flag == "--out":
            a["out"] = pathlib.Path(rest.pop(0))
        elif flag == "--no-outline":
            a["outline"] = False
        elif flag == "--note":
            a["note"] = rest.pop(0)
        elif flag == "--click":
            a["clicks"].append(rest.pop(0))
        elif flag == "--scroll-bottom":
            a["scroll_bottom"] = True
        else:
            sys.exit(f"unknown argument {flag}\n{__doc__}")
    return a


if __name__ == "__main__":
    capture(**parse(sys.argv[1:]))
