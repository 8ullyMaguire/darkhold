# 55. Curator-submitted source adapters

**Status:** reserved
**Milestone:** —
**Source:** §55 of `docs/spec.md`

> **Implementation note (2026-10-03).** **Path A (declarative YAML) shipped**, tag
> `v0.55-source-adapters`; **Path B (WASM) is specified and not built**, gated by
> §55.6. Plan: `docs/plans/m45-23-curator-adapters.md`.
>
> | Concern | Where |
> |---|---|
> | Migration | `migrations/*/0113_*` — `extension_submissions`, `adapter_reviews` |
> | TL3 gate (§55.2), 3-reviewer threshold (§19.4) | `crates/db/src/source_adapters.rs` |
> | Manifest + category types, DeclarativeAdapter | `crates/scrapers/src/source_manifest.rs`, `declarative.rs` |
> | HTTP surface | `crates/app/src/routes/source_adapters.rs` |
> | §55.5 automated check | `crates/scrapers/src/declarative_check.rs` |
> | §55.6 gate enforcement | `crates/app/tests/wasm_gate.rs` + `scripts/check-wasm-gate.py` |
>
> Migration `0113` was renumbered once during planning: M45-22's plan claimed it too,
> and M45-22 took `0114`.

Gaps review, tracked as M45-23.

§11.1 defines the adapter abstraction, §11.5 the safe fetcher, §11.6 the
credential vault, §11.8 runtime health, §21.1–§21.5 the extension machinery,
§19.4 the quorum thresholds and §24.14 the crawling posture. This section adds
the thing none of those provide: **a way for a TL3 curator to submit one, and a
decision about how much code that submission is allowed to contain.**

**What §21.1–§21.5 have, and do not have.** `crates/domain/src/extension.rs`
provides `ExtensionState`, `Capability` (storage, work read, webhook, job) and
`MemoryTier`, and `grant_is_subset` enforces that a grant cannot exceed the
manifest. There is **no `Manifest` struct and no `Category` enum** — §21.1's field
list and §21.2's category list are prose. So this section's work is partly
turning that prose into types, which is a prerequisite rather than an extra: a
submission has to be a value before it can be validated, reviewed and stored.
`Capability` also has no network or credential variant, and §55 needs both.

## 55.1 The decision, stated before the machinery

Two shapes are supported, and the split is the whole design:

| | **§55.3 Declarative** | **§55.4 WASM** |
|---|---|---|
| Submitted | YAML: URLs and CSS selectors | A compiled module |
| The curator runs | Nothing | Untrusted code |
| The sandbox is | The parser itself | `wasmi` + host API |
| Quorum reads | The YAML, directly | Source, and a compiled hash |
| JS-rendered sources | No | Yes |
| Review cost | Low — a steward can read selectors | High |

**A declarative adapter has no sandbox to escape from, because it is not code.**
That is not a stylistic preference. It is the reason Path A can ship to a young
instance while Path B cannot, and the reason §55.4's rules exist at all: Path B's
entire security story rests on the domain lockdown in §55.4.1 being airtight,
which is a property of an implementation that does not exist yet.

So the declarative path is the **complete** v1 feature and the WASM path is
specified alongside it but gated (§55.6). The declarative path covers the
sources that need only a URL pattern and selectors — of the porting backlog's 11
adapters, 8 are pure `URL + CSS selector` (ao3, chyoa, efiction, ffnet, ficbook,
royalroad, syosetu, xenforo).

**It does not cover the other three, and the spec says which and why**, because
pretending otherwise would make Path A look like it is the whole feature when it
is the majority of it:

| Adapter | What it needs | Why selectors are not enough |
|---|---|---|
| `scribblehub` | `Wall::Solver` | Metadata and chapter list come from a JSON API; the prose pages are behind a challenge. Reading them is what `§11.5` forbids us from circumventing, and the solver is not a selector. |
| `ffnet` | `Wall::Solver` | Same shape, different wall. |
| `pawchive` | Credential-gated | A Patreon-backed archive: access is a paid account, not a public page. |

These are not hypothetical gaps and they are not all solved by Path B. A solver
is §11.5's anti-circumvention boundary, and adding a WASM host call that drives a
browser would put a paid third-party service inside the sandbox. **This section
declines that**: JS-rendered sources are reachable in v1 only where the site also
serves a plain JSON endpoint, which is why `scribblehub` reads its metadata that
way today.

## 55.2 What a curator submits, and what gets them

**Submission requires TL3.** §19.14 already sets this bar for canonical
curation and §19.2 defines TL3 as reviewed trusted contributor. An adapter is a
claim about a third party's site *and* a thing that will fetch on this instance's
address, so it is held to the governance bar rather than the participation bar
§19.14 sets for ordinary signals. Like both of §19.14's, this one is not
configurable: §0.3 makes trust non-purchasable.

