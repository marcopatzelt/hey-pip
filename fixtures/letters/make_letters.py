#!/usr/bin/env python3
"""Build the invented example letters (PDF + PNG) and the phone photo of the sick note.

All people, insurers, offices, numbers and addresses are INVENTED. No real logos.
Run from anywhere:  python3 fixtures/letters/make_letters.py
Needs: reportlab, pymupdf, ImageMagick 7 (`magick`).
"""

import pathlib
import subprocess

import pymupdf
from reportlab.lib.colors import HexColor, black, white
from reportlab.lib.pagesizes import A4
from reportlab.lib.units import mm
from reportlab.pdfgen.canvas import Canvas

OUT = pathlib.Path(__file__).resolve().parent
W, H = A4
GREEN = HexColor("#0f7a5a")
GREY = HexColor("#555555")
LIGHT = HexColor("#e9f3ef")
FOOT = "Erfundenes Beispiel für Tests. Alle Namen, Nummern und Adressen sind ausgedacht."


def footer(c: Canvas, lines: list[str]) -> None:
    c.setStrokeColor(HexColor("#bbbbbb"))
    c.line(20 * mm, 22 * mm, W - 20 * mm, 22 * mm)
    c.setFont("Helvetica", 7)
    c.setFillColor(GREY)
    for i, line in enumerate(lines):
        c.drawString(20 * mm, 18 * mm - i * 3.2 * mm, line)
    c.setFont("Helvetica-Oblique", 6)
    c.drawRightString(W - 20 * mm, 8 * mm, FOOT)
    c.setFillColor(black)


def window_address(c: Canvas, sender_line: str, lines: list[str]) -> None:
    """DIN 5008 address window (form B): 45 mm from top, 20 mm from left."""
    c.setFont("Helvetica", 7)
    c.setFillColor(GREY)
    c.drawString(20 * mm, H - 50 * mm, sender_line)
    c.setFillColor(black)
    c.setFont("Helvetica", 11)
    y = H - 57 * mm
    for line in lines:
        c.drawString(20 * mm, y, line)
        y -= 5 * mm


def info_block(c: Canvas, rows: list[tuple[str, str]], top: float = 52 * mm) -> None:
    x = 125 * mm
    y = H - top
    for label, value in rows:
        c.setFont("Helvetica", 8)
        c.setFillColor(GREY)
        c.drawString(x, y, label)
        c.setFillColor(black)
        c.setFont("Helvetica", 9.5)
        c.drawString(x + 30 * mm, y, value)
        y -= 5 * mm


def paragraphs(c: Canvas, y: float, paras: list[str], size: float = 10.5, width_mm: float = 170) -> float:
    """Simple word wrap. Paragraphs starting with '**' are bold."""
    from reportlab.pdfbase.pdfmetrics import stringWidth

    max_w = width_mm * mm
    for para in paras:
        font = "Helvetica"
        if para.startswith("**"):
            font, para = "Helvetica-Bold", para[2:]
        if para.startswith("- "):
            indent, para, bullet = 5 * mm, para[2:], True
        else:
            indent, bullet = 0, False
        words = para.split()
        line = ""
        first = True
        for word in words:
            test = (line + " " + word).strip()
            if stringWidth(test, font, size) > max_w - indent:
                c.setFont(font, size)
                if bullet and first:
                    c.drawString(20 * mm, y, "•")
                c.drawString(20 * mm + indent, y, line)
                first = False
                y -= size * 1.45
                line = word
            else:
                line = test
        if line:
            c.setFont(font, size)
            if bullet and first:
                c.drawString(20 * mm, y, "•")
            c.drawString(20 * mm + indent, y, line)
            y -= size * 1.45
        y -= size * 0.6
    return y


def logo_evergreen_health(c: Canvas, x: float, y: float) -> None:
    c.setFillColor(GREEN)
    c.circle(x + 6 * mm, y + 3 * mm, 6 * mm, fill=1, stroke=0)
    c.setFillColor(white)
    c.setFont("Helvetica-Bold", 16)
    c.drawCentredString(x + 6 * mm, y + 0.8 * mm, "E")
    c.setFillColor(GREEN)
    c.setFont("Helvetica-Bold", 20)
    c.drawString(x + 15 * mm, y + 1 * mm, "Evergreen Health")
    c.setFont("Helvetica", 8)
    c.setFillColor(GREY)
    c.drawString(x + 15 * mm, y - 3.5 * mm, "Die Krankenkasse für Beispielstadt")
    c.setFillColor(black)


