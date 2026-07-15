# Task 15 report: Herdr process adapter

## Status

Implemented the Herdr process adapter for current-pane handle resolution.

Commit subject: `feat: adapt herdr process responses`

## Implementation and public API

- Exported `runtime::herdr` and added `HerdrRuntime<R>` with a production `HerdrRuntime::new()` constructor.
- Kept `ProcessRunner` private. The production runner delegates directly to `Command::new(program).args(args).output()`.
- Implemented `Runtime::current_handle` with `HERDR_PANE_ID` precedence and `herdr pane current` fallback, followed by `herdr pane get <pane_id>` in both cases.
- Returned `RuntimeHandle { terminal_id, pane_id, agent_name: None }` from the `pane get` response.
- Converted spawn errors, non-zero status, JSON errors, and missing required fields to `BleatError::Runtime`. Non-zero status errors include stderr.
- Left `spawn`, `alive`, and `nudge` as explicit not-implemented runtime errors so Tasks 17 and 18 retain their own behavior-first implementation scope.
- `Cargo.toml` required no change because `serde` derive and `serde_json` were already dependencies from earlier tasks.

## Process argv

The adapter never constructs a shell command string. It passes the program and OS arguments separately:

- Environment path: program `herdr`; argv `pane`, `get`, `<HERDR_PANE_ID>`.
- Fallback lookup: program `herdr`; argv `pane`, `current`.
- Fallback resolution: program `herdr`; argv `pane`, `get`, `<current pane_id>`.

The environment-precedence test uses `pane with spaces; $(echo nope)` and verifies that the production adapter gives the recording runner one unchanged `OsString` argument. The stub only records production invocations and supplies predetermined `Output` values.

## Response fixture basis

The brief/spec/plan fixed the command order but did not uniquely specify the JSON envelope. Work paused with `NEEDS_CONTEXT` rather than guessing. A read-only query against the installed Herdr 0.7.3 resolved the fixtures:

- `herdr pane current`: `{ "id": "cli:pane:current", "result": { "pane": { "pane_id": "w9:p4", "terminal_id": "term_...", ... }, "type": "pane_current" } }`
- `herdr pane get w9:p4`: `{ "id": "cli:pane:get", "result": { "pane": { "pane_id": "w9:p4", "terminal_id": "term_...", ... }, "type": "pane_info" } }`

The serde structs decode only `result.pane.pane_id` and `result.pane.terminal_id`; serde's default behavior ignores all unknown envelope and pane fields. `agent get` is not used by Task 15's `current_handle` path.

## Independent tests

Each test targets the production `HerdrRuntime::current_handle` behavior through the public `Runtime` trait:

- environment pane ID takes precedence and preserves program/argv boundaries;
- absent environment pane ID falls back through `pane current` then `pane get`;
- invalid JSON becomes a runtime error;
- non-zero status includes stderr in the runtime error;
- a missing `terminal_id` becomes a runtime error naming the field.

## RED and GREEN evidence

RED command: `cargo test herdr`

RED result: exit 101 with unresolved imports for `HerdrRuntime` and `ProcessRunner`, the expected failure because production adapter types did not exist.

GREEN command: `cargo test herdr`

GREEN result: exit 0; 5 passed, 0 failed. An intermediate green run exposed two visibility warnings from a private default runner type. Refactoring the production runner to a function pointer removed those warnings without changing behavior, and the focused suite remained green.

## Full verification

- `cargo test herdr`: exit 0; 5 passed, 0 failed.
- `cargo test`: exit 0; 89 library tests and 1 integration test passed; 0 failed.
- `cargo fmt --check`: exit 0.
- `cargo clippy --all-targets -- -D warnings`: exit 0.
- `git diff --check`: exit 0.

## Self-review

- Tests express five independent contracts; none tests the recording stub's own behavior.
- Production invocation uses `Command` with a separate program and `OsString` argv, so shell-like input is data rather than syntax.
- Production code contains no `unwrap`, `expect`, or `panic`, and no unnecessary clone. The only argument copy creates the owned `pane get` argv required by `Command`/the runner boundary.
- Serde types contain only required response data and permit forward-compatible unknown fields.
- The implementation does not invoke real Herdr in unit tests and does not pre-implement spawn/alive/nudge behavior.

## Concerns

No remaining blocker. The exact envelope ambiguity was resolved from Herdr 0.7.3 output and captured above. Agent response decoding, spawn, alive, and nudge remain deliberately deferred to Tasks 17 and 18.
