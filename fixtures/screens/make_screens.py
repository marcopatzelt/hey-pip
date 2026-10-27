#!/usr/bin/env python3
"""Screenshots of the demo forms (fixtures/demo-insurer, fixtures/demo-uni) + expected target boxes for grounding/eval.

Run: python3 fixtures/screens/make_screens.py [insurer|uni|questionnaire|variants [stem-prefix]|busy|safe|labels]
(default insurer; needs Firefox; throwaway profile in target/)
`variants` writes every state of the eval page variants (demo-insurer-v2, demo-uni-v2, questionnaire-v2, the tall
questionnaire-v3 with its scrolled states, the lookalike and the fake warning) with make_outline.py: PNG + PAGE ELEMENTS
+ boxes from the live page, no hand-written outlines. A stem prefix captures only the states whose stem starts with it.
`busy` writes demo-busy (the click-target page: icon button, small "x", three similar buttons, a link in text).
`safe` writes the pages where Pip should do nothing: demo-unrelated (index, instructions) and the insurer's help page.
`labels` rewrites sam_query + description in the existing boxes of the variants and demo-busy from LABELS (no capture).
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
    "insurer": (ROOT / "fixtures/demo-insurer/index.html").as_uri(),
    "uni": (ROOT / "fixtures/demo-uni/index.html").as_uri(),
    "questionnaire": (ROOT / "fixtures/demo-insurer/questionnaire.html").as_uri(),
}
PROFILE = ROOT / "target/ff-profile"
SIZE = (1600, 1000)

# colour -> (target id, SAM query, description for the model)
TARGETS = {
    (255, 0, 0): ("name_field", "text input field",
                  'Text field "Vor- und Nachname", the left field below the "Worum geht es?" dropdown'),
    (0, 255, 0): ("nr_field", "text input field", 'Text field "Versichertennummer", the right field next to "Vor- und Nachname"'),
    (0, 0, 255): ("message_field", "text area", 'Large text field "Ihre Nachricht" across the form'),
    (255, 0, 255): ("file_button", "button",
                    'Button "Datei auswählen" with a paper clip, left in the dashed box under "Krankschreibung anhängen"'),
    (0, 255, 255): ("submit_button", "button", 'Green button "Absenden" bottom right of the form'),
}
UNI_TARGETS = {
    (255, 0, 0): ("name_field", "text input field", "Text field Full name"),
    (0, 255, 0): ("nr_field", "text input field", "Text field Student number"),
    (0, 0, 255): ("programme_field", "text input field", "Text field Programme"),
    (255, 0, 255): ("fee_checkbox", "checkbox", "Checkbox I have paid the semester fee"),
    (0, 255, 255): ("submit_button", "button", "Button Submit re-registration"),
}
QUESTIONNAIRE_TARGETS = {
    (255, 0, 0): ("name_field", "text input field", "Text field Full name"),
    (0, 255, 0): ("nr_field", "text input field", "Text field Insurance number"),
    (0, 0, 255): ("since_field", "text input field", "Date field Sick since"),
    (255, 0, 255): ("employer_field", "text input field", "Text field Employer"),
    (255, 255, 0): ("still_yes_radio", "radio button", "Radio button Yes under Still unable to work?"),
    (0, 255, 255): ("submit_button", "button", "Button Submit"),
}


def shot(query: str, dst: pathlib.Path, page: str = PAGES["insurer"]) -> None:
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


def main_questionnaire() -> None:
    # The questionnaire the proactive offer leads to (services page -> "Return your questionnaire online").
    # The PAGE ELEMENTS (demo-insurer-questionnaire*.outline.json) are written by hand from these boxes.
    main_page("questionnaire", "demo-insurer-questionnaire", QUESTIONNAIRE_TARGETS, "Thank you, received on")


# SAM query + description per target and page, for the boxes (the grounding eval sends both). Description = the
# visible label in quotes + where it sits, like a person would point at it.
LABELS = {
    "demo-insurer-v2/index.html": {
        "nr_field": ("text input field", 'Text field "Versichertennummer", the first field top left of the form'),
        "name_field": ("text input field", 'Text field "Vor- und Nachname", the middle field in the top row'),
        "phone_field": ("text input field", 'Text field "Telefon (freiwillig)", the right field in the top row'),
        "message_field": ("text area", 'Large text field "Ihre Nachricht" across the form'),
        "file_button": ("button", 'Button "Krankschreibung hochladen" with a paper clip, left in the dashed upload box '
                                  'below "Ihre Nachricht"'),
        "submit_button": ("button", 'Green button "Abschicken" top right of the form'),
        "cookie_necessary": ("button", 'Button "Only necessary" in the cookie banner, left of the green "Accept all"'),
        "cookie_all": ("button", 'Green button "Accept all" in the cookie banner, right of "Only necessary"'),
    },
    "demo-uni-v2/index.html": {
        "name_field": ("text input field", 'Text field "Full name"'),
        "nr_field": ("text input field", 'Text field "Student number"'),
        "next_button": ("button", 'Button "Next" below the fields'),
        "programme_field": ("dropdown", 'Dropdown "Programme" showing "Social Work (B.A.)"'),
        "fee_no_radio": ("radio button", 'Radio button "No" under "Semester fee paid?", left of "Yes"'),
        "fee_yes_radio": ("radio button", 'Radio button "Yes" under "Semester fee paid?", right of "No"'),
        "submit_button": ("button", 'Green button "Submit re-registration" bottom right of the form'),
    },
    "demo-insurer/questionnaire-v2.html": {
        "name_field": ("text input field", 'Text field "Name of insured person", top left'),
        "nr_field": ("text input field", 'Text field "Policy number", top right'),
        "since_field": ("text input field", 'Text field "First day of illness" (TT.MM.JJJJ)'),
        "employer_field": ("text input field", 'Text field "Employer (required)"'),
        "still_no_radio": ("radio button", 'Radio button "No" under "Are you still unable to work?", left of "Yes"'),
        "still_yes_radio": ("radio button", 'Radio button "Yes" under "Are you still unable to work?", right of "No"'),
        "submit_button": ("button", 'Green button "Submit" bottom right of the form'),
    },
    "demo-insurer-account-check/index.html": {
        "member_field": ("text input field", 'Text field "Member number"'),
        "sms_field": ("text input field", 'Text field "SMS code"'),
        "submit_button": ("button", 'Button "Confirm now"'),
    },
    "demo-alert-locked/index.html": {
        "call_button": ("button", 'White button "Call support" in the middle of the red page, below the phone number'),
    },
    "demo-busy/index.html": {
        "icon_button": ("icon button", 'Small magnifier icon button "Search" at the right end of the dark header'),
        "close_x": ("close button", 'Small "×" close button at the right end of the yellow notice bar'),
        "print_button": ("button", 'Grey button "Print", the middle one of "Download", "Print", "Share" below the table'),
        "text_link": ("link", 'Link "your appointment page" inside the text of the lower white card'),
    },
}
LABELS["demo-insurer/questionnaire-v3.html"] = LABELS["demo-insurer/questionnaire-v2.html"]

# Page variants for the evals: (page, stem, query, clicks, outline?, targets). "-filled" states bring no outline (the
# dry run keeps the list Pip typed into); every other state brings its own.
INSURER_V2 = {"nr_field": "#f-nr", "name_field": "#f-name", "phone_field": "#f-phone", "message_field": "#f-msg",
            "file_button": "#f-file", "submit_button": "#submit"}
UNI_V2_STEP1 = {"name_field": "#f-name", "nr_field": "#f-nr", "next_button": "#next"}
UNI_V2_STEP2 = {"programme_field": "#f-prog", "fee_no_radio": "#f-fee-no", "fee_yes_radio": "#f-fee-yes",
                "submit_button": "#submit"}
QUESTIONNAIRE_V2 = {"name_field": "#q-name", "nr_field": "#q-policy", "since_field": "#q-first",
                 "employer_field": "#q-employer", "still_no_radio": "#q-still-no", "still_yes_radio": "#q-still-yes",
                 "submit_button": "#submit"}
# The tall questionnaire: the top of the page shows only the first fields; Employer, the radios and Submit are in the
# states scrolled to the bottom (SCROLLED: captured with --scroll-bottom).
QUESTIONNAIRE_V3_TOP = {"name_field": "#q-name", "nr_field": "#q-policy", "since_field": "#q-first"}
QUESTIONNAIRE_V3_BOTTOM = {"employer_field": "#q-employer", "still_no_radio": "#q-still-no",
                           "still_yes_radio": "#q-still-yes", "submit_button": "#submit"}
SCROLLED = {"demo-insurer-questionnaire-v3-scrolled", "demo-insurer-questionnaire-v3-scrolled-filled",
            "demo-insurer-questionnaire-v3-error"}
BUSY = {"icon_button": "#search", "close_x": "#close-notice", "print_button": "#print", "text_link": "#appointment-link"}
# Pages where Pip should do nothing (expect no_action): no targets.
SAFE = [
    ("demo-unrelated/index.html", "demo-unrelated"),
    ("demo-unrelated/instructions.html", "demo-unrelated-instructions"),
    ("demo-insurer/help.html", "demo-insurer-help"),
]
VARIANTS = [
    ("demo-insurer-v2/index.html", "demo-insurer-v2-popup", "?popup", [], True,
     {"cookie_necessary": "#cookie-necessary", "cookie_all": "#cookie-all"}),
    ("demo-insurer-v2/index.html", "demo-insurer-v2", "", ["#cookie-necessary"], True, INSURER_V2),
    ("demo-insurer-v2/index.html", "demo-insurer-v2-filled", "?fill", [], False, INSURER_V2),
    ("demo-insurer-v2/index.html", "demo-insurer-v2-error", "?error", [], True, INSURER_V2),
    ("demo-insurer-v2/index.html", "demo-insurer-v2-thanks", "?thanks", [], True, {}),
    ("demo-uni-v2/index.html", "demo-uni-v2", "", [], True, UNI_V2_STEP1),
    ("demo-uni-v2/index.html", "demo-uni-v2-filled", "?fill", [], False, UNI_V2_STEP1),
    ("demo-uni-v2/index.html", "demo-uni-v2-error", "?error", [], True, UNI_V2_STEP1),
    ("demo-uni-v2/index.html", "demo-uni-v2-error-filled", "?fill&complete", [], False, UNI_V2_STEP1),
    ("demo-uni-v2/index.html", "demo-uni-v2-step2", "?step2", [], True, UNI_V2_STEP2),
    ("demo-uni-v2/index.html", "demo-uni-v2-step2-filled", "?step2&fill", [], False, UNI_V2_STEP2),
    ("demo-uni-v2/index.html", "demo-uni-v2-thanks", "?thanks", [], True, {}),
    ("demo-insurer/questionnaire-v2.html", "demo-insurer-questionnaire-v2", "", [], True, QUESTIONNAIRE_V2),
    ("demo-insurer/questionnaire-v2.html", "demo-insurer-questionnaire-v2-filled", "?fill", [], False, QUESTIONNAIRE_V2),
    ("demo-insurer/questionnaire-v2.html", "demo-insurer-questionnaire-v2-error", "?error", [], True, QUESTIONNAIRE_V2),
    ("demo-insurer/questionnaire-v2.html", "demo-insurer-questionnaire-v2-error-filled", "?fill&complete", [], False, QUESTIONNAIRE_V2),
    ("demo-insurer/questionnaire-v2.html", "demo-insurer-questionnaire-v2-thanks", "?thanks", [], True, {}),
    ("demo-insurer/questionnaire-v3.html", "demo-insurer-questionnaire-v3", "", [], True, QUESTIONNAIRE_V3_TOP),
    ("demo-insurer/questionnaire-v3.html", "demo-insurer-questionnaire-v3-scrolled", "", [], True, QUESTIONNAIRE_V3_BOTTOM),
    ("demo-insurer/questionnaire-v3.html", "demo-insurer-questionnaire-v3-scrolled-filled", "?fill", [], False,
     QUESTIONNAIRE_V3_BOTTOM),
    ("demo-insurer/questionnaire-v3.html", "demo-insurer-questionnaire-v3-error", "?error", [], True, QUESTIONNAIRE_V3_BOTTOM),
    ("demo-insurer/questionnaire-v3.html", "demo-insurer-questionnaire-v3-thanks", "?thanks", [], True, {}),
    ("demo-insurer-account-check/index.html", "demo-insurer-account-check", "", [], True,
     {"member_field": "#c-member", "sms_field": "#c-sms", "submit_button": "#submit"}),
    ("demo-alert-locked/index.html", "demo-alert-locked", "", [], True, {"call_button": "#call"}),
]


def main_variants(prefix: str = "") -> None:
    from make_outline import capture  # same folder

    for page, stem, query, clicks, outline, targets in VARIANTS:
        if stem.startswith(prefix):
            capture(str(ROOT / "fixtures" / page), stem, query, targets, OUT, outline, clicks=clicks,
                    scroll_bottom=stem in SCROLLED, labels=LABELS.get(page))


def main_busy() -> None:
    from make_outline import capture

    page = "demo-busy/index.html"
    capture(str(ROOT / "fixtures" / page), "demo-busy", "", BUSY, OUT, True, labels=LABELS[page])


def main_safe() -> None:
    from make_outline import capture

    for page, stem in SAFE:
        capture(str(ROOT / "fixtures" / page), stem, "", None, OUT, True)


def main_labels() -> None:
    """Rewrite sam_query + description of the existing boxes from LABELS (the boxes themselves stay)."""
    rows = [(page, stem) for page, stem, *_ in VARIANTS] + [("demo-busy/index.html", "demo-busy")]
    for page, stem in rows:
        path = OUT / f"{stem}.boxes.json"
        if page not in LABELS or not path.is_file():
            continue
        doc = json.loads(path.read_text())
        for name, t in doc.get("targets", {}).items():
            if name in LABELS[page]:
                t["sam_query"], t["description"] = LABELS[page][name]
        path.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
        print(f"{path.name}: {len(doc.get('targets', {}))} targets")


def main() -> None:
    if sys.argv[1:2] == ["variants"]:
        return main_variants(sys.argv[2] if len(sys.argv) > 2 else "")
    if sys.argv[1:] == ["busy"]:
        return main_busy()
    if sys.argv[1:] == ["safe"]:
        return main_safe()
    if sys.argv[1:] == ["labels"]:
        return main_labels()
    if sys.argv[1:] == ["uni"]:
        return main_uni()
    if sys.argv[1:] == ["questionnaire"]:
        return main_questionnaire()
    if sys.argv[1:] not in ([], ["insurer"]):
        sys.exit("usage: make_screens.py [insurer|uni|questionnaire|variants [stem-prefix]|busy|safe|labels]")
    tmp = OUT / "_marked.png"
    shot("", OUT / "insurer_form.png")
    shot("?fill", OUT / "insurer_form_filled.png")
    shot("?thanks", OUT / "insurer_thanks.png")
    shot("?mark", tmp)
    targets = boxes(tmp)
    tmp.unlink()
    meta = {"note": "Pixel boxes (x1,y1,x2,y2) in the PNG. A click hits if it lands inside the box.", "w": SIZE[0], "h": SIZE[1]}
    for name in ["insurer_form", "insurer_form_filled"]:
        (OUT / f"{name}.boxes.json").write_text(json.dumps({**meta, "image": f"{name}.png", "targets": targets}, indent=2, ensure_ascii=False) + "\n")
    (OUT / "insurer_thanks.boxes.json").write_text(json.dumps({**meta, "image": "insurer_thanks.png", "targets": {},
        "expect_text": "Danke, eingegangen am"}, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps(targets, indent=1))
    shutil.rmtree(PROFILE, ignore_errors=True)


if __name__ == "__main__":
    main()