# --------------------------------------------------------------------------- sick note (AU)


def checkbox(c: Canvas, x: float, y: float, checked: bool, label: str) -> None:
    c.setStrokeColor(black)
    c.rect(x, y, 3.5 * mm, 3.5 * mm, fill=0)
    if checked:
        c.setFont("Helvetica-Bold", 11)
        c.drawString(x + 0.5 * mm, y + 0.5 * mm, "X")
    c.setFont("Helvetica", 9)
    c.drawString(x + 5.5 * mm, y + 0.6 * mm, label)


def field(c: Canvas, x: float, y: float, w: float, h: float, label: str, value: str, size: float = 11) -> None:
    c.setStrokeColor(HexColor("#c0392b"))
    c.setLineWidth(0.6)
    c.rect(x, y, w, h, fill=0)
    c.setFillColor(HexColor("#c0392b"))
    c.setFont("Helvetica", 6.5)
    c.drawString(x + 1.2 * mm, y + h - 2.8 * mm, label)
    c.setFillColor(black)
    c.setFont("Courier-Bold", size)
    c.drawString(x + 2 * mm, y + 1.8 * mm, value)


def make_au(name: str, *, first: bool, since: str, until: str, found: str, issued_note: str) -> None:
    c = Canvas(str(OUT / f"{name}.pdf"), pagesize=A4)
    c.setTitle("Arbeitsunfähigkeitsbescheinigung (Beispiel)")
    red = HexColor("#c0392b")
    # Light red form background like a printed form.
    c.setFillColor(HexColor("#fdf3f2"))
    c.rect(15 * mm, 95 * mm, W - 30 * mm, H - 110 * mm, fill=1, stroke=0)
    c.setFillColor(black)
    top = H - 22 * mm
    field(c, 18 * mm, top - 12 * mm, 110 * mm, 12 * mm, "Krankenkasse bzw. Kostenträger", "Evergreen Health")
    field(c, 18 * mm, top - 30 * mm, 110 * mm, 17 * mm, "Name, Vorname des Versicherten", "")
    c.setFont("Courier-Bold", 11)
    c.drawString(20 * mm, top - 19.6 * mm, "Beispiel, Erika")
    c.setFont("Courier-Bold", 9.5)
    c.drawString(20 * mm, top - 24.2 * mm, "Lindenweg 7")
    c.drawString(20 * mm, top - 28.4 * mm, "40999 Beispielstadt")
    field(c, 139 * mm, top - 42 * mm, 41 * mm, 11 * mm, "geb. am", "12.08.1971")
    field(c, 18 * mm, top - 42 * mm, 40 * mm, 11 * mm, "Kostenträgerkennung", "109900001")
    field(c, 60 * mm, top - 42 * mm, 50 * mm, 11 * mm, "Versicherten-Nr.", "G123456789")
    field(c, 112 * mm, top - 42 * mm, 25 * mm, 11 * mm, "Status", "1000000")
    field(c, 18 * mm, top - 54 * mm, 40 * mm, 11 * mm, "Betriebsstätten-Nr.", "999999900")
    field(c, 60 * mm, top - 54 * mm, 50 * mm, 11 * mm, "Arzt-Nr.", "999999901")
    field(c, 112 * mm, top - 54 * mm, 25 * mm, 11 * mm, "Datum", found)

    c.setFillColor(red)
    c.setFont("Helvetica-Bold", 15)
    c.drawString(140 * mm, top - 6 * mm, "Arbeitsunfähigkeits-")
    c.drawString(140 * mm, top - 12 * mm, "bescheinigung")
    c.setFont("Helvetica", 8)
    c.drawString(140 * mm, top - 17 * mm, "Ausfertigung für Versicherte")
    c.setFillColor(black)

    y = top - 66 * mm
    checkbox(c, 20 * mm, y, first, "Erstbescheinigung")
    checkbox(c, 75 * mm, y, not first, "Folgebescheinigung")
    checkbox(c, 20 * mm, y - 8 * mm, False, "Arbeitsunfall, Arbeitsunfallfolgen, Berufskrankheit")
    checkbox(c, 20 * mm, y - 15 * mm, False, "dem Durchgangsarzt zugewiesen")

    y -= 30 * mm
    field(c, 20 * mm, y, 70 * mm, 12 * mm, "arbeitsunfähig seit", since, 13)
    field(c, 20 * mm, y - 15 * mm, 70 * mm, 12 * mm, "voraussichtlich arbeitsunfähig bis einschließlich", until, 13)
    field(c, 20 * mm, y - 30 * mm, 70 * mm, 12 * mm, "festgestellt am", found, 13)

    c.setFont("Helvetica", 8.5)
    c.drawString(100 * mm, y + 8 * mm, "Diese Bescheinigung wurde elektronisch an die")
    c.drawString(100 * mm, y + 4 * mm, "Krankenkasse übermittelt (eAU).")
    c.drawString(100 * mm, y - 2 * mm, issued_note)

    # Doctor's stamp + signature.
    sx, sy = 112 * mm, y - 40 * mm
    c.setStrokeColor(HexColor("#2c4f9e"))
    c.setFillColor(HexColor("#2c4f9e"))
    c.setLineWidth(1)
    c.roundRect(sx, sy, 68 * mm, 26 * mm, 3 * mm, fill=0)
    c.setFont("Helvetica-Bold", 10)
    c.drawString(sx + 3 * mm, sy + 20 * mm, "Dr. med. Max Beispiel")
    c.setFont("Helvetica", 8.5)
    c.drawString(sx + 3 * mm, sy + 15.5 * mm, "Facharzt für Allgemeinmedizin")
    c.drawString(sx + 3 * mm, sy + 11 * mm, "Musterstraße 1, 40999 Beispielstadt")
    c.drawString(sx + 3 * mm, sy + 6.5 * mm, "Tel. 0999 123 45 67")
    # A scribbled signature.
    c.setStrokeColor(HexColor("#1b2a6b"))
    c.setLineWidth(1.2)
    p = c.beginPath()
    p.moveTo(sx + 8 * mm, sy - 6 * mm)
    for i, (dx, dy) in enumerate([(6, 5), (10, -3), (15, 4), (21, -2), (27, 3), (33, -4), (40, 2)]):
        p.curveTo(sx + (dx - 2) * mm, sy + (dy - 6) * mm, sx + dx * mm, sy + (dy - 4) * mm, sx + (dx + 2) * mm, sy - 6 * mm)
    c.drawPath(p, stroke=1, fill=0)
    c.setFillColor(GREY)
    c.setFont("Helvetica", 7)
    c.drawString(sx, sy - 10 * mm, "Stempel und Unterschrift des Arztes")
    c.setFillColor(black)

    c.setFont("Helvetica", 8)
    c.setFillColor(GREY)
    c.drawString(20 * mm, 100 * mm, "Muster 1b (Beispiel) – bitte für Ihre Unterlagen aufbewahren.")
    c.setFillColor(black)
    c.setFont("Helvetica-Oblique", 6)
    c.drawRightString(W - 20 * mm, 8 * mm, FOOT)
    c.showPage()
    c.save()


