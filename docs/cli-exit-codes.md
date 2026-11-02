# The `dh` exit codes

`M63-16`. The number a `dh` process exits with is **part of its interface**, not a side effect of a
`return 1`. A wrapper script — a cron entry, a Makefile, an editor's task runner — decides what to do
next from that number alone, so every code has one meaning and never changes meaning.

| code | meaning | what a script should do |
|---:|---|---|
| `0` | the command did what was asked | continue |
| `1` | the environment failed: `5xx`, connection refused, TLS, DNS | retry, later |
| `2` | the command itself was wrong: bad flag, bad value, unparseable window | **do not retry**; fix the call |
| `3` | the thing named does not exist (`404`) | do not retry; the input is stale |
| `4` | authentication or authorisation failed (`401`, `403`), or a session was required and not given | obtain a session, then retry |
| `5` | rate-limited (`429`) | retry after the window the server names in `Retry-After` |

The single definition is `crates/cli/src/exit.rs`; the contract test is
`crates/cli/tests/exit_codes.rs`, which runs the real binary against a canned server so the number
asserted is the number a shell sees.

## Why `2` is usage and not `1`

`clap` exits `2` when a flag does not parse, before any of this program runs. Making usage the same
code the argument parser already uses means a wrapper that sees `2` may assume **nothing ran**, which
is true in both cases — and it keeps `1` free for "it ran and the world failed", the only case worth
retrying.

## What changed, and why it mattered

`401`, `403` and `404` were all reported as `2`. A wrapper was therefore told *"your command is
wrong"* when the truth was *"your session is missing"* or *"that work was deleted"* — the two cases it
can act on were indistinguishable from the one it cannot. Two separate mappings caused it: one in
`rec.rs` and one in `module.rs`, each with its own idea of what a status meant.

Both now call `exit::for_status`, and the local refusals follow the same table: refusing to send a
request that is known to need a session (an engine, `--exclude-read`) exits **4**, because refusing
locally must not change what the number means.

## Adding a code

Append, never renumber. A script written against the current table is a script with an assumption in
it, and changing a number is a silent behaviour change in somebody else's automation. The test
`every_code_in_the_table_is_reachable_from_a_status_or_a_verb` fails if a documented code cannot be
produced, so the table cannot grow a row nobody has run.