**Submission is not deployment.** The path is §21.5's, unchanged:

```text
submitted → automated checks → permission review → security review
→ independent quorum approval → published
```

§19.4's "extension approval: three reviewers with permission-review expertise"
applies, and this section adds nothing to lower it.

## 55.3 Declarative adapters

A manifest (§21.1's fields) plus a `source:` block:

```yaml
id: example-archive
version: 1
category: source_adapters
entrypoint: source.yaml
required_host_api_version: 1
permissions: [network, credentials_read]
network_allowlist: [example-archive.org]
license: CC0-1.0
pricing: free

source:
  source_id: example-archive
  name: Example Archive
  base_url: https://example-archive.org
  robots_txt: respect
  rate_limit_per_second: 2
  work_pattern: "/works/{id}"
  chapter_pattern: "/works/{id}/chapters/{num}"
  selectors:
    title: "h1.work-title"
    author: ".byline a"
    summary: ".summary blockquote"
    body: ".chapter-content"
    tags: ".tags li"
    word_count: ".stats .words"
    date_published: "meta[property='article:published_time']"
  pagination:
    type: next_link
    selector: "a[rel='next']"
  auth:
    type: none            # none | cookie_login | api_key | oauth
    login_url: null
```

**The curator submits data and the host interprets it.** No Rust, no WASM, no
script. Every URL an adapter can reach is a pattern over its declared
`base_url`; every byte it parses arrives through the same `SafeFetcher` §11.5
already mandates for compiled adapters, so SSRF refusal, robots compliance and
per-domain pacing are inherited by construction rather than reimplemented.

That inheritance is the reason to build Path A on this codebase rather than
beside it. `crates/scrapers/src/safety.rs` already holds `validate_url`,
`SafeFetcher::new(allowed_hosts, policy)`, robots enforcement and
`FixtureFetcher`. A declarative adapter that could bypass them would be a second
set of weaker guards, and the whole argument for Path A would be that it is safe.

**An unparseable selector is a submission error, not a runtime failure.** Every
selector compiles at submission time. A manifest that will throw on first fetch
has a defect a steward can see by reading five lines, and it is refused before it
enters the queue rather than discovered by a reader's import.

## 55.4 WASM adapters

A module implementing §11.1's `SourceAdapter`, run in `wasmi` (§21.4's choice)
with fuel metering, the §21.4 memory tiers, output limits and host-call limits.
It opens no socket of its own. Network access is only through these host calls:

| Call | Bounded by |
|---|---|
| `host.http_get(url, headers)` | §55.4.1's domain lockdown, §11.5's fetcher |
| `host.parse_html(html, selector)` | the selector's own output limit |
| `host.sleep(ms)` | the wall-clock limit; a sleep is fuel, not a pause |
| `host.log(msg)` | the log-length limit |

`host.sleep` is listed because its absence is how a rate-limit claim gets faked:
a module that cannot ask for time cannot be *forced* to yield it, so the host's
pacing in §11.5 has to apply to the module's own pacing requests.

### 55.4.1 The domain lockdown is the security model

A module that can reach `169.254.169.254` reads cloud credentials; one that can
reach `127.0.0.1:5432` reads this instance's database. §11.5 rejects these
addresses for adapters, and the same refusal applies to the WASM host API.

**§11.5's guards are two functions, and using only one of them is a silent
hole.** `safety::validate_url` checks the *syntactic* shape — scheme, length,
port — and resolves nothing, so it accepts `http://169.254.169.254` without
complaint. `safety::resolve_public` holds the address rules and is `async`
because it resolves. So:

- **At submission**, everything checkable without a request: scheme, literal
  addresses against `is_forbidden_ip`, local hostnames against
  `is_local_hostname`, and the allowlist shape. This is what §55.3's `compile`
  does, and a manifest pointing at the metadata service is refused there.
- **At fetch**, the resolved case: `SafeFetcher` calls `resolve_public` before
  every connection and re-checks every redirect.

The remaining rules apply to both paths:

- **The allowlist is declared at submission and enforced at the socket.** The
  check is in the host's fetch path, not in the sandbox and not in the module. A
  guard the module can decline to call is not a guard.
- **Resolution happens once, and the address that was validated is the address
  that is connected to.** This is §11.5's DNS-rebinding rule, and it is the
  reason a hostname allowlist is not sufficient on its own.
- **Every redirect is re-validated against the allowlist.** A permitted host that
  redirects to a private address is refused, and the refusal names the redirect.
- **A refused request is a refusal, not a retry.** An adapter cannot turn a
  refusal into persistence by asking again.

### 55.4.2 Credentials are injected, never revealed