# --------------------------------------------------------------------------- Evergreen Health letter


def make_insurer_letter() -> None:
    c = Canvas(str(OUT / "insurer_letter.pdf"), pagesize=A4)
    c.setTitle("Evergreen Health – Fragebogen (Beispiel)")
    logo_evergreen_health(c, 20 * mm, H - 25 * mm)
    window_address(
        c,
        "Evergreen Health · Postfach 10 20 30 · 40998 Beispielstadt",
        ["Frau", "Erika Beispiel", "Lindenweg 7", "40999 Beispielstadt"],
    )
    info_block(
        c,
        [
            ("Ihr Zeichen", "–"),
            ("Versicherten-Nr.", "G123456789"),
            ("Ansprechpartnerin", "Frau Muster"),
            ("Telefon", "0999 888 77 66"),
            ("Datum", "27.10.2026"),
        ],
    )
    c.setFont("Helvetica-Bold", 11.5)
    c.drawString(20 * mm, H - 100 * mm, "Ihre Krankschreibung vom 20.10.2026 – bitte Fragebogen zurücksenden")
    y = paragraphs(
        c,
        H - 112 * mm,
        [
            "Sehr geehrte Frau Beispiel,",
            "Ihre Arztpraxis hat uns Ihre Krankschreibung vom 20.10.2026 elektronisch geschickt. Gute Besserung!",
            "Damit wir Ihren Anspruch auf Krankengeld prüfen können, brauchen wir noch:",
            "- den beiliegenden Fragebogen zur Arbeitsunfähigkeit, ausgefüllt und unterschrieben,",
            "- Ihre Bankverbindung (IBAN), falls sie sich geändert hat.",
            "**Bitte senden Sie uns die Unterlagen bis spätestens 13.11.2026 zurück.",
            "Wenn die Unterlagen später kommen, kann sich die Zahlung verzögern. Bitte denken Sie auch daran: "
            "Wenn Sie länger krank sind, muss die nächste Krankschreibung ohne Lücke anschließen.",
            "Sie können die Unterlagen per Post schicken oder über unser Kontaktformular hochladen: "
            "www.evergreen-health.example/kontakt",
            "Bei Fragen rufen Sie uns gern an.",
            "Mit freundlichen Grüßen",
            "Ihre Evergreen Health",
        ],
    )
    c.setFont("Helvetica", 9)
    c.drawString(20 * mm, y - 2 * mm, "Anlage: Fragebogen zur Arbeitsunfähigkeit")
    footer(
        c,
        [
            "Evergreen Health · Körperschaft des öffentlichen Rechts · Hauptstraße 1 · 40998 Beispielstadt",
            "Bank: Beispielbank · IBAN DE00 0000 0000 0000 0000 00 · www.evergreen-health.example",
        ],
    )
    c.showPage()
    c.save()


