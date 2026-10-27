#!/usr/bin/env python3
"""Build the invented example letters (PDF + PNG) and the phone photo of the sick note.

All people, insurers, offices, numbers and addresses are INVENTED. No real logos.
Run from anywhere:  python3 fixtures/letters/make_letters.py [flight|flight-variants]
(flight: only the Beispiel Air family; flight-variants: only the e-mail screenshot, the phone photo and the German
notice of BX 2041)
Needs: reportlab, pymupdf, ImageMagick 7 (`magick`).
"""

import pathlib
import subprocess
import sys

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


# --------------------------------------------------------------------------- Beispiel Air (flight family)

AIR = HexColor("#c8102e")
AIR_DARK = HexColor("#7a0a1c")


def logo_beispiel_air(c: Canvas, x: float, y: float) -> None:
    c.setFillColor(AIR)
    p = c.beginPath()  # a simple wing, no real logo
    p.moveTo(x, y)
    p.lineTo(x + 12 * mm, y + 6 * mm)
    p.lineTo(x + 9 * mm, y)
    p.close()
    c.drawPath(p, fill=1, stroke=0)
    c.setFont("Helvetica-Bold", 18)
    c.drawString(x + 15 * mm, y + 0.5 * mm, "Beispiel Air")
    c.setFillColor(black)


def email_header(c: Canvas, rows: list[tuple[str, str]], top: float) -> float:
    """The head of a printed e-mail: From / Sent / To / Subject, a rule below."""
    y = top
    for label, value in rows:
        c.setFont("Helvetica-Bold", 9.5)
        c.drawString(20 * mm, y, label)
        c.setFont("Helvetica", 9.5)
        c.drawString(42 * mm, y, value)
        y -= 5.2 * mm
    c.setStrokeColor(HexColor("#bbbbbb"))
    c.line(20 * mm, y + 1.5 * mm, W - 20 * mm, y + 1.5 * mm)
    return y - 6 * mm


def flight_table(c: Canvas, y: float, rows: list[tuple[str, str]]) -> float:
    c.setFillColor(HexColor("#f6f1f2"))
    c.rect(20 * mm, y - len(rows) * 6.2 * mm + 2 * mm, W - 40 * mm, len(rows) * 6.2 * mm + 2 * mm, fill=1, stroke=0)
    c.setFillColor(black)
    for label, value in rows:
        c.setFont("Helvetica", 9.5)
        c.setFillColor(GREY)
        c.drawString(24 * mm, y - 2 * mm, label)
        c.setFillColor(black)
        c.setFont("Helvetica-Bold", 10)
        c.drawString(75 * mm, y - 2 * mm, value)
        y -= 6.2 * mm
    return y - 6 * mm


def make_flight_email(name: str, *, sent: str, subject: str, intro: list[str], rows: list[tuple[str, str]],
                      outro: list[str]) -> None:
    """A printed e-mail of the invented airline "Beispiel Air" (the passenger printed it to keep it)."""
    c = Canvas(str(OUT / f"{name}.pdf"), pagesize=A4)
    c.setTitle(f"{subject} (Beispiel)")
    c.setFont("Helvetica", 7.5)
    c.setFillColor(GREY)
    c.drawString(20 * mm, H - 12 * mm, f"Printed e-mail · {sent}")
    c.setFillColor(black)
    y = email_header(
        c,
        [
            ("From:", "Beispiel Air Customer Service <service@beispiel-air.example>"),
            ("Sent:", sent),
            ("To:", "Erika Beispiel <erika.beispiel@example.com>"),
            ("Subject:", subject),
        ],
        H - 22 * mm,
    )
    logo_beispiel_air(c, 20 * mm, y - 6 * mm)
    y = paragraphs(c, y - 18 * mm, intro, size=10.5)
    y = flight_table(c, y, rows)
    paragraphs(c, y, outro, size=10.5)
    footer(
        c,
        [
            "Beispiel Air · Flughafenstraße 1 · 40999 Beispielstadt · www.beispiel-air.example",
            "You receive this e-mail because you booked a flight with Beispiel Air.",
        ],
    )
    c.showPage()
    c.save()