§11.6's vault holds the credential; the module never receives it. It receives a
session cookie or a bearer token, scoped to the declared `base_url`, and the
host attaches them — so a malicious adapter can *use* a credential and cannot
*exfiltrate* one. §11.6's "avoid forwarding credentials to unrelated origins"
is enforced against the same allowlist, not against the module's intentions.

### 55.4.3 Output is validated before it is stored

The module's return value is parsed into §4.3's work and chapter shapes by the
host. A module returning a string is a schema violation, not a work. A
submission-phase fuzz that feeds recorded hostile HTML and checks for panics,
non-termination and schema violations runs before security review, because a
reviewer reading source is a poor substitute for a machine finding the crash.

### 55.4.4 Rate limits are the host's to enforce, not the module's

A module *declares* a rate; the host *applies* it. §11.5's rules — the source's
published `Crawl-delay`, the one-request-per-second floor, the operator's
`Disallow` override — hold for both paths identically. A module cannot request a
faster pace, and a declarative manifest's `rate_limit_per_second` is an upper
bound the host may lower, never a floor the host honours as requested.

### 55.4.5 Provenance: adapter incidents are not bad themes

A theme that renders badly is a bug. An adapter that exfiltrates credentials, or
ignores a source's stated wishes, is an incident with a blast radius this
instance can measure.

- **Disabling is one action and it is instance-wide.** §19.5's emergency actions
  already include disabling an extension and pausing an unsafe importer; an
  adapter is both, and §55.5's audit entry records who did it and why.
- **The blast radius is reportable.** Readers whose credentials were used by the
  adapter are notified, because §11.6's vault holds real accounts on real sites
  and "we disabled something" is not an answer to "was my password used".
- **Responsibility follows trust, not code.** The submitting curator is
  accountable under §19.6's sanctions; the operator of the instance is
  accountable for the crawling it permitted, which is §24.14's posture already
  and is why §55.2's ToS declaration is reviewed rather than collected.

## 55.5 The automated check, and what a reviewer actually sees

Every submission — both paths — runs the same pre-review gate:

- Fetch three known works from the source (§11.7's discipline: a claim of
  support is evidence or it is nothing).
- Validate the output against §4.3's shapes.
- Verify robots compliance and pacing behaviour.
- Produce a report: pass/fail, sample parsed output, resource usage.

**The report is what makes quorum review meaningful.** Reviewers see the
manifest, the report, and the sample parses. For a declarative adapter that is
the entire artifact — and a non-technical steward can read a CSS selector. For a
WASM adapter it is source plus a compiled hash, which is a harder review, and
§55.4.6 says so rather than pretending otherwise.

## 55.6 WASM is gated, and the gate is not a version number

`wasmi` is not a dependency of this workspace today, and §21.4 names it as the
intended choice rather than an adopted one. So the WASM path is specified,
tracked, and **not built until the declarative path is in production and the
§55.4.1 lockdown has been written and attacked.**

The gate is deliberately not "after enough time". It is: the sandbox exists, the
lockdown has a test that proves a private address is refused, and a second
reviewer has read it. Shipping a WASM path with an unproven lockdown would make
every other §21 guarantee conditional on one unexamined function.

## 55.7 Invariants

- **A curator's adapter cannot do what §11.5 forbids a compiled adapter to do.**
  The two paths differ in what the curator writes, not in what the instance will
  permit. There is no "trusted curator" exemption anywhere in this section.
- **No adapter sees the database.** Both paths return data through the same
  import path, and neither constructs a connection.
- **Submission confers nothing.** An adapter is inert until published, and
  published is a quorum act (§55.2).
- **A submission cannot widen its own allowlist.** The declared
  `network_allowlist` is fixed at submission; changing it is a new version
  through the whole workflow.
- **Support is counted from verified adapters only** (§11.7, §11.16), never from
  submissions.
- **An adapter's ToS declaration is reviewed, not collected.** §55.4.5.

## 55.8 Acceptance

- A declarative manifest parses, compiles every selector, and fetches and parses
  a work end to end through `SafeFetcher`.
- A manifest naming a private, loopback, or link-local address is refused at
  submission.
- A manifest whose selector fails to compile is refused, naming the selector.
- A declarative adapter and the equivalent compiled adapter return the same work
  for the same fixture.
- A submitted adapter serves no reader traffic before publication.
- A curator below TL3 is refused, and the refusal names the bar.
- Quorum approval requires three permission-review reviewers (§19.4).
- An adapter incident disables the adapter instance-wide, writes an audit entry
  with a reason (§19.5), and reports which readers' credentials were involved.
- A declarative manifest cannot raise its own rate above the host's applied
  rate.
- No `wasmi` dependency exists while §55.6's gate is unmet.

---
