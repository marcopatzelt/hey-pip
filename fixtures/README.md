# fixtures

OWNER: agent "docs-cli". Only invented data: no real people, no real insurers or offices, no real logos.
Every letter carries the small footer "Erfundenes Beispiel für Tests".

## letters/
Made by `python3 fixtures/letters/make_letters.py` (reportlab, pymupdf, ImageMagick). Each letter is a PDF plus a
150 dpi PNG; `<name>.expected.json` holds the expected fields and the deadlines the rules must create
(rule clock: 2026-10-26).

| file | what | expected deadline |
|---|---|---|
| `au_erika_erst.pdf/.png` | sick note (Erstbescheinigung) for Erika Beispiel, 20.10.–30.10.2026, Gesundkasse | `au_lueckenlos` 2026-11-02 |
| `au_erika_erst_foto.jpg` | the same sick note as a phone photo on a wooden table (SAM crops the sheet) | `au_lueckenlos` 2026-11-02 |
| `au_erika_folge.pdf/.png` | follow-up sick note until 06.11.2026 (closes the gap deadline of the first one) | `au_lueckenlos` 2026-11-09 |
| `kasse_brief.pdf/.png` | Gesundkasse asks for a questionnaire by 13.11.2026 | `frist_aus_brief` 2026-11-13 |
| `bescheid_wohngeld.pdf/.png` | Wohngeld decision of "Stadt Beispielstadt", 14.10.2026, with Rechtsbehelfsbelehrung | `widerspruch_bescheid` 2026-11-18 |
| `uni_rueckmeldung.pdf/.png` | university re-registration for Jonas Muster, fee due 15.01.2027 | `frist_aus_brief` 2027-01-15 |

Live accuracy check (real API, about 5 cents):
`cargo test -p copilot-core --test docs_live -- --ignored --nocapture`

## screens/
Made by `python3 fixtures/screens/make_screens.py` (headless Firefox, 1600x1000).
`kasse_form.png` (empty form), `kasse_form_filled.png`, `kasse_danke.png` (confirmation) and `<name>.boxes.json`
with the target boxes (xyxy pixels) for name, Versichertennummer, message, "Datei auswählen" and "Absenden",
each with a suggested `sam_query`. Boxes come from the page's `?mark` mode, so they match the PNG exactly.

## demo-kasse/
`index.html`: contact and upload form of the invented insurer "Gesundkasse". Fields: Anliegen, Name,
Versichertennummer, Nachricht, Krankschreibung (file, button "Datei auswählen", also drag and drop), big "Absenden".
Absenden checks the fields and shows "Danke, eingegangen am …". Nothing leaves the page. Open via `file://`
(allowed in DEMO_MODE). Query switches for screenshots only: `?fill`, `?thanks`, `?mark`.