def make_flight_letters() -> None:
    sign = ["Kind regards,", "Your Beispiel Air customer service"]
    make_flight_email(
        "flight_delay_bx2041",
        sent="Sunday, 4 October 2026, 09:12",
        subject="Beispiel Air – Information about your flight BX 2041",
        intro=[
            "Dear Ms Beispiel,",
            "we are sorry that your flight arrived late. Here are the details of your journey:",
        ],
        rows=[
            ("Passenger", "Erika Beispiel"),
            ("Booking reference", "BX7Q2K"),
            ("Flight", "BX 2041"),
            ("Route", "Düsseldorf (DUS) → Tenerife South (TFS)"),
            ("Date of flight", "3 October 2026"),
            ("Scheduled arrival", "14:10"),
            ("Actual arrival", "18:20 (4 h 10 min late)"),
            ("Reason", "late arrival of the aircraft from an earlier flight"),
        ],
        outro=[
            "You can submit a request at beispiel-air.example/claim. Please have your booking reference and your "
            "boarding pass ready.",
            "We hope to welcome you on board again soon.",
            *sign,
        ],
    )
    make_flight_email(
        "flight_delay_short",
        sent="Tuesday, 13 October 2026, 08:40",
        subject="Beispiel Air – Information about your flight BX 1180",
        intro=[
            "Dear Ms Beispiel,",
            "we are sorry that your flight arrived late. Here are the details of your journey:",
        ],
        rows=[
            ("Passenger", "Erika Beispiel"),
            ("Booking reference", "BX4M8P"),
            ("Flight", "BX 1180"),
            ("Route", "Düsseldorf (DUS) → Palma de Mallorca (PMI)"),
            ("Date of flight", "12 October 2026"),
            ("Scheduled arrival", "11:05"),
            ("Actual arrival", "12:25 (1 h 20 min late)"),
            ("Reason", "busy airspace"),
        ],
        outro=["We hope to welcome you on board again soon.", *sign],
    )
    make_flight_email(
        "flight_cancel_notice",
        sent="Monday, 26 October 2026, 10:05",
        subject="Beispiel Air – Change to your flight BX 2305",
        intro=[
            "Dear Ms Beispiel,",
            "we have had to cancel your flight BX 2305 on 20 November 2026. We have already booked you on another "
            "flight on the same day at no extra cost:",
        ],
        rows=[
            ("Passenger", "Erika Beispiel"),
            ("Booking reference", "BX9R3T"),
            ("Cancelled flight", "BX 2305, 20 November 2026, 07:40"),
            ("Route", "Köln/Bonn (CGN) → Lisbon (LIS)"),
            ("Your new flight", "BX 2307, 20 November 2026, 09:15"),
            ("New arrival", "11:50 (scheduled before: 10:15)"),
        ],
        outro=[
            "You do not need to do anything. If the new flight does not suit you, you can ask for a refund of your "
            "ticket at beispiel-air.example/refund.",
            "We are sorry for the change.",
            *sign,
        ],
    )
    make_flight_email(
        "flight_cancel_late",
        sent="Saturday, 10 October 2026, 18:30",
        subject="Beispiel Air – Your flight BX 2305 has been cancelled",
        intro=[
            "Dear Ms Beispiel,",
            "we are sorry to tell you that your flight BX 2305 on 14 October 2026 has been cancelled.",
        ],
        rows=[
            ("Passenger", "Erika Beispiel"),
            ("Booking reference", "BX5L7W"),
            ("Cancelled flight", "BX 2305, 14 October 2026, 07:40"),
            ("Route", "Köln/Bonn (CGN) → Lisbon (LIS)"),
            ("Other flight", "none on this route this week"),
        ],
        outro=[
            "Unfortunately we cannot offer you another flight. You can ask for a refund of your ticket at "
            "beispiel-air.example/refund. You can submit a request at beispiel-air.example/claim.",
            "We apologise for the inconvenience.",
            *sign,
        ],
    )
    make_boarding_pass()


