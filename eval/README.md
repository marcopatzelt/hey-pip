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
