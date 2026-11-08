# ADR 0031 — The inbox refuses what it cannot verify

- **Status:** accepted
- **Date:** 2026-11-08
- **Supersedes:** nothing. **Amends:** the federation intake described in §25.1.

## The problem

`POST /federation/inbox` stored whatever arrived and answered `{"status":"received"}`. Nothing about the request was checked.
The intake ledger (`federation_inbound`) recorded every arrival as `quarantined`, which was the only honest thing in the flow and
also the reason it was useless: the ledger's own vocabulary offered `accepted` and `rejected`, and nothing in the codebase could
ever produce either. An operator could move a row by hand; the instance itself had no opinion.

The shape of the fault is worth naming, because it is the same fault as a route that returns `{"feed":"rss"}`: the surface
**existed** and **claimed** to do something. A federation inbox that accepts unsigned writes is not a lenient inbox; it is a
public write endpoint with a federation-shaped path.

## Decision

The inbox verifies draft-cavage HTTP Signatures (`rsa-sha256`) before it stores anything, and **refuses by default**:

- the `Signature` header must cover `(request-target)`, `host`, `date` and `digest`;
- `Date` must be within five minutes of the instance's clock;
- `Digest` must match the bytes actually received, compared per entry;
- the peer's key is fetched from its actor document (`publicKey.publicKeyPem`) and cached per `keyId`.

Configuration: `[federation] require_signature`, **default `true`**.

The ledger's state is the answer to "was this verified":

| Outcome | Response | Ledger row |
|---|---|---|
| verifies | `200` | `accepted`, note names the key id |
| does not verify | `401` | `rejected`, note says why |
| unsigned, `require_signature = true` | `401` | `rejected`, `no Signature header` |
| unsigned, `require_signature = false` | `200` | `quarantined` |

## Alternatives rejected

**Accept unsigned traffic and let the operator decide every row.** This is what the code did, in effect. It fails at the only
moment that matters: the instance has already written the activity, created a remote actor, and answered the peer "received".
Reviewing afterwards is a report, not a gate, and the work of deciding every row falls on someone with less information than the
signature check had.

**Verify when a signature is present, and accept when it is absent.** The worst of both: an attacker simply omits the header, and
every honest peer pays the cost of signing for no benefit.

**Require a shared secret per peer.** Simpler to implement, and wrong for federation: the point of ActivityPub is that peers are
unknown in advance. A secret exchange is a private network with extra steps.

**Treat a malformed `Signature` header as absent.** Tempting because it is one line shorter. It also means every header the
parser cannot read is a request that skips verification — the parser becomes the vulnerability.

**Cache a failed key fetch.** A peer whose host was briefly unreachable would be unverifiable until the process restarted, and
every retry they made would be refused for a reason that had already expired.

## Consequences

- `require_signature = false` exists and is honest: the rows stay `quarantined`, so an operator can always tell what was verified
  from what was merely allowed.
- A refusal writes a ledger row before returning the 401, so probing is visible. That costs one insert on a rejected request; the
  alternative is an inbox nobody can audit.
- The instance now makes an outbound HTTP request per unseen peer key. It is bounded by the cache and by the same client, timeout
  and redirect policy as every other outbound call, and a peer cannot make it fetch a key it never signed with.
- **Not done, and said here rather than implied by silence:** a separate `publicKey.id` document is not followed when an actor
  document points elsewhere, and `hs2019`-with-Ed25519 peers are refused by algorithm name. Both are the same trust assumption
  every implementation makes — the actor document is fetched over TLS from the host that signed — and neither is a substitute for
  it. A peer using Ed25519 is refused, not mis-verified, so the refusal is reportable.
