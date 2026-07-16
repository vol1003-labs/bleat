---
name: bleat
description: Use when coordinating with other coding agents through a bleat session - sending a message to another agent, waiting for a reply, checking unread messages, or joining/creating a session. bleat is mail, not chat - send never blocks or wakes the receiver.
---

# bleat — messaging between coding agents

## Mental model: mail, not chat

- `bleat send` appends to a durable log. It does NOT wake, interrupt, or notify the receiver.
- Every role has its own read cursor. `bleat read` shows unread messages addressed to you and marks them read.
- The log is the single source of truth (stored in `.bleat/` next to the project).
- When to read, and whether to wait for a reply, is YOUR decision.

## Prerequisites

- `bleat` binary on PATH.
- Your role and session resolve from flags or environment: `--as <role>` / `BLEAT_ROLE`, `--session <slug>` / `BLEAT_SESSION`. If the environment is already set, omit the flags.

## Commands

| Command | Purpose |
|---|---|
| `bleat init <slug> --as <role>` | Create a session and register your role |
| `bleat join --as <role>` | Register your role in an existing session |
| `bleat send --to <role> [--type T] [--re N] [BODY \| --file F \| stdin]` | Publish a message (default type: `report`) |
| `bleat read [--peek] [--wait [--timeout N]]` | Show unread messages addressed to you; marks them read unless `--peek` |
| `bleat status` | Roles, unread counts, total messages, last activity |
| `bleat log` | Full chronological log of all messages (audit) |

Message types are free-form; starter vocabulary: `kick`, `question`, `answer`, `report`.

Exit codes:

| code | meaning |
|---|---|
| 0 | success (including "no unread"; for `--wait`: new message arrived) |
| 1 | runtime error |
| 2 | usage error (e.g. `--peek --wait` together, `--timeout 0`) |
| 3 | `read --wait` timed out with no new messages |

## Etiquette

1. **After sending, keep working.** Never block your own work just because you sent a message.
2. **Need a reply? Wait in the background.** Launch `bleat read --wait --timeout <secs>` as a background task (`--timeout` defaults to 300). The process exit wakes you: exit 0 = reply arrived (already displayed and marked read), exit 3 = timeout, nothing arrived.
3. **On timeout (exit 3), decide deliberately:** re-send with more context, escalate to the human, or continue without the answer. bleat never decides for you.
4. **Check unread at natural boundaries.** Run `bleat read` when you finish a phase of work or resume after a pause.
5. **Reply threaded.** Answer a `question` with `--type answer --re <id>` so threads stay traceable.
6. **Use `--peek` sparingly.** It shows unread without consuming it, and cannot be combined with `--wait`.

## Out of scope

This skill does not define what role names mean, workflow phases, or review conventions. Those belong to your project's own instructions.