def make_boarding_pass() -> None:
    """One-page boarding pass of the flight in flight_delay_bx2041: the document Pip attaches to the claim."""
    c = Canvas(str(OUT / "boarding_pass_bx2041.pdf"), pagesize=A4)
    c.setTitle("Beispiel Air – Boarding pass BX 2041 (Beispiel)")
    x0, y0, w, h = 20 * mm, H - 120 * mm, W - 40 * mm, 95 * mm
    c.setStrokeColor(AIR_DARK)
    c.setLineWidth(1)
    c.roundRect(x0, y0, w, h, 4 * mm, fill=0)
    c.setFillColor(AIR)
    c.roundRect(x0, y0 + h - 16 * mm, w, 16 * mm, 4 * mm, fill=1, stroke=0)
    c.rect(x0, y0 + h - 16 * mm, w, 6 * mm, fill=1, stroke=0)
    c.setFillColor(white)
    c.setFont("Helvetica-Bold", 16)
    c.drawString(x0 + 6 * mm, y0 + h - 11 * mm, "Beispiel Air")
    c.drawRightString(x0 + w - 6 * mm, y0 + h - 11 * mm, "BOARDING PASS")
    c.setFillColor(black)

    def cell(x: float, y: float, label: str, value: str, size: float = 13) -> None:
        c.setFont("Helvetica", 7.5)
        c.setFillColor(GREY)
        c.drawString(x, y, label)
        c.setFillColor(black)
        c.setFont("Helvetica-Bold", size)
        c.drawString(x, y - 6 * mm, value)

    top = y0 + h - 26 * mm
    cell(x0 + 6 * mm, top, "PASSENGER", "BEISPIEL / ERIKA MS")
    cell(x0 + 110 * mm, top, "BOOKING REFERENCE", "BX7Q2K")
    cell(x0 + 6 * mm, top - 17 * mm, "FROM", "DÜSSELDORF DUS", 15)
    cell(x0 + 70 * mm, top - 17 * mm, "TO", "TENERIFE SOUTH TFS", 15)
    cell(x0 + 6 * mm, top - 34 * mm, "FLIGHT", "BX 2041")
    cell(x0 + 40 * mm, top - 34 * mm, "DATE", "03 OCT 2026")
    cell(x0 + 80 * mm, top - 34 * mm, "BOARDING", "09:20")
    cell(x0 + 108 * mm, top - 34 * mm, "GATE", "B14")
    cell(x0 + 130 * mm, top - 34 * mm, "SEAT", "14C")
    # An invented bar code: plain stripes, it encodes nothing.
    bx, by = x0 + 6 * mm, y0 + 6 * mm
    widths = [0.4, 0.8, 0.4, 1.2, 0.4, 0.4, 0.8, 1.2, 0.4, 0.8] * 9
    for i, bw in enumerate(widths):
        if i % 2 == 0:
            c.rect(bx, by, bw * mm, 10 * mm, fill=1, stroke=0)
        bx += bw * mm
    c.setFont("Helvetica", 8)
    c.setFillColor(GREY)
    c.drawString(x0 + 110 * mm, y0 + 12 * mm, "Gate closes 20 minutes before departure.")
    c.drawString(x0 + 110 * mm, y0 + 8 * mm, "Departure 10:05 · Arrival 14:10")
    c.setFillColor(black)
    footer(c, ["Beispiel Air · Flughafenstraße 1 · 40999 Beispielstadt · www.beispiel-air.example"])
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


