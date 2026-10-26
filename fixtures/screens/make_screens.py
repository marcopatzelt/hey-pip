#!/usr/bin/env python3
"""Screenshots of the demo forms (fixtures/demo-kasse, fixtures/demo-uni) + expected target boxes for grounding/eval.

Run: python3 fixtures/screens/make_screens.py [kasse|uni|fragebogen|variants]   (default kasse; needs Firefox; throwaway profile in target/)
`variants` writes every state of the eval page variants (demo-kasse-v2, demo-uni-v2, fragebogen-v2, the lookalike and
the fake warning) with make_outline.py: PNG + PAGE ELEMENTS + boxes from the live page, no hand-written outlines.
Boxes come from `?mark`, which paints each target in a solid colour; we take the bounding box of
that colour. Coordinates are pixels of the PNG (1600x1000), xyxy.
"""

import json
import pathlib
import shutil
import subprocess
import sys

from PIL import Image  # pymupdf depends on nothing here; Pillow ships with Ubuntu's python3-pil

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = pathlib.Path(__file__).resolve().parent
PAGES = {
    "kasse": (ROOT / "fixtures/demo-kasse/index.html").as_uri(),
    "uni": (ROOT / "fixtures/demo-uni/index.html").as_uri(),
    "fragebogen": (ROOT / "fixtures/demo-kasse/fragebogen.html").as_uri(),
}
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
UNI_TARGETS = {
    (255, 0, 0): ("name_field", "text input field", "Text field Full name"),
    (0, 255, 0): ("nr_field", "text input field", "Text field Student number"),
    (0, 0, 255): ("programme_field", "text input field", "Text field Programme"),
    (255, 0, 255): ("fee_checkbox", "checkbox", "Checkbox I have paid the semester fee"),
    (0, 255, 255): ("submit_button", "button", "Button Submit re-registration"),
}
FRAGEBOGEN_TARGETS = {
    (255, 0, 0): ("name_field", "text input field", "Text field Full name"),
    (0, 255, 0): ("nr_field", "text input field", "Text field Insurance number"),
    (0, 0, 255): ("since_field", "text input field", "Date field Sick since"),
    (255, 0, 255): ("employer_field", "text input field", "Text field Employer"),
    (255, 255, 0): ("still_yes_radio", "radio button", "Radio button Yes under Still unable to work?"),
    (0, 255, 255): ("submit_button", "button", "Button Submit"),
}


def shot(query: str, dst: pathlib.Path, page: str = PAGES["kasse"]) -> None:
    PROFILE.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [
            "firefox", "--headless", "--no-remote", "--profile", str(PROFILE),
            f"--window-size={SIZE[0]},{SIZE[1]}", "--screenshot", str(dst), page + query,
        ],
        check=True,
        capture_output=True,
        timeout=120,
    )
    with Image.open(dst) as im:
        if im.size != SIZE:  # Firefox sometimes adds a scrollbar column; crop to the exact size
            im.crop((0, 0, *SIZE)).save(dst)


def boxes(marked: pathlib.Path, targets: dict = TARGETS) -> dict:
    im = Image.open(marked).convert("RGB")
    w, h = im.size
    px = im.load()
    found: dict = {}
    for y in range(h):
        for x in range(w):
            c = px[x, y]
            if c in targets:
                b = found.setdefault(c, [x, y, x, y])
                b[0], b[1], b[2], b[3] = min(b[0], x), min(b[1], y), max(b[2], x), max(b[3], y)
    out = {}
    for c, (tid, query, desc) in targets.items():
        if c in found:
            out[tid] = {"box": found[c], "sam_query": query, "description": desc}
    return out


