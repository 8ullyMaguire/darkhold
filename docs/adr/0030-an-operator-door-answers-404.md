# ADR 0030: An operator door answers 404, and the refusal shape is part of the decision

- Status: accepted
- Date: 2026-11-08
- Supersedes: the status-code choice implied by `admin_scope::estate_admin_allowed`
- Spec: §59 (extensions), §14.4 (trust), ADR 0003 (one decision, one place)
- Milestone: M69 (found while shipping M69-10)

## Context

The operator surface used to answer `404` to a caller who was not the operator. Seven doors said so in their own
documentation, in the same words:

> an endpoint whose existence is a disclosure must answer 404, because 403 says "yes, and you may not, which tells a
> reader probing the URL that this instance runs an economy dashboard".

Fourteen files each carried their own six-line copy of the predicate, all keyed on `administration.operator_account_id` — a
column that is unset on this host, which meant every one of those doors answered `404` to **everybody**, operator included.
ADR 0003's fix was right: one authority, `admin_scope::estate_admin_allowed`.

The fix moved the authority and lost the shape. That helper answers `403 AccessDenied`, so from the refactor onward the
decision audit, the economy dashboard, discovery's operator doors, import administration, the job console, media health and
the north-star view were indistinguishable from a permissions problem — they told every signed-in reader that the surface
exists. Two tests went red on the next full-suite run (`decision_audit_routes`, `flow_dashboard`); five of the seven doors had
no test asserting their refusal shape at all, which is why the regression survived a green targeted suite.

## Decision

### 1. `404` is the default shape for an operator door

An operator surface is not a thing whose existence a stranger may learn. The uniform answer is `404` with a **generic**
resource name, because `AppError::NotFound { resource }` renders the name into the message and naming the surface would put
the disclosure back into the body of the refusal.

### 2. The shape is a second function, not a parameter

`admin_scope::require_estate_hidden` is the same authority as `estate_admin_allowed` and a different refusal. `estate_admin_allowed`
stays for doors that are allowed to admit they exist (a destructive door where "you may not" is useful information to an
operator who is nearly right). Two functions, one predicate between them: a boolean parameter would make the two shapes
look interchangeable, which is exactly the mistake that produced this ADR.

### 3. A door's refusal shape is a test, not a comment

Every operator door gets a test that asserts the status, not merely that the call failed. "Refused" passes against `403`;
the assertion is `NotFound`. Seven doors had the comment without the test, and the comment is what was wrong.

## Alternatives rejected

- **Make `estate_admin_allowed` return 404 everywhere.** Rejected: a destructive door that answers 404 to an operator who is
  merely under-privileged is a support burden, and "the surface is real but this isn't the account for it" is a legitimate
  thing to say about `/admin/retention`, which is documented in the manual anyway.
- **Return 403 everywhere and update the docs.** Rejected: the disclosure argument is the stronger one, and it is the
  argument the code already made in seven places. Docs follow the rule, not the other way round.
- **One helper with an `Option<&'static str>` resource.** Rejected as the same mistake in a smaller costume: the caller would
  choose the name, and a caller that names the surface undoes the decision at the call site.

## Consequences

- A caller probing `/api/v1/operator/…` or `/api/v1/admin/…` learns nothing beyond "not here".
- Doors that legitimately say 403 are now explicit exceptions with a reason, rather than the default.
- The fleet-wide gap is named: five of the seven doors still have no status assertion of their own. `route_inventory` checks
  that every route declares an audience; nothing yet checks that an operator route's refusal is a `404`. That check is worth
  writing and is not in this ADR.