# --------------------------------------------------------------------------- Bescheid (Widerspruch)


def make_bescheid() -> None:
    c = Canvas(str(OUT / "bescheid_wohngeld.pdf"), pagesize=A4)
    c.setTitle("Stadt Beispielstadt – Wohngeldbescheid (Beispiel)")
    c.setFont("Helvetica-Bold", 15)
    c.drawString(20 * mm, H - 22 * mm, "Stadt Beispielstadt")
    c.setFont("Helvetica", 10)
    c.drawString(20 * mm, H - 27.5 * mm, "Der Oberbürgermeister · Amt für Wohnen und Soziales")
    c.setStrokeColor(HexColor("#1f4e79"))
    c.setLineWidth(2)
    c.line(20 * mm, H - 31 * mm, W - 20 * mm, H - 31 * mm)
    window_address(
        c,
        "Stadt Beispielstadt · Amt 50 · 40990 Beispielstadt",
        ["Frau", "Erika Beispiel", "Lindenweg 7", "40999 Beispielstadt"],
    )
    info_block(
        c,
        [
            ("Aktenzeichen", "50.2-WG-2026-04711"),
            ("Sachbearbeitung", "Herr Beispielmann"),
            ("Zimmer", "3.14"),
            ("Telefon", "0999 300 41 41"),
            ("Datum", "14.10.2026"),
        ],
    )
    c.setFont("Helvetica-Bold", 12)
    c.drawString(20 * mm, H - 100 * mm, "Bescheid über Wohngeld (Mietzuschuss)")
    c.setFont("Helvetica", 10)
    c.drawString(20 * mm, H - 105.5 * mm, "Ihr Antrag vom 01.09.2026")
    y = paragraphs(
        c,
        H - 117 * mm,
        [
            "Sehr geehrte Frau Beispiel,",
            "auf Ihren Antrag vom 01.09.2026 bewillige ich Ihnen für die Zeit vom 01.10.2026 bis 30.09.2027 "
            "Wohngeld in Höhe von monatlich 96,00 Euro.",
            "**Begründung",
            "Bei der Berechnung wurde ein monatliches Gesamteinkommen von 1.684,00 Euro und eine zu "
            "berücksichtigende Miete von 612,00 Euro zugrunde gelegt. Ihr Krankengeld wurde als Einkommen "
            "angerechnet. Die Berechnung finden Sie in der Anlage.",
            "Ändert sich Ihr Einkommen oder Ihre Miete, teilen Sie mir das bitte sofort mit.",
            "**Rechtsbehelfsbelehrung",
            "Gegen diesen Bescheid kann innerhalb eines Monats nach Bekanntgabe Widerspruch erhoben werden. "
            "Der Widerspruch ist schriftlich oder zur Niederschrift bei der Stadt Beispielstadt, Amt für Wohnen "
            "und Soziales, Rathausplatz 1, 40990 Beispielstadt, einzulegen.",
            "Mit freundlichen Grüßen",
            "Im Auftrag",
            "Beispielmann",
        ],
        size=10,
    )
    c.setFont("Helvetica", 9)
    c.drawString(20 * mm, y - 2 * mm, "Anlage: Berechnungsbogen")
    footer(
        c,
        [
            "Stadt Beispielstadt · Rathausplatz 1 · 40990 Beispielstadt · Sprechzeiten Mo–Fr 8–12 Uhr",
            "Stadtkasse: IBAN DE00 1111 1111 1111 1111 11 · www.beispielstadt.example",
        ],
    )
    c.showPage()
    c.save()


