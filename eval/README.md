# eval

OWNER: agent "docs-cli". 10 tasks with invented data (SPEC §13.3):
5 letters (deadline correct?) and 5 screen tasks (target hit? steps? gate fired?).
Run: `cargo run -p pip-cli -- eval --tasks eval/tasks.json --out eval/results.md`
(uses the real API, costs cents; screen tasks run on Linux with the fake platform).

## Learning: first run vs. second run (SPEC §8)
Pip saves a playbook after a finished run (page, field label -> profile key, attach and send button; never the
typed values). The second run on the same site starts with "LAST TIME ON THIS SITE …" in its first request.
Measure it with one database file for both runs (real API, a few cents):

```
DB=$(mktemp -d)/memory.db
cargo run -p pip-cli -- --db "$DB" agent "Send my sick note to my health insurer" --approve all
cargo run -p pip-cli -- --db "$DB" agent "Send my sick note to my health insurer" --approve all
```
Each run ends with `Measure: steps=N seconds=S` and a `Memory:` line (new playbook / updated). Note both
pairs in results.md. Expectation: the second run needs fewer steps (no searching for fields), same gate cards.

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
