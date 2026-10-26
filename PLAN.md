# Papierkram-Copilot – Meta Global AI Developer Hackathon

Hackathon: Meta × Cerebral Valley, Indie Developer track. Build 26.10. – **4.11.2026 21:00 ET (= 5.11. 03:00 deutsche Zeit)**. Judging 5.–8.11., Finals 9.11.
**Technische Spec: `SPEC.md`** (was wir bauen und wie).
Infos: https://dev.meta.ai/events/global-hackathon · https://cerebralvalley.ai/e/meta-ai-developer-hackathon
Pflicht: Meta-Modelle im Kern (Muse Spark, SAM 3.1 über die Meta Model API). KI-Coding-Werkzeuge sind ok (keine Regel verbietet sie oder eine Sprache).

## Die Idee (so eingereicht)
Ein Copilot für Papierkram. Er liest Briefe von Ämtern, Versicherungen und Vermietern, erklärt sie in einfacher Sprache, findet die passenden Unterlagen in deinen Dateien und schreibt die Antwort. **Vor jedem Senden fragt er dich.**
Theme: "Build for Your World" (ein Problem, das Marco selbst kennt).

### Erweiterung (Marco, 20.10.2026): Orga-Helfer, der mitdenkt
- Auslöser: Marcos Mutter. Die Folge-Krankschreibung kam zwei Tage zu spät (rückwirkend). Die Krankenkasse zahlte die zwei Tage nicht. Grund: Die Folge-AU muss spätestens am nächsten Werktag nach dem Ende der alten AU ausgestellt sein (§ 46 SGB V, vor der Demo prüfen).
- Der Helfer liest Bescheinigungen und Briefe, merkt sich Zeiträume und Fristen und warnt rechtzeitig: "Deine Krankschreibung endet am Freitag. Geh spätestens am Montag zum Arzt, sonst zahlt die Kasse die Lücke nicht."
- Er macht nicht nur Vorschläge, sondern handelt selbst (Entwurf schreiben, Frist eintragen, Weitergabe vorbereiten). Senden nur nach Freigabe.
- Gedächtnis über den Nutzer (Verlauf, Fristen, Unterlagen). Kleiner Helfer auf dem Bildschirm, für alle Lebenslagen: Anmeldungen, Abgaben, Studenten-Fristen usw.
- Zielgruppe: Menschen, die eingeschränkt sind oder die Regeln nicht kennen.
- Achtung Regel: keine medizinische Beratung. Der Helfer bewertet nie Krankheit oder Behandlung, nur Papier, Fristen und Abläufe (die AU als Dokument).

## Ablauf für den Nutzer
Siehe `SPEC.md` Abschnitt 3 (Dokument erfassen, von selbst melden, am Bildschirm handeln) und Abschnitt 15 (Demo-Video).

## Form: Desktop-Helfer auf dem Bildschirm (Entscheidung 26.10.)
Keine Web-App. Ein eigenes Produkt: Ein kleiner Helfer sitzt auf dem Bildschirm, sieht ihn, spricht, zeigt auf Dinge, merkt sich Fristen, warnt von selbst und bedient den Computer selbst (Computer Use), mit Freigabe vor Senden/Bezahlen/Löschen.
- Keine anderen Produkte als Vorbild nennen, nirgends (Repo, Video, Texte). Nur eigener Code aus dem Hackathon.
- Ziel: läuft auf Windows (Demo, getestet). Linux in der Theorie (gleicher Code, Plattform-Teil vorhanden, unter Wayland eingeschränkt). Kein macOS.

## Technik: komplett Rust
Die Meta Model API ist HTTPS (REST): `https://api.meta.ai/v1`, `Authorization: Bearer $MODEL_API_KEY`. Kein Rust-SDK → `reqwest` + `serde` selbst.

**Wo welches Meta-Modell arbeitet (wichtig für "Use of Meta AI tooling")**
- `muse-spark-1.3`: das Gehirn. Bekommt Bildschirmfoto + Sprache + Gedächtnis, plant, ruft Werkzeuge (Tool Calls) auf. Liest Briefe, PDFs und Fotos direkt.
- `sam-3.1`: findet das Ziel genau. Muse Spark sagt "Knopf Senden", SAM 3.1 gibt Box/Maske auf dem Bildschirmfoto → Mittelpunkt → Klick bzw. Zeiger der Figur. Auch: Brief aus Handyfoto ausschneiden.
- `muse-voice-transcribe-1.0`: Sprache rein (Taste halten, sprechen).
- Optional `muse-image-1.0` (Figur/Grafiken), Muse Spark Open über Baseten (Bonus Open-Weight).
- Sprache raus: Sprachausgabe des Systems (Meta hat kein TTS in der API).

