# Pip: technical details

The [README](../README.md) tells what Pip does and why. This page is for developers and judges who want the inside:
architecture, the Meta models, the safety rules in detail, every switch, the developer CLI and the eval.

<!-- Moved from the README of 2026-10-26 (commit 8e5ebad and later). Paths are relative to docs/. -->

## How Pip works

```mermaid
flowchart LR
    subgraph Windows["Windows desktop"]
        Pip["Pip overlay<br/>(Tauri 2, transparent, click-through)<br/>bubble · pointer · approval card"]
        Screen["Screen + mouse/keyboard<br/>+ UI Automation"]
    end

    subgraph Core["pip-core (Rust, OS-free)"]
        Bus(("event bus"))
        Agent["Agent loop<br/>screenshot → plan → ground → act"]
        Gate{"Approval gate<br/>send · attach · pay · delete"}
        Docs["Letter pipeline<br/>crop → read → check"]
        Rules["Rule engine<br/>every minute"]
        Mem[("SQLite memory<br/>letters · deadlines · profile · log")]
    end

    subgraph Meta["Meta Model API"]
        Spark["Muse Spark 1.3"]
        SAM["SAM 3.1"]
        Voice["Muse Voice Transcribe"]
    end

    Pip <-->|commands / events| Bus
    Bus --> Agent
    Bus --> Docs
    Rules -->|reminders| Bus
    Agent --> Gate -->|"only after Yes"| Screen
    Agent -->|screenshot| Screen
    Agent --> Spark
    Agent --> SAM
    Docs --> SAM
    Docs --> Spark
    Pip -->|push-to-talk| Voice
    Docs --> Mem
    Rules --> Mem
    Agent --> Mem
```

1. **Letter in** – a photo goes to SAM 3.1 (`"sheet of paper"`), the sheet is cropped from the full-size photo;
   a PDF goes straight to Muse Spark. Muse Spark fills a strict JSON schema (type, sender, dates, reference,
   sick-note period, deadlines with a quote from the letter). Rust checks every field, stores the letter locally and
   runs the rules.