# --------------------------------------------------------------------------- university re-registration


def make_uni() -> None:
    c = Canvas(str(OUT / "uni_rueckmeldung.pdf"), pagesize=A4)
    c.setTitle("Universität Beispielstadt – Rückmeldung (Beispiel)")
    c.setFillColor(HexColor("#5b2a86"))
    c.rect(20 * mm, H - 30 * mm, 12 * mm, 12 * mm, fill=1, stroke=0)
    c.setFillColor(white)
    c.setFont("Helvetica-Bold", 13)
    c.drawCentredString(26 * mm, H - 26 * mm, "UB")
    c.setFillColor(HexColor("#5b2a86"))
    c.setFont("Helvetica-Bold", 14)
    c.drawString(36 * mm, H - 23 * mm, "Universität Beispielstadt")
    c.setFont("Helvetica", 9.5)
    c.drawString(36 * mm, H - 28.5 * mm, "Studierendensekretariat")
    c.setFillColor(black)
    window_address(
        c,
        "Universität Beispielstadt · Studierendensekretariat · 40995 Beispielstadt",
        ["Herrn", "Jonas Muster", "Am Campus 12, App. 304", "40995 Beispielstadt"],
    )
    info_block(
        c,
        [
            ("Matrikelnummer", "7654321"),
            ("Studiengang", "B.Sc. Informatik"),
            ("Semester", "3. Fachsemester"),
            ("Datum", "20.10.2026"),
        ],
    )
    c.setFont("Helvetica-Bold", 12)
    c.drawString(20 * mm, H - 100 * mm, "Rückmeldung zum Sommersemester 2027")
    y = paragraphs(
        c,
        H - 112 * mm,
        [
            "Sehr geehrter Herr Muster,",
            "die Rückmeldefrist für das Sommersemester 2027 läuft vom 01.12.2026 bis zum 15.01.2027.",
            "Sie melden sich zurück, indem Sie den Semesterbeitrag von 312,40 Euro überweisen. Bitte geben Sie "
            "bei der Überweisung Ihre Matrikelnummer an.",
            "Empfänger: Universität Beispielstadt",
            "IBAN: DE00 2222 2222 2222 2222 22",
            "Verwendungszweck: 7654321 SoSe 2027",
            "**Der Beitrag muss bis spätestens 15.01.2027 auf unserem Konto eingegangen sein.",
            "Wenn der Beitrag nicht rechtzeitig eingeht, wird eine Säumnisgebühr von 20,00 Euro fällig. "
            "Wer sich bis zum Ende der Nachfrist nicht zurückmeldet, wird exmatrikuliert.",
            "Mit freundlichen Grüßen",
            "Ihr Studierendensekretariat",
        ],
    )
    footer(
        c,
        [
            "Universität Beispielstadt · Universitätsstraße 1 · 40995 Beispielstadt",
            "Servicezeiten: Mo–Do 9–15 Uhr · studierende@uni-beispielstadt.example",
        ],
    )
    c.showPage()
    c.save()


