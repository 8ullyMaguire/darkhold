# Dark / evil protagonist progression fantasy, across fandoms.
#
# The four clauses are the four things the subgenre always has: a protagonist who is not
# reformed, a power curve that is the point of the story, mechanics that make the curve
# legible (system, gamer, litrpg), and — per-instance — a fandom clause.
#
# BUDGET: the server refuses a query with more than 24 terms, and a `tag:` term costs two
# (the field and its value), so about twelve tags is the ceiling. This file spends ten on the
# trope and two on exclusions. Add a fandom with `AND tag:naruto` on the command line rather
# than here, or the files stop parsing — `crates/cli/tests/shipped_queries.rs` is what says so.
#
# Excluded on purpose: `fluff` and `hurt/comfort`, the two tags most likely to ride along with
# `dark` and neither of which is this.

(tag:dark-protagonist
  OR tag:evil-protagonist
  OR tag:villain-protagonist
  OR tag:morally-grey-protagonist)

AND (tag:progression
  OR tag:progression-fantasy
  OR tag:overpowered)

AND (tag:system
  OR tag:gamer
  OR tag:litrpg)

AND NOT tag:fluff
AND NOT tag:hurt-comfort
