//! M70-12 — which surfaces a restricted work may appear on.
//!
//! The decision of 2026-11-05 made `restricted` a *listed but locked* tier: the record is public, the body needs a
//! session. That splits the database's queries into two classes, and the split is a rule rather than a preference:
//!
//! * **LIST** surfaces show the work — catalogue, search, tag browse — because a stranger is allowed to find it and be
//!   told why they cannot read it.
//! * **INFER** surfaces must not — taste vectors, tasting pools, the recommendation ladder, "is this pseud
//!   discoverable". A locked work cannot have been read by anyone who was not signed in, so an edge built on it is a
//!   claim about content neither side of the edge has seen, and a kudos or reader count built on it is a number
//!   nobody could have contributed to.
//!
//! The test asserts one of each, in both directions, with an unlisted work as the control that must stay invisible to
//! every surface — including the listing ones, because "listed" is the whole difference between the two tiers.

use test_support::TestDb;

const ACCOUNT: &str = "01955000-0000-4000-8000-0000000000a1";
const AUTHOR: &str = "01955000-0000-4000-8000-0000000000e1";
const QUIET_AUTHOR: &str = "01955000-0000-4000-8000-0000000000e2";

const PUBLIC_WORK: &str = "01957000-0000-4000-8000-0000000000d0";
const RESTRICTED_WORK: &str = "01957000-0000-4000-8000-0000000000d1";
const UNLISTED_WORK: &str = "01957000-0000-4000-8000-0000000000d2";
const QUIET_WORK: &str = "01957000-0000-4000-8000-0000000000d3";

const TAG: &str = "01958000-0000-4000-8000-0000000000a1";

async fn exec(tdb: &TestDb, statement: &str) {
    sqlx::query(statement)
        .execute(tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .unwrap_or_else(|error| panic!("{statement}\n{error}"));
}

async fn seed(tdb: &TestDb) {
    exec(
        tdb,
        &format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','surfaces@m7012.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    for (pseud, handle) in [(AUTHOR, "hollowgast"), (QUIET_AUTHOR, "quietgast")] {
        exec(
            tdb,
            &format!(
                "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                    updated_at, version)
                 VALUES ('{pseud}','{ACCOUNT}','{handle}','{handle}','listed',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }

    for (index, (id, owner, title, visibility)) in [
        (PUBLIC_WORK, AUTHOR, "A public work", "public"),
        (RESTRICTED_WORK, AUTHOR, "Held close", "restricted"),
        (UNLISTED_WORK, AUTHOR, "A link somebody sent", "unlisted"),
        // The quiet author's ONLY work is restricted: nothing about this person is knowable to a stranger.
        (QUIET_WORK, QUIET_AUTHOR, "Held quiet", "restricted"),
    ]
    .into_iter()
    .enumerate()
    {
        exec(
            tdb,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility,
                                    lifecycle, completion, created_at, updated_at, version)
                 VALUES ('{id}','{owner}','{title}','','en','general','{visibility}','published',
                         'complete','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
        // A published work with no publish event would sort oddly in the freshness list; the event is what `new` reads.
        exec(
            tdb,
            &format!(
                "INSERT INTO publication_events (id, work_id, action, occurred_at, actor_pseud_id)
                 VALUES ('01959000-0000-4000-8000-00000000{seq:04}', '{id}', 'publish',
                         '2026-01-0{n}T00:00:00Z', '{owner}')",
                seq = index + 1,
                n = index + 1,
            ),
        )
        .await;
    }

    // One tag on every work, canonicalised once: the tag browse surface is the third listing.
    exec(
        tdb,
        &format!(
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
             VALUES ('{TAG}','tag','Held Close','held close','2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    for work in [PUBLIC_WORK, RESTRICTED_WORK, UNLISTED_WORK, QUIET_WORK] {
        exec(
            tdb,
            &format!(
                "INSERT INTO work_tags (work_id, node_id, added_at)
                 VALUES ('{work}','{TAG}','2026-01-01T00:00:00Z')"
            ),
        )
        .await;
    }

    // Search reads `works_index_terms`, which the indexer fills for whatever it is given: the exclusion being tested
    // is the query's, not the indexer's.
    for (work, title) in [
        (PUBLIC_WORK, "A public work"),
        (RESTRICTED_WORK, "Held close"),
        (UNLISTED_WORK, "A link somebody sent"),
        (QUIET_WORK, "Held quiet"),
    ] {
        let work_id: darkhold_domain::ids::WorkId = work.parse().expect("work id");
        darkhold_db::search::rebuild_work_index(tdb.db(), &work_id, title)
            .await
            .expect("index");
    }
}

#[tokio::test]
async fn a_locked_work_is_listed_and_never_inferred_from() {
    let dir = test_support::scratch_dir("m7012");
    let tdb = TestDb::connect_with_dir("m7012", &dir).await;
    seed(&tdb).await;
    let db = tdb.db();

    // --- LIST: the catalogue, the tag browse and the search all show a restricted work -------------
    let fresh = darkhold_db::content::list_public_works_by_freshness(
        db,
        darkhold_db::content::FreshnessSort::New,
        50,
    )
    .await
    .expect("freshness");
    assert!(
        fresh.contains(&RESTRICTED_WORK.parse().expect("id")),
        "a locked work is listed, or the tier means nothing: {fresh:?}"
    );
    assert!(
        !fresh.contains(&UNLISTED_WORK.parse().expect("id")),
        "unlisted is not listed: {fresh:?}"
    );

    let tagged = darkhold_db::taxonomy::works_by_tag(db, "held close", 50, 0)
        .await
        .expect("tag browse");
    let tagged_ids: Vec<&str> = tagged.iter().map(|(id, ..)| id.as_str()).collect();
    assert!(
        tagged_ids.contains(&RESTRICTED_WORK),
        "a locked work keeps its tag: {tagged_ids:?}"
    );
    assert!(
        !tagged_ids.contains(&UNLISTED_WORK),
        "unlisted is not browsable: {tagged_ids:?}"
    );

    let hits = darkhold_db::search::search_works(db, "held", 50)
        .await
        .expect("search");
    let hit_ids: Vec<&str> = hits.iter().map(|hit| hit.work_id.as_str()).collect();
    assert!(
        hit_ids.contains(&RESTRICTED_WORK),
        "search must find what a stranger may see: {hit_ids:?}"
    );
    assert!(
        !hit_ids.contains(&UNLISTED_WORK),
        "unlisted is not searchable: {hit_ids:?}"
    );

    // --- INFER: the work COUNT on the people surface ------------------------------------------------
    //
    // Presence on `/people` is the reader's own opt-in (`discoverability = 'listed'`), so hiding a person because their
    // only work is locked would punish them for using the tier. The *number* is the inference: a count of works a
    // stranger may read, which a locked work must not inflate — otherwise the surface says "3 works" about someone
    // whose page shows one.
    let pseuds = darkhold_db::identity::list_discoverable_pseuds(db, 50, 0)
        .await
        .expect("discoverable pseuds");
    let count = |handle: &str| {
        pseuds
            .iter()
            .find(|(_, h, ..)| h == handle)
            .map(|(_, _, _, works)| works)
            .copied()
    };
    assert_eq!(
        count("hollowgast"),
        Some(1),
        "the count is what a stranger may OPEN, so the locked work is not in it: {pseuds:?}"
    );
    assert_eq!(
        count("quietgast"),
        Some(0),
        "a pseud whose only work is locked has no knowable output: {pseuds:?}"
    );
}