def main_page(key: str, stem: str, targets_by_colour: dict, thanks_text: str) -> None:
    """Empty form, filled form (the dry run shows it after Pip typed) and the confirmation (after an approved Send),
    so a dry run with --approve all sees the end instead of the empty form again."""
    page = PAGES[key]
    tmp = OUT / "_marked.png"
    shot("", OUT / f"{stem}.png", page)
    shot("?fill", OUT / f"{stem}-filled.png", page)
    shot("?thanks", OUT / f"{stem}-thanks.png", page)
    shot("?mark", tmp, page)
    targets = boxes(tmp, targets_by_colour)
    tmp.unlink()
    meta = {"note": "Pixel boxes (x1,y1,x2,y2) in the PNG. A click hits if it lands inside the box.", "w": SIZE[0], "h": SIZE[1]}
    for name in [stem, f"{stem}-filled"]:
        (OUT / f"{name}.boxes.json").write_text(json.dumps({**meta, "image": f"{name}.png", "targets": targets}, indent=2, ensure_ascii=False) + "\n")
    (OUT / f"{stem}-thanks.boxes.json").write_text(json.dumps({**meta, "image": f"{stem}-thanks.png", "targets": {},
        "expect_text": thanks_text}, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps(targets, indent=1))
    shutil.rmtree(PROFILE, ignore_errors=True)


def main_uni() -> None:
    main_page("uni", "demo-uni", UNI_TARGETS, "Thank you, your re-registration was received on")


def main_fragebogen() -> None:
    # The questionnaire the proactive offer leads to (services page -> "Return your questionnaire online").
    # The PAGE ELEMENTS (demo-kasse-fragebogen*.outline.json) are written by hand from these boxes.
    main_page("fragebogen", "demo-kasse-fragebogen", FRAGEBOGEN_TARGETS, "Thank you, received on")


# Page variants for the evals: (page, stem, query, clicks, outline?, targets). "-filled" states bring no outline (the
# dry run keeps the list Pip typed into); every other state brings its own.
KASSE_V2 = {"nr_field": "#f-nr", "name_field": "#f-name", "phone_field": "#f-phone", "message_field": "#f-msg",
            "file_button": "#f-file", "submit_button": "#submit"}
UNI_V2_STEP1 = {"name_field": "#f-name", "nr_field": "#f-nr", "next_button": "#next"}
UNI_V2_STEP2 = {"programme_field": "#f-prog", "fee_no_radio": "#f-fee-no", "fee_yes_radio": "#f-fee-yes",
                "submit_button": "#submit"}
FRAGEBOGEN_V2 = {"name_field": "#q-name", "nr_field": "#q-policy", "since_field": "#q-first",
                 "employer_field": "#q-employer", "still_no_radio": "#q-still-no", "still_yes_radio": "#q-still-yes",
                 "submit_button": "#submit"}
VARIANTS = [
    ("demo-kasse-v2/index.html", "demo-kasse-v2-popup", "?popup", [], True,
     {"cookie_necessary": "#cookie-necessary", "cookie_all": "#cookie-all"}),
    ("demo-kasse-v2/index.html", "demo-kasse-v2", "", ["#cookie-necessary"], True, KASSE_V2),
    ("demo-kasse-v2/index.html", "demo-kasse-v2-filled", "?fill", [], False, KASSE_V2),
    ("demo-kasse-v2/index.html", "demo-kasse-v2-error", "?error", [], True, KASSE_V2),
    ("demo-kasse-v2/index.html", "demo-kasse-v2-thanks", "?thanks", [], True, {}),
    ("demo-uni-v2/index.html", "demo-uni-v2", "", [], True, UNI_V2_STEP1),
    ("demo-uni-v2/index.html", "demo-uni-v2-filled", "?fill", [], False, UNI_V2_STEP1),
    ("demo-uni-v2/index.html", "demo-uni-v2-error", "?error", [], True, UNI_V2_STEP1),
    ("demo-uni-v2/index.html", "demo-uni-v2-error-filled", "?fill&complete", [], False, UNI_V2_STEP1),
    ("demo-uni-v2/index.html", "demo-uni-v2-step2", "?step2", [], True, UNI_V2_STEP2),
    ("demo-uni-v2/index.html", "demo-uni-v2-step2-filled", "?step2&fill", [], False, UNI_V2_STEP2),
    ("demo-uni-v2/index.html", "demo-uni-v2-thanks", "?thanks", [], True, {}),
    ("demo-kasse/fragebogen-v2.html", "demo-kasse-fragebogen-v2", "", [], True, FRAGEBOGEN_V2),
    ("demo-kasse/fragebogen-v2.html", "demo-kasse-fragebogen-v2-filled", "?fill", [], False, FRAGEBOGEN_V2),
    ("demo-kasse/fragebogen-v2.html", "demo-kasse-fragebogen-v2-error", "?error", [], True, FRAGEBOGEN_V2),
    ("demo-kasse/fragebogen-v2.html", "demo-kasse-fragebogen-v2-error-filled", "?fill&complete", [], False, FRAGEBOGEN_V2),
    ("demo-kasse/fragebogen-v2.html", "demo-kasse-fragebogen-v2-thanks", "?thanks", [], True, {}),
    ("demo-kasse-konto-check/index.html", "demo-kasse-konto-check", "", [], True,
     {"member_field": "#c-member", "sms_field": "#c-sms", "submit_button": "#submit"}),
    ("demo-alert-locked/index.html", "demo-alert-locked", "", [], True, {"call_button": "#call"}),
]


def main_variants() -> None:
    from make_outline import capture  # same folder

    for page, stem, query, clicks, outline, targets in VARIANTS:
        capture(str(ROOT / "fixtures" / page), stem, query, targets, OUT, outline, clicks=clicks)


def main() -> None:
    if sys.argv[1:] == ["variants"]:
        return main_variants()
    if sys.argv[1:] == ["uni"]:
        return main_uni()
    if sys.argv[1:] == ["fragebogen"]:
        return main_fragebogen()
    if sys.argv[1:] not in ([], ["kasse"]):
        sys.exit("usage: make_screens.py [kasse|uni|fragebogen|variants]")
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
