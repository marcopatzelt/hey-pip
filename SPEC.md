# SPEC – Papierkram-Copilot (Arbeitstitel)

Stand: 26.10.2026. Plan, Regeln und Zeitplan: `PLAN.md`. Diese Datei sagt, WAS wir bauen und WIE.

## 0. Grundsatz: Wir bauen für Menschen, die nicht technisch sind
Wir sind technisch und verstehen Computer Use. Die Nutzer nicht. Jede Entscheidung prüfen wir mit einer Frage: **"Versteht das meine Mutter beim ersten Mal?"**
- Keine Fachwörter in der Oberfläche. Keine Einrichtung über Dateien oder Terminal.
- Die Figur sagt immer, was sie gerade tut und warum ("Ich klicke jetzt auf Absenden. Darf ich?").
- Der Nutzer kann jederzeit stoppen: Maus bewegen oder Esc = sofort Pause, die Figur bestätigt das.
- Der erste Start ist ein Erlebnis, keine Einstellungsseite: Die Figur stellt sich vor, fragt drei Dinge (Name, Krankenkasse, Bundesland) und zeigt einmal, was sie kann.
- Fehler sagt die Figur freundlich und mit nächstem Schritt, nie als Fehlercode.
- UX steht vor Funktionsumfang: lieber drei Dinge, die sich großartig anfühlen, als zehn halbe.

## 1. Produkt in einem Satz
Ein kleiner Helfer auf dem Windows-Bildschirm, der Briefe und Bescheinigungen liest, sich Fristen merkt, sich rechtzeitig von selbst meldet und Formulare und Mails am Bildschirm selbst erledigt. Vor jedem Senden fragt er.

Zielgruppe: Menschen, die Behörden-Regeln nicht kennen oder nicht die Kraft dafür haben (Kranke, Ältere, Menschen mit Einschränkungen, Studenten, Zugezogene).

Auslöser (echte Geschichte, im Video erzählt): Die Folge-Krankschreibung von Marcos Mutter kam zwei Tage zu spät. Die Krankenkasse zahlte diese zwei Tage nicht, obwohl sie krank war.

## 2. Umfang

### Muss (MVP, für die Demo)
1. **Overlay-Figur** auf Windows: immer oben, durchsichtig, Klicks gehen durch. Sprechblase, Zeiger, Freigabe-Karte.
2. **Dokument rein**: Foto/PDF per Drag & Drop auf die Figur oder aus einem Ordner (z. B. `Downloads`). SAM 3.1 schneidet das Blatt aus, Muse Spark liest es als JSON.
3. **Gedächtnis**: Dokumente, Fakten über den Nutzer, Fristen in SQLite.
4. **Von selbst melden**: Regel-Motor prüft Fristen jede Minute und meldet sich (Figur + Windows-Mitteilung + Sprache).
5. **Computer Use**: Agenten-Schleife mit Bildschirmfoto → Muse Spark plant → SAM 3.1 findet das Ziel → Klick/Tippen → prüfen.
6. **Freigabe-Schranke**: Senden, Absenden, Hochladen privater Dokumente, Bezahlen, Löschen nur nach "Ja" auf der Karte.
7. **Stimme rein**: Taste halten, sprechen (Muse Voice Transcribe). **Stimme raus**: Windows-Sprachausgabe.
8. **Zwei Regeln** fertig: Folge-Krankschreibung (AU) lückenlos; Widerspruchsfrist bei Bescheiden.

### Soll (wenn Zeit)
- Privat-Modus: Muse Glimmer lokal liest Dokumente (Bonus Open Weight).
- UI-Baum (Windows UI Automation) als zweite Quelle neben SAM.
- `.ics`-Export der Fristen.
- Linux-Plattform-Teil.

### Nicht (bewusst weggelassen)
- Keine medizinische Bewertung. Die AU ist nur ein Dokument mit Zeitraum.
- Keine Rechtsberatung. Der Helfer nennt Regel + Quelle + "Bitte prüfen".
- Kein echter Versand in der Demo an echte Stellen. Ziel ist ein Demo-Postfach/Demo-Formular.
- Kein macOS. Keine Web-App. Keine anderen Produkte als Vorbild nennen.
- Keine echten Dokumente im Repo oder Video.

## 3. Nutzer-Abläufe

