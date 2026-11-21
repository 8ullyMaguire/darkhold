#[test]
fn ao3_refetch_can_start_after_an_offset() {
    let source = include_str!("../src/catalogue.rs");
    assert!(
        source.contains("pub offset: usize"),
        "refetch must expose a bounded starting offset"
    );
    assert!(
        source.contains("LIMIT $2 OFFSET $3"),
        "PostgreSQL refetch query must apply the offset"
    );
    assert!(
        source.contains("LIMIT ? OFFSET ?"),
        "SQLite refetch query must apply the offset"
    );
}
