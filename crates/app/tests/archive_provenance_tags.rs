use darkhold_app::imports::library_input_for_test;
use darkhold_db::imports::ImportJob;
use darkhold_scrapers::{SourceKey, SourceWork, WorkStatus};

#[test]
fn provenance_carries_source_tags() {
    let work = SourceWork {
        source_key: SourceKey::new("ao3"),
        source_work_key: "w1".into(),
        source_url: "https://archiveofourown.org/works/1".into(),
        title: "Dark MC With A System".into(),
        author_text: "author".into(),
        author_url: None,
        summary: "progression fantasy".into(),
        word_count: None,
        rating_text: None,
        warning_texts: vec![],
        language: None,
        status: WorkStatus::Unknown,
        published_at: None,
        updated_at: None,
        tags: vec!["Dark MC".into(), "Progression Fantasy".into()],
        units: vec![],
    };
    let job = ImportJob {
        id: "01956c00-0000-4000-8000-0000000000ff".into(),
        job_id: "01956c00-0000-4000-8000-0000000000fe".into(),
        account_id: "01956c00-0000-4000-8000-000000000001".into(),
        pseud_id: "01956c00-0000-4000-8000-000000000002".into(),
        source_key: "ao3".into(),
        source_url: work.source_url.clone(),
        destination_type: "library".into(),
        dry_run: false,
        state: "queued".into(),
        library_item_id: None,
        report_json: None,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    };
    let input = library_input_for_test(&work, &job, "role_operator");
    let value: serde_json::Value = serde_json::from_str(&input.provenance_json).unwrap();
    assert_eq!(value["tags"][0], "Dark MC");
    assert_eq!(value["tags"][1], "Progression Fantasy");
}