### A. Dokument erfassen
1. Nutzer zieht ein Foto der AU auf die Figur.
2. SAM 3.1 (`"sheet of paper"`) gibt die Box → zuschneiden.
3. Muse Spark liest → `DocumentExtract` (siehe 6.3).
4. Figur fasst zusammen: "Krankschreibung von Dr. Beispiel, 20.10. bis 30.10. Ich passe auf, dass keine Lücke entsteht."
5. Speichern: `documents`, `deadlines`.

### B. Von selbst melden
1. Regel-Motor sieht: AU endet Fr 30.10., keine Folge-AU gespeichert.
2. Do 29.10. 18:00: Figur meldet sich + Mitteilung + Sprache: "Deine Krankschreibung endet morgen. Wenn du noch krank bist: Die nächste muss spätestens Montag, 2.11. ausgestellt sein. Sonst zahlt die Kasse die Lücke nicht. Soll ich dich Montag früh erinnern und die Praxis-Nummer raussuchen?"
3. Knöpfe: "Ja, erinnern" / "Ich bin wieder gesund" / "Später".

### C. Am Bildschirm handeln
1. Nutzer (Stimme): "Schick die neue Krankschreibung an meine Krankenkasse."
2. Agent: findet AU im Gedächtnis, öffnet das Kontaktformular der Kasse (Demo-Seite) im Browser.
3. Für jedes Feld: Figur zeigt hin (SAM-Box), tippt den Wert.
4. Datei-Upload: Freigabe-Karte "Krankschreibung hochladen?".
5. Knopf "Absenden": Freigabe-Karte mit Zusammenfassung. Erst nach "Ja" klickt er.
6. Prüfen per Bildschirmfoto: Bestätigung sichtbar → Ereignis ins Gedächtnis.

## 4. Aufbau (Rust-Workspace)

```
papierkram-copilot/
├── Cargo.toml                 # workspace
├── crates/
│   ├── core/                  # ohne Betriebssystem, testbar auf Linux
│   │   ├── meta/              # Client: responses, files, asr, sam-Parser
│   │   ├── agent/             # Schleife, Tools, Prompt, Schranke
│   │   ├── memory/            # SQLite (rusqlite, FTS5)
│   │   ├── rules/             # Fristen-Regeln, Feiertage
│   │   └── docs/              # Dokument-Pipeline (SAM-Zuschnitt, Extraktion)
│   ├── platform/              # Trait + Umsetzungen
│   │   ├── windows.rs         # xcap, enigo, uiautomation
│   │   └── linux.rs           # später
│   └── app/                   # Tauri 2: Overlay, Karte, Tray, Hotkeys
│       └── ui/                # HTML/CSS/TS der Figur (klein, ohne Framework)
├── fixtures/                  # erfundene Beispielbriefe + Demo-Formular (HTML)
├── eval/                      # 10 Aufgaben + Messung
└── .env                       # MODEL_API_KEY (nicht im Repo)
```

Laufzeit: `tokio`. HTTP: `reqwest` (rustls) + `serde_json`. Fehler: `anyhow`/`thiserror`. Logs: `tracing` (nie Keys, nie Dokument-Inhalte in Logs).

### 4.1 Plattform-Trait
```rust
trait Screen  { fn capture(&self, monitor: MonitorId) -> Result<Frame>; fn monitors(&self) -> Vec<Monitor>; }
trait Input   { fn click(&self, p: PhysPoint) -> Result<()>; fn type_text(&self, s: &str) -> Result<()>;
                fn key(&self, combo: &str) -> Result<()>; fn scroll(&self, p: PhysPoint, dy: i32) -> Result<()>; }
trait UiTree  { fn element_at(&self, p: PhysPoint) -> Option<UiElement>; fn find(&self, name: &str) -> Vec<UiElement>; }
```
- Alle Koordinaten in **physischen Pixeln** (Windows-Skalierung beachten; Tauri setzt Per-Monitor-DPI).
- `Frame` = Bild + Monitor-Offset + Skalierung. Für die API verkleinern (lange Seite max. 1600 px), Koordinaten zurückrechnen.
- Die Figur selbst: vor dem Bildschirmfoto ihren Bereich merken und im Bild ausgrauen (nicht unsichtbar schalten, sonst fehlt sie im Demo-Video).

## 5. Meta-Modelle: wer macht was

