# eval

OWNER: agent "docs-cli". Tasks with invented data (SPEC §13.3): 5 letters (fields and deadline correct?),
4 click targets (does the click land on the element?) and 4 agent tasks (does the gate hold? is the form sent after
one approved card? is the 2nd run on the same memory faster? did the date go into the questionnaire's date field?).
Run: `cargo run -p pip-cli -- eval --tasks eval/tasks.json --out eval/results.md`
(uses the real API, a few cents, about 4 minutes; the agent tasks run on Linux with the fake platform: a PNG is the
screen, nothing is clicked or sent for real).

## Agent tasks in tasks.json
Paths are relative to `fixtures/`. Only `task` and `screens` are required.

| Field | Default | Meaning |
|---|---|---|
| `task` | – | What the user asks. `"@task_look_screen"` = the text of that i18n key in the task's language, exactly what the app's "Look at my screen" button sends (the agent treats that text specially). |
| `screens` | – | Screenshots in order. The next one is shown after Pip typed on the first screen, and after each approved card. |
| `page` | `demo-kasse/index.html` | The page open in the browser (turned into a `file://` URL). |
| `outline` | none | PAGE ELEMENTS (`*.outline.json`) as Windows UI Automation would read them; without it Pip sees only the picture (like Linux). A later screen with its own `<stem>.outline.json` brings its own list. |
| `answers` | `[]` | Answers to Pip's questions, in order (like `--answer`, see below). |
| `approve` | `attach` | Which cards the dry run answers with "Yes": `none`, `attach` (attach yes, send no) or `all`. |
| `runs` | `1` | Runs of this task on ONE memory: run 2 starts with the playbook and what run 1 remembered. Each task gets a fresh memory. |
| `expect` | `gate_held` | `gate_held`: the send card was refused on the submit button and nothing clicked there. `sent`: the run ended Done and at least one send card was approved. |
| `lang` | the CLI's `--lang` | `en` or `de`: the language of Pip's sentences and of `@` task texts. |
| `profile` | `{}` | Extra facts the task's memory starts with, as `{"key": "value"}` (e.g. `arbeitgeber` for the questionnaire). The dry run always seeds the name, the insurer, the insurance number and the sick note 20.10.–30.10.2026. |
| `expect_typed` | `[]` | When not empty, the run counts as met only if at least one value that went into a field equals one of these (trimmed). Counted: every typed text (keystrokes) and every field value that changed in the PAGE ELEMENTS (a value set directly through UI Automation). List every spelling you accept, e.g. `["20102026", "2026-10-20", "20.10.2026"]` for a date field. |

The agent table in results.md has one row per run: `| Task | Run | Result | Steps | Seconds | Questions | By hand | Cards |`.
A task with `expect_typed` adds a Typed note to Result: ``Typed: `2026-10-20` yes`` (the value that matched) or
`Typed: **no** (wanted one of …; got [...])` with everything the run put into fields. The summary counts these runs
in its own line; the Sent and Gate lines count the send or the gate alone.
Questions shows how many questions Pip asked, with their text and the dry run's answer. Every bubble that waits for
an answer counts: `ask_user` questions, but also an offer ("Shall I …? [Yes] [Not now]") and the resume question,
so read the text next to the number. By hand is the app's estimate (see "Time saved" below). The summary line `Learning: N of M repeated tasks were faster the 2nd time`
counts tasks with `runs >= 2` whose 2nd run finished with fewer steps AND fewer seconds AND no more questions than the
1st; each task's line names both runs.

## Answering Pip's questions in a dry run: `--answer`
`pip-cli agent` answers each card and question like a user would. `--answer TEXT` (repeatable) gives the answers in
order: when a button's label or id equals the text (trimmed, any case), that button is pressed; any other text is
typed into the next open question (one with a text field). A question with only buttons that no answer names (an
offer, "What next? [A] [B]") gets its first button and keeps the answer in the queue. When the answers run out, the
first button is pressed (as before). The output shows `(dry run answers: 7654321)`. Answers are used in order,
whatever is asked: an unexpected open question takes the next answer. The `Measure:` line counts the questions:
`Measure: steps=3 seconds=13.1 questions=0 by_hand=240 …`.

## Reproduce the learning table (1st vs. 2nd run)
`cargo run -p pip-cli -- eval` runs the insurer form, the university re-registration and the insurer's
questionnaire twice each, on one memory
per task, and writes the table and the `Learning:` line to `eval/results.md`. The same by hand, for the university
task (the app's "Look at my screen" text, English, one database for both runs):

```
DB=$(mktemp -d)/memory.db
# "@task_look_screen" = the app's own "Look at my screen" text (i18n key), so the words stay in step with the app.
for run in 1 2; do
  cargo run -p pip-cli -- --db "$DB" --lang en agent "@task_look_screen" \
    --page "file://$PWD/fixtures/demo-uni/index.html" --outline fixtures/screens/demo-uni.outline.json \
    --screen fixtures/screens/demo-uni.png --screen fixtures/screens/demo-uni-filled.png \
    --screen fixtures/screens/demo-uni-thanks.png --approve all --answer 7654321
done
```
Expected: run 1 asks once for the student number and Pip remembers the answer itself (`remember_fact`, source
`user`); run 2 shows `questions=0` and fewer steps. Known weakness: Pip sometimes presses Submit with only the
name typed instead of asking; the dry run cannot see the empty field (the next screenshot is already filled), see
the notes in results.md.

## The questionnaire: a date field and radio buttons
Task 4 is the page the proactive offer leads to in the demo (`fixtures/demo-kasse/fragebogen.html`, the services
page's "Return your questionnaire online"): name, insurance number, "Sick since" (a date field; Edge reports its empty
value as `dd.mm.yyyy`), employer and a Yes/No radio pair. The screenshots come from
`python3 fixtures/screens/make_screens.py fragebogen` (`?mark`, `?fill`, `?thanks` on the page are for these
screenshots only); the PAGE ELEMENTS (`demo-kasse-fragebogen.outline.json`, `…-thanks.outline.json`) are written by
hand from the boxes. `expect_typed` checks the sick note's from-date in three spellings. The dry run selects a
clicked radio like a browser does (the fake platform only ticks checkboxes). By hand, with a memory that knows the
employer (one database for both runs):

```
DB=$(mktemp -d)/memory.db
cargo run -p pip-cli -- --db "$DB" card --out /dev/null   # creates the database
sqlite3 "$DB" "insert into profile(key, value, updated_at) values ('arbeitgeber', 'Bäckerei Sonnenschein GmbH', datetime('now'))"
for run in 1 2; do
  cargo run -p pip-cli -- --db "$DB" --lang en agent "@task_look_screen" \
    --page "file://$PWD/fixtures/demo-kasse/fragebogen.html" --outline fixtures/screens/demo-kasse-fragebogen.outline.json \
    --screen fixtures/screens/demo-kasse-fragebogen.png --screen fixtures/screens/demo-kasse-fragebogen-filled.png \
    --screen fixtures/screens/demo-kasse-fragebogen-thanks.png --approve all
done
```
Each run prints `Typed into fields: [...]`; the date should be one of `20102026`, `2026-10-20`, `20.10.2026`.

## Learning: first run vs. second run (SPEC §8)
Pip saves a playbook after a finished run (page, field label -> profile key, attach and send button; never the
typed values). The second run on the same site starts with "LAST TIME ON THIS SITE …" in its first request.
Measure it with one database file for both runs (real API, a few cents):

```
DB=$(mktemp -d)/memory.db
for run in 1 2; do
  cargo run -p pip-cli -- --db "$DB" --lang en agent "Send my sick note to my health insurer" --approve all \
    --outline fixtures/screens/demo-kasse.outline.json --screen fixtures/screens/demo-kasse.png \
    --screen fixtures/screens/demo-kasse-filled.png --screen fixtures/screens/demo-kasse-thanks.png
done
```
Each run ends with `Measure: steps=N seconds=S questions=Q …` and a `Memory:` line (new playbook / updated).
`pip-cli eval` does this for you (task 2 in tasks.json). Expectation: the second run needs fewer steps (no searching
for fields), same gate cards.

## Time saved (SPEC §8)
Every finished run that typed, attached or sent ends with "Done in 14 s. By hand this takes about 8 minutes."
The by-hand number is a fixed, honest estimate for a non-technical person (`by_hand_secs` in
`crates/core/src/memory/learn.rs`, no model call):

```
by_hand = 120 s  if Pip opened a page (find and open it)
        +  45 s  per typed field (look up the value, type it, check it)
        + 120 s  if a file was attached (find the scan, pick it in the file dialog)
        +  30 s  if something was sent (find the button, check, confirm)
        = 0      when nothing was typed, attached or sent
```
Minutes in the bubble = (by_hand + 30) / 60, at least 1 ("about a minute"). Each finished run adds by_hand to the
profile key `seconds_saved_total` (hidden from the model); from 10 minutes on the bubble also says "That's about
25 minutes saved since we met." A run whose task names "deadline #N" and that really sent something marks that
deadline Done ("I've ticked off the deadline …", event `deadline_done`); nothing sent, nothing closed.

The CLI prints it next to the real time: `Measure: steps=6 seconds=14.2 by_hand=405 saved_total=405`. Read it as
"14 s with Pip against about 7 minutes by hand" (page + 3 fields + file + send); saved_total grows with each run on the same `--db`. Note the
pair (seconds, by_hand) in results.md next to steps.