**Rust-Bausteine (Crates)**
| Aufgabe | Crate | Windows | Linux/Wayland |
|---|---|---|---|
| Overlay-Fenster (durchsichtig, immer oben, Klicks gehen durch) | `tauri` 2 | ja | eingeschränkt |
| Bildschirmfoto | `xcap` | ja | über Portal |
| Maus + Tastatur steuern | `enigo` | ja | über libei/Portal, wackelig |
| Knöpfe/Felder nach Namen lesen | `uiautomation` / `atspi` | ja | ja |
| Mikrofon | `cpal` | ja | ja |
| Sprachausgabe | `tts` | ja | ja |
| Globale Taste | `tauri-plugin-global-shortcut` | ja | nein unter Wayland |
| Gedächtnis, Fristen | `rusqlite` (SQLite) | ja | ja |
| Erinnerungen | `tokio` + `tauri-plugin-notification` | ja | ja |

**Aufbau**
- `core` (reines Rust, ohne Betriebssystem): Agenten-Schleife, Meta-API-Client, Gedächtnis, Fristen-Regeln, Freigabe-Schranke.
- `platform`: ein Trait (Schnittstelle) `Screen` / `Input` / `UiTree`, Umsetzung für Windows; Linux als zweite Umsetzung, soweit Zeit bleibt.
- `app`: Tauri-Overlay (Figur, Sprechblase, Freigabe-Karte, Fristen-Liste).
- Agenten-Schleife: Bildschirmfoto → Muse Spark (Tools: `point`, `click`, `type`, `remember`, `set_deadline`, `draft`, `ask_user`) → SAM 3.1 für Koordinaten → Aktion → neues Bildschirmfoto prüfen.
- Freigabe: Aktionen mit Senden/Bezahlen/Löschen zeigen eine Karte. Ohne Ja nichts.

**Risiken**
- Marcos Laptop läuft mit GNOME 50 auf Wayland (kein X11 mehr). Wayland verbietet Programmen: Fenster immer oben, fremde Klicks, freie Bildschirmfotos. → Entwickeln auf Linux geht (Kern, Gedächtnis, API), aber das Overlay und Computer Use auf Windows testen.
- Windows testen: schnell in einer VM (QEMU/KVM, auf dem Laptop installiert), echt und für das Demo-Video auf Windows 11 von der T7 (`windows-starten`). Bauen: GitHub Actions (`windows-latest`) oder `cargo xwin` auf Linux. Details: SPEC.md 13.2.
- 10 Tage sind knapp. Erst der Kern-Ablauf (Demo-Geschichte), dann Plattformen.

## Demo-Geschichte (3 Minuten)
1. Nutzerin fotografiert eine Krankschreibung (AU) und den Brief der Krankenkasse (beides erfunden). Helfer liest, merkt sich: AU bis Freitag.
2. Donnerstag: Figur meldet sich von selbst: "Deine AU endet morgen. Für lückenlose Zahlung brauchst du die Folge-AU spätestens am Montag." Frist im Kalender.
3. Nutzerin fragt per Stimme: "Hilf mir, das der Kasse zu schicken." Helfer öffnet das Online-Formular / Mail, zeigt mit SAM auf die Felder, füllt sie aus.
4. Freigabe-Karte: "Senden?" → Ja. Erst dann klickt er.
5. Zweites kurzes Beispiel: Student mit Rückmeldefrist (zeigt "alle Lebenslagen").

## Gewinn-Strategie (Stand 26.10.)
Ziel: Hauptpreis im Indie-Track (1. Platz 150k $) plus Bonus-Preise. Offen: Darf ein Projekt Hauptpreis UND Bonus gewinnen? → @CV fragen.