| Aufgabe | Modell | Endpunkt |
|---|---|---|
| Planen, Bildschirm verstehen, Dokument lesen, Antworten schreiben | `muse-spark-1.3` | `POST /v1/responses` |
| Ziel auf dem Bildschirm finden (Text → Box), Blatt im Foto ausschneiden | `sam-3.1` | `POST /v1/responses` (stream) |
| Sprache → Text | `muse-voice-transcribe-1.0` | `POST /v1/asr/transcribe` |
| Figur/Grafiken gestalten (einmalig, offline) | `muse-image-1.0` | `POST /v1/images/generations` |
| Privat-Modus: Dokument lokal lesen (Soll) | Muse Glimmer 30B (Q4, llama.cpp `llama-server`) | lokal, OpenAI-kompatibel |
| Fallback Open-Weight | Muse Spark Open über Baseten `meta-models/muse-spark-open-1026` | Baseten |

Basis: `https://api.meta.ai/v1`, Header `Authorization: Bearer $MODEL_API_KEY`.

### 5.1 Responses API (Muse Spark)
- Eingabe: `input: [{type:"message", role:"user", content:[{type:"input_text"}, {type:"input_image", image_url:"data:image/png;base64,..."}]}]`. Feldname `image_url` am Tag 1 prüfen.
- Werkzeuge: `tools: [{type:"function", name, description, parameters, strict:true}]`. Nur `tool_choice:"auto"` geht. Mehrere Calls pro Antwort möglich → alle ausführen, je ein `function_call_output` mit `call_id` zurück.
- Verlauf: `previous_response_id`. **`instructions`, `tools`, `reasoning` bei jedem Aufruf neu mitschicken.**
- `reasoning: {effort: "low"}` für Bildschirm-Schritte, `"medium"` für Dokumente/Planung. `"none"` gibt Fehler 400.
- Streaming (`stream:true`) für Sprechblase: `response.output_text.delta`, `response.function_call_arguments.done`, `response.completed`.
- Argumente des Modells immer selbst prüfen (serde + Grenzen).

### 5.2 SAM 3.1
- Anfrage: `model:"sam-3.1"`, `input` mit `input_text` (kurze Nominalphrase, z. B. `"Absenden button"`) + `input_image`, `stream:true`.
- Antwort: Text mit Spezial-Tokens, eine Zeile pro Frame: `<0f>` dann pro Treffer `id<|box;x1=..;y1=..;x2=..;y2=..;w=..;h=..|><|mask;...|>`.
- Boxen: Pixel im Quellbild, Format xyxy, `w/h` = Bildgröße. Masken (base85) brauchen wir nicht → nicht dekodieren.
- `include_confidence:"true"` setzen → `c=` nutzen, um zwischen mehreren Treffern zu wählen.
- Kein Treffer → keine Zeile. Dann: Muse Spark neu fragen (anderer Begriff) oder UI-Baum.
- Parser in Rust mit `regex`, Unit-Tests mit festen Beispielzeilen.

### 5.3 Zielwahl (Grounding)
**Test 26.10. (echter Aufruf):** SAM 3.1 findet Knöpfe auf Bildschirmfotos nach Form ("Create API key button" → richtiger Knopf + ein zweiter blauer Knopf, 1,7 s), liest aber keinen Text ("Billing" → kein Treffer). `include_confidence` gibt Fehler 400 (unbekannter Parameter). Darum: SAM liefert Kandidaten-Boxen, Muse Spark wählt per Nummer, UI-Baum bestätigt den Namen.

1. Muse Spark sagt: `point/click { target: "Absenden-Knopf unten rechts im Formular", sam_query: "submit button" }`.
2. SAM 3.1 gibt 0..n Boxen.
3. Bei mehreren: Muse Spark bekommt das Bild mit nummerierten Boxen und wählt eine Nummer.
4. Klickpunkt = Box-Mitte → physische Pixel.
5. Optional: UI-Baum am Punkt fragen (`element_at`) → Name prüfen ("Absenden"?). Wichtig für die Schranke.

## 6. Agent

### 6.1 Schleife
```
loop (max 25 Schritte, max 3 min):
  frame = screen.capture()
  resp  = muse_spark(instructions, memory_context, frame, letzter Schritt, tools)
  für jeden tool_call:
     risk = gate.classify(call, ui_element_name)
     if risk == Commit: karte zeigen, warten (Ja/Nein/Timeout = Nein)
     ergebnis = ausführen
  if resp hat Text ohne tool_call: in Sprechblase zeigen, Schleife Ende
  Nutzer bewegt Maus/drückt Esc → sofort Pause
```

