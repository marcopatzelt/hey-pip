#!/usr/bin/env python3
"""The browser's "Open" file dialog as the dry run shows it (fixtures/screens/file-dialog.png).

Run: python3 fixtures/screens/make_file_dialog.py
`pip-cli agent` / `pip-cli eval` paste this picture onto the middle of the screen after a click on a page's
`file_button` (Windows shows the same modal dialog), and take it away again on Enter or Escape. Plain shapes and
DejaVu Sans, 880x540, no real file names.
"""

import pathlib

from PIL import Image, ImageDraw, ImageFont

OUT = pathlib.Path(__file__).resolve().parent / "file-dialog.png"
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"


def main() -> None:
    w, h = 880, 540
    im = Image.new("RGB", (w, h), "#f3f3f3")
    d = ImageDraw.Draw(im)
    f = ImageFont.truetype(FONT, 16)
    fb = ImageFont.truetype(BOLD, 17)
    d.rectangle([0, 0, w - 1, h - 1], outline="#8a8a8a", width=2)
    d.rectangle([2, 2, w - 3, 40], fill="#ffffff")
    d.text((16, 11), "Open", font=fb, fill="#1a1a1a")
    d.text((w - 34, 9), "×", font=fb, fill="#1a1a1a")
    # Address bar and search box
    d.rectangle([16, 54, 600, 86], fill="#ffffff", outline="#c8c8c8")
    d.text((26, 61), "This PC  >  Documents", font=f, fill="#333333")
    d.rectangle([614, 54, 864, 86], fill="#ffffff", outline="#c8c8c8")
    d.text((624, 61), "Search Documents", font=f, fill="#8a8a8a")
    # Left pane and the file list
    d.rectangle([16, 100, 220, 420], fill="#ffffff", outline="#dcdcdc")
    for i, name in enumerate(["Quick access", "Desktop", "Downloads", "Documents", "Pictures", "This PC"]):
        d.text((30, 112 + i * 34), name, font=f, fill="#333333")
    d.rectangle([232, 100, 864, 420], fill="#ffffff", outline="#dcdcdc")
    d.text((246, 110), "Name", font=fb, fill="#555555")
    d.text((620, 110), "Date modified", font=fb, fill="#555555")
    d.line([240, 138, 856, 138], fill="#e2e2e2", width=1)
    # File name box, type filter, buttons
    d.text((90, 446), "File name:", font=f, fill="#1a1a1a")
    d.rectangle([190, 438, 600, 470], fill="#ffffff", outline="#0067c0", width=2)
    d.rectangle([614, 438, 864, 470], fill="#ffffff", outline="#c8c8c8")
    d.text((624, 445), "All files (*.*)", font=f, fill="#333333")
    d.rectangle([614, 488, 734, 522], fill="#0067c0")
    d.text((654, 496), "Open", font=fb, fill="#ffffff")
    d.rectangle([744, 488, 864, 522], fill="#ffffff", outline="#c8c8c8")
    d.text((774, 496), "Cancel", font=f, fill="#1a1a1a")
    im.save(OUT)
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