def make_flight_de_minutes() -> None:
    """The German notice of the same flight, the delay given only in minutes ("245 Minuten")."""
    c = Canvas(str(OUT / "flight_delay_de_minutes.pdf"), pagesize=A4)
    c.setTitle("Beispiel Air – Information zu Ihrem Flug BX 2041 (Beispiel)")
    c.setFont("Helvetica", 7.5)
    c.setFillColor(GREY)
    c.drawString(20 * mm, H - 12 * mm, "Gedruckte E-Mail · Sonntag, 4. Oktober 2026, 09:12")
    c.setFillColor(black)
    y = email_header(
        c,
        [
            ("Von:", "Beispiel Air Kundenservice <service@beispiel-air.example>"),
            ("Gesendet:", "Sonntag, 4. Oktober 2026, 09:12"),
            ("An:", "Erika Beispiel <erika.beispiel@example.com>"),
            ("Betreff:", "Beispiel Air – Information zu Ihrem Flug BX 2041"),
        ],
        H - 22 * mm,
    )
    logo_beispiel_air(c, 20 * mm, y - 6 * mm)
    y = paragraphs(
        c,
        y - 18 * mm,
        ["Sehr geehrte Frau Beispiel,",
         "Ihr Flug ist leider verspätet angekommen. Hier die Angaben zu Ihrer Reise:"],
        size=10.5,
    )
    y = flight_table(
        c,
        y,
        [
            ("Fluggast", "Erika Beispiel"),
            ("Buchungsnummer", "BX7Q2K"),
            ("Flug", "BX 2041"),
            ("Strecke", "Düsseldorf (DUS) → Teneriffa Süd (TFS)"),
            ("Flugdatum", "03.10.2026"),
            ("Ankunftsverspätung", "245 Minuten"),
            ("Grund", "verspätete Ankunft des Flugzeugs aus einem früheren Flug"),
        ],
    )
    paragraphs(
        c,
        y,
        ["Anträge können Sie unter beispiel-air.example/claim stellen. Bitte halten Sie Ihre Buchungsnummer und Ihre "
         "Bordkarte bereit.",
         "Mit freundlichen Grüßen", "Ihr Beispiel Air Kundenservice"],
        size=10.5,
    )
    footer(
        c,
        [
            "Beispiel Air · Flughafenstraße 1 · 40999 Beispielstadt · www.beispiel-air.example",
            "Sie erhalten diese E-Mail, weil Sie einen Flug bei Beispiel Air gebucht haben.",
        ],
    )
    c.showPage()
    c.save()


def make_flight_mail_png() -> None:
    """flight_delay_bx2041_mail.png: the notice as an e-mail open in a web mail, 1600x1000 (headless Firefox through
    fixtures/screens/make_outline.py; the page is flight_delay_bx2041_mail.html)."""
    sys.path.insert(0, str(OUT.parent / "screens"))
    from make_outline import capture

    tmp = OUT / "_mail"
    capture(str(OUT / "flight_delay_bx2041_mail.html"), "mail", "", None, tmp, False)
    (tmp / "mail.png").replace(OUT / "flight_delay_bx2041_mail.png")
    for f in tmp.iterdir():
        f.unlink()
    tmp.rmdir()


def main() -> None:
    if sys.argv[1:] == ["flight-variants"]:
        # Run 4: the same notice as an e-mail screenshot, as a phone photo, and in German with the delay in minutes.
        # The existing letters stay byte for byte (the photo starts from the PNG that is already there).
        make_flight_mail_png()
        make_photo(OUT / "flight_delay_bx2041.png", OUT / "flight_delay_bx2041_foto.jpg")
        make_flight_de_minutes()
        print("ok: flight variants")
        return
    if sys.argv[1:] == ["flight"]:
        # Only the flight family (the other letters stay byte for byte as they are).
        make_flight_letters()
        render_png("flight_delay_bx2041")
        print("ok: flight letters")
        return
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
    make_flight_letters()
    for name in ["au_erika_erst", "au_erika_folge", "insurer_letter", "bescheid_wohngeld", "uni_rueckmeldung",
                 "flight_delay_bx2041"]:
        render_png(name)
    make_photo(OUT / "au_erika_erst.png", OUT / "au_erika_erst_foto.jpg")
    print("ok:", sorted(p.name for p in OUT.iterdir() if p.suffix in {".pdf", ".png", ".jpg"}))


if __name__ == "__main__":
    main()