### 6.2 Werkzeuge (Tools)
| Tool | Argumente | Stufe |
|---|---|---|
| `say` | `text` | frei |
| `point` | `sam_query`, `description` | frei (zeigt nur) |
| `click` | `sam_query`, `description`, `button?` | handeln (sichtbar) / Commit, wenn Ziel nach Senden aussieht |
| `type_text` | `text`, `into_sam_query?` | handeln |
| `key` | `combo` | handeln; `Enter` in Formular = Commit-Prüfung |
| `scroll` | `direction`, `amount` | handeln |
| `open_url` | `url` | handeln (nur http/https) |
| `wait` | `seconds` (≤ 10) | frei |
| `remember_fact` | `key`, `value`, `source_doc_id?` | frei |
| `search_memory` | `query` | frei |
| `add_deadline` | `title`, `due`, `rule_id?`, `doc_id?`, `remind_at[]` | frei (Karte zeigt sie danach) |
| `draft_message` | `to`, `subject`, `body`, `attachments[]` | frei (nur Entwurf) |
| `ask_user` | `question`, `options[]` | frei |
| `request_approval` | `summary`, `action` | – (Modell darf es selbst anfordern) |
| `done` | `summary` | frei |

### 6.3 Schranke (Freigabe)
- Stufen: **frei** (lesen, zeigen, merken), **handeln** (klicken, tippen; sichtbar, jederzeit stoppbar), **Commit** (Senden, Absenden, Abschicken, Hochladen eines privaten Dokuments, Bezahlen, Kaufen, Löschen, Kündigen, Submit, Send, Pay, Upload, Delete).
- Commit wird an DREI Stellen erkannt, eine reicht: Modell meldet es, Wortliste auf `description`/`sam_query`, Name aus dem UI-Baum am Klickpunkt.
- Karte zeigt: was, an wen, welche Anlagen, Bildausschnitt des Knopfs. Knöpfe: "Ja, senden" / "Ändern" / "Nein". Ohne Antwort in 5 min = Nein.
- Jede Commit-Entscheidung → `actions_log`.

### 6.4 Prompt (Kern)
- Rolle: Helfer für Papierkram. Kurze, einfache Sätze. Sprache des Nutzers.
- Regeln: keine Medizin-Bewertung, keine Rechtsberatung (Regel + Quelle + "bitte prüfen"), nie ohne Freigabe senden, nie Passwörter tippen, bei Login/Captcha/2FA an den Nutzer übergeben (`ask_user`).
- Kontext pro Aufruf: Profil (Name, Kasse, Arbeitgeber), offene Fristen, letzte 5 Ereignisse, Treffer aus `search_memory`.

## 7. Dokumente

### 7.1 Ablauf
Foto/PDF → (Foto: SAM `"sheet of paper"`, zuschneiden, gerade ziehen nur per Box) → Muse Spark (PDF direkt) → JSON → prüfen → speichern → Regeln anwenden.

### 7.2 `DocumentExtract` (JSON, vom Modell)
```json
{
  "doc_type": "au_bescheinigung | bescheid | rechnung | mahnung | vertrag | brief | sonstiges",
  "sender": "string", "recipient": "string",
  "date": "YYYY-MM-DD", "reference": "Aktenzeichen/Versichertennummer",
  "summary_simple": "max 3 Sätze, einfache Sprache",
  "requests": ["was verlangt wird"],
  "deadlines": [{"title": "", "due": "YYYY-MM-DD", "source_text": "Zitat"}],
  "au": {"from": "YYYY-MM-DD", "until": "YYYY-MM-DD", "first_or_follow": "erst|folge", "issued": "YYYY-MM-DD"},
  "consequences_if_ignored": "string",
  "confidence": 0.0
}
```
Daten mit `source_text` belegen; ohne Zitat → als "unsicher" zeigen.

## 8. Gedächtnis (SQLite)
| Tabelle | Inhalt |
|---|---|
| `profile` | key, value, updated_at (Name, Kasse, Arbeitgeber, Bundesland, Sprache) |
| `documents` | id, path (lokal), sha256, doc_type, extract_json, created_at |
| `documents_fts` | FTS5 über Text + Zusammenfassung |
| `deadlines` | id, title, due, rule_id, doc_id, status (offen/erledigt/verworfen), remind_at JSON |
| `reminders` | id, deadline_id, at, fired_at, answer |
| `events` | id, at, kind, text (Verlauf: "AU an Kasse geschickt") |
| `actions_log` | id, at, tool, args_redacted, risk, approved, result |