**Hauptpreis: die vier Punkte (gleich gewichtet)**
| Punkt | Was wir tun |
|---|---|
| Impact | Echte Geschichte (Krankschreibung, Kasse zahlt 2 Tage nicht). Zielgruppe: Menschen mit Einschränkungen, Ältere, alle ohne Behörden-Wissen. Zahl im Video: wie viele Menschen das trifft (seriöse Quelle suchen). |
| Use of Meta AI tooling | 5 Meta-Modelle mit klarer Rolle: Muse Spark (Gehirn), SAM 3.1 (zeigen/klicken), Muse Voice Transcribe (Stimme), Muse Glimmer lokal (private Dokumente), Muse Image (Figur). Schaubild im README und im Video. |
| Technical depth | Komplett Rust, eigener Agent mit Tool Calls, SAM-Grounding (Text → Box → Klick), lokales Modell, Freigabe-Schranke, Eval mit Zahlen (Trefferquote). |
| Demo quality | 3 Minuten, die ersten 10 Sekunden zeigen das Problem. Echte Bedienung auf Windows, nicht gestellt. Englisch mit Untertiteln. |

**Bonus-Preise: was passt**
| Preis | Geld | Chance | Was wir dafür tun |
|---|---|---|---|
| Best Agentic Use-Case | 5k $ | hoch | Kern des Produkts: Helfer meldet sich von selbst, plant, handelt am Bildschirm, fragt vor dem Senden. Im Video die Schleife sichtbar machen (denkt → zeigt → handelt → prüft). |
| Best Build on Meta Open Weight Models | 10k $ | mittel-hoch | "Privat-Modus": Muse Glimmer (30B, Apache 2.0, ~20 GB bei 4 Bit, kann Bilder lesen) läuft lokal per llama.cpp. Krankschreibungen und Briefe verlassen den Laptop nie. Fallback: Muse Spark Open über Baseten (`meta-models/muse-spark-open-1026`). Baseten-Credits beantragen. |
| Most Viral Project | 20k $ | mittel | Demo auf X posten, Link einreichen; Klicks zählen bis 8.11. Darum früh posten (spätestens 3.11.), nicht erst am Abgabetag. Vorher "Build in Public": kurze Clips während der Bauzeit. Hook: "My mom lost 2 days of sick pay because of one missing paper." |
| Community Vote | 10k $ | mittel | Abstimmung ab 9.11., eine Stimme pro Konto. Eigenes Netzwerk vorher bitten (Freunde, Familie, LinkedIn, Agentic-Content-Kanäle). |
| Best Creative Build | 5k $ | niedrig | Nebenbei: eigene Figur mit Charakter (mit Muse Image gestaltet). Kein Extra-Aufwand. |

Möglich: bis 50k $ Bonus. Realistisch: Agentic + Open Weight (15k $). Viral + Community hängen an der Reichweite.

**Risiken**
- Muse Glimmer lokal ist langsam (CPU ~1,6 Token/s; mit der Radeon-GPU über Vulkan schneller, noch messen). Darum lokal nur das Lesen der Dokumente; Bildschirm-Steuerung mit Muse Spark in der Cloud.
- Glimmer braucht ~20 GB RAM. Laptop hat 32 GB. Unter Windows auf der T7 prüfen.
- Ein Thema für alle Preise: Video und X-Post erzählen dieselbe Geschichte.

## Plan (10 Tage)
| Tag | Datum | Ziel |
|---|---|---|
| 1 | Mo 26.10. | Regeln, API-Zugang, Baseten-Credits beantragen, Rust-Workspace (`core`/`platform`/`app`), ein Aufruf an Muse Spark + SAM 3.1 aus Rust; **testen, ob SAM Knöpfe auf Bildschirmfotos findet** (entscheidet Grounding-Weg) |
| 2 | Di 27.10. | Overlay-Figur (Tauri, durchsichtig, immer oben) auf Windows; Bildschirmfoto + "zeig auf X" mit SAM 3.1 |
| 3 | Mi 28.10. | Agenten-Schleife mit Tool Calls (point/click/type), Freigabe-Karte |
| 4 | Do 29.10. | Stimme rein (Muse Voice Transcribe), Sprache raus (`tts`), globale Taste |
| 5 | Fr 30.10. | Dokumente lesen (Foto/PDF), Gedächtnis + Fristen (SQLite), Erinnerungen von selbst; Muse Glimmer lokal starten und messen |
| 6 | Sa 31.10. | Privat-Modus (Glimmer liest Dokumente lokal), Fristen-Regeln (AU lückenlos usw.), erfundene Beispielbriefe, erster X-Clip |
| 7 | So 1.11. | Stabil machen auf Windows, UI-Baum statt nur Pixel, Linux-Build prüfen; YC nicht vergessen (bis 3.11. 05:00) |
| 8 | Mo 2.11. | Eval: 10 Aufgaben, Trefferquote Klick/Frist messen |
| 9 | Di 3.11. | Demo-Video (≤ 3 min) auf Windows, README mit Modell-Schaubild, Post auf X (früh, für Most Viral) |
| 10 | Mi 4.11. | Puffer, Abgabe vor 4.11. 21:00 ET (X-Link mit einreichen) |
| – | 5.–9.11. | X-Post pflegen (zählt bis 8.11.), ab 9.11. um Stimmen für Community Vote bitten |

