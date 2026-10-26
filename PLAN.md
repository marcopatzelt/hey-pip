# Papierkram-Copilot – Meta Global AI Developer Hackathon

Hackathon: Meta × Cerebral Valley, Indie Developer track. Build 26.10. – **4.11.2026 21:00 ET (= 5.11. 02:00 deutsche Zeit)**. Judging 4.–8.11., Finals 9.11.
Infos: https://dev.meta.ai/events/global-hackathon · https://cerebralvalley.ai/e/meta-ai-developer-hackathon
Pflicht: Meta-Modelle im Kern (Muse Spark, SAM 3.1 über die Meta Model API). KI-Coding-Werkzeuge sind ok (Regeln prüfen, sobald veröffentlicht).

## Die Idee (so eingereicht)
Ein Copilot für Papierkram. Er liest Briefe von Ämtern, Versicherungen und Vermietern, erklärt sie in einfacher Sprache, findet die passenden Unterlagen in deinen Dateien und schreibt die Antwort. **Vor jedem Senden fragt er dich.**
Theme: "Build for Your World" (ein Problem, das Marco selbst kennt).

## Ablauf für den Nutzer (Demo in 2 Minuten)
1. Foto oder PDF eines Briefs hochladen (Handy-Kamera oder Scan).
2. SAM 3.1 schneidet das Blatt aus, Muse Spark liest es (Absender, Frist, was verlangt wird, Aktenzeichen).
3. Erklärung in einfacher Sprache: "Was will der Brief? Bis wann? Was passiert, wenn du nichts tust?"
4. Suche in deinen Unterlagen (Ordner mit PDFs/Fotos): passende Belege, z. B. Mietvertrag, Kontoauszug, Bescheid.
5. Antwortentwurf mit den Belegen als Anlage.
6. Freigabe-Karte: "Senden / Ändern / Nicht senden". Ohne Ja passiert nichts.
7. Frist landet im Kalender, Brief + Antwort im Verlauf (Gedächtnis).

## Technik (Vorschlag)
- Web-App (Next.js oder einfaches FastAPI + HTML), läuft lokal; Unterlagen bleiben auf dem Gerät.
- Meta Model API: Muse Spark (Vision + Text) für Lesen, Erklären, Antwort; SAM 3.1 für Dokument-Zuschnitt.
- Suche in Unterlagen: Text aus PDFs (pymupdf) + Embeddings oder einfache Volltextsuche (SQLite FTS5).
- Freigabe-Schranke wie bei lcu (nichts verlassen ohne Ja). Versand im Demo: E-Mail-Entwurf oder PDF, kein echter Versand.
- Datenschutz als Feature: lokale Unterlagen, keine Speicherung bei Dritten außer dem Modellaufruf.

## Plan (10 Tage)
| Tag | Datum | Ziel |
|---|---|---|
| 1 | Mo 26.10. | Regeln + API-Zugang (Muse Spark, SAM 3.1) prüfen, Repo, Gerüst, ein Brief end-to-end lesen |
| 2 | Di 27.10. | Erklärung in einfacher Sprache + Fristen-Erkennung (11:30 ElevenLabs-Call beachten) |
| 3 | Mi 28.10. | SAM-Zuschnitt für Handyfotos, 10 Beispielbriefe (erfunden, keine echten Daten) |
| 4 | Do 29.10. | Unterlagen-Suche (Ordner indexieren, passende Belege finden) |
| 5 | Fr 30.10. | Antwortentwurf mit Anlagen + Freigabe-Karte |
| 6 | Sa 31.10. | Verlauf/Gedächtnis, Frist in Kalender (.ics) |
| 7 | So 1.11. | UI glätten, Englisch + Deutsch; YC/speedrun nicht vergessen (YC bis 3.11. 05:00) |
| 8 | Mo 2.11. | Eval: 10 Briefe, Trefferquote Frist/Absender/Belege messen |
| 9 | Di 3.11. | Demo-Video (≤ 3 min), README, Screenshots |
| 10 | Mi 4.11. | Puffer, Abgabe vor 4.11. 21:00 ET |

## Regeln für uns
- Nur erfundene Beispielbriefe im Repo und im Video. Keine echten Bescheide, keine echten Namen.
- Das Wort "Jobcenter" nicht in Marketing-Texten (wie in der Bewerbung).
- Nichts wird ohne Freigabe gesendet, auch nicht im Demo.

## Offen
- [ ] Offizielle Regeln lesen (Abgabeformat, Video, Repo öffentlich?, alter Code erlaubt?)
- [ ] Meta Model API Key besorgen (Marco: Login nötig)
- [ ] Bestätigung der Teilnahme (Approval 26.10.) in der Mail prüfen
