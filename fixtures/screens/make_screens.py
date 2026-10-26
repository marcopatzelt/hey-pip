#!/usr/bin/env python3
"""Screenshots of the demo form (fixtures/demo-kasse) + expected target boxes for grounding/eval.

Run: python3 fixtures/screens/make_screens.py   (needs Firefox; uses a throwaway profile in target/)
Boxes come from `?mark`, which paints each target in a solid colour; we take the bounding box of
that colour. Coordinates are pixels of the PNG (1600x1000), xyxy.
"""

import json
import pathlib
import shutil
import subprocess

from PIL import Image  # pymupdf depends on nothing here; Pillow ships with Ubuntu's python3-pil

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = pathlib.Path(__file__).resolve().parent
PAGE = (ROOT / "fixtures/demo-kasse/index.html").as_uri()
PROFILE = ROOT / "target/ff-profile"
SIZE = (1600, 1000)

# colour -> (target id, SAM query, description for the model)
TARGETS = {
    (255, 0, 0): ("name_field", "text input field", "Eingabefeld Vor- und Nachname"),
    (0, 255, 0): ("nr_field", "text input field", "Eingabefeld Versichertennummer"),
    (0, 0, 255): ("message_field", "text area", "Feld Ihre Nachricht"),
    (255, 0, 255): ("file_button", "button", "Knopf Datei auswählen"),
    (0, 255, 255): ("submit_button", "button", "Knopf Absenden"),
}


def shot(query: str, dst: pathlib.Path) -> None:
    PROFILE.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [
            "firefox", "--headless", "--no-remote", "--profile", str(PROFILE),
            f"--window-size={SIZE[0]},{SIZE[1]}", "--screenshot", str(dst), PAGE + query,
        ],
        check=True,
        capture_output=True,
        timeout=120,
    )
    with Image.open(dst) as im:
        if im.size != SIZE:  # Firefox sometimes adds a scrollbar column; crop to the exact size
            im.crop((0, 0, *SIZE)).save(dst)


def boxes(marked: pathlib.Path) -> dict:
    im = Image.open(marked).convert("RGB")
    w, h = im.size
    px = im.load()
    found: dict = {}
    for y in range(h):
        for x in range(w):
            c = px[x, y]
            if c in TARGETS:
                b = found.setdefault(c, [x, y, x, y])
                b[0], b[1], b[2], b[3] = min(b[0], x), min(b[1], y), max(b[2], x), max(b[3], y)
    out = {}
    for c, (tid, query, desc) in TARGETS.items():
        if c in found:
            out[tid] = {"box": found[c], "sam_query": query, "description": desc}
    return out


def main() -> None:
    tmp = OUT / "_marked.png"
    shot("", OUT / "kasse_form.png")
    shot("?fill", OUT / "kasse_form_filled.png")
    shot("?thanks", OUT / "kasse_danke.png")
    shot("?mark", tmp)
    targets = boxes(tmp)
    tmp.unlink()
    meta = {"note": "Pixel boxes (x1,y1,x2,y2) in the PNG. A click hits if it lands inside the box.", "w": SIZE[0], "h": SIZE[1]}
    for name in ["kasse_form", "kasse_form_filled"]:
        (OUT / f"{name}.boxes.json").write_text(json.dumps({**meta, "image": f"{name}.png", "targets": targets}, indent=2, ensure_ascii=False) + "\n")
    (OUT / "kasse_danke.boxes.json").write_text(json.dumps({**meta, "image": "kasse_danke.png", "targets": {},
        "expect_text": "Danke, eingegangen am"}, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps(targets, indent=1))
    shutil.rmtree(PROFILE, ignore_errors=True)


if __name__ == "__main__":
    main()