## Regeln für uns
- Nur erfundene Beispielbriefe im Repo und im Video. Keine echten Bescheide, keine echten Namen.
- Das Wort "Jobcenter" nicht in Marketing-Texten (wie in der Bewerbung).
- Nichts wird ohne Freigabe gesendet, auch nicht im Demo.

## Offizielle Regeln (Stand 26.10.2026)
Quellen: FAQ von Cerebral Valley im Discord (26.10.), https://dev.meta.ai/events/global-hackathon, https://cerebralvalley.ai/e/meta-ai-developer-hackathon. Die vollständigen "Official Rules" sind öffentlich noch nicht zu finden; sie gehen vor, sobald sie da sind.

**Thema und Modelle**
- Pflicht: Meta-Modelle sinnvoll nutzen (Meta Model API und/oder Meta Open-Weight-Modelle). Sonst nicht zur Bewertung zugelassen.
- Eines der Themen: "Build for Your World" (Problem, das man selbst kennt) oder "Push the Frontier".
- Modelle: Muse Spark, Muse Wonder, Muse Image, Muse Voice Transcribe, SAM 3.1 (API); Muse Glimmer, Muse Spark Open (Open-Weight). Muse Code hilft beim Programmieren.
- Muse Code im "Contributor mode" (Standard bei Abos) gibt Prompts und Code an Meta zum Training. Für private Sachen "Standard mode" nehmen.

**Teilnahme**
- Ab 18 Jahren. Solo erlaubt, Teams bis 4. Jede Person muss einzeln angenommen sein.
- Jedes Projekt in genau einem Track: Indie Developer oder Startups.
- Ausgeschlossen: Sanktionsländer/-regionen, Mitarbeiter von Meta und Cerebral Valley, Regierungsnahe, deren Familie.

**Code und Projekt**
- Repo muss öffentlich sein.
- Bestehendes Projekt erlaubt, aber Abgabe und Demo zeigen nur, was im Hackathon gebaut wurde. Unklare Eigenleistung → Disqualifikation. (Unser Repo startet am 26.10. → passt.)
- Arbeit muss eigen sein und uns gehören. Open Source erlaubt, Lizenzen einhalten. Kein fremder Code, keine Daten oder Bilder ohne Erlaubnis.
- Keine Regel zur Programmiersprache und kein Verbot von KI-Coding-Werkzeugen.
- Nichts im Hackathon ist vertraulich. Keine echten privaten Daten zeigen.
- Lizenz an Meta/CV: Wir behalten das Eigentum, geben aber eine dauerhafte, kostenlose Nutzungslizenz (auch für Werbung).

**Verbotene Projekt-Arten**
- KI-Medizinberatung, Berater für psychische Gesundheit, Hauterkennung.
- Einfache RAG-Apps, einfache Bild-Analyse, allgemeine Lern-Chatbots.
- Bewerber-Screening, Ernährungs-Coaches, Persönlichkeits-Analyse.
- Sport-Analyse oder -Coaching.
- Alles Politische oder mit Krieg.
- Projekte, die gegen Gesetze, Ethik oder Plattform-Regeln verstoßen, werden disqualifiziert. Bei Zweifeln an der Idee: @CV im Discord fragen.
- Für uns heißt das: kein "Chat mit deinen PDFs" (wäre einfache RAG). Der Kern muss der handelnde Agent sein (Fristen, Erinnern, Entwürfe, Freigabe). Krankschreibung nur als Papier/Frist, nie als Medizin.

