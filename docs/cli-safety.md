# Safety at the tool boundary

`M63-21`. Two rules, both because the safe path should be the **short** path — a rule that costs an
extra flag on every invocation is a rule people stop following.

## A change is a preview unless you say `--yes`

Every mutating verb prints the exact request it would send and exits `0`:

```text
$ dh module off arena
would PATCH http://127.0.0.1:8081/api/v1/modules/arena {"enabled":false}
re-run with --yes to apply (nothing was changed)
$ echo $?
0
```

`--yes` applies. `--dry-run` is the same as the default, and exists so a generated script can say
what it means instead of relying on a missing flag. The pair is refused by the parser (`conflicts_with`)
with exit `2`: "apply" and "do not apply" is not a question with a tie-break.

What is covered, and why the list is not hand-kept:

| verb | method | guarded |
|---|---|---|
| `dh module on/off` | `PATCH` | yes |
| `dh supply recompute` | `POST` | yes |
| `dh supply route` | `POST` | yes |
| `dh supply complete` | `POST` | yes — it pays a patronage once |
| `dh supply keystones/droughts/routing` | `GET` | **no**, by design |
| `dh rec`, `dh module list/show`, `dh redact` | `GET` / local | no |

The guard is decided from the **HTTP method**, not from a list of subcommands, so a verb added later
is covered by construction: a new `POST` would take the same branch that reads the tuple the request
is built from. A read never asks for `--yes` — a tool that demands a confirmation to look at something
trains people to type the flag by reflex, which is how the confirmation stops working.

Refusing to preview a request that would be refused anyway still comes first: `dh module off` without
a cookie exits `4` before the preview, because there is no request to describe.

## `dh redact` — what leaves your machine is the thing worth pasting

```text
$ dh rec --fandom naruto --json | dh redact
$ dh redact < transcript.json
```

| class | becomes | why |
|---|---|---|
| UUIDs | `uuid#1`, `uuid#2` … | every account, pseud, work and routing id is one; a single leaked account id joins every other row about that person |
| email addresses | `email#1` … | an account identifier that is also a real-world identity |
| `darkhold_session=…` | `darkhold_session=session#1` | a live credential, not an identifier |
| `Bearer …`, `DARKHOLD_TOKEN=…` | `Bearer token#1` | the same |

Numbering is per class in order of first appearance, so two pastes of the same transcript redact
identically and a reader can still see that two lines name the same value without learning which. It
is idempotent: redacting a redacted transcript changes nothing, which matters because transcripts get
pasted twice.

**What is deliberately not touched:** fandom names, typed handles, work titles. A redactor that guesses
at "pseudonymous-looking" words corrupts the output that makes a paste useful, and a wrong redaction is
worse than none — it reads as if the field were scrubbed. The UUID matcher is anchored on the dashes,
so a longer hex string is left whole rather than half-redacted into a fragment that looks like an id
and is not one.