Ort: `%APPDATA%/papierkram-copilot/`. Dokumente bleiben lokal; an die API geht nur das, was ein Aufruf braucht.

## 9. Regel-Motor
- Läuft jede Minute (`tokio::time::interval`). Prüft `deadlines` und Regel-Bedingungen. Feuert Erinnerungen, nie doppelt (`fired_at`).
- Regeln als Rust-Code mit Quelle und Testfällen, nicht vom Modell erfunden.

| Regel | Bedingung | Frist | Quelle (vor Demo prüfen) |
|---|---|---|---|
| `au_lueckenlos` | AU mit `until = E`, keine Folge-AU ab E+1 | Folge-AU spätestens am nächsten Werktag nach E (Samstag zählt nicht als Werktag) | § 46 Satz 2 SGB V (Krankengeld) |
| `widerspruch_bescheid` | `doc_type = bescheid` mit Datum D | 1 Monat ab Bekanntgabe; Bekanntgabe = D + 4 Tage (Post) | § 84 SGG / § 70 VwGO, § 37 Abs. 2 SGB X |
| `frist_aus_brief` | `deadlines[]` aus dem Dokument | wie im Brief | Brief selbst |

- Erinnerungen: 3 Tage vorher, 1 Tag vorher (18:00), am Tag (09:00). Feiertage: NRW 2026/2027 fest im Code.
- Jede Meldung endet mit: "Das ist ein Hinweis, keine Rechtsberatung."

