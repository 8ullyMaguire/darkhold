# 0005 — Extensions are declarative documents, not plugins

Status: accepted, 2026-11-05. Decides M70-09, M70-10 and the rejection recorded in M70-11.

## Problem

Every self-hosted platform eventually gets the same request: let people share what they built. On this product it
arrives as "skins, and then more than skins" — shared layouts, shared recommendation recipes, shared reading
themes, and eventually someone's importer or integration.

The tempting answer is a plugin system with a marketplace. It is tempting because it is a single feature that
appears to satisfy all of them. It is wrong here for reasons that are properties of this product, not of taste.

## Decision

The extension surface is **data, plus documented integration points**:

1. **Extension documents** — a versioned JSON document describing a saved-view bundle, a dashboard layout, or a
   theme token set. Exportable, importable, validatable, previewable. No code, no callbacks, no conditions the
   server evaluates on an author's behalf. (M70-09, M70-10)
2. **A documented importer contract** — an adapter declares its version and records it in the import's provenance,
   so a badly-written adapter is visible in the data rather than inferred from a bug report.
3. **Webhooks** — the integration point for "tell another system that something happened". Already shipped; the
   event is data, and the receiver is the operator's choice.
4. **Module registry** — the internal extension mechanism, where a surface is a compiled module with a registry row,
   a `default_on` decision and a gate. Operators enable or disable it; they do not load code.

Nothing in this decision forbids third-party *code*. It forbids third-party code **running inside this server**.

## Rejected alternatives

**A plugin API with in-process code (the WordPress shape).** Rejected on five grounds, each sufficient alone:

* **The reader is server-rendered HTML that works without JavaScript** (DM5-01). A plugin model implies the
  browser executes author code, or the server does; both contradict a stated floor.
* **Erasure is an invariant, not a feature.** M45-64's gate names every account-bearing column and every FK-free
  column gets a treatment. A plugin holding reader data — a cache, a sidecar index, an analytics ping — breaks
  erasure in a way no test in this suite can observe, because the data leaves the schema the gate inspects.
* **Route inventory is a complete statement of what the server does.** Plugin routes end that completeness, and the
  inventory is one of the strongest guarantees in the repository.
* **A plugin API is a permanent compatibility promise.** The moment two third parties depend on it, every internal
  refactor is a breaking change. The module registry already provides this freedom *internally*, with none of the
  external obligation.
* **A marketplace is a supply chain.** Its success is what makes it dangerous — the largest plugin ecosystems are
  the largest source of vulnerabilities in their platforms. Curation is the actual product, and curation does not
  scale to one operator.

**WASM sandboxing.** Solves untrusted *execution*, not untrusted *data*. A useful plugin still needs reader context,
and the ABI becomes the frozen contract instead of the API. It is the right answer for a future third tier of
integrations, and the wrong first move.

**JavaScript in the reader.** Hands XSS to authors and defeats the no-JS floor at once.

**"Fork it and send a PR."** That is the current system, and for a single-operator instance it is the correct one:
it costs a rebuild, not a trust decision.

**A marketplace now.** An empty marketplace kills itself. With one operator and no third-party authors, the
storefront would list the operator's own four documents.

## Consequences

* **Extension documents must be validated on import exactly as they are validated locally**, and unknown fields must
  be refused rather than ignored. A document from a newer instance has to fail loudly; silently dropping a field it
  depends on produces two instances holding "the same" document that behave differently.
* **A public extension document is attributed to a pseud, never an account** (ADR 0003), and browsing has **no
  ratings, no install counts and no popularity ranking**. On a corpus of four, ranking by popularity is a way to make
  the first uploader permanently first.
* **Previews are produced by the server from the same renderer the reader uses**, so a preview cannot disagree with
  what importing does.
* **The question is answered, not deferred.** M70-11 exists so a future session finds a recorded rejection with its
  reasons rather than an absence that looks like an oversight.
