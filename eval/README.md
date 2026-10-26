# eval

OWNER: agent "docs-cli". 10 tasks with invented data (SPEC §13.3):
5 letters (deadline correct?) and 5 screen tasks (target hit? steps? gate fired?).
Run: `cargo run -p copilot-cli -- eval --tasks eval/tasks.json --out eval/results.md`
(uses the real API, costs cents; screen tasks run on Linux with the fake platform).
