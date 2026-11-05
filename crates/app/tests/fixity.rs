//! The wave doc's `dd` rehearsal, automated: corrupt one stored byte and the scan must name it.
//!
//! Everything here is real — a real blob store in a scratch directory, the real `content_blobs` table, the real
//! scan. The only thing a fixture supplies is the bytes.
//!
//! Run on both backends: `DARKHOLD_TEST_PG_URL=… cargo test -p darkhold-app --test fixity`. The SQLite default
//! runs the same assertions, so a gate can run it without a PostgreSQL.

use darkhold_db::storage::BlobStore;
use test_support::TestDb;

async fn store_three(db: &TestDb) -> (BlobStore, Vec<String>) {
    let dir = test_support::scratch_dir("fixity-blobs");
    let store = BlobStore::new(&dir);
    let mut checksums = Vec::new();
    for body in ["alpha", "beta", "gamma"] {
        let (checksum, _key) = store
            .put(db.db(), body.as_bytes(), "text/plain")
            .await
            .expect("store a blob");
        checksums.push(checksum);
    }
    (store, checksums)
}

#[tokio::test]
async fn a_clean_store_scans_clean() {
    let db =
        TestDb::connect_with_dir("fixity-clean", &test_support::scratch_dir("fixity-clean")).await;
    let (store, _checksums) = store_three(&db).await;

    let summary = darkhold_app::fixity::scan(db.db(), &store, 1 << 20)
        .await
        .expect("scan");
    assert_eq!(summary.clean, 3, "three blobs, three clean reads");
    assert_eq!(summary.drifted, 0);
    assert_eq!(summary.missing, 0);
    assert!(summary.complete, "a tiny store fits any budget");
    db.cleanup().await;
}

#[tokio::test]
async fn one_flipped_byte_is_caught_and_named() {
    let db =
        TestDb::connect_with_dir("fixity-flip", &test_support::scratch_dir("fixity-flip")).await;
    let (store, checksums) = store_three(&db).await;
    let victim = checksums[1].clone();

    // One byte, in place, size unchanged: this is the corruption a size check cannot see, which is the whole
    // reason the scan hashes instead of stat-ing.
    let path = store.path_for(&victim);
    let mut bytes = std::fs::read(&path).expect("read blob");
    bytes[0] ^= 0x01;
    std::fs::write(&path, &bytes).expect("write blob");

    let summary = darkhold_app::fixity::scan(db.db(), &store, 1 << 20)
        .await
        .expect("scan");
    assert_eq!(summary.drifted, 1, "exactly one blob drifted");
    assert_eq!(summary.clean, 2);

    let report = darkhold_app::fixity::latest_report(db.db())
        .await
        .expect("report");
    assert_eq!(report.drift.len(), 1);
    let row = &report.drift[0];
    assert_eq!(row.checksum, victim);
    assert_eq!(row.kind, "hash", "same size, wrong bytes is a hash finding");
    assert_eq!(
        row.expected_bytes,
        row.actual_bytes.unwrap_or(-1),
        "size agrees; the file was not truncated"
    );
    assert_ne!(
        row.actual_checksum.as_deref().unwrap_or(""),
        row.checksum,
        "the report carries the checksum it actually read"
    );
    db.cleanup().await;
}

#[tokio::test]
async fn a_deleted_file_is_a_missing_finding_not_an_error() {
    let db = TestDb::connect_with_dir(
        "fixity-missing",
        &test_support::scratch_dir("fixity-missing"),
    )
    .await;
    let (store, checksums) = store_three(&db).await;
    std::fs::remove_file(store.path_for(&checksums[2])).expect("delete blob");

    let summary = darkhold_app::fixity::scan(db.db(), &store, 1 << 20)
        .await
        .expect("a missing file is a finding, not a failure");
    assert_eq!(summary.missing, 1);
    assert_eq!(summary.clean, 2);
    let report = darkhold_app::fixity::latest_report(db.db())
        .await
        .expect("report");
    assert_eq!(report.drift[0].kind, "missing");
    db.cleanup().await;
}

#[tokio::test]
async fn restoring_the_bytes_clears_the_next_run_and_keeps_the_history() {
    let db = TestDb::connect_with_dir(
        "fixity-restore",
        &test_support::scratch_dir("fixity-restore"),
    )
    .await;
    let (store, checksums) = store_three(&db).await;
    let victim = checksums[0].clone();
    let path = store.path_for(&victim);
    let mut bytes = std::fs::read(&path).expect("read");
    bytes[2] ^= 0xff;
    std::fs::write(&path, &bytes).expect("corrupt");

    let first = darkhold_app::fixity::scan(db.db(), &store, 1 << 20)
        .await
        .expect("scan");
    assert_eq!(first.drifted, 1);

    // Restore the bytes the row claims. `put` is a no-op when the row exists *and* the file is present, so the
    // file is written directly: the point of the test is the scan, not the writer.
    std::fs::write(&path, b"alpha").expect("restore");

    let second = darkhold_app::fixity::scan(db.db(), &store, 1 << 20)
        .await
        .expect("scan");
    assert_eq!(second.drifted, 0);
    assert_ne!(first.run_id, second.run_id, "each pass is its own run");

    let report = darkhold_app::fixity::latest_report(db.db())
        .await
        .expect("report");
    assert_eq!(
        report.drift.len(),
        0,
        "the newest run has nothing to report"
    );

    // The old finding still exists, under the old run: "corrupt on Tuesday, clean on Wednesday".
    let old = db
        .count_by(
            "SELECT COUNT(*) FROM fixity_drift WHERE run_id = ?",
            &first.run_id,
        )
        .await;
    assert_eq!(old, 1, "history is not rewritten by a passing scan");
    db.cleanup().await;
}

#[tokio::test]
async fn a_budget_that_runs_out_says_so() {
    let db = TestDb::connect_with_dir("fixity-budget", &test_support::scratch_dir("fixity-budget"))
        .await;
    let (store, _checksums) = store_three(&db).await;

    // A five-byte budget buys exactly one five-byte blob, and then the pass must stop and say so. The first
    // version of this test asserted zero blobs checked; the code checked one, and the code was right — the
    // budget is compared before each read, so a blob that fits is read. The assertion follows the measurement.
    let summary = darkhold_app::fixity::scan(db.db(), &store, 5)
        .await
        .expect("scan");
    assert!(!summary.complete, "an exhausted budget is not a clean pass");
    assert_eq!(
        summary.clean + summary.drifted + summary.missing + summary.unreadable,
        1,
        "one blob fits in five bytes; the other two do not"
    );
    assert_eq!(summary.bytes_hashed, 5, "the budget is spent, not exceeded");
    let report = darkhold_app::fixity::latest_report(db.db())
        .await
        .expect("report");
    assert!(!report.complete, "the run row carries the same answer");
    db.cleanup().await;
}
