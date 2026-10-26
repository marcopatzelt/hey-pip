# SPEC – Papierkram-Copilot (Arbeitstitel)

Stand: 26.10.2026. Plan, Regeln und Zeitplan: `PLAN.md`. Diese Datei sagt, WAS wir bauen und WIE.

## 0. Grundsatz: Wir bauen für Menschen, die nicht technisch sind
Wir sind technisch und verstehen Computer Use. Die Nutzer nicht. Jede Entscheidung prüfen wir mit einer Frage: **"Versteht das meine Mutter beim ersten Mal?"**
- Keine Fachwörter in der Oberfläche. Keine Einrichtung über Dateien oder Terminal.
- Die Figur sagt immer, was sie gerade tut und warum ("Ich klicke jetzt auf Absenden. Darf ich?").
- Der Nutzer kann jederzeit stoppen: Maus bewegen oder Esc = sofort Pause, die Figur bestätigt das.
- Der erste Start ist ein Erlebnis, keine Einstellungsseite: Die Figur stellt sich vor, fragt nur den Namen und zeigt einmal, was sie kann. Die Krankenkasse liest sie aus dem ersten Brief; das Bundesland fragt sie erst, wenn eine Frist es braucht (Stand 26.10., siehe §10).
- Fehler sagt die Figur freundlich und mit nächstem Schritt, nie als Fehlercode.
- UX steht vor Funktionsumfang: lieber drei Dinge, die sich großartig anfühlen, als zehn halbe.

## 1. Produkt in einem Satz
Pip ist Computer Use für Alltagsmenschen: ein kleiner Helfer auf dem Windows-Bildschirm, der versteht, was du gerade tust, Vorschläge macht, sich Dinge über dich merkt und den Computer für dich bedient und Formulare ausfüllt. Vor jedem Senden fragt er.

**Nicht an einen Anwendungsfall gebunden (Marco, 26.10.):** Pip passt sich dem Nutzer an. Papierkram (Briefe, Fristen, Krankschreibung) ist nur die Hauptgeschichte für Demo und Video. Die Architektur bleibt allgemein: Gedächtnis über den Nutzer, Vorschläge aus dem, was auf dem Bildschirm passiert, Handeln am Bildschirm, Freigabe. Weitere Beispiele: Anträge, Anmeldungen, Studien-Fristen, Bewerbungen schreiben (nicht bewerten: "Job-Application-Screener" ist verboten), Online-Formulare aller Art.
- Für die Jury: eine starke, echte Geschichte zeigen, dann in 20 Sekunden zeigen, dass Pip auch anderes kann. "Für alles" ohne Geschichte wirkt beliebig.
- Kein Code, der nur für die AU funktioniert: Regeln und Abläufe sind Module; der Agent selbst ist allgemein.

Zielgruppe: alle, die nicht technisch sind oder keine Kraft für Bürokratie haben (Kranke, Ältere, Menschen mit Einschränkungen, Studenten, Zugezogene).

Auslöser (echte Geschichte, im Video erzählt): Die Folge-Krankschreibung von Marcos Mutter kam zwei Tage zu spät. Die Krankenkasse zahlte diese zwei Tage nicht, obwohl sie krank war.

## 1b. Entscheidungen vom 26.10.2026 (Abend, nach den ersten Windows-Tests)