**Abgabe**
- Ein Teammitglied reicht ein, über die Plattform. Frist 4.11. 21:00 ET (5.11. 03:00 bei uns). Zu spät = raus.
- Dabei: Demo-Video (3 Minuten), öffentlicher Repo-Link, Namen aller Mitwirkenden. Links vorher testen.
- Jury bewertet Video, Code und das laufende Produkt.

**Bewertung**
- Erst Pflicht-Prüfung (Thema + Meta-Modelle). Dann vier Punkte, laut offiziellem FAQ gleich gewichtet: Impact, Nutzung von Meta-KI, technische Tiefe und Ausführung, Demo-Qualität. (Eine Fremdquote nennt 30/25/20/25; das FAQ geht vor.)
- Indie-Track: Kreativität, technischer Ehrgeiz, ob es funktioniert.
- Top 5 pro Track ins Finale (9.11., live).

**Preise**
- Indie: 150k / 100k / 75k / 50k / 25k $. Startups: 250k / 150k / 75k / 50k / 25k $.
- Bonus: Most Viral 20k (Demo auf X posten, Link einreichen, zählt bis 8.11.), Best Open Weight 10k, Community Vote 10k (ab 9.11.), Best Agentic Use-Case 5k, Best Creative Build 5k.
- Für uns passend: "Best Agentic Use-Case" (der Helfer handelt selbst) und evtl. Open-Weight (Muse Spark Open über Baseten).

**Credits**
- 150 $ Meta-API-Credits pro Person, selbst beantragen bis 4.11. 21:00 ET (Formular bei @CV im Discord). Kommen in Schüben; Billing-Seite prüfen.
- Kreditkarte nötig. Verbrauch selbst beobachten: Kosten über die Credits werden berechnet, "Spend Limit" stoppt nicht sicher.
- Partner: Baseten 100 $ (bewerben), Modal 100 $ (erste 1000), Fireworks offen.

**Termine (ET)**
- Achtung Zeitumstellung: bis 31.10. sind es 5 Stunden Unterschied (Deutschland schon Winterzeit, USA noch Sommerzeit), ab 1.11. sind es 6 Stunden.
- 26.10. 12 Uhr ET Kickoff-Livestream (17 Uhr bei uns). Bauzeit 27.10.–4.11.
- 27.10. Agents 101, 28.10. Agents 102, 30.10. Fireworks, jeweils 12–13 Uhr ET (17–18 Uhr bei uns).
- Office Hours im Discord (#office-hours, Meta-Team): werktags 10–11 Uhr ET (bis 30.10. 15–16 Uhr, ab 2.11. 16–17 Uhr bei uns).

**Hilfe und Lernen**
- Discord: allgemeine Fragen mit @CV markieren; #office-hours (Meta-Team); Baseten-Kanal; #announcements (offizielle Neuigkeiten).
- Doku: Quickstart, Modell-Liste, API-Cookbook, Open-Weight-Cookbook (Links im Discord-FAQ; Doku unter https://dev.meta.ai/docs/).
- Judging 5.–8.11., Finale 9.11. 12 Uhr ET, Community-Vote-Gewinner 16.11.

## Offen
- [x] Offizielle Regeln lesen (Stand oben; vollständige Official Rules noch nachlesen, sobald öffentlich)
- [x] Sprache: komplett Rust, Desktop-Helfer (26.10.)
- [ ] API-Key für das Projekt erstellen (Marco, dev.meta.ai → API keys) und in `.env` als `MODEL_API_KEY` eintragen
- [x] Ausgaben-Warnung bei 150 € pro Monat gesetzt (26.10.; nur Mail, stoppt nicht)
- [ ] Windows-Test einrichten: VM mit QEMU oder direkt T7; GitHub-Repo + Actions für Windows-Build
- [x] API-Credits beantragt (26.10., Team-ID 1289158403342985, Karte hinterlegt). Billing prüfen: Credits kommen in 2 Werktagen unter "free credits"
- [ ] Baseten-Credits beantragen (100 $, für Muse Spark Open)
- [ ] @CV fragen: Kann ein Projekt Hauptpreis und Bonus-Preise zusammen gewinnen?
- [ ] X-Konto: Reichweite prüfen, erste Build-in-Public-Clips planen
- [ ] Meta Model API Key besorgen (Marco: Login nötig)
- [ ] Bestätigung der Teilnahme (Approval 26.10.) in der Mail prüfen
