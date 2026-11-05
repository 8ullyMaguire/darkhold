# The trope query library

Each `.q` file is one trope family, written as the query language the instance already speaks:
`dh q --from-file tropes/dark-progression`. Files are versioned so "what does this instance consider
dark progression" is a diff in review rather than a shell history.

Rules these files follow:

- `#` starts a comment at the start of a line or after whitespace, so a tag spelled `#angst` survives.
- Lines are joined with one space, so a multi-line paren group is one expression.
- Every clause that narrows says what it excludes and why, in a comment above it.

`crates/cli/tests/shipped_queries.rs` parses every file here, so a typo fails the build rather than
the operator's evening.