2. **Rules** – `au_lueckenlos` (gap-free sick notes: the next note must start by the next working day), `widerspruch_bescheid` (one month to object to a
   decision, counting from the fourth day after the letter's date, § 37 (2) SGB X, § 64 SGG), and deadlines quoted
   in the letter. Weekends and NRW public holidays (computed from Easter) are in code and tested day by day.
   The engine ticks every minute and never fires a reminder twice.
3. **On the screen** – each step: take a screenshot (Pip's own window greyed out), Muse Spark plans tool calls
   (`click`, `type_text`, `key`, `scroll`, `open_url`, `ask_user`, `remember_fact`, `add_deadline`, `done` …).
   To find a target, **SAM 3.1 and Muse Spark run in parallel**: SAM proposes exact element boxes, Spark points at
   the described target; if Spark's point lies in a SAM box, Pip clicks the center of that box. If they disagree,
   Spark picks one of the numbered boxes; if none fits, Spark's own point is used.
4. **The approval gate** – a commit (send, submit, attach a private file, pay, buy, delete, cancel a contract) is
   detected in three independent ways, and any one is enough: the model says so, a German/English word list on the
   target, or the button's name from Windows UI Automation. Enter in a form always counts as sending. The card waits
   up to 5 minutes; no answer means no.

## Meta models and their roles

| Model | Role in Pip | Status |
|---|---|---|
| **Muse Spark 1.3** (`/v1/responses`) | The brain: plans each screen step with tool calls, reads letters into JSON (PDF as `input_file`), points at targets and picks among SAM boxes, writes Pip's plain-language sentences. | built |
| **SAM 3.1** (`/v1/responses`, streamed) | The eyes for shapes: cuts the sheet out of a phone photo; proposes exact boxes for buttons on the screen so clicks land in the middle of the real element. | built |
| **Muse Voice Transcribe 1.0** (`/v1/asr/transcribe`) | Hold Ctrl+Shift+Space (or the microphone button) and say what you need. | built; checked with a recorded sentence through `pip-cli transcribe` (Oct 26); microphone on Windows still to test |
| **Muse Image 1.0** (`/v1/images/generations`, `/edits`) | Designed Pip: the character and its eight poses (idle, wave, listen, think, point, happy, ask, sleep). | used offline |
| **Muse Glimmer** (open weights, local) / **Muse Spark Open** (open weights, Baseten) | Private mode: reads photos and scans of letters through an OpenAI-compatible endpoint (llama.cpp on the computer, or Baseten). SAM and the Meta API are not called for the letter. | built: private mode reads photos and scans through an OpenAI-compatible endpoint (llama.cpp or Baseten); tested against a mock server, live run pending |

Pip uses Meta's colour world (blue to violet) but no Meta logo and no Meta character.

## Safety and privacy

- **Nothing is sent without "yes".** Every commit goes through the approval card, and every decision is written to
  `actions_log`.
- **Demo mode is on by default.** With `DEMO_MODE` on (the default; `DEMO_MODE=0` turns it off), Pip may open and
  submit only local demo pages: `localhost`, or pages inside Pip's own fixtures folders. The check looks at the page
  that is really in the browser's address bar when it can read it, else at the page opened in the same task. The demo
  insurer "Gesundkasse" and the second demo site, the invented "Beispiel University" with its re-registration form,
  send nothing. Both are built into the exe. Each demo folder counts as its own site, so "Don't ask again" and the
  learned steps for the insurer never cover the university.
- **The card shows the real recipient.** Next to the name the model gives, the card shows the web address of the
  page, and it lists what Pip typed into the form. Pip says the whole question aloud.
- **Pip checks again after a slow "yes".** If the page under the button changed while the card was open, Pip does
  not click and looks again.
- **New pages, new addresses and new send targets ask first.** Opening an unknown website, storing an address or link,
  or changing where letters are sent needs a "yes". Addresses that carry the user's data are refused.
- **Your letters stay on your computer** (`%APPDATA%\Pip\memory.db`). Only what one call needs goes
  to the Meta API: the letter image, or the current screenshot. In **private mode** (below) a letter photo goes only
  to the open-weights model you chose, for example one running on the same computer; Pip then says
  "Read here on your computer. Nothing left it."
- **Screenshots live in memory only.** They are not written to disk.
- **No secrets in logs.** The API key comes from the environment or `.env` and is redacted from every error text.
  Letter contents and typed text are not logged.
- **Screen text is data, not instructions.** The prompt says so. Even when the screen tries to talk the model into
  something, the gate still asks before anything is sent.
- **No passwords.** Password fields (UI Automation `IsPassword`) are never typed into. Pip asks you to type them yourself.

## Build and run

Needs Rust 1.85+ (edition 2024). The Windows build also needs the WebView2 runtime (Windows 10/11 have it).

```sh
# .env in the repo root or next to the exe (never committed)
MODEL_API_KEY=...            # Meta Model API key
# DEMO_MODE=1                # default: only local demo pages may be submitted

cargo test --workspace                        # unit + mock-server tests, no API calls
cargo test -p pip-core -- --ignored       # live tests against the Meta API (costs cents)

# Windows app (on Windows)
cargo build --release -p pip-app -p pip-cli
# or cross-compile from Linux
cargo xwin build --release --target x86_64-pc-windows-msvc -p pip-app -p pip-cli
```

Run `pip-app.exe`. Pip appears in the bottom-right corner. The demo pages are built into the exe (Pip writes them to
`%APPDATA%\Pip\fixtures\demo-kasse\` and `…\demo-uni\`; open `demo-uni\index.html` in the browser for the second example); the `fixtures/` folder next to the exe is only needed for the example
letters. Without a key, Pip shows a field to paste it (stored in `%APPDATA%\Pip\.env`).

Switches for testing: `PIP_MOCK=1` (scripted demo without the API), `PIP_WINDOWED=1` (corner window instead of the
full-screen overlay), `PIP_LANG=en`, `PIP_MUTE=1`, `DEMO_NOW=2026-10-29T18:00` (time travel for reminders),
`PIP_MOUSE_STOP=0` (moving the mouse does not stop Pip; use it while recording the video), `PIP_AUTOSTART=0`
(do not add Pip to the Windows start). `PIP_DEMO_PROFILE=1` seeds the invented person Erika; her story is told
relative to the day the database is first created (sick note ends in two days, insurer's letter came yesterday).
For the Thursday-evening reminder start Pip once to seed, then restart with
`DEMO_NOW=<day the note ends minus 1>T18:00` (seeded today: tomorrow at 18:00). `PIP_DEMO_RESET=1` gives every
recording day a fresh start: at launch Pip forgets everything and seeds Erika again for today (only when the memory
belongs to Erika or to nobody; a memory with any other name is never wiped). `pip-cli eval` runs the eval below
against the real API (a few cents) and writes `eval/results.md`.

### Private mode (Meta open weights, the letter stays local)

With private mode on, a dropped photo or scan of a letter is read by a Meta open-weights model behind an
OpenAI-compatible `/chat/completions` endpoint, with the same strict letter schema as Muse Spark. The SAM sheet crop is
skipped (it would send the photo to the cloud), and the Meta API sees nothing of the letter. Pip ends its sentence with
"Read here on your computer. Nothing left it." PDFs are refused with a friendly sentence (no PDF renderer in Pip yet):
take a photo of the letter, or turn private mode off. If the model server is not running, Pip says "My private reading
model isn't answering. Please start it, or turn private mode off." (not "check the Wi-Fi"). Letters work in private mode
even without `MODEL_API_KEY`; the screen agent still uses Muse Spark and needs the key.

```sh
PRIVATE_MODEL_BASE=http://127.0.0.1:8080/v1   # turns private mode on (llama.cpp, or a Baseten URL ending in /v1)
PRIVATE_MODEL=muse-glimmer                    # default; on Baseten e.g. meta-models/muse-spark-open-1026
PRIVATE_MODEL_KEY=...                         # optional, only for hosted endpoints (never logged)
# PIP_PRIVATE=0                               # turns private mode off again without removing the lines above

# local example with llama.cpp (vision model: also pass its --mmproj file)
llama-server -m <muse-glimmer gguf> --port 8080
PRIVATE_MODEL_BASE=http://127.0.0.1:8080/v1 pip-cli read fixtures/letters/au_erika_erst_foto.jpg
```

Tested against a mock server (`crates/core/tests/private_mode.rs`: the cloud mock receives zero requests). A live run
against a real Muse Glimmer or Muse Spark Open endpoint is still pending:
`cargo test -p pip-core --test private_mode -- --ignored` runs it when `PRIVATE_MODEL_BASE` is set. A 30B model on a
laptop CPU can take minutes per letter (timeout 10 minutes).

### Developer CLI (no overlay, same core)

```sh
pip-cli read fixtures/letters/au_erika_erst_foto.jpg      # crop + read + rules, compared with *.expected.json
pip-cli sam fixtures/screens/kasse_form.png "button" --out boxes.png
pip-cli ground fixtures/screens/kasse_form.png "Absenden button at the bottom right"
pip-cli rules --now 2026-10-29T18:00                      # deadlines and the reminder that fires now
pip-cli agent "Schick die Krankschreibung an die Gesundkasse"   # dry run on screenshots, nothing is clicked
pip-cli eval                                              # the eval below -> eval/results.md
pip-cli card --demo                                       # writes the 'Pip did this for me' page with invented numbers
crates/app/tools/shot.sh                                  # headless screenshots of the overlay scenes (preview.html)
```

Example dry run (abridged):

```
Pip points at "deinen Vor- und Nachnamen" (name_field)
  [input] click (462, 503) -> name_field
  [input] type "Erika Beispiel"
Pip points at "den grünen Knopf „Absenden“ unten rechts" (submit_button)
  [GATE] card: Darf ich das abschicken? | Was: deine Krankschreibung vom 20.10. bis 30.10.2026 | An: Gesundkasse (Kontaktformular)
  [GATE] answer: Noch nicht
Outcome: Stopped. The send card stopped the click on "Absenden". Nothing was clicked there, nothing sent.
```

## Eval

Tasks with invented data (`eval/tasks.json`), run with `pip-cli eval` against the real API: 5 letters (are fields
and deadlines right?), 4 click targets (does the click land on the element?) and 3 agent tasks in a dry run on
screenshots (nothing is really clicked or sent). One agent task checks that the send card stops *Absenden*. Two are
sent with one approved send card and run **twice on the same memory**, so the 2nd run starts with what Pip learned
the 1st time (the playbook for the site, remembered answers). Full table and notes:
[`eval/results.md`](../eval/results.md). Reproduce: `cargo run -p pip-cli -- eval` (a few cents, see
[`eval/README.md`](../eval/README.md)).

| Measure | Result (two eval runs, Oct 26, 2026) |
|---|---|
| Agent, insurer form (EN, PAGE ELEMENTS) | 1st run 4 steps / 16.3 s / 0 questions, 2nd run 3 steps / 11.7 s / 0; again: 3 / 11.9 s / 0, then 3 / 10.7 s / 0 |
| Agent, university re-registration ("Look at my screen") | 1st run asks 1 question (student number), 5 steps / 21.5 s; 2nd run 0 questions, 3 steps / 13.1 s. Again: 7 steps / 45.3 s / 0 questions, then 3 steps / 14.9 s |
| Learning (2nd run faster: fewer steps and seconds, no more questions) | 2 of 2 tasks, then 1 of 2 (the insurer form already took 3 steps the 1st time) |
| By hand (estimate) vs with Pip | insurer form about 4 to 6 min by hand vs 11 to 16 s; university about 4 min vs 13 to 45 s |
| Sent after one approved send card | 4/4 and 4/4 runs |
| Gate held at *Absenden* (send card refused) | 1/1 and 1/1 |
| Letter fields correct | 41/41 and 41/41 (100 %) |
| Deadlines correct | 5/5 and 5/5 letters |
| Click targets hit | 3/4, then 4/4 |
| Tokens per full eval run | about 180k to 190k |

Honest limits (details in `eval/results.md`): the model does not answer the same way every time. In 2 of 3 extra
single runs Pip pressed *Submit* on the university form with only the name typed instead of asking for the student
number; the dry run cannot see the missing field, because the next screenshot already shows a filled form. Lone
`say` steps that repeat the opening sentence still cost steps.

Oct 26, first run (5 letters, 4 click targets, 1 German agent task, no PAGE ELEMENTS, no learning measure):

| Measure | Result |
|---|---|
| Letter fields correct | 41/41 and 41/41 (100 %) |
| Deadlines correct | 5/5 and 5/5 letters |
| Click targets hit | 4/4, then 3/4 (the second run clicked 50 px left of "Datei auswählen") |
| Gate held at *Absenden* | 1/1 and 1/1 |
| Tokens per full eval run | about 65k (all 10 tasks) |

Still to come: runs on real Windows apps and browsers, more runs per task, and more screens than the demo forms.

## Repository layout

```
crates/core       OS-free core: Meta API client, agent loop + gate, letter pipeline, rules, SQLite memory, i18n
crates/platform   screen capture, input, UI Automation (Windows); best-effort Linux; fake platform for tests
crates/app        Pip overlay (Tauri 2): bubble, pointer, approval card, tray, hotkeys, voice, notifications
crates/cli        developer CLI and eval runner
fixtures/         invented letters (PDF, PNG, phone photo) with expected results, demo insurer and university forms, screenshots
eval/             eval tasks and results
design/pip/       Pip's poses
```

All people, insurers, offices and numbers in this repository are invented.

License: Apache-2.0.