- **Nur Englisch.** Die Jury bewertet auf Englisch. Pip zeigt und spricht nur Englisch (`Lang::En` ist Standard). Deutsche Briefe bleiben als Eingabe: Pip erklärt deutschen Papierkram auf Englisch („housing benefit (Wohngeld)“).
- **Einstieg universell und sofort klar.** Statt „Drag a letter onto me“ fragt Pip: „What can I help you with?“ mit drei Knöpfen: **👀 Look at my screen**, **📄 Read a letter for me**, **💬 Or just tell me what you need**. „Look at my screen“ funktioniert für alles (Uni, Formulare, Bewerbungen): Pip sagt einen Satz, was offen ist, und handelt sofort.
- **Autonom statt Nachfragen.** Ohne Karte: schauen, zeigen, scrollen, Seiten öffnen, Felder aus dem Gedächtnis füllen, Dateiauswahl öffnen, eigenen gespeicherten Brief anhängen, merken. **Genau eine Karte pro Aufgabe:** der Klick, der etwas vom Computer wegschickt („Ready to send to …?“). Häkchen „Don't ask again for <site>“ macht Pip auf dieser Seite ganz autonom. Keine zweite Bildschirm-Frage (der Knopfdruck ist die Zustimmung).
- **Proaktiv ist der Kern.** Nach einer erledigten Aufgabe wartet Pip 4 s (die Zeile „Done.“ bleibt lesbar) und bietet dann genau ein passendes Formular der Kasse an („Gesundkasse also offers ‚Return your questionnaire online‘, which fits their letter of 27 October. Shall we do it together?“ [Yes, let's do it] [Not now] [Why?]), gesprochen und als Blase. Das Angebot macht Rust ohne Modell (`rules::daily::offer_after_task`): alle offenen Fristen (nach Fälligkeit, beliebig weit weg) gegen die Überschriften der bekannten Service-Seite, Treffer = gemeinsames Wort ab 6 Buchstaben; Grund = Datum des Briefs zur Frist, sonst das Fristdatum. „Yes“ startet die Ausfüll-Aufgabe direkt wie beim Tagesbriefing (`Task::Text`), Senden nur mit Freigabe. Nur wenn nichts passt, schaut sich das Modell einmal um (`Task::Discover`, Überschrift muss wörtlich auf der Seite stehen, Brief/Frist im Gedächtnis; dabei kann Pip nichts klicken oder senden). Jede Überschrift wird höchstens einmal angeboten (Ereignisse `suggested` / `suggestion_accepted` / `suggestion_declined`, egal ob Briefing, Rust-Angebot oder Modell). Nach „Not now“ 7 Tage Ruhe; `PIP_DISCOVER=always` ignoriert die Ruhe (Video-Wiederholungen), `PIP_DISCOVER=0` schaltet alles ab.
- **Demo mit vollem Gedächtnis.** `PIP_DEMO_PROFILE=1` legt ein erfundenes Profil an (Erika Beispiel: Adresse, Kasse, Versichertennummer, Arbeitgeber, Briefe, Fristen, Ereignisse), damit Pip in der Demo schon viel weiß. Daten relativ zu heute (STORY_DAY 28.10. wird auf das Startdatum verschoben), damit die Demo an jedem Tag lebt.
- **Tagesbriefing beim Start.** Einmal pro Tag (`brief_shown_on` im Profil, nicht in „What Pip knows“) begrüßt Pip mit dem, was ansteht (`rules::daily::daily_brief`: Krankschreibung endet, nächste Frist), und bietet mit einem Klick das passende Service-Formular an („I can fill in … for you now.“ [Yes, do it now] [Later]). Angebot nur, wenn eine file://-Service-Seite bekannt ist; sonst nur [Later]. „Yes“ startet die Aufgabe direkt (`Task::Text`, nicht über die Wort-Erkennung) und zählt als Zusage für diese Überschrift. Wurde die Überschrift schon zugesagt oder abgelehnt, nennt das Briefing nur die Frist, ohne [Yes, do it now]. `PIP_BRIEF=1` zeigt es bei jedem Start (Video), `PIP_BRIEF=0` nie.
- **Zweite Demo-Seite: Uni-Rückmeldung** (`fixtures/demo-uni/index.html`, erfundene „Beispiel University“, wird wie demo-kasse ins Datenverzeichnis geschrieben). „Look at my screen“ füllt den Namen aus dem Gedächtnis, hakt die Gebühr ab, fragt genau einmal „What is your student number?“ und zeigt eine Karte „Ready to send to Beispiel University (re-registration)?“. Beim zweiten Mal keine Frage (Antwort ist ein Fakt mit Quelle `user`).
- **Nutzen sichtbar.** Jede erledigte Aufgabe, die getippt oder gesendet hat, endet mit „Done in 14 s. By hand this takes about 8 minutes.“ (feste Schätzung `learn::by_hand_secs`: Seite öffnen 120 s + 45 s je Feld + 120 s je Anhang + 30 s Senden), ab 10 min Gesamt „That's about N minutes saved since we met.“ (`seconds_saved_total` im Profil, nicht in „What Pip knows“, nie in der Adress-Prüfung auf persönliche Daten). Nennt die Aufgabe „deadline #N“ und wurde wirklich gesendet, hakt Pip die Frist ab („I've ticked off the deadline …“, Ereignis `deadline_done`). Endet der Lauf mit Text statt `done`, steht die Antwort nur einmal da, die Zeitzeile kommt als eigene Blase.
- **Tempo.** Jeder Agentenschritt ist eine frische Anfrage mit nur dem neuesten Bild (~3 s statt ~15 s). Pip wartet auf Dateifenster, statt doppelt zu klicken. Pip schließt nie Fenster des Nutzers (Klick und Tastenkürzel gesperrt).
- **Berater:** Im Optimier-Loop berät Fable 5.1.

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
trait UiTree  { fn element_at(&self, p: PhysPoint) -> Option<UiElement>; }
```
- Stand 26.10. (gebaut): `find(name)` ist kein Teil des Traits, sondern `pip_platform::windows::WinPlatform::find` (sucht im Vordergrundfenster). `element_at` gibt `None`, wenn Pips eigenes Fenster unter dem Punkt liegt.
- `Input::scroll(p, dy)`: `dy` in Mausrad-Rasten, positiv = nach unten.
- `type_text` nimmt keine Steuerzeichen (Zeilenumbruch, Tab): die würden wie Enter/Tab wirken und ein Formular an der Schranke vorbei abschicken. Enter und Tab gehen nur über `key`.
- Alle Koordinaten in **physischen Pixeln** (Windows-Skalierung beachten; Tauri setzt Per-Monitor-DPI).
- `Frame` = Bild + Monitor-Offset + Skalierung. Für die API verkleinern (lange Seite max. 1600 px), Koordinaten zurückrechnen.
- Die Figur selbst: vor dem Bildschirmfoto ihren Bereich merken und im Bild ausgrauen (nicht unsichtbar schalten, sonst fehlt sie im Demo-Video).

## 5. Meta-Modelle: wer macht was

| Aufgabe | Modell | Endpunkt |
|---|---|---|
| Planen, Bildschirm verstehen, Dokument lesen, Antworten schreiben | `muse-spark-1.3` | `POST /v1/responses` |
| Ziel auf dem Bildschirm finden (Text → Box), Blatt im Foto ausschneiden | `sam-3.1` | `POST /v1/responses` (stream) |
| Sprache → Text | `muse-voice-transcribe-1.0` | `POST /v1/asr/transcribe` |
| Figur/Grafiken gestalten (einmalig, offline) | `muse-image-1.0` | `POST /v1/images/generations` (immer `output_format:"png"`: Standard ist WebP 1600x1600, `size` wird ignoriert) |
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
- ~~`include_confidence`~~: gibt Fehler 400 (geprüft 26.10.). Nicht senden. Der Parser ignoriert Masken, nimmt Schlüssel in jeder Reihenfolge und rechnet Boxen um, wenn SAM eine andere Bildgröße meldet.
- Der Stream endet mit `response.completed`; fehlt es, gilt die Antwort als abgebrochen (`MetaError::StreamCut`, wird wiederholt).
- SAM findet Dinge nach Form: ein runder Knopf mit Text wird als "button" gefunden, eine flache Fläche ohne Text nicht, Textfelder ("text input field") meist gar nicht.
- Kein Treffer → keine Zeile. Dann: Muse Spark neu fragen (anderer Begriff) oder UI-Baum.
- Parser in Rust mit `regex`, Unit-Tests mit festen Beispielzeilen.

### 5.3 Zielwahl (Grounding)
**Test 26.10. (echter Aufruf):** SAM 3.1 findet Knöpfe auf Bildschirmfotos nach Form ("Create API key button" → richtiger Knopf + ein zweiter blauer Knopf, 1,7 s), liest aber keinen Text ("Billing" → kein Treffer). `include_confidence` gibt Fehler 400 (unbekannter Parameter). Darum: SAM liefert Kandidaten-Boxen, Muse Spark wählt per Nummer, UI-Baum bestätigt den Namen.

1. Muse Spark sagt: `point/click { target: "Absenden-Knopf unten rechts im Formular", sam_query: "submit button" }`.
2. SAM 3.1 gibt 0..n Boxen.
3. Bei mehreren: Muse Spark bekommt das Bild mit nummerierten Boxen und wählt eine Nummer.
4. Klickpunkt = Box-Mitte → physische Pixel.
5. Optional: UI-Baum am Punkt fragen (`element_at`) → Name prüfen ("Absenden"?). Wichtig für die Schranke.

**Gebaut (Stand 26.10., nach Live-Tests):** SAM 3.1 und ein Muse-Spark-Aufruf `locate {found, x, y}` laufen parallel. Liegt Sparks Punkt in einer SAM-Box, wird deren Mitte geklickt (kleinste Box gewinnt; beide Modelle sind sich einig). Sonst wählt Spark eine Nummer unter den nummerierten SAM-Boxen; passt keine (oder SAM fand nichts), gilt Sparks eigener Punkt. Grund: SAM fand Textfelder nicht, und bei einer einzigen Box "button" wäre für "Datei auswählen" der falsche Knopf (Absenden) geklickt worden. Für `type_text` sucht der Locator "das Eingabefeld, in das {what} kommt", nicht nur die Wörter für den Nutzer.

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
| `ask_user` | `question`, `options[]` (leer, `""` oder null = offene Frage mit Textfeld) | frei |
| `request_approval` | `summary`, `action` | – (Modell darf es selbst anfordern) |
| `done` | `summary` | frei |

Gebaut (26.10.): `click` und `key` haben zusätzlich `is_commit`, `commit_what`, `commit_to` (füllen die Karte); `type_text` hat `what` ("Ich trage {what} ein.").
Gebaut (26.10., Nacht): **Seiten-Elemente wie `lcu page`.** Jeder Schritt liest per UI Automation die benannten Bedienelemente der Seite im Vordergrund (`UiTree::outline`, max. 80, ein FindAll mit Cache, < 400 ms, sonst leer) und schickt sie neben dem Bild als Block „PAGE ELEMENTS“ (`#3 edit "Full name" = ""`, `(off-screen)`, Passwortfelder ohne Wert; max. 2500 Zeichen). `click` und `type_text` haben `element` (Nummer oder null): Pip handelt dann direkt am Element, ohne SAM/Spark-Suche; `type_text` klickt, Strg+A, tippt und liest den Wert zurück („the field now shows …“). Unsichtbare Elemente werden vorher hineingescrollt. Das Modell plant ein ganzes Formular in einem Zug; ändert sich die Seite nach einem Klick (Fenstertitel, Adresse, Dateidialog) oder nach `scroll`, bekommen die restlichen Aufrufe „not done: the page changed, look again“. Die Schranke prüft auch den Namen des Elements (Senden per Nummer = Karte); Häkchen per Nummer sind nie ein Senden. Trockenlauf: `pip-cli agent … --outline fixtures/screens/demo-kasse.outline.json`. `request_approval` und `draft_message` sind (noch) nicht gebaut; die Schranke fragt selbst. Alle Schemas sind strikt (`additionalProperties:false`, alle Felder Pflicht, nullable als `["string","null"]`); der String `"null"` gilt als leer.

### 6.3 Schranke (Freigabe)
- Stufen: **frei** (lesen, zeigen, merken), **handeln** (klicken, tippen; sichtbar, jederzeit stoppbar), **Commit** (Senden, Absenden, Abschicken, Hochladen eines privaten Dokuments, Bezahlen, Kaufen, Löschen, Kündigen, Submit, Send, Pay, Upload, Delete).
- Commit wird an DREI Stellen erkannt, eine reicht: Modell meldet es, Wortliste auf `description`/`sam_query`, Name aus dem UI-Baum am Klickpunkt.
- **Karten (Stand 26.10. abends): genau eine pro Aufgabe**, beim Klick, der etwas vom Computer wegschickt: „Ready to send to …?“ mit „Send“ / „Not yet“ und dem Häkchen „Don't ask again for <site>“ (<site> = Host, im Demo-Modus der Demo-Ordner unter fixtures, z. B. `…/fixtures/demo-uni`: Vertrauen gilt pro Ordner, nicht für alle Demo-Seiten; danach schickt Pip auf dieser Seite ohne Karte ab und sagt es: „Sent. You told me …“). Löschen, Bezahlen, Bestellen und Kündigen fragen immer und nie mit Häkchen. Der Knopfdruck ist die Zustimmung, keine zweite Frage.
- **Keine Karte für:** Seite öffnen (`open_url`; persönliche Daten in der Adresse werden abgelehnt), merken (`remember_fact`; Adressen, Links und Änderungen an geschützten Schlüsseln aus fremder Quelle warten auf das nächste Senden und stehen auf dessen Karte: „I'll remember: …“), Dateiauswahl öffnen und den eigenen gespeicherten Brief anhängen (nur dieser Pfad darf in den Dialog).
- Karte zeigt: was, an wen (Name + echter Host), Anlagen, was ausgefüllt wurde, Bildausschnitt des Knopfs. "Ändern" gibt es nicht: der Nutzer sagt es Pip einfach neu. Ohne Antwort in 5 min = Nein. Die Wartezeit auf die Karte zählt nicht gegen die 3 Minuten des Laufs.
- Ob die Karte "anhängen" oder "abschicken" sagt, entscheidet nur das Ziel (Beschreibung, SAM-Begriff, UI-Name), nie der Text "Was". Ein Sende-Wort im Ziel gewinnt (Live-Fehler 26.10.: "Absenden" mit "Was: … (Datei au.pdf)" zeigte "Darf ich das anhängen?").
- Tasten: Enter, Return und Strg+Enter brauchen immer eine Freigabe (Enter im Formular schickt es ab). Ausnahme: ein Enter direkt nach einem freigegebenen Anhängen bestätigt den Datei-Dialog. Leertaste auf einem Sende-Knopf und Umschalt+Entf brauchen eine Freigabe.
- `type_text` klickt nie auf einen Knopf, der nach Senden oder Hochladen aussieht; dafür gibt es `click` mit `is_commit`.
- DEMO_MODE: Commit nur auf einer erlaubten Seite (localhost oder `file:///…/fixtures/…`), die `open_url` geöffnet hat (oder die der CLI-Trockenlauf als offen angibt).
- Jede Commit-Entscheidung → `actions_log`.

### 6.4 Prompt (Kern)
- Rolle: Helfer für Papierkram. Kurze, einfache Sätze. Sprache des Nutzers.
- Regeln: keine Medizin-Bewertung, keine Rechtsberatung (Regel + Quelle + "bitte prüfen"), nie ohne Freigabe senden, nie Passwörter tippen, bei Login/Captcha/2FA an den Nutzer übergeben (`ask_user`).
- Kontext pro Aufruf: Profil (Name, Kasse, Arbeitgeber), offene Fristen, letzte 5 Ereignisse, Treffer aus `search_memory`.

## 7. Dokumente

### 7.1 Ablauf
Foto/PDF → (Foto: SAM `"sheet of paper"`, zuschneiden, gerade ziehen nur per Box) → Muse Spark (PDF direkt) → JSON → prüfen → speichern → Regeln anwenden.
- PDF geht direkt als `{"type":"input_file","filename","file_data":"data:application/pdf;base64,…"}` an Muse Spark (geprüft 26.10.: HTTP 200, Daten richtig gelesen).
- Fotos werden vorher nach EXIF aufgerichtet. Leere Felder und der String `"null"` (kommt live vor) werden zu `null`.

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
| `profile` | key, value, updated_at, source, confidence, last_used_at, uses (Name, Kasse, Arbeitgeber, Bundesland, Sprache) |
| `playbooks` | site, task, url, fields_json (`[[Feldname, Profil-Schlüssel]]`, nie Werte), attach_label, send_label, steps, seconds, runs, failed_since_ok, updated_at |
| `documents` | id, path (lokal), sha256, doc_type, extract_json, created_at |
| `documents_fts` | FTS5 über Text + Zusammenfassung |
| `deadlines` | id, title, due, rule_id, doc_id, status (offen/erledigt/verworfen), remind_at JSON |
| `reminders` | id, deadline_id, at, fired_at, answer |
| `events` | id, at, kind, text (Verlauf: "AU an Kasse geschickt") |
| `actions_log` | id, at, tool, args_redacted, risk, approved, result |

Ort: `%APPDATA%/papierkram-copilot/`. Dokumente bleiben lokal; an die API geht nur das, was ein Aufruf braucht.
- Zeiten in der Datenbank: RFC 3339 in UTC (Reihenfolge stimmt auch bei der Zeitumstellung). Fälligkeiten: `YYYY-MM-DD`.
- Profil-Schlüssel, die der Agent nutzt: `name`, `krankenkasse`, `bundesland`, `lang` ("de"/"en"), im DEMO_MODE `krankenkasse_kontaktformular` (file-URL des Demo-Formulars).

### 8.1 Lernendes Gedächtnis (gebaut 26.10.)
Pip startet mit wenig und wird mit jeder Aufgabe schneller. Alles deterministisch im Rust-Code, kein extra Modellaufruf (`memory/learn.rs`, `memory/recall.rs`, Migration 2 in `schema_2.sql`).
- **Quellen:** `user` (eigene Worte, Antwort auf `ask_user`, Einrichtung) > `typed` (auf der Senden-Karte bestätigt) > `letter:<id>` > `model`. Eine niedrigere Quelle ersetzt nie eine höhere (`remember_fact` antwortet dann „not changed …“).
- **Nie gespeichert:** Schlüssel mit password/passwort/pin/tan/code/diagnos/icd/befund → „not saved: Pip does not keep passwords or health details“.
- **Aus Briefen** (`learn_from_letter`, nach dem Einlesen): `full_name` (Person), `krankenkasse` (Kassen-Erkennung), `versichertennummer` (Buchstabe + 9 Ziffern, nur AU/Kassenbrief). Füllt nur Lücken oder ersetzt Vermutungen.
- **Nach einem Lauf** (`learn_from_run`): Ist er fertig und wurde getippt oder gesendet, entsteht/aktualisiert sich ein Playbook pro (Seite, Aufgabenart): Adresse ohne Query, welches Feld welchen Profil-Schlüssel bekam (Wert = Profilwert), Anhängen- und Senden-Knopf (nur ein wirklich ausgeführter Senden-Klick), Schritte, Sekunden. Fehlgeschlagen → `failed_since_ok + 1`; ab 2 wird es gelöscht. Abbruch lehrt nichts. Benutzte Fakten: `uses + 1`.
- **Erinnern** (`recall`, erste Anfrage eines Laufs, Budget 4500 Zeichen): Profil (geschützte zuerst, dann nach Nutzung, max. 30, mit Quelle) → 5 nächste offene Fristen mit Id → „LAST TIME ON THIS SITE (host, N runs, last DATUM, S steps, T s): open URL; fields: "Name" <- full_name …; send button "…". Follow this unless the page looks different.“ (gleiche Seite; sonst dieselbe Aufgabe woanders bei send_au/send_letter; sonst nur die Liste bekannter Seiten) → Briefe (neuester je Art + 2 FTS-Treffer) → Muster („3 sick notes so far, about 14 days each …“) → 5 Ereignisse. Seite: Formular der Kasse bei Krankschreibung, sonst die Seite im Vordergrund.
- **Sichtbar:** Neues Playbook → Erledigt-Satz endet mit „I also learned how this form works. Next time will be faster.“ Bekannte Seite (Playbook für Seite + Aufgabenart): Statuszeile am Anfang „I know this page: last time took 41 s.“, am Ende „2nd time on this site: 14 s instead of 41 s last time.“ (nur wenn wirklich schneller und der Lauf nicht pausiert/fortgesetzt war). `PIP_DISCOVER=always` ignoriert die 7-Tage-Sperre nach „Not now“ (Video-Wiederholungen). Tray und Frage-Kasten: **„What Pip knows“** mit „About you“ (mit Quelle: „from your letter of …“, „you told me“), „Your letters“, „Places I know“, „Where I may send without asking“; jede Zeile hat **Forget** (`UserCommand::Forget`).
- **Klein halten** (`prune` beim Öffnen): Playbooks mit 2 Fehlern oder > 180 Tage alt weg; max. 50 Playbooks, 500 Ereignisse, 5000 Aktionszeilen.
- **Messen:** `pip-cli --db X agent … --approve all` zweimal; jede Runde druckt `Measure: steps=… seconds=…` (eval/README.md).

## 9. Regel-Motor
- Läuft jede Minute (`tokio::time::interval`). Prüft `deadlines` und Regel-Bedingungen. Feuert Erinnerungen, nie doppelt (`fired_at`).
- Regeln als Rust-Code mit Quelle und Testfällen, nicht vom Modell erfunden.

| Regel | Bedingung | Frist | Quelle (vor Demo prüfen) |
|---|---|---|---|
| `au_lueckenlos` | AU mit `until = E`, keine Folge-AU ab E+1 | Folge-AU spätestens am nächsten Werktag nach E (Samstag zählt nicht als Werktag) | § 46 Satz 2 SGB V (Krankengeld) |
| `widerspruch_bescheid` | `doc_type = bescheid` mit Datum D | 1 Monat ab Bekanntgabe; Bekanntgabe = D + 4 Tage (Post) | § 84 SGG / § 70 VwGO, § 37 Abs. 2 SGB X |
| `frist_aus_brief` | `deadlines[]` aus dem Dokument | wie im Brief | Brief selbst |

- Erinnerungen: 3 Tage vorher, 1 Tag vorher (18:00), am Tag (09:00). Feiertage: NRW 2026/2027 fest im Code.
- Ausnahme `au_lueckenlos`: am Tag vor dem Ende der Krankschreibung um 18:00 und am Fälligkeitstag um 09:00.
- Bekanntgabe (Widerspruch): der vierte Tag nach dem Datum wird **nicht** verschoben, auch nicht am Wochenende (BSG, 6.5.2010, B 14 AS 12/09 R). Nur das Fristende rückt auf den nächsten Werktag (§ 64 Abs. 3 SGG).
- Nach einer langen Pause feuert pro Frist nur die letzte fällige Erinnerung; die älteren werden als "skipped" markiert.
- Jede Meldung endet mit: "Das ist ein Hinweis, keine Rechtsberatung."

## 10. Pip – Figur und Gefühl
- Name: **Pip**. Weicher blauer Tropfen (Meta-Blau #0866FF → Violett), kleines gefaltetes Papier als Haarbüschel. Vorlage: `design/maskottchen/b_tropfen_1.png`, gemacht mit Muse Image.
- Marke: passt zur Farbwelt von Meta, nutzt aber **kein Meta-Logo und keine Meta-Figur** (fremde Assets = Disqualifikation). Im Video: "Built with Meta Muse + SAM".
- Platz: ruht klein (96 px, gebaut 26.10.; 72 px war zu klein zum Anklicken) unten rechts. Wenn Pip zeigt oder handelt, fliegt er in einer weichen Kurve zum Ziel und zeigt darauf; danach zurück.
- Posen (Muse Image, aus der Vorlage): ruhig, hört zu, denkt, zeigt, freut sich, fragt (Freigabe), schläft (Pause). Freistellen: weißer Hintergrund → transparent (SAM 3.1 Maske oder Farbschlüssel).
- Bewegung: leichtes Atmen im Ruhezustand, Blinzeln, kleine Hüpfer bei Meldungen. Keine hektischen Effekte.
- Sprechblase: große Schrift (mind. 18 px), max. 2 Sätze, immer mit klaren Knöpfen.
- Stimme: ruhig, langsam, Deutsch oder Englisch.
- Erster Start: Pip fliegt ins Bild, stellt sich vor, fragt nur den Namen, zeigt einmal "Zieh einen Brief auf mich". Krankenkasse kommt aus dem ersten Brief, Bundesland erst, wenn eine Frist es braucht ("Wohnst du in Nordrhein-Westfalen?").

## 10b. Overlay (Tauri 2)
- Fenster: `transparent`, `decorations:false`, `alwaysOnTop`, `skipTaskbar`, ganzer Bildschirm; `set_ignore_cursor_events(true)`, aus nur über Figur/Karte.
- Zustände der Figur: ruhig, hört zu, denkt, zeigt (Pfeil zur Box), handelt, wartet auf Freigabe, Fehler.
- Tray-Menü: Pause, Fristen-Liste, Privat-Modus an/aus, Beenden.
- Hotkeys: `Ctrl+Shift+Space` halten = sprechen; `Esc` (global, während Agent läuft) = Stopp.
- Maus bewegen = Stopp: etwa 100 ms stetige Bewegung über mindestens 60 px, aus während Karte oder Frage offen ist und 2 s nach Start oder "Ja". `PIP_MOUSE_STOP=0` schaltet es ab.
- Schalter zum Testen: `PIP_MOCK=1` (gespielte Demo ohne Kern), `PIP_WINDOWED=1` (Eckfenster statt Overlay), `PIP_MUTE=1`, `PIP_LANG=en`, `PIP_SKIP_ONBOARDING=1`, `PIP_TICK_SECS`, `PIP_QUIT_AFTER_SECS`.
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
DEMO_MODE=1                                    # Standard AN (auch ohne Eintrag); DEMO_MODE=0 schaltet aus
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
- CLI `pip-cli` (Crate `crates/cli`), ohne Overlay: `read <datei>` (Brief lesen + Vergleich mit `*.expected.json`), `sam <bild> <begriff>` (Boxen + PNG mit Nummern), `ground <bild> <beschreibung>`, `rules --now <zeit>` (Fristen + fällige Erinnerungen, ohne API), `agent "<auftrag>"` (Trockenlauf auf Bildschirmfotos: plant, zeigt Karten, klickt nichts), `transcribe <wav>`, `eval` (10 Aufgaben → `eval/results.md`).

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