# --------------------------------------------------------------------------- PNG + photo


def render_png(name: str, dpi: int = 150) -> pathlib.Path:
    doc = pymupdf.open(OUT / f"{name}.pdf")
    pix = doc[0].get_pixmap(dpi=dpi, alpha=False)
    path = OUT / f"{name}.png"
    pix.save(path)
    return path


def make_photo(src_png: pathlib.Path, dst: pathlib.Path) -> None:
    """Phone-photo look: page slightly rotated with a shadow on a wooden table, warm light, JPEG."""
    tmp = OUT / "_tmp"
    tmp.mkdir(exist_ok=True)
    wood = tmp / "wood.png"
    page = tmp / "page.png"
    # Wood: stretched noise -> grain, coloured brown, a few darker planks.
    subprocess.run(
        [
            "magick", "-size", "600x2600", "xc:", "-seed", "7", "+noise", "Random", "-channel", "G", "-separate",
            "+channel", "-blur", "0x2", "-resize", "2600x2600!", "-motion-blur", "0x80+0", "-blur", "0x1.5",
            "-normalize", "-sigmoidal-contrast", "4x50%",
            "(", "-size", "1x256", "gradient:#3d2412-#a8713f", ")", "-clut",
            "(", "-size", "2600x2600", "xc:none", "-fill", "#00000030",
            "-draw", "rectangle 0,640 2600,650", "-draw", "rectangle 0,1300 2600,1312",
            "-draw", "rectangle 0,1960 2600,1969", ")", "-composite",
            "-crop", "2400x1800+100+400", "+repage", str(wood),
        ],
        check=True,
    )
    # Page: slightly warm paper tone, rotate, soft shadow.
    subprocess.run(
        [
            "magick", str(src_png), "-resize", "x1500", "-fill", "#fff6e6", "-colorize", "6%",
            "-background", "none", "-rotate", "-4.5",
            "(", "+clone", "-background", "black", "-shadow", "55x14+18+22", ")", "+swap",
            "-background", "none", "-layers", "merge", "+repage", str(page),
        ],
        check=True,
    )
    subprocess.run(
        [
            "magick", str(wood), str(page), "-gravity", "center", "-geometry", "+90+40", "-composite",
            # Uneven lighting: brighter top left, vignette.
            "(", "-size", "2400x1800", "radial-gradient:#ffffff-#a0a0a0", ")", "-compose", "multiply", "-composite",
            "-compose", "over", "-modulate", "105,92", "-blur", "0x0.6", "-attenuate", "0.25", "+noise", "Gaussian",
            "-resize", "2000x1500", "-quality", "82", "-sampling-factor", "4:2:0", str(dst),
        ],
        check=True,
    )
    for f in tmp.iterdir():
        f.unlink()
    tmp.rmdir()


def main() -> None:
    make_au(
        "au_erika_erst",
        first=True,
        since="20.10.2026",
        until="30.10.2026",
        found="20.10.2026",
        issued_note="Ausgestellt am 20.10.2026.",
    )
    make_au(
        "au_erika_folge",
        first=False,
        since="20.10.2026",
        until="06.11.2026",
        found="02.11.2026",
        issued_note="Ausgestellt am 02.11.2026.",
    )
    make_insurer_letter()
    make_bescheid()
    make_uni()
    for name in ["au_erika_erst", "au_erika_folge", "insurer_letter", "bescheid_wohngeld", "uni_rueckmeldung"]:
        render_png(name)
    make_photo(OUT / "au_erika_erst.png", OUT / "au_erika_erst_foto.jpg")
    print("ok:", sorted(p.name for p in OUT.iterdir() if p.suffix in {".pdf", ".png", ".jpg"}))


if __name__ == "__main__":
    main()