## 10. Pip – Figur und Gefühl
- Name: **Pip**. Weicher blauer Tropfen (Meta-Blau #0866FF → Violett), kleines gefaltetes Papier als Haarbüschel. Vorlage: `design/maskottchen/b_tropfen_1.png`, gemacht mit Muse Image.
- Marke: passt zur Farbwelt von Meta, nutzt aber **kein Meta-Logo und keine Meta-Figur** (fremde Assets = Disqualifikation). Im Video: "Built with Meta Muse + SAM".
- Platz: ruht klein (ca. 72 px) unten rechts. Wenn Pip zeigt oder handelt, fliegt er in einer weichen Kurve zum Ziel und zeigt darauf; danach zurück.
- Posen (Muse Image, aus der Vorlage): ruhig, hört zu, denkt, zeigt, freut sich, fragt (Freigabe), schläft (Pause). Freistellen: weißer Hintergrund → transparent (SAM 3.1 Maske oder Farbschlüssel).
- Bewegung: leichtes Atmen im Ruhezustand, Blinzeln, kleine Hüpfer bei Meldungen. Keine hektischen Effekte.
- Sprechblase: große Schrift (mind. 18 px), max. 2 Sätze, immer mit klaren Knöpfen.
- Stimme: ruhig, langsam, Deutsch oder Englisch.
- Erster Start: Pip fliegt ins Bild, stellt sich vor, fragt Name, Krankenkasse, Bundesland, zeigt einmal "Zieh einen Brief auf mich".

## 10b. Overlay (Tauri 2)
- Fenster: `transparent`, `decorations:false`, `alwaysOnTop`, `skipTaskbar`, ganzer Bildschirm; `set_ignore_cursor_events(true)`, aus nur über Figur/Karte.
- Zustände der Figur: ruhig, hört zu, denkt, zeigt (Pfeil zur Box), handelt, wartet auf Freigabe, Fehler.
- Tray-Menü: Pause, Fristen-Liste, Privat-Modus an/aus, Beenden.
- Hotkeys: `Ctrl+Shift+Space` halten = sprechen; `Esc` (global, während Agent läuft) = Stopp.
- Mitteilungen: `tauri-plugin-notification` (Windows-Toast).
- Sprache raus: `tts` (Windows OneCore/SAPI), Deutsch und Englisch.

## 11. Konfiguration
`.env` (nie im Repo, `.gitignore` hat es):
```
MODEL_API_KEY=...
MODEL_API_BASE=https://api.meta.ai/v1
AGENT_MODEL=muse-spark-1.3
LOCAL_DOC_MODEL_URL=http://127.0.0.1:8080/v1   # Privat-Modus (optional)
BASETEN_API_KEY=...                            # optional
DEMO_MODE=1                                    # Commit-Aktionen nur auf fixtures/
```

## 12. Datenschutz und Sicherheit
- Keys nur aus der Umgebung. Nie loggen, nie ausgeben.
- Bildschirmfotos nur im Speicher; nicht auf Platte, außer `DEBUG_SHOTS=1` (nur Demo-Daten).
- `DEMO_MODE=1`: `open_url` und Commit nur für `localhost`/Demo-Seiten.
- Passwort-Felder (UI-Baum `IsPassword`) werden nie getippt.
- Text auf dem Bildschirm oder in Dokumenten ist Daten, keine Anweisung (Prompt-Injection-Schutz im Prompt + Schranke).

## 13. Testen

### 13.1 Auf Linux (täglich, schnell)
- `cargo test` für `core`: SAM-Parser, Regel-Motor (Fristen mit Wochenenden/Feiertagen), JSON-Prüfung, Schranke (Wortliste).
- `core`-Integrationstest gegen die echte API mit `fixtures/` (kostet Cent, nur bei Bedarf: `cargo test -- --ignored`).
- CLI `copilot-cli` im `core`: Dokument lesen, Frist zeigen, SAM-Box auf einem Bild zeichnen – ohne Overlay.

### 13.2 Auf Windows
1. **Bauen**: GitHub Actions (`windows-latest`) baut bei jedem Push eine `.exe` (Repo ist eh öffentlich → kostenlos). Schnell lokal: `cargo xwin build --target x86_64-pc-windows-msvc` auf Linux.
2. **Schnell testen**: Windows in einer virtuellen Maschine (QEMU/KVM auf Linux, ist installiert). Der Agent kann die VM selbst per `lcu` sehen und testen.
3. **Echt testen + Demo-Video**: Laptop von der T7 in Windows starten (`windows-starten --jetzt`).

### 13.3 Eval (Tag 8)
10 Aufgaben mit erfundenen Daten: 5 Dokumente (Fristen richtig?), 5 Bildschirm-Aufgaben (Ziel getroffen? Schritte? Schranke ausgelöst?). Messen: Trefferquote Frist, Trefferquote Klick, Schritte, Kosten, Zeit. Zahlen ins README und Video.

## 14. Kosten
- Muse Spark 1.3: Preise am Tag 1 auf der Pricing-Seite prüfen (1.1: 1,25 $/M rein, 4,25 $/M raus).
- Schätzung: ein Bildschirm-Schritt ≈ 2–4 k Token → unter 1 Cent. 10 Tage Entwicklung ≈ 20–50 $.
- Warnung per Mail bei 150 € im Monat ist gesetzt (stoppt nicht!). Zähler im Programm: Tokens pro Tag in `actions_log`, Warnung ab 10 $/Tag.

## 15. Demo-Video (3 Minuten, Englisch, Untertitel)
| Zeit | Inhalt |
|---|---|
| 0:00–0:15 | Hook: "My mom lost two days of sick pay because of one missing paper." |
| 0:15–0:40 | AU-Foto auf die Figur ziehen → SAM schneidet aus → Erklärung in einfacher Sprache |
| 0:40–1:10 | Zeitsprung: Figur meldet sich von selbst ("ends tomorrow, next one by Monday") |
| 1:10–2:10 | Stimme: "Send it to my insurance" → Figur zeigt und füllt das Formular, Freigabe-Karte, Ja |
| 2:10–2:35 | Zweites Beispiel: Bescheid → Widerspruchsfrist; Student → Rückmeldefrist |
| 2:35–3:00 | Wie es gebaut ist: 5 Meta-Modelle, Rust, Privat-Modus, Eval-Zahlen |

## 16. Risiken und Ausweg
| Risiko | Ausweg |
|---|---|
| SAM findet UI-Elemente schlecht (trainiert auf Fotos) | Tag 1 testen. Sonst: UI-Baum zuerst, SAM für Dokumente + als Zweitquelle; Muse Spark gibt Koordinaten im Notfall selbst |
| Overlay-Transparenz/Click-through auf Windows hakt | Fallback: kleines randloses Fenster in der Ecke statt Vollbild-Overlay |
| Glimmer lokal zu langsam | Baseten Muse Spark Open, oder Privat-Modus streichen |
| VM zu langsam/zickig | direkt von der T7 testen |
| Zeit | Reihenfolge in PLAN.md; Soll-Punkte zuerst streichen |

## 17. Fertig heißt
- Demo-Ablauf A–C läuft dreimal hintereinander auf Windows ohne Eingriff.
- `cargo test` grün, Eval-Zahlen im README.
- Repo öffentlich, Lizenz (MIT oder Apache-2.0), README mit Schaubild, Video ≤ 3 min, X-Post, Abgabe vor 4.11. 21:00 ET.
