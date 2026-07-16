# bleat

Durable, mail-style messaging between coding agents.

`bleat send` delivers a message to an append-only log — it never wakes,
interrupts, or blocks the receiving agent. Each agent reads its own inbox
on its own schedule, or waits for new mail with `bleat read --wait`.
No daemon, no server: the log in `.bleat/` next to your project is the
single source of truth.

## Why mail, not chat?

Coding agents work in long, focused stretches. Interrupt-style delivery
(injecting text into another agent's terminal, poking its pane) couples
tools together and breaks whatever the receiver was doing. bleat instead
borrows the email model:

- **Senders publish and move on.** Sending never blocks your work.
- **Receivers own their timing.** Read at phase boundaries, or run
  `bleat read --wait` as a background task — its exit (new mail or timeout)
  is your wake-up call, delivered by your own harness.
- **The log is the truth.** Per-role cursors track what each role has read;
  `bleat log` replays everything for auditing.

## Install

From crates.io:

```sh
cargo install bleat
```

With Nix:

```sh
nix run github:vol1003-labs/bleat -- --help
```

From a source checkout:

```sh
cargo install --path .
```

## Quick start

Two agents, `driver` and `reviewer`, sharing one project directory.

Terminal A (driver) — create the session:

```sh
bleat init demo-session --as driver
```

Terminal B (reviewer) — join it:

```sh
bleat join --as reviewer
```

Terminal A — send a question and keep working:

```sh
bleat send --as driver --to reviewer --type question "Ready for review?"
# keep working; nothing blocks
```

Terminal B — read and reply:

```sh
bleat read --as reviewer
```

```text
---
id: 0001
from: driver
to: reviewer
type: question
ts: 2026-07-16T08:34:10Z
---
Ready for review?
```

```sh
bleat send --as reviewer --to driver --type answer --re 1 "Yes, go ahead."
```

Terminal A, when the driver wants the answer (or run it in the background
and get woken by the exit):

```sh
bleat read --as driver --wait --timeout 120
```

Check the session at any time:

```sh
bleat status --as driver
```

```text
Session: demo-session
Roles:
- driver: 0 unread
- reviewer: 0 unread
Total messages: 2
Last activity: 2026-07-16T08:34:10+00:00 reviewer -> driver
```

## Command reference

| Command | Purpose |
|---|---|
| `bleat init <slug> --as <role>` | Create a session and register your role |
| `bleat join --as <role>` | Register your role in an existing session |
| `bleat send --to <role> [--type T] [--re N] [BODY \| --file F \| stdin]` | Publish a message (default type: `report`) |
| `bleat read [--peek] [--wait [--timeout N]]` | Show unread messages addressed to you; marks them read unless `--peek` (`--wait --timeout` defaults to 300 s) |
| `bleat status` | Roles, unread counts, total messages, last activity |
| `bleat log` | Full chronological log (audit) |

Role and session resolve from `--as` / `BLEAT_ROLE` and `--session` /
`BLEAT_SESSION` (falling back to the most recently created session).
Message types are free-form; the starter vocabulary is `kick`, `question`,
`answer`, `report`.
`send` requires the recipient role to be registered in the session first (usage error otherwise).

Exit codes:

| code | meaning |
|---|---|
| 0 | success (including "no unread"; for `--wait`: new message arrived) |
| 1 | runtime error |
| 2 | usage error (e.g. `--peek --wait` together, `--timeout 0`, sending to an unregistered role) |
| 3 | `read --wait` timed out with no new messages |

## Agent integration

One canonical skill, `skills/bleat/SKILL.md`, teaches agents the CLI and
the mail-model etiquette (send then keep working, wait for replies with a
background `read --wait`, escalate on timeout). It follows the
[Agent Skills](https://agentskills.io) open standard, so every harness
consumes the same file.

**Claude Code** — this repo is a plugin marketplace:

```
/plugin marketplace add vol1003-labs/bleat
/plugin install bleat
```

**Codex CLI and other Agent Skills harnesses** — copy or symlink the skill
into the harness's skills directory, e.g.:

```sh
ln -s "$(pwd)/skills/bleat" ~/.agents/skills/bleat
```

Set `BLEAT_ROLE` and `BLEAT_SESSION` in the agent's environment so it can
omit `--as` / `--session`.

## Design principles

- **Append-only log, per-role cursors.** Reading is a cursor move, never a
  mutation of history.
- **Daemonless.** A local session is just files; no background process.
- **No runtime coupling.** bleat does not spawn, wake, or health-check
  agents. Waking on new mail belongs to each agent's own harness
  (`read --wait` exits → harness resumes the agent).

## Inspiration

- [agent-message-queue (AMQ)](https://github.com/avivsinai/agent-message-queue) —
  file-based, Maildir-style local queue for agent-to-agent messages; shares
  bleat's zero-infrastructure stance.
- [agmsg](https://github.com/fujibee/agmsg) — cross-vendor messaging for
  CLI coding agents over shared SQLite; shares the daemonless,
  connect-different-agents goal.

bleat differs in its session/role model with per-role read cursors and the
`read --wait` contract designed for harness-driven wake-ups.
