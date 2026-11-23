# Darkhold — imported works vs original writings (live measurement, 2026-11-23)

The 2026-11-07 review said the corpus is dominated by imports and that the product claims to be both an archive and a
place where people write. This file measures that claim against the deployed database. It is a **measurement, not a
product change**.

## Method

Queried the live `darkhold` database on `thinkcentre` through `sudo -u postgres psql`. A work counts as imported when
its id appears in `library_items.work_id` with a `source_key` or `source_url` — those columns are how the importer
records provenance. No rows were changed.

```sql
select count(*) as total_works,
       count(*) filter (where id in (
         select work_id from library_items
         where work_id is not null and (source_key is not null or source_url is not null)
       )) as sourced_works
from works;
```

## Output

```
 total_works | sourced_works
-------------+---------------
       58042 |         57225
```

All 57,225 linked library items carry a source key or URL; zero linked items lack one. So **57,225 / 58,042 =
98.59% of works are traceable to an import**, and **817 works have no import provenance**. The review's direction is
confirmed on the live corpus.

## Limits and decision

- This measures *provenance*, not quality, authorship or intent. An imported work can be someone's own writing
  originally posted elsewhere; an original work can be unlinked. Those are separate questions requiring owner data.
- `source_key` is populated for every linked item in this snapshot; if a future importer writes null provenance, this
  query would undercount imports. The query should be revisited when the importer changes.
- **Decision:** stop describing the current instance as a writing-first platform in new copy. Call it an archive with
  a writing path. Publishing the ratio beside any growth claim is the minimum honest presentation. Do not delete or
  demote imported works to improve the number — the number is a description of what was built, not a target to game.
- **Unblock for a writing-first claim:** a measured rise in works without import provenance, or an owner-approved
  definition of "original" that survives the limitation above. No threshold is claimed here.
